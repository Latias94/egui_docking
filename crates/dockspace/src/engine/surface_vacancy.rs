use std::collections::{BTreeMap, BTreeSet};

use crate::engine::surface_recovery::{
    SurfaceRecoveryObligation, SurfaceRecoveryObligationId, SurfaceRecoveryTarget,
};
use crate::engine::transition::InputOutcome;
use crate::graph::Workspace;
use crate::graph::error::CommandError;
use crate::ids::{InputSequence, SurfaceId};
use crate::platform::frame::SurfaceVacancyAuthority;
use crate::platform::viewport::ViewportBinding;
use crate::policy::{
    DockPolicyRequest, DockPolicySnapshot, DockPresentationMode, DockSurfaceRecoveryPolicyRequest,
    DockSurfaceRecoveryRootFacts, PolicyDecision,
};

use super::{DockEngine, EngineError};

/// Exact binding history needed to settle physical resources from the final
/// logical surface roster of one reducer tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TickVacancyLedger {
    start: BTreeMap<SurfaceId, SurfaceVacancyAuthority>,
    /// Latest exact binding observed while a surface belonged to the logical roster.
    ///
    /// This includes pending native-create bindings. A first-live transfer can
    /// add and vacate a surface in one reducer tick before first-live
    /// admission; that physical resource is still a tick-final obligation.
    observed: BTreeMap<SurfaceId, SurfaceVacancyAuthority>,
    /// Tick-start bindings which stopped owning their surface during this tick.
    departed_start_bindings: BTreeSet<ViewportBinding>,
    /// Bindings whose retirement was already owned by workspace reconciliation.
    reconciled_retirements: BTreeSet<ViewportBinding>,
}

impl TickVacancyLedger {
    pub(super) fn capture(engine: &DockEngine) -> Self {
        let start = engine
            .workspace
            .surfaces()
            .map(|(surface, _)| {
                (
                    surface,
                    engine.viewport.capture_surface_vacancy_authority(surface),
                )
            })
            .collect();
        Self {
            start,
            observed: BTreeMap::new(),
            departed_start_bindings: BTreeSet::new(),
            reconciled_retirements: BTreeSet::new(),
        }
    }

    pub(super) fn observe_workspace_reconciliation(&mut self, outcome: &InputOutcome) {
        let InputOutcome::WorkspaceReplaced { reconciliation, .. } = outcome else {
            return;
        };
        self.reconciled_retirements
            .extend(reconciliation.retired().iter().copied());
        self.reconciled_retirements.extend(
            reconciliation
                .rebound()
                .iter()
                .map(|(previous, _)| *previous),
        );
    }

    pub(super) fn observe_bindings(&mut self, engine: &DockEngine) {
        for (surface, _) in engine.workspace.surfaces() {
            let record = engine.viewport.viewport(surface);
            if let Some(start_binding) = self
                .start
                .get(&surface)
                .and_then(|authority| authority.binding())
                && record.is_none_or(|record| record.binding() != start_binding)
            {
                self.departed_start_bindings.insert(start_binding);
            }
            let Some(record) = record else {
                continue;
            };
            if self
                .observed
                .get(&surface)
                .and_then(|authority| authority.binding())
                != Some(record.binding())
            {
                self.observed.insert(
                    surface,
                    engine.viewport.capture_surface_vacancy_authority(surface),
                );
            }
        }
    }

    pub(super) fn vacant_authorities(&self, engine: &DockEngine) -> Vec<SurfaceVacancyAuthority> {
        let surfaces = self
            .start
            .keys()
            .chain(self.observed.keys())
            .copied()
            .collect::<BTreeSet<_>>();
        surfaces
            .into_iter()
            .filter(|surface| engine.workspace.surface(*surface).is_none())
            .filter_map(|surface| {
                let start = self.start.get(&surface).copied();
                let start_was_reconciled = start
                    .and_then(SurfaceVacancyAuthority::binding)
                    .is_some_and(|binding| self.reconciled_retirements.contains(&binding));
                let start_departed = start
                    .and_then(SurfaceVacancyAuthority::binding)
                    .is_some_and(|binding| self.departed_start_bindings.contains(&binding));
                let latest_observation = self.observed.get(&surface).copied();
                match start {
                    Some(authority)
                        if authority.has_binding() && !start_was_reconciled && !start_departed =>
                    {
                        Some(authority)
                    }
                    Some(authority) if start_departed => latest_observation.or(Some(authority)),
                    _ => latest_observation.or(start),
                }
            })
            .filter(|authority| {
                authority
                    .binding()
                    .is_none_or(|binding| !self.reconciled_retirements.contains(&binding))
            })
            .collect()
    }
}

