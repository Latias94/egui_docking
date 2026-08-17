//! Session ownership and host-frame preparation.

use std::collections::BTreeSet;

use crate::command::ContentCloseTarget;
#[cfg(feature = "serde")]
use crate::document::PreparedRuntimeDocumentRestore;
use crate::engine::{DockEngine, EngineError};
#[cfg(test)]
use crate::graph::Workspace;
use crate::model::{DockPlacement, DockspaceLayout, DockspaceView, WorkspaceVersion};
use crate::presentation_observation::PresentationHostLease;

#[cfg(feature = "serde")]
use super::DockspacePersistenceError;
use super::close::PreparedCloseRequest;
use super::host_frame::DockspaceHostFrame;
use super::{
    DockPresentationConfig, DockspaceRuntimeError, NativePlatformError, NativeReceiverAnswer,
    NativeReceiverQuery, NativeWindowPlacement, PreparedDockAction,
};
use super::{interaction, native, native_effect, presentation};

/// Renderer-neutral owner of one docking workspace and one application host.
///
/// The engine and presentation-host lease remain private. A caller can inspect
/// the published workspace and mutate it only through an affine host frame.
#[derive(Debug)]
pub struct DockspaceSession {
    pub(super) engine: DockEngine,
    pub(super) presentation_host: PresentationHostLease,
    pub(super) presentation: presentation::RuntimePresentationState,
    pub(super) pointer: Option<interaction::RuntimePointerState>,
    pub(super) native: Option<native::RuntimeNativeState>,
    #[cfg(feature = "serde")]
    pub(super) document: Option<crate::document::BoundDocumentState>,
    pub(super) abandoned_native_effects: native_effect::NativeEffectDropQueue,
    pub(super) committed_source_sequence: u64,
}

impl DockspaceSession {
    /// Creates one product session from stable item, root, and surface layout data.
    ///
    /// Runtime node identities are allocated privately while compiling the layout.
    ///
    /// # Errors
    ///
    /// Returns an error when the core cannot mint its private presentation-host identity.
    pub fn from_layout(
        layout: DockspaceLayout,
        policy: crate::policy::DockPolicy,
    ) -> Result<Self, DockspaceRuntimeError> {
        Self::from_workspace_with_presentation_config(
            layout.into_workspace(),
            policy,
            DockPresentationConfig::default(),
        )
    }

    /// Creates one product session with explicit renderer-neutral geometry.
    ///
    /// Visual-only styling remains adapter-owned. This configuration controls
    /// core layout and interaction geometry such as tab bars, splitters, guides,
    /// contained chrome, and drag thresholds.
    ///
    /// # Errors
    ///
    /// Returns an error when the layout is invalid or the core cannot mint its
    /// private presentation-host identity.
    pub fn from_layout_with_presentation_config(
        layout: DockspaceLayout,
        policy: crate::policy::DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, DockspaceRuntimeError> {
        Self::from_workspace_with_presentation_config(
            layout.into_workspace(),
            policy,
            presentation_config,
        )
    }

    /// Creates one test session from a strictly validated runtime workspace and policy.
    ///
    /// # Errors
    ///
    /// Returns an error when the workspace is invalid or the core cannot mint
    /// the private presentation-host identity.
    #[cfg(test)]
    pub(crate) fn from_workspace_for_test(
        workspace: Workspace,
        policy: crate::policy::DockPolicy,
    ) -> Result<Self, DockspaceRuntimeError> {
        Self::from_workspace_with_presentation_config(
            workspace,
            policy,
            DockPresentationConfig::default(),
        )
    }

    fn from_workspace_with_presentation_config(
        workspace: crate::graph::Workspace,
        policy: crate::policy::DockPolicy,
        presentation_config: DockPresentationConfig,
    ) -> Result<Self, DockspaceRuntimeError> {
        let engine =
            DockEngine::new_with_presentation_config(workspace, policy, presentation_config)?;
        Ok(Self::from_engine(engine)?)
    }

    pub(super) fn from_engine(mut engine: DockEngine) -> Result<Self, EngineError> {
        let presentation_host = engine.create_presentation_host()?;
        Ok(Self::from_engine_with_presentation_host(
            engine,
            presentation_host,
        ))
    }

    fn from_engine_with_presentation_host(
        engine: DockEngine,
        presentation_host: PresentationHostLease,
    ) -> Self {
        Self {
            engine,
            presentation_host,
            presentation: presentation::RuntimePresentationState::default(),
            pointer: None,
            native: None,
            #[cfg(feature = "serde")]
            document: None,
            abandoned_native_effects: native_effect::NativeEffectDropQueue::default(),
            committed_source_sequence: 0,
        }
    }

    #[cfg(test)]
    pub(crate) fn from_engine_for_test(
        engine: DockEngine,
        presentation_host: PresentationHostLease,
    ) -> Self {
        Self::from_engine_with_presentation_host(engine, presentation_host)
    }

    /// Returns the currently published, strictly validated workspace.
    #[cfg(test)]
    #[doc(hidden)]
    #[must_use]
    pub(crate) const fn workspace(&self) -> &Workspace {
        self.engine.workspace()
    }

    /// Returns the published item/surface-centric product view.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        self.engine.product_view()
    }

