// ============================================================
// src/scene/item.rs
// ============================================================
// 캔버스에 올라가는 모든 것의 통합 타입. 삽입 순서 = Z 순서라는 원칙은
// Scene의 order: Vec<ItemId>가 담당하고, 여기 CanvasItem 자체는
// "무엇인가"만 표현한다 (순서 정보 없음).

pub type ItemId = u64;

#[derive(Debug, Clone)]
pub enum CanvasItem {
    Stroke(Stroke),
    Image(ImageItem),
    Shape(Shape),
    Text(TextItem),
}

#[derive(Debug, Clone)]
pub struct PenPoint {
    pub pos: [f64; 2],   // world 좌표, f64 (카메라 상대 변환은 render 단계에서)
    pub pressure: f32,   // 0.0 ~ 1.0
}

#[derive(Debug, Clone)]
pub struct Stroke {
    pub points: Vec<PenPoint>,
    pub color: [f32; 4],
    pub base_width: f32,
    // 렌더용 테셀레이션 캐시. points가 바뀔 때만 재생성, 매 프레임 재계산 안 함.
    pub mesh_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct ImageItem {
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    pub texture_handle: u32, // GPU 텍스처 레지스트리 인덱스 (render 모듈이 관리)
}

#[derive(Debug, Clone)]
pub enum ShapeKind {
    Circle,
    Line,
    Rectangle,
}

#[derive(Debug, Clone)]
pub struct Shape {
    pub kind: ShapeKind,
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    pub color: [f32; 4],
    pub stroke_width: f32,
}

#[derive(Debug, Clone)]
pub struct TextItem {
    pub top_left: [f64; 2],
    pub content: String,
    pub font_size: f32,
    pub color: [f32; 4],
}

impl CanvasItem {
    /// 모든 아이템 종류가 공통으로 가져야 하는 바운딩 박스 — 선택/hit-test에 사용.
    pub fn bounding_box(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            CanvasItem::Stroke(s) => stroke_bbox(s),
            CanvasItem::Image(img) => (img.top_left, [img.top_left[0] + img.size[0], img.top_left[1] + img.size[1]]),
            CanvasItem::Shape(sh) => (sh.top_left, [sh.top_left[0] + sh.size[0], sh.top_left[1] + sh.size[1]]),
            CanvasItem::Text(t) => (t.top_left, t.top_left), // TODO: 실제 텍스트 크기 측정 붙여야 함
        }
    }
}

fn stroke_bbox(s: &Stroke) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::MAX, f64::MAX];
    let mut max = [f64::MIN, f64::MIN];
    for p in &s.points {
        min[0] = min[0].min(p.pos[0]);
        min[1] = min[1].min(p.pos[1]);
        max[0] = max[0].max(p.pos[0]);
        max[1] = max[1].max(p.pos[1]);
    }
    (min, max)
}