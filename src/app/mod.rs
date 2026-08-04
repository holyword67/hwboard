// ============================================================
// src/app/mod.rs
// ============================================================
mod clipboard_paste;
mod pointer;
mod render;
mod select;
pub mod shapes;

use crate::gpu::core::GpuCore;
use crate::input::{InputEvent, InputState};
use crate::render::camera::Camera;
use crate::render::gpu_resources::GpuResourceRegistry;
use crate::render::image_pipeline::ImagePipeline;
use crate::render::pipeline::StrokePipeline;
use crate::render::ui_pipeline::UiPipeline;
use crate::journal;
use crate::scene::{
    CanvasItem, ClearAll, Command, ItemId, PenPoint, Scene, Shape, UndoStack,
};
use std::thread::JoinHandle;
use crate::ui;
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::mouse::MouseUtil;
use sdl3::video::Window;
use std::time::Instant;

use crate::render::tessellate::IncrementalStrokeMesh;
use render::{LiveStrokeGpu, OverlayBuilder, OverlayGpu, UiCache};
use select::SelectDrag;
use shapes::SnapData;

const SAMPLE_COUNT: u32 = 4;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
    Pen,
    Eraser,
    Select,
}

/// 그리는 중인 자유획의 임시 상태(커밋 전). Stroke와 달리 points가
/// 순수 Vec라 push 가능 — anchor 확정(로컬화)과 bbox 캐싱은 Up 시점에
/// Stroke::new()로 커밋할 때만 1회 발생.
struct DrawingStroke {
    points: Vec<PenPoint>,
    color: [f32; 4],
    base_width: f32,
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
    pen_width: f32,
    drawing_stroke: Option<DrawingStroke>,
    /// Hold로 도형 인식이 성공하면 drawing_stroke 대신 여기로 옮겨져서
    /// 라이브 리사이즈/회전 프리뷰가 됨. Up 시점에 이게 Some이면 이걸
    /// CanvasItem::Shape로 커밋하고, None이면 drawing_stroke를 그대로
    /// Stroke로 커밋(자유필기).
    drawing_shape_preview: Option<Shape>,
    /// 그리는 중인 자유획의 점진적 테셀레이션 캐시(CPU) — pointer.rs가
    /// 점을 push할 때마다 같이 append됨. render()는 이걸 읽기만 함.
    drawing_mesh_cache: Option<IncrementalStrokeMesh>,
    /// 자유획 점 디시메이션 판정용 — 마지막으로 실제 채택된 점의
    /// 스크린 좌표.
    drawing_stroke_last_screen_pos: Option<[f32; 2]>,
    /// 경로 스무딩용 슬라이딩 윈도우(1점 지연) — pointer.rs::feed_smoother 참고.
    smoother_prev2: Option<[f64; 2]>,
    smoother_prev1: Option<PenPoint>,
    smoother_prev1_pending: bool,
    /// 리본 지오메트리용 2단계 접선 지연 버퍼 — pointer.rs::feed_geometry_stage 참고.
    geom_prev_pos: Option<[f64; 2]>,
    geom_pending: Option<PenPoint>,
    erasing_removed: Vec<(ItemId, CanvasItem, usize)>,
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
    ui_dirty: bool,
    ui_cache: Option<UiCache>,
    /// 그리는 중인 자유획 전용 growable GPU 버퍼 — 스트로크가 끝나도
    /// 안 버리고 재사용(다음 획에서 synced 카운터만 리셋).
    live_stroke_gpu: Option<LiveStrokeGpu>,
    /// 지우개 인디케이터/선택 오버레이/커스텀 커서 전용 growable 버퍼(C) —
    /// live_stroke_gpu와 마찬가지로 세션 내내 재사용, 매 프레임 내용만 갱신.
    overlay_gpu: Option<OverlayGpu>,
    /// 오버레이 CPU 빌더 — 매 프레임 clear()만 하고 Vec capacity는 재사용.
    overlay: OverlayBuilder,
    /// 저장 스레드 핸들 — Quit 시점에 join해서 "정상 종료는 유실 없음"을 보장.
    journal_thread: Option<JoinHandle<()>>,
    mouse: MouseUtil,
}

