//! Opaque actions prepared from one exact paintable surface candidate.

use std::fmt;

use thiserror::Error;

use crate::engine::{
    EngineInput, LocalContainedGesturePhase, LocalSplitterGesturePhase, LocalTabChromeAction,
    LocalTabGesturePhase,
};
use crate::geometry::LogicalPoint;
use crate::ids::{EngineAuthorityDomainId, FloatingPresentationId, ItemId, SurfaceId};
use crate::intent::{CloseSceneTarget, ContainedGestureKind, TabGestureSource};
use crate::interaction::{
    ContainedTransformPaintAcknowledgement, EscapeDelivery, PaintAcknowledgement,
};
use crate::model::WorkspaceVersion;
use crate::scene::{
    ContainedResizeDirection, SplitterJunctionId, SplitterResizeTarget, SplitterSceneId,
    SurfaceSceneStamp, TabSceneId,
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

/// One current-frame framework gesture expressed in surface-logical coordinates.
///
/// The adapter reports only the phase and exact points observed by its widget
/// system. Structural targets, scene identity, and reducer input remain sealed
/// inside [`PreparedSurfaceAction`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceGesturePhase {
    /// Begins one gesture after the framework crossed its drag threshold.
    Begin {
        /// Logical point at which the gesture began.
        initial: LogicalPoint,
        /// Current logical point reported by the same framework response.
        current: LogicalPoint,
    },
    /// Updates an active gesture.
    Move {
        /// Current logical point.
        current: LogicalPoint,
    },
    /// Commits an active gesture at its exact release point.
    Release {
        /// Exact logical release point.
        current: LogicalPoint,
    },
    /// Cancels the matching active gesture without a durable mutation.
    Cancel,
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

    pub(super) const fn dock_back(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: crate::ids::FloatingPresentationId,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::DockBack { scene, floating },
        }
    }

    pub(super) const fn activate_contained(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        point: LogicalPoint,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            action: SurfaceAction::ActivateContained {
                scene,
                floating,
                point,
            },
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

    pub(super) const fn local_tab_gesture(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        source: TabGestureSource,
        phase: LocalTabGesturePhase,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            action: SurfaceAction::LocalTabGesture { source, phase },
        }
    }

    pub(super) const fn local_splitter_gesture(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        target: SplitterResizeTarget,
        phase: LocalSplitterGesturePhase,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            action: SurfaceAction::LocalSplitterGesture { target, phase },
        }
    }

    pub(super) const fn local_contained_gesture(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        floating: crate::ids::FloatingPresentationId,
        kind: ContainedGestureKind,
        phase: LocalContainedGesturePhase,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            action: SurfaceAction::LocalContainedGesture {
                floating,
                kind,
                phase,
            },
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

    pub(super) const fn acknowledge_preview(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        acknowledgement: PaintAcknowledgement,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            action: SurfaceAction::AcknowledgePreview { acknowledgement },
        }
    }

    pub(super) const fn acknowledge_contained_transform_preview(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        acknowledgement: ContainedTransformPaintAcknowledgement,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            action: SurfaceAction::AcknowledgeContainedTransformPreview { acknowledgement },
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
            | SurfaceAction::DockBack { .. }
            | SurfaceAction::ActivateContained { .. }
            | SurfaceAction::PresentationCommand { .. }
            | SurfaceAction::TabChrome { .. }
            | SurfaceAction::AdjustSplitter { .. }
            | SurfaceAction::AdjustSplitterJunction { .. }
            | SurfaceAction::AdjustContainedResize { .. }
            | SurfaceAction::CancelWithEscape
            | SurfaceAction::LocalTabGesture { .. }
            | SurfaceAction::LocalSplitterGesture { .. }
            | SurfaceAction::LocalContainedGesture { .. }
            | SurfaceAction::AcknowledgePreview { .. }
            | SurfaceAction::AcknowledgeContainedTransformPreview { .. } => None,
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
            SurfaceAction::DockBack { scene, floating } => EngineInput::DockBackLocalContained {
                expected: self.expected,
                scene,
                floating,
            },
            SurfaceAction::ActivateContained {
                scene,
                floating,
                point,
            } => EngineInput::ActivateLocalContained {
                expected: self.expected,
                scene,
                floating,
                point,
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
            SurfaceAction::LocalTabGesture { source, phase } => EngineInput::LocalTabGesture {
                expected: self.expected,
                surface: self.surface,
                source,
                phase,
            },
            SurfaceAction::LocalSplitterGesture { target, phase } => {
                EngineInput::LocalSplitterGesture {
                    expected: self.expected,
                    surface: self.surface,
                    target,
                    phase,
                }
            }
            SurfaceAction::LocalContainedGesture {
                floating,
                kind,
                phase,
            } => EngineInput::LocalContainedGesture {
                expected: self.expected,
                surface: self.surface,
                floating,
                kind,
                phase,
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
            SurfaceAction::AcknowledgePreview { acknowledgement } => {
                EngineInput::AcknowledgePreview {
                    expected: self.expected,
                    acknowledgement,
                }
            }
            SurfaceAction::AcknowledgeContainedTransformPreview { acknowledgement } => {
                EngineInput::AcknowledgeContainedTransformPreview {
                    expected: self.expected,
                    acknowledgement,
                }
            }
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
    DockBack {
        scene: SurfaceSceneStamp,
        floating: crate::ids::FloatingPresentationId,
    },
    ActivateContained {
        scene: SurfaceSceneStamp,
        floating: FloatingPresentationId,
        point: LogicalPoint,
    },
    PresentationCommand {
        scene: SurfaceSceneStamp,
        action: crate::model::ProductAction,
    },
    TabChrome {
        scene: SurfaceSceneStamp,
        action: LocalTabChromeAction,
    },
    LocalTabGesture {
        source: TabGestureSource,
        phase: LocalTabGesturePhase,
    },
    LocalSplitterGesture {
        target: SplitterResizeTarget,
        phase: LocalSplitterGesturePhase,
    },
    LocalContainedGesture {
        floating: crate::ids::FloatingPresentationId,
        kind: ContainedGestureKind,
        phase: LocalContainedGesturePhase,
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
    AcknowledgePreview {
        acknowledgement: PaintAcknowledgement,
    },
    AcknowledgeContainedTransformPreview {
        acknowledgement: ContainedTransformPaintAcknowledgement,
    },
}

impl SurfaceAction {
    const fn kind(&self) -> &'static str {
        match self {
            Self::SelectTab { .. } => "select-tab",
            Self::Close { .. } => "close",
            Self::DockBack { .. } => "dock-back",
            Self::ActivateContained { .. } => "activate-contained",
            Self::PresentationCommand { .. } => "presentation-command",
            Self::TabChrome { .. } => "tab-chrome",
            Self::LocalTabGesture { .. } => "tab-gesture",
            Self::LocalSplitterGesture { .. } => "splitter-gesture",
            Self::LocalContainedGesture { .. } => "contained-gesture",
            Self::AdjustSplitter { .. } => "adjust-splitter",
            Self::AdjustSplitterJunction { .. } => "adjust-splitter-junction",
            Self::AdjustContainedResize { .. } => "adjust-contained-resize",
            Self::CancelWithEscape => "cancel-with-escape",
            Self::AcknowledgePreview { .. } => "acknowledge-preview",
            Self::AcknowledgeContainedTransformPreview { .. } => {
                "acknowledge-contained-transform-preview"
            }
        }
    }
}

#[derive(Debug, Error)]
#[error("prepared surface action belongs to another dockspace authority domain")]
pub(super) struct PreparedSurfaceActionAuthorityMismatch;
