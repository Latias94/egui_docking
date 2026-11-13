//! Product events exercised exclusively through public facade and egui input.

use std::collections::BTreeMap;
use std::num::NonZeroUsize;

use dockspace::policy::{CloseCapability, DockPolicy};
use egui::accesskit::{Action, ActionRequest, Node, NodeId, Role, TreeId};
use egui::{Context, Event, FullOutput, PointerButton, Pos2, RawInput, Rect, Ui, vec2};

use crate::{
    CloseDecision, DeferredCloseDecision, DockStyle, Dockspace, DockspaceAppliedClose,
    DockspaceCloseOutcome, DockspaceCloseRequestStatus, DockspaceCloseResolution, DockspaceEvent,
    DockspaceEventKind, DockspaceLayout, DockspaceNode, DockspaceNotification, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    SurfacePresentationResult, Tab, TabVisuals,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

#[derive(Default)]
struct Panes {
    tabs: BTreeMap<ItemId, Rect>,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> PaneResponse {
        PaneResponse::default()
    }

    fn tab_ui(&mut self, ui: &mut Ui, tab: &Tab<'_>) -> TabVisuals {
        self.tabs.insert(tab.item, tab.visible_rect);
        tab.paint(ui)
    }
}

struct Harness {
    context: Context,
    dock: Dockspace,
    panes: Panes,
    nodes: Vec<(NodeId, Node)>,
    frame: u32,
    passes: usize,
    notification: DockspaceNotification,
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap()
}

impl Harness {
    fn new(style: DockStyle) -> Self {
        let context = Context::default();
        context.enable_accesskit();
        let mut policy = DockPolicy::default();
        policy.set_close_capability(CloseCapability::DeferredAllowed);
        let mut harness = Self {
            context,
            dock: Dockspace::builder("public-lifecycle-events", layout())
                .policy(policy)
                .style(style)
                .build()
                .unwrap(),
            panes: Panes::default(),
            nodes: Vec::new(),
            frame: 0,
            passes: 0,
            notification: DockspaceNotification::default(),
        };
        assert!(harness.settle().is_empty());
        harness
    }

    fn step(&mut self, events: Vec<Event>) -> Vec<DockspaceEvent> {
        self.frame += 1;
        self.panes.tabs.clear();
        let (output, response) = self
            .dock
            .run_single_surface(
                &self.context,
                RawInput {
                    screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
                    time: Some(f64::from(self.frame) * 0.1),
                    events,
                    ..RawInput::default()
                },
                SURFACE,
                &mut self.panes,
            )
            .unwrap();
        let events = response.mutation().events().to_vec();
        self.notification = response.mutation().notification().clone();
        assert_eq!(
            self.notification.layout_change().is_some(),
            response.mutation().workspace_changed(),
        );
        let (output, receipt) = output.into_parts();
        self.nodes = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .clone();
        self.passes = output.platform_output.num_completed_passes;
        // The test sink accepts the actual complete egui output. No native
        // window or physical scanout is claimed by this logical boundary.
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        events
    }

    fn settle(&mut self) -> Vec<DockspaceEvent> {
        (0..4).flat_map(|_| self.step(Vec::new())).collect()
    }

    fn activate(&mut self, role: Role, label: &str) -> Vec<DockspaceEvent> {
        let target = self
            .nodes
            .iter()
            .find(|(_, node)| node.role() == role && node.label() == Some(label))
            .unwrap_or_else(|| panic!("missing real accessible {role:?}: {label}"))
            .0;
        self.step(vec![Event::AccessKitActionRequest(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        })])
    }
}

fn button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn selections(events: &[DockspaceEvent], item: ItemId) -> usize {
    events
        .iter()
        .filter(|event| event.kind() == &DockspaceEventKind::SelectionCommitted { item })
        .count()
}

#[test]
fn ordinary_tab_click_returns_the_committed_selection_once() {
    let mut harness = Harness::new(DockStyle::default());
    let position = harness.panes.tabs[&SECOND].center();
    let mut events = harness.step(vec![Event::PointerMoved(position)]);
    events.extend(harness.step(vec![button(position, true)]));
    events.extend(harness.step(vec![button(position, false)]));
    events.extend(harness.settle());
    assert_eq!(selections(&events, SECOND), 1);
    assert!(harness.dock.view().item(SECOND).unwrap().is_selected());
    assert!(harness.settle().is_empty());
}

struct LateDiscard {
    remaining: usize,
}

impl egui::plugin::Plugin for LateDiscard {
    fn debug_name(&self) -> &'static str {
        "product_events_tests::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.remaining > 0 {
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "exercise lifecycle publication",
                ));
        }
    }
}

