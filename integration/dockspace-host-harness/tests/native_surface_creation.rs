//! Existing logical surfaces acquire native hosting without transferring their topology.

use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceActionRejection, DockspaceAxis, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeWindowPlacement, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::{
    DockItemRule, DockOperation, DockPolicy, DockPresentationMode, DockSourceRule, DockSurfaceRule,
};
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceRuntimeErrorKind, DockspaceSession, DockspaceSubmittedAction,
    HostFrameReport, HostInputOutcome, HostWindowToken, HostWorkAreaToken, NativeCreateReadiness,
    NativeCreateStrategy, NativeDispatchFailure, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeEffectRequest, NativeHostCapabilities, NativeHostCapability,
    NativeHostErrorKind, NativeInputStorageLimit, NativePointerRoster, NativeProducerStorageLimit,
    NativeReceiverAnswer, NativeStagingPresentationPhase, NativeSurfaceBinding, NativeSurfaceRole,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaFacts,
    NativeWorkAreaRoster, PaintedNativeStagingOutput, PaintedSurfaceOutput,
    SurfacePresentationArtifact, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const HOST: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const MISSING: SurfaceId = SurfaceId::new(99);
const HOST_ROOT: RootId = RootId::new(1);
const TARGET_ROOT: RootId = RootId::new(2);
const BACK_ROOT: RootId = RootId::new(3);
const FRONT_ROOT: RootId = RootId::new(4);
const BACK: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT: FloatingPresentationId = FloatingPresentationId::new(2);
const HOST_ITEM: ItemId = ItemId::new(1);
const MAIN_ITEMS: [ItemId; 2] = [ItemId::new(2), ItemId::new(3)];
const SIDE_ITEM: ItemId = ItemId::new(4);
const BACK_ITEMS: [ItemId; 2] = [ItemId::new(5), ItemId::new(6)];
const FRONT_ITEM: ItemId = ItemId::new(7);

struct SurfaceHost {
    session: DockspaceSession,
    root: NativeSurfaceBinding,
    strategy: NativeCreateStrategy,
    version: WorkspaceVersion,
    topology: String,
}

impl SurfaceHost {
    fn new(strategy: NativeCreateStrategy) -> Self {
        Self::configured(strategy, native_policy(), capabilities())
    }

    fn configured(
        strategy: NativeCreateStrategy,
        policy: DockPolicy,
        capabilities: NativeHostCapabilities,
    ) -> Self {
        Self::configured_with_binding_slots(
            strategy,
            policy,
            capabilities,
            NonZeroUsize::new(2).unwrap(),
        )
    }

