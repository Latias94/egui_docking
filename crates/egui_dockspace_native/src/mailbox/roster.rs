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
            && self.work_area_capacity_exceeded == other.work_area_capacity_exceeded
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
    #[cfg(test)]
    pub(super) fn with_capacity(window_slots: usize, work_area_slots: usize) -> Self {
        Self::try_with_capacity(window_slots, work_area_slots)
            .expect("native test roster backing is available")
    }

    /// Reserves both payload buffers before any borrowed facts are captured.
    pub(super) fn try_with_capacity(
        window_slots: usize,
        work_area_slots: usize,
    ) -> Result<Self, NativeHostProtocolError> {
        let error = NativeHostProtocolError::NativeReportStorageUnavailable;
        Self::reservation_bytes(window_slots, work_area_slots).ok_or(error)?;
        let mut observations = Vec::new();
        observations
            .try_reserve_exact(window_slots)
            .map_err(|_| error)?;
        if observations.capacity() > window_slots {
            return Err(error);
        }
        let mut work_area_spare = Vec::new();
        work_area_spare
            .try_reserve_exact(work_area_slots)
            .map_err(|_| error)?;
        if work_area_spare.capacity() > work_area_slots {
            return Err(error);
        }
        Ok(Self {
            observations,
            work_areas: Ok(FrozenWorkAreaRoster::Unknown),
            work_area_spare,
            work_area_capacity_exceeded: false,
            backend: NativeWindowingBackend::Other,
            #[cfg(test)]
            compiled_override: None,
        })
    }

    /// Includes the payload header and its single window/work-area backings.
    pub(super) fn reservation_bytes(window_slots: usize, work_area_slots: usize) -> Option<usize> {
        size_of::<Self>()
            .checked_add(window_slots.checked_mul(size_of::<FrozenWindowObservation>())?)?
            .checked_add(work_area_slots.checked_mul(size_of::<FrozenWorkArea>())?)
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
        window_slots: usize,
        work_area_slots: usize,
    ) -> bool {
        if Self::mapped_window_count(roster, viewports)
            > window_slots.min(self.observations.capacity())
        {
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
        if matches!(roster.work_areas(), eframe::NativeWorkAreaRoster::Exact(records)
            if records.len() > work_area_slots)
        {
            self.work_area_spare.clear();
            self.work_areas = Ok(FrozenWorkAreaRoster::Unknown);
            self.work_area_capacity_exceeded = true;
        } else {
            (self.work_areas, self.work_area_capacity_exceeded) =
                FrozenWorkAreaRoster::capture_reusing(
                    roster.work_areas(),
                    &mut self.work_area_spare,
                );
        }
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
        work_area_slots: usize,
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
        let exact_len = match roster.work_areas() {
            eframe::NativeWorkAreaRoster::Exact(records) => Some(records.len()),
            eframe::NativeWorkAreaRoster::Unknown => None,
        };
        self.matches_normalized_work_areas(exact_len, work_area_slots, || match &self.work_areas {
            Ok(expected) => expected.matches_borrowed(roster.work_areas()),
            Err(expected) => {
                FrozenWorkAreaRoster::validate_borrowed(roster.work_areas())
                    .err()
                    .as_ref()
                    == Some(expected)
            }
        })
    }

    fn matches_normalized_work_areas(
        &self,
        exact_len: Option<usize>,
        work_area_slots: usize,
        compare_facts: impl FnOnce() -> bool,
    ) -> bool {
        let work_area_capacity = match &self.work_areas {
            Ok(FrozenWorkAreaRoster::Exact(records)) => records.capacity(),
            _ => self.work_area_spare.capacity(),
        };
        let exceeds_work_area_capacity =
            exact_len.is_some_and(|len| len > work_area_slots.min(work_area_capacity));
        if self.work_area_capacity_exceeded != exceeds_work_area_capacity {
            return false;
        }
        if exceeds_work_area_capacity {
            // Oversized display inventories consistently revoke work-area
            // authority before validation while preserving the window inventory.
            return true;
        }
        compare_facts()
    }

    pub(crate) fn observations(&self) -> &[FrozenWindowObservation] {
        &self.observations
    }

    pub(crate) fn observation_count(&self) -> usize {
        #[cfg(test)]
        if let Some(Ok(compiled)) = &self.compiled_override {
            return compiled.len();
        }
        self.observations.len()
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
    ) -> Option<Result<&[CompiledWindowObservation], NativeHostProtocolError>> {
        self.compiled_override
            .as_ref()
            .map(|result| result.as_ref().map(Vec::as_slice).map_err(|error| *error))
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
    /// The existing terminal roster backings already pay for this allocation.
    pub(super) prepaid_payload: bool,
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
            prepaid_payload: false,
        }
    }

    pub(crate) fn roster(&self) -> &NativeViewportRosterRecord {
        &self.key.roster
    }

    pub(crate) const fn is_terminal(&self) -> bool {
        self.terminal_generation.is_some()
    }
}

#[cfg(test)]
mod capacity_tests;

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use super::*;

    #[test]
    fn oversized_input_cannot_match_an_old_validation_error() {
        let mut old = NativeViewportRosterRecord::with_capacity(0, 64);
        old.work_areas = Err(NativeHostProtocolError::InvalidWorkAreaRoster);
        assert!(!old.matches_normalized_work_areas(Some(2), 1, || {
            panic!("capture skips validation for an oversized roster, even for equal old errors")
        }));
        old.work_areas = Ok(FrozenWorkAreaRoster::Unknown);
        old.work_area_capacity_exceeded = true;
        assert!(old.matches_normalized_work_areas(Some(2), 1, || {
            panic!("two oversized rosters share their normalized Unknown result")
        }));
        assert!(!old.matches_normalized_work_areas(None, 1, || {
            panic!("Unknown must clear the previous capacity diagnostic")
        }));
    }

    #[test]
    fn normalized_comparison_uses_active_backing_and_compares_facts_at_the_exact_limit() {
        use dockspace::geometry::{PhysicalRect, ScaleFactor};

        let mut old = NativeViewportRosterRecord::with_capacity(0, 0);
        let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap();
        old.work_areas = Ok(FrozenWorkAreaRoster::for_test_exact([(
            1,
            bounds,
            bounds,
            ScaleFactor::new(1.0).unwrap(),
        )]));
        let compared = Cell::new(0);
        for equal in [false, true] {
            assert_eq!(
                old.matches_normalized_work_areas(Some(1), 1, || {
                    compared.set(compared.get() + 1);
                    equal
                }),
                equal
            );
        }
        assert_eq!(compared.get(), 2);
        assert_eq!(old.work_area_spare.capacity(), 0);
    }

    #[test]
    fn clearing_overflow_diagnostics_passes_the_second_source_deduplication() {
        use crate::mailbox::NativeHostBridge;

        let bridge = NativeHostBridge::new(NativeViewportMap::default());
        let mut state = bridge.lock();
        let mut oversized = NativeViewportRosterRecord::with_capacity(0, 64);
        oversized.work_area_capacity_exceeded = true;
        let ordinary_unknown = NativeViewportRosterRecord::with_capacity(0, 64);
        assert_ne!(oversized, ordinary_unknown);
        state.committed_viewport_roster = Some(NativeViewportRosterKey::from_shared(
            state.roster_context,
            Arc::new(oversized),
        ));
        let ordinal = state.journal.captured_through();
        assert!(state.record_viewport_roster(ordinary_unknown));
        assert_eq!(state.journal.captured_through(), ordinal + 1);
    }
}
