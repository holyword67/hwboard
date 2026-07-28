// ============================================================
// src/app.rs
// ============================================================
// WgpuRenderer 대응 — 모든 조각(camera/input/scene/render)이 여기서
// 만난다. 메인 루프 오케스트레이터.

use crate::clipboard::read_clipboard_image_bytes;
use crate::gpu::core::GpuCore;
use crate::input::{InputEvent, InputState, PointerEvent, PointerSource};
use crate::render::camera::Camera;
use crate::render::gpu_resources::GpuResourceRegistry;
use crate::render::image_pipeline::ImagePipeline;
use crate::render::pipeline::{DrawImmediate, GlobalUniforms, StrokePipeline, Vertex};
use crate::render::tessellate::tessellate_stroke;
use crate::render::ui_pipeline::UiPipeline;
use crate::scene::{AddItem, CanvasItem, DeleteItems, ImageItem, ItemId, PenPoint, Scene, Stroke, UndoStack};
use crate::ui::{self, UiAction};
use sdl3::event::Event;
use sdl3::keyboard::{Keycode, Mod};
use sdl3::video::Window;
use std::sync::Arc;
use std::time::Instant;
use wgpu::util::DeviceExt;

const PEN_BASE_WIDTH: f32 = 3.0;
/// [미검증 가설] world 단위 고정 반경 — zoom에 따라 스크린상 크기가
/// 달라짐(확대하면 지우개가 스크린상 커 보임). 줌 불변으로 할지는
/// 실제 써보고 판단 필요.
const ERASER_RADIUS_WORLD: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Tool {
    Pen,
    Eraser,
}

pub struct SnapData {
    center: [f64; 2],
    local_points: Vec<[f64; 2]>, // 중심점 기준 상대 좌표 (회전/크기 조절의 원본)
    initial_pen: [f64; 2],       // 스냅된 순간의 펜 위치 (드래그 기준점)
    is_line: bool,               // 직선은 회전/크기 대신 한쪽 끝점만 고무줄처럼 따라가야 함
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
    pointer_captured_by_ui: bool,
    /// 마우스 좌클릭 드래그 = 팬(캔버스 이동). 펜은 이 상태와 무관하게
    /// 항상 그리기/지우기.
    panning: bool,
    last_pan_pos: [f32; 2],
    is_fullscreen: bool,
    open: bool,
    snap_state: Option<SnapData>,
}

