//! Public native preparation and first-live ownership without staging placeholders.

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSemanticOutput, DockspaceSession, HostFrameReport,
    HostWindowToken, HostWorkAreaToken, NativeCreateEffectAcknowledgement, NativeCreateReadiness,
    NativeCreateStrategy, NativeEffectAcknowledgement, NativeEffectOperation,
    NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaFacts, NativeWorkAreaRoster, PaintedSurfaceOutput, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const HOST: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const MAIN_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

struct PendingDirect {
    session: DockspaceSession,
    root: NativeSurfaceBinding,
    child: NativeSurfaceBinding,
    placement: PhysicalRect,
    acknowledgement: Option<NativeCreateEffectAcknowledgement>,
    version: WorkspaceVersion,
}

impl PendingDirect {
    fn new() -> Self {
        Self::with_strategy(NativeCreateStrategy::DirectPrepared)
    }

    fn with_strategy(strategy: NativeCreateStrategy) -> Self {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(CHILD_ROOT, DockspaceNode::tabs([CHILD_ITEM])),
            contained_bounds(),
        ))])
        .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        session
            .enable_managed_native_host(strategy, NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        let capabilities = [
            NativeHostCapability::NativeWindowLifecycle,
            NativeHostCapability::AuthoritativeInventory,
            NativeHostCapability::GlobalWindowPlacement,
            NativeHostCapability::WorkArea,
            NativeHostCapability::PointerHitTestObservation,
            NativeHostCapability::CloseCancellation,
        ]
        .into_iter()
        .fold(NativeHostCapabilities::none_supported(), |caps, cap| {
            caps.with(cap)
        });
        session
            .configure_managed_native_capabilities(capabilities)
            .unwrap();
        session
            .register_native_root(HOST, HostWindowToken::new(1))
            .unwrap();
        let registration = idle(&mut session);
        let [root] = registration.native_bindings() else {
            panic!("the external host must publish exactly one native binding");
        };
        let root = *root;
        session
            .report_managed_native_snapshot([(root, root_facts())], work_areas())
            .unwrap();
        idle(&mut session);
        let output = paint(&mut session, root);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut session);
        let version = session.version();
        let placement = PhysicalRect::new(900.0, 120.0, 420.0, 320.0).unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .move_root_to_new_window_current(CHILD_ROOT, NativeWindowPlacement::new(placement))
            .unwrap();
        let mut report = finish(frame);
        assert!(report.native_admissions().is_empty());
        let [create] = report.take_native_effects().try_into().unwrap();
        let child = match create.operation() {
            NativeEffectOperation::CreateWindow {
                binding,
                placement: actual,
                strategy: actual_strategy,
                ..
            } if *actual == placement && *actual_strategy == strategy => *binding,
            operation => {
                panic!("expected CreateWindow with the enrolled strategy, got {operation:?}")
            }
        };
        let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = create.accepted() else {
            panic!("creation must return its separate readiness acknowledgement");
        };
        let fixture = Self {
            session,
            root,
            child,
            placement,
            acknowledgement: Some(acknowledgement),
            version,
        };
        fixture.assert_source();
        fixture
    }

    fn observe(&mut self, readiness: Option<NativeCreateReadiness>) -> HostFrameReport {
        let mut facts = window_facts(self.placement);
        if let Some(readiness) = readiness {
            facts = facts.with_create_readiness(readiness, self.acknowledgement.take());
        }
        self.observe_facts(facts)
    }

    fn observe_facts(&mut self, facts: NativeWindowFacts) -> HostFrameReport {
        self.session
            .report_managed_native_snapshot(
                [(self.root, root_facts()), (self.child, facts)],
                work_areas(),
            )
            .unwrap();
        let report = idle(&mut self.session);
        assert!(
            report.native_admissions().is_empty(),
            "readiness alone cannot transfer ownership"
        );
        report
    }

    fn assert_source(&self) {
        assert_eq!(self.session.version(), self.version);
        let view = self.session.view();
        let item = view.item(CHILD_ITEM).unwrap();
        assert_eq!(
            (item.root(), item.surface(), item.contained()),
            (CHILD_ROOT, HOST, Some(FLOATING))
        );
        assert_eq!(view.contained(FLOATING).unwrap().rect(), contained_bounds());
        assert_eq!(view.item(MAIN_ITEM).unwrap().surface(), HOST);
        assert!(view.surface(self.child.surface()).is_none());
        assert!(
            self.session
                .presented_surface(self.child.surface())
                .is_none()
        );
        assert!(self.session.has_pending_native_create());
    }

    fn present(&mut self, output: PaintedSurfaceOutput) -> HostFrameReport {
        self.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        self.assert_source();
        idle(&mut self.session)
    }
}

