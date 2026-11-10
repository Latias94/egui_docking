//! Programmatic native payloads share source reservation and exact first-live admission.

use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceAxis,
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, NativeTearOffSource,
    NativeWindowPlacement, PreparedDockAction, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePresentationCommandKind, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostWindowToken, HostWorkAreaToken, NativeCreateEffectAcknowledgement,
    NativeCreateReadiness, NativeDispatchFailure, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeEffectRequest, NativeHostCapabilities, NativeHostCapability,
    NativeInputStorageLimit, NativePointerRoster, NativeProducerStorageLimit, NativeReceiverAnswer,
    NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaFacts, NativeWorkAreaRoster, PaintedSurfaceOutput, SurfaceMeasurementAnswer,
    SurfaceMeasurementRequest, SurfacePresentationResult, SurfaceUnavailableReason,
    TabStripMetrics, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const HOST: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const CONTAINED_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const SIBLING: ItemId = ItemId::new(3);
const CONTAINED: ItemId = ItemId::new(4);

struct Fixture {
    session: DockspaceSession,
    root: NativeSurfaceBinding,
}

struct Request {
    binding: NativeSurfaceBinding,
    root: RootId,
    placement: PhysicalRect,
    effect: Option<NativeEffectRequest>,
}

impl Request {
    fn accept(&mut self) -> NativeCreateEffectAcknowledgement {
        let Some(NativeEffectAcknowledgement::Create(acknowledgement)) =
            self.effect.take().unwrap().accepted()
        else {
            panic!("native creation must retain its readiness acknowledgement");
        };
        acknowledgement
    }
}

impl Fixture {
    fn new() -> Self {
        Self::configured(2, false)
    }

    fn configured(binding_slots: usize, offscreen: bool) -> Self {
        let contained =
            LogicalRect::new(if offscreen { 2_000.0 } else { 40.0 }, 50.0, 300.0, 220.0).unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(
                ROOT,
                DockspaceNode::split(
                    DockspaceAxis::Horizontal,
                    [
                        (
                            DockspaceNode::central_tabs_with_selection([A, B], Some(B)).unwrap(),
                            3.0,
                        ),
                        (DockspaceNode::tabs([SIBLING]), 1.0),
                    ],
                )
                .unwrap(),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([CONTAINED])),
            contained,
        ))])
        .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        let producer = NativeProducerStorageLimit::new(
            NonZeroUsize::new(binding_slots).unwrap(),
            NonZeroUsize::MIN,
            1 << 20,
        );
        session
            .enable_managed_native_host_with_input_storage_limit(
                NativePointerRoster::Exact(Vec::new()),
                NativeInputStorageLimit::new(NonZeroUsize::new(16).unwrap(), 1 << 20)
                    .with_producer_storage_limit(producer),
            )
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
        let report = idle(&mut session);
        let [root] = report.native_bindings() else {
            panic!("the root registration must publish one binding");
        };
        let root = *root;
        session
            .report_managed_native_snapshot([(root, root_facts())], work_areas())
            .unwrap();
        idle(&mut session);
        let output = paint(&mut session, root, root_bounds());
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut session);
        Self { session, root }
    }

    fn request(&mut self, action: PreparedDockAction) -> Request {
        let before = self.session.version();
        let locations = [A, B, SIBLING, CONTAINED].map(|item| {
            let location = self.session.view().item(item).unwrap();
            (location.root(), location.surface(), location.contained())
        });
        let mut frame = native_frame(&mut self.session);
        let submitted = frame.submit_prepared_action(action).unwrap();
        let mut report = finish(frame);
        let Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::NativeRootTearOffRequested {
                root,
                source_surface: HOST,
                target_surface,
                ..
            },
        )) = report.submitted_action_outcome(submitted)
        else {
            panic!(
                "native tear-off must start without transferring content: {:?}",
                report.inputs()
            );
        };
        let root = *root;
        let target_surface = *target_surface;
        let [effect] = report.take_native_effects().try_into().unwrap();
        let (binding, placement) = match effect.operation() {
            NativeEffectOperation::CreateWindow {
                binding, placement, ..
            } => (*binding, *placement),
            operation => panic!("expected native creation, got {operation:?}"),
        };
        assert_eq!(binding.surface(), target_surface);
        assert_eq!(self.session.version(), before);
        for (item, expected) in [A, B, SIBLING, CONTAINED].into_iter().zip(locations) {
            let location = self.session.view().item(item).unwrap();
            assert_eq!(
                (location.root(), location.surface(), location.contained()),
                expected
            );
        }
        assert!(self.session.view().surface(target_surface).is_none());
        Request {
            binding,
            root,
            placement,
            effect: Some(effect),
        }
    }

    fn assert_source(&self, before: WorkspaceVersion) {
        assert_eq!(self.session.version(), before);
        let view = self.session.view();
        for item in [A, B, SIBLING] {
            let location = view.item(item).unwrap();
            assert_eq!(
                (location.root(), location.surface(), location.contained()),
                (ROOT, HOST, None)
            );
        }
        assert_eq!(view.item(A).unwrap().tabs().items(), &[A, B]);
        assert_eq!(view.item(A).unwrap().tabs().selected(), Some(B));
        let contained = view.item(CONTAINED).unwrap();
        assert_eq!(
            (contained.root(), contained.surface(), contained.contained()),
            (CONTAINED_ROOT, HOST, Some(FLOATING))
        );
    }

    fn observe(
        &mut self,
        request: &Request,
        readiness: NativeCreateReadiness,
        acknowledgement: Option<NativeCreateEffectAcknowledgement>,
    ) {
        let state = if readiness == NativeCreateReadiness::Created {
            NativeWindowPresentationState::Hidden
        } else {
            NativeWindowPresentationState::Visible
        };
        let facts = window_facts(request.placement)
            .with_presentation(state)
            .with_create_readiness(readiness, acknowledgement);
        self.session
            .report_managed_native_snapshot(
                [(self.root, root_facts()), (request.binding, facts)],
                work_areas(),
            )
            .unwrap();
        assert!(idle(&mut self.session).native_admissions().is_empty());
    }
}

