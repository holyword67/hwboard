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
    color: vec4<f32>, // 틴트, 기본은 흰색(원본 그대로)
};
var<immediate> im: DrawImmediate;

@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@location(0) local_pos: vec2<f32>, @location(1) uv: vec2<f32>) -> VertexOutput {
    let cam_relative = local_pos + im.offset;
    let screen = cam_relative * globals.zoom + vec2<f32>(globals.viewport_w, globals.viewport_h) * 0.5;
    let ndc_x = screen.x / globals.viewport_w * 2.0 - 1.0;
    let ndc_y = 1.0 - screen.y / globals.viewport_h * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = uv;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, in.uv) * im.color;
}