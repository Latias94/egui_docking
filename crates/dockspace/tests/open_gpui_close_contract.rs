//! Close-plan contracts derived from Open GPUI docking behavior.
//!
//! Sources in `repo-ref/open-gpui` revision
//! `56604588ee0a047c59e9ef6a2346f4c5839d90de`:
//! - `workspace_panel_lifecycle_tests.rs`:
//!   `workspace_close_item_transaction_respects_panel_policy`;
//! - `host_interaction_tests.rs`:
//!   `clicking_tab_close_removes_closable_panel_from_graph` and
//!   `non_closable_tab_omits_close_control_and_rejects_close_action`;
//! - `host_viewport_close_tests.rs`:
//!   `viewport_runtime_merge_back_should_close_records_pending_plan_without_graph_mutation`,
//!   `viewport_runtime_pending_merge_back_freezes_should_close_target_tabs`, and the
//!   vacated-source viewport close cases.
//!
//! The port strengthens the source behavior by binding every activation to one
//! acknowledged core scene, freezing the complete decision roster, and making
//! the final topology mutation an all-or-none reducer transaction.

mod support;

use support::TestPresentationHost;

use dockspace::command::{CloseCommitOutcome, ContentCloseTarget};
use dockspace::engine::{DockEngine, EngineInput};
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{
    FloatingPresentationId, ItemId, NodeId, RootId, SourceSequence, StableInputSourceId, SurfaceId,
};
use dockspace::intent::{Authority, CloseSceneTarget, PointerButton, PointerId};
use dockspace::interaction::{InteractionOutcome, InteractionRejection, InteractionStatus};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverCandidate, PointerReceiverDelivery, PointerReceiverDeliveryDisposition,
    PointerReceiverHoverHit, PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverProbeRequest, PointerReceiverReceipt,
    PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::policy::{CloseCapability, DockItemRule, DockPolicy};
use dockspace::presentation_hit::{PresentationHitRegionId, PresentationHitRegionKind};
use dockspace::scene::SurfaceSceneStamp;
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{ViewportRole, WindowToken};
use dockspace::{
    CloseDecision, CloseInertReason, ClosePlan, ClosePlanPhase, ClosePlanTarget,
    CloseResolutionOutcome, DeferredCloseDecision,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(10);
const CONTAINED_ROOT: RootId = RootId::new(11);
const CONTAINED: FloatingPresentationId = FloatingPresentationId::new(20);

const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const SELECTED: ItemId = ItemId::new(3);
const CONTAINED_FIRST: ItemId = ItemId::new(11);
const CONTAINED_DEFERRED: ItemId = ItemId::new(12);
const CONTAINED_LAST: ItemId = ItemId::new(13);

const POINTER: PointerId = PointerId::new(1);
const CONTRACT_SOURCE: StableInputSourceId = StableInputSourceId::new(0xc105_e000_0000_0001);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).expect("fixture rectangle is finite")
}

fn center(rect: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        rect.x() + rect.width() * 0.5,
        rect.y() + rect.height() * 0.5,
    )
    .expect("fixture point is finite")
}

fn graph_bytes(workspace: &Workspace) -> Vec<u8> {
    format!("{workspace:?}").into_bytes()
}

fn selected(workspace: &Workspace, tabs: NodeId) -> Option<ItemId> {
    let Some(Node::Tabs { selected, .. }) = workspace.node(tabs) else {
        panic!("fixture node must remain a tabs stack");
    };
    *selected
}

fn main_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs_with_selection(
        [FIRST, SECOND, SELECTED],
        Some(SELECTED),
    ));
    builder
        .set_tab_mru(tabs, [SELECTED, SECOND, FIRST])
        .expect("fixture MRU is an exact selected-first permutation");
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    (builder.build().expect("fixture workspace is valid"), tabs)
}