    fn configured_with_binding_slots(
        strategy: NativeCreateStrategy,
        policy: DockPolicy,
        capabilities: NativeHostCapabilities,
        binding_slots: NonZeroUsize,
    ) -> Self {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                HOST,
                DockspaceRootLayout::new(HOST_ROOT, DockspaceNode::central_tabs([HOST_ITEM])),
            ),
            DockspaceSurfaceLayout::new(
                TARGET,
                DockspaceRootLayout::new(
                    TARGET_ROOT,
                    DockspaceNode::split(
                        DockspaceAxis::Horizontal,
                        [
                            (
                                DockspaceNode::central_tabs_with_selection(
                                    MAIN_ITEMS,
                                    Some(MAIN_ITEMS[1]),
                                )
                                .unwrap(),
                                3.0,
                            ),
                            (DockspaceNode::tabs([SIDE_ITEM]), 1.0),
                        ],
                    )
                    .unwrap(),
                ),
            )
            .with_contained(DockspaceContainedLayout::new(
                BACK,
                DockspaceRootLayout::new(
                    BACK_ROOT,
                    DockspaceNode::tabs_with_selection(BACK_ITEMS, Some(BACK_ITEMS[1])).unwrap(),
                ),
                LogicalRect::new(40.0, 50.0, 300.0, 200.0).unwrap(),
            ))
            .with_contained(DockspaceContainedLayout::new(
                FRONT,
                DockspaceRootLayout::new(FRONT_ROOT, DockspaceNode::tabs([FRONT_ITEM])),
                LogicalRect::new(180.0, 140.0, 280.0, 220.0).unwrap(),
            )),
        ])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        let version = session.version();
        // The public view includes every split, weight, tab, selection and contained bound.
        let topology = format!("{:?}", session.view());
        let producer = NativeProducerStorageLimit::new(binding_slots, NonZeroUsize::MIN, 1 << 20);
        session
            .enable_managed_native_host_with_input_storage_limit(
                strategy,
                NativePointerRoster::Exact(Vec::new()),
                NativeInputStorageLimit::new(NonZeroUsize::new(16).unwrap(), 1 << 20)
                    .with_producer_storage_limit(producer),
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(capabilities)
            .unwrap();
        session
            .register_native_root(HOST, HostWindowToken::new(1))
            .unwrap();
        let registration = idle(&mut session);
        let [root] = registration.native_bindings() else {
            panic!("the real root registration must publish exactly one binding");
        };
        let root = *root;
        assert!(registration.inputs().iter().any(|input| matches!(
            input,
            HostInputOutcome::NativeSurfaceRegistered { binding } if *binding == root
        )));
        session
            .report_managed_native_snapshot([(root, root_facts())], work_areas())
            .unwrap();
        idle(&mut session);
        let output = paint(&mut session, root);
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut session);
        assert!(session.native_local_coordinates(root).is_some());
        let fixture = Self {
            session,
            root,
            strategy,
            version,
            topology,
        };
        fixture.assert_unchanged();
        fixture
    }

    fn assert_unchanged(&self) {
        assert_eq!(self.session.version(), self.version);
        assert_eq!(format!("{:?}", self.session.view()), self.topology);
    }

    fn assert_pending(&self, binding: NativeSurfaceBinding) {
        self.assert_unchanged();
        assert_eq!(binding.surface(), TARGET);
        assert!(self.session.presented_surface(TARGET).is_none());
        assert!(self.session.native_local_coordinates(binding).is_none());
        assert!(self.session.native_local_coordinates(self.root).is_some());
    }

    fn request(&mut self) -> (NativeSurfaceBinding, NativeEffectRequest) {
        let mut frame = native_frame(&mut self.session);
        let submitted = frame
            .create_native_surface_current(TARGET, HOST, NativeWindowPlacement::new(placement()))
            .unwrap();
        let mut report = finish(frame);
        let (binding, create) = take_create(&mut report, submitted, self.strategy);
        assert!(self.session.has_pending_native_create());
        self.assert_pending(binding);
        (binding, create)
    }

    fn observe(&mut self, binding: NativeSurfaceBinding, facts: NativeWindowFacts) {
        self.session
            .report_managed_native_snapshot(
                [(self.root, root_facts()), (binding, facts)],
                work_areas(),
            )
            .unwrap();
        let report = idle(&mut self.session);
        assert!(report.native_admissions().is_empty());
        self.assert_pending(binding);
    }

    fn reject(
        &mut self,
        surface: SurfaceId,
        recovery_host: SurfaceId,
        reason: DockspaceActionRejection,
    ) {
        let mut frame = native_frame(&mut self.session);
        let submitted = frame
            .create_native_surface_current(
                surface,
                recovery_host,
                NativeWindowPlacement::new(placement()),
            )
            .unwrap();
        let mut report = finish(frame);
        assert_eq!(
            report.submitted_action_outcome(submitted),
            Some(&HostInputOutcome::NativeSurfaceCreateRejected { surface, reason }),
            "strategy {:?}, policy {:?}",
            self.strategy,
            self.session.policy(),
        );
        assert!(!report.workspace_changed());
        assert!(report.native_admissions().is_empty());
        assert!(report.take_native_effects().is_empty());
        self.assert_unchanged();
    }

    fn present(&mut self, binding: NativeSurfaceBinding, output: PaintedSurfaceOutput) {
        let artifact = output.presentation_artifact();
        self.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        self.assert_pending(binding);
        let mut report = idle(&mut self.session);
        self.assert_admitted(binding, artifact, &report);
        assert!(report.take_native_effects().is_empty());
    }

    fn assert_admitted(
        &self,
        binding: NativeSurfaceBinding,
        artifact: SurfacePresentationArtifact,
        report: &HostFrameReport,
    ) {
        assert_eq!(report.native_admissions(), &[binding]);
        assert!(!report.workspace_changed());
        assert_eq!(
            (report.before(), report.after()),
            (self.version, self.version)
        );
        self.assert_unchanged();
        assert!(report.native_bindings().contains(&binding));
        assert!(self.session.is_current_native_binding(binding));
        assert!(self.session.native_local_coordinates(binding).is_some());
        assert!(!self.session.has_pending_native_create());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(artifact))
        );
        assert!(
            self.session
                .presented_surface(TARGET)
                .unwrap()
                .matches_presentation_artifact(artifact)
        );
    }
}

fn native_policy() -> DockPolicy {
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    policy
}

fn fine_grained_native_policy_denials() -> Vec<DockPolicy> {
    let mut policies = Vec::new();
    let mut surface_policy = native_policy();
    let mut surface = DockSurfaceRule::new();
    surface
        .set_allowed_presentations([DockPresentationMode::Tiled, DockPresentationMode::Contained]);
    surface_policy.set_surface_rule(TARGET, surface);
    policies.push(surface_policy);

    // Keep every other operation, especially contained recovery, available.
    let non_native_operations = [
        DockOperation::TabMerge,
        DockOperation::EdgeSplit,
        DockOperation::SplitterResize,
        DockOperation::TiledPresentation,
        DockOperation::ContainedFloating,
        DockOperation::ContainedTransform,
    ];
    for root in [TARGET_ROOT, BACK_ROOT, FRONT_ROOT] {
        let mut policy = native_policy();
        let mut source = DockSourceRule::new();
        source.set_allowed_operations(non_native_operations);
        policy.set_source_rule(root, source);
        policies.push(policy);
    }
    // Include unselected tabs and the main root's other split branch.
    for item in [MAIN_ITEMS[0], SIDE_ITEM, BACK_ITEMS[0], FRONT_ITEM] {
        let mut policy = native_policy();
        let mut rule = DockItemRule::new();
        rule.set_allowed_operations(non_native_operations);
        policy.set_item_rule(item, rule);
        policies.push(policy);
    }
    policies
}

