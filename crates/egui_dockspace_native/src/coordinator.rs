//! Thin coordinator between eframe callbacks and the renderer-neutral session.

use std::collections::{BTreeMap, VecDeque};
use std::ops::{Deref, DerefMut};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken,
    NativeCloseEffectAcknowledgement, NativeCloseState, NativeEffectResult,
    NativeEffectSubmissionError, NativePointerInput, NativePointerRoster, NativeReceiverAnswer,
    NativeReceiverQuery, NativeSurfaceBinding, NativeWindowFacts, NativeWorkAreaBinding,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationResult,
};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputResult, NativeOutputStatus, NativeOutputToken,
    NativeWindowEvent, egui::ViewportId,
};
use winit::window::WindowId;

use crate::error::{
    NativeHostProtocolError, NativeOutputBindingError, NativeOutputBindingErrorKind,
    NativeOutputReservationError, NativeOutputReservationErrorKind,
};
use crate::viewport_map::NativeViewportMap;
use crate::{NativeRuntimeError, NativeViewportBindingError, NativeWindowEventRecord};

#[derive(Debug, Clone)]
enum HostRecord {
    WindowEvent(NativeWindowEventRecord),
    Output {
        result: NativeOutputResult,
        submitted: bool,
    },
}

#[derive(Debug, Clone, Copy)]
struct OutputReservation {
    binding: NativeSurfaceBinding,
    terminal_recorded: bool,
}

#[derive(Debug)]
struct HostRecords {
    active: bool,
    journal: VecDeque<HostRecord>,
    output_reservations: BTreeMap<NativeOutputToken, OutputReservation>,
}

impl HostRecords {
    fn active() -> Self {
        Self {
            active: true,
            journal: VecDeque::new(),
            output_reservations: BTreeMap::new(),
        }
    }

    fn reserve_output(&mut self, token: NativeOutputToken, binding: NativeSurfaceBinding) -> bool {
        if !self.active || self.output_reservations.contains_key(&token) {
            return false;
        }
        self.output_reservations.insert(
            token,
            OutputReservation {
                binding,
                terminal_recorded: false,
            },
        );
        true
    }

    fn record_output(&mut self, result: NativeOutputResult) -> bool {
        let Some(reservation) = self.output_reservations.get_mut(&result.token()) else {
            return false;
        };
        if !self.active || reservation.terminal_recorded {
            return false;
        }
        reservation.terminal_recorded = true;
        self.journal.push_back(HostRecord::Output {
            result,
            submitted: false,
        });
        true
    }

    fn deactivate(&mut self) {
        self.active = false;
        self.journal.clear();
        self.output_reservations.clear();
    }
}

#[derive(Debug)]
struct NativeHostBridge {
    records: Mutex<HostRecords>,
    viewports: Arc<Mutex<NativeViewportMap>>,
}

impl NativeHostBridge {
    fn new(viewports: Arc<Mutex<NativeViewportMap>>) -> Self {
        Self {
            records: Mutex::new(HostRecords::active()),
            viewports,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HostRecords> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_viewports(&self) -> MutexGuard<'_, NativeViewportMap> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    fn front_event(&self) -> Option<NativeWindowEventRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::WindowEvent(event)) => Some(event.clone()),
            Some(HostRecord::Output { .. }) | None => None,
        }
    }

    fn acknowledge_event(&self, ordinal: u64) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::WindowEvent(event)) if event.ordinal() == ordinal
        );
        if matches {
            records.journal.pop_front();
        }
        matches
    }

    fn output_prefix(&self) -> Vec<(NativeOutputResult, bool)> {
        self.lock()
            .journal
            .iter()
            .map_while(|record| match record {
                HostRecord::Output { result, submitted } => Some((*result, *submitted)),
                HostRecord::WindowEvent(_) => None,
            })
            .collect()
    }

    fn reserve_output(&self, token: NativeOutputToken, binding: NativeSurfaceBinding) -> bool {
        self.lock().reserve_output(token, binding)
    }

    fn output_binding(&self, token: NativeOutputToken) -> Option<NativeSurfaceBinding> {
        self.lock()
            .output_reservations
            .get(&token)
            .map(|reservation| reservation.binding)
    }

    fn mark_output_submitted(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let Some(HostRecord::Output { result, submitted }) = records.journal.iter_mut().find(
            |record| matches!(record, HostRecord::Output { result, .. } if result.token() == token),
        ) else {
            return false;
        };
        if result.token() != token {
            return false;
        }
        *submitted = true;
        true
    }

    fn discard_output(&self, token: NativeOutputToken) {
        let mut records = self.lock();
        records.journal.retain(|record| {
            !matches!(record, HostRecord::Output { result, .. } if result.token() == token)
        });
        records.output_reservations.remove(&token);
    }

    fn commit_submitted_output_prefix(&self) {
        let mut records = self.lock();
        while matches!(
            records.journal.front(),
            Some(HostRecord::Output {
                submitted: true,
                ..
            })
        ) {
            let Some(HostRecord::Output { result, .. }) = records.journal.pop_front() else {
                unreachable!("matched output record must still be at the journal head");
            };
            records.output_reservations.remove(&result.token());
        }
    }

    fn deactivate(&self) {
        self.lock().deactivate();
    }
}

