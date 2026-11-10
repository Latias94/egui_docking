use super::*;
use crate::runtime::{
    DockspaceFocusTarget, DockspacePaneFocusObservation, DockspacePaneFocusRequest,
    NativeGlobalFocus,
};

const OTHER_SURFACE: SurfaceId = SurfaceId::new(40);
const OTHER_ROOT: RootId = RootId::new(400);

fn root_focus_session(selected: ItemId) -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            MAIN_SURFACE,
            DockspaceRootLayout::new(
                MAIN_ROOT,
                DockspaceNode::tabs_with_selection([FIRST, SECOND], Some(selected))
                    .expect("source selection validates"),
            ),
        ),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::tabs([FLOATING])),
        ),
    ])
    .expect("root focus layout validates");
    DockspaceSession::from_layout(layout, crate::policy::DockPolicy::default())
        .expect("root focus session initializes")
}

fn pane_focus_request(
    session: &mut DockspaceSession,
    surface: SurfaceId,
) -> Option<DockspacePaneFocusRequest> {
    let before = session.version();
    let mut frame = session
        .begin_host_frame()
        .expect("focus inspection frame begins");
    let request = frame
        .paint_plan(surface)
        .expect("focus paint plan resolves")
        .expect("the focus surface has a ready candidate")
        .pane_focus_request();
    let report = commit(frame);
    assert_eq!(report.before(), before);
    assert_eq!(report.after(), before);
    request
}

fn observe_pane_focus(
    session: &mut DockspaceSession,
    request: DockspacePaneFocusRequest,
    observation: DockspacePaneFocusObservation,
) {
    let before = session.version();
    let mut frame = session
        .begin_host_frame()
        .expect("focus observation frame begins");
    let plan = frame
        .paint_plan(request.surface())
        .expect("observation paint plan resolves")
        .expect("the requested surface remains paintable");
    assert_eq!(plan.pane_focus_request(), Some(request));
    let prepared = plan
        .prepare_pane_focus_observation(request, observation)
        .expect("the exact pending request accepts an observation");
    frame
        .submit_pane_focus_observation(prepared)
        .expect("the exact focus observation stages");
    let report = commit(frame);
    assert_eq!(report.before(), before);
    assert_eq!(report.after(), before);
}

fn assert_pane_focus_lifecycle(
    session: &mut DockspaceSession,
    surface: SurfaceId,
    item: ItemId,
) -> DockspacePaneFocusRequest {
    let request = pane_focus_request(session, surface)
        .expect("the explicit product action publishes a pane-focus request");
    assert_eq!(request.surface(), surface);
    assert_eq!(request.item(), Some(item));
    assert_eq!(request.target(), DockspaceFocusTarget::Pane(item));

    observe_pane_focus(session, request, DockspacePaneFocusObservation::NotFocused);
    assert_eq!(pane_focus_request(session, surface), Some(request));
    observe_pane_focus(session, request, DockspacePaneFocusObservation::Focused);
    assert_eq!(pane_focus_request(session, surface), None);
    request
}

#[test]
fn native_activation_focus_request_waits_for_observation_commit_and_survives_frame_drop() {
    let mut session = root_focus_session(FIRST);
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the focus backend enrolls");
    session
        .configure_managed_native_capabilities(
            managed_native_capabilities()
                .with(NativeHostCapability::GlobalFocusObservation)
                .with(NativeHostCapability::WindowActivationControl),
        )
        .expect("the focus backend capabilities configure");
    session
        .register_native_root(MAIN_SURFACE, NATIVE_ROOT_WINDOW)
        .expect("the native focus root registers");
    let registration = commit_managed_native_frame(&mut session);
    let [HostInputOutcome::NativeSurfaceRegistered { binding }] = registration.inputs() else {
        panic!(
            "unexpected native focus registration: {:?}",
            registration.inputs()
        );
    };
    let binding = *binding;
    session
        .report_managed_native_snapshot(
            [(
                binding,
                managed_native_window_facts(
                    PhysicalRect::new(80.0, 60.0, 800.0, 600.0)
                        .expect("native focus bounds validate"),
                ),
            )],
            managed_native_work_areas(),
        )
        .expect("the native focus root reports live geometry");
    session
        .report_native_global_focus(NativeGlobalFocus::Foreign)
        .expect("foreign focus establishes the activation baseline");
    commit_managed_native_frame(&mut session);
    present_surfaces(&mut session, &[MAIN_SURFACE, OTHER_SURFACE]);
    assert_eq!(session.pane_focus_request(MAIN_SURFACE), None);

    let mut activation = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the native pane activation frame begins");
    activation
        .select_item_current(SECOND)
        .expect("selecting the pane requests native activation");
    let mut activated = commit(activation);
    // Keep the published effect owner alive while its matching observation retries.
    let effects = activated.take_native_effects();
    assert!(matches!(
        effects.as_slice(),
        [effect] if matches!(effect.operation(),
            NativeEffectOperation::RequestFocus { binding: target } if *target == binding)
    ));
    assert_eq!(session.pane_focus_request(MAIN_SURFACE), None);

    session
        .report_native_global_focus(NativeGlobalFocus::Dock(binding))
        .expect("the admitted activation receives a newer matching focus fact");
    let before = session.version();
    let unpublished = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the matching observation enters an unpublished frame");
    assert_eq!(unpublished.pane_focus_request(MAIN_SURFACE), None);
    drop(unpublished);
    assert_eq!(session.version(), before);
    assert_eq!(session.pane_focus_request(MAIN_SURFACE), None);
    assert!(session.has_pending_native_input());

    let retry = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the uncommitted focus observation retries");
    assert_eq!(retry.pane_focus_request(MAIN_SURFACE), None);
    commit(retry);
    assert!(!session.has_pending_native_input());
    let request = session
        .pane_focus_request(MAIN_SURFACE)
        .expect("only the committed observation publishes its pane-focus request");
    assert_eq!(request.surface(), MAIN_SURFACE);
    assert_eq!(request.target(), DockspaceFocusTarget::Pane(SECOND));

    let later = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the next frame can read the committed request");
    assert_eq!(later.pane_focus_request(MAIN_SURFACE), Some(request));
    drop(later);
    assert_eq!(session.pane_focus_request(MAIN_SURFACE), Some(request));
}

