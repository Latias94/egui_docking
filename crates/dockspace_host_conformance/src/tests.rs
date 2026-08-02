use std::collections::BTreeMap;

use dockspace::command::{
    CloseCommitOutcome, CommandOutcome, ContentCloseTarget, DockFraction, DockTarget, Edge,
    MovePayload, WorkspaceCommand,
};
use dockspace::error::{CommandError, ReferenceRole};
use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, NodeId, RootId, SurfaceId};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostWindowToken, NativeCloseState, NativePlatformError, NativeSurfaceLease,
    NativeWindowFacts, PresentedDockReceiver, SurfacePointerEvent, SurfacePointerReceiverFacts,
    UniformSurfaceMetrics,
};
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::{CloseDecision, CloseResolutionOutcome};

const SURFACE: SurfaceId = SurfaceId::new(1);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const SOURCE_ROOT: RootId = RootId::new(2);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);
const X: ItemId = ItemId::new(4);
const WINDOW: HostWindowToken = HostWindowToken::new(41);
const SECOND_WINDOW: HostWindowToken = HostWindowToken::new(42);

struct DeterministicHost {
    session: DockspaceSession,
}

impl DeterministicHost {
    fn new(workspace: Workspace) -> Self {
        Self {
            session: DockspaceSession::new(workspace, DockPolicy::default())
                .expect("the conformance workspace must initialize"),
        }
    }

    fn workspace(&self) -> &Workspace {
        self.session.workspace()
    }

    fn run(&mut self, mutate: impl FnOnce(&mut DockspaceHostFrame<'_>)) -> HostFrameReport {
        let mut frame = self
            .session
            .begin_host_frame()
            .expect("the deterministic host frame must begin");
        mutate(&mut frame);
        frame
            .complete_unpainted_surfaces(MeasurementUnavailableReason::Deferred)
            .expect("the driver explicitly settles every unpainted surface");
        frame
            .commit()
            .expect("the host frame must commit atomically")
    }

    fn observe_painted_outputs(&mut self, mut report: HostFrameReport) {
        let outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 1, "the fixture paints one logical surface");
        for output in outputs {
            self.session
                .confirm_presented(output)
                .expect("the host confirms the exact output it presented");
        }
        self.run(|_| {});
    }
}

fn tabs_workspace(items: impl IntoIterator<Item = ItemId>) -> (Workspace, NodeId) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs(items));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    (builder.build().expect("the tabs workspace is valid"), tabs)
}

fn tabs_containing(workspace: &Workspace, item: ItemId) -> NodeId {
    workspace
        .nodes()
        .find_map(|(node, record)| match record {
            Node::Tabs { items, .. } if items.contains(&item) => Some(node),
            Node::Tabs { .. } | Node::Split { .. } => None,
        })
        .expect("the item must remain owned by one tabs node")
}

fn assert_command_applied(report: &HostFrameReport) {
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CommandApplied {
            outcome: CommandOutcome::Moved { changed: true, .. },
            changed: true,
        }]
    ));
}

