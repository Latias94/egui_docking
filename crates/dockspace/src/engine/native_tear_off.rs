//! Shared native-create preparation for pointer and product entry points.

use super::*;

use crate::geometry::{LogicalRect, LogicalSize, PhysicalRect};
use crate::model::{
    DockspaceActionOutcome, DockspaceActionRejection, NativeTearOffSource, NativeWindowPlacement,
};
use crate::platform::ObservedWorkArea;
use crate::scene_compiler::{contained_minimum_size, pane_outer_minimum};
use crate::scene_manifest::PaneMinimumKey;

#[cfg(test)]
mod tests;

const DEFAULT_NATIVE_WINDOW_CASCADE: f64 = 24.0;

#[derive(Debug)]
pub(super) enum NativeRootCreateRejection {
    Command(CommandError),
    RootPending { source_surface: SurfaceId },
    RecoveryUnavailable { source_surface: SurfaceId },
}

impl NativeRootCreateRejection {
    pub(super) fn into_interaction_command_error(self) -> CommandError {
        match self {
            Self::Command(error) => error,
            Self::RootPending { source_surface } | Self::RecoveryUnavailable { source_surface } => {
                CommandError::SurfaceLifecycleFrozen {
                    surface: source_surface,
                }
            }
        }
    }
}

struct PreparedProductNativeTearOff {
    payload: MovePayload,
    source_presentation: PresentedSurfaceAuthority,
    command: WorkspaceCommand,
    proposal: crate::frame::NativeCreateProposal,
    pane_focus: PaneFocusDisposition,
    items: Vec<ItemId>,
}

impl DockEngine {
    pub(super) fn root_is_bound_native_main(&self, root: RootId) -> bool {
        matches!(
            self.workspace.presentation_for_root(root),
            Some(crate::RootPresentationOwner::Main { surface })
                if self.bound_surface_recoveries.contains_key(&surface)
        )
    }

    fn native_payload_owner(
        &self,
        root: RootId,
    ) -> Result<(SurfaceId, Option<FloatingPresentationId>), DockspaceActionRejection> {
        match self.workspace.presentation_for_root(root) {
            Some(crate::RootPresentationOwner::Main { surface }) => Ok((surface, None)),
            Some(crate::RootPresentationOwner::Contained { surface, floating }) => {
                Ok((surface, Some(floating)))
            }
            None => Err(DockspaceActionRejection::RootUnavailable { root }),
        }
    }

    fn native_payload_geometry(
        &self,
        payload: &MovePayload,
        complete_root: bool,
        plan: &crate::scene::PresentationPlan,
    ) -> Result<(Option<LogicalRect>, LogicalSize), DockspaceActionRejection> {
        let (root, node) = match payload {
            MovePayload::Item(source) => (source.root(), source.tabs()),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => {
                (source.root(), source.node())
            }
        };
        let (surface, floating) = self.native_payload_owner(root)?;
        let unavailable = DockspaceActionRejection::PresentationUnavailable { surface };
        if plan.surface() != surface {
            return Err(unavailable);
        }
        if !complete_root {
            let facts = plan.layout_facts().ok_or(unavailable)?;
            let root_facts = facts.root(root).ok_or(unavailable)?;
            let pane_minimum = match payload {
                MovePayload::Item(source) => {
                    let measured = root_facts
                        .pane_minimum(PaneMinimumKey::new(root, node, Some(source.item())))
                        .ok_or(unavailable)?;
                    let visibility = self
                        .presentation_requirements()
                        .surface(surface)
                        .and_then(|requirements| {
                            requirements.tab_bar(crate::scene::TabBarSceneId { root, tabs: node })
                        })
                        .map(|bar| bar.policy().visibility())
                        .ok_or(unavailable)?;
                    pane_outer_minimum(measured, facts.config(), visibility)
                        .map_err(|_| DockspaceActionRejection::Conflict)?
                }
                MovePayload::Tabs(_) | MovePayload::Subtree(_) => root_facts
                    .projection()
                    .nodes
                    .get(&node)
                    .map(|result| result.constraints.min())
                    .ok_or(unavailable)?,
            };
            let minimum = contained_minimum_size(facts.config(), pane_minimum)
                .map_err(|_| DockspaceActionRejection::Conflict)?;
            let pane = plan
                .pane_records()
                .iter()
                .find(|pane| pane.id() == (crate::scene::PaneSceneId { root, tabs: node }))
                .ok_or(unavailable)?;
            let bounds = pane.bounds();
            // A partial payload uses its pane and measured recovery minimum,
            // independently of the enclosing floating window and its siblings.
            let rect = LogicalRect::new(
                bounds.x(),
                bounds.y(),
                bounds.width().max(minimum.width()),
                bounds.height().max(minimum.height()),
            )
            .map_err(|_| DockspaceActionRejection::Conflict)?;
            return Ok((Some(rect), minimum));
        }
        if let Some(floating) = floating {
            let rect = plan
                .contained_record(floating)
                .filter(|record| record.root() == root)
                .map(crate::scene::ContainedRecord::outer_bounds);
            let minimum = plan
                .contained_minimums()
                .iter()
                .find(|measurement| measurement.floating() == floating)
                .map(|measurement| measurement.minimum_size())
                .ok_or(unavailable)?;
            return Ok((rect, minimum));
        }
        let rect = plan
            .layout_facts()
            .and_then(|facts| facts.root(root))
            .map(|facts| facts.bounds())
            .ok_or(unavailable)?;
        Ok((
            Some(rect),
            self.presentation_config().minimum_floating_size(),
        ))
    }

