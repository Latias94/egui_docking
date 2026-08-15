//! Translation from one committed core transition to product host outcomes.

use std::collections::BTreeSet;

use super::{
    DockspaceCloseOutcome, DockspaceClosePlan, DockspaceCloseRejection, DockspaceCloseResolution,
    HostCloseRequestOrigin, HostFrameReport, HostInputOutcome, HostSurfaceCommit,
    NativeEffectRequest, NativeSurfaceBinding, NativeSurfaceCloseRequest,
    PaintedNativeStagingOutput, PaintedSurfaceOutput, native_effect,
};
use crate::command::CloseCommitOutcome;
use crate::error::CommandError;
use crate::interaction::InteractionOutcome;
use crate::model::SurfaceId;
use crate::transition::InputOutcome;

impl HostFrameReport {
    pub(super) fn from_transition(
        transition: &crate::transition::EngineTransition,
        painted_outputs: Vec<PaintedSurfaceOutput>,
        painted_native_staging_outputs: Vec<PaintedNativeStagingOutput>,
        native_admissions: Vec<NativeSurfaceBinding>,
        native_provider: Option<crate::platform_provider::PlatformObservationLease>,
        abandoned_native_effects: native_effect::NativeEffectDropQueue,
    ) -> Self {
        let mut ordered_inputs = Vec::new();
        for reduced in transition.reduced_inputs() {
            let outcome = match reduced.outcome() {
                #[cfg(any(feature = "backend", test))]
                InputOutcome::CommandProcessed {
                    outcome, changed, ..
                } => Some(HostInputOutcome::CommandApplied {
                    outcome: outcome.clone(),
                    changed: *changed,
                }),
                #[cfg(any(feature = "backend", test))]
                InputOutcome::CommandRejected { error, .. } => {
                    Some(HostInputOutcome::CommandRejected(error.clone()))
                }
                InputOutcome::ProductActionProcessed { outcome, .. } => {
                    Some(HostInputOutcome::ProductActionApplied(outcome.clone()))
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
                    Some(HostInputOutcome::CloseRejected {
                        target: *target,
                        reason: reason.clone(),
                    })
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
                    Some(HostInputOutcome::NativeSurfaceRegistrationRejected { surface: *surface })
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
                InputOutcome::SurfaceCloseRequested { request, plan, .. } => {
                    Some(HostInputOutcome::NativeSurfaceCloseRequested {
                        request: request.clone(),
                        plan: DockspaceClosePlan::from_core(plan),
                    })
                }
                InputOutcome::SurfaceCloseRejected {
                    edge,
                    request,
                    reason,
                    ..
                } => native_provider.map(|provider| HostInputOutcome::NativeSurfaceCloseRejected {
                    close: NativeSurfaceCloseRequest::from_edge(provider, *edge),
                    request: request.clone(),
                    reason: reason.clone(),
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
                    interaction_close_request(outcome)
                }
                _ => None,
            };
            if let Some(outcome) = outcome {
                ordered_inputs.push((reduced.causal_ordinal().get(), 0_usize, 0_usize, outcome));
            }
        }
        for (edge_index, edge) in transition.reduced_pointer_edges().iter().enumerate() {
            for (outcome_index, outcome) in edge.interaction_outcomes().iter().enumerate() {
                if let Some(outcome) = interaction_close_request(outcome) {
                    ordered_inputs.push((
                        edge.causal_ordinal().get(),
                        edge_index,
                        outcome_index,
                        outcome,
                    ));
                }
            }
        }
        let inputs = finish_ordered_inputs(ordered_inputs);
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
            painted_outputs,
            painted_native_staging_outputs,
            native_admissions,
            native_effects,
            repaint_surfaces,
        }
    }
}

fn interaction_close_request(outcome: &InteractionOutcome) -> Option<HostInputOutcome> {
    match outcome {
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
    mut ordered: Vec<(u64, usize, usize, HostInputOutcome)>,
) -> Vec<HostInputOutcome> {
    ordered.sort_by_key(|(ordinal, edge, outcome, _)| (*ordinal, *edge, *outcome));
    ordered
        .into_iter()
        .map(|(_, _, _, outcome)| outcome)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_pointer_ordinal_preserves_edge_then_outcome_order() {
        let outcomes = finish_ordered_inputs(vec![
            (7, 1, 0, HostInputOutcome::NativePlatformSnapshotStale),
            (
                7,
                0,
                1,
                HostInputOutcome::NativeCloseObservationApplied {
                    close_requests: Vec::new(),
                },
            ),
            (
                7,
                0,
                0,
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
    }
}