fn root_bounds() -> PhysicalRect {
    PhysicalRect::new(100.0, 80.0, 640.0, 480.0).unwrap()
}

fn placement() -> NativeWindowPlacement {
    NativeWindowPlacement::new(PhysicalRect::new(900.0, 120.0, 420.0, 320.0).unwrap())
}

fn window_facts(bounds: PhysicalRect) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_outer_bounds(bounds)
        .with_content_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn root_facts() -> NativeWindowFacts {
    window_facts(root_bounds())
}

fn work_areas() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    )])
}

fn native_frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(native_frame(session))
}

fn paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    bounds: PhysicalRect,
) -> PaintedSurfaceOutput {
    let mut frame = native_frame(session);
    frame
        .measure_surface(
            binding.surface(),
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, bounds.width(), bounds.height()).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    finish(frame);
    execute_paint(session, binding).0
}

fn execute_paint(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
) -> (PaintedSurfaceOutput, Vec<OwnedRecord>) {
    let mut frame = native_frame(session);
    let plan = frame.paint_plan(binding.surface()).unwrap().unwrap();
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    (output, recording.records)
}

#[test]
fn item_group_and_root_transfer_only_after_exact_first_live() {
    for kind in 0..3 {
        let mut fixture = Fixture::new();
        let source = match kind {
            0 => NativeTearOffSource::Item(B),
            1 => NativeTearOffSource::Tabs(
                fixture
                    .session
                    .capture_tabs_group(DockAnchor::Item(A))
                    .unwrap(),
            ),
            _ => NativeTearOffSource::Root(ROOT),
        };
        let command = fixture
            .session
            .native_window_command(source, placement())
            .unwrap();
        assert!(command.is_ready(), "{:?}", command.unavailable_reason());
        let mut request = fixture.request(command.into_prepared().unwrap());
        if kind == 2 {
            assert_eq!(request.root, ROOT);
        } else {
            assert_ne!(request.root, ROOT);
        }
        let before = fixture.session.version();
        let mut frame = native_frame(&mut fixture.session);
        frame
            .dock_item_current(B, DockPlacement::Center(DockAnchor::Item(SIBLING)))
            .unwrap();
        let mut reserved = finish(frame);
        assert!(
            reserved
                .inputs()
                .contains(&HostInputOutcome::ProductActionRejected(
                    DockspaceActionRejection::Conflict,
                ))
        );
        assert!(reserved.take_native_effects().is_empty());
        fixture.assert_source(before);
        let ack = request.accept();
        fixture.observe(&request, NativeCreateReadiness::Created, Some(ack));
        fixture.assert_source(before);
        fixture.observe(&request, NativeCreateReadiness::Prepared, None);
        let output = paint(&mut fixture.session, request.binding, request.placement);
        fixture.assert_source(before);
        fixture
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut fixture.session);
        assert_eq!(report.native_admissions(), &[request.binding]);
        assert!(!fixture.session.has_pending_native_create());
        let view = fixture.session.view();
        let moved: &[ItemId] = match kind {
            0 => &[B],
            1 => &[A, B],
            _ => &[A, B, SIBLING],
        };
        for &item in moved {
            let location = view.item(item).unwrap();
            assert_eq!(
                (location.root(), location.surface(), location.contained()),
                (request.root, request.binding.surface(), None)
            );
        }
        if kind != 2 {
            assert_eq!(view.item(SIBLING).unwrap().surface(), HOST);
            let source_root = view.root(ROOT).unwrap();
            assert_eq!(source_root.id(), ROOT);
            let central = source_root.central().unwrap().tabs().unwrap();
            assert!(central.is_central());
            assert_eq!(central.items(), if kind == 0 { &[A][..] } else { &[][..] });
            let split = source_root.content().unwrap().split().unwrap();
            assert_eq!(split.child_count(), 2);
            assert_eq!(split.weights().collect::<Vec<_>>(), vec![0.75, 0.25]);
        }
        if kind == 0 {
            assert_eq!(view.item(A).unwrap().tabs().items(), &[A]);
        } else {
            let tabs = view.item(A).unwrap().tabs();
            assert_eq!(tabs.items(), &[A, B]);
            assert_eq!(tabs.selected(), Some(B));
        }
        assert_eq!(view.item(CONTAINED).unwrap().contained(), Some(FLOATING));
    }
}

