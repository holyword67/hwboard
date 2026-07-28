// ============================================================
// src/scene/command.rs
// ============================================================
use super::{CanvasItem, ItemId, Scene};

pub trait Command: std::fmt::Debug {
    fn apply(&self, scene: &mut Scene);
    fn undo(&self, scene: &mut Scene);
}

// ---- 구체 커맨드들 ----

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
    // undo 시 되살려야 하므로 지워지는 시점의 (id, item, 원래 z-order 인덱스)를 통째로 보관.
    pub removed: Vec<(ItemId, CanvasItem, usize)>,
}
impl Command for DeleteItems {
    fn apply(&self, scene: &mut Scene) {
        for (id, _, _) in &self.removed {
            scene.remove(*id);
        }
    }
    fn undo(&self, scene: &mut Scene) {
        // z-order 인덱스 오름차순으로 되돌려야 삽입 위치가 꼬이지 않음
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
    pub from: Vec<[f64; 2]>,
    pub to: Vec<[f64; 2]>,
}
impl Command for MoveItems {
    fn apply(&self, scene: &mut Scene) {
        for (id, pos) in self.ids.iter().zip(&self.to) {
            scene.translate_to(*id, *pos);
        }
    }
    fn undo(&self, scene: &mut Scene) {
        for (id, pos) in self.ids.iter().zip(&self.from) {
            scene.translate_to(*id, *pos);
        }
    }
}

// TODO: ResizeItems, EraseStrokePoints (지우개는 스트로크 일부 삭제라
// AddItem/DeleteItems 둘 다 아닌 별도 커맨드가 필요할 수도 있음 — 다음에 논의)