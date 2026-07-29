// ============================================================
// src/render/tessellate.rs
// ============================================================
// Stroke(포인트+압력 리스트) -> 리본형 렌더 메시 변환. 점마다 좌/우
// 오프셋 정점 쌍 하나씩만 만들고 인접 쌍끼리 삼각형 스트립으로 이음 —
// "세그먼트/관절"이라는 개념 자체가 없어서(점 하나 = 평균 접선 하나) 
// 겹치는 프리미티브가 아예 없음. 예전 "포인트마다 원 스탬프" 방식이
// 관절마다 완전한 원을 겹쳐 그리면서 만들던 라운드 조인 돌기/이중
// 블렌딩 문제가 구조적으로 발생 불가능해짐. SDF 셰이더도 불필요 —
// 겹침이 없으니 평범한 삼각형 채우기 + 기존 4x MSAA로 충분.

use crate::render::pipeline::Vertex;
use crate::scene::Stroke;

pub struct StrokeMesh {
    pub origin: [f64; 2],
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
}

/// 점진적으로 append 가능한 리본형 스트로크 메시.
pub struct IncrementalStrokeMesh {
    pub origin: [f64; 2],
    pub vertices: Vec<Vertex>,
    pub indices: Vec<u32>,
    has_prev_pair: bool,
}

impl IncrementalStrokeMesh {
    pub fn new(origin: [f64; 2]) -> Self {
        Self { origin, vertices: Vec::new(), indices: Vec::new(), has_prev_pair: false }
    }

    /// 점 하나(월드좌표+half_width+단위접선벡터)를 리본에 추가. tangent는
    /// 호출부가 이웃 점들로부터 미리 계산해서 넘김 — 여기선 오프셋+
    /// 스트립 연결만 담당(순수 append, 이전 정점은 다시 안 건드림).
    pub fn push_point(&mut self, world_pos: [f64; 2], half_width: f32, tangent: [f32; 2]) {
        let local = [
            (world_pos[0] - self.origin[0]) as f32,
            (world_pos[1] - self.origin[1]) as f32,
        ];
        let normal = [-tangent[1], tangent[0]];
        let left = [local[0] + normal[0] * half_width, local[1] + normal[1] * half_width];
        let right = [local[0] - normal[0] * half_width, local[1] - normal[1] * half_width];

        let base = self.vertices.len() as u32;
        self.vertices.push(Vertex { pos: left });
        self.vertices.push(Vertex { pos: right });

        if self.has_prev_pair {
            self.indices.extend_from_slice(&[
                base - 2, base - 1, base,
                base - 1, base + 1, base,
            ]);
        }
        self.has_prev_pair = true;
    }
}

/// 점 i의 접선을 중심차분으로 추정. prev/next가 없으면(경로 끝) 한쪽만
/// 보는 편측차분으로 자연스럽게 귀결. 라이브 지오메트리 지연 단계
/// (app::pointer)와 원샷 tessellate_stroke가 동일 공식을 공유.
pub fn estimate_tangent(prev: Option<[f64; 2]>, cur: [f64; 2], next: Option<[f64; 2]>) -> [f32; 2] {
    let a = prev.unwrap_or(cur);
    let b = next.unwrap_or(cur);
    let dir = [(b[0] - a[0]) as f32, (b[1] - a[1]) as f32];
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if len > f32::EPSILON { [dir[0] / len, dir[1] / len] } else { [1.0, 0.0] }
}

/// 원샷 전체 테셀레이션 — 커밋된 아이템(GpuResourceRegistry)이나 도형
/// 프리뷰(as_stroke)처럼 전체 점 목록에 랜덤 액세스 가능한 경우용.
/// 라이브 드로잉과 달리 지연 버퍼링이 필요 없음(전체를 이미 알고 있음).
pub fn tessellate_stroke(stroke: &Stroke) -> StrokeMesh {
    let n = stroke.points.len();
    if n == 0 {
        return StrokeMesh { origin: [0.0, 0.0], vertices: Vec::new(), indices: Vec::new() };
    }

    let mut mesh = IncrementalStrokeMesh::new(stroke.points[0].pos);
    for i in 0..n {
        let p = &stroke.points[i];
        let half_width = stroke.base_width * p.pressure.max(0.05) * 0.5;
        let prev = if i > 0 { Some(stroke.points[i - 1].pos) } else { None };
        let next = if i + 1 < n { Some(stroke.points[i + 1].pos) } else { None };
        let tangent = estimate_tangent(prev, p.pos, next);
        mesh.push_point(p.pos, half_width, tangent);
    }

    StrokeMesh { origin: mesh.origin, vertices: mesh.vertices, indices: mesh.indices }
}