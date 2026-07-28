// ============================================================
// src/clipboard.rs
// ============================================================
// SDL3 클립보드에서 이미지 바이트를 읽어옴. sdl3-rs 고수준 래퍼가
// mime-type 기반 바이너리 클립보드 API를 감싸주는지 불확실해서
// sdl3::sys::clipboard를 직접 unsafe로 호출함 — 함수 시그니처가 실제로
// 이거랑 맞는지는 컴파일해봐야 확정됨.

use sdl3::sys::clipboard::{SDL_GetClipboardData, SDL_HasClipboardData};
use std::ffi::CString;
use std::os::raw::c_void;

/// png/bmp 먼저 시도(가장 흔한 클립보드 이미지 형식), 나머지는 폴백.
/// image 크레이트가 디코딩만 성공하면 되니 mime 이름 자체의 정확도보다
/// "클립보드에 그 타입으로 데이터가 있는지"만 중요함.
const CANDIDATE_MIME_TYPES: &[&str] =
    &["image/png", "image/bmp", "image/webp", "image/jpeg", "image/jpg", "image/gif"];

pub fn read_clipboard_image_bytes() -> Option<Vec<u8>> {
    for mime in CANDIDATE_MIME_TYPES {
        let c_mime = CString::new(*mime).ok()?;
        unsafe {
            if !SDL_HasClipboardData(c_mime.as_ptr()) {
                continue;
            }
            let mut size: usize = 0;
            let ptr = SDL_GetClipboardData(c_mime.as_ptr(), &mut size);
            if ptr.is_null() || size == 0 {
                continue;
            }
            let bytes = std::slice::from_raw_parts(ptr as *const u8, size).to_vec();
            sdl3::sys::stdinc::SDL_free(ptr as *mut c_void);
            return Some(bytes);
        }
    }
    None
}