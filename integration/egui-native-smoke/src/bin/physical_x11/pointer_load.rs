//! Finite native-event and release-latency measurement for the X11 smoke.

use std::num::NonZeroUsize;
use std::sync::PoisonError;
use std::time::{Duration, Instant};

use dockspace::runtime::{
    NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit,
};
use eframe::egui::ViewportId;
use egui_dockspace_native::NativeStorageLimit;
use winit::event::{DeviceId, ElementState, MouseButton, WindowEvent};
use winit::window::WindowId;

use super::app::{ProductPhase, Shared, SharedState};

pub(super) const POINTER_LOAD_ENV: &str = "EGUI_DOCKSPACE_PHYSICAL_LOAD";
pub(super) const POINTER_LOAD_MOVES: u32 = 1_000;
const LOAD_BYTE_LIMIT: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PointerSource {
    viewport: ViewportId,
    window: WindowId,
    device: DeviceId,
}

/// One smoke-only measurement, not another input owner or event history.
#[derive(Debug)]
pub(super) struct PointerLoad {
    source: PointerSource,
    roster_epoch: u64,
    points: [super::assertions::PhysicalPoint; 5],
    moves: usize,
    first_move: Option<Instant>,
    last_move: Option<Instant>,
    release: Option<Instant>,
    model_commit: Option<Instant>,
    injected: usize,
    injection_elapsed: Duration,
    pub(super) max_root_update: Duration,
    owned_peak: usize,
    reserved_peak: usize,
    invalidation: Option<&'static str>,
    storage_sample_failed: bool,
}

impl PointerLoad {
    fn invalidate(&mut self, reason: &'static str) {
        if self.model_commit.is_none() {
            self.invalidation.get_or_insert(reason);
        }
    }

    pub(super) fn observe_model(&mut self, center_merge_verified: bool, now: Instant) {
        if self.invalidation.is_none() && center_merge_verified {
            if self.release.is_some() {
                self.model_commit.get_or_insert(now);
            } else {
                self.invalidate("center merge was observed before the measured release");
            }
        }
    }

    pub(super) fn observe_storage(&mut self, owned: Option<usize>, reserved: Option<usize>) {
        if let (Some(owned), Some(reserved)) = (owned, reserved)
            && owned <= reserved
            && reserved <= LOAD_BYTE_LIMIT
        {
            self.owned_peak = self.owned_peak.max(owned);
            self.reserved_peak = self.reserved_peak.max(reserved);
        } else {
            self.storage_sample_failed = true;
        }
    }
}

impl SharedState {
    pub(super) fn observe_pointer_event(
        &mut self,
        viewport: Option<ViewportId>,
        window: WindowId,
        event: &WindowEvent,
        now: Instant,
    ) {
        if matches!(event, WindowEvent::Destroyed) {
            if self
                .last_primary_press
                .is_some_and(|source| source.window == window)
            {
                self.last_primary_press = None;
            }
            if let Some(load) = &mut self.pointer_load
                && load.source.window == window
            {
                load.invalidate("the source window was destroyed before model commit");
            }
            return;
        }
        let device = match event {
            WindowEvent::CursorMoved { device_id, .. } => *device_id,
            WindowEvent::MouseInput {
                device_id,
                button: MouseButton::Left,
                state,
                ..
            } => {
                if *state == ElementState::Pressed {
                    if let Some(load) = &mut self.pointer_load {
                        load.invalidate("a new primary press interrupted the measured gesture");
                    }
                    self.last_primary_press = viewport.map(|viewport| PointerSource {
                        viewport,
                        window,
                        device: *device_id,
                    });
                } else if self.last_primary_press
                    == viewport.map(|viewport| PointerSource {
                        viewport,
                        window,
                        device: *device_id,
                    })
                {
                    self.last_primary_press = None;
                }
                *device_id
            }
            _ => return,
        };
        let Some(load) = &mut self.pointer_load else {
            return;
        };
        if viewport != Some(load.source.viewport)
            || window != load.source.window
            || device != load.source.device
            || load.release.is_some()
            || load.invalidation.is_some()
        {
            return;
        }
        match event {
            WindowEvent::CursorMoved { facts, .. } => {
                if load.moves >= POINTER_LOAD_MOVES as usize {
                    load.invalidate("an extra native move entered the measured interval");
                    return;
                }
                let expected = if load.moves == POINTER_LOAD_MOVES as usize - 1 {
                    load.points[4]
                } else {
                    load.points[load.moves % 4]
                };
                if !facts.desktop_position.is_some_and(|point| {
                    point.x == f64::from(expected.x) && point.y == f64::from(expected.y)
                }) {
                    load.invalidate("native move did not match the injected desktop sequence");
                    return;
                }
                load.moves += 1;
                load.first_move.get_or_insert(now);
                load.last_move = Some(now);
            }
            WindowEvent::MouseInput {
                state: ElementState::Released,
                ..
            } => {
                if load.injected != POINTER_LOAD_MOVES as usize {
                    load.invalidate("the measured release preceded injection completion");
                    return;
                }
                load.release = Some(now);
            }
            _ => {}
        }
    }

