// ============================================================
// src/scene/item.rs
// ============================================================
pub type ItemId = u64;

#[derive(Debug, Clone)]
pub enum CanvasItem {
    Stroke(Stroke),
    Image(ImageItem),
    Shape(Shape),
}

#[derive(Debug, Clone)]
pub struct PenPoint {
    pub pos: [f64; 2],
    pub pressure: f32,
}

#[derive(Debug, Clone)]
pub struct Stroke {
    pub points: Vec<PenPoint>,
    pub color: [f32; 4],
    pub base_width: f32,
    pub mesh_dirty: bool,
}

#[derive(Debug, Clone)]
pub struct ImageItem {
    pub top_left: [f64; 2],
    pub size: [f64; 2],
    pub pixel_width: u32,
    pub pixel_height: u32,
    pub rgba: std::sync::Arc<[u8]>,
    pub mesh_dirty: bool, // 추가됨: 이동/리사이즈 될때 GPU 버텍스 갱신을 알리기 위함
}

impl ImageItem {
    pub fn set_bounds(&mut self, top_left: [f64; 2], size: [f64; 2]) {
        self.top_left = top_left;
        self.size = size;
        self.mesh_dirty = true; // 변경 알림
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ShapeKind {
    Circle,
    Line,
    Rectangle,
    Triangle, // 추가됨
}

#[derive(Debug, Clone)]
pub struct Shape {
    pub kind: ShapeKind,
    pub center: [f64; 2],
    pub half_extent: [f64; 2],
    pub rotation: f32,
    pub color: [f32; 4],
    pub stroke_width: f32,
    pub mesh_dirty: bool,
}

impl Shape {
    fn local_outline(&self) -> Vec<[f64; 2]> {
        let (hx, hy) = (self.half_extent[0], self.half_extent[1]);
        match self.kind {
            ShapeKind::Rectangle => vec![
                [-hx, -hy], [hx, -hy], [hx, hy], [-hx, hy], [-hx, -hy],
            ],
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
            ShapeKind::Triangle => vec![
                [0.0, -hy], [hx, hy], [-hx, hy], [0.0, -hy], // 위(중앙), 우측하단, 좌측하단, 닫기
            ],
        }
    }

    pub fn world_outline(&self) -> Vec<[f64; 2]> {
        let sin_r = (self.rotation as f64).sin();
        let cos_r = (self.rotation as f64).cos();
        self.local_outline()
            .into_iter()
            .map(|[lx, ly]| {
                let rx = lx * cos_r - ly * sin_r;
                let ry = lx * sin_r + ly * cos_r;
                [self.center[0] + rx, self.center[1] + ry]
            })
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
        Stroke {
            points: self.world_outline().into_iter().map(|pos| PenPoint { pos, pressure: 1.0 }).collect(),
            color: self.color,
            base_width: self.stroke_width,
            mesh_dirty: true,
        }
    }
}

impl CanvasItem {
    pub fn bounding_box(&self) -> ([f64; 2], [f64; 2]) {
        match self {
            CanvasItem::Stroke(s) => stroke_bbox(s),
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
                if s.points.len() == 1 {
                    let dx = point[0] - s.points[0].pos[0];
                    let dy = point[1] - s.points[0].pos[1];
                    return (dx * dx + dy * dy) <= r_sq;
                }
                for i in 0..s.points.len() - 1 {
                    let p0 = s.points[i].pos;
                    let p1 = s.points[i + 1].pos;
                    if segment_dist_sq(p0, p1, point) <= r_sq {
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
                for p in s.points.iter_mut() {
                    p.pos[0] += delta[0];
                    p.pos[1] += delta[1];
                }
                s.mesh_dirty = true;
            }
            CanvasItem::Image(img) => {
                img.top_left[0] += delta[0];
                img.top_left[1] += delta[1];
                img.mesh_dirty = true; // 변경 알림
            }
            CanvasItem::Shape(sh) => {
                sh.center[0] += delta[0];
                sh.center[1] += delta[1];
                sh.mesh_dirty = true;
            }
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