//! Application-owned chrome and input preferences.

use open_gpui::{Font, Hsla, Keystroke, Modifiers, Pixels, font, px, rgb, rgba};

/// Chrome appearance and gesture bindings for one retained dockspace.
///
/// Geometry, drag thresholds, and docking policies remain configured on the
/// renderer-neutral session.
#[derive(Clone, Debug)]
pub struct GpuiDockspaceConfig {
    /// Font used for both intrinsic measurement and chrome painting.
    pub font: Font,
    /// Text size used for both intrinsic measurement and chrome painting.
    pub text_size: Pixels,
    /// Colors of adapter-owned chrome; pane content keeps its own styling.
    pub colors: DockspaceColors,
    /// Keyboard bindings observed during pointer gestures.
    pub input: DockspaceInputBindings,
}

impl Default for GpuiDockspaceConfig {
    fn default() -> Self {
        Self {
            font: font("Arial"),
            text_size: px(13.0),
            colors: DockspaceColors::default(),
            input: DockspaceInputBindings::default(),
        }
    }
}

/// Colors of the chrome painted by the adapter.
#[derive(Clone, Debug)]
pub struct DockspaceColors {
    pub background: Hsla,
    pub pane: Hsla,
    pub tab_bar: Hsla,
    pub tab: Hsla,
    pub selected_tab: Hsla,
    /// Inactive tab under the actual GPUI pointer target.
    pub hovered_tab: Hsla,
    pub text: Hsla,
    pub muted_text: Hsla,
    pub border: Hsla,
    pub splitter: Hsla,
    pub contained_title: Hsla,
    pub accent: Hsla,
    pub guide: Hsla,
    pub active_guide: Hsla,
    pub disabled_guide: Hsla,
    pub preview: Hsla,
    pub drag_ghost: Hsla,
    pub shadow: Hsla,
}

impl Default for DockspaceColors {
    fn default() -> Self {
        Self {
            background: rgb(0xf0f1f3).into(),
            pane: rgb(0xfafafa).into(),
            tab_bar: rgb(0xe9ebee).into(),
            tab: rgb(0xe9ebee).into(),
            selected_tab: rgb(0xfafafa).into(),
            hovered_tab: rgb(0xf3f5f8).into(),
            text: rgb(0x29313d).into(),
            muted_text: rgb(0x66717f).into(),
            border: rgb(0xd4d9e0).into(),
            splitter: rgb(0xd4d9e0).into(),
            contained_title: rgb(0xf1f2f4).into(),
            accent: rgb(0x3b72c4).into(),
            guide: rgb(0xffffff).into(),
            active_guide: rgb(0xe2edfc).into(),
            disabled_guide: rgb(0xf0f1f3).into(),
            preview: rgba(0x3b72c423).into(),
            drag_ghost: rgb(0xffffff).into(),
            shadow: rgba(0x1b273023).into(),
        }
    }
}

/// Bindings applied to event-time modifiers, before input is queued.
#[derive(Clone, Debug)]
pub struct DockspaceInputBindings {
    /// Holding these modifiers suspends docking. Extra modifiers are allowed.
    /// `None` disables this binding.
    pub suspend_docking: Option<Modifiers>,
    /// Cancels a captured gesture when the logical key matches and all required
    /// modifiers are held. Extra modifiers are allowed, so cancellation remains
    /// available while docking is suspended. `None` disables this binding.
    pub cancel: Option<Keystroke>,
}

impl DockspaceInputBindings {
    pub(crate) fn docking_enabled(&self, modifiers: Modifiers) -> bool {
        !self
            .suspend_docking
            .is_some_and(|binding| binding.is_subset_of(&modifiers))
    }

    pub(crate) fn cancel_matches(&self, keystroke: &Keystroke) -> bool {
        self.cancel.as_ref().is_some_and(|binding| {
            binding.key == keystroke.key && binding.modifiers.is_subset_of(&keystroke.modifiers)
        })
    }
}

impl Default for DockspaceInputBindings {
    fn default() -> Self {
        Self {
            suspend_docking: Some(Modifiers {
                shift: true,
                ..Modifiers::default()
            }),
            cancel: Some(Keystroke {
                modifiers: Modifiers::default(),
                key: "escape".into(),
                key_char: None,
            }),
        }
    }
}
