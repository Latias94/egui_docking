use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::{CloseDecision, ClosePlanTarget};
use egui::accesskit::{Action, ActionRequest};
use egui::{Context, Event, Id, Key, Modifiers, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{Dockspace, DockspaceClosePlan, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const ITEM_A: ItemId = ItemId::new(10);
const ITEM_B: ItemId = ItemId::new(11);
const ITEM_C: ItemId = ItemId::new(12);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

#[derive(Clone)]
struct FrameObservation {
    focused: Option<Id>,
    tab_ids: [Id; 3],
    close_ids: [Id; 3],
    splitter_id: Id,
    selected: Option<ItemId>,
    weights: Vec<f32>,
    retained_presentation_current: bool,
    saw_stale_pass: bool,
    close_requests: Vec<DockspaceClosePlan>,
}

fn tabs_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM_A, ITEM_B, ITEM_C]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("tabs fixture must be valid"), tabs)
}

fn split_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ITEM_A]));
    let right = builder.insert_node(Node::tabs([ITEM_B]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, right]).expect("two children form a split"),
    );
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("split fixture must be valid"), split)
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(600.0, 400.0))),
        events: events.into_iter().map(Into::into).collect(),
        ..RawInput::default()
    }
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

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &'static str,
    tabs: Option<NodeId>,
    split: Option<NodeId>,
    events: Vec<Event>,
) -> FrameObservation {
    let mut observation = None;
    let mut saw_stale_pass = false;
    let mut close_requests = Vec::new();
    let _ = crate::test_support::run_ui(context, input(events), |ui| {
        let instance_id = Id::new(("egui_dockspace", salt));
        let (tab_ids, close_ids) = tabs.map_or(([Id::NULL; 3], [Id::NULL; 3]), |tabs| {
            let list_id = ui.id().with(egui::IdSalt::new((
                "egui_dockspace",
                instance_id,
                SURFACE,
                ROOT,
                tabs,
                "tab-list",
            )));
            (
                [ITEM_A, ITEM_B, ITEM_C].map(|item| list_id.with((instance_id, "tab", item))),
                [ITEM_A, ITEM_B, ITEM_C].map(|item| list_id.with((instance_id, "tab-close", item))),
            )
        });
        let splitter_id = split.map_or(Id::NULL, |split| {
            ui.make_persistent_id((
                "egui_dockspace",
                instance_id,
                SURFACE,
                ROOT,
                split,
                0_usize,
                "splitter",
            ))
        });

        let response = dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("fixture frame must advance");
        close_requests.extend(response.close_requests().cloned());
        let interaction_capabilities = response.interaction_capabilities();
        saw_stale_pass |= !interaction_capabilities.retained_presentation_current();
        let root_node = dockspace
            .core_engine()
            .workspace()
            .root(ROOT)
            .and_then(|root| dockspace.core_engine().workspace().node(root.node));
        let (selected, weights) = match root_node {
            Some(Node::Tabs { selected, .. }) => (*selected, Vec::new()),
            Some(Node::Split { weights, .. }) => {
                (None, weights.iter().map(|weight| weight.get()).collect())
            }
            None => panic!("fixture root must exist"),
        };
        observation = Some(FrameObservation {
            focused: ui.memory(egui::Memory::focused),
            tab_ids,
            close_ids,
            splitter_id,
            selected,
            weights,
            retained_presentation_current: interaction_capabilities.retained_presentation_current(),
            saw_stale_pass,
            close_requests: Vec::new(),
        });
    });
    let mut observation = observation.expect("run_ui must paint one pass");
    observation.close_requests = close_requests;
    observation
}

