use crate::close_plan::ClosePlanTarget;
use crate::geometry::LogicalRect;
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome,
    DockspaceActionRejection, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout,
};
use crate::runtime::{
    DockspaceHostFrame, DockspaceRuntimeErrorKind, DockspaceSession, HostCloseRequestOrigin,
    HostFrameReport, HostInputOutcome, SurfaceUnavailableReason,
};

const MAIN_SURFACE: SurfaceId = SurfaceId::new(10);
const ROOTLESS_SURFACE: SurfaceId = SurfaceId::new(20);
const MAIN_ROOT: RootId = RootId::new(100);
const CONTAINED_ROOT: RootId = RootId::new(200);

const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const FLOATING: ItemId = ItemId::new(3);
const NEW_ITEM: ItemId = ItemId::new(4);

fn session_with_policy(policy: crate::policy::DockPolicy) -> DockspaceSession {
    let main = DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    );
    let contained = DockspaceContainedLayout::new(
        FloatingPresentationId::new(1_000),
        DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FLOATING])),
        LogicalRect::new(10.0, 20.0, 320.0, 180.0).expect("test rect validates"),
    );
    let rootless = DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(contained);
    let layout = DockspaceLayout::new([main, rootless]).expect("product layout validates");

    DockspaceSession::from_layout(layout, policy).expect("product session initializes")
}

fn session() -> DockspaceSession {
    session_with_policy(crate::policy::DockPolicy::default())
}

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("test host answers the frozen surface roster");
    frame.commit().expect("product action frame commits")
}

#[test]
fn product_actions_select_open_and_dock_without_runtime_node_ids() {
    let mut session = session();

    let mut frame = session.begin_host_frame().expect("selection frame begins");
    frame.select_item_current(SECOND).expect("selection stages");
    let report = commit(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            },
        )]
    );
    assert_eq!(
        session
            .view()
            .root(MAIN_ROOT)
            .and_then(|root| root.central())
            .and_then(|central| central.tabs())
            .and_then(|tabs| tabs.selected()),
        Some(SECOND),
    );

    let mut frame = session.begin_host_frame().expect("open frame begins");
    frame
        .open_item_current(
            NEW_ITEM,
            DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
        )
        .expect("open stages");
    let report = commit(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Opened {
                item: NEW_ITEM,
                root: MAIN_ROOT,
            },
        )]
    );

    let mut frame = session
        .begin_host_frame()
        .expect("duplicate open frame begins");
    frame
        .open_item_current(NEW_ITEM, DockPlacement::Center(DockAnchor::Item(FIRST)))
        .expect("duplicate open stages");
    let report = commit(frame);
    assert_eq!(report.before(), report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Existing { item: NEW_ITEM },
        )]
    );

    let mut frame = session.begin_host_frame().expect("dock frame begins");
    frame
        .dock_item_current(
            FLOATING,
            DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
        )
        .expect("dock stages");
    let report = commit(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FLOATING,
                source_root: CONTAINED_ROOT,
                target_root: MAIN_ROOT,
                changed: true,
            },
        )]
    );
    let tabs = session
        .view()
        .root(MAIN_ROOT)
        .and_then(|root| root.central())
        .and_then(|central| central.tabs())
        .expect("central tabs remain queryable");
    assert_eq!(tabs.items(), &[FIRST, SECOND, NEW_ITEM, FLOATING]);
    assert!(session.view().root(CONTAINED_ROOT).is_none());
}

