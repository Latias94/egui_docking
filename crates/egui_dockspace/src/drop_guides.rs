//! Pure docking-guide paint planning and egui shape emission.

use dockspace::command::Edge;
use dockspace::drop_guide::DropGuideSlot;
use dockspace::drop_resolver::{DropAffordance, DropGuideEligibility};
use dockspace::ids::SurfaceId;
use egui::{Color32, Painter, Rect, Stroke, StrokeKind, vec2};

use crate::renderer::from_logical_rect;
use crate::style::DockStyle;

const BUTTON_CORNER_RADIUS: f32 = 3.0;
const CUE_INSET: f32 = 4.0;
const CUE_CENTER_EXTENT: f32 = 4.0;
const CUE_EDGE_EXTENT: f32 = 3.0;
const DISABLED_FILL_OPACITY: f32 = 0.35;
const DISABLED_CUE_OPACITY: f32 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GuideVisualState {
    Passive,
    Active,
    DisabledPassive,
    DisabledActive,
}

impl GuideVisualState {
    const fn from_facts(active: bool, eligible: bool) -> Self {
        match (active, eligible) {
            (false, true) => Self::Passive,
            (true, true) => Self::Active,
            (false, false) => Self::DisabledPassive,
            (true, false) => Self::DisabledActive,
        }
    }

    const fn active(self) -> bool {
        matches!(self, Self::Active | Self::DisabledActive)
    }

