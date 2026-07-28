// ============================================================
// src/app/render.rs
// ============================================================
// GPU 렌더 패스 — 캔버스(스트로크/이미지), 그리는 중인 스트로크, UI
// 오버레이(툴바+지우개 인디케이터)까지 프레임 하나에 순서대로 그림.

use super::{App, Tool};
use crate::render::pipeline::{DrawImmediate, GlobalUniforms, Vertex};
use crate::render::tessellate::tessellate_stroke;
use crate::scene::CanvasItem;
use crate::ui;
use wgpu::util::DeviceExt;

/// 지우개 범위 점선 인디케이터 스타일.
const ERASER_INDICATOR_DASH_COUNT: usize = 16;
const ERASER_INDICATOR_LINE_WIDTH: f32 = 2.0;
const ERASER_INDICATOR_COLOR: [f32; 4] = [0.2, 0.2, 0.2, 0.6];

impl App {

    pub fn render_if_needed(&mut self) {
        if self.dirty && self.has_focus {
            self.render();
            self.dirty = false;
        }
    }

    pub fn render(&mut self) {
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
                    view: &self.msaa_texture_view, // 👇 여기에 MSAA 뷰를 넣고
                    resolve_target: Some(&view),   // 👇 결과를 최종 화면(view)으로 쏩니다
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
                        let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: s.color };
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
                        let immediate =
                            DrawImmediate { offset, _pad: [0.0; 2], color: [1.0, 1.0, 1.0, 1.0] };
                        pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                        pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                        pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..6, 0, 0..1);
                    }
                    _ => {} // Shape/Text 미구현
                }
            }

            // 이후 pass 안에서 stroke_pipeline을 다시 쓰는 지점(그리는 중인
            // 스트로크, UI 오버레이)들이 있으니 마지막 상태를 stroke로
            // 되돌려둠.
            if !on_stroke_pipeline {
                pass.set_pipeline(&self.pipeline.pipeline);
                pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
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

            // UI 오버레이 — 캔버스 다음에 그려서 항상 위에 보이게.
            pass.set_pipeline(&self.ui_pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);

            let buttons = ui::layout(self.camera.viewport_size, self.tool, self.pen_color);
            for b in &buttons {
                if b.selected {
                    let hl = ui::Rect {
                        x: b.rect.x - ui::HIGHLIGHT_PADDING,
                        y: b.rect.y - ui::HIGHLIGHT_PADDING,
                        w: b.rect.w + ui::HIGHLIGHT_PADDING * 2.0,
                        h: b.rect.h + ui::HIGHLIGHT_PADDING * 2.0,
                    };
                    draw_ui_quad(&self.core.device, &mut pass, hl, ui::HIGHLIGHT_COLOR);
                }
                draw_ui_quad(&self.core.device, &mut pass, b.rect, b.color);
            }

            // 지우개 범위 인디케이터 — 누르고 있을 때만이 아니라 도구가
            // Eraser인 동안 항상(호버 중에도) 표시. 실제 삭제 발동 여부는
            // pointer.rs의 eraser_pressed로 게이팅되고, 이 인디케이터는
            // 그거랑 무관하게 위치만 따라다님.
            if self.tool == Tool::Eraser {
                draw_eraser_indicator(
                    &self.core.device,
                    &mut pass,
                    self.input.last_pen_pos(),
                    super::pointer::ERASER_RADIUS_SCREEN_PX,
                );
            }
        }

        self.core.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}

/// UI 버튼 하나를 사각형(삼각형 2개, 인덱스버퍼 없이 정점 6개)으로 그림.
/// 캔버스 스트로크와 달리 버튼은 개수가 적고(6개) 정적이라 매 프레임
/// 버퍼 새로 만들어도 비용 무시할 만함.
fn draw_ui_quad(device: &wgpu::Device, pass: &mut wgpu::RenderPass, rect: ui::Rect, color: [f32; 4]) {
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let verts = [
        Vertex { pos: [x0, y0] },
        Vertex { pos: [x1, y0] },
        Vertex { pos: [x0, y1] },
        Vertex { pos: [x1, y0] },
        Vertex { pos: [x1, y1] },
        Vertex { pos: [x0, y1] },
    ];
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ui_quad_vbuf"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color };
    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
    pass.set_vertex_buffer(0, vbuf.slice(..));
    pass.draw(0..6, 0..1);
}

/// 스크린 좌표계에서 두 점 사이를 얇은 사각형(quad)으로 잇는다. 카메라
/// 변환 없는 UI 좌표라 tessellate::connect_quad와 달리 f32 그대로 계산.
fn draw_screen_line_segment(
    device: &wgpu::Device,
    pass: &mut wgpu::RenderPass,
    p0: [f32; 2],
    p1: [f32; 2],
    width: f32,
    color: [f32; 4],
) {
    let dir = [p1[0] - p0[0], p1[1] - p0[1]];
    let len = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
    if len < f32::EPSILON {
        return;
    }
    let normal = [-dir[1] / len, dir[0] / len];
    let hw = width * 0.5;

    let verts = [
        Vertex { pos: [p0[0] + normal[0] * hw, p0[1] + normal[1] * hw] },
        Vertex { pos: [p0[0] - normal[0] * hw, p0[1] - normal[1] * hw] },
        Vertex { pos: [p1[0] + normal[0] * hw, p1[1] + normal[1] * hw] },
        Vertex { pos: [p0[0] - normal[0] * hw, p0[1] - normal[1] * hw] },
        Vertex { pos: [p1[0] - normal[0] * hw, p1[1] - normal[1] * hw] },
        Vertex { pos: [p1[0] + normal[0] * hw, p1[1] + normal[1] * hw] },
    ];
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("eraser_indicator_segment_vbuf"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color };
    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
    pass.set_vertex_buffer(0, vbuf.slice(..));
    pass.draw(0..6, 0..1);
}

/// 지우개 범위를 점선 원으로 표시. 대시:간격 = 1:1 비율로
/// ERASER_INDICATOR_DASH_COUNT개 배치.
fn draw_eraser_indicator(
    device: &wgpu::Device,
    pass: &mut wgpu::RenderPass,
    center: [f32; 2],
    radius: f32,
) {
    let slots = ERASER_INDICATOR_DASH_COUNT * 2; // dash + gap 쌍
    for i in 0..ERASER_INDICATOR_DASH_COUNT {
        let a0 = (i * 2) as f32 / slots as f32 * std::f32::consts::TAU;
        let a1 = (i * 2 + 1) as f32 / slots as f32 * std::f32::consts::TAU;
        let p0 = [center[0] + radius * a0.cos(), center[1] + radius * a0.sin()];
        let p1 = [center[0] + radius * a1.cos(), center[1] + radius * a1.sin()];
        draw_screen_line_segment(device, pass, p0, p1, ERASER_INDICATOR_LINE_WIDTH, ERASER_INDICATOR_COLOR);
    }
}