#[test]
fn docking_a_complete_root_preserves_group_order_and_selection() {
    const SOURCE_SURFACE: SurfaceId = SurfaceId::new(30);
    const SOURCE_ROOT: RootId = RootId::new(300);
    const SOURCE_FIRST: ItemId = ItemId::new(5);
    const SOURCE_SECOND: ItemId = ItemId::new(6);

    let target = DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(
            MAIN_ROOT,
            DockspaceNode::tabs_with_selection([FIRST, SECOND], Some(SECOND))
                .expect("target selection validates"),
        ),
    );
    let source = DockspaceSurfaceLayout::new(
        SOURCE_SURFACE,
        DockspaceRootLayout::new(
            SOURCE_ROOT,
            DockspaceNode::tabs_with_selection([SOURCE_FIRST, SOURCE_SECOND], Some(SOURCE_SECOND))
                .expect("source selection validates"),
        ),
    );
    let layout = DockspaceLayout::new([target, source]).expect("root-dock layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("root-dock session initializes");

    let mut frame = session.begin_host_frame().expect("root-dock frame begins");
    frame
        .dock_root_current(SOURCE_ROOT, DockPlacement::Center(DockAnchor::Item(SECOND)))
        .expect("complete-root docking stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootDocked {
                root: SOURCE_ROOT,
                target_root: MAIN_ROOT,
                items: vec![SOURCE_FIRST, SOURCE_SECOND],
                changed: true,
            },
        )]
    );
    let tabs = session
        .view()
        .root(MAIN_ROOT)
        .and_then(|root| root.content())
        .and_then(|content| content.tabs())
        .expect("merged tabs remain product-visible");
    assert_eq!(tabs.items(), &[FIRST, SECOND, SOURCE_FIRST, SOURCE_SECOND]);
    assert_eq!(tabs.selected(), Some(SOURCE_SECOND));
    let moved = session
        .view()
        .item(SOURCE_SECOND)
        .expect("the moved item has one stable product location");
    assert_eq!(moved.surface(), MAIN_SURFACE);
    assert_eq!(moved.root(), MAIN_ROOT);
    assert_eq!(moved.contained(), None);
    assert_eq!(moved.tab_index(), 3);
    assert!(moved.is_selected());
    assert!(session.view().root(SOURCE_ROOT).is_none());
    assert!(session.view().surface(SOURCE_SURFACE).is_none());
}

#[test]
fn prepared_root_docking_rejects_a_newer_workspace_revision() {
    const SOURCE_SURFACE: SurfaceId = SurfaceId::new(30);
    const SOURCE_ROOT: RootId = RootId::new(300);
    const SOURCE_ITEM: ItemId = ItemId::new(5);

    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            MAIN_SURFACE,
            DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            SOURCE_SURFACE,
            DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([SOURCE_ITEM])),
        ),
    ])
    .expect("prepared root layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("prepared root session initializes");
    let prepared =
        session.prepare_dock_root(SOURCE_ROOT, DockPlacement::Center(DockAnchor::Item(FIRST)));

    let mut frame = session.begin_host_frame().expect("stale root frame begins");
    frame
        .select_item_current(SECOND)
        .expect("selection advances the candidate revision");
    frame
        .submit_prepared_action(prepared)
        .expect("stale root action is structurally accepted");
    let report = commit(frame);

    assert!(matches!(
        report.inputs(),
        [
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }),
            HostInputOutcome::StaleRejected { .. },
        ]
    ));
    assert!(session.view().root(SOURCE_ROOT).is_some());
}

#[test]
fn root_action_promotes_a_contained_root_without_item_inference() {
    let mut session = session();
    let mut frame = session
        .begin_host_frame()
        .expect("contained root action frame begins");
    frame
        .dock_root_current(CONTAINED_ROOT, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("contained root promotion stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootDocked {
                root: CONTAINED_ROOT,
                target_root: CONTAINED_ROOT,
                items: vec![FLOATING],
                changed: true,
            },
        )]
    );
    let surface = session
        .view()
        .surface(ROOTLESS_SURFACE)
        .expect("promoted surface remains product-visible");
    assert_eq!(
        surface.main_root().map(|root| root.id()),
        Some(CONTAINED_ROOT)
    );
    assert_eq!(surface.contained_count(), 0);
}

