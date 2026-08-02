//! Presentation authority contracts for the core-owned host observation protocol.

mod support;

use std::collections::BTreeSet;

use dockspace::engine::{DockEngine, EngineError};
use dockspace::geometry::LogicalRect;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::pointer_journal::PointerEdgeSequence;
use dockspace::policy::DockPolicy;
use dockspace::presentation_observation::{
    HostPresentationObservation, HostPresentationObservationOutcome,
    HostPresentationObservationRejection, PresentationHostRetirementReason,
};
use dockspace::scene::SurfaceScene;
use dockspace::transition::{PresentationHostRetirementOutcome, SurfaceContributionOutcome};
use support::{
    TestPresentationHost, complete_host_frame_with_current_outputs,
    complete_host_frame_with_unavailable,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(10);
const TARGET_ROOT: RootId = RootId::new(20);

fn bounds(width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, height).expect("test bounds are finite")
}

fn engine() -> DockEngine {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    DockEngine::new(
        builder.build().expect("test workspace is valid"),
        DockPolicy::default(),
    )
    .expect("test engine is valid")
}

fn ready_authority(
    engine: &DockEngine,
) -> dockspace::presentation_observation::PresentedSurfaceAuthority {
    engine
        .interaction_authority(SURFACE)
        .expect("surface has observed interaction authority")
}

fn two_surface_engine() -> (DockEngine, NodeId) {
    let mut builder = Workspace::builder();
    let source_tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(ROOT, RootRecord::new(source_tabs));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    (
        DockEngine::new(
            builder.build().expect("test workspace is valid"),
            DockPolicy::default(),
        )
        .expect("test engine is valid"),
        source_tabs,
    )
}

#[test]
fn a_compiled_candidate_is_not_interactive_until_a_later_observation() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let stamp =
        support::install_surface_projection(&mut engine, &mut host, SURFACE, bounds(640.0, 480.0));

    assert!(engine.scene().ready_surface(SURFACE).is_none());
    assert_eq!(
        engine
            .scene()
            .surface(SURFACE)
            .and_then(SurfaceScene::ready)
            .map(|ready| ready.candidate().stamp()),
        Some(stamp)
    );

    let emit = host.begin(&engine);
    let emit = complete_host_frame_with_current_outputs(&engine, emit);
    let emitted = host.finish_presentation(emit, &mut engine);
    assert_eq!(emitted.presentation_emissions().len(), 1);
    assert!(engine.scene().ready_surface(SURFACE).is_none());

    let observe = host.begin(&engine);
    let observe = complete_host_frame_with_current_outputs(&engine, observe);
    let observed = host.finish_presentation(observe, &mut engine);
    assert!(observed.presentation_observations().iter().any(|outcome| {
        matches!(
            outcome,
            dockspace::presentation_observation::HostPresentationObservationOutcome::Retired {
                promotion_eligible: true,
                ..
            }
        )
    }));

    let authority = ready_authority(&engine);
    assert_eq!(authority.emission().stream(), authority.stream());
    assert_eq!(
        engine
            .scene()
            .ready_surface(SURFACE)
            .map(|ready| ready.stamp()),
        Some(stamp)
    );
}