#[test]
fn captured_group_never_retargets_its_anchor_or_refreshes_its_version() {
    let mut fixture = Fixture::new();
    let group = fixture
        .session
        .capture_tabs_group(DockAnchor::Item(A))
        .unwrap();
    let captured_version = group.expected_version();
    let mut frame = native_frame(&mut fixture.session);
    frame
        .dock_item_current(A, DockPlacement::Center(DockAnchor::Item(SIBLING)))
        .unwrap();
    finish(frame);
    let before = fixture.session.version();
    assert_ne!(before, captured_version);
    assert_eq!(
        fixture.session.view().item(A).unwrap().tabs().items(),
        &[SIBLING, A]
    );
    let source = NativeTearOffSource::Tabs(group);
    let command = fixture
        .session
        .native_window_command(source.clone(), placement())
        .unwrap();
    assert_eq!(
        command.unavailable_reason(),
        Some(DockspaceActionRejection::Conflict)
    );
    let action = fixture
        .session
        .prepare_move_to_new_window(source.clone(), placement());
    assert_eq!(action.expected_version(), captured_version);
    let mut frame = native_frame(&mut fixture.session);
    let submitted = frame.submit_prepared_action(action).unwrap();
    let mut report = finish(frame);
    assert!(matches!(
        report.submitted_action_outcome(submitted),
        Some(HostInputOutcome::StaleRejected { .. })
    ));
    assert!(report.take_native_effects().is_empty());
    let mut frame = native_frame(&mut fixture.session);
    frame
        .move_to_new_window_current(source, placement())
        .unwrap();
    let mut report = finish(frame);
    assert!(
        report
            .inputs()
            .contains(&HostInputOutcome::ProductActionRejected(
                DockspaceActionRejection::Conflict
            ))
    );
    assert!(report.take_native_effects().is_empty());
    assert_eq!(fixture.session.version(), before);
    assert_eq!(fixture.session.view().item(B).unwrap().tabs().items(), &[B]);
    assert_eq!(
        fixture.session.view().item(A).unwrap().tabs().items(),
        &[SIBLING, A]
    );
}

#[test]
fn foreign_group_preserves_its_issuing_session_authority() {
    let source = Fixture::new();
    let group = source
        .session
        .capture_tabs_group(DockAnchor::Item(A))
        .unwrap();
    let mut destination = Fixture::new();
    let before = destination.session.version();
    let action = destination
        .session
        .prepare_move_to_new_window(NativeTearOffSource::Tabs(group), placement());
    let mut frame = native_frame(&mut destination.session);
    assert!(frame.submit_prepared_action(action).is_err());
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    destination.assert_source(before);
}