fn capabilities() -> NativeHostCapabilities {
    [
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
    })
}

fn placement() -> PhysicalRect {
    PhysicalRect::new(900.0, 120.0, 640.0, 480.0).unwrap()
}

fn window_facts(bounds: PhysicalRect, state: NativeWindowPresentationState) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(state, None)
}

fn child_facts(state: NativeWindowPresentationState) -> NativeWindowFacts {
    window_facts(placement(), state)
}

fn root_facts() -> NativeWindowFacts {
    window_facts(
        PhysicalRect::new(100.0, 80.0, 640.0, 480.0).unwrap(),
        NativeWindowPresentationState::Visible,
    )
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
    let mut report = finish(native_frame(session));
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    assert!(report.take_painted_native_staging_outputs().is_empty());
    report
}

fn take_create(
    report: &mut HostFrameReport,
    submitted: DockspaceSubmittedAction,
    strategy: NativeCreateStrategy,
) -> (NativeSurfaceBinding, NativeEffectRequest) {
    let Some(HostInputOutcome::NativeSurfaceCreateRequested { binding }) =
        report.submitted_action_outcome(submitted)
    else {
        panic!("the exact submitted action must publish its pending native binding");
    };
    let binding = *binding;
    assert_eq!(binding.surface(), TARGET);
    assert!(report.native_bindings().contains(&binding));
    assert!(!report.workspace_changed());
    assert!(report.native_admissions().is_empty());
    let [create] = report.take_native_effects().try_into().unwrap();
    assert!(matches!(
        create.operation(),
        NativeEffectOperation::CreateWindow {
            binding: actual,
            placement: bounds,
            role: NativeSurfaceRole::Child,
            strategy: actual_strategy,
        } if *actual == binding && *bounds == placement() && *actual_strategy == strategy
    ));
    (binding, create)
}

fn measure(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        LogicalSize::new(32.0, 24.0).unwrap(),
        80.0,
    )
    .unwrap();
    let mut frame = native_frame(session);
    for surface in frame.surfaces() {
        frame.measure_surface(surface, metrics).unwrap();
    }
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
}

fn paint(session: &mut DockspaceSession, binding: NativeSurfaceBinding) -> PaintedSurfaceOutput {
    measure(session);
    let mut frame = native_frame(session);
    assert_eq!(frame.native_staging_paints().len(), 0);
    let plan = frame
        .paint_plan(binding.surface())
        .unwrap()
        .expect("the current ready binding must expose a complete semantic program");
    let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    let semantic = recording
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::SurfaceEnd(semantic) => Some(*semantic),
            _ => None,
        })
        .expect("the renderer must accept the complete semantic seal");
    if binding.surface() == TARGET {
        let roots = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::RootBegin(root) => Some(*root),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(roots, [TARGET_ROOT, BACK_ROOT, FRONT_ROOT]);
        let contents = recording
            .records
            .iter()
            .filter_map(|record| match record {
                OwnedRecord::PaneContent {
                    item: Some(item), ..
                } => Some(*item),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            contents,
            [MAIN_ITEMS[1], SIDE_ITEM, BACK_ITEMS[1], FRONT_ITEM]
        );
    }
    frame.confirm_surface_painted(receipt).unwrap();
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(output.matches_native_binding(binding));
    assert!(output.matches_semantic_output(semantic));
    output
}

fn assert_unpaintable(session: &mut DockspaceSession) {
    measure(session);
    let mut frame = native_frame(session);
    let scheduled = frame.surfaces().contains(&TARGET);
    match frame.paint_plan(TARGET) {
        Ok(None) => {}
        Ok(Some(_)) => panic!("revoked native preparation cannot expose a semantic program"),
        // Revocation can leave a retained scene outside the ordinary contribution roster.
        Err(error) => {
            assert!(!scheduled);
            assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
        }
    }
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_outputs().is_empty());
}

fn paint_staging(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    phase: NativeStagingPresentationPhase,
) -> PaintedNativeStagingOutput {
    let mut frame = native_frame(session);
    let requests = frame.native_staging_paints().collect::<Vec<_>>();
    let [request] = requests.as_slice() else {
        panic!("one exact {phase:?} placeholder must be requested");
    };
    assert_eq!(request.binding(), binding);
    assert_eq!(request.phase(), phase);
    assert!(frame.paint_plan(TARGET).unwrap().is_none());
    // Accept the public placeholder callback; no pane program runs during staging.
    frame.confirm_native_staging_painted(*request).unwrap();
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert!(report.take_painted_outputs().is_empty());
    let [output] = report
        .take_painted_native_staging_outputs()
        .try_into()
        .unwrap();
    assert_eq!(output.binding(), binding);
    assert_eq!(output.phase(), phase);
    output
}

