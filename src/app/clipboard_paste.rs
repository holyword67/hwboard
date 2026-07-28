// ============================================================
// src/app/clipboard_paste.rs
// ============================================================
// Ctrl+V로 클립보드 이미지를 캔버스에 붙여넣기.

use super::App;
use crate::clipboard::read_clipboard_image_bytes;
use crate::scene::{AddItem, CanvasItem, ImageItem};
use std::sync::Arc;

impl App {
    pub(super) fn paste_image_from_clipboard(&mut self) {
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
}