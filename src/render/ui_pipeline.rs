// ============================================================
// src/render/ui_pipeline.rs
// ============================================================
// UI 오버레이(펜/지우개 토글, 색상 팔레트) 전용 파이프라인. stroke.wgsl의
// vs_ui_main(카메라 변환 없음) + fs_main(immediate 색상 그대로 출력)
// 조합. StrokePipeline이 만든 global_bgl을 그대로 재사용 — 같은 레이아웃
// 객체를 참조하므로 global_bind_group도 공유 가능.

use crate::gpu::core::GpuCore;
use crate::render::pipeline::{Vertex, IMMEDIATE_SIZE};

pub struct UiPipeline {
    pub pipeline: wgpu::RenderPipeline,
}

impl UiPipeline {
    pub fn new(core: &GpuCore, global_bgl: &wgpu::BindGroupLayout) -> Self {
        let shader = core.device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("ui_shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("stroke.wgsl").into()),
        });

        let layout = core.device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("ui_pipeline_layout"),
            bind_group_layouts: &[Some(global_bgl)],
            immediate_size: IMMEDIATE_SIZE,
        });

        let pipeline = core.device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("ui_pipeline"),
            layout: Some(&layout),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("vs_ui_main"),
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

        Self { pipeline }
    }
}