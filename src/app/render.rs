// ============================================================
// src/app/render.rs
// ============================================================
use super::{App, Tool};
use crate::render::camera::Camera;
use crate::render::pipeline::{DrawImmediate, GlobalUniforms, Vertex};
use crate::render::tessellate::tessellate_stroke;
use crate::scene::CanvasItem;
use crate::ui;
use wgpu::util::DeviceExt;

const ERASER_INDICATOR_DASH_COUNT: usize = 16;
const ERASER_INDICATOR_LINE_WIDTH: f32 = 2.0;
const ERASER_INDICATOR_COLOR: [f32; 4] = [0.2, 0.2, 0.2, 0.6];

const SELECTION_LINE_WIDTH: f32 = 1.5;
const SELECTION_COLOR: [f32; 4] = [0.1, 0.4, 0.9, 0.9];
const CURSOR_ICON_SIZE_SCREEN_PX: f32 = 22.0;

/// 도구함(버튼/팔레트/두께바) 캐시 — tool/color/thickness/뷰포트 크기가
/// 바뀔 때(ui_dirty)만 재조립. 지우개 커서/선택 핸들은 이 캐시를 전혀
/// 모름 — 매 프레임 그대로 즉석 생성(격리된 시스템, 서로 참조 없음).
pub(super) struct UiCache {
    vertex_buf: wgpu::Buffer,
    entries: Vec<UiDrawEntry>,
}

struct UiDrawEntry {
    start: u32,
    count: u32,
    color: [f32; 4],
}

