//! Affine host-frame orchestration and atomic publication.

use std::collections::BTreeSet;

use crate::close_plan::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use crate::command::ContentCloseTarget;
#[cfg(feature = "serde")]
use crate::document::PendingRuntimeDocumentRestore;
use crate::engine::{CoreHostFrame, EngineInput, HostPresentationUnavailableReason};
use crate::ids::{SourceSequence, StableInputSourceId, SurfaceId};
use crate::model::{DockPlacement, DockspaceView, WorkspaceVersion};
use crate::policy::DockPolicy;

#[cfg(feature = "serde")]
use super::DockspacePersistenceError;
use super::close::PreparedCloseRequest;
use super::local_action::PreparedSurfaceAction;
use super::native::NativePlatformError;
use super::presentation;
use super::{
    DockPresentationConfig, DockspaceRuntimeError, DockspaceSession, HostFrameReport,
    NativeStagingPaintRequest, NativeSurfaceBinding, NativeWindowPlacement, PreparedDockAction,
    SurfaceUnavailableReason,
};

const APPLICATION_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0x64_6f_63_6b_73_70_61_63);

/// One affine, rollbackable application host frame.
///
/// Dropping this value discards the private candidate and leaves the published
/// session unchanged.
#[must_use = "dropping a host frame rolls back its uncommitted candidate"]
pub struct DockspaceHostFrame<'session> {
    pub(super) session: &'session mut DockspaceSession,
    pub(super) frame: CoreHostFrame,
    pub(super) next_source_sequence: u64,
    pub(super) next_pointer_sequence: Option<u64>,
    pub(super) pointer_input_submitted: bool,
    pub(super) submitted_presentation: presentation::SubmittedPresentationObservation,
    pub(super) painted_surfaces: BTreeSet<SurfaceId>,
    pub(super) painted_native_staging:
        BTreeSet<crate::presentation_observation::NativeStagingPresentation>,
    #[cfg(feature = "serde")]
    pub(super) document_restore: Option<PendingRuntimeDocumentRestore>,
}

