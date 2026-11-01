//! Magnetic targeting intent through ordered public host input and exact outputs.

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationFeedback as Feedback, DockspaceManipulationPendingReason as PendingReason,
    DockspaceManipulationStatus as Status, DockspacePreviewVisual, DockspaceSession,
    HostFrameReport, HostInputOutcome, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    PointerDeviceKind, PointerEventMetadata, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);
const FOREIGN_POINTER: SurfacePointerId = SurfacePointerId::new(2);

struct Geometry {
    press: LogicalPoint,
    magnetic: LogicalPoint,
    guide: LogicalPoint,
    gap: LogicalPoint,
}

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn metadata() -> PointerEventMetadata {
    PointerEventMetadata::untimed(PointerDeviceKind::Mouse)
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    commit(session.begin_host_frame().unwrap())
}

fn record(frame: &mut DockspaceHostFrame<'_>) -> [RecordingRenderer; 2] {
    [A, B].map(|surface| {
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        recording
    })
}

fn paint(session: &mut DockspaceSession) -> ([RecordingRenderer; 2], Vec<PaintedSurfaceOutput>) {
    let mut frame = session.begin_host_frame().unwrap();
    let recordings = record(&mut frame);
    let mut report = commit(frame);
    assert!(!report.workspace_changed());
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    for (surface, recording) in [A, B].into_iter().zip(&recordings) {
        let output = outputs
            .iter()
            .find(|output| output.surface() == surface)
            .unwrap();
        assert!(
            matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
            if output.matches_semantic_output(*semantic))
        );
    }
    (recordings, outputs)
}

fn inspect(session: &mut DockspaceSession) -> [RecordingRenderer; 2] {
    let mut frame = session.begin_host_frame().unwrap();
    [A, B].map(|surface| {
        RecordingRenderer::execute(frame.paint_plan(surface).unwrap().unwrap())
            .unwrap()
            .0
    })
}

fn previews(recordings: &[RecordingRenderer; 2]) -> Vec<DockspacePreviewVisual> {
    recordings
        .iter()
        .flat_map(|recording| &recording.records)
        .filter_map(|record| match record {
            OwnedRecord::Preview(preview) => Some(*preview),
            _ => None,
        })
        .collect()
}

fn assert_dock_preview(recordings: &[RecordingRenderer; 2]) {
    assert!(matches!(
        previews(recordings).as_slice(),
        [DockspacePreviewVisual::Dock { surface: B, .. }]
    ));
}

fn present(session: &mut DockspaceSession, outputs: Vec<PaintedSurfaceOutput>) -> HostFrameReport {
    let version = session.version();
    let artifacts = outputs
        .iter()
        .map(PaintedSurfaceOutput::presentation_artifact)
        .collect::<Vec<_>>();
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    assert_eq!(
        session.version(),
        version,
        "reporting cannot deliver before commit"
    );
    let report = settle(session);
    for artifact in artifacts {
        assert_eq!(
            report
                .surface_presentation_commits()
                .iter()
                .filter(|ack| ack.settles(artifact))
                .count(),
            1,
            "each exact output must settle once"
        );
    }
    report
}

fn fixture() -> (DockspaceSession, Geometry) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([THIRD, FOURTH])),
        ),
    ])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap();
    for surface in [A, B] {
        frame.measure_surface(surface, metrics).unwrap();
    }
    commit(frame);

    let mut frame = session.begin_host_frame().unwrap();
    let press = frame
        .paint_plan(A)
        .unwrap()
        .unwrap()
        .tab_receiver(FIRST)
        .unwrap()
        .center();
    let target = frame.paint_plan(B).unwrap().unwrap();
    let guide = target
        .center_drop_receiver_for_item(THIRD)
        .unwrap()
        .center();
    let third = target.tab_receiver(THIRD).unwrap();
    let fourth = target.tab_receiver(FOURTH).unwrap();
    let gap = point(
        (third.bounds().max().x() + fourth.bounds().x()) * 0.5,
        third.center().y(),
    );
    let mut guide_hits = Vec::new();
    target
        .visit_surface_program(|step| {
            if let Step::DropGuide { structural, .. } = step {
                guide_hits.extend(structural.targets().map(|target| target.hit_bounds()));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let recordings = record(&mut frame);
    let mut report = commit(frame);
    let content = recordings[1]
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::PaneContent {
                item: Some(THIRD),
                bounds,
                ..
            } => Some(*bounds),
            _ => None,
        })
        .unwrap();
    // This central basin point deliberately avoids every explicit guide button.
    let magnetic = point(
        content.x() + content.width() * 0.65,
        content.y() + content.height() * 0.65,
    );
    assert!(content.contains(magnetic));
    assert!(guide_hits.iter().all(|bounds| !bounds.contains(magnetic)));
    assert!(guide_hits.iter().any(|bounds| bounds.contains(guide)));
    assert!(!present(&mut session, report.take_painted_outputs()).workspace_changed());
    assert_undocked(&session);
    (
        session,
        Geometry {
            press,
            magnetic,
            guide,
            gap,
        },
    )
}