impl DockEngine {
    pub(super) fn recovery_policy_request(
        workspace: &Workspace,
        source_surface: crate::ids::SurfaceId,
        host_surface: crate::ids::SurfaceId,
    ) -> Result<DockSurfaceRecoveryPolicyRequest, CommandError> {
        let presentation =
            workspace
                .surface(source_surface)
                .ok_or(CommandError::MissingSurface {
                    surface: source_surface,
                })?;
        let mut roots = BTreeMap::new();
        if let Some(root) = presentation.main_root {
            Self::insert_recovery_root_facts(
                workspace,
                &mut roots,
                root,
                DockPresentationMode::Native,
            )?;
        }
        for floating in &presentation.contained {
            let contained =
                workspace
                    .contained_floating(*floating)
                    .ok_or(CommandError::MissingFloating {
                        floating: *floating,
                    })?;
            Self::insert_recovery_root_facts(
                workspace,
                &mut roots,
                contained.root,
                DockPresentationMode::Contained,
            )?;
        }
        Ok(DockSurfaceRecoveryPolicyRequest::new(
            source_surface,
            host_surface,
            roots,
        ))
    }

    pub(super) fn insert_recovery_root_facts(
        workspace: &Workspace,
        roots: &mut BTreeMap<crate::ids::RootId, DockSurfaceRecoveryRootFacts>,
        root: crate::ids::RootId,
        presentation: DockPresentationMode,
    ) -> Result<(), CommandError> {
        let record = workspace
            .root(root)
            .ok_or(CommandError::MissingRoot { root })?;
        let facts = DockSurfaceRecoveryRootFacts::new(
            presentation,
            workspace.collect_items_in_subtree(record.node),
        );
        if roots.insert(root, facts).is_some() {
            return Err(CommandError::Invariant {
                stage: "normalize surface recovery roots",
            });
        }
        Ok(())
    }

    pub(super) fn next_surface_recovery_obligation_id(
        &self,
        input: InputSequence,
    ) -> Result<SurfaceRecoveryObligationId, EngineError> {
        self.last_surface_recovery_obligation
            .checked_next()
            .ok_or(EngineError::SurfaceRecoveryObligationExhausted { input })
    }

    pub(super) fn authorize_surface_recovery_obligation(
        &self,
        input: InputSequence,
        id: SurfaceRecoveryObligationId,
        workspace: &Workspace,
        source_surface: crate::ids::SurfaceId,
        target: SurfaceRecoveryTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<SurfaceRecoveryObligation, CommandError> {
        if workspace.surface(target.host_surface()).is_none() {
            return Err(CommandError::SurfaceLifecycleFrozen {
                surface: target.host_surface(),
            });
        }
        let request =
            Self::recovery_policy_request(workspace, source_surface, target.host_surface())?;
        self.authorize_surface_recovery_obligation_for_request(input, id, request, target, policy)
    }

    pub(super) fn authorize_surface_recovery_obligation_for_request(
        &self,
        _input: InputSequence,
        id: SurfaceRecoveryObligationId,
        request: DockSurfaceRecoveryPolicyRequest,
        target: SurfaceRecoveryTarget,
        policy: &DockPolicySnapshot,
    ) -> Result<SurfaceRecoveryObligation, CommandError> {
        if let PolicyDecision::Reject(reason) =
            policy.evaluate(&DockPolicyRequest::RecoverSurface(request.clone()))
        {
            return Err(CommandError::Policy(reason));
        }
        SurfaceRecoveryObligation::new(
            id,
            self.authority_domain,
            request,
            target,
            policy.revision(),
        )
        .map_err(|_| CommandError::SurfaceLifecycleFrozen {
            surface: target.host_surface(),
        })
    }
}
