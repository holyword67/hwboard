// ============================================================
// src/render/gpu_resources.rs
// ============================================================
// ItemId -> GPU 버퍼 캐시. Scene은 wgpu를 전혀 모르고, 이 레지스트리가
// 매 프레임 Scene을 읽으면서 자기 캐시를 스스로 동기화한다 (B안).

use crate::gpu::core::GpuCore;
use crate::render::pipeline::Vertex;
use crate::render::tessellate::tessellate_stroke;
use crate::scene::{CanvasItem, ItemId, Scene};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

pub struct StrokeGpuResource {
    pub vertex_buf: wgpu::Buffer,
    pub index_buf: wgpu::Buffer,
    pub index_count: u32,
    pub origin: [f64; 2], // world 좌표 — draw 시점에 카메라 오프셋 계산용
}

pub struct GpuResourceRegistry {
    strokes: HashMap<ItemId, StrokeGpuResource>,
}

impl GpuResourceRegistry {
    pub fn new() -> Self {
        Self { strokes: HashMap::new() }
    }

    /// 매 프레임 호출. dirty하거나 캐시에 없는 스트로크는 재생성하고,
    /// scene에서 사라진 아이템의 캐시는 정리(메모리 누수 방지).
    pub fn sync(&mut self, core: &GpuCore, scene: &mut Scene) {
        let mut seen: HashSet<ItemId> = HashSet::new();
        let mut needs_rebuild: Vec<ItemId> = Vec::new();

        for (id, item) in scene.iter_ordered_with_id() {
            seen.insert(id);
            if let CanvasItem::Stroke(s) = item {
                if s.mesh_dirty || !self.strokes.contains_key(&id) {
                    needs_rebuild.push(id);
                }
            }
        }

        for id in needs_rebuild {
            let Some(CanvasItem::Stroke(s)) = scene.item(id) else { continue };
            let mesh = tessellate_stroke(s);
            let vertex_data: Vec<Vertex> = mesh.vertices.iter().map(|&pos| Vertex { pos }).collect();

            let vertex_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stroke_vertex_buf"),
                contents: bytemuck::cast_slice(&vertex_data),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stroke_index_buf"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.strokes.insert(
                id,
                StrokeGpuResource {
                    vertex_buf,
                    index_buf,
                    index_count: mesh.indices.len() as u32,
                    origin: mesh.origin,
                },
            );
            scene.mark_stroke_clean(id);
        }

        self.strokes.retain(|id, _| seen.contains(id));
    }

    pub fn get_stroke(&self, id: ItemId) -> Option<&StrokeGpuResource> {
        self.strokes.get(&id)
    }
}