#[test]
fn cancellation_and_late_output_preserve_the_partial_source() {
    let mut fixture = Fixture::new();
    let action = fixture
        .session
        .prepare_move_to_new_window(NativeTearOffSource::Item(B), placement());
    let mut request = fixture.request(action);
    let ack = request.accept();
    fixture.observe(&request, NativeCreateReadiness::Prepared, Some(ack));
    let output = paint(&mut fixture.session, request.binding, request.placement);
    let before = fixture.session.version();
    let mut frame = native_frame(&mut fixture.session);
    assert!(frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    let [close] = report.take_native_effects().try_into().unwrap();
    assert!(
        matches!(close.operation(), NativeEffectOperation::CompensatingClose { binding } if *binding == request.binding)
    );
    assert!(matches!(
        close.accepted(),
        Some(NativeEffectAcknowledgement::Close(_))
    ));
    fixture
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(idle(&mut fixture.session).native_admissions().is_empty());
    fixture.assert_source(before);
    assert!(!fixture.session.has_pending_native_create());
}

#[test]
fn failed_partial_creation_and_discarded_attempt_keep_original_content() {
    let mut fixture = Fixture::new();
    let before = fixture.session.version();
    let mut frame = native_frame(&mut fixture.session);
    frame
        .move_to_new_window_default_current(NativeTearOffSource::Item(B))
        .unwrap();
    drop(frame);
    fixture.assert_source(before);
    assert!(!fixture.session.has_pending_native_create());
    let group = fixture
        .session
        .capture_tabs_group(DockAnchor::Item(A))
        .unwrap();
    let action = fixture
        .session
        .prepare_move_to_new_window(NativeTearOffSource::Tabs(group), placement());
    let request = fixture.request(action);
    fixture
        .session
        .report_native_effect_result(
            request
                .effect
                .unwrap()
                .dispatch_failed(NativeDispatchFailure::WindowUnavailable),
        )
        .unwrap();
    let mut report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    for cleanup in report.take_native_effects() {
        assert!(
            matches!(cleanup.operation(), NativeEffectOperation::CompensatingClose { binding } if *binding == request.binding)
        );
        assert!(matches!(
            cleanup.accepted(),
            Some(NativeEffectAcknowledgement::Close(_))
        ));
    }
    fixture.assert_source(before);
    assert!(!fixture.session.has_pending_native_create());
}

#[test]
fn default_session_prepared_and_paint_commands_share_placement() {
    let expected = PhysicalRect::new(124.0, 104.0, 640.0, 480.0).unwrap();
    for path in 0..3 {
        let mut fixture = Fixture::new();
        let mut report = if path < 2 {
            let action = if path == 0 {
                fixture
                    .session
                    .native_window_command_default(NativeTearOffSource::Root(ROOT))
                    .unwrap()
                    .into_prepared()
                    .unwrap()
            } else {
                fixture
                    .session
                    .prepare_move_to_new_window_default(NativeTearOffSource::Root(ROOT))
            };
            let mut request = fixture.request(action);
            assert_eq!(request.placement, expected);
            let _ = request.accept();
            continue;
        } else {
            let mut frame = native_frame(&mut fixture.session);
            let plan = frame.paint_plan(HOST).unwrap().unwrap();
            let command = plan
                .presentation_command(ROOT, DockspacePresentationCommandKind::MoveToNewWindow)
                .unwrap()
                .unwrap();
            assert!(command.is_ready());
            let action = plan.prepare_presentation_command(command).unwrap();
            finish(frame);
            let mut frame = native_frame(&mut fixture.session);
            frame.submit_surface_action(action).unwrap();
            finish(frame)
        };
        let [create] = report.take_native_effects().try_into().unwrap();
        assert!(
            matches!(create.operation(), NativeEffectOperation::CreateWindow { placement, .. } if *placement == expected)
        );
        assert!(matches!(
            create.accepted(),
            Some(NativeEffectAcknowledgement::Create(_))
        ));
    }
}

#[test]
fn missing_default_facts_and_finite_capacity_never_move_the_source() {
    let mut fixture = Fixture::new();
    let before = fixture.session.version();
    fixture
        .session
        .report_managed_native_snapshot(
            [(fixture.root, root_facts())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut fixture.session);
    let source = NativeTearOffSource::Item(B);
    assert_eq!(
        fixture
            .session
            .native_window_command_default(source.clone())
            .unwrap()
            .unavailable_reason(),
        Some(DockspaceActionRejection::NativeUnavailable)
    );
    assert!(
        fixture
            .session
            .native_window_command(source.clone(), placement())
            .unwrap()
            .is_ready()
    );
    let mut frame = native_frame(&mut fixture.session);
    frame.move_to_new_window_default_current(source).unwrap();
    let mut report = finish(frame);
    assert!(
        report
            .inputs()
            .contains(&HostInputOutcome::ProductActionRejected(
                DockspaceActionRejection::NativeUnavailable
            ))
    );
    assert!(report.take_native_effects().is_empty());
    fixture.assert_source(before);

    let mut bounded = Fixture::configured(1, false);
    let before = bounded.session.version();
    assert_eq!(
        bounded
            .session
            .native_window_command(NativeTearOffSource::Item(B), placement())
            .unwrap()
            .unavailable_reason(),
        Some(DockspaceActionRejection::NativeUnavailable)
    );
    let mut frame = native_frame(&mut bounded.session);
    frame
        .move_to_new_window_current(NativeTearOffSource::Item(B), placement())
        .unwrap();
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    bounded.assert_source(before);
}

#[test]
fn every_programmatic_payload_obeys_the_native_tear_off_policy() {
    let mut fixture = Fixture::new();
    let mut policy = fixture.session.policy().clone();
    policy.set_allow_native_surfaces(false);
    let mut frame = native_frame(&mut fixture.session);
    frame.replace_policy(policy).unwrap();
    finish(frame);
    let before = fixture.session.version();
    let group = fixture
        .session
        .capture_tabs_group(DockAnchor::Item(A))
        .unwrap();
    for source in [
        NativeTearOffSource::Item(B),
        NativeTearOffSource::Tabs(group),
        NativeTearOffSource::Root(ROOT),
    ] {
        assert_eq!(
            fixture
                .session
                .native_window_command(source.clone(), placement())
                .unwrap()
                .unavailable_reason(),
            Some(DockspaceActionRejection::PolicyDenied)
        );
        let action = fixture
            .session
            .prepare_move_to_new_window(source, placement());
        let mut frame = native_frame(&mut fixture.session);
        let submitted = frame.submit_prepared_action(action).unwrap();
        let mut report = finish(frame);
        assert_eq!(
            report.submitted_action_outcome(submitted),
            Some(&HostInputOutcome::ProductActionRejected(
                DockspaceActionRejection::PolicyDenied
            ))
        );
        assert!(report.take_native_effects().is_empty());
        fixture.assert_source(before);
    }
}

#[test]
fn explicit_placement_can_recover_a_contained_root_without_visible_chrome() {
    let mut fixture = Fixture::configured(2, true);
    let source = NativeTearOffSource::Root(CONTAINED_ROOT);
    assert!(
        !fixture
            .session
            .native_window_command_default(source.clone())
            .unwrap()
            .is_ready()
    );
    let command = fixture
        .session
        .native_window_command(source, placement())
        .unwrap();
    assert!(command.is_ready(), "{:?}", command.unavailable_reason());
    let mut request = fixture.request(command.into_prepared().unwrap());
    assert_eq!(request.root, CONTAINED_ROOT);
    let _ = request.accept();
}

fn paint_distinct_minima(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    bounds: PhysicalRect,
) -> (PaintedSurfaceOutput, Vec<OwnedRecord>) {
    let mut frame = native_frame(session);
    frame
        .measure_surface_with(binding.surface(), |request| match request {
            SurfaceMeasurementRequest::DockBounds { .. }
            | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                SurfaceMeasurementAnswer::Bounds(
                    LogicalRect::new(0.0, 0.0, bounds.width(), bounds.height()).unwrap(),
                )
            }
            SurfaceMeasurementRequest::PaneMinimum { item, .. } => {
                let (width, height) = match item {
                    Some(A) => (700.0, 260.0),
                    Some(SIBLING) => (420.0, 300.0),
                    _ => (40.0, 30.0),
                };
                SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(width, height).unwrap())
            }
            SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                SurfaceMeasurementAnswer::TabIntrinsic(80.0)
            }
            SurfaceMeasurementRequest::TabStrip { .. } => {
                SurfaceMeasurementAnswer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
            }
        })
        .unwrap();
    finish(frame);
    execute_paint(session, binding)
}

