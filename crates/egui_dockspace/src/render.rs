//! Stateful egui rendering resources independent from docking authority.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use dockspace::backend::engine::{
    CoreHostFrameError, CoreHostPresentationFrame, DockEngine, EngineInput, HostFrameView,
    HostPresentationDisposition, HostPresentationObligation, HostPresentationUnavailableReason,
    PreparedSurfaceContribution, SurfaceContributionPrepareError, SurfaceContributionToken,
};
use dockspace::backend::interaction::InteractionEventKind;
use dockspace::backend::presentation_observation::{
    HostFrameKey, HostInteractionPresentation, HostPresentationEmissionRequest,
    HostPresentationOutput, HostPresentationOutputPayload, NativeStagingPresentation,
    PresentedSurfaceAuthority, SurfacePresentationOutputTicket,
};
use dockspace::backend::scene::{SurfaceScene, SurfaceSceneStamp, TabBarSceneId, TabSceneId};
use dockspace::backend::transition::{EngineTransition, InputOutcome, SurfaceContributionOutcome};
use dockspace::backend::viewport_focus::PaneFocusObservation;
use dockspace::command::CommandOutcome;
use dockspace::ids::SurfaceId;
use dockspace::runtime::WorkspaceVersion;
use dockspace::scene_manifest::SurfaceMeasurements;
use egui::{Context, Id, Rect, Ui, ViewportId};
use thiserror::Error;

use crate::error::DockspaceError;
use crate::pane::PaneView;
use crate::projection::{
    EguiSurfaceMeasurementSet, EguiSurfacePaintResources, EguiSurfaceProjection, ProjectionError,
    TabKeyboardFocusContinuation, TabStripKey, TabStripStateMap, build_surface_projection,
    continue_committed_tab_keyboard_focus, load_tab_strip_states, store_tab_strip_states,
};
use crate::receiver::{
    PaintReceiverFingerprint, PaintReceiverLookup, PaintReceiverRegistrations, PaintReceiverStore,
};
use crate::response::{
    DockspaceCapability, DockspaceSurfaceStatus, DockspaceUnavailableReason, SurfaceCommitResponse,
    SurfaceFrameDisposition, SurfacePaintResponse,
};
use crate::style::{DockStyle, DockStyleError};

/// Retained egui-only text and pane availability resources for exact core scenes.
#[derive(Default)]
struct EguiPaintResourceIndex {
    surfaces: BTreeMap<
        SurfaceId,
        Vec<(
            SurfaceSceneStamp,
            SurfaceMeasurements,
            EguiSurfacePaintResources,
        )>,
    >,
}

impl EguiPaintResourceIndex {
    fn get(
        &self,
        surface: SurfaceId,
        stamp: SurfaceSceneStamp,
    ) -> Option<&EguiSurfacePaintResources> {
        self.surfaces
            .get(&surface)?
            .iter()
            .find_map(|(retained, _, resources)| (*retained == stamp).then_some(resources))
    }

    fn measurements(
        &self,
        surface: SurfaceId,
        stamp: SurfaceSceneStamp,
    ) -> Option<&SurfaceMeasurements> {
        self.surfaces
            .get(&surface)?
            .iter()
            .find_map(|(retained, measurements, _)| (*retained == stamp).then_some(measurements))
    }

    fn insert(
        &mut self,
        surface: SurfaceId,
        stamp: SurfaceSceneStamp,
        measurements: SurfaceMeasurements,
        resources: EguiSurfacePaintResources,
    ) {
        let entries = self.surfaces.entry(surface).or_default();
        if let Some((_, retained_measurements, retained_resources)) = entries
            .iter_mut()
            .find(|(retained, _, _)| *retained == stamp)
        {
            *retained_measurements = measurements;
            *retained_resources = resources;
        } else {
            entries.push((stamp, measurements, resources));
        }
    }

    fn retain_core_resources(&mut self, engine: &DockEngine) {
        self.surfaces.retain(|surface, entries| {
            let Some(retained) = engine.scene().retained_plan_stamps(*surface) else {
                return false;
            };
            entries.retain(|(stamp, _, _)| retained.unique().any(|retained| retained == *stamp));
            !entries.is_empty()
        });
    }

    #[cfg(test)]
    fn resource_set_count(&self) -> usize {
        self.surfaces.values().map(Vec::len).sum()
    }
}

pub(crate) struct EguiPaintResourceUpdate {
    pub(crate) stamp: SurfaceSceneStamp,
    pub(crate) measurements: SurfaceMeasurements,
    pub(crate) resources: EguiSurfacePaintResources,
}

#[derive(Clone)]
struct RendererBinding {
    identity: Arc<()>,
    style_revision: u64,
}

struct StagedTabStripStateUpdate {
    context: Context,
    surface: SurfaceId,
    live_surfaces: BTreeSet<SurfaceId>,
    states: TabStripStateMap,
    keyboard_focus_continuations: Vec<TabKeyboardFocusContinuation>,
}

pub(crate) struct PreparedEguiSurfaceProjection {
    pub(crate) measurements: EguiSurfaceMeasurementSet,
    pub(crate) resources: EguiSurfacePaintResources,
    pub(crate) status: DockspaceSurfaceStatus,
    binding: RendererBinding,
    tab_strip_states: StagedTabStripStateUpdate,
}

pub(crate) enum EguiSurfaceContribution {
    Prepared(PreparedSurfaceContribution),
    Retained(SurfaceContributionToken),
    Recorded,
    Submitted,
}

/// Requested publication semantics for one actually painted surface draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EguiSurfacePublicationMode {
    /// Publish measurements or retained authority without claiming final presentation.
    PaintOnly,
    /// Record the exact painted result in the core host frame.
    EmitPresentedOutput,
}

enum EguiSurfacePublication {
    Pending(EguiSurfacePublicationMode),
    PaintOnly,
    Presented(HostPresentationEmissionRequest),
}

/// One actually painted native staging slot awaiting transition validation.
pub(crate) struct EguiNativeStagingPublication {
    presentation: NativeStagingPresentation,
    request: HostPresentationEmissionRequest,
}

impl EguiNativeStagingPublication {
    pub(crate) const fn new(
        presentation: NativeStagingPresentation,
        request: HostPresentationEmissionRequest,
    ) -> Self {
        Self {
            presentation,
            request,
        }
    }
}

trait EguiCorePaintStage {
    fn view(&self) -> HostFrameView<'_>;

    fn resolve_presentation_obligation(
        &mut self,
        obligation: HostPresentationObligation,
        disposition: HostPresentationDisposition,
    ) -> Result<Option<HostPresentationEmissionRequest>, CoreHostFrameError>;

