//! Opaque actions prepared from one exact paintable surface candidate.

use std::fmt;

use thiserror::Error;

use crate::engine::EngineInput;
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::intent::CloseSceneTarget;
use crate::model::WorkspaceVersion;
use crate::scene::{SurfaceSceneStamp, TabSceneId};

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
}

impl SurfaceAction {
    const fn kind(&self) -> &'static str {
        match self {
            Self::SelectTab { .. } => "select-tab",
            Self::Close { .. } => "close",
        }
    }
}

#[derive(Debug, Error)]
#[error("prepared surface action belongs to another dockspace authority domain")]
pub(super) struct PreparedSurfaceActionAuthorityMismatch;
