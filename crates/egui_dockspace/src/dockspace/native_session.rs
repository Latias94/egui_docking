//! Owned affine session for native input-before-paint integration.

#![allow(
    clippy::result_large_err,
    reason = "the public facade preserves structured transactional diagnostics"
)]

use std::marker::PhantomData;
use std::rc::Rc;
use std::sync::{Arc, Weak};

use dockspace::backend::engine::{
    BackendIngressProgress, CoreHostFrame, EngineInput, HostFrameView,
};
use dockspace::backend::ingress::{BackendIngressBatch, BackendIngressLease};
use dockspace::backend::presentation_observation::{
    NativeStagingPresentation, PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use dockspace::ids::SurfaceId;
use dockspace::policy::DockPolicy;
use dockspace::scene_manifest::MeasurementUnavailableReason;
use egui::{Context, FullOutput, RawInput, Ui};

use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::receiver::{PaintReceiverFingerprint, PaintReceiverLookup};
use crate::response::SurfacePaintResponse;
use crate::style::DockStyle;

use super::host_frame::{HostFrameState, HostFrameStateSlot};
use super::{
    Dockspace, DockspaceHostFrame, EGUI_APPLICATION_INPUT_SOURCE, EguiFrameScheduleKey,
    EguiOuterFrameCommit, ExactNativeViewport, PreparedEguiOuterFrameCommit,
};

/// Adapter-side lifecycle state for one owned native session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum NativeSessionStatus {
    /// No session has been enrolled.
    Idle,
    /// The affine capability was dropped before completing.
    Abandoned,
    /// A live capability still owns the host-frame state.
    Active,
}

/// Weak registry that blocks facade mutations without owning the session.
#[derive(Default)]
pub(super) struct NativeSessionRegistry {
    active: Option<Weak<()>>,
}

impl NativeSessionRegistry {
    pub(super) fn enroll(&mut self) -> NativeSessionLease {
        debug_assert!(self.active.is_none());
        let identity = Arc::new(());
        self.active = Some(Arc::downgrade(&identity));
        NativeSessionLease {
            identity,
            thread_bound: PhantomData,
        }
    }

    pub(super) fn reap_abandoned(&mut self) -> NativeSessionStatus {
        let Some(active) = self.active.as_ref() else {
            return NativeSessionStatus::Idle;
        };
        if active.upgrade().is_some() {
            NativeSessionStatus::Active
        } else {
            self.active = None;
            NativeSessionStatus::Abandoned
        }
    }

    pub(super) fn validate(&self, lease: &NativeSessionLease) -> bool {
        self.active
            .as_ref()
            .and_then(Weak::upgrade)
            .is_some_and(|active| Arc::ptr_eq(&active, &lease.identity))
    }

    pub(super) fn complete(&mut self, lease: &NativeSessionLease) {
        if self.validate(lease) {
            self.active = None;
        }
    }
}

/// Unforgeable identity for one exact native session.
///
/// `Rc` appears only as a marker so the capability cannot cross threads. Native
/// viewport callbacks must return to the event-loop owner that began the cycle.
pub(super) struct NativeSessionLease {
    identity: Arc<()>,
    thread_bound: PhantomData<Rc<()>>,
}

/// Owned native session while its exact input prefix is still being reduced.
///
/// This affine capability intentionally does not borrow [`Dockspace`]. It can be
/// retained by a native runtime across all viewport callbacks, while the weak
/// lease in the facade prevents any unrelated mutation from overtaking it.
#[must_use = "dropping the session aborts its uncommitted host frame"]
pub struct EguiNativeInputSession {
    lease: NativeSessionLease,
    state: Option<HostFrameState>,
}

impl EguiNativeInputSession {
    pub(super) const fn new(lease: NativeSessionLease, state: HostFrameState) -> Self {
        Self {
            lease,
            state: Some(state),
        }
    }

