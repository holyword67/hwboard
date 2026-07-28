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
            // [이어받음] doxa2가 DX12로 고정했던 걸 그대로 유지 —
            // Windows 타겟이 이미 전제된 선택이라 새로 정한 게 아님.
            // 크로스플랫폼 필요해지면 이 부분 재검토 필요.
            backends: wgpu::Backends::DX12,
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
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format: surface.get_capabilities(&adapter).formats[0],
            width,
            height,
            // [결정 필요할 수도] doxa2는 Immediate(vsync 없음, 지연 최소화
            // 우선)였는데, 화이트보드는 애니메이션이 거의 없고 정적인
            // 화면이 대부분이라 Fifo(vsync)로 바꿨어 — 티어링 방지 +
            // 불필요한 GPU 사용 감소. 이견 있으면 알려줘.
            present_mode: wgpu::PresentMode::Fifo,
            alpha_mode: surface.get_capabilities(&adapter).alpha_modes[0],
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