#[test]
fn a_superseded_host_can_settle_but_cannot_reauthorize_the_surface() {
    let mut engine = engine();
    let mut old_host = TestPresentationHost::new(&mut engine);
    support::install_surface_projection(&mut engine, &mut old_host, SURFACE, bounds(640.0, 480.0));
    let old_emit = old_host.begin(&engine);
    let old_emit = complete_host_frame_with_current_outputs(&engine, old_emit);
    let old_emit_transition = old_host.finish_presentation(old_emit, &mut engine);
    assert_eq!(old_emit_transition.presentation_emissions().len(), 1);
    let old_stream = old_emit_transition.presentation_emissions()[0]
        .output()
        .stream();

    let mut current_host = TestPresentationHost::new(&mut engine);
    let current_emit = current_host.begin(&engine);
    let current_emit = complete_host_frame_with_current_outputs(&engine, current_emit);
    let current_emit_transition = current_host.finish_presentation(current_emit, &mut engine);
    assert_eq!(current_emit_transition.presentation_emissions().len(), 1);
    let current_stream = current_emit_transition.presentation_emissions()[0]
        .output()
        .stream();
    assert!(engine.scene().ready_surface(SURFACE).is_none());

    let mut current_observation_prelude = engine
        .begin_host_frame_with_observers(current_host.lease(), [old_host.lease()])
        .expect("current host frame must enrol the superseded observer");
    old_host.submit_observation(&mut current_observation_prelude);
    current_host.submit_observation(&mut current_observation_prelude);
    let current_observation = current_observation_prelude
        .seal(&engine)
        .expect("combined observation frame seals");
    let current_observation =
        complete_host_frame_with_current_outputs(&engine, current_observation);
    let current_transition = current_host.finish_presentation(current_observation, &mut engine);
    old_host.confirm_submitted_observation(&current_transition);
    assert!(
        current_transition
            .presentation_observations()
            .iter()
            .any(|outcome| {
                matches!(
            outcome,
            dockspace::presentation_observation::HostPresentationObservationOutcome::Retired {
                stream,
                promotion_eligible: false,
                ..
            } if *stream == old_stream
        )
            })
    );

    assert!(
        current_transition
            .presentation_observations()
            .iter()
            .any(|outcome| {
                matches!(
            outcome,
            dockspace::presentation_observation::HostPresentationObservationOutcome::Retired {
                stream,
                promotion_eligible: true,
                ..
            } if *stream == current_stream
        )
            })
    );
    let authority = ready_authority(&engine);
    assert_eq!(authority.emission().stream(), authority.stream());
}

#[test]
fn a_host_frame_without_an_observation_is_rejected_before_any_reduction() {
    let mut engine = engine();
    let host = engine
        .create_presentation_host()
        .expect("test host lease must mint");
    let prelude = engine
        .begin_host_frame(host)
        .expect("host frame must begin");

    assert!(matches!(
        prelude.seal(&engine),
        Err(EngineError::HostFramePresentationObservationMissing)
    ));
    assert_eq!(engine.last_reducer_tick().get(), 0);
}

#[test]
fn an_enrolled_superseded_host_must_submit_before_the_frame_can_finish() {
    let mut engine = engine();
    let rendering_host = engine
        .create_presentation_host()
        .expect("rendering host lease must mint");
    let superseded_host = engine
        .create_presentation_host()
        .expect("superseded host lease must mint");
    let mut prelude = engine
        .begin_host_frame_with_observers(rendering_host, [superseded_host])
        .expect("observer scope must freeze");
    prelude
        .submit_presentation_observation(
            dockspace::presentation_observation::HostPresentationObservation::NoUpdate,
        )
        .expect("rendering host observation must submit");

    assert!(matches!(
        prelude.seal(&engine),
        Err(EngineError::HostFrameSupplementaryPresentationObservationMissing { host })
            if host == superseded_host
    ));
    assert_eq!(engine.last_reducer_tick().get(), 0);
}

#[test]
fn contribution_roster_remains_exact_after_the_observation_barrier() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let token = engine
        .begin_surface_contribution(SURFACE)
        .expect("surface is rostered");
    let prepared = engine
        .prepare_surface_contribution(
            token,
            support::measurements(
                &engine,
                SURFACE,
                bounds(640.0, 480.0),
                support::MeasurementProfile::default(),
            ),
        )
        .expect("complete measurements prepare");
    let mut frame = host.begin(&engine);
    frame
        .push_surface_contribution(prepared)
        .expect("one contribution is accepted");
    complete_host_frame_with_unavailable(&engine, &mut frame);
    let transition = host.finish(frame, &mut engine);
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Ready {
            surface: SURFACE,
            ..
        }]
    ));
}

