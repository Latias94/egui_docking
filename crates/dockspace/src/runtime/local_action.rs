//! Opaque actions prepared from one exact paintable surface candidate.

use std::fmt;
use std::sync::Arc;

use thiserror::Error;

use crate::engine::{
    EngineInput, LocalSurfaceAction as SurfaceAction, LocalTabChromeAction, LocalTabFocus,
};
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
    pub(super) fn into_core(self) -> crate::tab_strip::TabNavigation {
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
    pub(super) fn direction(self) -> f64 {
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
    pub(super) fn direction(self) -> f64 {
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
    witness: Arc<crate::scene::SurfacePlanScene>,
}

impl PreparedSurfaceAction {
    pub(super) fn select_tab(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        tab: TabSceneId,
        focus: LocalTabFocus,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::SelectTab { scene, tab, focus },
        }
    }

    pub(super) fn close(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        target: CloseSceneTarget,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::Close { scene, target },
        }
    }

    pub(super) fn presentation_command(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        action: crate::model::ProductAction,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::PresentationCommand { scene, action },
        }
    }

    pub(super) fn close_root(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        root: crate::ids::RootId,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::CloseRoot { scene, root },
        }
    }

    pub(super) fn adjust_splitter(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        splitter: SplitterSceneId,
        delta: f64,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::AdjustSplitter {
                scene,
                splitter,
                delta,
            },
        }
    }

    pub(super) fn adjust_splitter_junction(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        junction: SplitterJunctionId,
        axis: crate::model::DockspaceAxis,
        delta: f64,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::AdjustSplitterJunction {
                scene,
                junction,
                axis,
                delta,
            },
        }
    }

    pub(super) fn adjust_contained_resize(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        floating: FloatingPresentationId,
        direction: ContainedResizeDirection,
        delta: f64,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::AdjustContainedResize {
                scene,
                floating,
                direction,
                delta,
            },
        }
    }

    pub(super) fn cancel_with_escape(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        surface: SurfaceId,
        witness: Arc<crate::scene::SurfacePlanScene>,
        status: crate::interaction::InteractionStatus,
        delivery: EscapeDelivery,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface,
            witness,
            action: SurfaceAction::CancelWithEscape { status, delivery },
        }
    }

    pub(super) fn tab_chrome(
        authority_domain: EngineAuthorityDomainId,
        expected: WorkspaceVersion,
        scene: SurfaceSceneStamp,
        witness: Arc<crate::scene::SurfacePlanScene>,
        action: LocalTabChromeAction,
    ) -> Self {
        Self {
            authority_domain,
            expected,
            surface: scene.surface(),
            witness,
            action: SurfaceAction::TabChrome { scene, action },
        }
    }

    /// Retains this action for a later update without preparing its target again.
    #[must_use]
    pub fn into_deferred(self) -> DeferredSurfaceAction {
        DeferredSurfaceAction(self)
    }

    /// Compares event intent while ignoring the captured scene and workspace version.
    ///
    /// Adapters may compare repeated passes within one event slot. Equal intent
    /// does not identify an event and must not deduplicate independent runs.
    #[must_use]
    pub fn same_deferred_intent(&self, other: &Self) -> bool {
        self.surface == other.surface && self.action.same_intent(&other.action)
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

    /// Returns the tab which should receive immediate framework focus.
    ///
    /// Navigation preserves this focus after selection commits. Content activation
    /// subsequently requests pane focus through the ordinary observation path.
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
            | SurfaceAction::CancelWithEscape { .. } => None,
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
            SurfaceAction::SelectTab { scene, tab, focus } => EngineInput::SelectLocalSceneTab {
                expected: self.expected,
                scene,
                tab,
                focus,
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
            SurfaceAction::CancelWithEscape { delivery, .. } => {
                EngineInput::CancelActiveInteractionWithEscape {
                    expected: self.expected,
                    delivery,
                }
            }
        })
    }
}

/// One owned semantic intent retaining the exact scene that exposed its target.
///
/// The core validates relevant current state when the action enters a later
/// update. A discarded encoding pass does not invalidate the owned witness.
/// Hosts may clone it for an attempt and retain the original until publication.
/// Cloning grants no event identity or deduplication: submit each original event
/// once after a successful publication, and retire it only then.
#[must_use = "a deferred surface action must be submitted or deliberately discarded"]
#[derive(PartialEq)]
pub struct DeferredSurfaceAction(PreparedSurfaceAction);

impl Clone for DeferredSurfaceAction {
    fn clone(&self) -> Self {
        Self(PreparedSurfaceAction {
            authority_domain: self.0.authority_domain,
            expected: self.0.expected,
            surface: self.0.surface,
            action: self.0.action.clone(),
            witness: Arc::clone(&self.0.witness),
        })
    }
}

impl DeferredSurfaceAction {
    /// Returns the logical surface that exposed the action.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.0.surface()
    }

    /// Returns the workspace version observed when the action was prepared.
    #[must_use]
    pub const fn expected_version(&self) -> WorkspaceVersion {
        self.0.expected_version()
    }

    /// Returns the tab which should receive immediate framework focus.
    ///
    /// Navigation preserves this focus after selection commits; content activation
    /// subsequently requests pane focus.
    #[must_use]
    pub const fn tab_focus_target(&self) -> Option<ItemId> {
        self.0.tab_focus_target()
    }

    /// Compares semantics within one adapter event slot, ignoring scene and version.
    ///
    /// This comparison supplies no event identity. Independent events with the
    /// same intent must each be submitted to the core.
    #[must_use]
    pub fn same_intent(&self, other: &Self) -> bool {
        self.0.same_deferred_intent(&other.0)
    }

    pub(super) fn into_engine_input(self) -> EngineInput {
        EngineInput::ApplyDeferredSurfaceAction {
            expected: self.0.expected,
            witness: self.0.witness,
            action: self.0.action,
        }
    }
}

impl fmt::Debug for DeferredSurfaceAction {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DeferredSurfaceAction")
            .field(&self.0)
            .finish()
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

#[derive(Debug, Error)]
#[error("prepared surface action belongs to another dockspace authority domain")]
pub(super) struct PreparedSurfaceActionAuthorityMismatch;