fn assert_cancelled(
    fixture: &SurfaceHost,
    binding: NativeSurfaceBinding,
    report: &HostFrameReport,
) {
    let cancellations = report
        .inputs()
        .iter()
        .filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceCreateCancelled { binding, changed } => {
                Some((*binding, *changed))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(cancellations, [(binding, true)]);
    assert!(report.native_admissions().is_empty());
    assert!(!report.workspace_changed());
    assert_eq!(
        (report.before(), report.after()),
        (fixture.version, fixture.version)
    );
    assert!(!report.native_bindings().contains(&binding));
    assert!(report.native_bindings().contains(&fixture.root));
    assert!(!fixture.session.is_current_native_binding(binding));
    assert!(fixture.session.is_current_native_binding(fixture.root));
    fixture.assert_pending(binding);
}

#[test]
fn direct_prepared_first_live_admits_the_existing_surface_without_changing_its_topology() {
    for first in [
        NativeCreateReadiness::Created,
        NativeCreateReadiness::Prepared,
    ] {
        let mut fixture = SurfaceHost::new(NativeCreateStrategy::DirectPrepared);
        let (child, create) = fixture.request();
        let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
            panic!("creation must return its exact readiness acknowledgement");
        };
        fixture.observe(
            child,
            NativeWindowFacts::live().with_create_readiness(first, Some(created)),
        );
        assert_unpaintable(&mut fixture.session);
        for incomplete in [
            child_facts(NativeWindowPresentationState::Hidden)
                .with_create_readiness(NativeCreateReadiness::Prepared, None),
            NativeWindowFacts::live()
                .with_input(NativeWindowInputState::ReceivesInput, None)
                .with_presentation(NativeWindowPresentationState::Visible, None)
                .with_create_readiness(NativeCreateReadiness::Prepared, None),
            child_facts(NativeWindowPresentationState::Visible)
                .with_create_readiness(NativeCreateReadiness::Created, None),
        ] {
            fixture.observe(child, incomplete);
            assert_unpaintable(&mut fixture.session);
            fixture.assert_pending(child);
        }
        fixture.observe(
            child,
            child_facts(NativeWindowPresentationState::Visible)
                .with_create_readiness(NativeCreateReadiness::Prepared, None),
        );
        let output = paint(&mut fixture.session, child);
        fixture.assert_pending(child);
        fixture.present(child, output);
        let repeated = idle(&mut fixture.session);
        assert!(repeated.native_admissions().is_empty());
        fixture.assert_unchanged();
    }
}

#[test]
fn staged_existing_surface_requires_pre_show_show_ack_post_show_and_semantic_first_live() {
    let mut fixture = SurfaceHost::new(NativeCreateStrategy::Staged);
    let (child, create) = fixture.request();
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("creation must return its exact readiness acknowledgement");
    };
    fixture.observe(
        child,
        child_facts(NativeWindowPresentationState::Hidden)
            .with_create_readiness(NativeCreateReadiness::Created, Some(created)),
    );
    let pre_show = paint_staging(
        &mut fixture.session,
        child,
        NativeStagingPresentationPhase::PreShow,
    );
    fixture.assert_pending(child);
    fixture
        .session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .unwrap();
    let mut report = finish(native_frame(&mut fixture.session));
    assert!(report.native_admissions().is_empty());
    fixture.assert_pending(child);
    let [show] = report.take_native_effects().try_into().unwrap();
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child
    ));
    let Some(NativeEffectAcknowledgement::Presentation(shown)) = show.accepted() else {
        panic!("Show must return its exact visibility acknowledgement");
    };
    fixture.observe(
        child,
        child_facts(NativeWindowPresentationState::Hidden)
            .with_create_readiness(NativeCreateReadiness::Created, None)
            .with_presentation(NativeWindowPresentationState::Hidden, Some(shown)),
    );
    assert_unpaintable(&mut fixture.session);
    fixture.observe(
        child,
        child_facts(NativeWindowPresentationState::Visible)
            .with_create_readiness(NativeCreateReadiness::Created, None),
    );
    let post_show = paint_staging(
        &mut fixture.session,
        child,
        NativeStagingPresentationPhase::PostShow,
    );
    fixture
        .session
        .report_native_staging_presentation(post_show, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    fixture.assert_pending(child);
    let output = paint(&mut fixture.session, child);
    fixture.assert_pending(child);
    fixture.present(child, output);
}

