//! Real X11/XTest multiview docking smoke.

#[cfg(target_os = "linux")]
#[path = "physical_x11/app.rs"]
mod app;
#[cfg(any(target_os = "linux", test))]
#[path = "physical_x11/assertions.rs"]
mod assertions;
#[cfg(target_os = "linux")]
#[path = "physical_x11/driver.rs"]
mod driver;

#[cfg(target_os = "linux")]
#[path = "physical_x11/pointer_load.rs"]
mod pointer_load;

#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    app::run()
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("egui-native-physical-smoke requires Linux/X11");
}