impl App {
    pub async fn new(window: &Window, sdl_context: &sdl3::Sdl) -> Self {
        let core = GpuCore::new(window).await;
        let msaa_texture_view = create_msaa_texture_view(&core.device, &core.config);
        let pipeline = StrokePipeline::new(&core);
        let ui_pipeline = UiPipeline::new(&core, &pipeline.global_bgl);
        let image_pipeline = ImagePipeline::new(&core, &pipeline.global_bgl);
        let (w, h) = window.size();
        let mouse = sdl_context.mouse();
        mouse.show_cursor(false); // 보드는 창 전체 — 시작 시점부터 OS 커서 숨김(커스텀 커서로 대체)
        let journal_path = journal::journal_path();
        let (scene, mut undo_stack) = journal::replay(&journal_path);
        let (journal_tx, journal_thread) = journal::spawn(journal_path);
        undo_stack.set_journal_tx(journal_tx);

        Self {
            core,
            pipeline,
            ui_pipeline,
            image_pipeline,
            registry: GpuResourceRegistry::new(),
            scene,
            undo_stack,
            camera: Camera::new([w as f32, h as f32]),
            input: InputState::new(),
            tool: Tool::Pen,
            pen_color: ui::PALETTE[0],
            pen_width: ui::THICKNESS_LEVELS[1],
            drawing_stroke: None,
            drawing_shape_preview: None,
            drawing_mesh_cache: None,
            drawing_stroke_last_screen_pos: None,
            smoother_prev2: None,
            smoother_prev1: None,
            smoother_prev1_pending: false,
            geom_prev_pos: None,
            geom_pending: None,
            erasing_removed: Vec::new(),
            selected_item: None,
            select_drag: None,
            pointer_captured_by_ui: false,
            is_fullscreen: false,
            open: true,
            panning: false,
            last_pan_pos: [
                0.0, 0.0,
            ],
            snap_state: None,
            msaa_texture_view,
            dirty: true,
            has_focus: true,
            ui_dirty: true,
            ui_cache: None,
            live_stroke_gpu: None,
            overlay_gpu: None,
            overlay: OverlayBuilder::default(),
            journal_thread: Some(journal_thread),
            mouse,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_tool(&mut self, tool: Tool) {
        // Select를 벗어나면 선택 상태도 같이 리셋 — 다른 도구로 바꾼 뒤
        // Del 눌러서 옛날에 선택해뒀던 아이템이 지워지는 걸 방지.
        if tool != Tool::Select {
            self.selected_item = None;
            self.select_drag = None;
        }
        // ==========================================
        // [수정된 부분] 다른 도구로 변경될 때, 
        // 그리다 만 스트로크나 도형 프리뷰가 있다면 강제로 폐기합니다.
        // ==========================================
        if self.tool != tool {
            self.drawing_stroke = None;
            self.drawing_shape_preview = None;
            self.drawing_mesh_cache = None;
            self.smoother_prev2 = None;
            self.smoother_prev1 = None;
            self.smoother_prev1_pending = false;
            self.geom_prev_pos = None;
            self.geom_pending = None;
            self.snap_state = None;
        }

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
            Event::Quit { .. } => {
                // Sender drop → 채널 닫힘 → 저장 스레드가 큐에 남은 걸
                // 다 비우고 자연 종료 → join으로 기다렸다가 진짜 종료.
                // 이걸로 "정상 종료는 유실 없음"이 보장됨.
                self.undo_stack.close_journal();
                if let Some(handle) = self.journal_thread.take() {
                    let _ = handle.join();
                }
                self.open = false;
            }
            Event::Window {
                win_event:
                    sdl3::event::WindowEvent::PixelSizeChanged(w, h)
                    | sdl3::event::WindowEvent::Resized(w, h),
                ..
            } => {
                self.core.resize(*w as u32, *h as u32);
                self.camera.resize([
                    *w as f32, *h as f32,
                ]);
                self.msaa_texture_view =
                    create_msaa_texture_view(&self.core.device, &self.core.config);
                self.ui_dirty = true; // 도구함 레이아웃이 뷰포트 크기에 의존함
            }
            Event::Window {
                win_event: sdl3::event::WindowEvent::FocusGained,
                ..
            } => {
                self.has_focus = true;
            }
            Event::Window {
                win_event: sdl3::event::WindowEvent::FocusLost,
                ..
            } => {
                self.has_focus = false;
            }
            Event::Window {
                win_event: sdl3::event::WindowEvent::MouseEnter,
                ..
            } => {
                self.mouse.show_cursor(false); // 보드 안으로 진입 — OS 커서 숨기고 커스텀 커서로 대체
            }
            Event::Window {
                win_event: sdl3::event::WindowEvent::MouseLeave,
                ..
            } => {
                self.mouse.show_cursor(true); // 보드 밖으로 이탈 — OS 커서 원복
            }
            Event::KeyDown {
                keycode: Some(kc),
                keymod,
                repeat: false,
                ..
            } => self.handle_key(*kc, *keymod, window),
            Event::MouseWheel {
                y,
                mouse_x,
                mouse_y,
                ..
            } => {
                let factor = 1.0 + y * 0.1;
                self.camera.zoom_at(
                    [
                        *mouse_x, *mouse_y,
                    ],
                    factor,
                );
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
            Keycode::Delete => self.delete_selected_item(),
            Keycode::Escape => self.clear_all(),
            Keycode::Return => {
                self.is_fullscreen = !self.is_fullscreen;
                let _ = window.set_fullscreen(self.is_fullscreen);
            }
            Keycode::V if ctrl => self.paste_image_from_clipboard(),

            // 도구 선택: A=펜, S=지우개, D=선택기
            Keycode::A => {
                self.set_tool(Tool::Pen);
                self.ui_dirty = true;
            }
            Keycode::S => {
                self.set_tool(Tool::Eraser);
                self.ui_dirty = true;
            }
            Keycode::D => {
                self.set_tool(Tool::Select);
                self.ui_dirty = true;
            }

            // 컬러 팔레트: Q,W,E,R = ui::PALETTE[0..4] 순서 그대로
            Keycode::Q => {
                self.pen_color = ui::PALETTE[0];
                self.ui_dirty = true;
            }
            Keycode::W => {
                self.pen_color = ui::PALETTE[1];
                self.ui_dirty = true;
            }
            Keycode::E => {
                self.pen_color = ui::PALETTE[2];
                self.ui_dirty = true;
            }
            Keycode::R => {
                self.pen_color = ui::PALETTE[3];
                self.ui_dirty = true;
            }

            // 두께: 1~5 = ui::THICKNESS_LEVELS[0..5] 순서 그대로
            Keycode::_1 => {
                self.pen_width = ui::THICKNESS_LEVELS[0];
                self.ui_dirty = true;
            }
            Keycode::_2 => {
                self.pen_width = ui::THICKNESS_LEVELS[1];
                self.ui_dirty = true;
            }
            Keycode::_3 => {
                self.pen_width = ui::THICKNESS_LEVELS[2];
                self.ui_dirty = true;
            }
            Keycode::_4 => {
                self.pen_width = ui::THICKNESS_LEVELS[3];
                self.ui_dirty = true;
            }
            Keycode::_5 => {
                self.pen_width = ui::THICKNESS_LEVELS[4];
                self.ui_dirty = true;
            }

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
            InputEvent::MouseSideButton {
                button: sdl3::mouse::MouseButton::X1,
                pressed: true,
            } => {
                self.undo_stack.undo(&mut self.scene);
            }
            InputEvent::MouseSideButton {
                button: sdl3::mouse::MouseButton::X2,
                pressed: true,
            } => {
                self.undo_stack.redo(&mut self.scene);
            }
            InputEvent::MouseSideButton {
                ..
            } => {}
        }
    }

    fn clear_all(&mut self) {
        if self.scene.iter_ordered_with_id().next().is_none() {
            return;
        }
        let items: Vec<_> = self.scene.iter_ordered_with_id()
            .enumerate()
            .map(|(idx, (id, item))| (id, item.clone(), idx))
            .collect();
        let cmd = Command::ClearAll(ClearAll { items });
        self.undo_stack.execute(cmd, &mut self.scene);
    }

}

fn create_msaa_texture_view(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
) -> wgpu::TextureView {
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
