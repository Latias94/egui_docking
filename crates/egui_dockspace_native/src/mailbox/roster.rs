//! Immutable root-roster payloads shared by ordinary and protected ingress.

use std::mem::size_of;
use std::sync::Arc;

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{
    NativeViewportRoster, NativeWindowSnapshot, NativeWindowingBackend, egui::ViewportId,
};

use crate::error::NativeHostProtocolError;
use crate::viewport_map::NativeViewportMap;
#[cfg(test)]
use crate::window_snapshot::CompiledWindowObservation;
use crate::work_area::{FrozenWorkArea, FrozenWorkAreaRoster};

/// Frozen dockspace observations captured from one complete root-window roster.
#[derive(Debug, Clone)]
pub(crate) struct NativeViewportRosterRecord {
    observations: Vec<FrozenWindowObservation>,
    work_areas: Result<FrozenWorkAreaRoster, NativeHostProtocolError>,
    work_area_spare: Vec<FrozenWorkArea>,
    work_area_capacity_exceeded: bool,
    backend: NativeWindowingBackend,
    #[cfg(test)]
    compiled_override: Option<Result<Vec<CompiledWindowObservation>, NativeHostProtocolError>>,
}

impl PartialEq for NativeViewportRosterRecord {
    fn eq(&self, other: &Self) -> bool {
        let equal = self.observations == other.observations
            && self.work_areas == other.work_areas
            && self.backend == other.backend;
        #[cfg(test)]
        let equal = equal && self.compiled_override == other.compiled_override;
        equal
    }
}

/// One callback-time window snapshot paired with its exact dockspace binding.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrozenWindowObservation {
    viewport: ViewportId,
    window_id: winit::window::WindowId,
    binding: NativeSurfaceBinding,
    snapshot: NativeWindowSnapshot,
}

impl FrozenWindowObservation {
    const fn new(
        viewport: ViewportId,
        window_id: winit::window::WindowId,
        binding: NativeSurfaceBinding,
        snapshot: NativeWindowSnapshot,
    ) -> Self {
        Self {
            viewport,
            window_id,
            binding,
            snapshot,
        }
    }

    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }

    pub(crate) const fn snapshot(self) -> NativeWindowSnapshot {
        self.snapshot
    }
}

impl NativeViewportRosterRecord {
    pub(super) fn with_capacity(window_slots: usize, work_area_slots: usize) -> Self {
        Self {
            observations: Vec::with_capacity(window_slots),
            work_areas: Ok(FrozenWorkAreaRoster::Unknown),
            work_area_spare: Vec::with_capacity(work_area_slots),
            work_area_capacity_exceeded: false,
            backend: NativeWindowingBackend::Other,
            #[cfg(test)]
            compiled_override: None,
        }
    }

    /// Counts only exact mapped windows; unrelated backend windows need no storage.
    pub(super) fn mapped_window_count(
        roster: NativeViewportRoster<'_>,
        viewports: &NativeViewportMap,
    ) -> usize {
        roster
            .records()
            .iter()
            .filter(|record| {
                viewports
                    .binding_for_roster(record.viewport_id(), record.window_id())
                    .is_some()
            })
            .count()
    }

    /// Fills preallocated storage without growing either vector. Window overflow
    /// rejects the whole capture before mutation; display overflow revokes only
    /// work-area authority and keeps every mapped window observation.
    pub(super) fn capture_into(
        &mut self,
        roster: NativeViewportRoster<'_>,
        viewports: &NativeViewportMap,
    ) -> bool {
        if Self::mapped_window_count(roster, viewports) > self.observations.capacity() {
            return false;
        }
        self.observations.clear();
        for record in roster.records() {
            let Some(binding) =
                viewports.binding_for_roster(record.viewport_id(), record.window_id())
            else {
                continue;
            };
            self.observations.push(FrozenWindowObservation::new(
                record.viewport_id(),
                record.window_id(),
                binding,
                record.window(),
            ));
        }
        self.observations
            .sort_unstable_by_key(|observation| observation.binding().surface());
        if let Ok(FrozenWorkAreaRoster::Exact(storage)) =
            std::mem::replace(&mut self.work_areas, Ok(FrozenWorkAreaRoster::Unknown))
        {
            self.work_area_spare = storage;
        }
        (self.work_areas, self.work_area_capacity_exceeded) =
            FrozenWorkAreaRoster::capture_reusing(roster.work_areas(), &mut self.work_area_spare);
        self.backend = roster.backend();
        #[cfg(test)]
        {
            self.compiled_override = None;
        }
        true
    }

    pub(super) fn window_capacity(&self) -> usize {
        self.observations.capacity()
    }

    /// Fixed payload plus owned vector capacities, not allocator metadata or RSS.
    pub(super) fn owned_payload_capacity_bytes(&self) -> usize {
        let active_work_area_capacity = match &self.work_areas {
            Ok(FrozenWorkAreaRoster::Exact(records)) => records.capacity(),
            _ => 0,
        };
        let bytes = size_of::<Self>()
            .saturating_add(
                self.observations
                    .capacity()
                    .saturating_mul(size_of::<FrozenWindowObservation>()),
            )
            .saturating_add(
                active_work_area_capacity
                    .saturating_add(self.work_area_spare.capacity())
                    .saturating_mul(size_of::<FrozenWorkArea>()),
            );
        #[cfg(test)]
        let bytes = bytes.saturating_add(match &self.compiled_override {
            Some(Ok(records)) => records
                .capacity()
                .saturating_mul(size_of::<CompiledWindowObservation>()),
            _ => 0,
        });
        bytes
    }

