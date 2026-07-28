// ============================================================
// src/ui/mod.rs
// ============================================================
use crate::app::Tool;

pub const BUTTON_SIZE: f32 = 20.0; // 크기 절반 축소 (면적 1/4)
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
    pub x: f32, pub y: f32, pub w: f32, pub h: f32,
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

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ButtonKind {
    Tool(Tool),
    Color([f32; 4]),
    ThicknessBar { selected_index: usize },
}

#[derive(Debug, Clone, Copy)]
pub struct UiButton {
    pub rect: Rect,
    pub kind: ButtonKind,
    pub action: Option<UiAction>,
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
    let color_w = n_color as f32 * BUTTON_SIZE; // 팔레트는 간격 없음
    let total_w = tool_w + GROUP_GAP + color_w;

    let start_x = (viewport[0] - total_w) * 0.5;
    let color_start_x = start_x + tool_w + GROUP_GAP;
    
    let tool_y = viewport[1] - MARGIN_BOTTOM - BUTTON_SIZE;
    let thickness_y = tool_y - BUTTON_SIZE - GAP * 1.5;

    let mut buttons = Vec::new();

    // 1. 물방울 두께 바 (도구 버튼들 너비에 맞춤)
    let bar_w = tool_w * 0.9;
    let bar_h = BUTTON_SIZE * 0.7;
    let mut selected_idx = 1;
    for (i, &t) in THICKNESS_LEVELS.iter().enumerate() {
        if (t - current_thickness).abs() < 0.1 { selected_idx = i; break; }
    }
    buttons.push(UiButton {
        rect: Rect { x: start_x, y: thickness_y + (BUTTON_SIZE - bar_h)*0.5, w: bar_w, h: bar_h },
        kind: ButtonKind::ThicknessBar { selected_index: selected_idx },
        action: None, // hit_test에서 비율로 분기
        selected: false,
    });

    // 2. 도구 버튼
    let mut x = start_x;
    for tool in [Tool::Pen, Tool::Eraser, Tool::Select] {
        buttons.push(UiButton {
            rect: Rect { x, y: tool_y, w: BUTTON_SIZE, h: BUTTON_SIZE },
            kind: ButtonKind::Tool(tool),
            action: Some(UiAction::SelectTool(tool)),
            selected: tool == current_tool,
        });
        x += BUTTON_SIZE + GAP;
    }

    // 3. 컬러 팔레트 (딱 붙여서)
    let mut cx = color_start_x;
    for &color in PALETTE.iter() {
        buttons.push(UiButton {
            rect: Rect { x: cx, y: tool_y, w: BUTTON_SIZE, h: BUTTON_SIZE },
            kind: ButtonKind::Color(color),
            action: Some(UiAction::SelectColor(color)),
            selected: color == current_color,
        });
        cx += BUTTON_SIZE;
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
    for b in layout(viewport, current_tool, current_color, current_thickness) {
        if b.rect.contains(pos) {
            if let Some(action) = b.action {
                return Some(action);
            }
            if let ButtonKind::ThicknessBar { .. } = b.kind {
                // 두께 바 클릭 시, 가로 x 좌표의 비율(0.0~1.0)을 구해 5등분 중 어디인지 판단
                let ratio = (pos[0] - b.rect.x) / b.rect.w;
                let idx = (ratio * 5.0) as usize;
                let idx = idx.clamp(0, 4);
                return Some(UiAction::SelectThickness(THICKNESS_LEVELS[idx]));
            }
        }
    }
    None
}