    fn record_output(
        &mut self,
        obligation: HostPresentationObligation,
        interaction: HostInteractionPresentation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError>;

    fn record_painted_contribution(
        &mut self,
        obligation: HostPresentationObligation,
        token: SurfaceContributionToken,
        interaction: HostInteractionPresentation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError>;

    fn push_contribution(
        &mut self,
        contribution: PreparedSurfaceContribution,
    ) -> Result<(), CoreHostFrameError>;
}

impl EguiCorePaintStage for CoreHostPresentationFrame {
    fn view(&self) -> HostFrameView<'_> {
        CoreHostPresentationFrame::view(self)
    }

    fn resolve_presentation_obligation(
        &mut self,
        obligation: HostPresentationObligation,
        disposition: HostPresentationDisposition,
    ) -> Result<Option<HostPresentationEmissionRequest>, CoreHostFrameError> {
        CoreHostPresentationFrame::resolve_presentation_obligation(self, obligation, disposition)
    }

    fn record_output(
        &mut self,
        obligation: HostPresentationObligation,
        interaction: HostInteractionPresentation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError> {
        self.resolve_presentation_obligation(
            obligation,
            HostPresentationDisposition::Painted(interaction),
        )
        .map(|request| request.expect("Painted obligation always stages one emission"))
    }

    fn record_painted_contribution(
        &mut self,
        obligation: HostPresentationObligation,
        token: SurfaceContributionToken,
        interaction: HostInteractionPresentation,
    ) -> Result<HostPresentationEmissionRequest, CoreHostFrameError> {
        self.record_painted_surface_contribution(obligation, token, interaction)
    }

    fn push_contribution(
        &mut self,
        contribution: PreparedSurfaceContribution,
    ) -> Result<(), CoreHostFrameError> {
        self.push_surface_contribution(contribution)
    }
}

/// Owned result of measuring and painting one frozen egui surface.
///
/// The draft borrows neither the core frame nor its renderer. It is bound to
/// the exact renderer instance and style revision which created it, and can be
/// accepted only after the matching core host frame commits successfully.
pub(crate) struct EguiSurfaceDraft {
    surface: SurfaceId,
    paint: Option<SurfacePaintResponse>,
    measurements: Option<EguiSurfaceMeasurementSet>,
    resources: Option<EguiSurfacePaintResources>,
    resource_update: Option<EguiPaintResourceUpdate>,
    contribution: EguiSurfaceContribution,
    publication: EguiSurfacePublication,
    unavailable_reason: HostPresentationUnavailableReason,
    receiver_registrations: Option<PaintReceiverRegistrations>,
    pointer_receivers_current: bool,
    painted_interaction: HostInteractionPresentation,
    superseded: bool,
    semantic_inputs: Vec<StagedSemanticInput>,
    tab_strip_states: Option<StagedTabStripStateUpdate>,
    binding: RendererBinding,
    pane_focus_observation: Option<PaneFocusObservation>,
}

/// Causal position of one non-pointer semantic input within a captured egui event batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SemanticInputPosition {
    /// Reduce at the boundary immediately preceding this exact raw egui event.
    RawEvent(usize),
    /// Reduce after the batch because this is an adapter-owned continuation.
    PostBatchContinuation,
    /// Reduce after the batch because this is a final state observation.
    PostBatchObservation,
}

/// One semantic input retained with its exact position relative to pointer edges.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct StagedSemanticInput {
    position: SemanticInputPosition,
    input: EngineInput,
}

impl StagedSemanticInput {
    pub(crate) const fn at_raw_event(raw_event_index: usize, input: EngineInput) -> Self {
        Self {
            position: SemanticInputPosition::RawEvent(raw_event_index),
            input,
        }
    }

    pub(crate) const fn post_batch_continuation(input: EngineInput) -> Self {
        Self {
            position: SemanticInputPosition::PostBatchContinuation,
            input,
        }
    }

    pub(crate) const fn post_batch_observation(input: EngineInput) -> Self {
        Self {
            position: SemanticInputPosition::PostBatchObservation,
            input,
        }
    }

    pub(crate) const fn position(&self) -> SemanticInputPosition {
        self.position
    }

    pub(crate) fn into_input(self) -> EngineInput {
        self.input
    }
}

/// Successfully accepted renderer sidecars for one committed core frame.
pub(crate) struct EguiFrameAcceptance {
    surfaces: BTreeMap<SurfaceId, SurfaceCommitResponse>,
    presentation_outputs: Vec<HostPresentationOutput>,
    contribution_rejected: bool,
}

struct PreparedEguiSurfaceAcceptance {
    outcome: SurfaceContributionOutcome,
    output: Option<HostPresentationOutput>,
    draft: EguiSurfaceDraft,
}

/// Fully validated renderer sidecars awaiting an infallible apply.
pub(crate) struct PreparedEguiFrameAcceptance {
    renderer_identity: Arc<()>,
    renderer_style_revision: u64,
    surfaces: Vec<PreparedEguiSurfaceAcceptance>,
    native_staging_outputs: Vec<HostPresentationOutput>,
    committed_selections: BTreeSet<(dockspace::ids::NodeId, dockspace::ids::ItemId)>,
    semantic_focus_requests: Vec<TabSceneId>,
}

impl PreparedEguiFrameAcceptance {
    pub(crate) fn presentation_surfaces(&self) -> impl Iterator<Item = SurfaceId> + '_ {
        let surfaces = self
            .surfaces
            .iter()
            .filter(|prepared| prepared.output.is_some())
            .map(|prepared| prepared.draft.surface);
        surfaces.chain(
            self.native_staging_outputs
                .iter()
                .map(|output| output.surface()),
        )
    }

    pub(crate) fn presentation_outputs(&self) -> impl Iterator<Item = HostPresentationOutput> + '_ {
        self.surfaces
            .iter()
            .filter_map(|prepared| prepared.output)
            .chain(self.native_staging_outputs.iter().copied())
    }

    /// Applies this renderer-bound capability without another failure boundary.
    pub(crate) fn validate_renderer(
        &self,
        renderer: &EguiDockRenderer,
    ) -> Result<(), DockspaceError> {
        let surface = self
            .surfaces
            .first()
            .map(|prepared| prepared.draft.surface)
            .or_else(|| {
                self.native_staging_outputs
                    .first()
                    .map(|output| output.surface())
            })
            .expect("a prepared renderer acceptance retains at least one physical output");
        if !Arc::ptr_eq(&self.renderer_identity, &renderer.identity) {
            return Err(DockspaceError::from_detail(
                EguiRendererError::RendererBindingMismatch { surface },
            ));
        }
        if self.renderer_style_revision != renderer.style_revision {
            return Err(DockspaceError::from_detail(
                EguiRendererError::RendererStyleRevisionMismatch {
                    surface,
                    draft_revision: self.renderer_style_revision,
                    renderer_revision: renderer.style_revision,
                },
            ));
        }
        Ok(())
    }

    /// Applies this renderer-bound capability without another failure boundary.
    pub(crate) fn commit(
        self,
        renderer: &mut EguiDockRenderer,
        engine: &DockEngine,
    ) -> EguiFrameAcceptance {
        let Self {
            renderer_identity,
            renderer_style_revision,
            surfaces: prepared_surfaces,
            native_staging_outputs,
            committed_selections,
            semantic_focus_requests,
        } = self;
        assert!(Arc::ptr_eq(&renderer_identity, &renderer.identity));
        assert_eq!(renderer_style_revision, renderer.style_revision);
        let mut semantic_focus_by_surface = BTreeMap::<SurfaceId, Vec<TabSceneId>>::new();
        for tab in semantic_focus_requests {
            let Some(owner) = engine.workspace().presentation_for_root(tab.root) else {
                continue;
            };
            let surface = match owner {
                dockspace::backend::scene::RootPresentationOwner::Main { surface }
                | dockspace::backend::scene::RootPresentationOwner::Contained { surface, .. } => {
                    surface
                }
            };
            semantic_focus_by_surface
                .entry(surface)
                .or_default()
                .push(tab);
        }
        let mut surfaces = BTreeMap::new();
        let mut presentation_outputs = Vec::new();
        let mut contribution_rejected = false;
        for prepared in prepared_surfaces {
            contribution_rejected |= matches!(
                prepared.outcome,
                SurfaceContributionOutcome::Rejected { .. }
            );
            let surface = prepared.draft.surface;
            let output = prepared.output;
            let response = renderer.accept_surface(
                engine,
                &committed_selections,
                semantic_focus_by_surface
                    .get(&surface)
                    .map_or(&[], Vec::as_slice),
                prepared.outcome,
                output,
                prepared.draft,
            );
            if let Some(output) = output {
                presentation_outputs.push(output);
            }
            surfaces.insert(surface, response);
        }
        presentation_outputs.extend(native_staging_outputs);
        renderer.reconcile_core_retention(engine);
        EguiFrameAcceptance {
            surfaces,
            presentation_outputs,
            contribution_rejected,
        }
    }
}