fn press<'receiver>(
    surface: PresentedDockspaceSurface,
    source: &'receiver PresentedDockReceiver,
    metadata: PointerEventMetadata,
) -> HostPointerInput<'receiver> {
    let route = HostPointerRoute::new(surface, source.center());
    HostPointerInput::new(
        POINTER,
        Event::ButtonPressed(Button::Primary),
        route,
        route,
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::delivery(source),
    )
    .with_metadata(metadata)
}

fn target_edge(
    source: PresentedDockspaceSurface,
    target: &PresentedDockspaceSurface,
    event: Event,
    point: LogicalPoint,
    metadata: PointerEventMetadata,
) -> HostPointerInput<'_> {
    HostPointerInput::new(
        POINTER,
        event,
        HostPointerRoute::new(source, point),
        HostPointerRoute::new(*target, point),
        SurfacePointerCapture::ProviderEndpoint,
        SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(target),
    )
    .with_metadata(metadata)
}

fn targeting(pointer: SurfacePointerId, enabled: bool) -> HostPointerInput<'static> {
    HostPointerInput::new(
        pointer,
        Event::MagneticTargetingChanged { enabled },
        HostPointerRoute::Unknown,
        HostPointerRoute::Unknown,
        SurfacePointerCapture::Unknown,
        SurfacePointerReceiverFacts::unknown(),
    )
}

fn assert_undocked(session: &DockspaceSession) {
    let view = session.view();
    assert_eq!(view.item(FIRST).unwrap().surface(), A);
    assert_eq!(view.item(FIRST).unwrap().tabs().items(), &[FIRST, SECOND]);
    assert_eq!(view.item(THIRD).unwrap().surface(), B);
    assert_eq!(view.item(THIRD).unwrap().tabs().items(), &[THIRD, FOURTH]);
}

fn assert_docked(session: &DockspaceSession, items: &[ItemId]) {
    let view = session.view();
    assert_eq!(view.item(FIRST).unwrap().surface(), B);
    assert_eq!(view.item(FIRST).unwrap().tabs().items(), items);
    assert_eq!(view.item(SECOND).unwrap().surface(), A);
    assert_eq!(view.item(SECOND).unwrap().tabs().items(), &[SECOND]);
    assert!(!session.has_active_gesture());
}

fn assert_pending(report: &HostFrameReport) -> Feedback {
    let [feedback] = report.manipulation_feedback() else {
        panic!("the exact unpresented release must remain pending: {report:?}");
    };
    assert_eq!(
        feedback.status(),
        Status::Pending(PendingReason::AwaitingPresentation)
    );
    assert!(!report.workspace_changed());
    *feedback
}

fn assert_delivered(report: &HostFrameReport, pending: Feedback) {
    let [feedback] = report.manipulation_feedback() else {
        panic!("the exact frozen output must deliver once: {report:?}");
    };
    assert_eq!(feedback.status(), Status::Delivered(Delivery::Dock));
    assert_eq!(feedback.manipulation_id(), pending.manipulation_id());
    assert_ne!(feedback.id(), pending.id());
    assert!(report.workspace_changed());
}

