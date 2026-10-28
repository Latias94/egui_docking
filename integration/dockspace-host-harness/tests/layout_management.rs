//! Application-owned closed inventory using the public open command and focus observations.

use std::collections::BTreeMap;

use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceFocusTarget, DockspaceHostFrame, DockspacePaneFocusObservation,
    DockspacePaneFocusRequest, DockspaceSession, HostFrameReport, HostInputOutcome,
    SurfacePaintDisposition, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const OTHER_ROOT: RootId = RootId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::central_tabs([THIRD])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    commit_measured(session.begin_host_frame().unwrap());
    session
}

fn commit_measured(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    frame.commit().unwrap()
}

fn close(session: &mut DockspaceSession, item: ItemId) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.request_close_item_current(item).unwrap();
    let report = commit_measured(frame);
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("the public close command must expose its application decision plan");
    };
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), item);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let report = commit_measured(frame);
    assert!(
        matches!(report.inputs(), [HostInputOutcome::CloseDecisionProcessed {
        application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: closed, .. })),
        changed: true, ..
    }] if *closed == item)
    );
    assert!(session.view().item(item).is_none());
}

fn paint_and_present(session: &mut DockspaceSession) -> Vec<RecordingRenderer> {
    let mut frame = session.begin_host_frame().unwrap();
    let mut recordings = Vec::new();
    for surface in frame.surfaces() {
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        recordings.push(recording);
    }
    let mut report = frame.commit().unwrap();
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), recordings.len());
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert_eq!(
        frame.commit().unwrap().surface_presentation_commits().len(),
        recordings.len()
    );
    recordings
}

fn focus_request(
    session: &mut DockspaceSession,
    surface: SurfaceId,
) -> Option<DockspacePaneFocusRequest> {
    let mut frame = session.begin_host_frame().unwrap();
    let request = frame
        .paint_plan(surface)
        .unwrap()
        .unwrap()
        .pane_focus_request();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    request
}

fn observe_focus(
    session: &mut DockspaceSession,
    request: DockspacePaneFocusRequest,
    observation: DockspacePaneFocusObservation,
) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(request.surface()).unwrap().unwrap();
    assert_eq!(plan.pane_focus_request(), Some(request));
    let observation = plan
        .prepare_pane_focus_observation(request, observation)
        .unwrap();
    frame.submit_pane_focus_observation(observation).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn application_closed_catalog_reopens_the_same_item_and_requests_its_pane_focus() {
    let mut session = session();
    // The application retains stable IDs and default placements independently of open topology.
    let catalog = BTreeMap::from([(SECOND, DockPlacement::After(FIRST))]);
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    commit_measured(frame);
    paint_and_present(&mut session);
    let previous = focus_request(&mut session, SURFACE).unwrap();
    observe_focus(
        &mut session,
        previous,
        DockspacePaneFocusObservation::Focused,
    );

    close(&mut session, SECOND);
    let closed = catalog
        .keys()
        .copied()
        .filter(|item| session.view().item(*item).is_none())
        .collect::<Vec<_>>();
    assert_eq!(closed, [SECOND]);
    let item = closed[0];
    let mut frame = session.begin_host_frame().unwrap();
    frame.open_item_current(item, catalog[&item]).unwrap();
    let report = commit_measured(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Opened {
                item: SECOND,
                root: ROOT
            },
        )]
    );
    let reopened = session.view().item(item).unwrap();
    assert_eq!(reopened.surface(), SURFACE);
    assert_eq!(reopened.root(), ROOT);
    assert_eq!(reopened.tabs().items(), &[FIRST, SECOND]);
    assert!(reopened.is_selected());

    let recordings = paint_and_present(&mut session);
    assert!(
        recordings
            .iter()
            .flat_map(|recording| &recording.records)
            .any(|record| matches!(
                record,
                OwnedRecord::PaneContent {
                    item: Some(SECOND),
                    paint: SurfacePaintDisposition::Visible,
                    ..
                }
            ))
    );
    let request =
        focus_request(&mut session, SURFACE).expect("a newly opened item must request pane focus");
    assert_eq!(request.target(), DockspaceFocusTarget::Pane(SECOND));
    assert_eq!(request.surface(), SURFACE);
    assert_ne!(request, previous);
    observe_focus(
        &mut session,
        request,
        DockspacePaneFocusObservation::NotFocused,
    );
    assert_eq!(focus_request(&mut session, SURFACE), Some(request));
    observe_focus(
        &mut session,
        request,
        DockspacePaneFocusObservation::Focused,
    );
    assert_eq!(focus_request(&mut session, SURFACE), None);
}

#[test]
fn missing_default_anchor_rejects_open_without_implicit_fallback_or_focus_change() {
    let mut session = session();
    let catalog = BTreeMap::from([(SECOND, DockPlacement::After(FIRST))]);
    close(&mut session, SECOND);
    close(&mut session, FIRST);
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(THIRD).unwrap();
    commit_measured(frame);
    paint_and_present(&mut session);
    let focus = focus_request(&mut session, OTHER_SURFACE).unwrap();
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.open_item_current(SECOND, catalog[&SECOND]).unwrap();
    let report = commit_measured(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::AnchorUnavailable {
                anchor: DockAnchor::Item(FIRST)
            },
        )]
    );
    assert_eq!(session.version(), version);
    assert!(!report.workspace_changed());
    assert!(session.view().item(SECOND).is_none());
    assert!(session.view().item(FIRST).is_none());
    assert_eq!(session.view().item(THIRD).unwrap().tabs().items(), &[THIRD]);
    assert_eq!(focus_request(&mut session, OTHER_SURFACE), Some(focus));
}

#[test]
fn opening_an_existing_item_preserves_location_selection_and_pending_or_settled_focus() {
    let mut session = session();
    close(&mut session, SECOND);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .open_item_current(SECOND, DockPlacement::After(FIRST))
        .unwrap();
    commit_measured(frame);
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    commit_measured(frame);
    paint_and_present(&mut session);
    let focus = focus_request(&mut session, SURFACE).unwrap();
    assert_eq!(focus.target(), DockspaceFocusTarget::Pane(FIRST));

    for (placement, expected_focus) in [
        (DockPlacement::After(THIRD), Some(focus)),
        (DockPlacement::After(ItemId::new(99)), None),
    ] {
        let version = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame.open_item_current(SECOND, placement).unwrap();
        let report = commit_measured(frame);
        assert_eq!(
            report.inputs(),
            &[HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::Existing { item: SECOND },
            )]
        );
        assert_eq!(session.version(), version);
        assert!(!report.workspace_changed());
        let existing = session.view().item(SECOND).unwrap();
        assert_eq!(existing.surface(), SURFACE);
        assert_eq!(existing.root(), ROOT);
        assert_eq!(existing.tabs().items(), &[FIRST, SECOND]);
        assert!(!existing.is_selected());
        assert!(session.view().item(FIRST).unwrap().is_selected());
        assert_eq!(session.view().item(THIRD).unwrap().tabs().items(), &[THIRD]);
        assert_eq!(focus_request(&mut session, SURFACE), expected_focus);
        assert_eq!(focus_request(&mut session, OTHER_SURFACE), None);
        if let Some(request) = expected_focus {
            observe_focus(
                &mut session,
                request,
                DockspacePaneFocusObservation::Focused,
            );
            assert_eq!(focus_request(&mut session, SURFACE), None);
        }
    }
}
