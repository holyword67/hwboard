// ============================================================
// src/app/render/overlay.rs
// ============================================================
// [빌드 미검증 — wgpu 네이티브 의존]
// 지우개 인디케이터/선택 오버레이/커스텀 커서 — 매 프레임 위치가
// 바뀌어서 캐싱은 불가능하지만("즉석 생성" 자체는 맞는 설계), 예전엔
// "즉석 생성"이 선분/사각형 하나당 device.create_buffer_init() 개별
// 호출로 구현돼 있었음(지우개 인디케이터만 매 프레임 16번). 이번
// 라운드에서 growable 버퍼 하나로 통합.

use crate::gpu::core::GpuCore;
use crate::render::growable_buffer::GrowableBuffer;
use crate::render::pipeline::Vertex;
use crate::ui;

const INITIAL_OVERLAY_VERTEX_CAPACITY: u64 = 256;

pub(in crate::app) struct OverlayEntry {
    pub(in crate::app) offset: u32,
    pub(in crate::app) count: u32,
    pub(in crate::app) color: [f32; 4],
}

pub(in crate::app) struct OverlayGpu {
    vertex: GrowableBuffer,
}

impl OverlayGpu {
    pub(in crate::app) fn new(core: &GpuCore) -> Self {
        Self {
            vertex: GrowableBuffer::new(
                core,
                INITIAL_OVERLAY_VERTEX_CAPACITY,
                std::mem::size_of::<Vertex>() as u64,
                wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                "overlay_vertex_buf",
            ),
        }
    }

    /// 이번 프레임 오버레이 정점 전체를 업로드(캐패시티 넘을 때만 재할당,
    /// 그 외엔 write_buffer 한 번).
    pub(in crate::app) fn upload(&mut self, core: &GpuCore, vertices: &[Vertex]) {
        self.vertex.write_full(core, vertices);
    }

    pub(in crate::app) fn buffer(&self) -> &wgpu::Buffer {
        self.vertex.buffer()
    }
}

/// 프레임 하나 안에서 오버레이 정점을 CPU에 쌓는 빌더.
#[derive(Default)]
pub(in crate::app) struct OverlayBuilder {
    vertices: Vec<Vertex>,
    pub(in crate::app) entries: Vec<OverlayEntry>,
}

impl OverlayBuilder {
    pub(in crate::app) fn vertices(&self) -> &[Vertex] {
        &self.vertices
    }

    pub(in crate::app) fn push_quad(&mut self, rect: ui::Rect, color: [f32; 4]) {
        let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
        let offset = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&[
            Vertex { pos: [x0, y0] },
            Vertex { pos: [x1, y0] },
            Vertex { pos: [x0, y1] },
            Vertex { pos: [x1, y0] },
            Vertex { pos: [x1, y1] },
            Vertex { pos: [x0, y1] },
        ]);
        self.entries.push(OverlayEntry { offset, count: 6, color });
    }

    pub(in crate::app) fn push_line_segment(&mut self, p0: [f32; 2], p1: [f32; 2], width: f32, color: [f32; 4]) {
        let dir = [p1[0] - p0[0], p1[1] - p0[1]];
        let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
        if len < f32::EPSILON {
            return;
        }
        let normal = [-dir[1] / len, dir[0] / len];
        let hw = width * 0.5;
        let offset = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&[
            Vertex { pos: [p0[0] + normal[0] * hw, p0[1] + normal[1] * hw] },
            Vertex { pos: [p0[0] - normal[0] * hw, p0[1] - normal[1] * hw] },
            Vertex { pos: [p1[0] + normal[0] * hw, p1[1] + normal[1] * hw] },
            Vertex { pos: [p0[0] - normal[0] * hw, p0[1] - normal[1] * hw] },
            Vertex { pos: [p1[0] - normal[0] * hw, p1[1] - normal[1] * hw] },
            Vertex { pos: [p1[0] + normal[0] * hw, p1[1] + normal[1] * hw] },
        ]);
        self.entries.push(OverlayEntry { offset, count: 6, color });
    }
}