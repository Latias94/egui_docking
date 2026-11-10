//! Regression coverage using real native tokens, frozen intents and egui responses.

#![cfg(target_os = "linux")]

use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    HostWindowToken, NativePointerRoster, NativeReceiverAnswer, SurfacePresentationResult,
    SurfaceUnavailableReason,
};
use eframe::egui;
use egui::accesskit::{Action, ActionRequest, NodeId, Role, TreeId};
use egui::{Event, FullOutput, Id, Pos2, RawInput, Rect};
use egui_dockspace::native_support::{self, EguiSurfaceAction};
use egui_dockspace::{DockStyle, PaneView};
use winit::platform::x11::EventLoopBuilderExtX11 as _;

use super::*;

const SURFACE: SurfaceId = SurfaceId::new(1);
const CHILD: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const INSTANCE: &str = "native-ui-ingress-regression";

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        ui.label("The candidate owns this actual pane paint.");
        egui_dockspace::PaneResponse::default()
    }
}

fn layout(surface: SurfaceId, count: u64) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(
            RootId::new(surface.get()),
            DockspaceNode::central_tabs((1..=count).map(ItemId::new)),
        ),
    )])
    .unwrap()
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
        events,
        ..Default::default()
    }
}

fn accesskit(node: NodeId, action: Action) -> Event {
    Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: TreeId::ROOT,
        target_node: node,
        data: None,
    })
}

fn node<'a>(
    output: &'a FullOutput,
    role: Role,
    label: &str,
) -> (NodeId, &'a egui::accesskit::Node) {
    output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find(|(_, node)| node.role() == role && node.label() == Some(label))
        .map(|(id, node)| (*id, node))
        .expect("the production renderer exports the actual receiver")
}

struct Fixture {
    session: DockspaceSession,
    context: egui::Context,
    surface: SurfaceId,
    style: DockStyle,
}

impl Fixture {
    fn new(surface: SurfaceId, layout: DockspaceLayout) -> Self {
        let context = egui::Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
        let mut fixture = Self {
            session: DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap(),
            context,
            surface,
            style: DockStyle::default(),
        };
        fixture
            .render(Vec::new(), Vec::new())
            .1
            .drop_without_applying_deltas();
        fixture
            .render(Vec::new(), Vec::new())
            .1
            .drop_without_applying_deltas();
        fixture
    }

    fn intent(&mut self, item: ItemId) -> DeferredSurfaceAction {
        self.session
            .begin_host_frame()
            .unwrap()
            .paint_plan(self.surface)
            .unwrap()
            .unwrap()
            .prepare_tab_select(item)
            .unwrap()
            .into_deferred()
    }

    fn submit(&mut self, actions: Vec<DeferredSurfaceAction>) -> usize {
        let mut frame = self.session.begin_host_frame().unwrap();
        for action in actions {
            frame.submit_deferred_surface_action(action).unwrap();
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap().inputs().len()
    }

    fn render(
        &mut self,
        events: Vec<Event>,
        incoming: Vec<DeferredSurfaceAction>,
    ) -> (Vec<EguiSurfaceAction>, FullOutput) {
        let mut frame = self.session.begin_host_frame().unwrap();
        for action in incoming {
            frame.submit_deferred_surface_action(action).unwrap();
        }
        frame.begin_encoding_pass().unwrap();
        let mut paint = None;
        let mut output = self.context.run_ui(input(events), |ui| {
            native_support::measure_surface(
                &mut frame,
                self.surface,
                ui,
                ui.available_rect_before_wrap(),
                ui.max_rect(),
                &Panes,
                &self.style,
            )
            .unwrap();
            let mut register = |ui: &egui::Ui, rect, id| {
                let hit = ui.register_scroll_hit_candidate(rect, id);
                (hit.id(), hit.layer_id())
            };
            paint = Some(
                native_support::paint_surface(
                    &mut frame,
                    Id::unique(INSTANCE),
                    self.surface,
                    ui,
                    &mut Panes,
                    &self.style,
                    &mut register,
                )
                .unwrap(),
            );
        });
        let mut paint = paint.unwrap();
        assert!(paint.validate_final_output(&mut output));
        let actions = paint.take_local_actions();
        assert!(paint.confirm_into(&mut frame).unwrap());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        for output in report.take_painted_outputs() {
            self.session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
        }
        (actions, output)
    }
}

fn pending(
    run: NativeOutputToken,
    origin: Id,
    action: DeferredSurfaceAction,
) -> PendingLocalAction {
    PendingLocalAction {
        event: UiActionEvent { run, origin },
        owner: None,
        action,
    }
}

fn records(run: NativeOutputToken, records: Vec<EguiSurfaceAction>) -> Vec<PendingLocalAction> {
    records
        .into_iter()
        .map(|record| pending(run, record.origin(), record.into_deferred()))
        .collect()
}

struct PanicHook(Arc<AtomicBool>);

impl egui::plugin::Plugin for PanicHook {
    fn debug_name(&self) -> &'static str {
        "native-ui-ingress-panic"
    }

