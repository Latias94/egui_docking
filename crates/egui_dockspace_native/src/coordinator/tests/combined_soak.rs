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

#[test]
fn configured_close_and_create_owners_survive_freeze_and_exact_terminal_retry() {
    use std::num::NonZeroUsize;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::Duration;

    use dockspace::model::{
        DockspaceContainedLayout, FloatingPresentationId, NativeWindowPlacement,
    };
    use dockspace::runtime::{NativeProducerStorageLimit, NativeReportStorageLimit};
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, PointerEventFacts};

    use super::close::begin_model_cancellation;

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
            NonZeroUsize::MIN,
            256 * 1024,
        ));
    profile.owners = NativeOwnerStorageLimit {
        resource_slots: 4,
        command_slots: 0,
        close_slots: 2,
        byte_limit: NativeOwnerStorageLimit::required_bytes(4, 1, 4, 0, 2).unwrap(),
    };
    assert_eq!(profile.byte_limit, TOTAL_BYTES);
    assert!(profile.initial_required_bytes().unwrap() <= TOTAL_BYTES);
    let layout = DockspaceLayout::new((1..=3).map(|id| {
        let surface = DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        );
        if id == 3 {
            surface.with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(4),
                DockspaceRootLayout::new(RootId::new(4), DockspaceNode::tabs([ItemId::new(4)])),
                LogicalRect::new(40.0, 40.0, 320.0, 240.0).unwrap(),
            ))
        } else {
            surface
        }
    }))
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
            WindowId::from(11 * (index as u64 + 1)),
            bindings[index],
        )
    });
    for &(viewport, window, binding) in &routes {
        native.bind_viewport(viewport, window, binding).unwrap();
    }
    native
        .report_snapshot(
            bindings
                .iter()
                .enumerate()
                .map(|(index, &binding)| (binding, ready_window_facts(index as f64 * 720.0))),
            NativeWorkAreaRoster::Unknown,
            test_managed_capabilities(),
        )
        .unwrap();
    drop(commit_boundary(&mut native));
    let presented = paint_and_present_all_surfaces(&mut native);
    drop(settle_report(&mut native, presented));

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
            RootId::new(4),
            NativeWindowPlacement::new(PhysicalRect::new(2200.0, 80.0, 420.0, 320.0).unwrap()),
        )
        .unwrap();
    create
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = create.commit().unwrap();
    native.settle_host_frame_report(&report).unwrap();
    let effects = report.take_native_effects();
    let child = match effects.as_slice() {
        [request] => match request.operation() {
            NativeEffectOperation::CreateWindow { binding, .. } => *binding,
            operation => panic!("expected the sole create request, got {operation:?}"),
        },
        requests => panic!("expected one create request, got {requests:?}"),
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
    let (_, input_window, input_binding) = routes[2];
    let cursor = |ordinal| {
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
    let mut accepted = 0;
    for ordinal in 1_000..5_096 {
        let captured = native.bridge.captured_through_for_test();
        native.bridge.push_record(cursor(ordinal));
        peaks.observe(&native, 0);
        if native.bridge.captured_through_for_test() == captured {
            break;
        }
        accepted += 1;
    }
    assert!((1..4_096).contains(&accepted));
    let boundary = native.bridge.captured_through_for_test();
    native.bridge.push_record(cursor(6_000));
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
    native.bridge.push_record(cursor(7_001));
    assert_eq!(native.bridge.captured_through_for_test(), boundary + 2);
    assert_eq!(native.close_progress().requests_retained(), 4);
    peaks.observe(&native, 0);

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
        let mut budget = test_budget();
        native
            .reduce_callback_prefix(&context, &mut budget)
            .unwrap();
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
        let mut report = frame.commit().unwrap();
        assert!(report.take_native_effects().is_empty());
        assert!(
            !report
                .inputs()
                .contains(&HostInputOutcome::NativeInputCancelledForOverload)
        );
        native.settle_host_frame_report(&report).unwrap();
        peaks.observe(&native, 0);
    }
    assert_eq!(
        native.bridge.admission_dispatch(),
        NativeDispatch::Cancel { boundary }
    );
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
    assert!(native.session.has_pending_native_create());
    assert!(!native.try_report_retirement_quiescence().unwrap());
    peaks.observe(&native, 0);
    let cancelled = commit_boundary(&mut native);
    assert_eq!(
        cancelled
            .inputs()
            .iter()
            .filter(|input| **input == HostInputOutcome::NativeInputCancelledForOverload)
            .count(),
        1
    );
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
    assert!(native.session.has_pending_native_create());
    assert!(native.session.is_current_native_binding(child));
    assert!(!native.bridge.retirement_is_quiescence_candidate(child));
    assert!(!native.try_report_retirement_quiescence().unwrap());
    assert_eq!(native.bridge.captured_through_for_test(), boundary + 2);
    peaks.observe(&native, 0);
    drop(commit_boundary(&mut native));
    assert_eq!(native.session.native_input_recorded_count(), recorded + 1);
    assert!(!native.session.has_pending_native_create());
    assert!(!native.session.is_current_native_binding(child));
    assert!(native.session.recognizes_native_binding(child));
    assert!(native.bridge.retirement_is_quiescence_candidate(child));
    assert!(native.try_report_retirement_quiescence().unwrap());
    assert!(!native.try_report_retirement_quiescence().unwrap());
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
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
        assert_eq!(native.bridge.captured_through_for_test(), boundary + 2);
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
    assert_eq!(native.lifecycle_progress().quiescent_retirements(), 1);
    let view = native.session.view();
    let source_item = view.item(ItemId::new(4)).unwrap();
    assert_eq!(source_item.surface(), bindings[2].surface());
    assert_eq!(source_item.root(), RootId::new(4));
    assert_eq!(
        source_item.contained(),
        Some(FloatingPresentationId::new(4))
    );
    assert_eq!(
        native
            .bridge
            .with_viewports(|routes| routes.bindings().count()),
        3
    );
    assert!(native.is_quiescent());
    let idle = peaks.observe(&native, 0);
    let report = commit_boundary(&mut native);
    assert!(
        !report
            .inputs()
            .contains(&HostInputOutcome::NativeInputCancelledForOverload)
    );
    assert_eq!(peaks.observe(&native, 0), idle);
    eprintln!(
        "close-create: W4/A1 E1 M4/C0/L2, raw_accepted={accepted}, owned_peak={}, reserved_peak={}",
        peaks.resident, peaks.reserved
    );
}