fn contained_bounds() -> LogicalRect {
    LogicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap()
}

fn window_facts(bounds: PhysicalRect) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
}

fn root_facts() -> NativeWindowFacts {
    // An externally supplied root did not receive a create request.
    window_facts(PhysicalRect::new(100.0, 80.0, 640.0, 480.0).unwrap())
}

fn work_areas() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    )])
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    assert_eq!(
        frame.native_staging_paints().len(),
        0,
        "direct creation cannot request staging"
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.take_painted_native_staging_outputs().is_empty());
    report
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut report = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(
        report.take_native_effects().is_empty(),
        "direct readiness/presentation must not emit ShowWindow"
    );
    assert!(report.take_painted_outputs().is_empty());
    report
}

fn measure(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
}

fn paint(session: &mut DockspaceSession, binding: NativeSurfaceBinding) -> PaintedSurfaceOutput {
    measure(session);
    paint_without_measurement(session, binding).0
}

fn paint_without_measurement(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (PaintedSurfaceOutput, DockspaceSemanticOutput) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = frame
        .paint_plan(binding.surface())
        .unwrap()
        .expect("a current prepared window must expose its complete semantic plan");
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    let semantic = recording
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::SurfaceEnd(output) => Some(*output),
            _ => None,
        })
        .expect("the accepting sink must reach the semantic seal");
    if binding.surface() != HOST {
        assert!(
            recording
                .records
                .contains(&OwnedRecord::RootBegin(CHILD_ROOT))
        );
        assert!(recording.records.iter().any(|record| matches!(
            record,
            OwnedRecord::PaneContent {
                item: Some(CHILD_ITEM),
                ..
            }
        )));
        assert!(!recording.records.iter().any(|record| matches!(
            record,
            OwnedRecord::PaneContent {
                item: Some(MAIN_ITEM),
                ..
            }
        )));
    }
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    assert!(output.matches_semantic_output(semantic));
    (output, semantic)
}

fn assert_admitted(fixture: &PendingDirect, report: &HostFrameReport) {
    assert_eq!(report.native_admissions(), &[fixture.child]);
    assert!(report.workspace_changed());
    assert_ne!(fixture.session.version(), fixture.version);
    assert_eq!(report.before(), fixture.version);
    assert_eq!(report.after(), fixture.session.version());
    let view = fixture.session.view();
    let item = view.item(CHILD_ITEM).unwrap();
    assert_eq!(
        (item.root(), item.surface(), item.contained()),
        (CHILD_ROOT, fixture.child.surface(), None)
    );
    assert_eq!(view.item(MAIN_ITEM).unwrap().surface(), HOST);
    assert!(!fixture.session.has_pending_native_create());
}

#[test]
fn first_created_or_prepared_snapshot_can_ack_create_without_staging_or_show() {
    for first in [
        NativeCreateReadiness::Created,
        NativeCreateReadiness::Prepared,
    ] {
        let mut fixture = PendingDirect::new();
        fixture.observe(Some(first));
        assert!(fixture.acknowledgement.is_none());
        fixture.assert_source();
        if first == NativeCreateReadiness::Created {
            // Consume the acknowledgement once while retaining the creation fact.
            fixture.observe(Some(NativeCreateReadiness::Created));
            fixture.assert_source();
            let mut frame = fixture
                .session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            assert!(frame.paint_plan(fixture.child.surface()).unwrap().is_none());
            drop(frame);
            fixture.observe(Some(NativeCreateReadiness::Prepared));
        }
        fixture.observe(Some(NativeCreateReadiness::Prepared));
        fixture.assert_source();
        let output = paint(&mut fixture.session, fixture.child);
        let artifact = output.presentation_artifact();
        fixture.assert_source();
        let report = fixture.present(output);
        assert_admitted(&fixture, &report);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(artifact))
        );
        assert!(
            fixture
                .session
                .presented_surface(fixture.child.surface())
                .unwrap()
                .matches_presentation_artifact(artifact)
        );
        let committed = fixture.session.version();
        let repeated = idle(&mut fixture.session);
        assert!(!repeated.workspace_changed());
        assert!(repeated.native_admissions().is_empty());
        assert_eq!(fixture.session.version(), committed);
    }
}

