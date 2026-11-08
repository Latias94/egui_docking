//! Finite contained-tab redocking regression using real XTest input.
//!
//! Run alone under X11 with Xvfb, Openbox, and xdotool. AccessKit supplies only
//! pointer coordinates; the native application's semantic view and actual
//! presentation readiness decide whether either drag has committed.

#[cfg(not(target_os = "linux"))]
fn main() {
    println!("contained_redock_x11 requires X11 on Linux");
}

#[cfg(target_os = "linux")]
fn main() -> eframe::Result {
    x11::run()
}

#[cfg(target_os = "linux")]
mod x11 {
    use std::process::{Command, Stdio};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::{Duration, Instant};

    use dockspace::geometry::LogicalRect;
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, DockspaceView,
        FloatingPresentationId, ItemId, RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::DockspaceSession;
    use eframe::egui;
    use egui_dockspace::{DockStyle, PaneResponse, PaneView};
    use egui_dockspace_native::NativeDockspaceApp;
    use winit::platform::x11::EventLoopBuilderExtX11 as _;

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const ROOT: RootId = RootId::new(1);
    const FIRST: ItemId = ItemId::new(1);
    const SECOND: ItemId = ItemId::new(2);
    const STAGE_TIMEOUT: Duration = Duration::from_secs(15);
    const STABLE_TIME: Duration = Duration::from_millis(100);
    const INPUT_PAUSE: Duration = Duration::from_millis(100);

    fn item_index(item: ItemId) -> usize {
        match item {
            FIRST => 0,
            SECOND => 1,
            _ => panic!("unexpected pane {item:?}"),
        }
    }

    #[derive(Default)]
    struct Panes {
        callbacks: usize,
        content: [Option<egui::Rect>; 2],
    }

