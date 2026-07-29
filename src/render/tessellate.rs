// ============================================================
// src/render/tessellate.rs
// ============================================================
// Stroke(포인트+압력 리스트) -> 렌더용 삼각형 메시 변환.
// "포인트마다 원을 찍고, 연속된 포인트 사이는 사각형으로 잇는다" 방식
// (stamped-circle) 으로 라운드 조인/캡을 동시에 얻는다.
//
// IncrementalStrokeMesh: stamp_circle/connect_quad가 원래 순수 로컬 +
// append 전용 연산이라는 점을 이용해, "포인트 1개 추가"를 독립적으로
// 뽑아낸 버전. 그리는 중인 자유획(app::pointer)이 포인트를 push할
// 때마다 여기에도 같이 push해서, 매 프레임 전체 재테셀레이션을 피하기
// 위해 도입됨. tessellate_stroke(원샷, 커밋된 아이템/도형 프리뷰용)도
// 내부적으로 이 위에서 재구현 — 로직 중복 방지.

use crate::scene::Stroke;

const CIRCLE_SEGMENTS: usize = 8; // [미검증 가설] 시각적으로 충분히 둥근지 눈으로 확인 후 조정

pub struct StrokeMesh {
    /// 이 메시의 정점들이 상대적으로 표현된 기준점 (world 좌표, f64).
    pub origin: [f64; 2],
    /// origin 기준 로컬 좌표 (f32) — 그리는 시점에 카메라 오프셋을 더해야 함.
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
}

/// 점진적으로 append 가능한 스트로크 메시. push_point를 호출한 순서대로
/// "원 스탬프 + 직전 점과의 연결 사각형"이 쌓인다 — 이미 쌓인 부분은
/// 다시 안 건드림(순수 append, O(1) amortized per point).
pub struct IncrementalStrokeMesh {
    pub origin: [f64; 2],
    pub vertices: Vec<[f32; 2]>,
    pub indices: Vec<u32>,
    last_point: Option<([f32; 2], f32)>, // (직전 점 로컬좌표, half_width)
}

impl IncrementalStrokeMesh {
    pub fn new(origin: [f64; 2]) -> Self {
        Self { origin, vertices: Vec::new(), indices: Vec::new(), last_point: None }
    }

    /// world 좌표 점 하나(+half_width)를 메시에 추가. origin 기준
    /// 로컬로 변환 후 stamp_circle, 직전 점이 있으면 connect_quad로 이음.
    pub fn push_point(&mut self, world_pos: [f64; 2], half_width: f32) {
        let local = [
            (world_pos[0] - self.origin[0]) as f32,
            (world_pos[1] - self.origin[1]) as f32,
        ];
        stamp_circle(local, half_width, &mut self.vertices, &mut self.indices);
        if let Some((prev_local, prev_hw)) = self.last_point {
            connect_quad(prev_local, prev_hw, local, half_width, &mut self.vertices, &mut self.indices);
        }
        self.last_point = Some((local, half_width));
    }
}

/// 원샷 전체 테셀레이션 — 커밋된 아이템(GpuResourceRegistry)이나 도형
/// 프리뷰(as_stroke)처럼 "매번 처음부터 다시 만들어도 되는" 경우용.
/// 내부적으로 IncrementalStrokeMesh를 그대로 재사용.
pub fn tessellate_stroke(stroke: &Stroke) -> StrokeMesh {
    let Some(first) = stroke.points.first() else {
        return StrokeMesh { origin: [0.0, 0.0], vertices: Vec::new(), indices: Vec::new() };
    };

    let mut mesh = IncrementalStrokeMesh::new(first.pos);
    for p in &stroke.points {
        let half_width = stroke.base_width * p.pressure.max(0.05) * 0.5;
        mesh.push_point(p.pos, half_width);
    }

    StrokeMesh { origin: mesh.origin, vertices: mesh.vertices, indices: mesh.indices }
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