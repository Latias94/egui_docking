mod support;

use dockspace::command::Edge;
use dockspace::drop_guide::{DropGuideScope, DropGuideSlot};
use dockspace::drop_target::DropTargetId;
use dockspace::engine::{CoreHostFrame, DockEngine, EngineInput};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, PointerButton, PointerId};
use dockspace::interaction::InteractionOutcome;
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, PointerInputLease, PointerProviderScope, SurfaceLocalPointerEndpoint,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceipt, PointerReceiverReceiptBatch,
    PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::scene::{ContainedResizeDirection, PresentationPlan, SplitterSceneId, TabSceneId};
use dockspace::transition::{EngineTransition, InputOutcome};
use support::{
    MeasurementProfile, TestPresentationHost, complete_host_frame_with_current_outputs,
    complete_host_frame_with_retained_or_unavailable, measurements, submit_input,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const OPEN_GPUI_PRESENTATION_SOURCE: StableInputSourceId = StableInputSourceId::new(0x0A91);
const POINTER: PointerId = PointerId::new(1);

#[derive(Debug, Clone, Copy)]
struct TestPointerStream {
    lease: PointerInputLease,
    through: u64,
}

fn rect(width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, height).expect("surface bounds are valid")
}

fn compile(
    workspace: Workspace,
    bounds: LogicalRect,
    tab_content_width: f64,
) -> (DockEngine, TestPresentationHost, PresentationPlan) {
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        bounds,
        MeasurementProfile {
            tab_content_width,
            ..MeasurementProfile::default()
        },
    );
    let ready = support::painted_plan(&engine, SURFACE).clone();
    (engine, host, ready)
}

fn create_pointer_stream(
    engine: &mut DockEngine,
    host: &TestPresentationHost,
) -> TestPointerStream {
    let lease = engine
        .create_pointer_provider(
            PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            )),
            PointerEdgeSequence::new(0),
        )
        .expect("the presentation fixture admits one surface-local pointer provider");
    TestPointerStream { lease, through: 0 }
}

fn empty_pointer_journal(pointer: TestPointerStream) -> PointerEdgeJournal {
    let watermark = PointerEdgeSequence::new(pointer.through);
    PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("an empty pointer journal preserves its watermark")
}

fn stage_pointer_edge(
    frame: &mut CoreHostFrame,
    pointer: &mut TestPointerStream,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    delivered_to: Option<PresentationHitRegionId>,
) {
    let previous = PointerEdgeSequence::new(pointer.through);
    let sequence = PointerEdgeSequence::new(
        pointer
            .through
            .checked_add(1)
            .expect("the test pointer sequence does not exhaust"),
    );
    let capture = if matches!(kind, PointerEdgeKind::ButtonReleased(_)) {
        PointerCaptureOwner::None
    } else {
        PointerCaptureOwner::ProviderEndpoint
    };
    let journal = PointerEdgeJournal::new(
        previous,
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            },
            Authority::Known(capture),
        )],
    )
    .expect("one pointer edge is contiguous");
    frame
        .submit_pointer_journal(pointer.lease, journal)
        .expect("the pointer edge follows the provider watermark");
    pointer.through = sequence.get();

    let candidate = frame
        .pointer_receiver_candidates()
        .expect("the pointer edge freezes one receiver roster")
        .candidates()
        .first()
        .expect("one pointer edge creates one receiver candidate")
        .clone();
    let observation = if let Some(region) = delivered_to {
        let projection = frame
            .view()
            .interaction_projection(SURFACE)
            .expect("the sealed frame retains current interaction authority");
        let delivery = PointerReceiverDelivery::new(
            projection,
            PointerReceiverDeliveryDisposition::Dock(region),
        )
        .expect("the resize receiver belongs to the current output");
        PointerReceiverObservation::Presented(
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                delivery,
            )])
            .expect("the resize press answers its exact delivery probe"),
        )
    } else {
        PointerReceiverObservation::NotApplicable
    };
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(observation)])
                .expect("the pointer receipt batch is exact"),
        )
        .expect("the pointer receiver receipt stages");
}

fn submit_pointer_edge(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    pointer: &mut TestPointerStream,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    delivered_to: Option<PresentationHitRegionId>,
) -> EngineTransition {
    let mut frame = host.begin(engine);
    stage_pointer_edge(&mut frame, pointer, kind, position, delivered_to);
    complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    host.finish(frame, engine)
}

fn pointer_interaction_outcome(transition: &EngineTransition) -> &InteractionOutcome {
    let [edge] = transition.reduced_pointer_edges() else {
        panic!(
            "expected one reduced pointer edge, got {:?}",
            transition.reduced_pointer_edges()
        );
    };
    let [outcome] = edge.interaction_outcomes() else {
        panic!(
            "expected one pointer interaction outcome, got {:?}",
            edge.interaction_outcomes()
        );
    };
    outcome
}