#[test]
fn ogc_01_repeated_same_axis_docks_flatten_and_stale_targets_are_inert() {
    let (workspace, original_tabs) = tabs_workspace([A, B, C]);
    let mut host = DeterministicHost::new(workspace);

    let stale_target = host
        .workspace()
        .capture_inner_edge_target(
            ROOT,
            original_tabs,
            Edge::Right,
            DockFraction::new(0.5).expect("the fixture fraction is valid"),
        )
        .expect("the original target is current");
    let source_b = host
        .workspace()
        .capture_item_source(ROOT, original_tabs, B)
        .expect("item B is current");
    let first = host.run(|frame| {
        frame
            .submit_command(WorkspaceCommand::Move {
                payload: MovePayload::Item(source_b),
                target: DockTarget::InnerEdge(stale_target.clone()),
            })
            .expect("the first checked move must append");
    });
    assert_command_applied(&first);

    let b_tabs = tabs_containing(host.workspace(), B);
    let source_c = host
        .workspace()
        .capture_item_source(ROOT, original_tabs, C)
        .expect("item C remains in the original tabs");
    let target_b = host
        .workspace()
        .capture_inner_edge_target(
            ROOT,
            b_tabs,
            Edge::Right,
            DockFraction::new(0.5).expect("the fixture fraction is valid"),
        )
        .expect("the second target is current");
    let second = host.run(|frame| {
        frame
            .submit_command(WorkspaceCommand::Move {
                payload: MovePayload::Item(source_c),
                target: DockTarget::InnerEdge(target_b),
            })
            .expect("the second checked move must append");
    });
    assert_command_applied(&second);

    let root_node = host.workspace().root(ROOT).expect("the root remains").node;
    let Node::Split { axis, children, .. } = host
        .workspace()
        .node(root_node)
        .expect("the canonical root node remains")
    else {
        panic!("the root must be a same-axis split");
    };
    assert_eq!(*axis, Axis::Horizontal);
    assert_eq!(children.len(), 3, "same-axis wrappers must flatten");
    host.workspace()
        .validate()
        .expect("the flattened workspace remains canonical");

    let before_stale = host.workspace().clone();
    let current_source = host
        .workspace()
        .capture_item_source(ROOT, tabs_containing(host.workspace(), A), A)
        .expect("item A is current");
    let stale = host.run(|frame| {
        frame
            .submit_command(WorkspaceCommand::Move {
                payload: MovePayload::Item(current_source),
                target: DockTarget::InnerEdge(stale_target),
            })
            .expect("a stale checked command is still structurally accepted");
    });
    assert!(matches!(
        stale.inputs(),
        [HostInputOutcome::CommandRejected(CommandError::StaleNode {
            role: ReferenceRole::Target,
            ..
        })]
    ));
    assert_eq!(host.workspace(), &before_stale);
}

#[test]
fn ogc_02_merge_and_close_preserve_target_local_mru_atomically() {
    let mut builder = Workspace::builder();
    let target_tabs = builder.insert_node(Node::tabs_with_selection([A, B, C], Some(C)));
    let source_tabs = builder.insert_node(Node::tabs([X]));
    builder.set_root(ROOT, RootRecord::new(target_tabs));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let source_surface = SurfaceId::new(2);
    builder.set_surface(source_surface, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut host = DeterministicHost::new(builder.build().expect("the merge workspace is valid"));

    let select_b = host
        .workspace()
        .capture_item_source(ROOT, target_tabs, B)
        .expect("target item B is current");
    host.run(|frame| {
        frame
            .submit_command(WorkspaceCommand::Select { source: select_b })
            .expect("selection must append");
    });
    assert_eq!(
        host.workspace().tab_mru(target_tabs),
        Some([B, C, A].as_slice())
    );

    let source = host
        .workspace()
        .capture_node_source(SOURCE_ROOT, source_tabs)
        .expect("the source tabs are current");
    let target = host
        .workspace()
        .capture_tab_target(ROOT, target_tabs)
        .expect("the target tabs are current");
    let before_items = host.workspace().item_multiset();
    let merge = host.run(|frame| {
        frame
            .submit_command(WorkspaceCommand::Move {
                payload: MovePayload::Tabs(source),
                target: DockTarget::Center(target),
            })
            .expect("the merge must append");
    });
    assert_command_applied(&merge);
    assert_eq!(host.workspace().item_multiset(), before_items);
    assert_eq!(
        host.workspace().tab_mru(target_tabs),
        Some([X, B, C, A].as_slice())
    );

    let request = host.run(|frame| {
        frame
            .request_close(ContentCloseTarget::Item(X))
            .expect("the close request must append");
    });
    let plan = match request.inputs() {
        [
            HostInputOutcome::CloseRequested {
                plan,
                reused: false,
            },
        ] => plan.clone(),
        outcomes => panic!("expected one new close plan, got {outcomes:?}"),
    };
    let close_item = plan
        .items()
        .iter()
        .copied()
        .find(|item| item.item() == X)
        .expect("the close plan contains item X");
    let close = host.run(|frame| {
        frame
            .resolve_close(plan.request(), close_item.token(), CloseDecision::Allow)
            .expect("the close decision must append");
    });
    assert!(matches!(
        close.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Approved { request },
            application: Some(Ok(CloseCommitOutcome::ItemClosed { item: X, .. })),
            changed: true,
            ..
        }] if *request == plan.request()
    ));

    let Node::Tabs { items, selected } = host
        .workspace()
        .node(target_tabs)
        .expect("the target tabs remain")
    else {
        panic!("the target remains a tabs node");
    };
    assert_eq!(items, &[A, B, C]);
    assert_eq!(*selected, Some(B));
    assert_eq!(
        host.workspace().tab_mru(target_tabs),
        Some([B, C, A].as_slice())
    );
    assert_eq!(
        host.workspace().item_multiset(),
        BTreeMap::from([(A, 1), (B, 1), (C, 1)])
    );
    host.workspace()
        .validate()
        .expect("the merge-close workflow remains canonical");
}

