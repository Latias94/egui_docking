use super::*;
use crate::behavior_tests::support::{self, MeasurementProfile, TestPresentationHost};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::graph::{RootRecord, SurfacePresentation};
use crate::model::{DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspaceSemanticOutput, DockspaceSession, PaintedSurfaceOutput, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use crate::scene_compiler::{reset_surface_compilation_count, surface_compilation_count};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);

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
