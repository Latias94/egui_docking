//! One immutable, dispatchable callback head from the existing ordered mailbox.

use super::*;

pub(crate) enum NativeCallbackHead {
    WindowEvent,
    DeviceRemoval(NativeDeviceRemovalRecord),
    GlobalFocus(NativeGlobalFocusRecord),
    ViewportFocus(NativeViewportFocusRecord),
    ViewportPointerPassthrough(NativePointerPassthroughRecord),
    PointerObservation(NativePointerObservationRecord, std::time::Duration),
    PointerCapture(NativePointerCaptureRecord, std::time::Duration),
    ViewportCloseCancelled(NativeViewportCloseCancellationRecord),
    ViewportRoster(NativeViewportRosterEnvelope),
    ViewportCreateFailed(NativeViewportCreateFailureRecord),
    ViewportVisibility(NativeViewportVisibilityRecord),
    ViewportCreated(NativeViewportCreatedRecord),
    Output,
}

impl NativeHostBridge {
    pub(crate) fn next_callback(&self) -> Option<NativeCallbackHead> {
        let records = self.lock();
        if !records.front_dispatchable() {
            return None;
        }
        let ingress = records.front_ingress()?;
        Some(match ingress.record() {
            // The pointer lane may claim/coalesce a complete raw prefix. Avoid
            // cloning its payload until that lane chooses the exact records.
            HostRecord::WindowEvent(_) => NativeCallbackHead::WindowEvent,
            HostRecord::DeviceRemoval(record) => NativeCallbackHead::DeviceRemoval(*record),
            HostRecord::GlobalFocus(record) => NativeCallbackHead::GlobalFocus(*record),
            HostRecord::ViewportFocus(record) => NativeCallbackHead::ViewportFocus(*record),
            HostRecord::ViewportPointerPassthrough(record) => {
                NativeCallbackHead::ViewportPointerPassthrough(*record)
            }
            HostRecord::PointerObservation(key) => NativeCallbackHead::PointerObservation(
                records.window_resources.observation_record(*key)?,
                ingress.captured_at(),
            ),
            HostRecord::PointerCapture(key) => NativeCallbackHead::PointerCapture(
                records.window_resources.capture_record(*key)?,
                ingress.captured_at(),
            ),
            HostRecord::ViewportCloseCancelled(record) => {
                NativeCallbackHead::ViewportCloseCancelled(*record)
            }
            HostRecord::ViewportRoster(record) => {
                NativeCallbackHead::ViewportRoster(record.clone())
            }
            HostRecord::ViewportCreateFailed(record) => {
                NativeCallbackHead::ViewportCreateFailed(*record)
            }
            HostRecord::ViewportVisibility(record) => {
                NativeCallbackHead::ViewportVisibility(*record)
            }
            HostRecord::ViewportCreated(record) => NativeCallbackHead::ViewportCreated(*record),
            HostRecord::Output { .. } => NativeCallbackHead::Output,
        })
    }
}

#[cfg(test)]
impl NativeHostBridge {
    pub(crate) fn front_global_focus(&self) -> Option<NativeGlobalFocusRecord> {
        let NativeCallbackHead::GlobalFocus(record) = self.next_callback()? else {
            return None;
        };
        Some(record)
    }
    pub(crate) fn front_viewport_focus(&self) -> Option<NativeViewportFocusRecord> {
        let NativeCallbackHead::ViewportFocus(record) = self.next_callback()? else {
            return None;
        };
        Some(record)
    }
    pub(crate) fn front_viewport_pointer_passthrough(
        &self,
    ) -> Option<NativePointerPassthroughRecord> {
        let NativeCallbackHead::ViewportPointerPassthrough(record) = self.next_callback()? else {
            return None;
        };
        Some(record)
    }

    pub(crate) fn front_viewport_roster(&self) -> Option<NativeViewportRosterEnvelope> {
        let NativeCallbackHead::ViewportRoster(record) = self.next_callback()? else {
            return None;
        };
        Some(record)
    }

    pub(crate) fn front_viewport_create_failure(
        &self,
    ) -> Option<NativeViewportCreateFailureRecord> {
        let NativeCallbackHead::ViewportCreateFailed(record) = self.next_callback()? else {
            return None;
        };
        Some(record)
    }
    pub(crate) fn front_viewport_visibility(&self) -> Option<NativeViewportVisibilityRecord> {
        let NativeCallbackHead::ViewportVisibility(record) = self.next_callback()? else {
            return None;
        };
        Some(record)
    }
    pub(crate) fn front_pointer_observation(
        &self,
    ) -> Option<(NativePointerObservationRecord, std::time::Duration)> {
        let NativeCallbackHead::PointerObservation(record, captured_at) = self.next_callback()?
        else {
            return None;
        };
        Some((record, captured_at))
    }
    #[cfg(target_os = "linux")]
    pub(crate) fn front_pointer_capture(
        &self,
    ) -> Option<(NativePointerCaptureRecord, std::time::Duration)> {
        let NativeCallbackHead::PointerCapture(record, captured_at) = self.next_callback()? else {
            return None;
        };
        Some((record, captured_at))
    }
}