#[test]
fn dropped_and_late_first_live_outputs_cannot_restore_revoked_native_readiness() {
    for (explicit_drop, revoked) in [(false, None), (true, Some(NativeCreateReadiness::Created))] {
        let mut fixture = SurfaceHost::new(NativeCreateStrategy::DirectPrepared);
        let (child, create) = fixture.request();
        let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
            panic!("creation must return its exact readiness acknowledgement");
        };
        fixture.observe(
            child,
            child_facts(NativeWindowPresentationState::Visible)
                .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
        );
        let dropped = paint(&mut fixture.session, child);
        let dropped_artifact = dropped.presentation_artifact();
        if explicit_drop {
            fixture
                .session
                .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
                .unwrap();
        } else {
            drop(dropped);
        }
        let report = idle(&mut fixture.session);
        assert!(report.native_admissions().is_empty());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(dropped_artifact))
        );
        fixture.assert_pending(child);

        let late = paint(&mut fixture.session, child);
        let late_artifact = late.presentation_artifact();
        let facts = child_facts(NativeWindowPresentationState::Visible);
        fixture.observe(
            child,
            revoked.map_or(facts, |readiness| {
                facts.with_create_readiness(readiness, None)
            }),
        );
        assert_unpaintable(&mut fixture.session);
        fixture.observe(
            child,
            facts.with_create_readiness(NativeCreateReadiness::Prepared, None),
        );
        fixture
            .session
            .report_surface_presentation(late, SurfacePresentationResult::Presented)
            .unwrap();
        let report = idle(&mut fixture.session);
        assert!(report.native_admissions().is_empty());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(late_artifact))
        );
        fixture.assert_pending(child);
        let current = paint(&mut fixture.session, child);
        assert_ne!(current.presentation_artifact(), dropped_artifact);
        assert_ne!(current.presentation_artifact(), late_artifact);
        fixture.present(child, current);
    }
}

#[test]
fn typed_creation_rejections_and_dispatch_failure_preserve_the_complete_surface() {
    let mut fixture = SurfaceHost::configured_with_binding_slots(
        NativeCreateStrategy::DirectPrepared,
        native_policy(),
        capabilities(),
        NonZeroUsize::new(3).unwrap(),
    );
    fixture.reject(
        TARGET,
        MISSING,
        DockspaceActionRejection::SurfaceUnavailable { surface: MISSING },
    );
    fixture.reject(
        MISSING,
        HOST,
        DockspaceActionRejection::SurfaceUnavailable { surface: MISSING },
    );
    fixture.reject(HOST, HOST, DockspaceActionRejection::Conflict);
    let (child, create) = fixture.request();
    fixture.reject(TARGET, HOST, DockspaceActionRejection::Conflict);
    fixture
        .session
        .report_native_effect_result(
            create.dispatch_failed(NativeDispatchFailure::WindowUnavailable),
        )
        .unwrap();
    // TARGET has never had native source coordinates. Failure cannot invent recovery bounds.
    let mut failed = finish(native_frame(&mut fixture.session));
    assert!(failed.native_admissions().is_empty());
    fixture.assert_pending(child);
    for cleanup in failed.take_native_effects() {
        assert!(matches!(
            cleanup.operation(),
            NativeEffectOperation::CompensatingClose { binding } if *binding == child
        ));
        let Some(NativeEffectAcknowledgement::Close(_)) = cleanup.accepted() else {
            panic!("cleanup must retain the failed binding's exact close acknowledgement");
        };
    }
    assert!(idle(&mut fixture.session).native_admissions().is_empty());
    fixture.assert_pending(child);
    let (retry, create) = fixture.request();
    assert_eq!(retry.surface(), child.surface());
    assert_ne!(retry, child);
    assert!(!fixture.session.is_current_native_binding(child));
    fixture.reject(TARGET, HOST, DockspaceActionRejection::Conflict);
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the failed attempt's successor must have a fresh creation acknowledgement");
    };
    fixture.observe(
        retry,
        child_facts(NativeWindowPresentationState::Visible)
            .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
    );
    let output = paint(&mut fixture.session, retry);
    fixture.present(retry, output);

    let mut unsupported = SurfaceHost::configured(
        NativeCreateStrategy::DirectPrepared,
        native_policy(),
        // Its snapshots still publish an exact work-area roster, even without creation support.
        NativeHostCapabilities::none_supported()
            .with(NativeHostCapability::AuthoritativeInventory)
            .with(NativeHostCapability::WorkArea),
    );
    unsupported.reject(TARGET, HOST, DockspaceActionRejection::NativeUnavailable);
    assert_eq!(
        idle(&mut unsupported.session).native_bindings(),
        &[unsupported.root]
    );
    for policy in [DockPolicy::default(), {
        let mut locked = native_policy();
        locked.set_arrangement_locked(true);
        locked
    }] {
        let mut denied =
            SurfaceHost::configured(NativeCreateStrategy::DirectPrepared, policy, capabilities());
        denied.reject(TARGET, HOST, DockspaceActionRejection::PolicyDenied);
        assert_eq!(idle(&mut denied.session).native_bindings(), &[denied.root]);
    }
}

#[test]
fn initial_native_surface_creation_respects_surface_root_and_item_native_policy() {
    for strategy in [
        NativeCreateStrategy::DirectPrepared,
        NativeCreateStrategy::Staged,
    ] {
        for policy in fine_grained_native_policy_denials() {
            let mut fixture = SurfaceHost::configured(strategy, policy, capabilities());
            fixture.reject(TARGET, HOST, DockspaceActionRejection::PolicyDenied);
            assert!(!fixture.session.has_pending_native_create());
            assert!(fixture.session.presented_surface(TARGET).is_none());
            let mut report = idle(&mut fixture.session);
            assert_eq!(report.native_bindings(), &[fixture.root]);
            assert!(report.native_admissions().is_empty());
            assert!(report.take_native_effects().is_empty());
            fixture.assert_unchanged();
        }
    }
}