    pub(super) fn observe_device_removal(&mut self) {
        self.last_primary_press = None;
        if let Some(load) = &mut self.pointer_load {
            // Raw-device removal cannot be correlated with window-input DeviceId values.
            load.invalidate("a device was removed before model commit");
        }
    }
}

pub(super) fn pointer_load_enabled() -> Result<bool, String> {
    match std::env::var(POINTER_LOAD_ENV) {
        Err(std::env::VarError::NotPresent) => Ok(false),
        Ok(value) if value == "1khz" => Ok(true),
        value => Err(format!(
            "{POINTER_LOAD_ENV}: expected 1khz or an unset variable, found {value:?}"
        )),
    }
}

pub(super) fn begin_pointer_load(
    shared: &Shared,
    viewport: ViewportId,
    points: [super::assertions::PhysicalPoint; 5],
) -> Result<(), String> {
    let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
    if state.pointer_load.is_some() {
        return Err("only one physical pointer load may run".to_owned());
    }
    if state.observation.phase != ProductPhase::ChildFirstLive
        || state.observation.center_merge_verified
    {
        return Err("physical load requires a separate first-live child before redock".to_owned());
    }
    let source = state
        .last_primary_press
        .filter(|source| source.viewport == viewport)
        .ok_or_else(|| {
            "load has no exact native primary press on its source viewport".to_owned()
        })?;
    state.pointer_load = Some(PointerLoad {
        source,
        roster_epoch: state.roster_epoch,
        points,
        moves: 0,
        first_move: None,
        last_move: None,
        release: None,
        model_commit: None,
        injected: 0,
        injection_elapsed: Duration::ZERO,
        max_root_update: Duration::ZERO,
        owned_peak: 0,
        reserved_peak: 0,
        invalidation: None,
        storage_sample_failed: false,
    });
    Ok(())
}

pub(super) fn finish_pointer_load(
    shared: &Shared,
    injected: usize,
    elapsed: Duration,
) -> Result<(), String> {
    let mut state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
    let roster_epoch = state.roster_epoch;
    let load = state
        .pointer_load
        .as_mut()
        .ok_or("physical load was not started")?;
    if load.injected != 0 || injected != POINTER_LOAD_MOVES as usize || elapsed.is_zero() {
        return Err("physical load must finish exactly once after 1,000 injected moves".to_owned());
    }
    if roster_epoch != load.roster_epoch {
        load.invalidate("the native roster changed during the injected sequence");
    }
    if let Some(reason) = load.invalidation {
        return Err(format!("physical load was invalidated: {reason}"));
    }
    load.injected = injected;
    load.injection_elapsed = elapsed;
    Ok(())
}

