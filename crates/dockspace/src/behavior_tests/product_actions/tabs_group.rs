use super::*;

fn split_session(source_is_central: bool) -> DockspaceSession {
    let source = if source_is_central {
        DockspaceNode::central_tabs_with_selection([FIRST, SECOND], Some(SECOND))
    } else {
        DockspaceNode::tabs_with_selection([FIRST, SECOND], Some(SECOND))
    }
    .expect("source selection validates");
    let remainder = if source_is_central {
        DockspaceNode::tabs([FLOATING, NEW_ITEM])
    } else {
        DockspaceNode::central_tabs([FLOATING, NEW_ITEM])
    };
    let content = DockspaceNode::equal_split(DockspaceAxis::Horizontal, [source, remainder])
        .expect("the source group and its sibling form a valid split");
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(MAIN_SURFACE, DockspaceRootLayout::new(MAIN_ROOT, content)),
        DockspaceSurfaceLayout::rootless(ROOTLESS_SURFACE).with_contained(
            DockspaceContainedLayout::new(
                FloatingPresentationId::new(1_000),
                DockspaceRootLayout::new(CONTAINED_ROOT, DockspaceNode::tabs([ItemId::new(5)])),
                LogicalRect::new(10.0, 20.0, 320.0, 180.0).unwrap(),
            ),
        ),
    ])
    .expect("the group-action layout validates");
    DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default()).unwrap()
}

#[test]
fn docking_a_captured_group_within_one_root_preserves_order_and_selection() {
    let mut session = split_session(false);
    let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    let submitted = frame
        .dock_tabs_current(group, DockPlacement::After(FLOATING))
        .unwrap();
    let report = commit(frame);

    assert_one_revision_advance(before, report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::TabsDocked {
                source_root: MAIN_ROOT,
                target_root: MAIN_ROOT,
                items: vec![FIRST, SECOND],
                changed: true,
            },
        )]
    );
    assert!(report.submitted_action_outcome(submitted).is_some());
    let central = session.view().root(MAIN_ROOT).unwrap().central().unwrap();
    let tabs = central.tabs().unwrap();
    assert_eq!(tabs.items(), &[FLOATING, FIRST, SECOND, NEW_ITEM]);
    assert_eq!(tabs.selected(), Some(SECOND));
    assert_eq!(session.view().item(SECOND).unwrap().root(), MAIN_ROOT);
}

#[test]
fn docking_a_partial_group_across_surfaces_preserves_the_remaining_central_area() {
    let target_item = ItemId::new(5);
    for install_main in [false, true] {
        let mut session = split_session(false);
        let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
        let placement = if install_main {
            DockPlacement::Main(ROOTLESS_SURFACE)
        } else {
            DockPlacement::Center(DockAnchor::Item(target_item))
        };
        let prepared = session.prepare_dock_tabs(group, placement);
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_prepared_action(prepared).unwrap();
        let report = commit(frame);
        assert_one_revision_advance(before, report.after());

        let target_root = session.view().item(SECOND).unwrap().root();
        assert_ne!(target_root, MAIN_ROOT);
        assert_eq!(target_root == CONTAINED_ROOT, !install_main);
        assert_eq!(
            report.inputs(),
            &[HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::TabsDocked {
                    source_root: MAIN_ROOT,
                    target_root,
                    items: vec![FIRST, SECOND],
                    changed: true,
                },
            )]
        );
        let moved = session.view().item(SECOND).unwrap();
        assert_eq!(moved.surface(), ROOTLESS_SURFACE);
        assert_eq!(moved.tabs().selected(), Some(SECOND));
        let expected_items = if install_main {
            vec![FIRST, SECOND]
        } else {
            vec![target_item, FIRST, SECOND]
        };
        assert_eq!(moved.tabs().items(), expected_items);
        let central = session.view().root(MAIN_ROOT).unwrap().central().unwrap();
        assert_eq!(central.tabs().unwrap().items(), &[FLOATING, NEW_ITEM]);
    }
}