    fn state(&self) -> &HostFrameState {
        self.state
            .as_ref()
            .expect("a live native input session retains its host-frame state")
    }

    fn state_mut(&mut self) -> &mut HostFrameState {
        self.state
            .as_mut()
            .expect("a live native input session retains its host-frame state")
    }

    /// Returns the host-owned schedule identity.
    #[must_use]
    pub fn key(&self) -> EguiFrameScheduleKey {
        self.state().key()
    }

    /// Returns the exact pre-paint receiver authority for this input prefix.
    #[must_use]
    pub fn receiver_view(&self) -> HostFrameView<'_> {
        self.state().view()
    }

    /// Returns the joined backend provider frozen by the core.
    #[must_use]
    pub fn backend_ingress_provider(&self) -> Option<BackendIngressLease> {
        self.state()
            .input_core_frame()
            .and_then(CoreHostFrame::backend_ingress_provider)
    }

    /// Returns the typed core failure retained after a rejected ingress prefix.
    ///
    /// This is diagnostic-only. The affine input session remains poisoned and
    /// must not be resumed after such a failure.
    #[must_use]
    pub fn input_prefix_error(&self) -> Option<&dockspace::backend::engine::EngineError> {
        self.state()
            .input_core_frame()
            .and_then(CoreHostFrame::input_prefix_error)
    }

    /// Replays one immutable backend batch until it completes or needs receipts.
    ///
    /// # Errors
    ///
    /// Returns an error when the batch does not extend this provider's exact
    /// committed prefix or fails core ingress validation.
    ///
    /// # Panics
    ///
    /// Panics only if an internal type-state invariant was violated and an input
    /// session no longer owns an input-phase core capability.
    pub fn submit_ingress(
        &mut self,
        batch: BackendIngressBatch,
    ) -> Result<BackendIngressProgress, DockspaceError> {
        Ok(self
            .state_mut()
            .input_core_frame_mut()
            .expect("a native input session cannot hold a presentation capability")
            .submit_backend_ingress(batch)
            .map_err(DockspaceError::from_detail)?)
    }

    /// Returns the exact receiver challenge for the paused pointer record.
    #[must_use]
    pub fn pointer_receiver_candidates(
        &self,
    ) -> Option<&dockspace::backend::pointer_receiver::PointerReceiverCandidateRoster> {
        self.state()
            .input_core_frame()
            .and_then(CoreHostFrame::pointer_receiver_candidates)
    }

    /// Resolves one event-time receiver against an exact retained output.
    ///
    /// # Errors
    ///
    /// Returns a host-protocol [`DockspaceError`] when `dockspace` is not the
    /// exact facade that created this session.
    pub fn resolve_receiver(
        &self,
        dockspace: &Dockspace,
        native: ExactNativeViewport,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
        widget_pass: u64,
        fingerprint: PaintReceiverFingerprint,
    ) -> Result<PaintReceiverLookup, DockspaceError> {
        dockspace.validate_native_session(&self.lease)?;
        let route = self.state().resolve_native_input_receiver(native)?;
        if output.surface() != route.surface() {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::NativeReceiverOutputSurfaceMismatch {
                    native,
                    expected: route.surface(),
                    submitted: output.surface(),
                },
            ));
        }
        if authority.binding() != Some(route.core()) || !authority.matches_output(output) {
            return Ok(PaintReceiverLookup::GenerationUnavailable);
        }
        Ok(dockspace.renderer.resolve_receiver(
            output,
            authority,
            native.viewport(),
            widget_pass,
            fingerprint,
        ))
    }

    /// Answers the paused pointer record and resumes the immutable batch.
    ///
    /// # Errors
    ///
    /// Returns an error when the receipts do not answer the exact paused
    /// candidate roster or the underlying batch cannot resume.
    ///
    /// # Panics
    ///
    /// Panics only if an internal type-state invariant was violated and an input
    /// session no longer owns an input-phase core capability.
    pub fn submit_pointer_receiver_receipts(
        &mut self,
        receipts: dockspace::backend::pointer_receiver::PointerReceiverReceiptBatch,
    ) -> Result<BackendIngressProgress, DockspaceError> {
        Ok(self
            .state_mut()
            .input_core_frame_mut()
            .expect("a native input session cannot hold a presentation capability")
            .submit_backend_pointer_receiver_receipts(receipts)
            .map_err(DockspaceError::from_detail)?)
    }

    /// Closes causal ingress and enters the terminal configuration phase.
    ///
    /// # Errors
    ///
    /// Returns an error when the immutable ingress suffix is incomplete or the
    /// core cannot freeze a post-input presentation capability.
    pub fn into_configuration(mut self) -> Result<EguiNativeConfigurationSession, DockspaceError> {
        self.state_mut().begin_configuration_phase()?;
        Ok(EguiNativeConfigurationSession {
            lease: self.lease,
            state: self.state,
        })
    }

    /// Closes input without configuration and freezes the post-input surface roster.
    ///
    /// This convenience transition is equivalent to entering an empty terminal
    /// configuration phase and immediately closing it.
    pub fn into_presentation(self) -> Result<EguiNativePresentationSession, DockspaceError> {
        self.into_configuration()?.into_presentation()
    }
}