pub(super) fn verify_pointer_load(shared: &Shared) -> Result<(), String> {
    let state = shared.0.lock().unwrap_or_else(PoisonError::into_inner);
    let load = state
        .pointer_load
        .as_ref()
        .ok_or("physical load was not measured")?;
    if let Some(reason) = load.invalidation {
        return Err(format!("physical load was invalidated: {reason}"));
    }
    if load.storage_sample_failed {
        return Err("physical load had an unavailable or invalid storage sample".to_owned());
    }
    let (Some(first), Some(last), Some(release), Some(committed)) = (
        load.first_move,
        load.last_move,
        load.release,
        load.model_commit,
    ) else {
        return Err(format!("incomplete physical load observation: {load:?}"));
    };
    let callback_elapsed = last.duration_since(first);
    let injection_hz = load.injected as f64 / load.injection_elapsed.as_secs_f64();
    let callback_hz = load.moves.saturating_sub(1) as f64 / callback_elapsed.as_secs_f64();
    let latency = committed
        .checked_duration_since(release)
        .ok_or("model commit preceded release")?;
    // The model changes only after typed delivery and output settlement. Measuring from the
    // earlier native callback to the later root observation conservatively bounds that interval.
    eprintln!(
        "physical-load: injected={} rate={injection_hz:.1}Hz matched_native_callbacks={} rate={callback_hz:.1}Hz release_to_model_upper_bound={latency:?} max_root_ui_update={:?} root_sampled_owned_max={} root_sampled_reserved_max={} limit={LOAD_BYTE_LIMIT}",
        load.injected, load.moves, load.max_root_update, load.owned_peak, load.reserved_peak
    );
    if load.moves != load.injected
        || load.injected != POINTER_LOAD_MOVES as usize
        || !injection_hz.is_finite()
        || !callback_hz.is_finite()
        || injection_hz < 900.0
        || callback_hz < 900.0
        || latency > Duration::from_millis(100)
        || load.owned_peak == 0
        || load.owned_peak > load.reserved_peak
        || load.reserved_peak > LOAD_BYTE_LIMIT
    {
        return Err(format!(
            "physical load missed its callback/rate/latency/storage contract: {load:?}"
        ));
    }
    Ok(())
}

pub(super) fn load_storage_limit()
-> Result<NativeStorageLimit, egui_dockspace_native::NativeRuntimeError> {
    let slots = |count| NonZeroUsize::new(count).expect("physical smoke limits are nonzero");
    let input = NativeInputStorageLimit::new(slots(512), 768 * 1024)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            slots(16),
            slots(8),
            128 * 1024,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            slots(64),
            slots(16),
            256 * 1024,
        ));
    NativeStorageLimit::new(input, LOAD_BYTE_LIMIT, 48, 32, 16)
}

#[cfg(test)]
mod tests {
    use super::super::app::ProductObservation;
    use super::super::assertions::PhysicalPoint;
    use super::*;
    use std::sync::{Arc, Condvar, Mutex};
    use winit::dpi::PhysicalPosition;
    use winit::event::PointerEventFacts;

    fn load_fixture() -> (Shared, PointerSource, [PhysicalPoint; 5]) {
        let source = PointerSource {
            viewport: ViewportId::ROOT,
            window: WindowId::dummy(),
            device: DeviceId::dummy(),
        };
        let points = [(98, 98), (102, 98), (102, 102), (98, 102), (100, 100)]
            .map(|(x, y)| PhysicalPoint { x, y });
        let mut state = SharedState::default();
        state.observation = ProductObservation {
            phase: ProductPhase::ChildFirstLive,
            ..Default::default()
        };
        state.observe_pointer_event(
            Some(source.viewport),
            source.window,
            &primary_event(source, ElementState::Pressed),
            Instant::now(),
        );
        let shared = Arc::new((Mutex::new(state), Condvar::new()));
        begin_pointer_load(&shared, source.viewport, points).unwrap();
        (shared, source, points)
    }

    fn primary_event(source: PointerSource, state: ElementState) -> WindowEvent {
        WindowEvent::MouseInput {
            device_id: source.device,
            state,
            button: MouseButton::Left,
            facts: PointerEventFacts::default(),
        }
    }

    fn motion_event(source: PointerSource, point: PhysicalPoint) -> WindowEvent {
        let position = PhysicalPosition::new(f64::from(point.x), f64::from(point.y));
        WindowEvent::CursorMoved {
            device_id: source.device,
            position,
            facts: PointerEventFacts {
                desktop_position: Some(position),
                ..Default::default()
            },
        }
    }

