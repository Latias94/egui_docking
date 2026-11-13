use super::*;

use crate::behavior_tests::support::{
    MeasurementProfile, TestPresentationHost, complete_host_frame_with_retained_or_unavailable,
    measurements, publish_surface_with,
};
use crate::engine::{CoreHostFrame, DockEngine, PendingMutation, PreparedSurfacePaintCandidate};
use crate::event::ReductionCause;
use crate::intent::{Authority, PointerButton, PointerId};
use crate::interaction::{
    InteractionCancelReason, InteractionOutcome, InteractionStatus, PreviewVisual,
};
use crate::pointer_journal::{
    HostLocalPointerRoute, PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind,
    PointerEdgeLocation, PointerEdgeSequence, PointerStreamCancelReason,
    SurfaceLocalPointerProvider,
};
use crate::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverProbeRequest, PointerReceiverReceiptBatch,
    PresentedPointerReceiverObservation,
};
use crate::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use crate::scene::ContainedLayoutOwner;
use crate::transition::{SurfaceContributionOutcome, SurfaceContributionRejection};
use crate::viewport_focus::{FocusCausalStamp, PaneFocusIntentGeneration};

const PANE_COUNT: usize = 16;

fn core_fixture() -> (DockEngine, TestPresentationHost) {
    core_fixture_at(initial_rect())
}

fn core_fixture_at(source_rect: LogicalRect) -> (DockEngine, TestPresentationHost) {
    let (session, _) = fixture(PANE_COUNT);
    let mut builder = session.workspace().clone().into_builder();
    builder
        .draft_mut()
        .contained_floatings
        .get_mut(&FLOATING)
        .unwrap()
        .rect = source_rect;
    let mut engine = DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap();
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface_with(
        &mut engine,
        &mut host,
        SURFACE,
        rect(0.0, 0.0, 131_072.0, 720.0),
        MeasurementProfile {
            pane_minimum: LogicalSize::new(64.0, 48.0).unwrap(),
            tab_content_width: 96.0,
            ..MeasurementProfile::default()
        },
    );
    (engine, host)
}

#[test]
fn clipped_contained_sources_keep_their_unclipped_durable_acquisition_geometry() {
    let source_rect = rect(-80.0, 140.0, 400.0, 300.0);
    for source in ["title", "resize", "tab", "splitter"] {
        let (mut engine, mut host) = core_fixture_at(source_rect);
        let projection = engine.interaction_projection(SURFACE).unwrap();
        assert_ne!(
            projection
                .plan()
                .contained_record(FLOATING)
                .unwrap()
                .outer_bounds(),
            source_rect
        );
        assert_eq!(
            projection.plan().unclipped_contained_bounds(FLOATING),
            Some(source_rect)
        );
        let authority = projection.authority();
        let (region, point) = core_receiver(&engine, |kind| match (source, kind) {
            ("title", PresentationHitRegionKind::ContainedTitle(floating)) => floating == FLOATING,
            (
                "resize",
                PresentationHitRegionKind::ContainedResize {
                    floating,
                    direction,
                },
            ) => floating == FLOATING && direction == crate::scene::ContainedResizeDirection::East,
            ("tab", PresentationHitRegionKind::TabBody(tab)) => tab.item == LEFT,
            ("splitter", PresentationHitRegionKind::SplitterHandle(splitter)) => {
                splitter.root == CONTAINED_ROOT
            }
            _ => false,
        });
        let provider = engine
            .create_host_local_pointer_provider(host.lease(), PointerEdgeSequence::new(0))
            .unwrap();
        let mut frame = host.begin(&engine);
        submit_core_edge(
            &mut frame,
            &provider,
            0,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            HostLocalPointerRoute::new(authority, point),
            Some(region),
        );
        let status = frame.view().interaction().status();
        assert!(
            match source {
                "title" | "tab" => matches!(status, InteractionStatus::Armed { .. }),
                "resize" => matches!(status, InteractionStatus::ContainedTransforming { .. }),
                "splitter" => matches!(status, InteractionStatus::Resizing { .. }),
                _ => false,
            },
            "{source} must acquire the visible clipped source: {status:?}"
        );
        complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
        host.finish(frame, &mut engine);
        assert_eq!(
            engine
                .workspace()
                .contained_floating(FLOATING)
                .unwrap()
                .rect,
            source_rect
        );
    }
}

