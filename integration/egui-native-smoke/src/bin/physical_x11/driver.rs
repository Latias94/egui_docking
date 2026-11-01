//! X11/XTest scenario driver for the physical docking smoke.

#[path = "driver/inspection.rs"]
mod inspection;
#[path = "driver/scenario.rs"]
mod scenario;
#[path = "driver/x11.rs"]
mod x11;

pub(crate) use scenario::run;
pub(super) use x11::StationaryInputBlocker;
