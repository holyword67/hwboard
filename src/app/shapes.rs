// ============================================================
// src/app/shapes.rs
// ============================================================
// 도형 자동스냅(Shape Recognizer) — App 상태와 무관한 순수 로직만 모음.
// Hold 이벤트가 오면 pointer.rs가 이 모듈의 recognize_and_snap_shape를
// 호출해서 그리는 중인 Stroke를 완벽한 도형 좌표로 치환한다.
// 다음 작업(도형 시스템 갈아엎기)이 벌어질 파일이라 App 로직과 의도적으로
// 분리해둠 — 이 파일만 건드려도 나머지 앱 로직 diff가 오염되지 않게.

use crate::scene::{PenPoint, Stroke};

pub(super) struct SnapData {
    pub(super) center: [f64; 2],
    pub(super) local_points: Vec<[f64; 2]>, // 중심점 기준 상대 좌표 (회전/크기 조절의 원본)
    pub(super) initial_pen: [f64; 2],       // 스냅된 순간의 펜 위치 (드래그 기준점)
    pub(super) is_line: bool,               // 직선은 회전/크기 대신 한쪽 끝점만 고무줄처럼 따라가야 함
}

/// RDP(Ramer-Douglas-Peucker) 알고리즘으로 자잘한 곡선을 단순한 다각형으로 축약합니다.
fn rdp(points: &[PenPoint], epsilon: f64, out: &mut Vec<PenPoint>) {
    if points.is_empty() { return; }

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

/// 스트로크를 분석하여 적절한 완벽한 기하학적 도형으로 변환합니다.
pub(super) fn recognize_and_snap_shape(stroke: &mut Stroke) -> Option<SnapData> {
    if stroke.points.len() < 10 { return None; }

    let (min, max) = crate::scene::stroke_bbox(stroke);
    let diag = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2)).sqrt();
    if diag < 10.0 { return None; }

    let first = stroke.points.first().unwrap().pos;
    let last = stroke.points.last().unwrap().pos;
    let initial_pen = last; // 스냅 발동 순간 펜의 위치

    let start_end_dist = ((first[0] - last[0]).powi(2) + (first[1] - last[1]).powi(2)).sqrt();
    let closed = start_end_dist < diag * 0.2;
    let avg_pressure = stroke.points.iter().map(|p| p.pressure).sum::<f32>() / stroke.points.len() as f32;

    if !closed {
        // [1. 직선]
        stroke.points = vec![
            PenPoint { pos: first, pressure: avg_pressure },
            PenPoint { pos: last, pressure: avg_pressure },
        ];
        stroke.mesh_dirty = true;

        // 직선은 회전 행렬 대신 "시작점 고정, 끝점 펜 추적"을 사용하므로 중심점을 시작점(first)으로 둠
        return Some(SnapData { center: first, local_points: vec![], initial_pen, is_line: true });

    } else {
        // [닫힌 도형 처리]
        let mut process_points = stroke.points.clone();
        let last_idx = process_points.len() - 1;
        process_points[last_idx].pos = process_points[0].pos;

        let mut simplified = Vec::new();
        rdp(&process_points, diag * 0.12, &mut simplified);
        let v_count = simplified.len();

        if v_count == 4 { // 삼각형
            stroke.points = simplified.into_iter().map(|mut p| { p.pressure = avg_pressure; p }).collect();
        } else if v_count == 5 { // 직사각형
            stroke.points = vec![
                PenPoint { pos: [min[0], min[1]], pressure: avg_pressure },
                PenPoint { pos: [max[0], min[1]], pressure: avg_pressure },
                PenPoint { pos: [max[0], max[1]], pressure: avg_pressure },
                PenPoint { pos: [min[0], max[1]], pressure: avg_pressure },
                PenPoint { pos: [min[0], min[1]], pressure: avg_pressure },
            ];
        } else { // 원
            let center_x = (min[0] + max[0]) / 2.0;
            let center_y = (min[1] + max[1]) / 2.0;
            let r = ((max[0] - min[0]) + (max[1] - min[1])) / 4.0;

            let mut circle_pts = Vec::new();
            let segments = 64;
            for i in 0..=segments {
                let theta = (i as f64 / segments as f64) * std::f64::consts::TAU;
                circle_pts.push(PenPoint {
                    pos: [center_x + r * theta.cos(), center_y + r * theta.sin()],
                    pressure: avg_pressure,
                });
            }
            stroke.points = circle_pts;
        }

        stroke.mesh_dirty = true;

        // 만들어진 완벽한 도형의 중심점과, 그 중심점 기준의 상대 좌표를 기록해 둡니다 (회전용)
        let (s_min, s_max) = crate::scene::stroke_bbox(stroke);
        let center = [(s_min[0] + s_max[0]) / 2.0, (s_min[1] + s_max[1]) / 2.0];
        let local_points = stroke.points.iter().map(|p| [p.pos[0] - center[0], p.pos[1] - center[1]]).collect();

        return Some(SnapData { center, local_points, initial_pen, is_line: false });
    }
}