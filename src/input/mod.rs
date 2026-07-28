// ============================================================
// src/input/mod.rs
// ============================================================
// SDL3 펜/마우스 이벤트를 도구 로직이 쓸 수 있는 공통 이벤트 스트림으로
// 변환한다.
//
// 핵심 설계 포인트:
// 1. SDL3에서 pressure는 PenMotion에 안 실려 오고 별도의 PenAxis 이벤트로
//    옴 — 최신 pressure 값을 캐시해뒀다가 위치 이벤트에 합성한다.
// 2. 마우스도 동일한 PointerEvent로 통일 — pressure는 1.0 고정.
// 3. Hold(도형 자동스냅 트리거)는 이벤트가 아니라 매 프레임 폴링
//    (update)으로 감지 — SDL은 "안 움직이면 이벤트 자체가 안 옴".
// 4. 좌표는 스크린(윈도우 픽셀) 좌표 그대로 낸다. world 좌표 변환은
//    호출 측(app 레이어)이 camera를 통해 처리.
// 5. 펜 barrel 버튼(PenButtonDown/Up)은 위치 정보가 없는 별개의
//    "동작 트리거"라 PointerEvent에 안 섞고 InputEvent::PenButton으로
//    분리. 버튼 번호(1/2)가 실제 하드웨어에서 뭘로 찍히는지는 아직
//    검증 안 됨 — PEN_BUTTON_UNDO/REDO 상수만 바꾸면 재배정 가능.

use sdl3::event::Event;
use sdl3::mouse::MouseButton;
use sdl3::pen::PenAxis;
use sdl3::sys::pen::SDL_PEN_MOUSEID;
use std::time::{Duration, Instant};

const HOLD_DURATION: Duration = Duration::from_millis(400);
const MOVE_JITTER_PX: f32 = 3.0;

/// SDL은 펜 입력이 들어오면 호환성을 위해 "이 펜 입력을 흉내 낸 가짜
/// 마우스 이벤트"도 같이 쏴줌 — which 필드가 이 값이면 진짜 마우스가
/// 아니라 펜에서 합성된 이벤트. 이걸 안 걸러내면 PenMotion 스트림이랑
/// 가짜 MouseMotion 스트림이 같은 스트로크에 섞여서 좌표가 튀는 버그가
/// 생김 (실제로 겪은 버그).
fn is_pen_synthesized(which: u32) -> bool {
    which == SDL_PEN_MOUSEID.0
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSample {
    pub pos: [f32; 2],
    pub pressure: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Down(InputSample),
    Move(InputSample),
    Up(InputSample),
    Hold(InputSample),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    MouseSideButton { button: MouseButton, pressed: bool },
}

pub struct InputState {
    last_pressure: f32,
    pointer_down: bool,
    last_significant_pos: [f32; 2],
    last_move_at: Instant,
    hold_fired: bool,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            last_pressure: 1.0,
            pointer_down: false,
            last_significant_pos: [0.0, 0.0],
            last_move_at: Instant::now(),
            hold_fired: false,
        }
    }

    pub fn process_event(&mut self, event: &Event) -> Option<InputEvent> {
        match event {
            Event::PenAxis { axis, value, .. } => {
                if *axis == PenAxis::Pressure {
                    self.last_pressure = *value;
                }
                None
            }
            Event::PenDown { x, y, .. } => {
                Some(InputEvent::Pointer(self.begin(*x, *y, self.last_pressure)))
            }
            Event::PenMotion { x, y, .. } => {
                self.moved(*x, *y, self.last_pressure).map(InputEvent::Pointer)
            }
            Event::PenUp { x, y, .. } => {
                Some(InputEvent::Pointer(self.end(*x, *y, self.last_pressure)))
            }


            Event::MouseButtonDown { mouse_btn: MouseButton::Left, which, x, y, .. } => {
                if is_pen_synthesized(*which) {
                    return None; // 펜이 이미 PenDown으로 처리됨 — 중복 무시
                }
                Some(InputEvent::Pointer(self.begin(*x, *y, 1.0)))
            }
            Event::MouseButtonDown { mouse_btn: btn @ (MouseButton::X1 | MouseButton::X2), .. } => {
                Some(InputEvent::MouseSideButton { button: *btn, pressed: true })
            }
            Event::MouseButtonUp { mouse_btn: btn @ (MouseButton::X1 | MouseButton::X2), .. } => {
                Some(InputEvent::MouseSideButton { button: *btn, pressed: false })
            }
            Event::MouseMotion { which, x, y, .. } => {
                if is_pen_synthesized(*which) {
                    return None;
                }
                if self.pointer_down {
                    self.moved(*x, *y, 1.0).map(InputEvent::Pointer)
                } else {
                    None
                }
            }
            Event::MouseButtonUp { mouse_btn: MouseButton::Left, which, x, y, .. } => {
                if is_pen_synthesized(*which) {
                    return None;
                }
                Some(InputEvent::Pointer(self.end(*x, *y, 1.0)))
            }
            _ => None,
        }
    }

    pub fn update(&mut self, now: Instant) -> Option<InputEvent> {
        if self.pointer_down
            && !self.hold_fired
            && now.duration_since(self.last_move_at) >= HOLD_DURATION
        {
            self.hold_fired = true;
            return Some(InputEvent::Pointer(PointerEvent::Hold(InputSample {
                pos: self.last_significant_pos,
                pressure: self.last_pressure,
            })));
        }
        None
    }

    fn begin(&mut self, x: f32, y: f32, pressure: f32) -> PointerEvent {
        self.pointer_down = true;
        self.hold_fired = false;
        self.last_significant_pos = [x, y];
        self.last_move_at = Instant::now();
        PointerEvent::Down(InputSample { pos: [x, y], pressure })
    }

    fn moved(&mut self, x: f32, y: f32, pressure: f32) -> Option<PointerEvent> {
        let dx = x - self.last_significant_pos[0];
        let dy = y - self.last_significant_pos[1];
        if (dx * dx + dy * dy).sqrt() > MOVE_JITTER_PX {
            self.last_significant_pos = [x, y];
            self.last_move_at = Instant::now();
            self.hold_fired = false;
        }
        Some(PointerEvent::Move(InputSample { pos: [x, y], pressure }))
    }

    fn end(&mut self, x: f32, y: f32, pressure: f32) -> PointerEvent {
        self.pointer_down = false;
        PointerEvent::Up(InputSample { pos: [x, y], pressure })
    }
}