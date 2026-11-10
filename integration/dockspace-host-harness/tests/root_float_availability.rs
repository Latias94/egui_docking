//! Root-float availability agrees with the public paint and submission contracts.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceActionRejection, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePresentationCommandKind, DockspacePresentationCommandUnavailable,
    DockspaceSession, HostFrameReport, HostInputOutcome, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const A_ROOT: RootId = RootId::new(1);
const B_ROOT: RootId = RootId::new(2);
const GROUP_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const A_ITEM: ItemId = ItemId::new(1);
const B_ITEM: ItemId = ItemId::new(2);
const GROUP_FIRST: ItemId = ItemId::new(3);
const GROUP_SECOND: ItemId = ItemId::new(4);

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn fixture() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(A_ROOT, DockspaceNode::central_tabs([A_ITEM])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(
                GROUP_ROOT,
                DockspaceNode::tabs_with_selection([GROUP_FIRST, GROUP_SECOND], Some(GROUP_SECOND))
                    .unwrap(),
            ),
            LogicalRect::new(40.0, 50.0, 360.0, 260.0).unwrap(),
        )),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(B_ROOT, DockspaceNode::central_tabs([B_ITEM])),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    for surface in [A, B] {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    finish(frame);
    session
}

#[test]
fn same_surface_contained_root_float_query_agrees_with_paint_unavailability() {
    let mut session = fixture();
    let version = session.version();
    let topology = format!("{:?}", session.view());
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(A).unwrap().unwrap();
    let paint_command = plan
        .presentation_command(GROUP_ROOT, DockspacePresentationCommandKind::Float)
        .unwrap()
        .unwrap();
    assert!(!paint_command.is_ready());
    assert_eq!(
        paint_command.unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::AlreadyContained)
    );
    assert!(plan.prepare_presentation_command(paint_command).is_none());
    assert!(!finish(frame).workspace_changed());

    let command = session
        .presentation_commands(GROUP_ROOT)
        .float_to(A)
        .unwrap();
    assert!(
        !command.is_ready(),
        "the session must not enable a float that its own paint contract rejects"
    );
    assert!(command.into_prepared().is_err());
    assert_eq!(session.version(), version);
    assert_eq!(format!("{:?}", session.view()), topology);
}

#[test]
fn direct_same_surface_contained_root_float_is_a_local_rejection_without_changes() {
    let mut session = fixture();
    let version = session.version();
    let topology = format!("{:?}", session.view());
    let prepared = session.prepare_float_root(GROUP_ROOT, A);
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.submit_prepared_action(prepared).unwrap();
    let report = finish(frame);
    assert!(matches!(
        report.submitted_action_outcome(submitted),
        Some(HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::Conflict
        ))
    ));
    assert!(!report.workspace_changed());
    assert_eq!((report.before(), report.after()), (version, version));
    assert_eq!(session.version(), version);
    assert_eq!(format!("{:?}", session.view()), topology);
}

#[test]
fn cross_surface_contained_root_float_query_is_ready_and_executes() {
    let mut session = fixture();
    let version = session.version();
    let command = session
        .presentation_commands(GROUP_ROOT)
        .float_to(B)
        .unwrap();
    assert!(command.is_ready(), "{:?}", command.unavailable_reason());
    let prepared = command.into_prepared().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame.submit_prepared_action(prepared).unwrap();
    let report = finish(frame);
    assert!(matches!(
        report.submitted_action_outcome(submitted),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootFloated {
                root: GROUP_ROOT,
                surface: B,
                floating: FLOATING,
                items,
                changed: true,
            }
        )) if items == &[GROUP_FIRST, GROUP_SECOND]
    ));
    assert!(report.workspace_changed());
    assert!(session.version().is_same_epoch(version));
    assert_ne!(session.version(), version);
    assert_eq!(session.view().surface(A).unwrap().contained_count(), 0);
    assert_eq!(session.view().surface(B).unwrap().contained_count(), 1);
    let contained = session
        .view()
        .surface(B)
        .unwrap()
        .contained()
        .next()
        .unwrap();
    assert_eq!(contained.id(), FLOATING);
    assert_eq!(contained.root().unwrap().id(), GROUP_ROOT);
    let tabs = contained.root().unwrap().content().unwrap().tabs().unwrap();
    assert_eq!(tabs.items(), &[GROUP_FIRST, GROUP_SECOND]);
    assert_eq!(tabs.selected(), Some(GROUP_SECOND));
    for item in [GROUP_FIRST, GROUP_SECOND] {
        let location = session.view().item(item).unwrap();
        assert_eq!(location.surface(), B);
        assert_eq!(location.root(), GROUP_ROOT);
        assert_eq!(location.contained(), Some(FLOATING));
    }
    assert_eq!(session.view().item(A_ITEM).unwrap().surface(), A);
    assert_eq!(session.view().item(B_ITEM).unwrap().surface(), B);
}
