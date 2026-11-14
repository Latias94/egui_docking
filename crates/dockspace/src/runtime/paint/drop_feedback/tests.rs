use super::*;

use crate::geometry::{LogicalPoint, LogicalSize};
use crate::graph::command::MovePayload;
use crate::graph::transaction::WorkspaceTransaction;
use crate::graph::workspace::WorkspaceIndex;
use crate::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{FloatingPresentationId, NodeId};
use crate::interaction::targeting::resolver::{DropResolution, resolve_presented_drop};
use crate::interaction::{DragGeneration, DragSessionId};
use crate::policy::{
    DockPolicy, DockTargetRule, DockTargetRuleKey, TabBarInteraction, TabBarPolicy,
    TabBarVisibility,
};
use crate::runtime::{
    DockspaceHostFrame, DockspaceSession, HostPointerInput, HostPointerRoute, PointerDeviceKind,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);

fn measured_session(workspace: Workspace, policy: DockPolicy) -> DockspaceSession {
    let mut session = DockspaceSession::from_workspace_for_test(workspace, policy).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 1200.0, 600.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    session
}

fn pane(plan: &PresentationPlan, tabs: NodeId) -> &crate::presentation::scene::PaneRecord {
    plan.pane_records()
        .iter()
        .find(|pane| pane.id().tabs == tabs)
        .unwrap()
}

