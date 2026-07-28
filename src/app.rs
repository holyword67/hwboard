// ============================================================
// src/app.rs
// ============================================================
// WgpuRenderer 대응 — 모든 조각(camera/input/scene/render)이 여기서
// 만난다. 메인 루프 오케스트레이터.

use crate::gpu::core::GpuCore;
use crate::input::{InputEvent, InputState, PointerEvent, PEN_BUTTON_REDO, PEN_BUTTON_UNDO};
use crate::render::camera::Camera;
use crate::render::gpu_resources::GpuResourceRegistry;
use crate::render::pipeline::{DrawImmediate, GlobalUniforms, StrokePipeline, Vertex};
use crate::render::tessellate::tessellate_stroke;
use crate::scene::{AddItem, CanvasItem, DeleteItems, ItemId, PenPoint, Scene, Stroke, UndoStack};
use sdl3::event::Event;
use sdl3::video::Window;
use std::time::Instant;
use wgpu::util::DeviceExt;

const PEN_COLOR: [f32; 4] = [0.0, 0.0, 0.0, 1.0];
const PEN_BASE_WIDTH: f32 = 3.0;
/// [미검증 가설] world 단위 고정 반경 — zoom에 따라 스크린상 크기가
/// 달라짐(확대하면 지우개가 스크린상 커 보임). 줌 불변으로 할지는
/// 실제 써보고 판단 필요.
const ERASER_RADIUS_WORLD: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
    Pen,
    Eraser,
}

pub struct App {
    core: GpuCore,
    pipeline: StrokePipeline,
    registry: GpuResourceRegistry,
    scene: Scene,
    undo_stack: UndoStack,
    camera: Camera,
    input: InputState,
    tool: Tool,
    /// A안: 그리는 중인 스트로크는 Scene 밖 임시 상태로 보관, Up에서 커밋.
    drawing_stroke: Option<Stroke>,
    /// 지우개 드래그 중 누적된 삭제분 — Up에서 한 번에 커밋.
    erasing_removed: Vec<(ItemId, CanvasItem, usize)>,
    open: bool,
}

impl App {
    pub async fn new(window: &Window) -> Self {
        let core = GpuCore::new(window).await;
        let pipeline = StrokePipeline::new(&core);
        let (w, h) = window.size();
        Self {
            core,
            pipeline,
            registry: GpuResourceRegistry::new(),
            scene: Scene::new(),
            undo_stack: UndoStack::new(),
            camera: Camera::new([w as f32, h as f32]),
            input: InputState::new(),
            tool: Tool::Pen,
            drawing_stroke: None,
            erasing_removed: Vec::new(),
            open: true,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub fn handle_sdl_event(&mut self, event: &Event, _window: &mut Window) {
        match event {
            Event::Quit { .. } => self.open = false,
            Event::Window {
                win_event: sdl3::event::WindowEvent::PixelSizeChanged(w, h)
                    | sdl3::event::WindowEvent::Resized(w, h),
                ..
            } => {
                self.core.resize(*w as u32, *h as u32);
                self.camera.resize([*w as f32, *h as f32]);
            }
            _ => {
                if let Some(input_event) = self.input.process_event(event) {
                    self.handle_input_event(input_event);
                }
            }
        }
    }

    /// 매 프레임 호출 — hold(도형 자동스냅) 폴링.
    pub fn poll(&mut self) {
        if let Some(input_event) = self.input.update(Instant::now()) {
            self.handle_input_event(input_event);
        }
    }

    fn handle_input_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Pointer(p) => self.handle_pointer(p),
            InputEvent::PenButton { button, pressed: true } if button == PEN_BUTTON_UNDO => {
                self.undo_stack.undo(&mut self.scene);
            }
            InputEvent::PenButton { button, pressed: true } if button == PEN_BUTTON_REDO => {
                self.undo_stack.redo(&mut self.scene);
            }
            InputEvent::PenButton { .. } => {}
        }
    }