/// Owned native session after causal ingress and before presentation begins.
///
/// Policy and style updates belong to this terminal phase. They execute after
/// the current frame's measured contributions and publish atomically with the
/// core frame; the current paint therefore remains non-authoritative.
#[must_use = "dropping the session aborts its uncommitted host frame"]
pub struct EguiNativeConfigurationSession {
    lease: NativeSessionLease,
    state: Option<HostFrameState>,
}

impl EguiNativeConfigurationSession {
    fn state(&self) -> &HostFrameState {
        self.state
            .as_ref()
            .expect("a live native configuration session retains its host-frame state")
    }

    fn state_mut(&mut self) -> &mut HostFrameState {
        self.state
            .as_mut()
            .expect("a live native configuration session retains its host-frame state")
    }

    fn append_configuration(
        &mut self,
        input: EngineInput,
    ) -> Result<dockspace::ids::SourceSequence, DockspaceError> {
        let sequence = self
            .state()
            .semantic_source_sequence()
            .checked_next()
            .ok_or(
                crate::error::DockspaceErrorSource::InputSourceSequenceExhausted {
                    input_source: EGUI_APPLICATION_INPUT_SOURCE,
                },
            )?;
        self.state_mut()
            .input_core_frame_mut()
            .expect("a native configuration session retains its core input capability")
            .append_configuration(EGUI_APPLICATION_INPUT_SOURCE, sequence, input)
            .map_err(DockspaceError::from_detail)?;
        self.state_mut().set_semantic_source_sequence(sequence);
        self.state_mut().mark_terminal_configuration_pending();
        Ok(sequence)
    }

    /// Replaces core docking policy at this frame's terminal configuration boundary.
    ///
    /// # Errors
    ///
    /// Returns an error for a mismatched dockspace lease, exhausted source
    /// sequence, or rejected core configuration phase.
    pub fn set_policy(
        &mut self,
        dockspace: &Dockspace,
        policy: DockPolicy,
    ) -> Result<(), DockspaceError> {
        dockspace.validate_native_session(&self.lease)?;
        let expected = self.state().view().version();
        self.append_configuration(EngineInput::ReplacePolicy { expected, policy })?;
        Ok(())
    }

