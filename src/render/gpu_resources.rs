// ============================================================
// src/render/gpu_resources.rs
// ============================================================
// [빌드 미검증 — wgpu 네이티브 의존]
//
// [설계 변경, B+A+C 라운드]
// - sync()가 매 프레임 전체 씬을 스캔하던 걸 그만두고, Scene이 넘겨주는
//   touched/inserted/removed만 처리(drain 기반) — 커서만 움직여서
//   dirty가 서도 씬 크기와 무관하게 즉시 리턴됨.
// - "위치만 바뀜"(geometry_dirty == false)인 touched 아이템은 GPU 호출
//   자체가 없음 — res.origin 필드 하나만 갱신.
// - Stroke/Shape GPU 메시는 growable_buffer::GrowableMesh로 통일 —
//   지오메트리가 바뀔 때도 create_buffer_init(완전 재할당) 대신
//   capacity 안에서는 write_buffer만.
// - 뷰포트 컬링용 bbox는 "로컬(anchor/center 기준)"로 캐싱해서 지오메트리가
//   바뀔 때만 재계산 — 위치만 바뀔 땐 캐시된 로컬 bbox + 최신 origin을
//   합쳐서 매번 구함(덧셈 두 번, O(1)).

use crate::gpu::core::GpuCore;
use crate::render::growable_buffer::GrowableMesh;
use crate::render::image_pipeline::{ImagePipeline, ImageVertex};
use crate::render::tessellate::{local_padded_bbox, tessellate_stroke};
use crate::scene::{CanvasItem, ItemId, Scene, Shape};
use std::collections::HashMap;
use wgpu::util::DeviceExt;

const INITIAL_STROKE_VERTEX_CAPACITY: u64 = 512;
const INITIAL_STROKE_INDEX_CAPACITY: u64 = 1536;
// 도형은 대체로 훨씬 작음(사각형/삼각형 5점 내외, 원이 제일 커도 65점).
const INITIAL_SHAPE_VERTEX_CAPACITY: u64 = 128;
const INITIAL_SHAPE_INDEX_CAPACITY: u64 = 384;

pub struct StrokeGpuResource {
    pub mesh: GrowableMesh,
    pub origin: [f64; 2],
}

pub struct ImageGpuResource {
    pub bind_group: wgpu::BindGroup,
    pub vertex_buf: wgpu::Buffer, // 항상 4정점 고정 크기라 growable 불필요
    pub index_buf: wgpu::Buffer,  // [0,1,2,1,3,2] 고정값 — 최초 1회만 생성
    pub origin: [f64; 2],
}