#[test]
fn product_actions_resolve_anchors_against_the_same_frame_candidate() {
    let mut session = session();
    let mut frame = session.begin_host_frame().expect("compound frame begins");
    frame
        .open_item_current(
            NEW_ITEM,
            DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
        )
        .expect("open stages");
    frame
        .dock_item_current(NEW_ITEM, DockPlacement::Before(FIRST))
        .expect("move stages against the post-open candidate");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Opened {
                item: NEW_ITEM,
                root: MAIN_ROOT,
            }),
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Docked {
                item: NEW_ITEM,
                source_root: MAIN_ROOT,
                target_root: MAIN_ROOT,
                changed: true,
            }),
        ]
    );
    let tabs = session
        .view()
        .root(MAIN_ROOT)
        .and_then(|root| root.central())
        .and_then(|central| central.tabs())
        .expect("central tabs remain queryable");
    assert_eq!(tabs.items(), &[NEW_ITEM, FIRST, SECOND]);
}

#[test]
fn prepared_product_actions_reject_a_newer_same_frame_candidate() {
    let mut session = session();
    let expected = session.version();
    let prepared = session.prepare_dock_item(
        FLOATING,
        DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
    );

    let mut frame = session
        .begin_host_frame()
        .expect("prepared-action frame begins");
    frame
        .select_item_current(SECOND)
        .expect("current selection stages");
    frame
        .submit_prepared_action(prepared)
        .expect("stale prepared action stages structurally");
    let report = commit(frame);

    assert_eq!(report.before(), expected);
    assert_eq!(
        report.inputs(),
        &[
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }),
            HostInputOutcome::StaleRejected {
                expected,
                accepted: report.after(),
            },
        ]
    );
    assert!(session.view().root(CONTAINED_ROOT).is_some());
}

#[test]
fn prepared_product_actions_are_bound_to_the_preparing_session() {
    let source_session = session();
    let prepared = source_session.prepare_select_item(SECOND);
    let mut target_session = session();
    let mut frame = target_session
        .begin_host_frame()
        .expect("target frame begins");

    let error = frame
        .submit_prepared_action(prepared)
        .expect_err("foreign prepared action is rejected");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);

    frame
        .select_item_current(SECOND)
        .expect("frame remains usable after foreign capability rejection");
    let report = commit(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            },
        )]
    );
}

#[test]
fn docking_a_complete_contained_root_to_same_surface_main_preserves_root_identity() {
    let mut session = session();
    let mut frame = session
        .begin_host_frame()
        .expect("contained promotion frame begins");
    frame
        .dock_item_current(FLOATING, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("contained promotion stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FLOATING,
                source_root: CONTAINED_ROOT,
                target_root: CONTAINED_ROOT,
                changed: true,
            },
        )]
    );
    let surface = session
        .view()
        .surface(ROOTLESS_SURFACE)
        .expect("promoted surface remains available");
    assert_eq!(
        surface.main_root().map(|root| root.id()),
        Some(CONTAINED_ROOT)
    );
    assert_eq!(surface.contained_count(), 0);
}

#[test]
fn docking_part_of_a_root_to_main_allocates_a_new_root() {
    let mut session = session();
    let expected_root = RootId::new(CONTAINED_ROOT.get() + 1);
    let mut frame = session
        .begin_host_frame()
        .expect("partial main move frame begins");
    frame
        .dock_item_current(FIRST, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("partial main move stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FIRST,
                source_root: MAIN_ROOT,
                target_root: expected_root,
                changed: true,
            },
        )]
    );
    assert_eq!(
        session
            .view()
            .surface(ROOTLESS_SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(expected_root)
    );
    assert!(session.view().root(MAIN_ROOT).is_some());
}

