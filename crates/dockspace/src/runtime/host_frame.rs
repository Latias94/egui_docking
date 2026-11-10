//! Affine host-frame orchestration and atomic publication.

use std::collections::BTreeMap;

use crate::close_plan::{
    CloseDecision, CloseDecisionToken, CloseRequestId, DeferredCloseDecision, DeferredCloseToken,
};
use crate::command::ContentCloseTarget;
#[cfg(feature = "serde")]
use crate::document::PendingRuntimeDocumentRestore;
use crate::engine::{
    CoreHostFrame, EngineInput, HostPresentationSlot, HostPresentationUnavailableReason,
};
use crate::ids::{RootId, SourceSequence, StableInputSourceId, SurfaceId};
use crate::model::{DockPlacement, DockspaceView, WorkspaceVersion};
use crate::policy::DockPolicy;
use crate::transition::InputOutcome;

#[cfg(feature = "serde")]
use super::DockspacePersistenceError;
use super::close::PreparedCloseRequest;
use super::local_action::PreparedSurfaceAction;
use super::native::NativePlatformError;
use super::presentation;
use super::{
    DockPresentationConfig, DockspaceCloseItem, DockspaceRuntimeError, DockspaceSession,
    DockspaceSubmittedAction, HostFrameReport, NativeSurfaceBinding, NativeWindowPlacement,
    PreparedDockAction, PreparedPaneFocusObservation, SurfaceProgramExecutionReceipt,
    SurfaceUnavailableReason,
};

pub(super) const APPLICATION_INPUT_SOURCE: StableInputSourceId =
    StableInputSourceId::new(0x64_6f_63_6b_73_70_61_63);

fn has_ordinary_application_input(transition: &crate::transition::EngineTransition) -> bool {
    transition.reduced_inputs().iter().any(|input| {
        input.source() == APPLICATION_INPUT_SOURCE
            && !matches!(
                input.outcome(),
                InputOutcome::NativeInputCancelledForOverload
                    | InputOutcome::NativeCreateCancelled { .. }
                    | InputOutcome::NativeSurfaceCreateCancelled { .. }
                    | InputOutcome::NativeSurfaceCreateIntentCancelled { .. }
                    | InputOutcome::NativeSurfaceCreateAdvanced { .. }
            )
    })
}

fn workspace_root_continuations(
    transition: &crate::transition::EngineTransition,
) -> &[(
    crate::viewport::ViewportBinding,
    crate::viewport::ViewportBinding,
)] {
    transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::WorkspaceReplaced { reconciliation, .. } => {
                Some(reconciliation.rebound())
            }
            _ => None,
        })
        .unwrap_or(&[])
}

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
    pub(super) native_input_pending: bool,
    pub(super) submitted_presentation: presentation::SubmittedPresentationObservation,
    pub(super) painted_surfaces: BTreeMap<SurfaceId, SurfaceProgramExecutionReceipt>,
    pub(super) encoding_surfaces:
        BTreeMap<SurfaceId, Option<crate::presentation_observation::PresentationStreamAdmission>>,
    pub(super) encoding_started: std::cell::Cell<bool>,
    pub(super) encoding_checkpoint: Option<crate::engine::CoreEncodingCheckpoint>,
    pub(super) encoding_pass: u64,

    pub(super) replacement_submitted: bool,
    #[cfg(feature = "serde")]
    pub(super) document_restore: Option<PendingRuntimeDocumentRestore>,
    pub(super) update_lease: Option<std::sync::Arc<()>>,
}