    /// Replaces semantic geometry and the complete egui style atomically.
    ///
    /// Multiple style updates in one configuration session use last-wins
    /// adapter semantics while retaining every core configuration outcome.
    ///
    /// # Errors
    ///
    /// Returns an error for a mismatched dockspace lease, invalid geometry,
    /// exhausted renderer/source revisions, or a rejected core phase.
    pub fn set_style(
        &mut self,
        dockspace: &Dockspace,
        style: DockStyle,
    ) -> Result<(), DockspaceError> {
        dockspace.validate_native_session(&self.lease)?;
        style.validate().map_err(DockspaceError::from_detail)?;
        let config = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let prepared = dockspace
            .renderer
            .prepare_style_replacement(style)
            .map_err(DockspaceError::from_detail)?;
        let expected = self.state().view().version();
        let source_sequence =
            self.append_configuration(EngineInput::ReplacePresentationConfig { expected, config })?;
        self.state_mut()
            .stage_style_replacement(source_sequence, prepared);
        Ok(())
    }

    /// Freezes the terminally configured post-input roster used for paint.
    ///
    /// # Errors
    ///
    /// Returns an error when the core cannot enter presentation after the
    /// completed ingress and configuration prefix.
    pub fn into_presentation(mut self) -> Result<EguiNativePresentationSession, DockspaceError> {
        self.state_mut().close_input()?;
        Ok(EguiNativePresentationSession {
            lease: self.lease,
            state: self.state,
        })
    }
}

/// Owned native session after input closes and before atomic publication.
#[must_use = "dropping the session aborts its uncommitted host frame"]
pub struct EguiNativePresentationSession {
    lease: NativeSessionLease,
    state: Option<HostFrameState>,
}

impl EguiNativePresentationSession {
    fn state(&self) -> &HostFrameState {
        self.state
            .as_ref()
            .expect("a live native presentation session retains its host-frame state")
    }

    fn state_mut(&mut self) -> &mut HostFrameState {
        self.state
            .as_mut()
            .expect("a live native presentation session retains its host-frame state")
    }

    fn with_driver<T>(
        &mut self,
        dockspace: &mut Dockspace,
        operation: impl FnOnce(&mut DockspaceHostFrame<'_>) -> Result<T, DockspaceError>,
    ) -> Result<T, DockspaceError> {
        dockspace.validate_native_session(&self.lease)?;
        let state = self
            .state
            .take()
            .expect("a live native presentation session retains its host-frame state");
        let mut driver = DockspaceHostFrame {
            dockspace,
            state: HostFrameStateSlot::new(state),
        };
        let result = operation(&mut driver);
        self.state = driver.state.take();
        result
    }

    /// Returns the exact post-input presentation authority.
    #[must_use]
    pub fn view(&self) -> HostFrameView<'_> {
        self.state().view()
    }

