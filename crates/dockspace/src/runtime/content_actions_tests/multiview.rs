use super::*;

use std::collections::{BTreeMap, BTreeSet};

use crate::model::{
    DockEdge, DockFraction, DockspaceAxis, DockspaceContainedLayout, FloatingPresentationId,
};

const FIFTH: ItemId = ItemId::new(5);
const CONTAINED_ROOT: RootId = RootId::new(3);
const CONTAINED_FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const ITEM_POOL: [ItemId; 8] = [
    FIRST,
    SECOND,
    THIRD,
    FOURTH,
    FIFTH,
    ItemId::new(6),
    ItemId::new(7),
    ItemId::new(8),
];

fn multiview_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
        )
        .with_contained(DockspaceContainedLayout::new(
            CONTAINED_FLOATING,
            DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([FIFTH])),
            LogicalRect::new(80.0, 80.0, 320.0, 240.0).unwrap(),
        )),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::central_tabs([THIRD, FOURTH])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    DockspaceSession::from_layout(layout, policy).unwrap()
}

struct ActionRng(u64);

impl ActionRng {
    fn index(&mut self, len: usize) -> usize {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        let len = u64::try_from(len).unwrap();
        usize::try_from(self.0 % len).unwrap()
    }
}

fn assert_multiview_invariants(
    session: &DockspaceSession,
    expected: &BTreeSet<ItemId>,
    trace: &str,
) {
    session.workspace().validate().expect(trace);
    let expected_counts: BTreeMap<_, _> = expected.iter().map(|item| (*item, 1)).collect();
    assert_eq!(
        session.workspace().item_multiset(),
        expected_counts,
        "{trace}"
    );
    let view = session.view();
    for &item in expected {
        let location = view.item(item).expect(trace);
        let surface = view.surface(location.surface()).expect(trace);
        let root = view.root(location.root()).expect(trace);
        if let Some(floating) = location.contained() {
            assert!(
                surface.contained().any(|window| window.id() == floating),
                "{trace}"
            );
            assert_eq!(
                view.contained(floating)
                    .and_then(|window| window.root().map(|root| root.id())),
                Some(root.id()),
                "{trace}"
            );
        } else {
            assert_eq!(
                surface.main_root().map(|main| main.id()),
                Some(root.id()),
                "{trace}"
            );
        }
    }
    for surface in view.surfaces() {
        assert!(
            !surface.is_rootless() || surface.contained_count() > 0,
            "{trace}"
        );
    }
}

fn commit_random(mut frame: DockspaceHostFrame<'_>, trace: &str) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect(trace);
    frame.commit().expect(trace)
}

fn measure_surfaces(session: &mut DockspaceSession, step: usize, trace: &str) {
    // Whole-root moves may legitimately retire a logical surface. Reopen that
    // host so the remaining walk continues to exercise multiview operations.
    for surface in [SURFACE, OTHER_SURFACE] {
        if session.view().surface(surface).is_none() {
            let root = RootId::new(
                session
                    .workspace()
                    .roots()
                    .map(|(root, _)| root.get())
                    .max()
                    .unwrap()
                    + 1,
            );
            let items = session.workspace().item_multiset();
            let mut frame = session.begin_host_frame().expect(trace);
            let action = frame
                .attach_empty_surface_current(surface, root)
                .expect(trace);
            let report = commit_random(frame, trace);
            assert!(
                matches!(report.submitted_action_outcome(action), Some(HostInputOutcome::LogicalSurfaceAttached {
                surface: actual_surface, root: actual_root, ..
            }) if *actual_surface == surface && *actual_root == root),
                "{trace}: {report:?}"
            );
            assert_eq!(session.workspace().item_multiset(), items, "{trace}");
        }
    }
    // A small viewport must still accept topology changes when content cannot fit.
    let (width, height) = [(800.0, 600.0), (96.0, 64.0), (320.0, 120.0)][step % 3];
    let surfaces: Vec<_> = session
        .view()
        .surfaces()
        .map(|surface| surface.id())
        .collect();
    let mut frame = session.begin_host_frame().expect(trace);
    for surface in surfaces {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, width, height).unwrap(),
                    LogicalSize::new(128.0, 96.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .expect(trace);
    }
    commit_random(frame, trace);
}

