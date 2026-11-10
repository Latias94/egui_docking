//! Ordered callback mailbox for the fork-backed native host.

use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use winit::window::WindowId;

use crate::coordinator::{
    NoFrameInvalidationReadiness, NoFrameTerminal, PendingNoFrameInvalidation,
    PreparedNoFrameInvalidations, no_frame_invalidation_readiness,
    output_terminal_requires_adapter_abandonment,
};
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, NativeCleanupObservation,
    NativeCloseEffectAcknowledgement, NativeCreateEffectAcknowledgement, NativeEffectResult,
    NativeInputBudget, NativeMonotonicClock, NativeRootContinuation, NativeSurfaceBinding,
    NativeWindowFacts,
};
#[cfg(test)]
use eframe::NativeViewportCreateFailureKind;
use eframe::{
    NativeDeviceRemoval, NativeGlobalFocusObservation, NativeHostAttachment, NativeHostHandler,
    NativeHostWake, NativeOutputAdmission, NativeOutputPassFinalization, NativeOutputPassFinalizer,
    NativeOutputResult, NativeOutputToken, NativePhysicalRect, NativeViewportCloseRequest,
    NativeViewportCreateAdmission, NativeViewportCreateAttempt, NativeViewportCreateFailure,
    NativeViewportFocusResult, NativeViewportPointerPassthroughResult, NativeViewportRoster,
    NativeViewportVisibilityResult, NativeWindowEvent, NativeWindowEventDisposition,
    NativeWindowSnapshot, egui::ViewportId,
};

use crate::close_control::{
    NativeCloseCallbackAdmission, NativeCloseCaptureAdmission, NativeCloseControl,
    NativeViewportCloseCancellationRecord,
};
use crate::error::{NativeHostProtocolError, NativeRuntimeError, NativeViewportBindingError};
use crate::event::{NativeDeviceRemovalRecord, NativeWindowEventClass, NativeWindowEventRecord};
use crate::focus_control::{NativeGlobalFocusRecord, NativeViewportFocusRecord};
use crate::input_control::NativePointerPassthroughRecord;
use crate::lifecycle_progress::NativeOutputBacklog;
use crate::pointer_capture::{NativePointerCaptureKey, NativePointerCaptureRecord};
use crate::pointer_event::NativePointerTranslator;
use crate::pointer_observation::{NativePointerObservationKey, NativePointerObservationRecord};
use crate::retirement::{
    CleanupResultRetentionError, CommittedRetirement, DestroyedObservation, NativeRetirementState,
};
use crate::viewport_callback::{NativeViewportCreateFailureRecord, NativeViewportVisibilityRecord};
use crate::viewport_map::{CreateAdmission, CreateTerminal, NativeViewportMap};
#[cfg(test)]
use crate::window_snapshot::CompiledWindowObservation;

mod admission;
#[cfg(test)]
mod admission_tests;
use admission::NativeAdmission;
pub(crate) use admission::NativeDispatch;
mod capacity;
mod command_callback;
#[cfg(test)]
mod command_ingress_tests;
pub(crate) mod journal;
#[cfg(test)]
mod lifecycle_ingress_tests;
mod output_resource;
#[cfg(test)]
mod pointer_observation_tests;
#[cfg(all(test, target_os = "linux"))]
mod preparation_output_tests;
mod roster;
mod terminal_roster;
mod window_resource;
use command_callback::CommandCallbackAdmission;
use journal::{HostIngressJournal, ReservedHostRecord};
use output_resource::NativeOutputResource;
pub(crate) use output_resource::PendingNativeOutput;
use roster::NativeViewportRosterKey;
pub(crate) use roster::{NativeViewportRosterEnvelope, NativeViewportRosterRecord};
use terminal_roster::TerminalRosterState;
use window_resource::NativeWindowResources;
pub(crate) use window_resource::{
    NativePreparationObservation, NativePreparationPaint, NativePreparationPaintPhase,
};

#[derive(Debug, Clone)]
pub(crate) enum HostRecord {
    WindowEvent(NativeWindowEventRecord),
    DeviceRemoval(NativeDeviceRemovalRecord),
    GlobalFocus(NativeGlobalFocusRecord),
    ViewportFocus(NativeViewportFocusRecord),
    ViewportPointerPassthrough(NativePointerPassthroughRecord),
    PointerObservation(NativePointerObservationKey),
    PointerCapture(NativePointerCaptureKey),
    ViewportCloseCancelled(NativeViewportCloseCancellationRecord),
    ViewportRoster(NativeViewportRosterEnvelope),
    ViewportCreateFailed(NativeViewportCreateFailureRecord),
    ViewportVisibility(NativeViewportVisibilityRecord),
    ViewportCreated(NativeViewportCreatedRecord),
    Output { token: NativeOutputToken },
}