fn core_receiver(
    engine: &DockEngine,
    kind: impl Fn(PresentationHitRegionKind) -> bool,
) -> (PresentationHitRegionId, LogicalPoint) {
    let projection = engine.interaction_projection(SURFACE).unwrap();
    let region = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| kind(region.id().kind()))
        .unwrap();
    let bounds = region.hit().rect();
    (
        region.id(),
        LogicalPoint::new(
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        )
        .unwrap(),
    )
}

fn submit_core_edge(
    frame: &mut CoreHostFrame,
    provider: &SurfaceLocalPointerProvider,
    previous: u64,
    kind: PointerEdgeKind,
    route: HostLocalPointerRoute,
    delivery: Option<PresentationHitRegionId>,
) {
    let edge = PointerEdge::new(
        PointerEdgeSequence::new(previous + 1),
        PointerId::new(1),
        kind,
        PointerEdgeLocation::HostLocal {
            manipulation: Authority::Known(route),
            hover: Authority::Known(route),
        },
        Authority::Known(PointerCaptureOwner::ProviderEndpoint),
    );
    frame
        .submit_surface_pointer_journal(
            provider,
            PointerEdgeJournal::new(
                PointerEdgeSequence::new(previous),
                edge.sequence(),
                vec![edge],
            )
            .unwrap(),
        )
        .unwrap();
    let [candidate] = frame.pointer_receiver_candidates().unwrap().candidates() else {
        panic!("one physical edge must produce one receiver challenge");
    };
    let observation = match candidate.probes() {
        PointerReceiverProbeRequest::NotApplicable => PointerReceiverObservation::NotApplicable,
        PointerReceiverProbeRequest::Delivery => {
            let projection = frame.view().interaction_projection(SURFACE).unwrap();
            let delivery = PointerReceiverDelivery::new(
                projection,
                PointerReceiverDeliveryDisposition::Dock(delivery.unwrap()),
            )
            .unwrap();
            PointerReceiverObservation::Presented(
                PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                    delivery,
                )])
                .unwrap(),
            )
        }
        PointerReceiverProbeRequest::HoverHit => {
            let hover = frame
                .view()
                .resolve_hover_drop_receiver(SURFACE, route.position())
                .unwrap();
            PointerReceiverObservation::Presented(
                PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                    hover,
                )])
                .unwrap(),
            )
        }
    };
    let receipts = PointerReceiverReceiptBatch::new([candidate.receipt(observation)]).unwrap();
    frame.submit_pointer_receiver_receipts(receipts).unwrap();
}

fn submit_empty_core_pointer(
    frame: &mut CoreHostFrame,
    provider: &SurfaceLocalPointerProvider,
    previous: u64,
) {
    let through = PointerEdgeSequence::new(previous);
    frame
        .submit_surface_pointer_journal(
            provider,
            PointerEdgeJournal::new(through, through, Vec::new()).unwrap(),
        )
        .unwrap();
    frame
        .submit_pointer_receiver_receipts(PointerReceiverReceiptBatch::new([]).unwrap())
        .unwrap();
}

