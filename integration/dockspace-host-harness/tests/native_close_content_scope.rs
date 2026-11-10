//! Native content closure revalidates its scope without blocking destruction facts.

use dockspace::close::CloseDecision;
use dockspace::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockItemRule, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseInertReason, DockspaceClosePhase, DockspaceClosePlan, DockspaceCloseResolution,
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken,
    NativeCloseDisposition, NativeCloseEffectAcknowledgement, NativeCloseState,
    NativeEffectAcknowledgement, NativeEffectOperation, NativeHostCapabilities,
    NativeHostCapability, NativePointerRoster, NativeReceiverAnswer, NativeSurfaceBinding,
    NativeSurfaceCloseAction, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, SurfaceUnavailableReason,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const SURVIVOR: SurfaceId = SurfaceId::new(2);
const MAIN_ROOT: RootId = RootId::new(1);
const FLOAT_ROOT: RootId = RootId::new(2);
const SURVIVOR_ROOT: RootId = RootId::new(3);
const FLOAT: FloatingPresentationId = FloatingPresentationId::new(1);
const MAIN_ITEM: ItemId = ItemId::new(1);
const FLOAT_ITEM: ItemId = ItemId::new(2);
const SURVIVOR_ITEM: ItemId = ItemId::new(3);

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().expect("the native frame must publish")
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("valid native facts must reduce without poisoning their input prefix"),
    )
}

struct Harness {
    session: DockspaceSession,
    binding: NativeSurfaceBinding,
}

