use divan::{black_box, Bencher};
use hwboard::scene::{CanvasItem, PenPoint, Scene, Stroke};
use hwboard::render::tessellate::tessellate_stroke;
// use hwboard::app::shapes::recognize_shape; // pub으로 노출했다고 가정

fn main() {
    divan::main();
}

// ---------------------------------------------------------
// 유틸리티: 테스트용 더미 데이터 생성기
// ---------------------------------------------------------
fn generate_dummy_stroke(point_count: usize) -> Stroke {
    let mut points = Vec::with_capacity(point_count);
    for i in 0..point_count {
        let t = i as f64 * 0.1;
        points.push(PenPoint {
            pos: [t.cos() * 100.0, t.sin() * 100.0], // 나선형 또는 원형 모양
            pressure: (i % 10) as f32 * 0.1 + 0.1,
        });
    }
    Stroke {
        anchor: [0.0, 0.0],
        points,
        color: [0.0, 0.0, 0.0, 1.0],
        base_width: 5.0,
        geometry_dirty: true,
    }
}

/// RDP 알고리즘이 무조건 돌도록 시작점과 끝점이 일치하는(닫힌) 원형 스트로크를 생성합니다.
fn generate_circular_stroke(point_count: usize) -> Stroke {
    let mut points = Vec::with_capacity(point_count);
    // 점이 최소 2개 이상이어야 하므로 안전 장치
    let count = point_count.max(2); 

    for i in 0..count {
        // 0부터 360도(TAU)까지 한 바퀴를 정확히 돕니다.
        let t = (i as f64 / (count - 1) as f64) * std::f64::consts::TAU;
        points.push(PenPoint {
            pos: [t.cos() * 100.0, t.sin() * 100.0],
            pressure: 1.0,
        });
    }
    
    Stroke {
        anchor: [0.0, 0.0],
        points,
        color: [0.0, 0.0, 0.0, 1.0],
        base_width: 5.0,
        geometry_dirty: true,
    }
}

// =========================================================
// 1. 테셀레이션 시스템 벤치마크 (CPU 메싱)
// =========================================================
#[divan::bench(args = [10, 100, 1000, 10000])]
fn bench_tessellate(bencher: Bencher, point_count: usize) {
    bencher
        .with_inputs(|| generate_dummy_stroke(point_count))
        .bench_refs(|stroke| {
            tessellate_stroke(black_box(stroke))
        });
}

// =========================================================
// 2. 씬 시스템 벤치마크 (지우개 충돌 판정 등)
// =========================================================
#[divan::bench(args = [100, 1000, 5000])]
fn bench_scene_hit_test(bencher: Bencher, item_count: usize) {
    bencher
        .with_inputs(|| {
            let mut scene = Scene::new();
            let base_stroke = generate_dummy_stroke(50); // 50포인트짜리 스트로크
            for _ in 0..item_count {
                let id = scene.alloc_id();
                scene.insert(id, CanvasItem::Stroke(base_stroke.clone()));
            }
            scene
        })
        .bench_refs(|scene| {
            // 역순(화면 위부터)으로 히트테스트를 수행하는 씬 스캔 시간 측정
            let hit = scene.iter_ordered_with_id_rev().find_map(|(id, item)| {
                item.hit_test(black_box([50.0, 50.0]), black_box(12.0)).then_some(id)
            });
            black_box(hit);
        });
}

// =========================================================
// 3. 도형 인식 시스템 (RDP 알고리즘 등)
// =========================================================
// 현실적인 사용자의 입력 크기: 
// STROKE_POINT_MIN_DISTANCE_SCREEN_PX 필터가 있기 때문에 
// 사람이 아무리 오래 그려도 점 1000개를 넘기기 매우 힘듭니다.
#[divan::bench(args = [50, 200, 500, 1000])]
fn bench_shape_recognizer(bencher: Bencher, point_count: usize) {
    bencher
        .with_inputs(|| generate_circular_stroke(point_count))
        .bench_local_values(|stroke| {
            // 입력(stroke)과 출력(result) 모두 black_box로 감싸서
            // 컴파일러가 코드를 임의로 삭제하지 못하게 강제합니다.
            let result = hwboard::app::shapes::recognize_shape(black_box(&stroke));
            black_box(result);
        });
}