#[test]
fn staged_creation_acknowledgement_precedes_visibility_without_bypassing_staging() {
    let mut fixture = PendingDirect::with_strategy(NativeCreateStrategy::Staged);
    let acknowledgement = fixture.acknowledgement.take().unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    let not_yet_visible = NativeWindowFacts::live()
        .with_content_bounds(fixture.placement)
        .with_outer_bounds(fixture.placement)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_create_readiness(NativeCreateReadiness::Prepared, Some(acknowledgement));
    fixture.observe_facts(not_yet_visible);
    fixture.assert_source();
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    fixture.assert_source();
    let mut inspect = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(
        inspect
            .paint_plan(fixture.child.surface())
            .unwrap()
            .is_none()
    );
    drop(inspect);

    fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (
                    fixture.child,
                    window_facts(fixture.placement)
                        .with_presentation(NativeWindowPresentationState::Hidden, None)
                        .with_create_readiness(NativeCreateReadiness::Created, None),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    let mut frame = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let requests = frame.native_staging_paints().collect::<Vec<_>>();
    assert_eq!(
        requests.len(),
        1,
        "the earlier creation fact must survive until hidden staging becomes possible"
    );
    assert_eq!(
        requests[0].phase(),
        dockspace::runtime::NativeStagingPresentationPhase::PreShow
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert!(report.native_admissions().is_empty());
    assert!(
        report.take_native_effects().is_empty(),
        "Show still requires its exact pre-show renderer output"
    );
    fixture.assert_source();
}

#[test]
fn prepared_alone_cannot_start_first_live_without_visible_complete_coordinates() {
    let mut fixture = PendingDirect::new();
    let bounds = fixture.placement;
    let scale = ScaleFactor::new(1.0).unwrap();
    let coordinates = NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None);
    let not_ready = [
        coordinates.with_presentation(NativeWindowPresentationState::Hidden, None),
        coordinates.with_presentation(NativeWindowPresentationState::Minimized, None),
        coordinates,
        window_facts(PhysicalRect::new(bounds.min().x(), bounds.min().y(), 0.0, 320.0).unwrap()),
        window_facts(PhysicalRect::new(bounds.min().x(), bounds.min().y(), 420.0, 0.0).unwrap()),
        NativeWindowFacts::live()
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None),
        NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None),
        NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None),
    ];
    for facts in not_ready {
        let acknowledgement = fixture.acknowledgement.take();
        fixture.observe_facts(
            facts.with_create_readiness(NativeCreateReadiness::Prepared, acknowledgement),
        );
        fixture.assert_source();
        measure(&mut fixture.session);
        let mut frame = fixture
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        assert!(frame.paint_plan(fixture.child.surface()).unwrap().is_none());
        let mut report = finish(frame);
        assert!(report.native_admissions().is_empty());
        assert!(report.take_painted_outputs().is_empty());
        assert!(report.take_native_effects().is_empty());
        fixture.assert_source();
    }
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    fixture.assert_source();
    let output = paint(&mut fixture.session, fixture.child);
    fixture.assert_source();
    let report = fixture.present(output);
    assert_admitted(&fixture, &report);
}

#[test]
fn dropped_outputs_require_a_new_current_complete_semantic_output() {
    for explicit in [false, true] {
        let mut fixture = PendingDirect::new();
        fixture.observe(Some(NativeCreateReadiness::Prepared));
        let output = paint(&mut fixture.session, fixture.child);
        let artifact = output.presentation_artifact();
        if explicit {
            fixture
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Dropped)
                .unwrap();
        } else {
            drop(output);
        }
        fixture.assert_source();
        let report = idle(&mut fixture.session);
        assert!(report.native_admissions().is_empty());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(artifact))
        );
        fixture.assert_source();
        let replacement = paint(&mut fixture.session, fixture.child);
        assert_ne!(replacement.presentation_artifact(), artifact);
        fixture.assert_source();
        let report = fixture.present(replacement);
        assert_admitted(&fixture, &report);
    }
}

