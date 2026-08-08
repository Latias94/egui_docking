//! Authoritative egui facade over [`dockspace::backend::engine::DockEngine`].

#[cfg(any(feature = "backend", test))]
#[path = "dockspace/backend.rs"]
pub(crate) mod backend;
#[path = "dockspace/contained_resize.rs"]
mod contained_resize;
#[path = "dockspace/driver.rs"]
mod driver;
#[path = "dockspace/engine_owner.rs"]
mod engine_owner;
#[path = "dockspace/frame_contract.rs"]
mod frame_contract;
#[path = "dockspace/host_frame.rs"]
mod host_frame;
#[path = "dockspace/interaction_source.rs"]
mod interaction_source;
#[path = "dockspace/native_binding.rs"]
mod native_binding;
#[path = "dockspace/native_session.rs"]
mod native_session;
#[path = "dockspace/pane_focus.rs"]
mod pane_focus;
#[path = "dockspace/presentation_ledger.rs"]
mod presentation_ledger;

pub use self::driver::{
    DockspaceHostFrame, EguiOuterFrameCommit, EguiOuterHostFrame, PreparedEguiOuterFrameCommit,
};

use std::collections::BTreeSet;
use std::{fmt::Debug, hash::Hash};

use dockspace::backend::engine::{
    CoreHostFrame, CoreHostFramePrelude, DockEngine, EngineError, EngineInput, HostFrameView,
    HostPresentationDisposition, HostPresentationUnavailableReason, PreparedSurfaceContribution,
};
use dockspace::backend::ingress::{
    BackendIngressError, BackendIngressOrdinal, BackendIngressProviderReplacementTicket,
    BackendIngressRecorder,
};
use dockspace::backend::interaction::InteractionStatus;
use dockspace::backend::pointer_journal::{
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerScope,
};
use dockspace::backend::presentation_observation::{
    HostFrameKey, HostPresentationObservation, HostPresentationObservationEntry,
    HostPresentationOutput, HostPresentationStreamId, PresentationHostLease,
    SurfacePresentationOutputTicket,
};
use dockspace::backend::surface_recovery::{SurfaceRecoveryBootstrap, SurfaceRecoveryTarget};
use dockspace::backend::transition::{BackendIngressProviderReplacementStart, EngineTransition};
use dockspace::backend::viewport_focus::GlobalFocusedWindow;
use dockspace::command::WorkspaceCommand;
#[cfg(feature = "serde")]
use dockspace::document::{
    DockspaceDocumentRestore, DockspaceDocumentSession, PreparedDockspaceDocumentPublication,
};
use dockspace::graph::Workspace;
use dockspace::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use dockspace::intent::Authority;
use dockspace::policy::DockPolicy;
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::viewport::{ViewportBinding, ViewportRole, WindowToken};
use dockspace::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
    NativeCloseEdge, SurfaceCloseRequest,
};
use egui::{Context, Id, Ui, ViewportId};

use self::host_frame::{HostFrameState, HostFrameStateSlot};
use self::native_binding::NativeBindingRegistry;
#[cfg_attr(not(any(feature = "backend", test)), allow(unused_imports))]
pub use self::native_binding::{
    ExactNativeViewport, NativeBindingError, NativeBindingRoster, NativeCoreRoute,
    NativeViewportIncarnation,
};
#[cfg_attr(not(any(feature = "backend", test)), allow(unused_imports))]
pub use self::native_session::{
    EguiNativeConfigurationSession, EguiNativeInputSession, EguiNativePresentationSession,
};
use self::presentation_ledger::{
    AcceptedPresentationDelta, AutomaticPresentationFrame, OuterPresentationFrame,
    PresentationOutputLedger, should_request_presentation_follow_up_pass,
};
use crate::builder::DockspaceBuilder;
use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::pointer_input::EguiPointerInput;
use crate::presentation_settlement::PendingEguiPresentation;
use crate::receiver::{PaintReceiverFingerprint, PaintReceiverLookup};
use crate::render::EguiDockRenderer;
use crate::response::{
    DockspaceCloseResult, DockspaceCommandResult, DockspaceMutation, DockspaceResponse,
    HostFrameResponse, SurfaceCommitResponse,
};
use crate::style::DockStyle;

use self::engine_owner::{EguiApplicationInputOwner, EguiDockEngine, EguiEngineOwner};
use self::interaction_source::payload_surface;

const EGUI_RENDER_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0x6567_7569_5f72_656e);
const EGUI_APPLICATION_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0x6567_7569_5f61_7070);

pub use self::frame_contract::EguiFrameScheduleKey;
use self::frame_contract::{EguiHostFrameMode, EguiInputAuthority, EguiOutputBoundary};

use self::pane_focus::PaneFocusAdapterState;

/// Stateful egui adapter whose [`DockEngine`] is the sole docking authority.
pub struct Dockspace {
    pub(crate) engine: EguiDockEngine,
    pub(crate) presentation_host: PresentationHostLease,
    pub(crate) renderer: EguiDockRenderer,
    pub(crate) pane_focus: PaneFocusAdapterState,
    pub(crate) semantic_source_sequence: SourceSequence,
    last_host_frame: Option<EguiFrameScheduleKey>,
    presentation_ledger: PresentationOutputLedger,
    pub(crate) pointer_input: EguiPointerInput,
    pending_pointer_abort: bool,
    native_bindings: Option<NativeBindingRegistry>,
    native_sessions: native_session::NativeSessionRegistry,
}

impl Dockspace {
    /// Starts a builder for a stable egui instance and renderer-neutral workspace.
    pub fn builder(id_salt: impl Hash + Debug, workspace: Workspace) -> DockspaceBuilder {
        DockspaceBuilder::new(id_salt, workspace)
    }

    pub(crate) fn from_parts(
        id: Id,
        workspace: Workspace,
        policy: DockPolicy,
        style: DockStyle,
    ) -> Result<Self, DockspaceError> {
        let presentation_config = style
            .presentation_config()
            .map_err(DockspaceError::from_detail)?;
        let mut engine =
            DockEngine::new_with_presentation_config(workspace, policy, presentation_config)
                .map_err(DockspaceError::from_detail)?;
        let presentation_host = engine
            .create_presentation_host()
            .map_err(DockspaceError::from_detail)?;
        #[cfg(feature = "serde")]
        let engine =
            DockspaceDocumentSession::unbound(engine).map_err(DockspaceError::from_detail)?;
        Ok(Self {
            engine,
            presentation_host,
            renderer: EguiDockRenderer::new(id, style).map_err(DockspaceError::from_detail)?,
            pane_focus: PaneFocusAdapterState::default(),
            semantic_source_sequence: SourceSequence::default(),
            last_host_frame: None,
            presentation_ledger: PresentationOutputLedger::default(),
            pointer_input: EguiPointerInput::default(),
            pending_pointer_abort: false,
            native_bindings: None,
            native_sessions: native_session::NativeSessionRegistry::default(),
        })
    }

