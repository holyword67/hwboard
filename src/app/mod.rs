// ============================================================
// src/app/mod.rs
// ============================================================
mod clipboard_paste;
mod pointer;
mod render;
mod shapes;

use crate::gpu::core::GpuCore;
use crate::input::{InputEvent, InputState};
use crate::render::camera::Camera;
use crate::render::gpu_resources::GpuResourceRegistry;
use crate::render::image_pipeline::ImagePipeline;
use crate::render::pipeline::StrokePipeline;
use crate::render::ui_pipeline::UiPipeline;
use crate::scene::{CanvasItem, ItemId, Scene, Stroke, UndoStack};
use crate::ui;
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::video::Window;
use std::time::Instant;

use shapes::SnapData;

const SAMPLE_COUNT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
    Pen,
    Eraser,
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
    erasing_removed: Vec<(ItemId, CanvasItem, usize)>,
    eraser_pressed: bool,
    pointer_captured_by_ui: bool,
    panning: bool,
    last_pan_pos: [f32; 2],
    is_fullscreen: bool,
    open: bool,
    snap_state: Option<SnapData>,
    msaa_texture_view: wgpu::TextureView,
    /// 상태 변화(이벤트 발생 또는 Hold 등 poll발 변화)가 있었으면 true.
    /// render_if_needed()가 이 값을 보고 실제로 그릴지 결정하고, 그리고
    /// 나면 false로 리셋함.
    dirty: bool,
    /// 알트탭 등으로 창이 백그라운드로 밀려나면 false. GPU 렌더링을
    /// 완전히 건너뛰는 데 씀(비활성 창을 계속 그릴 이유가 없음).
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
            erasing_removed: Vec::new(),
            eraser_pressed: false,
            pointer_captured_by_ui: false,
            is_fullscreen: false,
            open: true,
            panning: false, last_pan_pos: [0.0, 0.0],
            snap_state: None,
            msaa_texture_view,
            dirty: true,       // 첫 프레임은 반드시 그려야 함
            has_focus: true,   // [가정값] 창이 뜨는 시점엔 포커스가 있다고 가정
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

    pub fn handle_sdl_event(&mut self, event: &Event, window: &mut Window) {
        // 어떤 종류든 SDL 이벤트가 왔다는 것 자체가 "화면이 바뀔 수도
        // 있다"는 신호 — 개별 mutation 지점마다 dirty를 심는 대신 여기
        // 한 곳에서 일괄 처리(빠뜨릴 여지를 원천 차단).
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
            // 알트탭 등으로 포커스가 오가는 순간 — has_focus 갱신.
            // 이벤트 자체가 이미 dirty=true를 세웠으니, 포커스 되찾는
            // 순간 바로 한 프레임 그려짐(백그라운드에서 밀린 동안 쌓인
            // 변화가 있었다면 그걸 반영).
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

    /// 매 프레임 호출 — hold(도형 자동스냅) 폴링. 이건 SDL 이벤트가
    /// 아니라 시간 경과로 발생하는 변화라 handle_sdl_event의 일괄
    /// dirty 처리에 안 걸림 — 여기서 따로 잡아줘야 함.
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