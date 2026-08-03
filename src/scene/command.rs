// ============================================================
// src/scene/command.rs
// ============================================================
use super::{CanvasItem, ItemId, Scene};

pub trait Command: std::fmt::Debug {
    fn apply(&self, scene: &mut Scene);
    fn undo(&self, scene: &mut Scene);
}

#[derive(Debug)]
pub struct AddItem {
    pub id: ItemId,
    pub item: CanvasItem,
}
impl Command for AddItem {
    fn apply(&self, scene: &mut Scene) {
        scene.insert(self.id, self.item.clone());
    }
    fn undo(&self, scene: &mut Scene) {
        scene.remove(self.id);
    }
}

#[derive(Debug)]
pub struct DeleteItems {
    pub removed: Vec<(ItemId, CanvasItem, usize)>,
}
impl Command for DeleteItems {
    fn apply(&self, scene: &mut Scene) {
        for (id, _, _) in &self.removed {
            scene.remove(*id);
        }
    }
    fn undo(&self, scene: &mut Scene) {
        let mut sorted = self.removed.clone();
        sorted.sort_by_key(|(_, _, idx)| *idx);
        for (id, item, idx) in sorted {
            scene.insert_at(id, item, idx);
        }
    }
}

#[derive(Debug)]
pub struct MoveItems {
    pub ids: Vec<ItemId>,
    pub delta: [f64; 2],
}
impl Command for MoveItems {
    fn apply(&self, scene: &mut Scene) {
        for &id in &self.ids {
            if let Some(item) = scene.item_mut(id) {
                item.translate(self.delta);
            }
        }
    }
    fn undo(&self, scene: &mut Scene) {
        let neg = [
            -self.delta[0],
            -self.delta[1],
        ];
        for &id in &self.ids {
            if let Some(item) = scene.item_mut(id) {
                item.translate(neg);
            }
        }
    }
}

#[derive(Debug)]
pub struct ResizeImage {
    pub id: ItemId,
    pub before: ([f64; 2], [f64; 2]),
    pub after: ([f64; 2], [f64; 2]),
}
impl Command for ResizeImage {
    fn apply(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Image(img)) = scene.item_mut(self.id) {
            img.set_bounds(self.after.0, self.after.1);
        }
    }
    fn undo(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Image(img)) = scene.item_mut(self.id) {
            img.set_bounds(self.before.0, self.before.1);
        }
    }
}

#[derive(Debug)]
pub struct TransformShape {
    pub id: ItemId,
    pub before: ([f64; 2], [f64; 2], f32),
    pub after: ([f64; 2], [f64; 2], f32),
}
impl Command for TransformShape {
    fn apply(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Shape(sh)) = scene.item_mut(self.id) {
            sh.center = self.after.0;
            sh.half_extent = self.after.1;
            sh.rotation = self.after.2;
            sh.geometry_dirty = true;
        }
    }
    fn undo(&self, scene: &mut Scene) {
        if let Some(CanvasItem::Shape(sh)) = scene.item_mut(self.id) {
            sh.center = self.before.0;
            sh.half_extent = self.before.1;
            sh.rotation = self.before.2;
            sh.geometry_dirty = true;
        }
    }
}
