// ============================================================
// src/render/gpu_resources.rs
// ============================================================
use crate::gpu::core::GpuCore;
use crate::render::image_pipeline::{ImagePipeline, ImageVertex};
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

/// bbox에 이 아이템 자체의 선 굵기 절반을 패딩으로 포함시킴 — 뷰포트
/// 컬링 마진을 배율(예: 1.2x)이 아니라 "아이템별 실제 삐져나오는
/// 만큼"으로 정확히 잡기 위함. 이미지는 굵기 개념이 없어 패딩 0.
fn padded_bbox(item: &CanvasItem) -> ([f64; 2], [f64; 2]) {
    let (min, max) = item.bounding_box();
    let pad = match item {
        CanvasItem::Stroke(s) => s.base_width as f64 * 0.5,
        CanvasItem::Shape(sh) => sh.stroke_width as f64 * 0.5,
        CanvasItem::Image(_) => 0.0,
    };
    ([min[0] - pad, min[1] - pad], [max[0] + pad, max[1] + pad])
}

pub struct GpuResourceRegistry {
    strokes: HashMap<ItemId, StrokeGpuResource>,
    shapes: HashMap<ItemId, StrokeGpuResource>,
    images: HashMap<ItemId, ImageGpuResource>,
    /// 뷰포트 컬링용 world bbox 캐시. GPU 메시가 다시 구워지는 시점(=
    /// 지오메트리가 바뀐 시점)과 정확히 같은 타이밍에만 갱신됨 — 별도
    /// dirty 플래그 없이 기존 mesh_dirty 판정에 편승.
    bboxes: HashMap<ItemId, ([f64; 2], [f64; 2])>,
}

impl GpuResourceRegistry {
    pub fn new() -> Self {
        Self { strokes: HashMap::new(), shapes: HashMap::new(), images: HashMap::new(), bboxes: HashMap::new() }
    }

    pub fn sync(&mut self, core: &GpuCore, image_pipeline: &ImagePipeline, scene: &mut Scene) {
        let mut seen_strokes: HashSet<ItemId> = HashSet::new();
        let mut seen_shapes: HashSet<ItemId> = HashSet::new();
        let mut seen_images: HashSet<ItemId> = HashSet::new();
        let mut strokes_to_build: Vec<ItemId> = Vec::new();
        let mut shapes_to_build: Vec<ItemId> = Vec::new();
        let mut images_to_create: Vec<ItemId> = Vec::new();
        let mut images_to_update: Vec<ItemId> = Vec::new(); // 텍스처 재활용, 크기/위치만 갱신

        for (id, item) in scene.iter_ordered_with_id() {
            match item {
                CanvasItem::Stroke(s) => {
                    seen_strokes.insert(id);
                    if s.mesh_dirty || !self.strokes.contains_key(&id) {
                        strokes_to_build.push(id);
                    }
                }
                CanvasItem::Shape(sh) => {
                    seen_shapes.insert(id);
                    if sh.mesh_dirty || !self.shapes.contains_key(&id) {
                        shapes_to_build.push(id);
                    }
                }
                CanvasItem::Image(img) => {
                    seen_images.insert(id);
                    if !self.images.contains_key(&id) {
                        images_to_create.push(id);
                    } else if img.mesh_dirty {
                        images_to_update.push(id);
                    }
                }
                _ => {}
            }
        }

        for id in strokes_to_build {
            let Some(item_ref) = scene.item(id) else { continue };
            let CanvasItem::Stroke(s) = item_ref else { continue };
            let mesh = tessellate_stroke(s);

            let vertex_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stroke_vertex_buf"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("stroke_index_buf"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.bboxes.insert(id, padded_bbox(item_ref));
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

        for id in shapes_to_build {
            let Some(item_ref) = scene.item(id) else { continue };
            let CanvasItem::Shape(sh) = item_ref else { continue };
            let virtual_stroke = sh.as_stroke();
            let mesh = tessellate_stroke(&virtual_stroke);

            let vertex_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shape_vertex_buf"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let index_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("shape_index_buf"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });

            self.bboxes.insert(id, padded_bbox(item_ref));
            self.shapes.insert(
                id,
                StrokeGpuResource {
                    vertex_buf,
                    index_buf,
                    index_count: mesh.indices.len() as u32,
                    origin: mesh.origin,
                },
            );
            scene.mark_shape_clean(id);
        }

        // 이미지 새로 붙여넣어졌을 때 (텍스처 포함 전부 생성)
        for id in images_to_create {
            let Some(item_ref) = scene.item(id) else { continue };
            let CanvasItem::Image(img) = item_ref else { continue };

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

            self.bboxes.insert(id, padded_bbox(item_ref));
            self.images.insert(id, ImageGpuResource { bind_group, vertex_buf, index_buf, origin: img.top_left });
            scene.mark_image_clean(id);
        }

        // 이미지 크기/위치만 바뀌었을 때 (텍스처는 놔두고 Vertex 사이즈만 갱신)
        for id in images_to_update {
            let Some(item_ref) = scene.item(id) else { continue };
            let CanvasItem::Image(img) = item_ref else { continue };

            let (w, h) = (img.size[0] as f32, img.size[1] as f32);
            let verts = [
                ImageVertex { pos: [0.0, 0.0], uv: [0.0, 0.0] },
                ImageVertex { pos: [w, 0.0], uv: [1.0, 0.0] },
                ImageVertex { pos: [0.0, h], uv: [0.0, 1.0] },
                ImageVertex { pos: [w, h], uv: [1.0, 1.0] },
            ];

            let vertex_buf = core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("image_vertex_buf_update"),
                contents: bytemuck::cast_slice(&verts),
                usage: wgpu::BufferUsages::VERTEX,
            });

            self.bboxes.insert(id, padded_bbox(item_ref));
            if let Some(res) = self.images.get_mut(&id) {
                res.vertex_buf = vertex_buf;
                res.origin = img.top_left;
            }
            scene.mark_image_clean(id);
        }

        self.strokes.retain(|id, _| seen_strokes.contains(id));
        self.shapes.retain(|id, _| seen_shapes.contains(id));
        self.images.retain(|id, _| seen_images.contains(id));
        self.bboxes.retain(|id, _| seen_strokes.contains(id) || seen_shapes.contains(id) || seen_images.contains(id));
    }

    pub fn get_stroke(&self, id: ItemId) -> Option<&StrokeGpuResource> {
        self.strokes.get(&id)
    }

    pub fn get_shape(&self, id: ItemId) -> Option<&StrokeGpuResource> {
        self.shapes.get(&id)
    }

    pub fn get_image(&self, id: ItemId) -> Option<&ImageGpuResource> {
        self.images.get(&id)
    }

    /// 뷰포트 컬링 판정. 캐시가 아직 없는 경우(sync 이전 신규 아이템
    /// 등 예외적 타이밍)는 안전하게 "보인다"로 취급 — 잘못 스킵해서
    /// 안 그려지는 것보다 한 프레임 더 그리는 쪽이 훨씬 안전.
    pub fn is_visible(&self, id: ItemId, view_min: [f64; 2], view_max: [f64; 2]) -> bool {
        match self.bboxes.get(&id) {
            Some((min, max)) => {
                !(max[0] < view_min[0] || min[0] > view_max[0] || max[1] < view_min[1] || min[1] > view_max[1])
            }
            None => true,
        }
    }
}