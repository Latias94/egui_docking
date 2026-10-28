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
    focus: CallbackSlot<(NativeViewportFocusCommandToken, WindowId)>,
    visibility: CallbackSlot<(NativeViewportVisibilityCommandToken, WindowId, bool)>,
}

impl NativeCommandCallbacks {
    pub(super) fn is_empty(&self) -> bool {
        self.pointer.is_empty() && self.focus.is_empty() && self.visibility.is_empty()
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.pointer.has_pending_work()
            || self.focus.has_pending_work()
            || self.visibility.has_pending_work()
    }

    pub(super) fn reserve_pointer(
        &mut self,
        token: NativeViewportPointerPassthroughCommandToken,
        window: WindowId,
        enabled: bool,
    ) -> bool {
        // A repeated opaque token cannot be rebound to another target or value.
        if self.pointer.receipt.as_ref().is_some_and(|receipt| {
            receipt.key.0 == token && receipt.key != (token, window, enabled)
        }) {
            return false;
        }
        self.pointer.reserve((token, window, enabled))
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
        ]
        .into_iter()
        .flatten()
    }

    pub(super) fn take_callback_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.pointer
            .take_ingress(ordinal)
            .or_else(|| self.focus.take_ingress(ordinal))
            .or_else(|| self.visibility.take_ingress(ordinal))
    }

    pub(super) fn commit_transferred(&mut self) {
        self.pointer.commit_transferred();
        self.focus.commit_transferred();
        self.visibility.commit_transferred();
    }
}

#[cfg(test)]
mod tests {
    use dockspace::runtime::NativeMonotonicClock;
    use eframe::{NativeViewportPointerPassthroughStatus, egui};

    use super::super::{HostRecord, journal::HostIngressJournal};
    use super::*;

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
        let context = egui::Context::default();
        let first = token(&context);
        let next = token(&context);
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let mut callbacks = NativeCommandCallbacks::default();
        assert!(callbacks.reserve_pointer(first, WindowId::from(11), true));
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
        assert!(!callbacks.reserve_pointer(next, WindowId::from(11), true));
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
        assert!(!callbacks.reserve_pointer(next, WindowId::from(11), true));
        callbacks.commit_transferred();
        assert!(!callbacks.has_pending_work());
        assert_eq!(
            callbacks.accept_pointer(record(first), || panic!(
                "a committed duplicate cannot reserve ingress"
            )),
            CommandCallbackAdmission::Duplicate
        );
        assert!(callbacks.reserve_pointer(next, WindowId::from(11), true));
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
        let context = egui::Context::default();
        let token = token(&context);
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let mut callbacks = NativeCommandCallbacks::default();
        assert!(callbacks.reserve_pointer(token, WindowId::from(11), true));
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
        assert!(!callbacks.reserve_pointer(token, WindowId::from(12), true));
        assert!(!callbacks.reserve_pointer(token, WindowId::from(11), false));
        assert_eq!(
            callbacks.accept_pointer(record(token), || {
                journal.reserve(HostRecord::ViewportPointerPassthrough(record(token)))
            }),
            CommandCallbackAdmission::First
        );
        assert_eq!(journal.captured_through(), 1);
    }
}
