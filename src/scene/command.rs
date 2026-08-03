// ============================================================
// src/scene/command.rs
// ============================================================
use super::{CanvasItem, ItemId, Scene};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Command {
    AddItem(AddItem),
    DeleteItems(DeleteItems),
    MoveItems(MoveItems),
    ResizeImage(ResizeImage),
    TransformShape(TransformShape),
    ClearAll(ClearAll),
}

impl Command {
    pub fn apply(&self, scene: &mut Scene) {
        match self {
            Command::AddItem(c) => c.apply(scene),
            Command::DeleteItems(c) => c.apply(scene),
            Command::MoveItems(c) => c.apply(scene),
            Command::ResizeImage(c) => c.apply(scene),
            Command::TransformShape(c) => c.apply(scene),
            Command::ClearAll(c) => c.apply(scene),
        }
    }

    pub fn undo(&self, scene: &mut Scene) {
        match self {
            Command::AddItem(c) => c.undo(scene),
            Command::DeleteItems(c) => c.undo(scene),
            Command::MoveItems(c) => c.undo(scene),
            Command::ResizeImage(c) => c.undo(scene),
            Command::TransformShape(c) => c.undo(scene),
            Command::ClearAll(c) => c.undo(scene),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddItem {
    pub id: ItemId,
    pub item: CanvasItem,
}
impl AddItem {
    fn apply(&self, scene: &mut Scene) { scene.insert(self.id, self.item.clone()); }
    fn undo(&self, scene: &mut Scene) { scene.remove(self.id); }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteItems {
    pub removed: Vec<(ItemId, CanvasItem, usize)>,
}
impl DeleteItems {
    fn apply(&self, scene: &mut Scene) {
        for (id, _, _) in &self.removed { scene.remove(*id); }
    }
    fn undo(&self, scene: &mut Scene) {
        let mut sorted = self.removed.clone();
        sorted.sort_by_key(|(_, _, idx)| *idx);
        for (id, item, idx) in sorted { scene.insert_at(id, item, idx); }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MoveItems {
    pub ids: Vec<ItemId>,
    pub delta: [f64; 2],
}
impl MoveItems {
    fn apply(&self, scene: &mut Scene) {
        for &id in &self.ids {
            if let Some(item) = scene.item_mut(id) { item.translate(self.delta); }
        }
    }
    fn undo(&self, scene: &mut Scene) {
        let neg = [-self.delta[0], -self.delta[1]];
        for &id in &self.ids {
            if let Some(item) = scene.item_mut(id) { item.translate(neg); }
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeImage {
    pub id: ItemId,
    pub before: ([f64; 2], [f64; 2]),
    pub after: ([f64; 2], [f64; 2]),
}
impl ResizeImage {
    fn apply(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Image(img)) = scene.item_mut(self.id) { img.set_bounds(self.after.0, self.after.1); }
    }
    fn undo(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Image(img)) = scene.item_mut(self.id) { img.set_bounds(self.before.0, self.before.1); }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformShape {
    pub id: ItemId,
    pub before: ([f64; 2], [f64; 2], f32),
    pub after: ([f64; 2], [f64; 2], f32),
}
impl TransformShape {
    fn apply(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Shape(sh)) = scene.item_mut(self.id) {
            sh.center = self.after.0; sh.half_extent = self.after.1; sh.rotation = self.after.2;
            sh.geometry_dirty = true;
        }
    }
    fn undo(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Shape(sh)) = scene.item_mut(self.id) {
            sh.center = self.before.0; sh.half_extent = self.before.1; sh.rotation = self.before.2;
            sh.geometry_dirty = true;
        }
    }
}

/// ESC 전체 리셋용. 지우기 직전 씬 전체를 (id, item, z_index) 스냅샷으로
/// 들고 있다가 undo하면 그대로 복원 — DeleteItems랑 구조가 같지만
/// "선택된 일부"가 아니라 "전체"라서 의미 명확화 차원에서 별도 타입.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClearAll {
    pub items: Vec<(ItemId, CanvasItem, usize)>,
}
impl ClearAll {
    fn apply(&self, scene: &mut Scene) {
        for (id, _, _) in &self.items { scene.remove(*id); }
    }
    fn undo(&self, scene: &mut Scene) {
        let mut sorted = self.items.clone();
        sorted.sort_by_key(|(_, _, idx)| *idx);
        for (id, item, idx) in sorted { scene.insert_at(id, item, idx); }
    }
}