// ============================================================
// src/app/shapes.rs
// ============================================================
// 도형 자동스냅(Shape Recognizer) — App 상태와 무관한 순수 로직만 모음.
// Hold 이벤트가 오면 pointer.rs가 recognize_shape를 호출해서 그리는
// 중인 Stroke를 실제 CanvasItem::Shape(center/half_extent/rotation
// 통일 모델)로 변환한다. 이후 펜을 계속 누른 채 움직이면 pointer.rs가
// SnapData를 참고해 그 Shape을 라이브로 리사이즈/회전함.
//
// [알려진 제약] Shape enum(scene/item.rs)엔 Circle/Line/Rectangle
// 세 종류만 있고 Triangle이 없음 — RDP 결과가 4점(삼각형 후보)이어도
// 지금은 원으로 분류됨. 예전엔 "가짜 정형화"로나마 삼각형 모양이라도
// 나왔는데, 이번 리팩터로 그마저 사라진 셈 — 필요하면
// ShapeKind::Triangle을 추가해서 되살릴 수 있음(지금은 스코프 밖,
// 명시적으로 확인 안 받고 넘어간 부분이라 짚어둠).

use crate::scene::{PenPoint, Shape, ShapeKind, Stroke};

pub struct SnapData {
    pub kind: ShapeKind,
    pub center: [f64; 2], // 원/사각형: 회전+스케일 기준 중심(고정)
    pub initial_pen: [f64; 2], // 스냅된 순간의 펜 위치
    pub initial_half_extent: [f64; 2],
    pub initial_rotation: f32,
    pub line_start: [f64; 2], // 직선 전용: 고정된 시작점
}

/// RDP(Ramer-Douglas-Peucker) 알고리즘으로 자잘한 곡선을 단순한 다각형으로 축약합니다.
fn rdp(points: &[PenPoint], epsilon: f64, out: &mut Vec<PenPoint>) {
    if points.is_empty() {
        return;
    }

    let mut dmax = 0.0;
    let mut index = 0;
    let end = points.len() - 1;

    for i in 1..end {
        let d = crate::scene::segment_dist_sq(points[0].pos, points[end].pos, points[i].pos);
        if d > dmax {
            index = i;
            dmax = d;
        }
    }

    if dmax > epsilon * epsilon {
        let mut rec_results1 = Vec::new();
        rdp(&points[0..=index], epsilon, &mut rec_results1);

        let mut rec_results2 = Vec::new();
        rdp(&points[index..=end], epsilon, &mut rec_results2);

        out.extend_from_slice(&rec_results1[0..rec_results1.len() - 1]);
        out.extend_from_slice(&rec_results2);
    } else {
        out.push(points[0].clone());
        out.push(points[end].clone());
    }
}

/// 자유필기 스트로크를 분석해서 완벽한 기하학적 Shape로 인식을 시도.
/// 성공하면 (초기 Shape, 라이브 드래그용 SnapData)를 돌려줌.
#[doc(hidden)]
pub fn recognize_shape(stroke: &Stroke) -> Option<(Shape, SnapData)> {
    if stroke.points.len() < 10 {
        return None;
    }

    let (min, max) = crate::scene::stroke_bbox(stroke);
    let diag = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2)).sqrt();
    if diag < 10.0 {
        return None;
    }

    let first = stroke.points.first().unwrap().pos;
    let last = stroke.points.last().unwrap().pos;

    let start_end_dist = ((first[0] - last[0]).powi(2) + (first[1] - last[1]).powi(2)).sqrt();
    let closed = start_end_dist < diag * 0.2;

    if !closed {
        let mid = [
            (first[0] + last[0]) * 0.5,
            (first[1] + last[1]) * 0.5,
        ];
        let vx = last[0] - first[0];
        let vy = last[1] - first[1];
        let length = (vx * vx + vy * vy).sqrt();
        let rotation = vy.atan2(vx) as f32;
        let half_extent = [
            length * 0.5,
            0.0,
        ];

        let shape = Shape {
            kind: ShapeKind::Line,
            center: mid,
            half_extent,
            rotation,
            color: stroke.color,
            stroke_width: stroke.base_width,
            geometry_dirty: true,
        };
        let snap = SnapData {
            kind: ShapeKind::Line,
            center: mid,
            initial_pen: last,
            initial_half_extent: half_extent,
            initial_rotation: rotation,
            line_start: first,
        };
        return Some((shape, snap));
    }

    let mut process_points = stroke.points.clone();
    let last_idx = process_points.len() - 1;
    process_points[last_idx].pos = process_points[0].pos;

    let mut simplified = Vec::new();
    rdp(&process_points, diag * 0.12, &mut simplified);
    let v_count = simplified.len();

    // v_count==5(4개 꼭짓점+닫는 점)면 사각형, 그 외(원래 삼각형 후보인
    // v_count==4 포함)는 전부 원으로 분류.
    // 수정, 5점(꼭짓점 4개+닫는 점)은 사각형, 4점(꼭짓점 3개+닫는 점)은 삼각형, 나머진 원
    let kind = if v_count == 5 {
        ShapeKind::Rectangle
    } else if v_count == 4 {
        ShapeKind::Triangle
    } else {
        ShapeKind::Circle
    };

    let center = [
        (min[0] + max[0]) * 0.5,
        (min[1] + max[1]) * 0.5,
    ];
    let half_extent = [
        (max[0] - min[0]) * 0.5,
        (max[1] - min[1]) * 0.5,
    ];

    let shape = Shape {
        kind,
        center,
        half_extent,
        rotation: 0.0,
        color: stroke.color,
        stroke_width: stroke.base_width,
        geometry_dirty: true,
    };
    let snap = SnapData {
        kind,
        center,
        initial_pen: last,
        initial_half_extent: half_extent,
        initial_rotation: 0.0,
        line_start: first,
    };
    Some((shape, snap))
}
