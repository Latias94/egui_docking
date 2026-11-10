//! One real window owns one close journey for its complete exact surface roster.

use dockspace::close::{CloseDecision, DeferredCloseDecision, DeferredCloseToken};
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseInertReason, DockspaceClosePhase, DockspaceClosePlan, DockspaceCloseResolution,
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken,
    NativeCloseDisposition, NativeCloseEffectAcknowledgement, NativeCloseState,
    NativeCreateReadiness, NativeDispatchFailure, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeHostCapabilities, NativeHostCapability, NativeHostErrorKind,
    NativePointerRoster, NativeReceiverAnswer, NativeSurfaceBinding, NativeSurfaceCloseAction,
    NativeSurfacePlacement, NativeWindowBinding, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const CHILD: SurfaceId = SurfaceId::new(3);
const ITEMS: [ItemId; 3] = [ItemId::new(1), ItemId::new(2), ItemId::new(3)];

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame
        .commit()
        .expect("the complete native boundary must publish")
}

fn native_frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(native_frame(session))
}

fn placements() -> [NativeSurfacePlacement; 2] {
    [A, B].map(|surface| {
        let x = if surface == A { 0.0 } else { 400.0 };
        NativeSurfacePlacement::new(
            surface,
            LogicalSize::new(400.0, 600.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(x, 0.0, 400.0, 600.0).unwrap(),
        )
    })
}

fn facts(bounds: PhysicalRect) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn root_facts() -> NativeWindowFacts {
    facts(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
}

fn child_bounds() -> PhysicalRect {
    PhysicalRect::new(900.0, 0.0, 300.0, 220.0).unwrap()
}

fn child_facts() -> NativeWindowFacts {
    facts(child_bounds()).with_create_readiness(NativeCreateReadiness::Prepared, None)
}

fn paint(session: &mut DockspaceSession, surfaces: &[(SurfaceId, LogicalSize)]) -> HostFrameReport {
    let mut frame = native_frame(session);
    for &(surface, size) in surfaces {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, size.width(), size.height()).unwrap(),
                    LogicalSize::new(24.0, 20.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        let plan = frame
            .paint_plan(surface)
            .unwrap()
            .expect("an exact live placement must be paintable");
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
    }
    let mut report = finish(frame);
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), surfaces.len());
    session
        .report_surface_presentations(outputs, SurfacePresentationResult::Presented)
        .unwrap();
    idle(session)
}

struct Harness {
    session: DockspaceSession,
    window: NativeWindowBinding,
    bindings: Vec<NativeSurfaceBinding>,
}