impl HostRecord {
    fn requires_terminal_roster(&self) -> bool {
        match self {
            Self::ViewportPointerPassthrough(_)
            | Self::ViewportVisibility(_)
            | Self::ViewportCreated(_) => true,
            Self::WindowEvent(event) => {
                matches!(event.event(), winit::event::WindowEvent::Destroyed)
            }
            Self::DeviceRemoval(_)
            | Self::PointerObservation(_)
            | Self::PointerCapture(_)
            | Self::GlobalFocus(_)
            | Self::ViewportFocus(_)
            | Self::ViewportCloseCancelled(_)
            | Self::ViewportRoster(_)
            | Self::ViewportCreateFailed(_)
            | Self::Output { .. } => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativeViewportCreatedRecord {
    token: NativeOutputToken,
    binding: NativeSurfaceBinding,
}

impl NativeViewportCreatedRecord {
    const fn new(token: NativeOutputToken, binding: NativeSurfaceBinding) -> Self {
        Self { token, binding }
    }

    pub(crate) const fn token(self) -> NativeOutputToken {
        self.token
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferredViewportPaint {
    Created,
    Staging(NativePreparationPaint),
    Semantic(NativeSurfaceBinding),
    Retain(NativeSurfaceBinding),
    Waiting,
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum NativeViewportReplacement {
    Reserve,
    ReserveRetired,
    Attach(winit::window::WindowId),
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputReservation {
    binding: Option<NativeSurfaceBinding>,
    terminal_recorded: bool,
    abandoned: bool,
    wake_on_terminal: bool,
    wake_after_current: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputAbandonment {
    NewlyAbandoned,
    AlreadyAbandonedAwaitingTerminal,
    MissingOrTerminal,
}

impl OutputReservation {
    pub(crate) const fn unbound() -> Self {
        Self {
            binding: None,
            terminal_recorded: false,
            abandoned: false,
            wake_on_terminal: true,
            wake_after_current: false,
        }
    }

    pub(crate) const fn bound(binding: NativeSurfaceBinding) -> Self {
        Self {
            binding: Some(binding),
            terminal_recorded: false,
            abandoned: false,
            wake_on_terminal: true,
            wake_after_current: false,
        }
    }

    const fn diagnostic(binding: Option<NativeSurfaceBinding>) -> Self {
        Self {
            binding,
            terminal_recorded: false,
            abandoned: true,
            wake_on_terminal: false,
            wake_after_current: false,
        }
    }

    const fn terminal_create(binding: NativeSurfaceBinding) -> Self {
        Self {
            binding: Some(binding),
            terminal_recorded: false,
            abandoned: true,
            wake_on_terminal: true,
            wake_after_current: true,
        }
    }

    pub(crate) fn attach(&mut self, binding: NativeSurfaceBinding) -> bool {
        if let Some(existing) = self.binding {
            return existing == binding;
        }
        self.binding = Some(binding);
        true
    }

    pub(crate) const fn binding(self) -> Option<NativeSurfaceBinding> {
        self.binding
    }

    pub(crate) const fn is_abandoned(self) -> bool {
        self.abandoned
    }

    #[cfg(test)]
    pub(crate) const fn unbound_for_test() -> Self {
        Self::unbound()
    }

    #[cfg(test)]
    const fn bound_for_test(binding: NativeSurfaceBinding) -> Self {
        Self::bound(binding)
    }

    fn abandon(&mut self) -> bool {
        self.abandon_with_terminal_wake(true)
    }

    fn abandon_silently(&mut self) -> bool {
        self.abandon_with_terminal_wake(false)
    }

    fn abandon_with_terminal_wake(&mut self, wake_on_terminal: bool) -> bool {
        if self.abandoned {
            return false;
        }
        self.abandoned = true;
        self.wake_on_terminal = wake_on_terminal;
        true
    }

    fn settle_abandonment(&mut self, wake_on_terminal: bool) -> OutputAbandonment {
        if self.terminal_recorded {
            return OutputAbandonment::MissingOrTerminal;
        }
        if self.abandoned {
            self.wake_on_terminal |= wake_on_terminal;
            return OutputAbandonment::AlreadyAbandonedAwaitingTerminal;
        }
        self.abandoned = true;
        self.wake_on_terminal = wake_on_terminal;
        OutputAbandonment::NewlyAbandoned
    }

    const fn wakes_on_terminal(self) -> bool {
        self.wake_on_terminal
    }

    const fn wakes_after_current(self) -> bool {
        self.wake_after_current
    }

    fn require_wake_after_current(&mut self) -> bool {
        if self.terminal_recorded {
            return self.wake_after_current;
        }
        self.wake_after_current = true;
        true
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct NativeCreateReservation {
    binding: NativeSurfaceBinding,
    rect: NativePhysicalRect,
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::DockspaceSession;
    use winit::dpi::PhysicalPosition;
    use winit::event::{DeviceId, PointerEventFacts, WindowEvent};
    use winit::window::WindowId;

    #[test]
    fn rejected_finalizer_drops_after_host_state_unlocks() {
        use std::sync::Weak;
        use std::sync::atomic::{AtomicUsize, Ordering};

        struct Finalizer {
            bridge: Weak<NativeHostBridge>,
            dropped: Arc<AtomicUsize>,
        }

        impl NativeOutputPassFinalizer for Finalizer {
            fn finalize(&self, _: NativeOutputPassFinalization<'_>) -> NativeHostWake {
                NativeHostWake::Wait
            }
        }

        impl Drop for Finalizer {
            fn drop(&mut self) {
                if let Some(bridge) = self.bridge.upgrade() {
                    assert!(
                        bridge.records.try_lock().is_ok(),
                        "a rejected external finalizer must drop outside the host-state lock"
                    );
                    assert!(!bridge.has_pending_coordinator_work());
                    self.dropped.fetch_add(1, Ordering::SeqCst);
                }
            }
        }

        let bridge = Arc::new(NativeHostBridge::new(NativeViewportMap::default()));
        let dropped = Arc::new(AtomicUsize::new(0));
        assert!(bridge.install_output_pass_finalizer(Arc::new(Finalizer {
            bridge: Weak::new(),
            dropped: Arc::clone(&dropped),
        })));
        assert!(!bridge.install_output_pass_finalizer(Arc::new(Finalizer {
            bridge: Arc::downgrade(&bridge),
            dropped: Arc::clone(&dropped),
        })));
        assert_eq!(dropped.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn displaced_window_destroyed_is_once_only_and_cannot_retire_its_successor() {
        let predecessor = bindings().0;
        let successor = bindings().0;
        let viewport = ViewportId::from_hash_of("overlapping-destroyed-receipts");
        let old_window = WindowId::from(81);
        let new_window = WindowId::from(82);
        let mut routes = NativeViewportMap::default();
        routes.bind(viewport, old_window, predecessor).unwrap();
        let bridge = Arc::new(NativeHostBridge::new(routes));
        bridge
            .replace_viewport_route(
                viewport,
                predecessor,
                successor,
                NativeViewportReplacement::Attach(new_window),
            )
            .unwrap();
        let destroyed = |ordinal| {
            NativeWindowEventRecord::for_test(
                ordinal,
                old_window,
                Some(viewport),
                None,
                WindowEvent::Destroyed,
            )
        };
        bridge.push_record(HostRecord::WindowEvent(destroyed(1)));
        bridge.push_record(HostRecord::WindowEvent(destroyed(2)));
        assert_eq!(bridge.lock().journal.len(), 0);
        assert_eq!(bridge.lock().retained_record_count(), 1);
        assert_eq!(bridge.front_event().unwrap().binding(), Some(predecessor));
        assert_eq!(
            bridge.retirement_observe_destroyed(viewport, predecessor, old_window, 1),
            DestroyedObservation::Recorded
        );
        assert!(bridge.acknowledge_event(1));
        assert!(!bridge.acknowledge_event(1));
        bridge.commit_frame_boundary(true);
        assert!(bridge.retirement_destroyed_observations().is_empty());
        assert!(bridge.retirement_quiescence_candidates().is_empty());
        bridge.push_record(HostRecord::WindowEvent(destroyed(3)));
        assert_eq!(bridge.lock().journal.retained_record_count(), 1);
        bridge.commit_frame_boundary(false);
        assert_eq!(bridge.retirement_destroyed_observations().len(), 1);
        bridge.retirement_mark_snapshot_queued();
        bridge.retirement_settle_snapshot(true);
        let committed = bridge.retirement_committed_routes();
        let prepared = bridge.prepare_route_retirements(&committed).unwrap();
        drop(prepared);
        assert!(bridge.retirement_quiescence_candidates().is_empty());
        assert!(
            bridge
                .prepare_route_retirements(&committed)
                .unwrap()
                .commit()
                .is_empty()
        );
        bridge.retirement_commit_routes(&committed);
        assert_eq!(bridge.retirement_quiescence_candidates(), [predecessor]);
        assert!(bridge.retirement_finish_quiescence(predecessor));
        bridge.push_record(HostRecord::WindowEvent(destroyed(4)));
        assert!(bridge.lock().journal.is_empty());
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(viewport)),
            Some(successor)
        );
        assert_eq!(
            bridge.with_viewports(|routes| routes.window(viewport)),
            Some(new_window)
        );
        assert_eq!(
            bridge.with_viewports(|routes| routes.pointer_binding_for_window(new_window)),
            Some(successor)
        );
        assert!(bridge.with_viewports(|routes| routes.has_window_receipt(viewport, successor)));
    }

    #[test]
    fn same_physical_window_rebind_rejects_a_received_destroyed_receipt() {
        let predecessor = bindings().0;
        let successor = bindings().0;
        let viewport = ViewportId::from_hash_of("same-window-rebind-receipt");
        let window = WindowId::from(83);
        let mut routes = NativeViewportMap::default();
        routes.bind(viewport, window, predecessor).unwrap();
        let bridge = NativeHostBridge::new(routes);
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            1,
            window,
            Some(viewport),
            Some(predecessor),
            WindowEvent::Destroyed,
        )));
        assert!(
            bridge
                .replace_viewport_route(
                    viewport,
                    predecessor,
                    successor,
                    NativeViewportReplacement::Attach(window),
                )
                .is_err()
        );
        assert_eq!(bridge.front_event().unwrap().binding(), Some(predecessor));
        assert!(!bridge.with_viewports(|routes| routes.has_window_receipt(viewport, successor)));
        assert!(bridge.with_viewports(|routes| routes.has_window_receipt(viewport, predecessor)));
        assert!(!bridge.lock().retirements.has_window(viewport, predecessor));
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            2,
            window,
            Some(viewport),
            Some(successor),
            WindowEvent::Destroyed,
        )));
        assert_eq!(bridge.lock().journal.len(), 0);
        assert_eq!(bridge.lock().retained_record_count(), 1);
        assert_eq!(
            bridge.retirement_observe_destroyed(viewport, predecessor, window, 1),
            DestroyedObservation::Recorded
        );
        assert!(bridge.acknowledge_event(1));
        bridge.commit_frame_boundary(false);
        assert_eq!(
            bridge.retirement_destroyed_observations(),
            [(predecessor, NativeWindowFacts::destroyed())]
        );
    }

    #[test]
    fn retired_physical_owners_reject_binding_and_window_reuse_before_quiescence() {
        let predecessor = bindings().0;
        let successor = bindings().0;
        let viewport = ViewportId::from_hash_of("retired-window-admission");
        let old_window = WindowId::from(86);
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        bridge
            .bind_viewport_route(viewport, old_window, predecessor)
            .unwrap();
        bridge.unbind_viewport_route(viewport, predecessor).unwrap();
        assert!(
            bridge
                .bind_viewport_route(viewport, WindowId::from(87), predecessor)
                .is_err()
        );
        assert!(
            bridge
                .bind_viewport_route(viewport, old_window, successor)
                .is_err()
        );
        assert!(
            bridge
                .with_viewports(|viewports| viewports.binding(viewport))
                .is_none()
        );
        assert!(bridge.lock().retirements.has_window(viewport, predecessor));
        bridge
            .bind_viewport_route(viewport, WindowId::from(87), successor)
            .unwrap();
        assert_eq!(
            bridge.with_viewports(|viewports| viewports.binding(viewport)),
            Some(successor)
        );
    }

    #[test]
    fn awaiting_physical_window_rebind_preserves_one_terminal_owner() {
        let predecessor = bindings().0;
        let successor = bindings().0;
        let viewport = ViewportId::from_hash_of("awaiting-window-rebind");
        let window = WindowId::from(88);
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        bridge
            .bind_viewport_route(viewport, window, predecessor)
            .unwrap();
        bridge
            .replace_viewport_route(
                viewport,
                predecessor,
                successor,
                NativeViewportReplacement::Attach(window),
            )
            .unwrap();
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            1,
            window,
            Some(viewport),
            None,
            WindowEvent::Destroyed,
        )));
        assert_eq!(bridge.front_event().unwrap().binding(), Some(successor));
        assert!(!bridge.lock().retirements.has_window(viewport, predecessor));
        assert!(!bridge.lock().retirements.has_window(viewport, successor));
        assert!(bridge.with_viewports(|routes| {
            routes.has_received_destroyed(viewport, successor, window, 1)
        }));
    }

    #[test]
    fn unbind_preserves_destroyed_owner_and_unattached_create_cannot_be_discarded() {
        let predecessor = bindings().0;
        let successor = bindings().0;
        let viewport = ViewportId::from_hash_of("unbound-window-receipt");
        let window = WindowId::from(84);
        let mut routes = NativeViewportMap::default();
        routes.bind(viewport, window, predecessor).unwrap();
        let bridge = NativeHostBridge::new(routes);
        assert_eq!(
            bridge.unbind_viewport_route(viewport, predecessor).unwrap(),
            predecessor
        );
        bridge
            .with_viewports_mut(|routes| routes.bind(viewport, WindowId::from(85), successor))
            .unwrap();
        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            1,
            window,
            None,
            None,
            WindowEvent::Destroyed,
        )));
        let event = bridge.front_event().unwrap();
        assert_eq!(event.binding(), Some(predecessor));
        assert_eq!(event.viewport_id(), Some(viewport));
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(viewport)),
            Some(successor)
        );

        let pending = ViewportId::from_hash_of("unattached-create-receipt");
        let binding = bindings().1;
        bridge
            .with_viewports_mut(|routes| routes.reserve(pending, binding))
            .unwrap();
        assert!(bridge.with_viewports_mut(|routes| routes.admit_test_create(pending, binding)));
        assert!(bridge.unbind_viewport_route(pending, binding).is_err());
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(pending)),
            Some(binding)
        );
        assert!(bridge.with_viewports(|routes| routes.has_pending_create_receipts()));
    }

    #[test]
    fn failed_create_receipt_moves_once_and_waits_for_its_recorded_full_boundary() {
        let (binding, successor) = bindings();
        let viewport = ViewportId::from_hash_of("create-receipt-transfer");
        let mut routes = NativeViewportMap::default();
        routes.reserve(viewport, binding).unwrap();
        let bridge = NativeHostBridge::new(routes);
        assert!(bridge.reserve_create(viewport, binding, NativePhysicalRect::new(0, 0, 320, 240)));
        assert_eq!(
            bridge.record_viewport_create_failure_for_test(viewport),
            NativeHostWake::RepaintRoot
        );
        let failure = bridge.front_viewport_create_failure().unwrap();
        assert_eq!(
            bridge.record_viewport_create_failure_for_test(viewport),
            NativeHostWake::Wait
        );
        assert_eq!(bridge.lock().journal.len(), 0);
        assert_eq!(bridge.lock().retained_record_count(), 1);
        assert!(!bridge.acknowledge_viewport_create_failure(
            NativeViewportCreateFailureRecord::new(viewport, successor, failure.kind()),
            true
        ));
        assert!(bridge.acknowledge_viewport_create_failure(failure, false));
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(viewport)),
            None
        );
        assert!(!bridge.acknowledge_viewport_create_failure(failure, true));
        assert_eq!(
            bridge.record_viewport_create_failure_for_test(viewport),
            NativeHostWake::Wait
        );
        assert!(bridge.retirement_quiescence_candidates().is_empty());
        assert!(!bridge.retirement_finish_quiescence(binding));
        bridge.commit_frame_boundary(false);
        assert!(
            bridge.retirement_quiescence_candidates().is_empty(),
            "an unrecorded result survives an unrelated full commit"
        );
        bridge.retirement_mark_create_failure_recorded(binding);
        bridge.commit_frame_boundary(true);
        assert!(bridge.retirement_quiescence_candidates().is_empty());
        assert!(!bridge.retirement_finish_quiescence(binding));
        bridge.commit_frame_boundary(false);
        assert_eq!(bridge.retirement_quiescence_candidates(), [binding]);
        assert!(bridge.retirement_finish_quiescence(binding));
        assert!(!bridge.retirement_finish_quiescence(binding));
        assert_eq!(
            bridge.record_viewport_create_failure_for_test(viewport),
            NativeHostWake::Wait
        );
    }

    #[test]
    fn transferred_create_success_keeps_one_owner_until_full_commit_and_preserves_successor() {
        let (binding, _) = bindings();
        let (successor, _) = bindings();
        let viewport = ViewportId::from_hash_of("create-receipt-success");
        let window = WindowId::from(81);
        let mut routes = NativeViewportMap::default();
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        {
            routes.reserve(viewport, binding).unwrap();
            assert!(routes.admit_test_create(viewport, binding));
            assert_eq!(
                routes.receive_create_terminal(
                    viewport,
                    binding,
                    None,
                    CreateTerminal::Created(window),
                    || journal.reserve(HostRecord::DeviceRemoval(
                        NativeDeviceRemovalRecord::for_test(1)
                    )),
                ),
                CreateAdmission::First
            );
            routes
                .attach_output(viewport, binding, window, None)
                .unwrap();
            assert!(routes.transfer_create_terminal(
                viewport,
                binding,
                None,
                CreateTerminal::Created(window)
            ));
            assert_eq!(
                routes.receive_create_terminal(
                    viewport,
                    binding,
                    None,
                    CreateTerminal::Created(window),
                    || panic!("a duplicate cannot recapture create ingress"),
                ),
                CreateAdmission::Duplicate
            );
            assert_eq!(
                routes.receive_create_terminal(
                    viewport,
                    binding,
                    None,
                    CreateTerminal::Failed(NativeViewportCreateFailureKind::WindowUnavailable),
                    || panic!("a conflicting terminal cannot recapture create ingress"),
                ),
                CreateAdmission::Conflict
            );
            assert!(
                routes
                    .reserve_replacement(viewport, binding, successor)
                    .is_err()
            );
        }
        // The receipt-state model transfers its real capture without fabricating
        // an opaque renderer output token for the Created payload.
        let ordinal = routes.create_ingress().next().unwrap().ordinal();
        journal.transfer_reserved(routes.take_create_ingress(ordinal).unwrap());
        let bridge = NativeHostBridge::new(routes);
        bridge.lock().journal = journal;
        bridge.commit_frame_boundary(true);
        assert!(bridge.with_viewports(|routes| routes.has_pending_create_receipts()));
        bridge.commit_frame_boundary(false);
        assert!(!bridge.with_viewports(|routes| routes.has_pending_create_receipts()));
        assert_eq!(
            bridge.with_viewports_mut(|routes| routes.receive_create_terminal(
                viewport,
                binding,
                None,
                CreateTerminal::Created(window),
                || panic!("a committed duplicate cannot recapture create ingress"),
            )),
            CreateAdmission::Duplicate
        );
        bridge
            .replace_viewport_route(
                viewport,
                binding,
                successor,
                NativeViewportReplacement::Reserve,
            )
            .unwrap();
        assert_eq!(
            bridge.with_viewports_mut(|routes| routes.receive_create_terminal(
                viewport,
                binding,
                None,
                CreateTerminal::Created(window),
                || panic!("a successor route cannot accept the old terminal"),
            )),
            CreateAdmission::Conflict
        );
        assert_eq!(
            bridge.with_viewports(|routes| routes.binding(viewport)),
            Some(successor)
        );
    }

    use super::*;

    fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                SurfaceId::new(1),
                DockspaceRootLayout::new(
                    RootId::new(1),
                    DockspaceNode::central_tabs([ItemId::new(1)]),
                ),
            ),
            DockspaceSurfaceLayout::new(
                SurfaceId::new(2),
                DockspaceRootLayout::new(
                    RootId::new(2),
                    DockspaceNode::central_tabs([ItemId::new(2)]),
                ),
            ),
        ])
        .expect("test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        session
            .enable_managed_native_host(dockspace::runtime::NativePointerRoster::Exact(Vec::new()))
            .expect("native host enrolls");
        session
            .register_native_root(
                SurfaceId::new(1),
                dockspace::runtime::HostWindowToken::new(1),
            )
            .expect("first root registers");
        session
            .register_native_root(
                SurfaceId::new(2),
                dockspace::runtime::HostWindowToken::new(2),
            )
            .expect("second root registers");
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
            .expect("registration surfaces settle");
        let report = frame.commit().expect("registration frame commits");
        let mut found = report.inputs().iter().filter_map(|input| match input {
            dockspace::runtime::HostInputOutcome::NativeSurfaceRegistered { binding } => {
                Some(*binding)
            }
            _ => None,
        });
        (
            found.next().expect("first binding exists"),
            found.next().expect("second binding exists"),
        )
    }

    fn workspace_root_layout(surface: SurfaceId) -> DockspaceLayout {
        DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .unwrap()
    }

    fn workspace_root_fixture() -> (DockspaceSession, NativeHostBridge, NativeSurfaceBinding) {
        use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
        use dockspace::runtime::{
            HostInputOutcome, HostWindowToken, NativeCloseState, NativePointerRoster,
            NativeReceiverAnswer, NativeWindowInputState, NativeWindowPresentationState,
            NativeWorkAreaRoster, SurfacePresentationResult, SurfaceUnavailableReason,
            UniformSurfaceMetrics,
        };

        let mut session = DockspaceSession::from_layout(
            workspace_root_layout(SurfaceId::new(1)),
            DockPolicy::default(),
        )
        .unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .unwrap();
        session
            .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let registration = frame.commit().unwrap();
        let predecessor = registration
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .unwrap();
        let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        let facts = NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible)
            .with_close(NativeCloseState::Clear, None);
        session
            .report_managed_native_snapshot([(predecessor, facts)], NativeWorkAreaRoster::Unknown)
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .measure_surface(
                predecessor.surface(),
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();

        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let receipt = frame
            .paint_plan(predecessor.surface())
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let output = frame
            .commit()
            .unwrap()
            .take_painted_outputs()
            .pop()
            .unwrap();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();

        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        bridge
            .bind_viewport_route(ViewportId::ROOT, WindowId::from(91), predecessor)
            .unwrap();
        bridge.with_viewports_mut(|routes| {
            routes.refresh_local_coordinates(&session);
            routes.set_focus_reportable(&[predecessor]);
            assert!(routes.block_semantic_viewport(ViewportId::ROOT));
            assert!(
                routes
                    .local_coordinates_for_event(WindowId::from(91), None)
                    .is_some()
            );
        });
        (session, bridge, predecessor)
    }

    #[test]
    fn workspace_root_continuation_moves_the_route_and_preserves_only_physical_authority() {
        use dockspace::runtime::{NativeEffectOperation, SurfaceUnavailableReason};

        let (mut session, bridge, predecessor) = workspace_root_fixture();
        let reset = session
            .prepare_layout_reset(workspace_root_layout(SurfaceId::new(2)))
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_native_layout_reset(&reset, predecessor, SurfaceId::new(2))
            .unwrap();
        let continuation = frame.proposed_native_root_continuations().next().unwrap();
        let successor = continuation.successor();
        assert_eq!(continuation.predecessor(), predecessor);
        assert_ne!(predecessor.surface(), successor.surface());
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut root_surface = predecessor.surface();
        let (mut report, _) = bridge
            .commit_host_frame_with_staging(frame, true, Some(&mut root_surface))
            .unwrap();

        assert_eq!(report.native_root_continuations(), &[continuation]);
        assert_eq!(root_surface, successor.surface());
        assert_eq!(session.native_local_coordinates(successor), None);
        bridge.with_viewports(|routes| {
            assert_eq!(routes.viewport(predecessor.surface()), None);
            assert_eq!(routes.viewport(successor.surface()), Some(ViewportId::ROOT));
            assert_eq!(routes.window(ViewportId::ROOT), Some(WindowId::from(91)));
            assert_eq!(
                routes.route_for_event(WindowId::from(91), None),
                Some((ViewportId::ROOT, successor))
            );
            assert!(routes.has_window_receipt(ViewportId::ROOT, successor));
            assert!(!routes.has_window_receipt(ViewportId::ROOT, predecessor));
            assert_eq!(
                routes.local_coordinates_for_event(WindowId::from(91), None),
                None
            );
            assert_eq!(
                routes.focus_binding_for_event(WindowId::from(91), None),
                None
            );
            assert!(routes.semantic_viewport_blocked(ViewportId::ROOT));
        });
        assert_eq!(
            bridge.workspace_replacement_predecessor(successor),
            Some((ViewportId::ROOT, predecessor))
        );
        assert!(bridge.retirement_is_quiescence_candidate(predecessor));
        assert!(
            !bridge
                .lock()
                .retirements
                .has_window(ViewportId::ROOT, predecessor)
        );
        assert!(bridge.retirement_destroyed_observations().is_empty());
        assert!(bridge.front_event().is_none());
        assert!(report.take_native_effects().iter().all(|effect| {
            !matches!(
                effect.operation(),
                NativeEffectOperation::CreateWindow { .. }
                    | NativeEffectOperation::CompensatingClose { .. }
                    | NativeEffectOperation::RequestReplacement { .. }
            )
        }));

        bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
            1,
            WindowId::from(91),
            Some(ViewportId::ROOT),
            None,
            WindowEvent::Destroyed,
        )));
        assert_eq!(bridge.front_event().unwrap().binding(), Some(successor));
        assert!(bridge.with_viewports(|routes| {
            routes.has_received_destroyed(ViewportId::ROOT, successor, WindowId::from(91), 1)
        }));
    }

    #[test]
    fn workspace_root_continuation_preserves_legacy_commits_and_rejects_invalid_publication() {
        use dockspace::runtime::{NativeReceiverAnswer, SurfaceUnavailableReason};

        for target_conflict in [true, false] {
            let (mut session, bridge, predecessor) = workspace_root_fixture();
            let before = session.version();
            let reset = session
                .prepare_layout_reset(workspace_root_layout(SurfaceId::new(2)))
                .unwrap();
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_native_layout_reset(&reset, predecessor, SurfaceId::new(2))
                .unwrap();
            let continuation = frame.proposed_native_root_continuations().next().unwrap();
            let successor = continuation.successor();
            let peer = ViewportId::from_hash_of("root-continuation-target-conflict");
            let peer_binding = bindings().1;
            let mut root_surface = if target_conflict {
                bridge
                    .bind_viewport_route(peer, WindowId::from(92), peer_binding)
                    .unwrap();
                assert_eq!(peer_binding.surface(), successor.surface());
                predecessor.surface()
            } else {
                SurfaceId::new(99)
            };
            let previous_root = root_surface;
            let resource_count = bridge.lock().window_resources.bindings().count();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            assert!(
                bridge
                    .commit_host_frame_with_staging(frame, true, Some(&mut root_surface))
                    .is_err()
            );

            assert_eq!(session.version(), before);
            assert_eq!(root_surface, previous_root);
            assert_eq!(bridge.workspace_replacement_predecessor(successor), None);
            assert_eq!(
                bridge.lock().window_resources.bindings().count(),
                resource_count
            );
            assert!(!bridge.lock().retirements.has_pending_work());
            bridge.with_viewports(|routes| {
                assert_eq!(routes.binding(ViewportId::ROOT), Some(predecessor));
                assert_eq!(routes.window(ViewportId::ROOT), Some(WindowId::from(91)));
                assert!(routes.has_window_receipt(ViewportId::ROOT, predecessor));
                assert!(!routes.has_window_receipt(ViewportId::ROOT, successor));
                assert!(
                    routes
                        .local_coordinates_for_event(WindowId::from(91), None)
                        .is_some()
                );
                assert_eq!(
                    routes.focus_binding_for_event(WindowId::from(91), None),
                    Some(predecessor)
                );
                if target_conflict {
                    assert_eq!(routes.binding(peer), Some(peer_binding));
                    assert_eq!(routes.window(peer), Some(WindowId::from(92)));
                }
            });
        }

        // Initial registration may replace a same-surface Root before its first
        // physical route is attached. Both commit entry points keep that path.
        for publish_root in [false, true] {
            let (mut session, _, predecessor) = workspace_root_fixture();
            let bridge = NativeHostBridge::new(NativeViewportMap::default());
            let reset = session
                .prepare_layout_reset(workspace_root_layout(predecessor.surface()))
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            frame.submit_layout_reset(&reset).unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            let mut root_surface = predecessor.surface();
            let (report, _) = bridge
                .commit_host_frame_with_staging(
                    frame,
                    true,
                    publish_root.then_some(&mut root_surface),
                )
                .unwrap();
            assert_eq!(report.native_root_continuations().len(), 1);
            assert_eq!(root_surface, predecessor.surface());
            assert_eq!(
                bridge.with_viewports(|routes| routes.binding(ViewportId::ROOT)),
                None
            );
        }
    }

    #[test]
    fn output_reservation_keeps_the_binding_seen_at_begin() {
        let (first, second) = bindings();
        let mut reservation = OutputReservation::bound_for_test(first);

        assert!(!reservation.attach(second));
        assert_eq!(reservation.binding(), Some(first));
    }

    #[test]
    fn reserved_pointer_callback_is_once_only_across_raw_handoff_partial_commit_and_retry() {
        let (binding, _) = bindings();
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(11);
        let context = eframe::egui::Context::default();
        let first = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
        let next = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let record = NativePointerPassthroughRecord::for_test(
            first,
            viewport,
            window,
            Some(binding),
            true,
            eframe::NativeViewportPointerPassthroughStatus::Applied,
        );
        {
            let mut records = bridge.lock();
            assert!(
                records
                    .window_resources
                    .reserve_pointer_callback(viewport, binding, window, first, true)
            );
            assert!(records.record_viewport_pointer_passthrough(record));
            assert!(!records.record_viewport_pointer_passthrough(record));
            assert_eq!(records.journal.len(), 0);
            assert_eq!(records.window_resources.callback_ingress().count(), 1);
            assert_eq!(records.retained_record_count(), 1);
        }
        bridge.commit_frame_boundary(false);
        assert!(bridge.lock().window_resources.has_pending_work());
        assert!(bridge.acknowledge_viewport_pointer_passthrough(record));
        assert!(!bridge.lock().record_viewport_pointer_passthrough(record));
        bridge.commit_frame_boundary(true);
        assert!(
            !bridge
                .lock()
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, next, false)
        );
        assert!(bridge.callback_boundary_pending());
        bridge.commit_frame_boundary(false);
        assert!(!bridge.lock().window_resources.has_pending_work());
        assert!(!bridge.lock().record_viewport_pointer_passthrough(record));
        assert!(
            bridge
                .lock()
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, next, false)
        );
        assert!(bridge.lock().window_resources.has_pending_work());
    }

    #[test]
    fn durable_output_wake_is_idempotent_after_the_terminal_when_prearmed() {
        let (binding, _) = bindings();
        let mut prearmed = OutputReservation::bound_for_test(binding);
        assert!(prearmed.require_wake_after_current());
        prearmed.terminal_recorded = true;
        assert!(prearmed.require_wake_after_current());

        let mut too_late = OutputReservation::bound_for_test(binding);
        too_late.terminal_recorded = true;
        assert!(!too_late.require_wake_after_current());
    }

    #[test]
    fn silent_output_abandonment_does_not_request_a_terminal_wake() {
        let mut reservation = OutputReservation::unbound_for_test();

        assert!(reservation.abandon_silently());
        assert!(!reservation.wakes_on_terminal());
        assert!(!reservation.abandon_silently());
    }

    #[test]
    fn repeated_output_settlement_only_upgrades_the_terminal_wake() {
        let (binding, _) = bindings();
        let mut silent = OutputReservation::unbound_for_test();
        assert_eq!(
            silent.settle_abandonment(false),
            OutputAbandonment::NewlyAbandoned
        );
        assert_eq!(
            silent.settle_abandonment(true),
            OutputAbandonment::AlreadyAbandonedAwaitingTerminal
        );
        assert!(silent.wakes_on_terminal());

        let mut terminal_create = OutputReservation::bound_for_test(binding);
        assert!(terminal_create.abandon());
        assert_eq!(
            terminal_create.settle_abandonment(false),
            OutputAbandonment::AlreadyAbandonedAwaitingTerminal
        );
        assert!(terminal_create.wakes_on_terminal());
    }

    #[test]
    fn output_ordinals_must_form_one_contiguous_sequence() {
        assert!(is_next_output_ordinal(0, 1));
        assert!(is_next_output_ordinal(1, 2));
        assert!(!is_next_output_ordinal(1, 1));
        assert!(!is_next_output_ordinal(1, 3));
        assert!(!is_next_output_ordinal(u64::MAX, 1));
    }

    #[test]
    fn native_runtime_host_attachment_is_one_shot() {
        let mut attachment = HostAttachmentState::Available;

        assert!(attachment.try_attach());
        assert_eq!(attachment, HostAttachmentState::Attached);
        assert!(!attachment.try_attach());

        attachment.detach();
        assert_eq!(attachment, HostAttachmentState::Retired);
        assert!(!attachment.try_attach());
    }

    #[test]
    fn deferred_semantic_prelude_waits_for_an_empty_journal_and_committed_input() {
        let child = ViewportId::from_hash_of("semantic-prelude-child");
        let cases = [
            ("empty", Vec::new(), true),
            ("root output", vec![Some(ViewportId::ROOT)], false),
            (
                "root outputs",
                vec![Some(ViewportId::ROOT), Some(ViewportId::ROOT)],
                false,
            ),
            ("child output", vec![Some(child)], false),
            (
                "root then child output",
                vec![Some(ViewportId::ROOT), Some(child)],
                false,
            ),
            (
                "root then callback",
                vec![Some(ViewportId::ROOT), None],
                false,
            ),
        ];

        for (name, records, expected) in cases {
            assert_eq!(
                deferred_semantic_prelude_ready(records.into_iter()),
                expected,
                "{name}"
            );
        }

        let mut records = NativeHostState::active();
        records.event_boundary_pending = true;
        assert!(!records.semantic_prelude_ready());
        records.event_boundary_pending = false;
        assert!(records.semantic_prelude_ready());
    }

    #[test]
    fn deferred_resource_owner_rejects_foreign_cleanup_and_prunes_only_after_last_obligation() {
        let (binding, foreign) = bindings();
        let viewport = ViewportId::from_hash_of("deferred-resource-owner");
        let rect = NativePhysicalRect::new(10, 20, 800, 600);
        let mut resources = NativeWindowResources::default();
        assert!(resources.reserve_create(viewport, binding, rect));
        assert!(resources.reserve_create(viewport, binding, rect));
        assert!(!resources.reserve_create(viewport, foreign, rect));
        assert!(resources.references_binding(binding));
        assert!(!resources.clear_create(viewport, foreign));
        assert!(!resources.clear_hidden(viewport, foreign));
        resources.retire(viewport, foreign);
        assert_eq!(
            resources.create(viewport),
            Some(NativeCreateReservation { binding, rect })
        );
        assert_eq!(resources.hidden_binding(viewport), Some(binding));

        assert!(resources.clear_create(viewport, binding));
        assert!(resources.has_pending_work());
        assert_eq!(resources.hidden_binding(viewport), Some(binding));
        assert!(resources.clear_hidden(viewport, binding));
        assert!(!resources.has_pending_work());
        assert!(!resources.references_binding(binding));
        assert!(resources.reserve_create(viewport, foreign, rect));
        resources.retire(viewport, binding);
        assert_eq!(resources.hidden_binding(viewport), Some(foreign));
        resources.retire(viewport, foreign);
        assert!(!resources.has_pending_work());
    }

    #[test]
    fn active_mailbox_accepts_only_events_consumed_by_dockspace() {
        let moved = WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(10.0, 20.0),
            facts: PointerEventFacts::default(),
        };
        let records = NativeHostState::active();

        assert!(records.accepts_window_event(&moved));
        assert!(records.accepts_window_event(&WindowEvent::CloseRequested));
        assert!(records.accepts_window_event(&WindowEvent::Destroyed));
        assert!(!records.accepts_window_event(&WindowEvent::Focused(true)));
    }

    #[test]
    fn idle_cursor_prefix_keeps_only_the_latest_same_lane_observation() {
        let (binding, _) = bindings();
        let window = WindowId::from(11);
        let mut records = NativeHostState::active();
        for ordinal in 1..=128 {
            records.record_window_event(NativeWindowEventRecord::for_test(
                ordinal,
                window,
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(ordinal as f64, 20.0),
                    facts: PointerEventFacts::default(),
                },
            ));
        }

        assert!(
            records.coalesce_idle_cursor_prefix(&mut NativeInputBudget::new(
                &NativeMonotonicClock::new(|| std::time::Duration::ZERO),
            ))
        );
        assert_eq!(records.journal.len(), 1);
        assert!(matches!(
            records.journal.front(),
            Some(HostRecord::WindowEvent(event)) if event.ordinal() == 128
        ));
    }

    #[test]
    fn idle_cursor_prefix_stops_at_outputs_and_other_callbacks() {
        use IdleCursorPrefixEntry::{Boundary, Cursor, Output};

        assert_eq!(
            idle_cursor_prefix_latest_index(
                [Cursor, Output, Cursor, Output, Cursor, Output].into_iter()
            ),
            None
        );
        assert_eq!(
            idle_cursor_prefix_latest_index(
                [Cursor, Cursor, Output, Cursor, Boundary, Output, Cursor].into_iter()
            ),
            Some(1)
        );
        assert_eq!(
            idle_cursor_prefix_latest_index([Cursor, Output, Output].into_iter()),
            None
        );
    }

    #[test]
    fn blocked_child_repaints_only_after_the_ordered_boundary_is_clear() {
        let (binding, _) = bindings();
        let child = ViewportId::from_hash_of("blocked-semantic-child");
        let mut records = NativeHostState::active();
        records.viewports.reserve(child, binding).unwrap();
        records.record_window_event(NativeWindowEventRecord::for_test(
            1,
            WindowId::from(22),
            Some(child),
            Some(binding),
            WindowEvent::CursorMoved {
                device_id: DeviceId::dummy(),
                position: PhysicalPosition::new(10.0, 20.0),
                facts: PointerEventFacts::default(),
            },
        ));
        records.block_semantic_viewport(child);

        records.release_semantic_viewports_if_ready();
        assert!(records.take_ready_semantic_viewports().is_empty());

        records.journal.pop_front();
        records.release_semantic_viewports_if_ready();
        assert_eq!(records.take_ready_semantic_viewports(), vec![child]);
    }

    #[test]
    fn claimed_native_suffix_cannot_release_a_blocked_child_with_an_empty_raw_journal() {
        let mut records = NativeHostState::active();
        let child = ViewportId::from_hash_of("claimed-native-child");
        let (binding, _) = bindings();
        records.viewports.reserve(child, binding).unwrap();
        records.block_semantic_viewport(child);
        records.event_boundary_pending = true;
        assert!(records.journal.is_empty());

        assert!(!records.semantic_prelude_ready());
        records.release_semantic_viewports_if_ready();
        assert!(records.take_ready_semantic_viewports().is_empty());
        assert!(records.viewports.semantic_viewport_blocked(child));

        records.event_boundary_pending = false;
        records.release_semantic_viewports_if_ready();
        assert_eq!(records.take_ready_semantic_viewports(), vec![child]);
    }

    #[test]
    fn callback_age_and_admission_reservation_survive_handoff_partial_commit_and_retry() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Duration;

        let now = Arc::new(AtomicU64::new(0));
        let source = Arc::clone(&now);
        let bridge = NativeHostBridge::with_clock(
            NativeViewportMap::default(),
            NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
        );
        bridge.push_record(HostRecord::DeviceRemoval(
            NativeDeviceRemovalRecord::for_test(1),
        ));
        now.store(25, Ordering::SeqCst);
        assert!(bridge.acknowledge_device_removal(NativeDeviceRemovalRecord::for_test(1)));
        assert!(!bridge.has_pending_callback_records());
        assert!(bridge.has_pending_coordinator_work());
        assert_eq!(bridge.lock().journal.retained_record_count(), 1);
        bridge.commit_frame_boundary(true);
        now.store(250, Ordering::SeqCst);
        assert_eq!(
            bridge.lock().journal.oldest_age(),
            Some(Duration::from_millis(250))
        );
        assert_eq!(bridge.lock().journal.retained_record_count(), 1);
        assert!(bridge.callback_boundary_pending());

        // A yielded or dropped attempt has no mailbox commit call at all.
        assert_eq!(bridge.lock().journal.front_ordinal(), None);
        assert_eq!(bridge.lock().journal.captured_through(), 1);
        bridge.commit_frame_boundary(false);
        assert_eq!(bridge.lock().journal.retained_record_count(), 0);
        assert_eq!(bridge.lock().journal.oldest_age(), None);
        assert!(!bridge.has_pending_coordinator_work());
    }

    #[test]
    fn cursor_claim_drop_and_idle_coalescing_preserve_original_capture_identity_and_age() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Duration;

        let (binding, _) = bindings();
        let now = Arc::new(AtomicU64::new(0));
        let source = Arc::clone(&now);
        let bridge = NativeHostBridge::with_clock(
            NativeViewportMap::default(),
            NativeMonotonicClock::new(move || Duration::from_millis(source.load(Ordering::SeqCst))),
        );
        for ordinal in 1..=3 {
            now.store((ordinal - 1) * 10, Ordering::SeqCst);
            bridge.push_record(HostRecord::WindowEvent(NativeWindowEventRecord::for_test(
                ordinal,
                WindowId::from(100),
                Some(ViewportId::ROOT),
                Some(binding),
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position: PhysicalPosition::new(ordinal as f64, 20.0),
                    facts: PointerEventFacts::default(),
                },
            )));
        }
        now.store(50, Ordering::SeqCst);
        {
            let claim = bridge.claim_cursor_prefix(256).unwrap();
            assert_eq!(claim.record_count().get(), 3);
        }
        assert_eq!(bridge.lock().journal.front_ordinal(), Some(1));
        assert_eq!(bridge.lock().journal.retained_record_count(), 3);
        assert_eq!(
            bridge.lock().journal.oldest_age(),
            Some(Duration::from_millis(50))
        );

        let mut budget = NativeInputBudget::new(&NativeMonotonicClock::new(|| Duration::ZERO));
        assert!(bridge.coalesce_idle_cursor_prefix(&mut budget));
        assert_eq!(bridge.lock().journal.front_ordinal(), Some(3));
        assert_eq!(bridge.lock().journal.captured_through(), 3);
        assert_eq!(bridge.lock().journal.retained_record_count(), 1);
        assert_eq!(
            bridge.lock().journal.oldest_age(),
            Some(Duration::from_millis(30))
        );
        assert_eq!(budget.remaining_records(), 254);
        assert_eq!(bridge.front_event().unwrap().ordinal(), 3);
    }

    #[test]
    fn identical_viewport_rosters_wait_for_a_new_fact_context() {
        let roster = NativeViewportRosterRecord::for_test(
            std::iter::empty::<CompiledWindowObservation>(),
            true,
        );
        let mut records = NativeHostState::active();

        assert!(records.record_viewport_roster(roster.clone()));
        assert!(!records.record_viewport_roster(roster.clone()));

        let first = records
            .front_viewport_roster()
            .expect("the queued roster freezes its enqueue context");
        assert!(records.acknowledge_viewport_roster(&first));
        assert!(!records.record_viewport_roster(roster.clone()));
        assert!(records.settle_viewport_roster(&first, false));

        assert!(records.record_viewport_roster(roster.clone()));
        let retry = records
            .front_viewport_roster()
            .expect("a rejected roster remains retryable");
        assert!(records.acknowledge_viewport_roster(&retry));
        assert!(records.settle_viewport_roster(&retry, true));
        assert!(!records.record_viewport_roster(roster.clone()));

        records.invalidate_viewport_roster();
        assert!(records.record_viewport_roster(roster.clone()));

        let mut queued = NativeHostState::active();
        assert!(queued.record_viewport_roster(roster.clone()));
        queued.invalidate_viewport_roster();
        assert!(queued.record_viewport_roster(roster));
        assert_eq!(
            queued
                .journal
                .iter()
                .filter(|record| matches!(record, HostRecord::ViewportRoster(_)))
                .count(),
            2
        );
    }

    #[test]
    fn queued_inflight_and_committed_roster_share_one_immutable_owned_snapshot() {
        let roster = NativeViewportRosterRecord::for_test(
            std::iter::empty::<CompiledWindowObservation>(),
            true,
        );
        let mut records = NativeHostState::active();
        assert!(records.record_viewport_roster(roster));
        let queued = records.front_viewport_roster().unwrap();
        let retry = records.front_viewport_roster().unwrap();
        assert!(
            std::ptr::eq(queued.roster(), retry.roster()),
            "reading an exact roster must not allocate another owned observation vector"
        );
        assert!(records.acknowledge_viewport_roster(&queued));
        assert!(std::ptr::eq(
            queued.roster(),
            records
                .in_flight_viewport_roster
                .as_ref()
                .unwrap()
                .roster
                .as_ref()
        ));
        assert!(records.settle_viewport_roster(&queued, true));
        assert!(std::ptr::eq(
            queued.roster(),
            records
                .committed_viewport_roster
                .as_ref()
                .unwrap()
                .roster
                .as_ref()
        ));
        assert_eq!(
            retry, queued,
            "a retained immutable receipt survives its owner's settlement"
        );
    }

    #[test]
    fn active_destroyed_does_not_leave_a_shutdown_roster_obligation() {
        let (binding, _) = bindings();
        let mut records = NativeHostState::active();
        records.record_window_event(NativeWindowEventRecord::for_test(
            1,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Destroyed,
        ));

        assert!(!records.terminal_roster.has_pending());
        assert!(records.journal.pop_front().is_some());
        assert!(!records.has_pending_coordinator_work());

        records.quarantine_after_fatal();
        records.record_window_event(NativeWindowEventRecord::for_test(
            2,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            Some(binding),
            WindowEvent::Destroyed,
        ));
        assert!(records.terminal_roster.has_pending());
    }

    #[test]
    fn quarantine_preserves_an_existing_visibility_roster_obligation() {
        let (binding, _) = bindings();
        let mut records = NativeHostState::active();
        assert!(
            records.record_viewport_visibility(NativeViewportVisibilityRecord::for_test(
                eframe::queue_native_viewport_visibility(
                    &eframe::egui::Context::default(),
                    ViewportId::ROOT,
                    true
                ),
                ViewportId::ROOT,
                WindowId::from(11),
                binding,
                true,
                eframe::NativeViewportVisibilityStatus::Dispatched,
            ))
        );
        assert!(!records.terminal_roster.has_pending());

        records.quarantine_after_fatal();

        assert!(records.terminal_roster.has_pending());
    }

    #[test]
    fn rejected_terminal_roster_is_rearmed_until_applied() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        bridge.quarantine_after_fatal();
        bridge.require_terminal_roster();
        let roster = NativeViewportRosterRecord::for_test([], true);

        bridge.push_terminal_viewport_roster(roster.clone());
        assert_eq!(bridge.lock().journal.len(), 0);
        assert!(bridge.lock().terminal_roster.has_pending());
        let rejected = bridge
            .front_viewport_roster()
            .expect("the terminal roster enters its reserved slot");
        assert!(bridge.acknowledge_viewport_roster(&rejected));
        assert!(bridge.settle_viewport_roster(&rejected, false));
        assert!(bridge.lock().terminal_roster.has_pending());
        assert!(
            !bridge
                .lock()
                .record_terminal_viewport_roster(roster.clone())
        );
        bridge.commit_frame_boundary(true);
        assert!(
            !bridge
                .lock()
                .record_terminal_viewport_roster(roster.clone())
        );
        bridge.commit_frame_boundary(false);

        bridge.push_terminal_viewport_roster(roster);
        let applied = bridge
            .front_viewport_roster()
            .expect("the rejected terminal roster remains retryable");
        assert!(bridge.acknowledge_viewport_roster(&applied));
        bridge.commit_frame_boundary(false);
        assert!(bridge.lock().terminal_roster.has_pending());
        assert!(bridge.settle_viewport_roster(&applied, true));
        assert!(!bridge.lock().terminal_roster.has_pending());
    }

    #[test]
    fn fresh_roster_state_is_clean_until_facts_are_invalidated() {
        let mut records = NativeHostState::active();

        assert!(!records.viewport_roster_dirty());
        records.invalidate_viewport_roster();
        assert!(records.viewport_roster_dirty());
    }

    #[test]
    fn newer_terminal_roster_obligation_survives_an_older_settlement() {
        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        bridge.quarantine_after_fatal();
        bridge.require_terminal_roster();
        let roster = NativeViewportRosterRecord::for_test([], true);

        bridge.push_terminal_viewport_roster(roster.clone());
        let first = bridge
            .front_viewport_roster()
            .expect("the first terminal roster enters its reserved slot");
        assert!(bridge.acknowledge_viewport_roster(&first));

        bridge.require_terminal_roster();
        for _ in 0..32 {
            assert!(
                !bridge
                    .lock()
                    .record_terminal_viewport_roster(roster.clone())
            );
        }
        assert_eq!(bridge.lock().journal.captured_through(), 1);
        assert!(bridge.settle_viewport_roster(&first, true));
        assert!(bridge.lock().terminal_roster.has_pending());
        bridge.commit_frame_boundary(true);
        assert!(!bridge.lock().terminal_roster.needs_capture());
        bridge.commit_frame_boundary(false);
        assert!(bridge.lock().terminal_roster.needs_capture());

        bridge.push_terminal_viewport_roster(roster);
        let second = bridge
            .front_viewport_roster()
            .expect("the newer obligation reuses the released slot");
        assert!(bridge.acknowledge_viewport_roster(&second));
        assert!(bridge.settle_viewport_roster(&second, true));
        assert!(bridge.lock().terminal_roster.has_pending());
        bridge.commit_frame_boundary(false);
        assert!(!bridge.lock().terminal_roster.has_pending());
    }

    #[test]
    fn freeze_preserves_owned_mailbox_prefix_and_rejects_new_callbacks() {
        let (binding, _) = bindings();
        let viewport = ViewportId::ROOT;
        let mut viewports = NativeViewportMap::default();
        viewports.reserve(viewport, binding).unwrap();
        assert!(viewports.admit_test_create(viewport, binding));
        let rect = NativePhysicalRect::new(10, 20, 800, 600);
        let pending = NativeWindowEventRecord::for_test(
            1,
            WindowId::from(11),
            Some(viewport),
            Some(binding),
            WindowEvent::Focused(true),
        );
        let mut records = NativeHostState::active();
        records.viewports = viewports;
        records
            .journal
            .push_back(HostRecord::WindowEvent(pending.clone()));
        assert!(
            records
                .window_resources
                .reserve_create(viewport, binding, rect)
        );

        records.freeze();

        assert_eq!(records.mode, HostIngressMode::Frozen);
        assert!(matches!(
            records.journal.front(),
            Some(HostRecord::WindowEvent(event)) if *event == pending
        ));
        assert_eq!(
            records.window_resources.create(viewport),
            Some(NativeCreateReservation { binding, rect })
        );
        assert_eq!(
            records.window_resources.hidden_binding(viewport),
            Some(binding)
        );
        let accepted =
            records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
                viewport,
                binding,
                NativeViewportCreateFailureKind::WindowUnavailable,
            ));
        assert!(!accepted);
    }

    #[test]
    fn quarantine_accepts_terminal_callbacks_and_close_requests() {
        let (binding, _) = bindings();
        let viewport = ViewportId::ROOT;
        let mut viewports = NativeViewportMap::default();
        viewports.reserve(viewport, binding).unwrap();
        assert!(viewports.admit_test_create(viewport, binding));
        let window = WindowId::from(11);
        let mut records = NativeHostState::active();
        records.viewports = viewports;
        records.block_semantic_viewport(viewport);
        records.viewports.release_blocked_semantic_viewports();
        let context = eframe::egui::Context::default();
        let visibility_token = eframe::queue_native_viewport_visibility(&context, viewport, false);
        let input_token =
            eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
        assert!(records.window_resources.reserve_visibility_callback(
            viewport,
            binding,
            window,
            visibility_token,
            false,
        ));
        assert!(records.window_resources.reserve_pointer_callback(
            viewport,
            binding,
            window,
            input_token,
            false,
        ));

        assert!(records.quarantine_after_fatal().is_empty());

        assert_eq!(records.mode, HostIngressMode::Quarantined);
        assert!(records.accepts_no_frame_invalidation());
        assert!(!records.viewports.has_semantic_wake());
        assert!(records.accepts_window_event(&WindowEvent::CloseRequested));
        assert!(records.accepts_window_event(&WindowEvent::Destroyed));
        assert!(!records.accepts_window_event(&WindowEvent::Focused(true)));
        assert!(
            records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
                viewport,
                binding,
                NativeViewportCreateFailureKind::WindowUnavailable,
            ),)
        );
        assert!(
            records.record_viewport_visibility(NativeViewportVisibilityRecord::for_test(
                visibility_token,
                viewport,
                window,
                binding,
                false,
                eframe::NativeViewportVisibilityStatus::Dispatched,
            ))
        );
        assert!(records.record_viewport_pointer_passthrough(
            NativePointerPassthroughRecord::for_test(
                input_token,
                viewport,
                window,
                Some(binding),
                false,
                eframe::NativeViewportPointerPassthroughStatus::Applied,
            )
        ));
        assert!(records.terminal_roster.has_pending());

        records.freeze();

        assert_eq!(records.mode, HostIngressMode::Frozen);
        assert!(!records.accepts_no_frame_invalidation());
        assert!(!records.accepts_window_event(&WindowEvent::Destroyed));
        assert!(
            !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
                viewport,
                binding,
                NativeViewportCreateFailureKind::WindowUnavailable,
            ),)
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputRecordDisposition {
    Recorded,
    RecordedService,
    RecordedAfterCurrent,
    Ignored,
    IgnoredAndWake,
    IgnoredAndWakeAfterCurrent,
    ProtocolViolation,
    Inactive,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostIngressMode {
    Active,
    Quarantined,
    Frozen,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostAttachmentState {
    Available,
    Attached,
    Retired,
}

impl HostAttachmentState {
    fn try_attach(&mut self) -> bool {
        if !matches!(self, Self::Available) {
            return false;
        }
        *self = Self::Attached;
        true
    }

    fn detach(&mut self) {
        if matches!(self, Self::Attached) {
            *self = Self::Retired;
        }
    }
}

impl HostIngressMode {
    const fn accepts_new_work(self) -> bool {
        matches!(self, Self::Active)
    }

    const fn accepts_terminal_callbacks(self) -> bool {
        !matches!(self, Self::Frozen)
    }

    fn accepts_window_event(self, event: &winit::event::WindowEvent) -> bool {
        match self {
            Self::Active => NativeWindowEventClass::classify(event).is_dockspace_ingress(),
            Self::Quarantined => matches!(
                event,
                winit::event::WindowEvent::CloseRequested | winit::event::WindowEvent::Destroyed
            ),
            Self::Frozen => false,
        }
    }

    const fn accepts_device_removal(self) -> bool {
        self.accepts_terminal_callbacks()
    }
}

#[derive(Debug)]
struct NativeHostState {
    viewports: NativeViewportMap,
    attachment: HostAttachmentState,
    mode: HostIngressMode,
    admission: NativeAdmission,
    journal: HostIngressJournal,
    output_resources: output_resource::NativeOutputResources,
    input_storage_allowance: Option<capacity::NativeHostInputAllowance>,
    pointer_translation_reserved_bytes: usize,
    snapshot_storage_reserved_bytes: usize,
    window_resource_staging_reserved_bytes: usize,
    host_owner_external_reserved_bytes: usize,
    window_resources: NativeWindowResources,
    retirements: NativeRetirementState,
    close_control: NativeCloseControl,
    roster_context: u64,
    committed_viewport_roster: Option<NativeViewportRosterKey>,
    in_flight_viewport_roster: Option<NativeViewportRosterKey>,
    output_context: Option<NativeOutputToken>,
    last_output_ordinal: u64,
    submitted_output_prefix: usize,
    output_order_invalid: bool,
    event_boundary_pending: bool,
    global_focus_refresh_pending: bool,
    terminal_roster: TerminalRosterState,
}

/// Exclusive claim over an immutable same-route cursor prefix.
///
/// The mailbox lock keeps callbacks and retirement from rewriting membership
/// while translation is admitted by the semantic recorder. Dropping a failed
/// claim leaves its raw records untouched. After retention only the recorder
/// owns the payload; the mailbox keeps the existing commit boundary.
pub(crate) struct CursorPrefixClaim<'records> {
    records: MutexGuard<'records, NativeHostState>,
    len: std::num::NonZeroUsize,
}

impl CursorPrefixClaim<'_> {
    #[cfg(test)]
    pub(crate) const fn record_count(&self) -> std::num::NonZeroUsize {
        self.len
    }

    #[cfg(test)]
    pub(crate) fn events(&self) -> impl Iterator<Item = &NativeWindowEventRecord> {
        self.captured_events().map(|(event, _)| event)
    }

    pub(crate) fn captured_events(
        &self,
    ) -> impl Iterator<Item = (&NativeWindowEventRecord, std::time::Duration)> {
        self.records
            .journal
            .reserved_iter()
            .take(self.len.get())
            .filter_map(|record| {
                if let HostRecord::WindowEvent(event) = record.record() {
                    Some((event, record.captured_at()))
                } else {
                    None
                }
            })
    }

    #[cfg(test)]
    pub(crate) fn retain_prefix(mut self, count: std::num::NonZeroUsize) -> bool {
        if count > self.len {
            return false;
        }
        self.records.journal.transfer_front(count.get());
        self.records.event_boundary_pending = true;
        true
    }

    pub(crate) fn retain_translated_prefix(
        mut self,
        count: std::num::NonZeroUsize,
        translator: &mut NativePointerTranslator,
        candidate: NativePointerTranslator,
    ) -> bool {
        if count > self.len {
            return false;
        }
        // Drop the old backing before returning its overlap promise to callbacks.
        *translator = candidate;
        self.records.pointer_translation_reserved_bytes =
            translator.reserved_translation_capacity_bytes();
        self.records.journal.transfer_front(count.get());
        self.records.event_boundary_pending = true;
        true
    }
}

