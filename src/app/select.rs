// ============================================================
// src/app/select.rs
// ============================================================
// Tool::Select 전용 로직 — 마우스로만 동작(설계 결정: 선택/이동/
// 리사이즈/회전은 마우스 전용, 펜은 Select 모드에서 완전히 비활성).
// 팬은 이 모드에서 포기함(스페이스+드래그 등 대안은 이번 스코프 제외,
// 사용자 확인받고 감수하기로 함).

use super::App;
use crate::input::PointerEvent;
use crate::scene::{
    CanvasItem, DeleteItems, ItemId, MoveItems, ResizeImage, Shape, TransformShape,
};

/// 클릭 판정 허용 오차(스크린 px, zoom으로 world 환산). 지우개보다
/// 살짝 타이트하게 잡음 — 얇은 직선을 정확히 집어야 하니까.
const SELECT_HIT_TOLERANCE_SCREEN_PX: f32 = 6.0;
/// 리사이즈/회전 핸들 크기(스크린 px). render.rs도 그리기용으로 이 값을
/// 그대로 참조 — 판정 반경과 그려지는 크기가 어긋나면 안 되니까.
pub(super) const HANDLE_SIZE_SCREEN_PX: f32 = 8.0;
/// 회전 핸들이 도형 위로 떨어진 거리(스크린 px).
pub(super) const ROTATE_HANDLE_DISTANCE_SCREEN_PX: f32 = 28.0;

pub(super) enum SelectDrag {
    Move {
        id: ItemId,
        last_world: [f64; 2],
        total_delta: [f64; 2],
    },
    /// center-anchored 리사이즈 — center/rotation은 안 건드리고
    /// half_extent만 바뀜(코너 어느 걸 잡았는지는 안 따짐, local 좌표
    /// 절대값으로 자동 처리됨).
    ResizeShape {
        id: ItemId,
        before: ([f64; 2], [f64; 2], f32),
    },
    /// Image는 center 개념이 없어서(top_left+size만 있음) 드래그 시작
    /// 시점 중심을 anchor로 캐싱해둬야 함.
    ResizeImage {
        id: ItemId,
        anchor_center: [f64; 2],
        before: ([f64; 2], [f64; 2]),
    },
    RotateShape {
        id: ItemId,
        before: ([f64; 2], [f64; 2], f32),
    },
}

impl App {
    pub(super) fn handle_select_pointer(&mut self, ev: PointerEvent) {
        match ev {
            PointerEvent::Down(s) => self.select_pointer_down(s.pos),
            PointerEvent::Move(s) => self.select_pointer_move(s.pos),
            PointerEvent::Up(_) => self.select_pointer_up(),
            PointerEvent::Hold(_) => {}
        }
    }

    /// 선택된 아이템 삭제(Del 키 전용 경로). 지우개 히트테스트를 안 거치므로
    /// 타입 상관없이 지워짐 — 지우개로 막아둔 이미지도 이 경로로는 삭제 가능.
    pub(super) fn delete_selected_item(&mut self) {
        let Some(id) = self.selected_item.take() else {
            return;
        };
        let Some(item) = self.scene.item(id).cloned() else {
            return;
        };
        let z = self.scene.z_index_of(id).unwrap_or(0);
        self.scene.remove(id);
        let cmd = Box::new(DeleteItems {
            removed: vec![(id, item, z)],
        });
        self.undo_stack.push_already_applied(cmd);
    }

    fn select_pointer_down(&mut self, screen_pos: [f32; 2]) {
        let world = self.camera.screen_to_world(screen_pos);
        let tol = (SELECT_HIT_TOLERANCE_SCREEN_PX / self.camera.zoom) as f64;
        let handle_tol = (HANDLE_SIZE_SCREEN_PX / self.camera.zoom) as f64;

        // 1) 이미 선택된 아이템이 있으면 핸들부터 검사(핸들이 몸통 위에
        // 겹쳐 있을 수 있어서 몸통 클릭 판정보다 먼저 해야 함).
        if let Some(id) = self.selected_item {
            if let Some(item) = self.scene.item(id) {
                if let CanvasItem::Shape(sh) = item {
                    if let Some(handle_pos) = self.rotate_handle_world(sh) {
                        if dist(handle_pos, world) <= handle_tol {
                            self.select_drag = Some(SelectDrag::RotateShape {
                                id,
                                before: (sh.center, sh.half_extent, sh.rotation),
                            });
                            return;
                        }
                    }
                    for corner in sh.world_corners() {
                        if dist(corner, world) <= handle_tol {
                            self.select_drag = Some(SelectDrag::ResizeShape {
                                id,
                                before: (sh.center, sh.half_extent, sh.rotation),
                            });
                            return;
                        }
                    }
                }
                if let CanvasItem::Image(img) = item {
                    let center = [
                        img.top_left[0] + img.size[0] * 0.5,
                        img.top_left[1] + img.size[1] * 0.5,
                    ];
                    let corners = [
                        img.top_left,
                        [
                            img.top_left[0] + img.size[0],
                            img.top_left[1],
                        ],
                        [
                            img.top_left[0] + img.size[0],
                            img.top_left[1] + img.size[1],
                        ],
                        [
                            img.top_left[0],
                            img.top_left[1] + img.size[1],
                        ],
                    ];
                    for corner in corners {
                        if dist(corner, world) <= handle_tol {
                            self.select_drag = Some(SelectDrag::ResizeImage {
                                id,
                                anchor_center: center,
                                before: (img.top_left, img.size),
                            });
                            return;
                        }
                    }
                }
            }
        }

        // 2) 핸들에 안 맞았으면 몸통 클릭 판정 — 맨 위 아이템부터.
        let hit = self
            .scene
            .iter_ordered_with_id_rev()
            .find_map(|(id, item)| item.hit_test(world, tol).then_some(id));

        match hit {
            Some(id) => {
                self.selected_item = Some(id);
                self.select_drag = Some(SelectDrag::Move {
                    id,
                    last_world: world,
                    total_delta: [
                        0.0, 0.0,
                    ],
                });
            }
            None => {
                self.selected_item = None;
                self.select_drag = None;
            }
        }
    }

