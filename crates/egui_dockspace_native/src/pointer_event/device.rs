//! Device enrollment, virtual identities, and native reset boundaries.

use std::collections::btree_map::Entry;

use dockspace::runtime::{
    NativePointerButton, NativePointerId, NativeScrollDeviceId, NativeWindowBinding,
};
use winit::event::{DeviceId, ElementState, PointerPressToken, TouchPhase, WindowEvent};

use crate::event::NativePointerRoutes;

use super::NativePointerTranslator;
use super::mouse::NativeMouseState;
use super::scroll::NativeScrollState;

#[derive(Debug, Clone)]
pub(super) enum NativePointerDeviceEntry {
    Live(NativePointerDeviceState),
    /// Late callbacks cannot reuse this identity without an explicit start.
    Removed,
    /// A cancelled phaseful scroll ignores tails until an explicit start.
    ScrollTail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NativeInputDeviceKey {
    Mouse(NativePhysicalDeviceKey),
    Scroll(NativePhysicalDeviceKey),
    Touch {
        device: NativePhysicalDeviceKey,
        contact: u64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum NativePhysicalDeviceKey {
    Winit(DeviceId),
    #[cfg(test)]
    Test(u64),
}

#[derive(Debug, Clone)]
pub(super) struct NativePointerDeviceState {
    pub(super) pointer: NativePointerId,
    pub(super) scroll_device: NativeScrollDeviceId,
    pub(super) mouse: NativeMouseState,
    pub(super) scroll: NativeScrollState,
}

impl NativePointerTranslator {
    /// Counts inline storage, device/tombstone entries and button entries.
    /// B-tree node and allocator overhead are excluded.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        let bytes = size_of::<Self>().saturating_add(self.devices.len().saturating_mul(
            size_of::<NativeInputDeviceKey>() + size_of::<NativePointerDeviceEntry>(),
        ));
        self.devices.values().fold(bytes, |bytes, entry| {
            let NativePointerDeviceEntry::Live(device) = entry else {
                return bytes;
            };
            bytes.saturating_add(device.mouse.owned_button_capacity_bytes())
        })
    }

    /// Promises the current dynamic data and its overlapping retry candidate.
    /// The permanent inline translator is counted by its coordinator owner.
    pub(crate) fn reserved_translation_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.owned_data_capacity_bytes()
                .saturating_sub(size_of::<Self>())
                .saturating_mul(2),
        )
    }

    /// One callback can add at most one device and one button, in both copies.
    /// Existing terminal transitions only replace entries in place.
    pub(crate) const fn callback_translation_capacity_bytes() -> usize {
        2 * (size_of::<NativeInputDeviceKey>()
            + size_of::<NativePointerDeviceEntry>()
            + NativeMouseState::button_slot_capacity_bytes())
    }

    pub(crate) fn can_coalesce_idle_cursor_moves(&self) -> bool {
        self.devices.values().all(|entry| match entry {
            NativePointerDeviceEntry::Live(device) => device.is_idle(),
            NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => true,
        })
    }

    /// Finds an existing winit mouse device and its original Primary identity, if supplied.
    ///
    /// Native stationary continuations require an ongoing Primary press. This lookup
    /// never enrolls a device or revives a retired identity.
    pub(crate) fn mouse_press_for_pointer(
        &self,
        pointer: NativePointerId,
    ) -> Option<(DeviceId, Option<&PointerPressToken>)> {
        self.devices.iter().find_map(|(key, entry)| {
            let (
                NativeInputDeviceKey::Mouse(NativePhysicalDeviceKey::Winit(device)),
                NativePointerDeviceEntry::Live(state),
            ) = (key, entry)
            else {
                return None;
            };
            if state.pointer != pointer {
                return None;
            }
            let primary_press = state.mouse.active_press(NativePointerButton::Primary)?;
            Some((*device, primary_press))
        })
    }

    #[cfg(test)]
    pub(crate) fn mouse_device_for_pointer(&self, pointer: NativePointerId) -> Option<DeviceId> {
        self.mouse_press_for_pointer(pointer)
            .map(|(device, _)| device)
    }

    /// Forgets adapter state after core overload cancellation commits.
    ///
    /// Core already terminated the semantic streams. Only phaseful scroll
    /// lanes retain their existing slot until the next explicit start.
    pub(crate) fn finish_overload_cancellation(&mut self) {
        self.observed_modifiers = None;
        self.devices.retain(|_, entry| match entry {
            NativePointerDeviceEntry::Live(device) => {
                if matches!(device.scroll, NativeScrollState::Idle) {
                    false
                } else {
                    *entry = NativePointerDeviceEntry::ScrollTail;
                    true
                }
            }
            NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => true,
        });
    }

    /// Installs local tail suppression after core records binding retirement.
    ///
    /// Touch tombstones and cancelled mouse buttons reuse their original slots;
    /// destruction never needs new adapter storage after the fact is accepted.
    pub(crate) fn retire_binding(&mut self, binding: NativeWindowBinding) {
        if self
            .observed_modifiers
            .is_some_and(|(observed, _)| observed == binding)
        {
            self.observed_modifiers = None;
        }
        for (key, entry) in &mut self.devices {
            if let NativePointerDeviceEntry::Live(device) = entry
                && device.retire_binding(binding)
                && matches!(key, NativeInputDeviceKey::Touch { .. })
            {
                *entry = NativePointerDeviceEntry::Removed;
            }
        }
    }

    /// Retains each removed identity's existing slot as a tail gate.
    ///
    /// Winit raw-device identifiers cannot select a subset of the virtual
    /// identities carried by window input. An explicit start allocates a fresh
    /// identity in the same slot; late tails cannot revive the previous one.
    pub(crate) fn forget_removed_devices(&mut self) {
        for entry in self.devices.values_mut() {
            *entry = NativePointerDeviceEntry::Removed;
        }
    }

    pub(crate) fn references_binding(&self, binding: NativeWindowBinding) -> bool {
        self.devices.values().any(|entry| {
            matches!(entry, NativePointerDeviceEntry::Live(device) if device.references_binding(binding))
        })
    }

    pub(super) fn device_mut(
        &mut self,
        key: NativeInputDeviceKey,
    ) -> Option<&mut NativePointerDeviceState> {
        let Self {
            devices,
            next_device_identity,
            ..
        } = self;
        let entry = match devices.entry(key) {
            Entry::Occupied(entry) => {
                let entry = entry.into_mut();
                if !matches!(entry, NativePointerDeviceEntry::Live(_)) {
                    let next = allocate_device_identity(next_device_identity)?;
                    *entry = NativePointerDeviceEntry::Live(NativePointerDeviceState::new(next));
                }
                entry
            }
            Entry::Vacant(entry) => {
                let next = allocate_device_identity(next_device_identity)?;
                entry.insert(NativePointerDeviceEntry::Live(
                    NativePointerDeviceState::new(next),
                ))
            }
        };
        match entry {
            NativePointerDeviceEntry::Live(device) => Some(device),
            NativePointerDeviceEntry::Removed | NativePointerDeviceEntry::ScrollTail => {
                unreachable!("device enrollment installs a live identity")
            }
        }
    }
}