    /// Returns the stable egui identity used to scope every adapter widget.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.renderer.id()
    }

    /// Returns the currently published workspace.
    #[must_use]
    pub fn workspace(&self) -> &Workspace {
        self.core_engine().workspace()
    }

    /// Returns the current durable workspace version.
    #[must_use]
    pub fn version(&self) -> dockspace::runtime::WorkspaceVersion {
        self.core_engine().version()
    }

    /// Returns the current declarative docking policy.
    #[must_use]
    pub fn policy(&self) -> &DockPolicy {
        self.core_engine().policy()
    }

    pub(crate) fn core_engine(&self) -> &DockEngine {
        #[cfg(feature = "serde")]
        {
            self.engine.engine()
        }
        #[cfg(not(feature = "serde"))]
        {
            &self.engine
        }
    }

    /// Reclaims renderer resources after the native host proves exact stream quiescence.
    ///
    /// This adapter boundary is hidden from the ordinary egui facade. A native runtime must
    /// retain and retry every pair for which this returns `false`; no frame age or callback
    /// disappearance is accepted as replacement evidence.
    ///
    /// # Errors
    ///
    /// Returns an error when a retained stream belongs to another engine or presentation host.
    #[cfg(any(feature = "backend", test))]
    #[doc(hidden)]
    pub fn adapter_reclaim_quiesced_presentation_streams(
        &mut self,
        streams: &BTreeSet<(PresentationHostLease, HostPresentationStreamId)>,
    ) -> Result<bool, DockspaceError> {
        let mut all_reclaimed = true;
        let mut quiescences = Vec::with_capacity(streams.len());
        for &(presentation_host, stream) in streams {
            if !EguiEngineOwner::engine(&self.engine)
                .presentation_retention_manifest()
                .retains_stream(stream)
            {
                continue;
            }
            let Some(quiescence) = EguiEngineOwner::try_prepare_presentation_stream_quiescence(
                &self.engine,
                presentation_host,
                stream,
            )
            .map_err(DockspaceError::from_detail)?
            else {
                all_reclaimed = false;
                continue;
            };
            quiescences.push(quiescence);
        }
        if !quiescences.is_empty() {
            EguiEngineOwner::confirm_presentation_stream_quiescence_batch(
                &mut self.engine,
                quiescences,
            )
            .map_err(DockspaceError::from_detail)?;
        }

        let retention = EguiEngineOwner::engine(&self.engine).presentation_retention_manifest();
        self.presentation_ledger.retain(&retention);
        self.renderer
            .reconcile_core_retention(EguiEngineOwner::engine(&self.engine));
        Ok(all_reclaimed)
    }

    /// Enrolls the joined native platform and desktop-pointer ingress provider.
    #[cfg(any(feature = "backend", test))]
    pub fn create_backend_ingress_provider(
        &mut self,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, DockspaceError> {
        self.ensure_native_session_idle()?;
        if self.pointer_input.provider().is_some() {
            self.abort_pointer_input()?;
        }
        let recorder = EguiEngineOwner::create_backend_ingress_provider(
            &mut self.engine,
            self.presentation_host,
            pointer_committed_through,
        )
        .map_err(DockspaceError::from_detail)?;
        if let Some(bindings) = self.native_bindings.as_mut() {
            bindings.rebind_provider(recorder.lease(), self.engine.version().epoch());
        }
        Ok(recorder)
    }

    /// Settles one core-committed backend prefix after the native producer reclaims it.
    ///
    /// # Errors
    ///
    /// Returns an error when a native session is active or the core rejects the
    /// receipt's authority, commit boundary, or binding-guard roster.
    #[cfg(any(feature = "backend", test))]
    #[doc(hidden)]
    pub fn adapter_settle_backend_ingress_prefix_retirement(
        &mut self,
        receipt: &mut dockspace::backend::ingress::BackendIngressPrefixRetirementReceipt,
    ) -> Result<Vec<dockspace::viewport::ViewportBinding>, DockspaceError> {
        self.ensure_native_session_idle()?;
        Ok(
            EguiEngineOwner::settle_backend_ingress_prefix_retirement(&mut self.engine, receipt)
                .map_err(DockspaceError::from_detail)?,
        )
    }

    /// Records one exact renderer result in the joined backend ingress order.
    ///
    /// The backend recorder is already bound to this facade's core-minted
    /// presentation host. The runtime supplies only the stream observation it
    /// derived from an exact renderer result.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_presentation_observation(
        &self,
        recorder: &mut BackendIngressRecorder,
        entry: HostPresentationObservationEntry,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        Ok(self
            .engine
            .record_backend_presentation_observation(recorder, entry)
            .map_err(DockspaceError::from_detail)?)
    }

    /// Records every newly contiguous renderer settlement at the backend's
    /// current causal position.
    ///
    /// The adapter ledger reserves a capture only after the recorder accepts
    /// its entry. A failed host-frame attempt therefore replays the same record,
    /// while a committed rejection releases the reservation for an exact retry.
    #[cfg(any(feature = "backend", test))]
    pub fn record_ready_backend_presentation_observations(
        &mut self,
        recorder: &mut BackendIngressRecorder,
    ) -> Result<Vec<BackendIngressOrdinal>, DockspaceError> {
        self.ensure_native_session_idle()?;
        let mut ordinals = Vec::new();
        while let Some(capture) = self.presentation_ledger.next_ordered_outer_capture()? {
            let ordinal = self
                .engine
                .record_backend_presentation_observation(recorder, capture.entry())
                .map_err(DockspaceError::from_detail)?;
            self.presentation_ledger.stage_ordered_outer_capture(
                capture,
                recorder.lease(),
                ordinal,
            );
            ordinals.push(ordinal);
        }
        Ok(ordinals)
    }

    /// Rolls adapter-owned recorder sidecars back to one affine savepoint.
    ///
    /// Renderer completions remain terminal physical facts. Only their
    /// uncommitted causal records are released so an exact ingress replay can
    /// stage them again at the same relative position.
    #[cfg(any(feature = "backend", test))]
    pub fn rollback_backend_ingress_to(
        &mut self,
        recorder: &mut BackendIngressRecorder,
        savepoint: dockspace::backend::ingress::BackendIngressSavepoint,
    ) -> Result<(), DockspaceError> {
        self.ensure_native_session_idle()?;
        let lease = savepoint.lease();
        let recorded_through = savepoint.recorded_through();
        recorder
            .rollback_to(savepoint)
            .map_err(DockspaceError::from_detail)?;
        self.presentation_ledger
            .rollback_ordered_outer_after(lease, recorded_through);
        self.pane_focus
            .rollback_backend_observations_after(lease, recorded_through);
        #[cfg(feature = "serde")]
        self.engine
            .adapter_reconcile_pending_backend_restore_record();
        Ok(())
    }

    /// Records every pane-focus sample produced by completed native paint.
    ///
    /// Samples remain adapter-owned until the core commits their exact causal
    /// position. Recorder acceptance only reserves that position; an aborted
    /// frame replays the recorder prefix, while provider replacement returns
    /// uncommitted samples to the successor recorder.
    #[cfg(any(feature = "backend", test))]
    pub fn record_ready_backend_pane_focus_observations(
        &mut self,
        recorder: &mut BackendIngressRecorder,
    ) -> Result<Vec<BackendIngressOrdinal>, DockspaceError> {
        self.ensure_native_session_idle()?;
        self.pane_focus.reconcile_backend_provider(recorder.lease());
        let mut ordinals = Vec::new();
        while let Some(observation) = self.pane_focus.next_backend_observation() {
            let ordinal = self.record_backend_input(
                recorder,
                EngineInput::PublishPaneFocusObservation {
                    expected_epoch: observation.binding().epoch(),
                    observation,
                },
            )?;
            self.pane_focus
                .stage_backend_observation(recorder.lease(), ordinal, observation);
            ordinals.push(ordinal);
        }
        Ok(ordinals)
    }

    /// Revokes one exact joined backend after consuming its stopped producer.
    #[cfg(any(feature = "backend", test))]
    pub fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: &mut dockspace::backend::ingress::BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, DockspaceError> {
        self.ensure_native_session_idle()?;
        if self.pointer_input.provider().is_some() {
            self.abort_pointer_input()?;
        }
        let replacement =
            EguiEngineOwner::begin_backend_ingress_provider_replacement(&mut self.engine, drained)
                .map_err(DockspaceError::from_detail)?;
        self.pane_focus.accept_transition(replacement.transition());
        Ok(replacement)
    }

    /// Reissues a pending joined-provider handoff after its adapter ticket was lost.
    ///
    /// The quiescence proof remains owned by the core from the original begin
    /// boundary. This method recovers only the opaque finish capability; it
    /// does not reactivate the predecessor or infer authority from ticket loss.
    #[cfg(any(feature = "backend", test))]
    pub fn reissue_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<BackendIngressProviderReplacementTicket, DockspaceError> {
        self.ensure_native_session_idle()?;
        EguiEngineOwner::reissue_backend_ingress_provider_replacement(&mut self.engine)
            .map_err(DockspaceError::from_detail)
    }

    /// Abandons the pending joined-provider handoff and leaves no provider active.
    #[cfg(any(feature = "backend", test))]
    pub fn abort_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<EngineTransition, DockspaceError> {
        self.ensure_native_session_idle()?;
        let transition =
            EguiEngineOwner::abort_backend_ingress_provider_replacement(&mut self.engine)
                .map_err(DockspaceError::from_detail)?;
        self.pane_focus.accept_transition(&transition);
        Ok(transition)
    }

    /// Reaps a pending joined handoff only after its current ticket was dropped.
    #[cfg(any(feature = "backend", test))]
    pub fn reap_abandoned_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<Option<EngineTransition>, DockspaceError> {
        self.ensure_native_session_idle()?;
        let transition =
            EguiEngineOwner::reap_abandoned_backend_ingress_provider_replacement(&mut self.engine)
                .map_err(DockspaceError::from_detail)?;
        if let Some(transition) = &transition {
            self.pane_focus.accept_transition(transition);
        }
        Ok(transition)
    }

    /// Activates the joined successor from the predecessor's affine drain proof.
    #[cfg(any(feature = "backend", test))]
    pub fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
    ) -> Result<BackendIngressRecorder, DockspaceError> {
        self.ensure_native_session_idle()?;
        let recorder = EguiEngineOwner::finish_backend_ingress_provider_replacement(
            &mut self.engine,
            ticket,
            self.presentation_host,
        )
        .map_err(DockspaceError::from_detail)?;
        if let Some(bindings) = self.native_bindings.as_mut() {
            bindings.rebind_provider(recorder.lease(), self.engine.version().epoch());
        }
        Ok(recorder)
    }

    #[cfg(all(test, feature = "serde"))]
    pub(crate) fn set_semantic_source_sequence_for_test(&mut self, sequence: SourceSequence) {
        self.semantic_source_sequence = sequence;
    }

    /// Returns current fixed renderer geometry and colors.
    #[must_use]
    pub const fn style(&self) -> &DockStyle {
        self.renderer.style()
    }

    /// Returns the last successfully committed egui render-pass schedule identity.
    ///
    /// This is scheduling state only; it is not presentation authority.
    #[cfg(any(feature = "backend", test))]
    #[must_use]
    pub const fn last_egui_frame_schedule_key(&self) -> Option<EguiFrameScheduleKey> {
        self.last_host_frame
    }

    /// Replaces style after complete deterministic validation.
    pub fn set_style(&mut self, style: DockStyle) -> Result<DockspaceMutation, DockspaceError> {
        style.validate().map_err(DockspaceError::from_detail)?;
        self.renderer
            .ensure_style_revision_available()
            .map_err(DockspaceError::from_detail)?;
        let transition = self.submit_application_input(EngineInput::ReplacePresentationConfig {
            expected: self.engine.version(),
            config: style
                .presentation_config()
                .map_err(DockspaceError::from_detail)?,
        })?;
        self.renderer
            .replace_style(style)
            .map_err(DockspaceError::from_detail)?;
        Ok(DockspaceMutation::from_transition(&transition))
    }

    /// Submits one exact checked workspace command at an explicit application boundary.
    pub fn submit_command(
        &mut self,
        command: WorkspaceCommand,
    ) -> Result<DockspaceCommandResult, DockspaceError> {
        #[cfg(feature = "serde")]
        self.engine
            .validate_workspace_command_identity_bindings(&command)
            .map_err(DockspaceError::from_detail)?;
        let transition = self.submit_application_input(EngineInput::WorkspaceCommand {
            expected: self.engine.version(),
            command,
        })?;
        DockspaceCommandResult::from_transition(&transition)
            .ok_or(
                crate::error::DockspaceErrorSource::ApplicationOutcomeUnavailable {
                    operation: "workspace command",
                },
            )
            .map_err(Into::into)
    }

    /// Records one application workspace command in the active backend causal stream.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_command(
        &self,
        recorder: &mut BackendIngressRecorder,
        command: WorkspaceCommand,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        #[cfg(feature = "serde")]
        self.engine
            .validate_workspace_command_identity_bindings(&command)
            .map_err(DockspaceError::from_detail)?;
        self.record_backend_input(
            recorder,
            EngineInput::WorkspaceCommand {
                expected: self.engine.version(),
                command,
            },
        )
    }

    /// Records bootstrap or replacement registration for one exact native viewport.
    ///
    /// The core mints the resulting [`ViewportBinding`] when this record is
    /// reduced. A runtime must not construct an incarnation from its native
    /// window identity.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_viewport_registration(
        &self,
        recorder: &mut BackendIngressRecorder,
        surface: SurfaceId,
        token: WindowToken,
        role: ViewportRole,
        recovery_target: Option<SurfaceRecoveryTarget>,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        let provider = self
            .engine
            .platform_provider()
            .ok_or(BackendIngressError::ProviderUnavailable)
            .map_err(DockspaceError::from_detail)?;
        self.record_backend_input(
            recorder,
            EngineInput::RegisterViewport {
                provider,
                expected: self.engine.version(),
                surface,
                token,
                role,
                recovery_target,
            },
        )
    }

    /// Records the first registration of an existing docking-owned child viewport.
    ///
    /// The core, rather than the native runtime, resolves and reserves the exact recovery target.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_child_viewport_bootstrap(
        &self,
        recorder: &mut BackendIngressRecorder,
        surface: SurfaceId,
        token: WindowToken,
        recovery: SurfaceRecoveryBootstrap,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        let provider = self
            .engine
            .platform_provider()
            .ok_or(BackendIngressError::ProviderUnavailable)
            .map_err(DockspaceError::from_detail)?;
        self.record_backend_input(
            recorder,
            EngineInput::BootstrapChildViewport {
                provider,
                expected: self.engine.version(),
                surface,
                token,
                recovery,
            },
        )
    }

    /// Returns the current core-minted native binding for one logical surface.
    #[cfg(any(feature = "backend", test))]
    #[must_use]
    pub fn native_viewport_binding(&self, surface: SurfaceId) -> Option<ViewportBinding> {
        self.engine
            .viewport()
            .viewport(surface)
            .map(|record| record.binding())
    }

    /// Resolves one widget against the exact egui paint generation named by a
    /// core output and host emission.
    ///
    /// Native runtimes use this before replaying their ordered ingress batch;
    /// core independently rechecks the same output and emission when reducing
    /// the resulting semantic action.
    #[cfg(any(feature = "backend", test))]
    #[must_use]
    pub fn resolve_retained_receiver(
        &self,
        output: SurfacePresentationOutputTicket,
        emission: HostFrameKey,
        viewport: ViewportId,
        widget_pass: u64,
        fingerprint: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        self.renderer.resolve_receiver_for_emission(
            output,
            emission,
            viewport,
            widget_pass,
            fingerprint,
        )
    }

    /// Returns whether core accepts one semantic action for an exact retained output.
    #[cfg(any(feature = "backend", test))]
    #[must_use]
    pub fn retained_semantic_receiver_supports(
        &self,
        output: SurfacePresentationOutputTicket,
        emission: HostFrameKey,
        target: dockspace::backend::presentation_hit::PresentationHitRegionKind,
        action: dockspace::backend::semantic_input::SemanticReceiverAction,
    ) -> bool {
        self.engine
            .interaction_projection(output.surface())
            .is_some_and(|projection| {
                projection.output_ticket() == output
                    && projection.authority().emission() == emission
                    && projection.semantic_manifest().supports(target, action)
            })
    }

    /// Records one close decision in the active backend causal stream.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_close_resolution(
        &self,
        recorder: &mut BackendIngressRecorder,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        self.record_backend_input(
            recorder,
            EngineInput::ResolveClose {
                request,
                token,
                decision,
            },
        )
    }

    /// Records one exact native surface-close request in the active backend stream.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_surface_close_request(
        &self,
        recorder: &mut BackendIngressRecorder,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        self.record_backend_input(
            recorder,
            EngineInput::RequestSurfaceClose {
                expected: self.engine.version(),
                edge,
                request,
            },
        )
    }

    /// Records one deferred close decision in the active backend causal stream.
    #[cfg(any(feature = "backend", test))]
    pub fn record_backend_deferred_close_resolution(
        &self,
        recorder: &mut BackendIngressRecorder,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        self.record_backend_input(
            recorder,
            EngineInput::ContinueDeferredClose {
                request,
                token,
                decision,
            },
        )
    }

    /// Submits one exact application decision for a core-issued close token.
    pub fn resolve_close(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<DockspaceCloseResult, DockspaceError> {
        let transition = self.submit_application_input(EngineInput::ResolveClose {
            request,
            token,
            decision,
        })?;
        DockspaceCloseResult::from_transition(&transition)
            .ok_or(
                crate::error::DockspaceErrorSource::ApplicationOutcomeUnavailable {
                    operation: "close decision",
                },
            )
            .map_err(Into::into)
    }

    /// Submits one exact terminal decision for a deferred close continuation.
    pub fn resolve_deferred_close(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<DockspaceCloseResult, DockspaceError> {
        let transition = self.submit_application_input(EngineInput::ContinueDeferredClose {
            request,
            token,
            decision,
        })?;
        DockspaceCloseResult::from_transition(&transition)
            .ok_or(
                crate::error::DockspaceErrorSource::ApplicationOutcomeUnavailable {
                    operation: "deferred close decision",
                },
            )
            .map_err(Into::into)
    }

    /// Submits an epoch-advancing complete workspace replacement.
    pub fn replace_workspace(
        &mut self,
        workspace: Workspace,
    ) -> Result<DockspaceMutation, DockspaceError> {
        #[cfg(feature = "serde")]
        self.engine
            .validate_workspace_identity_bindings(&workspace)
            .map_err(DockspaceError::from_detail)?;
        let transition = self.submit_application_input(EngineInput::ReplaceWorkspace(workspace))?;
        Ok(DockspaceMutation::from_transition(&transition))
    }

    /// Submits a complete policy replacement against the current engine version.
    pub fn set_policy(&mut self, policy: DockPolicy) -> Result<DockspaceMutation, DockspaceError> {
        let transition = self.submit_application_input(EngineInput::ReplacePolicy {
            expected: self.engine.version(),
            policy,
        })?;
        Ok(DockspaceMutation::from_transition(&transition))
    }

    pub(crate) fn submit_application_input(
        &mut self,
        input: EngineInput,
    ) -> Result<EngineTransition, DockspaceError> {
        self.ensure_native_session_idle()?;
        Self::submit_application_input_on_owner(
            &mut self.engine,
            self.presentation_host,
            &mut self.pointer_input,
            &mut self.semantic_source_sequence,
            &mut self.pane_focus,
            input,
        )
        .map_err(Into::into)
    }

    #[cfg(feature = "serde")]
    pub(crate) fn submit_document_restore_input(
        restore: &mut DockspaceDocumentRestore<'_>,
        presentation_host: PresentationHostLease,
        pointer_input: &mut EguiPointerInput,
        semantic_source_sequence: &mut SourceSequence,
        input: EngineInput,
    ) -> Result<
        (PreparedDockspaceDocumentPublication, SourceSequence),
        crate::error::DockspaceErrorSource,
    > {
        if restore.engine().backend_ingress_provider().is_some() {
            return Err(crate::error::DockspaceErrorSource::BackendApplicationInputRequiresIngress);
        }
        let sequence = semantic_source_sequence.checked_next().ok_or(
            crate::error::DockspaceErrorSource::InputSourceSequenceExhausted {
                input_source: EGUI_APPLICATION_INPUT_SOURCE,
            },
        )?;
        let mut prelude = restore.adapter_begin_host_frame(presentation_host)?;
        prelude.submit_presentation_observation(HostPresentationObservation::NoUpdate)?;
        let mut frame = prelude.seal(restore.engine())?;
        pointer_input.submit_empty_interval(&mut frame)?;
        frame.append_input(EGUI_APPLICATION_INPUT_SOURCE, sequence, input)?;
        Self::append_unavailable_surface_contributions(
            &mut frame,
            MeasurementUnavailableReason::Deferred,
        )?;
        let mut frame = frame.into_presentation()?;
        for obligation in frame.take_presentation_obligations()? {
            frame.resolve_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )?;
        }
        Ok((restore.adapter_prepare_publication(frame)?, sequence))
    }

    #[cfg(feature = "serde")]
    pub(crate) fn accept_document_restore_transition(
        &mut self,
        sequence: SourceSequence,
        transition: &EngineTransition,
    ) {
        self.semantic_source_sequence = sequence;
        self.pane_focus.accept_transition(transition);
    }

    fn submit_application_input_on_owner(
        engine: &mut impl EguiApplicationInputOwner,
        presentation_host: PresentationHostLease,
        pointer_input: &mut EguiPointerInput,
        semantic_source_sequence: &mut SourceSequence,
        pane_focus: &mut PaneFocusAdapterState,
        input: EngineInput,
    ) -> Result<EngineTransition, crate::error::DockspaceErrorSource> {
        if engine
            .application_engine()
            .backend_ingress_provider()
            .is_some()
        {
            return Err(crate::error::DockspaceErrorSource::BackendApplicationInputRequiresIngress);
        }
        let sequence = semantic_source_sequence.checked_next().ok_or(
            crate::error::DockspaceErrorSource::InputSourceSequenceExhausted {
                input_source: EGUI_APPLICATION_INPUT_SOURCE,
            },
        )?;
        let mut prelude = engine.begin_application_host_frame(presentation_host)?;
        prelude.submit_presentation_observation(HostPresentationObservation::NoUpdate)?;
        let mut frame = prelude.seal(engine.application_engine())?;
        pointer_input.submit_empty_interval(&mut frame)?;
        if matches!(
            input,
            EngineInput::ReplacePolicy { .. } | EngineInput::ReplacePresentationConfig { .. }
        ) {
            frame.append_configuration(EGUI_APPLICATION_INPUT_SOURCE, sequence, input)?;
        } else {
            frame.append_input(EGUI_APPLICATION_INPUT_SOURCE, sequence, input)?;
        }
        Self::append_unavailable_surface_contributions(
            &mut frame,
            MeasurementUnavailableReason::Deferred,
        )?;
        let mut frame = frame.into_presentation()?;
        for obligation in frame.take_presentation_obligations()? {
            frame.resolve_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )?;
        }
        let transition = engine.prepare_application_host_frame(frame)?.commit()?;
        engine.reconcile_application_sidecars();
        *semantic_source_sequence = sequence;
        pane_focus.accept_transition(&transition);
        Ok(transition)
    }

    #[cfg(any(feature = "backend", test))]
    fn record_backend_input(
        &self,
        recorder: &mut BackendIngressRecorder,
        input: EngineInput,
    ) -> Result<BackendIngressOrdinal, DockspaceError> {
        let active = self
            .engine
            .backend_ingress_provider()
            .ok_or(BackendIngressError::ProviderUnavailable)
            .map_err(DockspaceError::from_detail)?;
        if recorder.lease() != active {
            return Err(DockspaceError::from_detail(
                BackendIngressError::ProviderLeaseMismatch {
                    expected: active,
                    submitted: recorder.lease(),
                },
            ));
        }
        Ok(recorder
            .record_semantic_input(input)
            .map_err(DockspaceError::from_detail)?)
    }

    fn append_unavailable_surface_contributions(
        frame: &mut CoreHostFrame,
        reason: MeasurementUnavailableReason,
    ) -> Result<(), crate::error::DockspaceErrorSource> {
        let contributions = {
            let view = frame.view();
            frame
                .surfaces()
                .map(
                    |surface| -> Result<
                        PreparedSurfaceContribution,
                        crate::error::DockspaceErrorSource,
                    > {
                        let token = view.begin_surface_contribution(surface)?;
                        Ok(view.prepare_surface_unavailable_contribution(token, reason)?)
                    },
                )
                .collect::<Result<Vec<_>, crate::error::DockspaceErrorSource>>()?
        };
        for contribution in contributions {
            frame.push_surface_contribution(contribution)?;
        }
        Ok(())
    }

    /// Starts one strict single-surface host frame.
    ///
    /// The core freezes its roster before any egui work is collected. This crates.io facade accepts
    /// at most one logical surface because ordinary callbacks cannot establish a global
    /// cross-viewport boundary. Explicit host frames deliberately submit no
    /// final-presentation assertion because this API has no exact egui pass
    /// completion fact. Consequently the public facade records paint-only
    /// contributions, not core presentation emissions that could never receive
    /// a terminal settlement.
    #[cfg(any(feature = "backend", test))]
    pub fn begin_host_frame(
        &mut self,
        key: EguiFrameScheduleKey,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceError> {
        self.ensure_native_session_idle()?;
        if self.pointer_input.provider().is_some() {
            self.abort_pointer_input()?;
        }
        let host = self.begin_host_frame_inner(
            key,
            None,
            EguiHostFrameMode::SingleSurface,
            EguiInputAuthority::FrameworkResponses,
            EguiOutputBoundary::UnobservableCallback,
        )?;
        let surface_count = host.expected_surfaces().len();
        if surface_count > 1 {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::MultiSurfaceHostFrameUnsupported {
                    surface_count,
                },
            ));
        }
        Ok(host)
    }

    /// Starts one outer-host frame for the complete logical surface roster.
    ///
    /// The caller must keep this capability across every egui viewport callback
    /// belonging to the same real host input epoch and call
    /// [`EguiOuterHostFrame::run_surface`] for every available surface.
    /// Repeated egui passes replace the prior draft for that surface; only
    /// internally confirmed final-pass drafts are staged into the core reducer
    /// when [`EguiOuterHostFrame::finish`] is called.
    ///
    /// This boundary proves which completed [`egui::FullOutput`] belongs to
    /// each painted surface, but not whether a renderer presented it. Finishing
    /// the frame therefore returns an affine settlement capability with every
    /// publishable output. If reducing framework input changed the presentation,
    /// the superseded output remains paint-only and a fresh pass is requested.
    #[cfg(any(feature = "backend", test))]
    pub fn begin_outer_frame(
        &mut self,
        key: EguiFrameScheduleKey,
    ) -> Result<EguiOuterHostFrame<'_>, DockspaceError> {
        self.ensure_native_session_idle()?;
        self.ensure_outer_pointer_provider()?;
        self.begin_host_frame_inner(
            key,
            None,
            EguiHostFrameMode::CompleteRoster,
            EguiInputAuthority::FrameworkResponses,
            EguiOutputBoundary::OuterFinalOutput,
        )
        .map(|inner| EguiOuterHostFrame { inner })
    }

    /// Starts an owned native/backend session before any egui surface is painted.
    ///
    /// The returned affine capability does not borrow this facade and can be
    /// retained across native viewport callbacks. Every later operation that
    /// touches renderer or engine-owned state must present the session back to
    /// this exact `Dockspace` instance.
    ///
    /// # Errors
    ///
    /// Returns an error when another native session is active, the joined
    /// backend provider is missing, or the core cannot seal the host frame.
    #[cfg(any(feature = "backend", test))]
    pub fn begin_native_cycle(
        &mut self,
        key: EguiFrameScheduleKey,
        bindings: NativeBindingRoster,
    ) -> Result<EguiNativeInputSession, DockspaceError> {
        self.ensure_native_session_idle()?;
        let provider = self
            .engine
            .backend_ingress_provider()
            .ok_or(crate::error::DockspaceErrorSource::BackendIngressProviderMissing)?;
        if self.pointer_input.provider().is_some() {
            self.abort_pointer_input()?;
        }
        let workspace_epoch = self.engine.version().epoch();
        let registry = self
            .native_bindings
            .get_or_insert_with(|| NativeBindingRegistry::new(provider, workspace_epoch));
        let (routes, retirements) = bindings.into_parts();
        let candidate = registry
            .reconcile_candidate(provider, workspace_epoch, routes, retirements)
            .map_err(NativeBindingError::from)
            .map_err(DockspaceError::from_detail)?;
        let mut state = self.begin_host_frame_state_inner(
            key,
            None,
            EguiHostFrameMode::CompleteRoster,
            EguiInputAuthority::CoreBackend,
            EguiOutputBoundary::BackendPostInputFinalOutput,
        )?;
        state.stage_native_bindings(candidate);
        let lease = self.native_sessions.enroll();
        Ok(EguiNativeInputSession::new(lease, state))
    }

    #[cfg(any(feature = "backend", test))]
    fn ensure_outer_pointer_provider(&mut self) -> Result<(), DockspaceError> {
        let schedule = EguiEngineOwner::engine(&self.engine)
            .host_presentation_schedule()
            .map_err(DockspaceError::from_detail)?;
        if schedule.native_staging_presentations().len() != 0 {
            if self.pointer_input.provider().is_some() {
                self.abort_pointer_input()?;
            }
            return Ok(());
        }
        let mut surfaces = schedule.surfaces();
        let sole_surface = match surfaces.len() {
            1 => surfaces.next(),
            _ => None,
        };
        let Some(surface) = sole_surface else {
            if self.pointer_input.provider().is_some() {
                self.abort_pointer_input()?;
            }
            return Ok(());
        };
        let workspace_epoch = self.engine.version().epoch();
        if self
            .pointer_input
            .scope_changed_unbound(surface, workspace_epoch)
        {
            self.abort_pointer_input()?;
        }
        if self.pointer_input.provider().is_none() {
            let scope = SurfaceLocalPointerScope::new(
                self.presentation_host,
                SurfaceLocalPointerEndpoint::Logical(surface),
            );
            match EguiEngineOwner::validate_surface_local_pointer_provider_scope(
                &self.engine,
                scope,
            ) {
                Ok(()) => {}
                Err(EngineError::PointerProviderSurfaceAuthorityUnavailable { .. }) => {
                    return Ok(());
                }
                Err(source) => return Err(DockspaceError::from_detail(source)),
            }
            let reservation = self.pointer_input.reserve_install()?;
            let watermark = PointerEdgeSequence::new(0);
            let provider = EguiEngineOwner::create_surface_local_pointer_provider(
                &mut self.engine,
                scope,
                watermark,
            )
            .map_err(DockspaceError::from_detail)?;
            self.pointer_input
                .install_unbound(reservation, provider, surface, workspace_epoch);
        }
        Ok(())
    }

    fn begin_host_frame_inner(
        &mut self,
        key: EguiFrameScheduleKey,
        automatic_presentation: Option<AutomaticPresentationFrame>,
        mode: EguiHostFrameMode,
        input_authority: EguiInputAuthority,
        output_boundary: EguiOutputBoundary,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceError> {
        self.ensure_native_session_idle()?;
        let state = self.begin_host_frame_state_inner(
            key,
            automatic_presentation,
            mode,
            input_authority,
            output_boundary,
        )?;
        Ok(DockspaceHostFrame {
            dockspace: self,
            state: HostFrameStateSlot::new(state),
        })
    }

    fn begin_host_frame_state_inner(
        &mut self,
        key: EguiFrameScheduleKey,
        mut automatic_presentation: Option<AutomaticPresentationFrame>,
        mode: EguiHostFrameMode,
        input_authority: EguiInputAuthority,
        output_boundary: EguiOutputBoundary,
    ) -> Result<HostFrameState, DockspaceError> {
        if let Some(previous) = self.last_host_frame
            && key <= previous
        {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::HostFrameNotIncreasing {
                    previous_sequence: previous.sequence(),
                    previous_pass: previous.pass(),
                    submitted_sequence: key.sequence(),
                    submitted_pass: key.pass(),
                },
            ));
        }

        macro_rules! try_or_abort_pointer {
            ($result:expr) => {
                match $result {
                    Ok(value) => value,
                    Err(error) => {
                        self.abort_pointer_input()?;
                        return Err(DockspaceError::from_detail(error));
                    }
                }
            };
        }

        let mut prelude = try_or_abort_pointer!(EguiEngineOwner::begin_host_frame(
            &mut self.engine,
            self.presentation_host,
        ));
        let mut outer_presentation = Some(OuterPresentationFrame::default());
        let observation = match (
            input_authority,
            automatic_presentation.as_mut(),
            outer_presentation.as_mut(),
        ) {
            (EguiInputAuthority::CoreBackend, _, _) => HostPresentationObservation::NoUpdate,
            (EguiInputAuthority::FrameworkResponses, Some(automatic), Some(outer)) => {
                try_or_abort_pointer!(
                    self.automatic_presentation_observation(&prelude, automatic, outer)
                )
            }
            (EguiInputAuthority::FrameworkResponses, None, Some(outer)) => {
                try_or_abort_pointer!(self.outer_presentation_observation(&prelude, outer))
            }
            (EguiInputAuthority::FrameworkResponses, None, None)
            | (EguiInputAuthority::FrameworkResponses, Some(_), None) => {
                HostPresentationObservation::NoUpdate
            }
        };
        try_or_abort_pointer!(prelude.submit_presentation_observation(observation));
        let core_frame = try_or_abort_pointer!(prelude.seal(&self.engine));
        let automatic_pointer = if let Some(automatic) = automatic_presentation.as_ref() {
            if self.pointer_input.provider().is_some() {
                let epoch = try_or_abort_pointer!(
                    self.pointer_input
                        .current_epoch(
                            automatic.progress.cumulative_frame,
                            automatic.context.cumulative_pass_nr_for(automatic.viewport),
                            automatic.viewport,
                        )
                        .ok_or(crate::error::DockspaceErrorSource::PointerInputBindingMissing)
                );
                try_or_abort_pointer!(self.pointer_input.prepare(&automatic.context, epoch, None))
            } else {
                None
            }
        } else {
            None
        };
        #[cfg(test)]
        let output_boundary = if automatic_presentation
            .as_ref()
            .is_some_and(|automatic| automatic.test_presentation_provider.is_some())
        {
            EguiOutputBoundary::DeterministicTestFinalOutput
        } else {
            output_boundary
        };
        let state = HostFrameState::new(
            core_frame,
            key,
            self.pane_focus.clone(),
            self.semantic_source_sequence,
            automatic_presentation,
            outer_presentation,
            automatic_pointer,
            mode,
            input_authority,
            output_boundary,
        );
        Ok(state)
    }

    /// Paints and commits the sole surface supported by the crates.io egui facade.
    ///
    /// This convenience path records actual output emissions. Upstream egui
    /// does not expose a typed final-pass acknowledgement, so the facade reports
    /// that fact as unavailable and never infers interaction authority from a
    /// scene stamp, callback order, or a guessed final pass.
    pub fn show_single_surface(
        &mut self,
        surface: SurfaceId,
        ui: &mut Ui,
        panes: &mut dyn PaneView,
    ) -> Result<DockspaceResponse, DockspaceError> {
        self.ensure_native_session_idle()?;
        if self.pointer_input.provider().is_some() {
            self.abort_pointer_input()?;
        }

        let pass = u32::try_from(ui.ctx().current_pass_index())
            .map_err(|_| crate::error::DockspaceErrorSource::AutomaticHostFrameSequenceExhausted)?;
        let automatic_presentation = self.automatic_presentation_frame(
            ui.ctx(),
            ui.ctx().viewport_id(),
            ui.ctx().cumulative_frame_nr(),
            pass,
        )?;
        let key = automatic_presentation.progress.host_frame;
        let mut host = self.begin_host_frame_inner(
            key,
            Some(automatic_presentation),
            EguiHostFrameMode::SingleSurface,
            EguiInputAuthority::FrameworkResponses,
            EguiOutputBoundary::UnobservableCallback,
        )?;
        let (surface_count, scheduled_surface) = {
            let mut scheduled_surfaces = host.expected_surfaces();
            (scheduled_surfaces.len(), scheduled_surfaces.next())
        };
        if surface_count != 1 {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::SingleSurfaceHostFrameRequiresOneSurface {
                    surface_count,
                },
            ));
        }
        if scheduled_surface != Some(surface) {
            return Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::HostFrameSurfaceOutsideRoster { surface },
            ));
        }
        let _ = host.show_surface(surface, ui, panes)?;
        let HostFrameResponse {
            mutation,
            mut surfaces,
            close_requests,
            ..
        } = host.end_host_frame()?;
        let surface_response = surfaces
            .remove(&surface)
            .ok_or(crate::error::DockspaceErrorSource::SingleSurfacePaintUnavailable { surface })?;
        let surface_commit_status = surface_response.status();
        let SurfaceCommitResponse { paint, status: _ } = surface_response;
        let paint = paint
            .ok_or(crate::error::DockspaceErrorSource::SingleSurfacePaintUnavailable { surface })?;
        Ok(DockspaceResponse {
            mutation,
            paint,
            surface_commit_status,
            close_requests,
        })
    }

    fn automatic_presentation_frame(
        &self,
        context: &Context,
        viewport: ViewportId,
        cumulative_frame: u64,
        pass: u32,
    ) -> Result<AutomaticPresentationFrame, DockspaceError> {
        self.presentation_ledger
            .prepare_automatic_frame(
                self.last_host_frame,
                context,
                viewport,
                cumulative_frame,
                pass,
            )
            .map_err(Into::into)
    }

    fn automatic_presentation_observation(
        &self,
        prelude: &CoreHostFramePrelude,
        automatic: &mut AutomaticPresentationFrame,
        outer: &mut OuterPresentationFrame,
    ) -> Result<HostPresentationObservation, crate::error::DockspaceErrorSource> {
        self.presentation_ledger
            .automatic_observation(prelude, automatic, outer)
    }

    fn outer_presentation_observation(
        &self,
        prelude: &CoreHostFramePrelude,
        outer: &mut OuterPresentationFrame,
    ) -> Result<HostPresentationObservation, crate::error::DockspaceErrorSource> {
        self.presentation_ledger.outer_observation(prelude, outer)
    }

    fn commit_outer_presentation_observation(
        &mut self,
        outer: OuterPresentationFrame,
        transition: &EngineTransition,
    ) {
        let accepted = AcceptedPresentationDelta::from_transition(transition);
        self.presentation_ledger.commit_outer(outer, &accepted);
    }

    fn commit_ordered_outer_presentation_observations(&mut self, transition: &EngineTransition) {
        let accepted = AcceptedPresentationDelta::from_transition(transition);
        self.presentation_ledger.commit_ordered_outer(&accepted);
    }

    fn register_outer_presentation(
        &mut self,
        outputs: Vec<HostPresentationOutput>,
    ) -> Vec<PendingEguiPresentation> {
        self.presentation_ledger.register_outer(outputs)
    }

    fn outer_surface_has_pending_presentation(&self, surface: SurfaceId) -> bool {
        self.presentation_ledger.outer_surface_has_pending(surface)
    }

    fn commit_automatic_presentation(
        &mut self,
        automatic: AutomaticPresentationFrame,
        transition: &EngineTransition,
        emissions: impl IntoIterator<Item = HostPresentationOutput>,
    ) {
        let accepted = AcceptedPresentationDelta::from_transition(transition);
        self.presentation_ledger
            .commit_automatic(automatic, &accepted, emissions);
    }
    fn binding_has_authoritative_global_focus(
        view: HostFrameView<'_>,
        binding: ViewportBinding,
    ) -> bool {
        view.viewport_focus()
            .focus_observation()
            .is_some_and(|observation| {
                matches!(
                    observation.focused(),
                    Authority::Known(GlobalFocusedWindow::Dock(focused)) if *focused == binding
                )
            })
    }

    fn is_gesture_source_surface(view: HostFrameView<'_>, surface: SurfaceId) -> bool {
        match view.interaction().status() {
            InteractionStatus::Idle => false,
            InteractionStatus::Pressed { .. } => view
                .interaction()
                .active_click_view()
                .is_some_and(|click| click.surface() == surface),
            InteractionStatus::Armed { .. } | InteractionStatus::Dragging { .. } => {
                view.interaction()
                    .active_drag_view()
                    .and_then(|drag| payload_surface(view.workspace(), drag.payload()))
                    == Some(surface)
            }
            InteractionStatus::Resizing { .. } => view
                .interaction()
                .active_resize_view()
                .is_some_and(|resize| resize.surface() == surface),
            InteractionStatus::ContainedTransforming { .. } => view
                .interaction()
                .active_contained_transform_view()
                .is_some_and(|transform| transform.surface() == surface),
        }
    }

    fn abort_pointer_input(&mut self) -> Result<(), DockspaceError> {
        let drained = match self.pointer_input.drain() {
            Ok(drained) => drained,
            Err(error) => {
                self.pending_pointer_abort = true;
                self.pointer_input.request_bound_repaint();
                return Err(error.into());
            }
        };
        let Some(mut drained) = drained else {
            self.pending_pointer_abort = false;
            return Ok(());
        };
        // A host-frame capability that never reaches `finish` cannot publish the
        // staged journal watermark. Retire that exact lease so the next egui
        // epoch can enroll a fresh provider instead of inheriting a poisoned
        // pending journal or a cancelled stream owner.
        let outcome = match EguiEngineOwner::retire_quiesced_surface_local_pointer_provider(
            &mut self.engine,
            drained.receipt_mut(),
        ) {
            Ok(outcome) => outcome,
            Err(error) => {
                self.pointer_input.restore_drained(drained);
                self.pending_pointer_abort = true;
                self.pointer_input.request_bound_repaint();
                return Err(DockspaceError::from_detail(error));
            }
        };
        if outcome.repaint_required() {
            drained.request_bound_repaint();
        }
        self.pending_pointer_abort = false;
        Ok(())
    }

    pub(crate) fn ensure_native_session_idle(&mut self) -> Result<(), DockspaceError> {
        if self.pending_pointer_abort {
            self.abort_pointer_input()?;
        }
        match self.native_sessions.reap_abandoned() {
            native_session::NativeSessionStatus::Idle => Ok(()),
            native_session::NativeSessionStatus::Abandoned => self.abort_pointer_input(),
            native_session::NativeSessionStatus::Active => Err(DockspaceError::from_source(
                crate::error::DockspaceErrorSource::NativeSessionAlreadyActive,
            )),
        }
    }

    fn validate_native_session(
        &self,
        lease: &native_session::NativeSessionLease,
    ) -> Result<(), DockspaceError> {
        self.native_sessions
            .validate(lease)
            .then_some(())
            .ok_or(crate::error::DockspaceErrorSource::NativeSessionLeaseMismatch)
            .map_err(Into::into)
    }

    fn complete_native_session(&mut self, lease: &native_session::NativeSessionLease) {
        debug_assert!(self.native_sessions.validate(lease));
        self.native_sessions.complete(lease);
    }
}

use self::contained_resize::requested_contained_resize_rect;
use self::pane_focus::{PaneFocusRequestFence, observe_surface_pane_focus};

#[cfg(test)]
#[path = "dockspace/tests.rs"]
mod tests;
