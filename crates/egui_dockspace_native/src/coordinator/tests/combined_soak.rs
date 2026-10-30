//! Reuses one bounded coordinator through model callbacks, without OS windows.

use super::owner_storage::{combined_profile, layout};
use super::*;

const LIFETIME_CYCLES: u64 = 10_000;
const TOTAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct CapacityPeaks {
    resident: usize,
    reserved: usize,
}

impl CapacityPeaks {
    fn observe(&mut self, native: &NativeCoordinator, cycle: u64) -> (usize, usize) {
        let resident = native
            .owned_input_capacity_bytes()
            .expect("the current input owners remain measurable");
        let reserved = native
            .combined_storage_reserved_bytes()
            .expect("the same combined allowance remains selected");
        assert!(
            resident <= reserved && reserved <= TOTAL_BYTES,
            "cycle {cycle}: resident={resident}, reserved={reserved}, limit={TOTAL_BYTES}"
        );
        self.resident = self.resident.max(resident);
        self.reserved = self.reserved.max(reserved);
        (resident, reserved)
    }
}

fn settle_report(native: &mut NativeCoordinator, mut report: HostFrameReport) -> HostFrameReport {
    assert!(
        !report.native_input_pending(),
        "each model boundary must commit its complete accepted prefix"
    );
    assert!(
        report.take_native_effects().is_empty(),
        "external root lifetimes must not leave an unconsumed native request"
    );
    native
        .settle_host_frame_report(&report)
        .expect("the exact queued snapshot settles after full commit");
    report
}

fn commit_boundary(native: &mut NativeCoordinator) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the model's next native boundary begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the model settles every surface without a renderer");
    let report = frame
        .commit()
        .expect("the complete native boundary commits");
    settle_report(native, report)
}

fn reset_layout(
    native: &mut NativeCoordinator,
    replacement: DockspaceLayout,
    drop_candidate: bool,
) {
    let reset = native
        .session
        .prepare_layout_reset(replacement)
        .expect("the application prepares its next layout on the same session");
    if drop_candidate {
        let version = native.session.version();
        let recorded = native.session.native_input_recorded_count();
        let mut frame = native
            .session
            .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
            .expect("the original reset candidate begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the dropped reset candidate settles every surface");
        drop(frame);
        assert_eq!(native.session.version(), version);
        assert_eq!(native.session.native_input_recorded_count(), recorded);
    }
    let mut frame = native
        .session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .expect("the same prepared reset remains available for its exact retry");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the layout replacement settles every surface");
    let report = frame.commit().expect("the layout replacement commits");
    // The public session reset supplies the core frame; settle the existing
    // bridge boundary just as NativeHostFrame does for ordinary commits.
    native
        .bridge
        .commit_frame_boundary(report.native_input_pending());
    drop(settle_report(native, report));
}