#[test]
fn dropping_paint_and_admission_frames_preserves_the_original_owner_until_retry_commits() {
    let mut fixture = PendingDirect::new();
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    measure(&mut fixture.session);
    let mut frame = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let plan = frame.paint_plan(fixture.child.surface()).unwrap().unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    fixture.assert_source();
    assert!(idle(&mut fixture.session).native_admissions().is_empty());
    fixture.assert_source();

    let output = paint(&mut fixture.session, fixture.child);
    let artifact = output.presentation_artifact();
    fixture
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    fixture.assert_source();
    let mut candidate = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert_eq!(
        candidate.view().item(CHILD_ITEM).unwrap().surface(),
        fixture.child.surface()
    );
    candidate
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(candidate);
    fixture.assert_source();
    let report = idle(&mut fixture.session);
    assert_admitted(&fixture, &report);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(idle(&mut fixture.session).native_admissions().is_empty());
}

#[test]
fn a_late_presented_output_cannot_admit_a_cancelled_direct_create() {
    let mut fixture = PendingDirect::new();
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    let output = paint(&mut fixture.session, fixture.child);
    let artifact = output.presentation_artifact();
    let mut cancellation = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(cancellation.cancel_next_native_create().unwrap());
    let mut report = finish(cancellation);
    assert!(report.native_admissions().is_empty());
    let [close] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(close.operation(), NativeEffectOperation::CompensatingClose { binding } if *binding == fixture.child)
    );
    let Some(NativeEffectAcknowledgement::Close(_)) = close.accepted() else {
        panic!("the cancelled window retains its exact destruction acknowledgement");
    };
    let cancelled_version = fixture.session.version();
    fixture
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
    assert_eq!(fixture.session.version(), cancelled_version);
    assert_eq!(
        fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
        HOST
    );
    assert_eq!(
        fixture.session.view().item(CHILD_ITEM).unwrap().contained(),
        Some(FLOATING)
    );
    assert!(
        fixture
            .session
            .presented_surface(fixture.child.surface())
            .is_none()
    );
    assert!(!fixture.session.has_pending_native_create());
    assert!(idle(&mut fixture.session).native_admissions().is_empty());
}

#[test]
fn revoked_preparation_cannot_be_resurrected_by_an_old_presented_output() {
    for revoked in [None, Some(NativeCreateReadiness::Created)] {
        let mut fixture = PendingDirect::new();
        fixture.observe(Some(NativeCreateReadiness::Prepared));
        let old = paint(&mut fixture.session, fixture.child);
        let old_artifact = old.presentation_artifact();
        fixture.observe(revoked);
        fixture.assert_source();
        fixture.observe(Some(NativeCreateReadiness::Prepared));
        fixture.assert_source();
        let report = fixture.present(old);
        assert!(report.native_admissions().is_empty());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(old_artifact))
        );
        fixture.assert_source();
        let current = paint(&mut fixture.session, fixture.child);
        let current_artifact = current.presentation_artifact();
        assert_ne!(current_artifact, old_artifact);
        let report = fixture.present(current);
        assert_admitted(&fixture, &report);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(current_artifact))
        );
        assert!(
            fixture
                .session
                .presented_surface(fixture.child.surface())
                .unwrap()
                .matches_presentation_artifact(current_artifact)
        );
    }
}

fn pending_direct_replacement() -> (PendingDirect, NativeSurfaceBinding) {
    let mut fixture = PendingDirect::new();
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    let output = paint(&mut fixture.session, fixture.child);
    let admitted = fixture.present(output);
    assert_admitted(&fixture, &admitted);
    let predecessor = fixture.child;
    let version = fixture.session.version();
    fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (predecessor, NativeWindowFacts::destroyed()),
            ],
            work_areas(),
        )
        .unwrap();
    let mut report = finish(
        fixture
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let [replacement] = report.take_native_effects().try_into().unwrap();
    let (successor, placement) = match replacement.operation() {
        NativeEffectOperation::RequestReplacement {
            binding,
            placement,
            strategy: NativeCreateStrategy::DirectPrepared,
            ..
        } => (*binding, *placement),
        operation => panic!("expected a direct RequestReplacement, got {operation:?}"),
    };
    assert_ne!(successor, predecessor);
    assert_eq!(successor.surface(), predecessor.surface());
    assert_eq!(fixture.session.version(), version);
    assert_eq!(
        fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
        predecessor.surface()
    );
    let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = replacement.accepted() else {
        panic!("replacement must use the create/readiness acknowledgement");
    };
    fixture.child = successor;
    fixture.placement = placement;
    fixture.acknowledgement = Some(acknowledgement);
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    (fixture, predecessor)
}

