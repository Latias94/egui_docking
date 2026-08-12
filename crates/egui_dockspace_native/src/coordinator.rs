//! Thin coordinator between eframe callbacks and the renderer-neutral session.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, PoisonError};

use dockspace::geometry::PhysicalRect;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceSession, HostWindowToken, NativeCloseEffectAcknowledgement, NativeCloseState,
    NativeEffectResult, NativeEffectSubmissionError, NativeHostErrorKind, NativePointerInput,
    NativePointerRoster, NativeReceiverAnswer, NativeReceiverQuery, NativeSurfaceBinding,
    NativeWindowFacts, NativeWorkAreaBinding, NativeWorkAreaRoster, PaintedSurfaceOutput,
    SurfacePresentationResult,
};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputStatus, NativeOutputToken, NativePhysicalRect,
    egui::ViewportId,
};
use winit::window::WindowId;

use crate::effect_coordinator::{
    NativeEffectCoordinator, NativeViewportEffectKind, NativeViewportEffectPlan,
};
use crate::error::{
    NativeHostProtocolError, NativeOutputBindingError, NativeOutputBindingErrorKind,
};
use crate::host_frame::NativeHostFrame;
#[cfg(test)]
use crate::mailbox::{HostRecord, OutputReservation};
use crate::mailbox::{
    NativeHostBridge, NativeViewportCreateFailureRecord, NativeViewportRosterRecord,
};
use crate::pointer_event::{NativePointerTranslation, NativePointerTranslator};
use crate::receiver::NativeReceiverStore;
use crate::viewport_map::NativeViewportMap;
use crate::window_snapshot::{CompiledWindowObservation, compile_window_observation};
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
    pending_destroyed: BTreeMap<SurfaceId, NativeSurfaceBinding>,
    receivers: NativeReceiverStore,
    effects: NativeEffectCoordinator,
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
            .field("pending_destroyed", &self.pending_destroyed)
            .field("receivers", &self.receivers)
            .field("effects", &self.effects)
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
            pending_destroyed: BTreeMap::new(),
            receivers: NativeReceiverStore::default(),
            effects: NativeEffectCoordinator::default(),
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

    /// Reserves one eframe viewport for an exact binding before eframe creates its window.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding is stale or the viewport/surface is already reserved by
    /// another exact peer.
    pub fn reserve_viewport(
        &mut self,
        viewport: ViewportId,
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
            .reserve(viewport, binding)
    }

    /// Attaches the native window created for an exact reserved viewport binding.
    ///
    /// # Errors
    ///
    /// Returns an error when the reservation changed or the window is already attached elsewhere.
    pub fn attach_viewport_window(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
        window: WindowId,
    ) -> Result<(), NativeViewportBindingError> {
        if !self.session.is_current_native_binding(expected) {
            return Err(NativeViewportBindingError::BindingNotCurrent {
                viewport,
                surface: expected.surface(),
            });
        }
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .attach(viewport, expected, window)
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
            .replace(viewport, expected, successor_window, successor)?;
        self.receivers.retire_binding(expected);
        Ok(())
    }

    /// Replaces one exact viewport reservation before the successor window exists.
    ///
    /// The predecessor window route is removed immediately, so any late callback from it becomes
    /// unknown rather than acquiring the successor's authority.
    ///
    /// # Errors
    ///
    /// Returns an error when the predecessor is stale or the successor changes logical surface.
    pub fn reserve_viewport_replacement(
        &mut self,
        viewport: ViewportId,
        expected: NativeSurfaceBinding,
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
            .reserve_replacement(viewport, expected, successor)?;
        self.receivers.retire_binding(expected);
        Ok(())
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
        let removed = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .remove_viewport(viewport, expected)?;
        self.receivers.retire_binding(removed);
        Ok(removed)
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

    /// Retains one core-emitted deferred viewport effect until eframe reports
    /// an exact success or failure callback.
    ///
    /// The caller must invoke this only after the core host frame which emitted
    /// `request` committed. A rejected request is returned unchanged so the
    /// driver can convert it into an explicit typed dispatch result.
    pub(crate) fn retain_viewport_effect(
        &mut self,
        viewport: ViewportId,
        request: dockspace::runtime::NativeEffectRequest,
    ) -> Result<NativeViewportEffectPlan, dockspace::runtime::NativeEffectRequest> {
        let Some(plan) = self.effects.plan(viewport, &request) else {
            return Err(request);
        };
        if !self.session.is_current_native_binding(plan.binding()) {
            return Err(request);
        }
        let predecessor = {
            let viewports = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            viewports.binding(viewport)
        };
        if matches!(plan.kind(), NativeViewportEffectKind::Replacement) && predecessor.is_none() {
            return Err(request);
        }
        let Some(requested_rect) = exact_native_rect(plan.placement()) else {
            return Err(request);
        };
        if !self
            .bridge
            .reserve_create(viewport, plan.binding(), requested_rect)
        {
            return Err(request);
        }
        if let Err(request) = self.effects.insert(plan, request) {
            debug_assert!(self.bridge.clear_create(viewport, plan.binding()));
            return Err(request);
        }

        let reserve_result = {
            let mut viewports = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            match (plan.kind(), predecessor) {
                (NativeViewportEffectKind::Create, None) => {
                    viewports.reserve(viewport, plan.binding())
                }
                (NativeViewportEffectKind::Replacement, Some(predecessor)) => {
                    viewports.reserve_replacement(viewport, predecessor, plan.binding())
                }
                (NativeViewportEffectKind::Create, Some(_)) => {
                    Err(NativeViewportBindingError::ViewportAlreadyBound {
                        viewport,
                        existing: viewports
                            .binding(viewport)
                            .expect("matched viewport binding remains present")
                            .surface(),
                    })
                }
                (NativeViewportEffectKind::Replacement, None) => {
                    unreachable!("replacement predecessor was checked before retention")
                }
            }
        };
        if reserve_result.is_err() {
            debug_assert!(self.bridge.clear_create(viewport, plan.binding()));
            return Err(self
                .effects
                .remove_unstarted(plan)
                .expect("failed viewport reservation retains its affine request"));
        }
        Ok(plan)
    }

    /// Reduces one exact deferred-viewport failure without losing the affine
    /// effect result across a retryable core rejection.
    pub(crate) fn reduce_next_viewport_create_failure(
        &mut self,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(failure) = self.next_viewport_create_failure()? else {
            return Ok(false);
        };
        if !self.effects.prepare_failure(failure) {
            return Err(NativeHostProtocolError::ViewportCreateFailureWithoutEffect.into());
        }
        if let Some(result) = self.effects.take_failure_result(failure)
            && let Err(error) = self.session.report_native_effect_result(result)
        {
            let (kind, result) = error.into_parts();
            self.effects
                .restore_failure_result(failure, result)
                .unwrap_or_else(|_| {
                    panic!("retryable native effect result lost its exact failure owner")
                });
            return Err(NativeHostProtocolError::NativeEffectResultRejected(kind).into());
        }
        debug_assert!(self.effects.failure_reported(failure));
        self.acknowledge_viewport_create_failure(failure)?;
        debug_assert!(self.effects.finish_failure(failure));
        Ok(true)
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

    /// Returns the next exact deferred-viewport creation failure in callback order.
    ///
    /// The binding is frozen when eframe reports the failed OS attempt, so replacing the viewport
    /// before the next root update cannot retarget the failure to a newer incarnation.
    ///
    /// # Errors
    ///
    /// Returns an error when an earlier renderer settlement cannot be applied.
    fn next_viewport_create_failure(
        &mut self,
    ) -> Result<Option<NativeViewportCreateFailureRecord>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        Ok(self.bridge.front_viewport_create_failure())
    }

    /// Acknowledges one exact viewport creation failure after the driver has converted it into the
    /// matching affine native-effect result.
    ///
    /// # Errors
    ///
    /// Returns an error unless `failure` is the callback journal head.
    fn acknowledge_viewport_create_failure(
        &mut self,
        failure: NativeViewportCreateFailureRecord,
    ) -> Result<(), NativeRuntimeError> {
        if !self.bridge.acknowledge_viewport_create_failure(failure) {
            return Err(
                NativeHostProtocolError::ViewportCreateFailureAcknowledgementMismatch.into(),
            );
        }
        self.bridge
            .clear_create(failure.viewport(), failure.binding());
        let mut viewports = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if viewports.binding(failure.viewport()) == Some(failure.binding()) {
            let removed = viewports.remove_viewport(failure.viewport(), failure.binding());
            debug_assert!(
                removed.is_ok(),
                "exact failed reservation must be removable"
            );
        }
        drop(viewports);
        self.receivers.retire_binding(failure.binding());
        Ok(())
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

    /// Reduces the next callback record into one pending host-frame boundary.
    ///
    /// The mailbox remains the only callback-order authority. Pointer facts,
    /// close requests, exact destruction events, viewport rosters, and create
    /// failures are acknowledged only after their corresponding core input is
    /// retained. A later callback cannot join that boundary: the driver must
    /// commit or retry the exact core frame before reducing another record.
    pub(crate) fn reduce_callback_head(&mut self) -> Result<bool, NativeRuntimeError> {
        if self.bridge.callback_boundary_pending() {
            return Ok(false);
        }
        if self.reduce_next_viewport_roster()? {
            return Ok(true);
        }
        if self.reduce_next_viewport_create_failure()? {
            return Ok(true);
        }
        let Some(record) = self.next_window_event()? else {
            return Ok(false);
        };
        let mut candidate = self.pointer_translator;
        match candidate.translate(&record) {
            NativePointerTranslation::NotPointer => self.reduce_non_pointer_event(&record)?,
            NativePointerTranslation::Ignored => {}
            NativePointerTranslation::Input(input) => self.record_pointer(input)?,
        }
        self.acknowledge_window_event(record.ordinal())?;
        self.pointer_translator = candidate;
        Ok(true)
    }

    fn reduce_non_pointer_event(
        &mut self,
        record: &NativeWindowEventRecord,
    ) -> Result<(), NativeRuntimeError> {
        let Some(binding) = record.binding() else {
            return Ok(());
        };
        match record.event() {
            winit::event::WindowEvent::CloseRequested => {
                self.publish_close(binding, NativeCloseState::Requested, None)?;
            }
            winit::event::WindowEvent::Destroyed => {
                if self.session.is_current_native_binding(binding) {
                    self.pending_destroyed.insert(binding.surface(), binding);
                    self.receivers.retire_binding(binding);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// Reduces one same-callback root viewport roster into a complete platform fact.
    ///
    /// Eframe may expose live windows that are not dockspace surfaces, so
    /// unmapped records are ignored. Core still performs the exact-set check
    /// against every current dockspace binding. Any incomplete, stale, or
    /// malformed candidate revokes inventory authority as `Unknown`; it never
    /// infers destruction from absence.
    pub(crate) fn reduce_next_viewport_roster(&mut self) -> Result<bool, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        let Some(roster) = self.bridge.front_viewport_roster() else {
            return Ok(false);
        };
        self.record_viewport_roster(&roster)?;
        if !self.bridge.acknowledge_viewport_roster() {
            return Err(NativeHostProtocolError::ViewportRosterAcknowledgementMismatch.into());
        }
        Ok(true)
    }

    fn record_viewport_roster(
        &mut self,
        roster: &NativeViewportRosterRecord,
    ) -> Result<(), NativeRuntimeError> {
        self.pending_destroyed
            .retain(|_, binding| self.session.is_current_native_binding(*binding));
        if let Ok(mut observations) = Self::compile_viewport_roster(roster) {
            observations.extend(self.pending_destroyed.values().copied().map(|binding| {
                CompiledWindowObservation::new(binding, NativeWindowFacts::destroyed())
            }));
            match self.session.report_managed_native_snapshot(
                observations
                    .iter()
                    .map(|observation| (observation.binding(), observation.facts())),
                NativeWorkAreaRoster::Unknown,
            ) {
                Ok(()) => {
                    self.pending_destroyed.clear();
                    return Ok(());
                }
                Err(error)
                    if matches!(
                        error.native_kind(),
                        Some(
                            NativeHostErrorKind::StaleBinding
                                | NativeHostErrorKind::InvalidFacts
                                | NativeHostErrorKind::OperationConflict
                        )
                    ) => {}
                Err(error) => return Err(error.into()),
            }
        }
        self.session.report_native_inventory_unknown()?;
        Ok(())
    }

    fn compile_viewport_roster(
        roster: &NativeViewportRosterRecord,
    ) -> Result<Vec<CompiledWindowObservation>, NativeHostProtocolError> {
        #[cfg(test)]
        if let Some(compiled) = roster.compiled_override() {
            return compiled;
        }

        roster
            .observations()
            .iter()
            .copied()
            .map(|observation| {
                compile_window_observation(observation.binding(), observation.snapshot())
            })
            .collect()
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
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        Ok(NativeHostFrame::new(
            self.session.begin_native_host_frame(resolve)?,
            self.bridge.clone(),
        ))
    }

    /// Begins one native frame using only final-pass egui receiver authority.
    pub(crate) fn begin_resolved_host_frame(
        &mut self,
        context: &eframe::egui::Context,
    ) -> Result<NativeHostFrame<'_>, NativeRuntimeError> {
        self.prepare_output_prefix()?;
        if self.bridge.has_pending_input() && !self.bridge.callback_boundary_pending() {
            return Err(NativeHostProtocolError::CallbackRecordPending.into());
        }
        let bridge = self.bridge.clone();
        let receivers = &self.receivers;
        let frame = self
            .session
            .begin_native_host_frame(|query| receivers.resolve(context, query))?;
        Ok(NativeHostFrame::new(frame, bridge))
    }

    /// Binds one affine painted output and its final-pass receiver roster to the
    /// token active while that viewport UI ran.
    ///
    /// # Errors
    ///
    /// Returns an error carrying the output when the token was not announced,
    /// the exact native binding differs, or the token is already pending.
    pub fn bind_painted_surface(
        &mut self,
        token: NativeOutputToken,
        output: PaintedSurfaceOutput,
        paint: &egui_dockspace::native_support::NativeSurfacePaint,
    ) -> Result<(), NativeOutputBindingError> {
        let Some(reservation) = self.bridge.output_reservation(token) else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::TokenNotReserved,
                token,
                output,
            ));
        };
        if self.pending_outputs.contains_key(&token) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::OutputAlreadyBound,
                token,
                output,
            ));
        }
        let current_binding = self
            .viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .binding_for_output(token.viewport_id(), token.window_id());
        let binding = reservation.binding().or(current_binding);
        let Some(binding) = binding else {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::RouteUnavailable,
                token,
                output,
            ));
        };
        if reservation.binding().is_some() && current_binding != Some(binding) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if !output.matches_native_binding(binding) {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if self
            .receivers
            .stage(token, binding, &output, paint)
            .is_err()
        {
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::ReceiverMismatch,
                token,
                output,
            ));
        }
        if !self.bridge.attach_output_binding(token, binding) {
            self.receivers.abandon(token);
            return Err(NativeOutputBindingError::new(
                NativeOutputBindingErrorKind::BindingMismatch,
                token,
                output,
            ));
        }
        if self.effects.pending_binding(token.viewport_id()) == Some(binding) {
            let attach = self
                .viewports
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .attach(token.viewport_id(), binding, token.window_id());
            if attach.is_err() {
                self.receivers.abandon(token);
                return Err(NativeOutputBindingError::new(
                    NativeOutputBindingErrorKind::BindingMismatch,
                    token,
                    output,
                ));
            }
            let request = self
                .effects
                .take_for_output(token.viewport_id(), binding)
                .expect("matching pending viewport effect remains present");
            let acknowledgement = request.accepted();
            debug_assert!(
                acknowledgement.is_none(),
                "native create effects settle through ordinary lifecycle facts"
            );
            debug_assert!(self.bridge.clear_create(token.viewport_id(), binding));
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
        self.receivers.abandon(token);
        self.pending_outputs.remove(&token);
        NativeHostWake::RepaintRoot
    }

    fn prepare_output_prefix(&mut self) -> Result<(), NativeRuntimeError> {
        if !self.bridge.output_order_is_valid() {
            return Err(NativeHostProtocolError::OutputOrderViolation.into());
        }
        for (result, submitted) in self.bridge.output_prefix() {
            if submitted {
                continue;
            }
            let Some(output) = self.pending_outputs.remove(&result.token()) else {
                return Err(NativeHostProtocolError::OutputAwaitingAttachment.into());
            };
            let binding = self
                .bridge
                .output_reservation(result.token())
                .and_then(|reservation| reservation.binding());
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
                self.receivers.abandon(result.token());
                return Err(error.into());
            }
            match (result.status(), binding) {
                (NativeOutputStatus::Presented, Some(binding)) => {
                    self.receivers.presented(result.token(), binding);
                }
                (NativeOutputStatus::Presented, None) | (NativeOutputStatus::NotPresented, _) => {
                    self.receivers.dropped(result.token());
                }
            }
            debug_assert!(self.bridge.mark_output_submitted(result.token()));
        }
        Ok(())
    }
}

fn exact_native_rect(rect: PhysicalRect) -> Option<NativePhysicalRect> {
    Some(NativePhysicalRect::new(
        exact_i32(rect.x())?,
        exact_i32(rect.y())?,
        exact_non_zero_u32(rect.width())?,
        exact_non_zero_u32(rect.height())?,
    ))
}

fn exact_i32(value: f64) -> Option<i32> {
    (value.fract() == 0.0 && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX))
        .then_some(value as i32)
}

fn exact_non_zero_u32(value: f64) -> Option<u32> {
    (value.fract() == 0.0 && value >= 1.0 && value <= f64::from(u32::MAX)).then_some(value as u32)
}

impl Drop for NativeCoordinator {
    fn drop(&mut self) {
        self.bridge.deactivate();
    }
}

#[cfg(test)]
mod tests;