fn apply_random_action(
    session: &mut DockspaceSession,
    prepared: PreparedDockAction,
    trace: &str,
) -> Option<DockspaceActionOutcome> {
    let before = session.workspace().clone();
    let version = session.version();
    let mut frame = session.begin_host_frame().expect(trace);
    let action = frame.submit_prepared_action(prepared).expect(trace);
    let report = commit_random(frame, trace);
    match report.submitted_action_outcome(action).expect(trace) {
        HostInputOutcome::ProductActionApplied(outcome) => {
            let changed = session.workspace() != &before;
            assert_eq!(outcome.changed(), changed, "{trace}");
            assert_eq!(report.workspace_changed(), changed, "{trace}");
            assert_eq!(session.version() != version, changed, "{trace}");
            Some(outcome.clone())
        }
        HostInputOutcome::ProductActionRejected(_) => {
            assert_eq!(session.workspace(), &before, "{trace}");
            assert_eq!(session.version(), version, "{trace}");
            None
        }
        outcome => panic!("{trace}: unexpected outcome {outcome:?}"),
    }
}

struct GroupExpectation {
    members: Vec<ItemId>,
    selected: Option<ItemId>,
    source: SurfaceId,
    target: SurfaceId,
    target_items: Vec<ItemId>,
}

impl GroupExpectation {
    fn assert_applied(
        &self,
        session: &DockspaceSession,
        outcome: &DockspaceActionOutcome,
        trace: &str,
    ) -> bool {
        let DockspaceActionOutcome::TabsDocked { items, changed, .. } = outcome else {
            panic!("{trace}: expected complete group docking, got {outcome:?}");
        };
        assert!(*changed, "{trace}");
        assert_eq!(items, &self.members, "{trace}");
        let mut target_items = self.target_items.clone();
        target_items.extend_from_slice(&self.members);
        for item in &self.members {
            let view = session.view();
            let location = view.item(*item).expect(trace);
            assert_eq!(location.surface(), self.target, "{trace}");
            assert_eq!(location.tabs().items(), target_items, "{trace}");
            assert_eq!(location.tabs().selected(), self.selected, "{trace}");
        }
        self.members.len() > 1 && self.source != self.target
    }
}

