//! Application-owned close decisions through the renderer-neutral public API.

use dockspace::close::{CloseDecision, ClosePlanTarget, DeferredCloseDecision};
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceCloseResolution, DockspaceSession, HostInputOutcome,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    DockspaceSession::from_layout(layout, policy).unwrap()
}

fn measure(frame: &mut dockspace::runtime::DockspaceHostFrame<'_>) {
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
}

#[test]
fn root_close_waits_for_every_decision_and_deferred_continuation_commits_once() {
    let mut session = session();
    let before = session.version();
    let request = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(request).unwrap();
    measure(&mut frame);
    let report = frame.commit().unwrap();
    let [
        HostInputOutcome::CloseRequested {
            plan,
            reused: false,
            ..
        },
    ] = report.inputs()
    else {
        panic!("an independent host receives the complete application close plan")
    };
    assert_eq!(plan.target(), ClosePlanTarget::Root { root: ROOT });
    let request = plan.request();
    let items = plan.items().to_vec();
    assert_eq!(
        items.iter().map(|item| item.item()).collect::<Vec<_>>(),
        [FIRST, SECOND]
    );
    assert!(!report.workspace_changed());

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(request, items[0].token(), CloseDecision::Allow)
        .unwrap();
    frame
        .resolve_close(request, items[1].token(), CloseDecision::Deferred)
        .unwrap();
    measure(&mut frame);
    let report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).is_some() && session.view().item(SECOND).is_some());
    let continuation = report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::CloseDecisionProcessed {
                plan: Some(plan), ..
            } => plan
                .items()
                .iter()
                .find(|item| item.item() == SECOND)
                .and_then(|item| item.deferred_token()),
            _ => None,
        })
        .expect("the deferred decision publishes its exact continuation");

    let mut frame = session.begin_host_frame().unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.workspace_changed());
    assert!(
        matches!(report.inputs(), [HostInputOutcome::CloseDecisionProcessed {
        application: Some(Ok(DockspaceCloseOutcome::RootClosed { root: ROOT, items })), changed: true, ..
    }] if items == &[FIRST, SECOND])
    );
    assert!(session.view().item(FIRST).is_none() && session.view().item(SECOND).is_none());
    let committed = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(request, items[0].token(), CloseDecision::Allow)
        .unwrap();
    frame
        .continue_deferred_close(request, continuation, DeferredCloseDecision::Allow)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), committed);
    assert!(report.inputs().iter().all(|input| matches!(
        input,
        HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            application: None,
            changed: false,
            ..
        }
    )));
}

#[test]
fn veto_keeps_the_entire_group_and_later_local_selection_remains_usable() {
    let mut session = session();
    let request = session.prepare_close_root(ROOT);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_close_request(request).unwrap();
    measure(&mut frame);
    let report = frame.commit().unwrap();
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("close plan")
    };
    let request = plan.request();
    let items = plan.items().to_vec();
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(request, items[0].token(), CloseDecision::Allow)
        .unwrap();
    frame
        .resolve_close(request, items[1].token(), CloseDecision::Veto)
        .unwrap();
    measure(&mut frame);
    assert!(!frame.commit().unwrap().workspace_changed());
    assert_eq!(session.version(), before);
    assert_eq!(
        session.view().item(FIRST).unwrap().tabs().items(),
        &[FIRST, SECOND]
    );
    let select = session.prepare_select_item(SECOND);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_action(select).unwrap();
    measure(&mut frame);
    assert!(frame.commit().unwrap().workspace_changed());
    assert!(session.view().item(SECOND).unwrap().is_selected());
}
