// ============================================================
// src/app/render/primitives.rs
// ============================================================
// 화면 좌표계(카메라 변환 없음) 즉석 드로우 기본 도형들. cursor.rs가
// 지우개 인디케이터/선택 핸들/커스텀 커서를 그릴 때 재사용한다.
// (참고: 도구함 캐시(ui_cache.rs)는 이 함수들을 안 씀 — 정점을 직접
// vertices Vec에 쌓는 push_* 계열을 따로 씀. 즉석 draw vs 캐시용 push는
// 의도적으로 분리된 두 경로.)

use crate::render::pipeline::{DrawImmediate, Vertex};
use crate::ui;
use wgpu::util::DeviceExt;

pub(super) fn draw_ui_quad(device: &wgpu::Device, pass: &mut wgpu::RenderPass, rect: ui::Rect, color: [f32; 4]) {
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

pub(super) fn draw_screen_line_segment(
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