impl NativeHostState {
    fn workspace_replacement_route(
        &self,
        continuation: NativeRootContinuation,
    ) -> Result<Option<ViewportId>, NativeHostProtocolError> {
        let predecessor = continuation.predecessor();
        match self.viewports.viewport(predecessor.surface()) {
            Some(viewport) if self.viewports.binding(viewport) == Some(predecessor) => {
                Ok(Some(viewport))
            }
            // A same-surface reset can precede the first physical attachment.
            None if predecessor.surface() == continuation.successor().surface() => Ok(None),
            _ => Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                predecessor.surface(),
            )),
        }
    }

    /// The speculative rows carry backing promises, never callback authority.
    /// Holding this lock through publication prevents another owner spending them.
    fn prepare_workspace_replacement(
        &mut self,
        continuations: impl Iterator<Item = NativeRootContinuation>,
    ) -> Result<(), NativeRuntimeError> {
        self.refresh_window_resource_references();
        for continuation in continuations {
            let predecessor = continuation.predecessor();
            let successor = continuation.successor();
            let Some(viewport) = self.workspace_replacement_route(continuation)? else {
                continue;
            };
            self.viewports
                .validate_workspace_root_continuation(viewport, continuation)
                .map_err(|_| {
                    NativeHostProtocolError::NativeAdmissionRouteChanged(predecessor.surface())
                })?;
            let window = self
                .viewports
                .window(viewport)
                .expect("validated same-window replacement retains its physical window");
            if self.retirements.has_quiescence_owner(predecessor)
                || self.retirements.has_quiescence_owner(successor)
                || self.retirements.owns_window(window, None)
            {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    successor.surface(),
                )
                .into());
            }
            if !self.can_admit_binding(successor) {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
            self.window_resources
                .prepare_workspace_replacement(viewport, successor)?;
        }
        Ok(())
    }

    /// Every fallible capacity and receipt check ran under this same lock.
    fn commit_workspace_replacement(&mut self, report: &HostFrameReport) {
        if report.native_root_continuations().is_empty() {
            self.window_resources.abort_workspace_replacement();
            return;
        }
        for &continuation in report.native_root_continuations() {
            let predecessor = continuation.predecessor();
            let successor = continuation.successor();
            let Some(viewport) = self
                .workspace_replacement_route(continuation)
                .expect("the prepaid exact predecessor retains its viewport")
            else {
                continue;
            };
            self.viewports
                .commit_workspace_root_continuation(viewport, continuation);
            self.window_resources
                .retain_workspace_predecessor(viewport, predecessor, successor);
            self.window_resources.retire(viewport, predecessor);
            self.window_resources
                .commit_host_reference(viewport, successor);
            self.retirements.commit_detachment(predecessor);
        }
        self.refresh_window_resource_references();
        self.invalidate_viewport_roster();
    }

    fn refresh_window_resource_references(&mut self) {
        let Self {
            viewports,
            output_resources,
            window_resources,
            ..
        } = self;
        window_resources.refresh_host_references(|viewport, binding| {
            viewports.binding(viewport) == Some(binding)
                || output_resources.iter().any(|(token, resource)| {
                    token.viewport_id() == viewport
                        && resource.reservation.binding() == Some(binding)
                })
        });
    }

    fn prepare_window_resource_reference(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        self.refresh_window_resource_references();
        self.window_resources
            .prepare_host_reference(viewport, binding)
            .map_err(|_| NativeViewportBindingError::CapacityExceeded {
                limit: self
                    .window_resources
                    .resource_slot_limit()
                    .unwrap_or(usize::MAX),
            })
    }

    fn active() -> Self {
        Self::with_clock(NativeViewportMap::default(), NativeMonotonicClock::system())
    }

    fn with_clock(viewports: NativeViewportMap, clock: NativeMonotonicClock) -> Self {
        Self::try_with_clock(viewports, clock).expect("native test host storage is available")
    }

    fn try_with_clock(
        viewports: NativeViewportMap,
        clock: NativeMonotonicClock,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::try_with_clock_and_capacity(
            viewports,
            clock,
            terminal_roster::DEFAULT_WINDOW_CAPACITY,
            terminal_roster::DEFAULT_WORK_AREA_CAPACITY,
        )
    }

    fn try_with_clock_and_capacity(
        viewports: NativeViewportMap,
        clock: NativeMonotonicClock,
        windows: usize,
        work_areas: usize,
    ) -> Result<Self, NativeHostProtocolError> {
        NativeHostBridge::initial_storage_bytes(windows, work_areas)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let journal = HostIngressJournal::try_new(clock)?;
        let output_resources = output_resource::NativeOutputResources::try_new()?;
        let terminal_roster = TerminalRosterState::try_with_capacity(windows, work_areas)?;
        Ok(Self {
            viewports,
            attachment: HostAttachmentState::Available,
            mode: HostIngressMode::Active,
            admission: NativeAdmission::default(),
            journal,
            output_resources,
            input_storage_allowance: None,
            pointer_translation_reserved_bytes: NativePointerTranslator::default()
                .reserved_translation_capacity_bytes(),
            snapshot_storage_reserved_bytes: 0,
            window_resource_staging_reserved_bytes: 0,
            host_owner_external_reserved_bytes: 0,
            window_resources: NativeWindowResources::default(),
            retirements: NativeRetirementState::default(),
            close_control: NativeCloseControl::default(),
            roster_context: 0,
            committed_viewport_roster: None,
            in_flight_viewport_roster: None,
            output_context: None,
            last_output_ordinal: 0,
            submitted_output_prefix: 0,
            output_order_invalid: false,
            event_boundary_pending: false,
            global_focus_refresh_pending: false,
            terminal_roster,
        })
    }

    fn retained_record_count(&self) -> usize {
        self.journal
            .retained_record_count()
            .saturating_add(self.close_control.parked_ingress().count())
            .saturating_add(self.owner_ingress().count())
    }

    fn freeze_ordinary(&mut self) -> u64 {
        let was_running = matches!(self.admission, NativeAdmission::Running);
        let boundary = self.admission.freeze(self.journal.captured_through());
        if was_running {
            // Core may already own effect, presentation or atomic replay input
            // which is not represented by a ready raw callback.
            self.event_boundary_pending = true;
            self.terminal_roster.require();
        }
        boundary
    }

    fn admit_ordinary(&mut self) -> bool {
        self.admit_ordinary_with_data(0)
    }

    fn admit_ordinary_with_data(&mut self, additional: usize) -> bool {
        if !self.journal.can_push_raw()
            || !self.output_resources.can_reserve_ordinary()
            || !self.can_reserve_input_data(additional)
            || self.retained_record_count() >= admission::RECORD_LIMIT
            || self
                .oldest_record_age()
                .is_some_and(|age| age >= admission::AGE_LIMIT)
        {
            self.freeze_ordinary();
            return false;
        }
        self.mode.accepts_new_work() && matches!(self.admission, NativeAdmission::Running)
    }

    fn admit_pointer_callback(&mut self, payload_bytes: usize) -> bool {
        let Some(additional) = payload_bytes
            .checked_add(NativePointerTranslator::callback_translation_capacity_bytes())
        else {
            self.freeze_ordinary();
            return false;
        };
        self.admit_ordinary_with_data(additional)
    }

    fn dispatch(&self) -> NativeDispatch {
        self.admission.dispatch(
            self.event_boundary_pending
                || self.in_flight_viewport_roster.is_some()
                || self.close_control.has_unsettled_clear(),
            self.front_ingress().map(ReservedHostRecord::ordinal),
        )
    }

    fn dispatch_cutoff(&self) -> Option<u64> {
        match self.admission {
            NativeAdmission::Draining { boundary } => Some(boundary),
            NativeAdmission::Running | NativeAdmission::Recovering { .. } => None,
        }
    }

    fn front_dispatchable(&self) -> bool {
        self.front_ingress().is_some_and(|record| {
            self.dispatch_cutoff()
                .is_none_or(|cutoff| record.ordinal() <= cutoff)
        })
    }

    fn oldest_record_age(&self) -> Option<std::time::Duration> {
        let deferred = self
            .close_control
            .parked_ingress()
            .map(|record| self.journal.age_since(record.captured_at()))
            .max();
        let callback = self
            .owner_ingress()
            .map(|record| self.journal.age_since(record.captured_at()))
            .max();
        self.journal
            .oldest_age()
            .into_iter()
            .chain(deferred)
            .chain(callback)
            .max()
    }

    fn owner_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.window_resources
            .callback_ingress()
            .chain(self.close_control.callback_ingress())
            .chain(self.viewports.create_ingress())
            .chain(self.viewports.window_ingress())
            .chain(self.retirements.create_ingress())
            .chain(self.retirements.window_ingress())
            .chain(self.terminal_roster.ingress())
            .chain(
                self.output_resources
                    .values()
                    .filter_map(NativeOutputResource::terminal_ingress),
            )
    }

    fn first_owner_ingress(&self) -> Option<&ReservedHostRecord> {
        self.owner_ingress().min_by_key(|record| record.ordinal())
    }

    fn front_ingress(&self) -> Option<&ReservedHostRecord> {
        match (self.journal.front_reserved(), self.first_owner_ingress()) {
            (Some(raw), Some(callback)) if callback.ordinal() < raw.ordinal() => Some(callback),
            (Some(raw), _) => Some(raw),
            (None, callback) => callback,
        }
    }

    fn front_record(&self) -> Option<&HostRecord> {
        self.front_ingress().map(ReservedHostRecord::record)
    }

    fn front_dispatch_record(&self) -> Option<&HostRecord> {
        self.front_dispatchable()
            .then(|| self.front_record())
            .flatten()
    }

    fn raw_prefix(&self) -> impl Iterator<Item = &HostRecord> {
        let owner_cutoff = self.first_owner_ingress().map(ReservedHostRecord::ordinal);
        let admission_cutoff = self.dispatch_cutoff();
        self.journal
            .reserved_iter()
            .take_while(move |record| {
                owner_cutoff.is_none_or(|cutoff| record.ordinal() < cutoff)
                    && admission_cutoff.is_none_or(|cutoff| record.ordinal() <= cutoff)
            })
            .map(ReservedHostRecord::record)
    }

    /// Resource slots share the journal clock without allocating another queue.
    #[cfg(test)]
    fn ordered_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        let mut raw = self.journal.reserved_iter().peekable();
        let mut after = 0;
        std::iter::from_fn(move || {
            let owned = self
                .owner_ingress()
                .filter(|record| record.ordinal() > after)
                .min_by_key(|record| record.ordinal());
            let next = match (raw.peek().copied(), owned) {
                (Some(raw), Some(owned)) if owned.ordinal() < raw.ordinal() => owned,
                (Some(_), _) => raw.next().expect("the peeked raw ingress remains present"),
                (None, Some(owned)) => owned,
                (None, None) => return None,
            };
            after = next.ordinal();
            Some(next)
        })
    }

    fn first_output_barrier(&self, cutoff: Option<u64>) -> Option<u64> {
        self.journal
            .reserved_iter()
            .chain(self.owner_ingress())
            .filter(|ingress| {
                cutoff.is_none_or(|cutoff| ingress.ordinal() <= cutoff)
                    && !matches!(ingress.record(), HostRecord::Output { .. })
            })
            .map(ReservedHostRecord::ordinal)
            .min()
    }

    fn output_prefix_range(&self, limit: usize) -> NativeOutputPrefix {
        let limit = limit.min(OUTPUT_PREFIX_CAPACITY);
        let mut prefix = NativeOutputPrefix::default();
        if limit == 0 {
            return prefix;
        }
        let cutoff = self.dispatch_cutoff();
        let barrier = self.first_output_barrier(cutoff);
        let mut first = None;
        let mut ready = 0usize;
        let mut submitted = 0;
        let mut submitted_through = 0;
        for (resource, ingress) in self.output_resources.terminal_ingress() {
            let ordinal = ingress.ordinal();
            if cutoff.is_some_and(|cutoff| ordinal > cutoff)
                || barrier.is_some_and(|barrier| ordinal >= barrier)
            {
                continue;
            }
            if resource.submitted {
                submitted += 1;
                submitted_through = submitted_through.max(ordinal);
            } else {
                first = Some(first.map_or(ordinal, |first: u64| first.min(ordinal)));
                prefix.through = prefix.through.max(ordinal);
                ready += 1;
            }
        }
        // Submitted owners remain the exact leading capture prefix until commit.
        let Some(first) = first else {
            return prefix;
        };
        if submitted != self.submitted_output_prefix || submitted_through >= first {
            return NativeOutputPrefix::default();
        }

        if ready <= limit && prefix.through - first + 1 == ready as u64 {
            // Output-only ingress has contiguous capture ordinals. Use those
            // ordinals to place the common complete prefix without sorting.
            for (resource, ingress) in self.output_resources.terminal_ingress() {
                let ordinal = ingress.ordinal();
                if resource.submitted
                    || ordinal < first
                    || ordinal > prefix.through
                    || cutoff.is_some_and(|cutoff| ordinal > cutoff)
                    || barrier.is_some_and(|barrier| ordinal >= barrier)
                {
                    continue;
                }
                let HostRecord::Output { token } = ingress.record() else {
                    return NativeOutputPrefix::default();
                };
                let index = usize::try_from(ordinal - first)
                    .expect("a bounded output ordinal difference fits usize");
                if index >= ready || prefix.outputs[index].replace((ordinal, *token)).is_some() {
                    return NativeOutputPrefix::default();
                }
            }
            if prefix.outputs[..ready].iter().any(Option::is_none) {
                return NativeOutputPrefix::default();
            }
            prefix.remaining = ready;
            return prefix;
        }

        // A shorter budget or a non-contiguous stream selects only its first
        // bounded ordinals. This avoids building a heap index or sorting every
        // retained output when a caller needs only a small dispatch slice.
        let mut after = first.saturating_sub(1);
        let selected = limit.min(ready);
        for index in 0..selected {
            let Some((_, ingress)) = self
                .output_resources
                .terminal_ingress()
                .filter(|(resource, ingress)| {
                    !resource.submitted
                        && ingress.ordinal() > after
                        && ingress.ordinal() <= prefix.through
                        && cutoff.is_none_or(|cutoff| ingress.ordinal() <= cutoff)
                        && barrier.is_none_or(|barrier| ingress.ordinal() < barrier)
                })
                .min_by_key(|(_, ingress)| ingress.ordinal())
            else {
                return NativeOutputPrefix::default();
            };
            let HostRecord::Output { token } = ingress.record() else {
                return NativeOutputPrefix::default();
            };
            after = ingress.ordinal();
            prefix.outputs[index] = Some((after, *token));
        }
        prefix.remaining = selected;
        prefix
    }

    fn transfer_front_record(&mut self) {
        let ordinal = self
            .front_ingress()
            .expect("the exact front record was checked before transfer")
            .ordinal();
        if let Some(record) = self
            .window_resources
            .take_callback_ingress(ordinal)
            .or_else(|| self.close_control.take_callback_ingress(ordinal))
            .or_else(|| self.viewports.take_create_ingress(ordinal))
            .or_else(|| self.viewports.take_window_ingress(ordinal))
            .or_else(|| self.retirements.take_create_ingress(ordinal))
            .or_else(|| self.retirements.take_window_ingress(ordinal))
            .or_else(|| self.terminal_roster.take_ingress(ordinal))
        {
            self.journal.transfer_reserved(record);
        } else {
            assert_eq!(
                self.journal.front_ordinal(),
                Some(ordinal),
                "only the exact raw or resource-owned head can transfer"
            );
            self.journal.transfer_front(1);
        }
    }

    fn can_reduce_output_prefix(&self) -> bool {
        // Only terminals validated by accept_output_order reach output ingress.
        // Quarantine drains that FIFO without clearing the original violation.
        !self.output_order_invalid || matches!(self.mode, HostIngressMode::Quarantined)
    }

    fn has_pending_coordinator_work(&self) -> bool {
        !matches!(self.admission, NativeAdmission::Running)
            || self.retained_record_count() != 0
            || self.close_control.has_pending_work()
            || self.window_resources.has_pending_work()
            || self.retirements.has_pending_work()
            || self.in_flight_viewport_roster.is_some()
            || !self.can_reduce_output_prefix()
            || self.event_boundary_pending
            || self.viewports.has_semantic_wake()
            || self.terminal_roster.has_pending()
    }

    fn reserve_output(
        &mut self,
        token: NativeOutputToken,
        binding: Option<NativeSurfaceBinding>,
        budget: NativeInputBudget,
    ) -> bool {
        if matches!(self.mode, HostIngressMode::Frozen)
            || self.output_resources.contains_key(&token)
        {
            return false;
        }
        let output_capacity_available = self.can_reserve_output_data();
        if !output_capacity_available {
            self.freeze_ordinary();
        }
        let wake_after_current = binding.is_some_and(|binding| {
            self.window_resources.hidden_binding(token.viewport_id()) == Some(binding)
        });
        let prepaid_output = !output_capacity_available
            || matches!(self.admission, NativeAdmission::Draining { .. })
            || self.terminal_roster.has_pending();
        let mut reservation = if prepaid_output {
            let promised_staging = binding.filter(|binding| {
                token.viewport_id() != ViewportId::ROOT
                    && self.mode.accepts_new_work()
                    && self
                        .window_resources
                        .staging(token.viewport_id())
                        .is_some_and(|request| request.binding() == *binding)
            });
            promised_staging.map_or_else(
                || OutputReservation::diagnostic(binding),
                OutputReservation::bound,
            )
        } else {
            match self.mode {
                HostIngressMode::Active => {
                    binding.map_or_else(OutputReservation::unbound, OutputReservation::bound)
                }
                HostIngressMode::Quarantined => {
                    let terminal_create = binding.filter(|binding| {
                        token.create_attempt().is_some()
                            && self
                                .window_resources
                                .create(token.viewport_id())
                                .is_some_and(|reservation| reservation.binding == *binding)
                    });
                    terminal_create.map_or_else(
                        || OutputReservation::diagnostic(binding),
                        OutputReservation::terminal_create,
                    )
                }
                HostIngressMode::Frozen => unreachable!("frozen outputs return before reservation"),
            }
        };
        if wake_after_current {
            let armed = reservation.require_wake_after_current();
            debug_assert!(armed, "a new output can retain its wake obligation");
        }
        let resource = NativeOutputResource::new(reservation, budget);
        if prepaid_output || resource.reservation.abandoned {
            return self.output_resources.insert_control(token, resource);
        }
        self.output_resources.insert(token, resource).is_ok()
    }

    fn attach_output_binding(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if self
            .prepare_window_resource_reference(token.viewport_id(), binding)
            .is_err()
        {
            return false;
        }
        let Some(reservation) = self.output_resources.get_mut(&token) else {
            return false;
        };
        if !reservation.reservation.attach(binding) {
            return false;
        }
        self.window_resources
            .commit_host_reference(token.viewport_id(), binding);
        true
    }

    fn require_output_wake_after_current(&mut self, token: NativeOutputToken) -> bool {
        let Some(reservation) = self.output_resources.get_mut(&token) else {
            return false;
        };
        reservation.reservation.require_wake_after_current()
    }

    fn record_output(&mut self, result: NativeOutputResult) -> OutputRecordDisposition {
        if !self.mode.accepts_terminal_callbacks() {
            return OutputRecordDisposition::Inactive;
        }
        let Some(reservation) = self.output_resources.get(&result.token()) else {
            self.output_order_invalid = true;
            return OutputRecordDisposition::ProtocolViolation;
        };
        if reservation.reservation.terminal_recorded || !self.accept_output_order(result) {
            self.output_order_invalid = true;
            return OutputRecordDisposition::ProtocolViolation;
        }
        let reservation = self
            .output_resources
            .get_mut(&result.token())
            .expect("validated output reservation remains present");
        let requires_service = reservation.terminal_requires_repaint(result.status());
        reservation.record_terminal(result);
        let metadata = reservation.reservation;
        if metadata.abandoned {
            let wake = metadata.wakes_on_terminal();
            let wake_after_current = metadata.wakes_after_current();
            self.output_resources.remove(&result.token());
            self.refresh_window_resource_references();
            return match (wake, wake_after_current) {
                (true, true) => OutputRecordDisposition::IgnoredAndWakeAfterCurrent,
                (true, false) => OutputRecordDisposition::IgnoredAndWake,
                (false, true) => OutputRecordDisposition::IgnoredAndWakeAfterCurrent,
                (false, false) => OutputRecordDisposition::Ignored,
            };
        }
        let wake_after_current = metadata.wakes_after_current();
        let ingress = self.journal.reserve(HostRecord::Output {
            token: result.token(),
        });
        assert!(
            reservation.retain_terminal_ingress(ingress),
            "the validated output retains its exact terminal reservation"
        );
        if wake_after_current {
            OutputRecordDisposition::RecordedAfterCurrent
        } else if !requires_service {
            // Ordinary successful paint settles in a host-service event.
            // It must not schedule another application paint or leave its
            // original terminal envelope parked until future user input.
            OutputRecordDisposition::RecordedService
        } else {
            OutputRecordDisposition::Recorded
        }
    }

    fn accept_output_order(&mut self, result: NativeOutputResult) -> bool {
        match self.output_context {
            Some(context) if !context.same_context(result.token()) => return false,
            Some(_) => {}
            None => self.output_context = Some(result.token()),
        }
        let ordinal = result.ordinal().get();
        if !is_next_output_ordinal(self.last_output_ordinal, ordinal) {
            return false;
        }
        self.last_output_ordinal = ordinal;
        true
    }

    fn record_viewport_create_failure(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        if self.viewports.receive_create_terminal(
            failure.viewport(),
            failure.binding(),
            failure.attempt(),
            CreateTerminal::Failed(failure.kind()),
            || {
                self.journal
                    .reserve(HostRecord::ViewportCreateFailed(failure))
            },
        ) != CreateAdmission::First
        {
            return false;
        }
        true
    }

    fn record_viewport_visibility(&mut self, record: NativeViewportVisibilityRecord) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        let admission = self
            .window_resources
            .accept_visibility_callback(record, |binding| {
                self.journal.reserve(HostRecord::ViewportVisibility(
                    record.qualified_for_binding(binding),
                ))
            });
        if admission == CommandCallbackAdmission::Duplicate {
            return false;
        }
        if admission == CommandCallbackAdmission::Unreserved && !self.admit_ordinary() {
            return false;
        }
        if matches!(self.mode, HostIngressMode::Quarantined)
            || !matches!(self.admission, NativeAdmission::Running)
        {
            self.terminal_roster.require();
        }
        if admission == CommandCallbackAdmission::Unreserved {
            return self
                .journal
                .push_back(HostRecord::ViewportVisibility(record));
        }
        true
    }

    fn record_global_focus(&mut self, record: NativeGlobalFocusRecord) -> bool {
        let admitted =
            self.admit_ordinary() && self.journal.push_back(HostRecord::GlobalFocus(record));
        if admitted {
            self.global_focus_refresh_pending = false;
        } else if self.mode.accepts_new_work() {
            self.global_focus_refresh_pending = true;
        }
        admitted
    }

    fn record_viewport_focus(&mut self, record: NativeViewportFocusRecord) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        let admission = self
            .window_resources
            .accept_focus_callback(record, |binding| {
                self.journal.reserve(HostRecord::ViewportFocus(
                    record.qualified_for_binding(binding),
                ))
            });
        if admission == CommandCallbackAdmission::Duplicate {
            return false;
        }
        if admission == CommandCallbackAdmission::Unreserved {
            if !self.admit_ordinary() {
                return false;
            }
            return self.journal.push_back(HostRecord::ViewportFocus(record));
        }
        true
    }

    fn record_viewport_pointer_passthrough(
        &mut self,
        record: NativePointerPassthroughRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        let admission = self
            .window_resources
            .accept_pointer_callback(record, |binding| {
                self.journal.reserve(HostRecord::ViewportPointerPassthrough(
                    record.qualified_for_binding(binding),
                ))
            });
        if admission == CommandCallbackAdmission::Duplicate {
            return false;
        }
        if admission == CommandCallbackAdmission::Unreserved && !self.admit_ordinary() {
            return false;
        }
        if matches!(self.mode, HostIngressMode::Quarantined)
            || !matches!(self.admission, NativeAdmission::Running)
        {
            self.terminal_roster.require();
        }
        if admission == CommandCallbackAdmission::Unreserved {
            return self
                .journal
                .push_back(HostRecord::ViewportPointerPassthrough(record));
        }
        true
    }

    fn record_property_retirement(
        &mut self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        self.window_resources
            .retire_property_command(retirement, |record| self.journal.reserve(record))
            == CommandCallbackAdmission::First
    }

    fn record_viewport_created(&mut self, record: NativeViewportCreatedRecord) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        // This callback proves a real window before the root consumes its
        // create acknowledgement. Reserve its terminal owner immediately.
        if !self
            .retirements
            .can_retain_window(record.binding, record.token.window_id())
        {
            return false;
        }
        if self
            .viewports
            .capture_output_window(
                record.token.viewport_id(),
                record.binding,
                record.token.window_id(),
                record.token.create_attempt(),
            )
            .is_err()
        {
            return false;
        }
        if self.viewports.receive_create_terminal(
            record.token.viewport_id(),
            record.binding,
            record.token.create_attempt(),
            CreateTerminal::Created(record.token.window_id()),
            || self.journal.reserve(HostRecord::ViewportCreated(record)),
        ) != CreateAdmission::First
        {
            return false;
        }
        if matches!(self.mode, HostIngressMode::Quarantined)
            || !matches!(self.admission, NativeAdmission::Running)
        {
            self.terminal_roster.require();
        }
        true
    }

    fn semantic_prelude_ready(&self) -> bool {
        !self.event_boundary_pending
            && deferred_semantic_prelude_ready(self.front_record().into_iter())
    }

    fn abandon_output(&mut self, token: NativeOutputToken) -> bool {
        self.abandon_output_with_terminal_wake(token, true)
    }

    fn abandon_output_with_terminal_wake(
        &mut self,
        token: NativeOutputToken,
        wake_on_terminal: bool,
    ) -> bool {
        if self
            .output_resources
            .get(&token)
            .is_some_and(|resource| resource.submitted)
        {
            return false;
        }
        if let Some(PendingNativeOutput::Staging(request)) = self
            .output_resources
            .get(&token)
            .and_then(|resource| resource.paint.as_ref())
        {
            let request = *request;
            self.window_resources
                .finish_preparation_paint(token.viewport_id(), request, false);
            self.output_resources
                .get_mut(&token)
                .expect("the output is retained")
                .paint
                .take();
            self.invalidate_viewport_roster();
        }
        if self
            .output_resources
            .get(&token)
            .is_some_and(|resource| resource.terminal_ingress().is_some())
        {
            self.output_resources.remove(&token);
            self.refresh_window_resource_references();
            return true;
        }
        let Some(resource) = self.output_resources.get_mut(&token) else {
            return false;
        };
        if wake_on_terminal {
            resource.reservation.abandon()
        } else {
            resource.reservation.abandon_silently()
        }
    }

    fn settle_output_abandonment(
        &mut self,
        token: NativeOutputToken,
        wake_on_terminal: bool,
    ) -> OutputAbandonment {
        let Some(resource) = self.output_resources.get_mut(&token) else {
            return OutputAbandonment::MissingOrTerminal;
        };
        resource.reservation.settle_abandonment(wake_on_terminal)
    }

    const fn accepts_no_frame_invalidation(&self) -> bool {
        !matches!(self.mode, HostIngressMode::Frozen)
    }

    fn freeze(&mut self) {
        self.mode = HostIngressMode::Frozen;
    }

    fn enter_quarantine(&mut self) {
        if !matches!(self.mode, HostIngressMode::Frozen) {
            self.mode = HostIngressMode::Quarantined;
        }
        // Quarantine cannot accept another staging or semantic paint. Release
        // adapter-owned wake obligations here; exact output reservations and
        // binding retirement continue to carry terminal lifetime authority.
        if self.window_resources.clear_staging() {
            // Only an actual preparation cancellation changes this roster's
            // context. Callback-only or undeclared routes owe no new snapshot.
            self.invalidate_viewport_roster();
        }
        self.viewports.clear_semantic_wakes();
        if self
            .journal
            .iter()
            .chain(self.owner_ingress().map(ReservedHostRecord::record))
            .any(HostRecord::requires_terminal_roster)
        {
            self.terminal_roster.require();
        }
    }

    fn claim_quarantine_output(
        &mut self,
    ) -> Option<(NativeOutputToken, Option<PendingNativeOutput>)> {
        let token = self
            .output_resources
            .iter()
            .filter(|(_, resource)| {
                (!resource.reservation.terminal_recorded && !resource.reservation.abandoned)
                    || resource.terminal.is_some_and(|terminal| {
                        output_terminal_requires_adapter_abandonment(
                            terminal.status(),
                            resource.submitted,
                            resource.paint.is_some(),
                        )
                    })
            })
            .map(|(token, _)| *token)
            .min()?;
        let terminal = self
            .output_resources
            .get(&token)
            .expect("the selected exact output remains present")
            .reservation
            .terminal_recorded;
        if terminal {
            assert!(
                self.abandon_output(token),
                "an unattached negative terminal retains its original ingress owner"
            );
            return Some((token, None));
        }
        let resource = self
            .output_resources
            .get_mut(&token)
            .expect("the selected exact output remains present");
        let abandoned = resource.reservation.abandon();
        assert!(
            abandoned,
            "one quarantine claim owns the output abandonment"
        );
        Some((token, resource.paint.take()))
    }

    #[cfg(test)]
    fn quarantine_after_fatal(&mut self) -> Vec<NativeOutputToken> {
        self.enter_quarantine();
        std::iter::from_fn(|| self.claim_quarantine_output().map(|(token, _)| token)).collect()
    }

    fn require_terminal_roster(&mut self) {
        self.terminal_roster.require();
    }

    fn accepts_window_event(&self, event: &winit::event::WindowEvent) -> bool {
        self.mode.accepts_window_event(event)
    }

    fn record_window_event(&mut self, record: NativeWindowEventRecord) -> bool {
        if matches!(
            (self.mode, record.event()),
            (
                HostIngressMode::Quarantined,
                winit::event::WindowEvent::Destroyed
            )
        ) {
            self.terminal_roster.require();
        }
        self.journal.push_back(HostRecord::WindowEvent(record))
    }

    fn capture_window_event(&mut self, mut record: NativeWindowEventRecord) -> bool {
        if NativeWindowEventClass::classify(record.event()) == NativeWindowEventClass::Pointer {
            return self.admit_pointer_callback(record.owned_descendant_capacity_bytes())
                && self.record_window_event(record);
        }
        if matches!(record.event(), winit::event::WindowEvent::CloseRequested)
            && record
                .binding()
                .is_some_and(|binding| self.close_control.retains_request(&record, binding))
        {
            return true;
        }
        let reserved_close = matches!(record.event(), winit::event::WindowEvent::CloseRequested)
            && record
                .binding()
                .is_some_and(|binding| self.close_control.references_binding(binding));
        if !matches!(record.event(), winit::event::WindowEvent::Destroyed)
            && !reserved_close
            && !self.admit_ordinary()
        {
            return false;
        }
        if matches!(record.event(), winit::event::WindowEvent::CloseRequested)
            && let Some(binding) = record.binding()
        {
            match self.close_control.capture_request(&record, binding) {
                NativeCloseCaptureAdmission::Conflict => return false,
                NativeCloseCaptureAdmission::Duplicate => return true,
                NativeCloseCaptureAdmission::First => {
                    if self.close_control.needs_request_recording(&record, binding) {
                        return self.record_window_event(record);
                    }
                    let ingress = self.journal.reserve(HostRecord::WindowEvent(record));
                    self.close_control
                        .retain_successor_ingress(binding, ingress)
                        .expect("a first successor capture owns its sole ingress reservation");
                    return true;
                }
            }
        }
        if matches!(record.event(), winit::event::WindowEvent::Destroyed) {
            let window = record.window_id();
            let viewport = record.viewport_id();
            let ordinal = record.ordinal();
            let reserve = |owner_viewport, binding| {
                record.qualify_destroyed(owner_viewport, binding);
                self.journal.reserve(HostRecord::WindowEvent(record))
            };
            let received = if self.retirements.owns_window(window, viewport) {
                self.retirements
                    .receive_destroyed(window, viewport, ordinal, reserve)
            } else {
                self.viewports
                    .receive_window_destroyed(window, viewport, ordinal, reserve)
            };
            let Some((viewport, binding)) = received else {
                return false;
            };
            self.viewports.suppress_pointer_route(viewport, binding);
            if matches!(self.mode, HostIngressMode::Quarantined)
                || !matches!(self.admission, NativeAdmission::Running)
            {
                self.terminal_roster.require();
            }
            return true;
        }
        self.record_window_event(record)
    }

    fn record_device_removal(&mut self, record: NativeDeviceRemovalRecord) -> bool {
        let ordinary = self.admit_ordinary();
        // Quarantine may still terminate an existing provider tail, within the
        // same raw capacity and fixed admission boundary as every other source.
        let terminal = matches!(self.mode, HostIngressMode::Quarantined)
            && matches!(self.admission, NativeAdmission::Running);
        (ordinary || terminal) && self.journal.push_back(HostRecord::DeviceRemoval(record))
    }

    fn coalesce_idle_cursor_prefix(&mut self, budget: &mut NativeInputBudget) -> bool {
        let Some(HostRecord::WindowEvent(first)) = self.front_record() else {
            return false;
        };
        let Some(latest_cursor_index) = idle_cursor_prefix_latest_index(
            self.raw_prefix()
                .take(budget.remaining_records())
                .take_while(|_| !budget.is_exhausted())
                .map(|record| match record {
                    HostRecord::WindowEvent(event) if first.shares_cursor_lane(event) => {
                        IdleCursorPrefixEntry::Cursor
                    }
                    HostRecord::Output { .. } => IdleCursorPrefixEntry::Output,
                    HostRecord::WindowEvent(_)
                    | HostRecord::DeviceRemoval(_)
                    | HostRecord::GlobalFocus(_)
                    | HostRecord::ViewportFocus(_)
                    | HostRecord::ViewportPointerPassthrough(_)
                    | HostRecord::PointerObservation(_)
                    | HostRecord::PointerCapture(_)
                    | HostRecord::ViewportCloseCancelled(_)
                    | HostRecord::ViewportRoster(_)
                    | HostRecord::ViewportCreateFailed(_)
                    | HostRecord::ViewportVisibility(_)
                    | HostRecord::ViewportCreated(_) => IdleCursorPrefixEntry::Boundary,
                }),
        ) else {
            return false;
        };

        // Renderer terminals are causal boundaries. Only obsolete idle hover
        // observations before the first output or other callback may
        // disappear; output-separated cursor observations remain lossless.
        for _ in 0..latest_cursor_index {
            self.journal
                .pop_front()
                .expect("the scanned cursor prefix remains present");
        }
        budget.complete_unsequenced_records(latest_cursor_index);
        true
    }

    fn block_semantic_viewport(&mut self, viewport: ViewportId) {
        self.viewports.block_semantic_viewport(viewport);
    }

    fn release_semantic_viewports_if_ready(&mut self) {
        if self.semantic_prelude_ready() {
            self.viewports.release_blocked_semantic_viewports();
        }
    }

    #[cfg(test)]
    fn take_ready_semantic_viewports(&mut self) -> Vec<ViewportId> {
        std::iter::from_fn(|| self.viewports.claim_ready_semantic_viewport()).collect()
    }

    fn invalidate_viewport_roster(&mut self) {
        self.roster_context = self
            .roster_context
            .checked_add(1)
            .expect("native viewport roster context exhausted");
    }

    fn viewport_roster_known_borrowed(&self, roster: NativeViewportRoster<'_>) -> bool {
        let known = |key: &NativeViewportRosterKey| {
            key.context == self.roster_context
                && key.roster.matches_borrowed(
                    roster,
                    &self.viewports,
                    self.terminal_roster.work_area_capacity(),
                )
        };
        self.committed_viewport_roster.as_ref().is_some_and(known)
            || self.in_flight_viewport_roster.as_ref().is_some_and(known)
            || self
                .journal
                .iter()
                .chain(self.owner_ingress().map(ReservedHostRecord::record))
                .any(|record| matches!(record, HostRecord::ViewportRoster(envelope) if known(&envelope.key)))
    }

    fn defer_ordinary_roster_capture(&mut self) {
        if self.terminal_roster.window_capacity_exceeded() {
            self.freeze_ordinary();
        } else if !self.terminal_roster.has_pending() {
            // A baseline and an accepted snapshot can borrow both backings.
            // Drain that snapshot and recapture fresh framework facts through
            // the existing protected receipt without cancelling the gesture.
            self.terminal_roster.require();
        }
    }

    fn record_viewport_roster(
        &mut self,
        roster: impl Into<Arc<NativeViewportRosterRecord>>,
    ) -> bool {
        let roster = roster.into();
        if roster.observation_count() > self.terminal_roster.window_capacity() {
            self.freeze_ordinary();
            return false;
        }
        if !self.admit_ordinary() {
            return false;
        }
        let prepaid_payload = self.terminal_roster.owns_payload(&roster);
        let mut envelope = NativeViewportRosterEnvelope::from_shared(
            self.roster_context,
            roster,
            matches!(self.mode, HostIngressMode::Active),
            None,
        );
        envelope.prepaid_payload = prepaid_payload;
        let already_known = self.committed_viewport_roster.as_ref() == Some(&envelope.key)
            || self.in_flight_viewport_roster.as_ref() == Some(&envelope.key)
            || self.journal.iter().any(
                |record| matches!(record, HostRecord::ViewportRoster(queued) if queued.key == envelope.key),
            );
        if already_known {
            return false;
        }
        self.journal.push_back(HostRecord::ViewportRoster(envelope))
    }

    fn record_terminal_viewport_roster(
        &mut self,
        roster: impl Into<Arc<NativeViewportRosterRecord>>,
    ) -> bool {
        let roster = roster.into();
        if roster.observation_count() > self.terminal_roster.window_capacity() {
            return false;
        }
        let prepaid_payload = self.terminal_roster.owns_payload(&roster);
        self.terminal_roster.capture(|generation| {
            let mut envelope = NativeViewportRosterEnvelope::from_shared(
                self.roster_context,
                roster,
                true,
                Some(generation),
            );
            envelope.prepaid_payload = prepaid_payload;
            self.journal.reserve(HostRecord::ViewportRoster(envelope))
        })
    }

    fn front_viewport_roster(&self) -> Option<NativeViewportRosterEnvelope> {
        let HostRecord::ViewportRoster(envelope) = self.front_dispatch_record()? else {
            return None;
        };
        Some(envelope.clone())
    }

    fn acknowledge_viewport_roster(&mut self, expected: &NativeViewportRosterEnvelope) -> bool {
        let matches = self.front_viewport_roster().is_some_and(|front| {
            front.key == expected.key
                && front.commit_baseline == expected.commit_baseline
                && front.terminal_generation == expected.terminal_generation
        });
        if !matches {
            return false;
        }
        self.transfer_front_record();
        self.event_boundary_pending = true;
        if expected.commit_baseline {
            debug_assert!(self.in_flight_viewport_roster.is_none());
            self.in_flight_viewport_roster = Some(expected.key.clone());
        }
        true
    }

    fn settle_viewport_roster(
        &mut self,
        expected: &NativeViewportRosterEnvelope,
        applied: bool,
    ) -> bool {
        if expected.commit_baseline
            && self.in_flight_viewport_roster.as_ref() != Some(&expected.key)
        {
            return false;
        }
        if let Some(generation) = expected.terminal_generation
            && !self.terminal_roster.can_settle(generation)
        {
            return false;
        }
        if expected.commit_baseline {
            let key = self
                .in_flight_viewport_roster
                .take()
                .expect("the exact in-flight roster was checked");
            if applied {
                self.committed_viewport_roster = Some(key);
            }
        }
        if let Some(generation) = expected.terminal_generation {
            let settled = self.terminal_roster.settle(generation, applied);
            debug_assert!(
                settled,
                "the checked terminal roster generation remains current"
            );
        }
        true
    }

    fn viewport_roster_dirty(&self) -> bool {
        self.committed_viewport_roster.as_ref().map_or_else(
            || self.roster_context != 0,
            |committed| committed.context != self.roster_context,
        )
    }

    fn record_viewport_close_cancelled(
        &mut self,
        record: NativeViewportCloseCancellationRecord,
    ) -> bool {
        if !self.mode.accepts_terminal_callbacks() {
            return false;
        }
        if self
            .close_control
            .receive_cancellation_callback(record, || {
                self.journal
                    .reserve(HostRecord::ViewportCloseCancelled(record))
            })
            != NativeCloseCallbackAdmission::First
        {
            return false;
        }
        true
    }

    #[cfg(test)]
    fn retire_deferred_binding(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativeOutputToken> {
        let mut abandoned = Vec::new();
        self.retire_deferred_binding_into(viewport, binding, |token| abandoned.push(token));
        abandoned
    }

    fn retire_deferred_binding_into(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        mut abandoned: impl FnMut(NativeOutputToken),
    ) {
        self.retire_deferred_binding_resources(viewport, binding);
        for (token, reservation) in self.output_resources.iter_mut() {
            if reservation.reservation.binding() == Some(binding)
                && !reservation.reservation.terminal_recorded
                && reservation.reservation.abandon()
            {
                abandoned(*token);
            }
        }
    }

    fn retire_deferred_binding_resources(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        self.viewports.clear_semantic_wake(viewport, binding);
        self.window_resources.retire(viewport, binding);
    }

    fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.journal
            .iter()
            .chain(self.owner_ingress().map(ReservedHostRecord::record))
            .any(|record| match record {
                HostRecord::WindowEvent(event) => event.references_binding(binding),
                HostRecord::DeviceRemoval(_) => false,
                HostRecord::GlobalFocus(focus) => focus.references_binding(binding),
                HostRecord::ViewportFocus(focus) => focus.references_binding(binding),
                HostRecord::ViewportPointerPassthrough(input) => input.references_binding(binding),
                // Binding retention belongs to the exact pending close, not a
                // callback-time route lookup which may now name its successor.
                HostRecord::ViewportCloseCancelled(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_) => false,
                HostRecord::ViewportRoster(roster) => roster
                    .roster()
                    .observations()
                    .iter()
                    .any(|observation| observation.binding() == binding),
                HostRecord::ViewportCreateFailed(failure) => failure.binding() == binding,
                HostRecord::ViewportVisibility(visibility) => visibility.binding() == binding,
                HostRecord::ViewportCreated(created) => created.binding() == binding,
                HostRecord::Output { .. } => false,
            })
            || self
                .output_resources
                .values()
                .any(|resource| resource.reservation.binding() == Some(binding))
            || self.window_resources.references_binding(binding)
            || self.close_control.references_binding(binding)
    }
}

