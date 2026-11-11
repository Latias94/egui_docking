use super::*;
use crate::behavior_tests::support::{self, MeasurementProfile, TestPresentationHost};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::graph::{RootRecord, SurfacePresentation};
use crate::ids::NodeId;
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspaceSemanticOutput, DockspaceSession, PaintedSurfaceOutput, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use crate::scene_compiler::{reset_surface_compilation_count, surface_compilation_count};
use crate::scene_manifest::{
    Measurement, PaneMinimumKey, TabIntrinsic, TabIntrinsicKey, TabStripMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn bounds(width: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, 600.0).unwrap()
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(bounds(800.0), LogicalSize::new(64.0, 48.0).unwrap(), 96.0).unwrap()
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap();
    DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap()
}

fn engine() -> DockEngine {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap()
}

fn measured_paint(
    session: &mut DockspaceSession,
) -> (DockspaceSemanticOutput, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let semantic = plan.semantic_output();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = frame.commit().unwrap();
    assert!(!report.workspace_changed());
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    (semantic, outputs.pop().unwrap())
}

#[test]
fn identical_measured_ready_answers_keep_distinct_paints_and_rollback_on_drop() {
    let mut session = session();
    reset_surface_compilation_count();
    let (first_semantic, first) = measured_paint(&mut session);
    assert_eq!(surface_compilation_count(), 1);
    let first_artifact = first.presentation_artifact();
    let version = session.version();

    reset_surface_compilation_count();
    {
        let mut abandoned = session.begin_host_frame().unwrap();
        abandoned.measure_surface(SURFACE, metrics()).unwrap();
        let plan = abandoned.paint_plan(SURFACE).unwrap().unwrap();
        assert_eq!(plan.semantic_output(), first_semantic);
        let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
        abandoned.confirm_surface_painted(receipt).unwrap();
    }
    assert_eq!(session.version(), version);
    assert!(session.presented_surface(SURFACE).is_none());
    assert_eq!(surface_compilation_count(), 0);

    let (second_semantic, second) = measured_paint(&mut session);
    assert_eq!(surface_compilation_count(), 0);
    assert_eq!(second_semantic, first_semantic);
    let second_artifact = second.presentation_artifact();
    assert_ne!(first_artifact, second_artifact);
    assert!(first.matches_semantic_output(second_semantic));
    assert!(second.matches_semantic_output(first_semantic));

    session
        .report_surface_presentation(first, SurfacePresentationResult::Dropped)
        .unwrap();
    session
        .report_surface_presentation(second, SurfacePresentationResult::Presented)
        .unwrap();
    let mut observe = session.begin_host_frame().unwrap();
    observe
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = observe.commit().unwrap();
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(second_artifact) && commit.retires(first_artifact))
    );
    assert_eq!(surface_compilation_count(), 0);
}

#[test]
fn same_ticket_changed_or_unavailable_measurements_do_not_reuse_ready_geometry() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, bounds(800.0));
    let ticket = engine.begin_surface_contribution(SURFACE).unwrap().ticket();

    let mut changed = host.begin(&engine);
    let token = changed.view().begin_surface_contribution(SURFACE).unwrap();
    assert_eq!(token.ticket(), ticket);
    let measurements = support::measurements(
        &engine,
        SURFACE,
        bounds(900.0),
        MeasurementProfile::default(),
    );
    reset_surface_compilation_count();
    let contribution = changed
        .view()
        .prepare_surface_contribution(token, measurements)
        .unwrap();
    assert_eq!(surface_compilation_count(), 1);
    let PreparedSurfacePaintCandidate::Ready(candidate) = contribution.paint_candidate() else {
        panic!("changed authoritative bounds must compile a Ready candidate");
    };
    assert_eq!(candidate.plan().bounds(), bounds(900.0));
    changed.push_surface_contribution(contribution).unwrap();
    host.finish(changed, &mut engine);
    assert_eq!(
        engine
            .scene()
            .ready_candidate(SURFACE)
            .unwrap()
            .plan()
            .bounds(),
        bounds(900.0)
    );

    let mut unavailable = host.begin(&engine);
    let token = unavailable
        .view()
        .begin_surface_contribution(SURFACE)
        .unwrap();
    assert_eq!(token.ticket(), ticket);
    let measurements = SurfaceMeasurements::unavailable(
        engine.presentation_requirements().surface(SURFACE).unwrap(),
        MeasurementUnavailableReason::TextMetricsUnavailable,
    );
    reset_surface_compilation_count();
    let contribution = unavailable
        .view()
        .prepare_surface_contribution(token, measurements)
        .unwrap();
    assert_eq!(surface_compilation_count(), 1);
    assert!(matches!(
        contribution.paint_candidate(),
        PreparedSurfacePaintCandidate::Unavailable { .. }
    ));
    unavailable.push_surface_contribution(contribution).unwrap();
    host.finish(unavailable, &mut engine);
    assert!(engine.scene().ready_candidate(SURFACE).is_none());
}

