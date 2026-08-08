//! Engine ownership adapters for raw and document-bound dockspace sessions.

use dockspace::backend::engine::{
    CoreHostFramePrelude, CoreHostPresentationFrame, DockEngine, EngineError,
    OwnedPreparedHostFrameCommit, PreparedHostFrameCommit,
};
use dockspace::backend::ingress::{
    BackendIngressDrainReceipt, BackendIngressPrefixRetirementReceipt,
    BackendIngressProviderReplacementTicket, BackendIngressRecorder,
};
use dockspace::backend::pointer_journal::{
    PointerEdgeSequence, SurfaceLocalPointerDrainReceipt, SurfaceLocalPointerProvider,
    SurfaceLocalPointerRetirementOutcome, SurfaceLocalPointerScope,
};
#[cfg(test)]
use dockspace::backend::presentation_observation::PresentationHostRetirementReason;
use dockspace::backend::presentation_observation::{
    HostPresentationStreamId, PresentationHostLease, PresentationStreamQuiescence,
};
#[cfg(test)]
use dockspace::backend::transition::PresentationHostRetirementOutcome;
use dockspace::backend::transition::{BackendIngressProviderReplacementStart, EngineTransition};
#[cfg(feature = "serde")]
use dockspace::document::{
    DockspaceDocumentSession, DockspaceDocumentSessionError, PreparedDockspaceSessionHostCommit,
};
use dockspace::viewport::ViewportBinding;

use crate::DockspaceError;

#[cfg(feature = "serde")]
pub(super) type EguiDockEngine = DockspaceDocumentSession;
#[cfg(not(feature = "serde"))]
pub(super) type EguiDockEngine = DockEngine;

pub(super) type EguiPreparedHostFrameCommit =
    <EguiDockEngine as EguiEngineOwner>::PreparedOwnedHostCommit;

pub(super) fn prepared_host_transition(
    prepared: &EguiPreparedHostFrameCommit,
) -> &EngineTransition {
    <EguiDockEngine as EguiEngineOwner>::prepared_host_transition(prepared)
}

#[cfg(feature = "serde")]
fn map_document_session_error(error: DockspaceDocumentSessionError) -> DockspaceError {
    match error {
        DockspaceDocumentSessionError::Engine(source) => DockspaceError::Engine(*source),
        error => DockspaceError::DocumentSession(error),
    }
}

pub(super) trait EguiEngineOwner {
    type PreparedOwnedHostCommit;

    fn engine(&self) -> &DockEngine;

    fn reconcile_document_sidecars(&mut self);

    #[cfg(test)]
    fn create_presentation_host(&mut self) -> Result<PresentationHostLease, EngineError>;

    #[cfg(test)]
    fn retire_presentation_host_for_test(
        &mut self,
        host: PresentationHostLease,
        reason: PresentationHostRetirementReason,
    ) -> Result<PresentationHostRetirementOutcome, EngineError>;

    fn create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError>;

    fn settle_backend_ingress_prefix_retirement(
        &mut self,
        receipt: &mut BackendIngressPrefixRetirementReceipt,
    ) -> Result<Vec<ViewportBinding>, EngineError>;

    fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: &mut BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError>;

    fn reissue_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<BackendIngressProviderReplacementTicket, EngineError>;

    fn abort_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<EngineTransition, EngineError>;

    fn reap_abandoned_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<Option<EngineTransition>, EngineError>;

    fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError>;

    fn create_surface_local_pointer_provider(
        &mut self,
        scope: SurfaceLocalPointerScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<SurfaceLocalPointerProvider, EngineError>;

    fn validate_surface_local_pointer_provider_scope(
        &self,
        scope: SurfaceLocalPointerScope,
    ) -> Result<(), EngineError>;

    fn retire_quiesced_surface_local_pointer_provider(
        &mut self,
        receipt: &mut SurfaceLocalPointerDrainReceipt,
    ) -> Result<SurfaceLocalPointerRetirementOutcome, EngineError>;

    fn begin_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError>;

    fn prepare_host_presentation_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError>;

    fn prepare_owned_host_presentation_frame(
        &self,
        frame: CoreHostPresentationFrame,
    ) -> Result<Self::PreparedOwnedHostCommit, DockspaceError>;

    fn prepared_host_transition(prepared: &Self::PreparedOwnedHostCommit) -> &EngineTransition;

    fn commit_owned_host_presentation_frame(
        &mut self,
        prepared: Self::PreparedOwnedHostCommit,
    ) -> Result<EngineTransition, DockspaceError>;

    fn try_prepare_presentation_stream_quiescence(
        &self,
        presentation_host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<Option<PresentationStreamQuiescence>, EngineError>;

    fn confirm_presentation_stream_quiescence_batch(
        &mut self,
        quiescences: Vec<PresentationStreamQuiescence>,
    ) -> Result<(), EngineError>;
}

pub(super) trait EguiApplicationInputOwner {
    fn application_engine(&self) -> &DockEngine;

    fn reconcile_application_sidecars(&mut self);

    fn begin_application_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError>;

    fn prepare_application_host_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError>;
}

impl<T> EguiApplicationInputOwner for T
where
    T: EguiEngineOwner,
{
    fn application_engine(&self) -> &DockEngine {
        EguiEngineOwner::engine(self)
    }

    fn reconcile_application_sidecars(&mut self) {
        EguiEngineOwner::reconcile_document_sidecars(self);
    }

    fn begin_application_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        EguiEngineOwner::begin_host_frame(self, presentation_host)
    }

    fn prepare_application_host_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError> {
        EguiEngineOwner::prepare_host_presentation_frame(self, frame)
    }
}

impl EguiEngineOwner for DockEngine {
    type PreparedOwnedHostCommit = OwnedPreparedHostFrameCommit;

