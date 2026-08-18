//! Core-owned settlement of runtime retention obligations.

use super::*;

impl DockEngine {
    pub(super) fn retained_presentation_streams(&self) -> BTreeSet<HostPresentationStreamId> {
        self.retained_presentation_emissions()
            .into_iter()
            .map(HostFrameKey::stream)
            .collect()
    }

    /// Derives every concrete emission still referenced by core presentation authority.
    pub(super) fn retained_presentation_emissions(&self) -> BTreeSet<HostFrameKey> {
        let mut emissions = self
            .presentation_authority
            .presentation
            .pending_output_keys()
            .chain(
                self.presentation_authority
                    .scene
                    .retained_interaction_emissions(),
            )
            .collect::<BTreeSet<_>>();

        if let Some(active) = self.interaction.active_presentation_authority() {
            emissions.insert(active.presented.emission());
        }
        if let Some(pending) = &self.pending_drag_release {
            if let Some(presentation) = pending.drag.presentation.presented() {
                emissions.insert(presentation.presented.emission());
            }
            emissions.extend(pending.presentation_outputs.iter().copied());
            emissions.extend(pending.presented_output);
        }
        if let Some(pending) = &self.pending_contained_transform_release {
            if let Some(presentation) = pending.transform.presentation.presented() {
                emissions.insert(presentation.presented.emission());
            }
            emissions.extend(pending.presentation_outputs.iter().copied());
            emissions.extend(pending.presented_output);
        }
        if let Some(pending) = &self.pending_presentation_rehome {
            emissions.insert(pending.source_presentation.emission());
            emissions.extend(pending.presentation_outputs.iter().copied());
            emissions.extend(pending.presented_output);
        }
        emissions.extend(
            self.viewport
                .retained_native_staging_resources()
                .map(|resource| resource.source_presentation().emission()),
        );
        emissions
    }

    /// Reclaims detailed retired-host state only after every exact core reference disappears.
    ///
    /// Frozen host-frame capabilities are intentionally not blockers. Their core-minted serials
    /// remain represented by the compact retirement ranges and therefore continue to fail closed.
    pub(super) fn settle_retired_presentation_hosts(&mut self) -> Result<usize, EngineError> {
        let retained_streams = self.retained_presentation_streams();
        let backend_host = self
            .backend_ingress
            .active()
            .map(BackendIngressLease::presentation_host);
        let pointer_host = self
            .pointer_journal
            .active_lease()
            .and_then(|lease| lease.scope().surface_local().map(|local| local.host()));
        let candidates = self
            .presentation_authority
            .presentation
            .detailed_retired_host_rosters()
            .map(|(host, streams)| (host, streams.clone()))
            .collect::<Vec<_>>();
        let mut compacted = 0;

        for (host, streams) in candidates {
            if backend_host == Some(host)
                || pointer_host == Some(host)
                || streams
                    .iter()
                    .any(|stream| retained_streams.contains(stream))
            {
                continue;
            }
            compacted += usize::from(
                self.presentation_authority
                    .presentation
                    .compact_retired_host(host)
                    .map_err(presentation_ledger_error)?,
            );
        }
        Ok(compacted)
    }
}
