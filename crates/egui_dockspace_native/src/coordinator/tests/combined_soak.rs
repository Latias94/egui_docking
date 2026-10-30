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