#[test]
fn preparing_or_submitting_a_group_again_never_refreshes_its_captured_version() {
    for float in [false, true] {
        for current in [false, true] {
            let mut session = session();
            let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
            let captured = group.expected_version();
            let mut move_anchor = session.begin_host_frame().unwrap();
            move_anchor
                .dock_item_current(FIRST, DockPlacement::After(FLOATING))
                .unwrap();
            commit(move_anchor);
            let accepted = session.version();
            assert_ne!(captured, accepted);

            let prepared = if float {
                session.prepare_float_tabs(group.clone(), ROOTLESS_SURFACE)
            } else {
                session.prepare_dock_tabs(group.clone(), DockPlacement::After(FLOATING))
            };
            assert_eq!(prepared.expected_version(), captured);
            let mut frame = session.begin_host_frame().unwrap();
            if current {
                if float {
                    frame.float_tabs_current(group, ROOTLESS_SURFACE).unwrap();
                } else {
                    frame
                        .dock_tabs_current(group, DockPlacement::After(FLOATING))
                        .unwrap();
                }
            } else {
                frame.submit_prepared_action(prepared).unwrap();
            }
            let report = commit(frame);
            assert_eq!(report.before(), report.after());
            assert_eq!(
                report.inputs(),
                &[HostInputOutcome::StaleRejected {
                    expected: captured,
                    accepted,
                }]
            );
            assert_eq!(session.view().item(FIRST).unwrap().root(), CONTAINED_ROOT);
            assert_eq!(session.view().item(SECOND).unwrap().root(), MAIN_ROOT);
            assert_eq!(
                session.view().item(FIRST).unwrap().tabs().items(),
                &[FLOATING, FIRST]
            );
        }
    }
}

#[test]
fn group_actions_keep_the_capturing_session_authority() {
    for float in [false, true] {
        for current in [false, true] {
            let source = session();
            let group = source.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
            let mut target = session();
            let prepared = if float {
                target.prepare_float_tabs(group.clone(), ROOTLESS_SURFACE)
            } else {
                target.prepare_dock_tabs(group.clone(), DockPlacement::After(FLOATING))
            };
            let mut frame = target.begin_host_frame().unwrap();
            let result = if current {
                if float {
                    frame.float_tabs_current(group, ROOTLESS_SURFACE)
                } else {
                    frame.dock_tabs_current(group, DockPlacement::After(FLOATING))
                }
            } else {
                frame.submit_prepared_action(prepared)
            };
            assert_eq!(
                result.unwrap_err().kind(),
                DockspaceRuntimeErrorKind::OperationConflict
            );
            frame.select_item_current(SECOND).unwrap();
            let report = commit(frame);
            assert!(matches!(
                report.inputs(),
                [HostInputOutcome::ProductActionApplied(
                    DockspaceActionOutcome::Selected {
                        item: SECOND,
                        changed: true,
                    }
                )]
            ));
            assert_eq!(
                source.view().item(FIRST).unwrap().tabs().items(),
                &[FIRST, SECOND]
            );
            assert_eq!(target.view().item(FIRST).unwrap().root(), MAIN_ROOT);
        }
    }
}

#[test]
fn floating_one_group_keeps_siblings_and_the_original_central_leaf() {
    for source_is_central in [false, true] {
        let mut session = split_session(source_is_central);
        present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
        let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
        let before = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        frame.float_tabs_current(group, ROOTLESS_SURFACE).unwrap();
        let report = commit(frame);
        assert_one_revision_advance(before, report.after());
        let (root, floating) = match report.inputs() {
            [
                HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::TabsFloated {
                    source_root: MAIN_ROOT,
                    root,
                    surface: ROOTLESS_SURFACE,
                    floating,
                    items,
                    changed: true,
                }),
            ] if items == &[FIRST, SECOND] => (*root, *floating),
            inputs => panic!("one complete group must float atomically: {inputs:?}"),
        };
        assert_ne!(root, MAIN_ROOT);
        let moved = session.view().item(SECOND).unwrap();
        assert_eq!(moved.root(), root);
        assert_eq!(moved.contained(), Some(floating));
        assert_eq!(moved.tabs().items(), &[FIRST, SECOND]);
        assert_eq!(moved.tabs().selected(), Some(SECOND));
        let central = session.view().root(MAIN_ROOT).unwrap().central().unwrap();
        assert_eq!(
            central.tabs().unwrap().items(),
            if source_is_central {
                &[][..]
            } else {
                &[FLOATING, NEW_ITEM][..]
            }
        );
        assert_eq!(
            session.view().item(FLOATING).unwrap().tabs().items(),
            &[FLOATING, NEW_ITEM]
        );
        assert_eq!(session.view().item(FLOATING).unwrap().root(), MAIN_ROOT);
    }
}

