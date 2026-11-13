//! Caller-owned native windows with exact software presentation and retirement.

mod app;
mod host;
#[path = "../support/paint.rs"]
mod paint;
mod smoke;
mod workspace;

type AppResult<T> = Result<T, Box<dyn std::error::Error>>;

fn main() -> AppResult<()> {
    app::run()
}