#[test]
fn destroyed_child_replacement_keeps_the_enrolled_direct_strategy() {
    let (mut fixture, predecessor) = pending_direct_replacement();
    let successor = fixture.child;
    let version = fixture.session.version();
    let output = paint(&mut fixture.session, successor);
    let artifact = output.presentation_artifact();
    assert!(!output.matches_native_binding(predecessor));
    assert_eq!(fixture.session.version(), version);
    fixture
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut fixture.session);
    assert_eq!(report.native_admissions(), &[successor]);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert_eq!(
        fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
        successor.surface()
    );
    assert!(
        fixture
            .session
            .presented_surface(successor.surface())
            .unwrap()
            .matches_presentation_artifact(artifact)
    );
}

#[test]
fn replacement_readiness_recovery_refreshes_semantic_output_without_measurement() {
    for revoked in [None, Some(NativeCreateReadiness::Created)] {
        let (mut fixture, predecessor) = pending_direct_replacement();
        let successor = fixture.child;
        let version = fixture.session.version();
        // Keep the original host unavailable as a recovery target.
        let mut measurement = fixture
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measurement
            .measure_surface(
                successor.surface(),
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        let mut measured = finish(measurement);
        assert!(measured.native_admissions().is_empty());
        assert!(measured.take_native_effects().is_empty());
        let (old, old_semantic) = paint_without_measurement(&mut fixture.session, successor);
        let old_artifact = old.presentation_artifact();
        assert!(!old.matches_native_binding(predecessor));

        // Only renderer readiness changes; visibility, coordinates and topology stay fixed.
        fixture.observe(revoked);
        fixture.observe(Some(NativeCreateReadiness::Prepared));
        assert_eq!(fixture.session.version(), version);
        assert_eq!(
            fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
            successor.surface()
        );
        assert!(
            fixture
                .session
                .presented_surface(successor.surface())
                .is_none()
        );

        fixture
            .session
            .report_surface_presentation(old, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut fixture.session);
        assert!(report.native_admissions().is_empty());
        assert!(!report.workspace_changed());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(old_artifact))
        );
        assert!(
            fixture
                .session
                .presented_surface(successor.surface())
                .is_none()
        );
        assert_eq!(fixture.session.version(), version);

        // Repaint the retained public plan without supplying new measurements.
        let (current, current_semantic) =
            paint_without_measurement(&mut fixture.session, successor);
        let current_artifact = current.presentation_artifact();
        assert_ne!(
            current_semantic, old_semantic,
            "restored replacement readiness must refresh the semantic ticket, not only its emission"
        );
        assert!(!current.matches_semantic_output(old_semantic));
        assert_ne!(current_artifact, old_artifact);
        assert_eq!(fixture.session.version(), version);
        assert!(
            fixture
                .session
                .presented_surface(successor.surface())
                .is_none()
        );
        fixture
            .session
            .report_surface_presentation(current, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut fixture.session);
        assert_eq!(report.native_admissions(), &[successor]);
        assert!(!report.workspace_changed());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(current_artifact))
        );
        assert!(
            fixture
                .session
                .presented_surface(successor.surface())
                .unwrap()
                .matches_presentation_artifact(current_artifact)
        );
        assert_eq!(fixture.session.version(), version);
        let view = fixture.session.view();
        let item = view.item(CHILD_ITEM).unwrap();
        assert_eq!(
            (item.root(), item.surface(), item.contained()),
            (CHILD_ROOT, successor.surface(), None)
        );
        assert_eq!(view.item(MAIN_ITEM).unwrap().surface(), HOST);

        let repeated = idle(&mut fixture.session);
        assert!(repeated.native_admissions().is_empty());
        assert!(repeated.surface_presentation_commits().is_empty());
        assert!(!repeated.workspace_changed());
        assert_eq!(fixture.session.version(), version);
        assert!(
            fixture
                .session
                .presented_surface(successor.surface())
                .unwrap()
                .matches_presentation_artifact(current_artifact)
        );
    }
}

