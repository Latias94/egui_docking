use dockspace::backend::interaction::{InteractionOutcome, InteractionStatus};
use dockspace::backend::transition::{EngineTransition, InputOutcome, WorkspaceVersion};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use egui::accesskit::{Action, ActionRequest, Role, TreeUpdate};
use egui::{Context, Event, Id, Key, Modifiers, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const FLOATING_ROOT: RootId = RootId::new(11);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const ITEM_A: ItemId = ItemId::new(100);
const ITEM_B: ItemId = ItemId::new(101);
const ITEM_C: ItemId = ItemId::new(102);
const SCREEN_SIZE: egui::Vec2 = vec2(600.0, 400.0);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

#[derive(Debug)]
struct FrameObservation {
    interactions_current: bool,
    transitions: Vec<EngineTransition>,
    version: WorkspaceVersion,
    accesskit: Option<TreeUpdate>,
}

fn split_workspace(occluded: bool) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ITEM_A]));
    let right = builder.insert_node(Node::tabs([ITEM_B]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right])
            .expect("two children form a horizontal split"),
    );
    builder.set_root(MAIN_ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));

    if occluded {
        let floating = builder.insert_node(Node::tabs([ITEM_C]));
        builder.set_root(FLOATING_ROOT, RootRecord::new(floating));
        builder.set_contained_floating(
            FLOATING,
            ContainedFloating::new(
                FLOATING_ROOT,
                LogicalRect::new(0.0, 0.0, f64::from(SCREEN_SIZE.x), f64::from(SCREEN_SIZE.y))
                    .expect("covering contained rect is valid"),
            ),
        );
        builder
            .attach_contained(SURFACE, FLOATING)
            .expect("surface exists");
    }

    (builder.build().expect("split fixture must be valid"), split)
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, SCREEN_SIZE)),
        events: events.into_iter().map(Into::into).collect(),
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    events: Vec<Event>,
) -> FrameObservation {
    let mut interactions_current = false;
    let mut transitions = Vec::new();
    let output = crate::test_support::run_ui(context, input(events), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("egui frame must advance");
        interactions_current = response.interactions_current();
        transitions.push(response.backend_transition().clone());
    });
    FrameObservation {
        interactions_current,
        transitions,
        version: dockspace.core_engine().version(),
        accesskit: output.platform_output.accesskit_update,
    }
}

fn settle(context: &Context, dockspace: &mut Dockspace, panes: &mut TestPanes) -> FrameObservation {
    let _ = run_frame(context, dockspace, panes, Vec::new());
    let stable = run_frame(context, dockspace, panes, Vec::new());
    assert!(stable.interactions_current);
    stable
}

fn key_press(key: Key) -> Vec<Event> {
    [true, false]
        .into_iter()
        .map(|pressed| Event::Key {
            key,
            physical_key: Some(key),
            pressed,
            repeat: false,
            modifiers: Modifiers::NONE,
        })
        .collect()
}

fn accesskit_action(id: Id, action: Action) -> Event {
    Event::AccessKitActionRequest(ActionRequest {
        action,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: id.accesskit_id(),
        data: None,
    })
}

fn workspace_weights(workspace: &Workspace, split: NodeId) -> Vec<f32> {
    match workspace.node(split) {
        Some(Node::Split { weights, .. }) => weights.iter().map(|weight| weight.get()).collect(),
        Some(Node::Tabs { .. }) => panic!("fixture node must remain a split"),
        None => panic!("fixture split must remain present"),
    }
}

fn splitter_widget_id(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &'static str,
    split: NodeId,
) -> Id {
    let mut splitter_id = None;
    let _ = crate::test_support::run_ui(context, input(Vec::new()), |ui| {
        let instance_id = Id::new(("egui_dockspace", salt));
        splitter_id = Some(ui.make_persistent_id((
            "egui_dockspace",
            instance_id,
            SURFACE,
            MAIN_ROOT,
            split,
            0_usize,
            "splitter",
        )));
        dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("splitter id frame must advance");
    });
    splitter_id.expect("splitter id is computed during the frame")
}

fn count_splitter_adjustments(transitions: &[EngineTransition]) -> usize {
    transitions
        .iter()
        .flat_map(EngineTransition::reduced_inputs)
        .filter(|input| {
            matches!(
                input.outcome(),
                InputOutcome::InteractionProcessed {
                    outcome: InteractionOutcome::SplitterAdjusted { changed: true, .. },
                    ..
                }
            )
        })
        .count()
}