impl App {
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

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.msaa_texture_view,
                    resolve_target: Some(&view),
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
                        let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: sh.color };
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
                    CanvasItem::Text(_) => {} // 미구현
                }
            }

            if !on_stroke_pipeline {
                pass.set_pipeline(&self.pipeline.pipeline);
                pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
            }

            // 그리는 중인 자유필기 스트로크(도형으로 스냅되기 전).
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

            // 스냅된 도형 프리뷰(Hold 이후, 아직 Up 안 됨) — Shape을
            // 임시 outline Stroke로 변환해서 즉석 테셀레이션(캐싱 없음,
            // 어차피 매 프레임 바뀌니 drawing_stroke 프리뷰와 같은 이유).
            if let Some(shape) = &self.drawing_shape_preview {
                let virtual_stroke = shape.as_stroke();
                let mesh = tessellate_stroke(&virtual_stroke);
                if !mesh.indices.is_empty() {
                    let vertex_data: Vec<Vertex> =
                        mesh.vertices.iter().map(|&pos| Vertex { pos }).collect();
                    let vertex_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_shape_preview_vertex_buf"),
                            contents: bytemuck::cast_slice(&vertex_data),
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
                    let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: shape.color };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.set_vertex_buffer(0, vertex_buf.slice(..));
                    pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                }
            }

            // UI 오버레이 — 캔버스 다음에 그려서 항상 위에 보이게.
            // 도구함(버튼/팔레트/두께바)은 ui_cache에서 그대로 재생 —
            // build_ui_cache가 위에서 필요할 때만 이미 재조립해뒀음.
            pass.set_pipeline(&self.ui_pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);

            if let Some(cache) = &self.ui_cache {
                pass.set_vertex_buffer(0, cache.vertex_buf.slice(..));
                for e in &cache.entries {
                    let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color: e.color };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.draw(e.start..e.start + e.count, 0..1);
                }
            }

            // 지우개 커서/선택 핸들 — 도구함 캐시와 완전히 독립. 마우스를
            // 따라 매 프레임 바뀌므로 캐싱 대상이 아님(즉석 생성 유지).
            if self.tool == Tool::Eraser {
                draw_eraser_indicator(
                    &self.core.device,
                    &mut pass,
                    self.input.last_pen_pos(),
                    super::pointer::ERASER_RADIUS_SCREEN_PX,
                );
            }

            if self.tool == Tool::Select {
                if let Some(id) = self.selected_item {
                    if let Some(item) = self.scene.item(id) {
                        draw_selection_overlay(&self.core.device, &mut pass, &self.camera, item);
                    }
                }
            }

            // 커스텀 포인터 커서 — OS 커서 대신 현재 도구 버튼과 같은
            // 모양+색으로 그림. Pen/Eraser는 펜 위치, Select는 마우스
            // 위치를 따라감(사용자 확인 사항). 지우개는 기존 점선 원만
            // 유지하고 별도 아이콘은 안 그림.
            match self.tool {
                Tool::Pen => draw_tool_icon_at(
                    &self.core.device, &mut pass, Tool::Pen,
                    self.input.last_pen_pos(), CURSOR_ICON_SIZE_SCREEN_PX, self.pen_color,
                ),
                Tool::Select => draw_tool_icon_at(
                    &self.core.device, &mut pass, Tool::Select,
                    self.input.last_mouse_pos(), CURSOR_ICON_SIZE_SCREEN_PX, self.pen_color,
                ),
                Tool::Eraser => {}
            }
        }

        self.core.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }

    /// 도구함(버튼/팔레트/두께바) 정점을 하나의 버퍼로 통째로 재조립.
    /// ui_dirty일 때만 호출 — 지우개 커서/선택 핸들은 이 함수와 완전히
    /// 무관(각각 draw_eraser_indicator/draw_selection_overlay가 별도 처리).
    fn build_ui_cache(&mut self) {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut entries: Vec<UiDrawEntry> = Vec::new();

        let buttons = ui::layout(self.camera.viewport_size, self.tool, self.pen_color, self.pen_width);
        for b in &buttons {
            match b.kind {
                ui::ButtonKind::Color(c) => {
                    push_quad(&mut vertices, &mut entries, b.rect, c);
                    if b.selected {
                        let cx = b.rect.x + b.rect.w * 0.5;
                        let cy = b.rect.y - 6.0;
                        push_triangle_inverted(&mut vertices, &mut entries, [cx, cy], 8.0, c);
                    }
                }
                ui::ButtonKind::ThicknessBar { selected_index } => {
                    push_thickness_bar(&mut vertices, &mut entries, b.rect, selected_index, self.pen_color);
                }
                ui::ButtonKind::Tool(tool) => {
                    let color = if b.selected { self.pen_color } else { [0.6, 0.6, 0.6, 1.0] };
                    push_tool_icon(&mut vertices, &mut entries, tool, b.rect, color);
                }
            }
        }

        let vertex_buf = self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("ui_cache_vertex_buf"),
            contents: bytemuck::cast_slice(&vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });

        self.ui_cache = Some(UiCache { vertex_buf, entries });
        self.ui_dirty = false;
    }
}

/// 아래 push_* 함수들은 즉석 draw 대신 vertices/entries에 데이터만
/// 쌓는다 — draw_* 계열(즉석 생성)과 이름으로 구분해서 혼동 방지.
fn push_quad(vertices: &mut Vec<Vertex>, entries: &mut Vec<UiDrawEntry>, rect: ui::Rect, color: [f32; 4]) {
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let start = vertices.len() as u32;
    vertices.extend_from_slice(&[
        Vertex { pos: [x0, y0] },
        Vertex { pos: [x1, y0] },
        Vertex { pos: [x0, y1] },
        Vertex { pos: [x1, y0] },
        Vertex { pos: [x1, y1] },
        Vertex { pos: [x0, y1] },
    ]);
    entries.push(UiDrawEntry { start, count: 6, color });
}