#[test]
fn reused_ready_answers_cannot_cross_a_new_requirement_ticket() {
    let mut engine = engine();
    let mut host = TestPresentationHost::new(&mut engine);
    support::install_surface_projection(&mut engine, &mut host, SURFACE, bounds(800.0));
    let token = engine.begin_surface_contribution(SURFACE).unwrap();
    let measurements = support::measurements(
        &engine,
        SURFACE,
        bounds(800.0),
        MeasurementProfile::default(),
    );
    reset_surface_compilation_count();
    let prepared = engine
        .prepare_surface_contribution(token, measurements.clone())
        .unwrap();
    assert_eq!(surface_compilation_count(), 0);
    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Ready(_)
    ));

    let expected = engine.version();
    support::submit_input(
        &mut engine,
        &mut host,
        StableInputSourceId::new(1),
        EngineInput::ReplacePresentationConfig {
            expected,
            config: DockPresentationConfig::builder()
                .tab_bar_height(44.0)
                .build()
                .unwrap(),
        },
    )
    .unwrap();
    assert!(matches!(
        engine.prepare_surface_contribution(token, measurements.clone()),
        Err(SurfaceContributionPrepareError::StaleBase { .. })
    ));
    let current = engine.begin_surface_contribution(SURFACE).unwrap();
    assert_ne!(current.ticket(), token.ticket());
    assert!(matches!(
        engine.prepare_surface_contribution(current, measurements),
        Err(SurfaceContributionPrepareError::TicketMismatch { .. })
    ));

    let mut late = host.begin(&engine);
    late.push_surface_contribution(prepared).unwrap();
    let transition = host.finish(late, &mut engine);
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Rejected {
            reason: SurfaceContributionRejection::StaleBase { .. },
            ..
        }]
    ));
    assert!(engine.scene().ready_candidate(SURFACE).is_none());
}

