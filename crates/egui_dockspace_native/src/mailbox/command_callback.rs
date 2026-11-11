//! One pre-dispatched command reserves one exact terminal callback.

use dockspace::runtime::NativeSurfaceBinding;
use eframe::{
    NativeViewportFocusCommandToken, NativeViewportPointerPassthroughCommandToken,
    NativeViewportPropertyCommandRetirement, NativeViewportVisibilityCommandToken,
    egui::ViewportId,
};
use winit::window::WindowId;

use super::{HostRecord, journal::ReservedHostRecord};
use crate::focus_control::NativeViewportFocusRecord;
use crate::input_control::NativePointerPassthroughRecord;
use crate::pointer_capture::{NativePointerCaptureKey, NativePointerCaptureRecord};
use crate::pointer_observation::{NativePointerObservationKey, NativePointerObservationRecord};
use crate::viewport_callback::NativeViewportVisibilityRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum CommandCallbackAdmission {
    Unreserved,
    First,
    Duplicate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReceiptState {
    Awaiting,
    Received,
    Transferred,
    Committed,
}

#[derive(Debug)]
struct CallbackReceipt<Key> {
    key: Key,
    state: ReceiptState,
    ingress: Option<ReservedHostRecord>,
}

#[derive(Debug)]
struct CallbackSlot<Key> {
    receipt: Option<CallbackReceipt<Key>>,
}

impl<Key> Default for CallbackSlot<Key> {
    fn default() -> Self {
        Self { receipt: None }
    }
}

impl<Key: PartialEq> CallbackSlot<Key> {
    fn is_empty(&self) -> bool {
        self.receipt.is_none()
    }

    fn has_pending_work(&self) -> bool {
        self.receipt
            .as_ref()
            .is_some_and(|receipt| receipt.state != ReceiptState::Committed)
    }

    fn reserve(&mut self, key: Key) -> bool {
        if let Some(receipt) = &self.receipt {
            if receipt.key == key {
                return true;
            }
            if receipt.state != ReceiptState::Committed {
                return false;
            }
        }
        self.receipt = Some(CallbackReceipt {
            key,
            state: ReceiptState::Awaiting,
            ingress: None,
        });
        true
    }

    fn contains(&self, key: &Key) -> bool {
        self.receipt
            .as_ref()
            .is_some_and(|receipt| &receipt.key == key)
    }

    fn accept(
        &mut self,
        key: Key,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let Some(receipt) = &mut self.receipt else {
            return CommandCallbackAdmission::Unreserved;
        };
        if receipt.key != key {
            return CommandCallbackAdmission::Unreserved;
        }
        if receipt.state != ReceiptState::Awaiting {
            return CommandCallbackAdmission::Duplicate;
        }
        let ingress = reserve();
        receipt.ingress = Some(ingress);
        receipt.state = ReceiptState::Received;
        CommandCallbackAdmission::First
    }

    fn ingress(&self) -> Option<&ReservedHostRecord> {
        self.receipt.as_ref()?.ingress.as_ref()
    }

    fn take_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        let receipt = self.receipt.as_mut()?;
        if receipt.state != ReceiptState::Received || receipt.ingress.as_ref()?.ordinal() != ordinal
        {
            return None;
        }
        let ingress = receipt.ingress.take()?;
        receipt.state = ReceiptState::Transferred;
        Some(ingress)
    }

    fn commit_transferred(&mut self) {
        if let Some(receipt) = &mut self.receipt
            && receipt.state == ReceiptState::Transferred
        {
            receipt.state = ReceiptState::Committed;
        }
    }
}

#[derive(Debug, Default)]
pub(super) struct NativeCommandCallbacks {
    pointer: CallbackSlot<(NativeViewportPointerPassthroughCommandToken, WindowId, bool)>,
    pointer_binding: Option<NativeSurfaceBinding>,
    focus: CallbackSlot<(NativeViewportFocusCommandToken, WindowId)>,
    visibility: CallbackSlot<(NativeViewportVisibilityCommandToken, WindowId, bool)>,
    observation: CallbackSlot<NativePointerObservationKey>,
    observation_binding: Option<NativeSurfaceBinding>,
    observation_record: Option<NativePointerObservationRecord>,
    capture: CallbackSlot<NativePointerCaptureKey>,
    capture_binding: Option<NativeSurfaceBinding>,
    capture_record: Option<NativePointerCaptureRecord>,
}