fn selected_pane_geometry(records: &[OwnedRecord], item: ItemId) -> (LogicalRect, LogicalRect) {
    let (visual, content) = records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::PaneContent {
                visual,
                item: Some(selected),
                bounds,
                ..
            } if *selected == item => Some((*visual, *bounds)),
            _ => None,
        })
        .unwrap();
    let bounds = records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::Geometry {
                visual: actual,
                bounds,
                ..
            } if *actual == visual => Some(*bounds),
            _ => None,
        })
        .unwrap();
    (bounds, content)
}

fn assert_partial_default_geometry(source_contained: bool) {
    for group in [false, true] {
        let mut fixture = Fixture::new();
        if source_contained {
            let action = fixture.session.prepare_float_root(ROOT, HOST);
            let mut frame = native_frame(&mut fixture.session);
            frame.submit_prepared_action(action).unwrap();
            assert!(finish(frame).workspace_changed());
            assert!(
                fixture
                    .session
                    .view()
                    .item(A)
                    .unwrap()
                    .contained()
                    .is_some()
            );
        }
        let (output, records) =
            paint_distinct_minima(&mut fixture.session, fixture.root, root_bounds());
        let (pane, content) = selected_pane_geometry(&records, B);
        assert!(
            pane.width() < 700.0,
            "the inactive item must exceed its currently selected sibling's pane"
        );
        fixture
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut fixture.session);
        let config = fixture.session.presentation_config();
        let border = 2.0 * config.floating_border_width();
        let measured = if group {
            (40.0_f64, 30.0_f64)
        } else {
            (700.0_f64, 260.0_f64)
        };
        let minimum_width = (measured.0.max(config.minimum_pane_size().width()) + border)
            .max(config.minimum_floating_size().width());
        let minimum_height = (measured.1.max(config.minimum_pane_size().height()) + content.y()
            - pane.y()
            + config.floating_title_height()
            + border)
            .max(config.minimum_floating_size().height());
        let expected = PhysicalRect::new(
            root_bounds().x() + pane.x() + 24.0,
            root_bounds().y() + pane.y() + 24.0,
            pane.width().max(minimum_width),
            pane.height().max(minimum_height),
        )
        .unwrap();
        let source = if group {
            NativeTearOffSource::Tabs(
                fixture
                    .session
                    .capture_tabs_group(DockAnchor::Item(A))
                    .unwrap(),
            )
        } else {
            NativeTearOffSource::Item(A)
        };
        let command = fixture
            .session
            .native_window_command_default(source)
            .unwrap();
        assert!(command.is_ready(), "{:?}", command.unavailable_reason());
        let mut request = fixture.request(command.into_prepared().unwrap());
        assert_eq!(
            request.placement, expected,
            "contained={source_contained}, group={group}"
        );
        let before = fixture.session.version();
        let ack = request.accept();
        fixture.observe(&request, NativeCreateReadiness::Created, Some(ack));
        assert_eq!(fixture.session.version(), before);
        fixture.observe(&request, NativeCreateReadiness::Prepared, None);
        let (output, _) =
            paint_distinct_minima(&mut fixture.session, request.binding, request.placement);
        assert_eq!(fixture.session.version(), before);
        assert_eq!(fixture.session.view().item(A).unwrap().surface(), HOST);
        fixture
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        assert_eq!(
            idle(&mut fixture.session).native_admissions(),
            &[request.binding]
        );
        let view = fixture.session.view();
        let moved = view.item(A).unwrap();
        assert_eq!(
            (moved.root(), moved.surface()),
            (request.root, request.binding.surface())
        );
        assert_eq!(
            moved.tabs().items(),
            if group { &[A, B][..] } else { &[A][..] }
        );
        assert_eq!(moved.tabs().selected(), Some(if group { B } else { A }));
        assert_eq!(view.item(SIBLING).unwrap().surface(), HOST);
        assert_eq!(view.item(CONTAINED).unwrap().contained(), Some(FLOATING));
    }
}

#[test]
fn main_split_partial_defaults_use_exact_pane_and_payload_minimum() {
    assert_partial_default_geometry(false);
}

#[test]
fn contained_split_partial_defaults_do_not_inherit_window_or_sibling_minimum() {
    assert_partial_default_geometry(true);
}
