use super::support;

use dockspace::command::MovePayload;
use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, PointerButton, PointerId, SurfacePointer, TabGestureSource};
use dockspace::interaction::{InteractionOutcome, InteractionStatus};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::scene::{SurfaceSceneStamp, TabBarSceneId, TabGroupDragRegionKind};
use dockspace::transition::EngineTransition;

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const REAR_ROOT: RootId = RootId::new(11);
const FRONT_ROOT: RootId = RootId::new(12);
const REAR_FLOATING: FloatingPresentationId = FloatingPresentationId::new(20);
const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(21);
const MAIN_ITEM: ItemId = ItemId::new(1);
const INACTIVE_MAIN_ITEM: ItemId = ItemId::new(4);
const REAR_ITEM: ItemId = ItemId::new(2);
const INACTIVE_REAR_ITEM: ItemId = ItemId::new(5);
const FRONT_ITEM: ItemId = ItemId::new(3);
const POINTER: PointerId = PointerId::new(1);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("fixture rectangle is finite")
}

fn center(rect: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        (rect.min().x() + rect.max().x()) * 0.5,
        (rect.min().y() + rect.max().y()) * 0.5,
    )
    .expect("fixture point is finite")
}

fn workspace(rear: LogicalRect, front: LogicalRect) -> Workspace {
    let mut builder = Workspace::builder();
    let main = builder.insert_node(Node::tabs([MAIN_ITEM, INACTIVE_MAIN_ITEM]));
    let rear_root = builder.insert_node(Node::tabs([REAR_ITEM, INACTIVE_REAR_ITEM]));
    let front_root = builder.insert_node(Node::tabs([FRONT_ITEM]));
    builder.set_root(MAIN_ROOT, RootRecord::new(main));
    builder.set_root(REAR_ROOT, RootRecord::new(rear_root));
    builder.set_root(FRONT_ROOT, RootRecord::new(front_root));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(REAR_FLOATING, ContainedFloating::new(REAR_ROOT, rear));
    builder.set_contained_floating(FRONT_FLOATING, ContainedFloating::new(FRONT_ROOT, front));
    builder
        .attach_contained(SURFACE, REAR_FLOATING)
        .expect("surface exists");
    builder
        .attach_contained(SURFACE, FRONT_FLOATING)
        .expect("surface exists");
    builder.build().expect("fixture workspace is valid")
}

fn engine(rear: LogicalRect, front: LogicalRect) -> (DockEngine, support::TestPresentationHost) {
    let mut engine = DockEngine::new(workspace(rear, front), Default::default())
        .expect("fixture engine is valid");
    let mut host = support::TestPresentationHost::new(&mut engine);
    support::publish_surface(
        &mut engine,
        &mut host,
        SURFACE,
        rect(0.0, 0.0, 640.0, 480.0),
    );
    (engine, host)
}

fn tab_press(
    engine: &DockEngine,
    root: RootId,
    item: ItemId,
) -> (SurfaceSceneStamp, TabGestureSource, SurfacePointer) {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("painted scene exists");
    let record = ready
        .plan()
        .tab_records()
        .iter()
        .find(|record| record.id().root == root && record.id().item == item)
        .expect("tab record exists");
    (
        ready.stamp(),
        TabGestureSource::Item(*record.id()),
        SurfacePointer::new(SURFACE, center(record.drag_hit().rect())),
    )
}

fn group_press(
    engine: &DockEngine,
    root: RootId,
    region: TabGroupDragRegionKind,
) -> (SurfaceSceneStamp, TabGestureSource, SurfacePointer) {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("painted scene exists");
    let bar = ready
        .plan()
        .tab_bar_records()
        .iter()
        .find(|record| record.id().root == root)
        .expect("tab bar record exists");
    let group = bar
        .group_drag()
        .expect("non-empty tabs have group drag regions");
    let region = group
        .region(region)
        .expect("requested group drag region exists");
    (
        ready.stamp(),
        TabGestureSource::Group(TabBarSceneId {
            root,
            tabs: bar.id().tabs,
        }),
        SurfacePointer::new(SURFACE, center(region.hit().rect())),
    )
}