impl App {
    pub async fn new(window: &Window) -> Self {
        let core = GpuCore::new(window).await;
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
            pointer_captured_by_ui: false,
            is_fullscreen: false,
            open: true,
            panning: false, last_pan_pos: [0.0, 0.0],
            snap_state: None,
        }
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn set_tool(&mut self, tool: Tool) {
        self.tool = tool;
    }

pub fn handle_sdl_event(&mut self, event: &Event, window: &mut Window) {
        match event {
            Event::Quit { .. } => self.open = false,
            Event::Window {
                win_event: sdl3::event::WindowEvent::PixelSizeChanged(w, h)
                    | sdl3::event::WindowEvent::Resized(w, h),
                ..
            } => {
                self.core.resize(*w as u32, *h as u32);
                self.camera.resize([*w as f32, *h as f32]);
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

    /// 키보드 단축키 — 언두/리두는 펜 barrel 버튼이랑 중복 할당(입력
    /// 수단 다를 뿐 같은 undo_stack을 씀). `repeat: false`로 걸러서
    /// 키 꾹 누르고 있을 때 undo가 프레임마다 연타되는 것 방지.
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

    fn paste_image_from_clipboard(&mut self) {
        let Some(bytes) = read_clipboard_image_bytes() else {
            eprintln!(
                "[paste] 클립보드에서 지원하는 이미지 형식(png/bmp/webp/jpeg/gif)을 찾지 못했습니다 — 다른 형식으로 복사해서 다시 시도해 주세요."
            );
            return;
        };
        let img = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(e) => {
                eprintln!("[paste] 이미지 디코딩 실패: {e}");
                return;
            }
        };
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();
        let cursor_world = self.camera.screen_to_world(self.input.last_mouse_pos());
        let top_left = [cursor_world[0] - w as f64 / 2.0, cursor_world[1] - h as f64 / 2.0];

        let id = self.scene.alloc_id();
        let item = CanvasItem::Image(ImageItem {
            top_left,
            size: [w as f64, h as f64],
            pixel_width: w,
            pixel_height: h,
            rgba: Arc::from(rgba.into_raw()),
        });
        let cmd = Box::new(AddItem { id, item });
        self.undo_stack.execute(cmd, &mut self.scene);
    }

    /// 매 프레임 호출 — hold(도형 자동스냅) 폴링.
    pub fn poll(&mut self) {
        if let Some(input_event) = self.input.update(Instant::now()) {
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

    fn handle_pointer(&mut self, ev: PointerEvent) {
        // UI 버튼 히트테스트가 항상 먼저 — 소스(펜/마우스) 상관없이 클릭
        // 이면 다 잡음.
        if let PointerEvent::Down(s) = ev {
            if let Some(action) = ui::hit_test(s.pos, self.camera.viewport_size, self.tool, self.pen_color) {
                self.pointer_captured_by_ui = true;
                match action {
                    UiAction::SelectTool(t) => self.tool = t,
                    UiAction::SelectColor(c) => self.pen_color = c,
                }
                return;
            }
        }
        if self.pointer_captured_by_ui {
            if let PointerEvent::Up(_) = ev {
                self.pointer_captured_by_ui = false;
            }
            return;
        }

        match ev.sample().source {
            PointerSource::Mouse => self.handle_mouse_pointer(ev),
            PointerSource::Pen => self.handle_pen_pointer(ev),
        }
    }

    /// 마우스 좌클릭 드래그 = 팬. 그리기/지우기는 관여하지 않음.
    fn handle_mouse_pointer(&mut self, ev: PointerEvent) {
        match ev {
            PointerEvent::Down(s) => {
                self.panning = true;
                self.last_pan_pos = s.pos;
            }
            PointerEvent::Move(s) => {
                if self.panning {
                    let delta = [s.pos[0] - self.last_pan_pos[0], s.pos[1] - self.last_pan_pos[1]];
                    self.camera.pan_by_screen_delta(delta);
                    self.last_pan_pos = s.pos;
                }
            }
            PointerEvent::Up(_) => self.panning = false,
            PointerEvent::Hold(_) => {}
        }
    }

fn handle_pen_pointer(&mut self, ev: PointerEvent) {
        match (self.tool, ev) {
            (Tool::Pen, PointerEvent::Down(s)) => {
                self.snap_state = None; // 새로 그릴 때 스냅 상태 초기화
                let world = self.camera.screen_to_world(s.pos);
                self.drawing_stroke = Some(Stroke {
                    points: vec![PenPoint { pos: world, pressure: s.pressure }],
                    color: self.pen_color,
                    base_width: PEN_BASE_WIDTH,
                    mesh_dirty: true,
                });
            }
            (Tool::Pen, PointerEvent::Move(s)) => {
                let world = self.camera.screen_to_world(s.pos);
                
                // 👇 [추가됨] 스냅 상태일 경우, 점을 추가하지 않고 도형 전체를 회전/크기 조절함
                if let Some(snap) = &self.snap_state {
                    if let Some(stroke) = &mut self.drawing_stroke {
                        if snap.is_line {
                            // 직선: 시작점은 고정, 끝점은 고무줄처럼 현재 펜 위치로
                            stroke.points[1].pos = world;
                        } else {
                            // 도형: 중심점 기준으로 Scale & Rotation 동시 적용
                            let mut dx0 = snap.initial_pen[0] - snap.center[0];
                            let mut dy0 = snap.initial_pen[1] - snap.center[1];
                            let dist0 = (dx0 * dx0 + dy0 * dy0).sqrt();
                            
                            // 스냅 순간에 중심점과 펜이 완전히 겹쳐 0으로 나누어지는 오류 방지
                            if dist0 < 1.0 { dx0 = 1.0; dy0 = 0.0; }
                            let safe_dist0 = dist0.max(1.0);
                            
                            let dx1 = world[0] - snap.center[0];
                            let dy1 = world[1] - snap.center[1];
                            let dist1 = (dx1 * dx1 + dy1 * dy1).sqrt();
                            
                            // 크기 변화율과 각도 변화량 계산
                            let scale = dist1 / safe_dist0;
                            let delta_angle = dy1.atan2(dx1) - dy0.atan2(dx0);
                            let cos_a = delta_angle.cos();
                            let sin_a = delta_angle.sin();
                            
                            // 미리 저장해둔 원본 상대좌표에 변환 행렬 적용
                            for (i, p) in stroke.points.iter_mut().enumerate() {
                                let lp = snap.local_points[i];
                                let sx = lp[0] * scale;
                                let sy = lp[1] * scale;
                                
                                let rx = sx * cos_a - sy * sin_a;
                                let ry = sx * sin_a + sy * cos_a;
                                
                                p.pos = [snap.center[0] + rx, snap.center[1] + ry];
                            }
                        }
                        stroke.mesh_dirty = true;
                    }
                    return; // 렌더링 끝냈으니 여기서 함수 종료
                }

                // 스냅 상태가 아닐 땐 평범하게 선 긋기
                if let Some(stroke) = &mut self.drawing_stroke {
                    stroke.points.push(PenPoint { pos: world, pressure: s.pressure });
                }
            }
            (Tool::Pen, PointerEvent::Up(_)) => {
                self.snap_state = None; // 그리기 완료 시 상태 해제
                if let Some(stroke) = self.drawing_stroke.take() {
                    if stroke.points.len() >= 2 {
                        let id = self.scene.alloc_id();
                        let cmd = Box::new(AddItem { id, item: CanvasItem::Stroke(stroke) });
                        self.undo_stack.execute(cmd, &mut self.scene);
                    }
                }
            }
            (Tool::Pen, PointerEvent::Hold(_)) => {
                if self.snap_state.is_some() { return; } // 이미 변환됐으면 무시
                if let Some(stroke) = &mut self.drawing_stroke {
                    // 👇 인식 성공 시 snap_state에 변환 데이터를 저장
                    if let Some(snap_data) = recognize_and_snap_shape(stroke) {
                        self.snap_state = Some(snap_data);
                    }
                }
            }
            (Tool::Eraser, PointerEvent::Down(s)) => {
                self.erasing_removed.clear();
                self.try_erase_at(s.pos);
            }
            (Tool::Eraser, PointerEvent::Move(s)) => {
                self.try_erase_at(s.pos);
            }
            (Tool::Eraser, PointerEvent::Up(_)) => {
                if !self.erasing_removed.is_empty() {
                    let cmd = Box::new(DeleteItems { removed: std::mem::take(&mut self.erasing_removed) });
                    self.undo_stack.push_already_applied(cmd);
                }
            }
            (_, PointerEvent::Hold(_)) => {
                // TODO: 도형 자동스냅 — 다음 논의 주제
            }
        }
    }

    /// [단순 구현] bounding box 기준 히트테스트. 스트로크 실제 선과의
    /// 거리가 아니라 사각형 범위라 정밀하지 않음 — 실사용해보고 부정확함이
    /// 느껴지면 세그먼트 거리 기반으로 교체 (지금은 가설 단계 최적화 보류).
    fn try_erase_at(&mut self, screen_pos: [f32; 2]) {
        let world = self.camera.screen_to_world(screen_pos);
        let r = ERASER_RADIUS_WORLD;

        // [변경됨] .rev()를 추가하여 맨 위(가장 나중에 그려진) 아이템부터 역순 검사.
        // 이를 통해 겹쳐진 선을 지울 때 아래에 있는 선이 잘못 지워지는 현상 방지.
        let hit = self.scene.iter_ordered_with_id_rev().find_map(|(id, item)| {
            // 이미 이번 드래그 세션에 지워진 항목은 무시
            if self.erasing_removed.iter().any(|(rid, _, _)| *rid == id) {
                return None;
            }
            
            // 정밀 히트테스트 호출
            item.hit_test(world, r).then_some(id)
        });

        if let Some(id) = hit {
            if let Some(item) = self.scene.item(id).cloned() {
                let z = self.scene.z_index_of(id).unwrap_or(0);
                self.scene.remove(id);
                self.erasing_removed.push((id, item, z));
            }
        }
    }

    pub fn render(&mut self) {

        let uniforms = GlobalUniforms {
            zoom: self.camera.zoom,
            viewport_w: self.camera.viewport_size[0],
            viewport_h: self.camera.viewport_size[1],
            _pad: 0.0,
        };
        self.core.queue.write_buffer(&self.core.global_uniform_buf, 0, bytemuck::bytes_of(&uniforms));

        let frame = match self.core.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t) => t,
            _ => {
                self.core.surface.configure(&self.core.device, &self.core.config);
                return;
            }
        };
        let view = frame.texture.create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder =
            self.core.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: None,
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    depth_slice: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            self.registry.sync(&self.core, &self.image_pipeline, &mut self.scene);

            pass.set_pipeline(&self.pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
            let mut on_stroke_pipeline = true;

            for (id, item) in self.scene.iter_ordered_with_id() {
                match item {
                    CanvasItem::Stroke(s) => {
                        let Some(res) = self.registry.get_stroke(id) else { continue };
                        if !on_stroke_pipeline {
                            pass.set_pipeline(&self.pipeline.pipeline);
                            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
                            on_stroke_pipeline = true;
                        }
                        let offset = [
                            (res.origin[0] - self.camera.center[0]) as f32,
                            (res.origin[1] - self.camera.center[1]) as f32,
                        ];
                        let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: s.color };
                        pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                        pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                        pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..res.index_count, 0, 0..1);
                    }
                    CanvasItem::Image(_) => {
                        let Some(res) = self.registry.get_image(id) else { continue };
                        if on_stroke_pipeline {
                            pass.set_pipeline(&self.image_pipeline.pipeline);
                            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
                            on_stroke_pipeline = false;
                        }
                        pass.set_bind_group(1, &res.bind_group, &[]);
                        let offset = [
                            (res.origin[0] - self.camera.center[0]) as f32,
                            (res.origin[1] - self.camera.center[1]) as f32,
                        ];
                        let immediate =
                            DrawImmediate { offset, _pad: [0.0; 2], color: [1.0, 1.0, 1.0, 1.0] };
                        pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                        pass.set_vertex_buffer(0, res.vertex_buf.slice(..));
                        pass.set_index_buffer(res.index_buf.slice(..), wgpu::IndexFormat::Uint32);
                        pass.draw_indexed(0..6, 0, 0..1);
                    }
                    _ => {} // Shape/Text 미구현
                }
            }

            // 이후 pass 안에서 stroke_pipeline을 다시 쓰는 지점(그리는 중인
            // 스트로크, UI 오버레이)들이 있으니 마지막 상태를 stroke로
            // 되돌려둠.
            if !on_stroke_pipeline {
                pass.set_pipeline(&self.pipeline.pipeline);
                pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);
            }

            // 그리는 중인 스트로크 — registry 캐싱 없이 매 프레임 즉석
            // 테셀레이션 + 버퍼 생성. 아이템 딱 하나뿐이고 어차피 매 프레임
            // 포인트가 바뀌어서 캐싱해봐야 이득이 거의 없음 (실측 전 가설
            // 이지만, 여긴 애초에 캐싱 이득 구조 자체가 없는 케이스라 굳이
            // 재보고 결정할 것도 없다고 판단).
            if let Some(stroke) = &self.drawing_stroke {
                let mesh = tessellate_stroke(stroke);
                if !mesh.indices.is_empty() {
                    let vertex_data: Vec<Vertex> =
                        mesh.vertices.iter().map(|&pos| Vertex { pos }).collect();

                    let vertex_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_stroke_vertex_buf"),
                            contents: bytemuck::cast_slice(&vertex_data),
                            usage: wgpu::BufferUsages::VERTEX,
                        });
                    let index_buf =
                        self.core.device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                            label: Some("drawing_stroke_index_buf"),
                            contents: bytemuck::cast_slice(&mesh.indices),
                            usage: wgpu::BufferUsages::INDEX,
                        });

                    let offset = [
                        (mesh.origin[0] - self.camera.center[0]) as f32,
                        (mesh.origin[1] - self.camera.center[1]) as f32,
                    ];
                    let immediate = DrawImmediate { offset, _pad: [0.0; 2], color: stroke.color };
                    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
                    pass.set_vertex_buffer(0, vertex_buf.slice(..));
                    pass.set_index_buffer(index_buf.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(0..mesh.indices.len() as u32, 0, 0..1);
                }
            }

            // UI 오버레이 — 캔버스 다음에 그려서 항상 위에 보이게.
            pass.set_pipeline(&self.ui_pipeline.pipeline);
            pass.set_bind_group(0, &self.pipeline.global_bind_group, &[]);

            let buttons = ui::layout(self.camera.viewport_size, self.tool, self.pen_color);
            for b in &buttons {
                if b.selected {
                    let hl = ui::Rect {
                        x: b.rect.x - ui::HIGHLIGHT_PADDING,
                        y: b.rect.y - ui::HIGHLIGHT_PADDING,
                        w: b.rect.w + ui::HIGHLIGHT_PADDING * 2.0,
                        h: b.rect.h + ui::HIGHLIGHT_PADDING * 2.0,
                    };
                    draw_ui_quad(&self.core.device, &mut pass, hl, ui::HIGHLIGHT_COLOR);
                }
                draw_ui_quad(&self.core.device, &mut pass, b.rect, b.color);
            }
        }

        self.core.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
    }
}