struct InteractionFixture {
    host: DeterministicHost,
    source: DockspaceReceiverDescriptor,
    target: DockspaceReceiverDescriptor,
}

impl InteractionFixture {
    fn new() -> Self {
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([A, B]));
        let target_tabs = builder.insert_node(Node::tabs([C]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [source_tabs, target_tabs])
                .expect("the fixture split is valid"),
        );
        builder.set_root(ROOT, RootRecord::new(split));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let mut host = DeterministicHost::new(
            builder
                .build()
                .expect("the interaction workspace is canonical"),
        );
        let bounds =
            LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture surface bounds are valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
        let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
            .expect("the fixture measurements are valid");
        host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the first pass measures the complete surface");
        });

        let mut source = None;
        let mut target = None;
        let paint = host.run(|frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the pointer-input phase must close before paint")
                .expect("the measured surface exposes one paint plan");
            source = Some(
                plan.tab_receiver(A)
                    .expect("the source tab exposes its exact receiver"),
            );
            target = Some(
                plan.center_drop_receiver_for_item(C)
                    .expect("the target stack exposes its exact center receiver"),
            );
            assert!(plan.drag_preview().is_none());
            frame
                .confirm_surface_painted(SURFACE)
                .expect("the host records the exact plan it painted");
        });
        host.observe_painted_outputs(paint);
        host.session
            .enable_surface_pointer(SURFACE)
            .expect("the observed surface admits one local pointer provider");

        Self {
            host,
            source: source.expect("source descriptor was captured while painting"),
            target: target.expect("target descriptor was captured while painting"),
        }
    }

    fn current_source(&self) -> PresentedDockReceiver {
        self.host
            .session
            .bind_presented_receiver(&self.source)
            .expect("the source receiver belongs to the current presented output")
    }

    fn current_target(&self) -> PresentedDockReceiver {
        self.host
            .session
            .bind_presented_receiver(&self.target)
            .expect("the target receiver belongs to the current presented output")
    }

    fn begin_drag_without_target(&mut self) {
        let source = self.current_source();
        let press = source.center();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(
                    SurfacePointerEvent::PrimaryPressed,
                    press,
                    SurfacePointerReceiverFacts::delivery(&source),
                )
                .expect("the source tab receives the press");
        });

        let surface = self
            .host
            .session
            .presented_surface(SURFACE)
            .expect("the surface remains presented");
        let moved = LogicalPoint::new(press.x() + 24.0, press.y() + 24.0)
            .expect("the threshold-crossing point is valid");
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(
                    SurfacePointerEvent::Moved,
                    moved,
                    SurfacePointerReceiverFacts::no_hover(&surface),
                )
                .expect("the threshold move has an exact known-empty hover result");
        });
    }

    fn preview_target(&mut self) {
        let target = self.current_target();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(
                    SurfacePointerEvent::Moved,
                    target.center(),
                    SurfacePointerReceiverFacts::hover(&target),
                )
                .expect("the target receives one exact hover edge");
        });
    }

    fn paint_preview(&mut self) {
        let paint = self.host.run(|frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the pointer-input phase must close before paint")
                .expect("the active drag retains one paint plan");
            assert!(
                plan.drag_preview().is_some(),
                "the renderer must actually see the preview it confirms"
            );
            frame
                .confirm_surface_painted(SURFACE)
                .expect("the preview-bearing output was painted");
        });
        self.host.observe_painted_outputs(paint);
    }

    fn attach_native_surface(&mut self) -> NativeSurfaceLease {
        self.host
            .session
            .enable_native_platform()
            .expect("the deterministic host enrolls one native platform provider");
        let registration = self.host.run(|frame| {
            frame
                .register_native_root(SURFACE, WINDOW)
                .expect("the logical surface binds to the host window token");
        });
        let lease = match registration.inputs() {
            [HostInputOutcome::NativeSurfaceRegistered { lease }] => *lease,
            outcomes => panic!("expected one native registration, got {outcomes:?}"),
        };

        let physical = PhysicalRect::new(0.0, 0.0, 640.0, 360.0)
            .expect("the fixture physical bounds are valid");
        let scale = ScaleFactor::new(1.0).expect("the fixture scale is valid");
        let snapshot = self
            .host
            .session
            .capture_native_snapshot([(lease, NativeWindowFacts::ready(physical, scale))])
            .expect("the host captures one complete native roster");
        self.host.run(|frame| {
            frame
                .publish_native_snapshot(snapshot)
                .expect("the ready native snapshot joins the host frame");
        });

        let bounds =
            LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
        let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
            .expect("the fixture measurements are valid");
        self.host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("native coordinate authority admits a fresh measurement");
        });
        let mut source = None;
        let mut target = None;
        let paint = self.host.run(|frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the fresh native plan is inspectable")
                .expect("the fresh native plan is ready");
            source = plan.tab_receiver(A);
            target = plan.center_drop_receiver_for_item(C);
            frame
                .confirm_surface_painted(SURFACE)
                .expect("the native surface plan was painted");
        });
        self.host.observe_painted_outputs(paint);
        self.source = source.expect("the native source receiver is present");
        self.target = target.expect("the native target receiver is present");
        lease
    }
}