impl DockspaceHostFrame<'_> {
    /// Replaces all logical placements in one real native window atomically.
    ///
    /// Measurements and painting in this candidate use the proposed placements.
    /// Dropping the frame preserves the committed roster and exact generations.
    /// The clip is independent of each surface's logical layout size.
    ///
    /// # Errors
    ///
    /// Rejects stale windows, invalid geometry, missing surfaces, storage refusal,
    /// pending native input or an already started presentation phase.
    pub fn set_native_surface_placements(
        &mut self,
        window: super::NativeWindowBinding,
        placements: &[super::NativeSurfacePlacement],
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        self.session.ensure_managed_native_host_available()?;
        let native = self
            .session
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        if native.provider() != window.provider() {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        if native
            .placement_roster_capacity()
            .is_some_and(|capacity| placements.len() > capacity)
        {
            return Err(NativePlatformError::InputCapacity.into());
        }
        let mut owned = Vec::new();
        owned
            .try_reserve_exact(placements.len())
            .map_err(|_| NativePlatformError::InputCapacity)?;
        owned.extend_from_slice(placements);
        let expected = self.frame.view().version();
        self.append(EngineInput::SetNativeSurfacePlacements {
            provider: window.provider(),
            expected,
            window: window.core(),
            placements: owned,
        })
    }

    /// Queries the exact proposed surface roster of a real window.
    pub fn native_surface_bindings_for_window(
        &self,
        window: super::NativeWindowBinding,
    ) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        let provider = self
            .session
            .native
            .as_ref()
            .map(super::native::RuntimeNativeState::provider);
        self.frame
            .view()
            .viewport()
            .registry()
            .bindings_for_window(window.core())
            .filter_map(move |binding| {
                (provider == Some(window.provider()))
                    .then(|| NativeSurfaceBinding::from_binding(window.provider(), binding))
            })
    }

    /// Returns committed predecessor placements while a candidate proposes successors.
    ///
    /// Resource preparation uses this exact scope; candidate queries return the
    /// proposed scope and cannot identify pending callbacks from the predecessor.
    pub fn committed_native_surface_bindings_for_window(
        &self,
        window: super::NativeWindowBinding,
    ) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.session.native_surface_bindings_for_window(window)
    }

    /// Returns layout and physical mapping only for this exact proposed placement.
    #[must_use]
    pub fn native_surface_placement(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<super::NativeSurfacePlacement> {
        let native = self.session.native.as_ref()?;
        if binding.window().provider() != native.provider() {
            return None;
        }
        self.frame
            .view()
            .viewport()
            .registry()
            .surface_placement(binding.core_binding())
    }

    /// Returns exact Root binding pairs proposed by this terminal replacement.
    ///
    /// Prepay route/resource changes from these opaque pairs before committing
    /// the frame. They grant no current authority. A failed or dropped frame
    /// leaves the predecessor live; only the matching committed report permits
    /// installing the successor on that same physical window.
    pub fn proposed_native_root_continuations(
        &self,
    ) -> impl Iterator<Item = super::NativeRootContinuation> + '_ {
        let provider = self.session.native.as_ref().map(|native| native.provider());
        self.frame
            .proposed_workspace_root_rebounds()
            .filter_map(move |(before, after)| {
                provider.map(|provider| {
                    super::NativeRootContinuation::from_pair(provider, before, after)
                })
            })
    }

    /// Advances at most one committed native document creation intent.
    ///
    /// Fresh root coordinates, current platform capabilities, exact predecessor
    /// quiescence and allocation capacity are required before an attempt starts.
    /// Temporary absence of these facts returns `false` without consuming input.
    /// A started or definitively rejected attempt returns `true`; the host must
    /// charge one record to its shared lifecycle budget. Dropping this frame
    /// publishes neither progress nor an effect. Shutdown should cancel instead.
    ///
    /// # Errors
    ///
    /// Returns an error without a managed host, with pending native input or a
    /// closed semantic phase, or if the lifecycle invariant cannot be preserved.
    pub fn advance_next_native_surface_create(&mut self) -> Result<bool, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        self.session.ensure_managed_native_host_available()?;
        let surface = {
            let viewport = self.frame.view().viewport();
            viewport
                .waiting_native_surfaces()
                .find(|initial| viewport.queued_native_surface_can_advance(initial))
                .map(|initial| initial.request.surface)
        };
        let Some(surface) = surface else {
            return Ok(false);
        };
        self.append(EngineInput::AdvanceNativeSurfaceCreate {
            expected: self.frame.view().version(),
            surface,
        })?;
        Ok(true)
    }

    /// Attaches a logical surface with one empty central main root.
    ///
    /// Declared identities may be reused after exact retirement. Unaffected
    /// surfaces keep their current presented input proof; the workspace epoch
    /// is unchanged. This operation creates no native window.
    ///
    /// # Errors
    ///
    /// Returns a frame-phase or submission error. Semantic refusals are reported
    /// by the returned action token after this complete frame commits.
    pub fn attach_empty_surface_current(
        &mut self,
        surface: SurfaceId,
        root: RootId,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let attempt = self.frame.presentation_attempt();
        let sequence = self.append_identified(EngineInput::AttachEmptySurface {
            expected: self.frame.view().version(),
            surface,
            root,
        })?;
        Ok(DockspaceSubmittedAction::new(attempt, sequence))
    }

    /// Retires an item-free surface and its complete empty main/contained forest.
    ///
    /// Close or migrate content first. Native hosting and recovery obligations
    /// must finish before their logical surface can be retired. An abandoned
    /// or rejected frame publishes no membership change.
    ///
    /// # Errors
    ///
    /// Returns a frame-phase or submission error. A nonempty surface or lifecycle
    /// conflict is a typed outcome associated with the returned action token.
    pub fn retire_empty_surface_current(
        &mut self,
        surface: SurfaceId,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let attempt = self.frame.presentation_attempt();
        let sequence = self.append_identified(EngineInput::RetireEmptySurface {
            expected: self.frame.view().version(),
            surface,
        })?;
        Ok(DockspaceSubmittedAction::new(attempt, sequence))
    }

    /// Creates a managed child window for an existing complete logical surface.
    ///
    /// Its main root and entire contained forest stay in the one workspace.
    /// Creation, renderer preparation and exact first-live presentation admit
    /// the binding without repeating a topology transfer. A current application
    /// root must own the recovery destination. Document preferences are optional
    /// inputs to this explicit request and never create windows by themselves.
    /// After a failed or cancelled initial attempt, the same request may create
    /// a fresh binding while cleanup retains ownership of the old incarnation.
    /// A healthy attempt or a destruction-driven recovery remains a conflict.
    ///
    /// # Errors
    ///
    /// Returns an error for a nonmanaged host, unfinished native input or a closed
    /// input phase. Policy, unavailable surfaces and conflicting native ownership
    /// are typed outcomes of the exact submitted action in the committed report.
    pub fn create_native_surface_current(
        &mut self,
        surface: SurfaceId,
        recovery_host: SurfaceId,
        placement: NativeWindowPlacement,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        self.session.ensure_managed_native_host_available()?;
        let attempt = self.frame.presentation_attempt();
        let sequence = self.append_identified(EngineInput::CreateNativeSurface {
            expected: self.frame.view().version(),
            surface,
            recovery_host,
            placement: placement.outer_rect(),
        })?;
        Ok(DockspaceSubmittedAction::new(attempt, sequence))
    }

    /// Cancels managed pointer and scroll input without retiring its provider.
    ///
    /// Earlier native input must be fully reduced. This is the frame's final
    /// input operation; measurement and paint may still follow. Dropping or
    /// failing to commit the frame publishes no cancellation.
    ///
    /// # Errors
    ///
    /// Returns an error for a nonmanaged host, pending native input, a stale
    /// provider, or an input phase which has already closed.
    pub fn cancel_native_input_for_overload(
        &mut self,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let input = self
            .session
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?
            .overload_cancellation_input(&self.frame)?;
        let attempt = self.frame.presentation_attempt();
        let sequence = self.append_identified(input)?;
        Ok(DockspaceSubmittedAction::new(attempt, sequence))
    }

    /// Explicitly cancels at most one pending native create.
    ///
    /// Selects the first saga from this frame's current candidate, including
    /// creates awaiting first-live presentation. Source content remains on its
    /// current surface until the exact first-live output is presented, so every
    /// pending create can use the existing cleanup protocol without moving it.
    /// Returns whether one cancellation input was appended; dropping or failing
    /// to commit the frame publishes no cancellation. Hosts account for each
    /// appended input against their own lifecycle work budget. Appending does
    /// not reserve effect-report storage; compensation stays in the existing
    /// effect ledger until delivery capacity is available.
    ///
    /// # Errors
    ///
    /// Returns an error when native input remains pending, the input phase has
    /// closed, the private source sequence is exhausted, or core rejects cleanup.
    pub fn cancel_next_native_create(&mut self) -> Result<bool, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let saga = self
            .frame
            .view()
            .viewport()
            .native_create_sagas()
            .next()
            .map(|(id, _)| id);
        if let Some(saga) = saga {
            self.append(EngineInput::CancelNativeCreate {
                expected: self.frame.view().version(),
                saga,
            })?;
            return Ok(true);
        }
        let initial = self
            .frame
            .view()
            .viewport()
            .pending_recoveries()
            .find_map(|(_, pending)| {
                (matches!(
                    pending.origin(),
                    crate::viewport::NativeSurfaceEstablishmentOrigin::Initial { .. }
                ) && !matches!(
                    pending.status(),
                    crate::frame::RecoveryPendingStatus::ReplacementCancelled { .. }
                        | crate::frame::RecoveryPendingStatus::AwaitingReplacementCleanup { .. }
                ))
                .then(|| pending.replacement_binding())
                .flatten()
            });
        if let Some(binding) = initial {
            self.append(EngineInput::CancelNativeSurface {
                expected: self.frame.view().version(),
                binding,
            })?;
            return Ok(true);
        }
        let queued = self
            .frame
            .view()
            .viewport()
            .waiting_native_surfaces()
            .next()
            .map(|initial| initial.request.surface);
        let Some(surface) = queued else {
            return Ok(false);
        };
        self.append(EngineInput::CancelNativeSurfaceCreateIntent {
            expected: self.frame.view().version(),
            surface,
        })?;
        Ok(true)
    }

    /// Returns whether native input or report submission remains after this prefix.
    ///
    /// Render and commit this prefix, then poll again before taking an application
    /// action. Application semantic and configuration input is rejected meanwhile.
    #[must_use]
    pub const fn native_input_pending(&self) -> bool {
        self.native_input_pending
    }

    /// Returns exact native binding identities proposed by a terminal replacement.
    ///
    /// Hosts may use this candidate roster to prepay resources before commit.
    /// It grants no current binding, routing, focus or presentation authority.
    /// Dropping or failing the frame discards these proposals; install them only
    /// after the complete committed report confirms its successor roster.
    /// Ordinary frames and sessions without native hosting return no entries.
    pub fn proposed_native_bindings(&self) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        let provider = self
            .replacement_submitted
            .then(|| {
                self.session
                    .native
                    .as_ref()
                    .map(super::native::RuntimeNativeState::provider)
            })
            .flatten();
        self.frame
            .view()
            .viewport()
            .registry()
            .records()
            .filter_map(move |(_, record)| {
                provider
                    .map(|provider| NativeSurfaceBinding::from_binding(provider, record.binding()))
            })
    }

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

    /// Returns the configuration used to measure this frame's surfaces.
    #[must_use]
    pub fn presentation_config(&self) -> &DockPresentationConfig {
        self.frame.view().presentation_config()
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
    /// Selection requests pane focus and explicit activation of an eligible native
    /// window, even when the item is already selected. Focus still requires host observation.
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
    /// A newly opened item becomes selected and requests pane focus. Opening an
    /// already owned item is a valid no-op, preserving location, selection and focus.
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
    /// Source content remains on its current surface until the exact first-live
    /// target output is presented. That atomic boundary transfers ownership and
    /// admits routing, focus, accessibility, and source-resource retirement.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame is poisoned or its private source
    /// sequence cannot advance.
    pub fn move_root_to_new_window_current(
        &mut self,
        root: crate::ids::RootId,
        placement: NativeWindowPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.move_to_new_window_current(crate::model::NativeTearOffSource::Root(root), placement)
    }

    /// Requests a native window for an item, frozen tabs group, or complete root.
    ///
    /// The source stays unchanged until the exact target first-live output is
    /// presented. Creation failure and cancellation retain its original owner.
    /// A tabs-group selection remains bound to the version where it was captured.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame cannot accept another semantic input.
    /// Product-level availability and policy rejections are reported at commit.
    pub fn move_to_new_window_current(
        &mut self,
        source: crate::model::NativeTearOffSource,
        placement: NativeWindowPlacement,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::TearOffNative {
            expected: self.frame.view().version(),
            source,
            placement: Some(placement),
        })
    }

    /// Requests native tear-off using core placement from current authoritative facts.
    ///
    /// Placement uses the same source geometry and work-area solver as paint,
    /// menu and prepared commands. Missing facts produce an ordinary product
    /// rejection without moving the source or emitting a Create effect.
    ///
    /// # Errors
    ///
    /// Returns an error when the frame cannot accept another semantic input.
    pub fn move_to_new_window_default_current(
        &mut self,
        source: crate::model::NativeTearOffSource,
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::TearOffNative {
            expected: self.frame.view().version(),
            source,
            placement: None,
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
    ) -> Result<(), DockspaceRuntimeError> {
        self.append(EngineInput::FloatItem {
            expected: self.frame.view().version(),
            item,
            surface,
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
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let input = self
            .session
            .engine
            .accept_prepared_action(prepared)
            .map_err(DockspaceRuntimeError::prepared_action_authority_mismatch)?;
        let frame_attempt = self.frame.presentation_attempt();
        let source_sequence = self.append_identified(input)?;
        Ok(DockspaceSubmittedAction::new(
            frame_attempt,
            source_sequence,
        ))
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
        self.ensure_semantic_input_allowed()?;
        let input = prepared
            .into_engine_input(self.session.engine.authority_domain())
            .map_err(DockspaceRuntimeError::prepared_surface_action_authority_mismatch)?;
        self.append(input)
    }

    /// Submits an action retained from an earlier framework pass or update.
    ///
    /// Core validates the captured target against current relevant state.
    /// Stale or foreign witnesses produce a typed input rejection in the report.
    /// Independent events are always reduced independently; adapters own any
    /// deduplication of repeated passes for the same event slot.
    ///
    /// # Errors
    ///
    /// Returns an error when semantic ingress is closed or the input cannot be appended.
    pub fn submit_deferred_surface_action(
        &mut self,
        prepared: super::local_action::DeferredSurfaceAction,
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        self.append(prepared.into_engine_input())
    }

    /// Submits one observation prepared from an exact published pane-focus request.
    ///
    /// The runtime assigns a non-repeating provider generation only when the
    /// terminal pass hands the affine observation back to a host frame.
    ///
    /// # Errors
    ///
    /// Returns an error when the observation belongs to another session or the
    /// provider generation space is exhausted.
    pub fn submit_pane_focus_observation(
        &mut self,
        prepared: PreparedPaneFocusObservation,
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let authority = self.session.engine.authority_domain();
        prepared
            .validate_authority(authority)
            .map_err(DockspaceRuntimeError::prepared_pane_focus_observation_authority_mismatch)?;
        let generation = self
            .session
            .focus
            .next_generation(prepared.request())
            .ok_or_else(DockspaceRuntimeError::pane_focus_observation_generation_exhausted)?;
        let input = prepared
            .into_engine_input(authority, generation)
            .map_err(DockspaceRuntimeError::prepared_pane_focus_observation_authority_mismatch)?;
        self.append(input)
    }

    /// Submits one revision-bound programmatic close request.
    ///
    /// Returns the identity for querying this request's exact committed outcome.
    ///
    /// # Errors
    ///
    /// Returns an error without poisoning the frame when the request belongs to
    /// another dockspace session. Stale requests are accepted structurally and
    /// reported as [`crate::runtime::HostInputOutcome::StaleRejected`].
    pub fn submit_prepared_close_request(
        &mut self,
        prepared: PreparedCloseRequest,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let input = prepared
            .into_engine_input(self.session.engine.authority_domain())
            .map_err(DockspaceRuntimeError::prepared_close_request_authority_mismatch)?;
        let frame_attempt = self.frame.presentation_attempt();
        let source_sequence = self.append_identified(input)?;
        Ok(DockspaceSubmittedAction::new(
            frame_attempt,
            source_sequence,
        ))
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
        self.submit_prepared_close_request(prepared).map(|_| ())
    }

    /// Resolves one initial close decision token.
    ///
    /// Returns the identity for querying this decision's exact committed outcome.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn resolve_close(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        decision: CloseDecision,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        let frame_attempt = self.frame.presentation_attempt();
        let source_sequence = self.append_identified(EngineInput::ResolveClose {
            request,
            token,
            decision,
        })?;
        Ok(DockspaceSubmittedAction::new(
            frame_attempt,
            source_sequence,
        ))
    }

    /// Visits current unresolved initial decisions without cloning a complete plan.
    ///
    /// Items retain the plan's normative order. Only decision-phase plans are
    /// visited; deferred continuations are separate, and a terminal, stale,
    /// foreign or compacted request yields no items. Earlier unresolved native
    /// input also yields no items because application decisions must wait for
    /// its complete boundary. Callers can take a bounded prefix, submit those
    /// copied tokens, commit, and query the remaining decisions in a later frame.
    ///
    /// Reading grants no authority beyond each existing exact decision token.
    /// Dropping the frame leaves published decisions unchanged; ordinary token
    /// validation still applies when resolving a copied item.
    pub fn pending_close_decisions(
        &self,
        request: CloseRequestId,
    ) -> impl Iterator<Item = DockspaceCloseItem> + '_ {
        self.frame
            .view()
            .close_plan(request)
            .filter(|plan| {
                !self.native_input_pending
                    && matches!(
                        plan.phase(),
                        crate::close_plan::ClosePlanPhase::Requested
                            | crate::close_plan::ClosePlanPhase::Resolving
                            | crate::close_plan::ClosePlanPhase::Deferred
                    )
            })
            .into_iter()
            .flat_map(|plan| plan.items().iter())
            .filter(|item| item.state() == crate::close_plan::CloseItemDecisionState::Pending)
            .copied()
            .map(DockspaceCloseItem::from_core)
    }

    /// Resolves one deferred close continuation.
    ///
    /// Returns the identity for querying this continuation's exact committed outcome.
    ///
    /// # Errors
    ///
    /// Returns an error when the affine frame rejects the input structurally.
    pub fn continue_deferred_close(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        decision: DeferredCloseDecision,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        let frame_attempt = self.frame.presentation_attempt();
        let source_sequence = self.append_identified(EngineInput::ContinueDeferredClose {
            request,
            token,
            decision,
        })?;
        Ok(DockspaceSubmittedAction::new(
            frame_attempt,
            source_sequence,
        ))
    }

    /// Replaces the complete declarative docking policy before measurement and encoding.
    ///
    /// Earlier semantic and pointer input retains its original policy and presentation facts.
    /// This replacement updates the private candidate immediately and closes semantic input.
    /// Measurements and encoding use the resulting policy; publication waits for frame commit.
    ///
    /// Returns the identity for querying this replacement's exact committed outcome,
    /// including an unchanged policy and its version at this point in reducer order.
    ///
    /// # Errors
    ///
    /// Returns an error when the input prefix is incomplete, encoding has begun, document
    /// replacement already owns the terminal phase, or the application sequence is exhausted.
    pub fn replace_policy(
        &mut self,
        policy: DockPolicy,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        let frame_attempt = self.frame.presentation_attempt();
        let sequence = self.begin_configuration_input()?;
        self.frame
            .append_policy_replacement(APPLICATION_INPUT_SOURCE, sequence, policy)?;
        Ok(DockspaceSubmittedAction::new(frame_attempt, sequence))
    }

    /// Replaces renderer-neutral presentation geometry before measurement and encoding.
    ///
    /// The configuration controls semantic layout and interaction geometry. Visual-only colors,
    /// fonts, and animation remain adapter-owned. Earlier input retains its original geometry;
    /// subsequent measurement and encoding use this private candidate's new configuration.
    /// The returned receipt resolves this replacement's exact committed result
    /// through [`HostFrameReport::submitted_action_outcome`].
    ///
    /// # Errors
    ///
    /// Returns an error when the input prefix is incomplete, encoding has begun, document
    /// replacement already owns the terminal phase, or the application sequence is exhausted.
    pub fn replace_presentation_config(
        &mut self,
        config: DockPresentationConfig,
    ) -> Result<DockspaceSubmittedAction, DockspaceRuntimeError> {
        let frame_attempt = self.frame.presentation_attempt();
        let sequence = self.begin_configuration_input()?;
        self.frame.append_presentation_config_replacement(
            APPLICATION_INPUT_SOURCE,
            sequence,
            config,
        )?;
        Ok(DockspaceSubmittedAction::new(frame_attempt, sequence))
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
            update_lease: _update_lease,
            frame,
            next_source_sequence,
            next_pointer_sequence: _,
            pointer_input_submitted: _,
            native_input_pending,
            submitted_presentation,
            painted_surfaces,
            encoding_surfaces,
            encoding_started: _,
            encoding_checkpoint: _,
            encoding_pass: _,
            replacement_submitted: _,
            #[cfg(feature = "serde")]
            document_restore,
        } = self;
        let frame_attempt = frame.presentation_attempt();
        let accepted_native_prefix =
            frame.backend_ingress_through() > session.engine.backend_ingress_committed_through();
        let mut frame = frame.into_presentation()?;
        let mut obligations = BTreeMap::new();
        for obligation in frame.take_presentation_obligations()? {
            if obligations.insert(obligation.slot(), obligation).is_some() {
                return Err(NativePlatformError::ProtocolInvariant.into());
            }
        }
        for (surface, receipt) in painted_surfaces {
            let obligation = obligations
                .remove(&HostPresentationSlot::Surface { surface })
                .ok_or_else(|| DockspaceRuntimeError::paint_obligation_unavailable(surface))?;
            let (token, interaction) = receipt.into_presentation_parts();
            frame.record_painted_surface_contribution(obligation, token, interaction)?;
        }
        for obligation in obligations.into_values() {
            frame.resolve_presentation_obligation(
                obligation,
                crate::engine::HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )?;
        }
        // Report backpressure delays external delivery on the original effect
        // ledger. It must not stop an already accepted native prefix from
        // committing and freeing ingress for an older terminal result.
        let effect_capacity = session.abandoned_native_effects.available_slots();
        let frame = frame.with_platform_effect_delivery_limit(effect_capacity);
        #[cfg(feature = "serde")]
        let (transition, presentation, effects, prepared_native) = if let Some(restore) =
            document_restore
        {
            let prepared = restore
                .prepare_publication(frame, &session.engine)
                .map_err(DockspacePersistenceError::session)?;
            if effect_capacity
                .is_some_and(|limit| prepared.transition().platform_effects().len() == limit)
                && prepared.has_new_undelivered_platform_effects(&session.engine)
            {
                return Err(NativePlatformError::InputCapacity.into());
            }
            let prepared_native = session
                .native
                .as_ref()
                .map(|native| {
                    native.prepare_native_commit(
                        prepared.candidate_engine(),
                        workspace_root_continuations(prepared.transition()),
                    )
                })
                .transpose()?;
            let presentation = session.presentation.prepare_emissions(
                prepared.transition(),
                &submitted_presentation,
                &encoding_surfaces,
                prepared.candidate_engine(),
            )?;
            let effects = session
                .abandoned_native_effects
                .prepare_emissions(prepared.transition().platform_effects())?;
            let (transition, binding) = prepared.commit(&mut session.engine)?;
            session.document = Some(binding);
            (transition, presentation, effects, prepared_native)
        } else {
            let prepared = frame.prepare_owned(&session.engine)?;
            let document_placements = session
                .document
                .as_ref()
                .map(|binding| binding.prepare_placement_publication(prepared.candidate_engine()))
                .transpose()
                .map_err(DockspacePersistenceError::capture)?;
            if !accepted_native_prefix
                && has_ordinary_application_input(prepared.transition())
                && effect_capacity
                    .is_some_and(|limit| prepared.transition().platform_effects().len() == limit)
                && prepared.has_new_undelivered_platform_effects(&session.engine)
            {
                return Err(NativePlatformError::InputCapacity.into());
            }
            let prepared_native = session
                .native
                .as_ref()
                .map(|native| {
                    native.prepare_native_commit(
                        prepared.candidate_engine(),
                        workspace_root_continuations(prepared.transition()),
                    )
                })
                .transpose()?;
            let presentation = session.presentation.prepare_emissions(
                prepared.transition(),
                &submitted_presentation,
                &encoding_surfaces,
                prepared.candidate_engine(),
            )?;
            let effects = session
                .abandoned_native_effects
                .prepare_emissions(prepared.transition().platform_effects())?;
            let transition = prepared.commit(&mut session.engine)?;
            if let Some(placements) = document_placements {
                // All fallible work preceded the engine publication. Keep the
                // append-only identity history in its original session owner.
                session
                    .document
                    .as_mut()
                    .expect("prepared placement publication retains its document binding")
                    .publish_placements(placements);
            }
            (transition, presentation, effects, prepared_native)
        };
        #[cfg(not(feature = "serde"))]
        let (transition, presentation, effects, prepared_native) = {
            let prepared = frame.prepare_owned(&session.engine)?;
            if !accepted_native_prefix
                && has_ordinary_application_input(prepared.transition())
                && effect_capacity
                    .is_some_and(|limit| prepared.transition().platform_effects().len() == limit)
                && prepared.has_new_undelivered_platform_effects(&session.engine)
            {
                return Err(NativePlatformError::InputCapacity.into());
            }
            let prepared_native = session
                .native
                .as_ref()
                .map(|native| {
                    native.prepare_native_commit(
                        prepared.candidate_engine(),
                        workspace_root_continuations(prepared.transition()),
                    )
                })
                .transpose()?;
            let presentation = session.presentation.prepare_emissions(
                prepared.transition(),
                &submitted_presentation,
                &encoding_surfaces,
                prepared.candidate_engine(),
            )?;
            let effects = session
                .abandoned_native_effects
                .prepare_emissions(prepared.transition().platform_effects())?;
            (
                prepared.commit(&mut session.engine)?,
                presentation,
                effects,
                prepared_native,
            )
        };
        session.committed_source_sequence = next_source_sequence;
        let mut native_commit =
            prepared_native
                .zip(session.native.as_mut())
                .map(|(prepared, native)| {
                    let mut committed = prepared.commit(native);
                    committed.input_pending |= native_input_pending;
                    committed
                });
        let surface_presentation_commits = session.presentation.commit_observation(
            &session.engine,
            &transition,
            &submitted_presentation,
        );
        let painted_outputs = presentation.commit(&mut session.presentation);
        let native_effects = effects.commit();
        if let Some(committed) = native_commit.as_mut() {
            committed.input_pending |= session.abandoned_native_effects.has_pending()
                || session.presentation.has_unrecorded_backend_report();
        }
        let mut report = HostFrameReport::from_transition(
            frame_attempt,
            &transition,
            painted_outputs,
            surface_presentation_commits,
            native_commit,
            native_effects,
        );
        report
            .repaint_surfaces
            .extend(session.engine.surfaces_requiring_presentation_paint());
        report.repaint_surfaces.sort_unstable();
        report.repaint_surfaces.dedup();
        Ok(report)
    }

    pub(super) fn append(&mut self, input: EngineInput) -> Result<(), DockspaceRuntimeError> {
        self.append_identified(input).map(|_| ())
    }

    fn append_identified(
        &mut self,
        input: EngineInput,
    ) -> Result<SourceSequence, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        let sequence = self.next_application_source_sequence()?;
        self.frame
            .append_input(APPLICATION_INPUT_SOURCE, sequence, input)?;
        Ok(sequence)
    }

    fn begin_configuration_input(&mut self) -> Result<SourceSequence, DockspaceRuntimeError> {
        self.ensure_semantic_input_allowed()?;
        self.complete_pointer_input()?;
        self.next_application_source_sequence()
    }

    pub(super) fn next_application_source_sequence(
        &mut self,
    ) -> Result<SourceSequence, DockspaceRuntimeError> {
        self.next_source_sequence = self
            .next_source_sequence
            .checked_add(1)
            .ok_or_else(DockspaceRuntimeError::source_sequence_exhausted)?;
        Ok(SourceSequence::new(self.next_source_sequence))
    }

    pub(super) fn ensure_semantic_input_allowed(&self) -> Result<(), DockspaceRuntimeError> {
        if self.encoding_started.get() {
            return Err(crate::engine::CoreHostFrameError::InputAfterPresentation.into());
        }
        if self.frame.input_terminated_for_overload() {
            return Err(crate::engine::CoreHostFrameError::InputAfterOverloadCancellation.into());
        }
        if self.native_input_pending {
            return Err(NativePlatformError::InputPending.into());
        }
        if self.replacement_submitted {
            return Err(DockspaceRuntimeError::replacement_already_submitted());
        }
        Ok(())
    }
}