    fn derive_default_native_payload_placement(
        &self,
        surface: SurfaceId,
        complete_main: bool,
        source_rect: LogicalRect,
    ) -> Result<NativeWindowPlacement, DockspaceActionRejection> {
        let unavailable = DockspaceActionRejection::PresentationUnavailable { surface };
        let coordinates = self
            .viewport
            .viewport(surface)
            .and_then(crate::viewport_registry::ViewportRecord::coordinates)
            .ok_or(unavailable)?;
        let source_outer = coordinates.outer_bounds().ok_or(unavailable)?;
        let requested = if complete_main {
            source_outer
        } else {
            let content = coordinates
                .surface_rect_to_desktop(source_rect)
                .map_err(|_| unavailable)?;
            let source_content = coordinates.content_bounds().ok_or(unavailable)?;
            expand_content_rect_to_outer(content, source_content, source_outer)
                .ok_or(unavailable)?
        };
        let work_area = select_native_work_area(
            requested,
            self.viewport.work_areas().map(|(_, work_area)| work_area),
        )
        .ok_or(DockspaceActionRejection::NativeUnavailable)?;
        let offset = DEFAULT_NATIVE_WINDOW_CASCADE * coordinates.native_scale_factor().get();
        let cascaded = PhysicalRect::new(
            requested.x() + offset,
            requested.y() + offset,
            requested.width(),
            requested.height(),
        )
        .map_err(|_| unavailable)?;
        let clamped = clamp_native_window_rect(cascaded, work_area.bounds())
            .ok_or(DockspaceActionRejection::NativeUnavailable)?;
        Ok(NativeWindowPlacement::new(clamped))
    }

    fn capture_product_native_payload(
        &self,
        source: &NativeTearOffSource,
    ) -> Result<(MovePayload, Vec<ItemId>), DockspaceActionRejection> {
        match source {
            NativeTearOffSource::Item(item) => self
                .workspace
                .capture_item_source_by_id(*item)
                .map_err(|_| DockspaceActionRejection::Conflict)?
                .map(|source| (MovePayload::Item(source), vec![*item]))
                .ok_or(DockspaceActionRejection::ItemUnavailable { item: *item }),
            NativeTearOffSource::Tabs(group) => {
                if group.authority_domain != self.authority_domain || group.expected != self.version
                {
                    return Err(DockspaceActionRejection::Conflict);
                }
                self.validate_node_source(&group.source)
                    .map_err(|_| DockspaceActionRejection::Conflict)?;
                let Some(Node::Tabs { items, .. }) = self.workspace.node(group.source.node())
                else {
                    return Err(DockspaceActionRejection::Conflict);
                };
                if items.is_empty() {
                    return Err(DockspaceActionRejection::Conflict);
                }
                Ok((MovePayload::Tabs(group.source.clone()), items.to_vec()))
            }
            NativeTearOffSource::Root(root) => self
                .capture_product_root_payload(*root)
                .map(|(_, payload, items)| (payload, items)),
        }
    }