fn resize_region(
    engine: &DockEngine,
    kind: PresentationHitRegionKind,
) -> (PresentationHitRegionId, LogicalPoint) {
    let projection = engine
        .interaction_projection(SURFACE)
        .expect("the fixture surface has current interaction authority");
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| region.id().kind() == kind)
        .expect("the authoritative hit manifest contains the resize receiver");
    let bounds = region.hit().rect();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("the resize receiver center is finite");
    (region.id(), point)
}

fn republish_surface_with_active_pointer(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    pointer: TestPointerStream,
    bounds: LogicalRect,
) {
    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("the replacement surface contribution begins");
    let contribution = engine
        .prepare_surface_contribution(
            token,
            measurements(engine, SURFACE, bounds, MeasurementProfile::default()),
        )
        .expect("the replacement surface measurements prepare");
    let mut measurement_frame = host.begin(engine);
    measurement_frame
        .submit_pointer_journal(pointer.lease, empty_pointer_journal(pointer))
        .expect("the measurement frame preserves the pointer watermark");
    measurement_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("an empty journal has an empty receipt roster"),
        )
        .expect("empty measurement receipts stage");
    measurement_frame
        .push_surface_contribution(contribution)
        .expect("the replacement contribution stages");
    complete_host_frame_with_retained_or_unavailable(engine, &mut measurement_frame);
    host.finish(measurement_frame, engine);

    let mut paint_frame = host.begin(engine);
    paint_frame
        .submit_pointer_journal(pointer.lease, empty_pointer_journal(pointer))
        .expect("the paint frame preserves the pointer watermark");
    paint_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("an empty journal has an empty receipt roster"),
        )
        .expect("empty paint receipts stage");
    let paint_frame = complete_host_frame_with_current_outputs(engine, paint_frame);
    host.finish_presentation(paint_frame, engine);

    let mut observation_frame = host.begin(engine);
    observation_frame
        .submit_pointer_journal(pointer.lease, empty_pointer_journal(pointer))
        .expect("the observation frame preserves the pointer watermark");
    observation_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                .expect("an empty journal has an empty receipt roster"),
        )
        .expect("empty observation receipts stage");
    complete_host_frame_with_retained_or_unavailable(engine, &mut observation_frame);
    host.finish(observation_frame, engine);
}

fn adjust_splitter(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    scene: dockspace::scene::SurfaceSceneStamp,
    splitter: SplitterSceneId,
    delta: f64,
) -> InteractionOutcome {
    let expected = engine.version();
    let transition = submit_input(
        engine,
        host,
        OPEN_GPUI_PRESENTATION_SOURCE,
        EngineInput::AdjustSplitterResize {
            expected,
            scene,
            splitter,
            delta,
        },
    )
    .expect("splitter adjustment reduces");
    match transition.reduced_inputs() {
        [input] => match input.outcome() {
            InputOutcome::InteractionProcessed { outcome, .. } => outcome.clone(),
            outcome => panic!("unexpected splitter adjustment outcome: {outcome:?}"),
        },
        inputs => panic!("expected one splitter adjustment input, got {inputs:?}"),
    }
}

fn central_root_workspace(items: impl IntoIterator<Item = ItemId>) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("workspace is valid"), tabs)
}

fn area(rect: LogicalRect) -> f64 {
    rect.width() * rect.height()
}

#[test]
fn root_central_leaf_keeps_inner_center_separate_from_outer_four_way_guides() {
    // Ported from Open GPUI host_render_tests::root_central_leaf_hides_inner_side_guides
    // and root_drop_guides_use_outer_edge_drop_box_geometry.
    let (workspace, tabs) = central_root_workspace([ItemId::new(10), ItemId::new(11)]);
    let (engine, _host, ready) = compile(workspace, rect(500.0, 240.0), 56.0);

    let inner = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
        .expect("central root leaf has an inner cluster");
    assert_eq!(
        inner.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
        vec![DropGuideSlot::Center],
        "a root-central leaf owns only the inner center target"
    );

    let outer = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("the complete root has an independent outer cluster");
    assert_eq!(
        outer.targets().map(|(slot, _)| slot).collect::<Vec<_>>(),
        [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom]
            .map(DropGuideSlot::Edge)
            .to_vec()
    );

    for (slot, guide) in outer.targets() {
        let DropGuideSlot::Edge(edge) = slot else {
            panic!("outer clusters cannot contain a center target");
        };
        let draw = guide.draw();
        let hit = guide.target().region().rect();
        let preview = guide.target().visual().rect();
        assert!(
            hit.x() <= draw.x()
                && hit.y() <= draw.y()
                && hit.max().x() >= draw.max().x()
                && hit.max().y() >= draw.max().y(),
            "{edge:?} visible guide stays inside its exact hit geometry",
        );
        assert_ne!(draw, preview, "{edge:?} draw and preview stay distinct");
        assert_ne!(hit, preview, "{edge:?} hit and preview stay distinct");
        match edge {
            Edge::Left | Edge::Right => {
                assert_eq!(preview.width(), ready.bounds().width() * 0.5);
                assert_eq!(preview.height(), ready.bounds().height());
            }
            Edge::Top | Edge::Bottom => {
                assert_eq!(preview.width(), ready.bounds().width());
                assert_eq!(preview.height(), ready.bounds().height() * 0.5);
            }
        }
    }
    assert_eq!(engine.presentation_config().dock_fraction(), 0.5);
}

