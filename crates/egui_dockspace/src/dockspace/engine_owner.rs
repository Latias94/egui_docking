//! Engine ownership adapters for raw and document-bound dockspace sessions.

use dockspace::backend_ingress::{
    BackendIngressDrainReceipt, BackendIngressProviderReplacementTicket, BackendIngressRecorder,
};
#[cfg(feature = "serde")]
use dockspace::document::{DockspaceDocumentRestore, DockspaceDocumentSession};
use dockspace::engine::{
    CoreHostFramePrelude, CoreHostPresentationFrame, DockEngine, EngineError,
    OwnedPreparedHostFrameCommit, PreparedHostFrameCommit,
};
use dockspace::pointer_journal::{PointerEdgeSequence, PointerInputLease, PointerProviderScope};
use dockspace::presentation_observation::{
    HostPresentationStreamId, PresentationHostLease, PresentationStreamQuiescence,
};
use dockspace::transition::{BackendIngressProviderReplacementStart, EngineTransition};

#[cfg(feature = "serde")]
pub(super) type EguiDockEngine = DockspaceDocumentSession;
#[cfg(not(feature = "serde"))]
pub(super) type EguiDockEngine = DockEngine;

pub(super) trait EguiEngineOwner {
    fn engine(&self) -> &DockEngine;

    fn reconcile_document_sidecars(&mut self);

    #[cfg(test)]
    fn create_presentation_host(&mut self) -> Result<PresentationHostLease, EngineError>;

    fn create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError>;

    fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError>;

    fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError>;

    fn create_pointer_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, EngineError>;

    fn retire_pointer_provider(
        &mut self,
        provider: PointerInputLease,
    ) -> Result<EngineTransition, EngineError>;

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
    ) -> Result<OwnedPreparedHostFrameCommit, EngineError>;

    fn commit_owned_host_presentation_frame(
        &mut self,
        prepared: OwnedPreparedHostFrameCommit,
    ) -> Result<EngineTransition, EngineError>;

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

#[cfg(feature = "serde")]
impl EguiApplicationInputOwner for DockspaceDocumentRestore<'_> {
    fn application_engine(&self) -> &DockEngine {
        self.engine()
    }

    fn reconcile_application_sidecars(&mut self) {}

    fn begin_application_host_frame(
        &mut self,
        presentation_host: PresentationHostLease,
    ) -> Result<CoreHostFramePrelude, EngineError> {
        self.adapter_begin_host_frame(presentation_host)
    }

    fn prepare_application_host_frame(
        &mut self,
        frame: CoreHostPresentationFrame,
    ) -> Result<PreparedHostFrameCommit<'_>, EngineError> {
        self.adapter_prepare_host_presentation_frame(frame)
    }
}

impl EguiEngineOwner for DockEngine {
    fn engine(&self) -> &DockEngine {
        self
    }

    fn reconcile_document_sidecars(&mut self) {}

    #[cfg(test)]
    fn create_presentation_host(&mut self) -> Result<PresentationHostLease, EngineError> {
        DockEngine::create_presentation_host(self)
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

    fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError> {
        DockEngine::begin_backend_ingress_provider_replacement(self, drained)
    }

    fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError> {
        DockEngine::finish_backend_ingress_provider_replacement(self, ticket, presentation_host)
    }

    fn create_pointer_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, EngineError> {
        DockEngine::create_pointer_provider(self, scope, committed_through)
    }

    fn retire_pointer_provider(
        &mut self,
        provider: PointerInputLease,
    ) -> Result<EngineTransition, EngineError> {
        DockEngine::retire_pointer_provider(self, provider)
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
    ) -> Result<OwnedPreparedHostFrameCommit, EngineError> {
        frame.prepare_owned(self)
    }

    fn commit_owned_host_presentation_frame(
        &mut self,
        prepared: OwnedPreparedHostFrameCommit,
    ) -> Result<EngineTransition, EngineError> {
        prepared.commit(self)
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

    fn create_backend_ingress_provider(
        &mut self,
        presentation_host: PresentationHostLease,
        pointer_committed_through: PointerEdgeSequence,
    ) -> Result<BackendIngressRecorder, EngineError> {
        self.adapter_create_backend_ingress_provider(presentation_host, pointer_committed_through)
    }

    fn begin_backend_ingress_provider_replacement(
        &mut self,
        drained: BackendIngressDrainReceipt,
    ) -> Result<BackendIngressProviderReplacementStart, EngineError> {
        self.adapter_begin_backend_ingress_provider_replacement(drained)
    }

    fn finish_backend_ingress_provider_replacement(
        &mut self,
        ticket: &mut BackendIngressProviderReplacementTicket,
        presentation_host: PresentationHostLease,
    ) -> Result<BackendIngressRecorder, EngineError> {
        self.adapter_finish_backend_ingress_provider_replacement(ticket, presentation_host)
    }

    fn create_pointer_provider(
        &mut self,
        scope: PointerProviderScope,
        committed_through: PointerEdgeSequence,
    ) -> Result<PointerInputLease, EngineError> {
        self.adapter_create_pointer_provider(scope, committed_through)
    }

    fn retire_pointer_provider(
        &mut self,
        provider: PointerInputLease,
    ) -> Result<EngineTransition, EngineError> {
        self.adapter_retire_pointer_provider(provider)
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
    ) -> Result<OwnedPreparedHostFrameCommit, EngineError> {
        self.adapter_prepare_owned_host_presentation_frame(frame)
    }

    fn commit_owned_host_presentation_frame(
        &mut self,
        prepared: OwnedPreparedHostFrameCommit,
    ) -> Result<EngineTransition, EngineError> {
        self.adapter_commit_owned_host_presentation_frame(prepared)
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
