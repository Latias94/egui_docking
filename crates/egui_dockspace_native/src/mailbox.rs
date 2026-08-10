//! Ordered callback mailbox for the fork-backed native host.

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputResult, NativeOutputToken, NativeWindowEvent,
};

use crate::event::NativeWindowEventRecord;
use crate::viewport_map::NativeViewportMap;

#[derive(Debug, Clone)]
pub(crate) enum HostRecord {
    WindowEvent(NativeWindowEventRecord),
    Output {
        result: NativeOutputResult,
        submitted: bool,
    },
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct OutputReservation {
    binding: Option<NativeSurfaceBinding>,
    terminal_recorded: bool,
}

impl OutputReservation {
    pub(crate) const fn unbound() -> Self {
        Self {
            binding: None,
            terminal_recorded: false,
        }
    }

    pub(crate) fn attach(&mut self, binding: NativeSurfaceBinding) -> bool {
        if let Some(existing) = self.binding {
            return existing == binding;
        }
        self.binding = Some(binding);
        true
    }

    #[cfg(test)]
    pub(crate) const fn binding(self) -> Option<NativeSurfaceBinding> {
        self.binding
    }
}

#[derive(Debug)]
struct HostRecords {
    active: bool,
    journal: VecDeque<HostRecord>,
    output_reservations: BTreeMap<NativeOutputToken, OutputReservation>,
    event_boundary_pending: bool,
}

impl HostRecords {
    fn active() -> Self {
        Self {
            active: true,
            journal: VecDeque::new(),
            output_reservations: BTreeMap::new(),
            event_boundary_pending: false,
        }
    }

    fn reserve_output(&mut self, token: NativeOutputToken) -> bool {
        if !self.active || self.output_reservations.contains_key(&token) {
            return false;
        }
        self.output_reservations
            .insert(token, OutputReservation::unbound());
        true
    }

    fn attach_output_binding(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(reservation) = self.output_reservations.get_mut(&token) else {
            return false;
        };
        reservation.attach(binding)
    }

    fn record_output(&mut self, result: NativeOutputResult) -> bool {
        let Some(reservation) = self.output_reservations.get_mut(&result.token()) else {
            return false;
        };
        if !self.active || reservation.terminal_recorded {
            return false;
        }
        reservation.terminal_recorded = true;
        self.journal.push_back(HostRecord::Output {
            result,
            submitted: false,
        });
        true
    }

    fn deactivate(&mut self) {
        self.active = false;
        self.journal.clear();
        self.output_reservations.clear();
        self.event_boundary_pending = false;
    }
}

#[derive(Debug)]
pub(crate) struct NativeHostBridge {
    records: Mutex<HostRecords>,
    viewports: Arc<Mutex<NativeViewportMap>>,
}

impl NativeHostBridge {
    pub(crate) fn new(viewports: Arc<Mutex<NativeViewportMap>>) -> Self {
        Self {
            records: Mutex::new(HostRecords::active()),
            viewports,
        }
    }

    fn lock(&self) -> MutexGuard<'_, HostRecords> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_viewports(&self) -> MutexGuard<'_, NativeViewportMap> {
        self.viewports
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    pub(crate) fn front_event(&self) -> Option<NativeWindowEventRecord> {
        match self.lock().journal.front() {
            Some(HostRecord::WindowEvent(event)) => Some(event.clone()),
            Some(HostRecord::Output { .. }) | None => None,
        }
    }

    pub(crate) fn acknowledge_event(&self, ordinal: u64) -> bool {
        let mut records = self.lock();
        let matches = matches!(
            records.journal.front(),
            Some(HostRecord::WindowEvent(event)) if event.ordinal() == ordinal
        );
        if matches {
            records.journal.pop_front();
            records.event_boundary_pending = true;
        }
        matches
    }

    pub(crate) fn output_prefix(&self) -> Vec<(NativeOutputResult, bool)> {
        let records = self.lock();
        if records.event_boundary_pending {
            return Vec::new();
        }
        let outputs: Vec<_> = records
            .journal
            .iter()
            .map_while(|record| match record {
                HostRecord::Output { result, submitted } => Some((*result, *submitted)),
                HostRecord::WindowEvent(_) => None,
            })
            .collect();

        // Preserve the single callback journal order. Output ordinals are
        // context-local generation identities, not a cross-viewport clock;
        // comparing them here would let one viewport's result overtake an
        // older event or another context's output.
        outputs
    }

    pub(crate) fn reserve_output(&self, token: NativeOutputToken) -> bool {
        self.lock().reserve_output(token)
    }

    pub(crate) fn has_output_reservation(&self, token: NativeOutputToken) -> bool {
        self.lock().output_reservations.contains_key(&token)
    }

    pub(crate) fn attach_output_binding(
        &self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.lock().attach_output_binding(token, binding)
    }

    pub(crate) fn mark_output_submitted(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        let Some(HostRecord::Output { result, submitted }) = records.journal.iter_mut().find(
            |record| matches!(record, HostRecord::Output { result, .. } if result.token() == token),
        ) else {
            return false;
        };
        if result.token() != token {
            return false;
        }
        *submitted = true;
        true
    }

    pub(crate) fn abandon_output(&self, token: NativeOutputToken) -> bool {
        let mut records = self.lock();
        if records.journal.iter().any(|record| {
            matches!(
                record,
                HostRecord::Output {
                    result,
                    submitted: true,
                } if result.token() == token
            )
        }) {
            return false;
        }
        let previous_len = records.journal.len();
        records.journal.retain(|record| {
            !matches!(record, HostRecord::Output { result, .. } if result.token() == token)
        });
        let removed_reservation = records.output_reservations.remove(&token).is_some();
        removed_reservation || records.journal.len() != previous_len
    }

    pub(crate) fn commit_frame_boundary(&self) {
        let mut records = self.lock();
        records.event_boundary_pending = false;
        while matches!(
            records.journal.front(),
            Some(HostRecord::Output {
                submitted: true,
                ..
            })
        ) {
            let Some(HostRecord::Output { result, .. }) = records.journal.pop_front() else {
                unreachable!("matched output record must still be at the journal head");
            };
            records.output_reservations.remove(&result.token());
        }
    }

    pub(crate) fn deactivate(&self) {
        self.lock().deactivate();
    }

    #[cfg(test)]
    pub(crate) fn push_record(&self, record: HostRecord) {
        self.lock().journal.push_back(record);
    }

    #[cfg(test)]
    pub(crate) fn event_boundary_pending(&self) -> bool {
        self.lock().event_boundary_pending
    }
}

impl NativeHostHandler for NativeHostBridge {
    fn on_window_event(&self, event: NativeWindowEvent<'_>) {
        let record = NativeWindowEventRecord::from_eframe(event, &self.lock_viewports());
        let mut records = self.lock();
        if records.active {
            records.journal.push_back(HostRecord::WindowEvent(record));
        }
    }

    fn on_output_begin(&self, token: NativeOutputToken) {
        self.reserve_output(token);
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        if !self.lock().record_output(result) {
            return NativeHostWake::Wait;
        }
        NativeHostWake::RepaintRoot
    }
}
