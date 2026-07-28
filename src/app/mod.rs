// ============================================================
// src/app/mod.rs
// ============================================================
mod clipboard_paste;
mod pointer;
mod render;
mod select;
mod shapes;

use crate::gpu::core::GpuCore;
use crate::input::{InputEvent, InputState};
use crate::render::camera::Camera;
use crate::render::gpu_resources::GpuResourceRegistry;
use crate::render::image_pipeline::ImagePipeline;
use crate::render::pipeline::StrokePipeline;
use crate::render::ui_pipeline::UiPipeline;
use crate::scene::{CanvasItem, ItemId, Scene, Shape, Stroke, UndoStack};
use crate::ui;
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::video::Window;
use std::time::Instant;

use select::SelectDrag;
use shapes::SnapData;

const SAMPLE_COUNT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
    Pen,
    Eraser,
    Select,
}

pub struct App {
    core: GpuCore,
    pipeline: StrokePipeline,
    ui_pipeline: UiPipeline,
    image_pipeline: ImagePipeline,
    registry: GpuResourceRegistry,
    scene: Scene,
    undo_stack: UndoStack,
    camera: Camera,
    input: InputState,
    tool: Tool,
    pen_color: [f32; 4],
    drawing_stroke: Option<Stroke>,
    /// Hold로 도형 인식이 성공하면 drawing_stroke 대신 여기로 옮겨져서
    /// 라이브 리사이즈/회전 프리뷰가 됨. Up 시점에 이게 Some이면 이걸
    /// CanvasItem::Shape로 커밋하고, None이면 drawing_stroke를 그대로
    /// Stroke로 커밋(자유필기).
    drawing_shape_preview: Option<Shape>,
    erasing_removed: Vec<(ItemId, CanvasItem, usize)>,
    eraser_pressed: bool,
    /// Tool::Select에서 현재 선택된 아이템.
    selected_item: Option<ItemId>,
    /// Tool::Select에서 진행 중인 드래그(이동/리사이즈/회전) 상태.
    select_drag: Option<SelectDrag>,
    pointer_captured_by_ui: bool,
    panning: bool,
    last_pan_pos: [f32; 2],
    is_fullscreen: bool,
    open: bool,
    snap_state: Option<SnapData>,
    msaa_texture_view: wgpu::TextureView,
    dirty: bool,
    has_focus: bool,
}

impl App {
    pub async fn new(window: &Window) -> Self {
        let core = GpuCore::new(window).await;
        let msaa_texture_view = create_msaa_texture_view(&core.device, &core.config);
        let pipeline = StrokePipeline::new(&core);
        let ui_pipeline = UiPipeline::new(&core, &pipeline.global_bgl);
        let image_pipeline = ImagePipeline::new(&core, &pipeline.global_bgl);
        let (w, h) = window.size();
        Self {
            core,
            pipeline,
            ui_pipeline,
            image_pipeline,
            registry: GpuResourceRegistry::new(),
            scene: Scene::new(),
            undo_stack: UndoStack::new(),
            camera: Camera::new([w as f32, h as f32]),
            input: InputState::new(),
            tool: Tool::Pen,
            pen_color: ui::PALETTE[0],
            drawing_stroke: None,
            drawing_shape_preview: None,
            erasing_removed: Vec::new(),
            eraser_pressed: false,
            selected_item: None,
            select_drag: None,
            pointer_captured_by_ui: false,
            is_fullscreen: false,
            open: true,
            panning: false, last_pan_pos: [0.0, 0.0],
            snap_state: None,
            msaa_texture_view,
            dirty: true,
            has_focus: true,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub fn wants_frequent_wake(&self) -> bool {
        self.input.is_pen_down()
    }

    pub fn has_focus(&self) -> bool {
        self.has_focus
    }

    pub fn handle_sdl_event(&mut self, event: &Event, window: &mut Window) {
        self.dirty = true;

        match event {
            Event::Quit { .. } => self.open = false,
            Event::Window {
                win_event: sdl3::event::WindowEvent::PixelSizeChanged(w, h)
                    | sdl3::event::WindowEvent::Resized(w, h),
                ..
            } => {
                self.core.resize(*w as u32, *h as u32);
                self.camera.resize([*w as f32, *h as f32]);
                self.msaa_texture_view = create_msaa_texture_view(&self.core.device, &self.core.config);
            }
            Event::Window { win_event: sdl3::event::WindowEvent::FocusGained, .. } => {
                self.has_focus = true;
            }
            Event::Window { win_event: sdl3::event::WindowEvent::FocusLost, .. } => {
                self.has_focus = false;
            }
            Event::KeyDown { keycode: Some(kc), keymod, repeat: false, .. } => {
                self.handle_key(*kc, *keymod, window)
            }
            Event::MouseWheel { y, mouse_x, mouse_y, .. } => {
                let factor = 1.0 + y * 0.1;
                self.camera.zoom_at([*mouse_x, *mouse_y], factor);
            }
            _ => {
                if let Some(input_event) = self.input.process_event(event) {
                    self.handle_input_event(input_event);
                }
            }
        }
    }

    fn handle_key(&mut self, kc: Keycode, keymod: Mod, window: &mut Window) {
        let ctrl = keymod.contains(Mod::LCTRLMOD) || keymod.contains(Mod::RCTRLMOD);
        match kc {
            Keycode::Backspace => self.undo_stack.undo(&mut self.scene),
            Keycode::Equals => self.undo_stack.redo(&mut self.scene),
            Keycode::Return => {
                self.is_fullscreen = !self.is_fullscreen;
                let _ = window.set_fullscreen(self.is_fullscreen);
            }
            Keycode::V if ctrl => self.paste_image_from_clipboard(),
            _ => {}
        }
    }

    pub fn poll(&mut self) {
        if let Some(input_event) = self.input.update(Instant::now()) {
            self.dirty = true;
            self.handle_input_event(input_event);
        }
    }

    fn handle_input_event(&mut self, event: InputEvent) {
        match event {
            InputEvent::Pointer(p) => self.handle_pointer(p),
            InputEvent::MouseSideButton { button: sdl3::mouse::MouseButton::X1, pressed: true } => {
                self.undo_stack.undo(&mut self.scene);
            }
            InputEvent::MouseSideButton { button: sdl3::mouse::MouseButton::X2, pressed: true } => {
                self.undo_stack.redo(&mut self.scene);
            }
            InputEvent::MouseSideButton { .. } => {}
        }
    }
}

fn create_msaa_texture_view(device: &wgpu::Device, config: &wgpu::SurfaceConfiguration) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("msaa_texture"),
        size: wgpu::Extent3d {
            width: config.width,
            height: config.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: SAMPLE_COUNT,
        dimension: wgpu::TextureDimension::D2,
        format: config.format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}