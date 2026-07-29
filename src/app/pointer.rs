// ============================================================
// src/app/pointer.rs
// ============================================================
use super::{App, Tool};
use crate::input::{PointerEvent, PointerSource};
use crate::render::tessellate::IncrementalStrokeMesh;
use crate::scene::{AddItem, CanvasItem, DeleteItems, PenPoint, ShapeKind, Stroke};
use crate::ui::{self, UiAction};

pub(super) const ERASER_RADIUS_SCREEN_PX: f32 = 12.0;
/// [미검증 가설] 자유획 점 디시메이션 문턱값 — 직전에 실제로 채택된
/// 점과 스크린상 이 거리 미만이면 점 자체를 안 늘림.
const STROKE_POINT_MIN_DISTANCE_SCREEN_PX: f32 = 2.0;
/// [미검증 가설] 경로 스무딩용 3점 가중 이동평균 가중치(좌/중/우).
/// 관절의 라운드 조인이 미세한 폴리라인 꺾임 때문에 튀어나오는 문제를
/// 완화하려고 도입 — 과하게 세게 걸면 의도된 뾰족한 코너(숫자/기호
/// 필기)까지 뭉개지므로 가볍게만.
const SMOOTHING_WEIGHTS: [f64; 3] = [0.25, 0.5, 0.25];

impl App {
pub(super) fn handle_pointer(&mut self, ev: PointerEvent) {
        if let PointerEvent::Down(s) = ev {
            if let Some(action) = ui::hit_test(s.pos, self.camera.viewport_size, self.tool, self.pen_color, self.pen_width) {
                self.pointer_captured_by_ui = true;
                match action {
                    UiAction::SelectTool(t) => self.tool = t,
                    UiAction::SelectColor(c) => self.pen_color = c,
                    UiAction::SelectThickness(w) => self.pen_width = w,
                }
                self.ui_dirty = true;
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
                    points: Vec::new(),
                    color: self.pen_color,
                    base_width: self.pen_width,
                    mesh_dirty: true,
                });
                self.drawing_mesh_cache = Some(IncrementalStrokeMesh::new(world));
                self.drawing_stroke_last_screen_pos = Some(s.pos);
                if let Some(live) = &mut self.live_stroke_gpu {
                    live.reset();
                }

                // 새 획 시작 — 스무딩 버퍼 리셋. 첫 점(앵커)은 스무딩 없이
                // 즉시 확정(펜 닿자마자 dot이 바로 보이도록).
                self.smoother_prev2 = None;
                self.smoother_prev1_pending = false;
                let p0 = PenPoint { pos: world, pressure: s.pressure };
                self.push_finalized_point(p0.clone());
                self.smoother_prev1 = Some(p0);
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

                // 디시메이션: 직전 채택 점과 스크린상 너무 가까우면 점
                // 자체를 안 늘림. 스무딩은 이걸 통과한 점에 대해서만
                // 적용됨(둘은 독립된 단계).
                if let Some(last_pos) = self.drawing_stroke_last_screen_pos {
                    let dx = s.pos[0] - last_pos[0];
                    let dy = s.pos[1] - last_pos[1];
                    if (dx * dx + dy * dy).sqrt() < STROKE_POINT_MIN_DISTANCE_SCREEN_PX {
                        return;
                    }
                }
                self.drawing_stroke_last_screen_pos = Some(s.pos);

                self.feed_smoother(PenPoint { pos: world, pressure: s.pressure });
            }
            (Tool::Pen, PointerEvent::Up(_)) => {
                self.snap_state = None;
                // 스무딩 대기 중이던 마지막 점(오른쪽 이웃이 없어 평활
                // 확정 못 했던 꼬리) — 원본 그대로 흘려보냄.
                if self.smoother_prev1_pending {
                    if let Some(tail) = self.smoother_prev1.take() {
                        self.push_finalized_point(tail);
                    }
                }
                self.smoother_prev2 = None;
                self.smoother_prev1 = None;
                self.smoother_prev1_pending = false;
                self.drawing_mesh_cache = None;
                self.drawing_stroke_last_screen_pos = None;
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
                        self.drawing_mesh_cache = None;
                        self.smoother_prev2 = None;
                        self.smoother_prev1 = None;
                        self.smoother_prev1_pending = false;
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
            _ => {}
        }
    }

    /// 새 raw 점을 스무딩 슬라이딩 윈도우에 흘려넣음. 윈도우(직전-직전,
    /// 직전, 지금)가 다 찼을 때만 "직전" 점의 평활 위치를 확정해서
    /// push — 1점 지연 append. 윈도우 안 찬 상태(획 시작 직후)는
    /// 슬라이드만 하고 아직 아무것도 push 안 함.
    fn feed_smoother(&mut self, raw: PenPoint) {
        match (self.smoother_prev2, self.smoother_prev1.clone()) {
            (Some(a), Some(b)) => {
                let smoothed_pos = [
                    SMOOTHING_WEIGHTS[0] * a[0] + SMOOTHING_WEIGHTS[1] * b.pos[0] + SMOOTHING_WEIGHTS[2] * raw.pos[0],
                    SMOOTHING_WEIGHTS[0] * a[1] + SMOOTHING_WEIGHTS[1] * b.pos[1] + SMOOTHING_WEIGHTS[2] * raw.pos[1],
                ];
                self.push_finalized_point(PenPoint { pos: smoothed_pos, pressure: b.pressure });
                self.smoother_prev2 = Some(b.pos);
                self.smoother_prev1 = Some(raw);
                self.smoother_prev1_pending = true;
            }
            (None, Some(b)) => {
                // 윈도우 부족(획의 두 번째 점) — b는 이미 앵커로 push된
                // 상태라 다시 확정할 필요 없음, 슬라이드만.
                self.smoother_prev2 = Some(b.pos);
                self.smoother_prev1 = Some(raw);
                self.smoother_prev1_pending = true;
            }
            (_, None) => {
                // Down이 항상 prev1을 세팅하므로 이론상 도달 안 함.
                self.smoother_prev1 = Some(raw);
                self.smoother_prev1_pending = true;
            }
        }
    }

    /// 확정된 점(스무딩 완료 또는 앵커) 하나를 stroke.points와
    /// mesh_cache 양쪽에 동시 반영.
    fn push_finalized_point(&mut self, point: PenPoint) {
        let half_width = match &mut self.drawing_stroke {
            Some(stroke) => {
                let hw = stroke.base_width * point.pressure.max(0.05) * 0.5;
                stroke.points.push(point.clone());
                hw
            }
            None => return,
        };
        if let Some(mesh_cache) = &mut self.drawing_mesh_cache {
            mesh_cache.push_point(point.pos, half_width);
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