fn contained_workspace() -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let main_tabs = builder.insert_node(Node::tabs([FIRST]));
    let first = builder.insert_node(Node::tabs([CONTAINED_FIRST]));
    let deferred = builder.insert_node(Node::tabs([CONTAINED_DEFERRED]));
    let last = builder.insert_node(Node::tabs([CONTAINED_LAST]));
    let contained_root = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [first, deferred, last])
            .expect("contained split is valid"),
    );
    builder.set_root(MAIN_ROOT, RootRecord::new(main_tabs));
    builder.set_root(CONTAINED_ROOT, RootRecord::new(contained_root));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    builder.set_contained_floating(
        CONTAINED,
        ContainedFloating::new(CONTAINED_ROOT, rect(40.0, 50.0, 480.0, 260.0)),
    );
    builder
        .attach_contained(SURFACE, CONTAINED)
        .expect("surface exists");
    (
        builder.build().expect("contained fixture is valid"),
        contained_root,
    )
}

struct Harness {
    engine: DockEngine,
    host: TestPresentationHost,
    source_sequence: u64,
    pointer: Option<TestPointerStream>,
}

#[derive(Debug)]
struct TestPointerStream {
    provider: SurfaceLocalPointerProvider,
    through: u64,
}

impl Harness {
    fn new(workspace: Workspace, policy: DockPolicy) -> Self {
        let mut engine = DockEngine::new(workspace, policy).expect("fixture engine is valid");
        let mut host = TestPresentationHost::new(&mut engine);
        support::publish_surface(
            &mut engine,
            &mut host,
            SURFACE,
            rect(0.0, 0.0, 640.0, 420.0),
        );
        Self {
            engine,
            host,
            source_sequence: 0,
            pointer: None,
        }
    }

    fn submit(&mut self, input: EngineInput) -> EngineTransition {
        self.source_sequence += 1;
        let mut frame = self.host.begin(&self.engine);
        if let Some(pointer) = self.pointer.as_ref() {
            frame
                .submit_surface_pointer_journal(
                    &pointer.provider,
                    empty_pointer_journal(pointer.through),
                )
                .expect("semantic input preserves the pointer watermark");
            frame
                .submit_pointer_receiver_receipts(
                    PointerReceiverReceiptBatch::new(Vec::<PointerReceiverReceipt>::new())
                        .expect("an empty journal has an empty receipt roster"),
                )
                .expect("empty pointer receipts stage");
        }
        support::append_host_input(
            &mut frame,
            CONTRACT_SOURCE,
            SourceSequence::new(self.source_sequence),
            input,
        )
        .expect("contract input must fit the core host-frame phase");
        support::complete_host_frame_with_retained_or_unavailable(&self.engine, &mut frame);
        self.host.finish(frame, &mut self.engine)
    }

    fn request_close(
        &mut self,
        scene: SurfaceSceneStamp,
        target: CloseSceneTarget,
    ) -> (ClosePlan, bool, EngineTransition) {
        let expected = self.engine.version();
        let transition = self.submit(EngineInput::RequestSceneClose {
            expected,
            scene,
            target,
        });
        let (plan, reused) = match only_outcome(&transition) {
            InputOutcome::InteractionProcessed {
                outcome: InteractionOutcome::CloseRequested { plan, reused },
                ..
            } => (plan.clone(), *reused),
            outcome => panic!("unexpected close activation outcome: {outcome:?}"),
        };
        (plan, reused, transition)
    }

