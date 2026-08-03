// ============================================================
// src/app/render/cursor.rs
// ============================================================
// [빌드 미검증 — wgpu 네이티브 의존]
// 지우개 인디케이터 / 선택 핸들 / 커스텀 포인터 커서. 도구함 캐시
// (ui_cache.rs)와 완전히 독립된 시스템 — 매 프레임 위치가 바뀌므로
// 항상 즉석으로 그린다.
//
// [설계 변경] 예전엔 이 함수들이 device/pass를 직접 받아서 선분 하나당
// GPU 버퍼를 새로 만들었음(create_buffer_init 개별 호출). 이제
// OverlayBuilder에 정점만 쌓는 push_* 계열로 바뀌었고, 실제 GPU
// 업로드/드로우는 mod.rs가 프레임당 한 번만 처리(overlay.rs 참고).
// 즉 이 파일은 더 이상 wgpu Device/RenderPass를 직접 다루지 않음 —
// 순수하게 "어떤 선분/사각형을 어디에 그릴지"만 계산.

use super::overlay::OverlayBuilder;
use crate::app::Tool;
use crate::render::camera::Camera;
use crate::scene::CanvasItem;
use crate::ui;

const ERASER_INDICATOR_DASH_COUNT: usize = 16;
const ERASER_INDICATOR_LINE_WIDTH: f32 = 2.0;
const ERASER_INDICATOR_COLOR: [f32; 4] = [
    0.2, 0.2, 0.2, 0.6,
];

const SELECTION_LINE_WIDTH: f32 = 1.5;
const SELECTION_COLOR: [f32; 4] = [
    0.1, 0.4, 0.9, 0.9,
];

pub(super) fn draw_eraser_indicator(builder: &mut OverlayBuilder, center: [f32; 2], radius: f32) {
    let slots = ERASER_INDICATOR_DASH_COUNT * 2;
    for i in 0..ERASER_INDICATOR_DASH_COUNT {
        let a0 = (i * 2) as f32 / slots as f32 * std::f32::consts::TAU;
        let a1 = (i * 2 + 1) as f32 / slots as f32 * std::f32::consts::TAU;
        let p0 = [
            center[0] + radius * a0.cos(),
            center[1] + radius * a0.sin(),
        ];
        let p1 = [
            center[0] + radius * a1.cos(),
            center[1] + radius * a1.sin(),
        ];
        builder.push_line_segment(p0, p1, ERASER_INDICATOR_LINE_WIDTH, ERASER_INDICATOR_COLOR);
    }
}

fn draw_handle_square(builder: &mut OverlayBuilder, center: [f32; 2], size: f32) {
    let half = size * 0.5;
    let rect = ui::Rect {
        x: center[0] - half,
        y: center[1] - half,
        w: size,
        h: size,
    };
    builder.push_quad(rect, SELECTION_COLOR);
}

/// 선택된 아이템 위에 점선(을 흉내낸 얇은 실선) bbox + (도형이면)
/// 리사이즈/회전 핸들을 그림.
pub(super) fn draw_selection_overlay(
    builder: &mut OverlayBuilder,
    camera: &Camera,
    item: &CanvasItem,
) {
    match item {
        CanvasItem::Shape(sh) => {
            let corners_screen: Vec<[f32; 2]> = sh
                .world_corners()
                .iter()
                .map(|&c| camera.world_to_screen(c))
                .collect();
            for i in 0..4 {
                builder.push_line_segment(
                    corners_screen[i],
                    corners_screen[(i + 1) % 4],
                    SELECTION_LINE_WIDTH,
                    SELECTION_COLOR,
                );
            }
            for c in &corners_screen {
                draw_handle_square(builder, *c, crate::app::select::HANDLE_SIZE_SCREEN_PX);
            }

            let d = (crate::app::select::ROTATE_HANDLE_DISTANCE_SCREEN_PX / camera.zoom) as f64;
            let angle = sh.rotation as f64 - std::f64::consts::FRAC_PI_2;
            let handle_world = [
                sh.center[0] + d * angle.cos(),
                sh.center[1] + d * angle.sin(),
            ];
            let handle_screen = camera.world_to_screen(handle_world);
            let top_mid_screen = [
                (corners_screen[0][0] + corners_screen[1][0]) * 0.5,
                (corners_screen[0][1] + corners_screen[1][1]) * 0.5,
            ];
            builder.push_line_segment(
                top_mid_screen,
                handle_screen,
                SELECTION_LINE_WIDTH,
                SELECTION_COLOR,
            );
            draw_handle_square(
                builder,
                handle_screen,
                crate::app::select::HANDLE_SIZE_SCREEN_PX,
            );
        }
        CanvasItem::Image(_) => {
            let (min, max) = item.bounding_box();
            let corners_world = [
                min,
                [
                    max[0], min[1],
                ],
                max,
                [
                    min[0], max[1],
                ],
            ];
            let corners_screen: Vec<[f32; 2]> = corners_world
                .iter()
                .map(|&c| camera.world_to_screen(c))
                .collect();
            for i in 0..4 {
                builder.push_line_segment(
                    corners_screen[i],
                    corners_screen[(i + 1) % 4],
                    SELECTION_LINE_WIDTH,
                    SELECTION_COLOR,
                );
            }
            for c in &corners_screen {
                draw_handle_square(builder, *c, crate::app::select::HANDLE_SIZE_SCREEN_PX);
            }
        }
        CanvasItem::Stroke(_) => {
            let (min, max) = item.bounding_box();
            let corners_world = [
                min,
                [
                    max[0], min[1],
                ],
                max,
                [
                    min[0], max[1],
                ],
            ];
            let corners_screen: Vec<[f32; 2]> = corners_world
                .iter()
                .map(|&c| camera.world_to_screen(c))
                .collect();
            for i in 0..4 {
                builder.push_line_segment(
                    corners_screen[i],
                    corners_screen[(i + 1) % 4],
                    SELECTION_LINE_WIDTH,
                    SELECTION_COLOR,
                );
            }
            // 핸들 없음 — 이동만 가능.
        }
    }
}

