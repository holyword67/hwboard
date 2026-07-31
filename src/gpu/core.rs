// ============================================================
// src/gpu/core.rs
// ============================================================
use crate::gpu::window_handle::OwnedWindowHandle;
use crate::render::pipeline::GlobalUniforms;
use sdl3::video::Window;

pub struct GpuCore {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub global_uniform_buf: wgpu::Buffer,
    /// [실측 계산값, 2026.7.31] 시작 시점 1회 조회한 Windows HDR
    /// 헤드룸(풀프레임 지속 밝기 / SDR 흰색 기준) 기반 색상 부스트
    /// 배율. HDR 비활성/정보 없음이면 1.0(무변화). 설계 합의: 세션
    /// 내내 재조회 안 함 — Windows 밝기 설정이 실시간으로 바뀌어도
    /// 다음 실행부터 반영됨.
    pub color_boost: f32,
}

impl GpuCore {
    pub async fn new(window: &Window) -> Self {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: Default::default(),
            memory_budget_thresholds: Default::default(),
            backend_options: Default::default(),
            display: None,
        });

        let surface = instance
            .create_surface(OwnedWindowHandle::new(window).unwrap())
            .unwrap();

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
                apply_limit_buckets: false,
            })
            .await
            .unwrap();

        let required_features = wgpu::Features::IMMEDIATES;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                required_features,
                required_limits: adapter.limits(),
                memory_hints: wgpu::MemoryHints::Performance,
                ..Default::default()
            })
            .await
            .unwrap();

        let (width, height) = window.size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            // [실측 확인됨, 2026.7.31] 이 환경은 Rgba16Float +
            // ExtendedSrgbLinear(scRGB)를 지원함(format_capabilities로
            // 확인). 8비트 *Srgb 포맷으로는 SDR 압축 경로를 못 벗어나서
            // Windows HDR 데스크톱에서 흰색이 눌려 보이던 문제의 원인이었음.
            format: wgpu::TextureFormat::Rgba16Float,
            color_space: wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
            width,
            height,
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface.get_capabilities(&adapter).alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let color_boost = compute_color_boost(&surface.display_hdr_info(&adapter));
        println!("[HDR] color_boost = {color_boost:.3}");

        let global_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("global_uniforms"),
            size: std::mem::size_of::<GlobalUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self { surface, device, queue, config, global_uniform_buf, color_boost }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}

/// [실측 계산 방식, 2026.7.31] Windows HDR 헤드룸 기반 색상 부스트
/// 배율. `max_nits`(순간 최대)가 아니라 `max_full_frame_nits`(지속
/// 가능 밝기)를 씀 — 배경/펜이 화면 전체를 채우는 "풀프레임" 케이스라
/// wgpu 기본 제공 tone_map_headroom()(내부적으로 max_nits 사용)은
/// 우리 케이스엔 안 맞아서 직접 계산.
fn compute_color_boost(info: &wgpu::DisplayHdrInfo) -> f32 {
    const SAFETY_MARGIN: f32 = 0.9; // [미검증 가설] 실사용 후 조정 대상

    if info.coarse.and_then(|c| c.high_dynamic_range) == Some(false) {
        return 1.0; // 확정 SDR 디스플레이 — 손대지 않음
    }
    let Some(lum) = info.luminance else { return 1.0 };
    let (Some(peak), Some(white)) = (lum.max_full_frame_nits.or(lum.max_nits), lum.sdr_white_nits) else {
        return 1.0;
    };
    if white <= 0.0 {
        return 1.0;
    }
    let ratio = (peak / white).max(1.0);
    // 마진을 "1.0 넘는 부분"에만 적용 — 계산 결과가 절대 1.0 밑으로
    // 안 내려가게(=절대 어둡게는 안 만듦) 보장.
    1.0 + (ratio - 1.0) * SAFETY_MARGIN
}