impl NativeHostHandler for NativeHostBridge {
    fn on_window_event(&self, event: NativeWindowEvent<'_>) {
        let binding = self
            .lock_viewports()
            .binding_for_event(event.window_id(), event.viewport_id());
        let record = NativeWindowEventRecord::from_eframe(event, binding);
        let mut records = self.lock();
        if records.active {
            records.journal.push_back(HostRecord::WindowEvent(record));
        }
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        if !self.lock().record_output(result) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }
}

/// Sole native coordinator for one renderer-neutral docking session.
///
/// Native callbacks only append immutable records. Application code reduces
/// those records at the next root update, acknowledges each accepted window
/// event, renders one affine host frame, and explicitly binds every painted
/// output to the eframe output token that produced it.
pub struct NativeCoordinator {
    session: DockspaceSession,
    bridge: Arc<NativeHostBridge>,
    viewports: Arc<Mutex<NativeViewportMap>>,
    pending_outputs: BTreeMap<NativeOutputToken, PaintedSurfaceOutput>,
}

impl std::fmt::Debug for NativeCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeCoordinator")
            .field("session", &self.session)
            .field(
                "viewports",
                &self
                    .viewports
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner),
            )
            .field("pending_outputs", &self.pending_outputs.len())
            .finish_non_exhaustive()
    }
}

impl NativeCoordinator {
    /// Enrolls the one managed desktop provider for a session.
    ///
    /// # Errors
    ///
    /// Returns an error when another native or pointer provider is active or
    /// the initial pointer roster is not a complete valid checkpoint.
    pub fn new(
        mut session: DockspaceSession,
        initial_pointer_roster: NativePointerRoster,
    ) -> Result<Self, NativeRuntimeError> {
        session.enable_managed_native_host(initial_pointer_roster)?;
        let viewports = Arc::new(Mutex::new(NativeViewportMap::default()));
        Ok(Self {
            session,
            bridge: Arc::new(NativeHostBridge::new(viewports.clone())),
            viewports,
            pending_outputs: BTreeMap::new(),
        })
    }

    /// Returns the eframe host handler to install in [`eframe::NativeOptions`].
    #[must_use]
    pub fn native_host_handler(&self) -> Arc<dyn NativeHostHandler> {
        self.bridge.clone()
    }

    /// Returns the published product view owner.
    #[must_use]
    pub const fn session(&self) -> &DockspaceSession {
        &self.session
    }

    /// Associates one eframe viewport with an exact current core binding.
    ///
    /// # Errors
    ///
    /// Returns an error when either identity is already associated with a
    /// different peer.
    pub fn bind_viewport(
        &mut self,
        viewport: ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .bind(viewport, window, binding)
    }

