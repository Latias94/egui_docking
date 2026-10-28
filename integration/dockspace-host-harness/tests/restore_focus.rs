//! Replacement focus through public document, paint, and observation contracts.

#![cfg(feature = "serde")]

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceFocusTarget, DockspaceHostFrame,
    DockspacePaneFocusObservation, DockspacePaneFocusRequest, DockspaceSession, HostFrameReport,
    HostInputOutcome, SurfacePaintDisposition, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x52; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

fn surface(id: SurfaceId, root: u64, content: DockspaceNode) -> DockspaceSurfaceLayout {
    DockspaceSurfaceLayout::new(id, DockspaceRootLayout::new(RootId::new(root), content))
}

fn layout(surfaces: impl IntoIterator<Item = DockspaceSurfaceLayout>) -> DockspaceLayout {
    DockspaceLayout::new(surfaces).unwrap()
}

fn persistent(layout: DockspaceLayout) -> DockspaceSession {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    for (key, item) in [("first", FIRST), ("second", SECOND), ("third", THIRD)] {
        assert_eq!(bootstrap.ensure_item(key).unwrap(), item);
    }
    let mut session =
        DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap();
    commit_measured(session.begin_host_frame().unwrap());
    session
}

fn saved(layout: DockspaceLayout) -> Vec<u8> {
    persistent(layout).save_document_json().unwrap()
}

fn recognize(document: DockspaceDocumentId, key: &str) -> bool {
    document == DOCUMENT && matches!(key, "first" | "second" | "third")
}

fn commit_measured(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
        LogicalSize::new(48.0, 32.0).unwrap(),
        96.0,
    )
    .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    frame.commit().unwrap()
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

fn requests(session: &mut DockspaceSession) -> Vec<DockspacePaneFocusRequest> {
    let mut frame = session.begin_host_frame().unwrap();
    let requests = frame
        .surfaces()
        .into_iter()
        .filter_map(|surface| {
            frame
                .paint_plan(surface)
                .unwrap()
                .unwrap()
                .pane_focus_request()
        })
        .collect();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    requests
}

fn observe(
    session: &mut DockspaceSession,
    request: DockspacePaneFocusRequest,
    state: DockspacePaneFocusObservation,
) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(request.surface()).unwrap().unwrap();
    assert_eq!(plan.pane_focus_request(), Some(request));
    let observed = plan.prepare_pane_focus_observation(request, state).unwrap();
    frame.submit_pane_focus_observation(observed).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

fn select(session: &mut DockspaceSession, item: ItemId) -> DockspacePaneFocusRequest {
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(item).unwrap();
    commit_measured(frame);
    let recordings = paint_and_present(session);
    assert!(
        recordings
            .iter()
            .any(|recording| recording.records.iter().any(|record| {
                matches!(record, OwnedRecord::PaneContent {
            item: Some(painted), paint: SurfacePaintDisposition::Visible, ..
        } if *painted == item)
            }))
    );
    let requests = requests(session);
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].item(), Some(item));
    requests[0]
}

fn restore(session: &mut DockspaceSession, bytes: &[u8]) {
    let prepared = session
        .prepare_document_restore_json(bytes, recognize)
        .unwrap();
    let report = commit_measured(session.begin_document_restore_frame(&prepared).unwrap());
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::DocumentRestored { .. }]
    ));
    paint_and_present(session);
}

#[test]
fn restore_without_focus_history_requests_the_primary_selection() {
    let bytes = saved(layout([surface(
        SURFACE,
        1,
        DockspaceNode::central_tabs([SECOND]),
    )]));
    let mut session = persistent(layout([surface(
        SURFACE,
        10,
        DockspaceNode::central_tabs([FIRST]),
    )]));
    assert!(requests(&mut session).is_empty());
    restore(&mut session, &bytes);
    let request = requests(&mut session);
    assert_eq!(request.len(), 1);
    assert_eq!(request[0].target(), DockspaceFocusTarget::Pane(SECOND));
    observe(
        &mut session,
        request[0],
        DockspacePaneFocusObservation::Focused,
    );
    assert!(requests(&mut session).is_empty());
}

#[test]
fn restore_relocates_stable_focus_and_prefers_pending_over_other_surface_history() {
    let bytes = saved(layout([
        surface(SURFACE, 1, DockspaceNode::central_tabs([SECOND])),
        surface(
            OTHER_SURFACE,
            2,
            DockspaceNode::central_tabs([FIRST, THIRD]),
        ),
    ]));
    for acknowledge_second in [false, true] {
        let mut session = persistent(layout([
            surface(SURFACE, 10, DockspaceNode::central_tabs([FIRST])),
            surface(
                OTHER_SURFACE,
                20,
                DockspaceNode::central_tabs([SECOND, THIRD]),
            ),
        ]));
        let first = select(&mut session, FIRST);
        observe(&mut session, first, DockspacePaneFocusObservation::Focused);
        let second = select(&mut session, SECOND);
        if acknowledge_second {
            observe(&mut session, second, DockspacePaneFocusObservation::Focused);
        }

        restore(&mut session, &bytes);

        let restored = session.view().item(SECOND).unwrap();
        assert_eq!(restored.surface(), SURFACE);
        assert_ne!(
            restored.root(),
            RootId::new(1),
            "retired roots must be remapped"
        );
        assert!(restored.is_selected());
        let request = requests(&mut session);
        assert_eq!(request.len(), 1);
        assert_eq!(request[0].surface(), SURFACE);
        assert_eq!(request[0].target(), DockspaceFocusTarget::Pane(SECOND));
        assert_ne!(request[0], second);
        observe(
            &mut session,
            request[0],
            DockspacePaneFocusObservation::Focused,
        );
        assert!(requests(&mut session).is_empty());
    }
}

