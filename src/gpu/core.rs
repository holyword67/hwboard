// ============================================================
// src/gpu/core.rs
// ============================================================
// doxa2 render_state/core.rs 재활용 — instance/adapter/device/queue/
// surface 생성 흐름은 그대로. FLOAT32_FILTERABLE(지형 텍스처 필터링용)은
// 제거, IMMEDIATES는 유지하되 용도를 "LOD 모프 팩터"에서 "스트로크
// 카메라 오프셋+색상"으로 교체. camera_uniform_buf도 3D CameraUniforms
// 대신 우리 GlobalUniforms(zoom+viewport) 크기로 새로 만듦.

use crate::gpu::window_handle::OwnedWindowHandle;
use crate::render::pipeline::GlobalUniforms;
use sdl3::video::Window;

pub struct GpuCore {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub global_uniform_buf: wgpu::Buffer,
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

        // 스트로크 draw call마다 카메라 오프셋+색상을 즉석으로 넘기기
        // 위해 필요. (doxa2에선 LOD 모프 팩터용이었던 걸 다른 목적으로
        // 재활용하는 것 — feature 자체는 같음)
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
let caps = surface.get_capabilities(&adapter);

// [실측 확인됨, 2026.7.31] 이 환경은 Rgba16Float+ExtendedSrgbLinear를
// 지원하고, Windows가 HDR active로 보고함(sdr_white 240nit,
// max_full_frame 343.75nit → 헤드룸 약 1.43x). Auto/8비트 포맷으로는
// SDR 압축 경로를 못 벗어나던 게 그레이보드의 원인이었음 — scRGB
// linear로 명시 전환해서 SDR 압축 경로 자체를 우회함.
// 셰이더 쪽 수정은 불필요: ExtendedSrgbLinear는 "1.0 = SDR 흰색"
// 기준의 선형 인코딩이라, 기존에 *Srgb 포맷에 쓰던 0.0~1.0 색상값을
// 그대로 둬도 동일한 밝기로 재현됨(하드웨어 sRGB 인코드가 스킵될
// 뿐, 값의 의미 자체는 유지).
let config = wgpu::SurfaceConfiguration {
    usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
    format: wgpu::TextureFormat::Rgba16Float,
    color_space: wgpu::SurfaceColorSpace::ExtendedSrgbLinear,
    width,
    height,
    present_mode: wgpu::PresentMode::Fifo,
    alpha_mode: caps.alpha_modes[0],
    view_formats: vec![],
    desired_maximum_frame_latency: 2,
};
        surface.configure(&device, &config);

        let global_uniform_buf = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("global_uniforms"),
            size: std::mem::size_of::<GlobalUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        Self { surface, device, queue, config, global_uniform_buf }
    }

    pub fn resize(&mut self, width: u32, height: u32) {
        if width > 0 && height > 0 {
            self.config.width = width;
            self.config.height = height;
            self.surface.configure(&self.device, &self.config);
        }
    }
}