// ============================================================
// src/render/gpu_resources.rs
// ============================================================
use crate::gpu::core::GpuCore;
use crate::render::image_pipeline::{ImagePipeline, ImageVertex};
use crate::render::pipeline::Vertex;
use crate::render::tessellate::tessellate_stroke;
use crate::scene::{CanvasItem, ItemId, Scene};
use std::collections::{HashMap, HashSet};
use wgpu::util::DeviceExt;

pub struct StrokeGpuResource {
    pub vertex_buf: wgpu::Buffer,
    pub index_buf: wgpu::Buffer,
    pub index_count: u32,
    pub origin: [f64; 2],
}

pub struct ImageGpuResource {
    pub bind_group: wgpu::BindGroup,
    pub vertex_buf: wgpu::Buffer,
    pub index_buf: wgpu::Buffer,
    pub origin: [f64; 2],
}

pub struct GpuResourceRegistry {
    strokes: HashMap<ItemId, StrokeGpuResource>,
    images: HashMap<ItemId, ImageGpuResource>,
}

impl GpuResourceRegistry {
    pub fn new() -> Self {
        Self { strokes: HashMap::new(), images: HashMap::new() }
    }

    pub fn sync(&mut self, core: &GpuCore, image_pipeline: &ImagePipeline, scene: &mut Scene) {
        let mut seen_strokes: HashSet<ItemId> = HashSet::new();
        let mut seen_images: HashSet<ItemId> = HashSet::new();
        let mut strokes_to_build: Vec<ItemId> = Vec::new();
        let mut images_to_build: Vec<ItemId> = Vec::new();

        for (id, item) in scene.iter_ordered_with_id() {
            match item {
                CanvasItem::Stroke(s) => {
                    seen_strokes.insert(id);
                    if s.mesh_dirty || !self.strokes.contains_key(&id) {
                        strokes_to_build.push(id);
                    }
                }
                CanvasItem::Image(_) => {
                    seen_images.insert(id);
                    // 이미지는 붙여넣은 후 안 바뀌니 dirty 플래그 없이
                    // "캐시에 없으면 한 번만 생성"으로 충분.
                    if !self.images.contains_key(&id) {
                        images_to_build.push(id);
                    }
                }
                _ => {}
            }
        }

        for id in strokes_to_build {
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

        for id in images_to_build {
            let Some(CanvasItem::Image(img)) = scene.item(id) else { continue };

            let texture = core.device.create_texture(&wgpu::TextureDescriptor {
                label: Some("pasted_image_texture"),
                size: wgpu::Extent3d {
                    width: img.pixel_width,
                    height: img.pixel_height,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });

            // ⚠️ [미검증] wgpu 버전마다 이 세 타입 이름이 계속 바뀌어온
            // 이력이 있음(ImageCopyTexture/TexelCopyTextureInfo 등) —
            // 컴파일해서 실제 이름 확인 필요할 수 있음.
            core.queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                &img.rgba,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * img.pixel_width),
                    rows_per_image: Some(img.pixel_height),
                },
                wgpu::Extent3d {
                    width: img.pixel_width,
                    height: img.pixel_height,
                    depth_or_array_layers: 1,
                },
            );

            let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
            let bind_group = core.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("image_bind_group"),
                layout: &image_pipeline.texture_bgl,
                entries: &[
                    wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(&image_pipeline.sampler),
                    },
                ],
            });

            let (w, h) = (img.size[0] as f32, img.size[1] as f32);
            let verts = [
                ImageVertex { pos: [0.0, 0.0], uv: [0.0, 0.0] },
                ImageVertex { pos: [w, 0.0], uv: [1.0, 0.0] },
                ImageVertex { pos: [0.0, h], uv: [0.0, 1.0] },
                ImageVertex { pos: [w, h], uv: [1.0, 1.0] },
            ];
            let indices: [u32; 6] = [0, 1, 2, 1, 3, 2];

            let vertex_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("image_vertex_buf"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("image_index_buf"),
                contents: bytemuck::cast_slice(&indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.images.insert(id, ImageGpuResource { bind_group, vertex_buf, index_buf, origin: img.top_left });
        }

        self.strokes.retain(|id, _| seen_strokes.contains(id));
        self.images.retain(|id, _| seen_images.contains(id));
    }

    pub fn get_stroke(&self, id: ItemId) -> Option<&StrokeGpuResource> {
        self.strokes.get(&id)
    }

    pub fn get_image(&self, id: ItemId) -> Option<&ImageGpuResource> {
        self.images.get(&id)
    }
}