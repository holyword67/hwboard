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
    pub size: [f64; 2], // world 크기 = 붙여넣을 때 픽셀 크기 그대로
    pub pixel_width: u32,
    pub pixel_height: u32,
    /// RGBA8 원본 픽셀. Arc로 감싸서, DeleteItems가 undo용으로
    /// CanvasItem을 통째로 clone할 때마다 수 MB를 복사하지 않게 함 —
    /// 이건 이미지 크기가 크면 확실히 발생하는 비용이라 가설이 아니라
    /// 처음부터 막아둔 것.
    pub rgba: std::sync::Arc<[u8]>,
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

    /// [추가됨] 정밀 히트테스트 (Broad Phase -> Narrow Phase)
    pub fn hit_test(&self, point: [f64; 2], eraser_radius: f64) -> bool {
        let (min, max) = self.bounding_box();
        
        // 1차 필터링: Bounding Box 체크 (연산 최적화)
        if point[0] < min[0] - eraser_radius || point[0] > max[0] + eraser_radius ||
           point[1] < min[1] - eraser_radius || point[1] > max[1] + eraser_radius {
            return false; // 근처에도 안 왔으면 즉시 기각
        }

        // 2차 필터링: 아이템 타입별 정밀 거리 계산
        match self {
            CanvasItem::Stroke(s) => {
                // 선 두께의 절반도 지우개 반경에 포함시켜야 두꺼운 선의 외곽선을 만져도 지워짐
                let r = eraser_radius + (s.base_width as f64 * 0.5); 
                let r_sq = r * r; // sqrt 연산을 피하기 위해 거리 제곱 사용

                if s.points.is_empty() { return false; }
                
                // 점이 1개일 경우 (점 찍기)
                if s.points.len() == 1 {
                    let dx = point[0] - s.points[0].pos[0];
                    let dy = point[1] - s.points[0].pos[1];
                    return (dx * dx + dy * dy) <= r_sq;
                }

                // 점이 여러 개일 경우 (선분마다 검사)
                for i in 0..s.points.len() - 1 {
                    let p0 = s.points[i].pos;
                    let p1 = s.points[i+1].pos;
                    if segment_dist_sq(p0, p1, point) <= r_sq {
                        return true;
                    }
                }
                false
            },
            // 이미지는 Bounding Box 내부 클릭만으로 충분함
            CanvasItem::Image(_) | CanvasItem::Shape(_) | CanvasItem::Text(_) => true,
        }
    }
}


pub fn stroke_bbox(s: &Stroke) -> ([f64; 2], [f64; 2]) {
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


/// [추가됨] 점 P와 선분 AB 사이의 최단 거리 제곱을 구하는 수학 함수
pub fn segment_dist_sq(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let ab_x = b[0] - a[0];
    let ab_y = b[1] - a[1];
    let ap_x = p[0] - a[0];
    let ap_y = p[1] - a[1];

    let ab_len_sq = ab_x * ab_x + ab_y * ab_y;
    if ab_len_sq == 0.0 {
        // A와 B가 완전히 같은 위치에 찍힌 경우
        return ap_x * ap_x + ap_y * ap_y;
    }

    // 선분 AB 상에 점 P를 수직으로 투영했을 때의 비율 t
    // (선분 밖을 벗어나지 않도록 0.0 ~ 1.0 사이로 클램프)
    let t = ((ap_x * ab_x + ap_y * ab_y) / ab_len_sq).clamp(0.0, 1.0);

    // 선분 위에서 P와 가장 가까운 점 C
    let c_x = a[0] + t * ab_x;
    let c_y = a[1] + t * ab_y;

    // 점 P와 점 C 사이의 거리 제곱
    let pc_x = p[0] - c_x;
    let pc_y = p[1] - c_y;
    
    pc_x * pc_x + pc_y * pc_y
}