    fn output_hook(&mut self, _context: &egui::Context, _output: &mut FullOutput) {
        if self.0.swap(false, Ordering::SeqCst) {
            panic!("the output hook fails after the queued event entered its candidate");
        }
    }
}

fn queued_event_survives_a_real_hook_panic(tokens: &[NativeOutputToken]) {
    let mut fixture = Fixture::new(SURFACE, layout(SURFACE, 3));
    let (_, output) = fixture.render(Vec::new(), Vec::new());
    let target = node(&output, Role::Tab, "Pane 2").0;
    output.drop_without_applying_deltas();
    let (actions, output) = fixture.render(vec![accesskit(target, Action::Click)], Vec::new());
    output.drop_without_applying_deltas();
    assert_eq!(actions.len(), 1);
    let mut queue = NativePassActions::default();
    queue
        .retain_local_actions(SURFACE, records(tokens[0], actions))
        .unwrap();
    let version = fixture.session.version();
    let armed = Arc::new(AtomicBool::new(true));
    fixture.context.add_plugin(PanicHook(armed));
    let result = catch_unwind(AssertUnwindSafe(|| {
        let (actions, _acknowledgement) = queue.peek_deferred_actions(tokens[1], SURFACE);
        fixture
            .render(Vec::new(), actions)
            .1
            .drop_without_applying_deltas();
    }));
    assert!(result.is_err());
    assert_eq!(fixture.session.version(), version);
    assert!(fixture.session.view().item(FIRST).unwrap().is_selected());
    assert!(queue.has_retained_action_work());
    let (actions, acknowledgement) = queue.peek_deferred_actions(tokens[2], SURFACE);
    assert_eq!(
        actions.len(),
        1,
        "the original event still owns its ingress slot"
    );
    fixture
        .render(Vec::new(), actions)
        .1
        .drop_without_applying_deltas();
    queue.acknowledge(acknowledgement);
    assert!(fixture.session.view().item(SECOND).unwrap().is_selected());
    assert!(!queue.has_retained_action_work());
}

fn independent_tokens_and_origins_each_reach_core(tokens: &[NativeOutputToken]) {
    let mut fixture = Fixture::new(SURFACE, layout(SURFACE, 3));
    let action = fixture.intent(SECOND);
    let mut queue = NativePassActions::default();
    queue
        .retain_local_actions(
            SURFACE,
            vec![
                pending(tokens[0], Id::unique("first-origin"), action.clone()),
                pending(tokens[0], Id::unique("second-origin"), action.clone()),
                pending(tokens[1], Id::unique("first-origin"), action),
            ],
        )
        .unwrap();
    let (actions, acknowledgement) = queue.peek_deferred_actions(tokens[2], SURFACE);
    assert_eq!(actions.len(), 3);
    queue
        .retain_local_actions(
            SURFACE,
            vec![pending(
                tokens[2],
                Id::unique("new-current-run"),
                fixture.intent(THIRD),
            )],
        )
        .unwrap();
    assert_eq!(
        fixture.submit(actions),
        3,
        "equal intent never identifies independent events"
    );
    queue.acknowledge(acknowledgement);
    assert_eq!(
        queue.local_actions[&SURFACE].len(),
        1,
        "publication must not acknowledge actions emitted after its ingress snapshot"
    );
    assert!(
        queue.peek_deferred_actions(tokens[2], SURFACE).0.is_empty(),
        "the current run's new intent is input for a subsequent run"
    );
}

