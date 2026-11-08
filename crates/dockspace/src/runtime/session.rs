//! Session ownership and host-frame preparation.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Weak};

use crate::command::ContentCloseTarget;
use crate::engine::{DockEngine, EngineError};
#[cfg(test)]
use crate::graph::Workspace;
use crate::model::{DockPlacement, DockspaceLayout, DockspaceView, WorkspaceVersion};
use crate::presentation_observation::PresentationHostLease;

use super::close::PreparedCloseRequest;
use super::host_frame::DockspaceHostFrame;
use super::{
    DockPresentationConfig, DockspacePresentationCommands, DockspaceRuntimeError, DockspaceUpdate,
    NativeInputBudget, NativePlatformError, NativeReceiverAnswer, NativeReceiverQuery,
    NativeWindowPlacement, PreparedDockAction,
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
    pub(super) focus: super::focus::RuntimePaneFocusState,
    pub(super) pointer: Option<interaction::RuntimePointerState>,
    pub(super) native: Option<native::RuntimeNativeState>,
    #[cfg(feature = "serde")]
    pub(super) document: Option<crate::document::BoundDocumentState>,
    pub(super) abandoned_native_effects: native_effect::NativeEffectDropQueue,
    pub(super) native_report_storage_limit: Option<native::NativeReportStorageLimit>,
    pub(super) committed_source_sequence: u64,
    active_update: Weak<()>,
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
            focus: super::focus::RuntimePaneFocusState::default(),
            pointer: None,
            native: None,
            #[cfg(feature = "serde")]
            document: None,
            abandoned_native_effects: native_effect::NativeEffectDropQueue::default(),
            native_report_storage_limit: None,
            committed_source_sequence: 0,
            active_update: Weak::new(),
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
    pub(crate) fn workspace(&self) -> &Workspace {
        self.engine.workspace()
    }

    /// Returns the published item/surface-centric product view.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        self.engine.product_view()
    }

    /// Sets the admitted output and retained stream bound, which defaults to 64.
    /// Native transport owns this budget after bounded report storage is configured.
    pub fn configure_output_capacity(
        &mut self,
        slots: std::num::NonZeroUsize,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.native_report_storage_limit.is_some() {
            return Err(NativePlatformError::ProtocolInvariant.into());
        }
        self.presentation.configure_output_capacity(slots.get())?;
        Ok(())
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

    /// Returns whether the published session still owns an active pointer gesture.
    ///
    /// Native adapters use this summary to preserve every motion edge while a
    /// drag, resize, or press may still have semantic meaning. The underlying
    /// gesture identity and authority remain private to the session.
    #[must_use]
    pub fn has_active_gesture(&self) -> bool {
        !matches!(
            self.engine.interaction().status(),
            crate::interaction::InteractionStatus::Idle
        )
    }

    /// Returns whether a release or root rehome still owns a presentation obligation.
    ///
    /// This derives from pending drag releases, contained-transform releases, and
    /// presentation-gated root rehomes in the committed core state. It remains
    /// true until a host-frame commit delivers or cancels the obligation, including
    /// while an exact output awaits its final result. Hosts must keep a commit
    /// continuation for that result even after the active gesture becomes idle.
    /// Active gestures and native creation are separate from this query.
    #[must_use]
    pub fn has_pending_presentation(&self) -> bool {
        self.engine.has_pending_presentation()
    }

    /// Returns whether an existing obligation still needs presentation on this surface.
    ///
    /// An already emitted output awaiting its final result still counts. Reporting
    /// a renderer result only queues it; a successful host-frame commit must accept
    /// the exact presentation or cancellation before the committed query clears.
    /// An obligation already proven presented or failed needs settlement, not paint.
    /// This query neither grants presentation authority nor includes ordinary scene
    /// changes, active gestures, or native creation. Use
    /// [`HostFrameReport::repaint_surfaces`](super::HostFrameReport::repaint_surfaces)
    /// to distinguish a missing paint from an output already awaiting its result.
    #[must_use]
    pub fn surface_requires_presentation(&self, surface: crate::model::SurfaceId) -> bool {
        self.engine.surface_requires_presentation(surface)
    }

    /// Returns presentation commands for one stable root in the current published state.
    ///
    /// Each command is preflighted by the same core topology, policy, presentation, and native
    /// capability authorities used during submission. A ready action remains revision-bound and
    /// is revalidated when submitted to a host frame.
    #[must_use]
    pub const fn presentation_commands(
        &self,
        root: crate::ids::RootId,
    ) -> DockspacePresentationCommands<'_> {
        DockspacePresentationCommands::new(self, root)
    }

    /// Prepares one revision-bound selection action from the published workspace.
    ///
    /// Selection requests pane focus and explicit activation of an eligible native
    /// window, even when the item is already selected. Focus still requires host observation.
    pub const fn prepare_select_item(&self, item: crate::ids::ItemId) -> PreparedDockAction {
        self.engine.prepare_select_item(item)
    }

    /// Prepares one revision-bound open action from the published workspace.
    ///
    /// A newly opened item becomes selected and requests pane focus. An existing
    /// item is an inert no-op, preserving its location, selection and focus.
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

    /// Prepares one complete-root dock-back against the exact published workspace version.
    #[must_use]
    pub const fn prepare_dock_root_back(&self, root: crate::ids::RootId) -> PreparedDockAction {
        self.engine.prepare_dock_root_back(root)
    }

    /// Prepares one complete-root move to a native window against the published version.
    #[must_use]
    pub const fn prepare_move_root_to_new_window(
        &self,
        root: crate::ids::RootId,
        placement: NativeWindowPlacement,
    ) -> PreparedDockAction {
        self.engine.prepare_move_root_to_new_window(root, placement)
    }

    /// Prepares one revision-bound complete-root contained-floating action.
    #[must_use]
    pub const fn prepare_float_root(
        &self,
        root: crate::ids::RootId,
        surface: crate::ids::SurfaceId,
    ) -> PreparedDockAction {
        self.engine.prepare_float_root(root, surface)
    }

    /// Prepares one revision-bound contained-floating action.
    pub const fn prepare_float_item(
        &self,
        item: crate::ids::ItemId,
        surface: crate::ids::SurfaceId,
    ) -> PreparedDockAction {
        self.engine.prepare_float_item(item, surface)
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

    /// Begins one owned, rollbackable update without borrowing the session.
    ///
    /// Resume it through [`Self::resume_update`] to use the host-frame API.
    /// Dropping it releases the active-update lease and discards its candidate.
    ///
    /// # Errors
    ///
    /// Returns [`super::DockspaceRuntimeErrorKind::UpdateInProgress`] while a
    /// suspended update is active, or any ordinary host-frame prelude error.
    pub fn begin_update(&mut self) -> Result<DockspaceUpdate, DockspaceRuntimeError> {
        Ok(self.begin_host_frame()?.suspend())
    }

    /// Resumes the exact active update through a temporary session borrow.
    ///
    /// This moves the existing candidate; it does not repeat the frame prelude.
    /// Only [`DockspaceHostFrame::commit`] publishes it and revalidates the core
    /// authority fence. On failure, the update is discarded and its originating
    /// session's active-update lease is released.
    ///
    /// # Errors
    ///
    /// Returns [`super::DockspaceRuntimeErrorKind::ForeignUpdate`] when this
    /// session does not own the update's active lease, or its exact host or input
    /// producer authority changed while the update was suspended. A suspended
    /// document restore also returns
    /// [`super::DockspaceRuntimeErrorKind::OperationConflict`] if its source
    /// document sidecars changed; that update is consumed and the restore must be
    /// prepared again from the current session.
    pub fn resume_update(
        &mut self,
        update: DockspaceUpdate,
    ) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        if !self
            .active_update
            .ptr_eq(&Arc::downgrade(&update.update_lease))
        {
            return Err(DockspaceRuntimeError::foreign_update());
        }
        update.resume(self)
    }

    pub(super) fn lease_update(&mut self) -> Arc<()> {
        let lease = Arc::new(());
        self.active_update = Arc::downgrade(&lease);
        lease
    }

    /// Begins one affine application host frame.
    ///
    /// Pending output observations are supplied from the facade-owned
    /// presentation sidecar before input authority is frozen.
    ///
    /// # Errors
    ///
    /// Returns an error when presentation output is awaiting an explicit host
    /// observation, another update is active, or the core rejects the frame prelude.
    pub fn begin_host_frame(&mut self) -> Result<DockspaceHostFrame<'_>, DockspaceRuntimeError> {
        self.begin_host_frame_with_native_resolver(None)
    }

    /// Begins one native host frame and synchronously resolves every frozen
    /// receiver question through product-level descriptors.
    ///
    /// The callback never sees pointer leases, provider sequences, backend
    /// ordinals, candidate identities, or receipts. A failed frame retains the
    /// original native edge for an exact retry.
    /// This synchronous convenience drains its selected interval without a time
    /// budget. Buffered event loops should use [`Self::poll_native_host_frame`].
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

    /// Polls native input using one budget started before the host's prefix claim.
    ///
    /// `None` retains an unpublished candidate for the next scheduling turn. A
    /// returned frame may still have [`DockspaceHostFrame::native_input_pending`];
    /// render/commit that prefix and poll again before taking application input.
    /// The budget includes receiver queries and stops only between full records.
    /// An indivisible terminal group can span polls but publishes as one group.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable managed host, invalid receiver facts,
    /// or a rejected rollbackable candidate. Failed attempts remain retryable
    /// from the last published input watermark.
    pub fn poll_native_host_frame(
        &mut self,
        budget: &mut NativeInputBudget,
        mut resolve: impl FnMut(NativeReceiverQuery) -> NativeReceiverAnswer,
    ) -> Result<Option<DockspaceHostFrame<'_>>, DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        self.poll_host_frame_with_options(Some(&mut resolve), Some(budget))
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
        self.poll_host_frame_with_options(resolver, None)?
            .ok_or_else(|| NativePlatformError::ProtocolInvariant.into())
    }

    pub(super) fn ensure_standalone_replacement_available(
        &self,
    ) -> Result<(), DockspaceRuntimeError> {
        if self
            .native
            .as_ref()
            .is_some_and(|native| native.profile == native::NativeHostProfile::ManagedDesktop)
        {
            return Err(NativePlatformError::ReplacementRequiresNativeFrame.into());
        }
        Ok(())
    }

    fn poll_host_frame_with_options(
        &mut self,
        resolver: Option<&mut dyn FnMut(NativeReceiverQuery) -> NativeReceiverAnswer>,
        budget: Option<&mut NativeInputBudget>,
    ) -> Result<Option<DockspaceHostFrame<'_>>, DockspaceRuntimeError> {
        if self.active_update.strong_count() != 0 {
            return Err(DockspaceRuntimeError::update_in_progress());
        }
        let pending = self
            .native
            .as_mut()
            .and_then(native::RuntimeNativeState::take_pending_replay);
        let (mut frame, submitted_presentation, ingress_pending, prelude_pending) =
            if let Some(pending) = pending {
                (
                    pending.frame,
                    pending.submitted_presentation,
                    true,
                    pending.prelude_pending,
                )
            } else {
                self.reconcile_pointer_provider()?;
                self.presentation
                    .reclaim_quiescent_streams(&mut self.engine, self.presentation_host)?;
                let mut prelude_pending = false;
                if let Some(native) = self.native.as_mut() {
                    let prefix_retirement = native.reclaim_committed_prefix(&mut self.engine);
                    self.presentation.reclaim_compacted_streams(&self.engine);
                    prefix_retirement?;
                    match native.record_abandoned_effects(&self.abandoned_native_effects) {
                        Ok(()) => {}
                        Err(NativePlatformError::InputCapacity) => prelude_pending = true,
                        Err(error) => return Err(error.into()),
                    }
                }
                let mut prelude = self.engine.begin_host_frame(self.presentation_host)?;
                prelude.set_native_binding_allocation_budget(
                    self.native
                        .as_ref()
                        .and_then(native::RuntimeNativeState::binding_allocation_budget),
                );
                #[cfg(feature = "serde")]
                if let Some(document) = self.document.as_ref() {
                    prelude.restrict_item_identity_scope(document.item_identity_scope());
                }
                let submitted_presentation = if let Some(native) = self.native.as_mut() {
                    let (submitted, capacity_blocked) =
                        self.presentation.submit_backend_observation(
                            &mut prelude,
                            &self.engine,
                            native.recorder_mut(),
                        )?;
                    prelude_pending |= capacity_blocked;
                    if prelude_pending
                        && native.recorder_mut().recorded_through()
                            == self.engine.backend_ingress_committed_through()
                    {
                        return Err(NativePlatformError::InputCapacity.into());
                    }
                    submitted
                } else {
                    self.presentation.submit_observation(&mut prelude)?
                };
                let mut frame = prelude.seal(&self.engine)?;
                let ingress_pending = if let Some(native) = self.native.as_ref() {
                    native.prepare_pending_input(&self.engine, &mut frame, budget.is_some())?
                } else {
                    false
                };
                (
                    frame,
                    submitted_presentation,
                    ingress_pending,
                    prelude_pending,
                )
            };
        if ingress_pending {
            let native = self
                .native
                .as_mut()
                .ok_or(NativePlatformError::ProtocolInvariant)?;
            if !native.reduce_pending_input(&mut frame, budget, resolver, &self.presentation)? {
                native.retain_pending_replay(native::PendingNativeReplay {
                    frame,
                    submitted_presentation,
                    prelude_pending,
                });
                return Ok(None);
            }
        }
        let native_input_pending = self.native.as_ref().is_some_and(|native| {
            prelude_pending
                || native.input_pending_after(&frame)
                || self.abandoned_native_effects.has_pending()
                || self.presentation.has_unrecorded_backend_report()
        });
        let next_source_sequence = self.committed_source_sequence;
        let next_pointer_sequence = self
            .pointer
            .as_ref()
            .map(interaction::RuntimePointerState::committed_sequence);
        let host_frame = DockspaceHostFrame {
            session: self,
            update_lease: None,
            frame,
            next_source_sequence,
            next_pointer_sequence,
            pointer_input_submitted: false,
            native_input_pending,
            submitted_presentation,
            painted_surfaces: BTreeMap::new(),
            encoding_surfaces: BTreeSet::new(),
            encoding_started: std::cell::Cell::new(false),
            encoding_checkpoint: None,
            encoding_pass: 0,
            painted_native_staging: BTreeSet::new(),
            replacement_submitted: false,
            #[cfg(feature = "serde")]
            document_restore: None,
        };
        Ok(Some(host_frame))
    }
}