impl EguiFrameAcceptance {
    #[cfg(test)]
    #[must_use]
    pub(crate) const fn surfaces(&self) -> &BTreeMap<SurfaceId, SurfaceCommitResponse> {
        &self.surfaces
    }

    pub(crate) fn into_parts(
        self,
    ) -> (
        BTreeMap<SurfaceId, SurfaceCommitResponse>,
        Vec<HostPresentationOutput>,
        bool,
    ) {
        (
            self.surfaces,
            self.presentation_outputs,
            self.contribution_rejected,
        )
    }
}

/// Typed failure while staging or accepting owned egui renderer drafts.
#[derive(Debug, Error)]
pub enum EguiRendererError {
    /// The core rejected an actual post-paint presentation record.
    #[error("core host frame rejected an egui surface draft: {0}")]
    CoreHostFrame(#[from] CoreHostFrameError),
    /// The core could not prepare a retained surface contribution.
    #[error("core could not prepare a retained egui surface contribution: {0}")]
    SurfaceContributionPrepare(#[from] SurfaceContributionPrepareError),
    /// Publication was staged more than once for one surface draft.
    #[error("surface {surface} publication was already staged")]
    PublicationAlreadyStaged { surface: SurfaceId },
    /// A non-painted draft attempted to record an actual presentation output.
    #[error("surface {surface} has no painted output to publish")]
    PaintedOutputMissing { surface: SurfaceId },
    /// A retained contribution reached submission before publication was resolved.
    #[error("surface {surface} retained contribution publication is unresolved")]
    RetainedContributionUnresolved { surface: SurfaceId },
    /// A contribution was submitted more than once.
    #[error("surface {surface} contribution was already submitted")]
    ContributionAlreadySubmitted { surface: SurfaceId },
    /// A draft was accepted by a renderer other than the renderer which created it.
    #[error("surface {surface} draft belongs to another egui renderer instance")]
    RendererBindingMismatch { surface: SurfaceId },
    /// A draft was painted with a renderer style revision that is no longer current.
    #[error(
        "surface {surface} draft style revision {draft_revision} does not match renderer revision {renderer_revision}"
    )]
    RendererStyleRevisionMismatch {
        surface: SurfaceId,
        draft_revision: u64,
        renderer_revision: u64,
    },
    /// A prepared style replacement belongs to another renderer instance.
    #[error("prepared style replacement belongs to another egui renderer instance")]
    StyleRendererBindingMismatch,
    /// Renderer style changed after a replacement candidate was prepared.
    #[error(
        "prepared style revision {prepared_revision} does not match renderer revision {renderer_revision}"
    )]
    PreparedStyleRevisionMismatch {
        prepared_revision: u64,
        renderer_revision: u64,
    },
    /// Renderer style revision cannot advance without wrapping.
    #[error("egui renderer style revision exhausted")]
    StyleRevisionExhausted,
    /// The transition omitted one exact surface contribution outcome.
    #[error("committed transition omitted surface contribution outcome for {surface}")]
    SurfaceOutcomeMissing { surface: SurfaceId },
    /// The transition supplied one surface contribution outcome more than once.
    #[error("committed transition repeated surface contribution outcome for {surface}")]
    SurfaceOutcomeDuplicate { surface: SurfaceId },
    /// The transition supplied an outcome for which no renderer draft exists.
    #[error("committed transition contains unexpected surface outcome for {surface}")]
    SurfaceOutcomeUnexpected { surface: SurfaceId },
    /// A ready contribution did not retain the exact adapter measurements it submitted.
    #[error("ready surface contribution for {surface} has no adapter measurements")]
    ReadyMeasurementsMissing { surface: SurfaceId },
    /// An emitted draft request was not settled by the successful transition.
    #[error("committed transition omitted presented output for surface {surface}")]
    PresentationEmissionMissing { surface: SurfaceId },
    /// The transition settled one presentation request more than once.
    #[error("committed transition repeated a presentation emission request")]
    PresentationEmissionDuplicate,
    /// The transition emitted an output which no draft requested.
    #[error("committed transition contains an unexpected presentation emission")]
    PresentationEmissionUnexpected,
    /// A staging emission carried a different core staging request.
    #[error(
        "native staging output for surface {surface} carried {submitted:?}, expected {expected:?}"
    )]
    NativeStagingPresentationMismatch {
        surface: SurfaceId,
        expected: NativeStagingPresentation,
        submitted: HostPresentationOutputPayload,
    },
    /// The settled output belongs to another logical surface.
    #[error(
        "presented output for surface {output_surface} cannot settle draft for surface {draft_surface}"
    )]
    PresentationSurfaceMismatch {
        draft_surface: SurfaceId,
        output_surface: SurfaceId,
    },
    /// A draft reached acceptance without resolving its publication mode.
    #[error("surface {surface} publication was not staged before acceptance")]
    PublicationPending { surface: SurfaceId },
    /// Two egui passes assigned one exact raw event to different semantic inputs.
    #[error(
        "surface {surface} assigned raw event {raw_event_index} to conflicting semantic inputs across egui passes"
    )]
    MultipassSemanticInputConflict {
        /// Surface whose repeated callback produced the conflict.
        surface: SurfaceId,
        /// Exact raw event position claimed by both semantic inputs.
        raw_event_index: usize,
    },
    /// The map key used to submit a draft does not match its owned surface.
    #[error("surface draft map key {key} does not match owned surface {draft_surface}")]
    DraftSurfaceKeyMismatch {
        key: SurfaceId,
        draft_surface: SurfaceId,
    },
}