impl Harness {
    fn new() -> Self {
        let layout =
            DockspaceLayout::new([A, B, CHILD].into_iter().zip(ITEMS).map(|(surface, item)| {
                DockspaceSurfaceLayout::new(
                    surface,
                    DockspaceRootLayout::new(
                        RootId::new(surface.get()),
                        DockspaceNode::central_tabs([item]),
                    ),
                )
            }))
            .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        policy.set_close_capability(CloseCapability::DeferredAllowed);
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        let capabilities = [
            NativeHostCapability::NativeWindowLifecycle,
            NativeHostCapability::AuthoritativeInventory,
            NativeHostCapability::GlobalWindowPlacement,
            NativeHostCapability::PointerHitTestObservation,
            NativeHostCapability::PointerHitTestControl,
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
            .register_native_root(A, HostWindowToken::new(7))
            .unwrap();
        let registration = idle(&mut session);
        let [original] = registration.native_bindings() else {
            panic!("one real root must register")
        };
        let window = original.window();
        session
            .report_managed_native_snapshot([(window, root_facts())], NativeWorkAreaRoster::Unknown)
            .unwrap();
        idle(&mut session);
        let mut frame = native_frame(&mut session);
        frame
            .set_native_surface_placements(window, &placements())
            .unwrap();
        let bindings = frame
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>();
        assert_eq!(
            bindings
                .iter()
                .map(|binding| binding.surface())
                .collect::<Vec<_>>(),
            [A, B]
        );
        finish(frame);
        let ready = paint(
            &mut session,
            &[(A, placements()[0].size()), (B, placements()[1].size())],
        );
        assert!(!ready.workspace_changed());
        Self {
            session,
            window,
            bindings,
        }
    }

    fn request(&mut self, action: NativeSurfaceCloseAction) -> DockspaceClosePlan {
        // The real callback names the physical window, not an arbitrary placement.
        self.session
            .report_managed_native_snapshot(
                [(
                    self.window,
                    root_facts().with_close(NativeCloseState::Requested, None),
                )],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let observed = idle(&mut self.session);
        let requests = observed
            .inputs()
            .iter()
            .flat_map(|input| match input {
                HostInputOutcome::NativePlatformSnapshotApplied { close_requests }
                | HostInputOutcome::NativeCloseObservationApplied { close_requests } => {
                    close_requests.as_slice()
                }
                _ => &[],
            })
            .copied()
            .collect::<Vec<_>>();
        let [close] = requests.as_slice() else {
            panic!("one physical request must produce one edge: {observed:?}")
        };
        assert_eq!(close.binding().window(), self.window);
        self.session
            .request_native_surface_close(*close, action)
            .unwrap();
        let mut requested = idle(&mut self.session);
        let [HostInputOutcome::NativeSurfaceCloseRequested { plan, .. }] = requested.inputs()
        else {
            panic!("the entire window must produce one approval plan: {requested:?}")
        };
        let plan = plan.clone();
        assert_eq!(
            plan.items()
                .iter()
                .map(|item| item.item())
                .collect::<Vec<_>>(),
            ITEMS[..2]
        );
        assert_eq!(plan.phase(), DockspaceClosePhase::AwaitingDecision);
        assert!(!requested.workspace_changed());
        assert!(requested.take_native_effects().is_empty());
        plan
    }

    fn decide(&mut self, plan: &DockspaceClosePlan, last: CloseDecision) -> HostFrameReport {
        let mut frame = native_frame(&mut self.session);
        for (index, item) in plan.items().iter().enumerate() {
            frame
                .resolve_close(
                    plan.request(),
                    item.token(),
                    if index + 1 == plan.items().len() {
                        last
                    } else {
                        CloseDecision::Allow
                    },
                )
                .unwrap();
        }
        finish(frame)
    }

    fn take_resolution(
        &self,
        report: &mut HostFrameReport,
        expected: NativeCloseDisposition,
    ) -> NativeCloseEffectAcknowledgement {
        assert!(
            !report.workspace_changed(),
            "approval only authorizes the later physical result"
        );
        let [effect] = report.take_native_effects().try_into().unwrap();
        assert!(
            matches!(effect.operation(), NativeEffectOperation::ResolveNativeClose { close, resolution }
            if close.binding().window() == self.window && *resolution == expected)
        );
        let Some(NativeEffectAcknowledgement::Close(ack)) = effect.accepted() else {
            panic!("one exact close result must settle the physical obligation")
        };
        ack
    }

    fn destroy(&mut self, ack: NativeCloseEffectAcknowledgement) -> HostFrameReport {
        self.session
            .report_managed_native_snapshot(
                [(self.window, NativeWindowFacts::destroyed_after(ack))],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let report = idle(&mut self.session);
        assert!(
            self.session
                .native_surface_bindings_for_window(self.window)
                .next()
                .is_none()
        );
        assert!(self.session.native_window_bindings().next().is_none());
        for &binding in &self.bindings {
            assert!(!self.session.is_current_native_binding(binding));
        }
        assert!(!report.native_input_pending());
        report
    }

    fn assert_retired(&mut self, plan: &DockspaceClosePlan) {
        let mut quiet = idle(&mut self.session);
        assert!(quiet.take_native_effects().is_empty());
        let mut frame = native_frame(&mut self.session);
        assert_eq!(frame.pending_close_decisions(plan.request()).count(), 0);
        frame
            .resolve_close(
                plan.request(),
                plan.items()[0].token(),
                CloseDecision::Allow,
            )
            .unwrap();
        let mut replay = finish(frame);
        assert!(matches!(
            replay.inputs(),
            [HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(
                    DockspaceCloseInertReason::RetiredRequest
                ),
                changed: false,
                ..
            }]
        ));
        assert!(replay.take_native_effects().is_empty());
        assert!(!replay.workspace_changed());
    }

    fn create_child(&mut self) -> NativeSurfaceBinding {
        let mut frame = native_frame(&mut self.session);
        let submitted = frame
            .create_native_surface_current(CHILD, B, NativeWindowPlacement::new(child_bounds()))
            .unwrap();
        let mut requested = finish(frame);
        let Some(HostInputOutcome::NativeSurfaceCreateRequested { binding }) =
            requested.submitted_action_outcome(submitted)
        else {
            panic!("the logical child must acquire its own native lifetime")
        };
        let child = *binding;
        let [effect] = requested.take_native_effects().try_into().unwrap();
        let Some(NativeEffectAcknowledgement::Create(created)) = effect.accepted() else {
            panic!("the child requires exact creation causality")
        };
        self.session
            .report_managed_native_snapshot(
                [
                    (self.window, root_facts()),
                    (
                        child.window(),
                        child_facts()
                            .with_create_readiness(NativeCreateReadiness::Prepared, Some(created)),
                    ),
                ],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let prepared = idle(&mut self.session);
        assert!(prepared.native_admissions().is_empty());
        let admitted = paint(
            &mut self.session,
            &[(CHILD, LogicalSize::new(300.0, 220.0).unwrap())],
        );
        assert_eq!(admitted.native_admissions(), [child]);
        assert!(!admitted.workspace_changed());
        child
    }
}

fn deferred(report: &HostFrameReport) -> DeferredCloseToken {
    report
        .inputs()
        .iter()
        .find_map(|input| match input {
            HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Deferred { continuation, .. },
                ..
            } => Some(*continuation),
            _ => None,
        })
        .expect("the last sibling must own one deferred continuation")
}

fn assert_destroyed_window_quiescence_and_token_reuse(host: &mut Harness, reverse: bool) {
    let before = format!("{:?}", host.session.view());
    let version = host.session.version();
    let order = if reverse {
        [host.bindings[1], host.bindings[0]]
    } else {
        [host.bindings[0], host.bindings[1]]
    };
    assert!(host.session.native_window_bindings().next().is_none());
    for (index, binding) in order.into_iter().enumerate() {
        assert!(host.session.recognizes_native_binding(binding));
        host.session
            .report_native_binding_quiescence(binding)
            .expect("each destroyed placement retains its own provider-bound guard");
        let mut drained = idle(&mut host.session);
        assert!(!drained.workspace_changed());
        assert!(drained.take_native_effects().is_empty());
        // Reclaim the committed ingress now: recording alone does not validate its guard.
        host.session
            .prepare_native_pointer_capacity()
            .expect("the exact quiescence must compact without a missing terminal guard");
        assert!(!host.session.recognizes_native_binding(binding));
        if index == 0 {
            assert!(host.session.recognizes_native_binding(order[1]));
        }
    }
    assert!(!host.session.has_pending_native_input());
    host.session
        .register_native_root(A, HostWindowToken::new(7))
        .expect("all retired placements have released the physical token");
    let registration = idle(&mut host.session);
    let [successor] = registration.native_bindings() else {
        panic!("one fresh native lifetime must reuse the released host token");
    };
    let successor = *successor;
    assert_ne!(successor.window(), host.window);
    assert!(host.session.is_current_native_binding(successor));
    let recorded = host.session.native_input_recorded_count();
    assert_eq!(
        host.session
            .report_managed_native_snapshot(
                [
                    (successor.window(), root_facts()),
                    (host.window, NativeWindowFacts::destroyed()),
                ],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap_err()
            .native_kind(),
        Some(NativeHostErrorKind::StaleBinding)
    );
    assert_eq!(host.session.native_input_recorded_count(), recorded);
    assert!(host.session.is_current_native_binding(successor));
    host.session
        .report_managed_native_snapshot(
            [(successor.window(), root_facts())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut host.session);
    paint(
        &mut host.session,
        &[(A, LogicalSize::new(800.0, 600.0).unwrap())],
    );
    assert!(host.session.presented_surface(A).is_some());
    assert_eq!(host.session.version(), version);
    assert_eq!(format!("{:?}", host.session.view()), before);
}

#[test]
fn shared_window_allow_closes_both_surfaces_once_and_candidate_drop_preserves_approval() {
    let mut host = Harness::new();
    let before = format!("{:?}", host.session.view());
    let plan = host.request(NativeSurfaceCloseAction::CloseContent);
    {
        let mut frame = native_frame(&mut host.session);
        for item in plan.items() {
            frame
                .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
                .unwrap();
        }
    }
    assert_eq!(format!("{:?}", host.session.view()), before);
    let mut approved = host.decide(&plan, CloseDecision::Allow);
    let ack = host.take_resolution(&mut approved, NativeCloseDisposition::Accept);
    assert_eq!(format!("{:?}", host.session.view()), before);
    let destroyed = host.destroy(ack);
    assert!(destroyed.workspace_changed());
    for surface in [A, B] {
        assert!(host.session.view().surface(surface).is_none());
    }
    for item in &ITEMS[..2] {
        assert!(host.session.view().item(*item).is_none());
    }
    assert!(host.session.view().item(ITEMS[2]).is_some());
    host.assert_retired(&plan);
}

#[test]
fn shared_window_failed_close_dispatch_preserves_both_surfaces_and_allows_a_fresh_request() {
    let mut host = Harness::new();
    let before = format!("{:?}", host.session.view());
    let version = host.session.version();
    let first = host.request(NativeSurfaceCloseAction::CloseContent);
    let mut approved = host.decide(&first, CloseDecision::Allow);
    let [effect] = approved.take_native_effects().try_into().unwrap();
    assert!(matches!(
        effect.operation(),
        NativeEffectOperation::ResolveNativeClose { close, resolution: NativeCloseDisposition::Accept }
            if close.binding().window() == host.window
                && !host.bindings.contains(&close.binding())
    ));
    host.session
        .report_native_effect_result(
            effect.dispatch_failed(NativeDispatchFailure::WindowUnavailable),
        )
        .expect("the effect receipt still belongs to the original physical callback lane");
    let mut failed = idle(&mut host.session);
    assert!(!failed.workspace_changed());
    assert!(failed.take_native_effects().is_empty());
    assert_eq!(host.session.version(), version);
    assert_eq!(format!("{:?}", host.session.view()), before);
    assert_eq!(
        host.session
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    paint(
        &mut host.session,
        &[(A, placements()[0].size()), (B, placements()[1].size())],
    );
    assert!(host.session.presented_surface(A).is_some());
    assert!(host.session.presented_surface(B).is_some());

    // A failed dispatch does not grant implicit retry of the frozen approval.
    host.session
        .report_managed_native_snapshot(
            [(
                host.window,
                root_facts().with_close(NativeCloseState::Clear, None),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut cleared = idle(&mut host.session);
    assert!(cleared.take_native_effects().is_empty());
    let retry = host.request(NativeSurfaceCloseAction::CloseContent);
    assert_ne!(retry.request(), first.request());
    let mut approved = host.decide(&retry, CloseDecision::Allow);
    let ack = host.take_resolution(&mut approved, NativeCloseDisposition::Accept);
    assert!(host.destroy(ack).workspace_changed());
    assert!(host.session.view().surface(A).is_none());
    assert!(host.session.view().surface(B).is_none());
    host.assert_retired(&retry);
}

#[test]
fn shared_window_deferred_sibling_blocks_every_surface_and_duplicate_callback_is_inert() {
    let mut host = Harness::new();
    let before = format!("{:?}", host.session.view());
    let plan = host.request(NativeSurfaceCloseAction::CloseContent);
    let mut waiting = host.decide(&plan, CloseDecision::Deferred);
    let continuation = deferred(&waiting);
    assert!(waiting.take_native_effects().is_empty());
    assert_eq!(format!("{:?}", host.session.view()), before);
    let mut frame = native_frame(&mut host.session);
    frame
        .continue_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let mut approved = finish(frame);
    let ack = host.take_resolution(&mut approved, NativeCloseDisposition::Accept);
    let mut frame = native_frame(&mut host.session);
    frame
        .continue_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let mut duplicate = finish(frame);
    assert!(matches!(
        duplicate.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            changed: false,
            ..
        }]
    ));
    assert!(duplicate.take_native_effects().is_empty());
    assert_eq!(format!("{:?}", host.session.view()), before);
    assert!(host.destroy(ack).workspace_changed());
    host.assert_retired(&plan);
}

#[test]
fn shared_window_one_veto_cancels_the_physical_request_and_keeps_both_rosters() {
    let mut host = Harness::new();
    let version = host.session.version();
    let before = format!("{:?}", host.session.view());
    let plan = host.request(NativeSurfaceCloseAction::CloseContent);
    let mut vetoed = host.decide(&plan, CloseDecision::Veto);
    let ack = host.take_resolution(&mut vetoed, NativeCloseDisposition::Cancel);
    host.session
        .report_managed_native_snapshot(
            [(
                host.window,
                root_facts().with_close(NativeCloseState::Clear, Some(ack)),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let settled = idle(&mut host.session);
    assert!(!settled.workspace_changed());
    assert_eq!(host.session.version(), version);
    assert_eq!(format!("{:?}", host.session.view()), before);
    assert_eq!(
        host.session
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    host.assert_retired(&plan);
}

#[test]
fn shared_window_identical_roster_preserves_pending_approval_and_cancel_awaiting_clear() {
    let mut host = Harness::new();
    let plan = host.request(NativeSurfaceCloseAction::CloseContent);
    let version = host.session.version();
    let before = format!("{:?}", host.session.view());
    let presented = [A, B].map(|surface| host.session.presented_surface(surface));
    let mut frame = native_frame(&mut host.session);
    assert_eq!(
        frame
            .pending_close_decisions(plan.request())
            .collect::<Vec<_>>()
            .as_slice(),
        plan.items()
    );
    frame
        .set_native_surface_placements(host.window, &placements())
        .expect("an unchanged full roster must not conflict with its existing close scope");
    assert_eq!(
        frame
            .pending_close_decisions(plan.request())
            .collect::<Vec<_>>()
            .as_slice(),
        plan.items()
    );
    assert_eq!(
        frame
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    let mut unchanged = finish(frame);
    assert!(!unchanged.workspace_changed());
    assert!(unchanged.take_native_effects().is_empty());
    assert_eq!(host.session.version(), version);
    assert_eq!(
        [A, B].map(|surface| host.session.presented_surface(surface)),
        presented
    );

    // The original decision tokens must still authorize this exact physical request.
    let mut vetoed = host.decide(&plan, CloseDecision::Veto);
    let ack = host.take_resolution(&mut vetoed, NativeCloseDisposition::Cancel);
    let presented = [A, B].map(|surface| host.session.presented_surface(surface));
    let mut frame = native_frame(&mut host.session);
    assert_eq!(frame.pending_close_decisions(plan.request()).count(), 0);
    frame
        .set_native_surface_placements(host.window, &placements())
        .expect("waiting for the exact clear callback must permit an unchanged roster");
    assert_eq!(
        frame
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    let mut waiting = finish(frame);
    assert!(!waiting.workspace_changed());
    assert!(
        waiting.take_native_effects().is_empty(),
        "the no-op must not redispatch cancellation"
    );
    assert_eq!(host.session.version(), version);
    assert_eq!(
        [A, B].map(|surface| host.session.presented_surface(surface)),
        presented
    );
    assert_eq!(format!("{:?}", host.session.view()), before);

    host.session
        .report_managed_native_snapshot(
            [(
                host.window,
                root_facts().with_close(NativeCloseState::Clear, Some(ack)),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut cleared = idle(&mut host.session);
    assert!(!cleared.workspace_changed());
    assert!(cleared.take_native_effects().is_empty());
    assert_eq!(host.session.version(), version);
    assert_eq!(format!("{:?}", host.session.view()), before);
    assert_eq!(
        host.session
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    host.assert_retired(&plan);
}

#[test]
fn shared_window_pending_close_rejects_roster_mutation_without_staling_its_original_votes() {
    let mut host = Harness::new();
    let plan = host.request(NativeSurfaceCloseAction::CloseContent);
    let mut waiting = host.decide(&plan, CloseDecision::Deferred);
    let continuation = deferred(&waiting);
    assert!(waiting.take_native_effects().is_empty());
    {
        let mut frame = native_frame(&mut host.session);
        assert!(
            frame
                .set_native_surface_placements(host.window, &placements()[..1])
                .is_err()
        );
    }
    assert_eq!(
        host.session
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    assert!(host.session.view().surface(A).is_some() && host.session.view().surface(B).is_some());
    let mut frame = native_frame(&mut host.session);
    frame
        .continue_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let mut approved = finish(frame);
    let ack = host.take_resolution(&mut approved, NativeCloseDisposition::Accept);
    assert!(host.destroy(ack).workspace_changed());
    host.assert_retired(&plan);
}

#[test]
fn shared_window_retention_and_late_destroy_callback_cannot_remove_a_successor_window() {
    let mut host = Harness::new();
    let before = format!("{:?}", host.session.view());
    let version = host.session.version();
    let plan = host.request(NativeSurfaceCloseAction::RetainLayout);
    let mut approved = host.decide(&plan, CloseDecision::Allow);
    let ack = host.take_resolution(&mut approved, NativeCloseDisposition::Accept);
    assert!(!host.destroy(ack).workspace_changed());
    assert_eq!(format!("{:?}", host.session.view()), before);
    assert_eq!(host.session.version(), version);
    host.assert_retired(&plan);
    host.session
        .register_native_root(A, HostWindowToken::new(7))
        .unwrap();
    let registration = idle(&mut host.session);
    let [successor] = registration.native_bindings() else {
        panic!("the replacement physical lifetime must be current")
    };
    let successor = *successor;
    assert_ne!(successor.window(), host.window);
    let pending = host.session.native_input_recorded_count();
    match host.session.report_managed_native_snapshot(
        [
            (successor.window(), root_facts()),
            (host.window, NativeWindowFacts::destroyed_after(ack)),
        ],
        NativeWorkAreaRoster::Unknown,
    ) {
        Ok(()) => {
            let mut drained = idle(&mut host.session);
            assert!(!drained.workspace_changed());
            assert!(drained.take_native_effects().is_empty());
        }
        Err(error) => {
            assert_eq!(error.native_kind(), Some(NativeHostErrorKind::StaleBinding));
            assert_eq!(host.session.native_input_recorded_count(), pending);
        }
    }
    assert!(host.session.is_current_native_binding(successor));
    assert_eq!(format!("{:?}", host.session.view()), before);
}

#[test]
fn shared_window_planned_destruction_quiesces_each_placement_before_token_reuse() {
    for reverse in [false, true] {
        let mut host = Harness::new();
        let before = format!("{:?}", host.session.view());
        let version = host.session.version();
        let plan = host.request(NativeSurfaceCloseAction::RetainLayout);
        let mut approved = host.decide(&plan, CloseDecision::Allow);
        let ack = host.take_resolution(&mut approved, NativeCloseDisposition::Accept);
        let mut destroyed = host.destroy(ack);
        assert!(!destroyed.workspace_changed());
        assert!(destroyed.take_native_effects().is_empty());
        assert_eq!(host.session.version(), version);
        assert_eq!(format!("{:?}", host.session.view()), before);
        host.assert_retired(&plan);
        assert_destroyed_window_quiescence_and_token_reuse(&mut host, reverse);
    }
}

#[test]
fn shared_window_unplanned_destruction_quiesces_each_placement_before_token_reuse() {
    for reverse in [false, true] {
        let mut host = Harness::new();
        let before = format!("{:?}", host.session.view());
        let version = host.session.version();
        host.session
            .report_managed_native_snapshot(
                [(host.window, NativeWindowFacts::destroyed())],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let mut destroyed = idle(&mut host.session);
        assert!(!destroyed.workspace_changed());
        assert!(destroyed.take_native_effects().is_empty());
        assert_eq!(host.session.version(), version);
        assert_eq!(format!("{:?}", host.session.view()), before);
        assert_destroyed_window_quiescence_and_token_reuse(&mut host, reverse);
    }
}

#[test]
fn shared_window_child_recovery_uses_its_own_b_anchor_and_keeps_a_hosted() {
    let mut host = Harness::new();
    let child = host.create_child();
    host.session
        .report_managed_native_snapshot(
            [
                (host.window, root_facts()),
                (child.window(), NativeWindowFacts::destroyed()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let recovered = idle(&mut host.session);
    assert!(recovered.workspace_changed());
    assert!(host.session.view().surface(CHILD).is_none());
    let item = host
        .session
        .view()
        .item(ITEMS[2])
        .expect("recovery preserves the application item");
    assert_eq!(item.surface(), B);
    assert!(item.contained().is_some());
    assert_eq!(
        host.session
            .native_surface_bindings_for_window(host.window)
            .collect::<Vec<_>>(),
        host.bindings
    );
    assert!(
        host.session
            .native_surface_bindings_for_window(child.window())
            .next()
            .is_none()
    );
    assert!(
        host.session.view().item(ITEMS[0]).is_some()
            && host.session.view().item(ITEMS[1]).is_some()
    );
}

#[test]
fn shared_root_unplanned_destroy_preserves_both_logical_rosters_and_the_live_child() {
    let mut host = Harness::new();
    let child = host.create_child();
    let before = format!("{:?}", host.session.view());
    let version = host.session.version();
    host.session
        .report_managed_native_snapshot(
            [
                (host.window, NativeWindowFacts::destroyed()),
                (child.window(), child_facts()),
            ],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut destroyed = idle(&mut host.session);
    assert!(!destroyed.workspace_changed());
    assert_eq!(host.session.version(), version);
    assert_eq!(format!("{:?}", host.session.view()), before);
    assert!(
        host.session
            .native_surface_bindings_for_window(host.window)
            .next()
            .is_none()
    );
    assert_eq!(
        host.session.native_window_bindings().collect::<Vec<_>>(),
        [child.window()]
    );
    assert!(host.session.is_current_native_binding(child));
    assert!(destroyed.take_native_effects().is_empty());
}