    fn request_pointer_close(
        &mut self,
        region: PresentationHitRegionId,
        point: LogicalPoint,
    ) -> (ClosePlan, bool, EngineTransition) {
        if self.pointer.is_none() {
            let provider = self
                .engine
                .create_surface_local_pointer_provider(
                    SurfaceLocalPointerScope::new(
                        self.host.lease(),
                        SurfaceLocalPointerEndpoint::Logical(SURFACE),
                    ),
                    PointerEdgeSequence::new(0),
                )
                .expect("the close fixture admits one surface-local pointer provider");
            self.pointer = Some(TestPointerStream {
                provider,
                through: 0,
            });
        }

        let through = self
            .pointer
            .as_ref()
            .expect("the close pointer provider is active")
            .through;
        let press = pointer_journal(
            through,
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            point,
            PointerCaptureOwner::ProviderEndpoint,
        );
        let mut press_frame = self.host.begin(&self.engine);
        press_frame
            .submit_surface_pointer_journal(
                &self
                    .pointer
                    .as_ref()
                    .expect("the close pointer provider is active")
                    .provider,
                press,
            )
            .expect("close press follows the provider watermark");
        let projection = press_frame
            .view()
            .interaction_projection(SURFACE)
            .expect("the close press uses current presentation authority");
        let candidate = press_frame
            .pointer_receiver_candidates()
            .expect("the close press freezes one receiver candidate")
            .candidates()[0]
            .clone();
        let delivery = PointerReceiverDelivery::new(
            projection,
            PointerReceiverDeliveryDisposition::Dock(region),
        )
        .expect("the close receiver belongs to the current output");
        press_frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([candidate.receipt(
                    PointerReceiverObservation::Presented(
                        PresentedPointerReceiverObservation::new([
                            PointerReceiverProbeReceipt::Delivery(delivery),
                        ])
                        .expect("the press answers its exact delivery probe"),
                    ),
                )])
                .expect("the close press receipt batch is exact"),
            )
            .expect("the close press receipt stages");
        support::complete_host_frame_with_retained_or_unavailable(&self.engine, &mut press_frame);
        let pressed = self.host.finish(press_frame, &mut self.engine);
        assert!(
            pressed.reduced_pointer_edges()[0]
                .interaction_outcomes()
                .is_empty()
        );
        assert!(matches!(
            self.engine.interaction().status(),
            InteractionStatus::Pressed { .. }
        ));
        let pointer = self
            .pointer
            .as_mut()
            .expect("the close pointer provider is active");
        pointer.through = through + 1;
        assert_eq!(
            pointer.provider.committed_through(),
            PointerEdgeSequence::new(pointer.through),
            "committed close press watermark must advance"
        );

        let release = pointer_journal(
            pointer.through,
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            point,
            PointerCaptureOwner::None,
        );
        let mut release_frame = self.host.begin(&self.engine);
        release_frame
            .submit_surface_pointer_journal(&pointer.provider, release)
            .expect("close release follows the press watermark");
        let projection = release_frame
            .view()
            .interaction_projection(SURFACE)
            .expect("the close release uses current presentation authority");
        let candidate = release_frame
            .pointer_receiver_candidates()
            .expect("the close release freezes one receiver candidate")
            .candidates()[0]
            .clone();
        let delivery = PointerReceiverDelivery::new(
            projection,
            PointerReceiverDeliveryDisposition::Dock(region),
        )
        .expect("the released close receiver belongs to the current output");
        let hover = PointerReceiverHoverHit::new(
            projection,
            point,
            PointerReceiverHoverHitDisposition::NoReceiver,
        )
        .expect("the release hover fact belongs to the current output");
        release_frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new([candidate.receipt(exact_candidate_observation(
                    &candidate,
                    Some(delivery),
                    Some(hover),
                ))])
                .expect("the close release receipt batch is exact"),
            )
            .expect("the close release receipt stages");
        support::complete_host_frame_with_retained_or_unavailable(&self.engine, &mut release_frame);
        let transition = self.host.finish(release_frame, &mut self.engine);
        let pointer = self
            .pointer
            .as_mut()
            .expect("the close pointer provider is active");
        pointer.through += 1;
        assert_eq!(
            pointer.provider.committed_through(),
            PointerEdgeSequence::new(pointer.through),
            "committed close release watermark must advance"
        );

        let [edge] = transition.reduced_pointer_edges() else {
            panic!(
                "expected one reduced close release, got {:?}",
                transition.reduced_pointer_edges()
            );
        };
        let [InteractionOutcome::CloseRequested { plan, reused }] = edge.interaction_outcomes()
        else {
            panic!(
                "unexpected pointer close activation outcomes: {:?}",
                edge.interaction_outcomes()
            );
        };
        (plan.clone(), *reused, transition)
    }

    fn resolve(
        &mut self,
        plan: &ClosePlan,
        item_index: usize,
        decision: CloseDecision,
    ) -> EngineTransition {
        self.submit(EngineInput::ResolveClose {
            request: plan.request(),
            token: plan.items()[item_index].token(),
            decision,
        })
    }
}