#[test]
fn publish_helper_exercises_the_complete_three_boundary_protocol() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    let stamp = support::publish_surface(&mut engine, &mut host, SURFACE, bounds(960.0, 540.0));

    assert_eq!(
        engine
            .scene()
            .ready_surface(SURFACE)
            .map(|ready| ready.stamp()),
        Some(stamp)
    );
    let authority = ready_authority(&engine);
    assert_eq!(authority.emission().stream(), authority.stream());
}

#[test]
fn retirement_discards_pending_outputs_and_terminates_the_complete_host_roster() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    support::install_surface_projection(&mut engine, &mut host, SURFACE, bounds(640.0, 480.0));

    let emit = host.begin(&engine);
    let emit = complete_host_frame_with_current_outputs(&engine, emit);
    let transition = host.finish_presentation(emit, &mut engine);
    let output = transition.presentation_emissions()[0].output();
    let stream = output.stream();
    assert_eq!(host.pending_output_count(), 1);
    let before = engine.presentation_ledger_diagnostics();
    assert_eq!(before.live_hosts(), 1);
    assert_eq!(before.active_streams(), 1);
    assert_eq!(before.pending_outputs(), 1);
    assert!(
        engine
            .presentation_retention_manifest()
            .retains_stream(stream)
    );

    let retired = host
        .close(&mut engine)
        .expect("terminal host retirement must publish");
    assert!(matches!(
        retired,
        PresentationHostRetirementOutcome::Retired {
            retired_stream_count: 1,
            retired_output_count: 1,
            ref released_active_surfaces,
            ref affected_surfaces,
            ..
        } if released_active_surfaces == &[SURFACE] && affected_surfaces.is_empty()
    ));

    let after = engine.presentation_ledger_diagnostics();
    assert_eq!(after.live_hosts(), 0);
    assert_eq!(after.retired_hosts(), 0);
    assert_eq!(after.active_streams(), 0);
    assert_eq!(after.terminated_streams(), 0);
    assert_eq!(after.active_surface_owners(), 0);
    assert_eq!(after.pending_streams(), 0);
    assert_eq!(after.pending_outputs(), 0);
    assert_eq!(after.retained_host_states(), 0);
    assert_eq!(after.retained_stream_states(), 0);
    assert_eq!(after.compacted_retired_hosts(), 1);
    assert_eq!(after.compacted_retirement_ranges(), 1);
    assert!(
        !engine
            .presentation_retention_manifest()
            .retains_stream(stream),
        "adapter stream sidecars become reclaimable with detailed host compaction",
    );
    assert_eq!(
        engine
            .scene()
            .surface(SURFACE)
            .and_then(SurfaceScene::ready)
            .map(|ready| ready.output_ticket()),
        match output.payload() {
            dockspace::presentation_observation::HostPresentationOutputPayload::Paint {
                scene: ticket,
                ..
            } => {
                Some(ticket)
            }
            _ => None,
        }
    );
}

#[test]
fn retirement_revokes_exact_interaction_authority_but_retains_paint_output() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, bounds(640.0, 480.0));
    let authority = ready_authority(&engine);
    let paint_ticket = engine
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::paint_projection)
        .expect("observed output remains paintable")
        .output_ticket();

    let outcome = host
        .close(&mut engine)
        .expect("authoritative host retirement must publish");
    let PresentationHostRetirementOutcome::Retired {
        affected_surfaces,
        transition,
        ..
    } = outcome
    else {
        panic!("first retirement must publish");
    };
    assert_eq!(affected_surfaces, vec![SURFACE]);
    assert!(transition.published_state_changed());
    assert!(engine.scene().ready_surface(SURFACE).is_none());
    let retained = engine
        .scene()
        .surface(SURFACE)
        .and_then(SurfaceScene::paint_projection)
        .expect("retirement must retain the exact paint candidate or fallback");
    assert_eq!(retained.output_ticket(), paint_ticket);
    assert!(engine.interaction_projection(SURFACE).is_none());
    assert_eq!(authority.surface(), SURFACE);
}

