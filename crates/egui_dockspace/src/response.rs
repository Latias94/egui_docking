//! Structured output from explicit egui docking host-frame boundaries.

use std::collections::BTreeMap;

use dockspace::ClosePlan;
use dockspace::error::CommandError;
use dockspace::ids::{ItemId, ReducerCausalOrdinal, SurfaceId};
use dockspace::interaction::InteractionOutcome;
use dockspace::transition::{EngineTransition, InputOutcome, SurfaceContributionOutcome};

/// Truthful availability of one adapter operation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceCapability {
    /// Every application and platform prerequisite is currently available.
    Supported,
    /// The operation is disabled for an explicit, queryable reason.
    Unavailable(DockspaceUnavailableReason),
}

/// Lifecycle state of one logical surface painted during a host frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceSurfaceStatus {
    /// The surface exists and was projected into a sealed, ready scene.
    Ready,
    /// The surface retains a previous paint projection, but current facts cannot authorize input.
    ///
    /// The selected application's pane callback, when available, renders the retained projection
    /// through a disabled child UI while a new exact contribution is collected. Docking chrome
    /// hit testing remains unavailable until that contribution is compiled and acknowledged.
    Stale,
    /// The surface has neither current authority nor a previous paint projection.
    ///
    /// This includes first publication, workspace replacement, and measurement unavailability
    /// before any ready plan existed. A currently measurable pane candidate may still render
    /// through disabled child UIs, but geometry-derived docking input is unavailable.
    Bootstrap,
    /// The surface is not owned by the tick-start workspace roster.
    ///
    /// A strict host frame cannot paint an absent surface. This status remains available to
    /// report a surface removed by the one reducer tick which closed the frame.
    Absent,
}

/// Why an egui adapter operation is unavailable in the current frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DockspaceUnavailableReason {
    /// The current host cannot report terminal presentation settlement.
    ///
    /// Callback-only painting cannot authorize contained interaction because
    /// the core cannot prove that the corresponding receiver graph reached the
    /// screen. Use an explicit outer host that settles its final egui output.
    PresentationSettlementRequired,
    /// Application policy disables contained-floating presentation.
    ContainedPolicyDisabled,
    /// No ready sealed scene can authorize placement on this surface.
    SurfaceBoundsUnavailable,
    /// The requested logical surface is not owned by the current workspace.
    SurfaceAbsent,
    /// The surface has no exact current native binding eligible to report focus.
    PaneFocusBindingUnavailable,
    /// Global platform facts do not prove that this exact native binding is focused.
    PaneFocusWindowNotFocused,
    /// The application did not expose a stable focus target for this pane.
    PaneFocusTargetMissing { item: ItemId },
    /// The application could not authoritatively report this pane's focus state.
    PaneFocusStateUnknown { item: ItemId },
    /// More than one pane on the same surface claimed focus.
    ConflictingPaneFocus,
    /// The adapter exhausted its monotonic pane-focus observation identity domain.
    PaneFocusObservationGenerationExhausted,
}

/// Immediate paint result from one [`crate::DockspaceHostFrame::show_surface`] call.
///
/// This value deliberately contains no reducer transition. Semantic input and measurement facts
/// are staged until the enclosing host frame ends.
#[derive(Clone, Debug)]
pub struct SurfacePaintResponse {
    pub(crate) surface: SurfaceId,
    pub(crate) missing_panes: Vec<ItemId>,
    pub(crate) capture_errors: Vec<CommandError>,
    pub(crate) interactions_current: bool,
    pub(crate) surface_status: DockspaceSurfaceStatus,
    pub(crate) contained_capability: DockspaceCapability,
    pub(crate) pane_focus_capability: DockspaceCapability,
}

impl SurfacePaintResponse {
    /// Returns the painted logical surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns stable item identities missing from the application pane registry.
    #[must_use]
    pub fn missing_panes(&self) -> &[ItemId] {
        &self.missing_panes
    }

    /// Returns exact source/target capture errors observed while registering widgets.
    #[must_use]
    pub fn capture_errors(&self) -> &[CommandError] {
        &self.capture_errors
    }

    /// Returns whether egui's prior-pass hit geometry exactly matched this projection.
    #[must_use]
    pub const fn interactions_current(&self) -> bool {
        self.interactions_current
    }

    /// Returns the exact core lifecycle state used for this paint.
    #[must_use]
    pub const fn surface_status(&self) -> DockspaceSurfaceStatus {
        self.surface_status
    }

