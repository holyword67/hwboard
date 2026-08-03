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

    // 이번 프레임에 실제로 바뀐 아이템만 기록 — GpuResourceRegistry가
    // 매 프레임 전체 씬을 스캔하는 대신 이 세 버퍼만 drain해서 쓰도록.
    // 4개 뮤테이션 진입점(item_mut/insert/insert_at/remove)에서만 채워짐.
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

    /// 렌더링용 — Z순서대로 순회
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

    /// 선택 도구와 Command apply/undo가 아이템을 직접 변형할 때 씀.
    /// 호출 = 변경 의도로 간주하고 touched에 기록.
    pub fn item_mut(&mut self, id: ItemId) -> Option<&mut CanvasItem> {
        self.touched.push(id);
        self.items.get_mut(&id)
    }

    /// GpuResourceRegistry::sync()가 프레임마다 호출해서 이번 프레임에
    /// 바뀐 아이템 id만 꺼내감(비우면서).
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

pub struct UndoStack {
    undo: Vec<Box<dyn Command>>,
    redo: Vec<Box<dyn Command>>,
}

impl UndoStack {
    pub fn new() -> Self {
        Self { undo: Vec::new(), redo: Vec::new() }
    }

    pub fn execute(&mut self, cmd: Box<dyn Command>, scene: &mut Scene) {
        cmd.apply(scene);
        self.undo.push(cmd);
        if self.undo.len() > UNDO_STACK_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

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