#[test]
fn keyboard_and_accesskit_adjustments_are_scene_bound_one_shot_commits() {
    let context = Context::default();
    let salt = "scene-bound-splitter-adjustment";
    let (workspace, split) = split_workspace(false);
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture dockspace must build");
    let mut panes = TestPanes;
    let stable = settle(&context, &mut dockspace, &mut panes);
    let initial = workspace_weights(dockspace.core_engine().workspace(), split);
    let splitter_id = splitter_widget_id(&context, &mut dockspace, &mut panes, salt, split);
    context.memory_mut(|memory| memory.request_focus(splitter_id));
    let focused = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(focused.version, stable.version);

    let key_adjustment = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowRight),
    );
    let after_key = workspace_weights(dockspace.core_engine().workspace(), split);
    assert!(after_key[0] > initial[0]);
    assert_eq!(count_splitter_adjustments(&key_adjustment.transitions), 1);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );

    let _ = settle(&context, &mut dockspace, &mut panes);
    let before_accesskit = dockspace.core_engine().version();
    let accesskit_adjustment = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![accesskit_action(splitter_id, Action::Decrement)],
    );
    let after_accesskit = workspace_weights(dockspace.core_engine().workspace(), split);
    assert!(after_accesskit[0] < after_key[0]);
    assert_eq!(
        count_splitter_adjustments(&accesskit_adjustment.transitions),
        1
    );
    assert_eq!(
        accesskit_adjustment.version.revision().get(),
        before_accesskit.revision().get() + 1
    );
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn presentation_acknowledgement_restores_splitter_input_in_the_same_host_frame() {
    let context = Context::default();
    let salt = "unacknowledged-splitter-keyboard";
    let (workspace, split) = split_workspace(false);
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture dockspace must build");
    let mut panes = TestPanes;
    let _ = settle(&context, &mut dockspace, &mut panes);
    let splitter_id = splitter_widget_id(&context, &mut dockspace, &mut panes, salt, split);
    context.memory_mut(|memory| memory.request_focus(splitter_id));

    let mut style = dockspace.style().clone();
    style.splitter_thickness += 1.0;
    dockspace
        .set_style(style)
        .expect("style change invalidates the presentation");
    let painted = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(!painted.interactions_current);
    assert!(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .is_none()
    );

    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let before_version = dockspace.core_engine().version();
    let before_weights = workspace_weights(dockspace.core_engine().workspace(), split);
    let acknowledgement_pass = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        key_press(Key::ArrowRight),
    );

    assert!(
        !acknowledgement_pass.interactions_current,
        "the accepted adjustment immediately invalidates the painted projection"
    );
    assert_eq!(
        count_splitter_adjustments(&acknowledgement_pass.transitions),
        1
    );
    assert_ne!(acknowledgement_pass.version, before_version);
    assert_ne!(
        workspace_weights(dockspace.core_engine().workspace(), split),
        before_weights
    );
    assert!(
        dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .is_none(),
        "the committed resize invalidates the projection after using sealed-frame authority"
    );
}

#[test]
fn fully_occluded_splitter_with_old_focus_exposes_no_action_and_cannot_adjust() {
    let context = Context::default();
    context.enable_accesskit();
    let salt = "fully-occluded-splitter";
    let (workspace, split) = split_workspace(false);
    let (occluded, occluded_split) = split_workspace(true);
    assert_eq!(occluded_split, split);
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture dockspace must build");
    let mut panes = TestPanes;
    let _ = settle(&context, &mut dockspace, &mut panes);
    let splitter_id = splitter_widget_id(&context, &mut dockspace, &mut panes, salt, split);
    context.memory_mut(|memory| memory.request_focus(splitter_id));

    dockspace
        .replace_workspace(occluded.clone())
        .expect("workspace replacement commits immediately");
    assert_eq!(dockspace.core_engine().workspace(), &occluded);
    let _ = settle(&context, &mut dockspace, &mut panes);
    let semantic_frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let ready = dockspace
        .core_engine()
        .interaction_projection(SURFACE)
        .expect("occluded surface remains authoritative");
    let record = ready
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .expect("occluded splitter stays in the draw plan");
    assert!(!record.operable());
    let accesskit = semantic_frame
        .accesskit
        .expect("AccessKit output is enabled for semantic widgets");
    let stale_node = accesskit
        .nodes
        .iter()
        .find(|(id, _)| *id == splitter_id.accesskit_id())
        .map(|(_, node)| node);
    assert!(stale_node.is_none_or(|node| {
        node.role() != Role::Splitter
            && !node.supports_action(Action::Increment)
            && !node.supports_action(Action::Decrement)
    }));
    assert!(!context.memory(|memory| memory.has_focus(splitter_id)));

    context.memory_mut(|memory| memory.request_focus(splitter_id));
    let before = dockspace.core_engine().version();
    let before_weights = workspace_weights(dockspace.core_engine().workspace(), split);
    let rejected_actions = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![
            accesskit_action(splitter_id, Action::Increment),
            key_press(Key::ArrowRight).remove(0),
        ],
    );
    assert_eq!(rejected_actions.version, before);
    assert_eq!(count_splitter_adjustments(&rejected_actions.transitions), 0);
    assert_eq!(
        workspace_weights(dockspace.core_engine().workspace(), split),
        before_weights
    );
    assert!(!context.memory(|memory| memory.has_focus(splitter_id)));
}