fn warm_tabs(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &'static str,
    tabs: NodeId,
) -> FrameObservation {
    run_frame(
        context,
        dockspace,
        panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    let stable = run_frame(
        context,
        dockspace,
        panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert!(stable.retained_presentation_current);
    stable
}

fn warm_split(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut TestPanes,
    salt: &'static str,
    split: NodeId,
) -> FrameObservation {
    run_frame(
        context,
        dockspace,
        panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    let stable = run_frame(
        context,
        dockspace,
        panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    assert!(stable.retained_presentation_current);
    stable
}

#[test]
fn arrow_home_and_end_keep_tab_selection_and_focus_together() {
    let context = Context::default();
    let (workspace, tabs) = tabs_workspace();
    let salt = "tab-keyboard-focus";
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture facade must build");
    let mut panes = TestPanes;
    let stable = warm_tabs(&context, &mut dockspace, &mut panes, salt, tabs);
    context.memory_mut(|memory| memory.request_focus(stable.tab_ids[0]));
    let focused = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(focused.focused, Some(focused.tab_ids[0]));

    let arrow = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        key_press(Key::ArrowRight),
    );
    assert_eq!(arrow.selected, Some(ITEM_B));
    assert_eq!(arrow.focused, Some(arrow.tab_ids[1]));
    let arrow_projection_painted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(arrow_projection_painted.selected, Some(ITEM_B));
    assert_eq!(
        arrow_projection_painted.focused,
        Some(arrow_projection_painted.tab_ids[1])
    );
    assert!(!arrow_projection_painted.retained_presentation_current);
    let arrow_acknowledged = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(arrow_acknowledged.selected, Some(ITEM_B));
    assert_eq!(
        arrow_acknowledged.focused,
        Some(arrow_acknowledged.tab_ids[1])
    );
    assert!(arrow_acknowledged.retained_presentation_current);

    let home = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        key_press(Key::Home),
    );
    assert_eq!(home.selected, Some(ITEM_A));
    assert_eq!(home.focused, Some(home.tab_ids[0]));
    let home_projection_painted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(home_projection_painted.selected, Some(ITEM_A));
    assert_eq!(
        home_projection_painted.focused,
        Some(home_projection_painted.tab_ids[0])
    );
    assert!(!home_projection_painted.retained_presentation_current);
    let home_acknowledged = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(home_acknowledged.selected, Some(ITEM_A));
    assert_eq!(
        home_acknowledged.focused,
        Some(home_acknowledged.tab_ids[0])
    );
    assert!(home_acknowledged.retained_presentation_current);

    let end = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        key_press(Key::End),
    );
    assert_eq!(end.selected, Some(ITEM_C));
    assert_eq!(end.focused, Some(end.tab_ids[2]));
    let end_stable = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(end_stable.selected, Some(ITEM_C));
    assert_eq!(end_stable.focused, Some(end_stable.tab_ids[2]));
}

#[test]
fn focused_close_button_accepts_enter_and_space_without_a_pointer_click() {
    for (salt, key) in [
        ("tab-close-enter", Key::Enter),
        ("tab-close-space", Key::Space),
    ] {
        let context = Context::default();
        let (workspace, tabs) = tabs_workspace();
        let mut dockspace = Dockspace::builder(salt, workspace)
            .build()
            .expect("fixture facade must build");
        let mut panes = TestPanes;
        let stable = warm_tabs(&context, &mut dockspace, &mut panes, salt, tabs);
        context.memory_mut(|memory| memory.request_focus(stable.close_ids[1]));

        let focused = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            Vec::new(),
        );
        assert_eq!(focused.focused, Some(focused.close_ids[1]));

        let requested = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            key_press(key),
        );
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .item_multiset()
                .contains_key(&ITEM_B)
        );
        let [plan] = requested.close_requests.as_slice() else {
            panic!("one keyboard activation must publish exactly one close plan");
        };
        assert_eq!(plan.target(), ClosePlanTarget::Item { item: ITEM_B });
        let [item] = plan.items() else {
            panic!("an item close plan must contain exactly one decision token");
        };
        assert_eq!(item.item(), ITEM_B);

        dockspace
            .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
            .expect("the exact keyboard close decision must commit immediately");
        assert!(
            !dockspace
                .core_engine()
                .workspace()
                .item_multiset()
                .contains_key(&ITEM_B)
        );

        let committed = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            Vec::new(),
        );
        assert!(committed.close_requests.is_empty());
        assert!(
            !dockspace
                .core_engine()
                .workspace()
                .item_multiset()
                .contains_key(&ITEM_B)
        );
    }
}