fn push_line_segment(
    vertices: &mut Vec<Vertex>,
    entries: &mut Vec<UiDrawEntry>,
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

    let start = vertices.len() as u32;
    vertices.extend_from_slice(&[
        Vertex { pos: [p0[0] + normal[0] * hw, p0[1] + normal[1] * hw] },
        Vertex { pos: [p0[0] - normal[0] * hw, p0[1] - normal[1] * hw] },
        Vertex { pos: [p1[0] + normal[0] * hw, p1[1] + normal[1] * hw] },
        Vertex { pos: [p0[0] - normal[0] * hw, p0[1] - normal[1] * hw] },
        Vertex { pos: [p1[0] - normal[0] * hw, p1[1] - normal[1] * hw] },
        Vertex { pos: [p1[0] + normal[0] * hw, p1[1] + normal[1] * hw] },
    ]);
    entries.push(UiDrawEntry { start, count: 6, color });
}

fn push_triangle_inverted(
    vertices: &mut Vec<Vertex>,
    entries: &mut Vec<UiDrawEntry>,
    center: [f32; 2],
    size: f32,
    color: [f32; 4],
) {
    let hw = size * 0.73;
    let start = vertices.len() as u32;
    vertices.extend_from_slice(&[
        Vertex { pos: [center[0] - hw, center[1] - hw] },
        Vertex { pos: [center[0] + hw, center[1] - hw] },
        Vertex { pos: [center[0], center[1] + hw] },
    ]);
    entries.push(UiDrawEntry { start, count: 3, color });
}

fn push_tool_icon(
    vertices: &mut Vec<Vertex>,
    entries: &mut Vec<UiDrawEntry>,
    tool: Tool,
    rect: ui::Rect,
    color: [f32; 4],
) {
    let cx = rect.x + rect.w * 0.5;
    let cy = rect.y + rect.h * 0.5;
    let s = rect.w * 0.45;
    let w = 1.5;

    match tool {
        Tool::Pen => {
            let rotate = |x: f32, y: f32| -> [f32; 2] {
                let angle = std::f32::consts::FRAC_PI_4;
                let rx = x * angle.cos() - y * angle.sin();
                let ry = x * angle.sin() + y * angle.cos();
                [cx + rx, cy + ry]
            };

            let pw = s * 0.4;
            let ph = s * 0.8;
            let pt = s * 1.3;

            let tl = rotate(-pw, -ph);
            let tr = rotate(pw, -ph);
            let bl = rotate(-pw, ph);
            let br = rotate(pw, ph);
            let tip = rotate(0.0, pt);

            push_line_segment(vertices, entries, tl, tr, w, color);
            push_line_segment(vertices, entries, tl, bl, w, color);
            push_line_segment(vertices, entries, tr, br, w, color);
            push_line_segment(vertices, entries, bl, br, w, color);
            push_line_segment(vertices, entries, bl, tip, w, color);
            push_line_segment(vertices, entries, br, tip, w, color);
        }
        Tool::Eraser => {
            let ew = s * 0.5;
            let eh_top = s * 0.6;
            let eh_bot = s * 0.3;

            let tl = [cx - ew, cy - eh_top];
            let tr = [cx + ew, cy - eh_top];
            let bl = [cx - ew, cy + eh_bot - eh_bot * 0.5];
            let br = [cx + ew, cy + eh_bot - eh_bot * 0.5];

            push_line_segment(vertices, entries, tl, tr, w, color);
            push_line_segment(vertices, entries, tl, bl, w, color);
            push_line_segment(vertices, entries, tr, br, w, color);

            let wrap_w = ew * 1.1;
            let wrap_tl = [cx - wrap_w, cy + eh_bot - eh_bot * 0.5];
            let wrap_tr = [cx + wrap_w, cy + eh_bot - eh_bot * 0.5];
            let wrap_bl = [cx - wrap_w, cy + eh_bot * 1.2];
            let wrap_br = [cx + wrap_w, cy + eh_bot * 1.2];

            push_line_segment(vertices, entries, wrap_tl, wrap_tr, w, color);
            push_line_segment(vertices, entries, wrap_bl, wrap_br, w, color);
            push_line_segment(vertices, entries, wrap_tl, wrap_bl, w, color);
            push_line_segment(vertices, entries, wrap_tr, wrap_br, w, color);
        }
        Tool::Select => {
            let p0 = [cx - s * 0.4, cy - s * 0.7];
            let p1 = [cx + s * 0.6, cy + s * 0.4];
            let p2 = [cx, cy + s * 0.2];
            let p3 = [cx - s * 0.4, cy + s * 0.8];
            push_line_segment(vertices, entries, p0, p1, w, color);
            push_line_segment(vertices, entries, p1, p2, w, color);
            push_line_segment(vertices, entries, p2, p3, w, color);
            push_line_segment(vertices, entries, p3, p0, w, color);
        }
    }
}