#[test]
fn nested_central_leaf_retains_center_and_all_four_inner_edges() {
    // Ported from Open GPUI host_render_tests::nested_central_region_drop_guides_keep_side_zones.
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let central = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let sibling = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let nested = builder.insert_node(
        Node::equal_split(Axis::Vertical, [central, sibling]).expect("nested split is valid"),
    );
    let root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [source, nested]).expect("root split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(root).with_central(central));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let (_engine, _host, ready) = compile(workspace, rect(640.0, 360.0), 56.0);

    let inner = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(central))
        .expect("nested central leaf has an inner cluster");
    assert_eq!(inner.targets().count(), 5);
    assert!(inner.target(DropGuideSlot::Center).is_some());
    for edge in [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom] {
        let guide = inner
            .target(DropGuideSlot::Edge(edge))
            .unwrap_or_else(|| panic!("nested central leaf exposes {edge:?}"));
        assert!(area(guide.target().region().rect()) > area(guide.draw()));
        assert_ne!(guide.draw(), guide.target().visual().rect());
        assert_ne!(
            guide.target().region().rect(),
            guide.target().visual().rect()
        );
    }

    let outer = ready
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
        .expect("root keeps its independent outer cluster");
    assert_eq!(outer.targets().count(), 4);
}

#[test]
fn narrow_three_tab_strip_publishes_only_operable_tabs_and_stable_gap_indices() {
    // This carries Open GPUI's narrow-target clipping contract into the core-owned
    // presentation protocol and additionally proves stable pre-scroll gap identities.
    let first = ItemId::new(10);
    let second = ItemId::new(11);
    let selected = ItemId::new(12);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs_with_selection(
        [first, second, selected],
        Some(selected),
    ));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("selected workspace is valid");

    let (automatic_engine, _automatic_host, automatic) =
        compile(workspace, rect(180.0, 180.0), 180.0);
    assert_eq!(visible_items(&automatic, tabs), vec![second, selected]);
    assert_eq!(tab_gap_indices(&automatic, tabs), vec![1, 2, 3]);

    let bar = automatic
        .tab_bar_records()
        .iter()
        .find(|bar| bar.id().tabs == tabs)
        .expect("tab bar exists")
        .clone();
    assert!(bar.viewport().x() >= bar.bounds().x());
    assert!(bar.viewport().max().x() <= bar.bounds().max().x());
    assert!(bar.scroll_offset() <= bar.maximum_scroll_offset());
    for tab in automatic
        .tab_records()
        .iter()
        .filter(|tab| tab.id().tabs == tabs)
    {
        assert!(tab.visible_bounds().x() >= bar.viewport().x());
        assert!(tab.visible_bounds().max().x() <= bar.viewport().max().x());
        assert!(
            tab.visible_bounds().width()
                >= automatic_engine.presentation_config().tab_close_extent(),
            "published tabs remain operable"
        );
        assert!(tab.drag_hit().rect().width() > 0.0);
        assert!(tab.drag_hit().rect().max().x() <= tab.visible_bounds().max().x());
        let close = tab.close_bounds().expect("fixture tabs are closeable");
        assert!(close.x() >= tab.drag_hit().rect().max().x());
        assert!(close.max().x() <= tab.visible_bounds().max().x());
        assert!(tab.text_bounds().x() >= tab.visible_bounds().x());
        assert!(tab.text_bounds().max().x() <= tab.visible_bounds().max().x());
    }
    assert_eq!(automatic.tab_bar_records()[0].hidden_items(), &[first]);
    assert_eq!(
        automatic
            .tab_records()
            .iter()
            .map(|tab| tab.ordinal())
            .collect::<Vec<_>>(),
        [1, 2]
    );
    assert!(automatic.tab_records()[1].selected());
}

