// These behavior suites need an explicit final-presentation provider. Keeping
// them in the crate test target makes that provider strictly `cfg(test)` while
// the external facade suites continue to exercise the crates.io path.

#[path = "../tests/backend_frame.rs"]
mod backend_frame;
#[path = "../tests/capture_focus_lifecycle.rs"]
mod capture_focus_lifecycle;
#[path = "../tests/contained_focus_gesture.rs"]
mod contained_focus_gesture;
#[path = "../tests/docking_guides.rs"]
mod docking_guides;
#[path = "../tests/egui_integration.rs"]
mod egui_integration;
#[path = "../tests/keyboard_accessibility.rs"]
mod keyboard_accessibility;
#[path = "../tests/multi_surface_frame.rs"]
mod multi_surface_frame;
#[path = "../tests/splitter_resize.rs"]
mod splitter_resize;
#[path = "../tests/stale_renderer_actions.rs"]
mod stale_renderer_actions;