fn deferred_semantic_prelude_ready<T>(mut records: impl Iterator<Item = T>) -> bool {
    records.next().is_none()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IdleCursorPrefixEntry {
    Cursor,
    Output,
    Boundary,
}

fn idle_cursor_prefix_latest_index(
    entries: impl Iterator<Item = IdleCursorPrefixEntry>,
) -> Option<usize> {
    let mut latest_cursor_index = None;
    for (index, entry) in entries.enumerate() {
        match entry {
            IdleCursorPrefixEntry::Cursor => latest_cursor_index = Some(index),
            IdleCursorPrefixEntry::Output | IdleCursorPrefixEntry::Boundary => break,
        }
    }
    latest_cursor_index.filter(|index| *index != 0)
}

const fn is_next_output_ordinal(previous: u64, current: u64) -> bool {
    matches!(previous.checked_add(1), Some(expected) if expected == current)
}

pub(crate) struct NativeHostBridge {
    records: Mutex<NativeHostState>,
    output_pass_finalizer: OnceLock<Arc<dyn NativeOutputPassFinalizer>>,
    clock: NativeMonotonicClock,
}

const OUTPUT_PREFIX_CAPACITY: usize = 256;

/// A fixed range of the existing capture journal, without owning its records.
/// Reading advances before callers mutate or remove an exact output owner.
pub(crate) struct NativeOutputPrefix {
    outputs: [Option<(u64, NativeOutputToken)>; OUTPUT_PREFIX_CAPACITY],
    next: usize,
    through: u64,
    remaining: usize,
}

impl Default for NativeOutputPrefix {
    fn default() -> Self {
        Self {
            outputs: [None; OUTPUT_PREFIX_CAPACITY],
            next: 0,
            through: 0,
            remaining: 0,
        }
    }
}

pub(crate) struct NativeOutputPrefixEntry {
    pub(crate) result: NativeOutputResult,
    pub(crate) submitted: bool,
    pub(crate) attached: bool,
    pub(crate) binding: Option<NativeSurfaceBinding>,
}

pub(crate) struct NativeDispatchBudget<'bridge> {
    bridge: &'bridge NativeHostBridge,
    token: NativeOutputToken,
    budget: Option<NativeInputBudget>,
}