#[test]
fn failed_initial_retry_reauthorizes_native_policy_and_preserves_the_frozen_surface() {
    for strategy in [
        NativeCreateStrategy::DirectPrepared,
        NativeCreateStrategy::Staged,
    ] {
        for policy in fine_grained_native_policy_denials() {
            let mut fixture = SurfaceHost::configured_with_binding_slots(
                strategy,
                native_policy(),
                capabilities(),
                NonZeroUsize::new(3).unwrap(),
            );
            let (child, create) = fixture.request();
            fixture
                .session
                .report_native_effect_result(
                    create.dispatch_failed(NativeDispatchFailure::WindowUnavailable),
                )
                .unwrap();
            let mut failed = finish(native_frame(&mut fixture.session));
            assert!(failed.native_admissions().is_empty());
            for cleanup in failed.take_native_effects() {
                assert!(matches!(
                    cleanup.operation(),
                    NativeEffectOperation::CompensatingClose { binding } if *binding == child
                ));
                let Some(NativeEffectAcknowledgement::Close(_)) = cleanup.accepted() else {
                    panic!("cleanup must retain only the failed binding's close acknowledgement");
                };
            }
            assert!(!fixture.session.is_current_native_binding(child));
            assert!(!fixture.session.has_pending_native_create());
            fixture.assert_pending(child);
            assert_unpaintable(&mut fixture.session);

            // Replace policy through the public terminal configuration phase after failure.
            let before = fixture.version;
            let mut frame = native_frame(&mut fixture.session);
            let submitted = frame.replace_policy(policy.clone()).unwrap();
            let mut replaced = finish(frame);
            assert_eq!(
                replaced.submitted_action_outcome(submitted),
                Some(&HostInputOutcome::PolicyReplaced {
                    changed: true,
                    version: replaced.after(),
                })
            );
            assert_eq!(replaced.before(), before);
            assert_ne!(replaced.after(), before);
            assert_eq!(fixture.session.policy(), &policy);
            assert!(replaced.native_admissions().is_empty());
            assert!(replaced.take_native_effects().is_empty());
            fixture.version = replaced.after();
            fixture.assert_pending(child);

            fixture.reject(TARGET, HOST, DockspaceActionRejection::PolicyDenied);
            assert!(!fixture.session.is_current_native_binding(child));
            assert!(!fixture.session.has_pending_native_create());
            fixture.assert_pending(child);
            assert_unpaintable(&mut fixture.session);
            let report = idle(&mut fixture.session);
            assert_eq!(report.native_bindings(), &[fixture.root]);
            assert!(report.native_admissions().is_empty());
            fixture.assert_pending(child);
        }
    }
}

#[test]
fn dropped_and_failed_frames_retry_one_exact_binding_and_deliver_create_only_once() {
    let mut fixture = SurfaceHost::new(NativeCreateStrategy::DirectPrepared);
    let recorded = fixture.session.native_input_recorded_count();
    let backing = fixture
        .session
        .native_input_producer_storage_capacity_bytes();
    let mut frame = native_frame(&mut fixture.session);
    let abandoned = frame
        .create_native_surface_current(TARGET, HOST, NativeWindowPlacement::new(placement()))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    fixture.assert_unchanged();
    let mut frame = native_frame(&mut fixture.session);
    let failed = frame
        .create_native_surface_current(TARGET, HOST, NativeWindowPlacement::new(placement()))
        .unwrap();
    frame
        .commit()
        .expect_err("an incomplete creation frame must roll back");
    fixture.assert_unchanged();
    assert_eq!(fixture.session.native_input_recorded_count(), recorded);
    assert_eq!(
        idle(&mut fixture.session).native_bindings(),
        &[fixture.root]
    );

    let mut frame = native_frame(&mut fixture.session);
    let committed = frame
        .create_native_surface_current(TARGET, HOST, NativeWindowPlacement::new(placement()))
        .unwrap();
    let mut report = finish(frame);
    assert!(report.submitted_action_outcome(abandoned).is_none());
    assert!(report.submitted_action_outcome(failed).is_none());
    let (child, create) = take_create(&mut report, committed, fixture.strategy);
    fixture.assert_pending(child);
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the sole committed create must return its exact readiness acknowledgement");
    };
    fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (
                    child,
                    child_facts(NativeWindowPresentationState::Visible)
                        .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    let recorded = fixture.session.native_input_recorded_count();
    let frame = native_frame(&mut fixture.session);
    drop(frame);
    fixture.assert_pending(child);
    let frame = native_frame(&mut fixture.session);
    frame
        .commit()
        .expect_err("an incomplete readiness frame must roll back");
    fixture.assert_pending(child);
    let ready = idle(&mut fixture.session);
    assert!(ready.native_admissions().is_empty());
    assert_eq!(ready.native_bindings(), &[fixture.root, child]);
    assert_eq!(fixture.session.native_input_recorded_count(), recorded);
    assert_eq!(
        fixture
            .session
            .native_input_producer_storage_capacity_bytes(),
        backing
    );
    fixture.assert_pending(child);

    let output = paint(&mut fixture.session, child);
    let artifact = output.presentation_artifact();
    fixture
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    let mut frame = native_frame(&mut fixture.session);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    drop(frame);
    fixture.assert_pending(child);
    let frame = native_frame(&mut fixture.session);
    frame
        .commit()
        .expect_err("an incomplete admission frame must roll back");
    fixture.assert_pending(child);
    let admitted = idle(&mut fixture.session);
    fixture.assert_admitted(child, artifact, &admitted);
    let repeated = idle(&mut fixture.session);
    assert!(repeated.native_admissions().is_empty());
    assert_eq!(repeated.native_bindings(), &[fixture.root, child]);
    fixture.assert_unchanged();
}

