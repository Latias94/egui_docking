//! Runtime motion policy uses the existing retained host and frame clock.

use dockspace::motion::{MotionFrame, MotionPreference, MotionSettings};
use open_gpui::Window;

use super::{GpuiDockspace, HostState};

impl GpuiDockspace {
    /// Returns the current geometry policy; this adapter animates layout only.
    #[must_use]
    pub fn motion_settings(&self) -> MotionSettings {
        self.host.borrow().config.motion
    }

    /// Replaces transitions and requests an update in the owning window.
    ///
    /// Active transitions retarget from their current pose. Off, reduced motion
    /// and zero timing settle immediately on the next accepted update.
    pub fn set_motion_settings(&self, settings: MotionSettings, window: &mut Window) {
        let mut host = self.host.borrow_mut();
        if host.config.motion != settings {
            host.config.motion = settings;
            host.dirty = true;
            window.refresh();
        }
    }

    /// Returns the preference supplied by the application.
    #[must_use]
    pub fn motion_preference(&self) -> MotionPreference {
        self.host.borrow().config.motion_preference
    }

    /// Supplies an observed system preference and refreshes the owning window.
    ///
    /// The adapter starts with Unknown instead of inferring platform support.
    /// An explicit Reduced preference takes priority over effect settings.
    pub fn set_motion_preference(&self, preference: MotionPreference, window: &mut Window) {
        let mut host = self.host.borrow_mut();
        if host.config.motion_preference != preference {
            host.config.motion_preference = preference;
            host.dirty = true;
            window.refresh();
        }
    }
}

impl HostState {
    pub(crate) fn motion_frame(&self) -> MotionFrame {
        MotionFrame {
            now: self.observed_at(),
            settings: self.config.motion,
            preference: self.config.motion_preference,
        }
    }
}
