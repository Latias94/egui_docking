//! Translation from one committed core transition to product host outcomes.

use std::collections::BTreeSet;

use super::{
    DockspaceCloseOutcome, DockspaceClosePlan, DockspaceCloseRejection,
    DockspaceCloseRequestRejection, DockspaceCloseResolution, DockspacePresentationTransition,
    DockspacePresentationTransitionId, DockspacePresentationTransitionResult,
    DockspaceSubmittedAction, HostCloseRequestOrigin, HostFrameReport, HostInputOutcome,
    HostSurfaceCommit, NativeEffectRequest, NativeSurfaceBinding, NativeSurfaceCloseRejection,
    NativeSurfaceCloseRequest, PaintedNativeStagingOutput, PaintedSurfaceOutput, native_effect,
};
use crate::command::CloseCommitOutcome;
use crate::error::CommandError;
use crate::event::{PresentationRehomeResult, WorkspaceEventKind};
use crate::interaction::InteractionOutcome;
use crate::model::SurfaceId;
use crate::transition::InputOutcome;

impl HostFrameReport {
    pub(super) fn from_transition(
        frame_attempt: crate::ids::HostPresentationAttemptId,
        transition: &crate::transition::EngineTransition,
        painted_outputs: Vec<PaintedSurfaceOutput>,
        painted_native_staging_outputs: Vec<PaintedNativeStagingOutput>,
        native_commit: Option<super::native::NativeHostCommit>,
        abandoned_native_effects: native_effect::NativeEffectDropQueue,
    ) -> Self {
        let (native_admissions, native_bindings, native_provider) = native_commit.map_or_else(
            || (Vec::new(), Vec::new(), None),
            |native| (native.admissions, native.bindings, Some(native.provider)),
        );
        let mut ordered_inputs = Vec::new();
        let document_restore = transition.reduced_inputs().iter().find_map(|reduced| {
            let InputOutcome::WorkspaceReplaced {
                before,
                after,
                restored_identity_frontier: Some(_),
                ..
            } = reduced.outcome()
            else {
                return None;
            };
            Some((reduced.causal_ordinal().get(), *before, *after))
        });
        if let Some((ordinal, previous, current)) = document_restore {
            ordered_inputs.push((
                ordinal,
                0,
                0,
                None,
                HostInputOutcome::DocumentRestored { previous, current },
            ));
        } else {
            for reduced in transition.reduced_inputs() {
                let outcome = match reduced.outcome() {
                    InputOutcome::ProductActionProcessed { outcome, .. } => {
                        Some(product_action_host_outcome(outcome, reduced.cause()))
                    }
                    InputOutcome::ProductActionRejected { reason, .. } => {
                        Some(HostInputOutcome::ProductActionRejected(*reason))
                    }
                    InputOutcome::ContentCloseRequested { plan, reused, .. } => {
                        Some(HostInputOutcome::CloseRequested {
                            plan: DockspaceClosePlan::from_core(plan),
                            reused: *reused,
                            origin: HostCloseRequestOrigin::Application,
                        })
                    }
                    InputOutcome::ContentCloseRejected { target, reason, .. } => {
                        Some(HostInputOutcome::CloseRejected(
                            DockspaceCloseRequestRejection::from_core(*target, reason),
                        ))
                    }
                    InputOutcome::CloseDecisionProcessed {
                        resolution,
                        plan,
                        application,
                        changed,
                        ..
                    } => Some(HostInputOutcome::CloseDecisionProcessed {
                        resolution: DockspaceCloseResolution::from_core(*resolution),
                        plan: plan.as_ref().map(DockspaceClosePlan::from_core),
                        application: application.as_ref().map(map_close_application),
                        changed: *changed,
                    }),
                    InputOutcome::ViewportRegistered { binding } => {
                        native_provider.map(|provider| HostInputOutcome::NativeSurfaceRegistered {
                            binding: NativeSurfaceBinding::from_binding(provider, *binding),
                        })
                    }
                    InputOutcome::ViewportRegistrationRejected { surface } => {
                        Some(HostInputOutcome::NativeSurfaceRegistrationRejected {
                            surface: *surface,
                        })
                    }
                    InputOutcome::PlatformSnapshotPublished {
                        native_close_edges, ..
                    } => native_provider.map(|provider| {
                        HostInputOutcome::NativePlatformSnapshotApplied {
                            close_requests: native_close_edges
                                .iter()
                                .copied()
                                .map(|edge| NativeSurfaceCloseRequest::from_edge(provider, edge))
                                .collect(),
                        }
                    }),
                    InputOutcome::NativeCloseObservationPublished {
                        native_close_edges, ..
                    } => native_provider.map(|provider| {
                        HostInputOutcome::NativeCloseObservationApplied {
                            close_requests: native_close_edges
                                .iter()
                                .copied()
                                .map(|edge| NativeSurfaceCloseRequest::from_edge(provider, edge))
                                .collect(),
                        }
                    }),
                    InputOutcome::GlobalFocusObservationPublished { .. } => {
                        Some(HostInputOutcome::NativeFocusObservationApplied)
                    }
                    InputOutcome::SurfaceCloseRequested { request, plan, .. } => {
                        Some(HostInputOutcome::NativeSurfaceCloseRequested {
                            disposition: request.disposition(),
                            plan: DockspaceClosePlan::from_core(plan),
                        })
                    }
                    InputOutcome::SurfaceCloseRejected {
                        edge,
                        request,
                        reason,
                        ..
                    } => native_provider.map(|provider| {
                        HostInputOutcome::NativeSurfaceCloseRejected {
                            close: NativeSurfaceCloseRequest::from_edge(provider, *edge),
                            disposition: request.disposition(),
                            reason: NativeSurfaceCloseRejection::from(reason),
                        }
                    }),
                    InputOutcome::SurfaceCloseCancellationRequested { edge, plan, .. } => {
                        native_provider.map(|provider| {
                            HostInputOutcome::NativeSurfaceCloseCancellationRequired {
                                close: NativeSurfaceCloseRequest::from_edge(provider, *edge),
                                plan: DockspaceClosePlan::from_core(plan),
                            }
                        })
                    }
                    InputOutcome::PlatformEffectReported { .. } => None,
                    InputOutcome::PlatformSnapshotStale { .. } => {
                        Some(HostInputOutcome::NativePlatformSnapshotStale)
                    }
                    InputOutcome::PlatformProviderRejected { .. } => {
                        Some(HostInputOutcome::NativePlatformProviderRejected)
                    }
                    InputOutcome::StaleRejected {
                        expected,
                        accepted_base,
                    } => Some(HostInputOutcome::StaleRejected {
                        expected: *expected,
                        accepted: *accepted_base,
                    }),
                    InputOutcome::InteractionProcessed { outcome, .. } => {
                        interaction_host_outcome(outcome, reduced.cause())
                    }
                    _ => None,
                };
                if let Some(outcome) = outcome {
                    let submitted = (reduced.source()
                        == super::host_frame::APPLICATION_INPUT_SOURCE)
                        .then(|| {
                            DockspaceSubmittedAction::new(frame_attempt, reduced.source_sequence())
                        });
                    ordered_inputs.push((
                        reduced.causal_ordinal().get(),
                        0_usize,
                        0_usize,
                        submitted,
                        outcome,
                    ));
                }
            }
            for (edge_index, edge) in transition.reduced_pointer_edges().iter().enumerate() {
                for (outcome_index, outcome) in edge.interaction_outcomes().iter().enumerate() {
                    if let Some(outcome) = interaction_host_outcome(outcome, edge.cause()) {
                        ordered_inputs.push((
                            edge.causal_ordinal().get(),
                            edge_index,
                            outcome_index,
                            None,
                            outcome,
                        ));
                    }
                }
            }
        }
        let (inputs, submitted_actions) = finish_ordered_inputs(ordered_inputs);
        let presentation_transitions = transition
            .events()
            .iter()
            .filter_map(|event| {
                let WorkspaceEventKind::PresentationRehomeSettled {
                    root,
                    source_surface,
                    target_surface,
                    result,
                    outcome,
                } = event.kind()
                else {
                    return None;
                };
                Some(DockspacePresentationTransition {
                    id: DockspacePresentationTransitionId::from_cause(event.cause()),
                    root: *root,
                    source_surface: *source_surface,
                    target_surface: *target_surface,
                    result: match result {
                        PresentationRehomeResult::Applied => {
                            DockspacePresentationTransitionResult::Applied
                        }
                        PresentationRehomeResult::TargetNotPresented => {
                            DockspacePresentationTransitionResult::TargetNotPresented
                        }
                        PresentationRehomeResult::WorkspaceChanged => {
                            DockspacePresentationTransitionResult::WorkspaceChanged
                        }
                        PresentationRehomeResult::PolicyChanged => {
                            DockspacePresentationTransitionResult::PolicyChanged
                        }
                        PresentationRehomeResult::SourceUnavailable => {
                            DockspacePresentationTransitionResult::SourceUnavailable
                        }
                        PresentationRehomeResult::CommandRejected => {
                            DockspacePresentationTransitionResult::CommandRejected
                        }
                    },
                    outcome: outcome.clone(),
                })
            })
            .collect();
        let surface_commits = transition
            .surface_contributions()
            .iter()
            .map(HostSurfaceCommit::from_outcome)
            .collect();
        let repaint_surfaces: Vec<SurfaceId> = transition
            .affected_surfaces()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        let native_effects = transition
            .platform_effects()
            .iter()
            .map(|emission| {
                NativeEffectRequest::from_emission(emission, abandoned_native_effects.clone())
            })
            .collect();
        Self {
            before: transition.before(),
            after: transition.after(),
            workspace_changed: transition.changed(),
            published_state_changed: transition.published_state_changed(),
            affected_surfaces: repaint_surfaces.clone(),
            surface_commits,
            inputs,
            submitted_actions,
            presentation_transitions,
            painted_outputs,
            painted_native_staging_outputs,
            native_admissions,
            native_bindings,
            native_effects,
            repaint_surfaces,
        }
    }
}