impl DockspaceHostFrame<'_> {
    /// Returns the post-input product view without exposing runtime node identities.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        DockspaceView::new(self.frame.view().workspace())
    }

    /// Returns the durable candidate workspace version visible inside this frame.
    #[must_use]
    pub fn version(&self) -> WorkspaceVersion {
        self.frame.view().version()
    }

    /// Resolves one stable item identity to its session-owned external key.
    ///
    /// This is available while a candidate frame is open so an adapter can build
    /// immutable render resources before commit. A restore frame resolves keys
    /// from its pending document binding; an ordinary frame uses the published
    /// session binding.
    #[cfg(feature = "serde")]
    #[must_use]
    pub fn external_key_for_item(&self, item: crate::ids::ItemId) -> Option<&str> {
        self.document_restore
            .as_ref()
            .and_then(|restore| restore.external_item_key(item))
            .or_else(|| self.session.external_key_for_item(item))
    }

    /// Returns the complete post-input logical surface roster.
    #[must_use]
    pub fn surfaces(&self) -> Vec<SurfaceId> {
        self.frame.surfaces().collect()
    }

    /// Selects one currently open item by stable identity.
    ///
    /// The reducer resolves the current tabs source inside this rollback candidate.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn select_item_current(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::SelectItem {
            expected: self.frame.view().version(),
            item,
        })
    }

    /// Opens one item at a stable product placement.
    ///
    /// Opening an already owned item is a valid no-op and does not reposition it.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn open_item_current(
        &mut self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::OpenItem {
            expected: self.frame.view().version(),
            item,
            placement,
        })
    }

    /// Docks one currently open item at a stable product placement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn dock_item_current(
        &mut self,
        item: crate::ids::ItemId,
        placement: DockPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::DockItem {
            expected: self.frame.view().version(),
            item,
            placement,
        })
    }

    /// Docks one complete root at a stable product placement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn dock_root_current(
        &mut self,
        root: crate::ids::RootId,
        placement: DockPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::DockRoot {
            expected: self.frame.view().version(),
            root,
            placement,
        })
    }

    /// Requests a native child window for one complete root.
    ///
    /// Source content remains on its current surface until post-show staging
    /// authorizes the ownership-transfer barrier. First-live presentation then
    /// admits routing, focus, accessibility, and source-resource retirement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn tear_off_root_current(
        &mut self,
        root: crate::ids::RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::TearOffRoot {
            expected: self.frame.view().version(),
            root,
            placement,
        })
    }

    /// Moves one open item into a contained presentation on an existing surface.
    ///
    /// A complete singleton root preserves its root identity. Moving one item out
    /// of a larger root allocates a fresh root and contained-presentation identity.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn float_item_current(
        &mut self,
        item: crate::ids::ItemId,
        surface: crate::ids::SurfaceId,
        rect: crate::geometry::LogicalRect,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::FloatItem {
            expected: self.frame.view().version(),
            item,
            surface,
            rect,
        })
    }

    /// Updates the contained bounds owning one open item.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn set_contained_rect_current(
        &mut self,
        item: crate::ids::ItemId,
        rect: crate::geometry::LogicalRect,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::SetContainedRect {
            expected: self.frame.view().version(),
            item,
            rect,
        })
    }

    /// Raises the contained presentation owning one open item.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn raise_contained_current(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::RaiseContained {
            expected: self.frame.view().version(),
            item,
        })
    }

    /// Clamps the contained presentation owning one item into current exact surface bounds.
    ///
    /// Passive surface or measurement changes never rewrite durable geometry. This
    /// explicit action uses the latest exact ready surface bounds and contained minimum.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn bring_contained_into_view_current(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::BringContainedIntoView {
            expected: self.frame.view().version(),
            item,
        })
    }

    /// Submits one action against the exact published revision which prepared it.
    ///
    /// # Errors
    ///
    /// Returns an error without poisoning the frame when the action belongs to
    /// another dockspace session. A stale action from this session is accepted
    /// structurally and reported as [`crate::runtime::HostInputOutcome::StaleRejected`].
    pub fn submit_prepared_action(
        &mut self,
        prepared: PreparedDockAction,
    ) -> Result<(), DockspaceRuntimeError> {
        let input = self
            .session
            .engine
            .accept_prepared_action(prepared)
            .map_err(DockspaceRuntimeError::prepared_action_authority_mismatch)?;
        self.append(input)
    }

    /// Submits one action prepared from an exact paintable surface candidate.
    ///
    /// The action must be submitted before this frame publishes measurements or
    /// other presentation contributions. Core revalidates the frozen scene,
    /// workspace revision, policy, and topology when reducing the action.
    ///
    /// # Errors
    ///
    /// Returns an error without poisoning the frame when the action belongs to
    /// another dockspace session. Stale actions from this session are accepted
    /// structurally and reported as [`crate::runtime::HostInputOutcome::StaleRejected`].
    pub fn submit_surface_action(
        &mut self,
        prepared: PreparedSurfaceAction,
    ) -> Result<(), DockspaceRuntimeError> {
        let input = prepared
            .into_engine_input(self.session.engine.authority_domain())
            .map_err(DockspaceRuntimeError::prepared_surface_action_authority_mismatch)?;
        self.append(input)
    }

    /// Submits one revision-bound programmatic close request.
    ///
    /// # Errors
    ///
    /// Returns an error without poisoning the frame when the request belongs to
    /// another dockspace session. Stale requests are accepted structurally and
    /// reported as [`crate::runtime::HostInputOutcome::StaleRejected`].
    pub fn submit_prepared_close_request(
        &mut self,
        prepared: PreparedCloseRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        let input = prepared
            .into_engine_input(self.session.engine.authority_domain())
            .map_err(DockspaceRuntimeError::prepared_close_request_authority_mismatch)?;
        self.append(input)
    }

    /// Opens or reuses one core-owned close plan for an item.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn request_close_item_current(
        &mut self,
        item: crate::ids::ItemId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.request_close_current(ContentCloseTarget::Item(item))
    }

    /// Opens or reuses one core-owned close plan for a complete root.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn request_close_root_current(
        &mut self,
        root: crate::ids::RootId,
    ) -> Result<(), DockspaceRuntimeError> {
        self.request_close_current(ContentCloseTarget::Root(root))
    }

    fn request_close_current(
        &mut self,
        target: ContentCloseTarget,
    ) -> Result<(), DockspaceRuntimeError> {
        let prepared = PreparedCloseRequest::new(
            self.session.engine.authority_domain(),
            self.frame.view().version(),
            target,
        );
        self.submit_prepared_close_request(prepared)
    }

    /// Resolves one initial close decision token.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn resolve_close(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::ResolveClose {
            request,
            token,
            decision,
        })
    }

    /// Resolves one deferred close continuation.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn continue_deferred_close(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::ContinueDeferredClose {
            request,
            token,
            decision,
        })
    }

    /// Replaces the complete declarative docking policy at the terminal configuration phase.
    ///
    /// Earlier semantic and pointer input remains ordered before this replacement. Later
    /// semantic input is rejected, while surface contributions may still settle the frame's
    /// pre-configuration presentation before the replacement invalidates it atomically.
    ///
    /// # Errors
    ///
    /// Returns an error when the input prefix is incomplete, document replacement already owns
    /// the terminal phase, or the private application source sequence is exhausted.
    pub fn replace_policy(&mut self, policy: DockPolicy) -> Result<(), DockspaceRuntimeError> {
        let sequence = self.begin_configuration_input()?;
        self.frame
            .append_policy_replacement(APPLICATION_INPUT_SOURCE, sequence, policy)?;
        Ok(())
    }

    /// Replaces renderer-neutral presentation geometry at the terminal configuration phase.
    ///
    /// The configuration controls semantic layout and interaction geometry. Visual-only colors,
    /// fonts, and animation remain adapter-owned.
    ///
    /// # Errors
    ///
    /// Returns an error when the input prefix is incomplete, document replacement already owns
    /// the terminal phase, or the private application source sequence is exhausted.
    pub fn replace_presentation_config(
        &mut self,
        config: DockPresentationConfig,
    ) -> Result<(), DockspaceRuntimeError> {
        let sequence = self.begin_configuration_input()?;
        self.frame.append_presentation_config_replacement(
            APPLICATION_INPUT_SOURCE,
            sequence,
            config,
        )?;
        Ok(())
    }

    /// Explicitly marks one frozen surface unavailable for this presentation pass.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface is outside the frame roster, was
    /// already answered, or its contribution authority became stale.
    pub fn defer_surface(
        &mut self,
        surface: SurfaceId,
        reason: SurfaceUnavailableReason,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let token = self
            .frame
            .view()
            .begin_surface_contribution(surface)
            .map_err(DockspaceRuntimeError::from)?;
        let contribution = self
            .frame
            .view()
            .prepare_surface_unavailable_contribution(token, reason.into())?;
        self.frame.push_surface_contribution(contribution)?;
        Ok(())
    }

    /// Returns the non-interactive native staging paints required by this frame.
    ///
    /// These requests are lifecycle placeholders, not dock scenes. Hosts must
    /// paint only adapter-owned background or loading chrome and must not invoke
    /// pane UI, publish receivers, or infer focus authority from the callback.
    #[must_use]
    pub fn native_staging_paints(&self) -> Vec<NativeStagingPaintRequest> {
        self.frame
            .view()
            .native_staging_presentations()
            .map(|presentation| {
                NativeStagingPaintRequest::from_core(
                    NativeSurfaceBinding::from_binding(
                        presentation.basis().platform_provider(),
                        presentation.binding(),
                    ),
                    presentation,
                )
            })
            .collect()
    }

    /// Records that the renderer painted one exact native staging placeholder.
    ///
    /// The request must belong to the current frame and may be answered once.
    /// Final renderer presentation is reported later through
    /// [`DockspaceSession::report_native_staging_presentation`].
    ///
    /// # Errors
    ///
    /// Returns an error when the request is stale, foreign, or duplicated.
    pub fn confirm_native_staging_painted(
        &mut self,
        request: NativeStagingPaintRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        if !self
            .frame
            .view()
            .native_staging_presentations()
            .any(|presentation| presentation == request.core())
        {
            return Err(NativePlatformError::StaleSurface {
                surface: request.surface(),
            }
            .into());
        }
        if !self.painted_native_staging.insert(request.core()) {
            return Err(NativePlatformError::ProtocolInvariant.into());
        }
        Ok(())
    }

    /// Atomically publishes the frame after every surface received an explicit
    /// contribution disposition.
    ///
    /// # Errors
    ///
    /// Returns an error when the contribution roster is incomplete or the core
    /// rejects the final candidate.
    pub fn commit(mut self) -> Result<HostFrameReport, DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        let Self {
            session,
            frame,
            next_source_sequence,
            next_pointer_sequence: _,
            pointer_input_submitted: _,
            submitted_presentation,
            painted_surfaces,
            painted_native_staging,
            #[cfg(feature = "serde")]
            document_restore,
        } = self;
        let mut frame = frame.into_presentation()?;
        let mut obligations = frame.take_presentation_obligations()?;
        for surface in painted_surfaces {
            let index = obligations
                .iter()
                .position(|obligation| {
                    obligation.slot() == (crate::engine::HostPresentationSlot::Surface { surface })
                })
                .ok_or_else(|| DockspaceRuntimeError::paint_obligation_unavailable(surface))?;
            let obligation = obligations.swap_remove(index);
            let token = frame.view().begin_surface_contribution(surface)?;
            let interaction = frame
                .view()
                .presentation_interaction(surface)
                .ok_or_else(|| DockspaceRuntimeError::paint_obligation_unavailable(surface))?;
            frame.record_painted_surface_contribution(obligation, token, interaction)?;
        }
        for presentation in painted_native_staging {
            let index = obligations
                .iter()
                .position(|obligation| {
                    obligation.slot()
                        == (crate::engine::HostPresentationSlot::NativeStaging { presentation })
                })
                .ok_or_else(|| {
                    DockspaceRuntimeError::paint_obligation_unavailable(
                        presentation.binding().surface(),
                    )
                })?;
            let obligation = obligations.swap_remove(index);
            frame.resolve_presentation_obligation(
                obligation,
                crate::engine::HostPresentationDisposition::Painted(
                    crate::presentation_observation::HostInteractionPresentation::default(),
                ),
            )?;
        }
        for obligation in obligations {
            frame.resolve_presentation_obligation(
                obligation,
                crate::engine::HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )?;
        }
        #[cfg(feature = "serde")]
        let transition = if let Some(restore) = document_restore {
            let prepared = restore
                .prepare_publication(frame, &session.engine)
                .map_err(DockspacePersistenceError::session)?;
            let (transition, binding) = prepared.commit(&mut session.engine)?;
            session.document = Some(binding);
            transition
        } else {
            frame.finish(&mut session.engine)?
        };
        #[cfg(not(feature = "serde"))]
        let transition = frame.finish(&mut session.engine)?;
        session.committed_source_sequence = next_source_sequence;
        let native_commit = session
            .native
            .as_mut()
            .map(|native| native.commit(&session.engine));
        session
            .presentation
            .commit_observation(&transition, &submitted_presentation);
        let (painted_outputs, painted_native_staging_outputs) = session
            .presentation
            .retain_emissions(transition.presentation_emissions());
        Ok(HostFrameReport::from_transition(
            &transition,
            painted_outputs,
            painted_native_staging_outputs,
            native_commit,
            session.abandoned_native_effects.clone(),
        ))
    }

    pub(super) fn append(&mut self, input: EngineInput) -> Result<(), DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let sequence = self.next_application_source_sequence()?;
        self.frame
            .append_input(APPLICATION_INPUT_SOURCE, sequence, input)?;
        Ok(())
    }

    fn begin_configuration_input(&mut self) -> Result<SourceSequence, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        self.complete_pointer_input()?;
        self.next_application_source_sequence()
    }

    fn next_application_source_sequence(
        &mut self,
    ) -> Result<SourceSequence, DockspaceRuntimeError> {
        self.next_source_sequence = self
            .next_source_sequence
            .checked_add(1)
            .ok_or_else(DockspaceRuntimeError::source_sequence_exhausted)?;
        Ok(SourceSequence::new(self.next_source_sequence))
    }

    pub(super) fn ensure_semantic_input_allowed(&self) -> Result<(), DockspaceRuntimeError> {
        #[cfg(feature = "serde")]
        if self.document_restore.is_some() {
            return Err(DockspaceRuntimeError::document_restore_already_submitted());
        }
        Ok(())
    }
}
