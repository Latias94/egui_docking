fn main() {
    println!("cargo::rustc-check-cfg=cfg(egui_backend_event_envelope)");
}