    fn prepare_product_native_payload_tear_off(
        &self,
        source: &NativeTearOffSource,
        placement: Option<NativeWindowPlacement>,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<PreparedProductNativeTearOff, DockspaceActionRejection>, EngineError> {
        let native_supported = self
            .viewport
            .native_exact_placement_create_capability()
            .is_supported();
        // A default request needs a native placement provider before it can
        // derive a rectangle. Explicit requests preserve policy-first rejection.
        if placement.is_none() && !native_supported {
            return Ok(Err(DockspaceActionRejection::NativeUnavailable));
        }
        if policy.check_tear_off(TearOffPresentation::Native).is_err() {
            return Ok(Err(DockspaceActionRejection::PolicyDenied));
        }
        if !native_supported || !self.viewport.native_binding_allocation_available() {
            return Ok(Err(DockspaceActionRejection::NativeUnavailable));
        }
        if self.pending_mutation.is_some() || self.interaction.status() != InteractionStatus::Idle {
            return Ok(Err(DockspaceActionRejection::Conflict));
        }
        let (payload, items) = match self.capture_product_native_payload(source) {
            Ok(captured) => captured,
            Err(reason) => return Ok(Err(reason)),
        };
        let source_root = match &payload {
            MovePayload::Item(source) => source.root(),
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => source.root(),
        };
        if self.root_has_pending_presentation_transition(source_root) {
            return Ok(Err(DockspaceActionRejection::Conflict));
        }
        let complete = match self.capture_complete_root_payload(&payload) {
            Ok(complete) => complete,
            Err(reason) => return Ok(Err(reason)),
        };
        let (source_surface, floating) = match self.native_payload_owner(source_root) {
            Ok(owner) => owner,
            Err(reason) => return Ok(Err(reason)),
        };
        let Some(projection) = self.interaction_projection(source_surface) else {
            return Ok(Err(DockspaceActionRejection::PresentationUnavailable {
                surface: source_surface,
            }));
        };
        let (source_rect, minimum_size) =
            match self.native_payload_geometry(&payload, complete.is_some(), projection.plan()) {
                Ok(geometry) => geometry,
                Err(reason) => return Ok(Err(reason)),
            };
        let placement = match placement {
            Some(placement) => placement,
            None => {
                let Some(source_rect) = source_rect else {
                    return Ok(Err(DockspaceActionRejection::PresentationUnavailable {
                        surface: source_surface,
                    }));
                };
                match self.derive_default_native_payload_placement(
                    source_surface,
                    complete.is_some() && floating.is_none(),
                    source_rect,
                ) {
                    Ok(placement) => placement,
                    Err(reason) => return Ok(Err(reason)),
                }
            }
        };
        let pane_focus = match self.freeze_payload_focus(&payload) {
            Ok(focus) => match &payload {
                MovePayload::Item(source) => PaneFocusDisposition::Set(source.item()),
                MovePayload::Tabs(_) | MovePayload::Subtree(_) => focus,
            },
            Err(error) => {
                return Ok(Err(super::product_action::product_command_rejection(
                    ProductAction::TearOffNative {
                        source: source.clone(),
                        placement: Some(placement),
                    },
                    &error,
                )?));
            }
        };
        let retained_floating = complete.as_ref().and(floating);
        let reservation = match self.prepare_native_payload_transfer_identities(
            complete.as_ref().map(NodeSource::root),
            retained_floating,
        ) {
            Ok(reservation) => reservation,
            Err(
                EngineError::PresentationRootIdentityExhausted
                | EngineError::PresentationSurfaceIdentityExhausted
                | EngineError::PresentationFloatingIdentityExhausted,
            ) => return Ok(Err(DockspaceActionRejection::IdentityExhausted)),
            Err(error) => return Err(error),
        };
        if self
            .surface_recovery_target(source_surface)
            .map(SurfaceRecoveryTarget::anchor)
            .or_else(|| self.root_recovery_anchor(source_surface))
            .is_none()
        {
            return Ok(Err(DockspaceActionRejection::NativeUnavailable));
        }
        let command = Self::tear_off_command_from_complete_root(
            &payload,
            RootPresentationTarget::NewSurface {
                surface: reservation.surface(),
            },
            reservation.root(),
            complete,
        );
        let proposal = crate::frame::NativeCreateProposal::new(
            reservation.surface(),
            placement.outer_rect(),
            ConvertedMainRecovery::new(reservation.root(), reservation.floating(), minimum_size),
        );
        Ok(Ok(PreparedProductNativeTearOff {
            payload,
            source_presentation: projection.authority(),
            command,
            proposal,
            pane_focus,
            items,
        }))
    }

    pub(super) fn validate_product_native_payload_tear_off_availability(
        &self,
        source: &NativeTearOffSource,
        placement: Option<NativeWindowPlacement>,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<(), DockspaceActionRejection>, EngineError> {
        let prepared =
            match self.prepare_product_native_payload_tear_off(source, placement, policy)? {
                Ok(prepared) => prepared,
                Err(reason) => return Ok(Err(reason)),
            };
        if let Err(error) = crate::operation::authorize_workspace_command(
            &self.workspace,
            policy,
            &prepared.command,
        ) {
            return Ok(Err(super::product_action::product_command_rejection(
                ProductAction::TearOffNative {
                    source: source.clone(),
                    placement,
                },
                &error,
            )?));
        }
        Ok(Ok(()))
    }

    pub(super) fn reduce_product_native_payload_tear_off(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        source: &NativeTearOffSource,
        placement: Option<NativeWindowPlacement>,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        let prepared =
            match self.prepare_product_native_payload_tear_off(source, placement, policy)? {
                Ok(prepared) => prepared,
                Err(reason) => {
                    return Ok(InputOutcome::ProductActionRejected {
                        reason,
                        version: self.version,
                    });
                }
            };
        let source_surface = prepared.source_presentation.surface();
        let root = prepared.proposal.root();
        match self.start_native_root_create(
            cause,
            focus_causal,
            prepared.source_presentation,
            prepared.payload,
            prepared.command,
            prepared.proposal,
            prepared.pane_focus,
            policy,
        )? {
            Ok(request) => Ok(InputOutcome::ProductActionProcessed {
                outcome: DockspaceActionOutcome::NativeRootTearOffRequested {
                    root,
                    source_surface,
                    target_surface: request.binding().surface(),
                    items: prepared.items,
                },
                version: self.version,
            }),
            Err(NativeRootCreateRejection::RootPending { .. }) => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::Conflict,
                    version: self.version,
                })
            }
            Err(NativeRootCreateRejection::RecoveryUnavailable { .. }) => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::NativeUnavailable,
                    version: self.version,
                })
            }
            Err(NativeRootCreateRejection::Command(error)) => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(
                        ProductAction::TearOffNative {
                            source: source.clone(),
                            placement,
                        },
                        &error,
                    )?,
                    version: self.version,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_native_root_create(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        source_presentation: PresentedSurfaceAuthority,
        payload: MovePayload,
        command: WorkspaceCommand,
        proposal: crate::frame::NativeCreateProposal,
        pane_focus: PaneFocusDisposition,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<crate::frame::NativeCreateRequest, NativeRootCreateRejection>, EngineError>
    {
        let source_surface = source_presentation.surface();
        if !self.viewport.native_binding_allocation_available() {
            // Accepted pointer release must terminate with a semantic rejection;
            // rolling back its recorder prefix would replay it without progress.
            return Ok(Err(NativeRootCreateRejection::RecoveryUnavailable {
                source_surface,
            }));
        }
        if self.root_has_pending_presentation_transition(proposal.root()) {
            return Ok(Err(NativeRootCreateRejection::RootPending {
                source_surface,
            }));
        }

        let recovery_anchor = self
            .surface_recovery_target(source_surface)
            .map(SurfaceRecoveryTarget::anchor)
            .or_else(|| self.root_recovery_anchor(source_surface));
        let Some(recovery_anchor) = recovery_anchor else {
            return Ok(Err(NativeRootCreateRejection::RecoveryUnavailable {
                source_surface,
            }));
        };
        let recovery_target =
            SurfaceRecoveryTarget::with_converted_main(recovery_anchor, proposal.converted_main());
        let future = match self.preflight_native_root_create_command(
            cause,
            source_surface,
            &command,
            policy,
        )? {
            Ok(future) => future,
            Err(error) => return Ok(Err(NativeRootCreateRejection::Command(error))),
        };
        if future.surface(recovery_target.host_surface()).is_none() {
            return Ok(Err(NativeRootCreateRejection::RecoveryUnavailable {
                source_surface,
            }));
        }
        let id = self.next_surface_recovery_obligation_id(self.last_input)?;
        let obligation = match self.authorize_surface_recovery_obligation(
            self.last_input,
            id,
            &future,
            proposal.surface(),
            recovery_target,
            policy,
        ) {
            Ok(obligation) => obligation,
            Err(error) => return Ok(Err(NativeRootCreateRejection::Command(error))),
        };
        let proposal_surface = proposal.surface();
        let proposal_root = proposal.root();
        let proposal_floating = proposal.converted_main().floating();
        let prepared = crate::frame::PreparedNativeCreate::new(
            source_presentation,
            payload,
            self.version.epoch(),
            command,
            proposal,
            obligation,
            focus_causal,
        );
        let request = self
            .viewport
            .start_native_create(prepared)
            .map_err(|source| EngineError::Viewport {
                input: self.last_input,
                source,
            })?;
        let _ = self.viewport_focus.reserve_activation_causal(
            request.saga(),
            focus_causal,
            ViewportActivationRequest::tear_off_committed(request.binding(), pane_focus),
        );
        self.presentation_identity.observe_native(
            proposal_surface,
            proposal_root,
            proposal_floating,
        );
        self.last_surface_recovery_obligation = id;
        Ok(Ok(request))
    }

    fn preflight_native_root_create_command(
        &self,
        cause: ReductionCause,
        source_surface: SurfaceId,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<Workspace, CommandError>, EngineError> {
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&self.workspace, None, None)
        {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        let prepared = match crate::operation::prepare_transaction(
            &self.workspace,
            policy,
            std::slice::from_ref(command),
        ) {
            Ok(prepared) => prepared,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(Err(source));
            }
            Err(source) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                });
            }
        };
        let (future, report) = prepared.into_parts();
        if report.outcomes().len() != 1 {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "native root create preflight produced a non-unit outcome roster",
            });
        }
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&future, None, Some(source_surface))
        {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        match self.stage_workspace_publication(future, policy) {
            Ok(publication) => Ok(Ok(publication.workspace.into_workspace())),
            Err(source) if source.is_expected_rejection() => Ok(Err(source)),
            Err(source) => Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            }),
        }
    }
}

