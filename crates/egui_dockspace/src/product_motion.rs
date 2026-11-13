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
    pub fn set_motion_settings(&mut self, settings: MotionSettings) {
        self.motion_settings = settings;
    }

    /// Supplies a system preference observed by the application.
    ///
    /// egui does not expose a portable OS preference, so the default is Unknown.
    /// A zero egui animation time also forces reduced motion. Applications can
    /// explicitly choose the fallback for Unknown in [`MotionSettings`].
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
mod tests {
    use super::*;
    use crate::{
        DockPlacement, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
        SurfacePresentationResult,
    };
    use std::{
        num::NonZeroUsize,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    struct Panes;
    impl PaneView for Panes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Editor {item}").into())
        }
        fn ui(&mut self, _: ItemId, ui: &mut egui::Ui) -> PaneResponse {
            PaneResponse::from(&ui.label("Editor content"))
        }
    }

    fn frame(context: &egui::Context, dock: &mut Dockspace, time: f64) -> crate::DockspaceOutput {
        crate::run_ui(
            context,
            egui::RawInput {
                time: Some(time),
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(800.0, 600.0),
                )),
                ..Default::default()
            },
            |ui| {
                dock.show_single_surface(SurfaceId::new(1), ui, &mut Panes)
                    .unwrap();
            },
        )
    }

    #[test]
    fn terminal_receipt_wakes_caller_owned_motion_after_output_backpressure() {
        let context = egui::Context::default();
        context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
        let nodes = [1, 2, 3].map(|id| DockspaceNode::tabs([ItemId::new(id)]));
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SurfaceId::new(1),
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::equal_split(DockspaceAxis::Horizontal, nodes).unwrap(),
            ),
        )])
        .unwrap();
        let mut dock = Dockspace::builder("motion-capacity", layout)
            .build()
            .unwrap();
        dock.session
            .configure_output_capacity(NonZeroUsize::new(1).unwrap())
            .unwrap();
        dock.set_motion_preference(MotionPreference::Full);
        for time in [0.0, 0.1, 0.2] {
            let (output, receipt) = frame(&context, &mut dock, time).into_parts();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
        dock.dock_item_current(ItemId::new(3), DockPlacement::After(ItemId::new(1)))
            .unwrap();
        let (output, active) = frame(&context, &mut dock, 1.0).into_parts();
        output.drop_without_applying_deltas();
        assert!(dock.session.surface_motion_is_active(SurfaceId::new(1)));
        for time in [1.01, 1.02, 1.03] {
            let (output, receipt) = frame(&context, &mut dock, time).into_parts();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Dropped);
        }
        let wakeups = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&wakeups);
        context.set_request_repaint_callback(move |_| {
            observed.fetch_add(1, Ordering::Relaxed);
        });
        active.report(SurfacePresentationResult::Presented);
        assert!(
            wakeups.load(Ordering::Relaxed) > 0,
            "terminal settlement must resume the paused trajectory"
        );
    }
}