    /// Returns the complete post-input logical surface roster.
    #[must_use]
    pub fn expected_surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.state().expected_surfaces()
    }

    /// Returns every non-interactive native staging request frozen by the core.
    ///
    /// These requests are physical output slots, not logical dock surfaces.
    /// Each must either be painted through [`Self::show_native_staging`] (or
    /// [`Self::run_native_staging`]) and confirmed, or it is explicitly
    /// reported unavailable when the session finishes.
    #[must_use]
    pub fn native_staging_requests(
        &self,
    ) -> impl ExactSizeIterator<Item = NativeStagingPresentation> + '_ {
        self.state().native_staging_presentations()
    }

    /// Measures and paints one post-input surface pass.
    ///
    /// # Errors
    ///
    /// Returns an error for a mismatched session lease, invalid surface slot,
    /// unavailable measurement, or renderer preparation failure.
    pub fn show_native_surface(
        &mut self,
        dockspace: &mut Dockspace,
        native: ExactNativeViewport,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<SurfacePaintResponse, DockspaceError> {
        let route = self.state().resolve_native_callback(native)?;
        if ui.ctx().viewport_id() != native.viewport() {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::NativeCallbackViewportMismatch {
                    native,
                    submitted: ui.ctx().viewport_id(),
                },
            ));
        }
        self.state_mut().record_native_surface_pass(route)?;
        let surface = route.surface();
        self.with_driver(dockspace, |driver| driver.show_surface(surface, ui, panes))
    }

    /// Paints one exact core-requested native staging pass.
    ///
    /// This callback paints only an adapter-owned non-interactive fill. It does
    /// not invoke [`PaneView::ui`], install pointer receivers, or expose dock
    /// interaction authority while ownership is still staged.
    pub fn show_native_staging(
        &mut self,
        dockspace: &mut Dockspace,
        native: ExactNativeViewport,
        ui: &mut Ui,
    ) -> Result<NativeStagingPresentation, DockspaceError> {
        let (route, presentation) = self.state().resolve_native_staging_callback(native)?;
        if ui.ctx().viewport_id() != native.viewport() {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::NativeCallbackViewportMismatch {
                    native,
                    submitted: ui.ctx().viewport_id(),
                },
            ));
        }
        self.state_mut().record_native_surface_pass(route)?;
        self.with_driver(dockspace, |driver| {
            driver.show_native_staging(presentation, ui)
        })?;
        Ok(presentation)
    }

    /// Runs and owns one complete post-input egui surface pass.
    ///
    /// # Errors
    ///
    /// Returns an error for a mismatched session lease, invalid surface slot,
    /// incomplete egui output, or renderer preparation failure.
    pub fn run_native_surface(
        &mut self,
        dockspace: &mut Dockspace,
        native: ExactNativeViewport,
        context: &Context,
        input: RawInput,
        panes: &mut dyn PaneView,
    ) -> Result<SurfacePaintResponse, DockspaceError> {
        let route = self.state().resolve_native_callback(native)?;
        if input.viewport_id != native.viewport() {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::NativeCallbackViewportMismatch {
                    native,
                    submitted: input.viewport_id,
                },
            ));
        }
        let surface = route.surface();
        let viewport = input.viewport_id;
        self.state_mut().record_native_surface_pass(route)?;
        self.with_driver(dockspace, |driver| {
            let mut paint = None;
            let output = context.run_ui(input, |ui| {
                paint = Some(driver.show_surface(surface, ui, panes));
            });
            let paint = match paint {
                Some(Ok(paint)) => paint,
                Some(Err(error)) => {
                    driver.defer_full_output(output);
                    return Err(error);
                }
                None => {
                    driver.defer_full_output(output);
                    return Err(DockspaceError::from_source(
                        crate::error::DockspaceErrorSource::OuterHostSurfaceOutputUnconfirmed {
                            surface,
                        },
                    ));
                }
            };
            driver.confirm_surface_output(surface, context, viewport, output)?;
            Ok(paint)
        })
    }

    /// Runs one complete non-interactive native staging egui pass.
    ///
    /// The returned request is the exact core slot represented by the output.
    /// Renderer presentation is still reported later through the affine output
    /// settlement returned by [`Self::finish`].
    pub fn run_native_staging(
        &mut self,
        dockspace: &mut Dockspace,
        native: ExactNativeViewport,
        context: &Context,
        input: RawInput,
    ) -> Result<NativeStagingPresentation, DockspaceError> {
        let (route, presentation) = self.state().resolve_native_staging_callback(native)?;
        if input.viewport_id != native.viewport() {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::NativeCallbackViewportMismatch {
                    native,
                    submitted: input.viewport_id,
                },
            ));
        }
        let surface = route.surface();
        self.state_mut().record_native_surface_pass(route)?;
        self.with_driver(dockspace, |driver| {
            let mut paint = None;
            let output = context.run_ui(input, |ui| {
                paint = Some(driver.show_native_staging(presentation, ui));
            });
            match paint {
                Some(Ok(())) => {}
                Some(Err(error)) => {
                    driver.defer_full_output(output);
                    return Err(error);
                }
                None => {
                    driver.defer_full_output(output);
                    return Err(DockspaceError::from_source(
                        crate::error::DockspaceErrorSource::OuterHostSurfaceOutputUnconfirmed {
                            surface,
                        },
                    ));
                }
            }
            driver.confirm_surface_output(surface, context, native.viewport(), output)?;
            Ok(())
        })?;
        Ok(presentation)
    }

    /// Confirms one externally-owned hosted-cycle output at the final roster boundary.
    ///
    /// The exact native lifetime must match both the callback route and the
    /// post-ingress core binding. This is the split counterpart of
    /// [`Self::run_native_surface`] for eframe's begin/UI/end hosted hooks. On
    /// success the output is moved into the affine frame and the supplied slot
    /// becomes [`FullOutput::default`]; commit returns that same owner to the host.
    pub fn confirm_native_surface_output(
        &mut self,
        dockspace: &mut Dockspace,
        native: ExactNativeViewport,
        context: &Context,
        output: &mut FullOutput,
    ) -> Result<(), DockspaceError> {
        let route = self.state().resolve_native_presentation(native)?;
        self.state()
            .validate_native_surface_output(route.surface(), native)?;
        let surface = route.surface();
        self.with_driver(dockspace, |driver| {
            driver.confirm_external_surface_output(surface, context, native.viewport(), output)
        })
    }

    /// Confirms one externally-owned staging output at the final roster boundary.
    ///
    /// The callback route, core staging request, and exact native lifetime are
    /// revalidated before the [`FullOutput`] enters the affine settlement path.
    /// Successful confirmation moves the output out of the supplied slot until
    /// the enclosing frame commits.
    pub fn confirm_native_staging_output(
        &mut self,
        dockspace: &mut Dockspace,
        native: ExactNativeViewport,
        context: &Context,
        output: &mut FullOutput,
    ) -> Result<NativeStagingPresentation, DockspaceError> {
        let (route, presentation) = self.state().resolve_native_staging_presentation(native)?;
        self.state()
            .validate_native_surface_output(route.surface(), native)?;
        let surface = route.surface();
        self.with_driver(dockspace, |driver| {
            driver.confirm_external_surface_output(surface, context, native.viewport(), output)
        })?;
        Ok(presentation)
    }

    /// Supplies an explicit unavailable fact for one post-input surface.
    ///
    /// # Errors
    ///
    /// Returns an error for a mismatched session lease, invalid surface slot,
    /// or rejected core contribution.
    pub fn mark_surface_unavailable(
        &mut self,
        dockspace: &mut Dockspace,
        surface: SurfaceId,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), DockspaceError> {
        self.with_driver(dockspace, |driver| {
            driver.mark_surface_unavailable(surface, reason)
        })
    }

    /// Atomically publishes the post-input presentation and adapter sidecars.
    ///
    /// # Errors
    ///
    /// Returns an error when the lease mismatches, the complete roster is not
    /// sealed, or core/renderer publication fails. Failure commits nothing.
    ///
    /// # Panics
    ///
    /// Panics only if an internal affine-state invariant was violated and the
    /// live session no longer owns its host-frame capability.
    pub fn finish(self, dockspace: &mut Dockspace) -> Result<EguiOuterFrameCommit, DockspaceError> {
        let prepared = self.prepare_finish(dockspace)?;
        prepared.commit(dockspace)
    }

    /// Preflights the complete frame while leaving the live dockspace unpublished.
    ///
    /// Native hosts retain the returned affine capability until their own output
    /// and window-roster transaction has sealed.
    pub fn prepare_finish(
        mut self,
        dockspace: &mut Dockspace,
    ) -> Result<PreparedEguiOuterFrameCommit, DockspaceError> {
        dockspace.validate_native_session(&self.lease)?;
        let state = self
            .state
            .take()
            .expect("a live native presentation session retains its host-frame state");
        let result = {
            let mut driver = DockspaceHostFrame {
                dockspace,
                state: HostFrameStateSlot::new(state),
            };
            driver.prepare_inner()
        };
        dockspace.complete_native_session(&self.lease);
        result
    }
}