    /// Returns the current durable workspace version.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.engine.version()
    }

    /// Returns the current declarative docking policy.
    #[must_use]
    pub const fn policy(&self) -> &crate::policy::DockPolicy {
        self.engine.policy()
    }

    /// Returns the renderer-neutral geometry and interaction configuration.
    #[must_use]
    pub const fn presentation_config(&self) -> &DockPresentationConfig {
        self.engine.presentation_config()
    }

    /// Prepares one revision-bound selection action from the published workspace.
    pub const fn prepare_select_item(&self, item: crate::ids::ItemId) -> PreparedDockAction {
        self.engine.prepare_select_item(item)
    }

    /// Prepares one revision-bound open action from the published workspace.
    pub const fn prepare_open_item(
        &self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_open_item(item, placement)
    }

    /// Prepares one revision-bound move action from the published workspace.
    pub const fn prepare_dock_item(
        &self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_dock_item(item, placement)
    }

    /// Prepares one complete-root move against the exact published workspace version.
    pub const fn prepare_dock_root(
        &self,
        root: crate::ids::RootId,
        placement: DockPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_dock_root(root, placement)
    }

    /// Prepares one complete-root native tear-off against the current published version.
    #[must_use]
    pub const fn prepare_tear_off_root(
        &self,
        root: crate::ids::RootId,
        placement: NativeWindowPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_tear_off_root(root, placement)
    }

    /// Prepares one revision-bound contained-floating action.
    pub const fn prepare_float_item(
        &self,
        item: crate::ids::ItemId,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
    ) -> PreparedDockAction {
        self.engine.prepare_float_item(item, surface, rect)
    }

    /// Prepares one revision-bound contained-bounds update addressed by item identity.
    pub const fn prepare_set_contained_rect(
        &self,
        item: crate::ids::ItemId,
        rect: crate::geometry::LogicalRect,
    ) -> PreparedDockAction {
        self.engine.prepare_set_contained_rect(item, rect)
    }

    /// Prepares one revision-bound contained raise addressed by item identity.
    pub const fn prepare_raise_contained(&self, item: crate::ids::ItemId) -> PreparedDockAction {
        self.engine.prepare_raise_contained(item)
    }

    /// Prepares one revision-bound contained bring-into-view action.
    pub const fn prepare_bring_contained_into_view(
        &self,
        item: crate::ids::ItemId,
    ) -> PreparedDockAction {
        self.engine.prepare_bring_contained_into_view(item)
    }

    /// Prepares one revision-bound item close request.
    pub const fn prepare_close_item(&self, item: crate::ids::ItemId) -> PreparedCloseRequest {
        PreparedCloseRequest::new(
            self.engine.authority_domain(),
            self.engine.version(),
            ContentCloseTarget::Item(item),
        )
    }

    /// Prepares one revision-bound complete-root close request.
    pub const fn prepare_close_root(&self, root: crate::ids::RootId) -> PreparedCloseRequest {
        PreparedCloseRequest::new(
            self.engine.authority_domain(),
            self.engine.version(),
            ContentCloseTarget::Root(root),
        )
    }

    /// Begins one affine application host frame.
    ///
    /// Pending output observations are supplied from the facade-owned
    /// presentation sidecar before input authority is frozen.
    ///
    /// # Errors
    ///
    /// Returns an error when presentation output is awaiting an explicit host
    /// observation or the core rejects the frame prelude.
    pub fn begin_host_frame(&mut self) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.begin_host_frame_with_native_resolver(None)
    }

    /// Begins one native host frame and synchronously resolves every frozen
    /// receiver question through product-level descriptors.
    ///
    /// The callback never sees pointer leases, provider sequences, backend
    /// ordinals, candidate identities, or receipts. A failed frame retains the
    /// original native edge for an exact retry.
    ///
    /// # Errors
    ///
    /// Returns an error when no managed native host is active, presentation
    /// authority is unavailable, or the core rejects the ordered input prefix.
    pub fn begin_native_host_frame(
        &mut self,
        mut resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        self.begin_host_frame_with_native_resolver(Some(&mut resolve))
    }

    fn require_native_host(&self) -> Result<&native::RuntimeNativeState, DockspaceRuntimeError> {
        self.native
            .as_ref()
            .ok_or_else(|| NativePlatformError::ProviderUnavailable.into())
    }

    pub(super) fn ensure_managed_native_host_available(&self) -> Result<(), DockspaceRuntimeError> {
        let native = self.require_native_host()?;
        if native.profile != native::NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch.into());
        }
        Ok(())
    }

    fn begin_host_frame_with_native_resolver(
        &mut self,
        resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.begin_host_frame_with_options(resolver, None)
    }

    #[cfg(feature = "serde")]
    pub(super) fn begin_host_frame_with_document_restore(
        &mut self,
        restore: PreparedRuntimeDocumentRestore,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.begin_host_frame_with_options(None, Some(restore))
    }

    #[cfg(feature = "serde")]
    pub(super) fn ensure_standalone_document_restore_available(
        &self,
    ) -> Result<(), DockspaceRuntimeError> {
        if self
            .native
            .as_ref()
            .is_some_and(|native| native.profile == native::NativeHostProfile::ManagedDesktop)
        {
            return Err(NativePlatformError::DocumentRestoreRequiresNativeFrame.into());
        }
        Ok(())
    }

    pub(super) fn begin_host_frame_with_options(
        &mut self,
        mut resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
        #[cfg(feature = "serde")] mut document_restore: Option<PreparedRuntimeDocumentRestore>,
        #[cfg(not(feature = "serde"))] _document_restore: Option<()>,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.reconcile_surface_pointer_provider()?;
        self.presentation
            .reclaim_quiescent_streams(&mut self.engine, self.presentation_host)?;
        if let Some(native) = self.native.as_mut() {
            native.record_abandoned_effects(&self.abandoned_native_effects)?;
            let prefix_retirement = native.reclaim_committed_prefix(&mut self.engine);
            self.presentation.reclaim_compacted_streams(&self.engine);
            prefix_retirement?;
        }
        let mut prelude = self.engine.begin_host_frame(self.presentation_host)?;
        #[cfg(feature = "serde")]
        if let Some(restore) = document_restore.as_ref() {
            prelude.restrict_item_identity_scope(restore.item_identity_scope());
        } else if let Some(document) = self.document.as_ref() {
            prelude.restrict_item_identity_scope(document.item_identity_scope());
        }
        let submitted_presentation = if let Some(native) = self.native.as_mut() {
            self.presentation.submit_backend_observation(
                &mut prelude,
                &self.engine,
                native.recorder_mut(),
            )?
        } else {
            self.presentation.submit_observation(&mut prelude)?
        };
        let frame = prelude.seal(&self.engine)?;
        let next_source_sequence = self.committed_source_sequence;
        let next_pointer_sequence = self
            .pointer
            .as_ref()
            .map(interaction::RuntimePointerState::committed_sequence);
        let mut host_frame = DockspaceHostFrame {
            session: self,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted: false,
            submitted_presentation,
            painted_surfaces: BTreeSet::new(),
            painted_native_staging: BTreeSet::new(),
            #[cfg(feature = "serde")]
            document_restore: None,
        };
        if let Some(native) = host_frame.session.native.as_mut() {
            let batch = native.prepare_batch(&host_frame.session.engine)?;
            let mut progress = host_frame.frame.submit_backend_ingress(batch)?;
            while progress == crate::engine::BackendIngressProgress::ReceiverReceiptsRequired {
                let candidates = host_frame
                    .frame
                    .pointer_receiver_candidates()
                    .ok_or(NativePlatformError::ProtocolInvariant)?;
                let receipts = candidates
                    .candidates()
                    .iter()
                    .map(|candidate| {
                        let observation = if candidate.receiver_is_applicable() {
                            let resolver = resolver
                                .as_deref_mut()
                                .ok_or(NativePlatformError::ReceiverResolverRequired)?;
                            native::resolve_receiver_observation(
                                &host_frame.frame,
                                candidate,
                                resolver,
                            )?
                        } else {
                            crate::pointer_receiver::PointerReceiverObservation::NotApplicable
                        };
                        Ok(candidate.receipt(observation))
                    })
                    .collect::<Result<Vec<_>, DockspaceRuntimeError>>()?;
                let receipts = crate::pointer_receiver::PointerReceiverReceiptBatch::new(receipts)
                    .map_err(|_| NativePlatformError::ProtocolInvariant)?;
                progress = host_frame
                    .frame
                    .submit_backend_pointer_receiver_receipts(receipts)?;
            }
        }
        #[cfg(feature = "serde")]
        if let Some(restore) = document_restore.take() {
            let (input, pending) = restore
                .into_engine_input(host_frame.frame.view())
                .map_err(DockspacePersistenceError::session)?;
            host_frame.append(input)?;
            host_frame.document_restore = Some(pending);
        }
        Ok(host_frame)
    }
}
