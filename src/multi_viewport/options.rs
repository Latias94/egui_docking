use egui::Vec2;

/// Options for [`super::DockingMultiViewport`].
#[derive(Clone, Debug)]
pub struct DockingMultiViewportOptions {
    /// ImGui parity: controls whether docking during "window move" requires holding SHIFT.
    ///
    /// - `false` (ImGui default): holding SHIFT disables docking (useful to move without docking).
    /// - `true`: holding SHIFT enables docking (reduces visual noise, allows moving freely by default).
    ///
    /// This only affects "window move" drags (i.e. payloads with `tile_id == None`), not subtree/tab drags.
    pub config_docking_with_shift: bool,

    /// Fallback inner size (in points) when we can't infer a better size for a torn-off pane.
    pub default_detached_inner_size: Vec2,

    /// If true, holding SHIFT while tearing off a pane will instead tear off the closest parent `Tabs` container,
    /// preserving the whole tab-group (dear imgui style "dock node tear-off").
    pub detach_parent_tabs_on_shift: bool,

    /// If true, holding ALT while releasing a drag will force a tear-off into a new native viewport,
    /// even if the cursor is still inside the dock area.
    pub detach_on_alt_release_anywhere: bool,

    /// ImGui parity: when moving a whole window host (native viewport or contained floating),
    /// only allow "dock as tab" when hovering an explicit target rect (the target tab bar / title bar).
    ///
    /// If `false`, allow "dock as tab" anywhere over a dock node (more forgiving, but diverges from ImGui).
    pub window_move_tab_dock_requires_explicit_target: bool,

    /// If true, dragging the detached viewport's custom top bar will also request `ViewportCommand::Focus`,
    /// so the moving window is brought to front (reduces confusion when the window moves behind others).
    pub focus_detached_on_custom_title_drag: bool,

    /// If true, show ImGui-style docking overlay targets even for drags that stay within the same viewport.
    pub show_overlay_for_internal_drags: bool,

    /// If true, show ImGui-style *outer* docking targets (dockspace edge markers),
    /// allowing quick splits at the dockspace boundary (dear imgui style outer docking).
    pub show_outer_overlay_targets: bool,

    /// If true, holding CTRL while tearing off will create a contained floating window (within the current viewport)
    /// instead of a native viewport window.
    pub tear_off_to_floating_on_ctrl: bool,

    /// If true, dragging a tab/pane outside the dock area will immediately create a "ghost" floating window
    /// that follows the pointer, and can be docked back before releasing (dear imgui style).
    pub ghost_tear_off: bool,

    /// Pointer distance (in points) outside the dock area required to trigger ghost tear-off.
    pub ghost_tear_off_threshold: f32,

    /// If true, ghost tear-off will spawn a native viewport window immediately once the pointer
    /// leaves the dock area beyond `ghost_tear_off_threshold` (ImGui-like "it becomes a new OS window while dragging").
    ///
    /// If false, ghost tear-off starts as a contained floating window and may later be upgraded
    /// to a native viewport depending on other options.
    pub ghost_spawn_native_on_leave_dock: bool,

    /// If true, a contained ghost window will be upgraded to a native viewport once the pointer leaves
    /// the source viewport's inner rectangle.
    pub ghost_upgrade_to_native_on_leave_viewport: bool,

    /// If true, show on-screen debug info about drop targeting (inner/outer overlay, hit targets, insertion points).
    pub debug_drop_targets: bool,

    /// If true, record debug events (drop decisions + integrity checks) in a small ring buffer
    /// and show it in the debug panel for easy copy-paste.
    pub debug_event_log: bool,

    /// Maximum number of debug log lines to keep (ring buffer).
    pub debug_event_log_capacity: usize,

    /// If true, run tree integrity checks each frame (debug-only).
    pub debug_integrity: bool,

    /// If true, panic on integrity issues (debug-only).
    pub debug_integrity_panic: bool,
}

impl Default for DockingMultiViewportOptions {
    fn default() -> Self {
        Self {
            config_docking_with_shift: false,
            default_detached_inner_size: Vec2::new(480.0, 360.0),
            detach_parent_tabs_on_shift: true,
            detach_on_alt_release_anywhere: true,
            window_move_tab_dock_requires_explicit_target: true,
            focus_detached_on_custom_title_drag: true,
            show_overlay_for_internal_drags: true,
            show_outer_overlay_targets: true,
            tear_off_to_floating_on_ctrl: true,
            ghost_tear_off: true,
            ghost_tear_off_threshold: 8.0,
            ghost_spawn_native_on_leave_dock: true,
            ghost_upgrade_to_native_on_leave_viewport: true,
            debug_drop_targets: false,
            debug_event_log: false,
            debug_event_log_capacity: 200,
            debug_integrity: false,
            debug_integrity_panic: false,
        }
    }
}

impl DockingMultiViewportOptions {
    pub(crate) fn window_move_docking_enabled_by_shift(&self, shift_held: bool) -> bool {
        self.config_docking_with_shift == shift_held
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_docking_with_shift_matches_imgui_default() {
        let opt = DockingMultiViewportOptions {
            config_docking_with_shift: false,
            ..Default::default()
        };
        assert!(opt.window_move_docking_enabled_by_shift(false));
        assert!(!opt.window_move_docking_enabled_by_shift(true));
    }

    #[test]
    fn config_docking_with_shift_inverts_behavior_when_enabled() {
        let opt = DockingMultiViewportOptions {
            config_docking_with_shift: true,
            ..Default::default()
        };
        assert!(!opt.window_move_docking_enabled_by_shift(false));
        assert!(opt.window_move_docking_enabled_by_shift(true));
    }
}