/// 커서용 도구 아이콘. `center`는 "아이콘 중심"이 아니라 "실제 포인터가
/// 닿는 지점(핫스팟)" — Pen은 펜촉 끝, Select는 화살표 뾰족한 끝점이
/// 정확히 이 좌표에 오도록 전체 아이콘을 반대로 밀어서 그림.
pub(super) fn draw_tool_icon_at(
    builder: &mut OverlayBuilder,
    tool: Tool,
    center: [f32; 2],
    size: f32,
    color: [f32; 4],
) {
    let cx = center[0];
    let cy = center[1];
    let s = size * 0.45;
    let w = 1.5;

    match tool {
        Tool::Pen => {
            let local = |x: f32, y: f32| -> [f32; 2] {
                let angle = std::f32::consts::FRAC_PI_4;
                [
                    x * angle.cos() - y * angle.sin(),
                    x * angle.sin() + y * angle.cos(),
                ]
            };
            let pw = s * 0.4;
            let ph = s * 0.8;
            let pt = s * 1.3;

            let tip_local = local(0.0, pt);
            let anchor = |x: f32, y: f32| -> [f32; 2] {
                let l = local(x, y);
                [
                    cx + l[0] - tip_local[0],
                    cy + l[1] - tip_local[1],
                ]
            };

            let tl = anchor(-pw, -ph);
            let tr = anchor(pw, -ph);
            let bl = anchor(-pw, ph);
            let br = anchor(pw, ph);
            let tip = anchor(0.0, pt);
            builder.push_line_segment(tl, tr, w, color);
            builder.push_line_segment(tl, bl, w, color);
            builder.push_line_segment(tr, br, w, color);
            builder.push_line_segment(bl, br, w, color);
            builder.push_line_segment(bl, tip, w, color);
            builder.push_line_segment(br, tip, w, color);
        }
        Tool::Select => {
            let p0_local = [
                -s * 0.4,
                -s * 0.7,
            ];
            let p1_local = [
                s * 0.6,
                s * 0.4,
            ];
            let p2_local = [
                0.0,
                s * 0.2,
            ];
            let p3_local = [
                -s * 0.4,
                s * 0.8,
            ];

            let shift = |l: [f32; 2]| -> [f32; 2] {
                [
                    cx + l[0] - p0_local[0],
                    cy + l[1] - p0_local[1],
                ]
            };

            let p0 = [
                cx, cy,
            ];
            let p1 = shift(p1_local);
            let p2 = shift(p2_local);
            let p3 = shift(p3_local);
            builder.push_line_segment(p0, p1, w, color);
            builder.push_line_segment(p1, p2, w, color);
            builder.push_line_segment(p2, p3, w, color);
            builder.push_line_segment(p3, p0, w, color);
        }
        Tool::Eraser => {} // 지우개는 기존 점선 원 인디케이터만 사용, 아이콘 커서 없음
    }
}
