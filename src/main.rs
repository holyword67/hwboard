// ============================================================
// src/main.rs
// ============================================================
mod app;
mod gpu;
mod input;
mod render;
mod scene;
mod ui;

use app::App;

fn main() {
    let sdl_context = sdl3::init().expect("SDL3 초기화 실패");
    let video_subsystem = sdl_context.video().expect("video subsystem 실패");

    let mut window = video_subsystem
        .window("hwboard", 1600, 900)
        .position_centered()
        .resizable()
        .build()
        .expect("윈도우 생성 실패");

    let mut app = pollster::block_on(App::new(&window));
    let mut event_pump = sdl_context.event_pump().expect("event pump 실패");

    while app.is_open() {
        for event in event_pump.poll_iter() {
            app.handle_sdl_event(&event, &mut window);
        }
        app.poll();
        app.render();
    }
}