fn push_thickness_bar(
    vertices: &mut Vec<Vertex>,
    entries: &mut Vec<UiDrawEntry>,
    rect: ui::Rect,
    selected_index: usize,
    fill_color: [f32; 4],
) {
    let x0 = rect.x;
    let total_w = rect.w;
    let cy = rect.y + rect.h * 0.5;
    let max_r = rect.h * 0.5;

    let min_r = max_r * 0.15;

    let get_r = |x: f32| -> f32 {
        let cx = x0 + total_w - max_r;
        if x <= cx {
            let t = (x - x0) / (cx - x0).max(0.001);
            min_r + t * (max_r - min_r)
        } else {
            let dx = x - cx;
            if dx >= max_r { 0.0 } else { (max_r * max_r - dx * dx).sqrt() }
        }
    };

    let step = total_w / 5.0;

    // 1. 선택된 구간 색 채우기 — 한 entry로 통째 등록.
    let start_x = x0 + selected_index as f32 * step;
    let end_x = x0 + (selected_index + 1) as f32 * step;
    let slices = 15;
    let start = vertices.len() as u32;
    for i in 0..slices {
        let x_a = start_x + (i as f32 / slices as f32) * step;
        let x_b = start_x + ((i + 1) as f32 / slices as f32) * step;
        let ra = get_r(x_a);
        let rb = get_r(x_b);
        vertices.push(Vertex { pos: [x_a, cy - ra] });
        vertices.push(Vertex { pos: [x_b, cy - rb] });
        vertices.push(Vertex { pos: [x_b, cy + rb] });
        vertices.push(Vertex { pos: [x_a, cy - ra] });
        vertices.push(Vertex { pos: [x_b, cy + rb] });
        vertices.push(Vertex { pos: [x_a, cy + ra] });
    }
    let _ = end_x; // 원본 로직 유지(참고용 변수, 원본도 미사용)
    let count = vertices.len() as u32 - start;
    if count > 0 {
        entries.push(UiDrawEntry { start, count, color: fill_color });
    }

    // 2. 외곽선(검은색)
    let outline_color = [0.1, 0.1, 0.1, 1.0];
    let outline_w = 1.0;

    push_line_segment(vertices, entries, [x0, cy - min_r], [x0, cy + min_r], outline_w, outline_color);

    let total_slices = 40;
    for i in 0..total_slices {
        let x_a = x0 + (i as f32 / total_slices as f32) * total_w;
        let x_b = x0 + ((i + 1) as f32 / total_slices as f32) * total_w;
        let ra = get_r(x_a);
        let rb = get_r(x_b);
        push_line_segment(vertices, entries, [x_a, cy - ra], [x_b, cy - rb], outline_w, outline_color);
        push_line_segment(vertices, entries, [x_a, cy + ra], [x_b, cy + rb], outline_w, outline_color);
    }

    // 3. 5등분 구분선
    for i in 1..5 {
        let lx = x0 + i as f32 * step;
        let r = get_r(lx);
        push_line_segment(vertices, entries, [lx, cy - r], [lx, cy + r], outline_w, outline_color);
    }
}

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
        label: Some("screen_line_segment_vbuf"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color };
    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
    pass.set_vertex_buffer(0, vbuf.slice(..));
    pass.draw(0..6, 0..1);
}