fn capacity_and_conflicts_do_not_damage_accepted_events(tokens: &[NativeOutputToken]) {
    let mut fixture = Fixture::new(SURFACE, layout(SURFACE, 3));
    let action = fixture.intent(SECOND);
    let mut queue = NativePassActions::default();
    queue
        .retain_local_actions(
            SURFACE,
            (0..LOCAL_ACTION_CAPACITY)
                .map(|index| pending(tokens[0], Id::unique(index), action.clone()))
                .collect(),
        )
        .unwrap();
    queue
        .retain_local_actions(
            SURFACE,
            vec![pending(tokens[0], Id::unique(0_usize), action.clone())],
        )
        .unwrap();
    assert_eq!(
        queue.retain_local_actions(
            SURFACE,
            vec![pending(tokens[1], Id::unique(0_usize), action.clone()),]
        ),
        Err(NativePassActionError::LocalActionConflict)
    );
    assert_eq!(queue.peek_deferred_actions(tokens[2], SURFACE).0.len(), 64);
    assert_eq!(
        queue.retain_local_actions(
            SURFACE,
            vec![pending(
                tokens[0],
                Id::unique(0_usize),
                fixture.intent(THIRD)
            ),]
        ),
        Err(NativePassActionError::LocalActionConflict)
    );
    let (actions, acknowledgement) = queue.peek_deferred_actions(tokens[2], SURFACE);
    assert_eq!(fixture.submit(actions), 64);
    queue.acknowledge(acknowledgement);
    queue
        .retain_local_actions(
            SURFACE,
            vec![pending(tokens[1], Id::unique(0_usize), action)],
        )
        .unwrap();
    assert_eq!(queue.peek_deferred_actions(tokens[2], SURFACE).0.len(), 1);
}

fn publication_retires_removed_surfaces_and_preserves_unpresented_owners(
    tokens: &[NativeOutputToken],
) {
    let mut fixture = Fixture::new(CHILD, layout(CHILD, 3));
    let mut queue = NativePassActions::default();
    queue
        .retain_local_actions(
            CHILD,
            vec![pending(
                tokens[0],
                Id::unique("child"),
                fixture.intent(SECOND),
            )],
        )
        .unwrap();
    let reset = fixture
        .session
        .prepare_layout_reset(layout(SURFACE, 3))
        .unwrap();
    {
        let frame = fixture.session.begin_layout_reset_frame(&reset).unwrap();
        drop(frame);
    }
    queue.retire_unavailable(&fixture.session);
    assert!(
        queue.has_retained_action_work(),
        "a dropped replacement cannot retire accepted ingress"
    );
    let mut frame = fixture.session.begin_layout_reset_frame(&reset).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    queue.retire_unavailable(&fixture.session);
    assert!(
        !queue.has_retained_action_work(),
        "removed S cannot keep requesting root repaint"
    );

    let mut fixture = Fixture::new(SURFACE, layout(SURFACE, 3));
    let action = fixture.intent(SECOND);
    fixture.submit(Vec::new());
    fixture
        .session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .unwrap();
    fixture
        .session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let mut frame = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let owner = report.native_bindings()[0];
    assert!(fixture.session.is_current_native_binding(owner));
    assert!(fixture.session.presented_surface(SURFACE).is_none());
    let mut action = pending(tokens[0], Id::unique("live-owner"), action);
    action.owner = Some(owner);
    queue.retain_local_actions(SURFACE, vec![action]).unwrap();
    queue.retire_unavailable(&fixture.session);
    assert!(
        queue.has_retained_action_work(),
        "first-live presentation is not owner retirement"
    );
}