    fn engine(&self) -> &DockEngine {
        self
    }

    fn reconcile_document_sidecars(&mut self) {}

    #[cfg(test)]
    fn create_presentation_host(&mut self) -> Result<PresentationHostLease, EngineError> {
        DockEngine::create_presentation_host(self)
    }

    #[cfg(test)]
    fn retire_presentation_host_for_test(
        &mut self,
        host: PresentationHostLease,
        reason: PresentationHostRetirementReason,
    ) -> Result<PresentationHostRetirementOutcome, EngineError> {
        DockEngine::retire_presentation_host(self, host, reason)
    }

    fn create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError> {
        DockEngine::create_backend_ingress_provider(
            self,
            presentation_host,
            pointer_committed_through,
        )
    }

    fn settle_backend_ingress_prefix_retirement(
        &mut self,
        receipt: &mut BackendIngressPrefixRetirementReceipt,
    ) -> Result<Vec<ViewportBinding>, EngineError> {
        DockEngine::settle_backend_ingress_prefix_retirement(self, receipt)
    }

    fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: &mut BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError> {
        DockEngine::begin_backend_ingress_provider_replacement(self, drained)
    }

    fn reissue_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<BackendIngressProviderReplacementTicket, EngineError> {
        DockEngine::reissue_backend_ingress_provider_replacement(self)
    }

    fn abort_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<EngineTransition, EngineError> {
        DockEngine::abort_backend_ingress_provider_replacement(self)
    }

    fn reap_abandoned_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<Option<EngineTransition>, EngineError> {
        DockEngine::reap_abandoned_backend_ingress_provider_replacement(self)
    }

    fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError> {
        DockEngine::finish_backend_ingress_provider_replacement(self, ticket, presentation_host)
    }

    fn create_surface_local_pointer_provider(
        &mut self,
        scope: SurfaceLocalPointerScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<SurfaceLocalPointerProvider, EngineError> {
        DockEngine::create_surface_local_pointer_provider(self, scope, committed_through)
    }

    fn validate_surface_local_pointer_provider_scope(
        &self,
        scope: SurfaceLocalPointerScope,
    ) -> Result<(), EngineError> {
        DockEngine::validate_surface_local_pointer_provider_scope(self, scope)
    }

    fn retire_quiesced_surface_local_pointer_provider(
        &mut self,
        receipt: &mut SurfaceLocalPointerDrainReceipt,
    ) -> Result<SurfaceLocalPointerRetirementOutcome, EngineError> {
        DockEngine::retire_quiesced_surface_local_pointer_provider(self, receipt)
    }

    fn begin_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        DockEngine::begin_host_frame(self, presentation_host)
    }

    fn prepare_host_presentation_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError> {
        frame.prepare(self)
    }

    fn prepare_owned_host_presentation_frame(
        &self,
        frame: CoreHostPresentationFrame,
    ) -> Result<Self::PreparedOwnedHostCommit, DockspaceError> {
        frame.prepare_owned(self).map_err(Into::into)
    }

    fn prepared_host_transition(prepared: &Self::PreparedOwnedHostCommit) -> &EngineTransition {
        prepared.transition()
    }

    fn commit_owned_host_presentation_frame(
        &mut self,
        prepared: Self::PreparedOwnedHostCommit,
    ) -> Result<EngineTransition, DockspaceError> {
        prepared.commit(self).map_err(Into::into)
    }

