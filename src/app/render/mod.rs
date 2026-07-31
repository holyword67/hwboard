// ============================================================
// src/app/render/mod.rs
// ============================================================
mod cursor;
mod live_stroke;
mod primitives;
mod ui_cache;

pub(in crate::app) use live_stroke::LiveStrokeGpu;
pub(in crate::app) use ui_cache::UiCache;

use super::{App, Tool};
use crate::render::pipeline::{DrawImmediate, GlobalUniforms};
use crate::render::tessellate::tessellate_stroke;
use crate::scene::CanvasItem;
use wgpu::util::DeviceExt;

const CURSOR_ICON_SIZE_SCREEN_PX: f32 = 22.0;

impl App {
    /// HDR 색상 부스트 적용(rgb만, alpha 무변화). Scene에 저장된 색값은
    /// 항상 SDR 기준(0.0~1.0)으로 유지하고, 실제로 GPU에 넘기는 이
    /// 시점에만 곱해줌 — Undo/Redo나 나중에 헤드룸 재조회 기능이
    /// 생겨도 저장 데이터가 오염 안 되도록.
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

            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
            let mut on_stroke_pipeline = true;

            for (id, item) in self.scene.iter_ordered_with_id() {
                match item {
                    CanvasItem::Stroke(s) => {
                        let Some(res) = self.registry.get_stroke(id) else { continue };
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
                        pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                        pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..res.index_count, 0, 0..1);
                    }
                    CanvasItem::Shape(sh) => {
                        let Some(res) = self.registry.get_shape(id) else { continue };
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
                        pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                        pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..res.index_count, 0, 0..1);
                    }
                    CanvasItem::Image(_) => {
                        let Some(res) = self.registry.get_image(id) else { continue };
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
                        // 이미지는 부스트 대상 아님(원본 사진 색 왜곡 방지 — 이번 스코프 밖)
                        let immediate =
                            DrawImmediate { offset, _pad: [0.0; 2], color: [1.0, 1.0, 1.0, 1.0] };
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

            if self.tool == Tool::Eraser {
                cursor::draw_eraser_indicator(
                    &self.core.device,
                    &mut pass,
                    self.input.last_pen_pos(),
                    super::pointer::ERASER_RADIUS_SCREEN_PX,
                );
            }

            if self.tool == Tool::Select {
                if let Some(id) = self.selected_item {
                    if let Some(item) = self.scene.item(id) {
                        cursor::draw_selection_overlay(&self.core.device, &mut pass, &self.camera, item);
                    }
                }
            }

            match self.tool {
                Tool::Pen => cursor::draw_tool_icon_at(
                    &self.core.device, &mut pass, Tool::Pen,
                    self.input.last_pen_pos(), CURSOR_ICON_SIZE_SCREEN_PX, self.boosted(self.pen_color),
                ),
                Tool::Select => cursor::draw_tool_icon_at(
                    &self.core.device, &mut pass, Tool::Select,
                    self.input.last_mouse_pos(), CURSOR_ICON_SIZE_SCREEN_PX, self.boosted(self.pen_color),
                ),
                Tool::Eraser => {}
            }
        }

        self.core.queue.submit(std::iter::once(encoder.finish()));
        self.core.queue.present(frame);
    }
}