fn actual_repeat_preserves_scrollbar_response_identity(token: NativeOutputToken) {
    let mut fixture = Fixture::new(SURFACE, layout(SURFACE, 40));
    let (_, output) = fixture.render(Vec::new(), Vec::new());
    let control = node(&output, Role::Button, "Show hidden tabs").0;
    output.drop_without_applying_deltas();
    let (open, output) = fixture.render(vec![accesskit(control, Action::Click)], Vec::new());
    output.drop_without_applying_deltas();
    fixture.submit(
        open.into_iter()
            .map(EguiSurfaceAction::into_deferred)
            .collect(),
    );
    fixture
        .render(Vec::new(), Vec::new())
        .1
        .drop_without_applying_deltas();
    let (_, output) = fixture.render(Vec::new(), Vec::new());
    let (scrollbar, bar) = node(&output, Role::ScrollBar, "Open tabs scroll position");
    let bounds = bar.bounds().unwrap();
    #[allow(clippy::cast_possible_truncation)]
    let (source, target) = (
        Pos2::new(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            bounds.y0 as f32 + 6.0,
        ),
        Pos2::new(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            bounds.y1 as f32 - 6.0,
        ),
    );
    output.drop_without_applying_deltas();
    fixture
        .render(
            vec![
                Event::PointerMoved(source),
                Event::PointerButton {
                    pos: source,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            Vec::new(),
        )
        .1
        .drop_without_applying_deltas();

    fixture
        .context
        .options_mut(|options| options.max_passes = NonZeroUsize::new(2).unwrap());
    let mut frame = fixture.session.begin_host_frame().unwrap();
    frame.begin_encoding_pass().unwrap();
    let mut queue = NativePassActions::default();
    let mut origins = Vec::new();
    let mut output = fixture.context.run_ui(
        input(vec![
            Event::PointerMoved(target),
            accesskit(scrollbar, Action::Increment),
        ]),
        |ui| {
            if !origins.is_empty() {
                frame.restart_encoding_pass().unwrap();
            }
            native_support::measure_surface(
                &mut frame,
                SURFACE,
                ui,
                ui.available_rect_before_wrap(),
                ui.max_rect(),
                &Panes,
                &fixture.style,
            )
            .unwrap();
            let mut register = |ui: &egui::Ui, rect, id| {
                let hit = ui.register_scroll_hit_candidate(rect, id);
                (hit.id(), hit.layer_id())
            };
            let mut paint = native_support::paint_surface(
                &mut frame,
                Id::unique(INSTANCE),
                SURFACE,
                ui,
                &mut Panes,
                &fixture.style,
                &mut register,
            )
            .unwrap();
            let actions = paint.take_local_actions();
            origins.push(
                actions
                    .iter()
                    .map(EguiSurfaceAction::origin)
                    .collect::<Vec<_>>(),
            );
            queue
                .retain_local_actions(SURFACE, records(token, actions))
                .unwrap();
            if origins.len() == 1 {
                ui.ctx()
                    .request_discard("repeat the actual scrollbar response");
            }
        },
    );
    assert_eq!(output.platform_output.num_completed_passes, 2);
    assert_eq!(origins.iter().map(Vec::len).collect::<Vec<_>>(), vec![2, 1]);
    assert_ne!(origins[0][0], origins[0][1]);
    assert_eq!(
        origins[0][1], origins[1][0],
        "persistent pointer B keeps its origin after discrete A disappears"
    );
    assert_eq!(queue.local_actions[&SURFACE].len(), 2);
    output.textures_delta.clear();
}

struct TokenHost;
impl eframe::NativeHostHandler for TokenHost {}

struct Driver {
    tokens: Vec<NativeOutputToken>,
    completed: Arc<AtomicBool>,
    started: Instant,
}

impl eframe::App for Driver {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        assert!(self.started.elapsed() < Duration::from_secs(15));
        if self.completed.load(Ordering::SeqCst) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let token = eframe::current_native_output_token().unwrap();
        if self.tokens.last() != Some(&token) {
            self.tokens.push(token);
        }
        if self.tokens.len() == 3 {
            queued_event_survives_a_real_hook_panic(&self.tokens);
            independent_tokens_and_origins_each_reach_core(&self.tokens);
            capacity_and_conflicts_do_not_damage_accepted_events(&self.tokens);
            publication_retires_removed_surfaces_and_preserves_unpresented_owners(&self.tokens);
            actual_repeat_preserves_scrollbar_response_identity(token);
            self.completed.store(true, Ordering::SeqCst);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

#[test]
fn real_native_ui_ingress_retries_acknowledges_bounds_retires_and_deduplicates_actual_passes() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("SKIP native UI ingress regression: DISPLAY unavailable; run the X11 gate");
        return;
    }
    let completed = Arc::new(AtomicBool::new(false));
    let result = Arc::clone(&completed);
    eframe::run_native(
        "Native UI ingress regression",
        eframe::NativeOptions {
            native_host: Some(Arc::new(TokenHost)),
            renderer: eframe::Renderer::Glow,
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_x11().with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |_| {
            Ok(Box::new(Driver {
                tokens: Vec::new(),
                completed,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(result.load(Ordering::SeqCst));
}