#[test]
fn cancelling_initial_creation_in_its_request_frame_never_dispatches_the_window() {
    for strategy in [
        NativeCreateStrategy::DirectPrepared,
        NativeCreateStrategy::Staged,
    ] {
        let mut fixture = SurfaceHost::new(strategy);
        let recorded = fixture.session.native_input_recorded_count();
        let mut frame = native_frame(&mut fixture.session);
        assert!(!frame.cancel_next_native_create().unwrap());
        let submitted = frame
            .create_native_surface_current(TARGET, HOST, NativeWindowPlacement::new(placement()))
            .unwrap();
        assert!(frame.cancel_next_native_create().unwrap());
        assert!(!frame.cancel_next_native_create().unwrap());
        let mut report = finish(frame);
        let Some(HostInputOutcome::NativeSurfaceCreateRequested { binding }) =
            report.submitted_action_outcome(submitted)
        else {
            panic!("the cancelled request must still report its exact proposed binding");
        };
        let binding = *binding;
        assert_eq!(binding.surface(), TARGET);
        assert_cancelled(&fixture, binding, &report);
        assert!(report.take_native_effects().is_empty());
        assert_eq!(report.native_bindings(), &[fixture.root]);
        assert!(!fixture.session.has_pending_native_create());
        assert_eq!(fixture.session.native_input_recorded_count(), recorded);

        for _ in 0..2 {
            let mut frame = native_frame(&mut fixture.session);
            assert!(!frame.cancel_next_native_create().unwrap());
            let mut report = finish(frame);
            assert!(report.native_admissions().is_empty());
            assert!(report.take_native_effects().is_empty());
            assert_eq!(report.native_bindings(), &[fixture.root]);
            fixture.assert_unchanged();
        }
    }
}

#[test]
fn cancellation_retry_keeps_its_owner_and_rejects_a_late_create_ack_for_the_new_incarnation() {
    let mut fixture = SurfaceHost::configured_with_binding_slots(
        NativeCreateStrategy::DirectPrepared,
        native_policy(),
        capabilities(),
        NonZeroUsize::new(3).unwrap(),
    );
    let (child, create) = fixture.request();
    // Appearance is independent from the still-delayed create acknowledgement.
    fixture.observe(
        child,
        child_facts(NativeWindowPresentationState::Visible)
            .with_create_readiness(NativeCreateReadiness::Created, None),
    );
    let recorded = fixture.session.native_input_recorded_count();
    for complete_before_drop in [true, false] {
        let mut frame = native_frame(&mut fixture.session);
        assert!(frame.cancel_next_native_create().unwrap());
        assert!(!frame.cancel_next_native_create().unwrap());
        if complete_before_drop {
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(frame);
        } else {
            frame
                .commit()
                .expect_err("an incomplete cancellation frame must retain its original owner");
        }
        fixture.assert_pending(child);
        assert!(fixture.session.has_pending_native_create());
        assert!(fixture.session.is_current_native_binding(child));
        assert_eq!(fixture.session.native_input_recorded_count(), recorded);
        assert_eq!(
            idle(&mut fixture.session).native_bindings(),
            &[fixture.root, child]
        );
    }

    let mut frame = native_frame(&mut fixture.session);
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(!frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    assert_cancelled(&fixture, child, &report);
    assert!(!fixture.session.has_pending_native_create());
    let [cleanup] = report.take_native_effects().try_into().unwrap();
    assert!(matches!(
        cleanup.operation(),
        NativeEffectOperation::CompensatingClose { binding } if *binding == child
    ));
    let Some(NativeEffectAcknowledgement::Close(destroyed)) = cleanup.accepted() else {
        panic!("cleanup must retain the cancelled lifetime's exact destruction capability");
    };
    let Some(NativeEffectAcknowledgement::Create(old_created)) = create.accepted() else {
        panic!("the late create must retain its original exact acknowledgement");
    };

    let (next, create) = fixture.request();
    assert_eq!(next.surface(), child.surface());
    assert_ne!(next, child);
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the successor's creation must retain a different exact acknowledgement");
    };
    let recorded = fixture.session.native_input_recorded_count();
    let error = fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (
                    next,
                    child_facts(NativeWindowPresentationState::Visible)
                        .with_create_readiness(NativeCreateReadiness::Prepared, Some(old_created)),
                ),
            ],
            work_areas(),
        )
        .expect_err(
            "the old incarnation's acknowledgement cannot prepare its same-surface successor",
        );
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
    assert_eq!(fixture.session.native_input_recorded_count(), recorded);
    assert!(fixture.session.has_pending_native_create());
    fixture.assert_pending(next);
    assert_unpaintable(&mut fixture.session);

    fixture.observe(
        next,
        child_facts(NativeWindowPresentationState::Visible)
            .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
    );
    let output = paint(&mut fixture.session, next);
    fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (child, NativeWindowFacts::destroyed_after(destroyed)),
                (
                    next,
                    child_facts(NativeWindowPresentationState::Visible)
                        .with_create_readiness(NativeCreateReadiness::Prepared, None),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    let mut report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(report.native_bindings(), &[fixture.root, next]);
    assert!(fixture.session.has_pending_native_create());
    fixture.assert_pending(next);
    fixture
        .session
        .report_native_binding_quiescence(child)
        .unwrap();
    let report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    fixture.assert_pending(next);
    fixture.present(next, output);
    for _ in 0..2 {
        let mut frame = native_frame(&mut fixture.session);
        assert!(!frame.cancel_next_native_create().unwrap());
        let mut report = finish(frame);
        assert!(report.native_admissions().is_empty());
        assert!(report.take_native_effects().is_empty());
        assert_eq!(report.native_bindings(), &[fixture.root, next]);
        assert!(!fixture.session.has_pending_native_create());
        assert!(fixture.session.native_local_coordinates(next).is_some());
        assert!(!fixture.session.is_current_native_binding(child));
        fixture.assert_unchanged();
    }
}