#[test]
fn docking_a_complete_root_across_surfaces_rehomes_the_existing_root() {
    const TARGET_SURFACE: SurfaceId = SurfaceId::new(30);
    const TARGET_CONTAINED_ROOT: RootId = RootId::new(300);
    const TARGET_CONTAINED_ITEM: ItemId = ItemId::new(5);

    let main = DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    );
    let source = DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(1_000),
            DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FLOATING])),
            LogicalRect::new(10.0, 20.0, 320.0, 180.0).expect("test rect validates"),
        ),
    );
    let target = DockspaceSurfaceLayout::rootless(TARGET_SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(2_000),
            DockspaceRootLayout::new(
                TARGET_CONTAINED_ROOT,
                DockspaceNode::tabs([TARGET_CONTAINED_ITEM]),
            ),
            LogicalRect::new(40.0, 50.0, 240.0, 160.0).expect("target rect validates"),
        ),
    );
    let layout = DockspaceLayout::new([main, source, target]).expect("rehome layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("rehome session initializes");

    let mut frame = session.begin_host_frame().expect("rehome frame begins");
    frame
        .dock_item_current(FLOATING, DockPlacement::Main(TARGET_SURFACE))
        .expect("root rehome stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FLOATING,
                source_root: CONTAINED_ROOT,
                target_root: CONTAINED_ROOT,
                changed: true,
            },
        )]
    );
    assert_eq!(
        session
            .view()
            .surface(TARGET_SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(CONTAINED_ROOT)
    );
    assert!(session.view().surface(ROOTLESS_SURFACE).is_none());
}

#[test]
fn docking_a_complete_main_root_to_its_current_surface_is_a_noop() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([FIRST])),
    )])
    .expect("singleton main layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("singleton main session initializes");
    let before = session.version();

    let mut frame = session.begin_host_frame().expect("main no-op frame begins");
    frame
        .dock_item_current(FIRST, DockPlacement::Main(MAIN_SURFACE))
        .expect("main no-op stages");
    let report = commit(frame);

    assert_eq!(report.before(), before);
    assert_eq!(report.after(), before);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FIRST,
                source_root: MAIN_ROOT,
                target_root: MAIN_ROOT,
                changed: false,
            },
        )]
    );
}

#[test]
fn same_main_noop_still_uses_transaction_policy_authority() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([FIRST])),
    )])
    .expect("singleton main layout validates");
    let mut policy = crate::policy::DockPolicy::default();
    policy.set_allow_tiled_presentation(false);
    let mut session = DockspaceSession::from_layout(layout, policy)
        .expect("policy-restricted session initializes");

    let mut frame = session
        .begin_host_frame()
        .expect("policy no-op frame begins");
    frame
        .dock_item_current(FIRST, DockPlacement::Main(MAIN_SURFACE))
        .expect("same-main action stages structurally");
    let report = commit(frame);

    assert_eq!(report.before(), report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::PolicyDenied,
        )]
    );
}

#[test]
fn docking_a_complete_main_root_across_surfaces_preserves_root_identity() {
    let source = DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([FIRST])),
    );
    let target = DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(1_000),
            DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FLOATING])),
            LogicalRect::new(10.0, 20.0, 320.0, 180.0).expect("test rect validates"),
        ),
    );
    let layout = DockspaceLayout::new([source, target]).expect("main rehome layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("main rehome session initializes");

    let mut frame = session
        .begin_host_frame()
        .expect("main rehome frame begins");
    frame
        .dock_item_current(FIRST, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("main rehome stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FIRST,
                source_root: MAIN_ROOT,
                target_root: MAIN_ROOT,
                changed: true,
            },
        )]
    );
    assert_eq!(
        session
            .view()
            .surface(ROOTLESS_SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(MAIN_ROOT)
    );
    assert!(session.view().surface(MAIN_SURFACE).is_none());
}