impl std::ops::Deref for NativeDispatchBudget<'_> {
    type Target = NativeInputBudget;

    fn deref(&self) -> &Self::Target {
        self.budget
            .as_ref()
            .expect("a checked-out budget remains owned")
    }
}

impl std::ops::DerefMut for NativeDispatchBudget<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.budget
            .as_mut()
            .expect("a checked-out budget remains owned")
    }
}

impl Drop for NativeDispatchBudget<'_> {
    fn drop(&mut self) {
        let mut records = self.bridge.lock();
        if let (Some(resource), Some(budget)) = (
            records.output_resources.get_mut(&self.token),
            self.budget.take(),
        ) {
            resource.return_budget(budget);
        }
    }
}

#[must_use = "prepared route retirements must be committed or dropped"]
pub(crate) struct PreparedRouteRetirements {
    bridge: Arc<NativeHostBridge>,
    routes: Vec<CommittedRetirement>,
    abandoned: Vec<NativeOutputToken>,
    active: bool,
}

impl PreparedRouteRetirements {
    #[cfg(test)]
    pub(crate) fn commit(self) -> Vec<NativeOutputToken> {
        self.commit_with_routes().1
    }

    pub(crate) fn commit_with_routes(
        mut self,
    ) -> (Vec<CommittedRetirement>, Vec<NativeOutputToken>) {
        let mut records = self.bridge.lock();
        for retirement in &self.routes {
            records.retire_deferred_binding_into(
                retirement.viewport(),
                retirement.binding(),
                |token| {
                    assert!(self.abandoned.len() < self.abandoned.capacity());
                    self.abandoned.push(token);
                },
            );
        }
        records.viewports.commit_retirements(&self.routes);
        records.refresh_window_resource_references();
        self.active = false;
        (
            std::mem::take(&mut self.routes),
            std::mem::take(&mut self.abandoned),
        )
    }
}

impl Drop for PreparedRouteRetirements {
    fn drop(&mut self) {
        if !self.active {
            return;
        }
        self.bridge.lock().viewports.abort_retirements(&self.routes);
    }
}

impl NativeHostBridge {
    pub(crate) fn observation_time(&self) -> std::time::Duration {
        self.clock.now()
    }

    #[cfg(test)]
    pub(crate) fn enable_observation_attachment_for_test(&self) {
        assert!(self.lock().attachment.try_attach());
    }