fn draw_eraser_indicator(
    device: &wgpu::Device,
    pass: &mut wgpu::RenderPass,
    center: [f32; 2],
    radius: f32,
) {
    let slots = ERASER_INDICATOR_DASH_COUNT * 2;
    for i in 0..ERASER_INDICATOR_DASH_COUNT {
        let a0 = (i * 2) as f32 / slots as f32 * std::f32::consts::TAU;
        let a1 = (i * 2 + 1) as f32 / slots as f32 * std::f32::consts::TAU;
        let p0 = [center[0] + radius * a0.cos(), center[1] + radius * a0.sin()];
        let p1 = [center[0] + radius * a1.cos(), center[1] + radius * a1.sin()];
        draw_screen_line_segment(device, pass, p0, p1, ERASER_INDICATOR_LINE_WIDTH, ERASER_INDICATOR_COLOR);
    }
}

fn draw_handle_square(device: &wgpu::Device, pass: &mut wgpu::RenderPass, center: [f32; 2], size: f32) {
    let half = size * 0.5;
    let rect = ui::Rect { x: center[0] - half, y: center[1] - half, w: size, h: size };
    draw_ui_quad(device, pass, rect, SELECTION_COLOR);
}

/// 선택된 아이템 위에 점선(을 흉내낸 얇은 실선) bbox + (도형이면)
/// 리사이즈/회전 핸들을 그림. 여기서만 쓰는 자유 함수 — item은
/// self.scene에서 빌려온 참조라 &mut self 메서드로 넘기면 보로우
/// 충돌이 나서, App 메서드가 아니라 자유 함수로 뺐음.
fn draw_selection_overlay(
    device: &wgpu::Device,
    pass: &mut wgpu::RenderPass,
    camera: &Camera,
    item: &CanvasItem,
) {
    match item {
        CanvasItem::Shape(sh) => {
            let corners_screen: Vec<[f32; 2]> =
                sh.world_corners().iter().map(|&c| camera.world_to_screen(c)).collect();
            for i in 0..4 {
                draw_screen_line_segment(
                    device, pass, corners_screen[i], corners_screen[(i + 1) % 4],
                    SELECTION_LINE_WIDTH, SELECTION_COLOR,
                );
            }
            for c in &corners_screen {
                draw_handle_square(device, pass, *c, super::select::HANDLE_SIZE_SCREEN_PX);
            }

            let d = (super::select::ROTATE_HANDLE_DISTANCE_SCREEN_PX / camera.zoom) as f64;
            let angle = sh.rotation as f64 - std::f64::consts::FRAC_PI_2;
            let handle_world = [sh.center[0] + d * angle.cos(), sh.center[1] + d * angle.sin()];
            let handle_screen = camera.world_to_screen(handle_world);
            let top_mid_screen = [
                (corners_screen[0][0] + corners_screen[1][0]) * 0.5,
                (corners_screen[0][1] + corners_screen[1][1]) * 0.5,
            ];
            draw_screen_line_segment(device, pass, top_mid_screen, handle_screen, SELECTION_LINE_WIDTH, SELECTION_COLOR);
            draw_handle_square(device, pass, handle_screen, super::select::HANDLE_SIZE_SCREEN_PX);
        }
        CanvasItem::Image(_) => {
            let (min, max) = item.bounding_box();
            let corners_world = [min, [max[0], min[1]], max, [min[0], max[1]]];
            let corners_screen: Vec<[f32; 2]> = corners_world.iter().map(|&c| camera.world_to_screen(c)).collect();
            for i in 0..4 {
                draw_screen_line_segment(
                    device, pass, corners_screen[i], corners_screen[(i + 1) % 4],
                    SELECTION_LINE_WIDTH, SELECTION_COLOR,
                );
            }
            for c in &corners_screen {
                draw_handle_square(device, pass, *c, super::select::HANDLE_SIZE_SCREEN_PX);
            }
        }
        CanvasItem::Stroke(_) | CanvasItem::Text(_) => {
            let (min, max) = item.bounding_box();
            let corners_world = [min, [max[0], min[1]], max, [min[0], max[1]]];
            let corners_screen: Vec<[f32; 2]> = corners_world.iter().map(|&c| camera.world_to_screen(c)).collect();
            for i in 0..4 {
                draw_screen_line_segment(
                    device, pass, corners_screen[i], corners_screen[(i + 1) % 4],
                    SELECTION_LINE_WIDTH, SELECTION_COLOR,
                );
            }
            // 핸들 없음 — 이동만 가능.
        }
    }
}