#[test]
fn splitter_records_keep_draw_hit_and_adjacent_geometry_in_one_plan() {
    // Ported from Open GPUI host_divider_hit_map_tests: visible dividers and
    // expanded hit regions come from one authoritative geometry pass.
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let middle = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, middle, right])
            .expect("three-way split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let (_engine, _host, ready) = compile(workspace, rect(640.0, 240.0), 56.0);
    let records = ready
        .splitter_records()
        .iter()
        .filter(|record| record.id().split == split)
        .collect::<Vec<_>>();

    assert_eq!(records.len(), 2);
    for (index, record) in records.iter().enumerate() {
        assert_eq!(record.id().index, index);
        assert_eq!(record.axis(), Axis::Horizontal);
        assert!(record.hit().rect().x() <= record.draw_bounds().x());
        assert!(record.hit().rect().max().x() >= record.draw_bounds().max().x());
        assert!(record.before_bounds().max().x() <= record.after_bounds().x());
        assert_eq!(record.weights().len(), 3);
    }
    assert!(records[0].hit().rect().max().x() <= records[1].hit().rect().x());
}

#[test]
fn active_resize_override_is_the_geometry_compiled_into_the_plan() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("split is valid"));
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, rect(400.0, 200.0));
    let painted = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface is painted");
    let initial = painted
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .expect("splitter is painted");
    let press = LogicalPoint::new(
        initial.draw_bounds().x() + initial.draw_bounds().width() * 0.5,
        initial.draw_bounds().y() + initial.draw_bounds().height() * 0.5,
    )
    .expect("press point is valid");
    let before = initial.child_extents()[0];
    let available = initial.child_extents().iter().sum::<f64>();
    let splitter = *initial.id();
    let (region, region_press) =
        resize_region(&engine, PresentationHitRegionKind::SplitterHandle(splitter));
    assert_eq!(press, region_press);
    let mut pointer = create_pointer_stream(&mut engine, &host);
    let transition = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        Some(region),
    );
    let session = match pointer_interaction_outcome(&transition) {
        InteractionOutcome::ResizeBegan { session, .. } => *session,
        outcome => panic!("unexpected begin outcome: {outcome:?}"),
    };
    let override_weights =
        dockspace::graph::SplitWeight::normalize([0.25, 0.75]).expect("override weights are valid");
    let update_point = LogicalPoint::new(press.x() + available * 0.25 - before, press.y())
        .expect("update point is valid");
    let updated = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::Moved,
        update_point,
        None,
    );
    assert!(matches!(
        pointer_interaction_outcome(&updated),
        InteractionOutcome::ResizeUpdated { session: actual, .. } if *actual == session
    ));

    republish_surface_with_active_pointer(&mut engine, &mut host, pointer, rect(400.0, 200.0));
    let plan = support::painted_plan(&engine, SURFACE);
    let splitter = plan
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .expect("splitter record exists");
    assert_eq!(splitter.weights(), override_weights);
    assert!((splitter.before_bounds().width() - 99.75).abs() <= 1.0e-9);
    assert!((splitter.after_bounds().width() - 299.25).abs() <= 1.0e-9);
    assert!(matches!(
        engine.workspace().node(split),
        Some(Node::Split { weights, .. })
            if weights
                == &dockspace::graph::SplitWeight::normalize([0.5, 0.5])
                    .expect("durable weights are valid")
    ));
}