#[test]
fn marker_rollback_foreign_pointer_and_late_preview_require_fresh_acquisition() {
    let (mut session, geometry) = fixture();
    let a = session.presented_surface(A).unwrap();
    let b = session.presented_surface(B).unwrap();
    let source = session
        .host_pointer_hit(A, geometry.press)
        .unwrap()
        .delivery()
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            press(a, &source, metadata()),
            target_edge(a, &b, Event::Moved, geometry.magnetic, metadata()),
            targeting(FOREIGN_POINTER, false),
        ])
        .unwrap();
    let acquired = commit(frame);
    assert!(!acquired.workspace_changed());
    assert!(
        acquired
            .inputs()
            .contains(&HostInputOutcome::MagneticTargetingUpdated {
                enabled: false,
                changed: false,
            })
    );
    let (old_recordings, old_outputs) = paint(&mut session);
    assert_dock_preview(&old_recordings);
    let old_artifacts = old_outputs
        .iter()
        .map(PaintedSurfaceOutput::presentation_artifact)
        .collect::<Vec<_>>();

    let before = session.version();
    let mut abandoned = session.begin_host_frame().unwrap();
    abandoned
        .submit_host_pointer(targeting(POINTER, false))
        .unwrap();
    let suppressed = RecordingRenderer::execute(abandoned.paint_plan(B).unwrap().unwrap())
        .unwrap()
        .0;
    assert!(
        suppressed
            .records
            .iter()
            .all(|record| !matches!(record, OwnedRecord::Preview(_)))
    );
    drop(abandoned);
    assert_eq!(session.version(), before);
    assert_eq!(session.presented_surface(A), Some(a));
    assert_eq!(session.presented_surface(B), Some(b));
    assert!(session.has_active_gesture());
    assert_eq!(previews(&inspect(&mut session)), previews(&old_recordings));
    assert_undocked(&session);

    // Retry the identical marker after abandoning its unpublished candidate.
    let mut retry = session.begin_host_frame().unwrap();
    retry
        .submit_host_pointer(targeting(POINTER, false))
        .unwrap();
    let retried = commit(retry);
    assert!(!retried.workspace_changed());
    assert_eq!(
        retried.inputs(),
        &[HostInputOutcome::MagneticTargetingUpdated {
            enabled: false,
            changed: true,
        }],
        "the abandoned marker must not leak its flag into the published gesture"
    );
    assert!(previews(&inspect(&mut session)).is_empty());
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), before);

    let late = present(&mut session, old_outputs);
    assert!(!late.workspace_changed());
    assert!(late.manipulation_feedback().is_empty());
    assert!(previews(&inspect(&mut session)).is_empty());
    assert_undocked(&session);

    let mut enable = session.begin_host_frame().unwrap();
    enable
        .submit_host_pointer(targeting(POINTER, true))
        .unwrap();
    let enabled = commit(enable);
    assert!(!enabled.workspace_changed());
    assert_eq!(
        enabled.inputs(),
        &[HostInputOutcome::MagneticTargetingUpdated {
            enabled: true,
            changed: true,
        }]
    );
    assert!(
        previews(&inspect(&mut session)).is_empty(),
        "enabling cannot mint a target"
    );

    let a = session.presented_surface(A).unwrap();
    let b = session.presented_surface(B).unwrap();
    let mut motion = session.begin_host_frame().unwrap();
    motion
        .submit_host_pointer(target_edge(
            a,
            &b,
            Event::Moved,
            geometry.magnetic,
            metadata(),
        ))
        .unwrap();
    assert!(!commit(motion).workspace_changed());
    let (fresh_recordings, fresh_outputs) = paint(&mut session);
    assert_dock_preview(&fresh_recordings);
    assert_eq!(previews(&fresh_recordings), previews(&old_recordings));
    assert!(
        fresh_outputs
            .iter()
            .all(|output| !old_artifacts.contains(&output.presentation_artifact()))
    );

    let mut release = session.begin_host_frame().unwrap();
    release
        .submit_host_pointer(target_edge(
            a,
            &b,
            Event::ButtonReleased(Button::Primary),
            geometry.magnetic,
            metadata(),
        ))
        .unwrap();
    let pending = assert_pending(&commit(release));
    assert_undocked(&session);
    assert!(!session.has_active_gesture());
    let delivered = present(&mut session, fresh_outputs);
    assert_delivered(&delivered, pending);
    assert_docked(&session, &[THIRD, FOURTH, FIRST]);
    assert!(settle(&mut session).manipulation_feedback().is_empty());
}