/// UI 버튼 하나를 사각형(삼각형 2개, 인덱스버퍼 없이 정점 6개)으로 그림.
/// 캔버스 스트로크와 달리 버튼은 개수가 적고(6개) 정적이라 매 프레임
/// 버퍼 새로 만들어도 비용 무시할 만함.
fn draw_ui_quad(device: &wgpu::Device, pass: &mut wgpu::RenderPass, rect: ui::Rect, color: [f32; 4]) {
    let (x0, y0, x1, y1) = (rect.x, rect.y, rect.x + rect.w, rect.y + rect.h);
    let verts = [
        Vertex { pos: [x0, y0] },
        Vertex { pos: [x1, y0] },
        Vertex { pos: [x0, y1] },
        Vertex { pos: [x1, y0] },
        Vertex { pos: [x1, y1] },
        Vertex { pos: [x0, y1] },
    ];
    let vbuf = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("ui_quad_vbuf"),
        contents: bytemuck::cast_slice(&verts),
        usage: wgpu::BufferUsages::VERTEX,
    });
    let immediate = DrawImmediate { offset: [0.0, 0.0], _pad: [0.0; 2], color };
    pass.set_immediates(0, bytemuck::bytes_of(&immediate));
    pass.set_vertex_buffer(0, vbuf.slice(..));
    pass.draw(0..6, 0..1);
}