    fn try_prepare_presentation_stream_quiescence(
        &self,
        presentation_host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<Option<PresentationStreamQuiescence>, EngineError> {
        DockEngine::try_prepare_presentation_stream_quiescence(self, presentation_host, stream)
    }

    fn confirm_presentation_stream_quiescence_batch(
        &mut self,
        quiescences: Vec<PresentationStreamQuiescence>,
    ) -> Result<(), EngineError> {
        DockEngine::confirm_presentation_stream_quiescence_batch(self, quiescences)
    }
}

#[cfg(feature = "serde")]
impl EguiEngineOwner for DockspaceDocumentSession {
    type PreparedOwnedHostCommit = PreparedDockspaceSessionHostCommit;

    fn engine(&self) -> &DockEngine {
        self.engine()
    }

    fn reconcile_document_sidecars(&mut self) {
        self.adapter_reconcile_viewport_placements();
    }

    #[cfg(test)]
    fn create_presentation_host(&mut self) -> Result<PresentationHostLease, EngineError> {
        self.adapter_create_presentation_host()
    }

    #[cfg(test)]
    fn retire_presentation_host_for_test(
        &mut self,
        host: PresentationHostLease,
        reason: PresentationHostRetirementReason,
    ) -> Result<PresentationHostRetirementOutcome, EngineError> {
        self.adapter_retire_presentation_host(host, reason)
    }

    fn create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError> {
        self.adapter_create_backend_ingress_provider(presentation_host, pointer_committed_through)
    }

    fn settle_backend_ingress_prefix_retirement(
        &mut self,
        receipt: &mut BackendIngressPrefixRetirementReceipt,
    ) -> Result<Vec<ViewportBinding>, EngineError> {
        self.adapter_settle_backend_ingress_prefix_retirement(receipt)
    }

    fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: &mut BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError> {
        self.adapter_begin_backend_ingress_provider_replacement(drained)
    }

    fn reissue_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<BackendIngressProviderReplacementTicket, EngineError> {
        self.adapter_reissue_backend_ingress_provider_replacement()
    }

    fn abort_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<EngineTransition, EngineError> {
        self.adapter_abort_backend_ingress_provider_replacement()
    }

    fn reap_abandoned_backend_ingress_provider_replacement(
        &mut self,
    ) -> Result<Option<EngineTransition>, EngineError> {
        self.adapter_reap_abandoned_backend_ingress_provider_replacement()
    }

    fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError> {
        self.adapter_finish_backend_ingress_provider_replacement(ticket, presentation_host)
    }

    fn create_surface_local_pointer_provider(
        &mut self,
        scope: SurfaceLocalPointerScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<SurfaceLocalPointerProvider, EngineError> {
        self.adapter_create_surface_local_pointer_provider(scope, committed_through)
    }

    fn validate_surface_local_pointer_provider_scope(
        &self,
        scope: SurfaceLocalPointerScope,
    ) -> Result<(), EngineError> {
        self.adapter_validate_surface_local_pointer_provider_scope(scope)
    }

    fn retire_quiesced_surface_local_pointer_provider(
        &mut self,
        receipt: &mut SurfaceLocalPointerDrainReceipt,
    ) -> Result<SurfaceLocalPointerRetirementOutcome, EngineError> {
        self.adapter_retire_quiesced_surface_local_pointer_provider(receipt)
    }

    fn begin_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        self.adapter_begin_host_frame(presentation_host)
    }

    fn prepare_host_presentation_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError> {
        self.adapter_prepare_host_presentation_frame(frame)
    }

    fn prepare_owned_host_presentation_frame(
        &self,
        frame: CoreHostPresentationFrame,
    ) -> Result<Self::PreparedOwnedHostCommit, DockspaceError> {
        self.adapter_prepare_owned_host_presentation_frame(frame)
            .map_err(map_document_session_error)
    }

    fn prepared_host_transition(prepared: &Self::PreparedOwnedHostCommit) -> &EngineTransition {
        prepared.transition()
    }

    fn commit_owned_host_presentation_frame(
        &mut self,
        prepared: Self::PreparedOwnedHostCommit,
    ) -> Result<EngineTransition, DockspaceError> {
        self.adapter_commit_owned_host_presentation_frame(prepared)
            .map_err(map_document_session_error)
    }

    fn try_prepare_presentation_stream_quiescence(
        &self,
        presentation_host: PresentationHostLease,
        stream: HostPresentationStreamId,
    ) -> Result<Option<PresentationStreamQuiescence>, EngineError> {
        self.adapter_try_prepare_presentation_stream_quiescence(presentation_host, stream)
    }

    fn confirm_presentation_stream_quiescence_batch(
        &mut self,
        quiescences: Vec<PresentationStreamQuiescence>,
    ) -> Result<(), EngineError> {
        self.adapter_confirm_presentation_stream_quiescence_batch(quiescences)
    }
}