#[derive(Debug, Clone, Copy)]
struct TabCloseFact {
    scene: SurfaceSceneStamp,
    target: CloseSceneTarget,
    region: Option<PresentationHitRegionId>,
    close_point: Option<LogicalPoint>,
}

fn tab_close_fact(engine: &DockEngine, item: ItemId) -> TabCloseFact {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("fixture scene is acknowledged");
    let tab = ready
        .plan()
        .tab_records()
        .iter()
        .find(|tab| tab.id().item == item)
        .expect("fixture tab is painted");
    let region = engine
        .interaction_projection(SURFACE)
        .expect("fixture surface has current interaction authority")
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabClose(actual) if actual == *tab.id()
            )
        })
        .map(|region| region.id());
    TabCloseFact {
        scene: ready.stamp(),
        target: CloseSceneTarget::Tab(*tab.id()),
        region,
        close_point: tab.close_bounds().map(center),
    }
}

fn contained_close_fact(engine: &DockEngine) -> (SurfaceSceneStamp, CloseSceneTarget) {
    let ready = engine
        .scene()
        .ready_surface(SURFACE)
        .expect("fixture scene is acknowledged");
    let record = ready
        .plan()
        .contained_record(CONTAINED)
        .expect("contained presentation is painted");
    assert!(record.close_bounds().is_some(), "root close control exists");
    (ready.stamp(), CloseSceneTarget::Contained(CONTAINED))
}

fn pointer_journal(
    previous: u64,
    kind: PointerEdgeKind,
    position: LogicalPoint,
    capture: PointerCaptureOwner,
) -> PointerEdgeJournal {
    let sequence = PointerEdgeSequence::new(previous + 1);
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        sequence,
        vec![PointerEdge::new(
            sequence,
            POINTER,
            kind,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(position),
            },
            Authority::Known(capture),
        )],
    )
    .expect("one close pointer edge is contiguous")
}

fn empty_pointer_journal(watermark: u64) -> PointerEdgeJournal {
    let watermark = PointerEdgeSequence::new(watermark);
    PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("an empty pointer journal preserves its watermark")
}

fn exact_candidate_observation(
    candidate: &PointerReceiverCandidate,
    delivery: Option<PointerReceiverDelivery>,
    hover: Option<PointerReceiverHoverHit>,
) -> PointerReceiverObservation {
    let probes = match candidate.probes() {
        PointerReceiverProbeRequest::NotApplicable => {
            return PointerReceiverObservation::NotApplicable;
        }
        PointerReceiverProbeRequest::Delivery => vec![PointerReceiverProbeReceipt::Delivery(
            delivery.expect("the candidate requires a delivery fact"),
        )],
        PointerReceiverProbeRequest::HoverHit => vec![PointerReceiverProbeReceipt::HoverHit(
            hover.expect("the candidate requires a hover fact"),
        )],
        PointerReceiverProbeRequest::DeliveryAndHoverHit => vec![
            PointerReceiverProbeReceipt::Delivery(
                delivery.expect("the candidate requires a delivery fact"),
            ),
            PointerReceiverProbeReceipt::HoverHit(
                hover.expect("the candidate requires a hover fact"),
            ),
        ],
    };
    PointerReceiverObservation::Presented(
        PresentedPointerReceiverObservation::new(probes)
            .expect("the candidate observation answers its exact probe roster"),
    )
}

fn only_outcome(transition: &EngineTransition) -> &InputOutcome {
    let [input] = transition.reduced_inputs() else {
        panic!(
            "expected one reduced input, got {:?}",
            transition.reduced_inputs()
        );
    };
    input.outcome()
}