#[test]
fn restore_fallback_is_central_then_visual_then_front_contained_then_workspace() {
    let split = |central| {
        DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::tabs([SECOND]),
                if central {
                    DockspaceNode::central_tabs([FIRST, THIRD])
                } else {
                    DockspaceNode::tabs([FIRST, THIRD])
                },
            ],
        )
        .unwrap()
    };
    let floating = |id, item| {
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(id),
            DockspaceRootLayout::new(RootId::new(id), DockspaceNode::tabs([item])),
            LogicalRect::new(20.0 * id as f64, 30.0, 300.0, 260.0).unwrap(),
        )
    };
    let cases = [
        (
            layout([surface(SURFACE, 1, split(true))]),
            Some((SURFACE, DockspaceFocusTarget::Pane(FIRST))),
        ),
        (
            layout([surface(SURFACE, 1, split(false))]),
            Some((SURFACE, DockspaceFocusTarget::Pane(SECOND))),
        ),
        (
            layout([DockspaceSurfaceLayout::rootless(SURFACE)
                .with_contained(floating(1, FIRST))
                .with_contained(floating(2, SECOND))]),
            Some((SURFACE, DockspaceFocusTarget::Pane(SECOND))),
        ),
        (
            layout([surface(SURFACE, 1, DockspaceNode::central_tabs([]))]),
            Some((SURFACE, DockspaceFocusTarget::Workspace)),
        ),
        (DockspaceLayout::empty(), None),
        (
            layout([
                surface(SURFACE, 1, DockspaceNode::central_tabs([FIRST])),
                surface(OTHER_SURFACE, 2, DockspaceNode::central_tabs([SECOND])),
            ]),
            Some((OTHER_SURFACE, DockspaceFocusTarget::Pane(SECOND))),
        ),
    ];
    for (restored, expected) in cases {
        let bytes = saved(restored);
        let mut session = persistent(layout([surface(
            OTHER_SURFACE,
            20,
            DockspaceNode::central_tabs([THIRD]),
        )]));
        let old = select(&mut session, THIRD);
        observe(&mut session, old, DockspacePaneFocusObservation::Focused);

        restore(&mut session, &bytes);

        let actual = requests(&mut session);
        assert_eq!(actual.len(), usize::from(expected.is_some()));
        if let Some((surface, target)) = expected {
            let surface = match target {
                DockspaceFocusTarget::Pane(item) => session.view().item(item).unwrap().surface(),
                DockspaceFocusTarget::Workspace => session
                    .view()
                    .surface(surface)
                    .or_else(|| session.view().surfaces().next())
                    .unwrap()
                    .id(),
            };
            assert_eq!((actual[0].surface(), actual[0].target()), (surface, target));
            observe(
                &mut session,
                actual[0],
                DockspacePaneFocusObservation::Focused,
            );
        }
        if let Some(hidden) = session.view().item(THIRD) {
            assert!(
                !hidden.is_selected(),
                "restore must preserve the saved background tab"
            );
        }
        assert!(requests(&mut session).is_empty());
    }
}

#[test]
fn dropped_restore_preserves_pending_focus_and_retry_rejects_old_observation() {
    let bytes = saved(layout([surface(
        SURFACE,
        1,
        DockspaceNode::central_tabs([SECOND]),
    )]));
    let mut session = persistent(layout([surface(
        SURFACE,
        10,
        DockspaceNode::central_tabs([FIRST]),
    )]));
    let old = select(&mut session, FIRST);
    let old_observation = {
        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        plan.prepare_pane_focus_observation(old, DockspacePaneFocusObservation::Focused)
            .unwrap()
    };
    let prepared = session
        .prepare_document_restore_json(&bytes, recognize)
        .unwrap();
    let before = session.version();
    {
        let mut candidate = session.begin_document_restore_frame(&prepared).unwrap();
        assert!(candidate.view().item(SECOND).is_some());
        candidate
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
                    LogicalSize::new(48.0, 32.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
    assert_eq!(session.version(), before);
    assert!(session.view().item(FIRST).is_some());
    assert_eq!(requests(&mut session), [old]);

    commit_measured(session.begin_document_restore_frame(&prepared).unwrap());
    paint_and_present(&mut session);
    let fresh = requests(&mut session)[0];
    assert_eq!(fresh.target(), DockspaceFocusTarget::Pane(SECOND));
    assert_ne!(fresh, old);
    let mut frame = session.begin_host_frame().unwrap();
    assert!(
        frame
            .paint_plan(SURFACE)
            .unwrap()
            .unwrap()
            .prepare_pane_focus_observation(old, DockspacePaneFocusObservation::Focused)
            .is_none()
    );
    frame
        .submit_pane_focus_observation(old_observation)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert_eq!(requests(&mut session), [fresh]);
    observe(
        &mut session,
        fresh,
        DockspacePaneFocusObservation::NotFocused,
    );
    assert_eq!(requests(&mut session), [fresh]);
    observe(&mut session, fresh, DockspacePaneFocusObservation::Focused);
    assert!(requests(&mut session).is_empty());
}