#[test]
fn pointer_keyboard_and_accessibility_resize_share_the_scene_bound_evaluator() {
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("split is valid"));
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");

    let mut pointer_engine =
        DockEngine::new(workspace.clone(), DockPolicy::default()).expect("engine is valid");
    let mut pointer_host = TestPresentationHost::new(&mut pointer_engine);
    support::publish_surface(
        &mut pointer_engine,
        &mut pointer_host,
        SURFACE,
        rect(400.0, 200.0),
    );
    let painted = pointer_engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface is painted");
    let record = painted
        .plan()
        .splitter_records()
        .iter()
        .find(|record| record.id().split == split)
        .expect("splitter is painted");
    let splitter = *record.id();
    let press = LogicalPoint::new(
        record.draw_bounds().x() + record.draw_bounds().width() * 0.5,
        record.draw_bounds().y() + record.draw_bounds().height() * 0.5,
    )
    .expect("press point is valid");
    let delta = 64.0;
    let (region, region_press) = resize_region(
        &pointer_engine,
        PresentationHitRegionKind::SplitterHandle(splitter),
    );
    assert_eq!(press, region_press);
    let mut pointer = create_pointer_stream(&mut pointer_engine, &pointer_host);
    let pressed = submit_pointer_edge(
        &mut pointer_engine,
        &mut pointer_host,
        &mut pointer,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        Some(region),
    );
    let session = match pointer_interaction_outcome(&pressed) {
        InteractionOutcome::ResizeBegan { session, .. } => *session,
        outcome => panic!("unexpected begin outcome: {outcome:?}"),
    };
    let update_point =
        LogicalPoint::new(press.x() + delta, press.y()).expect("update point is valid");
    let updated = submit_pointer_edge(
        &mut pointer_engine,
        &mut pointer_host,
        &mut pointer,
        PointerEdgeKind::Moved,
        update_point,
        None,
    );
    assert!(matches!(
        pointer_interaction_outcome(&updated),
        InteractionOutcome::ResizeUpdated { session: actual, .. } if *actual == session
    ));
    let released = submit_pointer_edge(
        &mut pointer_engine,
        &mut pointer_host,
        &mut pointer,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        update_point,
        None,
    );
    assert!(matches!(
        pointer_interaction_outcome(&released),
        InteractionOutcome::ResizeDelivered { session: actual, changed: true, .. }
            if *actual == session
    ));

    let mut action_engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut action_host = TestPresentationHost::new(&mut action_engine);
    support::publish_surface(
        &mut action_engine,
        &mut action_host,
        SURFACE,
        rect(400.0, 200.0),
    );
    let action_scene = action_engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface is painted")
        .stamp();
    assert!(matches!(
        adjust_splitter(
            &mut action_engine,
            &mut action_host,
            action_scene,
            splitter,
            delta,
        ),
        InteractionOutcome::SplitterAdjusted { changed: true, .. }
    ));

    assert_eq!(
        pointer_engine.workspace().node(split),
        action_engine.workspace().node(split),
        "all rendered resize entry points must use the same core geometry evaluator"
    );
    let committed = action_engine.workspace().node(split).cloned();
    assert_eq!(
        adjust_splitter(
            &mut action_engine,
            &mut action_host,
            action_scene,
            splitter,
            delta,
        ),
        InteractionOutcome::Rejected(dockspace::interaction::InteractionRejection::StaleScene)
    );
    assert_eq!(action_engine.workspace().node(split).cloned(), committed);
}

#[test]
fn junction_resize_updates_and_commits_both_axes_atomically() {
    // Ported from Open GPUI's pair-only `corner_splitter_drag_*` behavior tests,
    // then strengthened here to target the core-owned junction model.
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let top_right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let bottom_right = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let vertical = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_right, bottom_right])
            .expect("vertical split is valid"),
    );
    let horizontal = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, vertical]).expect("horizontal split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(horizontal));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let mut engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine is valid");
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, rect(400.0, 240.0));

    let painted = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("surface is painted");
    let [junction] = painted.plan().splitter_junction_records() else {
        panic!("nested perpendicular splits expose one exact junction");
    };
    let hit = junction.hit().rect();
    let press = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
        .expect("junction center is valid");
    let junction_id = junction.id();
    let initial_version = engine.version();
    let initial_horizontal = engine.workspace().node(horizontal).cloned();
    let initial_vertical = engine.workspace().node(vertical).cloned();

    let (region, region_press) = resize_region(
        &engine,
        PresentationHitRegionKind::SplitterJunction(junction_id),
    );
    assert_eq!(press, region_press);
    let mut pointer = create_pointer_stream(&mut engine, &host);
    let pressed = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        press,
        Some(region),
    );
    let session = match pointer_interaction_outcome(&pressed) {
        InteractionOutcome::ResizeBegan { session, .. } => *session,
        outcome => panic!("unexpected begin outcome: {outcome:?}"),
    };
    let update_point =
        LogicalPoint::new(press.x() + 80.0, press.y() + 48.0).expect("corner update is valid");
    let updated = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::Moved,
        update_point,
        None,
    );
    let update = pointer_interaction_outcome(&updated);
    let InteractionOutcome::ResizeUpdated {
        session: actual,
        splits,
        ..
    } = update
    else {
        panic!("corner update must produce one atomic batch: {update:?}");
    };
    assert_eq!(*actual, session);
    assert_eq!(splits.len(), 2);
    assert_eq!(engine.version(), initial_version);
    assert_eq!(
        engine.workspace().node(horizontal).cloned(),
        initial_horizontal
    );
    assert_eq!(engine.workspace().node(vertical).cloned(), initial_vertical);
    let (_, overrides) = engine
        .interaction()
        .resize_overrides()
        .expect("corner session publishes transient overrides");
    assert_eq!(overrides.len(), 2);
    assert!(engine.scene().ready_surface(SURFACE).is_none());

    let released = submit_pointer_edge(
        &mut engine,
        &mut host,
        &mut pointer,
        PointerEdgeKind::ButtonReleased(PointerButton::Primary),
        update_point,
        None,
    );
    let delivered = pointer_interaction_outcome(&released);
    let InteractionOutcome::ResizeDelivered {
        session: actual,
        outcome:
            dockspace::command::CommandOutcome::SplitsResized {
                splits: committed,
                changed: true,
            },
        changed: true,
        ..
    } = delivered
    else {
        panic!("corner release must commit one two-split command: {delivered:?}");
    };
    assert_eq!(*actual, session);
    assert_eq!(committed.len(), 2);
    assert_eq!(
        engine.version().revision().get(),
        initial_version.revision().get() + 1,
        "both axes publish through one workspace revision"
    );
    assert_ne!(
        engine.workspace().node(horizontal).cloned(),
        initial_horizontal
    );
    assert_ne!(engine.workspace().node(vertical).cloned(), initial_vertical);
}

