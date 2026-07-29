// ============================================================
// src/render/capsule_pipeline.rs
// ============================================================
// SDF(캡슐) 기반 스트로크 렌더링 전용 파이프라인. 세그먼트(인접 두 점)
// 하나당 quad 하나만 그리고, 프래그먼트 셰이더에서 캡슐 거리함수로
// 경계를 매끄럽게 처리 — 포인트마다 원을 스탬프하던 기존 stroke.wgsl
// 방식과 달리 라운드 조인이 셰이더 계산만으로 자동으로 나옴.
// UI 오버레이(ui_pipeline.rs)는 기존 stroke.wgsl/Vertex를 그대로 씀 —
// 이 파이프라인은 스트로크/도형 전용으로 완전히 분리됨.

use crate::gpu::core::GpuCore;
use crate::render::pipeline::IMMEDIATE_SIZE;

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct StrokeVertex {
    pub pos: [f32; 2],   // quad 꼭짓점의 로컬 좌표(캡슐을 넉넉히 감싸는 사각형)
    pub seg_a: [f32; 2], // 세그먼트 시작점 로컬좌표
    pub seg_b: [f32; 2], // 세그먼트 끝점 로컬좌표
    pub radii: [f32; 2], // (반지름_a, 반지름_b)
}

pub struct CapsulePipeline {
    pub pipeline: wgpu::RenderPipeline,
}

impl CapsulePipeline {
    pub fn new(core: &GpuCore, global_bgl: &wgpu::BindGroupLayout) -> Self {
        let shader = core.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("capsule_stroke_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("capsule_stroke.wgsl").into()),
        });

        let layout = core.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("capsule_pipeline_layout"),
            bind_group_layouts: &[Some(global_bgl)],
            immediate_size: IMMEDIATE_SIZE,
        });

        let pipeline = core.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("capsule_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_main"),
                compilation_options: Default::default(),
                buffers: &[wgpu::VertexBufferLayout {
                    array_stride: std::mem::size_of::<StrokeVertex>() as u64,
                    step_mode: wgpu::VertexStepMode::Vertex,
                    attributes: &[
                        wgpu::VertexAttribute { offset: 0, shader_location: 0, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 8, shader_location: 1, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 16, shader_location: 2, format: wgpu::VertexFormat::Float32x2 },
                        wgpu::VertexAttribute { offset: 24, shader_location: 3, format: wgpu::VertexFormat::Float32x2 },
                    ],
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
            multisample: wgpu::MultisampleState {
                // 같은 렌더패스(msaa_texture_view, SAMPLE_COUNT=4)를 공유하는
                // 다른 파이프라인들과 반드시 동일해야 함(요구사항, 취향 아님).
                count: 4,
                mask: !0,
                alpha_to_coverage_enabled: false,
            },
            multiview_mask: None,
            cache: None,
        });

        Self { pipeline }
    }
}