#[test]
fn floating_part_of_a_root_allocates_fresh_product_identities() {
    let mut session = session();
    let rect = LogicalRect::new(40.0, 50.0, 240.0, 160.0).expect("float rect validates");
    let expected_root = RootId::new(CONTAINED_ROOT.get() + 1);
    let expected_floating = FloatingPresentationId::new(1_001);

    let mut frame = session.begin_host_frame().expect("float frame begins");
    frame
        .float_item_current(FIRST, MAIN_SURFACE, rect)
        .expect("partial-root float stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Floated {
                item: FIRST,
                root: expected_root,
                surface: MAIN_SURFACE,
                floating: expected_floating,
                changed: true,
            },
        )]
    );
    let item = session
        .view()
        .item(FIRST)
        .expect("floated item remains product-visible");
    assert_eq!(item.root(), expected_root);
    assert_eq!(item.surface(), MAIN_SURFACE);
    assert_eq!(item.contained(), Some(expected_floating));
    assert_eq!(
        session
            .view()
            .contained(expected_floating)
            .map(|contained| contained.rect()),
        Some(rect),
    );
    assert_eq!(
        session
            .view()
            .root(MAIN_ROOT)
            .and_then(|root| root.central())
            .and_then(|central| central.tabs())
            .map(|tabs| tabs.items()),
        Some([SECOND].as_slice()),
    );
}

#[test]
fn floating_a_singleton_main_root_preserves_root_identity() {
    let source = DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([FIRST])),
    );
    let target = DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(1_000),
            DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FLOATING])),
            LogicalRect::new(10.0, 20.0, 320.0, 180.0).expect("existing rect validates"),
        ),
    );
    let layout = DockspaceLayout::new([source, target]).expect("singleton float layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("singleton float session initializes");
    let rect = LogicalRect::new(60.0, 70.0, 260.0, 170.0).expect("float rect validates");
    let expected_floating = FloatingPresentationId::new(1_001);

    let mut frame = session
        .begin_host_frame()
        .expect("singleton float frame begins");
    frame
        .float_item_current(FIRST, ROOTLESS_SURFACE, rect)
        .expect("complete-root float stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Floated {
                item: FIRST,
                root: MAIN_ROOT,
                surface: ROOTLESS_SURFACE,
                floating: expected_floating,
                changed: true,
            },
        )]
    );
    let item = session
        .view()
        .item(FIRST)
        .expect("re-homed item remains product-visible");
    assert_eq!(item.root(), MAIN_ROOT);
    assert_eq!(item.contained(), Some(expected_floating));
    assert_eq!(item.surface(), ROOTLESS_SURFACE);
    assert_eq!(
        session
            .view()
            .contained(expected_floating)
            .map(|contained| contained.rect()),
        Some(rect),
    );
    assert!(session.view().surface(MAIN_SURFACE).is_none());
}

#[test]
fn contained_bounds_and_stacking_are_item_centric() {
    const FRONT_ROOT: RootId = RootId::new(300);
    const FRONT_ITEM: ItemId = ItemId::new(5);
    const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2_000);

    let rear = DockspaceContainedLayout::new(
        FloatingPresentationId::new(1_000),
        DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FLOATING])),
        LogicalRect::new(10.0, 20.0, 320.0, 180.0).expect("rear rect validates"),
    );
    let front = DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_ROOT, DockspaceNode::tabs([FRONT_ITEM])),
        LogicalRect::new(80.0, 90.0, 280.0, 170.0).expect("front rect validates"),
    );
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            MAIN_SURFACE,
            DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE)
            .with_contained(rear)
            .with_contained(front),
    ])
    .expect("contained action layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("contained action session initializes");
    let rect = LogicalRect::new(30.0, 40.0, 360.0, 220.0).expect("updated rect validates");

    let mut frame = session
        .begin_host_frame()
        .expect("contained action frame begins");
    frame
        .set_contained_rect_current(FLOATING, rect)
        .expect("contained bounds update stages");
    frame
        .raise_contained_current(FLOATING)
        .expect("contained raise stages");
    let report = commit(frame);

    assert_eq!(
        report.inputs(),
        &[
            HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::ContainedBoundsUpdated {
                    root: CONTAINED_ROOT,
                    surface: ROOTLESS_SURFACE,
                    floating: FloatingPresentationId::new(1_000),
                    changed: true,
                },
            ),
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::ContainedRaised {
                root: CONTAINED_ROOT,
                surface: ROOTLESS_SURFACE,
                floating: FloatingPresentationId::new(1_000),
                changed: true,
            }),
        ]
    );
    assert_eq!(
        session
            .view()
            .contained(FloatingPresentationId::new(1_000))
            .map(|contained| contained.rect()),
        Some(rect),
    );
    assert_eq!(
        session
            .view()
            .surface(ROOTLESS_SURFACE)
            .expect("contained surface remains available")
            .contained()
            .map(|contained| contained.id())
            .collect::<Vec<_>>(),
        vec![FRONT_FLOATING, FloatingPresentationId::new(1_000)],
    );

    let mut frame = session
        .begin_host_frame()
        .expect("contained no-op frame begins");
    frame
        .set_contained_rect_current(FLOATING, rect)
        .expect("same bounds stage");
    frame
        .raise_contained_current(FLOATING)
        .expect("same stacking stage");
    let report = commit(frame);
    assert_eq!(report.before(), report.after());
    assert!(report.inputs().iter().all(|outcome| matches!(
        outcome,
        HostInputOutcome::ProductActionApplied(action) if !action.changed()
    )));
}