fn activate(
    engine: &mut DockEngine,
    host: &mut support::TestPresentationHost,
    scene: SurfaceSceneStamp,
    source: TabGestureSource,
    press: SurfacePointer,
) -> EngineTransition {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("painted scene exists");
    assert_eq!(ready.stamp(), scene);
    let region = engine
        .interaction_projection(SURFACE)
        .expect("painted scene is interactive")
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| match (source, region.id().kind()) {
            (TabGestureSource::Item(expected), PresentationHitRegionKind::TabBody(actual)) => {
                actual == expected
            }
            (
                TabGestureSource::Group(expected),
                PresentationHitRegionKind::TabGroupDrag { bar: actual, .. },
            ) => actual == expected && region.hit().contains(press.position()),
            _ => false,
        })
        .copied()
        .expect("gesture source has one exact pointer receiver");
    assert!(region.hit().contains(press.position()));
    let provider = engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider must be admitted");
    let sequence = PointerEdgeSequence::new(1);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(press.position()),
            },
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    )
    .expect("activation pointer journal is contiguous");
    let mut frame = host.begin(engine);
    frame
        .submit_surface_pointer_journal(&provider, journal)
        .expect("activation pointer edge stages");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("activation freezes one receiver candidate")
        .candidates()[0]
        .clone();
    let projection = frame
        .view()
        .interaction_projection(SURFACE)
        .expect("sealed frame retains interaction authority");
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(region.id()),
    )
    .expect("activation delivery is output-bound");
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("activation answers the exact delivery probe"),
                ),
            )])
            .expect("activation receipt batch is exact"),
        )
        .expect("activation receipt stages");
    support::complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    let transition = host.finish(frame, engine);
    assert_eq!(
        provider.committed_through(),
        sequence,
        "committed activation watermark must advance"
    );
    transition
}

fn interaction_outcome(transition: &EngineTransition) -> &InteractionOutcome {
    let outcomes = transition.reduced_pointer_edges()[0].interaction_outcomes();
    assert_eq!(
        outcomes.len(),
        1,
        "activation emits one interaction outcome"
    );
    &outcomes[0]
}

fn selected_item(engine: &DockEngine, root: RootId) -> Option<ItemId> {
    let tabs = engine.workspace().root(root).expect("root exists").node;
    let Node::Tabs { selected, .. } = engine.workspace().node(tabs).expect("tabs exist") else {
        panic!("fixture root must contain tabs");
    };
    *selected
}

#[test]
fn rear_contained_item_atomically_raises_and_arms() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let (scene, source, press) = tab_press(&engine, REAR_ROOT, REAR_ITEM);
    let transition = activate(&mut engine, &mut host, scene, source, press);

    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert_eq!(
        engine
            .workspace()
            .surface(SURFACE)
            .expect("surface exists")
            .contained,
        vec![FRONT_FLOATING, REAR_FLOATING]
    );
    assert!(transition.events().iter().any(|event| matches!(
        event.kind(),
        dockspace::event::WorkspaceEventKind::CommandCommitted(
            dockspace::command::CommandOutcome::ContainedRaised {
                floating: REAR_FLOATING,
                changed: true,
                ..
            }
        )
    )));
    let active = engine
        .interaction()
        .active_drag_view()
        .expect("item gesture arms a drag");
    assert!(matches!(
        active.payload(),
        MovePayload::Item(item) if item.root() == REAR_ROOT && item.item() == REAR_ITEM
    ));
    assert!(matches!(
        engine.interaction().status(),
        InteractionStatus::Armed { .. }
    ));
}

#[test]
fn inactive_contained_tab_selects_raises_and_arms_in_one_revision() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let before = engine.version();
    let (scene, source, press) = tab_press(&engine, REAR_ROOT, INACTIVE_REAR_ITEM);

    let transition = activate(&mut engine, &mut host, scene, source, press);

    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert_eq!(selected_item(&engine, REAR_ROOT), Some(INACTIVE_REAR_ITEM));
    assert_eq!(
        engine
            .workspace()
            .surface(SURFACE)
            .expect("surface exists")
            .contained,
        vec![FRONT_FLOATING, REAR_FLOATING]
    );
    assert_eq!(
        engine.version().revision().get(),
        before.revision().get() + 1,
        "contained raise and inactive-tab selection share one revision"
    );
    assert!(matches!(
        engine
            .interaction()
            .active_drag_view()
            .expect("contained inactive tab arms a drag")
            .payload(),
        MovePayload::Item(source) if source.item() == INACTIVE_REAR_ITEM
    ));
}

