// ============================================================
// src/app/pointer.rs
// ============================================================
// 포인터(펜/마우스) 입력 처리 — 그리기/지우기/팬 라우팅과 지우개
// 히트테스트. App::handle_sdl_event → InputState::process_event를 거쳐
// 나온 PointerEvent가 최종적으로 도착하는 곳.

use super::{App, Tool};
use crate::input::{PointerEvent, PointerSource};
use crate::scene::{AddItem, CanvasItem, DeleteItems, PenPoint, Stroke};
use crate::ui::{self, UiAction};

/// 펜으로 그리는 스트로크의 기본 두께.
const PEN_BASE_WIDTH: f32 = 3.0;

/// 스크린 픽셀 기준 고정 반경 — 지울 때마다 camera.zoom으로 world 반경을
/// 역산한다. 이렇게 하면 확대/축소해도 화면상 지우개 크기가 항상
/// 동일하게 느껴짐. [가정값] 12px — 체감상 별로면 조정.
pub(super) const ERASER_RADIUS_SCREEN_PX: f32 = 12.0;

impl App {
    pub(super) fn handle_pointer(&mut self, ev: PointerEvent) {
        // UI 버튼 히트테스트가 항상 먼저 — 소스(펜/마우스) 상관없이 클릭
        // 이면 다 잡음.
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

    /// 마우스 좌클릭 드래그 = 팬. 그리기/지우기는 관여하지 않음.
    fn handle_mouse_pointer(&mut self, ev: PointerEvent) {
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
        match (self.tool, ev) {
            (Tool::Pen, PointerEvent::Down(s)) => {
                self.snap_state = None; // 새로 그릴 때 스냅 상태 초기화
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

                // 스냅 상태일 경우, 점을 추가하지 않고 도형 전체를 회전/크기 조절함
                if let Some(snap) = &self.snap_state {
                    if let Some(stroke) = &mut self.drawing_stroke {
                        if snap.is_line {
                            // 직선: 시작점은 고정, 끝점은 고무줄처럼 현재 펜 위치로
                            stroke.points[1].pos = world;
                        } else {
                            // 도형: 중심점 기준으로 Scale & Rotation 동시 적용
                            let mut dx0 = snap.initial_pen[0] - snap.center[0];
                            let mut dy0 = snap.initial_pen[1] - snap.center[1];
                            let dist0 = (dx0 * dx0 + dy0 * dy0).sqrt();

                            // 스냅 순간에 중심점과 펜이 완전히 겹쳐 0으로 나누어지는 오류 방지
                            if dist0 < 1.0 { dx0 = 1.0; dy0 = 0.0; }
                            let safe_dist0 = dist0.max(1.0);

                            let dx1 = world[0] - snap.center[0];
                            let dy1 = world[1] - snap.center[1];
                            let dist1 = (dx1 * dx1 + dy1 * dy1).sqrt();

                            // 크기 변화율과 각도 변화량 계산
                            let scale = dist1 / safe_dist0;
                            let delta_angle = dy1.atan2(dx1) - dy0.atan2(dx0);
                            let cos_a = delta_angle.cos();
                            let sin_a = delta_angle.sin();

                            // 미리 저장해둔 원본 상대좌표에 변환 행렬 적용
                            for (i, p) in stroke.points.iter_mut().enumerate() {
                                let lp = snap.local_points[i];
                                let sx = lp[0] * scale;
                                let sy = lp[1] * scale;

                                let rx = sx * cos_a - sy * sin_a;
                                let ry = sx * sin_a + sy * cos_a;

                                p.pos = [snap.center[0] + rx, snap.center[1] + ry];
                            }
                        }
                        stroke.mesh_dirty = true;
                    }
                    return; // 렌더링 끝냈으니 여기서 함수 종료
                }

                // 스냅 상태가 아닐 땐 평범하게 선 긋기
                if let Some(stroke) = &mut self.drawing_stroke {
                    stroke.points.push(PenPoint { pos: world, pressure: s.pressure });
                }
            }
            (Tool::Pen, PointerEvent::Up(_)) => {
                self.snap_state = None; // 그리기 완료 시 상태 해제
                if let Some(stroke) = self.drawing_stroke.take() {
                    if stroke.points.len() >= 2 {
                        let id = self.scene.alloc_id();
                        let cmd = Box::new(AddItem { id, item: CanvasItem::Stroke(stroke) });
                        self.undo_stack.execute(cmd, &mut self.scene);
                    }
                }
            }
            (Tool::Pen, PointerEvent::Hold(_)) => {
                if self.snap_state.is_some() { return; } // 이미 변환됐으면 무시
                if let Some(stroke) = &mut self.drawing_stroke {
                    // 인식 성공 시 snap_state에 변환 데이터를 저장
                    if let Some(snap_data) = super::shapes::recognize_and_snap_shape(stroke) {
                        self.snap_state = Some(snap_data);
                    }
                }
            }
            (Tool::Eraser, PointerEvent::Down(s)) => {
                // [변경됨] 실제로 눌린 상태로 표시 — Move에서 이 값으로
                // 게이팅해서, 호버 중(펜 다운 없이 오는 PenMotion)엔
                // 지워지지 않도록 함.
                self.eraser_pressed = true;
                self.erasing_removed.clear();
                self.try_erase_at(s.pos);
            }
            (Tool::Eraser, PointerEvent::Move(s)) => {
                // [변경됨] 펜이 눌려있을 때만 실제로 지움. 눌려있지
                // 않으면(호버) 조용히 무시 — 인디케이터는 render.rs가
                // last_pen_pos()로 독립적으로 그리니까 계속 따라다니는
                // 건 그대로 유지됨.
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
            (_, PointerEvent::Hold(_)) => {
                // TODO: 도형 자동스냅 — 다음 논의 주제
            }
        }
    }

    /// 정밀 히트테스트(Broad Phase bbox → Narrow Phase 점-선분 거리,
    /// `CanvasItem::hit_test` 참고). 반경은 스크린 픽셀 고정값을 매 호출
    /// 시점 zoom으로 world 단위로 환산해서 넘김 — 확대/축소해도 화면상
    /// 지우개 크기가 동일하게 느껴지도록.
    fn try_erase_at(&mut self, screen_pos: [f32; 2]) {
        let world = self.camera.screen_to_world(screen_pos);
        let r = (ERASER_RADIUS_SCREEN_PX / self.camera.zoom) as f64;

        // .rev()로 맨 위(가장 나중에 그려진) 아이템부터 역순 검사.
        // 이를 통해 겹쳐진 선을 지울 때 아래에 있는 선이 잘못 지워지는 현상 방지.
        let hit = self.scene.iter_ordered_with_id_rev().find_map(|(id, item)| {
            // 이미 이번 드래그 세션에 지워진 항목은 무시
            if self.erasing_removed.iter().any(|(rid, _, _)| *rid == id) {
                return None;
            }

            // 정밀 히트테스트 호출
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