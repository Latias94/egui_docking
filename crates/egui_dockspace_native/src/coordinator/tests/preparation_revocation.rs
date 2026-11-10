//! Real preparation state joins the coordinator's committed snapshot barrier.

use dockspace::model::NativeWindowPlacement;
use dockspace::runtime::{NativeCreateReadiness, PaintedSurfaceOutput, WorkspaceVersion};

use super::*;
use crate::window_snapshot::CompiledWindowObservation;
use crate::work_area::FrozenWorkAreaRoster;

fn preparation(
    native: &NativeCoordinator,
    child: NativeSurfaceBinding,
) -> NativePreparationObservation {
    native
        .bridge
        .with_retirement_destroyed_observations(|_, _, _, preparation| {
            preparation(child).expect("the child owns preparation")
        })
}

fn queue_snapshot(
    native: &mut NativeCoordinator,
    host: NativeSurfaceBinding,
    source: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
) -> NativePreparationObservation {
    let observation = preparation(native, child);
    let created = native
        .bridge
        .with_retirement_destroyed_observations(|_, _, created, _| {
            created(child).expect("the child retains its creation fact")
        });
    let observations = [
        CompiledWindowObservation::new(host, ready_window_facts(0.0)),
        CompiledWindowObservation::new(source, ready_window_facts(720.0)),
        CompiledWindowObservation::new(child, ready_window_facts(1400.0))
            .with_preparation(observation)
            .with_created(created),
    ];
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(observations, true));
    let roster = native.bridge.front_viewport_roster().unwrap();
    // The tokenless roster fixture models captured window facts. Queue its existing
    // sidecar directly so its empty raw roster cannot revoke real preparation state.
    let (work_areas, prepared_work_areas) = native
        .work_areas
        .prepare(&FrozenWorkAreaRoster::Unknown)
        .unwrap();
    native
        .session
        .report_managed_native_snapshot(
            observations.map(|value| (value.binding(), value.facts())),
            work_areas,
        )
        .unwrap();
    assert!(native.queued_snapshot.is_none());
    native.queued_snapshot = Some(QueuedNativeSnapshot {
        settlement: None,
        roster: roster.clone(),
        acknowledgements: vec![NativeSnapshotAcknowledgements {
            binding: child,
            create: created.is_some(),
            preparation: Some(observation),
        }],
        input_acknowledgement: None,
        focus_reportable: vec![host, source, child],
        work_areas: prepared_work_areas,
    });
    assert!(native.bridge.acknowledge_viewport_roster(&roster));
    observation
}

fn commit(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    native.settle_host_frame_report(&report).unwrap();
    report
}

fn paint_child(
    native: &mut NativeCoordinator,
    child: NativeSurfaceBinding,
) -> PaintedSurfaceOutput {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface(
            child.surface(),
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let receipt = frame
        .frame
        .paint_plan(child.surface())
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    frame.frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap();
    output
}

fn assert_source(
    native: &NativeCoordinator,
    source: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    version: WorkspaceVersion,
) {
    assert_eq!(native.session.version(), version);
    let view = native.session.view();
    let item = view.item(ItemId::new(2)).unwrap();
    assert_eq!(
        (item.surface(), item.root()),
        (source.surface(), RootId::new(2))
    );
    assert!(view.surface(child.surface()).is_none());
    assert!(native.session.presented_surface(child.surface()).is_none());
    assert!(native.session.has_pending_native_create());
}

#[test]
fn renderer_revocation_waits_for_committed_created_then_fresh_prepared_and_first_live() {
    let mut native = coordinator();
    let (host, source) = register_root_and_child(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), host)
        .unwrap();
    native
        .bind_viewport(viewport_id_for(source), WindowId::from(22), source)
        .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.replace_policy(policy).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    paint_and_present_all_surfaces(&mut native);
    let version = native.session.version();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .move_root_to_new_window_current(
            RootId::new(2),
            NativeWindowPlacement::new(PhysicalRect::new(1400.0, 0.0, 640.0, 480.0).unwrap()),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let [create] = report.take_native_effects().try_into().unwrap();
    let child = create.operation().binding();
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the exact create owns its acknowledgement");
    };
    let viewport = viewport_id_for(child);
    let window = WindowId::from(33);
    native.bind_viewport(viewport, window, child).unwrap();
    assert!(native.bridge.reserve_create(
        viewport,
        child,
        NativePhysicalRect::new(1400, 0, 640, 480)
    ));
    native
        .bridge
        .retain_create_ack(viewport, child, created)
        .unwrap();
    assert!(native.bridge.clear_create(viewport, child));
    native
        .bridge
        .ready_window_preparation_for_test(viewport, child, window);
    assert_eq!(
        queue_snapshot(&mut native, host, source, child).readiness,
        NativeCreateReadiness::Prepared
    );
    assert!(commit(&mut native).native_admissions().is_empty());
    let old_output = paint_child(&mut native, child);
    let old_artifact = old_output.presentation_artifact();
    assert_source(&native, source, child, version);

    let bridge = Arc::clone(&native.bridge);
    bridge.lose_preparation_renderer_for_test(viewport, child);
    let revoked = queue_snapshot(&mut native, host, source, child);
    assert_eq!(revoked.readiness, NativeCreateReadiness::Created);
    let recorded = native.session.native_input_recorded_count();
    let mut dropped = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(dropped.frame.paint_plan(child.surface()).unwrap().is_none());
    bridge.present_window_preparation_for_test(viewport, child);
    dropped
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(dropped);
    assert_eq!(preparation(&native, child), revoked);
    assert!(native.queued_snapshot.is_some());
    assert!(native.session.has_pending_native_input());
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_source(&native, source, child, version);

    let mut retry = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(retry.frame.paint_plan(child.surface()).unwrap().is_none());
    retry
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = retry.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    assert_eq!(
        preparation(&native, child),
        revoked,
        "core commit alone cannot clear the adapter barrier"
    );
    assert!(native.settle_host_frame_report(&report).unwrap());
    assert!(native.queued_snapshot.is_none());
    let prepared = preparation(&native, child);
    assert_eq!(prepared.generation, revoked.generation);
    assert_eq!(prepared.readiness, NativeCreateReadiness::Prepared);
    assert_source(&native, source, child, version);

    assert_eq!(queue_snapshot(&mut native, host, source, child), prepared);
    assert!(commit(&mut native).native_admissions().is_empty());
    assert_source(&native, source, child, version);
    native
        .session
        .report_surface_presentation(old_output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = commit(&mut native);
    assert!(
        report.native_admissions().is_empty(),
        "the old output is below the renewed readiness floor"
    );
    assert_source(&native, source, child, version);
    let output = paint_child(&mut native, child);
    assert_ne!(output.presentation_artifact(), old_artifact);
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = commit(&mut native);
    assert_eq!(report.native_admissions(), &[child]);
    assert!(
        native
            .settle_native_admissions(report.native_admissions())
            .unwrap()
    );
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .surface(),
        child.surface()
    );
    assert!(!native.session.has_pending_native_create());
}