#[test]
fn late_presented_and_destroyed_results_for_a_cancelled_initial_owner_cannot_admit_a_new_binding() {
    let mut fixture = SurfaceHost::configured_with_binding_slots(
        NativeCreateStrategy::DirectPrepared,
        native_policy(),
        capabilities(),
        NonZeroUsize::new(3).unwrap(),
    );
    let (child, create) = fixture.request();
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the first lifetime must retain its exact preparation acknowledgement");
    };
    fixture.observe(
        child,
        child_facts(NativeWindowPresentationState::Visible)
            .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
    );
    let old_output = paint(&mut fixture.session, child);
    let old_artifact = old_output.presentation_artifact();
    let mut frame = native_frame(&mut fixture.session);
    assert!(frame.cancel_next_native_create().unwrap());
    assert!(!frame.cancel_next_native_create().unwrap());
    let mut cancelled = finish(frame);
    assert_cancelled(&fixture, child, &cancelled);
    let [cleanup] = cancelled.take_native_effects().try_into().unwrap();
    assert!(matches!(
        cleanup.operation(),
        NativeEffectOperation::CompensatingClose { binding } if *binding == child
    ));
    let Some(NativeEffectAcknowledgement::Close(destroyed)) = cleanup.accepted() else {
        panic!("the old lifetime's close acknowledgement must remain independently owned");
    };

    let (next, create) = fixture.request();
    assert_eq!(next.surface(), child.surface());
    assert_ne!(next, child);
    assert_ne!(next, fixture.root);
    let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
        panic!("the new initial owner must have its own exact create acknowledgement");
    };
    fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (
                    next,
                    child_facts(NativeWindowPresentationState::Visible)
                        .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    let report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    assert_eq!(report.native_bindings(), &[fixture.root, next]);
    fixture.assert_pending(next);
    let next_output = paint(&mut fixture.session, next);
    let next_artifact = next_output.presentation_artifact();
    assert_ne!(next_artifact, old_artifact);

    fixture
        .session
        .report_surface_presentation(old_output, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(old_artifact))
    );
    assert!(fixture.session.has_pending_native_create());
    assert!(fixture.session.is_current_native_binding(next));
    assert!(fixture.session.presented_surface(TARGET).is_none());
    fixture.assert_pending(next);

    fixture
        .session
        .report_managed_native_snapshot(
            [
                (fixture.root, root_facts()),
                (child, NativeWindowFacts::destroyed_after(destroyed)),
                (
                    next,
                    child_facts(NativeWindowPresentationState::Visible)
                        .with_create_readiness(NativeCreateReadiness::Prepared, None),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    let mut report = idle(&mut fixture.session);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(report.native_bindings(), &[fixture.root, next]);
    assert!(fixture.session.has_pending_native_create());
    fixture.assert_pending(next);

    // Only this binding's own first-live output may settle its surviving owner.
    fixture.present(next, next_output);
    assert!(
        fixture
            .session
            .presented_surface(TARGET)
            .unwrap()
            .matches_presentation_artifact(next_artifact)
    );
    let mut frame = native_frame(&mut fixture.session);
    assert!(!frame.cancel_next_native_create().unwrap());
    let mut report = finish(frame);
    assert!(report.native_admissions().is_empty());
    assert!(report.take_native_effects().is_empty());
    assert_eq!(report.native_bindings(), &[fixture.root, next]);
    assert!(fixture.session.is_current_native_binding(next));
    assert!(fixture.session.native_local_coordinates(next).is_some());
    assert!(!fixture.session.is_current_native_binding(child));
    assert!(!fixture.session.has_pending_native_create());
    fixture.assert_unchanged();
}