fn allocate_device_identity(next_device_identity: &mut u64) -> Option<u64> {
    let next = next_device_identity.checked_add(1)?;
    *next_device_identity = next;
    Some(next)
}

impl NativePointerDeviceEntry {
    pub(super) fn active_touch_origin_mismatch(&self, routes: NativePointerRoutes) -> bool {
        matches!(self, Self::Live(device) if device.mouse.active_origin_mismatch(routes))
    }
}

impl NativePointerDeviceState {
    fn new(identity: u64) -> Self {
        Self {
            pointer: NativePointerId::new(identity),
            scroll_device: NativeScrollDeviceId::new(identity),
            mouse: NativeMouseState::default(),
            scroll: NativeScrollState::Idle,
        }
    }

    pub(super) fn is_idle(&self) -> bool {
        self.mouse.is_idle()
            && matches!(
                self.scroll,
                NativeScrollState::Idle | NativeScrollState::Retired
            )
    }

    pub(super) fn retire_binding(&mut self, binding: NativeWindowBinding) -> bool {
        let mouse_retired = self.mouse.retire_binding(binding);
        let scroll_retired = matches!(
            self.scroll,
            NativeScrollState::Live {
                delivery: Some(delivery),
                ..
            } if delivery == binding
        );
        if scroll_retired {
            self.scroll = NativeScrollState::Retired;
        }
        mouse_retired || scroll_retired
    }

    pub(super) fn references_binding(&self, binding: NativeWindowBinding) -> bool {
        self.mouse.references_binding(binding)
            || matches!(
                self.scroll,
                NativeScrollState::Live {
                    delivery: Some(delivery),
                    ..
                } if delivery == binding
            )
    }
}

pub(super) fn pointer_device(event: &WindowEvent) -> Option<NativeInputDeviceKey> {
    let device = match event {
        WindowEvent::CursorMoved { device_id, .. }
        | WindowEvent::MouseInput { device_id, .. }
        | WindowEvent::MouseWheel { device_id, .. }
        | WindowEvent::PointerCaptureChanged { device_id, .. }
        | WindowEvent::PanGesture { device_id, .. } => *device_id,
        WindowEvent::Touch(touch) => touch.device_id,
        _ => return None,
    };
    pointer_device_for_physical(event, NativePhysicalDeviceKey::Winit(device))
}

pub(super) fn pointer_device_for_physical(
    event: &WindowEvent,
    device: NativePhysicalDeviceKey,
) -> Option<NativeInputDeviceKey> {
    match event {
        WindowEvent::CursorMoved { .. }
        | WindowEvent::MouseInput { .. }
        | WindowEvent::PointerCaptureChanged { .. } => Some(NativeInputDeviceKey::Mouse(device)),
        WindowEvent::MouseWheel { .. } | WindowEvent::PanGesture { .. } => {
            Some(NativeInputDeviceKey::Scroll(device))
        }
        WindowEvent::Touch(touch) => Some(NativeInputDeviceKey::Touch {
            device,
            contact: touch.id,
        }),
        _ => None,
    }
}

pub(super) fn starts_reused_device_stream(event: &WindowEvent) -> bool {
    matches!(
        event,
        WindowEvent::MouseInput {
            state: ElementState::Pressed,
            ..
        } | WindowEvent::Touch(winit::event::Touch {
            phase: TouchPhase::Started,
            ..
        }) | WindowEvent::MouseWheel {
            phase: TouchPhase::Started,
            ..
        } | WindowEvent::PanGesture {
            phase: TouchPhase::Started,
            ..
        }
    )
}
