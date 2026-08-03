// ============================================================
// src/render/tessellate.rs
// ============================================================
// Stroke(포인트+압력 리스트) -> 리본형 렌더 메시 변환. 점마다 좌/우
// 오프셋 정점 쌍 하나씩만 만들고 인접 쌍끼리 삼각형 스트립으로 이음.
//
// 단, 열린 경로(진짜 손글씨 스트로크, Line 도형)의 양 끝 2곳만은
// 예외 — 반원 팬(라운드 캡)을 추가해서 뭉툭한 끝 마감을 둥글게 처리.
// Shape outline(사각형/원/삼각형)처럼 첫점=끝점인 "닫힌" 경로에는
// 캡을 붙이면 안 됨 — tessellate_stroke가 첫점/끝점 비교로 자동 구분.

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
        Self {
            origin,
            vertices: Vec::new(),
            indices: Vec::new(),
            has_prev_pair: false,
        }
    }

    /// 점 하나(월드좌표+half_width+단위접선벡터)를 리본에 추가.
    pub fn push_point(&mut self, world_pos: [f64; 2], half_width: f32, tangent: [f32; 2]) {
        let local = [
            (world_pos[0] - self.origin[0]) as f32,
            (world_pos[1] - self.origin[1]) as f32,
        ];
        let normal = [
            -tangent[1],
            tangent[0],
        ];
        let left = [
            local[0] + normal[0] * half_width,
            local[1] + normal[1] * half_width,
        ];
        let right = [
            local[0] - normal[0] * half_width,
            local[1] - normal[1] * half_width,
        ];

        let base = self.vertices.len() as u32;
        self.vertices.push(Vertex {
            pos: left,
        });
        self.vertices.push(Vertex {
            pos: right,
        });

        if self.has_prev_pair {
            self.indices.extend_from_slice(&[
                base - 2,
                base - 1,
                base,
                base - 1,
                base + 1,
                base,
            ]);
        }
        self.has_prev_pair = true;
    }

    /// 열린 경로의 시작/끝에 반원 팬(라운드 캡) 추가.
    pub fn push_round_cap(
        &mut self,
        world_pos: [f64; 2],
        tangent: [f32; 2],
        radius: f32,
        forward: bool,
    ) {
        const SEGMENTS: usize = 12;
        let local = [
            (world_pos[0] - self.origin[0]) as f32,
            (world_pos[1] - self.origin[1]) as f32,
        ];
        let normal = [
            -tangent[1],
            tangent[0],
        ];
        let sign = if forward { 1.0 } else { -1.0 };

        let center_idx = self.vertices.len() as u32;
        self.vertices.push(Vertex {
            pos: local,
        });

        let arc_start = self.vertices.len() as u32;
        for i in 0..=SEGMENTS {
            let t = std::f32::consts::PI * (i as f32) / (SEGMENTS as f32);
            let (sin_t, cos_t) = t.sin_cos();
            let dir = [
                cos_t * normal[0] + sign * sin_t * tangent[0],
                cos_t * normal[1] + sign * sin_t * tangent[1],
            ];
            self.vertices.push(Vertex {
                pos: [
                    local[0] + dir[0] * radius,
                    local[1] + dir[1] * radius,
                ],
            });
        }

        for i in 0..SEGMENTS {
            self.indices.extend_from_slice(&[
                center_idx,
                arc_start + i as u32,
                arc_start + i as u32 + 1,
            ]);
        }
    }
}

/// 점 i의 접선을 중심차분으로 추정.
pub fn estimate_tangent(prev: Option<[f64; 2]>, cur: [f64; 2], next: Option<[f64; 2]>) -> [f32; 2] {
    let a = prev.unwrap_or(cur);
    let b = next.unwrap_or(cur);
    let dir = [
        (b[0] - a[0]) as f32,
        (b[1] - a[1]) as f32,
    ];
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if len > f32::EPSILON {
        [
            dir[0] / len,
            dir[1] / len,
        ]
    } else {
        [
            1.0, 0.0,
        ]
    }
}

/// 원샷 전체 테셀레이션.
///
/// [설계 변경] stroke.points는 이제 stroke.anchor 기준 로컬 좌표라,
/// 테셀레이션 origin(=GPU 렌더용 기준점)도 그냥 anchor를 그대로 씀.
/// push_point/push_round_cap은 여전히 "world_pos - origin" 공식을
/// 내부에서 쓰므로, 여기선 local point에 anchor를 다시 더해 world_pos를
/// 재구성해서 넘겨줌(origin=anchor라 결과적으로 로컬로 되돌아옴 —
/// 왕복이지만 API를 그대로 재사용할 수 있어 최소 변경). 접선(tangent)
/// 계산은 차분(diff)이라 anchor 유무와 무관 — 로컬 좌표를 그대로 넘김.
pub fn tessellate_stroke(stroke: &Stroke) -> StrokeMesh {
    let n = stroke.points.len();
    if n == 0 {
        return StrokeMesh {
            origin: stroke.anchor,
            vertices: Vec::new(),
            indices: Vec::new(),
        };
    }

    let mut mesh = IncrementalStrokeMesh::new(stroke.anchor);
    let is_open = n >= 2 && stroke.points[0].pos != stroke.points[n - 1].pos;
    let to_world = |local: [f64; 2]| {
        [
            local[0] + stroke.anchor[0],
            local[1] + stroke.anchor[1],
        ]
    };

    for i in 0..n {
        let p = &stroke.points[i];
        let half_width = stroke.base_width * p.pressure.max(0.05) * 0.5;
        let prev_local = if i > 0 {
            Some(stroke.points[i - 1].pos)
        } else {
            None
        };
        let next_local = if i + 1 < n {
            Some(stroke.points[i + 1].pos)
        } else {
            None
        };
        let tangent = estimate_tangent(prev_local, p.pos, next_local);
        let cur_world = to_world(p.pos);

        if is_open && i == 0 {
            mesh.push_round_cap(cur_world, tangent, half_width, false);
        }
        mesh.push_point(cur_world, half_width, tangent);
        if is_open && i == n - 1 {
            mesh.push_round_cap(cur_world, tangent, half_width, true);
        }
    }

    StrokeMesh {
        origin: mesh.origin,
        vertices: mesh.vertices,
        indices: mesh.indices,
    }
}

/// Stroke의 로컬(anchor 기준) padded bbox — 뷰포트 컬링 캐시용.
/// 지오메트리가 바뀔 때(geometry_dirty)만 호출되고, 위치만 바뀔 땐
/// 이 값이 그대로 재사용됨(anchor만 더해서 world bbox를 얻음).
pub fn local_padded_bbox(stroke: &Stroke) -> ([f64; 2], [f64; 2]) {
    let mut min = [
        f64::MAX,
        f64::MAX,
    ];
    let mut max = [
        f64::MIN,
        f64::MIN,
    ];
    for p in &stroke.points {
        min[0] = min[0].min(p.pos[0]);
        min[1] = min[1].min(p.pos[1]);
        max[0] = max[0].max(p.pos[0]);
        max[1] = max[1].max(p.pos[1]);
    }
    let pad = stroke.base_width as f64 * 0.5;
    (
        [
            min[0] - pad,
            min[1] - pad,
        ],
        [
            max[0] + pad,
            max[1] + pad,
        ],
    )
}
