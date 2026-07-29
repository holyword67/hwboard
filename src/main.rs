// ============================================================
// src/main.rs
// ============================================================
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod clipboard;
mod gpu;
mod input;
mod render;
mod scene;
mod ui;

use app::App;
use std::time::Duration;

/// 펜이 눌린 채 멈춰있는 동안(Hold=도형 자동스냅 감지 필요) 깨어나는
/// 주기. HOLD_DURATION(input/mod.rs, 1000ms) 대비 이 정도 텀은 체감상
/// 무시할 만함. [가정값] — 반응성이 아쉬우면 줄이면 됨(그만큼 CPU
/// 깨어나는 빈도는 늘어남).
const HOLD_POLL_INTERVAL: Duration = Duration::from_millis(50);

fn main() {
    let sdl_context = sdl3::init().expect("SDL3 초기화 실패");
    let video_subsystem = sdl_context.video().expect("video subsystem 실패");

    let mut window = video_subsystem
        .window("hwboard", 1600, 900)
        .position_centered()
        .resizable()
        .build()
        .expect("윈도우 생성 실패");

    let mut app = pollster::block_on(App::new(&window, &sdl_context));
    let mut event_pump = sdl_context.event_pump().expect("event pump 실패");

    while app.is_open() {
        // 이벤트가 올 때까지 스레드를 재운다 — busy loop 방지. 펜이
        // 눌린 채 멈춰있을 때(Hold 감지 필요)만 짧은 timeout으로
        // 주기적으로 깨어나고, 그 외엔 다음 이벤트가 올 때까지 무기한
        // 대기 — 가만히 있을 땐 CPU를 진짜로 안 씀.
        let first_event = if app.has_focus() && app.wants_frequent_wake() {
            event_pump.wait_event_timeout(HOLD_POLL_INTERVAL)
        } else {
            Some(event_pump.wait_event())
        };
        if let Some(event) = first_event {
            app.handle_sdl_event(&event, &mut window);
        }

        // 대기 중 한꺼번에 쌓였을 수 있는 나머지 이벤트도 모두 소진.
        // poll_iter()는 여기서도 원래처럼 논블로킹이라 busy loop 문제
        // 없음 — 큐가 비면 즉시 끝남.
        for event in event_pump.poll_iter() {
            app.handle_sdl_event(&event, &mut window);
        }

        app.poll();
        app.render_if_needed();
    }
}