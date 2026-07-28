// ============================================================
// src/ui/mod.rs
// ============================================================
// 화면 고정 UI(펜/지우개 토글 + 색상 팔레트) 레이아웃 계산 + 히트테스트.
// 실제 그리기는 App::render가 이 정보로 ui_pipeline을 통해 수행.
// 매 프레임/클릭마다 layout()을 다시 계산 — 버튼 6개뿐이라 캐싱 불필요.

use crate::app::Tool;

pub const BUTTON_SIZE: f32 = 40.0;
pub const GAP: f32 = 8.0;
pub const GROUP_GAP: f32 = 24.0;
pub const MARGIN_BOTTOM: f32 = 20.0;
pub const HIGHLIGHT_PADDING: f32 = 4.0;
pub const HIGHLIGHT_COLOR: [f32; 4] = [1.0, 0.8, 0.0, 1.0];

pub const PALETTE: [[f32; 4]; 4] = [
    [0.0, 0.0, 0.0, 1.0],
    [0.0, 0.6, 0.2, 1.0],
    [0.8, 0.0, 0.0, 1.0],
    [0.0, 0.4, 0.8, 1.0],
];

#[derive(Debug, Clone, Copy)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn contains(&self, p: [f32; 2]) -> bool {
        p[0] >= self.x && p[0] <= self.x + self.w && p[1] >= self.y && p[1] <= self.y + self.h
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiAction {
    SelectTool(Tool),
    SelectColor([f32; 4]),
}

#[derive(Debug, Clone, Copy)]
pub struct UiButton {
    pub rect: Rect,
    pub color: [f32; 4],
    pub action: UiAction,
    pub selected: bool,
}

pub fn layout(viewport: [f32; 2], current_tool: Tool, current_color: [f32; 4]) -> Vec<UiButton> {
    let n_tool = 2;
    let n_color = PALETTE.len();
    let total_w = n_tool as f32 * BUTTON_SIZE
        + (n_tool as f32 - 1.0) * GAP
        + GROUP_GAP
        + n_color as f32 * BUTTON_SIZE
        + (n_color as f32 - 1.0) * GAP;

    let start_x = (viewport[0] - total_w) * 0.5;
    let y = viewport[1] - MARGIN_BOTTOM - BUTTON_SIZE;

    let mut buttons = Vec::with_capacity(n_tool + n_color);
    let mut x = start_x;

    let tool_button_color = [0.3, 0.3, 0.3, 1.0];
    for tool in [Tool::Pen, Tool::Eraser] {
        buttons.push(UiButton {
            rect: Rect { x, y, w: BUTTON_SIZE, h: BUTTON_SIZE },
            color: tool_button_color,
            action: UiAction::SelectTool(tool),
            selected: tool == current_tool,
        });
        x += BUTTON_SIZE + GAP;
    }

    x += GROUP_GAP - GAP;

    for &color in PALETTE.iter() {
        buttons.push(UiButton {
            rect: Rect { x, y, w: BUTTON_SIZE, h: BUTTON_SIZE },
            color,
            action: UiAction::SelectColor(color),
            selected: color == current_color,
        });
        x += BUTTON_SIZE + GAP;
    }

    buttons
}

pub fn hit_test(
    pos: [f32; 2],
    viewport: [f32; 2],
    current_tool: Tool,
    current_color: [f32; 4],
) -> Option<UiAction> {
    layout(viewport, current_tool, current_color)
        .into_iter()
        .find(|b| b.rect.contains(pos))
        .map(|b| b.action)
}