#[test]
fn cross_surface_float_item_from_idle_focuses_the_exact_destination_pane() {
    let mut session = session();
    present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
    assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
    assert_eq!(pane_focus_request(&mut session, ROOTLESS_SURFACE), None);

    let before = session.version();
    let mut frame = session.begin_host_frame().expect("item float frame begins");
    frame
        .float_item_current(SECOND, ROOTLESS_SURFACE)
        .expect("cross-surface item float stages");
    let report = commit(frame);
    assert_one_revision_advance(before, report.after());
    let (root, floating) = match report.inputs() {
        [
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Floated {
                item: SECOND,
                root,
                surface: ROOTLESS_SURFACE,
                floating,
                changed: true,
            }),
        ] => (*root, *floating),
        inputs => panic!("unexpected item float outcomes: {inputs:?}"),
    };
    let moved = session.view().item(SECOND).expect("floated item survives");
    assert_eq!(moved.surface(), ROOTLESS_SURFACE);
    assert_eq!(moved.root(), root);
    assert_eq!(moved.contained(), Some(floating));
    assert!(moved.is_selected());
    assert_eq!(moved.tabs().items(), &[SECOND]);
    assert_eq!(
        session.view().item(FIRST).map(|item| item.surface()),
        Some(MAIN_SURFACE)
    );
    assert_eq!(
        session.view().item(FLOATING).map(|item| item.root()),
        Some(CONTAINED_ROOT)
    );

    present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
    assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
    assert_pane_focus_lifecycle(&mut session, ROOTLESS_SURFACE, SECOND);
    assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
}

#[test]
fn cross_surface_dock_item_from_idle_focuses_the_exact_destination_pane() {
    let mut session = session();
    present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
    assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
    assert_eq!(pane_focus_request(&mut session, ROOTLESS_SURFACE), None);

    let before = session.version();
    let prepared = session.prepare_dock_item(
        FLOATING,
        DockPlacement::Center(DockAnchor::Central(MAIN_ROOT)),
    );
    let mut frame = session.begin_host_frame().expect("item dock frame begins");
    frame
        .submit_prepared_action(prepared)
        .expect("cross-surface item dock stages");
    let report = commit(frame);
    assert_one_revision_advance(before, report.after());
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: FLOATING,
                source_root: CONTAINED_ROOT,
                target_root: MAIN_ROOT,
                changed: true,
            }
        )]
    );
    let moved = session.view().item(FLOATING).expect("docked item survives");
    assert_eq!(moved.surface(), MAIN_SURFACE);
    assert_eq!(moved.root(), MAIN_ROOT);
    assert_eq!(moved.contained(), None);
    assert!(moved.is_selected());
    assert_eq!(moved.tabs().items(), &[FIRST, SECOND, FLOATING]);
    assert!(session.view().root(CONTAINED_ROOT).is_none());

    present_surfaces(&mut session, &[MAIN_SURFACE]);
    assert!(session.view().surface(ROOTLESS_SURFACE).is_none());
    assert_pane_focus_lifecycle(&mut session, MAIN_SURFACE, FLOATING);
}

