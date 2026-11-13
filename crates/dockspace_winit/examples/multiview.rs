//! Caller-owned native windows with exact software presentation and retirement.

#[path = "multiview/app.rs"]
mod app;
#[path = "multiview/host.rs"]
mod host;
#[path = "support/paint.rs"]
mod paint;
#[path = "multiview/smoke.rs"]
mod smoke;
#[path = "multiview/workspace.rs"]
mod workspace;

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> AppResult<()> {
    app::run()
}