    #[test]
    fn load_matches_exact_native_sequence_and_seals_before_source_retirement() {
        let (shared, source, points) = load_fixture();
        let started = Instant::now();
        {
            let mut state = shared.0.lock().unwrap();
            // Another delivery window cannot add to this measurement.
            state.observe_pointer_event(
                Some(source.viewport),
                WindowId::from(u64::from(source.window).wrapping_add(1)),
                &motion_event(source, points[0]),
                started,
            );
            for index in 0..POINTER_LOAD_MOVES as usize {
                let point = if index == POINTER_LOAD_MOVES as usize - 1 {
                    points[4]
                } else {
                    points[index % 4]
                };
                state.observe_pointer_event(
                    Some(source.viewport),
                    source.window,
                    &motion_event(source, point),
                    started + Duration::from_millis(index as u64 + 1),
                );
            }
        }
        finish_pointer_load(&shared, POINTER_LOAD_MOVES as usize, Duration::from_secs(1)).unwrap();
        {
            let mut state = shared.0.lock().unwrap();
            state.observe_pointer_event(
                Some(source.viewport),
                source.window,
                &primary_event(source, ElementState::Released),
                started + Duration::from_millis(1_001),
            );
            let load = state.pointer_load.as_mut().unwrap();
            load.observe_model(true, started + Duration::from_millis(1_020));
            load.observe_storage(Some(1_024), Some(2_048));
            state.observe_pointer_event(
                Some(source.viewport),
                source.window,
                &WindowEvent::Destroyed,
                started + Duration::from_millis(1_021),
            );
            state.observe_device_removal();
        }
        verify_pointer_load(&shared).unwrap();
    }

    #[test]
    fn missing_native_move_cannot_be_replaced_by_a_later_point() {
        let (shared, source, points) = load_fixture();
        let mut state = shared.0.lock().unwrap();
        state.observe_pointer_event(
            Some(source.viewport),
            source.window,
            &motion_event(source, points[1]),
            Instant::now(),
        );
        let load = state.pointer_load.as_ref().unwrap();
        assert_eq!(load.moves, 0);
        assert_eq!(
            load.invalidation,
            Some("native move did not match the injected desktop sequence")
        );
    }

    #[test]
    fn interrupted_load_cannot_borrow_a_later_model_success() {
        for interruption in 0..3 {
            let (shared, source, _) = load_fixture();
            finish_pointer_load(&shared, POINTER_LOAD_MOVES as usize, Duration::from_secs(1))
                .unwrap();
            let mut state = shared.0.lock().unwrap();
            state.observe_pointer_event(
                Some(source.viewport),
                source.window,
                &primary_event(source, ElementState::Released),
                Instant::now(),
            );
            match interruption {
                0 => state.observe_pointer_event(
                    Some(source.viewport),
                    source.window,
                    &WindowEvent::Destroyed,
                    Instant::now(),
                ),
                1 => state.observe_device_removal(),
                _ => state.observe_pointer_event(
                    Some(ViewportId::from_hash_of("another-viewport")),
                    source.window,
                    &primary_event(source, ElementState::Pressed),
                    Instant::now(),
                ),
            }
            let load = state.pointer_load.as_mut().unwrap();
            load.observe_model(true, Instant::now());
            assert!(load.invalidation.is_some());
            assert_eq!(load.model_commit, None);
            drop(state);
            assert!(verify_pointer_load(&shared).is_err());
        }
    }

    #[test]
    fn a_merge_before_release_cannot_supply_the_measured_terminal() {
        let (shared, source, _) = load_fixture();
        let mut state = shared.0.lock().unwrap();
        state
            .pointer_load
            .as_mut()
            .unwrap()
            .observe_model(true, Instant::now());
        state.observe_pointer_event(
            Some(source.viewport),
            source.window,
            &primary_event(source, ElementState::Released),
            Instant::now(),
        );
        let load = state.pointer_load.as_mut().unwrap();
        load.observe_model(true, Instant::now());
        assert_eq!(load.model_commit, None);
        assert!(load.invalidation.is_some());
    }

    #[test]
    fn a_failed_storage_sample_cannot_be_hidden_by_an_earlier_success() {
        let (shared, _, _) = load_fixture();
        let mut state = shared.0.lock().unwrap();
        let load = state.pointer_load.as_mut().unwrap();
        load.observe_storage(Some(1_024), Some(2_048));
        load.observe_storage(None, Some(2_048));
        load.observe_storage(Some(1_024), Some(2_048));
        assert!(load.storage_sample_failed);
        drop(state);
        assert!(
            verify_pointer_load(&shared)
                .unwrap_err()
                .contains("storage sample")
        );
    }
}
