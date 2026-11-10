//! Small public consumer; the physical driver owns no docking or host protocol.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{DockspaceSession, NativeSurfacePlacement};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneResponse, PaneView};
use egui_dockspace_native::NativeDockspaceApp;
use winit::platform::x11::EventLoopBuilderExtX11 as _;
use x11rb::connection::Connection as _;
use x11rb::protocol::xproto::ConnectionExt as _;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(11);
const DETACH: ItemId = ItemId::new(12);
const TITLE: &str = "Dockspace shared window";
const TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug)]
struct PaneGeometry {
    viewport: egui::ViewportId,
    token: eframe::NativeOutputToken,
    transform: egui::emath::TSTransform,
    pixels_per_point: f32,
    tab: [i32; 2],
    center: [i32; 2],
}

impl PaneGeometry {
    #[allow(clippy::cast_possible_truncation)]
    fn matches_placement(self, placement: NativeSurfacePlacement) -> bool {
        let pixels = f64::from(self.pixels_per_point);
        pixels.is_finite()
            && pixels > 0.0
            && self.transform
                == egui::emath::TSTransform::new(
                    egui::vec2(
                        (placement.origin().x() / pixels) as f32,
                        (placement.origin().y() / pixels) as f32,
                    ),
                    (placement.scale_factor().get() / pixels) as f32,
                )
    }
}

#[derive(Default)]
struct Shared {
    items: BTreeMap<ItemId, SurfaceId>,
    pending_geometry: BTreeMap<ItemId, PaneGeometry>,
    geometry: BTreeMap<ItemId, PaneGeometry>,
    presented: bool,
    child_presented: bool,
    shifted: bool,
    shifted_ready: bool,
    request_reset: bool,
    reset_submitted: bool,
    reset_done: bool,
    error: Option<String>,
    completed: bool,
    stage: &'static str,
    input_diagnostics: VecDeque<String>,
}

type State = Arc<Mutex<Shared>>;

fn lock(state: &State) -> MutexGuard<'_, Shared> {
    state
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct Panes {
    shared: State,
    text: BTreeMap<ItemId, String>,
    observed_viewports: BTreeSet<egui::ViewportId>,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let viewport = ui.ctx().viewport_id();
        let Some(token) = eframe::current_native_output_token() else {
            lock(&self.shared).error = Some("pane callback has no real output token".into());
            return PaneResponse::default();
        };
        if self.observed_viewports.len() < 4 && self.observed_viewports.insert(viewport) {
            eprintln!(
                "shared-window pane callback: item={item:?} context={viewport:?} token={:?}",
                eframe::current_native_output_token(),
            );
        }
        let rect = ui.max_rect();
        let transform = ui
            .ctx()
            .layer_transform_to_global(ui.layer_id())
            .unwrap_or(egui::emath::TSTransform::IDENTITY);
        let pixels = ui.ctx().pixels_per_point();
        let point = |point: egui::Pos2| {
            let point = transform * point;
            #[allow(clippy::cast_possible_truncation)]
            [
                (point.x * pixels).round() as i32,
                (point.y * pixels).round() as i32,
            ]
        };
        lock(&self.shared).pending_geometry.insert(
            item,
            PaneGeometry {
                viewport,
                token,
                transform,
                pixels_per_point: pixels,
                // Replaced by the actual Tab node from the same output hook.
                tab: [0, 0],
                center: point(rect.center()),
            },
        );
        let label = ui.label(format!("Independent surface pane {}", item.get()));
        let response =
            ui.text_edit_singleline(self.text.entry(item).or_insert_with(|| "editable".into()));
        PaneResponse::default()
            .with_content(&label)
            .with_content(&response)
    }
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([FIRST, DETACH, ItemId::new(13)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::central_tabs([ItemId::new(21), ItemId::new(22)]),
            ),
        ),
    ])
    .unwrap()
}