    fn select_pointer_move(&mut self, screen_pos: [f32; 2]) {
        let world = self.camera.screen_to_world(screen_pos);
        let Some(drag) = &mut self.select_drag else {
            return;
        };

        match drag {
            SelectDrag::Move {
                id,
                last_world,
                total_delta,
            } => {
                let delta = [
                    world[0] - last_world[0],
                    world[1] - last_world[1],
                ];
                if let Some(item) = self.scene.item_mut(*id) {
                    item.translate(delta);
                }
                total_delta[0] += delta[0];
                total_delta[1] += delta[1];
                *last_world = world;
            }
            SelectDrag::ResizeShape {
                id,
                ..
            } => {
                if let Some(CanvasItem::Shape(sh)) = self.scene.item_mut(*id) {
                    let local = sh.to_local(world);
                    sh.half_extent = [
                        local[0].abs(),
                        local[1].abs(),
                    ];
                    sh.geometry_dirty = true;
                }
            }
            SelectDrag::ResizeImage {
                id,
                anchor_center,
                ..
            } => {
                if let Some(CanvasItem::Image(img)) = self.scene.item_mut(*id) {
                    let half = [
                        (world[0] - anchor_center[0]).abs(),
                        (world[1] - anchor_center[1]).abs(),
                    ];
                    img.set_bounds(
                        [
                            anchor_center[0] - half[0],
                            anchor_center[1] - half[1],
                        ],
                        [
                            half[0] * 2.0,
                            half[1] * 2.0,
                        ],
                    );
                }
            }
            SelectDrag::RotateShape {
                id,
                ..
            } => {
                if let Some(CanvasItem::Shape(sh)) = self.scene.item_mut(*id) {
                    let angle = (world[1] - sh.center[1]).atan2(world[0] - sh.center[0]);
                    sh.rotation = (angle + std::f64::consts::FRAC_PI_2) as f32;
                    sh.geometry_dirty = true;
                }
            }
        }
    }

    fn select_pointer_up(&mut self) {
        let Some(drag) = self.select_drag.take() else {
            return;
        };
        match drag {
            SelectDrag::Move {
                id,
                total_delta,
                ..
            } => {
                if total_delta
                    != [
                        0.0, 0.0,
                    ]
                {
                    let cmd = Box::new(MoveItems {
                        ids: vec![id],
                        delta: total_delta,
                    });
                    self.undo_stack.push_already_applied(cmd);
                }
            }
            SelectDrag::ResizeShape {
                id,
                before,
            } => {
                if let Some(CanvasItem::Shape(sh)) = self.scene.item(id) {
                    let after = (sh.center, sh.half_extent, sh.rotation);
                    if after != before {
                        let cmd = Box::new(TransformShape {
                            id,
                            before,
                            after,
                        });
                        self.undo_stack.push_already_applied(cmd);
                    }
                }
            }
            SelectDrag::ResizeImage {
                id,
                before,
                ..
            } => {
                if let Some(CanvasItem::Image(img)) = self.scene.item(id) {
                    let after = (img.top_left, img.size);
                    if after != before {
                        let cmd = Box::new(ResizeImage {
                            id,
                            before,
                            after,
                        });
                        self.undo_stack.push_already_applied(cmd);
                    }
                }
            }
            SelectDrag::RotateShape {
                id,
                before,
            } => {
                if let Some(CanvasItem::Shape(sh)) = self.scene.item(id) {
                    let after = (sh.center, sh.half_extent, sh.rotation);
                    if after != before {
                        let cmd = Box::new(TransformShape {
                            id,
                            before,
                            after,
                        });
                        self.undo_stack.push_already_applied(cmd);
                    }
                }
            }
        }
    }

    /// 회전 핸들의 world 좌표. render.rs가 그릴 때도 같은 공식을 씀
    /// (숫자로 왕복 검증 완료 — 각도 계산 서로 어긋나지 않음).
    pub(super) fn rotate_handle_world(&self, sh: &Shape) -> Option<[f64; 2]> {
        let d = (ROTATE_HANDLE_DISTANCE_SCREEN_PX / self.camera.zoom) as f64;
        let angle = sh.rotation as f64 - std::f64::consts::FRAC_PI_2;
        Some([
            sh.center[0] + d * angle.cos(),
            sh.center[1] + d * angle.sin(),
        ])
    }
}

fn dist(a: [f64; 2], b: [f64; 2]) -> f64 {
    ((a[0] - b[0]).powi(2) + (a[1] - b[1]).powi(2)).sqrt()
}
