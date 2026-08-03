// ============================================================
// src/app/render/mod.rs
// ============================================================
mod cursor;
mod live_stroke;
mod overlay;
mod ui_cache;

pub(in crate::app) use live_stroke::LiveStrokeGpu;
pub(in crate::app) use overlay::OverlayGpu;
use overlay::OverlayBuilder;
pub(in crate::app) use ui_cache::UiCache;

use super::{App, Tool};
use crate::render::pipeline::{DrawImmediate, GlobalUniforms};
use crate::render::tessellate::tessellate_stroke;
use crate::scene::CanvasItem;
use wgpu::util::DeviceExt;

const CURSOR_ICON_SIZE_SCREEN_PX: f32 = 22.0;

impl App {
    /// HDR 색상 부스트 적용(rgb만, alpha 무변화).
    pub(super) fn boosted(&self, c: [f32; 4]) -> [f32; 4] {
        let b = self.core.color_boost;
        [c[0] * b, c[1] * b, c[2] * b, c[3]]
    }

    pub fn render_if_needed(&mut self) {
        if self.dirty && self.has_focus {
            self.render();
            self.dirty = false;
        }
    }

    pub fn render(&mut self) {
        if self.ui_dirty || self.ui_cache.is_none() {
            self.build_ui_cache();
        }

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

        let bg = self.boosted([1.0, 1.0, 1.0, 1.0]);

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_texture_view,
                    resolve_target: Some(&view),
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: bg[0] as f64, g: bg[1] as f64, b: bg[2] as f64, a: 1.0,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.registry.sync(&self.core, &self.image_pipeline, &mut self.scene);
            let (view_min, view_max) = self.camera.world_view_bounds();

            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
            let mut on_stroke_pipeline = true;

            for (id, item) in self.scene.iter_ordered_with_id() {
                // [변경] 컬링 체크가 리소스 fetch 뒤로 옮겨짐 — is_visible이
                // 이제 origin을 인자로 받아서(로컬 bbox 캐시 + 최신 origin
                // 합산) world bbox를 구하기 때문. res.origin을 이중으로
                // 들고 다닐 필요가 없어서 오히려 더 단순해짐.
                match item {
                    CanvasItem::Stroke(s) => {
                        let Some(res) = self.registry.get_stroke(id) else { continue };
                        if !self.registry.is_visible(id, res.origin, view_min, view_max) {
                            continue;
                        }
                        if !on_stroke_pipeline {
                            pass.set_pipeline(&self.pipeline.pipeline);
                            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
                            on_stroke_pipeline = true;
                        }
                        let offset = [
                            (res.origin[0] - self.camera.center[0]) as f32,
                            (res.origin[1] - self.camera.center[1]) as f32,
                        ];
                        let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: self.boosted(s.color) };
                        pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                        pass.set_vertex_buffer(0, res.mesh.vertex.buffer().slice(..));
                        pass.set_index_buffer(res.mesh.index.buffer().slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..res.mesh.index_count, 0, 0..1);
                    }
                    CanvasItem::Shape(sh) => {
                        let Some(res) = self.registry.get_shape(id) else { continue };
                        if !self.registry.is_visible(id, res.origin, view_min, view_max) {
                            continue;
                        }
                        if !on_stroke_pipeline {
                            pass.set_pipeline(&self.pipeline.pipeline);
                            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
                            on_stroke_pipeline = true;
                        }
                        let offset = [
                            (res.origin[0] - self.camera.center[0]) as f32,
                            (res.origin[1] - self.camera.center[1]) as f32,
                        ];
                        let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: self.boosted(sh.color) };
                        pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                        pass.set_vertex_buffer(0, res.mesh.vertex.buffer().slice(..));
                        pass.set_index_buffer(res.mesh.index.buffer().slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..res.mesh.index_count, 0, 0..1);
                    }
                    CanvasItem::Image(_) => {
                        let Some(res) = self.registry.get_image(id) else { continue };
                        if !self.registry.is_visible(id, res.origin, view_min, view_max) {
                            continue;
                        }
                        if on_stroke_pipeline {
                            pass.set_pipeline(&self.image_pipeline.pipeline);
                            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
                            on_stroke_pipeline = false;
                        }
                        pass.set_bind_group(1, &res.bind_group, &[]);
                        let offset = [
                            (res.origin[0] - self.camera.center[0]) as f32,
                            (res.origin[1] - self.camera.center[1]) as f32,
                        ];
                        let immediate =
                            DrawImmediate { offset, _pad: [0.0; 2], color: self.boosted([1.0, 1.0, 1.0, 1.0]) };
                        pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                        pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                        pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..6, 0, 0..1);
                    }
                }
            }

            if !on_stroke_pipeline {
                pass.set_pipeline(&self.pipeline.pipeline);
                pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
            }

            if let (Some(stroke), Some(mesh_cache)) = (&self.drawing_stroke, &self.drawing_mesh_cache) {
                if !mesh_cache.indices.is_empty() {
                    if self.live_stroke_gpu.is_none() {
                        self.live_stroke_gpu = Some(LiveStrokeGpu::new(&self.core));
                    }
                    let color = self.boosted(stroke.color);
                    let live = self.live_stroke_gpu.as_mut().unwrap();
                    live.sync(&self.core, mesh_cache);

                    let offset = [
                        (mesh_cache.origin[0] - self.camera.center[0]) as f32,
                        (mesh_cache.origin[1] - self.camera.center[1]) as f32,
                    ];
                    let immediate = DrawImmediate { offset, _pad: [0.0; 2], color };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.set_vertex_buffer(0, live.vertex_slice());
                    pass.set_index_buffer(live.index_slice(), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh_cache.indices.len() as u32, 0, 0..1);
                }
            }

            if let Some(shape) = &self.drawing_shape_preview {
                let virtual_stroke = shape.as_stroke();
                let mesh = tessellate_stroke(&virtual_stroke);
                if !mesh.indices.is_empty() {
                    let vertex_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_shape_preview_vertex_buf"),
                            contents: bytemuck::cast_slice(&mesh.vertices),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                    let index_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_shape_preview_index_buf"),
                            contents: bytemuck::cast_slice(&mesh.indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });
                    let offset = [
                        (mesh.origin[0] - self.camera.center[0]) as f32,
                        (mesh.origin[1] - self.camera.center[1]) as f32,
                    ];
                    let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: self.boosted(shape.color) };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.set_vertex_buffer(0, vertex_buf.slice(..));
                    pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                }
            }

            pass.set_pipeline(&self.ui_pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);

            if let Some(cache) = &self.ui_cache {
                cache.draw(&mut pass);
            }

            // [설계 변경, C] 지우개 인디케이터/선택 오버레이/커서 아이콘을
            // CPU 빌더에 전부 모았다가 growable 버퍼 하나로 한 번에
            // 업로드 — 예전엔 선분/사각형 하나당 GPU 버퍼를 새로 만들었음
            // (지우개 인디케이터만 매 프레임 16번). 각 요소의 색상이
            // 달라서 draw call 자체는 여전히 entry 개수만큼 나가지만,
            // "버퍼 생성"은 프레임당 최대 1번(재할당 필요시)뿐임.
            let mut overlay = OverlayBuilder::default();

            if self.tool == Tool::Eraser {
                cursor::draw_eraser_indicator(
                    &mut overlay,
                    self.input.last_pen_pos(),
                    super::pointer::ERASER_RADIUS_SCREEN_PX,
                );
            }

            if self.tool == Tool::Select {
                if let Some(id) = self.selected_item {
                    if let Some(item) = self.scene.item(id) {
                        cursor::draw_selection_overlay(&mut overlay, &self.camera, item);
                    }
                }
            }

            match self.tool {
                Tool::Pen => cursor::draw_tool_icon_at(
                    &mut overlay, Tool::Pen,
                    self.input.last_pen_pos(), CURSOR_ICON_SIZE_SCREEN_PX, self.boosted(self.pen_color),
                ),
                Tool::Select => cursor::draw_tool_icon_at(
                    &mut overlay, Tool::Select,
                    self.input.last_mouse_pos(), CURSOR_ICON_SIZE_SCREEN_PX, self.boosted(self.pen_color),
                ),
                Tool::Eraser => {}
            }

            if !overlay.entries.is_empty() {
                if self.overlay_gpu.is_none() {
                    self.overlay_gpu = Some(OverlayGpu::new(&self.core));
                }
                let overlay_gpu = self.overlay_gpu.as_mut().unwrap();
                overlay_gpu.upload(&self.core, overlay.vertices());

                // ui_pipeline 그대로 재사용(정점 포맷 동일: Vertex{pos}뿐,
                // 화면좌표계라 카메라 오프셋도 필요 없음 — offset=[0,0]).
                pass.set_vertex_buffer(0, overlay_gpu.buffer().slice(..));
                for entry in &overlay.entries {
                    let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color: entry.color };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.draw(entry.offset..entry.offset + entry.count, 0..1);
                }
            }
        }

        self.core.queue.submit(std::iter::once(encoder.finish()));
        self.core.queue.present(frame);
    }
}