    /// Replaces one viewport association only after proving its predecessor.
    ///
    /// This explicit compare-and-replace operation prevents a delayed A1
    /// callback from overwriting a recreated A2 native binding.
    ///
    /// # Errors
    ///
    /// Returns an error when the predecessor is stale, the successor changes
    /// the logical surface, or a peer identity is already bound elsewhere.
    pub fn replace_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        successor_window: WindowId,
        successor: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(viewport, expected, successor_window, successor)
    }

    /// Removes one viewport association after proving its exact incarnation.
    ///
    /// # Errors
    ///
    /// Returns an error when the viewport is absent or `expected` is stale.
    pub fn unbind_viewport(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
    ) -> Result<NativeSurfaceBinding, NativeViewportBindingError> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove_viewport(viewport, expected)
    }

    /// Returns the current exact binding for one eframe viewport.
    #[must_use]
    pub fn viewport_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding(viewport)
    }

    /// Returns the eframe viewport currently presenting one logical surface.
    #[must_use]
    pub fn surface_viewport(&self, surface: SurfaceId) -> Option<ViewportId> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .viewport(surface)
    }

    /// Returns the next event which is safe to translate.
    ///
    /// A presentation result at the journal head is submitted first and forms
    /// a frame barrier. The event behind that result is not exposed until the
    /// host commits the returned frame.
    ///
    /// # Errors
    ///
    /// Returns an error when a terminal output is still waiting for its affine
    /// painted output or core rejects its presentation result.
    pub fn next_window_event(
        &mut self,
    ) -> Result<Option<NativeWindowEventRecord>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        Ok(self.bridge.front_event())
    }

    /// Acknowledges the exact event after its typed facts have been accepted.
    ///
    /// The event remains in the journal until this method succeeds, so a
    /// failed translation or dropped host frame can retry the same raw edge.
    ///
    /// # Errors
    ///
    /// Returns an error unless `ordinal` names the exact journal-head event.
    pub fn acknowledge_window_event(&self, ordinal: u64) -> Result<(), NativeRuntimeError> {
        if self.bridge.acknowledge_event(ordinal) {
            Ok(())
        } else {
            Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into())
        }
    }

    /// Registers one application root window with the core-owned native roster.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface is absent or already has an active
    /// native registration.
    pub fn register_native_root(
        &mut self,
        surface: SurfaceId,
        window: HostWindowToken,
    ) -> Result<(), NativeRuntimeError> {
        self.session.register_native_root(surface, window)?;
        Ok(())
    }

    /// Submits one complete managed-desktop platform snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error when the roster is incomplete, stale, or internally
    /// contradictory.
    pub fn report_snapshot(
        &mut self,
        windows: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
        work_areas: NativeWorkAreaRoster,
    ) -> Result<(), NativeRuntimeError> {
        self.session
            .report_managed_native_snapshot(windows, work_areas)?;
        Ok(())
    }

    /// Explicitly revokes authoritative native inventory for the next frame.
    ///
    /// # Errors
    ///
    /// Returns an error when no managed native provider is active.
    pub fn report_inventory_unknown(&mut self) -> Result<(), NativeRuntimeError> {
        self.session.report_native_inventory_unknown()?;
        Ok(())
    }

    /// Returns a current exact work-area binding for outside-all routing.
    #[must_use]
    pub fn native_work_area(
        &self,
        token: dockspace::runtime::HostWorkAreaToken,
    ) -> Option<NativeWorkAreaBinding> {
        self.session.native_work_area(token)
    }

    /// Records one complete event-time pointer fact.
    ///
    /// # Errors
    ///
    /// Returns an error when the edge names stale authority or cannot follow
    /// the active provider sequence.
    pub fn record_pointer(&mut self, input: NativePointerInput) -> Result<(), NativeRuntimeError> {
        self.session.record_native_pointer(input)?;
        Ok(())
    }

    /// Publishes one exact native close edge.
    ///
    /// # Errors
    ///
    /// Returns an error when the close edge is stale or belongs to another provider.
    pub fn publish_close(
        &mut self,
        binding: NativeSurfaceBinding,
        state: NativeCloseState,
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    ) -> Result<(), NativeRuntimeError> {
        self.session
            .publish_native_close(binding, state, acknowledgement)?;
        Ok(())
    }

    /// Records one negative or indeterminate result for an emitted native effect.
    ///
    /// Successful dispatch acknowledgements are attached to the subsequent
    /// exact property observation instead of being reported through this lane.
    ///
    /// # Errors
    ///
    /// Returns an error retaining the result when its provider or binding is stale.
    pub fn report_effect_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Result<(), Box<NativeEffectSubmissionError>> {
        self.session
            .report_native_effect_result(result)
            .map_err(Box::new)
    }

    /// Records permanent host quiescence for one retired binding.
    ///
    /// # Errors
    ///
    /// Returns an error while any callback, route, or sidecar may still name
    /// the binding, or when the binding remains live.
    pub fn report_binding_quiescence(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeRuntimeError> {
        self.session.report_native_binding_quiescence(binding)?;
        Ok(())
    }

    /// Begins one native host frame after applying queued renderer settlements.
    ///
    /// # Errors
    ///
    /// Returns an error when a renderer result is stale or the core rejects the
    /// frozen native input prefix.
    pub fn begin_host_frame(
        &mut self,
        resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<NativeHostFrame<'_>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        if self.bridge.front_event().is_some() {
            return Err(NativeHostProtocolError::WindowEventPending.into());
        }
        Ok(NativeHostFrame {
            frame: self.session.begin_native_host_frame(resolve)?,
            bridge: self.bridge.clone(),
        })
    }

    /// Reserves an eframe output token before its renderer callback can report
    /// a terminal result.
    ///
    /// # Errors
    ///
    /// Returns an error when the viewport has no exact binding or the token
    /// was already reserved.
    pub fn reserve_painted_output(
        &self,
        token: NativeOutputToken,
    ) -> Result<NativeSurfaceBinding, NativeOutputReservationError> {
        let Some(binding) = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding(token.viewport_id())
        else {
            return Err(NativeOutputReservationError::new(
                NativeOutputReservationErrorKind::ViewportUnbound,
                token,
            ));
        };
        if !self.bridge.reserve_output(token, binding) {
            return Err(NativeOutputReservationError::new(
                NativeOutputReservationErrorKind::TokenAlreadyReserved,
                token,
            ));
        }
        Ok(binding)
    }

    /// Binds one affine painted output to the token active while its viewport UI ran.
    ///
    /// # Errors
    ///
    /// Returns an error carrying the output when the viewport is unknown, the
    /// logical surface differs, or the token is already pending.
    pub fn bind_painted_output(
        &mut self,
        token: NativeOutputToken,
        output: PaintedSurfaceOutput,
    ) -> Result<(), NativeOutputBindingError> {
        let Some(binding) = self.bridge.output_binding(token) else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::TokenNotReserved,
                token,
                output,
            ));
        };
        if binding.surface() != output.surface() {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::SurfaceMismatch,
                token,
                output,
            ));
        }
        if self.pending_outputs.contains_key(&token) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::OutputAlreadyBound,
                token,
                output,
            ));
        }
        self.pending_outputs.insert(token, output);
        Ok(())
    }

    fn prepare_output_prefix(&mut self) -> Result<(), NativeRuntimeError> {
        for (result, submitted) in self.bridge.output_prefix() {
            if submitted {
                continue;
            }
            let Some(output) = self.pending_outputs.remove(&result.token()) else {
                return Err(NativeHostProtocolError::OutputAwaitingAttachment.into());
            };
            let presentation = match result.status() {
                NativeOutputStatus::Presented => SurfacePresentationResult::Presented,
                NativeOutputStatus::NotPresented => SurfacePresentationResult::Dropped,
            };
            if let Err(error) = self
                .session
                .report_surface_presentation(output, presentation)
            {
                self.bridge.discard_output(result.token());
                return Err(error.into());
            }
            debug_assert!(self.bridge.mark_output_submitted(result.token()));
        }
        Ok(())
    }
}

