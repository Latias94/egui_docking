//! Thin coordinator between eframe callbacks and the renderer-neutral session.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken,
    NativeCloseEffectAcknowledgement, NativeCloseState, NativeEffectResult,
    NativeEffectSubmissionError, NativePointerInput, NativePointerRoster, NativeReceiverAnswer,
    NativeReceiverQuery, NativeSurfaceBinding, NativeWindowFacts, NativeWorkAreaBinding,
    NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationResult,
};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputStatus, NativeOutputToken, egui::ViewportId,
};
use egui_dockspace::{DockStyle, PaneView};
use winit::window::WindowId;

use crate::error::{
    NativeHostProtocolError, NativeOutputBindingError, NativeOutputBindingErrorKind,
};
use crate::mailbox::NativeHostBridge;
#[cfg(test)]
use crate::mailbox::{HostRecord, OutputReservation};
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::viewport_map::NativeViewportMap;
use crate::{NativeRuntimeError, NativeViewportBindingError, NativeWindowEventRecord};

/// Sole native coordinator for one renderer-neutral docking session.
///
/// Native callbacks only append immutable records. Application code reduces
/// those records at the next root update, acknowledges each accepted window
/// event, renders one affine host frame, and explicitly binds every painted
/// output to the eframe output token that produced it.
pub(crate) struct NativeCoordinator {
    session: DockspaceSession,
    bridge: Arc<NativeHostBridge>,
    viewports: Arc<Mutex<NativeViewportMap>>,
    pending_outputs: BTreeMap<NativeOutputToken, PaintedSurfaceOutput>,
    pointer_translator: NativePointerTranslator,
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
            .field("pointer_translator", &self.pointer_translator)
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
            pointer_translator: NativePointerTranslator::default(),
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
        if !self.session.is_current_native_binding(binding) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: binding.surface(),
            });
        }
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
        if !self.session.is_current_native_binding(successor) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: successor.surface(),
            });
        }
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
    /// a frame barrier. Conversely, an acknowledged event prefix must commit
    /// before a later presentation result can enter the core prelude. This
    /// prevents the runtime's presentation phase from overtaking older native
    /// input recorded in the backend ingress lane.
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

    /// Reduces the journal head when it is a pointer event.
    ///
    /// The translator is copied before reduction. Smooth-scroll sequence state
    /// is published only after the core accepts the corresponding input and the
    /// immutable callback record is acknowledged, so a rejected edge remains
    /// replayable without advancing the adapter-owned sequence state.
    pub(crate) fn reduce_next_pointer_event(&mut self) -> Result<bool, NativeRuntimeError> {
        let Some(record) = self.next_window_event()? else {
            return Ok(false);
        };
        let mut candidate = self.pointer_translator;
        match candidate.translate(&record) {
            NativePointerTranslation::NotPointer => Ok(false),
            NativePointerTranslation::Ignored => {
                self.acknowledge_window_event(record.ordinal())?;
                self.pointer_translator = candidate;
                Ok(true)
            }
            NativePointerTranslation::Input(input) => {
                self.record_pointer(input)?;
                self.acknowledge_window_event(record.ordinal())?;
                self.pointer_translator = candidate;
                Ok(true)
            }
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

    /// Binds one affine painted output to the token active while its viewport UI ran.
    ///
    /// # Errors
    ///
    /// Returns an error carrying the output when the token was not announced,
    /// the exact native binding differs, or the token is already pending.
    pub fn bind_painted_output(
        &mut self,
        token: NativeOutputToken,
        output: PaintedSurfaceOutput,
    ) -> Result<(), NativeOutputBindingError> {
        if !self.bridge.has_output_reservation(token) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::TokenNotReserved,
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
        let binding = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_event(token.window_id(), Some(token.viewport_id()));
        let Some(binding) = binding else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::RouteUnavailable,
                token,
                output,
            ));
        };
        if !output.matches_native_binding(binding) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if !self.bridge.attach_output_binding(token, binding) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        self.pending_outputs.insert(token, output);
        Ok(())
    }

    /// Abandons one announced eframe output which produced no dockspace paint.
    ///
    /// Any attached affine output is dropped into the core's explicit
    /// not-presented queue. A later renderer callback for the token is ignored.
    /// This method must be used for mapped viewport callbacks which finish
    /// without a matching [`PaintedSurfaceOutput`].
    ///
    /// Returns [`NativeHostWake::Wait`] when the token is unknown or its
    /// presentation result has already entered a core host-frame boundary.
    /// A successful abandon requests another root update so the core can
    /// consume the affine output's explicit dropped terminal.
    #[must_use]
    pub fn abandon_output_token(&mut self, token: NativeOutputToken) -> NativeHostWake {
        if !self.bridge.abandon_output(token) {
            return NativeHostWake::Wait;
        }
        self.pending_outputs.remove(&token);
        NativeHostWake::RepaintRoot
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
                let abandoned = self.bridge.abandon_output(result.token());
                debug_assert!(abandoned, "failed presentation was not submitted");
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
pub(crate) struct NativeHostFrame<'session> {
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

impl NativeHostFrame<'_> {
    pub(crate) fn complete_unpainted_surfaces(
        &mut self,
        reason: dockspace::runtime::SurfaceUnavailableReason,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .complete_unpainted_surfaces(reason)
            .map_err(Into::into)
    }

    pub(crate) fn paint_surface(
        &mut self,
        instance_id: eframe::egui::Id,
        surface: SurfaceId,
        ui: &mut eframe::egui::Ui,
        panes: &mut dyn PaneView,
        style: &DockStyle,
    ) -> Result<egui_dockspace::native_support::NativeSurfacePaint, NativeRuntimeError> {
        egui_dockspace::native_support::paint_surface(
            &mut self.frame,
            instance_id,
            surface,
            ui,
            panes,
            style,
        )
        .map_err(Into::into)
    }

    pub(crate) fn measure_surface(
        &mut self,
        surface: SurfaceId,
        ui: &eframe::egui::Ui,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> Result<(), NativeRuntimeError> {
        egui_dockspace::native_support::measure_surface(&mut self.frame, surface, ui, panes, style)
            .map(|_| ())
            .map_err(Into::into)
    }

    pub(crate) fn confirm_surface_painted(
        &mut self,
        surface: SurfaceId,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .confirm_surface_painted(surface)
            .map_err(Into::into)
    }

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
        bridge.commit_frame_boundary();
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
        let mut native = coordinator();
        let (first, second) = register_roots(&mut native);
        let child = ViewportId::from_hash_of("second-native-surface");

        native
            .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
            .expect("root viewport binds");
        native
            .bind_viewport(child, WindowId::from(22), second)
            .expect("child viewport binds");

        assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));
        assert_eq!(native.surface_viewport(SECOND_SURFACE), Some(child));
        assert_eq!(
            native
                .unbind_viewport(child, second)
                .expect("exact child binding retires"),
            second
        );
        assert_eq!(native.surface_viewport(SECOND_SURFACE), None);
    }

    #[test]
    fn viewport_mapping_rejects_aliasing_without_mutation() {
        let mut native = coordinator();
        let (first, second) = register_roots(&mut native);
        let child = ViewportId::from_hash_of("second-native-surface");
        native
            .bind_viewport(ViewportId::ROOT, WindowId::from(11), first)
            .expect("root viewport binds");
        native
            .bind_viewport(child, WindowId::from(22), second)
            .expect("child viewport binds");

        assert!(matches!(
            native.bind_viewport(ViewportId::ROOT, WindowId::from(22), second),
            Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport: ViewportId::ROOT,
                existing: FIRST_SURFACE,
            })
        ));
        assert!(matches!(
            native.bind_viewport(child, WindowId::from(11), first),
            Err(NativeViewportBindingError::ViewportAlreadyBound {
                viewport,
                existing: SECOND_SURFACE,
            }) if viewport == child
        ));
        assert_eq!(native.viewport_binding(ViewportId::ROOT), Some(first));
        assert_eq!(native.viewport_binding(child), Some(second));
    }

    #[test]
    fn viewport_mapping_rejects_a_foreign_same_surface_binding() {
        let mut current_coordinator = coordinator();
        let (current, _) = register_roots(&mut current_coordinator);
        let mut foreign = coordinator();
        let (foreign_same_surface, _) = register_roots(&mut foreign);

        assert_eq!(current.surface(), foreign_same_surface.surface());
        assert_ne!(current, foreign_same_surface);
        assert!(matches!(
            current_coordinator.bind_viewport(
                ViewportId::ROOT,
                WindowId::from(11),
                foreign_same_surface,
            ),
            Err(NativeViewportBindingError::BindingNotCurrent {
                viewport: ViewportId::ROOT,
                surface: FIRST_SURFACE,
            })
        ));
        assert_eq!(current_coordinator.viewport_binding(ViewportId::ROOT), None);
    }

    #[test]
    fn stale_unbind_cannot_remove_the_current_viewport_binding() {
        let mut native = coordinator();
        let (first, second) = register_roots(&mut native);
        let child = ViewportId::from_hash_of("second-native-surface");
        native
            .bind_viewport(child, WindowId::from(22), second)
            .expect("child viewport binds");

        assert!(matches!(
            native.unbind_viewport(child, first),
            Err(NativeViewportBindingError::BindingMismatch { viewport, .. })
                if viewport == child
        ));
        assert_eq!(native.viewport_binding(child), Some(second));
    }

    #[test]
    fn output_reservation_binds_only_after_the_ui_callback_updates_the_viewport() {
        let mut native = coordinator();
        let (first, second) = register_roots(&mut native);
        let mut reservation = OutputReservation::unbound();

        assert_eq!(reservation.binding(), None);
        assert!(reservation.attach(second));
        assert_eq!(reservation.binding(), Some(second));
        assert!(reservation.attach(second));
        assert!(!reservation.attach(first));
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
            .push_record(HostRecord::WindowEvent(event));

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
        assert!(coordinator.bridge.event_boundary_pending());

        let mut frame = coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("acknowledged journal permits the next core boundary");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("test host settles every surface");
        frame.commit().expect("frame commits");
        assert!(!coordinator.bridge.event_boundary_pending());
    }

    #[test]
    fn pointer_routes_are_frozen_before_viewport_replacement() {
        use winit::dpi::PhysicalPosition;
        use winit::event::{
            DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
        };

        use crate::event::NativePointerRouteSnapshot;

        let mut native = coordinator();
        let (first, second) = register_roots(&mut native);
        let first_window = WindowId::from(11);
        let second_window = WindowId::from(22);
        let child = ViewportId::from_hash_of("second-native-surface");
        native
            .bind_viewport(ViewportId::ROOT, first_window, first)
            .expect("root viewport binds");
        native
            .bind_viewport(child, second_window, second)
            .expect("child viewport binds");

        let event = WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
                desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
                modifiers: None,
                hover: PointerWindowRoute::Window(second_window),
                capture: PointerWindowRoute::Window(first_window),
            },
        };
        let record = {
            let viewports = native
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            NativeWindowEventRecord::for_test_snapshot(
                7,
                first_window,
                Some(ViewportId::ROOT),
                event,
                &viewports,
            )
        };

        let mut successor_source = coordinator();
        let (successor, _) = register_roots(&mut successor_source);
        native
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .replace(ViewportId::ROOT, first, WindowId::from(33), successor)
            .expect("test viewport replacement succeeds");

        let routes = record.pointer_routes().expect("pointer routes are frozen");
        assert_eq!(routes.delivery(), NativePointerRouteSnapshot::Dock(first));
        assert_eq!(routes.hover(), NativePointerRouteSnapshot::Dock(second));
        assert_eq!(routes.capture(), NativePointerRouteSnapshot::Dock(first));
        assert_ne!(
            routes.delivery(),
            NativePointerRouteSnapshot::Dock(successor)
        );
    }

    #[test]
    fn pointer_translation_preserves_delivery_hover_and_capture() {
        use dockspace::geometry::PhysicalPoint;
        use dockspace::runtime::{
            NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
            NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
            NativePointerOwner,
        };
        use winit::dpi::PhysicalPosition;
        use winit::event::{
            DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
        };

        use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};

        let mut native = coordinator();
        let (first, second) = register_roots(&mut native);
        let first_window = WindowId::from(11);
        let second_window = WindowId::from(22);
        let child = ViewportId::from_hash_of("second-native-surface");
        native
            .bind_viewport(ViewportId::ROOT, first_window, first)
            .expect("root viewport binds");
        native
            .bind_viewport(child, second_window, second)
            .expect("child viewport binds");
        let event = WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Released,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
                desktop_position: Some(PhysicalPosition::new(110.0, 220.0)),
                modifiers: None,
                hover: PointerWindowRoute::Window(second_window),
                capture: PointerWindowRoute::Window(first_window),
            },
        };
        let record = {
            let viewports = native
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            NativeWindowEventRecord::for_test_snapshot(
                8,
                first_window,
                Some(ViewportId::ROOT),
                event,
                &viewports,
            )
        };

        let mut translator = NativePointerTranslator::default();
        let actual = translator.translate(&record);
        let expected = NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(
                    PhysicalPoint::new(110.0, 220.0).expect("test point validates"),
                ),
                NativePointerHover::Dock(second),
                None,
            ),
            NativePointerOwner::Native(first),
            NativePointerOwner::Native(first),
        );
        assert_eq!(actual, NativePointerTranslation::Input(expected));
    }

    #[test]
    fn pointer_reduction_acknowledges_only_after_core_acceptance() {
        use winit::dpi::PhysicalPosition;
        use winit::event::{
            DeviceId, ElementState, MouseButton, PointerEventFacts, PointerWindowRoute,
        };

        let mut native = coordinator();
        let (first, _) = register_roots(&mut native);
        let window = WindowId::from(11);
        native
            .bind_viewport(ViewportId::ROOT, window, first)
            .expect("root viewport binds");
        let event = WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state: ElementState::Pressed,
            button: MouseButton::Left,
            facts: PointerEventFacts {
                surface_position: Some(PhysicalPosition::new(10.0, 20.0)),
                desktop_position: Some(PhysicalPosition::new(10.0, 20.0)),
                modifiers: None,
                hover: PointerWindowRoute::Window(window),
                capture: PointerWindowRoute::Window(window),
            },
        };
        let record = {
            let viewports = native
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            NativeWindowEventRecord::for_test_snapshot(
                9,
                window,
                Some(ViewportId::ROOT),
                event,
                &viewports,
            )
        };
        native.bridge.push_record(HostRecord::WindowEvent(record));

        assert!(
            native
                .reduce_next_pointer_event()
                .expect("core accepts the current pointer binding")
        );
        assert!(
            native
                .next_window_event()
                .expect("the accepted pointer event is acknowledged")
                .is_none()
        );
    }
}
