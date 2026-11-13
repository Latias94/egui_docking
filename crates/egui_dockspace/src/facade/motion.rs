//! Motion policy stays independent of measurement and adapter styling.

use std::time::Duration;

use dockspace::motion::{MotionFrame, MotionPreference, MotionSettings};

use super::Dockspace;

impl Dockspace {
    /// Returns runtime motion settings; changing them does not remeasure layout.
    #[must_use]
    pub const fn motion_settings(&self) -> &MotionSettings {
        &self.motion_settings
    }

    /// Replaces motion settings for the next outer update.
    ///
    /// Active springs retain their sampled pose and velocity when retargeted.
    /// Off, reduced motion and zero timing settle immediately.
    ///
    /// ```
    /// # fn disable_motion(dockspace: &mut egui_dockspace::Dockspace) {
    /// use egui_dockspace::motion::{MotionMode, MotionSettings};
    ///
    /// dockspace.set_motion_settings(MotionSettings {
    ///     mode: MotionMode::Off,
    ///     ..Default::default()
    /// });
    /// # }
    /// ```
    pub fn set_motion_settings(&mut self, settings: MotionSettings) {
        self.motion_settings = settings;
    }

    /// Supplies a system preference observed by the application.
    ///
    /// egui does not expose a portable OS preference, so the default is Unknown.
    /// Default settings permit motion when the preference is Unknown. Applications
    /// can choose its fallback in [`MotionSettings`]. An explicit Reduced
    /// preference or a zero egui animation time always disables geometry motion.
    pub fn set_motion_preference(&mut self, preference: MotionPreference) {
        self.motion_preference = preference;
    }

    /// Returns the application-supplied preference, before egui's zero-time override.
    #[must_use]
    pub const fn motion_preference(&self) -> MotionPreference {
        self.motion_preference
    }

    pub(super) fn motion_frame(&self, ui: &egui::Ui) -> MotionFrame {
        MotionFrame {
            now: Duration::try_from_secs_f64(ui.input(|input| input.time).max(0.0))
                .unwrap_or(Duration::MAX),
            settings: self.motion_settings,
            preference: if ui.style().animation_time <= 0.0 {
                MotionPreference::Reduced
            } else {
                self.motion_preference
            },
        }
    }
}

#[cfg(test)]
mod tests;
