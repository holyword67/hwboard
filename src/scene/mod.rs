// ============================================================
// src/scene/mod.rs
// ============================================================
mod item;
mod command;

pub use item::*;
pub use command::*;

use crate::journal::JournalEvent;
use std::collections::HashMap;
use std::sync::mpsc::Sender;

const UNDO_STACK_LIMIT: usize = 100;

pub struct Scene {
    items: HashMap<ItemId, CanvasItem>,
    order: Vec<ItemId>,
    next_id: ItemId,
    touched: Vec<ItemId>,
    inserted: Vec<ItemId>,
    removed: Vec<ItemId>,
}

impl Scene {
    pub fn new() -> Self {
        Self {
            items: HashMap::new(),
            order: Vec::new(),
            next_id: 0,
            touched: Vec::new(),
            inserted: Vec::new(),
            removed: Vec::new(),
        }
    }

    pub fn alloc_id(&mut self) -> ItemId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn insert(&mut self, id: ItemId, item: CanvasItem) {
        self.items.insert(id, item);
        self.order.push(id);
        self.inserted.push(id);
    }

    pub fn insert_at(&mut self, id: ItemId, item: CanvasItem, z_index: usize) {
        self.items.insert(id, item);
        let idx = z_index.min(self.order.len());
        self.order.insert(idx, id);
        self.inserted.push(id);
    }

    pub fn remove(&mut self, id: ItemId) {
        self.items.remove(&id);
        self.order.retain(|&x| x != id);
        self.removed.push(id);
    }

    pub fn iter_ordered(&self) -> impl Iterator<Item = &CanvasItem> {
        self.order.iter().filter_map(|id| self.items.get(id))
    }

    pub fn iter_ordered_with_id(&self) -> impl Iterator<Item = (ItemId, &CanvasItem)> {
        self.order.iter().filter_map(|&id| self.items.get(&id).map(|item| (id, item)))
    }

    pub fn iter_ordered_with_id_rev(&self) -> impl Iterator<Item = (ItemId, &CanvasItem)> {
        self.order.iter().rev().filter_map(|&id| self.items.get(&id).map(|item| (id, item)))
    }

    pub fn z_index_of(&self, id: ItemId) -> Option<usize> {
        self.order.iter().position(|&x| x == id)
    }

    pub fn item(&self, id: ItemId) -> Option<&CanvasItem> {
        self.items.get(&id)
    }

    pub fn item_mut(&mut self, id: ItemId) -> Option<&mut CanvasItem> {
        self.touched.push(id);
        self.items.get_mut(&id)
    }

    pub fn take_touched(&mut self) -> Vec<ItemId> {
        std::mem::take(&mut self.touched)
    }

    pub fn take_inserted(&mut self) -> Vec<ItemId> {
        std::mem::take(&mut self.inserted)
    }

    pub fn take_removed(&mut self) -> Vec<ItemId> {
        std::mem::take(&mut self.removed)
    }

    pub fn mark_stroke_clean(&mut self, id: ItemId) {
        if let Some(CanvasItem::Stroke(s)) = self.items.get_mut(&id) {
            s.geometry_dirty = false;
        }
    }

    pub fn mark_shape_clean(&mut self, id: ItemId) {
        if let Some(CanvasItem::Shape(s)) = self.items.get_mut(&id) {
            s.geometry_dirty = false;
        }
    }

    pub fn mark_image_clean(&mut self, id: ItemId) {
        if let Some(CanvasItem::Image(img)) = self.items.get_mut(&id) {
            img.geometry_dirty = false;
        }
    }
}

/// [설계 변경] undo/redo 스택 + "지금까지 실행된 Command들"의 유일한
/// 진입점(execute/push_already_applied/undo/redo). 저널 전송도 이
/// 4곳에서만 발생 — 호출부(pointer.rs/select.rs 등)는 저널 존재
/// 자체를 몰라도 됨.
pub struct UndoStack {
    undo: Vec<Command>,
    redo: Vec<Command>,
    journal_tx: Option<Sender<JournalEvent>>,
}

impl UndoStack {
    pub fn new(journal_tx: Option<Sender<JournalEvent>>) -> Self {
        Self { undo: Vec::new(), redo: Vec::new(), journal_tx }
    }

    /// 재생(replay) 시점엔 journal_tx 없이 만들었다가, 저장 스레드가
    /// 뜬 뒤 이걸로 물려줌(재생 중엔 다시 저널에 쓰면 안 되니까 순서상
    /// 이렇게 나눠야 함).
    pub fn set_journal_tx(&mut self, tx: Sender<JournalEvent>) {
        self.journal_tx = Some(tx);
    }

    /// 종료 시퀀스용 — Sender를 꺼내서 여기서 drop시키면 채널이 닫히고,
    /// 저장 스레드가 큐에 남은 걸 다 비운 뒤 자연 종료함(호출부가 그
    /// 뒤에 join).
    pub fn close_journal(&mut self) -> Option<Sender<JournalEvent>> {
        self.journal_tx.take()
    }

    /// 채널이 없거나(재생 중) 닫혀있으면(저장 스레드 이미 죽음) 조용히
    /// 무시 — 저널링은 best-effort, 실패해도 앱 정상 동작엔 영향 없음.
    fn journal(&self, event: JournalEvent) {
        if let Some(tx) = &self.journal_tx {
            let _ = tx.send(event);
        }
    }

    pub fn execute(&mut self, cmd: Command, scene: &mut Scene) {
        cmd.apply(scene);
        self.journal(JournalEvent::Do(cmd.clone()));
        self.undo.push(cmd);
        if self.undo.len() > UNDO_STACK_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn push_already_applied(&mut self, cmd: Command) {
        self.journal(JournalEvent::Do(cmd.clone()));
        self.undo.push(cmd);
        if self.undo.len() > UNDO_STACK_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, scene: &mut Scene) {
        if let Some(cmd) = self.undo.pop() {
            cmd.undo(scene);
            self.journal(JournalEvent::Undo);
            self.redo.push(cmd);
        }
    }

    pub fn redo(&mut self, scene: &mut Scene) {
        if let Some(cmd) = self.redo.pop() {
            cmd.apply(scene);
            self.journal(JournalEvent::Redo);
            self.undo.push(cmd);
        }
    }
}