//! Reuses one bounded coordinator through model callbacks, without OS windows.

use super::owner_storage::{combined_profile, layout};
use super::*;
use crate::focus_control::NativeViewportFocusRecord;

#[cfg(target_os = "linux")]
#[path = "combined_soak/output_envelopes.rs"]
mod output_envelopes;

const LIFETIME_CYCLES: u64 = 10_000;
const TOTAL_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
struct CapacityPeaks {
    resident: usize,
    reserved: usize,
    queued_focus: usize,
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
        self.queued_focus = self
            .queued_focus
            .max(usize::from(native.focus_control.queued_route().is_some()));
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

#[test]
fn configured_maximum_roster_and_terminal_fanout_survive_ordinary_freeze_and_retry() {
    use eframe::{NativeHostHandler, NativeHostWake, NativeViewportPropertyCommandRetirement};
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, PointerEventFacts};

    const WINDOWS: usize = 16;
    let profile = combined_profile();
    assert_eq!(
        profile
            .input
            .producer_storage_limit()
            .unwrap()
            .binding_slots()
            .get(),
        WINDOWS
    );
    let layout = DockspaceLayout::new((1..=WINDOWS as u64).map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap();
    let mut native = NativeCoordinator::new_with_combined_storage_limit_and_clock(
        DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        profile,
        dockspace::runtime::NativeMonotonicClock::new(|| std::time::Duration::ZERO),
    )
    .unwrap();
    let context = egui::Context::default();
    let mut peaks = CapacityPeaks::default();
    for id in 1..=WINDOWS as u64 {
        native
            .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
            .unwrap();
    }
    let report = commit_boundary(&mut native);
    let bindings = report.native_bindings().to_vec();
    assert_eq!(bindings.len(), WINDOWS);
    drop(report);

    // All configured lifetimes coexist. The fixture issues real fork command
    // identities but models their retirement; it creates no OS windows or output tokens.
    let mut routes = Vec::new();
    let mut commands = Vec::new();
    let mut retirements = Vec::new();
    for (index, binding) in bindings.iter().copied().enumerate() {
        let viewport = if index == 0 {
            ViewportId::ROOT
        } else {
            ViewportId::from_hash_of(("maximum-roster", index))
        };
        let window = WindowId::from(100 + index as u64);
        native.bind_viewport(viewport, window, binding).unwrap();
        let focus = eframe::prepare_native_viewport_focus(viewport);
        let visibility = eframe::prepare_native_viewport_visibility(viewport, true);
        assert!(
            native
                .bridge
                .reserve_focus_callback(viewport, binding, window, focus.token())
        );
        assert!(native.bridge.reserve_visibility_callback(
            viewport,
            binding,
            window,
            visibility.token(),
            true,
        ));
        retirements.extend([
            NativeViewportPropertyCommandRetirement::Focus {
                viewport_id: viewport,
                token: focus.token(),
            },
            NativeViewportPropertyCommandRetirement::Visibility {
                viewport_id: viewport,
                token: visibility.token(),
                visible: true,
            },
        ]);
        commands.push((focus, visibility));
        routes.push((viewport, window, binding));
        peaks.observe(&native, 0);
    }
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.bindings().count()),
        WINDOWS
    );
    native
        .bridge
        .push_viewport_roster(live_roster(bindings.iter().copied()));
    assert_eq!(
        native
            .bridge
            .front_viewport_roster()
            .unwrap()
            .roster()
            .observation_count(),
        WINDOWS
    );
    let (_, window, binding) = routes[0];
    let mut accepted = 0;
    for ordinal in 1..=4_096 {
        let before = native.bridge.captured_through_for_test();
        native
            .bridge
            .push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                ordinal,
                window,
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(ordinal as f64, 20.0),
                    facts: PointerEventFacts::default(),
                },
            )));
        peaks.observe(&native, 0);
        if native.bridge.captured_through_for_test() == before {
            break;
        }
        accepted += 1;
    }
    assert!((1..4_096).contains(&accepted));
    let frozen = native.bridge.captured_through_for_test();
    for &retirement in &retirements {
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                retirement,
            ),
            NativeHostWake::RepaintRootAfterCurrent
        );
        peaks.observe(&native, 0);
    }
    for (index, &(viewport, window, _)) in routes.iter().enumerate() {
        let destroyed = native.bridge.with_viewports_mut(|routes| {
            NativeWindowEventRecord::for_test_ingress(
                10_000 + index as u64,
                window,
                Some(viewport),
                WindowEvent::Destroyed,
                routes,
            )
        });
        native
            .bridge
            .push_record(HostRecord::WindowEvent(destroyed));
        peaks.observe(&native, 0);
    }
    assert_eq!(
        native.bridge.captured_through_for_test(),
        frozen + retirements.len() as u64 + WINDOWS as u64,
        "every reserved terminal remains admissible after ordinary ingress freezes"
    );

    let mut dropped = false;
    let mut retried_roster_commits = 0;
    let mut cancellations = 0;
    for _ in 0..128 {
        let mut budget = test_budget();
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap();
        let queued_roster = native
            .queued_snapshot
            .as_ref()
            .map(|queued| queued.roster.clone());
        let version = native.session.version();
        let recorded = native.session.native_input_recorded_count();
        let Some(mut frame) = native
            .poll_host_frame(&mut budget, |_| NativeReceiverAnswer::Unknown)
            .unwrap()
        else {
            assert!(budget.is_exhausted());
            continue;
        };
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        if !dropped && let Some(roster) = &queued_roster {
            assert_eq!(roster.roster().observation_count(), WINDOWS);
            drop(frame);
            assert_eq!(native.session.version(), version);
            assert_eq!(native.session.native_input_recorded_count(), recorded);
            assert!(native.bridge.callback_boundary_pending());
            let retained = &native.queued_snapshot.as_ref().unwrap().roster;
            assert_eq!(retained, roster);
            assert!(Arc::ptr_eq(
                retained.shared_roster(),
                roster.shared_roster()
            ));
            dropped = true;
            continue;
        }
        let mut report = frame.commit().unwrap();
        assert!(report.take_native_effects().is_empty());
        if queued_roster.is_some() {
            retried_roster_commits += report
                .inputs()
                .iter()
                .filter(|input| {
                    matches!(
                        input,
                        HostInputOutcome::NativePlatformSnapshotApplied { .. }
                    )
                })
                .count();
        }
        native.settle_host_frame_report(&report).unwrap();
        cancellations += report
            .inputs()
            .iter()
            .filter(|input| matches!(input, HostInputOutcome::NativeInputCancelledForOverload))
            .count();
        peaks.observe(&native, 0);
        if cancellations == 1
            && !native.bridge.has_pending_callback_records()
            && !native.session.has_pending_native_input()
        {
            break;
        }
    }
    assert!(dropped);
    assert_eq!(retried_roster_commits, 1);
    assert_eq!(cancellations, 1);
    assert_eq!(
        native.lifecycle_progress().destroyed_observations(),
        WINDOWS as u64
    );
    assert!(!native.bridge.has_pending_callback_records());
    assert!(!native.session.has_pending_native_input());

    reset_layout(&mut native, DockspaceLayout::empty(), false);
    native
        .bridge
        .with_retirement_destroyed_observations(|count, entries, _, _| {
            assert_eq!(count, WINDOWS);
            let observed: Vec<_> = entries.map(|(binding, _)| binding).collect();
            assert!(bindings.iter().all(|binding| observed.contains(binding)));
        });
    native.bridge.push_terminal_viewport_roster(live_roster([]));
    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    drop(commit_boundary(&mut native));
    let prepared = native.prepare_committed_retirements().unwrap().unwrap();
    drop(prepared);
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.bindings().count()),
        WINDOWS,
        "dropping the full retirement preparation keeps every exact route retryable"
    );
    let prepared = native.prepare_committed_retirements().unwrap().unwrap();
    assert!(native.commit_retirements(prepared).is_empty());
    assert!(native.try_report_retirement_quiescence().unwrap());
    drop(commit_boundary(&mut native));
    // The next prelude releases the committed quiescence prefix and its core guards.
    drop(commit_boundary(&mut native));
    assert!(!native.session.has_pending_native_input());
    for &(_, _, binding) in &routes {
        assert!(!native.session.recognizes_native_binding(binding));
        assert!(!native.binding_has_retained_host_references(binding));
    }
    for retirement in retirements {
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                retirement,
            ),
            NativeHostWake::Wait
        );
    }
    assert_eq!(
        native.lifecycle_progress().quiescent_retirements(),
        WINDOWS as u64
    );
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.bindings().count()),
        0
    );
    assert!(native.is_quiescent());
    peaks.observe(&native, 0);
    eprintln!(
        "maximum-roster: windows={WINDOWS}, command_terminals={}, destroyed={WINDOWS}, raw_accepted={accepted}, owned_peak={}, reserved_peak={}",
        commands.len() * 2,
        peaks.resident,
        peaks.reserved
    );
}