#[test]
fn perpendicular_splitter_records_preserve_one_exact_junction_intersection() {
    // Ported from Open GPUI host_divider_hit_map_tests::
    // `divider_hit_map_prefers_corner_when_splitter_hits_intersect`. The core
    // records must preserve enough exact geometry to derive one unambiguous
    // two-axis junction target rather than resolving either splitter by order.
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let top_right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let bottom_right = builder.insert_node(Node::tabs([ItemId::new(12)]));
    let vertical = builder.insert_node(
        Node::equal_split(Axis::Vertical, [top_right, bottom_right])
            .expect("nested split is valid"),
    );
    let horizontal = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [left, vertical]).expect("root split is valid"),
    );
    builder.set_root(ROOT, RootRecord::new(horizontal));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");
    let (_engine, _host, ready) = compile(workspace, rect(400.0, 240.0), 56.0);

    let records = ready.splitter_records();
    assert_eq!(records.len(), 2);
    let north_south = records
        .iter()
        .find(|record| record.axis() == Axis::Horizontal)
        .expect("vertical splitter supplies the junction x-band");
    let west_east = records
        .iter()
        .find(|record| record.axis() == Axis::Vertical)
        .expect("horizontal splitter supplies the junction y-band");
    let x_band = north_south.hit().rect();
    let y_band = west_east.hit().rect();
    let junction_hit = LogicalRect::new(x_band.x(), y_band.y(), x_band.width(), y_band.height())
        .expect("perpendicular hit bands form valid junction geometry");
    let [compiled_junction] = ready.splitter_junction_records() else {
        panic!("the core plan must publish the unique junction winner");
    };
    assert_eq!(compiled_junction.hit().rect(), junction_hit);
    assert_eq!(compiled_junction.layer(), north_south.layer());
    assert_eq!(
        compiled_junction.id().splitters(),
        if north_south.id() <= west_east.id() {
            [*north_south.id(), *west_east.id()]
        } else {
            [*west_east.id(), *north_south.id()]
        }
    );
    assert!(
        [north_south.id().split, west_east.id().split]
            .into_iter()
            .all(|split| split == horizontal || split == vertical)
    );
    assert!(
        positive_intersection(north_south.draw_bounds(), west_east.draw_bounds()).is_none(),
        "the junction is a composed hit target, not an invented paint overlap"
    );
    assert_eq!(north_south.layer(), west_east.layer());
}

#[test]
fn contained_chrome_partitions_move_close_content_and_eight_resize_regions() {
    // Derived from Open GPUI render_floating_bounds_match_presentation_scene_container
    // and accessibility_scene_enumerates_presentation_roles. The adapter must
    // receive one non-conflicting semantic chrome partition from the core.
    const FLOATING: FloatingPresentationId = FloatingPresentationId::new(30);
    let outer = LogicalRect::new(20.0, 24.0, 260.0, 180.0).expect("floating bounds are valid");
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(FLOATING, ContainedFloating::new(ROOT, outer));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    let workspace = builder.build().expect("workspace is valid");
    let (_engine, _host, ready) = compile(workspace, rect(320.0, 240.0), 56.0);

    let [contained] = ready.contained_records() else {
        panic!("one contained presentation must produce one chrome record");
    };
    assert_eq!(contained.outer_bounds(), outer);
    assert!(contains_rect(outer, contained.inner_bounds()));
    assert!(contains_rect(
        contained.inner_bounds(),
        contained.title_bounds()
    ));
    assert!(contains_rect(
        contained.inner_bounds(),
        contained.content_bounds()
    ));
    assert!(positive_intersection(contained.title_bounds(), contained.content_bounds()).is_none());
    assert_eq!(
        contained.title_bounds().height() + contained.content_bounds().height(),
        contained.inner_bounds().height()
    );

    let close = contained
        .close_bounds()
        .expect("the closeable root exposes a close control");
    assert!(contains_rect(contained.title_bounds(), close));
    assert!(contains_rect(
        contained.title_bounds(),
        contained.title_drag_hit().rect()
    ));
    assert!(
        positive_intersection(close, contained.title_drag_hit().rect()).is_none(),
        "close and title drag cannot compete for the same pointer"
    );

    assert_eq!(
        contained
            .resize()
            .iter()
            .map(|resize| resize.direction())
            .collect::<Vec<_>>(),
        vec![
            ContainedResizeDirection::NorthWest,
            ContainedResizeDirection::North,
            ContainedResizeDirection::NorthEast,
            ContainedResizeDirection::East,
            ContainedResizeDirection::SouthEast,
            ContainedResizeDirection::South,
            ContainedResizeDirection::SouthWest,
            ContainedResizeDirection::West,
        ]
    );
    for (index, first) in contained.resize().iter().enumerate() {
        assert!(area(first.hit().rect()) > 0.0);
        assert!(contains_rect(outer, first.hit().rect()));
        assert!(
            positive_intersection(first.hit().rect(), close).is_none(),
            "resize and close cannot compete for the same pointer"
        );
        assert!(
            positive_intersection(first.hit().rect(), contained.title_drag_hit().rect()).is_none(),
            "resize and title drag cannot compete for the same pointer"
        );
        for second in contained.resize().iter().skip(index + 1) {
            assert!(
                positive_intersection(first.hit().rect(), second.hit().rect()).is_none(),
                "each point on the resize border has one direction"
            );
        }
    }

    let occlusion = ready
        .drop_occlusions()
        .iter()
        .find(|occlusion| occlusion.floating() == FLOATING)
        .expect("contained chrome owns one drop occlusion");
    assert_eq!(occlusion.region().rect(), outer);
    assert_eq!(occlusion.layer(), contained.layer());
}

