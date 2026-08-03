// ============================================================
// src/journal.rs
// ============================================================
// [빌드 미검증 — bincode 1.3 API(serialize/deserialize) 기준, codebase
// 인덱스에 없어서 사전대조 못함. 나머지(serde/dirs)는 실제 소스 대조 완료.]
//
// 크래시 복구 전용 append-only 저널. "정상종료 vs 비정상종료" 구분
// 자체가 없음 — 항상 재생하고, ESC(ClearAll)만이 유일한 리셋 경로.
// 저장은 UndoStack의 4개 진입점(execute/push_already_applied/undo/redo)
// 에서만 발생 — Command 실행 "단위"로만 쓰기가 일어나므로 렌더 루프와
// 완전히 무관(프레임타임 영향 없음).

use crate::scene::{Scene, UndoStack};
use serde::{Deserialize, Serialize};
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Sender};
use std::thread::{self, JoinHandle};

const JOURNAL_VERSION: u8 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum JournalEvent {
    Do(crate::scene::Command),
    Undo,
    Redo,
}

/// `%LOCALAPPDATA%\hwboard\journal.bin`(Windows 기준) — `%TEMP%`와
/// 달리 OS/서드파티 임시폴더 청소 도구에 안 날아감. 디렉토리 없으면
/// 만듦(실패해도 조용히 무시 — spawn()에서 파일 열기 자체가 실패하면
/// 저널링을 그냥 포기하는 걸로 처리).
pub fn journal_path() -> PathBuf {
    let base = dirs::data_local_dir().unwrap_or_else(std::env::temp_dir);
    let dir = base.join("hwboard");
    let _ = std::fs::create_dir_all(&dir);
    dir.join("journal.bin")
}

/// 저장 전용 스레드 기동. 반환된 Sender로 이벤트를 던지면(non-blocking,
/// 거의 항상 즉시 리턴) 스레드가 순차적으로 파일에 append. 모든 Sender가
/// drop되면(App이 종료 시퀀스에서 close_journal 호출) 스레드가 큐를 다
/// 비운 뒤 자연 종료 — 호출부가 반환된 JoinHandle로 join하면 "정상
/// 종료 시엔 유실 없음"이 보장됨.
pub fn spawn(path: PathBuf) -> (Sender<JournalEvent>, JoinHandle<()>) {
    let (tx, rx) = mpsc::channel::<JournalEvent>();
    let handle = thread::spawn(move || {
        let Ok(mut file) = OpenOptions::new().create(true).append(true).open(&path) else {
            return; // 저장 자체가 불가능한 환경 — 앱 동작엔 영향 없이 조용히 포기
        };
        if file.metadata().map(|m| m.len()).unwrap_or(0) == 0 {
            let _ = file.write_all(&[JOURNAL_VERSION]);
        }
        while let Ok(event) = rx.recv() {
            let Ok(bytes) = postcard::to_allocvec(&event) else { continue };
            let len = (bytes.len() as u32).to_le_bytes();
            let _ = file.write_all(&len);
            let _ = file.write_all(&bytes);
            // fsync까지는 과함(전원차단 대비가 아니라 "우리 프로세스
            // 크래시" 대비라 write() 시스템콜 완료 시점에 이미 커널이
            // 데이터를 들고 있어 우리 크래시엔 안전 — flush()는 File엔
            // 사실상 no-op에 가깝지만 관례상 명시).
            let _ = file.flush();
        }
        // rx.recv() Err(모든 Sender drop) → 루프 자연 종료 → 스레드 끝
    });
    (tx, handle)
}

/// `[버전 1B][길이4B][bincode]` 반복 파싱. 버전 안 맞음/빈 파일/중간에
/// 깨짐(크래시로 인한 torn write) → 그 지점까지만 인정하고 멈춤(전부
/// 실패 처리 안 함 — "최신 커맨드 1~N개 유실은 감수"에 맞춘 설계).
fn read_all(path: &Path) -> Vec<JournalEvent> {
    let mut events = Vec::new();
    let Ok(mut file) = File::open(path) else { return events };

    let mut version_byte = [0u8; 1];
    if file.read_exact(&mut version_byte).is_err() || version_byte[0] != JOURNAL_VERSION {
        return events; // 버전 불일치 → 통째로 무시(새로 시작)
    }

    loop {
        let mut len_buf = [0u8; 4];
        if file.read_exact(&mut len_buf).is_err() { break; } // 정상 EOF든 torn write든 여기서 멈춤
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut buf = vec![0u8; len];
        if file.read_exact(&mut buf).is_err() { break; } // 크래시로 body 일부만 쓰인 경우
        match postcard::from_bytes::<JournalEvent>(&buf) {
            Ok(ev) => events.push(ev),
            Err(_) => break, // 손상된 엔트리 — 여기까지만 인정
        }
    }
    events
}

/// 저널을 재생해서 Scene+UndoStack을 재구성. 파일 없음/버전 불일치/
/// 파싱 실패 → 빈 상태(새로 시작). 반환되는 UndoStack은 journal_tx가
/// 없는 상태 — 호출부가 spawn()으로 얻은 Sender를 나중에 물려줘야 함
/// (재생 중 다시 저널에 쓰면 안 되므로 순서상 분리).
pub fn replay(path: &Path) -> (Scene, UndoStack) {
    let mut scene = Scene::new();
    let mut undo_stack = UndoStack::new(None);
    for event in read_all(path) {
        match event {
            JournalEvent::Do(cmd) => undo_stack.execute(cmd, &mut scene),
            JournalEvent::Undo => undo_stack.undo(&mut scene),
            JournalEvent::Redo => undo_stack.redo(&mut scene),
        }
    }
    (scene, undo_stack)
}