#[test]
fn combined_model_reuses_one_coordinator_for_ten_thousand_quiescent_lifetimes() {
    let profile = combined_profile();
    assert_eq!(profile.byte_limit, TOTAL_BYTES);
    let mut native = NativeCoordinator::new_with_combined_storage_limit_and_clock(
        DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        profile,
        dockspace::runtime::NativeMonotonicClock::new(|| std::time::Duration::ZERO),
    )
    .expect("the complete combined profile enrolls once");
    let context = egui::Context::default();
    let mut peaks = CapacityPeaks::default();
    let mut predecessor: Option<NativeWindowEventRecord> = None;
    let mut idle_capacity = None;
    let mut dropped_candidates = 0;
    let mut dropped_retirements = 0;
    let mut late_callbacks = 0;
    let producer_capacity = native
        .session
        .native_input_producer_storage_capacity_bytes();
    peaks.observe(&native, 0);

    for cycle in 1..=LIFETIME_CYCLES {
        // Reuse the stable application item, viewport and numeric window token.
        // Public reset rebases retired surface identities; resolve today's surface
        // through the same application item before registering its next lifetime.
        let surface = native
            .session()
            .view()
            .item(ItemId::new(1))
            .expect("the stable application item remains present")
            .surface();
        native
            .register_native_root(surface, HostWindowToken::new(11))
            .expect("quiescence must return the slot for the next registration");
        let report = commit_boundary(&mut native);
        let binding = match report.native_bindings() {
            [binding] => *binding,
            bindings => panic!("cycle {cycle}: expected one registered root, got {bindings:?}"),
        };
        assert_eq!(binding.surface(), surface);
        assert_eq!(binding.window_token(), HostWindowToken::new(11));
        if let Some(old) = &predecessor {
            let previous = old.binding().expect("the predecessor has an exact binding");
            assert!(!binding.same_window_lifetime(previous));
            assert!(!native.session.recognizes_native_binding(previous));
        }
        drop(report);
        let window = WindowId::from(10_000 + cycle);
        native
            .bind_viewport(ViewportId::ROOT, window, binding)
            .expect("the next exact incarnation reuses the root viewport");

        if cycle > 1 && cycle % 113 == 0 {
            let captured = native.bridge.captured_through_for_test();
            let recorded = native.session.native_input_recorded_count();
            let old = predecessor
                .as_ref()
                .expect("a completed predecessor supplies the late callback");
            native
                .bridge
                .push_record(HostRecord::WindowEvent(old.clone()));
            assert_eq!(native.bridge.captured_through_for_test(), captured);
            assert_eq!(native.session.native_input_recorded_count(), recorded);
            assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(binding));
            assert!(native.session.is_current_native_binding(binding));
            assert!(!native.bridge.has_pending_callback_records());
            late_callbacks += 1;
        }
        peaks.observe(&native, cycle);

        let destroyed = native.bridge.with_viewports_mut(|routes| {
            NativeWindowEventRecord::for_test_ingress(
                cycle,
                window,
                Some(ViewportId::ROOT),
                WindowEvent::Destroyed,
                routes,
            )
        });
        assert_eq!(destroyed.binding(), Some(binding));
        native
            .bridge
            .push_record(HostRecord::WindowEvent(destroyed.clone()));
        assert_eq!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap(),
            Some(std::num::NonZeroUsize::MIN),
            "cycle {cycle}: the exact destruction callback must transfer once"
        );
        assert_eq!(native.lifecycle_progress().destroyed_observations(), cycle);
        if cycle % 97 == 0 {
            let version = native.session.version();
            let recorded = native.session.native_input_recorded_count();
            let mut frame = native
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .expect("the accepted destruction prefix builds a candidate");
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(frame);
            assert_eq!(native.session.version(), version);
            assert_eq!(native.session.native_input_recorded_count(), recorded);
            assert!(native.bridge.callback_boundary_pending());
            assert!(native.session.recognizes_native_binding(binding));
            dropped_candidates += 1;
            peaks.observe(&native, cycle);
        }
        drop(commit_boundary(&mut native));
        peaks.observe(&native, cycle);

        // Removing the root through the public reset retires core authority.
        // The physical receipt must still publish its real destroyed tombstone.
        let drop_reset = cycle % 257 == 0;
        reset_layout(&mut native, DockspaceLayout::empty(), drop_reset);
        dropped_candidates += u64::from(drop_reset);
        assert!(!native.session.is_current_native_binding(binding));
        assert!(native.session.recognizes_native_binding(binding));
        assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(binding));
        native
            .bridge
            .with_retirement_destroyed_observations(|count, entries, _, _| {
                assert_eq!(count, 1);
                assert_eq!(
                    entries.next(),
                    Some((binding, NativeWindowFacts::destroyed()))
                );
                assert!(entries.next().is_none());
            });
        native
            .bridge
            .push_viewport_roster(live_roster(std::iter::empty()));
        assert_eq!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap(),
            Some(std::num::NonZeroUsize::MIN)
        );
        peaks.observe(&native, cycle);
        let report = commit_boundary(&mut native);
        assert!(
            report.inputs().iter().any(|input| matches!(
                input,
                HostInputOutcome::NativePlatformSnapshotApplied { .. }
            )),
            "cycle {cycle}: the full roster must accept the retired binding's tombstone"
        );
        drop(report);
        assert!(native.queued_snapshot.is_none());

        let prepared = native
            .prepare_committed_retirements()
            .unwrap()
            .expect("the committed tombstone permits exact route retirement");
        let prepared = if cycle % 127 == 0 {
            drop(prepared);
            assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(binding));
            assert!(!native.try_report_retirement_quiescence().unwrap());
            dropped_retirements += 1;
            native
                .prepare_committed_retirements()
                .unwrap()
                .expect("dropping the preparation must preserve its exact retry")
        } else {
            prepared
        };
        drop(commit_boundary(&mut native));
        assert!(native.commit_retirements(prepared).is_empty());
        assert_eq!(native.viewport_binding(ViewportId::ROOT), None);
        assert!(native.session.recognizes_native_binding(binding));
        peaks.observe(&native, cycle);

        let recorded = native.session.native_input_recorded_count();
        assert!(
            native.try_report_retirement_quiescence().unwrap(),
            "cycle {cycle}: terminal owners must permit quiescence without changing any quota"
        );
        assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
        assert!(!native.session.recognizes_native_binding(binding));
        assert_eq!(native.lifecycle_progress().quiescent_retirements(), cycle);
        assert!(!native.try_report_retirement_quiescence().unwrap());
        assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
        drop(commit_boundary(&mut native));

        // Beginning the next public reset also reclaims the committed quiescence
        // prefix. No report or owner history is retained by the soak fixture.
        reset_layout(&mut native, layout(), false);
        assert!(!native.session.has_pending_native_input());
        assert!(
            native.is_quiescent(),
            "cycle {cycle}: a terminal owner leaked"
        );
        assert_eq!(
            native
                .bridge
                .with_viewports(|routes| routes.bindings().count()),
            0
        );
        assert!(!native.bridge.retirement_has_pending_work());
        assert!(!native.binding_has_retained_host_references(binding));
        assert_eq!(
            native
                .session
                .native_input_producer_storage_capacity_bytes(),
            producer_capacity,
            "cycle {cycle}: quiescence keeps the original prepaid producer backing"
        );
        let current_idle = peaks.observe(&native, cycle);
        if let Some(previous_idle) = idle_capacity {
            assert_eq!(
                current_idle, previous_idle,
                "cycle {cycle}: idle storage must plateau"
            );
        } else {
            idle_capacity = Some(current_idle);
        }
        predecessor = Some(destroyed);
    }

    assert_eq!(
        native.lifecycle_progress().destroyed_observations(),
        LIFETIME_CYCLES
    );
    assert_eq!(
        native.lifecycle_progress().quiescent_retirements(),
        LIFETIME_CYCLES
    );
    assert!(dropped_candidates > 0 && dropped_retirements > 0 && late_callbacks > 0);
    eprintln!(
        "model lifetimes={LIFETIME_CYCLES}, dropped_candidates={dropped_candidates}, \
         dropped_retirements={dropped_retirements}, late_callbacks={late_callbacks}, \
         peak_resident={}, peak_reserved={}",
        peaks.resident, peaks.reserved
    );
}
