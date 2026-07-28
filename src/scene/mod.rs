// ============================================================
// src/scene/mod.rs
// ============================================================
mod item;
mod command;

pub use item::*;
pub use command::*;

use std::collections::HashMap;

const UNDO_STACK_LIMIT: usize = 100;

pub struct Scene {
    items: HashMap<ItemId, CanvasItem>,
    order: Vec<ItemId>,      // 삽입순서 = Z순서
    next_id: ItemId,
}

impl Scene {
    pub fn new() -> Self {
        Self { items: HashMap::new(), order: Vec::new(), next_id: 0 }
    }

    pub fn alloc_id(&mut self) -> ItemId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    pub fn insert(&mut self, id: ItemId, item: CanvasItem) {
        self.items.insert(id, item);
        self.order.push(id);
    }

    pub fn insert_at(&mut self, id: ItemId, item: CanvasItem, z_index: usize) {
        self.items.insert(id, item);
        let idx = z_index.min(self.order.len());
        self.order.insert(idx, id);
    }

    pub fn remove(&mut self, id: ItemId) {
        self.items.remove(&id);
        self.order.retain(|&x| x != id);
    }

    pub fn translate_to(&mut self, id: ItemId, _pos: [f64; 2]) {
        // TODO: CanvasItem 종류별로 top_left/points 이동 구현
        let _ = self.items.get_mut(&id);
    }

    /// 렌더링용 — Z순서대로 순회
    pub fn iter_ordered(&self) -> impl Iterator<Item = &CanvasItem> {
        self.order.iter().filter_map(|id| self.items.get(id))
    }

    pub fn iter_ordered_with_id(&self) -> impl Iterator<Item = (ItemId, &CanvasItem)> {
        self.order.iter().filter_map(|&id| self.items.get(&id).map(|item| (id, item)))
    }

    // 👇 [추가] 지우개/마우스 히트테스트용 — Z순서 역순(맨 위에서부터) 순회
    pub fn iter_ordered_with_id_rev(&self) -> impl Iterator<Item = (ItemId, &CanvasItem)> {
        self.order.iter().rev().filter_map(|&id| self.items.get(&id).map(|item| (id, item)))
    }    

    pub fn item(&self, id: ItemId) -> Option<&CanvasItem> {
        self.items.get(&id)
    }

    pub fn z_index_of(&self, id: ItemId) -> Option<usize> {
        self.order.iter().position(|&x| x == id)
    }

    pub fn mark_stroke_clean(&mut self, id: ItemId) {
        if let Some(CanvasItem::Stroke(s)) = self.items.get_mut(&id) {
            s.mesh_dirty = false;
        }
    }
}

pub struct UndoStack {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self { undo: Vec::new(), redo: Vec::new() }
    }

    /// 새 커맨드 실행 + 스택 push. redo 스택은 새 액션이 생기면 폐기(표준 undo/redo 동작).
    pub fn execute(&mut self, cmd: Box<dyn Command>, scene: &mut Scene) {
        cmd.apply(scene);
        self.undo.push(cmd);
        if self.undo.len() > UNDO_STACK_LIMIT {
            self.undo.remove(0); // TODO: VecDeque로 바꾸면 O(1)— 지금은 명확성 우선
        }
        self.redo.clear();
    }

    /// scene에 이미 apply된 커맨드를 스택에만 등록 (지우개처럼 "즉시
    /// 반영 + 나중에 한 번에 기록"하는 흐름용). apply()는 호출하지 않음
    /// — 중복 적용 방지.
    pub fn push_already_applied(&mut self, cmd: Box<dyn Command>) {
        self.undo.push(cmd);
        if self.undo.len() > UNDO_STACK_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, scene: &mut Scene) {
        if let Some(cmd) = self.undo.pop() {
            cmd.undo(scene);
            self.redo.push(cmd);
        }
    }

    pub fn redo(&mut self, scene: &mut Scene) {
        if let Some(cmd) = self.redo.pop() {
            cmd.apply(scene);
            self.undo.push(cmd);
        }
    }
}