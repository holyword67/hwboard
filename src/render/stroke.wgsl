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
    // origin 기준 로컬 정점 + 카메라 오프셋 = 카메라 상대 world 위치
    let cam_relative = local_pos + im.offset;
    let screen = cam_relative * globals.zoom + vec2<f32>(globals.viewport_w, globals.viewport_h) * 0.5;

    // 스크린 픽셀 -> NDC(-1~1), y축 반전(스크린은 아래로 갈수록 +y, NDC는 위로 갈수록 +y)
    let ndc_x = screen.x / globals.viewport_w * 2.0 - 1.0;
    let ndc_y = 1.0 - screen.y / globals.viewport_h * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    return out;
}

@fragment
fn fs_main() -> @location(0) vec4<f32> {
    return im.color;
}