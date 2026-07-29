// ============================================================
// src/app/render/live_stroke.rs
// ============================================================
// 그리는 중인 자유획(App::drawing_mesh_cache) 전용 GPU 버퍼. 커밋된
// 아이템용 GpuResourceRegistry(ItemId 키, 아이템 하나=버퍼 하나)와는
// 완전히 별개 — 이쪽은 "현재 그리는 중인 단 하나의 스트로크"만을 위한
// 재사용 가능한 growable 버퍼 하나. 스트로크가 끝나도 버퍼 자체는
// 버리지 않고 App에 계속 들고 있다가 다음 획에서 재사용(캐패시티만
// 있으면 재할당 없이 그대로 씀).

use crate::gpu::core::GpuCore;
use crate::render::capsule_pipeline::StrokeVertex;
use crate::render::tessellate::IncrementalStrokeMesh;

/// [미검증 가설] 초기 캐패시티(원소 개수) — 짧은 획은 재할당 없이 바로
/// 커버, 넘어가면 더블링으로 늘어남. 실사용 후 조정 대상.
const INITIAL_VERTEX_CAPACITY: u64 = 512;
const INITIAL_INDEX_CAPACITY: u64 = 1536;

pub(in crate::app) struct LiveStrokeGpu {
    vertex_buf: wgpu::Buffer,
    index_buf: wgpu::Buffer,
    vertex_capacity: u64, // 원소 개수 기준(바이트 아님)
    index_capacity: u64,
    synced_vertices: usize, // GPU에 이미 올라간 만큼
    synced_indices: usize,
}

impl LiveStrokeGpu {
    pub(in crate::app) fn new(core: &GpuCore) -> Self {
        let vertex_buf = core.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("live_stroke_vertex_buf"),
            size: INITIAL_VERTEX_CAPACITY * std::mem::size_of::<StrokeVertex>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let index_buf = core.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("live_stroke_index_buf"),
            size: INITIAL_INDEX_CAPACITY * std::mem::size_of::<u32>() as u64,
            usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        Self {
            vertex_buf,
            index_buf,
            vertex_capacity: INITIAL_VERTEX_CAPACITY,
            index_capacity: INITIAL_INDEX_CAPACITY,
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

    /// mesh 중 아직 GPU에 안 올라간 신규분만 write_buffer로 밀어넣음.
    /// 캐패시티 초과 시에만 재할당(더블링) — 재할당 시엔 반드시
    /// capacity 크기 그대로 빈 버퍼를 만들고 write_buffer로 채움(장부와
    /// 실물 버퍼 크기가 항상 일치하도록 — 예전에 create_buffer_init이
    /// contents 길이만큼만 버퍼를 잡아서 이 둘이 어긋나던 크래시 있었음).
    pub(in crate::app) fn sync(&mut self, core: &GpuCore, mesh: &IncrementalStrokeMesh) {
        let needed_vertices = mesh.vertices.len() as u64;
        let needed_indices = mesh.indices.len() as u64;

        if needed_vertices > self.vertex_capacity || needed_indices > self.index_capacity {
            self.vertex_capacity = self.vertex_capacity.max(needed_vertices).next_power_of_two();
            self.index_capacity = self.index_capacity.max(needed_indices).next_power_of_two();

            self.vertex_buf = core.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("live_stroke_vertex_buf"),
                size: self.vertex_capacity * std::mem::size_of::<StrokeVertex>() as u64,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.index_buf = core.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("live_stroke_index_buf"),
                size: self.index_capacity * std::mem::size_of::<u32>() as u64,
                usage: wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });

            core.queue.write_buffer(&self.vertex_buf, 0, bytemuck::cast_slice(&mesh.vertices));
            core.queue.write_buffer(&self.index_buf, 0, bytemuck::cast_slice(&mesh.indices));

            self.synced_vertices = mesh.vertices.len();
            self.synced_indices = mesh.indices.len();
            return;
        }

        if mesh.vertices.len() > self.synced_vertices {
            let offset = (self.synced_vertices * std::mem::size_of::<StrokeVertex>()) as u64;
            core.queue.write_buffer(&self.vertex_buf, offset, bytemuck::cast_slice(&mesh.vertices[self.synced_vertices..]));
            self.synced_vertices = mesh.vertices.len();
        }
        if mesh.indices.len() > self.synced_indices {
            let offset = (self.synced_indices * std::mem::size_of::<u32>()) as u64;
            core.queue.write_buffer(&self.index_buf, offset, bytemuck::cast_slice(&mesh.indices[self.synced_indices..]));
            self.synced_indices = mesh.indices.len();
        }
    }

    pub(in crate::app) fn vertex_slice(&self) -> wgpu::BufferSlice {
        self.vertex_buf.slice(..)
    }

    pub(in crate::app) fn index_slice(&self) -> wgpu::BufferSlice {
        self.index_buf.slice(..)
    }
}