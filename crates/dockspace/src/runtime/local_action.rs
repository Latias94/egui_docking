//! Opaque actions prepared from one exact paintable surface candidate.

use std::fmt;

use thiserror::Error;

use crate::engine::EngineInput;
use crate::engine::{LocalContainedGesturePhase, LocalSplitterGesturePhase, LocalTabGesturePhase};
use crate::geometry::LogicalPoint;
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::intent::{CloseSceneTarget, ContainedGestureKind, TabGestureSource};
use crate::interaction::{ContainedTransformPaintAcknowledgement, PaintAcknowledgement};
use crate::model::WorkspaceVersion;
use crate::scene::{SplitterResizeTarget, SurfaceSceneStamp, TabSceneId};

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

#[derive(Debug)]
enum SurfaceAction {
    SelectTab {
        scene: SurfaceSceneStamp,
        tab: TabSceneId,
    },
    Close {
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
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
            Self::LocalTabGesture { .. } => "tab-gesture",
            Self::LocalSplitterGesture { .. } => "splitter-gesture",
            Self::LocalContainedGesture { .. } => "contained-gesture",
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