    impl PaneView for Panes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(["First", "Second"][item_index(item)].into())
        }

        fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
            let index = item_index(item);
            let response = ui.allocate_rect(ui.max_rect(), egui::Sense::hover());
            self.callbacks += 1;
            self.content[index] = Some(response.rect);
            ui.painter().rect_filled(
                response.rect,
                0.0,
                [
                    egui::Color32::from_rgb(32, 55, 75),
                    egui::Color32::from_rgb(58, 45, 78),
                ][index],
            );
            ui.painter().text(
                response.rect.min + egui::vec2(16.0, 16.0),
                egui::Align2::LEFT_TOP,
                ["First content", "Second content"][index],
                egui::FontId::proportional(20.0),
                egui::Color32::WHITE,
            );
            PaneResponse::from(&response)
        }
    }

    #[derive(Debug, Default)]
    struct TabCoordinates {
        pass: u64,
        second: Vec<egui::Rect>,
    }

    struct TabProbe(Arc<Mutex<TabCoordinates>>);

    impl egui::plugin::Plugin for TabProbe {
        fn debug_name(&self) -> &'static str {
            "contained-redock-x11-tab-coordinates"
        }

        #[allow(clippy::cast_possible_truncation)]
        fn output_hook(&mut self, context: &egui::Context, output: &mut egui::FullOutput) {
            if context.viewport_id() != egui::ViewportId::ROOT {
                return;
            }
            let mut coordinates = self.0.lock().expect("tab coordinate lock");
            coordinates.pass = context.cumulative_pass_nr();
            coordinates.second.clear();
            if let Some(update) = &output.platform_output.accesskit_update {
                for (_, node) in &update.nodes {
                    if node.role() == egui::accesskit::Role::Tab
                        && node.label() == Some("Second")
                        && let Some(bounds) = node.bounds()
                    {
                        // These read-only diagnostics originate in egui's f32 geometry.
                        let rect = egui::Rect::from_min_max(
                            egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                            egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                        );
                        if rect.is_finite() && rect.is_positive() {
                            coordinates.second.push(rect);
                        }
                    }
                }
            }
        }
    }

    #[derive(Debug, Clone, Copy)]
    struct Drag {
        window: u64,
        pixels_per_point: f32,
        source: egui::Pos2,
        target: egui::Pos2,
    }

    fn xdotool(arguments: &[&str]) -> Result<(), String> {
        let mut child = Command::new("xdotool")
            .args(arguments)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| format!("xdotool {arguments:?}: {error}"))?;
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match child.try_wait() {
                Ok(Some(_)) => break,
                Ok(None) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(10));
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(format!("xdotool {arguments:?} did not finish: {result:?}"));
                }
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|error| format!("xdotool {arguments:?}: {error}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(format!(
                "xdotool {arguments:?}: {}: {}",
                output.status,
                String::from_utf8_lossy(&output.stderr),
            ))
        }
    }

    fn move_pointer(drag: Drag, point: egui::Pos2) -> Result<(), String> {
        let scale = f64::from(drag.pixels_per_point);
        let x = format!("{:.0}", f64::from(point.x) * scale);
        let y = format!("{:.0}", f64::from(point.y) * scale);
        xdotool(&["mousemove", "--window", &drag.window.to_string(), &x, &y])
    }

    fn drag_pointer(drag: Drag) -> Result<(), String> {
        eprintln!("contained-redock-x11: real drag {drag:?}");
        move_pointer(drag, drag.source)?;
        thread::sleep(INPUT_PAUSE);
        xdotool(&["mousedown", "1"])?;
        thread::sleep(INPUT_PAUSE);
        move_pointer(drag, drag.source + egui::vec2(12.0, 8.0))?;
        thread::sleep(INPUT_PAUSE);
        move_pointer(drag, drag.target)?;
        thread::sleep(INPUT_PAUSE);
        xdotool(&["mouseup", "1"])
    }

    fn drive(gestures: Receiver<Drag>) -> Result<(), String> {
        let tear_off = gestures
            .recv_timeout(STAGE_TIMEOUT)
            .map_err(|error| format!("waiting for stable live surface: {error}"))?;
        xdotool(&["windowactivate", "--sync", &tear_off.window.to_string()])?;
        thread::sleep(INPUT_PAUSE);
        xdotool(&["keydown", "Shift_L"])?;
        thread::sleep(INPUT_PAUSE);
        drag_pointer(tear_off)?;

        // The UI sends this only after the full-surface contained commit is presented.
        let redock = gestures
            .recv_timeout(STAGE_TIMEOUT)
            .map_err(|error| format!("waiting for contained commit: {error}"))?;
        xdotool(&["keyup", "Shift_L"])?;
        thread::sleep(INPUT_PAUSE);
        drag_pointer(redock)
    }

    #[derive(Debug, Clone, Copy)]
    enum Stage {
        Live,
        Contained,
        Redocked(FloatingPresentationId),
        Done,
    }

    fn root_tabs<'a>(view: DockspaceView<'a>) -> Option<&'a [ItemId]> {
        let root = view.surface(SURFACE)?.main_root()?;
        if root.id() != ROOT {
            return None;
        }
        Some(root.content()?.tabs()?.items())
    }

    struct RegressionApp {
        dockspace: NativeDockspaceApp<Panes>,
        coordinates: Arc<Mutex<TabCoordinates>>,
        gestures: SyncSender<Drag>,
        driver_result: Receiver<bool>,
        driver_done: bool,
        completed: Arc<AtomicBool>,
        stage: Stage,
        stage_started: Instant,
        stage_pass: u64,
        stable_since: Option<Instant>,
        post_live: bool,
        passes: usize,
    }

    impl RegressionApp {
        fn stable(&mut self, ready: bool) -> bool {
            if !ready {
                self.stable_since = None;
                return false;
            }
            self.stable_since.get_or_insert_with(Instant::now).elapsed() >= STABLE_TIME
        }

        fn advance(&mut self, stage: Stage, context: &egui::Context) {
            eprintln!("contained-redock-x11: {stage:?}");
            self.stage = stage;
            self.stage_started = Instant::now();
            self.stage_pass = context.cumulative_pass_nr();
            self.stable_since = None;
        }

        fn drag(&self, context: &egui::Context) -> Option<Drag> {
            let coordinates = self.coordinates.lock().expect("tab coordinate lock");
            if coordinates.pass <= self.stage_pass || coordinates.second.is_empty() {
                return None;
            }
            assert_eq!(coordinates.second.len(), 1, "one actual Second tab");
            let tab = coordinates.second[0];
            // Role::Tab bounds describe the draggable body. Stay on its left side,
            // away from the separate close control at the tab's right edge.
            let source = egui::pos2(tab.left() + tab.width() * 0.35, tab.center().y);
            let content = self.dockspace.with_panes(|panes| panes.content[0])?;
            let target = content.min + content.size() * 0.35;
            assert!(content.is_finite() && content.is_positive());
            assert!(content.contains(target));
            assert!(source.distance(target) > 32.0, "drag must leave the tab");
            let token = eframe::current_native_output_token().expect("real native output callback");
            Some(Drag {
                window: u64::from(token.window_id()),
                pixels_per_point: context.pixels_per_point(),
                source,
                target,
            })
        }
    }

    impl eframe::App for RegressionApp {
        fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
            let surface = ui.available_rect_before_wrap();
            let before = self.dockspace.with_panes(|panes| panes.callbacks);
            self.dockspace.ui(ui, frame);
            self.passes += 1;
            let painted = self.dockspace.with_panes(|panes| panes.callbacks) != before;
            let retained = self.dockspace.current_output_is_retained();
            let discarded = ui.ctx().will_discard();
            let presented = self.dockspace.is_surface_presented(SURFACE);
            self.post_live |= presented;
            assert!(
                !self.post_live || painted || retained || discarded,
                "blank native pass {} in {:?}: no PaneView callback, retention, or discard",
                self.passes,
                self.stage,
            );
            self.dockspace.with_errors(|primary, cleanup| {
                assert!(
                    primary.is_none() && cleanup.is_empty(),
                    "native errors in {:?}: {primary:?}; cleanup: {cleanup:?}",
                    self.stage,
                );
            });
            let settled = presented && self.dockspace.is_quiescent() && !retained && !discarded;

            if !self.driver_done {
                match self.driver_result.try_recv() {
                    Ok(true) => self.driver_done = true,
                    Ok(false) | Err(TryRecvError::Disconnected) => {
                        // main joins the driver and propagates its original error or panic.
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                        return;
                    }
                    Err(TryRecvError::Empty) => {}
                }
            }
            assert!(
                self.stage_started.elapsed() < STAGE_TIMEOUT,
                "timed out in {:?}; presented={presented}, quiescent={}, retained={retained}, discarded={discarded}; view={:?}; tabs={:?}; content={:?}",
                self.stage,
                self.dockspace.is_quiescent(),
                self.dockspace.with_view(|view| format!("{view:?}")),
                self.coordinates.lock().expect("tab coordinate lock"),
                self.dockspace.with_panes(|panes| panes.content),
            );

            match self.stage {
                Stage::Live => {
                    let initial = self.dockspace.with_view(|view| {
                        root_tabs(view) == Some([FIRST, SECOND].as_slice())
                            && view.surfaces().map(|surface| surface.id()).eq([SURFACE])
                            && view.surface(SURFACE).unwrap().contained_count() == 0
                    });
                    if self.stable(initial && settled && painted)
                        && let Some(drag) = self.drag(ui.ctx())
                    {
                        self.gestures.try_send(drag).expect("send real tear-off");
                        self.advance(Stage::Contained, ui.ctx());
                    }
                }
                Stage::Contained => {
                    let contained = self.dockspace.with_view(|view| {
                        let second = view.item(SECOND)?;
                        let floating = second.contained()?;
                        assert_eq!(root_tabs(view), Some([FIRST].as_slice()));
                        assert_eq!(second.surface(), SURFACE);
                        assert_ne!(second.root(), ROOT);
                        assert_eq!(second.tabs().items(), &[SECOND]);
                        assert!(second.is_selected());
                        assert_eq!(view.surfaces().count(), 1);
                        assert_eq!(view.surface(SURFACE)?.contained_count(), 1);
                        Some((floating, view.contained(floating)?.rect()))
                    });
                    if self.stable(contained.is_some() && settled && painted)
                        && let Some((floating, rect)) = contained
                    {
                        let expected = LogicalRect::new(
                            f64::from(surface.min.x),
                            f64::from(surface.min.y),
                            f64::from(surface.width()),
                            f64::from(surface.height()),
                        )
                        .expect("positive full-window surface");
                        assert_eq!(rect, expected, "contained must cover the entire surface");
                        if let Some(drag) = self.drag(ui.ctx()) {
                            self.gestures.try_send(drag).expect("send real redock");
                            self.advance(Stage::Redocked(floating), ui.ctx());
                        }
                    }
                }
                Stage::Redocked(floating) => {
                    let redocked = self.dockspace.with_view(|view| {
                        root_tabs(view) == Some([FIRST, SECOND].as_slice())
                            && view.contained(floating).is_none()
                            && view.surface(SURFACE).unwrap().contained_count() == 0
                            && view.surfaces().map(|surface| surface.id()).eq([SURFACE])
                            && view.item(SECOND).is_some_and(|second| {
                                second.surface() == SURFACE
                                    && second.root() == ROOT
                                    && second.contained().is_none()
                                    && second.is_selected()
                            })
                    });
                    if self.stable(redocked && settled && painted && self.driver_done) {
                        self.completed.store(true, Ordering::SeqCst);
                        self.advance(Stage::Done, ui.ctx());
                        eprintln!(
                            "contained-redock-x11: full-surface contained Second redocked to exact root tabs [First, Second], selected, without native errors or blank passes ({} UI passes)",
                            self.passes,
                        );
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                Stage::Done => ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close),
            }
            ui.ctx().request_repaint_after(Duration::from_millis(16));
        }
    }

    pub fn run() -> eframe::Result {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
        )])
        .expect("valid two-tab layout");
        let session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("valid native session");
        let dockspace = NativeDockspaceApp::new(
            egui::Id::unique("contained-redock-x11"),
            session,
            SURFACE,
            Panes::default(),
            DockStyle::default(),
        )
        .expect("native dockspace initializes");
        let options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([640.0, 480.0])
                .with_position([40.0, 40.0])
                .with_resizable(false),
            native_host: Some(dockspace.native_host_handler()),
            renderer: eframe::Renderer::Glow,
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_x11();
            })),
            ..Default::default()
        };
        let (gestures, commands) = mpsc::sync_channel(1);
        let (driver_finished, driver_result) = mpsc::sync_channel(1);
        let driver = thread::Builder::new()
            .name("contained-redock-x11-driver".into())
            .spawn(move || {
                let result = drive(commands);
                if result.is_err() {
                    let _ = xdotool(&["mouseup", "1", "keyup", "Shift_L"]);
                }
                let _ = driver_finished.send(result.is_ok());
                result
            })
            .expect("spawn X11 input driver");
        let completed = Arc::new(AtomicBool::new(false));
        let app_completed = Arc::clone(&completed);
        // Always join on the main thread, including when a UI assertion unwinds.
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            eframe::run_native(
                "Dockspace contained redock X11 regression",
                options,
                Box::new(move |creation| {
                    creation.egui_ctx.enable_accesskit();
                    let coordinates = Arc::new(Mutex::new(TabCoordinates::default()));
                    creation
                        .egui_ctx
                        .add_plugin(TabProbe(Arc::clone(&coordinates)));
                    Ok(Box::new(RegressionApp {
                        dockspace,
                        coordinates,
                        gestures,
                        driver_result,
                        driver_done: false,
                        completed: app_completed,
                        stage: Stage::Live,
                        stage_started: Instant::now(),
                        stage_pass: 0,
                        stable_since: None,
                        post_live: false,
                        passes: 0,
                    }))
                }),
            )
        }));
        let driven = driver.join();
        match run {
            Ok(result) => result?,
            Err(panic) => std::panic::resume_unwind(panic),
        }
        match driven {
            Ok(Ok(())) => {}
            Ok(Err(error)) => panic!("X11 input driver failed: {error}"),
            Err(panic) => std::panic::resume_unwind(panic),
        }
        assert!(
            completed.load(Ordering::SeqCst),
            "native event loop exited before contained redocking completed",
        );
        Ok(())
    }
}