fn assert_graph_unchanged(engine: &DockEngine, before: &Workspace, before_bytes: &[u8]) {
    assert_eq!(engine.workspace(), before);
    assert_eq!(graph_bytes(engine.workspace()), before_bytes);
}

#[test]
fn programmatic_item_close_uses_the_same_plan_and_commit_protocol() {
    let (workspace, tabs) = main_workspace();
    let mut fixture = Harness::new(workspace, DockPolicy::default());
    let before = fixture.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);

    let expected = fixture.engine.version();
    let requested = fixture.submit(EngineInput::RequestContentClose {
        expected,
        target: ContentCloseTarget::Item(SELECTED),
    });
    let plan = match only_outcome(&requested) {
        InputOutcome::ContentCloseRequested {
            target: ContentCloseTarget::Item(SELECTED),
            plan,
            reused: false,
            ..
        } => plan.clone(),
        outcome => panic!("unexpected programmatic close outcome: {outcome:?}"),
    };
    assert_eq!(plan.target(), ClosePlanTarget::Item { item: SELECTED });
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let reused = fixture.submit(EngineInput::RequestContentClose {
        expected,
        target: ContentCloseTarget::Item(SELECTED),
    });
    assert!(matches!(
        only_outcome(&reused),
        InputOutcome::ContentCloseRequested {
            target: ContentCloseTarget::Item(SELECTED),
            plan: current,
            reused: true,
            ..
        } if current.request() == plan.request()
    ));

    let committed = fixture.resolve(&plan, 0, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&committed),
        InputOutcome::CloseDecisionProcessed {
            application: Some(Ok(CloseCommitOutcome::ItemClosed {
                item: SELECTED,
                root: MAIN_ROOT,
            })),
            changed: true,
            ..
        }
    ));
    assert_eq!(selected(fixture.engine.workspace(), tabs), Some(SECOND));
}

#[test]
fn programmatic_root_close_is_all_or_none_and_policy_owned() {
    let (workspace, _) = contained_workspace();
    let mut policy = DockPolicy::default();
    let mut disabled = DockItemRule::default();
    disabled.set_close_capability(Some(CloseCapability::Disabled));
    policy.set_item_rule(CONTAINED_DEFERRED, disabled);
    let mut fixture = Harness::new(workspace, policy);
    let before = fixture.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);

    let rejected = fixture.submit(EngineInput::RequestContentClose {
        expected: fixture.engine.version(),
        target: ContentCloseTarget::Root(CONTAINED_ROOT),
    });
    assert!(matches!(
        only_outcome(&rejected),
        InputOutcome::ContentCloseRejected {
            target: ContentCloseTarget::Root(CONTAINED_ROOT),
            reason: dockspace::transition::ContentCloseRequestRejection::ItemCloseDisabled {
                item: CONTAINED_DEFERRED,
            },
            ..
        }
    ));
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);
    assert!(fixture.engine.active_close_plans().next().is_none());
}