impl NativeCommandCallbacks {
    pub(super) fn is_empty(&self) -> bool {
        self.pointer.is_empty()
            && self.focus.is_empty()
            && self.visibility.is_empty()
            && self.observation.is_empty()
            && self.capture.is_empty()
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.pointer.has_pending_work()
            || self.focus.has_pending_work()
            || self.visibility.has_pending_work()
            || self.observation.has_pending_work()
            || self.capture.has_pending_work()
    }

    pub(super) fn reserve_observation(
        &mut self,
        key: NativePointerObservationKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if self
            .observation
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.token == key.token && receipt.key != key)
        {
            return false;
        }
        if self.observation.contains(&key) {
            return self.observation_binding == Some(binding);
        }
        if !self.observation.reserve(key) {
            return false;
        }
        self.observation_binding = Some(binding);
        self.observation_record = None;
        true
    }

    pub(super) fn has_observation_token(
        &self,
        token: eframe::NativeViewportPointerObservationCommandToken,
    ) -> bool {
        self.observation
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.token == token)
    }

    pub(super) fn matches_observation(&self, key: NativePointerObservationKey) -> bool {
        self.observation.contains(&key)
    }

    pub(super) fn observation_binding(&self) -> Option<NativeSurfaceBinding> {
        self.observation_binding
    }

    pub(super) fn observation_pending(&self, key: NativePointerObservationKey) -> bool {
        self.observation.contains(&key) && self.observation.has_pending_work()
    }

    pub(super) fn has_pending_observation(&self) -> bool {
        self.observation.has_pending_work()
    }

    pub(super) fn awaiting_observation(&self) -> Option<NativePointerObservationKey> {
        self.observation
            .receipt
            .as_ref()
            .filter(|receipt| receipt.state == ReceiptState::Awaiting)
            .map(|receipt| receipt.key)
    }

    pub(super) fn observation_record(
        &self,
        key: NativePointerObservationKey,
    ) -> Option<NativePointerObservationRecord> {
        self.observation
            .contains(&key)
            .then_some(self.observation_record)
            .flatten()
    }

    pub(super) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.observation_record
            .is_some_and(|record| record.references_binding(binding))
            || self
                .capture_record
                .is_some_and(|record| record.references_binding(binding))
    }

    /// Window hover and capture facts cannot retain an unrelated detached sibling.
    /// The exact dispatched source remains pinned until its callback commits.
    pub(super) fn references_placement(&self, binding: NativeSurfaceBinding) -> bool {
        (self.pointer_binding == Some(binding) && self.pointer.has_pending_work())
            || (self.observation_binding == Some(binding) && self.observation.has_pending_work())
            || (self.capture_binding == Some(binding) && self.capture.has_pending_work())
    }

    pub(super) fn accept_observation(
        &mut self,
        record: NativePointerObservationRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        if self.observation_binding != Some(record.binding()) {
            return CommandCallbackAdmission::Unreserved;
        }
        let admission = self.observation.accept(record.key(), reserve);
        if admission == CommandCallbackAdmission::First {
            self.observation_record = Some(record);
        }
        admission
    }

    pub(super) fn reserve_capture(
        &mut self,
        key: NativePointerCaptureKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        if self
            .capture
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.token == key.token && receipt.key != key)
        {
            return false;
        }
        if self.capture.contains(&key) {
            return self.capture_binding == Some(binding);
        }
        if !self.capture.reserve(key) {
            return false;
        }
        self.capture_binding = Some(binding);
        self.capture_record = None;
        true
    }

    pub(super) fn has_capture_token(
        &self,
        token: eframe::NativeViewportPointerCaptureCommandToken,
    ) -> bool {
        self.capture
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.token == token)
    }

    pub(super) fn matches_capture(&self, key: NativePointerCaptureKey) -> bool {
        self.capture.contains(&key)
    }

    pub(super) fn capture_binding(&self) -> Option<NativeSurfaceBinding> {
        self.capture_binding
    }

    #[cfg(all(test, target_os = "linux"))]
    pub(super) fn capture_pending(&self, key: NativePointerCaptureKey) -> bool {
        self.capture.contains(&key) && self.capture.has_pending_work()
    }

    pub(super) fn has_pending_capture(&self) -> bool {
        self.capture.has_pending_work()
    }

    pub(super) fn awaiting_capture(&self) -> Option<NativePointerCaptureKey> {
        self.capture
            .receipt
            .as_ref()
            .filter(|receipt| receipt.state == ReceiptState::Awaiting)
            .map(|receipt| receipt.key)
    }

    pub(super) fn capture_record(
        &self,
        key: NativePointerCaptureKey,
    ) -> Option<NativePointerCaptureRecord> {
        self.capture
            .contains(&key)
            .then_some(self.capture_record)
            .flatten()
    }

    pub(super) fn accept_capture(
        &mut self,
        record: NativePointerCaptureRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        if self.capture_binding != Some(record.binding()) {
            return CommandCallbackAdmission::Unreserved;
        }
        let admission = self.capture.accept(record.key(), reserve);
        if admission == CommandCallbackAdmission::First {
            self.capture_record = Some(record);
        }
        admission
    }

    pub(super) fn reserve_pointer(
        &mut self,
        token: NativeViewportPointerPassthroughCommandToken,
        window: WindowId,
        enabled: bool,
        binding: NativeSurfaceBinding,
    ) -> bool {
        // A repeated opaque token cannot be rebound to another target or value.
        if self.pointer.receipt.as_ref().is_some_and(|receipt| {
            receipt.key.0 == token && receipt.key != (token, window, enabled)
        }) {
            return false;
        }
        if self.pointer.contains(&(token, window, enabled)) {
            return self.pointer_binding == Some(binding);
        }
        if !self.pointer.reserve((token, window, enabled)) {
            return false;
        }
        self.pointer_binding = Some(binding);
        true
    }

    pub(super) fn pointer_binding(&self) -> Option<NativeSurfaceBinding> {
        self.pointer_binding
    }

    pub(super) fn accept_pointer(
        &mut self,
        record: NativePointerPassthroughRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        self.pointer
            .accept((record.token(), record.window(), record.enabled()), reserve)
    }

    pub(super) fn matches_pointer(&self, record: NativePointerPassthroughRecord) -> bool {
        self.pointer
            .contains(&(record.token(), record.window(), record.enabled()))
    }

    pub(super) fn has_pointer_token(
        &self,
        token: NativeViewportPointerPassthroughCommandToken,
    ) -> bool {
        self.pointer
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.0 == token)
    }

    pub(super) fn reserve_focus(
        &mut self,
        token: NativeViewportFocusCommandToken,
        window: WindowId,
    ) -> bool {
        if self
            .focus
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.0 == token && receipt.key.1 != window)
        {
            return false;
        }
        self.focus.reserve((token, window))
    }

    pub(super) fn accept_focus(
        &mut self,
        record: NativeViewportFocusRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        self.focus
            .accept((record.token(), record.window()), reserve)
    }

    pub(super) fn matches_focus(&self, record: NativeViewportFocusRecord) -> bool {
        self.focus.contains(&(record.token(), record.window()))
    }

    pub(super) fn has_focus_token(&self, token: NativeViewportFocusCommandToken) -> bool {
        self.focus
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.0 == token)
    }

    pub(super) fn reserve_visibility(
        &mut self,
        token: NativeViewportVisibilityCommandToken,
        window: WindowId,
        visible: bool,
    ) -> bool {
        if self.visibility.receipt.as_ref().is_some_and(|receipt| {
            receipt.key.0 == token && receipt.key != (token, window, visible)
        }) {
            return false;
        }
        self.visibility.reserve((token, window, visible))
    }

    pub(super) fn accept_visibility(
        &mut self,
        record: NativeViewportVisibilityRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        self.visibility
            .accept((record.token(), record.window(), record.visible()), reserve)
    }

    pub(super) fn matches_visibility(
        &self,
        token: NativeViewportVisibilityCommandToken,
        window: WindowId,
        visible: bool,
    ) -> bool {
        self.visibility.contains(&(token, window, visible))
    }

    pub(super) fn has_visibility_token(&self, token: NativeViewportVisibilityCommandToken) -> bool {
        self.visibility
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.key.0 == token)
    }

    pub(super) fn matches_property_retirement(
        &self,
        retirement: &NativeViewportPropertyCommandRetirement,
    ) -> bool {
        match retirement {
            NativeViewportPropertyCommandRetirement::PointerObservation {
                viewport_id,
                token,
                expected_window_id,
                device_id,
            } => self.matches_observation(NativePointerObservationKey {
                token: *token,
                viewport: *viewport_id,
                window: *expected_window_id,
                device: *device_id,
            }),
            NativeViewportPropertyCommandRetirement::PointerCapture {
                viewport_id,
                token,
                expected_window_id,
                device_id,
                action,
            } => self.matches_capture(NativePointerCaptureKey {
                token: *token,
                viewport: *viewport_id,
                window: *expected_window_id,
                device: *device_id,
                action: *action,
            }),
            NativeViewportPropertyCommandRetirement::Focus { token, .. } => {
                self.has_focus_token(*token)
            }
            NativeViewportPropertyCommandRetirement::Visibility { token, visible, .. } => self
                .visibility
                .receipt
                .as_ref()
                .is_some_and(|receipt| receipt.key.0 == *token && receipt.key.2 == *visible),
            NativeViewportPropertyCommandRetirement::PointerPassthrough {
                token, enabled, ..
            } => self
                .pointer
                .receipt
                .as_ref()
                .is_some_and(|receipt| receipt.key.0 == *token && receipt.key.2 == *enabled),
        }
    }

    pub(super) fn retire_property_command(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        retirement: NativeViewportPropertyCommandRetirement,
        make_ingress: impl FnOnce(HostRecord) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        match retirement {
            NativeViewportPropertyCommandRetirement::PointerObservation {
                viewport_id,
                token,
                expected_window_id,
                device_id,
            } => {
                let key = NativePointerObservationKey {
                    token,
                    viewport: viewport_id,
                    window: expected_window_id,
                    device: device_id,
                };
                if viewport_id != viewport {
                    return CommandCallbackAdmission::Unreserved;
                }
                let Some(binding) = self.observation_binding else {
                    return CommandCallbackAdmission::Unreserved;
                };
                self.accept_observation(
                    NativePointerObservationRecord::retired(key, binding),
                    || make_ingress(HostRecord::PointerObservation(key)),
                )
            }
            NativeViewportPropertyCommandRetirement::PointerCapture {
                viewport_id,
                token,
                expected_window_id,
                device_id,
                action,
            } => {
                let key = NativePointerCaptureKey {
                    token,
                    viewport: viewport_id,
                    window: expected_window_id,
                    device: device_id,
                    action,
                };
                if viewport_id != viewport {
                    return CommandCallbackAdmission::Unreserved;
                }
                let Some(binding) = self.capture_binding else {
                    return CommandCallbackAdmission::Unreserved;
                };
                self.accept_capture(NativePointerCaptureRecord::retired(key, binding), || {
                    make_ingress(HostRecord::PointerCapture(key))
                })
            }
            NativeViewportPropertyCommandRetirement::Focus { viewport_id, token } => {
                let Some(receipt) = self.focus.receipt.as_ref() else {
                    return CommandCallbackAdmission::Unreserved;
                };
                if viewport_id != viewport || receipt.key.0 != token {
                    return CommandCallbackAdmission::Unreserved;
                }
                let record = NativeViewportFocusRecord::retired_before_dispatch(
                    token,
                    viewport,
                    receipt.key.1,
                    binding,
                );
                self.accept_focus(record, || make_ingress(HostRecord::ViewportFocus(record)))
            }
            NativeViewportPropertyCommandRetirement::Visibility {
                viewport_id,
                token,
                visible,
            } => {
                let Some(receipt) = self.visibility.receipt.as_ref() else {
                    return CommandCallbackAdmission::Unreserved;
                };
                if viewport_id != viewport || receipt.key.0 != token || receipt.key.2 != visible {
                    return CommandCallbackAdmission::Unreserved;
                }
                let record = NativeViewportVisibilityRecord::retired_before_dispatch(
                    token,
                    viewport,
                    receipt.key.1,
                    binding,
                    visible,
                );
                self.accept_visibility(record, || {
                    make_ingress(HostRecord::ViewportVisibility(record))
                })
            }
            NativeViewportPropertyCommandRetirement::PointerPassthrough {
                viewport_id,
                token,
                enabled,
            } => {
                let Some(receipt) = self.pointer.receipt.as_ref() else {
                    return CommandCallbackAdmission::Unreserved;
                };
                if viewport_id != viewport || receipt.key.0 != token || receipt.key.2 != enabled {
                    return CommandCallbackAdmission::Unreserved;
                }
                let Some(binding) = self.pointer_binding else {
                    return CommandCallbackAdmission::Unreserved;
                };
                let record = NativePointerPassthroughRecord::retired_before_dispatch(
                    token,
                    viewport,
                    receipt.key.1,
                    binding,
                    enabled,
                );
                self.accept_pointer(record, || {
                    make_ingress(HostRecord::ViewportPointerPassthrough(record))
                })
            }
        }
    }

    pub(super) fn callback_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        [
            self.pointer.ingress(),
            self.focus.ingress(),
            self.visibility.ingress(),
            self.observation.ingress(),
            self.capture.ingress(),
        ]
        .into_iter()
        .flatten()
    }

    pub(super) fn take_callback_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.pointer
            .take_ingress(ordinal)
            .or_else(|| self.focus.take_ingress(ordinal))
            .or_else(|| self.visibility.take_ingress(ordinal))
            .or_else(|| self.observation.take_ingress(ordinal))
            .or_else(|| self.capture.take_ingress(ordinal))
    }

    pub(super) fn commit_transferred(&mut self) {
        self.pointer.commit_transferred();
        self.focus.commit_transferred();
        self.visibility.commit_transferred();
        self.observation.commit_transferred();
        self.capture.commit_transferred();
        if self
            .observation
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.state == ReceiptState::Committed)
        {
            self.observation_record = None;
        }
        if self
            .capture
            .receipt
            .as_ref()
            .is_some_and(|receipt| receipt.state == ReceiptState::Committed)
        {
            self.capture_record = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use dockspace::runtime::{HostInputOutcome, HostWindowToken, NativeMonotonicClock};
    use eframe::{NativeViewportPointerPassthroughStatus, egui};

    use super::super::{HostRecord, journal::HostIngressJournal};
    use super::*;

    fn binding() -> NativeSurfaceBinding {
        use dockspace::model::{
            DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
            RootId, SurfaceId,
        };
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SurfaceId::new(1),
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .unwrap();
        let mut session = dockspace::runtime::DockspaceSession::from_layout(
            layout,
            dockspace::policy::DockPolicy::default(),
        )
        .unwrap();
        session.enable_observed_native_roots().unwrap();
        session
            .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(dockspace::runtime::SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame
            .commit()
            .unwrap()
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .unwrap()
    }

    fn token(context: &egui::Context) -> NativeViewportPointerPassthroughCommandToken {
        eframe::queue_native_viewport_pointer_passthrough(context, egui::ViewportId::ROOT, true)
    }

    fn record(
        token: NativeViewportPointerPassthroughCommandToken,
    ) -> NativePointerPassthroughRecord {
        NativePointerPassthroughRecord::for_test(
            token,
            egui::ViewportId::ROOT,
            WindowId::from(11),
            None,
            true,
            NativeViewportPointerPassthroughStatus::Applied,
        )
    }

    #[test]
    fn received_callback_does_not_close_its_reservation_before_exact_handoff_and_commit() {
        let binding = binding();
        let context = egui::Context::default();
        let first = token(&context);
        let next = token(&context);
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let mut callbacks = NativeCommandCallbacks::default();
        assert!(callbacks.reserve_pointer(first, WindowId::from(11), true, binding));
        assert_eq!(
            callbacks.accept_pointer(record(first), || {
                journal.reserve(HostRecord::ViewportPointerPassthrough(record(first)))
            }),
            CommandCallbackAdmission::First
        );
        assert_eq!(
            callbacks.accept_pointer(record(first), || panic!(
                "a duplicate cannot reserve ingress"
            )),
            CommandCallbackAdmission::Duplicate
        );
        assert_eq!(
            callbacks.accept_pointer(record(next), || panic!(
                "an unreserved callback cannot reserve ingress"
            )),
            CommandCallbackAdmission::Unreserved
        );
        callbacks.commit_transferred();
        assert!(
            callbacks.has_pending_work(),
            "an unrelated commit cannot settle a raw callback"
        );
        assert!(!callbacks.reserve_pointer(next, WindowId::from(11), true, binding));
        let ordinal = callbacks.callback_ingress().next().unwrap().ordinal();
        assert_eq!(journal.captured_through(), ordinal);
        assert!(callbacks.take_callback_ingress(ordinal + 1).is_none());
        callbacks.commit_transferred();
        assert!(callbacks.has_pending_work());

        let ingress = callbacks
            .take_callback_ingress(ordinal)
            .expect("exact ingress transfers");
        assert_eq!(ingress.ordinal(), ordinal);
        assert!(callbacks.callback_ingress().next().is_none());
        assert!(callbacks.take_callback_ingress(ordinal).is_none());
        assert!(
            callbacks.has_pending_work(),
            "handoff alone is not publication"
        );
        assert!(!callbacks.reserve_pointer(next, WindowId::from(11), true, binding));
        callbacks.commit_transferred();
        assert!(!callbacks.has_pending_work());
        assert_eq!(
            callbacks.accept_pointer(record(first), || panic!(
                "a committed duplicate cannot reserve ingress"
            )),
            CommandCallbackAdmission::Duplicate
        );
        assert!(callbacks.reserve_pointer(next, WindowId::from(11), true, binding));
        assert_eq!(
            callbacks.accept_pointer(record(next), || {
                journal.reserve(HostRecord::ViewportPointerPassthrough(record(next)))
            }),
            CommandCallbackAdmission::First
        );
        assert_eq!(journal.captured_through(), ordinal + 1);
    }

    #[test]
    fn wrong_window_or_value_cannot_consume_the_reserved_callback() {
        let binding = binding();
        let context = egui::Context::default();
        let token = token(&context);
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let mut callbacks = NativeCommandCallbacks::default();
        assert!(callbacks.reserve_pointer(token, WindowId::from(11), true, binding));
        for (window, enabled) in [(WindowId::from(12), true), (WindowId::from(11), false)] {
            let wrong = NativePointerPassthroughRecord::for_test(
                token,
                egui::ViewportId::ROOT,
                window,
                None,
                enabled,
                NativeViewportPointerPassthroughStatus::Applied,
            );
            assert_eq!(
                callbacks.accept_pointer(wrong, || panic!(
                    "a foreign callback cannot reserve ingress"
                )),
                CommandCallbackAdmission::Unreserved
            );
        }
        assert!(!callbacks.reserve_pointer(token, WindowId::from(12), true, binding));
        assert!(!callbacks.reserve_pointer(token, WindowId::from(11), false, binding));
        assert_eq!(
            callbacks.accept_pointer(record(token), || {
                journal.reserve(HostRecord::ViewportPointerPassthrough(record(token)))
            }),
            CommandCallbackAdmission::First
        );
        assert_eq!(journal.captured_through(), 1);
    }
}