#[test]
fn root_float_and_dock_back_from_idle_focus_the_current_selected_pane() {
    for target_surface in [MAIN_SURFACE, OTHER_SURFACE] {
        let mut session = root_focus_session(SECOND);
        present_surfaces(&mut session, &[MAIN_SURFACE, OTHER_SURFACE]);
        assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
        assert_eq!(pane_focus_request(&mut session, OTHER_SURFACE), None);

        let before = session.version();
        let prepared = session.prepare_float_root(MAIN_ROOT, target_surface);
        let mut frame = session.begin_host_frame().expect("root float frame begins");
        frame
            .submit_prepared_action(prepared)
            .expect("root float stages without an earlier focus request");
        let report = commit(frame);
        assert_one_revision_advance(before, report.after());
        let floating = match report.inputs() {
            [
                HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::RootFloated {
                    root: MAIN_ROOT,
                    surface,
                    floating,
                    items,
                    changed: true,
                }),
            ] if *surface == target_surface && items == &[FIRST, SECOND] => *floating,
            inputs => panic!("unexpected root float outcomes: {inputs:?}"),
        };
        let selected = session.view().item(SECOND).expect("selected pane survives");
        assert_eq!(selected.surface(), target_surface);
        assert_eq!(selected.root(), MAIN_ROOT);
        assert_eq!(selected.contained(), Some(floating));
        assert!(selected.is_selected());
        assert_eq!(selected.tabs().items(), &[FIRST, SECOND]);

        let surfaces = if target_surface == MAIN_SURFACE {
            vec![MAIN_SURFACE, OTHER_SURFACE]
        } else {
            assert!(session.view().surface(MAIN_SURFACE).is_none());
            vec![OTHER_SURFACE]
        };
        present_surfaces(&mut session, &surfaces);
        if target_surface == MAIN_SURFACE {
            assert_eq!(pane_focus_request(&mut session, OTHER_SURFACE), None);
        }
        let floated_request = assert_pane_focus_lifecycle(&mut session, target_surface, SECOND);

        assert_eq!(pane_focus_request(&mut session, target_surface), None);
        let before = session.version();
        let prepared = session.prepare_dock_root_back(MAIN_ROOT);
        let mut frame = session
            .begin_host_frame()
            .expect("root dock-back frame begins");
        frame
            .submit_prepared_action(prepared)
            .expect("dock-back stages after the float request was acknowledged");
        let report = commit(frame);
        assert_one_revision_advance(before, report.after());
        let target_root = if target_surface == MAIN_SURFACE {
            MAIN_ROOT
        } else {
            OTHER_ROOT
        };
        assert_eq!(
            report.inputs(),
            &[HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::RootDocked {
                    root: MAIN_ROOT,
                    target_root,
                    items: vec![FIRST, SECOND],
                    changed: true,
                }
            )]
        );
        assert!(session.view().contained(floating).is_none());
        let selected = session.view().item(SECOND).expect("docked pane survives");
        assert_eq!(selected.surface(), target_surface);
        assert_eq!(selected.root(), target_root);
        assert_eq!(selected.contained(), None);
        assert!(selected.is_selected());
        let expected_items: &[ItemId] = if target_surface == MAIN_SURFACE {
            &[FIRST, SECOND]
        } else {
            &[FLOATING, FIRST, SECOND]
        };
        assert_eq!(selected.tabs().items(), expected_items);

        present_surfaces(&mut session, &surfaces);
        let docked_request = assert_pane_focus_lifecycle(&mut session, target_surface, SECOND);
        assert_ne!(docked_request, floated_request);
        if target_surface == MAIN_SURFACE {
            assert_eq!(pane_focus_request(&mut session, OTHER_SURFACE), None);
        }
    }
}

#[test]
fn public_select_changed_and_already_selected_both_request_pane_focus() {
    for use_prepared_action in [false, true] {
        let mut session = session();
        present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
        assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
        assert_eq!(pane_focus_request(&mut session, ROOTLESS_SURFACE), None);
        let mut previous_request = None;

        for changed in [true, false] {
            let before = session.version();
            let prepared = use_prepared_action.then(|| session.prepare_select_item(SECOND));
            let mut frame = session
                .begin_host_frame()
                .expect("public select frame begins");
            if let Some(prepared) = prepared {
                frame
                    .submit_prepared_action(prepared)
                    .expect("prepared public selection stages");
            } else {
                frame
                    .select_item_current(SECOND)
                    .expect("current public selection stages");
            }
            let report = commit(frame);
            assert_eq!(report.before(), before);
            assert_eq!(
                report.inputs(),
                &[HostInputOutcome::ProductActionApplied(
                    DockspaceActionOutcome::Selected {
                        item: SECOND,
                        changed,
                    }
                )]
            );
            if changed {
                assert_one_revision_advance(before, report.after());
            } else {
                assert_eq!(report.after(), before);
                assert!(!report.workspace_changed());
            }
            let selected = session.view().item(SECOND).expect("selected pane survives");
            assert!(selected.is_selected());
            assert_eq!(selected.tabs().items(), &[FIRST, SECOND]);

            present_surfaces(&mut session, &[MAIN_SURFACE, ROOTLESS_SURFACE]);
            assert_eq!(pane_focus_request(&mut session, ROOTLESS_SURFACE), None);
            let request = assert_pane_focus_lifecycle(&mut session, MAIN_SURFACE, SECOND);
            assert_ne!(Some(request), previous_request);
            previous_request = Some(request);
            assert_eq!(session.version(), report.after());
        }
    }
}

