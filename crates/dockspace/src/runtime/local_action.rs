//! Opaque actions prepared from one exact paintable surface candidate.

use std::fmt;

use thiserror::Error;

use crate::engine::{EngineInput, LocalTabChromeAction};
use crate::ids::{EngineAuthorityDomainId, FloatingPresentationId, ItemId, SurfaceId};
use crate::intent::CloseSceneTarget;
use crate::interaction::EscapeDelivery;
use crate::model::WorkspaceVersion;
use crate::scene::{
    ContainedResizeDirection, SplitterJunctionId, SplitterSceneId, SurfaceSceneStamp, TabSceneId,
};

/// Product-facing navigation within one exact tab strip.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceTabNavigation {
    /// Select the previous tab, wrapping to the last tab.
    Previous,
    /// Select the next tab, wrapping to the first tab.
    Next,
    /// Select the first tab.
    First,
    /// Select the last tab.
    Last,
}

impl SurfaceTabNavigation {
    pub(super) const fn into_core(self) -> crate::tab_strip::TabNavigation {
        match self {
            Self::Previous => crate::tab_strip::TabNavigation::Previous,
            Self::Next => crate::tab_strip::TabNavigation::Next,
            Self::First => crate::tab_strip::TabNavigation::First,
            Self::Last => crate::tab_strip::TabNavigation::Last,
        }
    }
}

/// Product-facing navigation within one exact open tab-list menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceTabListNavigation {
    /// Move focus to the preceding row, clamped at the first row.
    Previous,
    /// Move focus to the following row, clamped at the last row.
    Next,
    /// Move focus to the first row.
    First,
    /// Move focus to the last row.
    Last,
}

/// Product-facing signed adjustment of one exact splitter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceSplitterAdjustment {
    /// Move the splitter toward the beginning of its axis.
    Decrement,
    /// Move the splitter toward the end of its axis.
    Increment,
}

impl SurfaceSplitterAdjustment {
    pub(super) const fn direction(self) -> f64 {
        match self {
            Self::Decrement => -1.0,
            Self::Increment => 1.0,
        }
    }
}

/// Product-facing signed adjustment of one contained-floating edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SurfaceContainedResizeAdjustment {
    /// Move the edge toward the beginning of its logical axis.
    Decrement,
    /// Move the edge toward the end of its logical axis.
    Increment,
}

impl SurfaceContainedResizeAdjustment {
    pub(super) const fn direction(self) -> f64 {
        match self {
            Self::Decrement => -1.0,
            Self::Increment => 1.0,
        }
    }
}

/// One affine framework action captured from an exact [`super::SurfacePaintPlan`].
///
/// The capability deliberately hides scene stamps, runtime tab identities, and
/// reducer input. Submit it through [`super::DockspaceHostFrame::submit_surface_action`]
/// before measuring the next framework pass.
#[must_use = "a prepared surface action must be submitted or deliberately discarded"]
#[derive(PartialEq)]
pub struct PreparedSurfaceAction {
    authority_domain: EngineAuthorityDomainId,
    expected: WorkspaceVersion,
    surface: SurfaceId,
    action: SurfaceAction,
}

