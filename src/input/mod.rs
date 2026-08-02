// ============================================================
// src/input/mod.rs
// ============================================================
// SDL3 펜/마우스 이벤트를 도구 로직이 쓸 수 있는 공통 이벤트 스트림으로
// 변환한다.
//
// 핵심 설계 포인트:
// 1. pressure는 PenMotion이 아니라 별도 PenAxis 이벤트로 옴 — 캐시해뒀다가
//    위치 이벤트에 합성.
// 2. 마우스/펜을 같은 PointerEvent 스트림으로 통일하되, source 필드로
//    구분 가능하게 함 — App이 "펜=그리기, 마우스=팬"으로 분기하는 데 씀.
// 3. Hold(도형 자동스냅)는 매 프레임 폴링(update)으로 감지.
// 4. 좌표는 스크린 좌표 그대로 냄, world 변환은 camera가 담당.
// 5. 마우스 위치와 펜 위치는 서로 다른 필드로 따로 추적 — 어느 소스가
//    최근에 움직였는지에 좌우되지 않게.

use sdl3::event::Event;
use sdl3::mouse::MouseButton;
use sdl3::pen::PenAxis;
use sdl3::sys::pen::SDL_PEN_MOUSEID;
use std::time::{Duration, Instant};

const HOLD_DURATION: Duration = Duration::from_millis(1000);
const MOVE_JITTER_PX: f32 = 3.0;

fn is_pen_synthesized(which: u32) -> bool {
    which == SDL_PEN_MOUSEID.0
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PointerSource {
    Mouse,
    Pen,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InputSample {
    pub pos: [f32; 2],
    pub pressure: f32,
    pub source: PointerSource,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PointerEvent {
    Down(InputSample),
    Move(InputSample),
    Up(InputSample),
    Hold(InputSample),
}

impl PointerEvent {
    pub fn sample(&self) -> InputSample {
        match self {
            PointerEvent::Down(s)
            | PointerEvent::Move(s)
            | PointerEvent::Up(s)
            | PointerEvent::Hold(s) => *s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    Pointer(PointerEvent),
    MouseSideButton { button: MouseButton, pressed: bool },
}

pub struct InputState {
    last_pressure: f32,
    pointer_down: bool,
    active_source: PointerSource,
    last_significant_pos: [f32; 2],
    last_move_at: Instant,
    hold_fired: bool,
    last_mouse_pos: [f32; 2],
    last_pen_pos: [f32; 2],
}

impl InputState {
    pub fn new() -> Self {
        Self {
            last_pressure: 1.0,
            pointer_down: false,
            active_source: PointerSource::Mouse,
            last_significant_pos: [0.0, 0.0],
            last_move_at: Instant::now(),
            hold_fired: false,
            last_mouse_pos: [0.0, 0.0],
            last_pen_pos: [0.0, 0.0],
        }
    }

    pub fn last_mouse_pos(&self) -> [f32; 2] {
        self.last_mouse_pos
    }

    pub fn last_pen_pos(&self) -> [f32; 2] {
        self.last_pen_pos
    }

    pub fn is_pen_down(&self) -> bool {
        self.pointer_down && self.active_source == PointerSource::Pen
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
                self.last_pen_pos = [*x, *y];
                Some(InputEvent::Pointer(self.begin(*x, *y, self.last_pressure, PointerSource::Pen)))
            }
            Event::PenMotion { x, y, .. } => {
                self.last_pen_pos = [*x, *y]; // 호버 중에도 위치는 계속 갱신(커서 아이콘 추적용)
                if self.pointer_down && self.active_source == PointerSource::Pen {
                    self.moved(*x, *y, self.last_pressure, PointerSource::Pen).map(InputEvent::Pointer)
                } else {
                    None // 접촉 없이 호버만 하는 중이면 Move 자체를 안 흘려보냄
                }
            }
            Event::PenUp { x, y, .. } => {
                self.last_pen_pos = [*x, *y];
                Some(InputEvent::Pointer(self.end(*x, *y, self.last_pressure, PointerSource::Pen)))
            }
            Event::PenProximityOut { .. } => {
                // 펜이 감지범위를 완전히 벗어남 — Down/Up 페어링이 어떤 이유로든
                // 깨져서 pointer_down이 stuck-true인 상태를 여기서 강제로 되돌림.
                // PenUp과 동일한 end() 경로를 재사용해서 별도 리셋 로직 없이 안전.
                if self.pointer_down && self.active_source == PointerSource::Pen {
                    Some(InputEvent::Pointer(self.end(
                        self.last_pen_pos[0],
                        self.last_pen_pos[1],
                        self.last_pressure,
                        PointerSource::Pen,
                    )))
                } else {
                    None
                }
            }

            Event::MouseButtonDown { mouse_btn: MouseButton::Left, which, x, y, .. } => {
                if is_pen_synthesized(*which) {
                    return None;
                }
                self.last_mouse_pos = [*x, *y];
                Some(InputEvent::Pointer(self.begin(*x, *y, 1.0, PointerSource::Mouse)))
            }
            Event::MouseMotion { which, x, y, .. } => {
                if is_pen_synthesized(*which) {
                    return None;
                }
                self.last_mouse_pos = [*x, *y];
                if self.pointer_down && self.active_source == PointerSource::Mouse {
                    self.moved(*x, *y, 1.0, PointerSource::Mouse).map(InputEvent::Pointer)
                } else {
                    None
                }
            }
            Event::MouseButtonUp { mouse_btn: MouseButton::Left, which, x, y, .. } => {
                if is_pen_synthesized(*which) {
                    return None;
                }
                self.last_mouse_pos = [*x, *y];
                Some(InputEvent::Pointer(self.end(*x, *y, 1.0, PointerSource::Mouse)))
            }

            Event::MouseButtonDown { mouse_btn: btn @ (MouseButton::X1 | MouseButton::X2), .. } => {
                Some(InputEvent::MouseSideButton { button: *btn, pressed: true })
            }
            Event::MouseButtonUp { mouse_btn: btn @ (MouseButton::X1 | MouseButton::X2), .. } => {
                Some(InputEvent::MouseSideButton { button: *btn, pressed: false })
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
                source: self.active_source,
            })));
        }
        None
    }

    fn begin(&mut self, x: f32, y: f32, pressure: f32, source: PointerSource) -> PointerEvent {
        self.pointer_down = true;
        self.active_source = source;
        self.hold_fired = false;
        self.last_significant_pos = [x, y];
        self.last_move_at = Instant::now();
        PointerEvent::Down(InputSample { pos: [x, y], pressure, source })
    }

    fn moved(&mut self, x: f32, y: f32, pressure: f32, source: PointerSource) -> Option<PointerEvent> {
        let dx = x - self.last_significant_pos[0];
        let dy = y - self.last_significant_pos[1];
        if (dx * dx + dy * dy).sqrt() > MOVE_JITTER_PX {
            self.last_significant_pos = [x, y];
            self.last_move_at = Instant::now();
            self.hold_fired = false;
        }
        Some(PointerEvent::Move(InputSample { pos: [x, y], pressure, source }))
    }

    fn end(&mut self, x: f32, y: f32, pressure: f32, source: PointerSource) -> PointerEvent {
        self.pointer_down = false;
        PointerEvent::Up(InputSample { pos: [x, y], pressure, source })
    }
}