#[test]
fn contained_layers_follow_back_to_front_roster_instead_of_ids_or_geometry() {
    // Open GPUI treats floating-container order as the sole z-order authority.
    // Stable ids and overlapping geometry must not silently reorder that roster.
    const BACK: FloatingPresentationId = FloatingPresentationId::new(90);
    const FRONT: FloatingPresentationId = FloatingPresentationId::new(10);
    const BACK_ROOT: RootId = RootId::new(20);
    const FRONT_ROOT: RootId = RootId::new(30);
    let shared_bounds =
        LogicalRect::new(30.0, 30.0, 240.0, 160.0).expect("contained bounds are valid");
    let mut builder = Workspace::builder();
    let back_tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let front_tabs = builder.insert_node(Node::tabs([ItemId::new(11)]));
    builder.set_root(BACK_ROOT, RootRecord::new(back_tabs));
    builder.set_root(FRONT_ROOT, RootRecord::new(front_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(BACK, ContainedFloating::new(BACK_ROOT, shared_bounds));
    builder.set_contained_floating(FRONT, ContainedFloating::new(FRONT_ROOT, shared_bounds));
    builder
        .attach_contained(SURFACE, BACK)
        .expect("surface exists");
    builder
        .attach_contained(SURFACE, FRONT)
        .expect("surface exists");
    let workspace = builder.build().expect("workspace is valid");
    let (_engine, _host, ready) = compile(workspace, rect(320.0, 240.0), 56.0);

    let records = ready.contained_records();
    assert_eq!(
        records
            .iter()
            .map(|record| record.floating())
            .collect::<Vec<_>>(),
        vec![BACK, FRONT]
    );
    assert_eq!(
        records
            .iter()
            .map(|record| record.ordinal())
            .collect::<Vec<_>>(),
        vec![0, 1]
    );
    assert!(records[0].layer() < records[1].layer());
    assert_eq!(records[0].outer_bounds(), records[1].outer_bounds());

    for record in records {
        let occlusion = ready
            .drop_occlusions()
            .iter()
            .find(|occlusion| occlusion.floating() == record.floating())
            .expect("every contained record has matching occlusion authority");
        assert_eq!(occlusion.layer(), record.layer());
        assert_eq!(occlusion.region().rect(), shared_bounds);
        assert!(
            ready
                .tab_records()
                .iter()
                .filter(|tab| tab.id().root == record.root())
                .all(|tab| tab.layer() == record.layer())
        );
    }
}

#[test]
fn structural_records_are_stable_unique_and_keep_final_accessibility_state() {
    // Ported from Open GPUI
    // accessibility_gpui_mapping_exposes_stable_roles_ids_and_final_tab_actions
    // and accessibility_scene_enumerates_splitters. Framework role and action
    // objects remain adapter mappings, but their structural identity, selection,
    // axis, bounds, and deterministic order must come from the presentation plan.
    let first = ItemId::new(10);
    let selected = ItemId::new(11);
    let right_item = ItemId::new(12);
    let mut builder = Workspace::builder();
    let left = builder.insert_node(Node::tabs_with_selection([first, selected], Some(selected)));
    let right = builder.insert_node(Node::tabs([right_item]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [left, right]).expect("split is valid"));
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().expect("workspace is valid");

    let (_first_engine, _first_host, first_plan) =
        compile(workspace.clone(), rect(480.0, 240.0), 56.0);
    let (_second_engine, _second_host, second_plan) = compile(workspace, rect(480.0, 240.0), 56.0);
    let signature = |plan: &PresentationPlan| {
        (
            plan.tab_records()
                .iter()
                .map(|tab| (*tab.id(), tab.selected(), tab.ordinal(), tab.layer()))
                .collect::<Vec<_>>(),
            plan.tab_bar_records()
                .iter()
                .map(|bar| (*bar.id(), bar.layer()))
                .collect::<Vec<_>>(),
            plan.splitter_records()
                .iter()
                .map(|splitter| (*splitter.id(), splitter.axis(), splitter.layer()))
                .collect::<Vec<_>>(),
        )
    };
    assert_eq!(signature(&first_plan), signature(&second_plan));

    let mut tab_ids = first_plan
        .tab_records()
        .iter()
        .map(|tab| *tab.id())
        .collect::<Vec<_>>();
    let tab_count = tab_ids.len();
    tab_ids.sort_unstable();
    tab_ids.dedup();
    assert_eq!(tab_ids.len(), tab_count, "tab semantic ids are unique");
    assert_eq!(
        first_plan
            .tab_records()
            .iter()
            .filter(|tab| tab.selected())
            .map(|tab| tab.id().item)
            .collect::<Vec<_>>(),
        vec![selected, right_item],
        "each tabs node carries its final selected item"
    );
    assert_eq!(first_plan.splitter_records()[0].axis(), Axis::Horizontal);
}

#[test]
fn clipped_tiny_floating_keeps_a_real_title_drag_and_does_not_invent_a_south_edge() {
    // Ported from Open GPUI
    // render_tiny_floating_handle_clamps_to_presentation_title_bar. The durable
    // floating remains large, but only its top title strip is visible after a
    // surface resize. The formal contribution must stay Ready without rewriting
    // that durable rect, and clipping must not turn the clip boundary into a fake edge.
    const FLOATING: FloatingPresentationId = FloatingPresentationId::new(30);
    let durable =
        LogicalRect::new(10.0, 208.0, 220.0, 140.0).expect("durable floating bounds are valid");
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(10)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    builder.set_contained_floating(FLOATING, ContainedFloating::new(ROOT, durable));
    builder
        .attach_contained(SURFACE, FLOATING)
        .expect("surface exists");
    let workspace = builder.build().expect("workspace is valid");
    let (_engine, _host, ready) = compile(workspace, rect(320.0, 220.0), 56.0);
    let [contained] = ready.contained_records() else {
        panic!("the visible title strip produces one contained record");
    };

    assert_eq!(contained.outer_bounds().height(), 12.0);
    assert!(contained.title_bounds().height() > 0.0);
    let off_surface_south_area = [
        ContainedResizeDirection::SouthEast,
        ContainedResizeDirection::South,
        ContainedResizeDirection::SouthWest,
    ]
    .into_iter()
    .map(|direction| {
        contained
            .resize()
            .iter()
            .find(|resize| resize.direction() == direction)
            .map(|resize| area(resize.hit().rect()))
            .expect("all directions keep stable identities")
    })
    .sum::<f64>();
    assert!(
        area(contained.title_drag_hit().rect()) > 0.0 && off_surface_south_area == 0.0,
        "a visible title strip must remain movable and an off-surface durable edge must not move to the clipping boundary; title drag area={}, invented south area={off_surface_south_area}",
        area(contained.title_drag_hit().rect()),
    );
}

fn visible_items(ready: &PresentationPlan, tabs: NodeId) -> Vec<ItemId> {
    ready
        .tab_records()
        .iter()
        .filter_map(|tab| {
            let TabSceneId {
                tabs: owner, item, ..
            } = *tab.id();
            (owner == tabs).then_some(item)
        })
        .collect()
}

fn tab_gap_indices(ready: &PresentationPlan, tabs: NodeId) -> Vec<usize> {
    ready
        .drop_targets()
        .iter()
        .filter_map(|target| match target.id() {
            DropTargetId::TabGap {
                tabs: owner, index, ..
            } if owner == tabs => Some(index),
            _ => None,
        })
        .collect()
}

fn contains_rect(outer: LogicalRect, inner: LogicalRect) -> bool {
    inner.x() >= outer.x()
        && inner.y() >= outer.y()
        && inner.max().x() <= outer.max().x()
        && inner.max().y() <= outer.max().y()
}

fn positive_intersection(left: LogicalRect, right: LogicalRect) -> Option<LogicalRect> {
    let x = left.x().max(right.x());
    let y = left.y().max(right.y());
    let max_x = left.max().x().min(right.max().x());
    let max_y = left.max().y().min(right.max().y());
    (max_x > x && max_y > y).then(|| {
        LogicalRect::new(x, y, max_x - x, max_y - y)
            .expect("the intersection of finite rectangles is valid")
    })
}