fn draw_tool_icon_at(
    device: &wgpu::Device,
    pass: &mut wgpu::RenderPass,
    tool: Tool,
    center: [f32; 2],
    size: f32,
    color: [f32; 4],
) {
    let cx = center[0];
    let cy = center[1];
    let s = size * 0.45;
    let w = 1.5;

    match tool {
        Tool::Pen => {
            let rotate = |x: f32, y: f32| -> [f32; 2] {
                let angle = std::f32::consts::FRAC_PI_4;
                let rx = x * angle.cos() - y * angle.sin();
                let ry = x * angle.sin() + y * angle.cos();
                [cx + rx, cy + ry]
            };
            let pw = s * 0.4;
            let ph = s * 0.8;
            let pt = s * 1.3;
            let tl = rotate(-pw, -ph);
            let tr = rotate(pw, -ph);
            let bl = rotate(-pw, ph);
            let br = rotate(pw, ph);
            let tip = rotate(0.0, pt);
            draw_screen_line_segment(device, pass, tl, tr, w, color);
            draw_screen_line_segment(device, pass, tl, bl, w, color);
            draw_screen_line_segment(device, pass, tr, br, w, color);
            draw_screen_line_segment(device, pass, bl, br, w, color);
            draw_screen_line_segment(device, pass, bl, tip, w, color);
            draw_screen_line_segment(device, pass, br, tip, w, color);
        }
        Tool::Select => {
            let p0 = [cx - s * 0.4, cy - s * 0.7];
            let p1 = [cx + s * 0.6, cy + s * 0.4];
            let p2 = [cx, cy + s * 0.2];
            let p3 = [cx - s * 0.4, cy + s * 0.8];
            draw_screen_line_segment(device, pass, p0, p1, w, color);
            draw_screen_line_segment(device, pass, p1, p2, w, color);
            draw_screen_line_segment(device, pass, p2, p3, w, color);
            draw_screen_line_segment(device, pass, p3, p0, w, color);
        }
        Tool::Eraser => {} // 지우개는 기존 점선 원 인디케이터만 사용, 아이콘 커서 없음
    }
}


fn draw_ui_circle(device: &wgpu::Device, pass: &mut wgpu::RenderPass, center: [f32; 2], radius: f32, color: [f32; 4]) {
    let segments = 16;
    let mut verts = Vec::with_capacity(segments * 3);
    for i in 0..segments {
        let a0 = i as f32 / segments as f32 * std::f32::consts::TAU;
        let a1 = (i + 1) as f32 / segments as f32 * std::f32::consts::TAU;
        verts.push(Vertex { pos: center });
        verts.push(Vertex { pos: [center[0] + radius * a0.cos(), center[1] + radius * a0.sin()] });
        verts.push(Vertex { pos: [center[0] + radius * a1.cos(), center[1] + radius * a1.sin()] });
    }
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ui_circle_vbuf"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let immediate = crate::render::pipeline::DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color };
    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
    pass.set_vertex_buffer(0, vbuf.slice(..));
    pass.draw(0..(segments as u32 * 3), 0..1);
}