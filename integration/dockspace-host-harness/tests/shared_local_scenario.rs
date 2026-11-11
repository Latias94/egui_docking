//! Retained, headless public-contract consumption of the shared L08 scenario.
//! The recording sink does not qualify a UI framework, GPU, or physical host.

use dockspace::close::ClosePlanTarget;
use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceHostFrame, DockspaceSession, HostCloseRequestOrigin,
    HostFrameReport, HostInputOutcome, PreparedSurfaceAction, SurfaceContainedResizeAdjustment,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationResult, SurfaceSplitterAdjustment, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_conformance::scenario::{
    ItemAlias, LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED, NATIVE_CREATE_REDOCK_RETIREMENT,
    ScenarioAdmission, ScenarioAxis, ScenarioCapability, ScenarioEdge, SemanticAction,
    SemanticObservation, SemanticSelector, SemanticStep,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const CONTAINED: ItemId = ItemId::new(4);

fn item(alias: ItemAlias) -> ItemId {
    match alias {
        ItemAlias::Primary => FIRST,
        ItemAlias::Secondary => SECOND,
        ItemAlias::Contained => CONTAINED,
    }
}

fn alias(item: ItemId) -> ItemAlias {
    match item {
        FIRST => ItemAlias::Primary,
        SECOND => ItemAlias::Secondary,
        CONTAINED => ItemAlias::Contained,
        other => panic!("the shared scenario observed an unnamed fixture item: {other:?}"),
    }
}

fn session() -> DockspaceSession {
    let split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([FIRST, SECOND]),
            DockspaceNode::tabs([THIRD]),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, split),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([CONTAINED])),
        LogicalRect::new(500.0, 300.0, 220.0, 180.0).unwrap(),
    ))])
    .unwrap();
    DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap()
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn paint_and_present(session: &mut DockspaceSession, step: SemanticStep) -> PreparedSurfaceAction {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                72.0,
            )
            .unwrap(),
        )
        .unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut prepared = None;
    plan.visit_surface_program(|step_record| {
        let Step::Operation { operation, .. } = step_record else {
            return Ok::<(), ()>(());
        };
        let action = match (step.selector(), step.action(), operation) {
            (SemanticSelector::Tab(target), SemanticAction::Activate, Operation::Tab(tab))
                if tab.item() == item(target) =>
            {
                plan.prepare_tab_select(tab.item())
            }
            (SemanticSelector::TabClose(target), SemanticAction::Activate, Operation::Tab(tab))
                if tab.item() == item(target) =>
            {
                plan.prepare_tab_close(tab.item())
            }
            (
                SemanticSelector::Splitter(ScenarioAxis::Horizontal),
                SemanticAction::Increment,
                Operation::Splitter(splitter),
            ) if splitter.axis() == DockspaceAxis::Horizontal => plan.prepare_splitter_adjustment(
                splitter.visual_id(),
                SurfaceSplitterAdjustment::Increment,
            ),
            (
                SemanticSelector::ContainedResize {
                    item: ItemAlias::Contained,
                    edge: ScenarioEdge::East,
                },
                SemanticAction::Increment,
                Operation::ContainedControls(contained),
            ) if contained.floating() == FLOATING => plan.prepare_contained_resize_adjustment(
                contained.floating(),
                ContainedResizeDirection::East,
                SurfaceContainedResizeAdjustment::Increment,
            ),
            _ => None,
        };
        if let Some(action) = action {
            assert!(
                prepared.replace(action).is_none(),
                "one exact semantic control"
            );
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let prepared = prepared.expect("the shared step must resolve to an operable painted control");
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = commit(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert_eq!(output.surface(), SURFACE);
    assert!(matches!(
        recording.records.last(),
        Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic)
    ));
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = commit(session.begin_host_frame().unwrap());
    let [acknowledgement] = settled.surface_presentation_commits() else {
        panic!("the exact recorded output must have one presentation acknowledgement");
    };
    assert!(acknowledgement.presents(artifact));
    assert_eq!(prepared.expected_version(), session.version());
    prepared
}

fn weights(session: &DockspaceSession) -> Vec<f32> {
    session
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap()
        .weights()
        .collect()
}

#[test]
fn shared_l08_executes_through_presented_public_prepared_actions() {
    let supported = [
        ScenarioCapability::LocalTabSelection,
        ScenarioCapability::LocalItemClose,
        ScenarioCapability::LocalSplitterAdjustment,
        ScenarioCapability::LocalContainedResize,
    ];
    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.admission(&supported),
        ScenarioAdmission::Ready
    );
    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.admission(&supported),
        ScenarioAdmission::Unsupported {
            missing: ScenarioCapability::NativeWindowLifecycle,
        }
    );
    let mut session = session();
    let mut observations = Vec::new();
    for step in LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.steps() {
        let action = paint_and_present(&mut session, *step);
        let before_selected = session.view().item(SECOND).unwrap().tabs().selected();
        let before_weights = weights(&session);
        let before_rect = session.view().contained(FLOATING).unwrap().rect();
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_surface_action(action).unwrap();
        let report = commit(frame);
        let observed = match (step.selector(), step.action()) {
            (SemanticSelector::Tab(target), SemanticAction::Activate) => {
                assert!(report.workspace_changed(), "{report:?}");
                let selected = session.view().item(item(target)).unwrap().tabs().selected();
                assert_ne!(selected, before_selected);
                assert_eq!(selected, Some(item(target)));
                SemanticObservation::ItemSelected(alias(selected.unwrap()))
            }
            (SemanticSelector::TabClose(target), SemanticAction::Activate) => {
                let [
                    HostInputOutcome::CloseRequested {
                        plan,
                        reused: false,
                        origin: HostCloseRequestOrigin::Interaction,
                    },
                ] = report.inputs()
                else {
                    panic!(
                        "the painted close control must emit one interaction request: {report:?}"
                    );
                };
                assert_eq!(plan.target(), ClosePlanTarget::Item { item: item(target) });
                let [requested] = plan.items() else {
                    panic!("the tab close request must name exactly one item");
                };
                assert_eq!(requested.item(), item(target));
                assert!(session.view().item(requested.item()).is_some());
                assert!(!report.workspace_changed());
                SemanticObservation::CloseRequested(alias(requested.item()))
            }
            (SemanticSelector::Splitter(ScenarioAxis::Horizontal), SemanticAction::Increment) => {
                assert!(report.workspace_changed(), "{report:?}");
                let after_weights = weights(&session);
                assert_eq!(after_weights.len(), before_weights.len());
                assert!(after_weights[0] > before_weights[0]);
                assert!(after_weights[1] < before_weights[1]);
                let split = session
                    .view()
                    .root(ROOT)
                    .unwrap()
                    .content()
                    .unwrap()
                    .split()
                    .unwrap();
                let axis = match split.axis() {
                    DockspaceAxis::Horizontal => ScenarioAxis::Horizontal,
                    DockspaceAxis::Vertical => ScenarioAxis::Vertical,
                };
                SemanticObservation::SplitterAdjusted(axis)
            }
            (
                SemanticSelector::ContainedResize {
                    item: target,
                    edge: ScenarioEdge::East,
                },
                SemanticAction::Increment,
            ) => {
                assert!(report.workspace_changed(), "{report:?}");
                let contained_item = session.view().item(item(target)).unwrap();
                let contained = session
                    .view()
                    .contained(contained_item.contained().unwrap())
                    .unwrap();
                let after = contained.rect();
                assert_eq!(after.min(), before_rect.min());
                assert_eq!(after.height(), before_rect.height());
                assert!(after.width() > before_rect.width());
                SemanticObservation::ContainedResized {
                    item: alias(contained_item.item()),
                    edge: ScenarioEdge::East,
                }
            }
            unsupported => panic!("the shared scenario added an unsupported step: {unsupported:?}"),
        };
        assert!(!session.has_active_gesture());
        observations.push(observed);
    }
    assert_eq!(
        observations,
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.expected_observations()
    );
}
