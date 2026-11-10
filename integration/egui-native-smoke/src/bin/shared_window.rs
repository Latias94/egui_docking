//! Real X11 shared-window composition and Root -> Child -> Root journey.
//!
//! Run alone under an X11 WM with `xdotool`. All pointer edges, output tokens,
//! native-window identities and presentation results come from the real host.

#[cfg(target_os = "linux")]
#[path = "shared_window/app.rs"]
mod app;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("shared_window requires Linux/X11");
}