/// Selects from the caller's current authoritative work-area roster only.
///
/// Like ImGui's FindPlatformMonitorForRect, positive axis-aligned intersection area
/// takes priority. With no positive intersection, distance is the Euclidean distance
/// between the closest points of the two rectangles: hypot(horizontal gap, vertical
/// gap), with touching edges at distance zero. Equal scores choose the lowest token,
/// independently of roster order. Cascade and clamping happen after this selection.
fn select_native_work_area(
    requested: PhysicalRect,
    work_areas: impl IntoIterator<Item = ObservedWorkArea>,
) -> Option<ObservedWorkArea> {
    work_areas
        .into_iter()
        .map(|work_area| {
            let bounds = work_area.bounds();
            let overlap_width =
                requested.max().x().min(bounds.max().x()) - requested.x().max(bounds.x());
            let overlap_height =
                requested.max().y().min(bounds.max().y()) - requested.y().max(bounds.y());
            let intersection = if overlap_width > 0.0 && overlap_height > 0.0 {
                overlap_width * overlap_height
            } else {
                0.0
            };
            let horizontal_gap = (requested.x() - bounds.max().x())
                .max(bounds.x() - requested.max().x())
                .max(0.0);
            let vertical_gap = (requested.y() - bounds.max().y())
                .max(bounds.y() - requested.max().y())
                .max(0.0);
            (work_area, intersection, horizontal_gap.hypot(vertical_gap))
        })
        .max_by(
            |(left, left_area, left_distance), (right, right_area, right_distance)| {
                left_area
                    .total_cmp(right_area)
                    .then_with(|| right_distance.total_cmp(left_distance))
                    .then_with(|| right.token().cmp(&left.token()))
            },
        )
        .map(|(work_area, _, _)| work_area)
}

fn expand_content_rect_to_outer(
    requested_content: PhysicalRect,
    source_content: PhysicalRect,
    source_outer: PhysicalRect,
) -> Option<PhysicalRect> {
    let left = source_content.x() - source_outer.x();
    let top = source_content.y() - source_outer.y();
    let right = source_outer.max().x() - source_content.max().x();
    let bottom = source_outer.max().y() - source_content.max().y();
    if [left, top, right, bottom]
        .into_iter()
        .any(|inset| !inset.is_finite() || inset < 0.0)
    {
        return None;
    }
    PhysicalRect::new(
        requested_content.x() - left,
        requested_content.y() - top,
        requested_content.width() + left + right,
        requested_content.height() + top + bottom,
    )
    .ok()
}

fn clamp_native_window_rect(
    requested: PhysicalRect,
    work_area: PhysicalRect,
) -> Option<PhysicalRect> {
    let width = requested.width().min(work_area.width());
    let height = requested.height().min(work_area.height());
    let maximum_x = work_area.max().x() - width;
    let maximum_y = work_area.max().y() - height;
    PhysicalRect::new(
        requested.x().clamp(work_area.x(), maximum_x),
        requested.y().clamp(work_area.y(), maximum_y),
        width,
        height,
    )
    .ok()
}