#[test]
fn seeded_multiview_actions_preserve_item_ownership_and_surface_liveness() {
    for seed in [
        0x4d55_4c54_4956_4945,
        0x5eed,
        0xdead_beef,
        0x0123_4567_89ab_cdef,
    ] {
        let mut session = multiview_session();
        let mut expected: BTreeSet<_> = [FIRST, SECOND, THIRD, FOURTH, FIFTH].into_iter().collect();
        let mut rng = ActionRng(seed);
        let mut changed = [0; 12];
        let mut rejected = 0;
        let mut migrations = [0; 2];
        let mut group_migrations = 0;
        let mut closed_once = BTreeSet::new();
        let mut reopened = 0;

        for step in 0..512 {
            let kind = rng.index(12);
            let trace = format!("seed={seed:#x}, step={step}, operation={kind}");
            measure_surfaces(&mut session, step, &trace);
            let items: Vec<_> = expected.iter().copied().collect();
            let source = items[rng.index(items.len())];
            let targets: Vec<_> = items
                .iter()
                .copied()
                .filter(|item| *item != source)
                .collect();
            let target = targets[rng.index(targets.len())];
            let before_owners: Vec<_> = items
                .iter()
                .map(|item| (*item, session.view().item(*item).unwrap().surface()))
                .collect();
            let anchor = DockAnchor::Item(target);
            let fraction =
                DockFraction::new([0.001, 0.25, 0.5, 0.75, 0.999][rng.index(5)]).unwrap();
            let edge = [
                DockEdge::Left,
                DockEdge::Right,
                DockEdge::Top,
                DockEdge::Bottom,
            ][rng.index(4)];
            let edge_placement = if rng.index(2) == 0 {
                DockPlacement::InnerEdge {
                    anchor,
                    edge,
                    fraction,
                }
            } else {
                DockPlacement::OuterEdge {
                    root: session.view().item(target).unwrap().root(),
                    edge,
                    fraction,
                }
            };
            let placement = if rng.index(2) == 0 {
                DockPlacement::Center(anchor)
            } else {
                edge_placement
            };
            let surfaces: Vec<_> = session
                .view()
                .surfaces()
                .map(|surface| surface.id())
                .collect();
            let surface = surfaces[rng.index(surfaces.len())];
            let trace = format!(
                "{trace}, source={source:?}, target={target:?}, surface={surface:?}, placement={placement:?}"
            );

            if kind == 10 {
                let before = session.workspace().clone();
                let version = session.version();
                let mut abandoned = session.begin_host_frame().expect(&trace);
                abandoned
                    .open_item_current(ItemId::new(10_000 + step as u64), placement)
                    .expect(&trace);
                assert_ne!(abandoned.version(), version, "{trace}");
                drop(abandoned);
                assert_eq!(session.workspace(), &before, "{trace}");
                assert_eq!(session.version(), version, "{trace}");
                changed[kind] += 1;
            } else if kind == 7 && expected.len() > 2 {
                let mut request = session.begin_host_frame().expect(&trace);
                let action = request.request_close_item_current(source).expect(&trace);
                let plan = requested_plan(&commit_random(request, &trace), action);
                let mut decision = session.begin_host_frame().expect(&trace);
                let action = decision
                    .resolve_close(
                        plan.request(),
                        plan.items()[0].token(),
                        CloseDecision::Allow,
                    )
                    .expect(&trace);
                let report = commit_random(decision, &trace);
                assert!(
                    matches!(report.submitted_action_outcome(action), Some(HostInputOutcome::CloseDecisionProcessed {
                    application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item, .. })), changed: true, ..
                }) if *item == source),
                    "{trace}: {report:?}"
                );
                assert!(expected.remove(&source));
                closed_once.insert(source);
                changed[kind] += 1;
            } else {
                let closed: Vec<_> = ITEM_POOL
                    .into_iter()
                    .filter(|item| !expected.contains(item))
                    .collect();
                let opening = if matches!(kind, 7 | 8) && !closed.is_empty() {
                    Some(closed[rng.index(closed.len())])
                } else {
                    None
                };
                let mut group_expectation = None;
                let prepared = match kind {
                    0 => session.prepare_select_item(source),
                    1 => session.prepare_dock_item(source, DockPlacement::Center(anchor)),
                    2 => session.prepare_dock_item(source, edge_placement),
                    3 => {
                        let grouped: Vec<_> = items
                            .iter()
                            .copied()
                            .filter(|item| {
                                session.view().item(*item).unwrap().tabs().items().len() > 1
                            })
                            .collect();
                        let source = if grouped.is_empty() {
                            source
                        } else {
                            grouped[rng.index(grouped.len())]
                        };
                        let owner = session.view().item(source).unwrap().surface();
                        let destinations: Vec<_> = session
                            .view()
                            .surfaces()
                            .filter(|surface| surface.id() != owner)
                            .map(|surface| {
                                let destination = surface
                                    .main_root()
                                    .and_then(|root| {
                                        root.central().map(|central| {
                                            (
                                                DockAnchor::Central(root.id()),
                                                central.tabs().unwrap().items().to_vec(),
                                            )
                                        })
                                    })
                                    .unwrap_or_else(|| {
                                        let target = items
                                            .iter()
                                            .copied()
                                            .find(|item| {
                                                session.view().item(*item).unwrap().surface()
                                                    == surface.id()
                                            })
                                            .expect(&trace);
                                        (
                                            DockAnchor::Item(target),
                                            session
                                                .view()
                                                .item(target)
                                                .unwrap()
                                                .tabs()
                                                .items()
                                                .to_vec(),
                                        )
                                    });
                                (surface.id(), destination)
                            })
                            .collect();
                        let (target, (anchor, target_items)) =
                            &destinations[rng.index(destinations.len())];
                        let location = session.view().item(source).unwrap();
                        group_expectation = Some(GroupExpectation {
                            members: location.tabs().items().to_vec(),
                            selected: location.tabs().selected(),
                            source: owner,
                            target: *target,
                            target_items: target_items.clone(),
                        });
                        let group = session
                            .capture_tabs_group(DockAnchor::Item(source))
                            .expect(&trace);
                        session.prepare_dock_tabs(group, DockPlacement::Center(*anchor))
                    }
                    4 => session
                        .prepare_dock_root(session.view().item(source).unwrap().root(), placement),
                    5 => session.prepare_float_item(source, surface),
                    6 => {
                        let group = session
                            .capture_tabs_group(DockAnchor::Item(source))
                            .expect(&trace);
                        session.prepare_float_tabs(group, surface)
                    }
                    7 | 8 => session.prepare_open_item(opening.unwrap_or(source), placement),
                    9 => session.prepare_dock_item(
                        source,
                        DockPlacement::Center(DockAnchor::Item(ItemId::new(999))),
                    ),
                    11 => session.prepare_bring_contained_into_view(source),
                    _ => session.prepare_open_item(source, placement),
                };
                let trace = format!("{trace}, action={prepared:?}");
                match apply_random_action(&mut session, prepared, &trace) {
                    Some(outcome) => {
                        assert_ne!(kind, 9, "missing anchor must be rejected: {trace}");
                        changed[kind] += usize::from(outcome.changed());
                        if let Some(group) = group_expectation {
                            group_migrations +=
                                usize::from(group.assert_applied(&session, &outcome, &trace));
                        }
                        if let DockspaceActionOutcome::Opened { item, .. } = outcome {
                            assert_eq!(Some(item), opening, "{trace}");
                            assert!(expected.insert(item), "{trace}");
                            reopened += usize::from(closed_once.contains(&item));
                        }
                    }
                    None => rejected += 1,
                }
            }

            assert_multiview_invariants(&session, &expected, &trace);
            assert_eq!(session.pending_content_close_plans().count(), 0, "{trace}");
            for (item, before) in before_owners {
                if let Some(location) = session.view().item(item) {
                    if before == SURFACE && location.surface() == OTHER_SURFACE {
                        migrations[0] += 1;
                    } else if before == OTHER_SURFACE && location.surface() == SURFACE {
                        migrations[1] += 1;
                    }
                }
            }
        }

        // Reject-only runs and a degenerate single-tabs workload must not pass.
        for kind in [1, 2, 3, 4, 5, 6, 7, 8, 10] {
            assert!(
                changed[kind] > 0,
                "seed={seed:#x}, operation={kind} never changed state: {changed:?}"
            );
        }
        assert!(rejected > 0, "seed={seed:#x}");
        assert!(
            group_migrations > 0,
            "seed={seed:#x}: no multi-item group crossed surfaces"
        );
        assert!(reopened > 0, "seed={seed:#x}: no closed item was reopened");
        assert!(
            migrations.into_iter().all(|count| count > 0),
            "seed={seed:#x}: missing bidirectional migrations {migrations:?}"
        );
    }
}