#[test]
fn changed_selection_retains_sealed_facts_but_rebinds_current_semantics_and_output() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut engine = DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap();
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, bounds(800.0));
    let original = engine.scene().ready_candidate(SURFACE).unwrap().clone();
    let previous_version = engine.version();
    support::submit_input(
        &mut engine,
        &mut host,
        StableInputSourceId::new(1),
        EngineInput::SelectItem {
            expected: previous_version,
            item: SECOND,
        },
    )
    .unwrap();
    assert_ne!(engine.version(), previous_version);
    assert!(engine.presentation_authority.selection_rebind.is_some());
    let measurements = support::measurements(
        &engine,
        SURFACE,
        bounds(800.0),
        MeasurementProfile::default(),
    );
    let mut frame = host.begin(&engine);
    let token = frame.view().begin_surface_contribution(SURFACE).unwrap();
    reset_surface_compilation_count();
    let contribution = frame
        .view()
        .prepare_surface_contribution(token, measurements)
        .unwrap();
    assert_eq!(surface_compilation_count(), 0);
    let PreparedSurfacePaintCandidate::Ready(candidate) = contribution.paint_candidate() else {
        panic!("a fully measured local selection must be Ready");
    };
    assert!(candidate.plan().shares_facts(original.plan()));
    assert_eq!(
        candidate
            .plan()
            .pane_selected(&candidate.plan().pane_records()[0]),
        Some(SECOND)
    );
    assert_eq!(
        original
            .plan()
            .pane_selected(&original.plan().pane_records()[0]),
        Some(ITEM)
    );
    assert!(
        candidate
            .plan()
            .tab_records()
            .iter()
            .all(|tab| candidate.plan().tab_selected(tab) == (tab.id().item == SECOND))
    );
    let mut title = None;
    candidate
        .plan()
        .visit_surface_program(|step| {
            if let crate::scene::ResolvedSurfaceProgramStep::RootBegin(root) = step {
                title = root.title_item();
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    assert_eq!(title, Some(SECOND));
    frame.push_surface_contribution(contribution).unwrap();
    host.finish(frame, &mut engine);
    let current = engine.scene().ready_candidate(SURFACE).unwrap();
    assert_ne!(current.output_ticket(), original.output_ticket());
    assert!(current.plan().shares_facts(original.plan()));
    assert!(std::ptr::eq(
        current.paint_program().unwrap(),
        original.paint_program().unwrap()
    ));
    assert_eq!(
        current.hit_manifest().regions().as_ptr(),
        original.hit_manifest().regions().as_ptr()
    );
    assert_eq!(current.hit_manifest().output(), current.output_ticket());
    assert_eq!(
        engine.workspace().tab_mru(tabs),
        Some([SECOND, ITEM].as_slice())
    );
    assert!(engine.presentation_authority.selection_rebind.is_none());
    assert_eq!(surface_compilation_count(), 0);

    let measurements = support::measurements(
        &engine,
        SURFACE,
        bounds(800.0),
        MeasurementProfile::default(),
    );
    let conservative = crate::scene_compiler::compile_surface_plan(
        engine.workspace(),
        engine.version(),
        engine.policy_snapshot(),
        &engine.presentation_authority.presentation_config,
        engine.presentation_requirements(),
        &measurements,
        &engine.presentation_authority.tab_strip_states,
        &[],
        None,
    )
    .unwrap()
    .finalize()
    .unwrap();
    assert_eq!(current.plan(), &conservative);
}

fn selection_engine(items: impl IntoIterator<Item = ItemId>) -> (DockEngine, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (
        DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap(),
        tabs,
    )
}

fn item_measurements(
    engine: &DockEngine,
    bounds: LogicalRect,
    mut minimum: impl FnMut(Option<ItemId>) -> LogicalSize,
    mut intrinsic: impl FnMut(ItemId) -> f64,
) -> SurfaceMeasurements {
    let requirements = engine.presentation_requirements().surface(SURFACE).unwrap();
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .unwrap();
    if let Some(key) = requirements.popup_plane_bounds() {
        measurements
            .set_popup_plane_bounds(key, Measurement::Measured(bounds))
            .unwrap();
    }
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum(key.selected())))
            .unwrap();
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(intrinsic(key.tab().item)).unwrap()),
            )
            .unwrap();
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(TabStripMetrics::new(0.0, 0.0).unwrap()),
            )
            .unwrap();
    }
    measurements
}

fn publish_measurement_answers(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    measurements: SurfaceMeasurements,
) {
    let mut frame = host.begin(engine);
    let token = frame.view().begin_surface_contribution(SURFACE).unwrap();
    let contribution = frame
        .view()
        .prepare_surface_contribution(token, measurements)
        .unwrap();
    assert!(matches!(
        contribution.paint_candidate(),
        PreparedSurfacePaintCandidate::Ready(_)
    ));
    frame.push_surface_contribution(contribution).unwrap();
    host.finish(frame, engine);

    let frame = host.begin(engine);
    let frame = support::complete_host_frame_with_current_outputs(engine, frame);
    host.finish_presentation(frame, engine);
    let mut observation = host.begin(engine);
    support::complete_host_frame_with_retained_or_unavailable(engine, &mut observation);
    host.finish(observation, engine);
    assert!(engine.scene().interaction_projection(SURFACE).is_some());
}