    /// Returns current contained-floating tear-off availability.
    #[must_use]
    pub const fn contained_capability(&self) -> DockspaceCapability {
        self.contained_capability
    }

    /// Returns whether this surface can publish exact pane-focus observations.
    #[must_use]
    pub const fn pane_focus_capability(&self) -> DockspaceCapability {
        self.pane_focus_capability
    }
}

/// The terminal host-frame disposition of one tick-start surface slot.
#[derive(Debug)]
pub enum SurfaceFrameDisposition {
    /// A complete contribution was reduced and its typed core result is visible to the host.
    ///
    /// In particular, [`SurfaceContributionOutcome::Rejected`] is a terminal,
    /// caller-visible retry outcome. It never means the adapter retained current
    /// interaction authority for the painted projection.
    Contribution(SurfaceContributionOutcome),
}

impl SurfaceFrameDisposition {
    /// Returns the exact core result for this surface contribution.
    #[must_use]
    pub const fn contribution(&self) -> &SurfaceContributionOutcome {
        match self {
            Self::Contribution(outcome) => outcome,
        }
    }

    /// Returns whether the contribution was superseded before it could install.
    #[must_use]
    pub const fn was_rejected(&self) -> bool {
        matches!(
            self,
            Self::Contribution(SurfaceContributionOutcome::Rejected { .. })
        )
    }
}

/// One surface's paint result and terminal host-frame disposition.
#[derive(Debug)]
pub struct SurfaceCommitResponse {
    pub(crate) paint: Option<SurfacePaintResponse>,
    pub(crate) disposition: SurfaceFrameDisposition,
}

impl SurfaceCommitResponse {
    /// Returns the local paint result when this surface was painted by egui.
    #[must_use]
    pub const fn paint(&self) -> Option<&SurfacePaintResponse> {
        self.paint.as_ref()
    }

    /// Returns the complete core contribution outcome for this frozen slot.
    #[must_use]
    pub const fn disposition(&self) -> &SurfaceFrameDisposition {
        &self.disposition
    }

    /// Returns the exact core result for this surface contribution.
    #[must_use]
    pub const fn contribution(&self) -> &SurfaceContributionOutcome {
        self.disposition.contribution()
    }
}

/// One atomic reducer transition and every frozen surface's terminal disposition.
#[derive(Debug)]
pub struct HostFrameResponse {
    pub(crate) transition: EngineTransition,
    pub(crate) surfaces: BTreeMap<SurfaceId, SurfaceCommitResponse>,
}

/// One close plan emitted by the core, retaining its causal position and reuse bit.
///
/// A plan may be requested more than once while an earlier close is still pending. The
/// `reused` flag lets an adapter distinguish that case without inspecting private transition
/// variants.
#[derive(Clone, Copy, Debug)]
pub struct CloseRequestRef<'a> {
    plan: &'a ClosePlan,
    reused: bool,
    causal_ordinal: ReducerCausalOrdinal,
}

impl<'a> CloseRequestRef<'a> {
    /// Returns the core-owned close plan.
    #[must_use]
    pub const fn plan(self) -> &'a ClosePlan {
        self.plan
    }

    /// Returns whether this request reused an unresolved plan for the same target.
    #[must_use]
    pub const fn reused(self) -> bool {
        self.reused
    }

    /// Returns the core-assigned causal position of this request.
    #[must_use]
    pub const fn causal_ordinal(self) -> ReducerCausalOrdinal {
        self.causal_ordinal
    }
}

struct OrderedCloseRequest<'a> {
    ordinal: ReducerCausalOrdinal,
    sequence: u64,
    request: CloseRequestRef<'a>,
}

fn close_request_events<'a>(transition: &'a EngineTransition) -> Vec<CloseRequestRef<'a>> {
    let mut ordered = Vec::new();
    for input in transition.reduced_inputs() {
        if let InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::CloseRequested { plan, reused },
            ..
        } = input.outcome()
        {
            ordered.push(OrderedCloseRequest {
                ordinal: input.causal_ordinal(),
                sequence: input.sequence().get(),
                request: CloseRequestRef {
                    plan,
                    reused: *reused,
                    causal_ordinal: input.causal_ordinal(),
                },
            });
        }
    }
    for edge in transition.reduced_pointer_edges() {
        for outcome in edge.interaction_outcomes() {
            if let InteractionOutcome::CloseRequested { plan, reused } = outcome {
                ordered.push(OrderedCloseRequest {
                    ordinal: edge.causal_ordinal(),
                    sequence: edge.ticket().sequence().get(),
                    request: CloseRequestRef {
                        plan,
                        reused: *reused,
                        causal_ordinal: edge.causal_ordinal(),
                    },
                });
            }
        }
    }
    ordered.sort_by_key(|entry| (entry.ordinal, entry.sequence));
    ordered.into_iter().map(|entry| entry.request).collect()
}