// ============================================================
// 도형 인식기 (Shape Recognizer)
// ============================================================

/// RDP(Ramer-Douglas-Peucker) 알고리즘으로 자잘한 곡선을 단순한 다각형으로 축약합니다.
fn rdp(points: &[PenPoint], epsilon: f64, out: &mut Vec<PenPoint>) {
    if points.is_empty() { return; }
    
    let mut dmax = 0.0;
    let mut index = 0;
    let end = points.len() - 1;
    
    for i in 1..end {
        let d = crate::scene::segment_dist_sq(points[0].pos, points[end].pos, points[i].pos);
        if d > dmax {
            index = i;
            dmax = d;
        }
    }
    
    if dmax > epsilon * epsilon {
        let mut rec_results1 = Vec::new();
        rdp(&points[0..=index], epsilon, &mut rec_results1);
        
        let mut rec_results2 = Vec::new();
        rdp(&points[index..=end], epsilon, &mut rec_results2);
        
        out.extend_from_slice(&rec_results1[0..rec_results1.len() - 1]);
        out.extend_from_slice(&rec_results2);
    } else {
        out.push(points[0].clone());
        out.push(points[end].clone());
    }
}

/// 스트로크를 분석하여 적절한 완벽한 기하학적 도형으로 변환합니다.
fn recognize_and_snap_shape(stroke: &mut Stroke) -> Option<SnapData> {
    if stroke.points.len() < 10 { return None; }

    let (min, max) = crate::scene::stroke_bbox(stroke);
    let diag = ((max[0] - min[0]).powi(2) + (max[1] - min[1]).powi(2)).sqrt();
    if diag < 10.0 { return None; }

    let first = stroke.points.first().unwrap().pos;
    let last = stroke.points.last().unwrap().pos;
    let initial_pen = last; // 스냅 발동 순간 펜의 위치
    
    let start_end_dist = ((first[0] - last[0]).powi(2) + (first[1] - last[1]).powi(2)).sqrt();
    let closed = start_end_dist < diag * 0.2; 
    let avg_pressure = stroke.points.iter().map(|p| p.pressure).sum::<f32>() / stroke.points.len() as f32;

    if !closed {
        // [1. 직선]
        stroke.points = vec![
            PenPoint { pos: first, pressure: avg_pressure },
            PenPoint { pos: last, pressure: avg_pressure },
        ];
        stroke.mesh_dirty = true;
        
        // 직선은 회전 행렬 대신 "시작점 고정, 끝점 펜 추적"을 사용하므로 중심점을 시작점(first)으로 둠
        return Some(SnapData { center: first, local_points: vec![], initial_pen, is_line: true });
        
    } else {
        // [닫힌 도형 처리]
        let mut process_points = stroke.points.clone();
        let last_idx = process_points.len() - 1;
        process_points[last_idx].pos = process_points[0].pos;

        let mut simplified = Vec::new();
        rdp(&process_points, diag * 0.12, &mut simplified);
        let v_count = simplified.len();
        
        if v_count == 4 { // 삼각형
            stroke.points = simplified.into_iter().map(|mut p| { p.pressure = avg_pressure; p }).collect();
        } else if v_count == 5 { // 직사각형
            stroke.points = vec![
                PenPoint { pos: [min[0], min[1]], pressure: avg_pressure },
                PenPoint { pos: [max[0], min[1]], pressure: avg_pressure },
                PenPoint { pos: [max[0], max[1]], pressure: avg_pressure },
                PenPoint { pos: [min[0], max[1]], pressure: avg_pressure },
                PenPoint { pos: [min[0], min[1]], pressure: avg_pressure },
            ];
        } else { // 원
            let center_x = (min[0] + max[0]) / 2.0;
            let center_y = (min[1] + max[1]) / 2.0;
            let r = ((max[0] - min[0]) + (max[1] - min[1])) / 4.0;
            
            let mut circle_pts = Vec::new();
            let segments = 64; 
            for i in 0..=segments {
                let theta = (i as f64 / segments as f64) * std::f64::consts::TAU;
                circle_pts.push(PenPoint {
                    pos: [center_x + r * theta.cos(), center_y + r * theta.sin()],
                    pressure: avg_pressure,
                });
            }
            stroke.points = circle_pts;
        }
        
        stroke.mesh_dirty = true;
        
        // 만들어진 완벽한 도형의 중심점과, 그 중심점 기준의 상대 좌표를 기록해 둡니다 (회전용)
        let (s_min, s_max) = crate::scene::stroke_bbox(stroke);
        let center = [(s_min[0] + s_max[0]) / 2.0, (s_min[1] + s_max[1]) / 2.0];
        let local_points = stroke.points.iter().map(|p| [p.pos[0] - center[0], p.pos[1] - center[1]]).collect();
        
        return Some(SnapData { center, local_points, initial_pen, is_line: false });
    }
}