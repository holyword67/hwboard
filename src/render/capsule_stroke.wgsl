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
    @location(0) local_pos: vec2<f32>,
    @location(1) seg_a: vec2<f32>,
    @location(2) seg_b: vec2<f32>,
    @location(3) radii: vec2<f32>,
};

@vertex
fn vs_main(
    @location(0) local_pos: vec2<f32>,
    @location(1) seg_a: vec2<f32>,
    @location(2) seg_b: vec2<f32>,
    @location(3) radii: vec2<f32>,
) -> VertexOutput {
    let cam_relative = local_pos + im.offset;
    let screen = cam_relative * globals.zoom + vec2<f32>(globals.viewport_w, globals.viewport_h) * 0.5;
    let ndc_x = screen.x / globals.viewport_w * 2.0 - 1.0;
    let ndc_y = 1.0 - screen.y / globals.viewport_h * 2.0;

    var out: VertexOutput;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    // SDF 계산은 local_pos/seg_a/seg_b 공간(카메라 오프셋 적용 전)에서
    // 그대로 함 — 오프셋은 평행이동이라 거리 계산엔 영향 없음.
    out.local_pos = local_pos;
    out.seg_a = seg_a;
    out.seg_b = seg_b;
    out.radii = radii;
    return out;
}

/// 세그먼트(A-B, 반지름 rA/rB)에 대한 "테이퍼드 캡슐" 거리 근사값.
/// 정확한 콘(cone) SDF는 아니고, 세그먼트 위 최근접점 파라미터 t를
/// (반지름 무시하고) 구한 뒤 그 지점에서 반지름을 선형보간해서 판정하는
/// 근사 — 수학/도식용 라인 렌더링엔 시각적으로 충분한 수준.
fn capsule_sdf(p: vec2<f32>, a: vec2<f32>, b: vec2<f32>, ra: f32, rb: f32) -> f32 {
    let ab = b - a;
    let len_sq = dot(ab, ab);

    if (len_sq < 1e-8) {
        // 시작점(dot) — 세그먼트 길이 0, 그냥 원.
        return length(p - a) - rb;
    }

    let len = sqrt(len_sq);
    let dir = ab / len;
    let rel = p - a;
    let along = dot(rel, dir);

    if (along <= 0.0) {
        // A 이전 구간 — flat cap(평평하게 자름). 이 세그먼트의 A는
        // 항상 "직전 세그먼트의 B(라운드 캡)"와 같은 점이라, 거기서
        // 이미 원을 그리고 있음 — 여기서 또 그리면 관절에서 AA가 두 번
        // 겹쳐 블렌딩되면서 규칙적인 돌기로 보임(실측으로 확인됨).
        // 그래서 A쪽은 평평하게 잘라서 중복 커버리지를 없앰.
        let perp = rel - along * dir;
        let perp_excess = max(length(perp) - ra, 0.0);
        return length(vec2<f32>(along, perp_excess));
    }

    let t = clamp(along / len, 0.0, 1.0);
    let closest = a + ab * t;
    let r = mix(ra, rb, t);
    return length(p - closest) - r;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let dist_local = capsule_sdf(in.local_pos, in.seg_a, in.seg_b, in.radii.x, in.radii.y);
    // local->screen 변환은 균등 스케일(zoom)+평행이동뿐이라, 거리에
    // zoom만 곱하면 스크린 픽셀 단위 거리가 됨(fwidth 불필요).
    let dist_screen = dist_local * globals.zoom;

    let aa_px = 1.0; // [미검증 가설] 전환 폭 1px — 계단지거나 너무 무디면 조정
    let edge = smoothstep(-aa_px, aa_px, dist_screen);
    let alpha = 1.0 - edge;
    if (alpha <= 0.0) {
        discard;
    }
    return vec4<f32>(im.color.rgb, im.color.a * alpha);
}


fn hash_color(p: vec2<f32>) -> vec3<f32> {
    let h = fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let h2 = fract(sin(dot(p, vec2<f32>(93.9898, 67.345))) * 24634.6345);
    let h3 = fract(sin(dot(p, vec2<f32>(41.9898, 289.233))) * 15731.7431);
    return vec3<f32>(h, h2, h3);
}