#[test]
fn scene_bound_tab_close_waits_for_allow_then_commits_once_with_mru() {
    let (workspace, tabs) = main_workspace();
    let mut fixture = Harness::new(workspace, DockPolicy::default());
    let fact = tab_close_fact(&fixture.engine, SELECTED);
    let before = fixture.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);
    let before_version = fixture.engine.version();

    let (plan, reused, requested) = fixture.request_pointer_close(
        fact.region
            .expect("the selected closeable tab exposes one exact receiver"),
        fact.close_point.expect("selected tab is closeable"),
    );

    assert!(!reused);
    assert_eq!(plan.target(), ClosePlanTarget::Item { item: SELECTED });
    assert_eq!(plan.phase(), ClosePlanPhase::Requested);
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), SELECTED);
    assert_eq!(fixture.engine.version(), before_version);
    assert!(requested.events().is_empty());
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let committed = fixture.resolve(&plan, 0, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&committed),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Approved { request },
            plan: Some(applied),
            application: Some(Ok(CloseCommitOutcome::ItemClosed {
                item: SELECTED,
                root: MAIN_ROOT,
            })),
            changed: true,
            ..
        } if *request == plan.request() && applied.phase() == ClosePlanPhase::Applied
    ));
    assert_eq!(selected(fixture.engine.workspace(), tabs), Some(SECOND));
    assert_eq!(
        fixture.engine.workspace().tab_mru(tabs),
        Some([SECOND, FIRST].as_slice())
    );

    let after_first_commit = fixture.engine.workspace().clone();
    let after_first_version = fixture.engine.version();
    let duplicate = fixture.resolve(&plan, 0, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&duplicate),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Inert(CloseInertReason::RetiredTerminal {
                request,
            }),
            application: None,
            changed: false,
            ..
        } if *request == plan.request()
    ));
    assert_eq!(fixture.engine.workspace(), &after_first_commit);
    assert_eq!(fixture.engine.version(), after_first_version);
}

#[test]
fn veto_and_disabled_close_activations_never_mutate_topology() {
    let (workspace, _) = main_workspace();
    let mut veto = Harness::new(workspace.clone(), DockPolicy::default());
    let fact = tab_close_fact(&veto.engine, SELECTED);
    let before = veto.engine.workspace().clone();
    let bytes = graph_bytes(&before);
    let (plan, _, _) = veto.request_pointer_close(
        fact.region
            .expect("the selected closeable tab exposes one exact receiver"),
        fact.close_point.expect("selected tab is closeable"),
    );
    let vetoed = veto.resolve(&plan, 0, CloseDecision::Veto);
    assert!(matches!(
        only_outcome(&vetoed),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Vetoed {
                request,
                item: SELECTED,
            },
            plan: Some(current),
            application: None,
            changed: false,
            ..
        } if *request == plan.request() && current.phase() == ClosePlanPhase::Vetoed
    ));
    assert_graph_unchanged(&veto.engine, &before, &bytes);

    let mut policy = DockPolicy::default();
    let mut disabled_rule = DockItemRule::default();
    disabled_rule.set_close_capability(Some(CloseCapability::Disabled));
    policy.set_item_rule(SELECTED, disabled_rule);
    let mut disabled = Harness::new(workspace.clone(), policy);
    let disabled_fact = tab_close_fact(&disabled.engine, SELECTED);
    assert!(disabled_fact.close_point.is_none());
    assert!(disabled_fact.region.is_none());
    let disabled_before = disabled.engine.workspace().clone();
    let disabled_bytes = graph_bytes(&disabled_before);
    let expected = disabled.engine.version();
    let rejected = disabled.submit(EngineInput::RequestSceneClose {
        expected,
        scene: disabled_fact.scene,
        target: disabled_fact.target,
    });
    assert!(matches!(
        only_outcome(&rejected),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(
                InteractionRejection::CloseControlUnavailable { target }
            ),
            ..
        } if *target == disabled_fact.target
    ));
    assert!(!rejected.published_state_changed());
    assert_graph_unchanged(&disabled.engine, &disabled_before, &disabled_bytes);
}