#[test]
fn consecutive_splitter_key_adjustments_retain_focus_and_both_commit() {
    let context = Context::default();
    let (workspace, split) = split_workspace();
    let salt = "splitter-keyboard-focus";
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture facade must build");
    let mut panes = TestPanes;
    let stable = warm_split(&context, &mut dockspace, &mut panes, salt, split);
    context.memory_mut(|memory| memory.request_focus(stable.splitter_id));
    run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );

    let first_input = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        key_press(Key::ArrowRight),
    );
    assert!(first_input.weights[0] > stable.weights[0]);
    assert_eq!(first_input.focused, Some(first_input.splitter_id));
    assert!(!first_input.retained_presentation_current);
    assert!(first_input.saw_stale_pass);

    let first_projection_painted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    assert_eq!(first_projection_painted.weights, first_input.weights);
    assert!(!first_projection_painted.retained_presentation_current);
    assert!(first_projection_painted.saw_stale_pass);

    let first_projection_acknowledged = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    assert_eq!(first_projection_acknowledged.weights, first_input.weights);
    assert_eq!(
        first_projection_acknowledged.focused,
        Some(first_projection_acknowledged.splitter_id)
    );
    assert!(first_projection_acknowledged.retained_presentation_current);

    let second_input = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        key_press(Key::ArrowRight),
    );
    assert!(second_input.weights[0] > first_input.weights[0]);
    assert_eq!(second_input.focused, Some(second_input.splitter_id));
    assert!(!second_input.retained_presentation_current);
    assert!(second_input.saw_stale_pass);

    let second_projection_painted = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    assert_eq!(second_projection_painted.weights, second_input.weights);
    assert!(!second_projection_painted.retained_presentation_current);
    assert!(second_projection_painted.saw_stale_pass);

    let second_stable = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    assert_eq!(second_stable.weights, second_projection_painted.weights);
    assert_eq!(second_stable.focused, Some(second_stable.splitter_id));
    assert!(second_stable.retained_presentation_current);
}

#[derive(Clone, Copy)]
enum AdjustmentEvent {
    Key(Key),
    AccessKit(Action),
}

fn committed_adjustment(salt: &'static str, event: AdjustmentEvent) -> Vec<f32> {
    let context = Context::default();
    let (workspace, split) = split_workspace();
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture facade must build");
    let mut panes = TestPanes;
    let stable = warm_split(&context, &mut dockspace, &mut panes, salt, split);
    let events = match event {
        AdjustmentEvent::Key(key) => {
            context.memory_mut(|memory| memory.request_focus(stable.splitter_id));
            run_frame(
                &context,
                &mut dockspace,
                &mut panes,
                salt,
                None,
                Some(split),
                Vec::new(),
            );
            key_press(key)
        }
        AdjustmentEvent::AccessKit(action) => {
            vec![accesskit_action(stable.splitter_id, action)]
        }
    };
    let input_frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        events,
    );
    assert_ne!(input_frame.weights, stable.weights);
    let stable_frame = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        None,
        Some(split),
        Vec::new(),
    );
    assert_eq!(stable_frame.weights, input_frame.weights);
    input_frame.weights
}

#[test]
fn accesskit_increment_and_decrement_match_keyboard_commit_semantics() {
    let right = committed_adjustment("splitter-key-right", AdjustmentEvent::Key(Key::ArrowRight));
    assert!(right[0] > 0.5);
    let increment = committed_adjustment(
        "splitter-accesskit-increment",
        AdjustmentEvent::AccessKit(Action::Increment),
    );
    assert_eq!(increment, right);

    let left = committed_adjustment("splitter-key-left", AdjustmentEvent::Key(Key::ArrowLeft));
    assert!(left[0] < 0.5);
    let decrement = committed_adjustment(
        "splitter-accesskit-decrement",
        AdjustmentEvent::AccessKit(Action::Decrement),
    );
    assert_eq!(decrement, left);
}

