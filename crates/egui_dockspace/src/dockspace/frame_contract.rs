//! Host frame scheduling and presentation-boundary types.

/// One caller-scheduled egui render pass for the crates.io single-surface adapter.
///
/// `sequence` names the host input epoch and `pass` distinguishes repeated render passes for
/// that same epoch. Cross-viewport hosts require the native runtime provider rather than this
/// facade because ordinary egui callbacks cannot prove a global input boundary. This is adapter
/// scheduling state, not a core presentation-emission key or presentation proof.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EguiFrameScheduleKey {
    sequence: u64,
    pass: u32,
}

impl EguiFrameScheduleKey {
    /// Creates one explicit egui render-pass schedule identity.
    #[must_use]
    pub const fn new(sequence: u64, pass: u32) -> Self {
        Self { sequence, pass }
    }

    /// Returns the host-owned input epoch.
    #[must_use]
    pub const fn sequence(self) -> u64 {
        self.sequence
    }

    /// Returns the repeated-render-pass index within the host epoch.
    #[must_use]
    pub const fn pass(self) -> u32 {
        self.pass
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EguiHostFrameMode {
    SingleSurface,
    CompleteRoster,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EguiInputAuthority {
    FrameworkResponses,
    CoreBackend,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum EguiOutputBoundary {
    UnobservableCallback,
    OuterFinalOutput,
    BackendPostInputFinalOutput,
    #[cfg(test)]
    DeterministicTestFinalOutput,
}

impl EguiOutputBoundary {
    pub(super) const fn paint_matches_final_presentation(
        self,
        paint_was_post_input: bool,
        presentation_changed: bool,
    ) -> bool {
        match self {
            Self::UnobservableCallback => false,
            Self::OuterFinalOutput => !paint_was_post_input && !presentation_changed,
            Self::BackendPostInputFinalOutput => paint_was_post_input,
            #[cfg(test)]
            Self::DeterministicTestFinalOutput => !paint_was_post_input && !presentation_changed,
        }
    }
}

impl EguiHostFrameMode {
    pub(super) const fn accepts_multiple_surfaces(self) -> bool {
        matches!(self, Self::CompleteRoster)
    }

    pub(super) const fn defers_publication_staging(self) -> bool {
        matches!(self, Self::CompleteRoster)
    }
}