#[test]
fn contained_root_freezes_complete_order_and_partial_decisions_remain_atomic() {
    let (workspace, _) = contained_workspace();
    let mut policy = DockPolicy::default();
    let mut deferred_rule = DockItemRule::default();
    deferred_rule.set_close_capability(Some(CloseCapability::DeferredAllowed));
    policy.set_item_rule(CONTAINED_DEFERRED, deferred_rule);
    let mut fixture = Harness::new(workspace, policy);
    let (scene, target) = contained_close_fact(&fixture.engine);
    let before = fixture.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);

    let (plan, reused, _) = fixture.request_close(scene, target);
    assert!(!reused);
    assert_eq!(
        plan.target(),
        ClosePlanTarget::Root {
            root: CONTAINED_ROOT,
        }
    );
    assert_eq!(
        plan.items()
            .iter()
            .map(|item| (item.item(), item.capability()))
            .collect::<Vec<_>>(),
        vec![
            (CONTAINED_FIRST, CloseCapability::Immediate),
            (CONTAINED_DEFERRED, CloseCapability::DeferredAllowed),
            (CONTAINED_LAST, CloseCapability::Immediate),
        ]
    );
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let first = fixture.resolve(&plan, 0, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&first),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Recorded {
                item: CONTAINED_FIRST,
                phase: ClosePlanPhase::Resolving,
                ..
            },
            application: None,
            changed: false,
            ..
        }
    ));
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let deferred = fixture.resolve(&plan, 1, CloseDecision::Deferred);
    let continuation = match only_outcome(&deferred) {
        InputOutcome::CloseDecisionProcessed {
            resolution:
                CloseResolutionOutcome::Deferred {
                    item: CONTAINED_DEFERRED,
                    continuation,
                    ..
                },
            plan: Some(current),
            application: None,
            changed: false,
            ..
        } => {
            assert_eq!(current.phase(), ClosePlanPhase::Deferred);
            *continuation
        }
        outcome => panic!("unexpected deferred decision outcome: {outcome:?}"),
    };
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let last = fixture.resolve(&plan, 2, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&last),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Recorded {
                item: CONTAINED_LAST,
                phase: ClosePlanPhase::Deferred,
                ..
            },
            application: None,
            changed: false,
            ..
        }
    ));
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let vetoed = fixture.submit(EngineInput::ContinueDeferredClose {
        request: plan.request(),
        token: continuation,
        decision: DeferredCloseDecision::Veto,
    });
    assert!(matches!(
        only_outcome(&vetoed),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Vetoed {
                item: CONTAINED_DEFERRED,
                ..
            },
            plan: Some(current),
            application: None,
            changed: false,
            ..
        } if current.phase() == ClosePlanPhase::Vetoed
    ));
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);
    assert!(fixture.engine.workspace().root(CONTAINED_ROOT).is_some());
    assert!(
        fixture
            .engine
            .workspace()
            .contained_floating(CONTAINED)
            .is_some()
    );
}

#[test]
fn repeated_activation_reuses_the_exact_unresolved_request() {
    let (workspace, _) = main_workspace();
    let mut fixture = Harness::new(workspace, DockPolicy::default());
    let fact = tab_close_fact(&fixture.engine, SELECTED);
    let before = fixture.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);

    let (first, first_reused, _) = fixture.request_close(fact.scene, fact.target);
    let (second, second_reused, _) = fixture.request_close(fact.scene, fact.target);

    assert!(!first_reused);
    assert!(second_reused);
    assert_eq!(second, first);
    assert_eq!(second.request(), first.request());
    assert_eq!(second.items()[0].token(), first.items()[0].token());
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);
}

#[test]
fn authority_change_stales_every_preapproval_token() {
    let (workspace, _) = main_workspace();
    let mut fixture = Harness::new(workspace, DockPolicy::default());
    let fact = tab_close_fact(&fixture.engine, SELECTED);
    let before = fixture.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);
    let (plan, _, _) = fixture.request_close(fact.scene, fact.target);

    let expected = fixture.engine.version();
    let mut replacement = fixture.engine.policy().clone();
    replacement.set_allow_native_surfaces(true);
    let replaced = fixture.submit(EngineInput::ReplacePolicy {
        expected,
        policy: replacement,
    });
    assert!(matches!(
        only_outcome(&replaced),
        InputOutcome::PolicyReplaced { changed: true, .. }
    ));
    assert_eq!(
        fixture
            .engine
            .close_plan(plan.request())
            .map(ClosePlan::phase),
        Some(ClosePlanPhase::Stale)
    );
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);

    let stale = fixture.resolve(&plan, 0, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&stale),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Inert(CloseInertReason::RetiredTerminal {
                request,
            }),
            application: None,
            changed: false,
            ..
        } if *request == plan.request()
    ));
    assert_graph_unchanged(&fixture.engine, &before, &before_bytes);
}

