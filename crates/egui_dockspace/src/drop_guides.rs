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
    use dockspace::command::MovePayload;
    use dockspace::drop_guide::DropGuideScope;
    use dockspace::drop_resolver::{
        DropAffordance, DropAffordanceTarget, DropResolution, resolve_drop,
    };
    use dockspace::engine::{
        DockEngine, HostFrameView, HostPresentationDisposition, HostPresentationUnavailableReason,
    };
    use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
    use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId};
    use dockspace::intent::Authority;
    use dockspace::interaction::{DragGeneration, DragSessionId};
    use dockspace::policy::DockPolicy;
    use dockspace::presentation_observation::{
        HostPresentationCaptureGeneration, HostPresentationObservation,
        HostPresentationObservationEntry, HostPresentationProgress,
        HostPresentationStreamObservation,
    };
    use dockspace::scene::{PresentationPlan, SurfaceScene};
    use dockspace::scene_manifest::{
        Measurement, MeasurementUnavailableReason, SurfaceMeasurements, TabIntrinsic,
        TabStripMetrics,
    };
    use dockspace::transition::SurfaceContributionOutcome;
    use egui::{Painter, Pos2, RawInput, Shape, pos2};

    use super::*;

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
    const SOURCE_ROOT: RootId = RootId::new(10);
    const TARGET_ROOT: RootId = RootId::new(20);
    const MOVED_ITEM: ItemId = ItemId::new(10);
    const SOURCE_REMAINDER: ItemId = ItemId::new(11);
    const TARGET_LEFT_ITEM: ItemId = ItemId::new(20);
    const TARGET_RIGHT_ITEM: ItemId = ItemId::new(21);
    fn logical_rect(width: f64, height: f64) -> LogicalRect {
        LogicalRect::new(0.0, 0.0, width, height).expect("test bounds are valid")
    }

    fn authoritative_measurements(
        view: HostFrameView<'_>,
        surface: SurfaceId,
    ) -> SurfaceMeasurements {
        let requirements = view
            .presentation_requirements()
            .surface(surface)
            .expect("test surface requirements exist");
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(
                requirements.bounds(),
                Measurement::Measured(logical_rect(400.0, 300.0)),
            )
            .expect("surface bounds answer is unique");
        let minimum = LogicalSize::new(0.0, 0.0).expect("zero minimum is valid");
        for key in requirements.pane_minimums() {
            measurements
                .insert_pane_minimum(key, Measurement::Measured(minimum))
                .expect("pane minimum answer is unique");
        }
        for key in requirements.tab_intrinsics() {
            measurements
                .insert_tab_intrinsic(
                    key,
                    Measurement::Measured(TabIntrinsic::new(56.0).expect("tab intrinsic is valid")),
                )
                .expect("tab intrinsic answer is unique");
        }
        for key in requirements.tab_strips() {
            measurements
                .insert_tab_strip(
                    key,
                    Measurement::Measured(
                        TabStripMetrics::new(0.0, 0.0).expect("tab strip metrics are valid"),
                    ),
                )
                .expect("tab strip answer is unique");
        }
        measurements
    }

    fn record_painted_or_deferred_contributions(
        frame: dockspace::engine::CoreHostFrame,
        except: Option<SurfaceId>,
    ) -> dockspace::engine::CoreHostPresentationFrame {
        let mut frame = frame
            .into_presentation()
            .expect("test frame enters its presentation phase");
        let mut obligations = frame
            .take_presentation_obligations()
            .expect("test frame issues its exact physical presentation roster")
            .into_iter()
            .map(|obligation| (obligation.slot().surface(), obligation))
            .collect::<std::collections::BTreeMap<_, _>>();
        for surface in frame.surfaces().collect::<Vec<_>>() {
            if Some(surface) == except {
                continue;
            }
            let (ready, token) = {
                let view = frame.view();
                (
                    view.scene()
                        .surface(surface)
                        .and_then(SurfaceScene::ready)
                        .is_some(),
                    view.begin_surface_contribution(surface)
                        .expect("test surface accepts one contribution"),
                )
            };
            if ready {
                let obligation = obligations
                    .remove(&surface)
                    .expect("ready surface has one presentation obligation");
                let interaction = frame
                    .view()
                    .presentation_interaction(surface)
                    .unwrap_or_default();
                frame
                    .record_painted_surface_contribution(obligation, token, interaction)
                    .expect("test surface records an actual retained paint");
            } else {
                let contribution = frame
                    .view()
                    .prepare_surface_unavailable_contribution(
                        token,
                        MeasurementUnavailableReason::Deferred,
                    )
                    .expect("bootstrap test surface can explicitly defer measurement");
                frame
                    .push_surface_contribution(contribution)
                    .expect("one test host frame carries every frozen surface");
            }
        }
        for (_, obligation) in obligations {
            frame
                .resolve_presentation_obligation(
                    obligation,
                    HostPresentationDisposition::Unavailable(
                        HostPresentationUnavailableReason::OutputNotProduced,
                    ),
                )
                .expect("unpainted test presentation slot settles explicitly");
        }
        frame
    }

    fn publish_surface(engine: &mut DockEngine, surface: SurfaceId) -> PresentationPlan {
        let host = engine
            .create_presentation_host()
            .expect("test presentation host mints");
        let mut prelude = engine
            .begin_host_frame(host)
            .expect("test host frame begins");
        prelude
            .submit_presentation_observation(HostPresentationObservation::NoUpdate)
            .expect("test observation submits");
        let mut frame = prelude.seal(engine).expect("test host frame seals");
        let contribution = {
            let view = frame.view();
            let token = view
                .begin_surface_contribution(surface)
                .expect("test surface accepts one contribution");
            let measurements = authoritative_measurements(view, surface);
            view.prepare_surface_contribution(token, measurements)
                .expect("test surface measurements prepare successfully")
        };
        frame
            .push_surface_contribution(contribution)
            .expect("test surface contribution fits the host frame");
        let frame = record_painted_or_deferred_contributions(frame, Some(surface));
        let transition = frame.finish(engine).expect("surface contribution reduces");
        let (stamp, ticket) = transition
            .surface_contributions()
            .iter()
            .find_map(|outcome| match outcome {
                SurfaceContributionOutcome::Ready {
                    surface: actual,
                    stamp,
                    ticket,
                } if *actual == surface => Some((*stamp, *ticket)),
                _ => None,
            })
            .expect("target surface installs one ready contribution");
        let mut prelude = engine
            .begin_host_frame(host)
            .expect("test emission frame begins");
        prelude
            .submit_presentation_observation(HostPresentationObservation::NoUpdate)
            .expect("test observation submits");
        let frame = prelude.seal(engine).expect("test emission frame seals");
        let frame = record_painted_or_deferred_contributions(frame, None);
        let emitted = frame
            .finish(engine)
            .expect("surface output emission reduces");
        let outputs = emitted
            .presentation_emissions()
            .iter()
            .map(|emission| emission.output())
            .collect::<Vec<_>>();
        let output = outputs
            .iter()
            .copied()
            .find(|output| output.surface() == surface)
            .expect("one actual surface output emits");

        let mut prelude = engine
            .begin_host_frame(host)
            .expect("test observation frame begins");
        prelude
            .submit_presentation_observation(HostPresentationObservation::Batch(
                outputs
                    .iter()
                    .copied()
                    .map(|candidate| {
                        HostPresentationObservationEntry::new(
                            candidate.stream(),
                            HostPresentationStreamObservation::Captured {
                                generation: HostPresentationCaptureGeneration::new(1),
                                progress: HostPresentationProgress::Retired {
                                    settled_through: candidate.key(),
                                    presented: Authority::Known(
                                        (candidate == output).then_some(candidate.key()),
                                    ),
                                },
                            },
                        )
                    })
                    .collect(),
            ))
            .expect("exact presentation observation submits");
        let frame = prelude.seal(engine).expect("test observation frame seals");
        let frame = record_painted_or_deferred_contributions(frame, None);
        let transition = frame
            .finish(engine)
            .expect("surface presentation observation reduces");
        assert!(
            transition
                .presentation_observations()
                .iter()
                .any(|outcome| {
                    matches!(
                outcome,
                dockspace::presentation_observation::HostPresentationObservationOutcome::Retired {
                    promotion_eligible: true,
                    ..
                }
            )
                })
        );
        assert!(transition.surface_contributions().iter().any(|outcome| {
            matches!(
                outcome,
                SurfaceContributionOutcome::Retained {
                    surface: actual,
                    stamp: retained,
                    ticket: retained_ticket,
                } if *actual == surface && *retained == stamp && *retained_ticket == ticket
            )
        }));
        let projection = engine
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::paint_projection)
            .expect("surface contribution installs a paint projection");
        let plan = projection.plan().clone();
        plan
    }

    fn midpoint(bounds: LogicalRect) -> LogicalPoint {
        LogicalPoint::new(
            bounds.x() + bounds.width() * 0.5,
            bounds.y() + bounds.height() * 0.5,
        )
        .expect("guide midpoint is valid")
    }

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
    fn core_compiled_inner_and_outer_guides_paint_at_authoritative_bounds() {
        // Behavior source: Open GPUI revision 56604588, Apache-2.0,
        // `host_render_tests::{drop_guides_render_while_tab_drag_is_active,
        // root_drop_guides_use_outer_edge_drop_box_geometry}`. This port uses exact
        // core-owned geometry instead of Open GPUI's radial guide selection.
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([MOVED_ITEM, SOURCE_REMAINDER]));
        let target_left = builder.insert_node(Node::tabs([TARGET_LEFT_ITEM]));
        let target_right = builder.insert_node(Node::tabs([TARGET_RIGHT_ITEM]));
        let target_split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [target_left, target_right])
                .expect("target split is valid"),
        );
        builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
        builder.set_root(
            TARGET_ROOT,
            RootRecord::new(target_split).with_central(target_right),
        );
        builder.set_surface(OTHER_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
        let workspace = builder.build().expect("guide workspace is valid");
        let mut engine =
            DockEngine::new(workspace, DockPolicy::default()).expect("guide engine is valid");
        let source_plan = publish_surface(&mut engine, OTHER_SURFACE);
        let target_plan = publish_surface(&mut engine, SURFACE);

        let inner_plan = target_plan
            .drop_guide_clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Inner(target_left))
            .expect("non-central target leaf has an inner guide cluster");
        let query_point = midpoint(
            inner_plan
                .target(DropGuideSlot::Center)
                .expect("inner center target exists")
                .target()
                .region()
                .rect(),
        );

        assert_eq!(source_plan.surface(), OTHER_SURFACE);

        let payload = MovePayload::Item(
            engine
                .workspace()
                .capture_item_source(SOURCE_ROOT, source_tabs, MOVED_ITEM)
                .expect("source item remains current"),
        );
        let query = resolve_drop(
            engine.scene(),
            engine.workspace(),
            engine.policy_snapshot(),
            DragSessionId::new(engine.version().epoch(), DragGeneration::new(1)),
            payload,
            None,
            SURFACE,
            query_point,
        )
        .expect("real drop query has no invariant failure");
        let (resolution, affordance) = query.into_parts();
        assert!(matches!(resolution, DropResolution::Resolved(_)));
        let affordance = affordance.expect("active target leaf publishes guide affordance");
        assert_eq!(
            affordance.active_target().map(DropAffordanceTarget::slot),
            Some(DropGuideSlot::Center)
        );

        let inner = affordance
            .clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Inner(target_left))
            .expect("resolved affordance keeps the inner cluster");
        assert_eq!(
            inner
                .targets()
                .iter()
                .map(DropAffordanceTarget::slot)
                .collect::<Vec<_>>(),
            vec![
                DropGuideSlot::Center,
                DropGuideSlot::Edge(Edge::Left),
                DropGuideSlot::Edge(Edge::Right),
                DropGuideSlot::Edge(Edge::Top),
                DropGuideSlot::Edge(Edge::Bottom),
            ]
        );
        let outer = affordance
            .clusters()
            .iter()
            .find(|cluster| cluster.id().scope == DropGuideScope::Outer)
            .expect("resolved affordance keeps the root outer cluster");
        assert_eq!(
            outer
                .targets()
                .iter()
                .map(DropAffordanceTarget::slot)
                .collect::<Vec<_>>(),
            vec![
                DropGuideSlot::Edge(Edge::Left),
                DropGuideSlot::Edge(Edge::Right),
                DropGuideSlot::Edge(Edge::Top),
                DropGuideSlot::Edge(Edge::Bottom),
            ]
        );

        let mut authoritative_draws = Vec::new();
        for cluster in affordance.clusters() {
            let compiled = target_plan
                .drop_guide_clusters()
                .iter()
                .find(|candidate| candidate.id() == cluster.id())
                .expect("every affordance cluster comes from the compiled plan");
            let compiled_targets = compiled.targets().collect::<Vec<_>>();
            assert_eq!(cluster.targets().len(), compiled_targets.len());
            for (target, (slot, compiled_target)) in cluster.targets().iter().zip(compiled_targets)
            {
                assert_eq!(target.slot(), slot);
                assert_eq!(target.draw(), compiled_target.draw());
                authoritative_draws.push(
                    from_logical_rect(compiled_target.draw())
                        .expect("compiled guide draw bounds fit egui coordinates"),
                );
            }
        }

        let style = DockStyle::default();
        let paint = paint_plan(SURFACE, Some(&affordance), &style);
        assert_eq!(paint.buttons.len(), 9);
        assert_eq!(
            paint
                .buttons
                .iter()
                .map(|button| button.draw)
                .collect::<Vec<_>>(),
            authoritative_draws
        );

        let context = egui::Context::default();
        let output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(400.0, 300.0))),
                ..RawInput::default()
            },
            |ui| super::paint(ui.painter(), SURFACE, Some(&affordance), &style),
        );
        let shape_groups = output.shapes.chunks_exact(3);
        assert!(shape_groups.remainder().is_empty());
        assert_eq!(shape_groups.len(), 9);
        assert_eq!(
            shape_groups
                .map(|group| match &group[0].shape {
                    Shape::Rect(shape) => shape.rect,
                    shape => panic!("guide button must emit a rectangle, got {shape:?}"),
                })
                .collect::<Vec<_>>(),
            authoritative_draws
        );
    }

    #[test]
    fn painter_boundary_has_no_action_or_acknowledgement_channel() {
        let paint_boundary: fn(&Painter, SurfaceId, Option<&DropAffordance>, &DockStyle) = paint;

        let _: fn(&Painter, SurfaceId, Option<&DropAffordance>, &DockStyle) = paint_boundary;
    }
}