#[test]
fn multiview_root_lifecycle_boundaries_are_explicit_and_atomic() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        ),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::central_tabs([SECOND])),
        ),
    ])
    .unwrap();
    let mut main_central = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let prepared =
        main_central.prepare_dock_item(FIRST, DockPlacement::Center(DockAnchor::Item(SECOND)));
    assert!(apply_action(&mut main_central, prepared));
    let view = main_central.view();
    let main_root = view.surface(SURFACE).unwrap().main_root().unwrap();
    assert!(main_root.retains_empty_central_root());
    assert!(
        main_root
            .central()
            .and_then(|node| node.tabs())
            .is_some_and(|tabs| tabs.items().is_empty())
    );
    assert!(view.root(ROOT).is_some());

    let nested_source = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::central_tabs([]),
            DockspaceNode::tabs([THIRD]),
        ],
    )
    .unwrap();
    let nested_layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        ),
        DockspaceSurfaceLayout::rootless(OTHER_SURFACE).with_contained(
            DockspaceContainedLayout::new(
                CONTAINED_FLOATING,
                DockspaceRootLayout::new(CONTAINED_ROOT, nested_source),
                LogicalRect::new(80.0, 80.0, 320.0, 240.0).unwrap(),
            ),
        ),
    ])
    .unwrap();
    let mut contained_nested =
        DockspaceSession::from_layout(nested_layout, DockPolicy::default()).unwrap();
    let prepared =
        contained_nested.prepare_dock_item(THIRD, DockPlacement::Center(DockAnchor::Item(FIRST)));
    assert!(apply_action(&mut contained_nested, prepared));
    let view = contained_nested.view();
    assert!(view.root(CONTAINED_ROOT).is_none());
    assert!(view.surface(OTHER_SURFACE).is_none());
    assert!(view.item(THIRD).is_some());

    let close_layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )])
    .unwrap();
    let mut close_session =
        DockspaceSession::from_layout(close_layout, DockPolicy::default()).unwrap();
    let prepared = close_session.prepare_close_item(FIRST);
    let mut request_frame = close_session.begin_host_frame().unwrap();
    let request = request_frame
        .submit_prepared_close_request(prepared)
        .unwrap();
    let report = commit(request_frame);
    let plan = match report.submitted_action_outcome(request) {
        Some(HostInputOutcome::CloseRequested { plan, .. }) => plan.clone(),
        outcome => panic!("expected a close plan, got {outcome:?}"),
    };
    let mut decision_frame = close_session.begin_host_frame().unwrap();
    let decision = decision_frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    let report = commit(decision_frame);
    assert!(matches!(
        report.submitted_action_outcome(decision),
        Some(HostInputOutcome::CloseDecisionProcessed {
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: FIRST, .. })),
            changed: true,
            ..
        })
    ));
    let view = close_session.view();
    assert!(view.root(ROOT).is_some());
    assert!(
        view.root(ROOT)
            .and_then(|root| root.central())
            .and_then(|node| node.tabs())
            .is_some_and(|tabs| tabs.items().is_empty())
    );
}
