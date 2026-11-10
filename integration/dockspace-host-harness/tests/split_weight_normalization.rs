//! Wide split construction and content removal through the public docking API.

use dockspace::close::CloseDecision;
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome,
    SurfaceUnavailableReason,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn equal_split_layout(child_count: u64) -> DockspaceLayout {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        (1..=child_count).map(|id| DockspaceNode::tabs([ItemId::new(id)])),
    )
    .unwrap();
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .expect("equal positive shares must produce a valid public layout")
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn equal_split_with_712_children_builds_a_valid_layout() {
    let layout = equal_split_layout(712);
    let split = layout
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap();
    assert_eq!(split.child_count(), 712);
    assert_eq!(
        split
            .children()
            .map(|child| child.tabs().unwrap().items()[0])
            .collect::<Vec<_>>(),
        (1..=712).map(ItemId::new).collect::<Vec<_>>()
    );
}

#[test]
fn closing_last_item_of_713_way_split_keeps_remaining_layout_valid() {
    let mut session =
        DockspaceSession::from_layout(equal_split_layout(713), DockPolicy::default()).unwrap();
    let last = ItemId::new(713);
    let before = session.version();
    let prepared = session.prepare_close_item(last);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(prepared).unwrap();
    let report = commit_deferred(frame);
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the last item must publish its close plan");
    };
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before);
    assert_eq!(plan.items().len(), 1);

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let report = commit_deferred(frame);
    assert!(report.workspace_changed());
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item, root })),
            changed: true,
            ..
        }] if *item == last && *root == ROOT
    ));
    assert_ne!(session.version(), before);
    assert!(session.version().is_same_epoch(before));
    assert!(session.view().item(last).is_none());
    let split = session
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap();
    assert_eq!(split.child_count(), 712);
    assert_eq!(
        split
            .children()
            .map(|child| child.tabs().unwrap().items()[0])
            .collect::<Vec<_>>(),
        (1..=712).map(ItemId::new).collect::<Vec<_>>()
    );
}