    #[cfg(test)]
    pub(crate) fn record_pointer_observation_for_test(
        &self,
        record: NativePointerObservationRecord,
    ) -> bool {
        let mut records = self.lock();
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        window_resources.receive_observation_record(record, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
    }

    pub(crate) fn reserve_pointer_observation(
        &self,
        key: NativePointerObservationKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if !records.mode.accepts_new_work()
            || records.attachment != HostAttachmentState::Attached
            || !matches!(records.admission, NativeAdmission::Running)
            || records.event_boundary_pending
            || records.front_record().is_some()
            || records
                .viewports
                .binding_for_event(key.window, Some(key.viewport))
                != Some(binding)
            || !records.admit_ordinary()
        {
            return false;
        }
        records.window_resources.reserve_observation(key, binding)
    }

    pub(crate) fn pointer_observation_pending(&self, key: NativePointerObservationKey) -> bool {
        self.lock().window_resources.observation_pending(key)
    }

    pub(crate) fn front_pointer_observation(
        &self,
    ) -> Option<(NativePointerObservationRecord, std::time::Duration)> {
        let records = self.lock();
        if !records.front_dispatchable() {
            return None;
        }
        let ingress = records.front_ingress()?;
        let HostRecord::PointerObservation(key) = ingress.record() else {
            return None;
        };
        Some((
            records.window_resources.observation_record(*key)?,
            ingress.captured_at(),
        ))
    }

    pub(crate) fn acknowledge_pointer_observation(&self, key: NativePointerObservationKey) -> bool {
        let mut records = self.lock();
        if !matches!(records.front_dispatch_record(), Some(HostRecord::PointerObservation(current)) if *current == key)
        {
            return false;
        }
        records.transfer_front_record();
        records.event_boundary_pending = true;
        true
    }

    pub(crate) fn reserve_pointer_capture(
        &self,
        key: NativePointerCaptureKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if records.attachment != HostAttachmentState::Attached
            || !records.mode.accepts_terminal_callbacks()
        {
            return false;
        }
        if key.action == eframe::NativeViewportPointerCaptureAction::Acquire
            && (!records.mode.accepts_new_work()
                || !matches!(records.admission, NativeAdmission::Running)
                || records.event_boundary_pending
                || records.front_record().is_some()
                || records
                    .viewports
                    .binding_for_event(key.window, Some(key.viewport))
                    != Some(binding)
                || !records.admit_ordinary())
        {
            return false;
        }
        // Release spends the original prepaid resource receipt even during
        // overload or quarantine. It cannot authorize a new native acquisition.
        records.window_resources.reserve_capture(key, binding)
    }

    pub(crate) fn pointer_capture_pending(&self, key: NativePointerCaptureKey) -> bool {
        self.lock().window_resources.capture_pending(key)
    }

    pub(crate) fn release_capture_binding(&self, binding: NativeSurfaceBinding) {
        self.lock()
            .window_resources
            .release_capture_binding(binding);
    }

    pub(crate) fn front_pointer_capture(
        &self,
    ) -> Option<(NativePointerCaptureRecord, std::time::Duration)> {
        let records = self.lock();
        if !records.front_dispatchable() {
            return None;
        }
        let ingress = records.front_ingress()?;
        let HostRecord::PointerCapture(key) = ingress.record() else {
            return None;
        };
        Some((
            records.window_resources.capture_record(*key)?,
            ingress.captured_at(),
        ))
    }

    pub(crate) fn acknowledge_pointer_capture(&self, key: NativePointerCaptureKey) -> bool {
        let mut records = self.lock();
        if !matches!(records.front_dispatch_record(), Some(HostRecord::PointerCapture(current)) if *current == key)
        {
            return false;
        }
        records.transfer_front_record();
        records.event_boundary_pending = true;
        true
    }

    #[cfg(test)]
    pub(crate) fn record_pointer_capture_for_test(
        &self,
        record: NativePointerCaptureRecord,
    ) -> bool {
        let mut records = self.lock();
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        window_resources.receive_capture_record(record, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
    }

    pub(crate) fn reserve_viewport_route(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        records.prepare_window_resource_reference(viewport, binding)?;
        records.viewports.reserve(viewport, binding)?;
        records
            .window_resources
            .commit_host_reference(viewport, binding);
        Ok(())
    }

    pub(crate) fn bind_viewport_route(
        &self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        if !records.retirements.can_retain_window(binding, window) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        records.prepare_window_resource_reference(viewport, binding)?;
        records.viewports.bind(viewport, window, binding)?;
        records
            .window_resources
            .commit_host_reference(viewport, binding);
        Ok(())
    }

    pub(crate) fn attach_viewport_route(
        &self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        if !records.retirements.can_retain_window(binding, window) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
        records.prepare_window_resource_reference(viewport, binding)?;
        records.viewports.attach(viewport, binding, window)?;
        records
            .window_resources
            .commit_host_reference(viewport, binding);
        Ok(())
    }

    pub(crate) fn unbind_viewport_route(
        &self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
    ) -> Result<NativeSurfaceBinding, NativeViewportBindingError> {
        let mut records = self.lock();
        if records.viewports.has_window_receipt(viewport, expected)
            && records
                .viewports
                .window(viewport)
                .is_some_and(|window| !records.retirements.can_retain_window(expected, window))
        {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        let (binding, receipt) = records.viewports.remove_viewport(viewport, expected)?;
        if let Some(receipt) = receipt {
            records
                .retirements
                .retain_window(receipt)
                .expect("preflighted unbind retains its physical owner");
        }
        records.window_resources.retire(viewport, binding);
        records.refresh_window_resource_references();
        Ok(binding)
    }

    /// Moves a displaced physical window into its existing retirement owner.
    pub(crate) fn replace_viewport_route(
        &self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor: NativeSurfaceBinding,
        replacement: NativeViewportReplacement,
    ) -> Result<(), NativeViewportBindingError> {
        let mut records = self.lock();
        if !records.can_admit_binding(successor) {
            return Err(NativeViewportBindingError::CapacityExceeded {
                limit: records.terminal_roster.window_capacity(),
            });
        }
        let same_window = match replacement {
            NativeViewportReplacement::Attach(window)
                if records.viewports.window(viewport) == Some(window) =>
            {
                Some(window)
            }
            _ => None,
        };
        if records.retirements.has_quiescence_owner(successor)
            || same_window.is_some_and(|window| records.retirements.owns_window(window, None))
            || matches!(replacement, NativeViewportReplacement::Attach(window) if same_window.is_none() && records.retirements.owns_window(window, None))
        {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: successor.surface(),
            });
        }
        if same_window.is_none()
            && records.viewports.has_window_receipt(viewport, expected)
            && records
                .viewports
                .window(viewport)
                .is_some_and(|window| !records.retirements.can_retain_window(expected, window))
        {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        records.prepare_window_resource_reference(viewport, successor)?;
        let displaced = match replacement {
            NativeViewportReplacement::Reserve => records
                .viewports
                .reserve_replacement(viewport, expected, successor)?,
            NativeViewportReplacement::ReserveRetired => records
                .viewports
                .reserve_retired_replacement(viewport, expected, successor)?,
            NativeViewportReplacement::Attach(window) => records
                .viewports
                .replace(viewport, expected, window, successor)?,
        };
        if let Some(receipt) = displaced {
            records
                .retirements
                .retain_window(receipt)
                .expect("preflighted replacement retains its physical owner");
        }
        if expected != successor {
            records.window_resources.retire(viewport, expected);
        }
        records
            .window_resources
            .commit_host_reference(viewport, successor);
        records.refresh_window_resource_references();
        Ok(())
    }

    pub(crate) fn new(viewports: NativeViewportMap) -> Self {
        Self::with_clock(viewports, NativeMonotonicClock::system())
    }

    pub(crate) fn try_new(viewports: NativeViewportMap) -> Result<Self, NativeHostProtocolError> {
        Self::try_new_with_clock(viewports, NativeMonotonicClock::system())
    }

    pub(crate) fn try_new_with_clock(
        viewports: NativeViewportMap,
        clock: NativeMonotonicClock,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::try_new_with_capacity_and_clock(
            viewports,
            terminal_roster::DEFAULT_WINDOW_CAPACITY,
            terminal_roster::DEFAULT_WORK_AREA_CAPACITY,
            clock,
        )
    }

    pub(crate) fn try_new_with_capacity(
        viewports: NativeViewportMap,
        windows: usize,
        work_areas: usize,
    ) -> Result<Self, NativeHostProtocolError> {
        Self::try_new_with_capacity_and_clock(
            viewports,
            windows,
            work_areas,
            NativeMonotonicClock::system(),
        )
    }

    pub(crate) fn try_new_with_capacity_and_clock(
        viewports: NativeViewportMap,
        windows: usize,
        work_areas: usize,
        clock: NativeMonotonicClock,
    ) -> Result<Self, NativeHostProtocolError> {
        Ok(Self {
            records: Mutex::new(NativeHostState::try_with_clock_and_capacity(
                viewports,
                clock.clone(),
                windows,
                work_areas,
            )?),
            output_pass_finalizer: OnceLock::new(),
            clock,
        })
    }

    fn with_clock(viewports: NativeViewportMap, clock: NativeMonotonicClock) -> Self {
        Self {
            records: Mutex::new(NativeHostState::with_clock(viewports, clock.clone())),
            output_pass_finalizer: OnceLock::new(),
            clock,
        }
    }

    pub(crate) fn dispatch_budget(
        &self,
        token: NativeOutputToken,
    ) -> Option<NativeDispatchBudget<'_>> {
        let budget = self
            .lock()
            .output_resources
            .get_mut(&token)?
            .take_budget()?;
        Some(NativeDispatchBudget {
            bridge: self,
            token,
            budget: Some(budget),
        })
    }

    pub(crate) fn service_budget(&self) -> NativeInputBudget {
        NativeInputBudget::new(&self.clock)
    }

    /// The borrow cannot escape this closure. Callers release the mailbox lock
    /// before invoking core, eframe, or another bridge operation.
    pub(crate) fn with_close_control<R>(
        &self,
        action: impl FnOnce(&mut NativeCloseControl) -> R,
    ) -> R {
        action(&mut self.lock().close_control)
    }

    pub(crate) fn can_retain_create_ack(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .window_resources
            .can_retain_create_ack(viewport, binding)
    }

    pub(crate) fn retain_create_ack(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCreateEffectAcknowledgement,
    ) -> Result<(), NativeCreateEffectAcknowledgement> {
        self.lock()
            .window_resources
            .retain_create_ack(viewport, binding, acknowledgement)
    }

    pub(crate) fn clear_create_ack(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().window_resources.clear_create_ack(binding)
    }

    pub(crate) fn create_ack_count(&self) -> usize {
        self.lock().window_resources.create_ack_count()
    }

    pub(crate) fn pending_no_frame_count(&self) -> usize {
        self.lock().window_resources.pending_no_frame().count()
    }

    #[cfg(test)]
    pub(crate) fn retain_no_frame(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        invalidation: PendingNoFrameInvalidation,
    ) -> Result<(), PendingNoFrameInvalidation> {
        self.lock()
            .window_resources
            .retain_no_frame(viewport, binding, invalidation)
    }

    #[cfg(test)]
    pub(crate) fn no_frame_for_binding(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<PendingNoFrameInvalidation> {
        self.lock()
            .window_resources
            .pending_no_frame()
            .find(|record| record.binding == binding)
            .copied()
    }

    pub(crate) fn has_pending_no_frame_invalidation(
        &self,
        surface: dockspace::model::SurfaceId,
    ) -> bool {
        self.lock()
            .window_resources
            .pending_no_frame()
            .any(|record| record.binding.surface() == surface)
    }

    pub(crate) fn finish_no_frame_invalidations(&self, committed: &[PendingNoFrameInvalidation]) {
        let mut records = self.lock();
        for &invalidation in committed {
            records.window_resources.clear_no_frame_if(invalidation);
        }
        records.refresh_window_resource_references();
    }

    pub(crate) fn prepare_no_frame_invalidations(
        &self,
        session: &DockspaceSession,
    ) -> Result<PreparedNoFrameInvalidations, NativeRuntimeError> {
        let mut records = self.lock();
        let slots = records
            .window_resources
            .resource_slot_limit()
            .unwrap_or_else(|| records.window_resources.pending_no_frame().count());
        let mut ready = Vec::new();
        let mut repaint_viewports = Vec::new();
        ready
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        repaint_viewports
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if ready.capacity() > slots || repaint_viewports.capacity() > slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        let NativeHostState {
            window_resources,
            output_resources,
            viewports,
            ..
        } = &mut *records;
        window_resources.retain_no_frame_records(|invalidation| {
            let (terminal_pending, binding) = match invalidation.terminal {
                NoFrameTerminal::Awaiting(token) => {
                    let pending = output_resources.contains_key(&token);
                    if !pending {
                        invalidation.terminal = NoFrameTerminal::Observed;
                    }
                    (
                        pending,
                        viewports.binding_for_output(
                            token.viewport_id(),
                            token.window_id(),
                            token.create_attempt(),
                        ),
                    )
                }
                NoFrameTerminal::Observed => (
                    false,
                    viewports.binding_for_output(
                        invalidation.viewport,
                        invalidation.window,
                        invalidation.create_attempt,
                    ),
                ),
            };
            match no_frame_invalidation_readiness(
                terminal_pending,
                binding == Some(invalidation.binding),
                session.is_current_native_binding(invalidation.binding),
            ) {
                NoFrameInvalidationReadiness::WaitingForTerminal => true,
                NoFrameInvalidationReadiness::Stale => false,
                NoFrameInvalidationReadiness::Ready => {
                    assert!(ready.len() < ready.capacity());
                    ready.push(*invalidation);
                    if !repaint_viewports.contains(&invalidation.viewport) {
                        assert!(repaint_viewports.len() < repaint_viewports.capacity());
                        repaint_viewports.push(invalidation.viewport);
                    }
                    true
                }
            }
        });
        Ok(PreparedNoFrameInvalidations {
            ready,
            repaint_viewports,
        })
    }

    pub(crate) fn settle_close_requests(&self, applied: bool) -> usize {
        self.lock().close_control.settle_clear(applied)
    }

    pub(crate) fn admission_dispatch(&self) -> NativeDispatch {
        let mut records = self.lock();
        let count = records.retained_record_count();
        let age = records.oldest_record_age();
        if matches!(records.admission, NativeAdmission::Running)
            && (!records.journal.can_push_raw()
                || !records.can_reserve_output_data()
                || count >= admission::RECORD_LIMIT
                || age.is_some_and(|age| age >= admission::AGE_LIMIT))
        {
            records.freeze_ordinary();
        }
        if records.can_reserve_output_data() {
            records.admission.try_resume(count, age);
        }
        records.dispatch()
    }

    pub(crate) fn permits_unscheduled_input(&self) -> bool {
        !matches!(self.lock().admission, NativeAdmission::Draining { .. })
    }

    pub(crate) fn accepts_new_work(&self) -> bool {
        self.lock().mode.accepts_new_work()
    }

    /// The raw owner remains queued while already accepted core input drains.
    pub(crate) fn defer_for_core_input_capacity(&self) {
        self.lock().event_boundary_pending = true;
    }

    pub(crate) fn finish_overload_cancellation(&self, boundary: u64) -> bool {
        self.lock().admission.finish_cancellation(boundary)
    }

    #[cfg(test)]
    pub(crate) fn freeze_ordinary_for_test(&self) -> u64 {
        self.lock().freeze_ordinary()
    }

    fn lock(&self) -> MutexGuard<'_, NativeHostState> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Runs a route query under the single host-state lock.
    /// The closure must not invoke core, eframe, or another bridge operation.
    pub(crate) fn with_viewports<R>(&self, action: impl FnOnce(&NativeViewportMap) -> R) -> R {
        action(&self.lock().viewports)
    }

    /// Mutates routes under the single host-state lock.
    /// The closure must not invoke core, eframe, or another bridge operation.
    pub(crate) fn with_viewports_mut<R>(
        &self,
        action: impl FnOnce(&mut NativeViewportMap) -> R,
    ) -> R {
        let mut records = self.lock();
        let result = action(&mut records.viewports);
        records.refresh_window_resource_references();
        result
    }

    pub(crate) fn set_focus_reportable(&self, bindings: &[NativeSurfaceBinding]) {
        let mut records = self.lock();
        let newly_reportable = records.viewports.set_focus_reportable(bindings);
        records.global_focus_refresh_pending |= newly_reportable;
    }

    /// Retry only after ordinary ingress recovers; retain no old focus payload.
    pub(crate) fn request_pending_global_focus(&self, context: &eframe::egui::Context) {
        let request = {
            let records = self.lock();
            records.global_focus_refresh_pending
                && records.mode.accepts_new_work()
                && matches!(records.admission, NativeAdmission::Running)
        };
        if request {
            eframe::request_native_global_focus(context);
        }
    }

    /// Transfers an exact never-admitted route into its prepaid retirement row.
    /// Receipt admission and route removal share this lock, so a concurrent
    /// physical create cannot disappear between the ownership check and transfer.
    pub(crate) fn retire_unstarted_viewport(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let mut records = self.lock();
        if records.retirements.has_quiescence_owner(binding)
            || records
                .viewports
                .has_create_lifetime_owner(viewport, binding)
        {
            return false;
        }
        let Ok((_, receipt)) = records.viewports.remove_viewport(viewport, binding) else {
            return false;
        };
        debug_assert!(
            receipt.is_none(),
            "an unstarted route owns no physical receipt"
        );
        records.retirements.commit_detachment(binding);
        records.refresh_window_resource_references();
        true
    }

    /// Core commit does not call eframe or the bridge. Holding the existing host
    /// lock through preparation and publication prevents concurrent callbacks
    /// from spending a prepared resource slot before its infallible install.
    pub(crate) fn commit_host_frame_with_staging(
        &self,
        frame: DockspaceHostFrame<'_>,
        include_staging: bool,
        root_surface: Option<&mut SurfaceId>,
    ) -> Result<(HostFrameReport, Vec<ViewportId>), NativeRuntimeError> {
        let mut records = self.lock();
        let root_continuation = frame
            .proposed_native_root_continuations()
            .find(|continuation| {
                records.viewports.binding(ViewportId::ROOT) == Some(continuation.predecessor())
            });
        if let (Some(root_surface), Some(continuation)) =
            (root_surface.as_deref(), root_continuation)
            && *root_surface != continuation.predecessor().surface()
        {
            return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                continuation.predecessor().surface(),
            )
            .into());
        }
        if let Err(error) =
            records.prepare_workspace_replacement(frame.proposed_native_root_continuations())
        {
            records.window_resources.abort_workspace_replacement();
            return Err(error);
        }
        let committed = (|| {
            let wake = records
                .window_resources
                .prepare_repaints(include_staging && records.mode.accepts_new_work())?;
            let report = frame.commit()?;
            Ok::<_, NativeRuntimeError>((report, wake))
        })();
        match committed {
            Ok((report, wake)) => {
                records.commit_workspace_replacement(&report);
                if let Some(root_surface) = root_surface
                    && let Some(continuation) = report
                        .native_root_continuations()
                        .iter()
                        .find(|&&continuation| Some(continuation) == root_continuation)
                {
                    *root_surface = continuation.successor().surface();
                }
                Ok((report, wake))
            }
            Err(error) => {
                records.window_resources.abort_workspace_replacement();
                Err(error)
            }
        }
    }

    pub(crate) fn workspace_replacement_predecessor(
        &self,
        successor: NativeSurfaceBinding,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        self.lock()
            .window_resources
            .workspace_predecessor(successor)
    }

    pub(crate) fn finish_workspace_replacement(
        &self,
        viewport: ViewportId,
        predecessor: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records
            .window_resources
            .finish_workspace_predecessor(viewport, predecessor);
        records.refresh_window_resource_references();
    }

    pub(crate) fn has_pending_coordinator_work(&self) -> bool {
        let records = self.lock();
        records.viewports.has_pending_create_receipts() || records.has_pending_coordinator_work()
    }

    #[cfg(test)]
    pub(crate) fn captured_through_for_test(&self) -> u64 {
        self.lock().journal.captured_through()
    }

    #[cfg(test)]
    pub(crate) fn input_backlog_for_test(&self) -> (usize, Option<std::time::Duration>) {
        let records = self.lock();
        (records.retained_record_count(), records.oldest_record_age())
    }

    #[cfg(test)]
    pub(crate) fn front_event(&self) -> Option<NativeWindowEventRecord> {
        self.front_captured_event().map(|(event, _)| event)
    }

    pub(crate) fn front_captured_event(
        &self,
    ) -> Option<(NativeWindowEventRecord, std::time::Duration)> {
        let records = self.lock();
        if !records.front_dispatchable() {
            return None;
        }
        let record = records.front_ingress()?;
        let HostRecord::WindowEvent(event) = record.record() else {
            return None;
        };
        Some((event.clone(), record.captured_at()))
    }