fn product_action_host_outcome(
    outcome: &crate::model::DockspaceActionOutcome,
    cause: crate::event::ReductionCause,
) -> HostInputOutcome {
    if matches!(
        outcome,
        crate::model::DockspaceActionOutcome::RootDockRequested { .. }
            | crate::model::DockspaceActionOutcome::RootFloatRequested { .. }
    ) {
        HostInputOutcome::ProductPresentationActionRequested {
            outcome: outcome.clone(),
            transition: DockspacePresentationTransitionId::from_cause(cause),
        }
    } else {
        HostInputOutcome::ProductActionApplied(outcome.clone())
    }
}

fn interaction_host_outcome(
    outcome: &InteractionOutcome,
    cause: crate::event::ReductionCause,
) -> Option<HostInputOutcome> {
    match outcome {
        InteractionOutcome::ProductActionApplied(outcome) => {
            Some(product_action_host_outcome(outcome, cause))
        }
        InteractionOutcome::ProductActionRejected(reason) => {
            Some(HostInputOutcome::ProductActionRejected(*reason))
        }
        InteractionOutcome::CloseRequested { plan, reused } => {
            Some(HostInputOutcome::CloseRequested {
                plan: DockspaceClosePlan::from_core(plan),
                reused: *reused,
                origin: HostCloseRequestOrigin::Interaction,
            })
        }
        _ => None,
    }
}