#[test]
fn superseded_host_retirement_cannot_revoke_the_current_hosts_authority() {
    let mut engine = engine();
    let mut old_host = TestPresentationHost::new(&mut engine);
    support::install_surface_projection(&mut engine, &mut old_host, SURFACE, bounds(640.0, 480.0));
    let old_emit = old_host.begin(&engine);
    let old_emit = complete_host_frame_with_current_outputs(&engine, old_emit);
    old_host.finish_presentation(old_emit, &mut engine);

    let mut current_host = TestPresentationHost::new(&mut engine);
    let current_emit = current_host.begin(&engine);
    let current_emit = complete_host_frame_with_current_outputs(&engine, current_emit);
    current_host.finish_presentation(current_emit, &mut engine);
    let mut current_observe = current_host.begin(&engine);
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut current_observe);
    current_host.finish(current_observe, &mut engine);
    let current_authority = ready_authority(&engine);

    let outcome = old_host
        .close(&mut engine)
        .expect("superseded host retirement must publish");
    assert!(matches!(
        outcome,
        PresentationHostRetirementOutcome::Retired {
            retired_output_count: 1,
            ref released_active_surfaces,
            ref affected_surfaces,
            ..
        } if released_active_surfaces.is_empty() && affected_surfaces.is_empty()
    ));
    assert_eq!(ready_authority(&engine), current_authority);
    let diagnostics = engine.presentation_ledger_diagnostics();
    assert_eq!(diagnostics.live_hosts(), 1);
    assert_eq!(diagnostics.retired_hosts(), 0);
    assert_eq!(diagnostics.compacted_retired_hosts(), 1);
    assert_eq!(diagnostics.compacted_retirement_ranges(), 1);
    assert_eq!(diagnostics.active_surface_owners(), 1);
}

#[test]
fn retirement_is_idempotent_and_rejects_every_late_host_entry_point() {
    let mut engine = engine();
    let host = engine
        .create_presentation_host()
        .expect("test host lease must mint");
    let mut late = engine
        .begin_host_frame(host)
        .expect("host frame freezes before retirement");
    late.submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("pre-retirement observation stages");

    let first = engine
        .retire_presentation_host(host, PresentationHostRetirementReason::RuntimeDestroyed)
        .expect("first retirement publishes");
    let first_tick = first
        .transition()
        .expect("first retirement has a transition")
        .tick();
    assert_eq!(engine.last_reducer_tick(), first_tick);

    assert!(matches!(
        late.seal(&engine),
        Err(EngineError::PresentationHostRetiredCompacted { host: rejected })
            if rejected == host
    ));
    assert!(matches!(
        engine.begin_host_frame(host),
        Err(EngineError::PresentationHostRetiredCompacted { host: rejected })
            if rejected == host
    ));

    let duplicate = engine
        .retire_presentation_host(host, PresentationHostRetirementReason::AdapterDetached)
        .expect("duplicate retirement is idempotent");
    assert!(matches!(
        duplicate,
        PresentationHostRetirementOutcome::Compacted { host: repeated }
            if repeated == host
    ));
    assert_eq!(engine.last_reducer_tick(), first_tick);
}

#[test]
fn active_backend_blocks_retirement_compaction_until_its_ingress_is_revoked() {
    let mut engine = engine();
    let host = engine
        .create_presentation_host()
        .expect("presentation host must mint");
    let backend = engine
        .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
        .expect("joined backend must enroll");

    engine
        .retire_presentation_host(host, PresentationHostRetirementReason::RuntimeDestroyed)
        .expect("semantic host retirement must publish");
    let blocked = engine.presentation_ledger_diagnostics();
    assert_eq!(blocked.retired_hosts(), 1);
    assert_eq!(blocked.compacted_retired_hosts(), 0);
    assert_eq!(blocked.retained_host_states(), 1);
    assert!(matches!(
        engine
            .retire_presentation_host(host, PresentationHostRetirementReason::AdapterDetached)
            .expect("duplicate retirement remains observable while detailed"),
        PresentationHostRetirementOutcome::AlreadyRetired { host: repeated, .. }
            if repeated == host
    ));

    let _replacement = engine
        .begin_backend_ingress_provider_replacement(backend.drain())
        .expect("provider replacement revokes the blocked ingress lanes");
    let compacted = engine.presentation_ledger_diagnostics();
    assert_eq!(compacted.retired_hosts(), 0);
    assert_eq!(compacted.compacted_retired_hosts(), 1);
    assert_eq!(compacted.compacted_retirement_ranges(), 1);
    assert_eq!(compacted.retained_host_states(), 0);
}