impl HostFrameResponse {
    /// Returns the sole atomic engine transition produced by `end_host_frame`.
    #[must_use]
    pub const fn transition(&self) -> &EngineTransition {
        &self.transition
    }

    /// Returns one frozen surface's typed result.
    #[must_use]
    pub fn surface(&self, surface: SurfaceId) -> Option<&SurfaceCommitResponse> {
        self.surfaces.get(&surface)
    }

    /// Iterates every frozen surface result in stable surface identity order.
    #[must_use]
    pub fn surfaces(&self) -> impl ExactSizeIterator<Item = (SurfaceId, &SurfaceCommitResponse)> {
        self.surfaces
            .iter()
            .map(|(surface, response)| (*surface, response))
    }

    /// Iterates close plans requested by semantic controls in this host frame.
    pub fn close_requests(&self) -> impl Iterator<Item = &ClosePlan> {
        self.close_request_events()
            .into_iter()
            .map(|request| request.plan())
    }

    /// Iterates close requests in core causal order, retaining whether each plan was reused.
    #[must_use]
    pub fn close_request_events(&self) -> Vec<CloseRequestRef<'_>> {
        close_request_events(&self.transition)
    }
}

/// Convenience result from one strict single-surface host frame.
///
/// This is not the legacy per-callback reducer response: it is produced by creating a complete
/// host frame, painting the one permitted surface slot, and ending that frame atomically.
#[derive(Debug)]
pub struct DockspaceResponse {
    pub(crate) transition: EngineTransition,
    pub(crate) paint: SurfacePaintResponse,
    pub(crate) disposition: SurfaceFrameDisposition,
}

impl DockspaceResponse {
    /// Returns the sole atomic engine transition published by this call.
    #[must_use]
    pub fn transitions(&self) -> &[EngineTransition] {
        std::slice::from_ref(&self.transition)
    }

    /// Returns the terminal scene disposition for the one surface slot.
    #[must_use]
    pub const fn disposition(&self) -> &SurfaceFrameDisposition {
        &self.disposition
    }

    /// Returns the exact core result for the sole surface contribution.
    #[must_use]
    pub const fn contribution(&self) -> &SurfaceContributionOutcome {
        self.disposition.contribution()
    }

    /// Iterates close plans requested by semantic controls in this call.
    pub fn close_requests(&self) -> impl Iterator<Item = &ClosePlan> {
        self.close_request_events()
            .into_iter()
            .map(|request| request.plan())
    }

    /// Iterates close requests in core causal order, retaining whether each plan was reused.
    #[must_use]
    pub fn close_request_events(&self) -> Vec<CloseRequestRef<'_>> {
        close_request_events(&self.transition)
    }

    /// Returns stable item identities missing from the application pane registry.
    #[must_use]
    pub fn missing_panes(&self) -> &[ItemId] {
        self.paint.missing_panes()
    }

    /// Returns exact source/target capture errors observed while registering widgets.
    #[must_use]
    pub fn capture_errors(&self) -> &[CommandError] {
        self.paint.capture_errors()
    }

    /// Returns whether egui's prior-pass hit geometry exactly matched this projection.
    #[must_use]
    pub const fn interactions_current(&self) -> bool {
        self.paint.interactions_current()
    }

    /// Returns the exact core lifecycle state used for this paint.
    #[must_use]
    pub const fn surface_status(&self) -> DockspaceSurfaceStatus {
        self.paint.surface_status()
    }

    /// Returns current contained-floating tear-off availability.
    #[must_use]
    pub const fn contained_capability(&self) -> DockspaceCapability {
        self.paint.contained_capability()
    }

    /// Returns whether this surface can publish exact pane-focus observations.
    #[must_use]
    pub const fn pane_focus_capability(&self) -> DockspaceCapability {
        self.paint.pane_focus_capability()
    }
}