#[derive(Clone, Copy)]
enum StableTabActivation {
    Enter,
    Space,
    AccessKitClick,
    AccessKitFocus,
    ArrowRight,
}

#[test]
fn tab_activation_requires_an_acknowledged_projection_after_external_selection() {
    for (salt, activation) in [
        ("stale-tab-enter", StableTabActivation::Enter),
        ("stale-tab-space", StableTabActivation::Space),
        ("stale-tab-accesskit", StableTabActivation::AccessKitClick),
        (
            "stale-tab-accesskit-focus",
            StableTabActivation::AccessKitFocus,
        ),
        ("stale-tab-arrow-right", StableTabActivation::ArrowRight),
    ] {
        let context = Context::default();
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::Tabs {
            items: vec![ITEM_A, ITEM_B, ITEM_C],
            selected: Some(ITEM_B),
        });
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().expect("tabs fixture must be valid");
        let mut dockspace = Dockspace::builder(salt, workspace)
            .build()
            .expect("fixture facade must build");
        let mut panes = TestPanes;
        let warmed = warm_tabs(&context, &mut dockspace, &mut panes, salt, tabs);
        context.memory_mut(|memory| memory.request_focus(warmed.tab_ids[1]));
        let focused = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            Vec::new(),
        );
        assert_eq!(focused.focused, Some(focused.tab_ids[1]));
        assert_eq!(focused.selected, Some(ITEM_B));

        let select_a = dockspace
            .core_engine()
            .workspace()
            .capture_item_source(ROOT, tabs, ITEM_A)
            .expect("selection source must capture");
        dockspace
            .submit_command(dockspace::command::WorkspaceCommand::Select { source: select_a })
            .expect("selection command must commit immediately");
        assert_eq!(
            dockspace
                .core_engine()
                .workspace()
                .node(tabs)
                .and_then(|node| match node {
                    Node::Tabs { selected, .. } => *selected,
                    Node::Split { .. } => None,
                }),
            Some(ITEM_A)
        );
        context.options_mut(|options| {
            options.max_passes = 1.try_into().expect("one is non-zero");
        });
        let events = match activation {
            StableTabActivation::Enter => key_press(Key::Enter),
            StableTabActivation::Space => key_press(Key::Space),
            StableTabActivation::AccessKitClick => {
                vec![accesskit_action(focused.tab_ids[1], Action::Click)]
            }
            StableTabActivation::AccessKitFocus => {
                vec![accesskit_action(focused.tab_ids[1], Action::Focus)]
            }
            StableTabActivation::ArrowRight => key_press(Key::ArrowRight),
        };

        let stale_frame = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            events,
        );
        assert!(!stale_frame.retained_presentation_current);
        assert_eq!(stale_frame.selected, Some(ITEM_A));
        if matches!(activation, StableTabActivation::ArrowRight) {
            assert_eq!(stale_frame.focused, Some(stale_frame.tab_ids[1]));
        }

        context.options_mut(|options| {
            options.max_passes = 2.try_into().expect("two is non-zero");
        });
        let projection_painted = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            Vec::new(),
        );
        assert!(!projection_painted.retained_presentation_current);
        assert_eq!(projection_painted.selected, Some(ITEM_A));

        let projection_acknowledged = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            Vec::new(),
        );
        assert!(projection_acknowledged.retained_presentation_current);
        assert_eq!(projection_acknowledged.selected, Some(ITEM_A));

        let activation_events = match activation {
            StableTabActivation::Enter => key_press(Key::Enter),
            StableTabActivation::Space => key_press(Key::Space),
            StableTabActivation::AccessKitClick => {
                vec![accesskit_action(focused.tab_ids[1], Action::Click)]
            }
            StableTabActivation::AccessKitFocus => {
                vec![accesskit_action(focused.tab_ids[1], Action::Focus)]
            }
            StableTabActivation::ArrowRight => key_press(Key::ArrowRight),
        };
        let committed = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            activation_events,
        );
        let expected = if matches!(activation, StableTabActivation::ArrowRight) {
            ITEM_C
        } else {
            ITEM_B
        };
        assert_eq!(committed.selected, Some(expected));
    }
}