#[test]
fn contained_product_actions_fail_closed_at_the_product_boundary() {
    let mut session = session();
    let rect = LogicalRect::new(40.0, 50.0, 240.0, 160.0).expect("test rect validates");
    let before = session.version();

    let mut frame = session
        .begin_host_frame()
        .expect("contained rejection frame begins");
    frame
        .set_contained_rect_current(FIRST, rect)
        .expect("non-contained bounds action stages structurally");
    frame
        .raise_contained_current(FIRST)
        .expect("non-contained raise stages structurally");
    frame
        .float_item_current(FIRST, SurfaceId::new(999), rect)
        .expect("missing-surface float stages structurally");
    let report = commit(frame);

    assert_eq!(report.before(), before);
    assert_eq!(report.after(), before);
    assert_eq!(
        report.inputs(),
        &[
            HostInputOutcome::ProductActionRejected(DockspaceActionRejection::ItemNotContained {
                item: FIRST
            },),
            HostInputOutcome::ProductActionRejected(DockspaceActionRejection::ItemNotContained {
                item: FIRST
            },),
            HostInputOutcome::ProductActionRejected(DockspaceActionRejection::SurfaceUnavailable {
                surface: SurfaceId::new(999),
            },),
        ]
    );
}

#[test]
fn rejected_main_placement_does_not_consume_the_next_root_identity() {
    let mut session = session();

    let mut frame = session.begin_host_frame().expect("rejected frame begins");
    frame
        .open_item_current(NEW_ITEM, DockPlacement::Main(MAIN_SURFACE))
        .expect("invalid product action still stages structurally");
    let report = commit(frame);
    assert_eq!(report.before(), report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::MainSurfaceUnavailable {
                surface: MAIN_SURFACE,
            },
        )]
    );

    let mut frame = session
        .begin_host_frame()
        .expect("missing root frame begins");
    frame
        .open_item_current(
            NEW_ITEM,
            DockPlacement::OuterEdge {
                root: RootId::new(999),
                edge: DockEdge::Left,
                fraction: DockFraction::new(0.25).expect("test fraction validates"),
            },
        )
        .expect("missing-root action stages structurally");
    let report = commit(frame);
    assert_eq!(report.before(), report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::RootUnavailable {
                root: RootId::new(999),
            },
        )]
    );

    let mut discarded = session.begin_host_frame().expect("discarded frame begins");
    discarded
        .open_item_current(NEW_ITEM, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("discarded main-root open stages");
    let expected_root = RootId::new(CONTAINED_ROOT.get() + 1);
    assert_eq!(
        discarded
            .view()
            .surface(ROOTLESS_SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(expected_root),
    );
    drop(discarded);
    assert!(
        session
            .view()
            .surface(ROOTLESS_SURFACE)
            .is_some_and(|surface| surface.main_root().is_none())
    );

    let mut frame = session.begin_host_frame().expect("accepted frame begins");
    frame
        .open_item_current(NEW_ITEM, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("main-root open stages");
    let report = commit(frame);
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Opened {
                item: NEW_ITEM,
                root: expected_root,
            },
        )]
    );
    assert_eq!(
        session
            .view()
            .surface(ROOTLESS_SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(expected_root),
    );
}

