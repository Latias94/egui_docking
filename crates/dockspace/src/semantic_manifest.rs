//! Output-bound, renderer-neutral semantic receiver inventory.
//!
//! Semantic receivers are intentionally independent from pointer hit geometry.
//! A menu row may remain keyboard and accessibility reachable while clipped out
//! of the pointer viewport, so using the hit manifest as its authority would
//! incorrectly erase a real presented control.

use crate::presentation_hit::PresentationHitRegionKind;
use crate::presentation_observation::SurfacePresentationOutputTicket;
use crate::scene::PresentationPlan;
use crate::semantic_input::{SemanticAccessibilityAction, SemanticKey, SemanticReceiverAction};

/// Exact semantic receiver roster bound to one presented output.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresentationSemanticManifest {
    output: SurfacePresentationOutputTicket,
    receivers: Vec<PresentationHitRegionKind>,
}

impl PresentationSemanticManifest {
    pub(crate) fn compile(
        output: SurfacePresentationOutputTicket,
        plan: &PresentationPlan,
    ) -> Self {
        debug_assert_eq!(output.surface(), plan.surface());
        let mut receivers = Vec::new();

        for tab in plan.tab_records() {
            receivers.push(PresentationHitRegionKind::TabBody(*tab.id()));
            if tab.close_bounds().is_some() {
                receivers.push(PresentationHitRegionKind::TabClose(*tab.id()));
            }
        }
        receivers.extend(
            plan.tab_strip_control_records()
                .iter()
                .filter(|control| control.enabled())
                .map(|control| PresentationHitRegionKind::TabStripControl(control.id())),
        );
        for menu in plan.tab_list_menu_records() {
            if menu.maximum_scroll_offset() > 0.0 {
                receivers.push(PresentationHitRegionKind::TabListMenuScroll(menu.session()));
            }
            receivers.extend(menu.rows().iter().map(|row| {
                PresentationHitRegionKind::TabListMenuRow {
                    menu: menu.session(),
                    tab: row.tab(),
                }
            }));
        }
        receivers.extend(
            plan.splitter_records()
                .iter()
                .filter(|splitter| splitter.operable())
                .map(|splitter| PresentationHitRegionKind::SplitterHandle(*splitter.id())),
        );
        for contained in plan.contained_records() {
            if contained.close_bounds().is_some() {
                receivers.push(PresentationHitRegionKind::ContainedClose(
                    contained.floating(),
                ));
            }
            receivers.extend(contained.resize().iter().filter_map(|resize| {
                is_cardinal_resize(resize.direction()).then_some(
                    PresentationHitRegionKind::ContainedResize {
                        floating: contained.floating(),
                        direction: resize.direction(),
                    },
                )
            }));
        }

        receivers.sort_unstable();
        receivers.dedup();
        Self { output, receivers }
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
