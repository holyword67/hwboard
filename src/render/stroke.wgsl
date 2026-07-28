struct GlobalUniforms {
    zoom: f32,
    viewport_w: f32,
    viewport_h: f32,
    _pad: f32,
};
@group(0) @binding(0) var<uniform> globals: GlobalUniforms;

struct DrawImmediate {
    offset: vec2<f32>,
    _pad: vec2<f32>,
    color: vec4<f32>,
};
var<immediate> im: DrawImmediate;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
};

@vertex
fn vs_main(@location(0) local_pos: vec2<f32>) -> VertexOutput {
    let cam_relative = local_pos + im.offset;
    let screen = cam_relative * globals.zoom + vec2<f32>(globals.viewport_w, globals.viewport_h) * 0.5;

    let ndc_x = screen.x / globals.viewport_w * 2.0 - 1.0;
    let ndc_y = 1.0 - screen.y / globals.viewport_h * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    return out;
}

// UI 오버레이용 — 카메라 변환(zoom/오프셋) 없이 스크린 픽셀 좌표를
// 그대로 NDC로 변환만 함. local_pos에 이미 절대 스크린 좌표가 들어옴.
@vertex
fn vs_ui_main(@location(0) local_pos: vec2<f32>) -> VertexOutput {
    let ndc_x = local_pos.x / globals.viewport_w * 2.0 - 1.0;
    let ndc_y = 1.0 - local_pos.y / globals.viewport_h * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return im.color;
}