#[test]
fn unchanged_answers_rebuild_when_selected_items_have_different_actual_minima() {
    let (mut engine, tabs) = selection_engine([ITEM, SECOND]);
    let mut host = TestPresentationHost::new(&mut engine);
    let minimum = |item| {
        if item == Some(SECOND) {
            LogicalSize::new(360.0, 240.0).unwrap()
        } else {
            LogicalSize::new(120.0, 100.0).unwrap()
        }
    };
    let initial_answers = item_measurements(&engine, bounds(800.0), minimum, |_| 56.0);
    publish_measurement_answers(&mut engine, &mut host, initial_answers);
    let original = engine.scene().ready_candidate(SURFACE).unwrap().clone();
    let previous_minimum = original
        .plan()
        .layout_facts()
        .unwrap()
        .root(ROOT)
        .unwrap()
        .projection()
        .nodes[&tabs]
        .constraints
        .min();
    let expected = engine.version();
    support::submit_input(
        &mut engine,
        &mut host,
        StableInputSourceId::new(2),
        EngineInput::SelectItem {
            expected,
            item: SECOND,
        },
    )
    .unwrap();
    assert!(engine.presentation_authority.selection_rebind.is_some());
    let answers = item_measurements(&engine, bounds(800.0), minimum, |_| 56.0);
    assert!(
        original
            .plan()
            .layout_facts()
            .unwrap()
            .measurements()
            .same_values(&answers),
        "all item answers remain identical while the selected constraint changes"
    );

    reset_surface_compilation_count();
    publish_measurement_answers(&mut engine, &mut host, answers.clone());
    assert_eq!(surface_compilation_count(), 1);
    let current = engine.scene().ready_candidate(SURFACE).unwrap();
    assert!(!current.plan().shares_facts(original.plan()));
    assert_eq!(
        current
            .plan()
            .pane_selected(&current.plan().pane_records()[0]),
        Some(SECOND)
    );
    let layout = current.plan().layout_facts().unwrap();
    let current_minimum = layout.root(ROOT).unwrap().projection().nodes[&tabs]
        .constraints
        .min();
    assert!(current_minimum.width() > previous_minimum.width());
    assert!(current_minimum.height() > previous_minimum.height());
    assert!(layout.measurements().same_values(&answers));
    assert_eq!(
        layout.measurements().ticket(),
        current.plan().measurement_ticket().unwrap()
    );
    assert_eq!(
        engine.workspace().tab_mru(tabs),
        Some([SECOND, ITEM].as_slice())
    );
}

#[test]
fn selecting_an_offscreen_tab_rebuilds_reveal_geometry_and_hit_receivers() {
    use crate::scene::{TabSceneId, TabStripMemberVisibility};

    let selected = ItemId::new(8);
    let (mut engine, tabs) = selection_engine((1..=8).map(ItemId::new));
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, bounds(240.0));
    let original = engine.scene().ready_candidate(SURFACE).unwrap().clone();
    let previous_bar = &original.plan().tab_bar_records()[0];
    assert_eq!(previous_bar.scroll_offset(), 0.0);
    assert_eq!(
        previous_bar
            .members()
            .iter()
            .find(|member| member.tab().item == selected)
            .unwrap()
            .visibility(),
        TabStripMemberVisibility::Hidden,
    );
    let tab = TabSceneId {
        root: ROOT,
        tabs,
        item: selected,
    };
    assert!(
        original
            .plan()
            .tab_records()
            .iter()
            .all(|record| *record.id() != tab)
    );
    assert!(
        original
            .hit_manifest()
            .region_for_kind(PresentationHitRegionKind::TabBody(tab))
            .is_none()
    );
    let expected = engine.version();
    support::submit_input(
        &mut engine,
        &mut host,
        StableInputSourceId::new(3),
        EngineInput::SelectItem {
            expected,
            item: selected,
        },
    )
    .unwrap();
    assert!(engine.presentation_authority.selection_rebind.is_some());
    let answers = support::measurements(
        &engine,
        SURFACE,
        bounds(240.0),
        MeasurementProfile::default(),
    );
    assert!(
        original
            .plan()
            .layout_facts()
            .unwrap()
            .measurements()
            .same_values(&answers)
    );

    reset_surface_compilation_count();
    publish_measurement_answers(&mut engine, &mut host, answers);
    assert_eq!(surface_compilation_count(), 1);
    let current = engine.scene().ready_candidate(SURFACE).unwrap();
    assert!(!current.plan().shares_facts(original.plan()));
    let bar = &current.plan().tab_bar_records()[0];
    assert!(bar.scroll_offset() > previous_bar.scroll_offset());
    assert_eq!(
        bar.members()
            .iter()
            .find(|member| member.tab() == tab)
            .unwrap()
            .visibility(),
        TabStripMemberVisibility::Visible,
    );
    let record = current
        .plan()
        .tab_records()
        .iter()
        .find(|record| *record.id() == tab)
        .unwrap();
    assert!(current.plan().tab_selected(record));
    let receiver = current
        .hit_manifest()
        .region_for_kind(PresentationHitRegionKind::TabBody(tab))
        .unwrap();
    assert_eq!(receiver.hit().rect(), record.drag_hit().rect());
    assert_ne!(
        current.hit_manifest().regions().as_ptr(),
        original.hit_manifest().regions().as_ptr()
    );
    assert_ne!(current.output_ticket(), original.output_ticket());
    assert_eq!(
        engine.workspace().tab_mru(tabs).unwrap().first(),
        Some(&selected)
    );
}

