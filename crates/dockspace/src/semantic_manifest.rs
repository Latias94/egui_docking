//! Output-bound, renderer-neutral semantic receiver inventory.
//!
//! Semantic receivers are intentionally independent from pointer hit geometry.
//! A menu row may remain keyboard and accessibility reachable while clipped out
//! of the pointer viewport, so using the hit manifest as its authority would
//! incorrectly erase a real presented control.

use std::convert::Infallible;
use std::sync::Arc;

use crate::presentation_hit::PresentationHitRegionKind;
use crate::presentation_observation::SurfacePresentationOutputTicket;
use crate::scene::{
    PresentationPlan, ResolvedSurfaceProgramOperation, ResolvedSurfaceProgramStep, SceneBuildError,
};
use crate::semantic_input::{SemanticAccessibilityAction, SemanticKey, SemanticReceiverAction};

/// Exact semantic receiver roster bound to one presented output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationSemanticManifest {
    output: SurfacePresentationOutputTicket,
    receivers: Arc<[PresentationHitRegionKind]>,
}

impl PresentationSemanticManifest {
    pub(crate) fn rebind_output(&self, output: SurfacePresentationOutputTicket) -> Self {
        Self {
            output,
            receivers: Arc::clone(&self.receivers),
        }
    }

    pub(crate) fn compile(
        output: SurfacePresentationOutputTicket,
        plan: &PresentationPlan,
    ) -> Result<Self, SceneBuildError> {
        debug_assert_eq!(output.surface(), plan.surface());
        let mut receivers = Vec::new();

        plan.visit_surface_program(|step| {
            match step {
                ResolvedSurfaceProgramStep::Operation { operation, .. } => match operation {
                    ResolvedSurfaceProgramOperation::Tab(tab) => {
                        receivers.push(PresentationHitRegionKind::TabBody(*tab.id()));
                        if tab.close_bounds().is_some() {
                            receivers.push(PresentationHitRegionKind::TabClose(*tab.id()));
                        }
                    }
                    ResolvedSurfaceProgramOperation::TabStripControl(control)
                        if control.enabled() =>
                    {
                        receivers.push(PresentationHitRegionKind::TabStripControl(control.id()));
                    }
                    ResolvedSurfaceProgramOperation::Splitter(splitter) if splitter.operable() => {
                        receivers.push(PresentationHitRegionKind::SplitterHandle(*splitter.id()));
                    }
                    ResolvedSurfaceProgramOperation::ContainedControls(contained) => {
                        if contained.close_bounds().is_some() {
                            receivers.push(PresentationHitRegionKind::ContainedClose(
                                contained.floating(),
                            ));
                        }
                        if contained.transform_operable() {
                            receivers.extend(contained.resize().iter().filter_map(|resize| {
                                is_cardinal_resize(resize.direction()).then_some(
                                    PresentationHitRegionKind::ContainedResize {
                                        floating: contained.floating(),
                                        direction: resize.direction(),
                                    },
                                )
                            }));
                        }
                    }
                    ResolvedSurfaceProgramOperation::Pane(_)
                    | ResolvedSurfaceProgramOperation::TabBar(_)
                    | ResolvedSurfaceProgramOperation::Splitter(_)
                    | ResolvedSurfaceProgramOperation::SplitterJunction(_)
                    | ResolvedSurfaceProgramOperation::TabStripControl(_)
                    | ResolvedSurfaceProgramOperation::PresentationMenuAnchor(_) => {}
                },
                ResolvedSurfaceProgramStep::PopupMenu(menu) => {
                    if menu.maximum_scroll_offset() > 0.0 {
                        receivers
                            .push(PresentationHitRegionKind::TabListMenuScroll(menu.session()));
                    }
                    receivers.extend(menu.rows().iter().map(|row| {
                        PresentationHitRegionKind::TabListMenuRow {
                            menu: menu.session(),
                            tab: row.tab(),
                        }
                    }));
                }
                ResolvedSurfaceProgramStep::RootBegin(_)
                | ResolvedSurfaceProgramStep::RootEnd(_)
                | ResolvedSurfaceProgramStep::PopupBackdrop(_)
                | ResolvedSurfaceProgramStep::DragPreview
                | ResolvedSurfaceProgramStep::DropTarget(_)
                | ResolvedSurfaceProgramStep::DropGuide(_)
                | ResolvedSurfaceProgramStep::ContainedTransformPreview
                | ResolvedSurfaceProgramStep::DragDecoration
                | ResolvedSurfaceProgramStep::InteractionFeedback
                | ResolvedSurfaceProgramStep::SurfaceEnd => {}
            }
            Ok::<(), Infallible>(())
        })
        .map_err(|_| SceneBuildError::InvalidSurfaceProgram {
            surface: plan.surface(),
        })?;

        receivers.sort_unstable();
        receivers.dedup();
        Ok(Self {
            output,
            receivers: receivers.into(),
        })
    }