#[test]
fn same_frame_main_actions_receive_distinct_authoritative_root_identities() {
    const SECOND_ROOTLESS_SURFACE: SurfaceId = SurfaceId::new(30);
    const SECOND_CONTAINED_ROOT: RootId = RootId::new(300);
    const SECOND_CONTAINED_ITEM: ItemId = ItemId::new(5);
    const SECOND_NEW_ITEM: ItemId = ItemId::new(6);

    let main = DockspaceSurfaceLayout::new(
        MAIN_SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    );
    let first_rootless = DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(1_000),
            DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FLOATING])),
            LogicalRect::new(10.0, 20.0, 320.0, 180.0).expect("first test rect validates"),
        ),
    );
    let second_rootless = DockspaceSurfaceLayout::rootless(SECOND_ROOTLESS_SURFACE).with_contained(
        DockspaceContainedLayout::new(
            FloatingPresentationId::new(2_000),
            DockspaceRootLayout::new(
                SECOND_CONTAINED_ROOT,
                DockspaceNode::tabs([SECOND_CONTAINED_ITEM]),
            ),
            LogicalRect::new(30.0, 40.0, 240.0, 160.0).expect("second test rect validates"),
        ),
    );
    let layout = DockspaceLayout::new([main, first_rootless, second_rootless])
        .expect("two-rootless product layout validates");
    let mut session = DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("two-rootless session initializes");

    let mut frame = session
        .begin_host_frame()
        .expect("compound main frame begins");
    frame
        .open_item_current(NEW_ITEM, DockPlacement::Main(ROOTLESS_SURFACE))
        .expect("first main-root open stages");
    frame
        .open_item_current(
            SECOND_NEW_ITEM,
            DockPlacement::Main(SECOND_ROOTLESS_SURFACE),
        )
        .expect("second main-root open stages");
    let report = commit(frame);
    let first_root = RootId::new(SECOND_CONTAINED_ROOT.get() + 1);
    let second_root = RootId::new(first_root.get() + 1);

    assert_eq!(
        report.inputs(),
        &[
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Opened {
                item: NEW_ITEM,
                root: first_root,
            }),
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Opened {
                item: SECOND_NEW_ITEM,
                root: second_root,
            }),
        ]
    );
}

#[test]
fn product_action_policy_rejection_rolls_back_without_leaking_internal_errors() {
    let mut policy = crate::policy::DockPolicy::default();
    policy.set_allow_tab_merge(false);
    let mut session = session_with_policy(policy);
    let mut frame = session.begin_host_frame().expect("policy frame begins");
    frame
        .open_item_current(
            NEW_ITEM,
            DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
        )
        .expect("policy-rejected action stages structurally");
    let report = commit(frame);

    assert_eq!(report.before(), report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::PolicyDenied,
        )]
    );
    assert!(
        session
            .view()
            .root(MAIN_ROOT)
            .and_then(|root| root.central())
            .and_then(|central| central.tabs())
            .is_some_and(|tabs| !tabs.items().contains(&NEW_ITEM))
    );
}

#[test]
fn product_close_request_exposes_only_stable_item_identity() {
    let mut session = session();
    let mut frame = session.begin_host_frame().expect("close frame begins");
    frame
        .request_close_item(FIRST)
        .expect("item close request stages");
    let report = commit(frame);

    let [
        HostInputOutcome::CloseRequested {
            plan,
            reused,
            origin,
        },
    ] = report.inputs()
    else {
        panic!("one product close request outcome is expected")
    };
    assert!(!reused);
    assert_eq!(*origin, HostCloseRequestOrigin::Application);
    assert_eq!(plan.target(), ClosePlanTarget::Item { item: FIRST });
}