#[test]
fn configured_close_and_create_owners_survive_freeze_and_exact_terminal_retry() {
    assert_close_and_create_pressure(false);
}

#[test]
fn accepted_release_keeps_its_exact_output_across_close_create_pressure_and_cancel_retry() {
    assert_close_and_create_pressure(true);
}

#[test]
fn ordinary_focus_peak_one_survives_release_pressure_and_late_predecessor_callbacks() {
    assert_close_and_create_command_pressure(true, true);
}

fn assert_close_and_create_pressure(with_accepted_release: bool) {
    assert_close_and_create_command_pressure(with_accepted_release, false);
}

fn dispatch_model_focus(
    native: &mut NativeCoordinator,
    context: &egui::Context,
    peaks: &mut CapacityPeaks,
) -> NativeViewportFocusRecord {
    let (viewport, binding, window) = native.focus_control.queued_route().unwrap();
    // Dispatch the single focus owner retained by accept_native_effects, then
    // model only its terminal callback. The default context has no OS attachment.
    peaks.observe(native, 0);
    assert!(native.dispatch_viewport_commands(context).unwrap());
    let token = native.focus_control.pending_token().unwrap();
    assert!(native.focus_control.queued_route().is_none());
    assert!(native.focus_control.references_binding(binding));
    peaks.observe(native, 0);
    NativeViewportFocusRecord::for_test(
        token,
        viewport,
        window,
        Some(binding),
        NativeViewportFocusStatus::Retired,
    )
}

fn settle_model_focus(
    native: &mut NativeCoordinator,
    context: &egui::Context,
    focus: NativeViewportFocusRecord,
) {
    assert_eq!(native.bridge.front_viewport_focus(), Some(focus));
    let captured = native.bridge.captured_through_for_test();
    let recorded = native.session.native_input_recorded_count();
    assert_eq!(
        native
            .reduce_callback_prefix(context, &mut test_budget())
            .unwrap(),
        Some(std::num::NonZeroUsize::MIN)
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(!native.focus_control.has_pending_work());
    let version = native.session.version();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    assert_eq!(native.session.version(), version);
    assert!(native.bridge.callback_boundary_pending());
    assert!(
        native
            .reduce_callback_prefix(context, &mut test_budget())
            .unwrap()
            .is_none()
    );
    let report = commit_boundary(native);
    assert!(report.manipulation_feedback().is_empty());
    assert!(!native.session.has_pending_native_input());
    native.bridge.push_record(HostRecord::ViewportFocus(focus));
    assert_eq!(native.bridge.captured_through_for_test(), captured);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.pending_effect_results.is_empty());
    assert!(native.take_callback_error().is_none());
}

// Keep focus-only frame candidates out of the shared fixture's debug stack,
// including when its original callers skip every focus branch.
#[inline(never)]
fn register_model_focus_child(
    native: &mut NativeCoordinator,
    recovery_host: NativeSurfaceBinding,
) -> (ViewportId, WindowId, NativeSurfaceBinding) {
    native
        .session
        .register_owned_native_child(
            SurfaceId::new(4),
            HostWindowToken::new(4),
            recovery_host.surface(),
        )
        .unwrap();
    let report = commit_boundary(native);
    assert_eq!(report.native_bindings().len(), 4);
    let binding = *report
        .native_bindings()
        .iter()
        .find(|binding| binding.surface() == SurfaceId::new(4))
        .unwrap();
    let viewport = ViewportId::from_hash_of("finite-focus-rehome-child");
    let window = WindowId::from(44);
    native.bind_viewport(viewport, window, binding).unwrap();
    (viewport, window, binding)
}

#[inline(never)]
fn request_model_focus_child_rehome(native: &mut NativeCoordinator, host: NativeSurfaceBinding) {
    native
        .session
        .report_native_global_focus(NativeGlobalFocus::Foreign)
        .unwrap();
    drop(commit_boundary(native));
    let mut rehome = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    rehome
        .frame
        .float_item_current(ItemId::new(4), host.surface())
        .unwrap();
    rehome
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = rehome.commit().unwrap();
    drop(settle_report(native, report));
}