impl Drop for NativeCoordinator {
    fn drop(&mut self) {
        self.bridge.deactivate();
    }
}

/// Affine native host frame which commits the output barrier and only then
/// releases later callback records.
pub struct NativeHostFrame<'session> {
    frame: DockspaceHostFrame<'session>,
    bridge: Arc<NativeHostBridge>,
}

impl std::fmt::Debug for NativeHostFrame<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeHostFrame")
            .finish_non_exhaustive()
    }
}

impl<'session> Deref for NativeHostFrame<'session> {
    type Target = DockspaceHostFrame<'session>;

    fn deref(&self) -> &Self::Target {
        &self.frame
    }
}

impl DerefMut for NativeHostFrame<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.frame
    }
}

impl NativeHostFrame<'_> {
    /// Commits the core frame and releases all presentation records included in
    /// that committed causal boundary.
    ///
    /// # Errors
    ///
    /// Returns an error without releasing the callback-order barrier when the
    /// core rejects the candidate frame.
    pub fn commit(self) -> Result<HostFrameReport, NativeRuntimeError> {
        let Self { frame, bridge } = self;
        let report = frame.commit()?;
        bridge.commit_submitted_output_prefix();
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{HostInputOutcome, SurfaceUnavailableReason};
    use winit::event::WindowEvent;

    use super::*;

    const FIRST_SURFACE: SurfaceId = SurfaceId::new(1);
    const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);

    fn coordinator() -> NativeCoordinator {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                FIRST_SURFACE,
                DockspaceRootLayout::new(
                    RootId::new(1),
                    DockspaceNode::central_tabs([ItemId::new(1)]),
                ),
            ),
            DockspaceSurfaceLayout::new(
                SECOND_SURFACE,
                DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
            ),
        ])
        .expect("test layout validates");
        let session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        NativeCoordinator::new(session, NativePointerRoster::Exact(Vec::new()))
            .expect("managed coordinator enrolls")
    }

    fn register_roots(
        coordinator: &mut NativeCoordinator,
    ) -> (NativeSurfaceBinding, NativeSurfaceBinding) {
        coordinator
            .register_native_root(FIRST_SURFACE, HostWindowToken::new(11))
            .expect("first root registration queues");
        coordinator
            .register_native_root(SECOND_SURFACE, HostWindowToken::new(22))
            .expect("second root registration queues");
        let mut frame = coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("test host settles every surface");
        let report = frame.commit().expect("registration frame commits");
        let bindings: Vec<_> = report
            .inputs()
            .iter()
            .filter_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .collect();
        assert_eq!(bindings.len(), 2);
        let first = bindings
            .iter()
            .copied()
            .find(|binding| binding.surface() == FIRST_SURFACE)
            .expect("first binding emitted");
        let second = bindings
            .iter()
            .copied()
            .find(|binding| binding.surface() == SECOND_SURFACE)
            .expect("second binding emitted");
        (first, second)
    }

    #[test]
    fn coordinator_owns_registration_and_viewport_identity() {
        let mut coordinator = coordinator();
        let (first, second) = register_roots(&mut coordinator);
        let child = ViewportId::from_hash_of("second-native-surface");

        coordinator
            .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
            .expect("root viewport binds");
        coordinator
            .bind_viewport(child, WindowId::from(22), second)
            .expect("child viewport binds");

        assert_eq!(coordinator.viewport_binding(ViewportId::ROOT), Some(first));
        assert_eq!(coordinator.surface_viewport(SECOND_SURFACE), Some(child));
        assert_eq!(
            coordinator
                .unbind_viewport(child, second)
                .expect("exact child binding retires"),
            second
        );
        assert_eq!(coordinator.surface_viewport(SECOND_SURFACE), None);
    }

    #[test]
    fn viewport_mapping_rejects_aliasing_without_mutation() {
        let mut coordinator = coordinator();
        let (first, second) = register_roots(&mut coordinator);
        let child = ViewportId::from_hash_of("second-native-surface");
        coordinator
            .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
            .expect("root viewport binds");
        coordinator
            .bind_viewport(child, WindowId::from(22), second)
            .expect("child viewport binds");

        assert!(matches!(
            coordinator.bind_viewport(ViewportId::ROOT, WindowId::from(22), second),
            Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport: ViewportId::ROOT,
                existing: FIRST_SURFACE,
            })
        ));
        assert!(matches!(
            coordinator.bind_viewport(child, WindowId::from(11), first),
            Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport,
                existing: SECOND_SURFACE,
            }) if viewport == child
        ));
        assert_eq!(coordinator.viewport_binding(ViewportId::ROOT), Some(first));
        assert_eq!(coordinator.viewport_binding(child), Some(second));
    }

    #[test]
    fn stale_unbind_cannot_remove_the_current_viewport_binding() {
        let mut coordinator = coordinator();
        let (first, second) = register_roots(&mut coordinator);
        let child = ViewportId::from_hash_of("second-native-surface");
        coordinator
            .bind_viewport(child, WindowId::from(22), second)
            .expect("child viewport binds");

        assert!(matches!(
            coordinator.unbind_viewport(child, first),
            Err(NativeViewportBindingError::BindingMismatch { viewport, .. })
                if viewport == child
        ));
        assert_eq!(coordinator.viewport_binding(child), Some(second));
    }

    #[test]
    fn window_event_remains_pending_until_exact_acknowledgement() {
        let mut coordinator = coordinator();
        let (first, _) = register_roots(&mut coordinator);
        let window = WindowId::from(11);
        coordinator
            .bind_viewport(ViewportId::ROOT, window, first)
            .expect("root viewport binds");
        let event = NativeWindowEventRecord::for_test(
            7,
            window,
            Some(ViewportId::ROOT),
            Some(first),
            WindowEvent::Focused(true),
        );
        coordinator
            .bridge
            .lock()
            .journal
            .push_back(HostRecord::WindowEvent(event));

        let pending = coordinator
            .next_window_event()
            .expect("journal inspection succeeds")
            .expect("event remains visible");
        assert_eq!(pending.ordinal(), 7);
        assert_eq!(pending.binding(), Some(first));
        assert_eq!(
            coordinator
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .expect_err("unacknowledged event blocks the core boundary")
                .kind(),
            crate::NativeRuntimeErrorKind::HostProtocol
        );
        assert_eq!(
            coordinator
                .acknowledge_window_event(8)
                .expect_err("wrong event identity is rejected")
                .kind(),
            crate::NativeRuntimeErrorKind::HostProtocol
        );
        assert_eq!(
            coordinator
                .next_window_event()
                .expect("journal remains readable")
                .map(|event| event.ordinal()),
            Some(7)
        );
        coordinator
            .acknowledge_window_event(7)
            .expect("exact event acknowledgement advances the journal");

        let mut frame = coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("acknowledged journal permits the next core boundary");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("test host settles every surface");
        frame.commit().expect("frame commits");
    }
}