#[test]
fn presented_target_preview_survives_unpresented_measurements_without_borrowing_them() {
    let (mut engine, mut host) = core_fixture();
    let version = engine.version();
    let presented = engine.interaction_projection(SURFACE).unwrap();
    let target_basis = presented.output().stamp();
    let authority = presented.authority();
    let target_button = presented
        .plan()
        .drop_guide_clusters()
        .iter()
        .filter(|guide| guide.id().root == MAIN_ROOT)
        .find_map(|guide| {
            guide.target(crate::drop_guide::DropGuideSlot::Edge(
                crate::command::Edge::Left,
            ))
        })
        .unwrap();
    let bounds = target_button.hit().rect();
    let target = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .unwrap();
    let (title, start) = core_receiver(
        &engine,
        |kind| matches!(kind, PresentationHitRegionKind::ContainedTitle(floating) if floating == FLOATING),
    );
    let provider = engine
        .create_host_local_pointer_provider(host.lease(), PointerEdgeSequence::new(0))
        .unwrap();
    let mut frame = host.begin(&engine);
    submit_core_edge(
        &mut frame,
        &provider,
        0,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        HostLocalPointerRoute::new(authority, start),
        Some(title),
    );
    submit_core_edge(
        &mut frame,
        &provider,
        1,
        PointerEdgeKind::Moved,
        HostLocalPointerRoute::new(authority, target),
        None,
    );
    let InteractionStatus::Dragging { session } = frame.view().interaction().status() else {
        panic!("the real contained title press and move must start a drag");
    };
    let original_visual = frame
        .view()
        .interaction()
        .active_drag(session)
        .unwrap()
        .preview()
        .unwrap()
        .public()
        .visual()
        .clone();
    assert!(matches!(original_visual, PreviewVisual::Dock { .. }));
    frame.refresh_presentation_layout().unwrap();
    complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    host.finish(frame, &mut engine);

    // B changes target geometry, but no paint or presentation may supersede A.
    let mut frame = host.begin(&engine);
    submit_empty_core_pointer(&mut frame, &provider, 2);
    let profile = MeasurementProfile {
        pane_minimum: LogicalSize::new(64.0, 48.0).unwrap(),
        tab_content_width: 96.0,
        ..MeasurementProfile::default()
    };
    let measured = measurements(
        &frame.candidate,
        SURFACE,
        rect(0.0, 0.0, 65_536.0, 720.0),
        profile,
    );
    let token = frame.view().begin_surface_contribution(SURFACE).unwrap();
    let contribution = frame
        .view()
        .prepare_surface_contribution(token, measured)
        .unwrap();
    frame.push_surface_contribution(contribution).unwrap();
    let installed = host.finish(frame, &mut engine);
    let [
        SurfaceContributionOutcome::Ready {
            stamp: unpresented_basis,
            ..
        },
    ] = installed.surface_contributions()
    else {
        panic!("changed external measurements must install a real unpresented B candidate");
    };
    let unpresented_basis = *unpresented_basis;
    assert_ne!(unpresented_basis, target_basis);
    assert_eq!(
        engine
            .interaction_projection(SURFACE)
            .unwrap()
            .output()
            .stamp(),
        target_basis,
        "the next hover must still solve against presented A"
    );

    let mut frame = host.begin(&engine);
    submit_core_edge(
        &mut frame,
        &provider,
        2,
        PointerEdgeKind::Moved,
        HostLocalPointerRoute::new(authority, offset(target, 1.0, 0.0)),
        None,
    );
    let drag = frame.view().interaction().active_drag(session).unwrap();
    let preview = drag.preview().unwrap();
    assert_eq!(preview.public().visual(), &original_visual);
    assert_eq!(preview.public().token().scene(), target_basis);
    assert_eq!(
        preview.target_layout_basis(),
        Some(target_basis),
        "publication must bind the layout that actually solved the hover, not ready B"
    );
    frame.refresh_presentation_layout().unwrap();
    complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
    host.finish(frame, &mut engine);

    let source_only = engine.scene().ready_candidate(SURFACE).unwrap();
    let source_only_stamp = source_only.stamp();
    let projection = source_only.plan().contained_projection().unwrap();
    assert_eq!(projection.owner, ContainedLayoutOwner::Drag(session));
    assert_eq!(projection.target_basis, Some(unpresented_basis));
    assert_ne!(source_only_stamp, unpresented_basis);
    let drag = engine.interaction().active_drag(session).unwrap().clone();
    assert!(
        drag.preview().is_some(),
        "release must test a retained A proof"
    );
    assert!(engine.current_contained_projection_is_owned(SURFACE));
    assert!(
        engine.drag_preview_authority_is_current(&drag),
        "actual A independently authorizes its own target proof despite unpresented B and C"
    );

    let cause = ReductionCause::SurfaceContribution {
        tick: engine.last_reducer_tick(),
        surface: SURFACE,
        base: source_only_stamp,
    };
    let policy = engine.policy.clone();
    let crate::interaction::PreviewProof::Dock { command, .. } = drag.preview().unwrap().proof()
    else {
        panic!("A must retain the exact docking proposal solved from its presented layout");
    };
    let mut expected_workspace = engine.workspace().clone();
    let expected = crate::transaction::WorkspaceTransaction::from_commands([command.clone()])
        .apply(&mut expected_workspace, &policy)
        .unwrap();
    assert!(expected.changed());

    // Without actual A, source-only C must not substitute its unpresented B basis.
    let mut without_actual = engine.candidate();
    without_actual
        .presentation_authority
        .scene
        .revoke_interaction_authority_for_streams(&std::collections::BTreeSet::from([
            authority.stream()
        ]));
    assert!(without_actual.interaction_projection(SURFACE).is_none());
    assert!(without_actual.current_contained_projection_is_owned(SURFACE));
    assert_eq!(
        without_actual
            .scene()
            .ready_candidate(SURFACE)
            .unwrap()
            .stamp(),
        source_only_stamp
    );
    assert!(!without_actual.drag_preview_authority_is_current(&drag));
    let mut rejected_events = Vec::new();
    let rejected = without_actual
        .finish_drag_release_after_evaluation(
            cause,
            FocusCausalStamp::new(PaneFocusIntentGeneration::new(1), cause),
            drag.owner,
            session,
            &policy,
            &mut rejected_events,
            &mut Vec::new(),
        )
        .unwrap();
    assert!(matches!(
        rejected,
        InteractionOutcome::Cancelled {
            reason: InteractionCancelReason::UnknownTargetAuthority,
            ..
        }
    ));
    assert!(rejected_events.is_empty());
    assert!(without_actual.pending_mutation.is_none());
    assert_eq!(
        without_actual.interaction().status(),
        InteractionStatus::Idle
    );
    assert_eq!(without_actual.version(), version);
    assert_eq!(without_actual.workspace(), engine.workspace());
    assert_eq!(
        without_actual
            .workspace()
            .contained_floating(FLOATING)
            .unwrap()
            .rect,
        initial_rect()
    );

    // The original engine still owns actual A and must deliver that same proposal now.
    assert_eq!(
        engine.interaction_projection(SURFACE).unwrap().plan_stamp(),
        target_basis
    );
    assert!(engine.drag_preview_authority_is_current(&drag));
    let mut events = Vec::new();
    let outcome = engine
        .finish_drag_release_after_evaluation(
            cause,
            FocusCausalStamp::new(PaneFocusIntentGeneration::new(1), cause),
            drag.owner,
            session,
            &policy,
            &mut events,
            &mut Vec::new(),
        )
        .unwrap();
    assert!(matches!(
        outcome,
        InteractionOutcome::DragDelivered {
            session: delivered,
            delivery: crate::interaction::InteractionDelivery::Workspace {
                kind: crate::interaction::WorkspaceDeliveryKind::Dock,
                changed: true,
                ..
            },
        } if delivered == session
    ));
    let [event] = events.as_slice() else {
        panic!("the release must commit exactly the docking proposal from A");
    };
    assert_eq!(
        event.kind(),
        &crate::event::WorkspaceEventKind::CommandCommitted(expected.outcomes()[0].clone())
    );
    assert!(
        engine
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
            .is_none()
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_ne!(engine.version(), version);
    assert_eq!(engine.workspace(), &expected_workspace);
}

#[test]
fn prepared_contained_contribution_is_rejected_after_same_frame_cancellation() {
    let (mut engine, mut host) = core_fixture();
    let version = engine.version();
    let authority = engine.interaction_projection(SURFACE).unwrap().authority();
    let (resize, start) = core_receiver(&engine, |kind| {
        matches!(kind, PresentationHitRegionKind::ContainedResize {
            floating,
            direction: crate::scene::ContainedResizeDirection::SouthEast,
        } if floating == FLOATING)
    });
    let provider = engine
        .create_host_local_pointer_provider(host.lease(), PointerEdgeSequence::new(0))
        .unwrap();
    let mut frame = host.begin(&engine);
    submit_core_edge(
        &mut frame,
        &provider,
        0,
        PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        HostLocalPointerRoute::new(authority, start),
        Some(resize),
    );
    let moved = offset(start, 40.0, 0.0);
    submit_core_edge(
        &mut frame,
        &provider,
        1,
        PointerEdgeKind::Moved,
        HostLocalPointerRoute::new(authority, moved),
        None,
    );
    let InteractionStatus::ContainedTransforming { session } = frame.view().interaction().status()
    else {
        panic!("the real resize must own the prepared projection");
    };
    let measured = frame
        .view()
        .scene()
        .ready_candidate(SURFACE)
        .unwrap()
        .plan()
        .layout_facts()
        .unwrap()
        .measurements()
        .as_ref()
        .clone();
    let token = frame.view().begin_surface_contribution(SURFACE).unwrap();
    let prepared = frame
        .view()
        .prepare_surface_contribution(token, measured)
        .unwrap();
    let PreparedSurfacePaintCandidate::Ready(candidate) = prepared.paint_candidate() else {
        panic!("the contribution must contain G's real compiled Ready plan");
    };
    let prepared_projection = candidate.plan().contained_projection().unwrap();
    assert_eq!(
        prepared_projection.owner,
        ContainedLayoutOwner::Transform(session)
    );
    assert_eq!(prepared_projection.rect, rect(200.0, 140.0, 440.0, 300.0));

    submit_core_edge(
        &mut frame,
        &provider,
        2,
        PointerEdgeKind::StreamCancelled(PointerStreamCancelReason::ExplicitPlatformCancellation),
        HostLocalPointerRoute::new(authority, moved),
        None,
    );
    assert_eq!(frame.view().interaction().status(), InteractionStatus::Idle);
    assert_eq!(
        frame.view().scene().surface(SURFACE).unwrap().stamp(),
        token.base()
    );
    assert!(
        frame
            .candidate
            .contained_layout_projection(SURFACE)
            .is_none()
    );
    frame.push_surface_contribution(prepared).unwrap();
    let transition = host.finish(frame, &mut engine);
    assert_eq!(
        transition.surface_contributions(),
        &[SurfaceContributionOutcome::Rejected {
            surface: SURFACE,
            reason: SurfaceContributionRejection::ContainedProjectionChanged { surface: SURFACE },
        }],
        "an unchanged scene stamp does not let a late callback resurrect cancelled G"
    );
    let plan = engine.scene().ready_candidate(SURFACE).unwrap().plan();
    assert!(plan.contained_projection().is_none());
    assert_eq!(
        plan.contained_record(FLOATING).unwrap().outer_bounds(),
        initial_rect()
    );
    assert_eq!(engine.interaction().status(), InteractionStatus::Idle);
    assert_eq!(engine.version(), version);
    assert_eq!(provider.committed_through(), PointerEdgeSequence::new(3));
}

#[test]
fn cancelled_presented_resize_cannot_rearm_title_resize_or_tab_from_old_projection() {
    for source in ["title", "resize", "tab", "splitter"] {
        let (mut session, before) = fixture(PANE_COUNT);
        let version = session.version();
        let durable = session.workspace().clone();
        let start = press(&mut session, before.resize);
        let moved = offset(start, 40.0, 0.0);
        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::Moved,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            )])
            .unwrap();
        let (tab, splitter) = {
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            let mut splitter = None;
            plan.visit_surface_program(|step| {
                if let Step::Operation {
                    root,
                    operation: Operation::Splitter(record),
                    ..
                } = step
                    && root.root() == CONTAINED_ROOT
                {
                    splitter = plan.receiver_for_splitter(record);
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            (plan.tab_receiver(LEFT).unwrap(), splitter.unwrap())
        };
        let (projected, output) = paint_frame(frame, PANE_COUNT);
        projected.assert_projection(&before, rect(200.0, 140.0, 440.0, 300.0));
        let artifact = output.presentation_artifact();
        present(&mut session, output);
        assert_eq!(
            session.view().contained(FLOATING).unwrap().rect(),
            initial_rect()
        );

        let descriptor = match source {
            "title" => projected.title,
            "resize" => projected.resize,
            "tab" => tab,
            "splitter" => splitter,
            _ => unreachable!(),
        };
        let receiver = session
            .bind_presented_receiver(&descriptor)
            .expect("the presented projection still belongs to the active resize");

        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::StreamCancelled(SurfacePointerCancelReason::ExplicitPlatformCancellation),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            )])
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(!frame.commit().unwrap().workspace_changed());
        assert!(!session.has_active_gesture());
        let surface = session.presented_surface(SURFACE).unwrap();
        assert!(surface.matches_presentation_artifact(artifact));
        assert!(
            session.bind_presented_receiver(&descriptor).is_none(),
            "{source}: cancelled geometry cannot mint a fresh receiver"
        );
        let start = receiver.center();
        let moved = offset(start, 10.0, 0.0);
        let press_route = HostPointerRoute::new(surface, start);
        let move_route = HostPointerRoute::new(surface, moved);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    Event::ButtonPressed(SurfacePointerButton::Primary),
                    press_route,
                    press_route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&receiver)
                        .with_clear_hover_lane(&surface),
                ),
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    Event::Moved,
                    move_route,
                    move_route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
                ),
            ])
            .unwrap();
        let (restored, output) = paint_frame(frame, PANE_COUNT);
        assert!(
            !session.has_active_gesture(),
            "{source}: a cancelled session's still-presented 440px source cannot rearm"
        );
        restored.assert_projection(&before, initial_rect());
        present(&mut session, output);

        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, moved);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::ButtonReleased(SurfacePointerButton::Primary),
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
            )])
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(
            !frame.commit().unwrap().workspace_changed(),
            "{source}: rejected acquisition cannot commit"
        );
        assert!(!session.has_active_gesture());
        assert_eq!(session.version(), version);
        assert_eq!(
            session.view().contained(FLOATING).unwrap().rect(),
            initial_rect()
        );
        assert_eq!(session.view().item(LEFT).unwrap().root(), CONTAINED_ROOT);
        assert_eq!(session.view().item(RIGHT).unwrap().root(), CONTAINED_ROOT);
        assert_eq!(session.workspace(), &durable);
    }
}