#[test]
fn stale_close_keyboard_and_accesskit_requests_do_not_open_close_plans() {
    for (salt, events) in [
        ("stale-close-enter", key_press(Key::Enter)),
        ("stale-close-space", key_press(Key::Space)),
    ] {
        let context = Context::default();
        let (workspace, tabs) = tabs_workspace();
        let mut dockspace = Dockspace::builder(salt, workspace)
            .build()
            .expect("fixture facade must build");
        let mut panes = TestPanes;
        let stable = warm_tabs(&context, &mut dockspace, &mut panes, salt, tabs);
        context.memory_mut(|memory| memory.request_focus(stable.close_ids[1]));
        let focused = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            Vec::new(),
        );
        assert_eq!(focused.focused, Some(focused.close_ids[1]));

        let select_a = dockspace
            .core_engine()
            .workspace()
            .capture_item_source(ROOT, tabs, ITEM_A)
            .expect("selection source must capture");
        dockspace
            .submit_command(dockspace::command::WorkspaceCommand::Select { source: select_a })
            .expect("external selection invalidates the presented tab projection");
        context.options_mut(|options| {
            options.max_passes = 1.try_into().expect("one is non-zero");
        });

        let stale = run_frame(
            &context,
            &mut dockspace,
            &mut panes,
            salt,
            Some(tabs),
            None,
            events,
        );
        assert!(!stale.retained_presentation_current);
        assert!(stale.close_requests.is_empty());
        assert_eq!(stale.selected, Some(ITEM_A));
        assert!(
            dockspace
                .core_engine()
                .workspace()
                .item_multiset()
                .contains_key(&ITEM_B)
        );
    }

    let context = Context::default();
    let (workspace, tabs) = tabs_workspace();
    let salt = "stale-close-accesskit";
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture facade must build");
    let mut panes = TestPanes;
    let stable = warm_tabs(&context, &mut dockspace, &mut panes, salt, tabs);
    let select_a = dockspace
        .core_engine()
        .workspace()
        .capture_item_source(ROOT, tabs, ITEM_A)
        .expect("selection source must capture");
    dockspace
        .submit_command(dockspace::command::WorkspaceCommand::Select { source: select_a })
        .expect("external selection invalidates the presented tab projection");
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let stale = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        vec![accesskit_action(stable.close_ids[1], Action::Click)],
    );
    assert!(!stale.retained_presentation_current);
    assert!(stale.close_requests.is_empty());
    assert_eq!(stale.selected, Some(ITEM_A));
}

#[test]
fn same_tick_selection_supersedes_before_submission_and_disables_the_painted_response() {
    let context = Context::default();
    let (workspace, tabs) = tabs_workspace();
    let salt = "same-tick-selection-supersedes-contribution";
    let mut dockspace = Dockspace::builder(salt, workspace)
        .build()
        .expect("fixture facade must build");
    let mut panes = TestPanes;
    let stable = warm_tabs(&context, &mut dockspace, &mut panes, salt, tabs);
    context.memory_mut(|memory| memory.request_focus(stable.tab_ids[0]));
    let focused = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        Vec::new(),
    );
    assert_eq!(focused.focused, Some(focused.tab_ids[0]));
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });

    let selected = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        salt,
        Some(tabs),
        None,
        key_press(Key::ArrowRight),
    );

    assert_eq!(selected.selected, Some(ITEM_B));
    assert!(!selected.retained_presentation_current);
    assert!(selected.saw_stale_pass);
}