#[test]
fn closing_the_last_root_settles_surface_and_external_binding_in_the_same_tick() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([FIRST]));
    builder.set_root(MAIN_ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(MAIN_ROOT));
    let workspace = builder.build().expect("single-root fixture is valid");
    let mut fixture = Harness::new(workspace, DockPolicy::default());
    let fact = tab_close_fact(&fixture.engine, FIRST);
    let (plan, _, _) = fixture.request_close(fact.scene, fact.target);
    let provider = fixture.host.platform_provider();

    let registered = fixture.submit(EngineInput::RegisterViewport {
        provider,
        expected: fixture.engine.version(),
        surface: SURFACE,
        token: WindowToken::new(90),
        role: ViewportRole::Root,
        recovery_target: None,
    });
    assert!(matches!(
        only_outcome(&registered),
        InputOutcome::ViewportRegistered { binding } if binding.surface() == SURFACE
    ));
    assert!(
        fixture
            .engine
            .viewport()
            .registry()
            .record(SURFACE)
            .is_some()
    );

    let closed = fixture.resolve(&plan, 0, CloseDecision::Allow);
    assert!(matches!(
        only_outcome(&closed),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Approved { request },
            plan: Some(applied),
            application: Some(Ok(CloseCommitOutcome::ItemClosed {
                item: FIRST,
                root: MAIN_ROOT,
            })),
            changed: true,
            ..
        } if *request == plan.request() && applied.phase() == ClosePlanPhase::Applied
    ));
    assert!(fixture.engine.workspace().surface(SURFACE).is_none());
    assert!(
        fixture
            .engine
            .viewport()
            .registry()
            .record(SURFACE)
            .is_none()
    );
    assert!(
        closed.platform_effects().is_empty(),
        "external binding only unbinds"
    );
}

#[test]
fn close_authority_from_an_old_engine_is_inert_in_a_new_engine() {
    let (workspace, _) = main_workspace();
    let mut old = Harness::new(workspace.clone(), DockPolicy::default());
    let old_fact = tab_close_fact(&old.engine, SELECTED);
    let (old_plan, _, _) = old.request_close(old_fact.scene, old_fact.target);

    let mut current = Harness::new(workspace, DockPolicy::default());
    let current_fact = tab_close_fact(&current.engine, SELECTED);
    let (current_plan, _, _) = current.request_close(current_fact.scene, current_fact.target);
    let before = current.engine.workspace().clone();
    let before_bytes = graph_bytes(&before);

    let stale_engine_input = current.submit(EngineInput::ResolveClose {
        request: old_plan.request(),
        token: old_plan.items()[0].token(),
        decision: CloseDecision::Allow,
    });

    assert!(matches!(
        only_outcome(&stale_engine_input),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Inert(CloseInertReason::RequestDomainMismatch {
                request,
                ..
            }),
            application: None,
            changed: false,
            ..
        } if *request == old_plan.request()
    ));
    assert_ne!(old_plan.request(), current_plan.request());
    assert_ne!(old_plan.items()[0].token(), current_plan.items()[0].token());
    assert_ne!(old_plan.request().domain(), current_plan.request().domain());
    assert_ne!(
        old_plan.items()[0].token().domain(),
        current_plan.items()[0].token().domain()
    );
    assert_graph_unchanged(&current.engine, &before, &before_bytes);

    let mixed_authority_input = current.submit(EngineInput::ResolveClose {
        request: current_plan.request(),
        token: old_plan.items()[0].token(),
        decision: CloseDecision::Allow,
    });
    assert!(matches!(
        only_outcome(&mixed_authority_input),
        InputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Inert(
                CloseInertReason::UnknownDecisionToken { request, token }
            ),
            application: None,
            changed: false,
            ..
        } if *request == current_plan.request() && *token == old_plan.items()[0].token()
    ));
    assert_graph_unchanged(&current.engine, &before, &before_bytes);
}