#[test]
fn local_selection_rebuilds_when_another_items_measurements_change() {
    use crate::scene::TabSceneId;

    #[derive(Clone, Copy, Debug)]
    enum ChangedAnswer {
        Intrinsic,
        Minimum,
    }

    const OTHER: ItemId = ItemId::new(3);
    for changed in [ChangedAnswer::Intrinsic, ChangedAnswer::Minimum] {
        let (mut engine, tabs) = selection_engine([ITEM, SECOND, OTHER]);
        let mut host = TestPresentationHost::new(&mut engine);
        support::publish_surface(&mut engine, &mut host, SURFACE, bounds(800.0));
        let original = engine.scene().ready_candidate(SURFACE).unwrap().clone();
        let expected = engine.version();
        support::submit_input(
            &mut engine,
            &mut host,
            StableInputSourceId::new(4),
            EngineInput::SelectItem {
                expected,
                item: SECOND,
            },
        )
        .unwrap();
        assert!(engine.presentation_authority.selection_rebind.is_some());
        let answers = item_measurements(
            &engine,
            bounds(800.0),
            |item| {
                if matches!(changed, ChangedAnswer::Minimum) && item == Some(OTHER) {
                    LogicalSize::new(180.0, 120.0).unwrap()
                } else {
                    LogicalSize::new(0.0, 0.0).unwrap()
                }
            },
            |item| {
                if matches!(changed, ChangedAnswer::Intrinsic) && item == OTHER {
                    220.0
                } else {
                    56.0
                }
            },
        );
        assert!(
            !original
                .plan()
                .layout_facts()
                .unwrap()
                .measurements()
                .same_values(&answers)
        );

        reset_surface_compilation_count();
        publish_measurement_answers(&mut engine, &mut host, answers.clone());
        assert_eq!(surface_compilation_count(), 1, "{changed:?}");
        let current = engine.scene().ready_candidate(SURFACE).unwrap();
        assert!(!current.plan().shares_facts(original.plan()), "{changed:?}");
        assert_eq!(
            current
                .plan()
                .pane_selected(&current.plan().pane_records()[0]),
            Some(SECOND)
        );
        assert_eq!(
            engine.workspace().tab_mru(tabs),
            Some([SECOND, ITEM, OTHER].as_slice())
        );
        let layout = current.plan().layout_facts().unwrap();
        assert!(layout.measurements().same_values(&answers));
        assert_eq!(
            layout.measurements().ticket(),
            current.plan().measurement_ticket().unwrap()
        );
        match changed {
            ChangedAnswer::Intrinsic => {
                let tab = TabSceneId {
                    root: ROOT,
                    tabs,
                    item: OTHER,
                };
                assert_eq!(
                    layout
                        .tab_intrinsic(TabIntrinsicKey::new(SURFACE, tab))
                        .unwrap()
                        .content_width(),
                    220.0
                );
                let previous = original
                    .plan()
                    .tab_records()
                    .iter()
                    .find(|record| *record.id() == tab)
                    .unwrap();
                let record = current
                    .plan()
                    .tab_records()
                    .iter()
                    .find(|record| *record.id() == tab)
                    .unwrap();
                assert!(record.full_bounds().width() > previous.full_bounds().width());
            }
            ChangedAnswer::Minimum => {
                assert_eq!(
                    layout.root(ROOT).unwrap().pane_minimum(PaneMinimumKey::new(
                        ROOT,
                        tabs,
                        Some(OTHER)
                    )),
                    Some(LogicalSize::new(180.0, 120.0).unwrap()),
                );
            }
        }
    }
}

