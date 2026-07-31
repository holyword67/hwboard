// ============================================================
// src/app/render/ui_cache.rs
// ============================================================
// 도구함(버튼/팔레트/두께바) 캐시 — tool/color/thickness/뷰포트 크기가
// 바뀔 때(app/mod.rs의 ui_dirty)만 재조립. 지우개 커서/선택 핸들
// (cursor.rs)은 이 캐시를 전혀 모름 — 완전히 독립된 시스템.

use crate::app::{App, Tool};
use crate::render::pipeline::{DrawImmediate, Vertex};
use crate::ui;
use wgpu::util::DeviceExt;

pub(in crate::app) struct UiCache {
    vertex_buf: wgpu::Buffer,
    entries: Vec<UiDrawEntry>,
}

struct UiDrawEntry {
    start: u32,
    count: u32,
    color: [f32; 4],
}

impl UiCache {
    /// 캐시된 버퍼를 그대로 재생 — draw_* 계열처럼 매 프레임 새로 만들지
    /// 않고, 이미 구워둔 정점/구간만 순회하며 그림.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass) {
        pass.set_vertex_buffer(0, self.vertex_buf.slice(..));
        for e in &self.entries {
            let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color: e.color };
            pass.set_immediates(0, bytemuck::bytes_of(&immediate));
            pass.draw(e.start..e.start + e.count, 0..1);
        }
    }
}

impl App {
    /// 도구함 정점을 하나의 버퍼로 통째로 재조립. ui_dirty일 때만
    /// 호출됨(app/render/mod.rs::render() 참고).
    pub(super) fn build_ui_cache(&mut self) {
        let mut vertices: Vec<Vertex> = Vec::new();
        let mut entries: Vec<UiDrawEntry> = Vec::new();

        let buttons = ui::layout(self.camera.viewport_size, self.tool, self.pen_color, self.pen_width);
        for b in &buttons {
            match b.kind {
                ui::ButtonKind::Color(c) => {
                    let c = self.boosted(c);
                    push_quad(&mut vertices, &mut entries, b.rect, c);
                    if b.selected {
                        let cx = b.rect.x + b.rect.w * 0.5;
                        let cy = b.rect.y - 6.0;
                        push_triangle_inverted(&mut vertices, &mut entries, [cx, cy], 8.0, c);
                    }
                }
                ui::ButtonKind::ThicknessBar { selected_index } => {
                    push_thickness_bar(&mut vertices, &mut entries, b.rect, selected_index, self.boosted(self.pen_color));
                }
                ui::ButtonKind::Tool(tool) => {
                    let color = if b.selected { self.boosted(self.pen_color) } else { [0.6, 0.6, 0.6, 1.0] };
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

// ---- 아래는 즉석 draw 대신 vertices/entries에 데이터만 쌓는 push_* ----
// (cursor.rs의 draw_* 계열과 형태 로직은 겹치는 부분도 있지만, "버퍼에
// 쌓는다" vs "매 프레임 즉석 draw한다"는 의도적으로 분리된 별개 경로.)

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

    let start_x = x0 + selected_index as f32 * step;
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
    let count = vertices.len() as u32 - start;
    if count > 0 {
        entries.push(UiDrawEntry { start, count, color: fill_color });
    }

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

    for i in 1..5 {
        let lx = x0 + i as f32 * step;
        let r = get_r(lx);
        push_line_segment(vertices, entries, [lx, cy - r], [lx, cy + r], outline_w, outline_color);
    }
}