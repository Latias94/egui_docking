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
            text_size: px(14.0),
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
    pub text: Hsla,
    pub border: Hsla,
    pub splitter: Hsla,
    pub contained_title: Hsla,
    pub accent: Hsla,
    pub guide: Hsla,
    pub preview: Hsla,
    pub drag_ghost: Hsla,
}

impl Default for DockspaceColors {
    fn default() -> Self {
        Self {
            background: rgb(0xe6eaf0).into(),
            pane: rgb(0xf5f6f8).into(),
            tab_bar: rgb(0xd8dee8).into(),
            tab: rgb(0xcbd4e1).into(),
            selected_tab: rgb(0xf5f6f8).into(),
            text: rgb(0x253247).into(),
            border: rgb(0x8391a6).into(),
            splitter: rgb(0xa3afbf).into(),
            contained_title: rgb(0xb9c6d8).into(),
            accent: rgb(0x2b7b99).into(),
            guide: rgb(0xc4dde7).into(),
            preview: rgba(0x2b7b993c).into(),
            drag_ghost: rgb(0xd3e0ed).into(),
        }
    }
}

/// Bindings applied to event-time modifiers, before input is queued.
#[derive(Clone, Debug)]
pub struct DockspaceInputBindings {
    /// Holding these modifiers suspends docking. Extra modifiers are allowed.
    /// `None` disables this binding.
    pub suspend_docking: Option<Modifiers>,
    /// Cancels a captured gesture when key and modifiers match exactly.
    /// `None` disables this binding.
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
            binding.key == keystroke.key && binding.modifiers == keystroke.modifiers
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
