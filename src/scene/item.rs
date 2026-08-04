// ============================================================
// src/scene/item.rs
// ============================================================
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub type ItemId = u64;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CanvasItem {
    Stroke(Stroke),
    Image(ImageItem),
    Shape(Shape),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PenPoint {
    pub pos: [f64; 2],
    pub pressure: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Stroke {
    pub anchor: [f64; 2],
    #[serde(serialize_with = "serialize_points", deserialize_with = "deserialize_points")]
    pub points: Arc<[PenPoint]>,
    pub color: [f32; 4],
    pub base_width: f32,
    pub geometry_dirty: bool,
    /// 로컬(anchor 기준) 미패딩 bbox 캐시. Stroke는 커밋 후 points가
    /// 절대 안 바뀌므로(부분삭제 없음, translate는 anchor만 갱신) 생성
    /// 시점에 딱 한 번만 계산하면 평생 재계산 불필요 — dirty flag조차
    /// 필요 없음. private로 막아서 Stroke::new() 경유를 강제(직접
    /// struct literal로 stale 값 넣는 걸 컴파일 타임에 차단).
    local_bbox: ([f64; 2], [f64; 2]),
}

fn serialize_points<S: serde::Serializer>(points: &Arc<[PenPoint]>, s: S) -> Result<S::Ok, S::Error> {
    // Arc<[T]>는 serde 기본 Serialize가 없어서(rc feature 없이) 수동 처리.
    // PenPoint는 raw byte가 아니라서 rgba처럼 serialize_bytes 고속경로는
    // 못 씀 — derive가 Vec<PenPoint>에 했을 법한 것과 동일한 seq 인코딩.
    s.collect_seq(points.iter())
}

fn deserialize_points<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Arc<[PenPoint]>, D::Error> {
    let v = Vec::<PenPoint>::deserialize(d)?;
    Ok(Arc::from(v))
}

impl Stroke {
    /// 유일한 생성 경로. bbox 캐시를 여기서 한 번 계산하고 points를
    /// Arc로 감쌈(커밋 후 clone은 refcount 증가만 — O(1)).
    pub fn new(anchor: [f64; 2], points: Vec<PenPoint>, color: [f32; 4], base_width: f32) -> Self {
        let local_bbox = points_bbox(&points);
        Self {
            anchor,
            points: Arc::from(points),
            color,
            base_width,
            geometry_dirty: true,
            local_bbox,
        }
    }

    pub fn bounding_box(&self) -> ([f64; 2], [f64; 2]) {
        let (min, max) = self.local_bbox;
        (
            [min[0] + self.anchor[0], min[1] + self.anchor[1]],
            [max[0] + self.anchor[0], max[1] + self.anchor[1]],
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageItem {
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    pub pixel_width: u32,
    pub pixel_height: u32,
    #[serde(serialize_with = "serialize_rgba", deserialize_with = "deserialize_rgba")]
    pub rgba: Arc<[u8]>,
    pub geometry_dirty: bool,
}

fn serialize_rgba<S: serde::Serializer>(data: &Arc<[u8]>, s: S) -> Result<S::Ok, S::Error> {
    s.serialize_bytes(data)
}

fn deserialize_rgba<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Arc<[u8]>, D::Error> {
    struct BytesVisitor;
    impl<'de> serde::de::Visitor<'de> for BytesVisitor {
        type Value = Arc<[u8]>;
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "byte array")
        }
        fn visit_borrowed_bytes<E: serde::de::Error>(self, v: &'de [u8]) -> Result<Self::Value, E> {
            Ok(Arc::from(v))
        }
        fn visit_bytes<E: serde::de::Error>(self, v: &[u8]) -> Result<Self::Value, E> {
            Ok(Arc::from(v))
        }
        fn visit_byte_buf<E: serde::de::Error>(self, v: Vec<u8>) -> Result<Self::Value, E> {
            Ok(Arc::from(v))
        }
    }
    d.deserialize_bytes(BytesVisitor)
}

impl ImageItem {
    pub fn set_bounds(&mut self, top_left: [f64; 2], size: [f64; 2]) {
        if size != self.size {
            self.geometry_dirty = true;
        }
        self.top_left = top_left;
        self.size = size;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ShapeKind {
    Circle,
    Line,
    Rectangle,
    Triangle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Shape {
    pub kind: ShapeKind,
    pub center: [f64; 2],
    pub half_extent: [f64; 2],
    pub rotation: f32,
    pub color: [f32; 4],
    pub stroke_width: f32,
    pub geometry_dirty: bool,
}

impl Shape {
    fn local_outline(&self) -> Vec<[f64; 2]> {
        let (hx, hy) = (self.half_extent[0], self.half_extent[1]);
        match self.kind {
            ShapeKind::Rectangle => vec![[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy], [-hx, -hy]],
            ShapeKind::Circle => {
                const SEGMENTS: usize = 64;
                (0..=SEGMENTS)
                    .map(|i| {
                        let theta = (i as f64 / SEGMENTS as f64) * std::f64::consts::TAU;
                        [hx * theta.cos(), hy * theta.sin()]
                    })
                    .collect()
            }
            ShapeKind::Line => vec![[-hx, 0.0], [hx, 0.0]],
            ShapeKind::Triangle => vec![[0.0, -hy], [hx, hy], [-hx, hy], [0.0, -hy]],
        }
    }

    fn rotated_local_outline(&self) -> Vec<[f64; 2]> {
        let sin_r = (self.rotation as f64).sin();
        let cos_r = (self.rotation as f64).cos();
        self.local_outline()
            .into_iter()
            .map(|[lx, ly]| [lx * cos_r - ly * sin_r, lx * sin_r + ly * cos_r])
            .collect()
    }

    fn local_corners(&self) -> [[f64; 2]; 4] {
        let (hx, hy) = (self.half_extent[0], self.half_extent[1]);
        [[-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy]]
    }

    pub fn world_corners(&self) -> [[f64; 2]; 4] {
        let sin_r = (self.rotation as f64).sin();
        let cos_r = (self.rotation as f64).cos();
        self.local_corners().map(|[lx, ly]| {
            let rx = lx * cos_r - ly * sin_r;
            let ry = lx * sin_r + ly * cos_r;
            [self.center[0] + rx, self.center[1] + ry]
        })
    }

    pub fn to_local(&self, point: [f64; 2]) -> [f64; 2] {
        let dx = point[0] - self.center[0];
        let dy = point[1] - self.center[1];
        let sin_r = (-self.rotation as f64).sin();
        let cos_r = (-self.rotation as f64).cos();
        [dx * cos_r - dy * sin_r, dx * sin_r + dy * cos_r]
    }

    fn hit_test_local(&self, point: [f64; 2], pad: f64) -> bool {
        let [lx, ly] = self.to_local(point);
        match self.kind {
            ShapeKind::Rectangle => {
                lx.abs() <= self.half_extent[0] + pad && ly.abs() <= self.half_extent[1] + pad
            }
            ShapeKind::Circle => {
                let rx = self.half_extent[0] + pad;
                let ry = self.half_extent[1] + pad;
                if rx <= 0.0 || ry <= 0.0 { return false; }
                (lx / rx).powi(2) + (ly / ry).powi(2) <= 1.0
            }
            ShapeKind::Line => {
                let r = pad + (self.stroke_width as f64 * 0.5);
                segment_dist_sq([-self.half_extent[0], 0.0], [self.half_extent[0], 0.0], [lx, ly]) <= r * r
            }
            ShapeKind::Triangle => {
                if ly < -self.half_extent[1] - pad || ly > self.half_extent[1] + pad {
                    return false;
                }
                let progress = (ly + self.half_extent[1] + pad) / (2.0 * self.half_extent[1] + 2.0 * pad).max(f64::EPSILON);
                let allowed_x = self.half_extent[0] * progress + pad;
                lx.abs() <= allowed_x
            }
        }
    }

    pub fn as_stroke(&self) -> Stroke {
        let points: Vec<PenPoint> = self
            .rotated_local_outline()
            .into_iter()
            .map(|pos| PenPoint { pos, pressure: 1.0 })
            .collect();
        // 이 Stroke는 outline 렌더링(tessellate_stroke)용 1회성 임시
        // 객체 — .bounding_box()를 호출하는 곳이 없어서 Stroke::new()의
        // bbox 계산은 낭비. 같은 모듈이라 private 필드 직접 literal
        // 가능 — 더미값으로 스킵.
        Stroke {
            anchor: self.center,
            points: Arc::from(points),
            color: self.color,
            base_width: self.stroke_width,
            geometry_dirty: true,
            local_bbox: ([0.0, 0.0], [0.0, 0.0]),
        }
    }
}

impl CanvasItem {
    pub fn bounding_box(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            CanvasItem::Stroke(s) => s.bounding_box(),
            CanvasItem::Image(img) => (img.top_left, [img.top_left[0] + img.size[0], img.top_left[1] + img.size[1]]),
            CanvasItem::Shape(sh) => {
                let corners = sh.world_corners();
                let mut min = [f64::MAX, f64::MAX];
                let mut max = [f64::MIN, f64::MIN];
                for c in corners {
                    min[0] = min[0].min(c[0]);
                    min[1] = min[1].min(c[1]);
                    max[0] = max[0].max(c[0]);
                    max[1] = max[1].max(c[1]);
                }
                (min, max)
            }
        }
    }

    pub fn hit_test(&self, point: [f64; 2], radius: f64) -> bool {
        let (min, max) = self.bounding_box();
        if point[0] < min[0] - radius || point[0] > max[0] + radius ||
           point[1] < min[1] - radius || point[1] > max[1] + radius {
            return false;
        }
        match self {
            CanvasItem::Stroke(s) => {
                let r = radius + (s.base_width as f64 * 0.5);
                let r_sq = r * r;
                if s.points.is_empty() { return false; }
                let lq = [point[0] - s.anchor[0], point[1] - s.anchor[1]];
                if s.points.len() == 1 {
                    let dx = lq[0] - s.points[0].pos[0];
                    let dy = lq[1] - s.points[0].pos[1];
                    return (dx * dx + dy * dy) <= r_sq;
                }
                for i in 0..s.points.len() - 1 {
                    let p0 = s.points[i].pos;
                    let p1 = s.points[i + 1].pos;
                    if segment_dist_sq(p0, p1, lq) <= r_sq {
                        return true;
                    }
                }
                false
            }
            CanvasItem::Shape(sh) => sh.hit_test_local(point, radius),
            CanvasItem::Image(_) => true,
        }
    }

    pub fn translate(&mut self, delta: [f64; 2]) {
        match self {
            CanvasItem::Stroke(s) => {
                s.anchor[0] += delta[0];
                s.anchor[1] += delta[1];
            }
            CanvasItem::Image(img) => {
                img.top_left[0] += delta[0];
                img.top_left[1] += delta[1];
            }
            CanvasItem::Shape(sh) => {
                sh.center[0] += delta[0];
                sh.center[1] += delta[1];
            }
        }
    }
}

/// 점들의 미패딩 로컬 bbox. `Stroke::new()`가 커밋 시점에 1회 호출해서
/// 캐싱하고, `shapes::recognize_shape`가 커밋 전 라이브 포인트에 대해
/// 직접 호출(그리는 중엔 Stroke가 아직 없어서 캐시 재사용 불가 —
/// Hold 이벤트당 1회뿐이라 문제 없음).
pub fn points_bbox(points: &[PenPoint]) -> ([f64; 2], [f64; 2]) {
    let mut min = [f64::MAX, f64::MAX];
    let mut max = [f64::MIN, f64::MIN];
    for p in points {
        min[0] = min[0].min(p.pos[0]);
        min[1] = min[1].min(p.pos[1]);
        max[0] = max[0].max(p.pos[0]);
        max[1] = max[1].max(p.pos[1]);
    }
    (min, max)
}

pub fn segment_dist_sq(a: [f64; 2], b: [f64; 2], p: [f64; 2]) -> f64 {
    let ab_x = b[0] - a[0];
    let ab_y = b[1] - a[1];
    let ap_x = p[0] - a[0];
    let ap_y = p[1] - a[1];
    let ab_len_sq = ab_x * ab_x + ab_y * ab_y;
    if ab_len_sq == 0.0 {
        return ap_x * ap_x + ap_y * ap_y;
    }
    let t = ((ap_x * ab_x + ap_y * ab_y) / ab_len_sq).clamp(0.0, 1.0);
    let c_x = a[0] + t * ab_x;
    let c_y = a[1] + t * ab_y;
    let pc_x = p[0] - c_x;
    let pc_y = p[1] - c_y;
    pc_x * pc_x + pc_y * pc_y
}