    fn handle_pointer(&mut self, ev: PointerEvent) {
        match (self.tool, ev) {
            (Tool::Pen, PointerEvent::Down(s)) => {
                let world = self.camera.screen_to_world(s.pos);
                self.drawing_stroke = Some(Stroke {
                    points: vec![PenPoint { pos: world, pressure: s.pressure }],
                    color: PEN_COLOR,
                    base_width: PEN_BASE_WIDTH,
                    mesh_dirty: true,
                });
            }
            (Tool::Pen, PointerEvent::Move(s)) => {
                if let Some(stroke) = &mut self.drawing_stroke {
                    let world = self.camera.screen_to_world(s.pos);
                    stroke.points.push(PenPoint { pos: world, pressure: s.pressure });
                }
            }
            (Tool::Pen, PointerEvent::Up(_)) => {
                if let Some(stroke) = self.drawing_stroke.take() {
                    // 점 1개짜리 스트로크(그냥 탭)는 무의미하니 버림.
                    if stroke.points.len() >= 2 {
                        let id = self.scene.alloc_id();
                        let cmd = Box::new(AddItem { id, item: CanvasItem::Stroke(stroke) });
                        self.undo_stack.execute(cmd, &mut self.scene);
                    }
                }
            }
            (Tool::Eraser, PointerEvent::Down(s)) => {
                self.erasing_removed.clear();
                self.try_erase_at(s.pos);
            }
            (Tool::Eraser, PointerEvent::Move(s)) => {
                self.try_erase_at(s.pos);
            }
            (Tool::Eraser, PointerEvent::Up(_)) => {
                if !self.erasing_removed.is_empty() {
                    let cmd = Box::new(DeleteItems { removed: std::mem::take(&mut self.erasing_removed) });
                    self.undo_stack.push_already_applied(cmd);
                }
            }
            (_, PointerEvent::Hold(_)) => {
                // TODO: 도형 자동스냅 — 다음 논의 주제
            }
        }
    }

    /// [단순 구현] bounding box 기준 히트테스트. 스트로크 실제 선과의
    /// 거리가 아니라 사각형 범위라 정밀하지 않음 — 실사용해보고 부정확함이
    /// 느껴지면 세그먼트 거리 기반으로 교체 (지금은 가설 단계 최적화 보류).
    fn try_erase_at(&mut self, screen_pos: [f32; 2]) {
        let world = self.camera.screen_to_world(screen_pos);
        let r = ERASER_RADIUS_WORLD;

        let hit = self.scene.iter_ordered_with_id().find_map(|(id, item)| {
            if self.erasing_removed.iter().any(|(rid, _, _)| *rid == id) {
                return None;
            }
            let (min, max) = item.bounding_box();
            let inside = world[0] >= min[0] - r
                && world[0] <= max[0] + r
                && world[1] >= min[1] - r
                && world[1] <= max[1] + r;
            inside.then_some(id)
        });

        if let Some(id) = hit {
            if let Some(item) = self.scene.item(id).cloned() {
                let z = self.scene.z_index_of(id).unwrap_or(0);
                self.scene.remove(id);
                self.erasing_removed.push((id, item, z));
            }
        }
    }

    pub fn render(&mut self) {
        self.registry.sync(&self.core, &mut self.scene);

        let uniforms = GlobalUniforms {
            zoom: self.camera.zoom,
            viewport_w: self.camera.viewport_size[0],
            viewport_h: self.camera.viewport_size[1],
            _pad: 0.0,
        };
        self.core.queue.write_buffer(&self.core.global_uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let frame = match self.core.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            _ => {
                self.core.surface.configure(&self.core.device, &self.core.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.core.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);

            for (id, item) in self.scene.iter_ordered_with_id() {
                let CanvasItem::Stroke(s) = item else {
                    continue; // TODO: Shape/Text/Image draw — 다음 단계
                };
                let Some(res) = self.registry.get_stroke(id) else { continue };

                let offset = [
                    (res.origin[0] - self.camera.center[0]) as f32,
                    (res.origin[1] - self.camera.center[1]) as f32,
                ];
                let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: s.color };
                pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..res.index_count, 0, 0..1);
            }

            // 그리는 중인 스트로크 — registry 캐싱 없이 매 프레임 즉석
            // 테셀레이션 + 버퍼 생성. 아이템 딱 하나뿐이고 어차피 매 프레임
            // 포인트가 바뀌어서 캐싱해봐야 이득이 거의 없음 (실측 전 가설
            // 이지만, 여긴 애초에 캐싱 이득 구조 자체가 없는 케이스라 굳이
            // 재보고 결정할 것도 없다고 판단).
            if let Some(stroke) = &self.drawing_stroke {
                let mesh = tessellate_stroke(stroke);
                if !mesh.indices.is_empty() {
                    let vertex_data: Vec<Vertex> =
                        mesh.vertices.iter().map(|&pos| Vertex { pos }).collect();

                    let vertex_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_stroke_vertex_buf"),
                            contents: bytemuck::cast_slice(&vertex_data),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                    let index_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_stroke_index_buf"),
                            contents: bytemuck::cast_slice(&mesh.indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });

                    let offset = [
                        (mesh.origin[0] - self.camera.center[0]) as f32,
                        (mesh.origin[1] - self.camera.center[1]) as f32,
                    ];
                    let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: stroke.color };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.set_vertex_buffer(0, vertex_buf.slice(..));
                    pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                }
            }
        }

        self.core.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}