#[test]
fn ogc_03_release_on_first_target_hit_is_inert_without_a_painted_preview() {
    let mut fixture = InteractionFixture::new();
    fixture.begin_drag_without_target();
    let target = fixture.current_target();
    let before = fixture.host.workspace().clone();

    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer(
                SurfacePointerEvent::PrimaryReleased,
                target.center(),
                SurfacePointerReceiverFacts::hover(&target),
            )
            .expect("the first target hit is reported exactly on release");
    });

    assert_eq!(fixture.host.workspace(), &before);
}

#[test]
fn ogc_03_cached_or_stale_receiver_cannot_authorize_release() {
    let mut cached = InteractionFixture::new();
    cached.begin_drag_without_target();
    cached.preview_target();
    cached.paint_preview();
    let before_cached = cached.host.workspace().clone();
    let point = cached.current_target().center();
    cached.host.run(|frame| {
        frame
            .submit_surface_pointer(
                SurfacePointerEvent::PrimaryReleased,
                point,
                SurfacePointerReceiverFacts::unknown(),
            )
            .expect("absence of a current hover fact is represented as unknown");
    });
    assert_eq!(cached.host.workspace(), &before_cached);

    let mut stale = InteractionFixture::new();
    stale.begin_drag_without_target();
    stale.preview_target();
    stale.paint_preview();
    let old_target = stale.current_target();
    stale.paint_preview();
    let before_stale = stale.host.workspace().clone();
    stale.host.run(|frame| {
        frame
            .submit_surface_pointer(
                SurfacePointerEvent::PrimaryReleased,
                old_target.center(),
                SurfacePointerReceiverFacts::hover(&old_target),
            )
            .expect("the facade converts a stale concrete receiver into fail-closed evidence");
    });
    assert_eq!(stale.host.workspace(), &before_stale);
}