#[test]
fn inactive_tab_selection_and_drag_arm_publish_atomically_at_one_revision() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let before = engine.version();
    assert_eq!(selected_item(&engine, MAIN_ROOT), Some(MAIN_ITEM));
    let (scene, source, press) = tab_press(&engine, MAIN_ROOT, INACTIVE_MAIN_ITEM);

    let transition = activate(&mut engine, &mut host, scene, source, press);

    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert_eq!(selected_item(&engine, MAIN_ROOT), Some(INACTIVE_MAIN_ITEM));
    assert_eq!(
        engine.version().revision().get(),
        before.revision().get() + 1,
        "selection and gesture activation publish through one workspace revision"
    );
    assert!(matches!(
        engine
            .interaction()
            .active_drag_view()
            .expect("inactive tab gesture arms the selected item")
            .payload(),
        MovePayload::Item(source) if source.item() == INACTIVE_MAIN_ITEM
    ));
    assert_eq!(
        transition
            .events()
            .iter()
            .filter(|event| matches!(
                event.kind(),
                dockspace::event::WorkspaceEventKind::CommandCommitted(
                    dockspace::command::CommandOutcome::Selected {
                        item: INACTIVE_MAIN_ITEM,
                        changed: true,
                        ..
                    }
                )
            ))
            .count(),
        1
    );
}

#[test]
fn frontmost_item_arms_without_advancing_workspace() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let before = engine.version();
    let (scene, source, press) = tab_press(&engine, FRONT_ROOT, FRONT_ITEM);
    let transition = activate(&mut engine, &mut host, scene, source, press);

    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert_eq!(engine.version(), before);
    assert!(transition.events().is_empty());
    assert_eq!(
        engine
            .workspace()
            .surface(SURFACE)
            .expect("surface exists")
            .contained,
        vec![REAR_FLOATING, FRONT_FLOATING]
    );
}

#[test]
fn rear_contained_group_atomically_raises_and_arms_complete_tabs_payload() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let (scene, source, press) =
        group_press(&engine, REAR_ROOT, TabGroupDragRegionKind::LeadingGrip);
    let transition = activate(&mut engine, &mut host, scene, source, press);

    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert_eq!(
        engine
            .workspace()
            .surface(SURFACE)
            .expect("surface exists")
            .contained,
        vec![FRONT_FLOATING, REAR_FLOATING]
    );
    let active = engine
        .interaction()
        .active_drag_view()
        .expect("group gesture arms a drag");
    assert!(matches!(
        active.payload(),
        MovePayload::Tabs(source) if source.root() == REAR_ROOT
    ));
}

#[test]
fn frontmost_group_arms_without_workspace_mutation() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let before = engine.version();
    let (scene, source, press) =
        group_press(&engine, FRONT_ROOT, TabGroupDragRegionKind::LeadingGrip);
    let transition = activate(&mut engine, &mut host, scene, source, press);
    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert_eq!(engine.version(), before);
    assert!(transition.events().is_empty());
}

#[test]
fn trailing_empty_region_arms_the_same_complete_tabs_payload() {
    let (mut engine, mut host) = engine(
        rect(40.0, 40.0, 180.0, 140.0),
        rect(330.0, 220.0, 180.0, 140.0),
    );
    let (scene, source, press) =
        group_press(&engine, MAIN_ROOT, TabGroupDragRegionKind::TrailingEmpty);

    let transition = activate(&mut engine, &mut host, scene, source, press);

    assert!(matches!(
        interaction_outcome(&transition),
        InteractionOutcome::DragArmed { .. }
    ));
    assert!(matches!(
        engine
            .interaction()
            .active_drag_view()
            .expect("trailing empty gesture arms a drag")
            .payload(),
        MovePayload::Tabs(source) if source.root() == MAIN_ROOT
    ));
}