impl PreparedEguiSurfaceProjection {
    pub(crate) const fn measurement_set(&self) -> &EguiSurfaceMeasurementSet {
        &self.measurements
    }

    pub(crate) const fn status(&self) -> DockspaceSurfaceStatus {
        self.status
    }

    pub(crate) fn paint_parts(&mut self) -> (&EguiSurfacePaintResources, &mut TabStripStateMap) {
        (&self.resources, &mut self.tab_strip_states.states)
    }
}

impl EguiSurfaceDraft {
    pub(crate) fn painted(
        mut projection: PreparedEguiSurfaceProjection,
        paint: SurfacePaintResponse,
        resource_update: Option<EguiPaintResourceUpdate>,
        contribution: EguiSurfaceContribution,
        publication_mode: EguiSurfacePublicationMode,
        receiver_registrations: Option<PaintReceiverRegistrations>,
        pointer_receivers_current: bool,
        painted_interaction: HostInteractionPresentation,
        semantic_inputs: Vec<StagedSemanticInput>,
    ) -> Self {
        let surface = paint.surface();
        projection.tab_strip_states.keyboard_focus_continuations = projection
            .tab_strip_states
            .states
            .take_keyboard_focus_continuations();
        Self {
            surface,
            paint: Some(paint),
            measurements: Some(projection.measurements),
            resources: Some(projection.resources),
            resource_update,
            contribution,
            publication: EguiSurfacePublication::Pending(publication_mode),
            unavailable_reason: HostPresentationUnavailableReason::FinalPresentationUnobservable,
            receiver_registrations,
            pointer_receivers_current,
            painted_interaction,
            superseded: false,
            semantic_inputs,
            tab_strip_states: Some(projection.tab_strip_states),
            binding: projection.binding,
            pane_focus_observation: None,
        }
    }

    fn unavailable(
        surface: SurfaceId,
        contribution: PreparedSurfaceContribution,
        binding: RendererBinding,
    ) -> Self {
        Self {
            surface,
            paint: None,
            measurements: None,
            resources: None,
            resource_update: None,
            contribution: EguiSurfaceContribution::Prepared(contribution),
            publication: EguiSurfacePublication::PaintOnly,
            unavailable_reason: HostPresentationUnavailableReason::OutputNotProduced,
            receiver_registrations: None,
            pointer_receivers_current: false,
            painted_interaction: HostInteractionPresentation::default(),
            superseded: false,
            semantic_inputs: Vec::new(),
            tab_strip_states: None,
            binding,
            pane_focus_observation: None,
        }
    }

    /// Returns the exact logical surface represented by this draft.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the surface paint response, if the host actually painted it.
    #[must_use]
    pub const fn paint(&self) -> Option<&SurfacePaintResponse> {
        self.paint.as_ref()
    }

    pub(crate) const fn publication_is_pending(&self) -> bool {
        matches!(self.publication, EguiSurfacePublication::Pending(_))
    }

    /// Records publication only after this draft contains a real painted output.
    ///
    /// The core frame mints and stores the exact request. Callers choose only the
    /// publication mode and cannot inject an unrelated request into the draft.
    pub(crate) fn stage_painted_output(
        &mut self,
        frame: &mut CoreHostPresentationFrame,
        obligation: HostPresentationObligation,
    ) -> Result<(), DockspaceError> {
        self.try_stage_painted_output(frame, obligation)
            .map_err(DockspaceError::from_detail)
    }

    fn try_stage_painted_output<F>(
        &mut self,
        frame: &mut F,
        obligation: HostPresentationObligation,
    ) -> Result<(), EguiRendererError>
    where
        F: EguiCorePaintStage,
    {
        let EguiSurfacePublication::Pending(mode) = self.publication else {
            return Err(EguiRendererError::PublicationAlreadyStaged {
                surface: self.surface,
            });
        };
        if self.paint.is_none()
            || (mode == EguiSurfacePublicationMode::EmitPresentedOutput
                && self.receiver_registrations.is_none())
        {
            return Err(EguiRendererError::PaintedOutputMissing {
                surface: self.surface,
            });
        }

        let expected_interaction = frame
            .view()
            .presentation_interaction(self.surface)
            .unwrap_or_default();
        let mode = if mode == EguiSurfacePublicationMode::EmitPresentedOutput
            && self.painted_interaction != expected_interaction
        {
            self.unavailable_reason = HostPresentationUnavailableReason::TransientVisualNotPainted;
            EguiSurfacePublicationMode::PaintOnly
        } else {
            mode
        };

        match mode {
            EguiSurfacePublicationMode::PaintOnly => {
                if let EguiSurfaceContribution::Retained(token) = self.contribution {
                    let contribution = frame.view().prepare_surface_retained_contribution(token)?;
                    self.contribution = EguiSurfaceContribution::Prepared(contribution);
                }
                frame.resolve_presentation_obligation(
                    obligation,
                    HostPresentationDisposition::Unavailable(self.unavailable_reason),
                )?;
                self.publication = EguiSurfacePublication::PaintOnly;
            }
            EguiSurfacePublicationMode::EmitPresentedOutput => {
                let request = match self.contribution {
                    EguiSurfaceContribution::Prepared(_) => {
                        frame.record_output(obligation, self.painted_interaction)?
                    }
                    EguiSurfaceContribution::Retained(token) => {
                        let request = frame.record_painted_contribution(
                            obligation,
                            token,
                            self.painted_interaction,
                        )?;
                        self.contribution = EguiSurfaceContribution::Recorded;
                        request
                    }
                    EguiSurfaceContribution::Recorded | EguiSurfaceContribution::Submitted => {
                        return Err(EguiRendererError::ContributionAlreadySubmitted {
                            surface: self.surface,
                        });
                    }
                };
                self.publication = EguiSurfacePublication::Presented(request);
            }
        }
        Ok(())
    }

    pub(crate) fn force_paint_only(&mut self) {
        if matches!(self.publication, EguiSurfacePublication::Pending(_)) {
            self.publication =
                EguiSurfacePublication::Pending(EguiSurfacePublicationMode::PaintOnly);
        }
    }

    pub(crate) fn supersede_with_unavailable_contribution(
        &mut self,
        contribution: PreparedSurfaceContribution,
    ) {
        self.contribution = EguiSurfaceContribution::Prepared(contribution);
        self.force_paint_only();
        self.unavailable_reason = HostPresentationUnavailableReason::SupersededBeforePublication;
        self.measurements = None;
        self.resources = None;
        self.resource_update = None;
        self.receiver_registrations = None;
        self.pointer_receivers_current = false;
        self.superseded = true;
        self.semantic_inputs.clear();
        if let Some(paint) = self.paint.as_mut() {
            paint.interactions_current = false;
            paint.contained_capability = DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
            paint.pane_focus_capability = DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
        }
        self.pane_focus_observation = None;
    }