#[test]
fn retained_facts_use_current_drop_policy_without_reauthorizing_old_output() {
    use crate::command::{DockTarget, MovePayload, WorkspaceCommand};
    use crate::drop_guide::{DropGuideScope, DropGuideSlot};
    use crate::drop_resolver::{DropRejectionReason, DropResolution, resolve_drop};
    use crate::graph::Axis;
    use crate::interaction::{DragGeneration, DragSessionId, InteractionRejection};
    use crate::policy::{DockTargetRule, DockTargetRuleKey, PolicyRejection};
    use crate::scene::{TabBarSceneId, TabSceneId};
    use crate::transition::InputOutcome;

    const SOURCE: ItemId = ItemId::new(3);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND]));
    let source_tabs = builder.insert_node(Node::tabs([SOURCE]));
    let split =
        builder.insert_node(Node::equal_split(Axis::Horizontal, [tabs, source_tabs]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut policy = DockPolicy::default();
    let mut denied = DockTargetRule::new();
    denied.set_enabled(false);
    policy.set_target_rule(DockTargetRuleKey::Item(ITEM), denied);
    let mut engine = DockEngine::new(builder.build().unwrap(), policy).unwrap();
    let mut host = TestPresentationHost::new(&mut engine);
    support::publish_surface(&mut engine, &mut host, SURFACE, bounds(800.0));
    let original = engine.scene().ready_candidate(SURFACE).unwrap().clone();
    let bar = TabBarSceneId { root: ROOT, tabs };
    let bar_policy = engine
        .presentation_requirements()
        .surface(SURFACE)
        .unwrap()
        .tab_bar(bar)
        .unwrap()
        .policy();
    let old_scene = original.stamp();
    let old_tab = TabSceneId {
        root: ROOT,
        tabs,
        item: ITEM,
    };
    let center = original
        .plan()
        .drop_guide_clusters()
        .iter()
        .find(|cluster| cluster.id().scope == DropGuideScope::Inner(tabs))
        .unwrap()
        .target(DropGuideSlot::Center)
        .unwrap();
    let hit = center.hit().rect();
    let point = crate::geometry::LogicalPoint::new(
        hit.x() + hit.width() * 0.5,
        hit.y() + hit.height() * 0.5,
    )
    .unwrap();
    let query = |engine: &DockEngine| {
        resolve_drop(
            engine.scene(),
            engine.workspace(),
            engine.policy_snapshot(),
            DragSessionId::new(engine.version().epoch(), DragGeneration::new(1)),
            MovePayload::Item(
                engine
                    .workspace()
                    .capture_item_source(ROOT, source_tabs, SOURCE)
                    .unwrap(),
            ),
            None,
            SURFACE,
            point,
        )
        .unwrap()
    };
    let rejected = query(&engine);
    let DropResolution::Rejected(rejected) = rejected.resolution() else {
        panic!("the initially selected item's actual drop rule must reject its center");
    };
    assert!(matches!(
        rejected.candidates()[0].reason(),
        DropRejectionReason::Policy(PolicyRejection::TargetDisabled { target })
            if *target == DockTargetRuleKey::Item(ITEM)
    ));

    let expected = engine.version();
    support::submit_input(
        &mut engine,
        &mut host,
        StableInputSourceId::new(5),
        EngineInput::SelectItem {
            expected,
            item: SECOND,
        },
    )
    .unwrap();
    assert_eq!(
        engine
            .presentation_requirements()
            .surface(SURFACE)
            .unwrap()
            .tab_bar(bar)
            .unwrap()
            .policy(),
        bar_policy
    );
    let answers = support::measurements(
        &engine,
        SURFACE,
        bounds(800.0),
        MeasurementProfile::default(),
    );
    reset_surface_compilation_count();
    publish_measurement_answers(&mut engine, &mut host, answers);
    assert_eq!(surface_compilation_count(), 0);
    let current = engine.scene().ready_candidate(SURFACE).unwrap();
    assert!(current.plan().shares_facts(original.plan()));
    assert_eq!(original.plan().selected_in_tabs(tabs), Some(ITEM));
    assert_eq!(current.plan().selected_in_tabs(tabs), Some(SECOND));
    assert_ne!(current.output_ticket(), original.output_ticket());
    let resolved = query(&engine);
    let DropResolution::Resolved(resolved) = resolved.resolution() else {
        panic!("the current selected item's drop rule must permit a real preview");
    };
    assert_eq!(resolved.target_id(), center.id());
    assert!(resolved.visual().rect().width() > 0.0);
    assert!(matches!(
        resolved.command(),
        WorkspaceCommand::Move { target: DockTarget::Center(target), .. }
            if target.rule() == DockTargetRuleKey::Item(SECOND)
    ));

    let expected = engine.version();
    let transition = support::submit_input(
        &mut engine,
        &mut host,
        StableInputSourceId::new(5),
        EngineInput::SelectLocalSceneTab {
            expected,
            scene: old_scene,
            selection: LocalTabSelection::Activate(old_tab),
        },
    )
    .unwrap();
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::StaleScene),
            ..
        }
    ));
    assert_eq!(engine.version(), expected);
    assert!(matches!(
        engine.workspace().node(tabs),
        Some(Node::Tabs {
            selected: Some(SECOND),
            ..
        })
    ));
}
