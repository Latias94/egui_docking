use std::collections::{BTreeMap, BTreeSet};

use crate::frame::SurfaceVacancyAuthority;
use crate::ids::SurfaceId;
use crate::transition::InputOutcome;
use crate::viewport::ViewportBinding;

use super::DockEngine;

/// Exact binding history needed to settle physical resources from the final
/// logical surface roster of one reducer tick.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct TickVacancyLedger {
    start: BTreeMap<SurfaceId, SurfaceVacancyAuthority>,
    /// Latest exact binding observed while a surface belonged to the logical roster.
    ///
    /// This includes pending native-create bindings. A post-show transfer can
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