impl Harness {
    fn new(empty_main: bool, capability: CloseCapability) -> Self {
        let main_items = (!empty_main).then_some(MAIN_ITEM);
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs(main_items)),
            )
            .with_contained(DockspaceContainedLayout::new(
                FLOAT,
                DockspaceRootLayout::new(FLOAT_ROOT, DockspaceNode::tabs([FLOAT_ITEM])),
                LogicalRect::new(60.0, 80.0, 300.0, 220.0).unwrap(),
            )),
            DockspaceSurfaceLayout::new(
                SURVIVOR,
                DockspaceRootLayout::new(
                    SURVIVOR_ROOT,
                    DockspaceNode::central_tabs([SURVIVOR_ITEM]),
                ),
            ),
        ])
        .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_close_capability(capability);
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
        .fold(
            NativeHostCapabilities::none_supported(),
            |capabilities, capability| capabilities.with(capability),
        );
        session
            .configure_managed_native_capabilities(capabilities)
            .unwrap();
        session
            .register_native_root(SURFACE, HostWindowToken::new(1))
            .unwrap();
        let registered = idle(&mut session);
        let [binding] = registered.native_bindings() else {
            panic!("the existing native root must publish one binding: {registered:?}");
        };
        let binding = *binding;
        let bounds = PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        let facts = NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible);
        session
            .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
            .unwrap();
        assert!(idle(&mut session).take_native_effects().is_empty());
        assert!(session.is_current_native_binding(binding));
        Self { session, binding }
    }

    fn request_content_close(&mut self) -> DockspaceClosePlan {
        self.session
            .publish_native_close(self.binding, NativeCloseState::Requested, None)
            .unwrap();
        let observed = idle(&mut self.session);
        let [HostInputOutcome::NativeCloseObservationApplied { close_requests }] =
            observed.inputs()
        else {
            panic!("the native root must publish its exact close edge: {observed:?}");
        };
        let [close] = close_requests.as_slice() else {
            panic!("one existing root must produce one close request");
        };
        assert_eq!(close.binding(), self.binding);
        self.session
            .request_native_surface_close(*close, NativeSurfaceCloseAction::CloseContent)
            .unwrap();
        let mut requested = idle(&mut self.session);
        let plan = match requested.inputs() {
            [HostInputOutcome::NativeSurfaceCloseRequested { plan, .. }] => plan.clone(),
            inputs => panic!("all surface content must be offered for approval: {inputs:?}"),
        };
        assert_eq!(plan.phase(), DockspaceClosePhase::AwaitingDecision);
        assert!(!requested.workspace_changed());
        assert!(requested.take_native_effects().is_empty());
        plan
    }

    fn approve(&mut self, plan: &DockspaceClosePlan) -> NativeCloseEffectAcknowledgement {
        let mut frame = self
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        for item in plan.items() {
            frame
                .resolve_close(plan.request(), item.token(), CloseDecision::Allow)
                .unwrap();
        }
        let mut approved = finish(frame);
        assert!(!approved.workspace_changed());
        assert!(matches!(
            approved.inputs().last(),
            Some(HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Approved { request },
                application: None,
                plan: Some(current),
                ..
            }) if *request == plan.request() && current.phase() == DockspaceClosePhase::Settling
        ));
        let mut effects = approved.take_native_effects();
        assert_eq!(effects.len(), 1, "only the approved native Accept may emit");
        let accept = effects.pop().unwrap();
        assert!(matches!(
            accept.operation(),
            NativeEffectOperation::ResolveNativeClose {
                close,
                resolution: NativeCloseDisposition::Accept,
            } if close.binding() == self.binding
        ));
        let Some(NativeEffectAcknowledgement::Close(acknowledgement)) = accept.accepted() else {
            panic!("the accepted close must mint its exact destruction acknowledgement");
        };
        acknowledgement
    }

    fn destroy(&mut self, acknowledgement: NativeCloseEffectAcknowledgement) -> HostFrameReport {
        self.session
            .report_managed_native_snapshot(
                [(
                    self.binding,
                    NativeWindowFacts::destroyed_after(acknowledgement),
                )],
                NativeWorkAreaRoster::Unknown,
            )
            .unwrap();
        let report = idle(&mut self.session);
        assert!(report.native_bindings().is_empty());
        assert!(!report.native_input_pending());
        assert!(!self.session.is_current_native_binding(self.binding));
        report
    }

    fn assert_terminal(&mut self, plan: &DockspaceClosePlan) {
        let mut settled = idle(&mut self.session);
        assert!(!settled.native_input_pending());
        assert!(settled.take_native_effects().is_empty());
        let mut frame = self
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .resolve_close(
                plan.request(),
                plan.items()[0].token(),
                CloseDecision::Allow,
            )
            .unwrap();
        let report = finish(frame);
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(
                    DockspaceCloseInertReason::RetiredRequest
                ),
                application: None,
                changed: false,
                ..
            }]
        ));
        assert!(!report.workspace_changed());
    }
}

fn capability_change_after_accept_preserves_content(
    initial: CloseCapability,
    replacements: &[CloseCapability],
) {
    let mut harness = Harness::new(false, initial);
    let topology = format!("{:?}", harness.session.view());
    let plan = harness.request_content_close();
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        [MAIN_ITEM, FLOAT_ITEM]
    );
    let acknowledgement = harness.approve(&plan);
    assert_eq!(format!("{:?}", harness.session.view()), topology);

    for &replacement in replacements {
        let mut policy = harness.session.policy().clone();
        policy.set_close_capability(replacement);
        let mut frame = harness
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let submitted = frame.replace_policy(policy).unwrap();
        let report = finish(frame);
        assert!(matches!(
            report.submitted_action_outcome(submitted),
            Some(HostInputOutcome::PolicyReplaced { changed: true, .. })
        ));
        assert_eq!(format!("{:?}", harness.session.view()), topology);
    }
    let version = harness.session.version();

    let destroyed = harness.destroy(acknowledgement);
    assert!(!destroyed.workspace_changed());
    assert_eq!(harness.session.version(), version);
    assert_eq!(
        format!("{:?}", harness.session.view()),
        topology,
        "policy drift must preserve unapproved content while committing the native destruction"
    );
    harness.assert_terminal(&plan);
    assert_eq!(format!("{:?}", harness.session.view()), topology);
}