/// Shape의 로컬(center 기준) padded bbox. world_corners()에서 center를
/// 빼서 로컬화 — Shape 자체엔 이 계산을 위한 전용 메서드가 없어도 공개
/// API(world_corners/center/stroke_width)만으로 충분해서 여기서 계산.
fn shape_local_padded_bbox(sh: &Shape) -> ([f64; 2], [f64; 2]) {
    let corners = sh.world_corners();
    let mut min = [
        f64::MAX,
        f64::MAX,
    ];
    let mut max = [
        f64::MIN,
        f64::MIN,
    ];
    for c in corners {
        let lx = c[0] - sh.center[0];
        let ly = c[1] - sh.center[1];
        min[0] = min[0].min(lx);
        min[1] = min[1].min(ly);
        max[0] = max[0].max(lx);
        max[1] = max[1].max(ly);
    }
    let pad = sh.stroke_width as f64 * 0.5;
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

pub struct GpuResourceRegistry {
    strokes: HashMap<ItemId, StrokeGpuResource>,
    shapes: HashMap<ItemId, StrokeGpuResource>,
    images: HashMap<ItemId, ImageGpuResource>,
    /// 로컬(anchor/center/top_left 기준) padded bbox 캐시. 지오메트리가
    /// 바뀔 때(geometry_dirty)만 갱신됨 — 위치만 바뀌는 흔한 케이스에서는
    /// 전혀 안 건드려짐(재계산 비용 자체가 없음).
    local_bboxes: HashMap<ItemId, ([f64; 2], [f64; 2])>,
}

impl GpuResourceRegistry {
    pub fn new() -> Self {
        Self {
            strokes: HashMap::new(),
            shapes: HashMap::new(),
            images: HashMap::new(),
            local_bboxes: HashMap::new(),
        }
    }

    /// [핵심 변경점] 전체 스캔 대신 Scene이 이번 프레임에 실제로 바뀐
    /// id만 drain해서 처리. removed → inserted → touched 순서(순서
    /// 자체는 크게 안 중요 — 각 단계가 scene.item(id)로 현재 상태를
    /// 다시 확인하므로 idempotent).
    pub fn sync(&mut self, core: &GpuCore, image_pipeline: &ImagePipeline, scene: &mut Scene) {
        for id in scene.take_removed() {
            self.strokes.remove(&id);
            self.shapes.remove(&id);
            self.images.remove(&id);
            self.local_bboxes.remove(&id);
        }

        for id in scene.take_inserted() {
            self.sync_item(core, image_pipeline, scene, id);
        }

        for id in scene.take_touched() {
            self.sync_item(core, image_pipeline, scene, id);
        }
    }

    fn sync_item(
        &mut self,
        core: &GpuCore,
        image_pipeline: &ImagePipeline,
        scene: &mut Scene,
        id: ItemId,
    ) {
        // 어떤 타입인지만 먼저 확인(빌려온 참조는 이 블록 안에서만 삶).
        let kind = match scene.item(id) {
            Some(CanvasItem::Stroke(_)) => 0,
            Some(CanvasItem::Shape(_)) => 1,
            Some(CanvasItem::Image(_)) => 2,
            None => return, // 이미 삭제된 뒤(같은 프레임에 touched+removed 겹친 경우) — 조용히 스킵
        };
        match kind {
            0 => self.sync_stroke(core, scene, id),
            1 => self.sync_shape(core, scene, id),
            _ => self.sync_image(core, image_pipeline, scene, id),
        }
    }

    fn sync_stroke(&mut self, core: &GpuCore, scene: &mut Scene, id: ItemId) {
        // 1) 읽기 전용으로 필요한 걸 전부 owned 값으로 뽑아냄 — 이 블록이
        //    끝나면 scene에 대한 참조가 사라지므로 이후 &mut scene 호출 가능.
        struct Plan {
            anchor: [f64; 2],
            build: Option<(crate::render::tessellate::StrokeMesh, ([f64; 2], [f64; 2]))>,
        }
        let plan = {
            let Some(CanvasItem::Stroke(s)) = scene.item(id) else {
                return;
            };
            let needs_build = !self.strokes.contains_key(&id) || s.geometry_dirty;
            if needs_build {
                let mesh = tessellate_stroke(s);
                let bbox = local_padded_bbox(s);
                Plan {
                    anchor: s.anchor,
                    build: Some((mesh, bbox)),
                }
            } else {
                Plan {
                    anchor: s.anchor,
                    build: None,
                }
            }
        };

        match plan.build {
            Some((mesh, bbox)) => {
                let entry = self.strokes.entry(id).or_insert_with(|| StrokeGpuResource {
                    mesh: GrowableMesh::new(
                        core,
                        INITIAL_STROKE_VERTEX_CAPACITY,
                        INITIAL_STROKE_INDEX_CAPACITY,
                        "stroke_mesh",
                    ),
                    origin: plan.anchor,
                });
                entry.mesh.upload(core, &mesh.vertices, &mesh.indices);
                entry.origin = plan.anchor;
                self.local_bboxes.insert(id, bbox);
                scene.mark_stroke_clean(id);
            }
            None => {
                // 위치만 바뀜 — GPU 호출 없이 origin 필드만 갱신.
                if let Some(res) = self.strokes.get_mut(&id) {
                    res.origin = plan.anchor;
                }
            }
        }
    }

    fn sync_shape(&mut self, core: &GpuCore, scene: &mut Scene, id: ItemId) {
        struct Plan {
            anchor: [f64; 2],
            build: Option<(crate::render::tessellate::StrokeMesh, ([f64; 2], [f64; 2]))>,
        }
        let plan = {
            let Some(CanvasItem::Shape(sh)) = scene.item(id) else {
                return;
            };
            let needs_build = !self.shapes.contains_key(&id) || sh.geometry_dirty;
            if needs_build {
                let virtual_stroke = sh.as_stroke(); // anchor=center, points=rotated_local_outline
                let mesh = tessellate_stroke(&virtual_stroke);
                let bbox = shape_local_padded_bbox(sh);
                Plan {
                    anchor: sh.center,
                    build: Some((mesh, bbox)),
                }
            } else {
                Plan {
                    anchor: sh.center,
                    build: None,
                }
            }
        };

        match plan.build {
            Some((mesh, bbox)) => {
                let entry = self.shapes.entry(id).or_insert_with(|| StrokeGpuResource {
                    mesh: GrowableMesh::new(
                        core,
                        INITIAL_SHAPE_VERTEX_CAPACITY,
                        INITIAL_SHAPE_INDEX_CAPACITY,
                        "shape_mesh",
                    ),
                    origin: plan.anchor,
                });
                entry.mesh.upload(core, &mesh.vertices, &mesh.indices);
                entry.origin = plan.anchor;
                self.local_bboxes.insert(id, bbox);
                scene.mark_shape_clean(id);
            }
            None => {
                // 순수 이동(center만 바뀜) — outline 재계산 없음.
                if let Some(res) = self.shapes.get_mut(&id) {
                    res.origin = plan.anchor;
                }
            }
        }
    }

    fn sync_image(
        &mut self,
        core: &GpuCore,
        image_pipeline: &ImagePipeline,
        scene: &mut Scene,
        id: ItemId,
    ) {
        enum Plan {
            Create {
                top_left: [f64; 2],
                size: [f64; 2],
                pixel_w: u32,
                pixel_h: u32,
                rgba: std::sync::Arc<[u8]>,
            },
            ResizeOrMove {
                top_left: [f64; 2],
                size: [f64; 2],
                size_changed: bool,
            },
        }
        let plan = {
            let Some(CanvasItem::Image(img)) = scene.item(id) else {
                return;
            };
            if !self.images.contains_key(&id) {
                Plan::Create {
                    top_left: img.top_left,
                    size: img.size,
                    pixel_w: img.pixel_width,
                    pixel_h: img.pixel_height,
                    rgba: img.rgba.clone(),
                }
            } else {
                Plan::ResizeOrMove {
                    top_left: img.top_left,
                    size: img.size,
                    size_changed: img.geometry_dirty,
                }
            }
        };

        match plan {
            Plan::Create {
                top_left,
                size,
                pixel_w,
                pixel_h,
                rgba,
            } => {
                let texture = core.device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("pasted_image_texture"),
                    size: wgpu::Extent3d {
                        width: pixel_w,
                        height: pixel_h,
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
                    &rgba,
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(4 * pixel_w),
                        rows_per_image: Some(pixel_h),
                    },
                    wgpu::Extent3d {
                        width: pixel_w,
                        height: pixel_h,
                        depth_or_array_layers: 1,
                    },
                );
                let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
                let bind_group = core.device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("image_bind_group"),
                    layout: &image_pipeline.texture_bgl,
                    entries: &[
                        wgpu::BindGroupEntry {
                            binding: 0,
                            resource: wgpu::BindingResource::TextureView(&view),
                        },
                        wgpu::BindGroupEntry {
                            binding: 1,
                            resource: wgpu::BindingResource::Sampler(&image_pipeline.sampler),
                        },
                    ],
                });

                let (w, h) = (size[0] as f32, size[1] as f32);
                let verts = [
                    ImageVertex {
                        pos: [
                            0.0, 0.0,
                        ],
                        uv: [
                            0.0, 0.0,
                        ],
                    },
                    ImageVertex {
                        pos: [
                            w, 0.0,
                        ],
                        uv: [
                            1.0, 0.0,
                        ],
                    },
                    ImageVertex {
                        pos: [
                            0.0, h,
                        ],
                        uv: [
                            0.0, 1.0,
                        ],
                    },
                    ImageVertex {
                        pos: [
                            w, h,
                        ],
                        uv: [
                            1.0, 1.0,
                        ],
                    },
                ];
                // 인덱스는 항상 [0,1,2,1,3,2] 고정값 — 이 아이템의 수명 내내
                // 다시 안 바뀌므로 최초 1회만 생성.
                let indices: [u32; 6] = [
                    0, 1, 2, 1, 3, 2,
                ];

                let vertex_buf =
                    core.device
                        .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("image_vertex_buf"),
                            contents: bytemuck::cast_slice(&verts),
                            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                        });
                let index_buf = core
                    .device
                    .create_buffer_init(&wgpu::util::BufferInitDescriptor {
                        label: Some("image_index_buf"),
                        contents: bytemuck::cast_slice(&indices),
                        usage: wgpu::BufferUsages::INDEX,
                    });

                self.images.insert(
                    id,
                    ImageGpuResource {
                        bind_group,
                        vertex_buf,
                        index_buf,
                        origin: top_left,
                    },
                );
                self.local_bboxes.insert(
                    id,
                    (
                        [
                            0.0, 0.0,
                        ],
                        size,
                    ),
                );
                scene.mark_image_clean(id);
            }
            Plan::ResizeOrMove {
                top_left,
                size,
                size_changed,
            } => {
                if size_changed {
                    let (w, h) = (size[0] as f32, size[1] as f32);
                    let verts = [
                        ImageVertex {
                            pos: [
                                0.0, 0.0,
                            ],
                            uv: [
                                0.0, 0.0,
                            ],
                        },
                        ImageVertex {
                            pos: [
                                w, 0.0,
                            ],
                            uv: [
                                1.0, 0.0,
                            ],
                        },
                        ImageVertex {
                            pos: [
                                0.0, h,
                            ],
                            uv: [
                                0.0, 1.0,
                            ],
                        },
                        ImageVertex {
                            pos: [
                                w, h,
                            ],
                            uv: [
                                1.0, 1.0,
                            ],
                        },
                    ];
                    // 고정 4정점 크기라 재할당 없이 항상 write_buffer만.
                    if let Some(res) = self.images.get(&id) {
                        core.queue
                            .write_buffer(&res.vertex_buf, 0, bytemuck::cast_slice(&verts));
                    }
                    self.local_bboxes.insert(
                        id,
                        (
                            [
                                0.0, 0.0,
                            ],
                            size,
                        ),
                    );
                    scene.mark_image_clean(id);
                }
                if let Some(res) = self.images.get_mut(&id) {
                    res.origin = top_left;
                }
            }
        }
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

    /// 뷰포트 컬링 판정. `origin`은 호출부가 이미 들고 있는 값(res.origin)을
    /// 그대로 넘겨줌 — 로컬 bbox 캐시 + 최신 origin을 합쳐서 world bbox를
    /// 매번 구함(덧셈 4번, O(1)). 캐시가 없는 예외적 타이밍엔 기존과
    /// 동일하게 "보인다"로 안전하게 처리.
    pub fn is_visible(
        &self,
        id: ItemId,
        origin: [f64; 2],
        view_min: [f64; 2],
        view_max: [f64; 2],
    ) -> bool {
        match self.local_bboxes.get(&id) {
            Some((lmin, lmax)) => {
                let min = [
                    lmin[0] + origin[0],
                    lmin[1] + origin[1],
                ];
                let max = [
                    lmax[0] + origin[0],
                    lmax[1] + origin[1],
                ];
                !(max[0] < view_min[0]
                    || min[0] > view_max[0]
                    || max[1] < view_min[1]
                    || min[1] > view_max[1])
            }
            None => true,
        }
    }
}
