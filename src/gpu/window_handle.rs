// ============================================================
// src/gpu/window_handle.rs
// ============================================================
// SDL3 윈도우 핸들을 wgpu가 요구하는 Send+Sync 형태로 감싸는 래퍼.
// GpuCore 초기화 시점에만 쓰임. doxa2에서 그대로 재활용 — 도메인 무관.
use sdl3::video::Window;
use wgpu::rwh::{
    DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle, RawDisplayHandle,
    RawWindowHandle, WindowHandle,
};

pub(crate) struct OwnedWindowHandle {
    raw_window: RawWindowHandle,
    raw_display: RawDisplayHandle,
}

unsafe impl Send for OwnedWindowHandle {}
unsafe impl Sync for OwnedWindowHandle {}

impl OwnedWindowHandle {
    pub(crate) fn new(window: &Window) -> Result<Self, Box<dyn std::error::Error>> {
        Ok(Self {
            raw_window: window.window_handle()?.as_raw(),
            raw_display: window.display_handle()?.as_raw(),
        })
    }
}

impl HasWindowHandle for OwnedWindowHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        Ok(unsafe { WindowHandle::borrow_raw(self.raw_window) })
    }
}

impl HasDisplayHandle for OwnedWindowHandle {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(unsafe { DisplayHandle::borrow_raw(self.raw_display) })
    }
}