#[test]
fn failed_renderer_preparation_preserves_source_and_settles_late_output_through_cleanup() {
    for paint_before_failure in [false, true] {
        let mut fixture = PendingDirect::new();
        let delayed = if paint_before_failure {
            fixture.observe(Some(NativeCreateReadiness::Prepared));
            Some(paint(&mut fixture.session, fixture.child))
        } else {
            None
        };
        let acknowledgement = fixture.acknowledgement.take();
        fixture
            .session
            .report_managed_native_snapshot(
                [
                    (fixture.root, root_facts()),
                    (
                        fixture.child,
                        window_facts(fixture.placement)
                            .with_create_readiness(NativeCreateReadiness::Failed, acknowledgement),
                    ),
                ],
                work_areas(),
            )
            .unwrap();
        let mut report = finish(
            fixture
                .session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );
        assert!(report.native_admissions().is_empty());
        assert!(!fixture.session.has_pending_native_create());
        assert_eq!(fixture.session.version(), fixture.version);
        assert_eq!(
            fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
            HOST
        );
        assert_eq!(
            fixture.session.view().contained(FLOATING).unwrap().rect(),
            contained_bounds()
        );
        assert!(
            fixture
                .session
                .view()
                .surface(fixture.child.surface())
                .is_none()
        );
        let [cleanup] = report.take_native_effects().try_into().unwrap();
        assert!(
            matches!(cleanup.operation(), NativeEffectOperation::CompensatingClose { binding } if *binding == fixture.child)
        );
        if let Some(output) = delayed {
            let artifact = output.presentation_artifact();
            fixture
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
            let report = idle(&mut fixture.session);
            assert!(report.native_admissions().is_empty());
            assert!(
                report
                    .surface_presentation_commits()
                    .iter()
                    .any(|commit| commit.retires(artifact))
            );
            assert_eq!(fixture.session.version(), fixture.version);
            assert_eq!(
                fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
                HOST
            );
        }
        let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = cleanup.accepted() else {
            panic!("the exact failed window retains its original cleanup capability");
        };
        fixture
            .session
            .report_managed_native_snapshot(
                [
                    (fixture.root, root_facts()),
                    (
                        fixture.child,
                        NativeWindowFacts::destroyed_after(acknowledgement),
                    ),
                ],
                work_areas(),
            )
            .unwrap();
        let mut report = idle(&mut fixture.session);
        assert!(report.native_admissions().is_empty());
        assert!(report.take_native_effects().is_empty());
        assert_eq!(fixture.session.version(), fixture.version);
        assert_eq!(
            fixture.session.view().item(CHILD_ITEM).unwrap().surface(),
            HOST
        );
    }
}

#[test]
fn foreign_create_acknowledgement_cannot_prepare_a_same_surface_in_another_session() {
    let mut current = PendingDirect::new();
    let mut foreign = PendingDirect::new();
    assert_eq!(current.child.surface(), foreign.child.surface());
    let acknowledgement = foreign.acknowledgement.take().unwrap();
    assert!(
        current
            .session
            .report_managed_native_snapshot(
                [
                    (current.root, root_facts()),
                    (
                        current.child,
                        window_facts(current.placement).with_create_readiness(
                            NativeCreateReadiness::Prepared,
                            Some(acknowledgement)
                        )
                    ),
                ],
                work_areas()
            )
            .is_err()
    );
    current.assert_source();
    foreign.assert_source();
    current.observe(Some(NativeCreateReadiness::Prepared));
    let output = paint(&mut current.session, current.child);
    let report = current.present(output);
    assert_admitted(&current, &report);
}

#[test]
fn unrelated_root_commit_requires_fresh_first_live_output_without_overwriting_it() {
    let mut fixture = PendingDirect::new();
    fixture.observe(Some(NativeCreateReadiness::Prepared));
    let obsolete = paint(&mut fixture.session, fixture.child);
    let old_artifact = obsolete.presentation_artifact();
    let unrelated = ItemId::new(3);
    let mut frame = fixture
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .open_item_current(
            unrelated,
            DockPlacement::Center(DockAnchor::Item(MAIN_ITEM)),
        )
        .unwrap();
    let report = finish(frame);
    assert!(report.workspace_changed());
    assert_ne!(fixture.session.version(), fixture.version);
    assert_eq!(
        fixture.session.view().item(unrelated).unwrap().root(),
        MAIN_ROOT
    );
    fixture.version = fixture.session.version();
    fixture.assert_source();
    fixture
        .session
        .report_surface_presentation(obsolete, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    assert!(
        !report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(old_artifact))
    );
    fixture.assert_source();
    let current = paint(&mut fixture.session, fixture.child);
    let report = fixture.present(current);
    assert_admitted(&fixture, &report);
    assert_eq!(
        fixture.session.view().item(unrelated).unwrap().root(),
        MAIN_ROOT
    );
    assert!(
        fixture
            .session
            .view()
            .item(unrelated)
            .unwrap()
            .is_selected()
    );
}