#[test]
fn ten_thousand_engine_retirements_bound_detailed_state_and_preserve_a_live_hole() {
    let mut engine = engine();
    let live = engine
        .create_presentation_host()
        .expect("live presentation host must mint");

    for _ in 2..=10_000 {
        let host = engine
            .create_presentation_host()
            .expect("retiring presentation host must mint");
        engine
            .retire_presentation_host(host, PresentationHostRetirementReason::ExplicitShutdown)
            .expect("unblocked retirement must compact at the engine boundary");
    }

    let with_hole = engine.presentation_ledger_diagnostics();
    assert_eq!(with_hole.live_hosts(), 1);
    assert_eq!(with_hole.retired_hosts(), 0);
    assert_eq!(with_hole.retained_host_states(), 1);
    assert_eq!(with_hole.retained_stream_states(), 0);
    assert_eq!(with_hole.compacted_retired_hosts(), 9_999);
    assert_eq!(with_hole.compacted_retirement_ranges(), 1);
    let live_frame = engine
        .begin_host_frame(live)
        .expect("the exact serial hole remains live");

    engine
        .retire_presentation_host(live, PresentationHostRetirementReason::ExplicitShutdown)
        .expect("retiring the live hole must merge both sides");
    let merged = engine.presentation_ledger_diagnostics();
    assert_eq!(merged.retained_host_states(), 0);
    assert_eq!(merged.compacted_retired_hosts(), 10_000);
    assert_eq!(merged.compacted_retirement_ranges(), 1);
    assert!(matches!(
        live_frame.seal(&engine),
        Err(EngineError::PresentationHostRetiredCompacted { host: rejected })
            if rejected == live
    ));
}

#[test]
fn persistent_test_host_does_not_grow_the_live_host_or_stream_roster() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, bounds(640.0, 480.0));
    let baseline = engine.presentation_ledger_diagnostics();

    for _ in 0..16 {
        let mut frame = host.begin(&engine);
        support::complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
        let transition = host.finish(frame, &mut engine);
        assert!(transition.presentation_emissions().is_empty());
        assert_eq!(host.pending_output_count(), 0);
    }

    let after = engine.presentation_ledger_diagnostics();
    assert_eq!(after.live_hosts(), 1);
    assert_eq!(after.live_hosts(), baseline.live_hosts());
    assert_eq!(after.active_streams(), baseline.active_streams());
    assert_eq!(after.retiring_streams(), baseline.retiring_streams());
    assert_eq!(after.terminated_streams(), baseline.terminated_streams());
    assert_eq!(after.pending_streams(), 0);
    assert_eq!(after.pending_outputs(), 0);
}