#[test]
fn discarded_encoding_passes_still_return_one_selection_event() {
    let mut harness = Harness::new(DockStyle::default());
    let position = harness.panes.tabs[&SECOND].center();
    harness.context.options_mut(|options| {
        options.max_passes = NonZeroUsize::new(3).unwrap();
    });
    harness.context.add_plugin(LateDiscard { remaining: 2 });
    let events = harness.step(vec![Event::PointerMoved(position), button(position, true)]);
    assert!(harness.notification.layout_change().is_some());
    assert_eq!(harness.passes, 3);
    assert_eq!(selections(&events, SECOND), 1);
    assert!(harness.dock.view().item(SECOND).unwrap().is_selected());
    harness.step(vec![button(position, false)]);
    assert!(harness.settle().is_empty());
}

#[test]
fn overflow_menu_selection_uses_the_same_committed_event_contract() {
    let mut harness = Harness::new(DockStyle {
        tab_min_width: 400.0,
        tab_max_width: 400.0,
        ..DockStyle::default()
    });
    let mut events = harness.activate(Role::Button, "Show hidden tabs");
    events.extend(harness.settle());
    events.extend(harness.activate(Role::MenuItem, "Second"));
    events.extend(harness.settle());
    assert_eq!(selections(&events, SECOND), 1);
    assert!(harness.dock.view().item(SECOND).unwrap().is_selected());
    assert!(harness.settle().is_empty());
}

#[test]
fn presentation_menu_float_returns_one_actual_placement_change() {
    let mut harness = Harness::new(DockStyle::default());
    let mut events = harness.activate(Role::Button, "Presentation commands");
    events.extend(harness.settle());
    events.extend(harness.activate(Role::MenuItem, "Float"));
    events.extend(harness.settle());
    let placements: Vec<_> = events
        .iter()
        .filter(|event| {
            matches!(
                event.kind(),
                DockspaceEventKind::RootInstalled {
                    floating: Some(_),
                    ..
                } | DockspaceEventKind::RootRehomed {
                    floating: Some(_),
                    ..
                }
            )
        })
        .collect();
    assert_eq!(placements.len(), 1, "{events:?}");
    assert_eq!(
        harness
            .dock
            .view()
            .surface(SURFACE)
            .unwrap()
            .contained()
            .count(),
        1
    );
    assert!(harness.settle().is_empty());
}

#[test]
fn deferred_close_and_programmatic_noops_share_the_mutation_event_api() {
    let mut harness = Harness::new(DockStyle::default());
    assert!(
        harness
            .dock
            .select_item_current(FIRST)
            .unwrap()
            .mutation()
            .events()
            .is_empty()
    );
    let request = harness.dock.request_close_item_current(FIRST).unwrap();
    assert!(request.mutation().events().is_empty());
    let DockspaceCloseRequestStatus::Requested(request) = request.status() else {
        panic!("the application close request must produce its real plan");
    };
    let plan = request.plan().clone();
    let deferred = harness
        .dock
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Deferred,
        )
        .unwrap();
    assert!(deferred.mutation().events().is_empty());
    let DockspaceCloseOutcome::Processed {
        resolution: DockspaceCloseResolution::Deferred { continuation, .. },
        ..
    } = deferred.outcome()
    else {
        panic!("the application must receive its deferred continuation");
    };
    let continuation = *continuation;
    let closed = harness
        .dock
        .resolve_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    assert!(closed.mutation().events().iter().any(|event| {
        event.kind()
            == &DockspaceEventKind::Closed(DockspaceAppliedClose::ItemClosed {
                item: FIRST,
                root: ROOT,
            })
    }));
    assert!(
        harness
            .dock
            .resolve_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
            .unwrap()
            .mutation()
            .events()
            .is_empty()
    );
    assert!(harness.settle().is_empty());
}

#[test]
fn reset_returns_a_single_replacement_event_without_replaying_it_in_show() {
    let mut harness = Harness::new(DockStyle::default());
    let mutation = harness.dock.reset_layout(layout()).unwrap();
    let change = mutation.layout_change().unwrap();
    assert_eq!(change.before(), mutation.before());
    assert_eq!(change.after(), mutation.after());
    assert_eq!(mutation.events().len(), 1);
    assert_eq!(
        mutation.events()[0].kind(),
        &DockspaceEventKind::LayoutReset
    );
    assert_eq!(mutation.events()[0].version(), mutation.after());
    assert!(harness.settle().is_empty());
}
