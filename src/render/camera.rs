// ============================================================
// src/render/camera.rs
// ============================================================
// world(f64) <-> screen(f32) 좌표 변환. center만 f64로 들고, zoom은
// 상대 배율이라 f64 정밀도가 필요 없어 f32로 둔다 (무한캔버스 정밀도
// 문제는 "카메라가 원점에서 얼마나 멀리 있는가"에서만 발생하고, 배율
// 자체엔 없음).

/// [미검증 가설] UX 튜닝 전 임시값 — 실제 조작감 확인 후 조정 예정.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 40.0;

pub struct Camera {
    pub center: [f64; 2],
    pub zoom: f32,
    pub viewport_size: [f32; 2],
}

impl Camera {
    pub fn new(viewport_size: [f32; 2]) -> Self {
        Self { center: [0.0, 0.0], zoom: 1.0, viewport_size }
    }

    pub fn resize(&mut self, viewport_size: [f32; 2]) {
        self.viewport_size = viewport_size;
    }

    fn viewport_half(&self) -> [f32; 2] {
        [self.viewport_size[0] * 0.5, self.viewport_size[1] * 0.5]
    }

    /// world 좌표 -> 스크린(윈도우 픽셀) 좌표.
    pub fn world_to_screen(&self, world: [f64; 2]) -> [f32; 2] {
        // camera-relative 변환: f64로 먼저 빼고(원점에서 먼 좌표라도
        // 정밀도 안 깨짐), 그 다음 작아진 값을 f32로 캐스팅.
        let rel = [(world[0] - self.center[0]) as f32, (world[1] - self.center[1]) as f32];
        let vp_half = self.viewport_half();
        [rel[0] * self.zoom + vp_half[0], rel[1] * self.zoom + vp_half[1]]
    }

    /// 스크린 좌표 -> world 좌표 (펜/마우스 입력 변환용).
    pub fn screen_to_world(&self, screen: [f32; 2]) -> [f64; 2] {
        let vp_half = self.viewport_half();
        let rel = [(screen[0] - vp_half[0]) / self.zoom, (screen[1] - vp_half[1]) / self.zoom];
        [self.center[0] + rel[0] as f64, self.center[1] + rel[1] as f64]
    }

    /// 드래그 팬 — 스크린 픽셀 이동량을 world 이동으로 변환해 center에 반영.
    pub fn pan_by_screen_delta(&mut self, delta_screen: [f32; 2]) {
        self.center[0] -= (delta_screen[0] / self.zoom) as f64;
        self.center[1] -= (delta_screen[1] / self.zoom) as f64;
    }

    /// 스크린상의 한 점(마우스 커서 위치 등)을 고정한 채로 확대/축소.
    /// factor > 1.0 = 확대, < 1.0 = 축소.
    pub fn zoom_at(&mut self, pivot_screen: [f32; 2], factor: f32) {
        let world_before = self.screen_to_world(pivot_screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);

        let vp_half = self.viewport_half();
        let offset = [
            (pivot_screen[0] - vp_half[0]) / self.zoom,
            (pivot_screen[1] - vp_half[1]) / self.zoom,
        ];
        self.center = [world_before[0] - offset[0] as f64, world_before[1] - offset[1] as f64];
    }

    /// 현재 뷰포트가 world 좌표에서 덮는 범위(AABB). 컬링 판정에 씀.
    pub fn world_view_bounds(&self) -> ([f64; 2], [f64; 2]) {
        let a = self.screen_to_world([0.0, 0.0]);
        let b = self.screen_to_world(self.viewport_size);
        ([a[0].min(b[0]), a[1].min(b[1])], [a[0].max(b[0]), a[1].max(b[1])])
    }
}