    pub(crate) fn front_device_removal(&self) -> Option<NativeDeviceRemovalRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::DeviceRemoval(removal)) => Some(*removal),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_global_focus(&self) -> Option<NativeGlobalFocusRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::GlobalFocus(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_focus(&self) -> Option<NativeViewportFocusRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::ViewportFocus(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_pointer_passthrough(
        &self,
    ) -> Option<NativePointerPassthroughRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::ViewportPointerPassthrough(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_close_cancelled(
        &self,
    ) -> Option<NativeViewportCloseCancellationRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::ViewportCloseCancelled(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_roster(&self) -> Option<NativeViewportRosterEnvelope> {
        self.lock().front_viewport_roster()
    }

    pub(crate) fn front_viewport_created(&self) -> Option<NativeViewportCreatedRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::ViewportCreated(created)) => Some(*created),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn front_viewport_create_failure(
        &self,
    ) -> Option<NativeViewportCreateFailureRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::ViewportCreateFailed(failure)) => Some(*failure),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::ViewportVisibility(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn has_pending_input(&self) -> bool {
        matches!(
            self.lock().front_dispatch_record(),
            Some(
                HostRecord::WindowEvent(_)
                    | HostRecord::DeviceRemoval(_)
                    | HostRecord::GlobalFocus(_)
                    | HostRecord::ViewportFocus(_)
                    | HostRecord::ViewportPointerPassthrough(_)
                    | HostRecord::PointerObservation(_)
                    | HostRecord::PointerCapture(_)
                    | HostRecord::ViewportCloseCancelled(_)
                    | HostRecord::ViewportRoster(_)
                    | HostRecord::ViewportCreateFailed(_)
                    | HostRecord::ViewportVisibility(_)
                    | HostRecord::ViewportCreated(_)
            )
        )
    }

    pub(crate) fn has_pending_callback_records(&self) -> bool {
        self.lock().front_record().is_some()
    }

    pub(crate) fn acknowledge_event(&self, ordinal: u64) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::WindowEvent(event)) if event.ordinal() == ordinal
        );
        if matches {
            if let Some(HostRecord::WindowEvent(event)) = records.front_dispatch_record()
                && matches!(event.event(), winit::event::WindowEvent::Destroyed)
            {
                let Some(binding) = event.binding() else {
                    return false;
                };
                let window = event.window_id();
                if !records
                    .retirements
                    .transfer_destroyed(binding, window, ordinal)
                {
                    return false;
                }
            }
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    /// Trades the accepted source's growth promise for the installed state.
    /// The old translator and retry candidate fit before this atomic transfer.
    pub(crate) fn acknowledge_translated_event(
        &self,
        ordinal: u64,
        translator: &mut NativePointerTranslator,
        candidate: NativePointerTranslator,
    ) -> bool {
        let mut records = self.lock();
        if !matches!(
            records.front_dispatch_record(),
            Some(HostRecord::WindowEvent(event))
                if event.ordinal() == ordinal
                    && NativeWindowEventClass::classify(event.event()) == NativeWindowEventClass::Pointer
        ) {
            return false;
        }
        *translator = candidate;
        records.pointer_translation_reserved_bytes =
            translator.reserved_translation_capacity_bytes();
        records.transfer_front_record();
        records.event_boundary_pending = true;
        true
    }

    /// Terminal transformations only shrink or replace existing entries.
    pub(crate) fn release_pointer_translation_capacity(
        &self,
        translator: &NativePointerTranslator,
    ) {
        let mut records = self.lock();
        records.pointer_translation_reserved_bytes =
            translator.reserved_translation_capacity_bytes();
    }

    pub(crate) fn acknowledge_device_removal(&self, expected: NativeDeviceRemovalRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::DeviceRemoval(removal)) if *removal == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_global_focus(&self, expected: NativeGlobalFocusRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::GlobalFocus(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_focus(&self, expected: NativeViewportFocusRecord) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportFocus(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_pointer_passthrough(
        &self,
        expected: NativePointerPassthroughRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportPointerPassthrough(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_close_cancelled(
        &self,
        expected: NativeViewportCloseCancellationRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportCloseCancelled(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_create_failure(
        &self,
        expected: NativeViewportCreateFailureRecord,
        result_recorded: bool,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportCreateFailed(failure)) if *failure == expected
        );
        if matches {
            if records.retirements.has_quiescence_owner(expected.binding())
                || !records.viewports.transfer_create_terminal(
                    expected.viewport(),
                    expected.binding(),
                    expected.attempt(),
                    CreateTerminal::Failed(expected.kind()),
                )
            {
                return false;
            }
            let Some(mut receipt) = records
                .viewports
                .take_failed_create(expected.viewport(), expected.binding())
            else {
                return false;
            };
            if result_recorded {
                receipt.mark_failure_recorded();
            }
            records
                .retirements
                .begin_unmaterialized_quiescence(expected.binding(), receipt)
                .expect("preflighted failed create transfers its sole receipt");
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn front_viewport_visibility(&self) -> Option<NativeViewportVisibilityRecord> {
        match self.lock().front_dispatch_record() {
            Some(HostRecord::ViewportVisibility(record)) => Some(*record),
            Some(
                HostRecord::WindowEvent(_)
                | HostRecord::DeviceRemoval(_)
                | HostRecord::GlobalFocus(_)
                | HostRecord::ViewportFocus(_)
                | HostRecord::ViewportPointerPassthrough(_)
                | HostRecord::PointerObservation(_)
                | HostRecord::PointerCapture(_)
                | HostRecord::ViewportCloseCancelled(_)
                | HostRecord::ViewportRoster(_)
                | HostRecord::ViewportCreateFailed(_)
                | HostRecord::ViewportCreated(_)
                | HostRecord::Output { .. },
            )
            | None => None,
        }
    }

    pub(crate) fn acknowledge_viewport_visibility(
        &self,
        expected: NativeViewportVisibilityRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportVisibility(record)) if *record == expected
        );
        if matches {
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn acknowledge_viewport_roster(
        &self,
        expected: &NativeViewportRosterEnvelope,
    ) -> bool {
        self.lock().acknowledge_viewport_roster(expected)
    }

    pub(crate) fn settle_viewport_roster(
        &self,
        expected: &NativeViewportRosterEnvelope,
        applied: bool,
    ) -> bool {
        self.lock().settle_viewport_roster(expected, applied)
    }

    pub(crate) fn acknowledge_viewport_created(
        &self,
        expected: NativeViewportCreatedRecord,
    ) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.front_dispatch_record(),
            Some(HostRecord::ViewportCreated(created)) if *created == expected
        );
        if matches {
            if !records.viewports.transfer_create_terminal(
                expected.token.viewport_id(),
                expected.binding,
                expected.token.create_attempt(),
                CreateTerminal::Created(expected.token.window_id()),
            ) {
                return false;
            }
            records.transfer_front_record();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn output_prefix(&self, limit: usize) -> NativeOutputPrefix {
        let records = self.lock();
        if !matches!(records.dispatch(), NativeDispatch::Replay { .. }) {
            return NativeOutputPrefix::default();
        }
        records.output_prefix_range(limit)
    }

    pub(crate) fn next_output_prefix(
        &self,
        prefix: &mut NativeOutputPrefix,
    ) -> Result<Option<NativeOutputPrefixEntry>, NativeHostProtocolError> {
        if prefix.remaining == 0 {
            return Ok(None);
        }
        let records = self.lock();
        // Submitted heads change dispatch to ContinueBoundary. Membership is
        // already frozen by capture ordinal; never reapply Replay or its skip.
        let (ordinal, token) =
            prefix.outputs[prefix.next].ok_or(NativeHostProtocolError::OutputOrderViolation)?;
        let resource = records
            .output_resources
            .get(&token)
            .ok_or(NativeHostProtocolError::OutputOrderViolation)?;
        let ingress = resource
            .terminal_ingress()
            .filter(|ingress| ingress.ordinal() == ordinal)
            .ok_or(NativeHostProtocolError::OutputOrderViolation)?;
        let HostRecord::Output {
            token: ingress_token,
        } = ingress.record()
        else {
            return Err(NativeHostProtocolError::OutputOrderViolation);
        };
        if *ingress_token != token {
            return Err(NativeHostProtocolError::OutputOrderViolation);
        }
        let entry = NativeOutputPrefixEntry {
            result: resource
                .terminal
                .ok_or(NativeHostProtocolError::OutputOrderViolation)?,
            submitted: resource.submitted,
            attached: resource.paint.is_some(),
            binding: resource.reservation.binding(),
        };
        prefix.next += 1;
        prefix.remaining -= 1;
        Ok(Some(entry))
    }

    pub(crate) fn has_pending_output_prefix(&self) -> bool {
        let records = self.lock();
        matches!(records.dispatch(), NativeDispatch::Replay { .. })
            && matches!(
                records.front_dispatch_record(),
                Some(HostRecord::Output { .. })
            )
    }

    pub(crate) fn has_unsubmitted_output_prefix(&self) -> bool {
        self.lock().output_prefix_range(1).remaining != 0
    }

    pub(crate) fn can_reduce_output_prefix(&self) -> bool {
        self.lock().can_reduce_output_prefix()
    }

    #[cfg(test)]
    pub(crate) fn output_order_is_valid(&self) -> bool {
        !self.lock().output_order_invalid
    }

    /// Reserves the exact binding which may admit one deferred create attempt.
    pub(crate) fn reserve_create(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        let mut records = self.lock();
        if !records.can_admit_binding(binding) || !records.admit_ordinary() {
            return false;
        }
        records
            .window_resources
            .reserve_create(viewport, binding, rect)
    }

    pub(crate) fn clear_create(&self, viewport: ViewportId, binding: NativeSurfaceBinding) -> bool {
        self.lock().window_resources.clear_create(viewport, binding)
    }

    pub(crate) fn clear_hidden_render(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().window_resources.clear_hidden(viewport, binding)
    }

    #[cfg(test)]
    pub(crate) fn retire_deferred_binding(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Vec<NativeOutputToken> {
        self.lock().retire_deferred_binding(viewport, binding)
    }

    /// Receiver retirement already removes every exact binding artifact. Drop
    /// affine paints in their original output slots without a token copy.
    pub(crate) fn retire_deferred_binding_paints(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records.retire_deferred_binding_resources(viewport, binding);
        for (_, resource) in records.output_resources.iter_mut() {
            if resource.reservation.binding() == Some(binding)
                && !resource.reservation.terminal_recorded
                && resource.reservation.abandon()
            {
                resource.paint.take();
            }
        }
    }

    pub(crate) fn prepare_committed_route_retirements(
        self: &Arc<Self>,
    ) -> Result<Option<PreparedRouteRetirements>, NativeRuntimeError> {
        let mut records = self.lock();
        let count = records.retirements.committed_routes().count();
        if count == 0 {
            return Ok(None);
        }
        let slots = records.retirements.capacity_limit().unwrap_or(count);
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if count > slots || routes.capacity() > slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        routes.extend(records.retirements.committed_routes());
        self.prepare_route_retirements_locked(&mut records, routes)
            .map(Some)
    }

    fn prepare_route_retirements_locked(
        self: &Arc<Self>,
        records: &mut NativeHostState,
        routes: Vec<CommittedRetirement>,
    ) -> Result<PreparedRouteRetirements, NativeRuntimeError> {
        for retirement in &routes {
            if !records
                .retirements
                .can_retire_committed_route_for_replacement(
                    retirement.viewport(),
                    retirement.binding(),
                )
            {
                return Err(NativeHostProtocolError::RetiredViewportRouteChanged(
                    retirement.binding().surface(),
                )
                .into());
            }
        }
        let token_slots = records.output_resources.maximum_token_count();
        let mut abandoned = Vec::new();
        abandoned
            .try_reserve_exact(token_slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if abandoned.capacity() > token_slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        records
            .viewports
            .prepare_retirements(&routes)
            .map_err(|binding| {
                NativeHostProtocolError::RetiredViewportRouteChanged(binding.surface())
            })?;
        Ok(PreparedRouteRetirements {
            bridge: self.clone(),
            routes,
            abandoned,
            active: true,
        })
    }

    #[cfg(test)]
    pub(crate) fn prepare_route_retirements(
        self: &Arc<Self>,
        retirements: &[CommittedRetirement],
    ) -> Result<PreparedRouteRetirements, NativeRuntimeError> {
        let mut routes = Vec::new();
        routes
            .try_reserve_exact(retirements.len())
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        routes.extend_from_slice(retirements);
        self.prepare_route_retirements_locked(&mut self.lock(), routes)
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().references_binding(binding)
    }

    fn hidden_render_enabled(&self, viewport: ViewportId) -> bool {
        let records = self.lock();
        records.mode.accepts_new_work()
            && records.window_resources.hidden_binding(viewport).is_some()
    }

    pub(crate) fn create_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.lock()
            .window_resources
            .create(viewport)
            .map(|reservation| reservation.binding)
    }

    pub(crate) fn reserve_pointer_callback(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        token: eframe::NativeViewportPointerPassthroughCommandToken,
        enabled: bool,
    ) -> bool {
        let mut records = self.lock();
        if records.viewports.binding_for_event(window, Some(viewport)) != Some(binding) {
            return false;
        }
        (matches!(records.mode, HostIngressMode::Quarantined) || records.admit_ordinary())
            && records
                .window_resources
                .reserve_pointer_callback(viewport, binding, window, token, enabled)
    }

    pub(crate) fn reserve_focus_callback(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        token: eframe::NativeViewportFocusCommandToken,
    ) -> bool {
        let mut records = self.lock();
        if records.viewports.binding_for_event(window, Some(viewport)) != Some(binding) {
            return false;
        }
        (matches!(records.mode, HostIngressMode::Quarantined) || records.admit_ordinary())
            && records
                .window_resources
                .reserve_focus_callback(viewport, binding, window, token)
    }

    #[cfg(test)]
    pub(crate) fn reserve_visibility_callback(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        token: eframe::NativeViewportVisibilityCommandToken,
        visible: bool,
    ) -> bool {
        let mut records = self.lock();
        if records.viewports.binding_for_event(window, Some(viewport)) != Some(binding) {
            return false;
        }
        // An emitted lifecycle effect owns this prepaid terminal slot. Ordinary
        // output pressure must not turn its pending Show into a dispatch failure.
        records.mode.accepts_terminal_callbacks()
            && records
                .window_resources
                .reserve_visibility_callback(viewport, binding, window, token, visible)
    }

    pub(crate) fn reserve_close_callback(
        &self,
        binding: NativeSurfaceBinding,
        request: NativeViewportCloseRequest,
    ) -> bool {
        let mut records = self.lock();
        if records
            .viewports
            .binding_for_event(request.window_id(), Some(request.viewport_id()))
            != Some(binding)
        {
            return false;
        }
        records.mode.accepts_terminal_callbacks()
            && records
                .close_control
                .reserve_cancellation_callback(binding, request)
    }

    pub(crate) fn cancel_close_callback_reservation(
        &self,
        binding: NativeSurfaceBinding,
        request: NativeViewportCloseRequest,
    ) {
        self.lock()
            .close_control
            .cancel_cancellation_callback_reservation(binding, request);
    }

    pub(crate) fn record_deferred_viewport_paint(
        &self,
        token: NativeOutputToken,
    ) -> DeferredViewportPaint {
        let mut records = self.lock();
        let Some(reservation) = records
            .output_resources
            .get(&token)
            .map(|resource| resource.reservation)
        else {
            return DeferredViewportPaint::Waiting;
        };
        if reservation.is_abandoned() {
            return DeferredViewportPaint::Waiting;
        }
        if let Some(PendingNativeOutput::Staging(request)) = records
            .output_resources
            .get(&token)
            .and_then(|resource| resource.paint.as_ref())
        {
            // Multipass callbacks reuse the original output owner. They repaint
            // the placeholder without consuming or abandoning its preparation.
            return DeferredViewportPaint::Staging(*request);
        }
        if let Some(create) = records.window_resources.create(token.viewport_id())
            && reservation.binding() == Some(create.binding)
        {
            let created = NativeViewportCreatedRecord::new(token, create.binding);
            let recorded = records.record_viewport_created(created);
            records
                .output_resources
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .reservation
                .abandon();
            return if recorded {
                DeferredViewportPaint::Created
            } else {
                DeferredViewportPaint::Waiting
            };
        }
        let Some(request) = records.window_resources.staging(token.viewport_id()) else {
            if !records
                .window_resources
                .preparation_allows_semantic(token.viewport_id())
            {
                records
                    .output_resources
                    .get_mut(&token)
                    .expect("the current output remains reserved")
                    .reservation
                    .abandon_silently();
                return DeferredViewportPaint::Waiting;
            }
            if let Some(binding) = reservation.binding() {
                if records.output_resources.is_control(token) {
                    records.block_semantic_viewport(token.viewport_id());
                    return DeferredViewportPaint::Retain(binding);
                }
                if records.semantic_prelude_ready() {
                    records
                        .viewports
                        .clear_semantic_wake(token.viewport_id(), binding);
                    return DeferredViewportPaint::Semantic(binding);
                }
                records.block_semantic_viewport(token.viewport_id());
                return DeferredViewportPaint::Retain(binding);
            }
            records
                .output_resources
                .get_mut(&token)
                .expect("the deferred output reservation remains present")
                .reservation
                .abandon();
            return DeferredViewportPaint::Waiting;
        };
        // A retained predecessor output cannot begin or roll back its successor's paint.
        if reservation.binding() == Some(request.binding())
            && records
                .window_resources
                .begin_preparation_paint(token.viewport_id(), request)
        {
            let resource = records
                .output_resources
                .get_mut(&token)
                .expect("the deferred output reservation remains present");
            if resource
                .bind_paint(PendingNativeOutput::Staging(request))
                .is_ok()
            {
                resource.reservation.require_wake_after_current();
                return DeferredViewportPaint::Staging(request);
            }
            records
                .window_resources
                .finish_preparation_paint(token.viewport_id(), request, false);
        }
        records
            .output_resources
            .get_mut(&token)
            .expect("the deferred output reservation remains present")
            .reservation
            .abandon();
        DeferredViewportPaint::Waiting
    }

    pub(crate) fn begin_window_preparation(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
    ) {
        self.lock()
            .window_resources
            .begin_preparation(viewport, binding, window);
    }

    #[cfg(test)]
    pub(crate) fn ready_window_preparation_for_test(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
    ) {
        self.lock()
            .window_resources
            .ready_preparation_for_test(viewport, binding, window);
    }

    #[cfg(test)]
    pub(crate) fn lose_preparation_renderer_for_test(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records.window_resources.renderer_lost(viewport, binding);
        records.invalidate_viewport_roster();
    }

    #[cfg(test)]
    pub(crate) fn present_window_preparation_for_test(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records
            .window_resources
            .present_preparation_for_test(viewport, binding);
        records.invalidate_viewport_roster();
    }

    pub(crate) fn observe_window_preparations(&self, roster: &NativeViewportRosterRecord) {
        let mut records = self.lock();
        let Some(ordinal) = records.front_ingress().map(ReservedHostRecord::ordinal) else {
            return;
        };
        records
            .window_resources
            .observe_preparations(roster, ordinal);
    }

    pub(crate) fn revoke_preparation_inventory(&self) {
        let mut records = self.lock();
        records.window_resources.revoke_preparation_inventory();
        records.invalidate_viewport_roster();
    }

    pub(crate) fn settle_window_preparation(
        &self,
        binding: NativeSurfaceBinding,
        observation: NativePreparationObservation,
    ) {
        let mut records = self.lock();
        if records
            .window_resources
            .settle_preparation(binding, observation)
        {
            records.invalidate_viewport_roster();
        }
    }

    pub(crate) fn next_preparation_show(
        &self,
    ) -> Option<(ViewportId, NativeSurfaceBinding, WindowId)> {
        let records = self.lock();
        records
            .mode
            .accepts_new_work()
            .then(|| records.window_resources.next_preparation_show())
            .flatten()
    }

    pub(crate) fn dispatch_preparation_show(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        token: eframe::NativeViewportVisibilityCommandToken,
    ) -> bool {
        let mut records = self.lock();
        if !records.mode.accepts_new_work()
            || records.window_resources.next_preparation_show() != Some((viewport, binding, window))
            || !records
                .window_resources
                .reserve_visibility_callback(viewport, binding, window, token, true)
        {
            return false;
        }
        records
            .window_resources
            .dispatch_preparation_show(viewport, binding, token)
    }

    pub(crate) fn reduce_preparation_visibility(
        &self,
        record: NativeViewportVisibilityRecord,
    ) -> bool {
        let mut records = self.lock();
        let Some(ordinal) = records.front_ingress().map(ReservedHostRecord::ordinal) else {
            return false;
        };
        let changed = records
            .window_resources
            .preparation_visibility(record, ordinal);
        if changed {
            records.invalidate_viewport_roster();
        }
        changed
    }

    pub(crate) fn finish_preparation_output(
        &self,
        token: NativeOutputToken,
        request: NativePreparationPaint,
        presented: bool,
    ) {
        let mut records = self.lock();
        if records.window_resources.finish_preparation_paint(
            token.viewport_id(),
            request,
            presented,
        ) {
            records.invalidate_viewport_roster();
        }
    }

    pub(crate) fn preparation_renderer_lost(
        &self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records
            .window_resources
            .renderer_lost(token.viewport_id(), binding);
        records.invalidate_viewport_roster();
    }

    pub(crate) fn reserve_output(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        root_roster: Option<NativeViewportRoster<'_>>,
    ) -> bool {
        let budget = NativeInputBudget::new(&self.clock);
        // Route lookup and reservation share the host-state lock, so retirement
        // cannot report quiescence before this exact binding reference is visible.
        let mut records = self.lock();
        let binding = records.viewports.binding_for_output(
            token.viewport_id(),
            token.window_id(),
            token.create_attempt(),
        );
        if let Some(binding) = binding {
            if !records.window_resources.validate_preparation_output(
                token.viewport_id(),
                binding,
                window,
            ) {
                records.invalidate_viewport_roster();
            }
        }
        if let Some(binding) = binding
            && records
                .prepare_window_resource_reference(token.viewport_id(), binding)
                .is_err()
        {
            records.freeze_ordinary();
            return false;
        }
        let ordinary_roster = root_roster.is_some() && records.admit_ordinary();
        if !records.reserve_output(token, binding, budget) {
            if token.viewport_id() != ViewportId::ROOT
                && binding.is_some()
                && records.mode.accepts_terminal_callbacks()
            {
                // Deferred output runs no egui pass, so its original redraw
                // cannot be relied on to wake again. Reuse the existing child
                // continuation released by the next complete root boundary.
                records.block_semantic_viewport(token.viewport_id());
            }
            return false;
        }
        if let Some(binding) = binding {
            records
                .window_resources
                .commit_host_reference(token.viewport_id(), binding);
        }
        if matches!(records.mode, HostIngressMode::Quarantined)
            && let Some(binding) = binding
            && token.create_attempt().is_some()
            && records
                .window_resources
                .create(token.viewport_id())
                .is_some_and(|create| create.binding == binding)
        {
            records.record_viewport_created(NativeViewportCreatedRecord::new(token, binding));
        }
        if let Some(roster) = root_roster {
            match records.mode {
                HostIngressMode::Active | HostIngressMode::Quarantined
                    if records.terminal_roster.needs_capture() =>
                {
                    let NativeHostState {
                        terminal_roster,
                        viewports,
                        ..
                    } = &mut *records;
                    let payload = terminal_roster.capture_roster(roster, viewports);
                    if let Some(payload) = payload {
                        records.record_terminal_viewport_roster(payload);
                    } else {
                        // Both preallocated payloads may still be retained by a
                        // baseline or settling snapshot. Keep the obligation and
                        // retry after this output without allocating a third one.
                        records.require_output_wake_after_current(token);
                    }
                }
                HostIngressMode::Active if ordinary_roster => {
                    if !records.viewport_roster_known_borrowed(roster) {
                        let NativeHostState {
                            terminal_roster,
                            viewports,
                            ..
                        } = &mut *records;
                        let payload = terminal_roster.capture_ordinary_roster(roster, viewports);
                        if let Some(payload) = payload {
                            records.record_viewport_roster(payload);
                        } else {
                            records.defer_ordinary_roster_capture();
                            records
                                .output_resources
                                .get_mut(&token)
                                .expect("the current output was reserved before roster capture")
                                .reservation
                                .abandon_silently();
                            records.require_output_wake_after_current(token);
                        }
                    }
                }
                HostIngressMode::Active
                | HostIngressMode::Quarantined
                | HostIngressMode::Frozen => {}
            }
        }
        true
    }

    pub(crate) fn invalidate_viewport_roster(&self) {
        self.lock().invalidate_viewport_roster();
    }

    pub(crate) fn viewport_roster_dirty(&self) -> bool {
        self.lock().viewport_roster_dirty()
    }

    pub(crate) fn output_reservation(&self, token: NativeOutputToken) -> Option<OutputReservation> {
        self.lock()
            .output_resources
            .get(&token)
            .map(|resource| resource.reservation)
    }

    pub(crate) fn is_control_output(&self, token: NativeOutputToken) -> bool {
        self.lock().output_resources.is_control(token)
    }

    pub(crate) fn has_pending_output(&self, token: NativeOutputToken) -> bool {
        self.lock()
            .output_resources
            .get(&token)
            .is_some_and(|resource| resource.paint.is_some())
    }

    pub(crate) fn output_has_staging_request(&self, token: NativeOutputToken) -> bool {
        let records = self.lock();
        records
            .window_resources
            .staging(token.viewport_id())
            .is_some_and(|request| {
                records
                    .output_resources
                    .get(&token)
                    .is_some_and(|resource| {
                        resource.reservation.binding() == Some(request.binding())
                    })
            })
    }

    pub(crate) fn output_backlog(&self) -> NativeOutputBacklog {
        let records = self.lock();
        records
            .output_resources
            .output_backlog(records.window_resources.awaiting_staging_count())
    }

    pub(crate) fn pending_output_count(&self) -> usize {
        self.lock()
            .output_resources
            .values()
            .filter(|resource| resource.paint.is_some())
            .count()
    }

    pub(crate) fn has_pending_lifecycle_output(
        &self,
        session: &dockspace::runtime::DockspaceSession,
    ) -> bool {
        self.lock().output_resources.values().any(|resource| {
            resource
                .paint
                .as_ref()
                .is_some_and(|paint| paint.requires_lifecycle_settlement(session))
        })
    }

    pub(crate) fn take_pending_output(
        &self,
        token: NativeOutputToken,
    ) -> Option<PendingNativeOutput> {
        self.lock().output_resources.get_mut(&token)?.paint.take()
    }

    pub(crate) fn bind_surface_output(
        &self,
        token: NativeOutputToken,
        output: dockspace::runtime::PaintedSurfaceOutput,
    ) -> Result<(), dockspace::runtime::PaintedSurfaceOutput> {
        let mut records = self.lock();
        let Some(resource) = records.output_resources.get_mut(&token) else {
            return Err(output);
        };
        match resource.bind_paint(PendingNativeOutput::Surface(output)) {
            Ok(()) => Ok(()),
            Err(PendingNativeOutput::Surface(output)) => Err(output),
            Err(PendingNativeOutput::Staging(_)) => {
                unreachable!("binding returns its original paint")
            }
        }
    }

    pub(crate) fn attach_output_binding(
        &self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().attach_output_binding(token, binding)
    }

    pub(crate) fn require_output_wake_after_current(&self, token: NativeOutputToken) -> bool {
        self.lock().require_output_wake_after_current(token)
    }

    pub(crate) fn mark_output_submitted(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let prefix = records.output_prefix_range(1);
        if prefix.remaining == 0 {
            return false;
        }
        let Some(resource) = records.output_resources.get_mut(&token) else {
            return false;
        };
        let Some((ordinal, expected_token)) = prefix.outputs[0] else {
            return false;
        };
        if resource.submitted
            || expected_token != token
            || resource.terminal.is_none()
            || !resource
                .terminal_ingress()
                .is_some_and(|ingress| ingress.ordinal() == ordinal)
        {
            return false;
        }
        resource.submitted = true;
        records.submitted_output_prefix += 1;
        records.event_boundary_pending = true;
        true
    }

    pub(crate) fn abandon_output(&self, token: NativeOutputToken) -> bool {
        self.lock().abandon_output(token)
    }

    pub(crate) fn defer_output_input(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let Some(resource) = records.output_resources.get_mut(&token) else {
            return false;
        };
        if !resource.defer_before_input() {
            return false;
        }
        let binding = resource.reservation.binding();
        if token.viewport_id() != ViewportId::ROOT
            && binding.is_some()
            && records.viewports.binding(token.viewport_id()) == binding
        {
            records.block_semantic_viewport(token.viewport_id());
        }
        true
    }

    pub(crate) fn settle_output_abandonment(
        &self,
        token: NativeOutputToken,
        wake_on_terminal: bool,
        no_frame: Option<PendingNoFrameInvalidation>,
    ) -> OutputAbandonment {
        let mut records = self.lock();
        let disposition = records.settle_output_abandonment(token, wake_on_terminal);
        if matches!(
            disposition,
            OutputAbandonment::NewlyAbandoned | OutputAbandonment::AlreadyAbandonedAwaitingTerminal
        ) && let Some(invalidation) = no_frame
        {
            records
                .window_resources
                .retain_no_frame(token.viewport_id(), invalidation.binding, invalidation)
                .expect("an announced output prepays its exact no-frame terminal slot");
        }
        disposition
    }

    pub(crate) fn accepts_no_frame_invalidation(&self) -> bool {
        self.lock().accepts_no_frame_invalidation()
    }

    pub(crate) fn commit_frame_boundary(&self, input_pending: bool) {
        let mut records = self.lock();
        records.event_boundary_pending = input_pending;
        if input_pending {
            return;
        }
        records.journal.commit_transferred();
        records.viewports.commit_create_boundary();
        records.window_resources.commit_transferred_callbacks();
        records.close_control.commit_transferred_callbacks();
        records.retirements.commit_create_failures();
        records.retirements.commit_destroyed_callbacks();
        records.terminal_roster.commit_transferred();
        loop {
            let Some(HostRecord::Output { token }) = records.front_record() else {
                break;
            };
            let token = *token;
            if !records
                .output_resources
                .get(&token)
                .is_some_and(|resource| resource.submitted)
            {
                break;
            }
            records.output_resources.remove(&token);
            records.submitted_output_prefix -= 1;
        }
        records.release_semantic_viewports_if_ready();
        records.refresh_window_resource_references();
    }

    #[cfg(test)]
    pub(crate) fn take_ready_semantic_viewports(&self) -> Vec<ViewportId> {
        self.lock().take_ready_semantic_viewports()
    }

    pub(crate) fn claim_ready_semantic_viewport(&self) -> Option<ViewportId> {
        self.lock().viewports.claim_ready_semantic_viewport()
    }

    pub(crate) fn block_semantic_viewport(&self, viewport: ViewportId) {
        self.lock().block_semantic_viewport(viewport);
    }

    pub(crate) fn freeze(&self) {
        self.lock().freeze();
    }

    pub(crate) fn enter_quarantine(&self) {
        self.lock().enter_quarantine();
    }

    pub(crate) fn claim_quarantine_output(
        &self,
    ) -> Option<(NativeOutputToken, Option<PendingNativeOutput>)> {
        self.lock().claim_quarantine_output()
    }

    #[cfg(test)]
    pub(crate) fn quarantine_after_fatal(&self) -> Vec<NativeOutputToken> {
        self.enter_quarantine();
        std::iter::from_fn(|| self.claim_quarantine_output().map(|(token, _)| token)).collect()
    }

    pub(crate) fn retirement_has_pending_work(&self) -> bool {
        self.lock().retirements.has_pending_work()
    }

    #[cfg(test)]
    pub(crate) fn retirement_committed_routes(&self) -> Vec<CommittedRetirement> {
        self.lock().retirements.committed_routes().collect()
    }

    pub(crate) fn retirement_commit_routes(&self, committed: &[CommittedRetirement]) {
        self.lock().retirements.commit_routes(committed)
    }

    pub(crate) fn retirement_settle_snapshot(&self, applied: bool) {
        self.lock().retirements.settle_snapshot(applied)
    }

    #[cfg(test)]
    pub(crate) fn retirement_quiescence_candidates(&self) -> Vec<NativeSurfaceBinding> {
        self.lock().retirements.quiescence_candidates().collect()
    }

    pub(crate) fn next_retirement_quiescence_candidate(
        &self,
        after: Option<NativeSurfaceBinding>,
    ) -> Option<NativeSurfaceBinding> {
        self.lock().retirements.next_quiescence_candidate(after)
    }

    pub(crate) fn retirement_references_cleanup(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.references_cleanup(binding)
    }

    pub(crate) fn retirement_finish_quiescence(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.finish_quiescence(binding)
    }

    pub(crate) fn retirement_is_quiescence_candidate(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.is_quiescence_candidate(binding)
    }

    pub(crate) fn retirement_blocks_quiescence(&self, binding: NativeSurfaceBinding) -> bool {
        let records = self.lock();
        records
            .retirements
            .bindings()
            .any(|retained| retained == binding)
            && !records.retirements.is_quiescence_candidate(binding)
    }

    /// Records input retirement once under the exact physical callback owner.
    pub(crate) fn retire_destroyed_input(
        &self,
        record: &NativeWindowEventRecord,
        retire: impl FnOnce() -> Result<(), NativeRuntimeError>,
    ) -> Result<(), NativeRuntimeError> {
        let mut records = self.lock();
        let (Some(viewport), Some(binding)) = (record.viewport_id(), record.binding()) else {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        };
        if !matches!(record.event(), winit::event::WindowEvent::Destroyed) {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        }
        let window = record.window_id();
        let ordinal = record.ordinal();
        let receipt = if records.retirements.owns_window(window, Some(viewport)) {
            records
                .retirements
                .received_destroyed_receipt_mut(viewport, binding, window, ordinal)
        } else {
            records
                .viewports
                .received_destroyed_receipt_mut(viewport, binding, window, ordinal)
        }
        .ok_or(NativeHostProtocolError::WindowEventAcknowledgementMismatch)?;
        if receipt.retire_input(retire)? {
            // A later lifecycle failure retains the raw callback, but the new
            // core fact must still be allowed to reach its commit boundary.
            records.event_boundary_pending = true;
        }
        Ok(())
    }

    pub(crate) fn retirement_observe_destroyed(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: winit::window::WindowId,
        ordinal: u64,
    ) -> DestroyedObservation {
        let mut records = self.lock();
        if !records.retirements.owns_window(window, Some(viewport)) {
            if !records.retirements.can_retain_window(binding, window)
                || !records
                    .viewports
                    .has_received_destroyed(viewport, binding, window, ordinal)
            {
                return DestroyedObservation::Rejected;
            }
            let Some(receipt) = records.viewports.take_window_receipt(viewport, binding) else {
                return DestroyedObservation::Rejected;
            };
            records
                .retirements
                .retain_window(receipt)
                .expect("the reducer moves the exact captured window owner into retirement");
        }
        records
            .retirements
            .observe_destroyed(viewport, binding, window, ordinal)
    }

    pub(crate) fn retirement_retire_unrecognized_destroyed(&self, binding: NativeSurfaceBinding) {
        self.lock()
            .retirements
            .retire_unrecognized_destroyed(binding);
    }

    #[cfg(test)]
    pub(crate) fn retirement_destroyed_observations(
        &self,
    ) -> Vec<(NativeSurfaceBinding, NativeWindowFacts)> {
        self.lock().retirements.destroyed_observations().collect()
    }

    pub(crate) fn with_retirement_destroyed_observations<R>(
        &self,
        action: impl FnOnce(
            usize,
            &mut dyn Iterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
            &dyn Fn(NativeSurfaceBinding) -> Option<Option<NativeCreateEffectAcknowledgement>>,
            &dyn Fn(NativeSurfaceBinding) -> Option<NativePreparationObservation>,
        ) -> R,
    ) -> R {
        let state = self.lock();
        let count = state.retirements.destroyed_observations().count();
        action(
            count,
            &mut state.retirements.destroyed_observations(),
            &|binding| state.window_resources.created_ack(binding),
            &|binding| state.window_resources.preparation_observation(binding),
        )
    }

    pub(crate) fn retirement_mark_snapshot_queued(&self) {
        self.lock().retirements.mark_snapshot_queued()
    }

    pub(crate) fn retirement_requires_snapshot(&self) -> bool {
        self.lock().retirements.requires_snapshot()
    }

    pub(crate) fn retirement_can_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let records = self.lock();
        (records.viewports.has_window_receipt(viewport, binding)
            || records.retirements.has_window(viewport, binding))
            && records.retirements.can_begin_release(viewport, binding)
    }

    pub(crate) fn retirement_begin_release(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCloseEffectAcknowledgement,
    ) -> bool {
        let mut records = self.lock();
        if !records.retirements.can_begin_release(viewport, binding) {
            return false;
        }
        if !records.viewports.has_window_receipt(viewport, binding)
            && !records.retirements.has_window(viewport, binding)
        {
            return false;
        }
        let window = records.viewports.take_window_receipt(viewport, binding);
        records
            .retirements
            .begin_release(viewport, binding, acknowledgement, window)
            .expect("admitted lifetime retains prepaid retirement storage");
        true
    }

    pub(crate) fn retirement_can_retire_committed_route_for_replacement(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .retirements
            .can_retire_committed_route_for_replacement(viewport, binding)
    }

    pub(crate) fn retirement_retire_committed_route_for_replacement(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .retirements
            .retire_committed_route_for_replacement(viewport, binding)
    }

    pub(crate) fn retirement_can_accept_cleanup_observation(
        &self,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock()
            .retirements
            .can_accept_cleanup_observation(binding)
    }

    pub(crate) fn retirement_accept_cleanup_observation(
        &self,
        binding: NativeSurfaceBinding,
        observation: NativeCleanupObservation,
    ) -> Result<Option<NativeEffectResult>, ()> {
        self.lock()
            .retirements
            .accept_cleanup_observation(binding, observation)
    }

    pub(crate) fn retirement_cleanup_is_terminal(&self, binding: NativeSurfaceBinding) -> bool {
        self.lock().retirements.cleanup_is_terminal(binding)
    }

    pub(crate) fn retirement_can_accept_cleanup_result(
        &self,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().retirements.can_accept_cleanup_result(binding)
    }

    pub(crate) fn retirement_retain_cleanup_result(
        &self,
        binding: NativeSurfaceBinding,
        result: NativeEffectResult,
    ) -> Result<Option<NativeEffectResult>, CleanupResultRetentionError> {
        self.lock()
            .retirements
            .retain_cleanup_result(binding, result)
    }

    pub(crate) fn retirement_mark_create_failure_recorded(&self, binding: NativeSurfaceBinding) {
        self.lock()
            .retirements
            .mark_create_failure_recorded(binding)
    }

    pub(crate) fn require_terminal_roster(&self) {
        self.lock().require_terminal_roster();
    }

    fn admit_viewport_create_attempt(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        let viewport = attempt.viewport_id();
        let mut records = self.lock();
        let Some(reservation) = records.window_resources.create(viewport) else {
            return NativeViewportCreateAdmission::Defer;
        };
        if !records.admit_ordinary()
            || records.window_resources.hidden_binding(viewport) != Some(reservation.binding)
            || !records
                .viewports
                .admit_create_attempt(viewport, reservation.binding, attempt)
        {
            return NativeViewportCreateAdmission::Defer;
        }
        NativeViewportCreateAdmission::Proceed {
            undecorated_outer_rect: Some(reservation.rect),
        }
    }

    fn record_eframe_viewport_create_failure(
        &self,
        failure: NativeViewportCreateFailure,
    ) -> NativeHostWake {
        let attempt = failure.create_attempt();
        let viewport = attempt.viewport_id();
        let mut records = self.lock();
        let Some(binding) = records.viewports.binding_for_create_attempt(attempt) else {
            return NativeHostWake::Wait;
        };
        if records
            .window_resources
            .create(viewport)
            .is_none_or(|reservation| reservation.binding != binding)
        {
            return NativeHostWake::Wait;
        }
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::from_attempt(
            attempt,
            binding,
            failure.kind(),
        )) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }

    #[cfg(test)]
    fn record_reserved_viewport_create_failure_for_test(
        &self,
        viewport: ViewportId,
        kind: NativeViewportCreateFailureKind,
    ) -> NativeHostWake {
        let mut records = self.lock();
        let Some(reservation) = records.window_resources.create(viewport) else {
            return NativeHostWake::Wait;
        };
        if !records
            .viewports
            .admit_test_create(viewport, reservation.binding)
        {
            return NativeHostWake::Wait;
        }
        if !records.record_viewport_create_failure(NativeViewportCreateFailureRecord::new(
            viewport,
            reservation.binding,
            kind,
        )) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }

    #[cfg(test)]
    pub(crate) fn record_viewport_create_failure_for_test(
        &self,
        viewport: eframe::egui::ViewportId,
    ) -> NativeHostWake {
        self.record_reserved_viewport_create_failure_for_test(
            viewport,
            NativeViewportCreateFailureKind::WindowUnavailable,
        )
    }

    #[cfg(test)]
    pub(crate) fn reserve_create_for_test(
        &self,
        viewport: eframe::egui::ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        self.reserve_create(viewport, binding, rect)
    }

    #[cfg(test)]
    pub(crate) fn push_record(&self, record: HostRecord) {
        let mut records = self.lock();
        if let HostRecord::WindowEvent(record) = record {
            records.capture_window_event(record);
            return;
        }
        match record {
            HostRecord::DeviceRemoval(record) => {
                records.record_device_removal(record);
            }
            HostRecord::ViewportFocus(record) => {
                records.record_viewport_focus(record);
            }
            HostRecord::ViewportVisibility(record) => {
                records.record_viewport_visibility(record);
            }
            HostRecord::ViewportPointerPassthrough(record) => {
                records.record_viewport_pointer_passthrough(record);
            }
            HostRecord::ViewportCloseCancelled(record) => {
                records.record_viewport_close_cancelled(record);
            }
            record => {
                records.journal.push_back(record);
            }
        }
    }

    #[cfg(test)]
    pub(crate) fn push_viewport_roster(&self, roster: NativeViewportRosterRecord) {
        assert!(self.lock().record_viewport_roster(roster));
    }

    #[cfg(test)]
    pub(crate) fn push_terminal_viewport_roster(&self, roster: NativeViewportRosterRecord) {
        assert!(self.lock().record_terminal_viewport_roster(roster));
    }

    pub(crate) fn callback_boundary_pending(&self) -> bool {
        self.lock().event_boundary_pending
    }

    pub(crate) fn hold_native_boundary(&self) {
        self.lock().event_boundary_pending = true;
    }

    pub(crate) fn coalesce_idle_cursor_prefix(&self, budget: &mut NativeInputBudget) -> bool {
        self.lock().coalesce_idle_cursor_prefix(budget)
    }

    pub(crate) fn claim_cursor_prefix(&self, limit: usize) -> Option<CursorPrefixClaim<'_>> {
        let records = self.lock();
        if records.event_boundary_pending {
            return None;
        }
        let HostRecord::WindowEvent(first) = records.front_record()? else {
            return None;
        };
        let len = records.raw_prefix().take(limit).take_while(|record| {
            matches!(record, HostRecord::WindowEvent(event) if first.shares_cursor_lane(event))
        }).count();
        std::num::NonZeroUsize::new(len).map(|len| CursorPrefixClaim { records, len })
    }
}

impl NativeHostHandler for NativeHostBridge {
    fn on_service(&self, context: &eframe::egui::Context) -> NativeHostWake {
        self.output_pass_finalizer
            .get()
            .map_or(NativeHostWake::Wait, |finalizer| {
                finalizer.on_service(context)
            })
    }

    fn before_output_input(
        &self,
        context: &eframe::egui::Context,
        token: NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        self.output_pass_finalizer
            .get()
            .map_or(eframe::NativeOutputInputAdmission::Proceed, |finalizer| {
                finalizer.before_output_input(context, token, has_input)
            })
    }

    fn on_viewport_pointer_observation(
        &self,
        result: eframe::NativeViewportPointerObservationResult,
    ) -> NativeHostWake {
        let mut records = self.lock();
        if !records.mode.accepts_terminal_callbacks() {
            return NativeHostWake::Wait;
        }
        let NativeHostState {
            window_resources,
            viewports,
            journal,
            ..
        } = &mut *records;
        if window_resources.accept_observation(result, viewports, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
        {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_pointer_capture(
        &self,
        result: eframe::NativeViewportPointerCaptureResult,
    ) -> NativeHostWake {
        let mut records = self.lock();
        if !records.mode.accepts_terminal_callbacks() {
            return NativeHostWake::Wait;
        }
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        if window_resources.accept_capture(result, |record| journal.reserve(record))
            == CommandCallbackAdmission::First
        {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }
    fn on_viewport_property_command_retired(
        &self,
        retirement: eframe::NativeViewportPropertyCommandRetirement,
    ) -> NativeHostWake {
        if self.lock().record_property_retirement(retirement) {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }

    fn install_output_pass_finalizer(&self, finalizer: Arc<dyn NativeOutputPassFinalizer>) -> bool {
        let installed = {
            let records = self.lock();
            if records.attachment != HostAttachmentState::Available {
                return false;
            }
            self.output_pass_finalizer.set(finalizer)
        };
        installed.is_ok()
    }

    fn try_attach(&self, _attachment: NativeHostAttachment) -> bool {
        self.lock().attachment.try_attach()
    }

    fn detach(&self, _attachment: NativeHostAttachment) {
        let mut records = self.lock();
        records.attachment.detach();
        let NativeHostState {
            window_resources,
            journal,
            ..
        } = &mut *records;
        window_resources.detach_observation(|record| journal.reserve(record));
        window_resources.detach_capture(|record| journal.reserve(record));
    }

    fn begin_deferred_viewport_create(
        &self,
        attempt: NativeViewportCreateAttempt,
    ) -> NativeViewportCreateAdmission {
        self.admit_viewport_create_attempt(attempt)
    }

    fn render_hidden_deferred_viewport(&self, viewport_id: ViewportId) -> bool {
        self.hidden_render_enabled(viewport_id)
    }

    fn on_window_event(&self, event: NativeWindowEvent<'_>) -> NativeWindowEventDisposition {
        // Freeze the route and publish the event atomically with respect to
        // route retirement. Otherwise retirement could report quiescence
        // after route lookup but before this event became visible.
        let mut records = self.lock();
        if !records.accepts_window_event(event.event()) {
            return NativeWindowEventDisposition::Continue;
        }
        if NativeWindowEventClass::classify(event.event()) == NativeWindowEventClass::Pointer {
            // The lock keeps this one complete admission valid through capture.
            // No pointer payload is allocated before its growth is prepaid.
            if records
                .admit_pointer_callback(NativeWindowEventRecord::pointer_callback_capacity_bytes())
            {
                let record = NativeWindowEventRecord::from_eframe(event, &mut records.viewports);
                debug_assert!(
                    record.owned_descendant_capacity_bytes()
                        <= NativeWindowEventRecord::pointer_callback_capacity_bytes()
                );
                let recorded = records.record_window_event(record);
                debug_assert!(recorded, "the admitted pointer retains its raw slot");
            }
            return NativeWindowEventDisposition::Continue;
        }
        let record = NativeWindowEventRecord::from_eframe(event, &mut records.viewports);
        let disposition = match (record.binding(), record.close_request()) {
            (Some(_), Some(request)) => NativeWindowEventDisposition::HoldClose(request),
            _ => NativeWindowEventDisposition::Continue,
        };
        if records.capture_window_event(record) {
            disposition
        } else {
            NativeWindowEventDisposition::Continue
        }
    }

    fn on_device_removed(&self, removal: NativeDeviceRemoval) {
        let mut records = self.lock();
        if !records.mode.accepts_device_removal() {
            return;
        }
        records.record_device_removal(NativeDeviceRemovalRecord::from_eframe(removal));
    }

    fn on_global_focus(&self, observation: NativeGlobalFocusObservation) -> NativeHostWake {
        let mut records = self.lock();
        let record = NativeGlobalFocusRecord::capture(observation, &records.viewports);
        if records.record_global_focus(record) {
            NativeHostWake::RepaintRoot
        } else if records.global_focus_refresh_pending && records.mode.accepts_new_work() {
            NativeHostWake::RepaintRootAfterCurrent
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_focus(&self, result: NativeViewportFocusResult) -> NativeHostWake {
        let mut records = self.lock();
        let record = NativeViewportFocusRecord::capture(result, &records.viewports);
        if records.record_viewport_focus(record) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_pointer_passthrough(
        &self,
        result: NativeViewportPointerPassthroughResult,
    ) -> NativeHostWake {
        let mut records = self.lock();
        let record = NativePointerPassthroughRecord::capture(result, &records.viewports);
        if records.record_viewport_pointer_passthrough(record) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_viewport_close_cancelled(&self, request: NativeViewportCloseRequest) -> NativeHostWake {
        let mut records = self.lock();
        if records.record_viewport_close_cancelled(
            NativeViewportCloseCancellationRecord::from_eframe(request),
        ) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }

    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        root_roster: Option<NativeViewportRoster<'_>>,
    ) -> NativeOutputAdmission {
        if self.reserve_output(token, window, root_roster) {
            NativeOutputAdmission::Proceed
        } else {
            NativeOutputAdmission::Defer
        }
    }

    fn on_output_pass_finalized(
        &self,
        finalization: NativeOutputPassFinalization<'_>,
    ) -> NativeHostWake {
        self.output_pass_finalizer
            .get()
            .map_or(NativeHostWake::Wait, |finalizer| {
                finalizer.finalize(finalization)
            })
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        let disposition = self.lock().record_output(result);
        match disposition {
            OutputRecordDisposition::Recorded => NativeHostWake::RepaintRoot,
            OutputRecordDisposition::RecordedAfterCurrent
            | OutputRecordDisposition::IgnoredAndWakeAfterCurrent
            | OutputRecordDisposition::ProtocolViolation => NativeHostWake::RepaintRootAfterCurrent,
            OutputRecordDisposition::IgnoredAndWake => NativeHostWake::RepaintRoot,
            OutputRecordDisposition::RecordedService => NativeHostWake::ServiceAfterCurrent,
            OutputRecordDisposition::Ignored | OutputRecordDisposition::Inactive => {
                NativeHostWake::Wait
            }
        }
    }

    fn on_viewport_create_failed(&self, failure: NativeViewportCreateFailure) -> NativeHostWake {
        self.record_eframe_viewport_create_failure(failure)
    }

    fn on_viewport_visibility(&self, result: NativeViewportVisibilityResult) -> NativeHostWake {
        let mut records = self.lock();
        let Some(binding) = records
            .window_resources
            .visibility_callback_binding(
                result.viewport_id(),
                result.token(),
                result.window_id(),
                result.visible(),
            )
            .or_else(|| {
                records
                    .viewports
                    .binding_for_event(result.window_id(), Some(result.viewport_id()))
            })
        else {
            return NativeHostWake::Wait;
        };
        if records.record_viewport_visibility(NativeViewportVisibilityRecord::from_eframe(
            result, binding,
        )) {
            NativeHostWake::RepaintRoot
        } else {
            NativeHostWake::Wait
        }
    }
}