    /// Stages this draft's exact contribution into the matching core frame.
    pub(crate) fn stage_core_contribution(
        &mut self,
        frame: &mut CoreHostPresentationFrame,
    ) -> Result<(), DockspaceError> {
        self.try_stage_core_contribution(frame)
            .map_err(DockspaceError::from_detail)
    }

    fn try_stage_core_contribution<F>(&mut self, frame: &mut F) -> Result<(), EguiRendererError>
    where
        F: EguiCorePaintStage,
    {
        let contribution =
            std::mem::replace(&mut self.contribution, EguiSurfaceContribution::Submitted);
        match contribution {
            EguiSurfaceContribution::Prepared(contribution) => {
                frame.push_contribution(contribution)?;
                Ok(())
            }
            EguiSurfaceContribution::Recorded => Ok(()),
            EguiSurfaceContribution::Retained(token) => {
                self.contribution = EguiSurfaceContribution::Retained(token);
                Err(EguiRendererError::RetainedContributionUnresolved {
                    surface: self.surface,
                })
            }
            EguiSurfaceContribution::Submitted => {
                Err(EguiRendererError::ContributionAlreadySubmitted {
                    surface: self.surface,
                })
            }
        }
    }

    pub(crate) const fn pointer_receivers_current(&self) -> bool {
        self.pointer_receivers_current
    }

    pub(crate) const fn receiver_registrations(&self) -> Option<&PaintReceiverRegistrations> {
        self.receiver_registrations.as_ref()
    }

    pub(crate) fn take_semantic_inputs(&mut self) -> Vec<StagedSemanticInput> {
        std::mem::take(&mut self.semantic_inputs)
    }

    pub(crate) fn stage_semantic_input(&mut self, input: StagedSemanticInput) {
        self.semantic_inputs.push(input);
    }

    pub(crate) fn preserve_prior_raw_event_inputs(
        &mut self,
        previous: &mut Self,
    ) -> Result<(), EguiRendererError> {
        merge_multipass_semantic_inputs(
            self.surface,
            &mut self.semantic_inputs,
            previous.take_semantic_inputs(),
        )
    }

    pub(crate) fn set_pane_focus_observation(
        &mut self,
        pane_focus_observation: Option<PaneFocusObservation>,
    ) {
        self.pane_focus_observation = pane_focus_observation;
    }

    pub(crate) const fn pane_focus_observation(&self) -> Option<PaneFocusObservation> {
        self.pane_focus_observation
    }
}

fn merge_multipass_semantic_inputs(
    surface: SurfaceId,
    current: &mut Vec<StagedSemanticInput>,
    previous: Vec<StagedSemanticInput>,
) -> Result<(), EguiRendererError> {
    let mut raw_inputs = BTreeMap::<usize, StagedSemanticInput>::new();
    let mut post_batch = Vec::new();
    for input in std::mem::take(current) {
        match input.position() {
            SemanticInputPosition::RawEvent(raw_event_index) => {
                insert_multipass_raw_input(surface, raw_event_index, input, &mut raw_inputs)?;
            }
            SemanticInputPosition::PostBatchContinuation
            | SemanticInputPosition::PostBatchObservation => post_batch.push(input),
        }
    }
    for input in previous {
        let SemanticInputPosition::RawEvent(raw_event_index) = input.position() else {
            continue;
        };
        insert_multipass_raw_input(surface, raw_event_index, input, &mut raw_inputs)?;
    }
    current.extend(raw_inputs.into_values());
    current.extend(post_batch);
    Ok(())
}

fn insert_multipass_raw_input(
    surface: SurfaceId,
    raw_event_index: usize,
    input: StagedSemanticInput,
    inputs: &mut BTreeMap<usize, StagedSemanticInput>,
) -> Result<(), EguiRendererError> {
    match inputs.entry(raw_event_index) {
        std::collections::btree_map::Entry::Vacant(slot) => {
            slot.insert(input);
            Ok(())
        }
        std::collections::btree_map::Entry::Occupied(slot) if slot.get() == &input => Ok(()),
        std::collections::btree_map::Entry::Occupied(_) => {
            Err(EguiRendererError::MultipassSemanticInputConflict {
                surface,
                raw_event_index,
            })
        }
    }
}

#[cfg(test)]
mod multipass_semantic_input_tests {
    use super::*;
    use dockspace::command::ContentCloseTarget;
    use dockspace::ids::ItemId;

    fn close_input(raw_event_index: usize, item: u64) -> StagedSemanticInput {
        StagedSemanticInput::at_raw_event(
            raw_event_index,
            EngineInput::RequestContentClose {
                expected: WorkspaceVersion::default(),
                target: ContentCloseTarget::Item(ItemId::new(item)),
            },
        )
    }

    #[test]
    fn repeated_pass_keeps_one_affine_raw_event_input() {
        let input = close_input(3, 1);
        let mut current = vec![input.clone()];
        merge_multipass_semantic_inputs(SurfaceId::new(1), &mut current, vec![input])
            .expect("the same event/action pair is one semantic input");
        assert_eq!(current, [close_input(3, 1)]);
    }

    #[test]
    fn repeated_pass_rejects_retargeting_one_raw_event() {
        let mut current = vec![close_input(3, 2)];
        assert!(matches!(
            merge_multipass_semantic_inputs(
                SurfaceId::new(1),
                &mut current,
                vec![close_input(3, 1)],
            ),
            Err(EguiRendererError::MultipassSemanticInputConflict {
                surface,
                raw_event_index: 3,
            }) if surface == SurfaceId::new(1)
        ));
    }
}

/// Stateful egui renderer resources for one docking workspace.
///
/// This type owns only egui identity, style, and retained paint resources. It
/// deliberately owns no [`DockEngine`], platform window, pointer provider, or
/// focus state, so a host can pair it with exactly one external docking
/// authority.
pub(crate) struct EguiDockRenderer {
    identity: Arc<()>,
    id: Id,
    style: DockStyle,
    style_revision: u64,
    paint_resources: EguiPaintResourceIndex,
    receiver_store: PaintReceiverStore,
}

/// Fully validated renderer style replacement awaiting an infallible commit.
pub(crate) struct PreparedStyleReplacement {
    renderer_identity: Arc<()>,
    expected_revision: u64,
    next_revision: u64,
    style: DockStyle,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RendererSidecarDiagnostics {
    pub(crate) paint_resource_set_count: usize,
    pub(crate) receiver_presentation_count: usize,
}

impl EguiDockRenderer {
    /// Creates renderer state for one stable egui identity.
    ///
    /// # Errors
    ///
    /// Returns [`DockStyleError`] when any style metric is invalid.
    pub fn new(id: Id, style: DockStyle) -> Result<Self, DockStyleError> {
        style.validate()?;
        Ok(Self {
            identity: Arc::new(()),
            id,
            style,
            style_revision: 0,
            paint_resources: EguiPaintResourceIndex::default(),
            receiver_store: PaintReceiverStore::default(),
        })
    }

    /// Returns the stable egui identity used to scope every renderer widget.
    #[must_use]
    pub const fn id(&self) -> Id {
        self.id
    }

