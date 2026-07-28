// ============================================================
// src/ui/mod.rs
// ============================================================
use crate::app::Tool;

pub const BUTTON_SIZE: f32 = 20.0; // 원래 40.0에서 절반 (면적은 1/4)
pub const GAP: f32 = 4.0;
pub const GROUP_GAP: f32 = 12.0;
pub const MARGIN_BOTTOM: f32 = 10.0;

// 5단계 펜 두께
pub const THICKNESS_LEVELS: [f32; 5] = [1.0, 3.0, 6.0, 10.0, 15.0];

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
    SelectThickness(f32),
}

// 렌더러가 버튼을 그릴 때 무엇을 그릴지 구분하기 위한 Enum
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonKind {
    Tool(Tool),
    Color([f32; 4]),
    Thickness(f32),
}

#[derive(Debug, Clone, Copy)]
pub struct UiButton {
    pub rect: Rect,
    pub kind: ButtonKind,
    pub action: UiAction,
    pub selected: bool,
}

pub fn layout(
    viewport: [f32; 2],
    current_tool: Tool,
    current_color: [f32; 4],
    current_thickness: f32,
) -> Vec<UiButton> {
    let n_tool = 3;
    let n_color = PALETTE.len();
    
    let tool_w = n_tool as f32 * BUTTON_SIZE + (n_tool as f32 - 1.0) * GAP;
    let color_w = n_color as f32 * BUTTON_SIZE; // 팔레트는 간격(GAP)이 0임
    let total_w = tool_w + GROUP_GAP + color_w;

    let start_x = (viewport[0] - total_w) * 0.5;
    let tool_start_x = start_x;
    let color_start_x = tool_start_x + tool_w + GROUP_GAP;
    
    let tool_y = viewport[1] - MARGIN_BOTTOM - BUTTON_SIZE;
    let thickness_y = tool_y - BUTTON_SIZE - GAP * 1.5; // 도구 버튼 위쪽에 배치

    let mut buttons = Vec::new();

    // 1. 두께 버튼 (도구 버튼 그룹 바로 위)
    let mut tx = tool_start_x;
    for &t in &THICKNESS_LEVELS {
        buttons.push(UiButton {
            rect: Rect { x: tx, y: thickness_y, w: BUTTON_SIZE * 0.8, h: BUTTON_SIZE * 0.8 },
            kind: ButtonKind::Thickness(t),
            action: UiAction::SelectThickness(t),
            selected: (t - current_thickness).abs() < 0.1,
        });
        tx += BUTTON_SIZE * 0.8 + GAP;
    }

    // 2. 도구 버튼
    let mut x = tool_start_x;
    for tool in [Tool::Pen, Tool::Eraser, Tool::Select] {
        buttons.push(UiButton {
            rect: Rect { x, y: tool_y, w: BUTTON_SIZE, h: BUTTON_SIZE },
            kind: ButtonKind::Tool(tool),
            action: UiAction::SelectTool(tool),
            selected: tool == current_tool,
        });
        x += BUTTON_SIZE + GAP;
    }

    // 3. 컬러 팔레트 (간격 없이 딱 붙여서)
    let mut cx = color_start_x;
    for &color in PALETTE.iter() {
        buttons.push(UiButton {
            rect: Rect { x: cx, y: tool_y, w: BUTTON_SIZE, h: BUTTON_SIZE },
            kind: ButtonKind::Color(color),
            action: UiAction::SelectColor(color),
            selected: color == current_color,
        });
        cx += BUTTON_SIZE; // GAP 없이 크기만큼만 더함
    }

    buttons
}

pub fn hit_test(
    pos: [f32; 2],
    viewport: [f32; 2],
    current_tool: Tool,
    current_color: [f32; 4],
    current_thickness: f32,
) -> Option<UiAction> {
    layout(viewport, current_tool, current_color, current_thickness)
        .into_iter()
        .find(|b| b.rect.contains(pos))
        .map(|b| b.action)
}