#[test]
fn immediate_to_deferred_policy_after_native_accept_does_not_poison_destruction() {
    capability_change_after_accept_preserves_content(
        CloseCapability::Immediate,
        &[CloseCapability::DeferredAllowed],
    );
}

#[test]
fn deferred_to_immediate_policy_after_native_accept_does_not_poison_destruction() {
    capability_change_after_accept_preserves_content(
        CloseCapability::DeferredAllowed,
        &[CloseCapability::Immediate],
    );
}

#[test]
fn immediate_deferred_immediate_policy_after_native_accept_never_revives_content_approval() {
    capability_change_after_accept_preserves_content(
        CloseCapability::Immediate,
        &[CloseCapability::DeferredAllowed, CloseCapability::Immediate],
    );
}

#[test]
fn immediate_disabled_immediate_policy_after_native_accept_never_revives_content_approval() {
    capability_change_after_accept_preserves_content(
        CloseCapability::Immediate,
        &[CloseCapability::Disabled, CloseCapability::Immediate],
    );
}

#[test]
fn unrelated_pane_close_policy_after_native_accept_preserves_content_approval() {
    let mut harness = Harness::new(false, CloseCapability::Immediate);
    let topology = format!("{:?}", harness.session.view());
    let plan = harness.request_content_close();
    let acknowledgement = harness.approve(&plan);

    let mut policy = harness.session.policy().clone();
    let mut rule = DockItemRule::default();
    rule.set_close_capability(Some(CloseCapability::Disabled));
    policy.set_item_rule(SURVIVOR_ITEM, rule);
    let mut frame = harness
        .session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let submitted = frame.replace_policy(policy).unwrap();
    let report = finish(frame);
    assert!(matches!(
        report.submitted_action_outcome(submitted),
        Some(HostInputOutcome::PolicyReplaced { changed: true, .. })
    ));
    assert_eq!(format!("{:?}", harness.session.view()), topology);

    let destroyed = harness.destroy(acknowledgement);
    assert!(destroyed.workspace_changed());
    assert!(harness.session.view().surface(SURFACE).is_none());
    assert!(harness.session.view().root(MAIN_ROOT).is_none());
    assert!(harness.session.view().root(FLOAT_ROOT).is_none());
    assert!(harness.session.view().item(MAIN_ITEM).is_none());
    assert!(harness.session.view().item(FLOAT_ITEM).is_none());
    assert_eq!(
        harness
            .session
            .view()
            .item(SURVIVOR_ITEM)
            .unwrap()
            .surface(),
        SURVIVOR
    );
    harness.assert_terminal(&plan);
}

#[test]
fn native_close_content_includes_a_nonempty_contained_root_beside_an_empty_central_main() {
    let mut harness = Harness::new(true, CloseCapability::Immediate);
    let version = harness.session.version();
    let topology = format!("{:?}", harness.session.view());
    let plan = harness.request_content_close();
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| item.item())
            .collect::<Vec<_>>(),
        [FLOAT_ITEM]
    );
    let acknowledgement = harness.approve(&plan);
    assert_eq!(harness.session.version(), version);
    assert_eq!(format!("{:?}", harness.session.view()), topology);

    let destroyed = harness.destroy(acknowledgement);
    assert!(destroyed.workspace_changed());
    assert!(harness.session.version().is_same_epoch(version));
    assert!(harness.session.view().surface(SURFACE).is_none());
    assert!(harness.session.view().root(MAIN_ROOT).is_none());
    assert!(harness.session.view().root(FLOAT_ROOT).is_none());
    assert!(harness.session.view().contained(FLOAT).is_none());
    assert!(harness.session.view().item(FLOAT_ITEM).is_none());
    assert_eq!(
        harness
            .session
            .view()
            .item(SURVIVOR_ITEM)
            .unwrap()
            .surface(),
        SURVIVOR
    );
    harness.assert_terminal(&plan);
    assert!(harness.session.view().surface(SURFACE).is_none());
    assert!(harness.session.view().item(FLOAT_ITEM).is_none());
}