#[test]
fn resolved_center_keeps_the_presented_target_when_its_future_rect_covers_a_neighbor() {
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([A]));
    let neighbor = builder.insert_node(Node::tabs([B]));
    let target = builder.insert_node(Node::tabs([C]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [source, neighbor, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let session = measured_session(workspace.clone(), DockPolicy::default());
    let candidate = session.engine.scene().ready_candidate(SURFACE).unwrap();
    let plan = candidate.plan();
    let target_id = DropTargetId::Center {
        surface: SURFACE,
        root: ROOT,
        tabs: target,
    };
    let hit = plan.drop_target(target_id).unwrap().region().rect();
    let point =
        LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5).unwrap();
    let version = session.engine.version();
    let payload = MovePayload::Item(workspace.capture_item_source(ROOT, source, A).unwrap());
    let policy = session
        .engine
        .policy()
        .snapshot(candidate.stamp().requirement().policy());
    let query = resolve_presented_drop(
        candidate.stamp(),
        plan,
        plan.layout_facts(),
        &workspace,
        version,
        &WorkspaceIndex::build(&workspace, version).unwrap(),
        &policy,
        DragSessionId::new(version.epoch(), DragGeneration::new(1)),
        payload.clone(),
        None,
        point,
    )
    .unwrap();
    let DropResolution::Resolved(drop) = query.resolution() else {
        panic!("the source must resolve to the exact C center: {query:?}");
    };
    assert_eq!(drop.target_id(), target_id);
    let feedback = resolve(plan, target_id, Some(&payload)).unwrap();
    assert_eq!(feedback.selected_item(), Some(C));
    assert_eq!(feedback.bounds(), pane(plan, target).bounds());
    assert_eq!(feedback.action(), DockspaceDropAction::Merge);
    assert_eq!(feedback.scope(), DockspaceDropTargetScope::Pane);
    assert_eq!(feedback.root(), Some(ROOT));
    assert_eq!(feedback.surface(), SURFACE);
    assert_eq!(
        feedback.visual_id(),
        DockspaceVisualId(VisualIdentity::DropTarget(target_id)),
    );
    let future = drop.visual().rect();
    assert!(future.x() < pane(plan, target).bounds().x());
    assert!(future.x() < pane(plan, neighbor).bounds().max().x());
    let mut committed = workspace;
    WorkspaceTransaction::from_commands([drop.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let Node::Tabs {
        items, selected, ..
    } = committed.node(target).unwrap()
    else {
        panic!("the committed destination remains C's tabs leaf");
    };
    assert_eq!(items.as_slice(), &[C, A]);
    assert_eq!(*selected, Some(A));
    assert_eq!(feedback.selected_item(), Some(C));
}

#[test]
fn tab_gap_neighbors_exclude_the_payload_and_preserve_the_pre_removal_index() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([A, B, C]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let session = measured_session(workspace.clone(), DockPolicy::default());
    let plan = session
        .engine
        .scene()
        .ready_candidate(SURFACE)
        .unwrap()
        .plan();
    for (source, index, previous, next) in [
        (A, 2, Some(B), Some(C)),
        (B, 1, Some(A), Some(C)),
        (B, 2, Some(A), Some(C)),
        (A, 0, None, Some(B)),
        (C, 3, Some(B), None),
    ] {
        let payload = MovePayload::Item(workspace.capture_item_source(ROOT, tabs, source).unwrap());
        let feedback = resolve(
            plan,
            DropTargetId::TabGap {
                surface: SURFACE,
                root: ROOT,
                tabs,
                index,
            },
            Some(&payload),
        )
        .unwrap();
        assert_eq!(
            feedback.action(),
            DockspaceDropAction::InsertTab {
                index,
                previous,
                next
            },
            "source={source:?}, pre-removal index={index}",
        );
        assert_eq!(feedback.selected_item(), Some(A));
        assert_eq!(feedback.bounds(), pane(plan, tabs).bounds());
    }
}

#[test]
fn hidden_bars_keep_their_selected_target_and_empty_central_has_no_representative() {
    let mut builder = Workspace::builder();
    let hidden = builder.insert_node(Node::tabs([C]));
    let central = builder.insert_node(Node::tabs([]));
    let split =
        builder.insert_node(Node::equal_split(Axis::Horizontal, [hidden, central]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(central));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut policy = DockPolicy::default();
    let mut rule = DockTargetRule::new();
    rule.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    policy.set_target_rule(DockTargetRuleKey::Item(C), rule);
    let session = measured_session(builder.build().unwrap(), policy);
    let plan = session
        .engine
        .scene()
        .ready_candidate(SURFACE)
        .unwrap()
        .plan();
    assert!(plan.tab_records().iter().all(|tab| tab.id().item != C));
    for (tabs, selected) in [(hidden, Some(C)), (central, None)] {
        let feedback = resolve(
            plan,
            DropTargetId::Center {
                surface: SURFACE,
                root: ROOT,
                tabs,
            },
            None,
        )
        .unwrap();
        assert_eq!(feedback.selected_item(), selected);
        assert_eq!(feedback.bounds(), pane(plan, tabs).bounds());
        assert_eq!(feedback.scope(), DockspaceDropTargetScope::Pane);
    }
    let outer = resolve(
        plan,
        DropTargetId::OuterEdge {
            surface: SURFACE,
            root: ROOT,
            node: split,
            edge: crate::graph::command::Edge::Right,
        },
        None,
    )
    .unwrap();
    assert_eq!(outer.scope(), DockspaceDropTargetScope::Root);
    assert_eq!(outer.root(), Some(ROOT));
    assert_eq!(outer.selected_item(), None);
    assert_eq!(outer.bounds(), plan.bounds());
    assert_eq!(
        outer.action(),
        DockspaceDropAction::Split {
            edge: DockEdge::Right
        }
    );
}

#[test]
fn rootless_background_feedback_installs_a_root_without_inventing_an_item() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([A]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::rootless());
    let floating = FloatingPresentationId::new(1);
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(ROOT, LogicalRect::new(20.0, 20.0, 200.0, 180.0).unwrap()),
    );
    builder.attach_contained(SURFACE, floating).unwrap();
    let session = measured_session(builder.build().unwrap(), DockPolicy::default());
    let plan = session
        .engine
        .scene()
        .ready_candidate(SURFACE)
        .unwrap()
        .plan();
    let feedback = resolve(
        plan,
        DropTargetId::SurfaceBackground { surface: SURFACE },
        None,
    )
    .unwrap();
    assert_eq!(feedback.scope(), DockspaceDropTargetScope::Surface);
    assert_eq!(feedback.root(), None);
    assert_eq!(feedback.selected_item(), None);
    assert_eq!(feedback.bounds(), plan.bounds());
    assert_eq!(feedback.action(), DockspaceDropAction::InstallRoot);
}

fn center(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .unwrap()
}

fn commit(mut frame: DockspaceHostFrame<'_>) {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

fn publish(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    let receipt = frame
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    for output in report.take_painted_outputs() {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    commit(session.begin_host_frame().unwrap());
}

fn input(session: &mut DockspaceSession, event: SurfacePointerEvent, point: LogicalPoint) {
    let mut frame = session.begin_host_frame().unwrap();
    let hit = frame
        .host_pointer_hit_for_device(SURFACE, point, PointerDeviceKind::Unknown)
        .unwrap();
    let surface = hit.surface();
    let delivery = hit.delivery();
    let drag = hit.drag_delivery();
    let mut facts = SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface);
    if let Some(delivery) = delivery.as_ref() {
        facts = facts.with_delivery(delivery);
    }
    if let Some(drag) = drag.as_ref() {
        facts = facts.with_drag_delivery(drag);
    }
    let route = HostPointerRoute::new(surface, point);
    frame
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            event,
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            facts,
        ))
        .unwrap();
    commit(frame);
}

#[test]
fn paint_feedback_reads_presented_bounds_and_shapes_hidden_target_titles() {
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([A]));
    let neighbor = builder.insert_node(Node::tabs([B]));
    let target = builder.insert_node(Node::tabs([C]));
    let split = builder
        .insert_node(Node::equal_split(Axis::Horizontal, [source, neighbor, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut policy = DockPolicy::default();
    let mut rule = DockTargetRule::new();
    rule.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Hidden,
        TabBarInteraction::Enabled,
    ));
    policy.set_target_rule(DockTargetRuleKey::Item(C), rule);
    let mut session = measured_session(builder.build().unwrap(), policy);
    session.enable_host_pointer().unwrap();
    publish(&mut session);
    let presented = session
        .engine
        .scene()
        .interaction_projection(SURFACE)
        .unwrap();
    let presented_bounds = pane(presented.plan(), target).bounds();
    let source_point = center(
        presented
            .plan()
            .tab_records()
            .iter()
            .find(|tab| tab.id().item == A)
            .unwrap()
            .drag_hit()
            .rect(),
    );
    let target_id = DropTargetId::Center {
        surface: SURFACE,
        root: ROOT,
        tabs: target,
    };
    let target_point = center(
        presented
            .plan()
            .drop_target(target_id)
            .unwrap()
            .region()
            .rect(),
    );
    input(
        &mut session,
        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
        source_point,
    );
    input(
        &mut session,
        SurfacePointerEvent::Moved,
        LogicalPoint::new(source_point.x() + 20.0, source_point.y() + 12.0).unwrap(),
    );
    input(&mut session, SurfacePointerEvent::Moved, target_point);
    assert!(session.has_active_gesture());

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 1800.0, 600.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    assert_ne!(pane(plan.plan, target).bounds(), presented_bounds);
    let feedback = plan.drag_preview().unwrap().target().unwrap();
    assert_eq!(feedback.selected_item(), Some(C));
    assert_eq!(
        feedback.bounds(),
        presented_bounds,
        "an unpresented resize cannot replace the actual destination geometry"
    );
    assert!(plan.plan.tab_records().iter().all(|tab| tab.id().item != C));
    let mut resources = Vec::new();
    plan.visit_tab_resources(|visual, item| resources.push((visual, item)))
        .unwrap();
    assert!(
        resources.contains(&(feedback.visual_id(), C)),
        "the hidden target's label still needs a text resource"
    );
}

#[test]
fn future_group_tabs_visit_resources_for_payload_members_newly_visible_at_the_destination() {
    let payload = (1..17).map(ItemId::new).collect::<Vec<_>>();
    let target_item = ItemId::new(100);
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs(payload.iter().copied()));
    let target = builder.insert_node(Node::tabs([target_item]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [source, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(target));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut session = measured_session(builder.build().unwrap(), DockPolicy::default());
    session.enable_host_pointer().unwrap();
    publish(&mut session);
    let presented = session
        .engine
        .scene()
        .interaction_projection(SURFACE)
        .unwrap();
    let source_point = center(
        presented
            .plan()
            .tab_bar_records()
            .iter()
            .find(|bar| bar.id().tabs == source)
            .unwrap()
            .group_grip_bounds()
            .unwrap(),
    );
    let source_visible = presented
        .plan()
        .tab_records()
        .iter()
        .filter(|tab| tab.id().tabs == source)
        .map(|tab| tab.id().item)
        .collect::<std::collections::BTreeSet<_>>();
    assert!(
        source_visible.len() < payload.len(),
        "the source strip must clip the group"
    );
    let target_id = DropTargetId::Center {
        surface: SURFACE,
        root: ROOT,
        tabs: target,
    };
    let target_point = center(
        presented
            .plan()
            .drop_target(target_id)
            .unwrap()
            .region()
            .rect(),
    );
    input(
        &mut session,
        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
        source_point,
    );
    input(
        &mut session,
        SurfacePointerEvent::Moved,
        LogicalPoint::new(source_point.x() + 20.0, source_point.y() + 12.0).unwrap(),
    );
    input(&mut session, SurfacePointerEvent::Moved, target_point);
    assert!(session.has_active_gesture());

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let preview = plan.drag_preview().unwrap();
    let feedback = preview.target().unwrap();
    assert_eq!(feedback.selected_item(), Some(target_item));
    assert_eq!(
        preview.tabs().filter(|tab| tab.is_payload()).count(),
        payload.len()
    );
    let paintable = preview
        .tabs()
        .filter(|tab| tab.visible_bounds().is_some())
        .collect::<Vec<_>>();
    let newly_visible = paintable
        .iter()
        .filter(|tab| tab.is_payload() && !source_visible.contains(&tab.item()))
        .collect::<Vec<_>>();
    assert!(
        !newly_visible.is_empty(),
        "the widened future target must expose previously clipped payload titles"
    );
    let mut resources = Vec::new();
    plan.visit_tab_resources(|visual, item| resources.push((visual, item)))
        .unwrap();
    for tab in paintable {
        assert!(
            resources.contains(&(feedback.visual_id(), tab.item())),
            "every future paintable tab needs a resource visit: {:?}",
            tab.item()
        );
    }
    for item in payload {
        assert!(
            resources.contains(&(feedback.visual_id(), item)),
            "the complete future payload must remain enumerable: {item:?}"
        );
    }
}