    /// Returns the current fixed renderer geometry and colors.
    #[must_use]
    pub const fn style(&self) -> &DockStyle {
        &self.style
    }

    #[cfg(test)]
    pub(crate) fn sidecar_diagnostics(&self) -> RendererSidecarDiagnostics {
        RendererSidecarDiagnostics {
            paint_resource_set_count: self.paint_resources.resource_set_count(),
            receiver_presentation_count: self.receiver_store.presentation_count(),
        }
    }

    pub(crate) fn reconcile_core_retention(&mut self, engine: &DockEngine) {
        self.paint_resources.retain_core_resources(engine);
        let retention = engine.runtime_retention_manifest();
        self.receiver_store.retain(retention.presentation());
    }

    /// Resolves an observed receiver against one exact retained paint generation.
    #[must_use]
    pub fn resolve_receiver(
        &self,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
        viewport: ViewportId,
        widget_pass: u64,
        fingerprint: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        self.receiver_store
            .region_for(output, authority, viewport, widget_pass, fingerprint)
    }

    pub(crate) fn resolve_receiver_for_latest_presented_pass(
        &self,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
        viewport: ViewportId,
        before_widget_pass_nr: u64,
        fingerprint: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        self.receiver_store.region_for_latest_presented_pass(
            output,
            authority,
            viewport,
            before_widget_pass_nr,
            fingerprint,
        )
    }

    pub(crate) fn resolve_receiver_for_emission(
        &self,
        output: SurfacePresentationOutputTicket,
        emission: HostFrameKey,
        viewport: ViewportId,
        widget_pass: u64,
        fingerprint: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        self.receiver_store.region_for_emission(
            output,
            emission,
            viewport,
            widget_pass,
            fingerprint,
        )
    }

    #[cfg(test)]
    pub(crate) fn first_receiver_registration_for(
        &self,
        authority: PresentedSurfaceAuthority,
    ) -> Option<(ViewportId, u64, PaintReceiverFingerprint)> {
        self.receiver_store.first_registration_for(authority)
    }

    pub(crate) fn ensure_style_revision_available(&self) -> Result<(), EguiRendererError> {
        self.style_revision
            .checked_add(1)
            .map(|_| ())
            .ok_or(EguiRendererError::StyleRevisionExhausted)
    }

    pub(crate) fn prepare_style_replacement(
        &self,
        style: DockStyle,
    ) -> Result<PreparedStyleReplacement, EguiRendererError> {
        let next_revision = self
            .style_revision
            .checked_add(1)
            .ok_or(EguiRendererError::StyleRevisionExhausted)?;
        Ok(PreparedStyleReplacement {
            renderer_identity: Arc::clone(&self.identity),
            expected_revision: self.style_revision,
            next_revision,
            style,
        })
    }

    pub(crate) fn commit_style_replacement(&mut self, prepared: PreparedStyleReplacement) {
        self.validate_style_replacement(&prepared)
            .expect("a prepared style replacement remains current");
        self.style_revision = prepared.next_revision;
        self.style = prepared.style;
    }

    pub(crate) fn validate_style_replacement(
        &self,
        prepared: &PreparedStyleReplacement,
    ) -> Result<(), EguiRendererError> {
        if !Arc::ptr_eq(&self.identity, &prepared.renderer_identity) {
            return Err(EguiRendererError::StyleRendererBindingMismatch);
        }
        if self.style_revision != prepared.expected_revision {
            return Err(EguiRendererError::PreparedStyleRevisionMismatch {
                prepared_revision: prepared.expected_revision,
                renderer_revision: self.style_revision,
            });
        }
        Ok(())
    }

    pub(crate) fn replace_style(&mut self, style: DockStyle) -> Result<(), EguiRendererError> {
        let prepared = self.prepare_style_replacement(style)?;
        self.commit_style_replacement(prepared);
        Ok(())
    }

    fn binding(&self) -> RendererBinding {
        RendererBinding {
            identity: Arc::clone(&self.identity),
            style_revision: self.style_revision,
        }
    }

    pub(crate) fn prepare_surface_projection(
        &self,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        bounds: Rect,
        ui: &Ui,
        panes: &dyn PaneView,
        source_workspace: WorkspaceVersion,
    ) -> Result<PreparedEguiSurfaceProjection, DockspaceError> {
        let live_surfaces = view
            .workspace()
            .surfaces()
            .map(|(id, _)| id)
            .collect::<BTreeSet<_>>();
        let mut tab_strip_states = load_tab_strip_states(ui.ctx(), self.id);
        tab_strip_states.retain_surface_inventory(live_surfaces.iter().copied());
        let EguiSurfaceProjection {
            measurements,
            resources,
        } = self.project_surface(
            view,
            surface,
            bounds,
            ui,
            panes,
            source_workspace,
            &mut tab_strip_states,
        )?;
        Ok(PreparedEguiSurfaceProjection {
            measurements,
            resources,
            status: Self::surface_status(view, surface),
            binding: self.binding(),
            tab_strip_states: StagedTabStripStateUpdate {
                context: ui.ctx().clone(),
                surface,
                live_surfaces,
                states: tab_strip_states,
                keyboard_focus_continuations: Vec::new(),
            },
        })
    }

    pub(crate) fn unavailable_surface_draft(
        &self,
        surface: SurfaceId,
        contribution: PreparedSurfaceContribution,
    ) -> EguiSurfaceDraft {
        EguiSurfaceDraft::unavailable(surface, contribution, self.binding())
    }

    fn surface_status(view: HostFrameView<'_>, surface: SurfaceId) -> DockspaceSurfaceStatus {
        match view.scene().surface(surface) {
            Some(SurfaceScene::Ready(_)) => DockspaceSurfaceStatus::Ready,
            Some(SurfaceScene::Stale(_)) => DockspaceSurfaceStatus::Stale,
            Some(SurfaceScene::Bootstrap(_)) => DockspaceSurfaceStatus::Bootstrap,
            None => DockspaceSurfaceStatus::Absent,
        }
    }

    pub(crate) fn committed_surface_status(
        &self,
        engine: &DockEngine,
        surface: SurfaceId,
    ) -> DockspaceSurfaceStatus {
        match engine.scene().surface(surface) {
            Some(SurfaceScene::Ready(_)) => DockspaceSurfaceStatus::Ready,
            Some(SurfaceScene::Stale(_)) => DockspaceSurfaceStatus::Stale,
            Some(SurfaceScene::Bootstrap(_)) => DockspaceSurfaceStatus::Bootstrap,
            None => DockspaceSurfaceStatus::Absent,
        }
    }

