use divan::{black_box, Bencher};
use hwboard::scene::{CanvasItem, PenPoint, Scene, Stroke};
use hwboard::render::tessellate::tessellate_stroke;

fn main() {
    divan::main();
}

fn generate_dummy_stroke(point_count: usize) -> Stroke {
    let mut points = Vec::with_capacity(point_count);
    for i in 0..point_count {
        let t = i as f64 * 0.1;
        points.push(PenPoint {
            pos: [t.cos() * 100.0, t.sin() * 100.0],
            pressure: (i % 10) as f32 * 0.1 + 0.1,
        });
    }
    Stroke::new([0.0, 0.0], points, [0.0, 0.0, 0.0, 1.0], 5.0)
}

/// RDP 알고리즘이 무조건 돌도록 시작점과 끝점이 일치하는(닫힌) 원형 스트로크를 생성합니다.
fn generate_circular_stroke(point_count: usize) -> Stroke {
    let mut points = Vec::with_capacity(point_count);
    let count = point_count.max(2);

    for i in 0..count {
        let t = (i as f64 / (count - 1) as f64) * std::f64::consts::TAU;
        points.push(PenPoint {
            pos: [t.cos() * 100.0, t.sin() * 100.0],
            pressure: 1.0,
        });
    }

    Stroke::new([0.0, 0.0], points, [0.0, 0.0, 0.0, 1.0], 5.0)
}

#[divan::bench(args = [10, 100, 1000, 10000])]
fn bench_tessellate(bencher: Bencher, point_count: usize) {
    bencher
        .with_inputs(|| generate_dummy_stroke(point_count))
        .bench_refs(|stroke| {
            tessellate_stroke(black_box(stroke))
        });
}

#[divan::bench(args = [100, 1000, 5000])]
fn bench_scene_hit_test(bencher: Bencher, item_count: usize) {
    bencher
        .with_inputs(|| {
            let mut scene = Scene::new();
            let base_stroke = generate_dummy_stroke(50);
            for _ in 0..item_count {
                let id = scene.alloc_id();
                scene.insert(id, CanvasItem::Stroke(base_stroke.clone()));
            }
            scene
        })
        .bench_refs(|scene| {
            let hit = scene.iter_ordered_with_id_rev().find_map(|(id, item)| {
                item.hit_test(black_box([50.0, 50.0]), black_box(12.0)).then_some(id)
            });
            black_box(hit);
        });
}

#[divan::bench(args = [50, 200, 500, 1000])]
fn bench_shape_recognizer(bencher: Bencher, point_count: usize) {
    bencher
        .with_inputs(|| generate_circular_stroke(point_count))
        .bench_local_values(|stroke| {
            let result = hwboard::app::shapes::recognize_shape(
                black_box(&stroke.points),
                stroke.color,
                stroke.base_width,
            );
            black_box(result);
        });
}