#[inline(never)]
fn finish_model_focus_child_rehome(
    native: &mut NativeCoordinator,
    context: &egui::Context,
    peaks: &mut CapacityPeaks,
    bindings: &[NativeSurfaceBinding; 3],
    child: (ViewportId, WindowId, NativeSurfaceBinding),
    host_window: WindowId,
) {
    let (viewport, window, binding) = child;
    let mut report = paint_and_present_all_surfaces(native);
    let effects = report.take_native_effects();
    assert_eq!(effects.len(), 2);
    assert!(effects.iter().any(|request| {
        request.operation() == &NativeEffectOperation::ReleaseChild { binding }
    }));
    assert!(effects.iter().any(|request| {
        request.operation()
            == &NativeEffectOperation::RequestFocus {
                binding: bindings[2],
            }
    }));
    native.accept_native_effects(context, effects).unwrap();
    drop(settle_report(native, report));
    assert_eq!(
        native.focus_control.queued_route(),
        Some((ViewportId::ROOT, bindings[2], host_window))
    );
    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            90,
            window,
            Some(viewport),
            WindowEvent::Destroyed,
            routes,
        )
    });
    assert_eq!(destroyed.binding(), Some(binding));
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    assert!(
        native
            .reduce_callback_prefix(context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    drop(commit_boundary(native));
    native
        .bridge
        .push_viewport_roster(NativeViewportRosterRecord::for_test(
            bindings.iter().enumerate().map(|(index, &binding)| {
                CompiledWindowObservation::new(binding, ready_window_facts(index as f64 * 720.0))
                    .with_focus_reportable_for_test()
            }),
            true,
        ));
    assert!(
        native
            .reduce_callback_prefix(context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    drop(commit_boundary(native));
    let prepared = native.prepare_committed_retirements().unwrap().unwrap();
    assert!(native.commit_retirements(prepared).is_empty());
    assert!(native.try_report_retirement_quiescence().unwrap());
    drop(commit_boundary(native));
    drop(commit_boundary(native));
    assert!(!native.session.recognizes_native_binding(binding));
    assert!(!native.binding_has_retained_host_references(binding));
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
    let presented = paint_and_present_all_surfaces(native);
    drop(settle_report(native, presented));
    assert_eq!(
        native.focus_control.queued_route(),
        Some((ViewportId::ROOT, bindings[2], host_window))
    );
    assert_eq!(native.focus_control.pending_binding(), Some(bindings[2]));
    assert!(native.focus_control.references_binding(bindings[2]));
    assert!(!native.is_quiescent());
    peaks.observe(native, 0);
}

#[inline(never)]
fn commit_model_focus_request(
    native: &mut NativeCoordinator,
    context: &egui::Context,
    binding: NativeSurfaceBinding,
) -> HostFrameReport {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    let effects = report.take_native_effects();
    assert!(
        matches!(effects.as_slice(), [request]
            if request.operation() == &NativeEffectOperation::RequestFocus { binding }),
        "expected one exact focus request; effects={effects:?}, inputs={:?}",
        report.inputs()
    );
    native.accept_native_effects(context, effects).unwrap();
    settle_report(native, report)
}

fn assert_close_and_create_command_pressure(
    with_accepted_release: bool,
    with_ordinary_focus: bool,
) {
    assert_close_create_and_output_pressure(
        with_accepted_release,
        with_ordinary_focus,
        None,
        |_, _| Vec::new(),
    );
}

fn assert_close_create_and_output_pressure(
    with_accepted_release: bool,
    with_ordinary_focus: bool,
    output_window: Option<WindowId>,
    fill_outputs: impl FnOnce(
        &mut NativeCoordinator,
        &mut CapacityPeaks,
    ) -> Vec<eframe::NativeOutputResult>,
) {
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use dockspace::geometry::LogicalPoint;
    use dockspace::model::{
        DockspaceActionOutcome, DockspaceContainedLayout, FloatingPresentationId,
        NativeWindowPlacement,
    };
    use dockspace::runtime::{
        DockspaceManipulationDelivery, DockspaceManipulationPendingReason,
        DockspaceManipulationStatus, NativeProducerStorageLimit, NativeReceiverPurpose,
        NativeReportStorageLimit,
    };
    use winit::dpi::{PhysicalPosition, PhysicalSize};
    use winit::event::{
        DeviceId, ElementState, MouseButton, PointerEventFacts, PointerEventGeometry,
        PointerWindowRoute,
    };

    use super::close::begin_model_cancellation;

    const INITIAL_TABS: [ItemId; 3] = [ItemId::new(3), ItemId::new(5), ItemId::new(6)];
    const REORDERED_TABS: [ItemId; 3] = [ItemId::new(5), ItemId::new(6), ItemId::new(3)];
    const RELEASE_ORDINAL: u64 = 104;

    assert!(!with_ordinary_focus || with_accepted_release);
    let effect_slots = if with_ordinary_focus { 2 } else { 1 };
    // This finite owner model uses real core effects and journal captures. It
    // does not manufacture canonical fork close/create or renderer output tokens.
    let mut profile = combined_profile();
    profile.input = profile
        .input
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            NonZeroUsize::new(4).unwrap(),
            NonZeroUsize::MIN,
            128 * 1024,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            NonZeroUsize::new(64).unwrap(),
            NonZeroUsize::new(effect_slots).unwrap(),
            256 * 1024,
        ));
    profile.owners = NativeOwnerStorageLimit {
        resource_slots: 4,
        close_slots: 2,
        byte_limit: NativeOwnerStorageLimit::required_bytes(4, effect_slots, 4, 2).unwrap(),
    };
    assert_eq!(profile.byte_limit, TOTAL_BYTES);
    assert!(profile.initial_required_bytes().unwrap() <= TOTAL_BYTES);
    let surfaces = (1..=3).map(|id| {
        let surface = DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs(if with_accepted_release && id == 3 {
                    INITIAL_TABS.to_vec()
                } else {
                    vec![ItemId::new(id)]
                }),
            ),
        );
        if id == 3 && !with_ordinary_focus {
            surface.with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(4),
                DockspaceRootLayout::new(RootId::new(4), DockspaceNode::tabs([ItemId::new(4)])),
                LogicalRect::new(40.0, 40.0, 320.0, 240.0).unwrap(),
            ))
        } else {
            surface
        }
    });
    let layout = DockspaceLayout::new(surfaces.chain(with_ordinary_focus.then(|| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(4),
            DockspaceRootLayout::new(RootId::new(4), DockspaceNode::tabs([ItemId::new(4)])),
        )
    })))
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let now = Arc::new(AtomicU64::new(0));
    let clock = Arc::clone(&now);
    let mut native = NativeCoordinator::new_with_combined_storage_limit_and_clock(
        DockspaceSession::from_layout(layout, policy).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        profile,
        dockspace::runtime::NativeMonotonicClock::new(move || {
            Duration::from_millis(clock.load(Ordering::Relaxed))
        }),
    )
    .unwrap();
    let context = egui::Context::default();
    let mut peaks = CapacityPeaks::default();
    peaks.observe(&native, 0);
    for id in 1..=3 {
        native
            .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
            .unwrap();
    }
    let report = commit_boundary(&mut native);
    let bindings: [_; 3] = std::array::from_fn(|index| {
        *report
            .native_bindings()
            .iter()
            .find(|binding| binding.surface() == SurfaceId::new(index as u64 + 1))
            .unwrap()
    });
    drop(report);
    let routes: [_; 3] = std::array::from_fn(|index| {
        (
            if index == 2 {
                ViewportId::ROOT
            } else {
                ViewportId::from_hash_of(("finite-close-create", index))
            },
            if index == 2 {
                output_window.unwrap_or_else(|| WindowId::from(33))
            } else {
                WindowId::from(11 * (index as u64 + 1))
            },
            bindings[index],
        )
    });
    for &(viewport, window, binding) in &routes {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    let focus_child =
        with_ordinary_focus.then(|| register_model_focus_child(&mut native, bindings[2]));
    native
        .report_snapshot(
            bindings
                .iter()
                .enumerate()
                .map(|(index, &binding)| (binding, ready_window_facts(index as f64 * 720.0)))
                .chain(focus_child.map(|(_, _, binding)| (binding, ready_window_facts(2160.0)))),
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    drop(commit_boundary(&mut native));
    let presented = paint_and_present_all_surfaces(&mut native);
    drop(settle_report(&mut native, presented));

    if let Some(child) = focus_child {
        // Return the bootstrapped child to the original fixture's contained root.
        // Its presentation-gated product action emits both ReleaseChild and a
        // genuine RequestFocus; E2 also leaves room for the later old create attempt.
        request_model_focus_child_rehome(&mut native, bindings[2]);
        finish_model_focus_child_rehome(
            &mut native,
            &context,
            &mut peaks,
            &bindings,
            child,
            routes[2].1,
        );
    }
    let (source_root, source_contained) = {
        let view = native.session.view();
        let source = view.item(ItemId::new(4)).unwrap();
        (source.root(), source.contained().unwrap())
    };
    if !with_ordinary_focus {
        assert_eq!(source_root, RootId::new(4));
        assert_eq!(source_contained, FloatingPresentationId::new(4));
    }

    let close_event = |index: usize, ordinal| {
        let (viewport, window, binding) = routes[index];
        NativeWindowEventRecord::for_test(
            ordinal,
            window,
            Some(viewport),
            Some(binding),
            WindowEvent::CloseRequested,
        )
    };
    let closes: [_; 2] = std::array::from_fn(|index| {
        let (viewport, _, binding) = routes[index];
        native
            .bridge
            .push_record(HostRecord::WindowEvent(close_event(
                index,
                index as u64 + 1,
            )));
        assert!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap()
                .is_some()
        );
        let report = commit_boundary(&mut native);
        assert!(native.settle_close_control_report(&report).unwrap());
        let close = native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding))
            .unwrap();
        let acknowledgement = begin_model_cancellation(&mut native, close, viewport);
        (close, acknowledgement)
    });
    assert_ne!(closes[0].0, closes[1].0);
    assert_eq!(native.close_progress().requests_retained(), 2);
    assert_eq!(native.close_progress().cancellations_committed(), 0);

    let mut create = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    create
        .frame
        .move_root_to_new_window_current(
            source_root,
            NativeWindowPlacement::new(PhysicalRect::new(2200.0, 80.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    create
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = create.commit().unwrap();
    native.settle_host_frame_report(&report).unwrap();
    assert!(
        report.inputs().iter().any(|input| matches!(
            input,
            HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::NativeRootTearOffRequested {
                    root, source_surface, items, ..
                }
            ) if *root == source_root
                && *source_surface == bindings[2].surface()
                && items.as_slice() == [ItemId::new(4)]
        )),
        "the current item root must enter the create saga; inputs={:?}",
        report.inputs()
    );
    let effects = report.take_native_effects();
    let child = match effects.as_slice() {
        [request] => match request.operation() {
            NativeEffectOperation::CreateWindow { binding, .. } => *binding,
            operation => panic!("expected the sole create request, got {operation:?}"),
        },
        requests => panic!(
            "expected one create request, got {requests:?}; inputs={:?}",
            report.inputs()
        ),
    };
    native.accept_native_effects(&context, effects).unwrap();
    let child_viewport = viewport_id_for(child);
    assert!(
        native
            .bridge
            .with_viewports_mut(|routes| { routes.admit_test_create(child_viewport, child) })
    );
    assert!(native.bridge.with_viewports(|routes| {
        routes.has_admitted_create_attempt(child_viewport, child) && routes.bindings().count() == 4
    }));
    assert_eq!(native.bridge.create_binding(child_viewport), Some(child));
    {
        let mut pending = native.effects.awaiting_viewport_callbacks();
        assert_eq!(pending.next().map(|plan| plan.binding()), Some(child));
        assert!(pending.next().is_none());
    }
    assert_eq!(native.deferred_viewport_specs().len(), 1);
    assert!(native.session.has_pending_native_create());
    assert_eq!(
        native
            .session
            .view()
            .item(ItemId::new(4))
            .unwrap()
            .surface(),
        bindings[2].surface()
    );
    drop(report);
    peaks.observe(&native, 0);

    if with_accepted_release {
        // A live create saga intentionally excludes a new gesture. Cancellation
        // ends that semantic transition, while the already admitted OS attempt
        // must retain its exact callback/cleanup owner until Failed arrives.
        let mut cancel = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(cancel.frame.cancel_next_native_create().unwrap());
        cancel
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = cancel.commit().unwrap();
        native.settle_host_frame_report(&report).unwrap();
        native
            .accept_native_effects(&context, report.take_native_effects())
            .unwrap();
        assert!(!native.session.has_pending_native_create());
        assert!(native.bridge.with_viewports(|routes| {
            routes.has_admitted_create_attempt(child_viewport, child)
        }));
        assert!(native.effects.references_binding(child));
        peaks.observe(&native, 0);
    }

    let (_, input_window, input_binding) = routes[2];
    let input_surface = input_binding.surface();
    let tab_order = |native: &NativeCoordinator| {
        native
            .session
            .view()
            .item(ItemId::new(3))
            .unwrap()
            .tabs()
            .items()
            .to_vec()
    };
    let (receivers, points, mut unrelated_output) = if with_accepted_release {
        // Repaint after close/create enrollment so receiver facts come from the
        // actual current output, independently of the pending native owners.
        let mut paint = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let plan = paint.frame.paint_plan(input_surface).unwrap().unwrap();
        let receivers = [
            plan.tab_receiver(ItemId::new(3)).unwrap(),
            plan.tab_receiver(ItemId::new(6)).unwrap(),
        ];
        let mut points = [None; 2];
        plan.visit_surface_program(|step| {
            if let SurfacePaintProgramStep::Operation {
                operation: SurfacePaintProgramOperation::Tab(tab),
                ..
            } = step
            {
                for (index, item, fraction) in [(0, 3, 0.25), (1, 6, 0.625)] {
                    if tab.item() == ItemId::new(item) {
                        let bounds = tab.visible_bounds();
                        let point = LogicalPoint::new(
                            bounds.x() + bounds.width() * fraction,
                            bounds.y() + bounds.height() * 0.5,
                        )
                        .unwrap();
                        assert!(receivers[index].bounds().contains(point));
                        points[index] = Some(point);
                    }
                }
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        for surface in [input_surface, bindings[0].surface()] {
            let receipt = paint
                .frame
                .paint_plan(surface)
                .unwrap()
                .unwrap()
                .execute_surface_program(|_| Ok::<(), ()>(()))
                .unwrap();
            paint.frame.confirm_surface_painted(receipt).unwrap();
        }
        paint
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = paint.commit().unwrap();
        native.settle_host_frame_report(&report).unwrap();
        assert!(report.take_native_effects().is_empty());
        let mut source_output = None;
        let mut unrelated = None;
        for output in report.take_painted_outputs() {
            if output.surface() == input_surface {
                assert!(source_output.replace(output).is_none());
            } else {
                assert_eq!(output.surface(), bindings[0].surface());
                assert!(unrelated.replace(output).is_none());
            }
        }
        native
            .session
            .report_surface_presentation(
                source_output.unwrap(),
                SurfacePresentationResult::Presented,
            )
            .unwrap();
        drop(commit_boundary(&mut native));
        (
            Some(receivers),
            Some(points.map(Option::unwrap)),
            Some(unrelated.unwrap()),
        )
    } else {
        (None, None, None)
    };
    let resolve = |query: NativeReceiverQuery| {
        let Some([source, target]) = receivers else {
            return NativeReceiverAnswer::Unknown;
        };
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            let receiver = if query
                .point()
                .is_some_and(|point| source.bounds().contains(point))
            {
                &source
            } else {
                &target
            };
            query.clear_hover_lane(receiver).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(&source)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    };
    let capture_pointer =
        |native: &NativeCoordinator, ordinal, point: LogicalPoint, state: Option<ElementState>| {
            let coordinates = native
                .session
                .native_local_coordinates(input_binding)
                .unwrap();
            let geometry = coordinates.geometry();
            let scale = geometry.presentation_scale_factor().get();
            let position = PhysicalPosition::new(point.x() * scale, point.y() * scale);
            let facts = PointerEventFacts {
                surface_position: Some(position),
                desktop_position: None,
                geometry: Some(PointerEventGeometry {
                    surface_size: PhysicalSize::new(640, 480),
                    scale_factor: geometry.native_scale_factor().get(),
                }),
                modifiers: None,
                hover: PointerWindowRoute::Unknown,
                capture: if state == Some(ElementState::Released) {
                    PointerWindowRoute::None
                } else {
                    PointerWindowRoute::Window(input_window)
                },
            };
            let event = match state {
                Some(state) => WindowEvent::MouseInput {
                    primary_press: None,
                    device_id: DeviceId::dummy(),
                    state,
                    button: MouseButton::Left,
                    facts,
                },
                None => WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position,
                    facts,
                },
            };
            native.bridge.with_viewports(|routes| {
                NativeWindowEventRecord::for_test_local_capture(
                    ordinal,
                    input_window,
                    Some(ViewportId::ROOT),
                    event,
                    routes,
                    Some(coordinates),
                    Some(geometry),
                )
            })
        };
    if let Some([source, target]) = points {
        now.store(5, Ordering::Relaxed);
        for (ordinal, point, state) in [
            (102, source, Some(ElementState::Pressed)),
            (103, target, None),
        ] {
            let record = capture_pointer(&native, ordinal, point, state);
            let captured = native.bridge.captured_through_for_test();
            native.bridge.push_record(HostRecord::WindowEvent(record));
            assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
            let mut budget = test_budget();
            assert_eq!(
                native
                    .reduce_callback_prefix(&context, &mut budget)
                    .unwrap(),
                Some(NonZeroUsize::MIN)
            );
            let mut frame = native
                .poll_host_frame(&mut budget, resolve)
                .unwrap()
                .unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            let report = frame.commit().unwrap();
            assert!(
                report.manipulation_feedback().is_empty(),
                "gesture prefix ordinal={ordinal}: {:?}",
                report.manipulation_feedback()
            );
            drop(settle_report(&mut native, report));
            peaks.observe(&native, 0);
        }
        assert!(native.session.has_active_gesture());
        assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
        assert_eq!(tab_order(&native), INITIAL_TABS);
    }

    let successors = [close_event(0, 101), close_event(1, 7_000)];
    let successor_capture = |native: &NativeCoordinator, binding| {
        native.bridge.with_close_control(|control| {
            control
                .deferred_ingress()
                .find(|ingress| ingress.window_event().unwrap().binding() == Some(binding))
                .map(|ingress| (ingress.ordinal(), ingress.captured_at()))
        })
    };
    now.store(10, Ordering::Relaxed);
    native
        .bridge
        .push_record(HostRecord::WindowEvent(successors[0].clone()));
    let first_capture = successor_capture(&native, bindings[0]).unwrap();
    assert_eq!(first_capture.1, Duration::from_millis(10));
    let cursor = |native: &NativeCoordinator, ordinal| {
        if let Some([_, target]) = points {
            return HostRecord::WindowEvent(capture_pointer(native, ordinal, target, None));
        }
        HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            ordinal,
            input_window,
            Some(ViewportId::ROOT),
            Some(input_binding),
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(ordinal as f64, 20.0),
                facts: PointerEventFacts::default(),
            },
        ))
    };
    now.store(11, Ordering::Relaxed);
    let release_through = points.map(|[_, target]| {
        let record = capture_pointer(
            &native,
            RELEASE_ORDINAL,
            target,
            Some(ElementState::Released),
        );
        let captured = native.bridge.captured_through_for_test();
        native.bridge.push_record(HostRecord::WindowEvent(record));
        assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
        native.bridge.captured_through_for_test()
    });
    let predecessor_focus = with_ordinary_focus.then(|| {
        assert!(release_through.is_some());
        assert_eq!(native.close_progress().cancellations_committed(), 0);
        assert_eq!(native.close_progress().requests_retained(), 3);
        assert!(native.effects.references_binding(child));
        assert!(native.focus_control.references_binding(input_binding));
        assert!(!native.session.has_pending_native_create());
        assert!(native.bridge.with_viewports(|routes| {
            routes.has_admitted_create_attempt(child_viewport, child)
        }));
        // The accepted release, old create attempt, close successors and one
        // queued focus owner coexist before its protected callback is reserved.
        dispatch_model_focus(&mut native, &context, &mut peaks)
    });
    let output_terminals = fill_outputs(&mut native, &mut peaks);
    let output_count = output_terminals.len();
    assert_eq!(native.output_backlog().ordinary(), output_count);
    assert_eq!(output_count, if output_window.is_some() { 256 } else { 0 });
    if output_count != 0 {
        assert!(release_through.is_some());
        assert_eq!(native.close_progress().cancellations_committed(), 0);
        assert_eq!(native.close_progress().requests_retained(), 3);
        assert!(native.effects.references_binding(child));
        assert_eq!(
            native.focus_control.pending_token(),
            Some(predecessor_focus.unwrap().token())
        );
        assert!(native.bridge.with_viewports(|routes| {
            routes.has_admitted_create_attempt(child_viewport, child)
        }));
        peaks.observe(&native, 0);
    }
    let mut accepted = 0;
    for ordinal in 1_000..5_096 {
        let captured = native.bridge.captured_through_for_test();
        native.bridge.push_record(cursor(&native, ordinal));
        peaks.observe(&native, 0);
        if native.bridge.captured_through_for_test() == captured {
            break;
        }
        accepted += 1;
    }
    if output_count == 0 {
        assert!((1..4_096).contains(&accepted));
    } else {
        assert_eq!(
            accepted, 0,
            "O256 freezes the first subsequent ordinary admission"
        );
    }
    let boundary = native.bridge.captured_through_for_test();
    if let Some(release_through) = release_through {
        if output_count == 0 {
            assert!(
                release_through < boundary,
                "B includes the release and later raw input"
            );
        } else {
            assert_eq!(
                boundary, release_through,
                "full output occupancy freezes exactly at the accepted release"
            );
        }
        assert!(native.session.has_active_gesture());
    }
    native.bridge.push_record(cursor(&native, 6_000));
    assert_eq!(native.bridge.captured_through_for_test(), boundary);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::ContinueBoundary
    );

    now.store(20, Ordering::Relaxed);
    native
        .bridge
        .push_record(HostRecord::WindowEvent(successors[1].clone()));
    let captures = [
        first_capture,
        successor_capture(&native, bindings[1]).unwrap(),
    ];
    assert!(captures[0].0 <= boundary);
    assert_eq!(captures[1], (boundary + 1, Duration::from_millis(20)));
    now.store(30, Ordering::Relaxed);
    assert_eq!(
        native
            .bridge
            .record_viewport_create_failure_for_test(child_viewport),
        NativeHostWake::RepaintRoot
    );
    let failure_capture = native.bridge.with_viewports(|routes| {
        let ingress = routes.create_ingress().next().unwrap();
        let HostRecord::ViewportCreateFailed(failure) = ingress.record() else {
            panic!("the admitted create retains its exact Failed envelope");
        };
        assert_eq!(failure.binding(), child);
        (*failure, ingress.ordinal(), ingress.captured_at())
    });
    assert_eq!(failure_capture.1, boundary + 2);
    assert_eq!(failure_capture.2, Duration::from_millis(30));
    if let Some(focus) = predecessor_focus {
        let captured = native.bridge.captured_through_for_test();
        let recorded = native.session.native_input_recorded_count();
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                eframe::NativeViewportPropertyCommandRetirement::Focus {
                    viewport_id: routes[0].0,
                    token: focus.token(),
                },
            ),
            NativeHostWake::Wait,
        );
        assert_eq!(native.bridge.captured_through_for_test(), captured);
        now.store(35, Ordering::Relaxed);
        let retirement = eframe::NativeViewportPropertyCommandRetirement::Focus {
            viewport_id: focus.viewport(),
            token: focus.token(),
        };
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                retirement
            ),
            NativeHostWake::RepaintRootAfterCurrent,
        );
        assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
        assert_eq!(
            NativeHostHandler::on_viewport_property_command_retired(
                native.bridge.as_ref(),
                retirement
            ),
            NativeHostWake::Wait,
        );
        assert_eq!(native.session.native_input_recorded_count(), recorded);
        assert!(native.bridge.front_viewport_focus().is_none());
        assert!(native.effects.references_binding(child));
        peaks.observe(&native, 0);
    }
    let terminal_through = boundary + 2 + u64::from(with_ordinary_focus);
    for successor in &successors {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(successor.clone()));
    }
    assert_eq!(
        native
            .bridge
            .record_viewport_create_failure_for_test(child_viewport),
        NativeHostWake::Wait
    );
    native.bridge.push_record(cursor(&native, 7_001));
    assert_eq!(native.bridge.captured_through_for_test(), terminal_through);
    assert_eq!(native.close_progress().requests_retained(), 4);
    peaks.observe(&native, 0);

    let mut release_output = None;
    let mut release_feedback = Vec::new();
    let mut dropped_release = false;
    drop(commit_boundary(&mut native));
    for _ in 0..64 {
        match native.bridge.admission_dispatch() {
            NativeDispatch::Cancel { boundary: frozen } => {
                assert_eq!(frozen, boundary);
                break;
            }
            NativeDispatch::Replay { through } => assert_eq!(through, Some(boundary)),
            NativeDispatch::ContinueBoundary => {}
            dispatch => {
                panic!("the fixed pre-B prefix must drain before cancellation: {dispatch:?}")
            }
        }
        let release = with_accepted_release
            && native.bridge.front_event().is_some_and(|event| {
                event.ordinal() == RELEASE_ORDINAL
                    && matches!(
                        event.event(),
                        WindowEvent::MouseInput {
                            state: ElementState::Released,
                            ..
                        }
                    )
            });
        let mut budget = test_budget();
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap();
        let recorded = native.session.native_input_recorded_count();
        let version = native.session.version();
        let Some(mut frame) = native.poll_host_frame(&mut budget, resolve).unwrap() else {
            assert!(budget.is_exhausted());
            continue;
        };
        if release {
            assert!(!dropped_release);
            let receipt = frame
                .frame
                .paint_plan(input_surface)
                .unwrap()
                .unwrap()
                .execute_surface_program(|_| Ok::<(), ()>(()))
                .unwrap();
            frame.frame.confirm_surface_painted(receipt).unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(frame);
            assert_eq!(native.session.version(), version);
            assert_eq!(native.session.native_input_recorded_count(), recorded);
            assert_eq!(tab_order(&native), INITIAL_TABS);
            assert!(native.session.has_active_gesture());
            assert_eq!(
                native.session.has_pending_native_create(),
                !with_accepted_release
            );
            assert!(native.bridge.callback_boundary_pending());
            peaks.observe(&native, 0);
            assert!(
                native
                    .reduce_callback_prefix(&context, &mut budget)
                    .unwrap()
                    .is_none(),
                "retry must reuse the retained release rather than translate its raw successor"
            );
            assert_eq!(native.session.native_input_recorded_count(), recorded);
            frame = native
                .poll_host_frame(&mut budget, resolve)
                .unwrap()
                .unwrap();
            let plan = frame.frame.paint_plan(input_surface).unwrap().unwrap();
            let mut preview = false;
            let receipt = plan
                .execute_surface_program(|step| {
                    preview |= matches!(step, SurfacePaintProgramStep::DragPreview(_));
                    Ok::<(), ()>(())
                })
                .unwrap();
            assert!(
                preview,
                "the actual retried release must paint its docking preview"
            );
            frame.frame.confirm_surface_painted(receipt).unwrap();
            dropped_release = true;
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        assert!(report.take_native_effects().is_empty());
        assert!(
            !report
                .inputs()
                .contains(&HostInputOutcome::NativeInputCancelledForOverload)
        );
        if release {
            let [pending] = report.manipulation_feedback() else {
                panic!("the accepted release must publish one pending outcome: {report:?}");
            };
            assert_eq!(
                pending.status(),
                DockspaceManipulationStatus::Pending(
                    DockspaceManipulationPendingReason::AwaitingPresentation
                )
            );
            release_feedback.push(*pending);
            let [output] = report.take_painted_outputs().try_into().unwrap();
            assert!(output.matches_native_binding(input_binding));
            assert!(release_output.replace(output).is_none());
        } else {
            assert!(report.manipulation_feedback().is_empty());
            assert!(report.take_painted_outputs().is_empty());
        }
        if with_accepted_release {
            assert_eq!(tab_order(&native), INITIAL_TABS);
        }
        native.settle_host_frame_report(&report).unwrap();
        peaks.observe(&native, 0);
    }
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert_eq!(dropped_release, with_accepted_release);
    assert_eq!(release_output.is_some(), with_accepted_release);
    assert_eq!(native.focus_control.has_pending_work(), with_ordinary_focus);
    now.store(40, Ordering::Relaxed);
    for (index, &binding) in bindings[..2].iter().enumerate() {
        assert_eq!(successor_capture(&native, binding), Some(captures[index]));
    }
    native.bridge.with_viewports(|routes| {
        let ingress = routes.create_ingress().next().unwrap();
        assert_eq!(
            (ingress.ordinal(), ingress.captured_at()),
            (failure_capture.1, failure_capture.2)
        );
    });
    assert!(native.bridge.front_viewport_create_failure().is_none());
    let version = native.session.version();
    let recorded = native.session.native_input_recorded_count();
    let mut cancelled = native
        .poll_host_frame(&mut test_budget(), |_| NativeReceiverAnswer::Unknown)
        .unwrap()
        .unwrap();
    assert!(cancelled.native_input_pending());
    assert!(!cancelled.frame.native_input_pending());
    cancelled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(cancelled);
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
    assert_eq!(
        native.session.has_pending_native_create(),
        !with_accepted_release
    );
    assert!(!native.try_report_retirement_quiescence().unwrap());
    peaks.observe(&native, 0);
    // The original renderer terminals settle retained framebuffer owners while
    // the dropped overload candidate, old create and close/focus duties remain.
    // Retained outputs carry no semantic presentation claim or journal edge.
    for (index, terminal) in output_terminals.into_iter().enumerate() {
        let token = terminal.token();
        assert_eq!(
            native.bridge.output_reservation(token).unwrap().binding(),
            Some(input_binding)
        );
        assert_eq!(native.bridge.on_output(terminal), NativeHostWake::Wait);
        assert!(native.bridge.output_reservation(token).is_none());
        assert_eq!(native.output_backlog().ordinary(), output_count - index - 1);
        assert_eq!(native.bridge.captured_through_for_test(), terminal_through);
        assert_eq!(
            native.bridge.admission_dispatch(),
            NativeDispatch::Cancel { boundary }
        );
        assert_eq!(native.session.version(), version);
        assert_eq!(native.session.native_input_recorded_count(), recorded);
        assert_eq!(native.close_progress().cancellations_committed(), 0);
        assert!(native.effects.references_binding(child));
        assert_eq!(native.focus_control.has_pending_work(), with_ordinary_focus);
        peaks.observe(&native, 0);
    }
    let cancelled = commit_boundary(&mut native);
    assert_eq!(
        cancelled
            .inputs()
            .iter()
            .filter(|input| **input == HostInputOutcome::NativeInputCancelledForOverload)
            .count(),
        1
    );
    assert!(cancelled.manipulation_feedback().is_empty());
    assert_eq!(
        native.session.has_pending_native_create(),
        !with_accepted_release
    );
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    assert!(!native.is_quiescent());
    assert_eq!(native.focus_control.has_pending_work(), with_ordinary_focus);
    if with_accepted_release {
        assert_eq!(release_feedback.len(), 1);
        assert_eq!(tab_order(&native), INITIAL_TABS);
        assert!(!native.session.has_active_gesture());
    }
    drop(cancelled);

    assert_eq!(
        native.bridge.front_viewport_create_failure(),
        Some(failure_capture.0)
    );
    native.bridge.with_viewports(|routes| {
        let ingress = routes.create_ingress().next().unwrap();
        assert_eq!(
            (ingress.ordinal(), ingress.captured_at()),
            (failure_capture.1, failure_capture.2)
        );
    });
    assert_eq!(native.close_progress().cancellations_committed(), 0);
    let recorded = native.session.native_input_recorded_count();
    assert_eq!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap(),
        Some(NonZeroUsize::MIN)
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(native.session.has_pending_native_input());
    assert!(native.bridge.callback_boundary_pending());
    assert!(!native.effects.references_binding(child));
    assert!(native.pending_effect_results.is_empty());
    assert!(native.bridge.retirement_has_pending_work());
    assert!(!native.bridge.retirement_is_quiescence_candidate(child));
    assert!(!native.try_report_retirement_quiescence().unwrap());
    let version = native.session.version();
    let mut failed = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(!failed.frame.native_input_pending());
    // The candidate has consumed Failed, rather than being an empty continuation:
    // the published session still has its saga, but this candidate has none.
    assert!(!failed.frame.cancel_next_native_create().unwrap());
    failed
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(failed);
    assert_eq!(native.session.version(), version);
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert_eq!(
        native.session.has_pending_native_create(),
        !with_accepted_release
    );
    assert_eq!(
        native.session.is_current_native_binding(child),
        !with_accepted_release
    );
    assert!(!native.bridge.retirement_is_quiescence_candidate(child));
    assert!(!native.try_report_retirement_quiescence().unwrap());
    assert_eq!(native.bridge.captured_through_for_test(), terminal_through);
    peaks.observe(&native, 0);
    let failed = if let Some(focus) = predecessor_focus {
        // Failed releases E capacity for the queued focus successor. Production
        // rejects it while the predecessor still owns the single focus lane.
        assert_eq!(native.focus_control.pending_token(), Some(focus.token()));
        let report = commit_model_focus_request(&mut native, &context, input_binding);
        assert_eq!(native.focus_control.pending_token(), Some(focus.token()));
        assert_eq!(native.focus_control.pending_binding(), Some(input_binding));
        assert!(native.focus_control.queued_route().is_none());
        assert!(native.pending_effect_results.is_empty());
        assert!(native.session.has_pending_native_input());
        report
    } else {
        commit_boundary(&mut native)
    };
    assert!(failed.manipulation_feedback().is_empty());
    drop(failed);
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded + 1 + u64::from(with_ordinary_focus)
    );
    assert!(!native.session.has_pending_native_create());
    assert!(!native.session.is_current_native_binding(child));
    assert!(native.session.recognizes_native_binding(child));
    assert!(native.bridge.retirement_is_quiescence_candidate(child));
    if let Some(focus) = predecessor_focus {
        settle_model_focus(&mut native, &context, focus);
        assert_eq!(native.bridge.captured_through_for_test(), terminal_through);
        assert_eq!(native.close_progress().cancellations_committed(), 0);
        assert_eq!(tab_order(&native), INITIAL_TABS);
        peaks.observe(&native, 0);
    }
    assert!(native.try_report_retirement_quiescence().unwrap());
    assert!(!native.try_report_retirement_quiescence().unwrap());
    assert_eq!(
        native.session.native_input_recorded_count(),
        recorded + 2 + 2 * u64::from(with_ordinary_focus)
    );
    assert!(!native.session.recognizes_native_binding(child));
    assert_eq!(
        native
            .bridge
            .record_viewport_create_failure_for_test(child_viewport),
        NativeHostWake::Wait
    );
    drop(commit_boundary(&mut native));
    drop(commit_boundary(&mut native));
    assert!(!native.binding_has_retained_host_references(child));
    peaks.observe(&native, 0);

    if let Some(output) = release_output.take() {
        assert_eq!(tab_order(&native), INITIAL_TABS);
        assert!(!native.is_quiescent());
        let unrelated = unrelated_output.take().unwrap();
        let unrelated_artifact = unrelated.presentation_artifact();
        native
            .session
            .report_surface_presentation(unrelated, SurfacePresentationResult::Presented)
            .unwrap();
        let unrelated = commit_boundary(&mut native);
        assert_eq!(
            unrelated
                .surface_presentation_commits()
                .iter()
                .filter(|commit| commit.settles(unrelated_artifact))
                .count(),
            1
        );
        assert!(unrelated.manipulation_feedback().is_empty());
        assert_eq!(tab_order(&native), INITIAL_TABS);
        drop(unrelated);
        let artifact = output.presentation_artifact();
        native
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let version = native.session.version();
        let mut settlement = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        settlement
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        drop(settlement);
        let recorded = native.session.native_input_recorded_count();
        assert_eq!(native.session.version(), version);
        assert_eq!(tab_order(&native), INITIAL_TABS);
        assert_eq!(native.close_progress().cancellations_committed(), 0);
        for (index, &binding) in bindings[..2].iter().enumerate() {
            assert_eq!(successor_capture(&native, binding), Some(captures[index]));
        }
        let delivered = if with_ordinary_focus {
            commit_model_focus_request(&mut native, &context, input_binding)
        } else {
            commit_boundary(&mut native)
        };
        assert_eq!(native.session.native_input_recorded_count(), recorded);
        assert_eq!(
            delivered
                .surface_presentation_commits()
                .iter()
                .filter(|commit| commit.settles(artifact))
                .count(),
            1
        );
        release_feedback.extend_from_slice(delivered.manipulation_feedback());
        let [pending, terminal] = release_feedback.as_slice() else {
            panic!("one release must have one pending and one terminal: {release_feedback:?}");
        };
        assert_eq!(pending.manipulation_id(), terminal.manipulation_id());
        assert_eq!(
            terminal.status(),
            DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
        );
        assert!(delivered.workspace_changed());
        assert_eq!(tab_order(&native), REORDERED_TABS);
        let successor_focus = with_ordinary_focus.then(|| {
            let successor = dispatch_model_focus(&mut native, &context, &mut peaks);
            assert_ne!(successor.token(), predecessor_focus.unwrap().token());
            successor
        });
        drop(delivered);
        let version = native.session.version();
        let repeated = commit_boundary(&mut native);
        assert!(repeated.manipulation_feedback().is_empty());
        assert_eq!(native.session.version(), version);
        assert_eq!(native.session.native_input_recorded_count(), recorded);
        assert!(
            !native.is_quiescent(),
            "the original close owners still await Clear"
        );
        peaks.observe(&native, 0);

        if let Some(successor) = successor_focus {
            let predecessor = predecessor_focus.unwrap();
            // Real earlier tokens and wrong routes may be admitted as ordinary
            // callbacks again, but none may spend the successor's exact owner.
            for unrelated in [
                NativeViewportFocusRecord::for_test(
                    predecessor.token(),
                    successor.viewport(),
                    successor.window(),
                    Some(input_binding),
                    NativeViewportFocusStatus::AlreadyFocused,
                ),
                NativeViewportFocusRecord::for_test(
                    successor.token(),
                    successor.viewport(),
                    routes[0].1,
                    Some(bindings[0]),
                    NativeViewportFocusStatus::AlreadyFocused,
                ),
            ] {
                assert_eq!(
                    native.focus_control.classify(unrelated),
                    NativeFocusResultDisposition::Unrelated
                );
                let captured = native.bridge.captured_through_for_test();
                let recorded = native.session.native_input_recorded_count();
                native
                    .bridge
                    .push_record(HostRecord::ViewportFocus(unrelated));
                assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
                assert_eq!(
                    native
                        .reduce_callback_prefix(&context, &mut test_budget())
                        .unwrap(),
                    Some(NonZeroUsize::MIN)
                );
                assert_eq!(native.session.native_input_recorded_count(), recorded);
                let report = commit_boundary(&mut native);
                assert!(report.inputs().is_empty());
                assert!(report.manipulation_feedback().is_empty());
                assert_eq!(native.focus_control.pending_binding(), Some(input_binding));
                assert_eq!(native.close_progress().cancellations_committed(), 0);
                assert_eq!(tab_order(&native), REORDERED_TABS);
            }
            let retirement = eframe::NativeViewportPropertyCommandRetirement::Focus {
                viewport_id: successor.viewport(),
                token: successor.token(),
            };
            let captured = native.bridge.captured_through_for_test();
            assert_eq!(
                NativeHostHandler::on_viewport_property_command_retired(
                    native.bridge.as_ref(),
                    retirement
                ),
                NativeHostWake::RepaintRootAfterCurrent
            );
            assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
            settle_model_focus(&mut native, &context, successor);
            assert_eq!(native.bridge.captured_through_for_test(), captured + 1);
            assert!(!native.effects.has_pending_work());
            assert_eq!(
                peaks.queued_focus, 1,
                "the single inline focus owner permits one queued command"
            );
            peaks.observe(&native, 0);
        }
    }

    let settled_through = terminal_through + 3 * u64::from(with_ordinary_focus);
    assert_eq!(native.bridge.captured_through_for_test(), settled_through);
    for index in 0..2 {
        let (viewport, _, binding) = routes[index];
        let (close, acknowledgement) = closes[index];
        assert_eq!(successor_capture(&native, binding), Some(captures[index]));
        native
            .session
            .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
            .unwrap();
        assert!(
            native
                .bridge
                .with_close_control(|control| control.mark_clear_recorded(binding))
        );
        let mut clear = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        clear
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = clear.commit().unwrap();
        native.bridge.with_close_control(|control| {
            assert_eq!(control.request_for_binding(binding), Some(close));
            assert_eq!(control.parked_ingress().count(), 2 - index);
            assert!(control.callback_ingress().next().is_none());
        });
        assert!(native.settle_close_control_report(&report).unwrap());
        assert!(!native.settle_close_control_report(&report).unwrap());
        drop(settle_report(&mut native, report));
        native.bridge.with_close_control(|control| {
            assert_eq!(control.request_for_binding(binding), None);
            assert_eq!(control.parked_ingress().count(), 1 - index);
            let mut promoted = control.callback_ingress();
            let ingress = promoted.next().unwrap();
            assert_eq!(ingress.window_event(), Some(&successors[index]));
            assert_eq!((ingress.ordinal(), ingress.captured_at()), captures[index]);
            assert!(promoted.next().is_none());
            if index == 0 {
                assert_eq!(control.request_for_binding(bindings[1]), Some(closes[1].0));
            }
        });
        if index == 0 {
            assert_eq!(successor_capture(&native, bindings[1]), Some(captures[1]));
        }
        let captured = native.bridge.captured_through_for_test();
        let recorded = native.session.native_input_recorded_count();
        native
            .bridge
            .push_record(HostRecord::WindowEvent(successors[index].clone()));
        assert_eq!(native.bridge.captured_through_for_test(), captured);
        assert_eq!(
            native
                .reduce_callback_prefix(&context, &mut test_budget())
                .unwrap(),
            Some(NonZeroUsize::MIN)
        );
        let report = commit_boundary(&mut native);
        assert!(native.settle_close_control_report(&report).unwrap());
        assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
        let next = native
            .bridge
            .with_close_control(|control| control.request_for_binding(binding))
            .unwrap();
        assert_ne!(next, close);
        let acknowledgement = begin_model_cancellation(&mut native, next, viewport);
        native
            .session
            .publish_native_close(binding, NativeCloseState::Clear, Some(acknowledgement))
            .unwrap();
        assert!(
            native
                .bridge
                .with_close_control(|control| control.mark_clear_recorded(binding))
        );
        let report = commit_boundary(&mut native);
        assert!(native.settle_close_control_report(&report).unwrap());
        assert!(
            !native
                .bridge
                .with_close_control(|control| control.references_binding(binding))
        );
        assert_eq!(
            native.close_progress().cancellations_committed(),
            (index as u64 + 1) * 2
        );
        assert_eq!(native.bridge.captured_through_for_test(), settled_through);
        peaks.observe(&native, 0);
    }

    native
        .bridge
        .push_terminal_viewport_roster(live_roster(bindings));
    assert!(
        native
            .reduce_callback_prefix(&context, &mut test_budget())
            .unwrap()
            .is_some()
    );
    drop(commit_boundary(&mut native));
    drop(commit_boundary(&mut native));
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert!(!native.bridge.has_pending_callback_records());
    assert!(!native.session.has_pending_native_input());
    assert!(
        !native
            .bridge
            .with_close_control(|control| control.has_pending_work())
    );
    assert!(!native.bridge.retirement_has_pending_work());
    assert_eq!(native.close_progress().requests_retained(), 4);
    assert_eq!(native.close_progress().cancellations_committed(), 4);
    assert_eq!(
        native.lifecycle_progress().quiescent_retirements(),
        1 + u64::from(with_ordinary_focus)
    );
    let view = native.session.view();
    let source_item = view.item(ItemId::new(4)).unwrap();
    assert_eq!(source_item.surface(), bindings[2].surface());
    assert_eq!(source_item.root(), source_root);
    assert_eq!(source_item.contained(), Some(source_contained));
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.bindings().count()),
        3
    );
    assert!(native.is_quiescent());
    assert!(!native.focus_control.has_pending_work());
    assert_eq!(peaks.queued_focus, usize::from(with_ordinary_focus));
    let idle = peaks.observe(&native, 0);
    let report = commit_boundary(&mut native);
    assert!(
        !report
            .inputs()
            .contains(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    assert!(report.manipulation_feedback().is_empty());
    if with_accepted_release {
        assert_eq!(tab_order(&native), REORDERED_TABS);
    }
    assert_eq!(peaks.observe(&native, 0), idle);
    eprintln!(
        "close-create: W4/A1 E{effect_slots} M4/L2 O{output_count} focus_owner=single, accepted_release={with_accepted_release}, queued_focus_peak={}, raw_accepted={accepted}, owned_peak={}, reserved_peak={}",
        peaks.queued_focus, peaks.resident, peaks.reserved
    );
}