fn placements(shifted: bool) -> [NativeSurfacePlacement; 2] {
    [
        NativeSurfacePlacement::new(
            A,
            LogicalSize::new(340.0, 440.0).unwrap(),
            PhysicalPoint::new(24.0, 34.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(24.0, 34.0, 340.0, 440.0).unwrap(),
        ),
        NativeSurfacePlacement::new(
            B,
            LogicalSize::new(300.0, 340.0).unwrap(),
            PhysicalPoint::new(if shifted { 450.0 } else { 420.0 }, 64.0).unwrap(),
            ScaleFactor::new(if shifted { 1.4 } else { 1.25 }).unwrap(),
            PhysicalRect::new(if shifted { 450.0 } else { 420.0 }, 64.0, 360.0, 390.0).unwrap(),
        ),
    ]
}

struct App {
    dockspace: NativeDockspaceApp<Panes>,
    shared: State,
    started: Instant,
    discard: bool,
}

#[derive(Default)]
struct AccessibilityTree {
    root: Option<egui::accesskit::NodeId>,
    nodes: BTreeMap<egui::accesskit::NodeId, egui::accesskit::Node>,
}

impl AccessibilityTree {
    fn apply(&mut self, update: &egui::accesskit::TreeUpdate) -> Result<(), String> {
        const MAX_NODES: usize = 4096;
        if self.nodes.len() + update.nodes.len() > MAX_NODES {
            return Err("shared-window accessibility tree exceeds the smoke's node budget".into());
        }
        if let Some(tree) = &update.tree {
            self.root = Some(tree.root);
        }
        for (id, node) in &update.nodes {
            self.nodes.insert(*id, node.clone());
        }
        let root = self
            .root
            .ok_or("shared-window accessibility update omitted its initial root")?;
        let mut reachable = BTreeSet::new();
        let mut pending = vec![root];
        while let Some(id) = pending.pop() {
            if !reachable.insert(id) {
                return Err("shared-window accessibility tree repeats a child".into());
            }
            let node = self
                .nodes
                .get(&id)
                .ok_or_else(|| format!("shared-window accessibility tree lacks {id:?}"))?;
            pending.extend_from_slice(node.children());
        }
        self.nodes.retain(|id, _| reachable.contains(id));
        Ok(())
    }

    fn bounds(&self, id: egui::accesskit::NodeId) -> Option<egui::accesskit::Rect> {
        let node = self.nodes.get(&id)?;
        let bounds = node.bounds()?;
        let mut transform = node.transform().copied().unwrap_or_default();
        let mut current = id;
        for _ in 0..self.nodes.len() {
            if Some(current) == self.root {
                return Some(transform.transform_rect_bbox(bounds));
            }
            let (parent_id, parent) = self
                .nodes
                .iter()
                .find(|(_, parent)| parent.children().contains(&current))?;
            transform = parent.transform().copied().unwrap_or_default() * transform;
            current = *parent_id;
        }
        None
    }
}

struct OutputGeometry {
    shared: State,
    trees: BTreeMap<egui::ViewportId, AccessibilityTree>,
    observed_hooks: BTreeSet<(egui::ViewportId, Option<egui::ViewportId>)>,
}

impl egui::plugin::Plugin for OutputGeometry {
    fn debug_name(&self) -> &'static str {
        "shared-window-output-geometry"
    }

    fn output_hook(&mut self, context: &egui::Context, output: &mut egui::FullOutput) {
        let mut shared = lock(&self.shared);
        let context_viewport = context.viewport_id();
        let token = eframe::current_native_output_token();
        let native_viewport = token.map(|token| token.viewport_id());
        if self.observed_hooks.len() < 8
            && self
                .observed_hooks
                .insert((context_viewport, native_viewport))
        {
            eprintln!(
                "shared-window output hook: context={context_viewport:?} token={token:?} accesskit={:?} viewport_outputs={:?} pending={:?}",
                output
                    .platform_output
                    .accesskit_update
                    .as_ref()
                    .map(|update| (update.nodes.len(), update.tree.is_some())),
                output.viewport_output.keys().collect::<Vec<_>>(),
                shared.pending_geometry,
            );
        }
        let Some(token) = token else {
            shared.error = Some("shared-window output hook has no real output token".into());
            return;
        };
        let viewport = token.viewport_id();
        if let Some(update) = &output.platform_output.accesskit_update {
            if !self.trees.contains_key(&viewport) && self.trees.len() == 4 {
                shared.error = Some("shared-window smoke exceeded four viewport trees".into());
                return;
            }
            if let Err(error) = self.trees.entry(viewport).or_default().apply(update) {
                shared.error = Some(error);
                return;
            }
        }
        let Some(tree) = self.trees.get(&viewport) else {
            return;
        };
        let authority = context.capture_completed_pass_hit_authority(viewport);
        // The context may already have restored Root after a Child pass. Only
        // read ambient input when it belongs to this exact native output.
        let pointer = (context_viewport == viewport)
            .then(|| {
                context.input(|input| {
                    (input.pointer.any_down()
                        || input.pointer.any_pressed()
                        || input.pointer.any_released())
                    .then(|| {
                        (
                            input.pointer.interact_pos(),
                            input.pointer.any_down(),
                            input.pointer.any_pressed(),
                            input.pointer.any_released(),
                        )
                    })
                })
            })
            .flatten();
        if let Some((point, down, pressed, released)) = pointer {
            let hit = point.and_then(|point| {
                authority
                    .as_ref()
                    .and_then(|authority| authority.hit_test(point))
            });
            let diagnostic = format!(
                "stage={} viewport={:?} pass={} pointer={point:?} down={down} pressed={pressed} released={released} hit={hit:?}",
                shared.stage,
                viewport,
                context.cumulative_pass_nr_for(viewport),
            );
            if shared.input_diagnostics.len() == 24 {
                shared.input_diagnostics.pop_front();
            }
            shared.input_diagnostics.push_back(diagnostic);
        }
        // TreeUpdate is a delta. Retain unchanged placement ancestors while
        // pruning detached subtrees, then resolve every Tab in the current tree.
        for (id, node) in &tree.nodes {
            if node.role() != egui::accesskit::Role::Tab {
                continue;
            }
            let Some(item) = node
                .label()
                .and_then(|label| label.strip_prefix("Pane "))
                .and_then(|number| number.parse::<u64>().ok())
                .map(ItemId::new)
            else {
                continue;
            };
            let Some(bounds) = tree.bounds(*id) else {
                continue;
            };
            if let Some(geometry) = shared.pending_geometry.get_mut(&item)
                && geometry.viewport == viewport
            {
                #[allow(clippy::cast_possible_truncation)]
                {
                    geometry.tab = [
                        ((bounds.x0 + bounds.x1) * 0.5).round() as i32,
                        ((bounds.y0 + bounds.y1) * 0.5).round() as i32,
                    ];
                }
                let point = egui::pos2(
                    geometry.tab[0] as f32 / output.pixels_per_point,
                    geometry.tab[1] as f32 / output.pixels_per_point,
                );
                let hit = authority
                    .as_ref()
                    .and_then(|authority| authority.hit_test(point));
                if shared.stage == "A -> B" {
                    let label = node.label().unwrap_or_default();
                    if shared.input_diagnostics.is_empty() {
                        eprintln!(
                            "shared-window initial {label}: accesskit={id:?} client_pixels={bounds:?} point={point:?} hit={hit:?}"
                        );
                    }
                }
            }
        }
        // This smoke requests repeats only before its third pass. Its own
        // output carries that request even when the context has restored Root.
        if output.platform_output.request_discard_reasons.is_empty() {
            if let Some(item) = shared.pending_geometry.iter().find_map(|(item, geometry)| {
                (geometry.viewport == viewport
                    && (geometry.tab == [0, 0]
                        || geometry.token != token
                        || geometry.pixels_per_point.to_bits()
                            != output.pixels_per_point.to_bits()))
                .then_some(*item)
            }) {
                shared.error = Some(format!(
                    "stage={}: pane {item:?} lacks exact geometry for the completed output",
                    shared.stage,
                ));
                return;
            }
            // The driver sees only complete viewport snapshots. A pane callback
            // must not erase its last completed Tab while this pass is building.
            let Shared {
                pending_geometry,
                geometry,
                ..
            } = &mut *shared;
            pending_geometry.retain(|item, pending| {
                if pending.viewport != viewport {
                    return true;
                }
                geometry.insert(*item, *pending);
                false
            });
        } else {
            shared
                .pending_geometry
                .retain(|_, pending| pending.viewport != viewport);
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if self.started.elapsed() > Duration::from_secs(90) {
            lock(&self.shared).error = Some("shared-window journey timed out".into());
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let shifted = lock(&self.shared).shifted;
        self.dockspace.show_root_surfaces(ui, &placements(shifted));
        if lock(&self.shared).error.is_some() {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        if self.discard && ui.ctx().current_pass_index() < 2 {
            ui.ctx()
                .request_discard("shared-window late-discard acceptance");
        }
        if let Some(error) = self.dockspace.error_kind() {
            self.dockspace.with_errors(|primary, _| {
                lock(&self.shared).error = Some(format!("native {error:?}: {primary:?}"));
            });
            return;
        }
        let should_reset = {
            let shared = lock(&self.shared);
            shared.request_reset && !shared.reset_submitted
        };
        if should_reset {
            match self.dockspace.request_layout_reset(layout()) {
                Ok(()) => lock(&self.shared).reset_submitted = true,
                Err(error) => {
                    lock(&self.shared).error = Some(format!("reset admission: {error:?}"))
                }
            }
        }
        if let Some(result) = self.dockspace.take_action_status() {
            match result {
                Ok(egui_dockspace_native::NativeActionStatus::LayoutReset { .. }) => {
                    lock(&self.shared).reset_done = true
                }
                other => {
                    lock(&self.shared).error =
                        Some(format!("unexpected application status: {other:?}"))
                }
            }
        }
        let items = self.dockspace.with_view(|view| {
            [FIRST, DETACH, ItemId::new(21), ItemId::new(22)]
                .into_iter()
                .filter_map(|item| view.item(item).map(|view| (item, view.surface())))
                .collect::<BTreeMap<_, _>>()
        });
        let presented =
            self.dockspace.is_surface_presented(A) && self.dockspace.is_surface_presented(B);
        let child_presented = items.get(&DETACH).is_some_and(|surface| {
            *surface != A && *surface != B && self.dockspace.is_surface_presented(*surface)
        });
        let mut shared = lock(&self.shared);
        shared.items = items;
        shared.presented = presented;
        shared.child_presented = child_presented;
        if shifted
            && presented
            && !shared.shifted_ready
            && let Some(current) = eframe::current_native_output_token()
                .filter(|token| token.viewport_id() == egui::ViewportId::ROOT)
            && let Some((item, geometry)) = shared.geometry.iter().find(|(item, geometry)| {
                shared.items.get(*item) == Some(&B)
                    && geometry.viewport == egui::ViewportId::ROOT
                    && geometry.token != current
                    && geometry.matches_placement(placements(true)[1])
            })
        {
            eprintln!(
                "shared-window changed B placement: item={item:?} completed={geometry:?} presented_in={current:?}"
            );
            shared.shifted_ready = true;
        }
        drop(shared);
        ui.ctx().request_repaint_after(Duration::from_millis(10));
    }
}

pub(super) fn run() -> Result<(), Box<dyn std::error::Error>> {
    if std::env::var_os("DISPLAY").is_none() {
        return Err("DISPLAY is required".into());
    }
    let shared = State::default();
    let style = DockStyle::default();
    let panes = Panes {
        shared: Arc::clone(&shared),
        text: BTreeMap::new(),
        observed_viewports: BTreeSet::new(),
    };
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(layout(), policy)?;
    let dockspace = NativeDockspaceApp::new(
        egui::Id::unique("shared-window-consumer"),
        session,
        A,
        panes,
        style,
    )?;
    let native_host = dockspace.native_host_handler();
    let driver_shared = Arc::clone(&shared);
    let result_shared = Arc::clone(&shared);
    let driver = thread::spawn(move || {
        let result = drive(&driver_shared);
        let _ = xdo(&["mouseup".into(), "1".into()]);
        if let Err(error) = result {
            lock(&driver_shared).error = Some(error);
            if let Ok(root) = window(TITLE) {
                let _ = xdo(&["windowactivate".into(), "--sync".into(), root]);
                let _ = xdo(&["key".into(), "alt+F4".into()]);
            }
        }
    });
    let discard = std::env::args().any(|arg| arg == "--late-discard");
    let result = eframe::run_native(
        TITLE,
        eframe::NativeOptions {
            native_host: Some(native_host),
            renderer: eframe::Renderer::Glow,
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([960.0, 600.0])
                .with_position([40.0, 40.0]),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_x11().with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |context| {
            context.egui_ctx.enable_accesskit();
            context.egui_ctx.add_plugin(OutputGeometry {
                shared: Arc::clone(&shared),
                trees: BTreeMap::new(),
                observed_hooks: BTreeSet::new(),
            });
            context.egui_ctx.options_mut(|options| {
                options.max_passes = std::num::NonZeroUsize::new(3).unwrap()
            });
            Ok(Box::new(App {
                dockspace,
                shared: Arc::clone(&shared),
                started: Instant::now(),
                discard,
            }))
        }),
    );
    driver
        .join()
        .map_err(|_| "shared-window physical driver panicked")?;
    result?;
    let shared = lock(&result_shared);
    if let Some(error) = &shared.error {
        return Err(error.clone().into());
    }
    if !shared.completed {
        return Err("shared-window journey exited before completion".into());
    }
    Ok(())
}

fn wait(state: &State, label: &str, ready: impl Fn(&Shared) -> bool) -> Result<(), String> {
    let started = Instant::now();
    loop {
        let shared = lock(state);
        if let Some(error) = &shared.error {
            return Err(error.clone());
        }
        if ready(&shared) {
            return Ok(());
        }
        drop(shared);
        if started.elapsed() > TIMEOUT {
            let shared = lock(state);
            return Err(format!(
                "timeout waiting for {label}; stage={}; items={:?}; geometry={:?}; presented={}; last inputs={:#?}",
                shared.stage,
                shared.items,
                shared.geometry,
                shared.presented,
                shared.input_diagnostics,
            ));
        }
        thread::sleep(Duration::from_millis(15));
    }
}

fn xdo(arguments: &[String]) -> Result<String, String> {
    let output = Command::new("xdotool")
        .args(arguments)
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        return Err(format!(
            "xdotool {arguments:?}: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    String::from_utf8(output.stdout).map_err(|error| error.to_string())
}

fn window(title: &str) -> Result<String, String> {
    let result = xdo(&[
        "search".into(),
        "--onlyvisible".into(),
        "--name".into(),
        ".".into(),
    ])?;
    // search --name can match the legacy WM_NAME encoding. getwindowname reads
    // the real UTF-8 caption, so compare it exactly after visible enumeration.
    let windows = result
        .lines()
        .filter(|window| {
            xdo(&["getwindowname".into(), (*window).into()])
                .is_ok_and(|name| name.strip_suffix('\n').unwrap_or(&name) == title)
        })
        .collect::<Vec<_>>();
    if let [window] = windows.as_slice() {
        Ok((*window).into())
    } else {
        Err(format!("expected one {title}: {windows:?}"))
    }
}

fn wait_window(state: &State, title: &str) -> Result<String, String> {
    let started = Instant::now();
    loop {
        if let Some(error) = &lock(state).error {
            return Err(error.clone());
        }
        match window(title) {
            Ok(window) => return Ok(window),
            Err(error) if started.elapsed() >= TIMEOUT => {
                return Err(format!(
                    "timeout waiting for the visible window titled {title:?}: {error}"
                ));
            }
            Err(_) => thread::sleep(Duration::from_millis(15)),
        }
    }
}

fn origin(window: &str) -> Result<[i32; 2], String> {
    let client = window.parse::<u32>().map_err(|error| error.to_string())?;
    let (connection, screen) = x11rb::connect(None).map_err(|error| error.to_string())?;
    let root = connection
        .setup()
        .roots
        .get(screen)
        .ok_or("X11 display returned no screen")?
        .root;
    // Query the actual client origin. Some xdotool versions add its relative
    // reparenting offset twice when reporting a decorated window's geometry.
    let origin = connection
        .translate_coordinates(client, root, 0, 0)
        .map_err(|error| error.to_string())?
        .reply()
        .map_err(|error| error.to_string())?;
    Ok([i32::from(origin.dst_x), i32::from(origin.dst_y)])
}

fn drag(from: [i32; 2], to: [i32; 2]) -> Result<(), String> {
    xdo(&[
        "mousemove".into(),
        "--sync".into(),
        from[0].to_string(),
        from[1].to_string(),
    ])?;
    xdo(&["mousedown".into(), "1".into()])?;
    for step in 1..=12 {
        let point = [
            from[0] + (to[0] - from[0]) * step / 12,
            from[1] + (to[1] - from[1]) * step / 12,
        ];
        xdo(&[
            "mousemove".into(),
            "--sync".into(),
            point[0].to_string(),
            point[1].to_string(),
        ])?;
        thread::sleep(Duration::from_millis(25));
    }
    thread::sleep(Duration::from_millis(250));
    xdo(&["mouseup".into(), "1".into()])?;
    Ok(())
}

fn point(state: &State, item: ItemId, window: &str, tab: bool) -> Result<[i32; 2], String> {
    let (geometry, stage) = {
        let shared = lock(state);
        let geometry = *shared.geometry.get(&item).ok_or_else(|| {
            format!(
                "stage={}: pane {item:?} has no completed callback geometry",
                shared.stage
            )
        })?;
        (geometry, shared.stage)
    };
    let local = if tab { geometry.tab } else { geometry.center };
    if local == [0, 0] {
        return Err(format!(
            "stage={stage}: pane {item:?} has no final-output tab geometry"
        ));
    }
    let origin = origin(window)?;
    Ok([origin[0] + local[0], origin[1] + local[1]])
}

fn drive(state: &State) -> Result<(), String> {
    wait(state, "both independently placed Root surfaces", |shared| {
        shared.presented
            && shared.geometry.contains_key(&FIRST)
            && shared.geometry.contains_key(&ItemId::new(21))
    })?;
    let root = window(TITLE)?;
    xdo(&["windowactivate".into(), "--sync".into(), root.clone()])?;
    lock(state).stage = "A -> B";
    let from = point(state, FIRST, &root, true)?;
    let to = point(state, ItemId::new(21), &root, false)?;
    eprintln!("shared-window A -> B: root={root} from={from:?} to={to:?}");
    drag(from, to)?;
    wait(state, "A -> B physical docking", |shared| {
        shared.items.get(&FIRST) == Some(&B) && shared.presented
    })?;
    wait(state, "next selected A tab", |shared| {
        shared.geometry.contains_key(&DETACH)
    })?;
    lock(state).stage = "A -> Child";
    let from = point(state, DETACH, &root, true)?;
    let root_origin = origin(&root)?;
    drag(from, [root_origin[0] + 1080, root_origin[1] + 210])?;
    wait(state, "real Child semantic first live", |shared| {
        shared.child_presented
            && shared
                .geometry
                .get(&DETACH)
                .is_some_and(|geometry| geometry.viewport != egui::ViewportId::ROOT)
    })?;
    let child = wait_window(state, "Pane 12 — Dockspace")?;
    lock(state).stage = "Child -> B";
    drag(
        point(state, DETACH, &child, true)?,
        point(state, FIRST, &root, false)?,
    )?;
    wait(state, "Child -> B physical docking", |shared| {
        shared.items.get(&DETACH) == Some(&B) && shared.presented
    })?;
    {
        let mut shared = lock(state);
        shared.stage = "placement change";
        shared.shifted = true;
    }
    wait(
        state,
        "changed origin and scale freshly presented",
        |shared| shared.shifted_ready,
    )?;
    lock(state).request_reset = true;
    wait(
        state,
        "whole-Root replacement and fresh siblings",
        |shared| {
            shared.reset_done
                && shared.presented
                && shared.items.get(&FIRST) == Some(&A)
                && shared.items.get(&ItemId::new(21)) == Some(&B)
        },
    )?;
    lock(state).completed = true;
    eprintln!(
        "shared-window: real A -> B -> native Child -> B, placement change and Root replacement passed"
    );
    xdo(&["windowactivate".into(), "--sync".into(), root])?;
    xdo(&["key".into(), "alt+F4".into()])?;
    Ok(())
}
