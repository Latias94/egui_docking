//! One deadline-driven sampler for the existing committed native gesture.

use dockspace::runtime::{NativePointerContinuation, PointerContinuationToken};
use winit::event::PointerPressToken;

use super::*;
use crate::pointer_observation::NativePointerObservationKey;

#[derive(Debug, Default)]
pub(super) struct NativeStationaryState {
    pub(super) capture: Option<super::capture::NativeCaptureDuty>,
    pub(super) capture_blocked: Option<dockspace::runtime::NativePointerCaptureRequest>,
    prepared: Option<(NativeOutputToken, NativeStationaryPreparation)>,
    attempt: Option<StationaryAttempt>,
    blocked: Option<PointerContinuationToken>,
    #[cfg(all(test, target_os = "linux"))]
    recorded_samples: usize,
}

pub(super) struct NativeStationaryPreparation {
    request: NativePointerContinuation,
    key: NativePointerObservationKey,
    expected_press: Option<PointerPressToken>,
    acquisition: bool,
    command:
        eframe::PreparedNativeViewportCommand<eframe::NativeViewportPointerObservationCommandToken>,
}

impl std::fmt::Debug for NativeStationaryPreparation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeStationaryPreparation")
            .field("request", &self.request)
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

#[derive(Debug)]
struct StationaryAttempt {
    request: NativePointerContinuation,
    key: NativePointerObservationKey,
    expected_press: Option<PointerPressToken>,
    acquisition: bool,
}

fn same_active_press<I: Eq>(expected: Option<&I>, current: Option<&I>) -> bool {
    matches!((expected, current), (Some(expected), Some(current)) if expected == current)
}

impl NativeStationaryState {
    pub(super) fn has_pending_work(&self) -> bool {
        self.attempt.is_some() || self.capture.is_some()
    }
}

impl NativeCoordinator {
    pub(super) fn rearm_stationary_after_capture_acquisition(
        &mut self,
        request: dockspace::runtime::NativePointerCaptureRequest,
    ) {
        if self.session.native_pointer_capture_request() == Some(request)
            && let Some(continuation) = self.session.native_pointer_continuation()
            && continuation.pointer() == request.pointer()
            && continuation.source_binding() == request.source_binding()
            && self.stationary.blocked == Some(continuation.token())
        {
            // A newly settled native acquisition permits one fresh sample.
            // Its own failure blocks the unchanged challenge again.
            self.stationary.blocked = None;
        }
    }

    pub(crate) fn stage_stationary_observation(
        &mut self,
        context: &eframe::egui::Context,
        token: NativeOutputToken,
        budget: &mut NativeInputBudget,
    ) {
        if self.stationary.prepared.is_none()
            && let Some(prepared) = self.prepare_stationary_observation(context, budget)
        {
            self.stationary.prepared = Some((token, prepared));
        }
    }

    pub(crate) fn abandon_stationary_output(&mut self, token: NativeOutputToken) {
        if self
            .stationary
            .prepared
            .as_ref()
            .is_some_and(|(owner, _)| *owner == token)
        {
            self.stationary.prepared = None;
        }
    }

    pub(super) fn discard_stationary_preparation(&mut self) {
        self.stationary.prepared = None;
    }

    pub(super) fn finish_stationary_observation(&mut self) {
        if self
            .stationary
            .prepared
            .as_ref()
            .is_some_and(|(owner, _)| self.bridge.output_reservation(*owner).is_none())
        {
            self.stationary.prepared = None;
        }
        let Some(attempt) = self.stationary.attempt.as_ref() else {
            return;
        };
        if !self.bridge.pointer_observation_pending(attempt.key) {
            // An unavailable result, unchanged challenge, or rejected sample must
            // not continuously reschedule its own deadline or terminal wake.
            self.stationary.blocked = Some(attempt.request.token());
            self.stationary.attempt = None;
        }
    }

    pub(crate) fn schedule_stationary_observation(
        &mut self,
        context: &eframe::egui::Context,
        token: NativeOutputToken,
        budget: &mut NativeInputBudget,
    ) -> Result<(), NativeRuntimeError> {
        let staged = self
            .stationary
            .prepared
            .as_ref()
            .is_some_and(|(owner, _)| *owner == token);
        let prepared = if staged {
            self.stationary
                .prepared
                .take()
                .map(|(_, prepared)| prepared)
        } else {
            self.prepare_stationary_observation(context, budget)
        };
        if let Some(command) =
            prepared.and_then(|prepared| self.admit_stationary_observation(prepared))
        {
            command.enqueue(context);
        } else if budget.is_exhausted()
            && self.stationary.prepared.is_none()
            && self.stationary.attempt.is_none()
            && self.bridge.accepts_new_work()
            && let Some(request) = self.session.native_pointer_continuation()
            && self.stationary.blocked != Some(request.token())
            && request.local_coordinates().is_none()
            && self
                .pointer_translator
                .mouse_press_for_pointer(request.pointer())
                .is_some()
        {
            let now = self.bridge.observation_time();
            if now < request.deadline() {
                context.request_repaint_after_for(request.deadline() - now, ViewportId::ROOT);
            } else if !self.bridge.require_output_wake_after_current(token) {
                return Err(NativeHostProtocolError::OutputBudgetUnavailable.into());
            }
        }
        Ok(())
    }