#[test]
fn provider_sidecar_clears_only_an_observation_the_core_actually_retired() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    support::install_surface_projection(&mut engine, &mut host, SURFACE, bounds(640.0, 480.0));
    let emit = host.begin(&engine);
    let emit = complete_host_frame_with_current_outputs(&engine, emit);
    host.finish_presentation(emit, &mut engine);
    assert_eq!(host.pending_output_count(), 1);

    let mut unknown_prelude = engine
        .begin_host_frame(host.lease())
        .expect("unknown observation frame begins");
    host.submit_unknown_observation(&mut unknown_prelude);
    let mut unknown = unknown_prelude
        .seal(&engine)
        .expect("unknown observation frame seals");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut unknown);
    let unknown = host.finish(unknown, &mut engine);
    assert!(matches!(
        unknown.presentation_observations(),
        [HostPresentationObservationOutcome::CapturedUnknown { .. }]
    ));
    assert_eq!(host.pending_output_count(), 1);
    assert_eq!(
        engine.presentation_ledger_diagnostics().pending_outputs(),
        1
    );

    let mut rejected_prelude = engine
        .begin_host_frame(host.lease())
        .expect("replayed observation frame begins");
    host.submit_replayed_retirement_observation(&mut rejected_prelude);
    let mut rejected = rejected_prelude
        .seal(&engine)
        .expect("replayed observation frame seals");
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut rejected);
    let rejected = host.finish(rejected, &mut engine);
    assert!(matches!(
        rejected.presentation_observations(),
        [HostPresentationObservationOutcome::Rejected {
            reason: HostPresentationObservationRejection::CaptureGenerationNotIncreasing { .. },
            ..
        }]
    ));
    assert_eq!(host.pending_output_count(), 1);
    assert_eq!(
        engine.presentation_ledger_diagnostics().pending_outputs(),
        1
    );

    let mut accepted = host.begin(&engine);
    support::complete_host_frame_with_retained_or_unavailable(&engine, &mut accepted);
    let accepted = host.finish(accepted, &mut engine);
    assert!(matches!(
        accepted.presentation_observations(),
        [HostPresentationObservationOutcome::Retired { .. }]
    ));
    assert_eq!(host.pending_output_count(), 0);
    assert_eq!(
        engine.presentation_ledger_diagnostics().pending_outputs(),
        0
    );
}

#[test]
fn inactive_surface_presentation_does_not_wait_for_no_update_or_unknown_siblings() {
    fn run(second_unknown: bool) {
        let (mut engine, _) = two_surface_engine();
        let mut host = TestPresentationHost::new(&mut engine);

        let mut measurement = host.begin(&engine);
        for surface in [SURFACE, TARGET_SURFACE] {
            let token = engine
                .begin_surface_contribution(surface)
                .expect("surface measurement begins");
            let contribution = engine
                .prepare_surface_contribution(
                    token,
                    support::measurements(
                        &engine,
                        surface,
                        bounds(420.0, 260.0),
                        support::MeasurementProfile::default(),
                    ),
                )
                .expect("surface measurement prepares");
            measurement
                .push_surface_contribution(contribution)
                .expect("surface contributes once");
        }
        host.finish(measurement, &mut engine);

        let emit = host.begin(&engine);
        let emit = complete_host_frame_with_current_outputs(&engine, emit);
        host.finish_presentation(emit, &mut engine);
        assert_eq!(host.pending_output_count(), 2);

        let mut partial_prelude = engine
            .begin_host_frame(host.lease())
            .expect("partial observation frame begins");
        let unknown = second_unknown
            .then_some(TARGET_SURFACE)
            .into_iter()
            .collect::<BTreeSet<_>>();
        host.submit_partitioned_observation(
            &mut partial_prelude,
            &BTreeSet::from([SURFACE]),
            &unknown,
        );
        let mut partial = partial_prelude
            .seal(&engine)
            .expect("partitioned observation frame seals");
        support::complete_host_frame_with_retained_or_unavailable(&engine, &mut partial);
        let partial = host.finish(partial, &mut engine);

        assert!(engine.interaction_projection(SURFACE).is_some());
        assert!(engine.interaction_projection(TARGET_SURFACE).is_none());
        assert_eq!(host.pending_output_count(), 1);
        assert!(partial.presentation_observations().iter().any(|outcome| {
            matches!(
                outcome,
                HostPresentationObservationOutcome::NoUpdate { .. }
                    | HostPresentationObservationOutcome::CapturedUnknown { .. }
            )
        }));

        let mut complete = host.begin(&engine);
        support::complete_host_frame_with_retained_or_unavailable(&engine, &mut complete);
        host.finish(complete, &mut engine);
        assert!(engine.interaction_projection(SURFACE).is_some());
        assert!(engine.interaction_projection(TARGET_SURFACE).is_some());
    }

    run(false);
    run(true);
}