#[test]
fn ogc_03_current_painted_preview_commits_exactly_once() {
    let mut fixture = InteractionFixture::new();
    fixture.begin_drag_without_target();
    fixture.preview_target();
    fixture.paint_preview();
    let target = fixture.current_target();

    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer(
                SurfacePointerEvent::PrimaryReleased,
                target.center(),
                SurfacePointerReceiverFacts::hover(&target),
            )
            .expect("the current receiver reports release over the painted preview");
    });

    let target_tabs = tabs_containing(fixture.host.workspace(), C);
    assert!(matches!(
        fixture.host.workspace().node(target_tabs),
        Some(Node::Tabs { items, .. }) if items == &[C, A]
    ));
    fixture
        .host
        .workspace()
        .validate()
        .expect("the delivered topology remains canonical");
}

#[test]
fn ogc_04_stale_window_facts_clear_preview_and_require_repaint() {
    let mut fixture = InteractionFixture::new();
    let lease = fixture.attach_native_surface();
    fixture.begin_drag_without_target();
    fixture.preview_target();
    fixture.paint_preview();
    let stale_target = fixture.current_target();
    let before = fixture.host.workspace().clone();

    let snapshot = fixture
        .host
        .session
        .capture_native_snapshot([(lease, NativeWindowFacts::unavailable())])
        .expect("unavailable facts are an explicit complete-roster tombstone");
    let invalidated = fixture.host.run(|frame| {
        frame
            .publish_native_snapshot(snapshot)
            .expect("the unavailable native facts join the host frame");
    });
    assert_eq!(invalidated.repaint_surfaces(), &[SURFACE]);
    assert!(
        fixture
            .host
            .session
            .bind_presented_receiver(&fixture.target)
            .is_none(),
        "the last receiver must lose authority with its native route facts"
    );

    fixture.host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the invalidated surface remains a valid frame member");
        assert!(
            plan.is_none() || plan.is_some_and(|plan| plan.drag_preview().is_none()),
            "the replacement paint may never carry the old routed preview"
        );
    });
    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer(
                SurfacePointerEvent::PrimaryReleased,
                stale_target.center(),
                SurfacePointerReceiverFacts::hover(&stale_target),
            )
            .expect("the facade degrades stale concrete receiver facts to Unknown");
    });
    assert_eq!(fixture.host.workspace(), &before);
}

