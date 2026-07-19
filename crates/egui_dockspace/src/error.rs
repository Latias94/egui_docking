//! Public failures produced by the egui facade.

use dockspace::engine::EngineError;
use dockspace::ids::{FloatingPresentationId, RootId, SurfaceId};
use dockspace::scene::SceneBuildError;
use egui::Rect;
use thiserror::Error;

use crate::{DockStyleError, ProjectionError};

/// Failure to construct or advance an egui docking frame.
#[derive(Debug, Error)]
pub enum DockspaceError {
    /// Style geometry is invalid and cannot produce authoritative scene facts.
    #[error("dock style is invalid: {0}")]
    Style(#[from] DockStyleError),
    /// The renderer-neutral engine could not publish an atomic boundary.
    #[error("dock engine failed: {0}")]
    Engine(#[from] EngineError),
    /// The egui projection could not represent the current workspace.
    #[error("egui projection failed: {0}")]
    Projection(#[from] ProjectionError),
    /// Complete surface facts could not form a building scene.
    #[error("dock scene construction failed: {0}")]
    Scene(#[from] SceneBuildError),
    /// The reducer consumed but rejected the scene which the adapter intended to paint.
    #[error("dock scene publication was rejected: {0}")]
    SceneRejected(SceneBuildError),
    /// One facade instance was asked to paint two logical surfaces in one egui frame.
    #[error(
        "dockspace already paints surface {expected} in this egui frame; surface {actual} is not part of that frame"
    )]
    SurfaceChangedWithinFrame {
        expected: SurfaceId,
        actual: SurfaceId,
    },
    /// A repeated egui pass changed the host rectangle after scene publication.
    #[error("dockspace host bounds changed within one egui frame: {expected:?} -> {actual:?}")]
    BoundsChangedWithinFrame { expected: Rect, actual: Rect },
    /// `show` was invoked more than once in the same egui pass.
    #[error("dockspace was shown more than once in egui frame {frame}, pass {pass}")]
    DuplicateShowInPass { frame: u64, pass: usize },
    /// A scene publication reported success but no matching ready surface was published.
    #[error("dock scene did not publish ready facts for surface {surface}")]
    ReadySceneUnavailable { surface: SurfaceId },
    /// The adapter lost its own frame accumulator before completing a `show` call.
    #[error("dockspace frame accumulator is unavailable")]
    FrameStateUnavailable,
    /// Contained tear-off was requested without an application identity source.
    #[error("contained tear-off requires an application PresentationIdSource")]
    PresentationIdsUnavailable,
    /// The application identity source reused a live root identity.
    #[error("presentation identity source returned live root {root}")]
    RootIdentityCollision { root: RootId },
    /// The application identity source reused a live contained-presentation identity.
    #[error("presentation identity source returned live floating presentation {floating}")]
    FloatingIdentityCollision { floating: FloatingPresentationId },
    /// The deterministic contained stack cannot advance without wrapping.
    #[error("contained floating z-order is exhausted on surface {surface}")]
    ContainedZOrderExhausted { surface: SurfaceId },
    /// A proof-bearing bounds reconciliation was consumed without updating its presentation.
    #[error("contained placement reconciliation did not update presentation {floating}")]
    ContainedPlacementRejected { floating: FloatingPresentationId },
}