#[test]
fn dock_back_after_focusing_a_and_selecting_b_focuses_b_instead_of_focus_history() {
    let mut session = root_focus_session(FIRST);
    present_surfaces(&mut session, &[MAIN_SURFACE, OTHER_SURFACE]);
    let prepared = session.prepare_float_root(MAIN_ROOT, MAIN_SURFACE);
    let mut frame = session
        .begin_host_frame()
        .expect("setup float frame begins");
    frame
        .submit_prepared_action(prepared)
        .expect("the two-pane root floats for the focus-history regression");
    let floated = commit(frame);
    assert!(matches!(
        floated.inputs(),
        [HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::RootFloated {
            root: MAIN_ROOT,
            surface: MAIN_SURFACE,
            items,
            changed: true,
            ..
        })] if items == &[FIRST, SECOND]
    ));
    present_surfaces(&mut session, &[MAIN_SURFACE, OTHER_SURFACE]);
    let focused_a = assert_pane_focus_lifecycle(&mut session, MAIN_SURFACE, FIRST);

    let before = session.version();
    let mut frame = session.begin_host_frame().expect("select B frame begins");
    frame
        .select_item_current(SECOND)
        .expect("B selects after A was observed focused");
    let tabs = frame
        .view()
        .root(MAIN_ROOT)
        .and_then(|root| root.content())
        .and_then(|node| node.tabs())
        .expect("the contained root exposes its current candidate selection");
    assert_eq!(tabs.items(), &[FIRST, SECOND]);
    assert_eq!(tabs.selected(), Some(SECOND));
    let selected = commit(frame);
    assert_one_revision_advance(before, selected.after());
    assert_eq!(
        selected.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: SECOND,
                changed: true,
            }
        )]
    );
    present_surfaces(&mut session, &[MAIN_SURFACE, OTHER_SURFACE]);
    let request_b = pane_focus_request(&mut session, MAIN_SURFACE)
        .expect("selecting B requests its exact pane");
    assert_eq!(request_b.target(), DockspaceFocusTarget::Pane(SECOND));
    assert_eq!(request_b.surface(), MAIN_SURFACE);
    assert_ne!(request_b, focused_a);

    // Unavailable clears B's request while preserving A as the observed focus history.
    observe_pane_focus(
        &mut session,
        request_b,
        DockspacePaneFocusObservation::Unavailable,
    );
    assert_eq!(pane_focus_request(&mut session, MAIN_SURFACE), None);
    let before = session.version();
    let prepared = session.prepare_dock_root_back(MAIN_ROOT);
    let mut frame = session
        .begin_host_frame()
        .expect("B dock-back frame begins");
    frame
        .submit_prepared_action(prepared)
        .expect("the selected root docks back with older A focus history");
    let docked = commit(frame);
    assert_one_revision_advance(before, docked.after());
    assert_eq!(
        docked.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootDocked {
                root: MAIN_ROOT,
                target_root: MAIN_ROOT,
                items: vec![FIRST, SECOND],
                changed: true,
            }
        )]
    );
    let selected = session.view().item(SECOND).expect("B survives dock-back");
    assert_eq!(selected.contained(), None);
    assert!(selected.is_selected());
    assert_eq!(selected.tabs().items(), &[FIRST, SECOND]);
    present_surfaces(&mut session, &[MAIN_SURFACE, OTHER_SURFACE]);
    assert_eq!(pane_focus_request(&mut session, OTHER_SURFACE), None);

    let mut frame = session
        .begin_host_frame()
        .expect("superseded focus inspection frame begins");
    let plan = frame
        .paint_plan(MAIN_SURFACE)
        .expect("dock-back focus paint plan resolves")
        .expect("the docked surface remains paintable");
    for superseded in [focused_a, request_b] {
        assert!(
            plan.prepare_pane_focus_observation(superseded, DockspacePaneFocusObservation::Focused)
                .is_none(),
            "an earlier request cannot acknowledge the dock-back focus intent"
        );
    }
    commit(frame);
    assert_pane_focus_lifecycle(&mut session, MAIN_SURFACE, SECOND);
}