    pub(crate) fn committed_contained_capability(
        &self,
        engine: &DockEngine,
        surface: SurfaceId,
        measurements: &EguiSurfaceMeasurementSet,
        presentation_authority_available: bool,
    ) -> DockspaceCapability {
        if engine.workspace().surface(surface).is_none() {
            return DockspaceCapability::Unavailable(DockspaceUnavailableReason::SurfaceAbsent);
        }
        if !measurements.bounds.is_positive() {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
        }
        if !engine.policy().allows_contained_floating() {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::ContainedPolicyDisabled,
            );
        }
        if !matches!(
            engine.scene().surface(surface),
            Some(SurfaceScene::Ready(_))
        ) {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::SurfaceBoundsUnavailable,
            );
        }
        if !presentation_authority_available {
            return DockspaceCapability::Unavailable(
                DockspaceUnavailableReason::PresentationSettlementRequired,
            );
        }
        DockspaceCapability::Supported
    }

    pub(crate) fn contribution_measurements(
        &self,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        measurement_set: &EguiSurfaceMeasurementSet,
        resources: &EguiSurfacePaintResources,
    ) -> Result<(Option<SurfaceMeasurements>, Option<EguiPaintResourceUpdate>), DockspaceError>
    {
        let measurements = measurement_set.values.clone();
        if let Some(resource) =
            self.prepare_reusable_projection_resource(view, surface, resources, &measurements)?
        {
            return Ok((None, Some(resource)));
        }
        Ok((Some(measurements), None))
    }

    fn prepare_reusable_projection_resource(
        &self,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        resources: &EguiSurfacePaintResources,
        measurements: &SurfaceMeasurements,
    ) -> Result<Option<EguiPaintResourceUpdate>, DockspaceError> {
        let Some(stamp) = view
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .map(|ready| ready.candidate().stamp())
        else {
            return Ok(None);
        };
        let Some(retained) = self.paint_resources.measurements(surface, stamp) else {
            return Ok(None);
        };
        if retained != measurements {
            return Ok(None);
        }

        Ok(Some(EguiPaintResourceUpdate {
            stamp,
            measurements: measurements.clone(),
            resources: resources.clone(),
        }))
    }

    fn project_surface(
        &self,
        view: HostFrameView<'_>,
        surface: SurfaceId,
        bounds: Rect,
        ui: &Ui,
        panes: &dyn PaneView,
        source_workspace: WorkspaceVersion,
        tab_strip_states: &mut crate::projection::TabStripStateMap,
    ) -> Result<EguiSurfaceProjection, DockspaceError> {
        let requirements = view
            .presentation_requirements()
            .surface(surface)
            .ok_or(ProjectionError::MissingSurface { surface })
            .map_err(DockspaceError::from_detail)?;
        build_surface_projection(
            ui,
            source_workspace,
            requirements,
            tab_strip_states,
            view.workspace(),
            surface,
            bounds,
            panes,
            &self.style,
        )
        .map_err(DockspaceError::from_detail)
    }

    pub(crate) fn retained_paint_resources(
        &self,
        surface: SurfaceId,
        stamp: SurfaceSceneStamp,
    ) -> Option<&EguiSurfacePaintResources> {
        self.paint_resources.get(surface, stamp)
    }

    /// Performs every fallible renderer-side validation without mutating sidecars.
    pub(crate) fn prepare_frame(
        &self,
        transition: &EngineTransition,
        drafts: BTreeMap<SurfaceId, EguiSurfaceDraft>,
        native_staging: BTreeMap<SurfaceId, EguiNativeStagingPublication>,
    ) -> Result<PreparedEguiFrameAcceptance, DockspaceError> {
        self.try_prepare_frame(transition, drafts, native_staging)
            .map_err(DockspaceError::from_detail)
    }

    fn try_prepare_frame(
        &self,
        transition: &EngineTransition,
        drafts: BTreeMap<SurfaceId, EguiSurfaceDraft>,
        native_staging: BTreeMap<SurfaceId, EguiNativeStagingPublication>,
    ) -> Result<PreparedEguiFrameAcceptance, EguiRendererError> {
        let mut outcomes = BTreeMap::new();
        for outcome in transition.surface_contributions() {
            let surface = outcome.surface();
            if outcomes.insert(surface, outcome.clone()).is_some() {
                return Err(EguiRendererError::SurfaceOutcomeDuplicate { surface });
            }
        }
        let mut emissions = BTreeMap::new();
        for emission in transition.presentation_emissions() {
            if emissions
                .insert(emission.request(), emission.output())
                .is_some()
            {
                return Err(EguiRendererError::PresentationEmissionDuplicate);
            }
        }

        let mut requested_emissions = BTreeSet::new();
        for (surface, publication) in &native_staging {
            if !requested_emissions.insert(publication.request) {
                return Err(EguiRendererError::PresentationEmissionDuplicate);
            }
            let output = emissions
                .get(&publication.request)
                .ok_or(EguiRendererError::PresentationEmissionMissing { surface: *surface })?;
            if output.surface() != *surface {
                return Err(EguiRendererError::PresentationSurfaceMismatch {
                    draft_surface: *surface,
                    output_surface: output.surface(),
                });
            }
            if output.payload()
                != (HostPresentationOutputPayload::NativeStaging {
                    presentation: publication.presentation,
                })
            {
                return Err(EguiRendererError::NativeStagingPresentationMismatch {
                    surface: *surface,
                    expected: publication.presentation,
                    submitted: output.payload(),
                });
            }
        }
        for (surface, draft) in &drafts {
            if *surface != draft.surface {
                return Err(EguiRendererError::DraftSurfaceKeyMismatch {
                    key: *surface,
                    draft_surface: draft.surface,
                });
            }
            self.validate_draft_binding(draft)?;
            let outcome = outcomes
                .get(surface)
                .ok_or(EguiRendererError::SurfaceOutcomeMissing { surface: *surface })?;
            if matches!(outcome, SurfaceContributionOutcome::Ready { .. })
                && draft.measurements.is_none()
            {
                return Err(EguiRendererError::ReadyMeasurementsMissing { surface: *surface });
            }
            match draft.publication {
                EguiSurfacePublication::Pending(_) => {
                    return Err(EguiRendererError::PublicationPending { surface: *surface });
                }
                EguiSurfacePublication::PaintOnly => {}
                EguiSurfacePublication::Presented(request) => {
                    if !requested_emissions.insert(request) {
                        return Err(EguiRendererError::PresentationEmissionDuplicate);
                    }
                    let output = emissions.get(&request).ok_or(
                        EguiRendererError::PresentationEmissionMissing { surface: *surface },
                    )?;
                    if output.surface() != *surface {
                        return Err(EguiRendererError::PresentationSurfaceMismatch {
                            draft_surface: *surface,
                            output_surface: output.surface(),
                        });
                    }
                }
            }
        }
        if let Some(surface) = outcomes
            .keys()
            .find(|surface| !drafts.contains_key(surface))
            .copied()
        {
            return Err(EguiRendererError::SurfaceOutcomeUnexpected { surface });
        }
        if emissions
            .keys()
            .any(|request| !requested_emissions.contains(request))
        {
            return Err(EguiRendererError::PresentationEmissionUnexpected);
        }

        let committed_selections = transition
            .reduced_inputs()
            .iter()
            .filter_map(|input| match input.outcome() {
                InputOutcome::CommandProcessed {
                    outcome: CommandOutcome::Selected { item, tabs, .. },
                    ..
                } => Some((*tabs, *item)),
                _ => None,
            })
            .collect::<BTreeSet<_>>();
        let semantic_focus_requests = transition
            .interaction_events()
            .iter()
            .filter_map(|event| match event.kind() {
                InteractionEventKind::SemanticFocusRequested { tab } => Some(*tab),
                _ => None,
            })
            .collect::<Vec<_>>();

        let mut surfaces = Vec::with_capacity(drafts.len());
        for (surface, draft) in drafts {
            let outcome = outcomes
                .remove(&surface)
                .ok_or(EguiRendererError::SurfaceOutcomeMissing { surface })?;
            let output = match draft.publication {
                EguiSurfacePublication::Presented(request) => emissions.remove(&request),
                EguiSurfacePublication::Pending(_) | EguiSurfacePublication::PaintOnly => None,
            };
            surfaces.push(PreparedEguiSurfaceAcceptance {
                outcome,
                output,
                draft,
            });
        }
        let native_staging_outputs = native_staging
            .into_values()
            .map(|publication| {
                emissions
                    .remove(&publication.request)
                    .expect("validated staging emissions remain available")
            })
            .collect();
        Ok(PreparedEguiFrameAcceptance {
            renderer_identity: Arc::clone(&self.identity),
            renderer_style_revision: self.style_revision,
            surfaces,
            native_staging_outputs,
            committed_selections,
            semantic_focus_requests,
        })
    }

    fn validate_draft_binding(&self, draft: &EguiSurfaceDraft) -> Result<(), EguiRendererError> {
        if !Arc::ptr_eq(&self.identity, &draft.binding.identity) {
            return Err(EguiRendererError::RendererBindingMismatch {
                surface: draft.surface,
            });
        }
        if self.style_revision != draft.binding.style_revision {
            return Err(EguiRendererError::RendererStyleRevisionMismatch {
                surface: draft.surface,
                draft_revision: draft.binding.style_revision,
                renderer_revision: self.style_revision,
            });
        }
        Ok(())
    }

    fn accept_surface(
        &mut self,
        engine: &DockEngine,
        committed_selections: &BTreeSet<(dockspace::ids::NodeId, dockspace::ids::ItemId)>,
        semantic_focus_requests: &[TabSceneId],
        outcome: SurfaceContributionOutcome,
        output: Option<HostPresentationOutput>,
        mut draft: EguiSurfaceDraft,
    ) -> SurfaceCommitResponse {
        let rejected =
            draft.superseded || matches!(outcome, SurfaceContributionOutcome::Rejected { .. });
        let presentation_authority_available = output.is_some();
        if !rejected {
            if let Some(update) = draft.tab_strip_states.as_ref() {
                self.accept_tab_strip_projection(update);
            }
            if let Some(resource) = draft.resource_update.take() {
                self.paint_resources.insert(
                    draft.surface,
                    resource.stamp,
                    resource.measurements,
                    resource.resources,
                );
            }
            if let (
                SurfaceContributionOutcome::Ready { stamp, .. },
                Some(measurements),
                Some(resources),
            ) = (
                &outcome,
                draft.measurements.as_ref(),
                draft.resources.as_ref(),
            ) {
                self.paint_resources.insert(
                    draft.surface,
                    *stamp,
                    measurements.values.clone(),
                    resources.clone(),
                );
            }
            if let (Some(output), Some(registrations)) =
                (output, draft.receiver_registrations.take())
            {
                self.receiver_store.bind(output, registrations);
            }
        }
        if let Some(update) = draft.tab_strip_states.take() {
            self.accept_tab_keyboard_focus_continuations(
                engine,
                committed_selections,
                semantic_focus_requests,
                update,
            );
        }

        let disposition = SurfaceFrameDisposition::Contribution(outcome);
        if let Some(paint) = draft.paint.as_mut() {
            if rejected {
                paint.interactions_current = false;
                paint.surface_status = self.committed_surface_status(engine, draft.surface);
            } else if engine.workspace().surface(draft.surface).is_none() {
                paint.surface_status = DockspaceSurfaceStatus::Absent;
            }
            if let Some(measurements) = draft.measurements.as_ref() {
                paint.contained_capability = self.committed_contained_capability(
                    engine,
                    draft.surface,
                    measurements,
                    presentation_authority_available,
                );
            }
        }
        SurfaceCommitResponse {
            paint: draft.paint,
            disposition,
        }
    }

    fn accept_tab_strip_projection(&self, update: &StagedTabStripStateUpdate) {
        let mut accepted = load_tab_strip_states(&update.context, self.id);
        accepted.retain_surface_inventory(update.live_surfaces.iter().copied());
        accepted.replace_surface_from(update.surface, update.states.clone());
        store_tab_strip_states(&update.context, self.id, accepted);
    }

    fn accept_tab_keyboard_focus_continuations(
        &self,
        engine: &DockEngine,
        committed_selections: &BTreeSet<(dockspace::ids::NodeId, dockspace::ids::ItemId)>,
        semantic_focus_requests: &[TabSceneId],
        update: StagedTabStripStateUpdate,
    ) {
        if update.keyboard_focus_continuations.is_empty() && semantic_focus_requests.is_empty() {
            return;
        }
        let mut accepted = load_tab_strip_states(&update.context, self.id);
        let mut changed = false;
        for continuation in update.keyboard_focus_continuations {
            let key = continuation.key;
            let selection_committed = committed_selections.contains(&(key.node, continuation.item));
            let selected_after = matches!(
                engine.workspace().node(key.node),
                Some(dockspace::graph::Node::Tabs { selected: Some(item), .. })
                    if *item == continuation.item
            );
            let presentation_matches = engine
                .workspace()
                .presentation_for_root(key.root)
                .is_some_and(|owner| match owner {
                    dockspace::backend::scene::RootPresentationOwner::Main { surface }
                    | dockspace::backend::scene::RootPresentationOwner::Contained {
                        surface, ..
                    } => surface == key.surface,
                });
            let source_current = engine
                .workspace()
                .capture_item_source(key.root, key.node, continuation.item)
                .is_ok();
            if selection_committed && selected_after && presentation_matches && source_current {
                changed |= accepted.apply_keyboard_focus_continuation(continuation);
            }
        }
        for tab in semantic_focus_requests {
            let key = TabStripKey::new(
                update.surface,
                TabBarSceneId {
                    root: tab.root,
                    tabs: tab.tabs,
                },
            );
            let selected_after = matches!(
                engine.workspace().node(tab.tabs),
                Some(dockspace::graph::Node::Tabs { selected: Some(item), .. })
                    if *item == tab.item
            );
            if selected_after
                && engine
                    .workspace()
                    .capture_item_source(tab.root, tab.tabs, tab.item)
                    .is_ok()
            {
                changed |= continue_committed_tab_keyboard_focus(&mut accepted, key, tab.item);
            }
        }
        if changed {
            store_tab_strip_states(&update.context, self.id, accepted);
            update.context.request_repaint();
        }
    }
}