#[test]
fn suppressed_press_or_move_survives_release_none_and_preserves_direct_targets() {
    for (target_kind, suppress_at_press) in [
        ("magnetic", false),
        ("magnetic", true),
        ("guide", false),
        ("tab_gap", false),
    ] {
        let (mut session, geometry) = fixture();
        let a = session.presented_surface(A).unwrap();
        let b = session.presented_surface(B).unwrap();
        let source = session
            .host_pointer_hit(A, geometry.press)
            .unwrap()
            .delivery()
            .unwrap();
        let target = match target_kind {
            "magnetic" => geometry.magnetic,
            "guide" => geometry.guide,
            "tab_gap" => geometry.gap,
            _ => unreachable!(),
        };
        let press_metadata = if suppress_at_press {
            metadata().with_magnetic_targeting(false)
        } else {
            metadata()
        };
        let move_metadata = if suppress_at_press {
            metadata()
        } else {
            metadata().with_magnetic_targeting(false)
        };
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([
                press(a, &source, press_metadata),
                target_edge(a, &b, Event::Moved, target, move_metadata),
                target_edge(
                    a,
                    &b,
                    Event::ButtonReleased(Button::Primary),
                    target,
                    metadata(),
                ),
            ])
            .unwrap();
        let recordings = record(&mut frame);
        let mut released = commit(frame);
        assert_undocked(&session);
        assert!(!session.has_active_gesture());
        if target_kind == "magnetic" {
            assert!(previews(&recordings).is_empty());
            assert!(!released.workspace_changed());
            assert!(
                released
                    .manipulation_feedback()
                    .iter()
                    .all(|feedback| matches!(
                        feedback.status(),
                        Status::Rejected(_) | Status::Cancelled(_)
                    ))
            );
            let settled = present(&mut session, released.take_painted_outputs());
            assert!(!settled.workspace_changed());
            assert!(settled.manipulation_feedback().is_empty());
            assert_eq!(session.version(), before);
            assert_undocked(&session);
        } else {
            assert_dock_preview(&recordings);
            let pending = assert_pending(&released);
            let delivered = present(&mut session, released.take_painted_outputs());
            assert_delivered(&delivered, pending);
            let items = if target_kind == "tab_gap" {
                [THIRD, FIRST, FOURTH]
            } else {
                [THIRD, FOURTH, FIRST]
            };
            assert_docked(&session, &items);
        }
    }
}

#[test]
fn late_targeting_markers_do_not_change_a_pending_releases_exact_output() {
    let (mut session, geometry) = fixture();
    let a = session.presented_surface(A).unwrap();
    let b = session.presented_surface(B).unwrap();
    let source = session
        .host_pointer_hit(A, geometry.press)
        .unwrap()
        .delivery()
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch([
            press(a, &source, metadata()),
            target_edge(a, &b, Event::Moved, geometry.magnetic, metadata()),
            target_edge(
                a,
                &b,
                Event::ButtonReleased(Button::Primary),
                geometry.magnetic,
                metadata(),
            ),
        ])
        .unwrap();
    let pending = assert_pending(&commit(frame));
    assert!(!session.has_active_gesture());
    assert_undocked(&session);
    let (frozen_recordings, frozen_outputs) = paint(&mut session);
    assert_dock_preview(&frozen_recordings);
    let before = session.version();

    let mut late = session.begin_host_frame().unwrap();
    late.submit_host_pointer_batch([
        targeting(POINTER, false),
        targeting(POINTER, true),
        targeting(POINTER, false),
    ])
    .unwrap();
    let report = commit(late);
    assert!(!report.workspace_changed());
    assert_eq!(
        report.inputs(),
        &[
            HostInputOutcome::MagneticTargetingUpdated {
                enabled: false,
                changed: false,
            },
            HostInputOutcome::MagneticTargetingUpdated {
                enabled: true,
                changed: false,
            },
            HostInputOutcome::MagneticTargetingUpdated {
                enabled: false,
                changed: false,
            },
        ],
        "a consumed release has no active owner for later intent updates"
    );
    assert!(report.manipulation_feedback().is_empty());
    assert_eq!(session.version(), before);
    assert!(!session.has_active_gesture());
    assert_undocked(&session);
    assert_eq!(
        previews(&inspect(&mut session)),
        previews(&frozen_recordings)
    );

    let delivered = present(&mut session, frozen_outputs);
    assert_delivered(&delivered, pending);
    assert_docked(&session, &[THIRD, FOURTH, FIRST]);
    let after = session.version();
    let unchanged = settle(&mut session);
    assert_eq!(session.version(), after);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.manipulation_feedback().is_empty());
    assert!(unchanged.surface_presentation_commits().is_empty());
}
