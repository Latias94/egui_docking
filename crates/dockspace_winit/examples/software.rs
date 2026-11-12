//! Renderer-neutral docking in a software-rendered winit window.

#[path = "software/app.rs"]
mod app;
#[path = "software/editor.rs"]
mod editor;
#[path = "software/paint.rs"]
mod paint;
#[path = "software/smoke.rs"]
mod smoke;

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> AppResult<()> {
    app::run()
}