impl PreparedSurfaceAction {
    pub(super) const fn select_tab(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        tab: TabSceneId,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::SelectTab { scene, tab },
        }
    }

    pub(super) const fn close(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::Close { scene, target },
        }
    }

    pub(super) const fn presentation_command(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        action: crate::model::ProductAction,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::PresentationCommand { scene, action },
        }
    }

    pub(super) const fn close_root(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        root: crate::ids::RootId,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::CloseRoot { scene, root },
        }
    }

    pub(super) const fn adjust_splitter(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::AdjustSplitter {
                scene,
                splitter,
                delta,
            },
        }
    }

    pub(super) const fn adjust_splitter_junction(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        junction: SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::AdjustSplitterJunction {
                scene,
                junction,
                axis,
                delta,
            },
        }
    }

    pub(super) const fn adjust_contained_resize(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        direction: ContainedResizeDirection,
        delta: f64,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::AdjustContainedResize {
                scene,
                floating,
                direction,
                delta,
            },
        }
    }

    pub(super) const fn cancel_with_escape(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            action: SurfaceAction::CancelWithEscape,
        }
    }

    pub(super) const fn tab_chrome(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        action: LocalTabChromeAction,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::TabChrome { scene, action },
        }
    }

    /// Returns the published workspace version from which the action was prepared.
    #[must_use]
    pub const fn expected_version(&self) -> WorkspaceVersion {
        self.expected
    }

    /// Returns the sole logical surface whose candidate produced the action.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the tab which should receive framework focus after this action.
    #[must_use]
    pub const fn tab_focus_target(&self) -> Option<ItemId> {
        match self.action {
            SurfaceAction::SelectTab { tab, .. } => Some(tab.item),
            SurfaceAction::Close { .. }
            | SurfaceAction::CloseRoot { .. }
            | SurfaceAction::PresentationCommand { .. }
            | SurfaceAction::TabChrome { .. }
            | SurfaceAction::AdjustSplitter { .. }
            | SurfaceAction::AdjustSplitterJunction { .. }
            | SurfaceAction::AdjustContainedResize { .. }
            | SurfaceAction::CancelWithEscape => None,
        }
    }

    pub(super) fn into_engine_input(
        self,
        authority_domain: EngineAuthorityDomainId,
    ) -> Result<EngineInput, PreparedSurfaceActionAuthorityMismatch> {
        if self.authority_domain != authority_domain {
            return Err(PreparedSurfaceActionAuthorityMismatch);
        }
        Ok(match self.action {
            SurfaceAction::SelectTab { scene, tab } => EngineInput::SelectLocalSceneTab {
                expected: self.expected,
                scene,
                tab,
            },
            SurfaceAction::Close { scene, target } => EngineInput::RequestLocalSceneClose {
                expected: self.expected,
                scene,
                target,
            },
            SurfaceAction::CloseRoot { scene, root } => EngineInput::RequestLocalRootClose {
                expected: self.expected,
                scene,
                root,
            },
            SurfaceAction::PresentationCommand { scene, action } => {
                EngineInput::ApplyLocalPresentationCommand {
                    expected: self.expected,
                    scene,
                    action,
                }
            }
            SurfaceAction::TabChrome { scene, action } => EngineInput::ApplyLocalTabChromeAction {
                expected: self.expected,
                scene,
                action,
            },
            SurfaceAction::AdjustSplitter {
                scene,
                splitter,
                delta,
            } => EngineInput::AdjustLocalSplitterResize {
                expected: self.expected,
                scene,
                splitter,
                delta,
            },
            SurfaceAction::AdjustSplitterJunction {
                scene,
                junction,
                axis,
                delta,
            } => EngineInput::AdjustLocalSplitterJunctionResize {
                expected: self.expected,
                scene,
                junction,
                axis,
                delta,
            },
            SurfaceAction::AdjustContainedResize {
                scene,
                floating,
                direction,
                delta,
            } => EngineInput::AdjustLocalContainedResize {
                expected: self.expected,
                scene,
                floating,
                direction,
                delta,
            },
            SurfaceAction::CancelWithEscape => EngineInput::CancelActiveInteractionWithEscape {
                expected: self.expected,
                delivery: EscapeDelivery::Surface(self.surface),
            },
        })
    }
}

impl fmt::Debug for PreparedSurfaceAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PreparedSurfaceAction")
            .field("expected", &self.expected)
            .field("surface", &self.surface)
            .field("kind", &self.action.kind())
            .finish_non_exhaustive()
    }
}

#[derive(Debug, PartialEq)]
enum SurfaceAction {
    SelectTab {
        scene: SurfaceSceneStamp,
        tab: TabSceneId,
    },
    Close {
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
    },
    CloseRoot {
        scene: SurfaceSceneStamp,
        root: crate::ids::RootId,
    },
    PresentationCommand {
        scene: SurfaceSceneStamp,
        action: crate::model::ProductAction,
    },
    TabChrome {
        scene: SurfaceSceneStamp,
        action: LocalTabChromeAction,
    },
    AdjustSplitter {
        scene: SurfaceSceneStamp,
        splitter: SplitterSceneId,
        delta: f64,
    },
    AdjustSplitterJunction {
        scene: SurfaceSceneStamp,
        junction: SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
    },
    AdjustContainedResize {
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        direction: ContainedResizeDirection,
        delta: f64,
    },
    CancelWithEscape,
}

impl SurfaceAction {
    const fn kind(&self) -> &'static str {
        match self {
            Self::SelectTab { .. } => "select-tab",
            Self::Close { .. } => "close",
            Self::CloseRoot { .. } => "close-root",
            Self::PresentationCommand { .. } => "presentation-command",
            Self::TabChrome { .. } => "tab-chrome",
            Self::AdjustSplitter { .. } => "adjust-splitter",
            Self::AdjustSplitterJunction { .. } => "adjust-splitter-junction",
            Self::AdjustContainedResize { .. } => "adjust-contained-resize",
            Self::CancelWithEscape => "cancel-with-escape",
        }
    }
}

#[derive(Debug, Error)]
#[error("prepared surface action belongs to another dockspace authority domain")]
pub(super) struct PreparedSurfaceActionAuthorityMismatch;