    pub(super) const fn work_area_capacity_exceeded(&self) -> bool {
        self.work_area_capacity_exceeded
    }

    /// Exact fact comparison for ordinary borrowed preflight. No allocation or
    /// hash substitutes for geometry, backend or complete work-area equality.
    pub(super) fn matches_borrowed(
        &self,
        roster: NativeViewportRoster<'_>,
        viewports: &NativeViewportMap,
    ) -> bool {
        if self.backend != roster.backend()
            || self.observations.len() != Self::mapped_window_count(roster, viewports)
        {
            return false;
        }
        #[cfg(test)]
        if self.compiled_override.is_some() {
            return false;
        }
        for expected in &self.observations {
            let matches = roster.records().iter().filter(|record| {
                record.viewport_id() == expected.viewport
                    && record.window_id() == expected.window_id
                    && record.window() == expected.snapshot
                    && viewports.binding_for_roster(record.viewport_id(), record.window_id())
                        == Some(expected.binding)
            });
            if matches.count() != 1 {
                return false;
            }
        }
        if self.work_area_capacity_exceeded
            && matches!(roster.work_areas(), eframe::NativeWorkAreaRoster::Exact(records)
                if records.len() > self.work_area_spare.capacity())
        {
            // Oversized display inventories consistently revoke work-area
            // authority while preserving the complete window inventory.
            return true;
        }
        match &self.work_areas {
            Ok(expected) => expected.matches_borrowed(roster.work_areas()),
            Err(expected) => {
                FrozenWorkAreaRoster::validate_borrowed(roster.work_areas())
                    .err()
                    .as_ref()
                    == Some(expected)
            }
        }
    }

    pub(crate) fn observations(&self) -> &[FrozenWindowObservation] {
        &self.observations
    }

    pub(crate) fn work_areas(&self) -> Result<&FrozenWorkAreaRoster, NativeHostProtocolError> {
        self.work_areas.as_ref().map_err(|error| *error)
    }

    pub(crate) const fn backend(&self) -> NativeWindowingBackend {
        self.backend
    }

    #[cfg(test)]
    pub(crate) fn compiled_override(
        &self,
    ) -> Option<Result<Vec<CompiledWindowObservation>, NativeHostProtocolError>> {
        self.compiled_override.clone()
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        observations: impl IntoIterator<Item = CompiledWindowObservation>,
        facts_valid: bool,
    ) -> Self {
        Self {
            observations: Vec::new(),
            work_areas: Ok(FrozenWorkAreaRoster::Unknown),
            work_area_spare: Vec::new(),
            work_area_capacity_exceeded: false,
            // Coordinator tests model the fully managed desktop contract. The
            // capability roster is immutable after the first snapshot, so a
            // deterministic desktop backend keeps later synthetic rosters
            // compatible with that contract.
            backend: NativeWindowingBackend::Windows,
            compiled_override: Some(if facts_valid {
                Ok(observations.into_iter().collect())
            } else {
                Err(NativeHostProtocolError::InvalidWindowSnapshot)
            }),
        }
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_work_areas(
        observations: impl IntoIterator<Item = CompiledWindowObservation>,
        work_areas: FrozenWorkAreaRoster,
    ) -> Self {
        Self::for_test_with_backend_and_work_areas(
            observations,
            work_areas,
            NativeWindowingBackend::Windows,
        )
    }

    #[cfg(test)]
    pub(crate) fn for_test_with_backend_and_work_areas(
        observations: impl IntoIterator<Item = CompiledWindowObservation>,
        work_areas: FrozenWorkAreaRoster,
        backend: NativeWindowingBackend,
    ) -> Self {
        Self {
            observations: Vec::new(),
            work_areas: Ok(work_areas),
            work_area_spare: Vec::new(),
            work_area_capacity_exceeded: false,
            backend,
            compiled_override: Some(Ok(observations.into_iter().collect())),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct NativeViewportRosterKey {
    pub(super) context: u64,
    pub(super) roster: Arc<NativeViewportRosterRecord>,
}

impl NativeViewportRosterKey {
    fn from_shared(context: u64, roster: Arc<NativeViewportRosterRecord>) -> Self {
        Self { context, roster }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct NativeViewportRosterEnvelope {
    pub(super) key: NativeViewportRosterKey,
    pub(super) commit_baseline: bool,
    pub(super) terminal_generation: Option<u64>,
}

impl NativeViewportRosterEnvelope {
    pub(crate) const fn shared_roster(&self) -> &Arc<NativeViewportRosterRecord> {
        &self.key.roster
    }

    pub(super) fn from_shared(
        context: u64,
        roster: Arc<NativeViewportRosterRecord>,
        commit_baseline: bool,
        terminal_generation: Option<u64>,
    ) -> Self {
        Self {
            key: NativeViewportRosterKey::from_shared(context, roster),
            commit_baseline,
            terminal_generation,
        }
    }

    pub(crate) fn roster(&self) -> &NativeViewportRosterRecord {
        &self.key.roster
    }

    pub(crate) const fn is_terminal(&self) -> bool {
        self.terminal_generation.is_some()
    }
}
