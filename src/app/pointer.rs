// ============================================================
// src/app/pointer.rs
// ============================================================
use super::{App, Tool};
use crate::input::{PointerEvent, PointerSource};
use crate::scene::{AddItem, CanvasItem, DeleteItems, PenPoint, ShapeKind, Stroke};
use crate::ui::{self, UiAction};

const PEN_BASE_WIDTH: f32 = 3.0;
pub(super) const ERASER_RADIUS_SCREEN_PX: f32 = 12.0;

impl App {
    pub(super) fn handle_pointer(&mut self, ev: PointerEvent) {
        if let PointerEvent::Down(s) = ev {
            if let Some(action) = ui::hit_test(s.pos, self.camera.viewport_size, self.tool, self.pen_color) {
                self.pointer_captured_by_ui = true;
                match action {
                    UiAction::SelectTool(t) => self.tool = t,
                    UiAction::SelectColor(c) => self.pen_color = c,
                }
                return;
            }
        }
        if self.pointer_captured_by_ui {
            if let PointerEvent::Up(_) = ev {
                self.pointer_captured_by_ui = false;
            }
            return;
        }

        match ev.sample().source {
            PointerSource::Mouse => self.handle_mouse_pointer(ev),
            PointerSource::Pen => self.handle_pen_pointer(ev),
        }
    }

    /// 마우스: Select 도구일 땐 select.rs로 위임(선택/이동/리사이즈/
    /// 회전). 그 외 도구(Pen/Eraser)일 땐 원래대로 팬 전용.
    fn handle_mouse_pointer(&mut self, ev: PointerEvent) {
        if self.tool == Tool::Select {
            self.handle_select_pointer(ev);
            return;
        }
        match ev {
            PointerEvent::Down(s) => {
                self.panning = true;
                self.last_pan_pos = s.pos;
            }
            PointerEvent::Move(s) => {
                if self.panning {
                    let delta = [s.pos[0] - self.last_pan_pos[0], s.pos[1] - self.last_pan_pos[1]];
                    self.camera.pan_by_screen_delta(delta);
                    self.last_pan_pos = s.pos;
                }
            }
            PointerEvent::Up(_) => self.panning = false,
            PointerEvent::Hold(_) => {}
        }
    }

    /// 펜: Select 도구일 땐 완전히 비활성(설계 결정 — 선택/이동/
    /// 리사이즈/회전은 마우스 전용). 그 외엔 원래대로 그리기/지우기.
    fn handle_pen_pointer(&mut self, ev: PointerEvent) {
        if self.tool == Tool::Select {
            return;
        }
        match (self.tool, ev) {
            (Tool::Pen, PointerEvent::Down(s)) => {
                self.snap_state = None;
                self.drawing_shape_preview = None;
                let world = self.camera.screen_to_world(s.pos);
                self.drawing_stroke = Some(Stroke {
                    points: vec![PenPoint { pos: world, pressure: s.pressure }],
                    color: self.pen_color,
                    base_width: PEN_BASE_WIDTH,
                    mesh_dirty: true,
                });
            }
            (Tool::Pen, PointerEvent::Move(s)) => {
                let world = self.camera.screen_to_world(s.pos);

                if let Some(snap) = &self.snap_state {
                    if let Some(shape) = &mut self.drawing_shape_preview {
                        match snap.kind {
                            ShapeKind::Line => {
                                let start = snap.line_start;
                                let mid = [(start[0] + world[0]) * 0.5, (start[1] + world[1]) * 0.5];
                                let vx = world[0] - start[0];
                                let vy = world[1] - start[1];
                                let length = (vx * vx + vy * vy).sqrt();
                                shape.center = mid;
                                shape.half_extent = [length * 0.5, 0.0];
                                shape.rotation = vy.atan2(vx) as f32;
                            }
                            _ => {
                                let d0 = [snap.initial_pen[0] - snap.center[0], snap.initial_pen[1] - snap.center[1]];
                                let dist0 = (d0[0] * d0[0] + d0[1] * d0[1]).sqrt().max(1.0);
                                let d1 = [world[0] - snap.center[0], world[1] - snap.center[1]];
                                let dist1 = (d1[0] * d1[0] + d1[1] * d1[1]).sqrt();
                                let scale = dist1 / dist0;
                                let angle0 = d0[1].atan2(d0[0]);
                                let angle1 = d1[1].atan2(d1[0]);
                                let delta_angle = (angle1 - angle0) as f32;
                                shape.center = snap.center;
                                shape.half_extent =
                                    [snap.initial_half_extent[0] * scale, snap.initial_half_extent[1] * scale];
                                shape.rotation = snap.initial_rotation + delta_angle;
                            }
                        }
                        shape.mesh_dirty = true;
                    }
                    return;
                }

                if let Some(stroke) = &mut self.drawing_stroke {
                    stroke.points.push(PenPoint { pos: world, pressure: s.pressure });
                }
            }
            (Tool::Pen, PointerEvent::Up(_)) => {
                self.snap_state = None;
                if let Some(shape) = self.drawing_shape_preview.take() {
                    let id = self.scene.alloc_id();
                    let cmd = Box::new(AddItem { id, item: CanvasItem::Shape(shape) });
                    self.undo_stack.execute(cmd, &mut self.scene);
                } else if let Some(stroke) = self.drawing_stroke.take() {
                    if stroke.points.len() >= 2 {
                        let id = self.scene.alloc_id();
                        let cmd = Box::new(AddItem { id, item: CanvasItem::Stroke(stroke) });
                        self.undo_stack.execute(cmd, &mut self.scene);
                    }
                }
            }
            (Tool::Pen, PointerEvent::Hold(_)) => {
                if self.snap_state.is_some() { return; }
                if let Some(stroke) = &self.drawing_stroke {
                    if let Some((shape, snap_data)) = super::shapes::recognize_shape(stroke) {
                        self.drawing_shape_preview = Some(shape);
                        self.drawing_stroke = None;
                        self.snap_state = Some(snap_data);
                    }
                }
            }
            (Tool::Eraser, PointerEvent::Down(s)) => {
                self.eraser_pressed = true;
                self.erasing_removed.clear();
                self.try_erase_at(s.pos);
            }
            (Tool::Eraser, PointerEvent::Move(s)) => {
                if self.eraser_pressed {
                    self.try_erase_at(s.pos);
                }
            }
            (Tool::Eraser, PointerEvent::Up(_)) => {
                self.eraser_pressed = false;
                if !self.erasing_removed.is_empty() {
                    let cmd = Box::new(DeleteItems { removed: std::mem::take(&mut self.erasing_removed) });
                    self.undo_stack.push_already_applied(cmd);
                }
            }
            // Tool::Select 조합은 함수 상단에서 이미 걸러져서 도달 안 함.
            // 나머지 Hold 조합(Eraser+Hold 등)도 여기서 무시.
            _ => {}
        }
    }

    fn try_erase_at(&mut self, screen_pos: [f32; 2]) {
        let world = self.camera.screen_to_world(screen_pos);
        let r = (ERASER_RADIUS_SCREEN_PX / self.camera.zoom) as f64;

        let hit = self.scene.iter_ordered_with_id_rev().find_map(|(id, item)| {
            if self.erasing_removed.iter().any(|(rid, _, _)| *rid == id) {
                return None;
            }
            item.hit_test(world, r).then_some(id)
        });

        if let Some(id) = hit {
            if let Some(item) = self.scene.item(id).cloned() {
                let z = self.scene.z_index_of(id).unwrap_or(0);
                self.scene.remove(id);
                self.erasing_removed.push((id, item, z));
            }
        }
    }
}