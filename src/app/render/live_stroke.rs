// ============================================================
// src/app/render/live_stroke.rs
// ============================================================
// [빌드 미검증 — wgpu 네이티브 의존]
// 그리는 중인 자유획(App::drawing_mesh_cache) 전용 GPU 버퍼. 커밋된
// 아이템용 GpuResourceRegistry와는 완전히 별개 — 이쪽은 "현재 그리는
// 중인 단 하나의 스트로크"만을 위한 재사용 가능한 growable 버퍼 하나.
//
// [설계 변경] 캐패시티 관리/재할당 로직이 growable_buffer::GrowableBuffer
// 로 뽑혀나가서, 여기는 "이미 올라간 만큼(synced_*)" 카운터를 들고
// write_suffix를 호출하는 얇은 래퍼로 단순화됨. gpu_resources.rs의
// StrokeGpuResource/ShapeGpuResource도 같은 GrowableBuffer를 씀 —
// 커밋 아이템은 write_full(통째로 확정), 라이브 드로잉은
// write_suffix(점진적 추가)라는 차이만 남고 캐패시티/재할당 로직은
// 완전히 공유됨.

use crate::gpu::core::GpuCore;
use crate::render::growable_buffer::GrowableBuffer;
use crate::render::pipeline::Vertex;
use crate::render::tessellate::IncrementalStrokeMesh;

const INITIAL_VERTEX_CAPACITY: u64 = 512;
const INITIAL_INDEX_CAPACITY: u64 = 1536;

pub(in crate::app) struct LiveStrokeGpu {
    vertex: GrowableBuffer,
    index: GrowableBuffer,
    synced_vertices: usize,
    synced_indices: usize,
}

impl LiveStrokeGpu {
    pub(in crate::app) fn new(core: &GpuCore) -> Self {
        Self {
            vertex: GrowableBuffer::new(
                core, INITIAL_VERTEX_CAPACITY,
                std::mem::size_of::<Vertex>() as u64,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                "live_stroke_vertex_buf",
            ),
            index: GrowableBuffer::new(
                core, INITIAL_INDEX_CAPACITY,
                std::mem::size_of::<u32>() as u64,
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                "live_stroke_index_buf",
            ),
            synced_vertices: 0,
            synced_indices: 0,
        }
    }

    /// 다음 획을 위해 "GPU에 이미 올라간 만큼" 카운터만 리셋 — 버퍼
    /// 자체(캐패시티)는 재사용, 재할당 안 함.
    pub(in crate::app) fn reset(&mut self) {
        self.synced_vertices = 0;
        self.synced_indices = 0;
    }

    /// mesh 중 아직 GPU에 안 올라간 신규분만 write_suffix로 밀어넣음.
    pub(in crate::app) fn sync(&mut self, core: &GpuCore, mesh: &IncrementalStrokeMesh) {
        self.synced_vertices = self.vertex.write_suffix(core, &mesh.vertices, self.synced_vertices);
        self.synced_indices = self.index.write_suffix(core, &mesh.indices, self.synced_indices);
    }

    pub(in crate::app) fn vertex_slice(&self) -> wgpu::BufferSlice<'_> {
        self.vertex.buffer().slice(..)
    }

    pub(in crate::app) fn index_slice(&self) -> wgpu::BufferSlice<'_> {
        self.index.buffer().slice(..)
    }
}