#[test]
fn floating_and_restoring_a_complete_group_preserves_its_root_identity() {
    let mut session = session();
    let mut select = session.begin_host_frame().unwrap();
    select.select_item_current(SECOND).unwrap();
    commit(select);
    present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
    let group = session
        .capture_tabs_group(DockAnchor::Central(MAIN_ROOT))
        .unwrap();
    let prepared = session.prepare_float_tabs(group, ROOTLESS_SURFACE);
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_action(prepared).unwrap();
    let floated = commit(frame);
    assert_one_revision_advance(before, floated.after());
    assert!(matches!(
        floated.inputs(),
        [HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::TabsFloated {
            source_root: MAIN_ROOT,
            root: MAIN_ROOT,
            surface: ROOTLESS_SURFACE,
            items,
            changed: true,
            ..
        })] if items == &[FIRST, SECOND]
    ));
    assert_eq!(
        session.view().item(SECOND).unwrap().tabs().selected(),
        Some(SECOND)
    );

    let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    let prepared = session.prepare_dock_tabs(group, DockPlacement::Main(ROOTLESS_SURFACE));
    let before = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_prepared_action(prepared).unwrap();
    let docked = commit(frame);
    assert_one_revision_advance(before, docked.after());
    assert_eq!(
        docked.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::TabsDocked {
                source_root: MAIN_ROOT,
                target_root: MAIN_ROOT,
                items: vec![FIRST, SECOND],
                changed: true,
            }
        )]
    );
    assert_eq!(
        session
            .view()
            .surface(ROOTLESS_SURFACE)
            .unwrap()
            .main_root()
            .unwrap()
            .id(),
        MAIN_ROOT
    );
    assert_eq!(
        session.view().item(SECOND).unwrap().tabs().selected(),
        Some(SECOND)
    );
    assert!(session.view().item(SECOND).unwrap().contained().is_none());
}

#[test]
fn complete_native_group_actions_wait_for_the_exact_target_presentation() {
    for float in [false, true] {
        let (mut session, child) = live_managed_native_root();
        let group = session
            .capture_tabs_group(DockAnchor::Item(NATIVE_FIRST))
            .unwrap();
        let prepared = if float {
            session.prepare_float_tabs(group, MAIN_SURFACE)
        } else {
            session.prepare_dock_tabs(group, DockPlacement::After(TARGET_FIRST))
        };
        let before = session.version();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame.submit_prepared_action(prepared).unwrap();
        let staged = commit(frame);
        assert_eq!(staged.before(), staged.after());
        assert!(session.has_pending_presentation());
        assert_eq!(
            session.view().item(NATIVE_SECOND).unwrap().surface(),
            NATIVE_SOURCE_SURFACE
        );
        let transition = match staged.inputs() {
            [
                HostInputOutcome::ProductPresentationActionRequested {
                    outcome,
                    transition,
                },
            ] => {
                assert!(matches!(
                    outcome,
                    DockspaceActionOutcome::RootFloatRequested { items, .. }
                        | DockspaceActionOutcome::RootDockRequested { items, .. }
                        if items == &[NATIVE_FIRST, NATIVE_SECOND]
                ));
                *transition
            }
            inputs => {
                panic!("the complete native group must retain a presentation barrier: {inputs:?}")
            }
        };
        let mut settled =
            present_surfaces_with_report(&mut session, &[MAIN_SURFACE, NATIVE_SOURCE_SURFACE]);
        assert_one_revision_advance(before, settled.after());
        assert!(!session.has_pending_presentation());
        let [terminal] = settled.presentation_transitions() else {
            panic!("the exact group transfer must settle once");
        };
        assert_eq!(terminal.id(), transition);
        assert_eq!(
            terminal.result(),
            DockspacePresentationTransitionResult::Applied
        );
        if float {
            assert!(matches!(
                terminal.outcome(),
                Some(DockspaceActionOutcome::TabsFloated {
                    source_root: NATIVE_SOURCE_ROOT,
                    root: NATIVE_SOURCE_ROOT,
                    surface: MAIN_SURFACE,
                    items,
                    changed: true,
                    ..
                }) if items == &[NATIVE_FIRST, NATIVE_SECOND]
            ));
            assert_eq!(
                session.view().item(NATIVE_SECOND).unwrap().tabs().items(),
                &[NATIVE_FIRST, NATIVE_SECOND]
            );
        } else {
            assert_eq!(
                terminal.outcome(),
                Some(&DockspaceActionOutcome::TabsDocked {
                    source_root: NATIVE_SOURCE_ROOT,
                    target_root: MAIN_ROOT,
                    items: vec![NATIVE_FIRST, NATIVE_SECOND],
                    changed: true,
                })
            );
            assert_eq!(
                session.view().item(NATIVE_SECOND).unwrap().tabs().items(),
                &[TARGET_FIRST, NATIVE_FIRST, NATIVE_SECOND, TARGET_SECOND]
            );
        }
        assert_eq!(
            session.view().item(NATIVE_SECOND).unwrap().surface(),
            MAIN_SURFACE
        );
        assert_eq!(
            session
                .view()
                .item(NATIVE_SECOND)
                .unwrap()
                .tabs()
                .selected(),
            Some(NATIVE_SECOND)
        );
        assert!(matches!(
            settled.take_native_effects().as_slice(),
            [effect] if matches!(effect.operation(), NativeEffectOperation::ReleaseChild { binding } if *binding == child)
        ));
    }
}