#[test]
fn ogc_04_late_a1_close_cannot_mutate_same_token_a2_binding() {
    let (workspace, _) = tabs_workspace([A, B]);
    let mut host = DeterministicHost::new(workspace);
    host.session
        .enable_native_platform()
        .expect("the deterministic host enrolls one native platform provider");

    let registered_a1 = host.run(|frame| {
        frame
            .register_native_root(SURFACE, WINDOW)
            .expect("A1 registration joins the frame");
    });
    let a1 = match registered_a1.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { lease }] => *lease,
        outcomes => panic!("expected A1 registration, got {outcomes:?}"),
    };
    let destroyed = host
        .session
        .capture_native_snapshot([(a1, NativeWindowFacts::destroyed())])
        .expect("A1 destruction is one exact complete-roster fact");
    host.run(|frame| {
        frame
            .publish_native_snapshot(destroyed)
            .expect("A1 exact destruction joins the frame");
    });
    assert_eq!(host.session.native_surface(SURFACE), None);

    let registered_a2 = host.run(|frame| {
        frame
            .register_native_root(SURFACE, WINDOW)
            .expect("the destroyed token may be rebound as A2");
    });
    let a2 = match registered_a2.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { lease }] => *lease,
        outcomes => panic!("expected A2 registration, got {outcomes:?}"),
    };
    assert_ne!(a1, a2, "the core must mint a fresh binding incarnation");
    assert_eq!(a1.window_token(), a2.window_token());

    host.run(|frame| {
        frame
            .publish_native_close(a2, NativeCloseState::Clear)
            .expect("A2 establishes its independent close-generation namespace");
    });
    let physical =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the A2 physical bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the A2 scale is valid");
    let ready_a2 = host
        .session
        .capture_native_snapshot([(a2, NativeWindowFacts::ready(physical, scale))])
        .expect("the next snapshot advances beyond A2's close observation");
    host.run(|frame| {
        frame
            .publish_native_snapshot(ready_a2)
            .expect("A2 becomes the current routeable binding");
    });

    let before = host.workspace().clone();
    let mut frame = host
        .session
        .begin_host_frame()
        .expect("the late callback frame begins");
    let error = frame
        .publish_native_close(a1, NativeCloseState::Requested)
        .expect_err("the delayed A1 close must be rejected before reduction");
    assert!(matches!(
        error,
        dockspace::runtime::DockspaceRuntimeError::Native(NativePlatformError::StaleSurface {
            surface: SURFACE
        })
    ));
    frame
        .complete_unpainted_surfaces(MeasurementUnavailableReason::Deferred)
        .expect("the inert late callback does not poison the host frame");
    frame
        .commit()
        .expect("the frame containing only an inert A1 callback commits");

    assert_eq!(host.workspace(), &before);
    assert_eq!(host.session.native_surface(SURFACE), Some(a2));
}

#[test]
fn ogc_04_snapshot_captured_before_roster_change_is_rejected_at_publish() {
    let mut builder = Workspace::builder();
    let first_tabs = builder.insert_node(Node::tabs([A]));
    let second_tabs = builder.insert_node(Node::tabs([B]));
    builder.set_root(ROOT, RootRecord::new(first_tabs));
    builder.set_root(SOURCE_ROOT, RootRecord::new(second_tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(SECOND_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let mut host = DeterministicHost::new(
        builder
            .build()
            .expect("the two-surface native workspace is valid"),
    );
    host.session
        .enable_native_platform()
        .expect("the deterministic host enrolls one native platform provider");

    let first_registration = host.run(|frame| {
        frame
            .register_native_root(SURFACE, WINDOW)
            .expect("the first native surface joins the host frame");
    });
    let first = match first_registration.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { lease }] => *lease,
        outcomes => panic!("expected the first registration, got {outcomes:?}"),
    };
    let physical =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the physical bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the scale is valid");
    let stale_snapshot = host
        .session
        .capture_native_snapshot([(first, NativeWindowFacts::ready(physical, scale))])
        .expect("the snapshot exactly covers the roster at capture time");

    let second_registration = host.run(|frame| {
        frame
            .register_native_root(SECOND_SURFACE, SECOND_WINDOW)
            .expect("the second native surface changes the committed roster");
    });
    assert!(matches!(
        second_registration.inputs(),
        [HostInputOutcome::NativeSurfaceRegistered { lease }]
            if lease.surface() == SECOND_SURFACE
    ));

    let mut frame = host
        .session
        .begin_host_frame()
        .expect("the stale snapshot frame begins");
    let error = frame
        .publish_native_snapshot(stale_snapshot)
        .expect_err("a captured snapshot cannot omit a subsequently registered binding");
    assert!(matches!(
        error,
        dockspace::runtime::DockspaceRuntimeError::Native(NativePlatformError::SnapshotRosterStale)
    ));
    frame
        .complete_unpainted_surfaces(MeasurementUnavailableReason::Deferred)
        .expect("the typed stale-roster rejection does not poison the frame");
    frame
        .commit()
        .expect("the frame remains atomically committable after rejection");

    assert_eq!(host.session.native_surface(SURFACE), Some(first));
    assert!(host.session.native_surface(SECOND_SURFACE).is_some());
}