    /// Returns the exact output which minted this roster.
    #[must_use]
    pub const fn output(&self) -> SurfacePresentationOutputTicket {
        self.output
    }

    /// Returns every core-consumable semantic receiver in stable order.
    #[must_use]
    pub fn receivers(&self) -> &[PresentationHitRegionKind] {
        &self.receivers
    }

    /// Returns whether the exact output exposes this semantic receiver.
    #[must_use]
    pub fn contains(&self, target: PresentationHitRegionKind) -> bool {
        self.receivers.binary_search(&target).is_ok()
    }

    /// Returns whether core defines the requested action for this exact receiver.
    #[must_use]
    pub fn supports(
        &self,
        target: PresentationHitRegionKind,
        action: SemanticReceiverAction,
    ) -> bool {
        self.contains(target) && receiver_supports(target, action)
    }
}

const fn receiver_supports(
    target: PresentationHitRegionKind,
    action: SemanticReceiverAction,
) -> bool {
    match (target, action) {
        (
            PresentationHitRegionKind::TabBody(_),
            SemanticReceiverAction::Key(
                SemanticKey::ArrowLeft
                | SemanticKey::ArrowRight
                | SemanticKey::Home
                | SemanticKey::End
                | SemanticKey::Enter
                | SemanticKey::Space,
            )
            | SemanticReceiverAction::Accessibility(
                SemanticAccessibilityAction::Click | SemanticAccessibilityAction::Focus,
            ),
        )
        | (
            PresentationHitRegionKind::TabClose(_)
            | PresentationHitRegionKind::ContainedClose(_)
            | PresentationHitRegionKind::TabStripControl(_),
            SemanticReceiverAction::Key(SemanticKey::Enter | SemanticKey::Space)
            | SemanticReceiverAction::Accessibility(SemanticAccessibilityAction::Click),
        )
        | (
            PresentationHitRegionKind::SplitterHandle(_),
            SemanticReceiverAction::Key(
                SemanticKey::ArrowLeft
                | SemanticKey::ArrowRight
                | SemanticKey::ArrowUp
                | SemanticKey::ArrowDown,
            )
            | SemanticReceiverAction::Accessibility(
                SemanticAccessibilityAction::Increment | SemanticAccessibilityAction::Decrement,
            ),
        )
        | (
            PresentationHitRegionKind::TabListMenuRow { .. },
            SemanticReceiverAction::Key(
                SemanticKey::ArrowUp
                | SemanticKey::ArrowDown
                | SemanticKey::Home
                | SemanticKey::End
                | SemanticKey::Enter
                | SemanticKey::Space,
            )
            | SemanticReceiverAction::Accessibility(
                SemanticAccessibilityAction::Click
                | SemanticAccessibilityAction::Focus
                | SemanticAccessibilityAction::ScrollIntoView,
            ),
        )
        | (
            PresentationHitRegionKind::TabListMenuScroll(_),
            SemanticReceiverAction::Accessibility(
                SemanticAccessibilityAction::Increment | SemanticAccessibilityAction::Decrement,
            ),
        )
        | (
            PresentationHitRegionKind::ContainedResize {
                direction:
                    crate::scene::ContainedResizeDirection::East
                    | crate::scene::ContainedResizeDirection::West,
                ..
            },
            SemanticReceiverAction::Key(SemanticKey::ArrowLeft | SemanticKey::ArrowRight)
            | SemanticReceiverAction::Accessibility(
                SemanticAccessibilityAction::Increment | SemanticAccessibilityAction::Decrement,
            ),
        )
        | (
            PresentationHitRegionKind::ContainedResize {
                direction:
                    crate::scene::ContainedResizeDirection::North
                    | crate::scene::ContainedResizeDirection::South,
                ..
            },
            SemanticReceiverAction::Key(SemanticKey::ArrowUp | SemanticKey::ArrowDown)
            | SemanticReceiverAction::Accessibility(
                SemanticAccessibilityAction::Increment | SemanticAccessibilityAction::Decrement,
            ),
        ) => true,
        _ => false,
    }
}

const fn is_cardinal_resize(direction: crate::scene::ContainedResizeDirection) -> bool {
    matches!(
        direction,
        crate::scene::ContainedResizeDirection::North
            | crate::scene::ContainedResizeDirection::East
            | crate::scene::ContainedResizeDirection::South
            | crate::scene::ContainedResizeDirection::West
    )
}
