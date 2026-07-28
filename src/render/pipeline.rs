// ============================================================
// src/render/pipeline.rs
// ============================================================
// 스트로크(및 향후 도형/텍스트/이미지) 렌더 파이프라인. global uniform
// (zoom+viewport, bind group 0) + per-draw immediate(카메라 오프셋+색상)
// 조합.
//
// ⚠️ [미검증] PipelineLayoutDescriptor/RenderPipelineDescriptor의 정확한
// 필드 구성은 wgpu 29 문서 기준으로 짰지만 실제 컴파일해서 확인 안 함 —
// 처음 빌드할 때 필드 불일치 나면 그 자리에서 고치면 됨. immediate_size
// 필드명/RenderPass::set_immediates는 wgpu 28+ 공식 API로 확인됨.

use crate::gpu::core::GpuCore;

pub const IMMEDIATE_SIZE: u32 = 32;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct GlobalUniforms {
    pub zoom: f32,
    pub viewport_w: f32,
    pub viewport_h: f32,
    pub _pad: f32,
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct DrawImmediate {
    pub offset: [f32; 2], // (stroke.origin - camera.center)를 f64로 계산 후 f32 캐스팅한 값
    pub _pad: [f32; 2],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct Vertex {
    pub pos: [f32; 2],
}

pub struct StrokePipeline {
    pub pipeline: wgpu::RenderPipeline,
    pub global_bind_group: wgpu::BindGroup,
    /// UiPipeline이 같은 레이아웃으로 파이프라인을 만들 때 재사용 —
    /// 같은 BindGroupLayout 객체를 공유하면 global_bind_group을 두
    /// 파이프라인 모두에서 그대로 쓸 수 있음(중복 생성 불필요).
    pub global_bgl: wgpu::BindGroupLayout,
}

impl StrokePipeline {
    pub fn new(core: &GpuCore) -> Self {
        let shader = core.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("stroke_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stroke.wgsl").into()),
        });

        let global_bgl = core.device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("global_uniforms_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let global_bind_group = core.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("global_uniforms_bg"),
            layout: &global_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: core.global_uniform_buf.as_entire_binding(),
            }],
        });

        let layout = core.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("stroke_pipeline_layout"),
            bind_group_layouts: &[Some(&global_bgl)],
            immediate_size: IMMEDIATE_SIZE,
        });

        let pipeline = core.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("stroke_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<Vertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[wgpu::VertexAttribute {
                        offset: 0,
                        shader_location: 0,
                        format: wgpu::VertexFormat::Float32x2,
                    }],
                }],
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fs_main"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: core.config.format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self { pipeline, global_bind_group, global_bgl }
    }
}