    pub(super) fn prepare_stationary_observation(
        &mut self,
        context: &eframe::egui::Context,
        budget: &mut NativeInputBudget,
    ) -> Option<NativeStationaryPreparation> {
        self.finish_stationary_observation();
        if self.stationary.prepared.is_some()
            || self.stationary.attempt.is_some()
            || budget.is_exhausted()
            || !self.bridge.accepts_new_work()
            || self.has_pending_callback_records()
            || self.bridge.callback_boundary_pending()
        {
            return None;
        }
        let request = self.session.native_pointer_continuation()?;
        if request.local_coordinates().is_some()
            || self.stationary.blocked == Some(request.token())
            || self.capture_acquisition_pending_for(request)
        {
            return None;
        }
        let (device, expected_press) = self
            .pointer_translator
            .mouse_press_for_pointer(request.pointer())?;
        let expected_press = expected_press.cloned();
        let (viewport, window) = self.exact_window_route_for_binding(request.source_binding())?;
        let now = self.bridge.observation_time();
        let acquisition =
            self.capture_is_acquired_for(request) && request.capture_acquired_event().is_some();
        if !acquisition && now < request.deadline() {
            context.request_repaint_after_for(request.deadline() - now, ViewportId::ROOT);
            return None;
        }
        // Pay before application painting can exhaust the dispatch budget.
        // This unqueued command owns no callback or presentation authority.
        let command = eframe::prepare_native_viewport_pointer_observation(
            viewport,
            window,
            device,
            expected_press.clone(),
        );
        let key = NativePointerObservationKey {
            token: command.token(),
            viewport,
            window,
            device,
        };
        budget.complete_unsequenced_records(1);
        Some(NativeStationaryPreparation {
            request,
            key,
            expected_press,
            acquisition,
            command,
        })
    }

    pub(super) fn admit_stationary_observation(
        &mut self,
        prepared: NativeStationaryPreparation,
    ) -> Option<
        eframe::PreparedNativeViewportCommand<eframe::NativeViewportPointerObservationCommandToken>,
    > {
        let NativeStationaryPreparation {
            request,
            key,
            expected_press,
            acquisition,
            command,
        } = prepared;
        if self.stationary.attempt.is_some()
            || !self.bridge.accepts_new_work()
            || self.has_pending_callback_records()
            || self.bridge.callback_boundary_pending()
            || self.session.native_pointer_continuation() != Some(request)
            || self.capture_acquisition_pending_for(request)
            || self.stationary.blocked == Some(request.token())
            || self
                .pointer_translator
                .mouse_press_for_pointer(request.pointer())
                != Some((key.device, expected_press.as_ref()))
            || self.exact_window_route_for_binding(request.source_binding())
                != Some((key.viewport, key.window))
            || (!acquisition && self.bridge.observation_time() < request.deadline())
            || (acquisition && !self.capture_is_acquired_for(request))
            || !self
                .bridge
                .reserve_pointer_observation(key, request.source_binding())
        {
            return None;
        }
        self.stationary.attempt = Some(StationaryAttempt {
            request,
            key,
            expected_press,
            acquisition,
        });
        Some(command)
    }

    #[cfg(test)]
    pub(super) fn pending_stationary_key(&self) -> Option<NativePointerObservationKey> {
        self.stationary.attempt.as_ref().map(|attempt| attempt.key)
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn stationary_observation_for_test(
        &self,
    ) -> Option<(
        NativePointerContinuation,
        NativePointerObservationKey,
        Option<PointerPressToken>,
    )> {
        self.stationary
            .attempt
            .as_ref()
            .map(|attempt| (attempt.request, attempt.key, attempt.expected_press.clone()))
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(crate) fn stationary_recorded_sample_count_for_test(&self) -> usize {
        self.stationary.recorded_samples
    }

    pub(super) fn reduce_pointer_observation(
        &mut self,
        record: crate::pointer_observation::NativePointerObservationRecord,
        captured_at: std::time::Duration,
    ) -> Result<(), NativeRuntimeError> {
        let input = if let Some(attempt) = self.stationary.attempt.as_ref()
            && self.bridge.accepts_new_work()
            && attempt.key == record.key()
            && self.session.native_pointer_continuation() == Some(attempt.request)
            && let Some((device, current_press)) = self
                .pointer_translator
                .mouse_press_for_pointer(attempt.request.pointer())
            && device == attempt.key.device
            && same_active_press(attempt.expected_press.as_ref(), current_press)
            && self.exact_window_route_for_binding(attempt.request.source_binding())
                == Some((attempt.key.viewport, attempt.key.window))
            && !self.capture_acquisition_pending_for(attempt.request)
        {
            let work_areas = &self.work_areas;
            // A query prepared before the acquire receipt may execute after it.
            // Its first fresh evidence must restart the interval, even when the
            // old ordinary deadline is already due.
            let acquisition = attempt.acquisition
                || (self.capture_is_acquired_for(attempt.request)
                    && attempt.request.capture_acquired_event().is_some());
            let event = if acquisition {
                attempt.request.capture_acquired_event()
            } else if captured_at >= attempt.request.deadline() {
                Some(attempt.request.event())
            } else {
                None
            };
            event.and_then(|event| {
                record.into_input_event(attempt.request, event, captured_at, |point| {
                    work_areas.binding_at(point)
                })
            })
        } else {
            None
        };
        if let Some(input) = input {
            self.record_pointer(input)?;
            #[cfg(all(test, target_os = "linux"))]
            {
                self.stationary.recorded_samples += 1;
            }
        }
        if !self.bridge.acknowledge_pointer_observation(record.key()) {
            return Err(NativeHostProtocolError::WindowEventAcknowledgementMismatch.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::same_active_press;

    #[test]
    fn press_identity_requires_two_present_equal_values() {
        assert!(same_active_press(Some(&1), Some(&1)));
        assert!(!same_active_press(Some(&1), Some(&2)));
        assert!(!same_active_press(Some(&1), None));
        assert!(!same_active_press(None, Some(&1)));
        assert!(!same_active_press::<u64>(None, None));
    }
}
