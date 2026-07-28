// ============================================================
// src/render/tessellate.rs
// ============================================================
// Stroke(포인트+압력 리스트) -> 렌더용 삼각형 메시 변환.
// "포인트마다 원을 찍고, 연속된 포인트 사이는 사각형으로 잇는다" 방식
// (stamped-circle) 으로 라운드 조인/캡을 동시에 얻는다.

use crate::scene::Stroke;

const CIRCLE_SEGMENTS: usize = 8; // [미검증 가설] 시각적으로 충분히 둥근지 눈으로 확인 후 조정

pub struct StrokeMesh {
    /// 이 메시의 정점들이 상대적으로 표현된 기준점 (world 좌표, f64).
    pub origin: [f64; 2],
    /// origin 기준 로컬 좌표 (f32) — 그리는 시점에 카메라 오프셋을 더해야 함.
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

pub fn tessellate_stroke(stroke: &Stroke) -> StrokeMesh {
    let mut mesh = StrokeMesh { origin: [0.0, 0.0], vertices: Vec::new(), indices: Vec::new() };

    let Some(first) = stroke.points.first() else {
        return mesh; // 빈 스트로크 — 방어적으로 빈 메시 반환
    };
    mesh.origin = first.pos;

    // origin 기준 로컬 좌표로 미리 변환 (f64 뺄셈 후 f32 캐스팅).
    let local: Vec<[f32; 2]> = stroke
        .points
        .iter()
        .map(|p| {
            [
                (p.pos[0] - mesh.origin[0]) as f32,
                (p.pos[1] - mesh.origin[1]) as f32,
            ]
        })
        .collect();

    for (i, p) in stroke.points.iter().enumerate() {
        let half_width = stroke.base_width * p.pressure.max(0.05) * 0.5; // 압력 0이어도 최소 두께는 보장
        stamp_circle(local[i], half_width, &mut mesh.vertices, &mut mesh.indices);
    }

    for i in 0..local.len().saturating_sub(1) {
        let hw0 = stroke.base_width * stroke.points[i].pressure.max(0.05) * 0.5;
        let hw1 = stroke.base_width * stroke.points[i + 1].pressure.max(0.05) * 0.5;
        connect_quad(local[i], hw0, local[i + 1], hw1, &mut mesh.vertices, &mut mesh.indices);
    }

    mesh
}

fn stamp_circle(center: [f32; 2], radius: f32, vertices: &mut Vec<[f32; 2]>, indices: &mut Vec<u32>) {
    let base = vertices.len() as u32;
    vertices.push(center); // 부채꼴 중심점
    for i in 0..=CIRCLE_SEGMENTS {
        let theta = i as f32 / CIRCLE_SEGMENTS as f32 * std::f32::consts::TAU;
        vertices.push([center[0] + radius * theta.cos(), center[1] + radius * theta.sin()]);
    }
    for i in 0..CIRCLE_SEGMENTS as u32 {
        indices.push(base);
        indices.push(base + 1 + i);
        indices.push(base + 2 + i);
    }
}

/// 두 원(반지름 다를 수 있음) 사이를 사각형(삼각형 2개)으로 연결.
/// 세그먼트 진행 방향에 수직인 법선 방향으로 각 반폭만큼 밀어서 4개
/// 꼭짓점을 만든다.
fn connect_quad(
    p0: [f32; 2],
    hw0: f32,
    p1: [f32; 2],
    hw1: f32,
    vertices: &mut Vec<[f32; 2]>,
    indices: &mut Vec<u32>,
) {
    let dir = [p1[0] - p0[0], p1[1] - p0[1]];
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if len < f32::EPSILON {
        return; // 같은 위치에 찍힌 포인트 — 연결 사각형 불필요 (원끼리 겹쳐서 이미 채워짐)
    }
    let normal = [-dir[1] / len, dir[0] / len];

    let base = vertices.len() as u32;
    vertices.push([p0[0] + normal[0] * hw0, p0[1] + normal[1] * hw0]); // base+0
    vertices.push([p0[0] - normal[0] * hw0, p0[1] - normal[1] * hw0]); // base+1
    vertices.push([p1[0] + normal[0] * hw1, p1[1] + normal[1] * hw1]); // base+2
    vertices.push([p1[0] - normal[0] * hw1, p1[1] - normal[1] * hw1]); // base+3

    indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
}