fn map_close_application(
    application: &Result<CloseCommitOutcome, CommandError>,
) -> Result<DockspaceCloseOutcome, DockspaceCloseRejection> {
    match application {
        Ok(CloseCommitOutcome::ItemClosed { item, root }) => {
            Ok(DockspaceCloseOutcome::ItemClosed {
                item: *item,
                root: *root,
            })
        }
        Ok(CloseCommitOutcome::RootClosed { root, items }) => {
            Ok(DockspaceCloseOutcome::RootClosed {
                root: *root,
                items: items.clone(),
            })
        }
        Ok(CloseCommitOutcome::SurfaceClosed { surface, items }) => {
            Ok(DockspaceCloseOutcome::SurfaceClosed {
                surface: *surface,
                items: items.clone(),
            })
        }
        Err(CommandError::Policy(_)) => Err(DockspaceCloseRejection::PolicyDenied),
        Err(error) => Err(if error.is_expected_rejection() {
            DockspaceCloseRejection::Conflict
        } else {
            DockspaceCloseRejection::Internal
        }),
    }
}

fn finish_ordered_inputs(
    mut ordered: Vec<(
        u64,
        usize,
        usize,
        Option<DockspaceSubmittedAction>,
        HostInputOutcome,
    )>,
) -> (
    Vec<HostInputOutcome>,
    Vec<(DockspaceSubmittedAction, usize)>,
) {
    ordered.sort_by_key(|(ordinal, edge, outcome, _, _)| (*ordinal, *edge, *outcome));
    let mut inputs = Vec::with_capacity(ordered.len());
    let mut submitted_actions = Vec::new();
    for (_, _, _, submitted, outcome) in ordered {
        let index = inputs.len();
        inputs.push(outcome);
        if let Some(submitted) = submitted {
            submitted_actions.push((submitted, index));
        }
    }
    (inputs, submitted_actions)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_pointer_ordinal_preserves_edge_then_outcome_order() {
        let (outcomes, submitted_actions) = finish_ordered_inputs(vec![
            (7, 1, 0, None, HostInputOutcome::NativePlatformSnapshotStale),
            (
                7,
                0,
                1,
                None,
                HostInputOutcome::NativeCloseObservationApplied {
                    close_requests: Vec::new(),
                },
            ),
            (
                7,
                0,
                0,
                None,
                HostInputOutcome::NativePlatformSnapshotApplied {
                    close_requests: Vec::new(),
                },
            ),
        ]);

        assert!(matches!(
            outcomes.as_slice(),
            [
                HostInputOutcome::NativePlatformSnapshotApplied { .. },
                HostInputOutcome::NativeCloseObservationApplied { .. },
                HostInputOutcome::NativePlatformSnapshotStale,
            ]
        ));
        assert!(submitted_actions.is_empty());
    }
}