    const fn disabled(self) -> bool {
        matches!(self, Self::DisabledPassive | Self::DisabledActive)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GuideButtonFact {
    slot: DropGuideSlot,
    draw: Rect,
    state: GuideVisualState,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GuideButtonPaint {
    draw: Rect,
    fill: Color32,
    outline: Stroke,
    cue: GuideCuePaint,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct GuideCuePaint {
    pane: Rect,
    emphasis: Rect,
    color: Color32,
}

#[derive(Clone, Debug, Default, PartialEq)]
struct GuidePaintPlan {
    buttons: Vec<GuideButtonPaint>,
}

/// Paints guide buttons without reporting input, delivery, or paint proof.
pub(crate) fn paint(
    painter: &Painter,
    surface: SurfaceId,
    affordance: Option<&DropAffordance>,
    style: &DockStyle,
) {
    let plan = paint_plan(surface, affordance, style);
    for button in plan.buttons {
        painter.rect(
            button.draw,
            BUTTON_CORNER_RADIUS,
            button.fill,
            button.outline,
            StrokeKind::Inside,
        );
        painter.rect_stroke(
            button.cue.pane,
            0.0,
            Stroke::new(1.0, button.cue.color),
            StrokeKind::Inside,
        );
        painter.rect_filled(button.cue.emphasis, 0.0, button.cue.color);
    }
}

fn paint_plan(
    surface: SurfaceId,
    affordance: Option<&DropAffordance>,
    style: &DockStyle,
) -> GuidePaintPlan {
    let Some(affordance) = affordance else {
        return GuidePaintPlan::default();
    };
    let active = affordance
        .active_target()
        .map(dockspace::drop_resolver::DropAffordanceTarget::key);
    let facts = affordance.clusters().iter().flat_map(|cluster| {
        cluster.targets().iter().filter_map(move |target| {
            from_logical_rect(target.draw()).map(|draw| GuideButtonFact {
                slot: target.slot(),
                draw,
                state: GuideVisualState::from_facts(
                    active == Some(target.key()),
                    matches!(target.eligibility(), DropGuideEligibility::Eligible),
                ),
            })
        })
    });
    describe_for_surface(surface, affordance.surface(), facts, style)
}

fn describe_for_surface(
    requested: SurfaceId,
    affordance_surface: SurfaceId,
    facts: impl IntoIterator<Item = GuideButtonFact>,
    style: &DockStyle,
) -> GuidePaintPlan {
    if requested != affordance_surface {
        return GuidePaintPlan::default();
    }
    GuidePaintPlan {
        buttons: facts
            .into_iter()
            .map(|fact| describe_button(fact, style))
            .collect(),
    }
}

fn describe_button(fact: GuideButtonFact, style: &DockStyle) -> GuideButtonPaint {
    let active = fact.state.active();
    let disabled = fact.state.disabled();
    let base_fill = if active {
        style.drop_guide_active_fill
    } else {
        style.drop_guide_fill
    };
    let base_outline = if active {
        style.drop_guide_active_border_color
    } else {
        style.drop_guide_border_color
    };
    let fill = if disabled {
        base_fill.gamma_multiply(DISABLED_FILL_OPACITY)
    } else {
        base_fill
    };
    // An exact disabled hit keeps the active outline so rejection stays legible.
    let outline_color = if disabled && !active {
        base_outline.gamma_multiply(DISABLED_CUE_OPACITY)
    } else {
        base_outline
    };
    let cue_color = if disabled {
        base_outline.gamma_multiply(DISABLED_CUE_OPACITY)
    } else {
        base_outline
    };
    GuideButtonPaint {
        draw: fact.draw,
        fill,
        outline: Stroke::new(1.0, outline_color),
        cue: describe_cue(fact.slot, fact.draw, cue_color),
    }
}

fn describe_cue(slot: DropGuideSlot, button: Rect, color: Color32) -> GuideCuePaint {
    let inset = CUE_INSET
        .min(0.5 * button.width())
        .min(0.5 * button.height());
    let pane = button.shrink(inset);
    let emphasis = match slot {
        DropGuideSlot::Center => Rect::from_center_size(
            pane.center(),
            vec2(
                CUE_CENTER_EXTENT.min(pane.width()),
                CUE_CENTER_EXTENT.min(pane.height()),
            ),
        ),
        DropGuideSlot::Edge(edge) => edge_emphasis(pane, edge),
    };
    GuideCuePaint {
        pane,
        emphasis,
        color,
    }
}

fn edge_emphasis(pane: Rect, edge: Edge) -> Rect {
    let horizontal_extent = CUE_EDGE_EXTENT.min(pane.width());
    let vertical_extent = CUE_EDGE_EXTENT.min(pane.height());
    match edge {
        Edge::Left => Rect::from_min_max(
            pane.min,
            egui::pos2(pane.min.x + horizontal_extent, pane.max.y),
        ),
        Edge::Right => Rect::from_min_max(
            egui::pos2(pane.max.x - horizontal_extent, pane.min.y),
            pane.max,
        ),
        Edge::Top => Rect::from_min_max(
            pane.min,
            egui::pos2(pane.max.x, pane.min.y + vertical_extent),
        ),
        Edge::Bottom => Rect::from_min_max(
            egui::pos2(pane.min.x, pane.max.y - vertical_extent),
            pane.max,
        ),
    }
}

#[cfg(test)]
mod tests {
    use dockspace::drop_resolver::DropAffordance;
    use egui::{Painter, pos2};

    use super::*;

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);

    fn fact(slot: DropGuideSlot, state: GuideVisualState) -> GuideButtonFact {
        GuideButtonFact {
            slot,
            draw: Rect::from_min_max(pos2(10.0, 20.0), pos2(26.0, 36.0)),
            state,
        }
    }

    fn inner_facts() -> [GuideButtonFact; 5] {
        [
            fact(DropGuideSlot::Center, GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Left), GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Right), GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Top), GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Bottom), GuideVisualState::Passive),
        ]
    }

    fn outer_facts() -> [GuideButtonFact; 4] {
        [
            fact(DropGuideSlot::Edge(Edge::Left), GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Right), GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Top), GuideVisualState::Passive),
            fact(DropGuideSlot::Edge(Edge::Bottom), GuideVisualState::Passive),
        ]
    }

    #[test]
    fn complete_inner_and_outer_inputs_keep_every_button() {
        let style = DockStyle::default();
        let inner = describe_for_surface(SURFACE, SURFACE, inner_facts(), &style);
        let outer = describe_for_surface(SURFACE, SURFACE, outer_facts(), &style);

        assert_eq!(inner.buttons.len(), 5);
        assert_eq!(outer.buttons.len(), 4);
    }

    #[test]
    fn center_top_and_bottom_cues_have_distinct_emphasis_geometry() {
        let color = Color32::WHITE;
        let rect = fact(DropGuideSlot::Center, GuideVisualState::Passive).draw;
        let center = describe_cue(DropGuideSlot::Center, rect, color);
        let top = describe_cue(DropGuideSlot::Edge(Edge::Top), rect, color);
        let bottom = describe_cue(DropGuideSlot::Edge(Edge::Bottom), rect, color);

        assert_eq!(center.emphasis.center(), center.pane.center());
        assert!((top.emphasis.min.y - top.pane.min.y).abs() < f32::EPSILON);
        assert!((bottom.emphasis.max.y - bottom.pane.max.y).abs() < f32::EPSILON);
        assert_ne!(center.emphasis, top.emphasis);
        assert_ne!(top.emphasis, bottom.emphasis);
    }

    #[test]
    fn visual_states_select_active_passive_and_disabled_colors() {
        let style = DockStyle::default();
        let passive = describe_button(
            fact(DropGuideSlot::Center, GuideVisualState::Passive),
            &style,
        );
        let active = describe_button(
            fact(DropGuideSlot::Center, GuideVisualState::Active),
            &style,
        );
        let disabled_passive = describe_button(
            fact(DropGuideSlot::Center, GuideVisualState::DisabledPassive),
            &style,
        );
        let disabled_active = describe_button(
            fact(DropGuideSlot::Center, GuideVisualState::DisabledActive),
            &style,
        );

        assert_eq!(passive.fill, style.drop_guide_fill);
        assert_eq!(active.fill, style.drop_guide_active_fill);
        assert_eq!(active.outline.color, style.drop_guide_active_border_color);
        assert_eq!(
            disabled_passive.fill,
            style.drop_guide_fill.gamma_multiply(DISABLED_FILL_OPACITY)
        );
        assert_eq!(
            disabled_active.fill,
            style
                .drop_guide_active_fill
                .gamma_multiply(DISABLED_FILL_OPACITY)
        );
        assert_eq!(
            disabled_active.outline.color,
            style.drop_guide_active_border_color
        );
        assert_ne!(disabled_active.cue.color, Color32::TRANSPARENT);
    }

    #[test]
    fn another_surface_produces_no_shapes() {
        let plan =
            describe_for_surface(SURFACE, OTHER_SURFACE, inner_facts(), &DockStyle::default());

        assert!(plan.buttons.is_empty());
    }

    #[test]
    fn painter_boundary_has_no_action_or_acknowledgement_channel() {
        let paint_boundary: fn(&Painter, SurfaceId, Option<&DropAffordance>, &DockStyle) = paint;

        let _: fn(&Painter, SurfaceId, Option<&DropAffordance>, &DockStyle) = paint_boundary;
    }
}
