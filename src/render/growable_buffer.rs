// ============================================================
// src/render/growable_buffer.rs
// ============================================================
// [빌드 미검증 — wgpu 네이티브 의존] Growable GPU 버퍼 공용 헬퍼.
// 예전엔 이 패턴(캐패시티 넘을 때만 재할당, 아니면 write_buffer로
// 내용만 갱신)이 LiveStrokeGpu(그리는 중인 스트로크 전용)에만 있었음.
// 커밋된 Stroke/Shape을 옮길 때마다 create_buffer_init으로 완전
// 새 버퍼를 만들던 문제(A) 때문에, 이 패턴을 일반화해서 커밋 아이템도
// 똑같이 쓰도록 뽑아냄.
//
// write_full/write_suffix 둘 다 지원 — 커밋 아이템(Stroke/Shape)은
// 지오메트리가 바뀔 때마다 "통째로 확정된 새 모양"이라 write_full만
// 씀. LiveStrokeGpu(그리는 중)는 점이 하나씩 늘어나는 특성상
// write_suffix(이미 올라간 만큼은 건드리지 않고 새로 늘어난 만큼만)를
// 씀 — 이 차이만 남기고 나머지(캐패시티 관리/재할당)는 공유.

use crate::gpu::core::GpuCore;
use bytemuck::Pod;

pub struct GrowableBuffer {
    buf: wgpu::Buffer,
    capacity_elements: u64,
    elem_size: u64,
    usage: wgpu::BufferUsages,
    label: &'static str,
}

impl GrowableBuffer {
    pub fn new(
        core: &GpuCore,
        initial_capacity_elements: u64,
        elem_size: u64,
        usage: wgpu::BufferUsages,
        label: &'static str,
    ) -> Self {
        let capacity_elements = initial_capacity_elements.max(1);
        let buf = core.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(label),
            size: capacity_elements * elem_size,
            usage,
            mapped_at_creation: false,
        });
        Self { buf, capacity_elements, elem_size, usage, label }
    }

    pub fn buffer(&self) -> &wgpu::Buffer {
        &self.buf
    }

    fn realloc(&mut self, core: &GpuCore, needed_elements: u64) {
        self.capacity_elements = needed_elements.max(1).next_power_of_two();
        self.buf = core.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some(self.label),
            size: self.capacity_elements * self.elem_size,
            usage: self.usage,
            mapped_at_creation: false,
        });
    }

    /// 버퍼 전체를 `data`로 덮어씀(오프셋 0). 캐패시티 초과 시에만
    /// 재할당(더블링) — 커밋 아이템(지오메트리 재확정)용.
    pub fn write_full<T: Pod>(&mut self, core: &GpuCore, data: &[T]) {
        let needed = data.len() as u64;
        if needed > self.capacity_elements {
            self.realloc(core, needed);
        }
        if !data.is_empty() {
            core.queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(data));
        }
    }

    /// `data[already_synced..]`만 이어붙여 업로드 — 라이브 드로잉처럼
    /// "이전에 올린 부분은 안 건드리고 새로 늘어난 부분만" 갱신하고
    /// 싶을 때. 캐패시티 초과 시엔 write_full과 동일하게 재할당 후
    /// 전체를 다시 올림(부분 이어붙이기가 의미 없어지므로).
    /// 반환값 = 이번 호출 후 "GPU에 올라간 원소 개수"(다음 호출의
    /// already_synced로 그대로 넘기면 됨).
    pub fn write_suffix<T: Pod>(&mut self, core: &GpuCore, data: &[T], already_synced: usize) -> usize {
        let needed = data.len() as u64;
        if needed > self.capacity_elements {
            self.realloc(core, needed);
            if !data.is_empty() {
                core.queue.write_buffer(&self.buf, 0, bytemuck::cast_slice(data));
            }
            return data.len();
        }
        if data.len() > already_synced {
            let offset = already_synced as u64 * self.elem_size;
            core.queue.write_buffer(&self.buf, offset, bytemuck::cast_slice(&data[already_synced..]));
        }
        data.len()
    }
}

/// vertex+index 버퍼 쌍 — 커밋 아이템(Stroke/Shape)의 GPU 메시.
/// 지오메트리가 바뀔 때(geometry_dirty)만 upload()가 불리고, 순수
/// 이동일 땐 이 구조체 자체가 전혀 안 건드려짐(origin은 별도 관리).
pub struct GrowableMesh {
    pub vertex: GrowableBuffer,
    pub index: GrowableBuffer,
    pub index_count: u32,
}

impl GrowableMesh {
    pub fn new(core: &GpuCore, initial_vertices: u64, initial_indices: u64, label: &'static str) -> Self {
        Self {
            vertex: GrowableBuffer::new(
                core, initial_vertices,
                std::mem::size_of::<crate::render::pipeline::Vertex>() as u64,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                label,
            ),
            index: GrowableBuffer::new(
                core, initial_indices,
                std::mem::size_of::<u32>() as u64,
                wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST,
                label,
            ),
            index_count: 0,
        }
    }

    pub fn upload(&mut self, core: &GpuCore, vertices: &[crate::render::pipeline::Vertex], indices: &[u32]) {
        self.vertex.write_full(core, vertices);
        self.index.write_full(core, indices);
        self.index_count = indices.len() as u32;
    }
}