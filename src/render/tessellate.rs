// ============================================================
// src/render/tessellate.rs
// ============================================================
// Stroke(포인트+압력 리스트) -> SDF 캡슐 기반 렌더 메시 변환. 포인트마다
// 원을 스탬프하던 기존 방식 대신, 세그먼트(인접 두 점) 하나당 quad
// 하나만 만든다 — 라운드 조인/캡은 capsule_stroke.wgsl의 거리함수가
// 셰이더에서 알아서 처리(기하학적으로 미리 만들 필요 없음).
//
// IncrementalStrokeMesh: push_point 한 번 = 세그먼트 quad 하나 append
// (첫 점은 예외 — "자기 자신과의 degenerate 세그먼트"로 점(dot) 하나를
// 만들어서, 움직이기 전에도 펜 닿은 자리가 바로 보이게 함). append 전용
// 연산이라 그리는 중인 자유획(app::pointer)이 점을 push할 때마다 여기
// 그대로 재사용 가능.

use crate::render::capsule_pipeline::StrokeVertex;
use crate::scene::Stroke;

/// [미검증 가설] SDF 경계 바깥으로 quad를 얼마나 넉넉히 잡을지(world
/// 단위). 너무 작으면 안티앨리어싱 전환대가 잘려서 계단져 보이고, 너무
/// 크면 낭비되는 프래그먼트가 늘어남. 실사용 후 조정 대상.
const AA_PADDING_WORLD: f32 = 1.5;

pub struct StrokeMesh {
    /// 이 메시의 정점들이 상대적으로 표현된 기준점 (world 좌표, f64).
    pub origin: [f64; 2],
    pub vertices: Vec<StrokeVertex>,
    pub indices: Vec<u32>,
}

/// 점진적으로 append 가능한 캡슐 스트로크 메시.
pub struct IncrementalStrokeMesh {
    pub origin: [f64; 2],
    pub vertices: Vec<StrokeVertex>,
    pub indices: Vec<u32>,
    last_point: Option<([f32; 2], f32)>, // (직전 점 로컬좌표, half_width)
}

impl IncrementalStrokeMesh {
    pub fn new(origin: [f64; 2]) -> Self {
        Self { origin, vertices: Vec::new(), indices: Vec::new(), last_point: None }
    }

    /// world 좌표 점 하나(+half_width)를 추가. 직전 점이 없으면(첫 점)
    /// degenerate 세그먼트(A=B)로 점 하나를 찍고, 있으면 직전 점과 이
    /// 점을 잇는 세그먼트 quad 하나를 추가.
    pub fn push_point(&mut self, world_pos: [f64; 2], half_width: f32) {
        let local = [
            (world_pos[0] - self.origin[0]) as f32,
            (world_pos[1] - self.origin[1]) as f32,
        ];
        match self.last_point {
            None => push_segment_quad(local, half_width, local, half_width, &mut self.vertices, &mut self.indices),
            Some((prev_local, prev_hw)) => {
                push_segment_quad(prev_local, prev_hw, local, half_width, &mut self.vertices, &mut self.indices)
            }
        }
        self.last_point = Some((local, half_width));
    }
}

/// 원샷 전체 테셀레이션 — 커밋된 아이템(GpuResourceRegistry)이나 도형
/// 프리뷰(as_stroke)처럼 "매번 처음부터 다시 만들어도 되는" 경우용.
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

/// 세그먼트(a-b, 반지름 ra/rb) 하나를 감싸는 quad 하나를 추가. a==b(같은
/// 점)면 그 점 하나짜리 "점(dot)"을 표현하는 정사각형 quad가 됨 —
/// capsule_sdf 공식이 이 경우도 그대로 처리(세그먼트 길이 0 → a에 대한
/// 원판정으로 자동 귀결).
fn push_segment_quad(
    a: [f32; 2],
    ra: f32,
    b: [f32; 2],
    rb: f32,
    vertices: &mut Vec<StrokeVertex>,
    indices: &mut Vec<u32>,
) {
    let dir = [b[0] - a[0], b[1] - a[1]];
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    let max_r = ra.max(rb) + AA_PADDING_WORLD;

    // 방향 벡터가 정의 안 되는 경우(점 하나)는 임의의 축으로.
    let (unit_dir, normal) = if len > f32::EPSILON {
        let u = [dir[0] / len, dir[1] / len];
        (u, [-u[1], u[0]])
    } else {
        ([1.0, 0.0], [0.0, 1.0])
    };

    // 양 끝을 max_r만큼 더 늘려서(둥근 캡까지 포함) 넉넉한 bounding quad로.
    let ext_a = [a[0] - unit_dir[0] * max_r, a[1] - unit_dir[1] * max_r];
    let ext_b = [b[0] + unit_dir[0] * max_r, b[1] + unit_dir[1] * max_r];

    let base = vertices.len() as u32;
    let make = |p: [f32; 2]| StrokeVertex { pos: p, seg_a: a, seg_b: b, radii: [ra, rb] };
    vertices.push(make([ext_a[0] + normal[0] * max_r, ext_a[1] + normal[1] * max_r])); // base+0
    vertices.push(make([ext_a[0] - normal[0] * max_r, ext_a[1] - normal[1] * max_r])); // base+1
    vertices.push(make([ext_b[0] + normal[0] * max_r, ext_b[1] + normal[1] * max_r])); // base+2
    vertices.push(make([ext_b[0] - normal[0] * max_r, ext_b[1] - normal[1] * max_r])); // base+3

    indices.extend_from_slice(&[base, base + 1, base + 2, base + 1, base + 3, base + 2]);
}