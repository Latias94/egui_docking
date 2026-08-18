//! Tab strip, overflow menu, and popup authority reducer tests.

use super::*;

#[test]
fn requirement_pipeline_reconciles_popup_before_finalizing_disabled_policy() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .is_some()
    );
    let (session, _) = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .open_tab_list_menu(key, Some(fixture.items[2]))
        .expect("fresh engine must open a live strip without a prior revision");
    assert_eq!(session.key(), key);
    let mut disabled_replacement = DockPolicy::default();
    disabled_replacement.set_tab_bar(crate::policy::TabBarPolicy::new(
        crate::policy::TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let next_policy = fixture
        .engine
        .policy
        .revision()
        .checked_next()
        .expect("test policy revision must advance");
    fixture.engine.policy = disabled_replacement.snapshot(next_policy);
    fixture
        .engine
        .advance_revision(InputSequence::new(1))
        .expect("policy reconciliation must rebuild requirements");
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .popup(),
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .popup_requirement()
    );
    assert!(matches!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .popup(),
        crate::tab_strip::PopupPlaneRequirement::Inactive { .. }
    ));

    let mut disabled_policy = DockPolicy::default();
    disabled_policy.set_tab_bar(crate::policy::TabBarPolicy::new(
        crate::policy::TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let disabled = tab_strip_reducer_fixture(disabled_policy);
    let measurements = tab_strip_reducer_measurements(&disabled, true);
    let plan = compile_tab_strip_reducer_plan(&disabled, &measurements);
    let control = TabStripControlId::TabListMenu(disabled.bar);
    let record = plan
        .tab_strip_control_records()
        .iter()
        .copied()
        .find(|record| record.id() == control)
        .expect("disabled control remains a blocker");
    assert!(!record.enabled());
    let region = compiled_control_region(&disabled, &plan, control);
    let point = LogicalPoint::new(
        record.bounds().x() + record.bounds().width() * 0.5,
        record.bounds().y() + record.bounds().height() * 0.5,
    )
    .expect("control midpoint must be finite");
    assert_eq!(
        disabled.engine.freeze_journal_click_action(
            &plan,
            disabled.surface,
            region,
            point,
            &disabled.engine.policy,
        ),
        Err(InteractionRejection::TabStripControlDisabled { control })
    );
    assert_eq!(
        disabled.engine.interaction.status(),
        InteractionStatus::Idle
    );
}

#[test]
fn prepared_tab_strip_scroll_is_finite_clamped_and_reveals_hidden_members() {
    for delta in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert_eq!(
            TabScrollAdjustment::scroll_by(delta),
            Err(TabScrollAdjustmentError::NonFiniteDelta)
        );
    }
    let duplicate = ItemId::new(510);
    assert_eq!(
        TabScrollAdjustment::scroll_by_preserving(1.0, [duplicate, duplicate]),
        Err(TabScrollAdjustmentError::DuplicatePreservedItem { item: duplicate })
    );

    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 1);
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let maximum = fixture
        .engine
        .interaction_projection(fixture.surface)
        .expect("published strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .maximum_scroll_offset();
    assert!(maximum > 24.0);

    let first = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by(24.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        first.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset == 24.0
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let clamped = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by(1_000_000.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        clamped.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset == maximum
    ));

    let mut boundary = tab_strip_reducer_fixture(DockPolicy::default());
    let boundary_host = boundary
        .engine
        .create_presentation_host()
        .expect("boundary presentation host must mint");
    publish_tab_strip_reducer_projection(&mut boundary, boundary_host, true);
    let boundary = submit_prepared_interaction(&mut boundary.engine, boundary_host, |view| {
        view.prepare_tab_strip_scroll(
            boundary.surface,
            boundary.bar,
            TabScrollAdjustment::scroll_by(10.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        boundary.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: false,
                ..
            },
            ..
        } if *offset == maximum
    ));

    let mut reveal = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 2);
    let reveal_key = TabStripStateKey::new(reveal.surface, reveal.bar);
    reveal
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(reveal_key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let reveal_host = reveal
        .engine
        .create_presentation_host()
        .expect("reveal presentation host must mint");
    publish_tab_strip_reducer_projection(&mut reveal, reveal_host, true);
    let hidden = reveal.items[3];
    let before = reveal
        .engine
        .interaction_projection(reveal.surface)
        .expect("published reveal strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .members()
        .iter()
        .find(|member| member.tab().item == hidden)
        .copied()
        .expect("hidden member remains in the complete roster");
    assert_eq!(before.visibility(), TabStripMemberVisibility::Hidden);
    let revealed = submit_prepared_interaction(&mut reveal.engine, reveal_host, |view| {
        view.prepare_tab_strip_scroll(
            reveal.surface,
            reveal.bar,
            TabScrollAdjustment::reveal_item(hidden),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        revealed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled { changed: true, .. },
            ..
        }
    ));
    publish_tab_strip_reducer_projection(&mut reveal, reveal_host, true);
    let plan = reveal
        .engine
        .interaction_projection(reveal.surface)
        .expect("revealed strip is interactive")
        .plan();
    let bar = &plan.tab_bar_records()[0];
    let member = bar
        .members()
        .iter()
        .find(|member| member.tab().item == hidden)
        .expect("revealed item remains in the roster");
    assert!(member.full_bounds().x() >= bar.viewport().x());
    assert!(member.full_bounds().max().x() <= bar.viewport().max().x());
}

#[test]
fn prepared_strip_scroll_preserves_highest_priority_operability() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let preserved = fixture.items[0];
    let incompatible = fixture.items[3];
    let transition = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by_preserving(1_000_000.0, [preserved, incompatible])
                .expect("ordered preserve roster is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset > 0.0
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    assert!(
        fixture
            .engine
            .interaction_projection(fixture.surface)
            .expect("updated strip is authoritative")
            .plan()
            .tab_records()
            .iter()
            .any(|tab| tab.id().item == preserved),
        "the highest-priority item must retain operable tab chrome",
    );
}

#[test]
fn prepared_strip_scroll_preserves_compatible_operable_tab_ranges() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let retained = [fixture.items[1], fixture.items[0]];
    let transition = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_scroll(
            fixture.surface,
            fixture.bar,
            TabScrollAdjustment::scroll_by_preserving(1_000_000.0, retained)
                .expect("ordered preserve roster is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabStripScroll { prepared })
    });
    assert!(matches!(
        transition.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabStripScrolled {
                offset,
                changed: true,
                ..
            },
            ..
        } if *offset > 0.0
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let plan = fixture
        .engine
        .interaction_projection(fixture.surface)
        .expect("updated strip is authoritative")
        .plan();
    for item in retained {
        assert!(
            plan.tab_records().iter().any(|tab| tab.id().item == item),
            "preserved tab {item} must retain its configured drag/close operability",
        );
    }
}

#[test]
fn direct_selection_reveals_the_new_priority_item_after_explicit_strip_scroll() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let surface = fixture.surface;
    let bar = fixture.bar;
    let selected = fixture.items[3];
    let key = TabStripStateKey::new(surface, bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(0.0)
        .expect("zero strip offset is valid");
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let before = fixture
        .engine
        .interaction_projection(surface)
        .expect("published strip is interactive")
        .plan()
        .tab_bar_records()[0]
        .members()
        .iter()
        .find(|member| member.tab().item == selected)
        .copied()
        .expect("target item remains in the complete roster");
    assert_eq!(before.visibility(), TabStripMemberVisibility::Hidden);

    let source = fixture
        .engine
        .workspace()
        .capture_item_source(bar.root, bar.tabs, selected)
        .expect("target item source must be current");
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source },
        },
    )
    .expect("direct selection must commit");
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(0.0),
        "selection keeps the last effective offset until the next exact solve"
    );

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let plan = fixture
        .engine
        .interaction_projection(surface)
        .expect("republished strip is interactive")
        .plan();
    let bar = &plan.tab_bar_records()[0];
    let member = bar
        .members()
        .iter()
        .find(|member| member.tab().item == selected)
        .expect("selected item remains in the complete roster");
    assert_eq!(member.visibility(), TabStripMemberVisibility::Visible);
    assert!(member.full_bounds().x() >= bar.viewport().x());
    assert!(member.full_bounds().max().x() <= bar.viewport().max().x());
    assert!(bar.scroll_offset() > 0.0);
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(bar.scroll_offset()),
        "a successfully installed Ready plan adopts its solved offset"
    );
}

#[test]
fn only_an_installed_ready_plan_silently_adopts_its_resolved_scroll_offset() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let key = TabStripStateKey::new(surface, fixture.bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 0.0)
        .expect("explicit beginning offset is valid");
    let token = fixture
        .engine
        .begin_surface_contribution(surface)
        .expect("surface contribution must begin");
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let stale_ready = fixture
        .engine
        .prepare_surface_contribution(token, measurements.clone())
        .expect("duplicate ready candidate must prepare against the same base");
    let ready = fixture
        .engine
        .prepare_surface_contribution(token, measurements)
        .expect("ready candidate must prepare");
    let resolved = match ready.paint_candidate() {
        PreparedSurfacePaintCandidate::Ready(candidate) => candidate
            .plan()
            .tab_bar_records()
            .first()
            .expect("fixture plan has one tab bar")
            .scroll_offset(),
        candidate => panic!("fixture must prepare Ready, got {candidate:?}"),
    };
    assert!(resolved > 0.0);
    let requirement_before = fixture
        .engine
        .presentation_authority
        .presentation_requirements
        .revision();
    let scene_before = fixture
        .engine
        .presentation_authority
        .scene
        .surface(surface)
        .expect("surface scene exists")
        .stamp();
    let ledger_before = fixture.engine.presentation_ledger_diagnostics();
    let policy = fixture.engine.policy.clone();

    let installed = fixture
        .engine
        .reduce_surface_contribution(ReducerTickId::new(900), ready, &policy)
        .expect("ready contribution must install");
    assert!(matches!(
        installed,
        SurfaceContributionOutcome::Ready { .. }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(resolved)
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision(),
        requirement_before,
        "silent adoption cannot mint a new measurement requirement"
    );
    assert_eq!(
        fixture.engine.presentation_ledger_diagnostics(),
        ledger_before,
        "installing a candidate cannot manufacture an emission"
    );
    let scene_after_ready = fixture
        .engine
        .presentation_authority
        .scene
        .surface(surface)
        .expect("ready surface scene exists")
        .stamp();
    assert_eq!(
        scene_after_ready.revision().get(),
        scene_before.revision().get() + 1,
        "Ready installation advances the scene exactly once"
    );
    fixture
        .engine
        .try_rebuild_presentation_requirements(None)
        .expect("adopted solved state already matches the installed plan");
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision(),
        requirement_before
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .scene
            .surface(surface)
            .expect("ready scene remains installed")
            .stamp(),
        scene_after_ready,
        "silent adoption cannot trigger a contribution feedback loop"
    );

    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 1.0)
        .expect("replacement transient offset is valid");
    let stale = fixture
        .engine
        .reduce_surface_contribution(ReducerTickId::new(901), stale_ready, &policy)
        .expect("stale Ready is a typed nonfatal rejection");
    assert!(matches!(
        stale,
        SurfaceContributionOutcome::Rejected {
            reason: SurfaceContributionRejection::StaleBase { .. },
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(1.0),
        "stale Ready cannot overwrite newer transient state"
    );

    let retained = fixture
        .engine
        .prepare_surface_retained_contribution(
            fixture
                .engine
                .begin_surface_contribution(surface)
                .expect("current Ready contribution must begin"),
        )
        .expect("current Ready candidate may be retained");
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 2.0)
        .expect("second replacement transient offset is valid");
    let retained = fixture
        .engine
        .reduce_surface_contribution(ReducerTickId::new(902), retained, &policy)
        .expect("retained contribution must reduce");
    assert!(matches!(
        retained,
        SurfaceContributionOutcome::Retained { .. }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(2.0),
        "Retained candidates never carry a replacement solved offset"
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .scene
            .surface(surface)
            .expect("retention keeps the same Ready scene")
            .stamp(),
        scene_after_ready
    );
}

#[test]
fn prepared_menu_scroll_revalidates_popup_authority_and_dismisses_exactly() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(
            fixture.surface,
            TabStripControlId::TabListMenu(fixture.bar),
        )
        .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    changed: true,
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared menu control must open one session: {outcome:?}"),
    };
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let stale_dismiss = {
        let frame = begin_test_host_frame(&fixture.engine, host);
        frame
            .view()
            .prepare_tab_list_menu_dismiss(fixture.surface, session)
            .expect("current menu dismissal proof prepares")
    };
    let revision_before_scroll = fixture
        .engine
        .presentation_authority
        .presentation_requirements
        .revision();
    let scrolled = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_scroll(
            fixture.surface,
            session,
            TabScrollAdjustment::scroll_by(1_000_000.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabListMenuScroll { prepared })
    });
    let maximum = match scrolled.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabListMenuScrolled {
                    offset,
                    changed: true,
                    ..
                },
            ..
        } => *offset,
        outcome => panic!("menu scroll must clamp at its maximum: {outcome:?}"),
    };
    assert!(maximum > 0.0);
    assert!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision()
            > revision_before_scroll,
        "menu scrolling invalidates the popup roster as one cross-surface authority"
    );
    assert!(
        fixture
            .engine
            .interaction_projection(fixture.surface)
            .is_none()
    );

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let stale = submit_test_input(
        &mut fixture.engine,
        host,
        EngineInput::DismissTabListMenu {
            prepared: stale_dismiss,
        },
    )
    .expect("stale prepared dismissal reduces as an explicit rejection");
    assert!(matches!(
        stale.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::StaleScene),
            ..
        }
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(session)
    );

    let boundary = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_scroll(
            fixture.surface,
            session,
            TabScrollAdjustment::scroll_by(1.0).expect("finite delta is valid"),
        )
        .map(|prepared| EngineInput::AdjustTabListMenuScroll { prepared })
    });
    assert!(matches!(
        boundary.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuScrolled {
                offset,
                changed: false,
                ..
            },
            ..
        } if *offset == maximum
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let revealed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_scroll(
            fixture.surface,
            session,
            TabScrollAdjustment::reveal_item(fixture.items[0]),
        )
        .map(|prepared| EngineInput::AdjustTabListMenuScroll { prepared })
    });
    assert!(matches!(
        revealed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuScrolled {
                offset: 0.0,
                changed: true,
                ..
            },
            ..
        }
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let dismissed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_dismiss(fixture.surface, session)
            .map(|prepared| EngineInput::DismissTabListMenu { prepared })
    });
    assert!(matches!(
        dismissed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuDismissed { session: actual },
            ..
        } if *actual == session
    ));
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
}

#[test]
fn prepared_menu_navigation_atomically_moves_focus_reveals_and_clamps() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let bar = fixture.bar;
    let items = fixture.items;
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared control must open one menu: {outcome:?}"),
    };
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.focus()),
        Some(items[3])
    );
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let first = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::First)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        first.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                offset: 0.0,
                changed: true,
                ..
            },
            ..
        } if *item == items[0]
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let next = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::Next)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        next.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                changed: true,
                ..
            },
            ..
        } if *item == items[1]
    ));

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let revision_before_reveal = fixture
        .engine
        .presentation_authority
        .presentation_requirements
        .revision();
    let revealed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(
            surface,
            session,
            TabListMenuNavigation::Focus(items[3]),
        )
        .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    let revealed_offset = match revealed.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabListMenuFocusMoved {
                    item,
                    offset,
                    changed: true,
                    ..
                },
            ..
        } if *item == items[3] => *offset,
        outcome => panic!("focus and reveal must commit atomically: {outcome:?}"),
    };
    assert!(revealed_offset > 0.0);
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .presentation_requirements
            .revision(),
        revision_before_reveal
            .checked_next()
            .expect("one navigation delta must advance one revision")
    );
    let active = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .active_menu()
        .expect("navigation keeps the menu open");
    assert_eq!(active.focus(), items[3]);
    assert_eq!(active.scroll_offset(), revealed_offset);
    assert!(fixture.engine.interaction_projection(surface).is_none());

    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let plan = fixture
        .engine
        .interaction_projection(surface)
        .expect("republished menu is interactive")
        .plan();
    let menu = plan
        .tab_list_menu_records()
        .iter()
        .find(|record| record.session() == session)
        .expect("republished plan carries the same menu session");
    let focused = menu
        .rows()
        .iter()
        .filter(|row| row.focused())
        .collect::<Vec<_>>();
    assert_eq!(focused.len(), 1);
    assert_eq!(focused[0].tab().item, items[3]);

    let boundary = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::Next)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        boundary.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                offset,
                changed: false,
                ..
            },
            ..
        } if *item == items[3] && *offset == revealed_offset
    ));
}

#[test]
fn prepared_menu_navigation_rejects_missing_targets_and_stale_session_aba() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let bar = fixture.bar;
    let items = fixture.items;
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let first_session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared control must open one menu: {outcome:?}"),
    };
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let missing = ItemId::new(999_999);
    let frame = begin_test_host_frame(&fixture.engine, host);
    assert_eq!(
        frame.view().prepare_tab_list_menu_navigation(
            surface,
            first_session,
            TabListMenuNavigation::Focus(missing),
        ),
        Err(InteractionRejection::TabListMenuFocusItemUnavailable {
            session: first_session,
            item: missing,
        })
    );
    let stale = frame
        .view()
        .prepare_tab_list_menu_navigation(surface, first_session, TabListMenuNavigation::Previous)
        .expect("current navigation proof prepares");

    let dismissed = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_dismiss(surface, first_session)
            .map(|prepared| EngineInput::DismissTabListMenu { prepared })
    });
    assert!(matches!(
        dismissed.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuDismissed { .. },
            ..
        }
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let reopened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let successor = match reopened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("control must reopen a successor menu: {outcome:?}"),
    };
    assert_ne!(successor, first_session);
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let rejected = submit_test_input(
        &mut fixture.engine,
        host,
        EngineInput::NavigateTabListMenu { prepared: stale },
    )
    .expect("stale prepared navigation reduces as a rejection");
    assert!(matches!(
        rejected.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::Rejected(InteractionRejection::StaleScene),
            ..
        }
    ));
    let active = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .active_menu()
        .expect("stale predecessor input must not close the successor");
    assert_eq!(active.session(), successor);
    assert_eq!(active.focus(), items[3]);
}

#[test]
fn prepared_menu_end_then_enter_binds_the_exact_workspace_event_cause() {
    let mut fixture = tab_strip_reducer_fixture_with_selection(DockPolicy::default(), 0);
    let surface = fixture.surface;
    let bar = fixture.bar;
    let target = fixture.items[3];
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let opened = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_strip_control_activation(surface, TabStripControlId::TabListMenu(bar))
            .map(|prepared| EngineInput::ActivateTabStripControl { prepared })
    });
    let session = match opened.reduced_inputs()[0].outcome() {
        InputOutcome::InteractionProcessed {
            outcome:
                InteractionOutcome::TabStripControlActivated {
                    menu: Some(session),
                    ..
                },
            ..
        } => *session,
        outcome => panic!("prepared control must open one menu: {outcome:?}"),
    };
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let navigated = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_navigation(surface, session, TabListMenuNavigation::Last)
            .map(|prepared| EngineInput::NavigateTabListMenu { prepared })
    });
    assert!(matches!(
        navigated.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuFocusMoved {
                item,
                changed: true,
                ..
            },
            ..
        } if *item == target
    ));
    publish_tab_strip_reducer_projection(&mut fixture, host, true);

    let tab = TabSceneId {
        root: bar.root,
        tabs: bar.tabs,
        item: target,
    };
    let selected = submit_prepared_interaction(&mut fixture.engine, host, |view| {
        view.prepare_tab_list_menu_row_activation(surface, session, tab)
            .map(|prepared| EngineInput::ActivateTabListMenuRow { prepared })
    });
    assert!(matches!(
        selected.reduced_inputs()[0].outcome(),
        InputOutcome::InteractionProcessed {
            outcome: InteractionOutcome::TabListMenuItemSelected {
                tab: actual,
                changed: true,
                ..
            },
            ..
        } if *actual == tab
    ));
    assert_eq!(selected.events().len(), 1);
    let reduced = &selected.reduced_inputs()[0];
    let cause = ReductionCause::Input {
        tick: reduced.tick(),
        ordinal: reduced.causal_ordinal(),
        input: reduced.sequence(),
        source: reduced.source(),
        source_sequence: reduced.source_sequence(),
    };
    assert_eq!(
        selected.events()[0].cause(),
        cause,
        "the shared journal/semantic reducer must retain the exact input cause"
    );
}

#[test]
fn strip_scroll_controls_align_the_adjacent_partial_tab_and_stop_at_boundaries() {
    let mut scroll_policy = DockPolicy::default();
    scroll_policy.set_close_capability(crate::policy::CloseCapability::Disabled);
    let mut backward = tab_strip_reducer_fixture_with_selection(scroll_policy.clone(), 1);
    let key = TabStripStateKey::new(backward.surface, backward.bar);
    backward
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(key)
        .set_scroll_offset(48.0)
        .expect("test scroll must be valid");
    let measurements = tab_strip_reducer_measurements(&backward, true);
    let plan = compile_tab_strip_reducer_plan(&backward, &measurements);
    let control = TabStripControlId::ScrollBackward(backward.bar);
    let expected = aligned_tab_strip_scroll_offset(&plan, key, control)
        .expect("one beginning-side partial tab must exist");
    let outcome = backward
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&plan, key, control),
            &plan,
        )
        .expect("backward control must settle");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabStripControlActivated {
            control: actual,
            changed: true,
            menu: None,
        } if actual == control
    ));
    assert_eq!(
        backward
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(crate::tab_strip::TabStripState::scroll_offset),
        Some(expected)
    );

    let mut forward = tab_strip_reducer_fixture_with_selection(scroll_policy.clone(), 1);
    let forward_key = TabStripStateKey::new(forward.surface, forward.bar);
    forward
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(forward_key)
        .set_scroll_offset(48.0)
        .expect("test scroll must be valid");
    let measurements = tab_strip_reducer_measurements(&forward, true);
    let plan = compile_tab_strip_reducer_plan(&forward, &measurements);
    let control = TabStripControlId::ScrollForward(forward.bar);
    let expected = aligned_tab_strip_scroll_offset(&plan, forward_key, control)
        .expect("one ending-side partial tab must exist");
    let outcome = forward
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&plan, forward_key, control),
            &plan,
        )
        .expect("forward control must settle");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabStripControlActivated { changed: true, .. }
    ));
    assert_eq!(
        forward
            .engine
            .presentation_authority
            .tab_strip_states
            .state(forward_key)
            .and_then(crate::tab_strip::TabStripState::scroll_offset),
        Some(expected)
    );

    let mut boundary = tab_strip_reducer_fixture_with_selection(scroll_policy, 0);
    let boundary_key = TabStripStateKey::new(boundary.surface, boundary.bar);
    boundary
        .engine
        .presentation_authority
        .tab_strip_states
        .state_mut(boundary_key)
        .set_scroll_offset(0.0)
        .expect("zero is a valid boundary");
    let measurements = tab_strip_reducer_measurements(&boundary, true);
    let plan = compile_tab_strip_reducer_plan(&boundary, &measurements);
    assert_eq!(
        aligned_tab_strip_scroll_offset(
            &plan,
            boundary_key,
            TabStripControlId::ScrollBackward(boundary.bar),
        ),
        None
    );
}

#[test]
fn menu_control_release_opens_then_toggles_and_invalidates_old_contributions() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let prepared = fixture
        .engine
        .prepare_surface_contribution(
            fixture
                .engine
                .begin_surface_contribution(fixture.surface)
                .expect("surface contribution must begin"),
            measurements.clone(),
        )
        .expect("closed-menu contribution must prepare");
    let plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    let control = TabStripControlId::TabListMenu(fixture.bar);
    let frozen = frozen_control(&plan, key, control);
    let selected_before = match fixture.engine.workspace.node(fixture.bar.tabs) {
        Some(Node::Tabs { selected, .. }) => *selected,
        _ => panic!("fixture bar must remain tabs"),
    };
    let outcome = fixture
        .engine
        .finish_journal_tab_strip_control(tab_strip_test_cause(), &frozen, &plan)
        .expect("menu open must settle");
    let opened = match outcome {
        InteractionOutcome::TabStripControlActivated {
            changed: true,
            menu: Some(session),
            ..
        } => session,
        outcome => panic!("menu control must open one session, got {outcome:?}"),
    };
    assert_eq!(opened.key(), key);
    assert_eq!(
        match fixture.engine.workspace.node(fixture.bar.tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("fixture bar must remain tabs"),
        },
        selected_before,
        "the opening release cannot be reinterpreted as a new menu row"
    );
    let policy = fixture.engine.policy.clone();
    assert!(matches!(
        fixture
            .engine
            .reduce_surface_contribution(ReducerTickId::new(2), prepared, &policy)
            .expect("stale contribution must reduce as a rejection"),
        SurfaceContributionOutcome::Rejected {
            reason: SurfaceContributionRejection::StaleBase { .. },
            ..
        }
    ));

    let outcome = fixture
        .engine
        .finish_journal_tab_strip_control(tab_strip_test_cause(), &frozen, &plan)
        .expect("menu toggle-close must settle");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabStripControlActivated {
            changed: true,
            menu: None,
            ..
        }
    ));
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
}

#[test]
fn owner_popup_geometry_unavailable_closes_only_after_the_contribution_batch() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let surface = fixture.surface;
    let key = TabStripStateKey::new(surface, fixture.bar);
    let host = fixture
        .engine
        .create_presentation_host()
        .expect("test presentation host must mint");
    publish_tab_strip_reducer_projection(&mut fixture, host, true);
    let closed_plan = fixture
        .engine
        .interaction_projection(surface)
        .expect("closed strip should be interactive")
        .plan()
        .clone();
    let opened = fixture
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(
                &closed_plan,
                key,
                TabStripControlId::TabListMenu(fixture.bar),
            ),
            &closed_plan,
        )
        .expect("menu control should open");
    let session = match opened {
        InteractionOutcome::TabStripControlActivated {
            menu: Some(session),
            changed: true,
            ..
        } => session,
        outcome => panic!("menu control should open one session: {outcome:?}"),
    };
    let popup_plane =
        LogicalRect::new(0.0, 100.0, 260.0, 80.0).expect("positive popup plane should be valid");
    let measurements = tab_strip_reducer_measurements_with_popup_plane(
        &fixture.engine,
        surface,
        true,
        Some(popup_plane),
    );
    let prepared = fixture
        .engine
        .prepare_surface_contribution(
            fixture
                .engine
                .begin_surface_contribution(surface)
                .expect("owner contribution should begin"),
            measurements,
        )
        .expect("proved popup geometry failure should prepare as unavailable");
    assert!(matches!(
        prepared.paint_candidate(),
        PreparedSurfacePaintCandidate::Unavailable {
            reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                session: actual,
                reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
            },
        } if actual == session
    ));
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(crate::tab_strip::ActiveTabListMenu::session),
        Some(session),
        "preparation is read-only and cannot close the active session"
    );

    let transition = submit_surface_measurement(&mut fixture.engine, host, prepared);
    assert!(matches!(
        transition.surface_contributions(),
        [SurfaceContributionOutcome::Unavailable {
            surface: actual,
            reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                session: actual_session,
                reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
            },
            ..
        }] if *actual == surface && *actual_session == session
    ));
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
    assert!(matches!(
        fixture.engine.presentation_requirements().popup(),
        crate::tab_strip::PopupPlaneRequirement::Inactive { .. }
    ));
    assert!(fixture.engine.interaction_projection(surface).is_none());
    assert!(matches!(
        fixture.engine.scene().surface(surface),
        Some(SurfaceScene::Stale(_) | SurfaceScene::Bootstrap(_))
    ));
}

#[test]
fn menu_control_replaces_cross_surface_session_and_invalidates_both_owners() {
    let first_surface = SurfaceId::new(601);
    let second_surface = SurfaceId::new(602);
    let first_root = RootId::new(603);
    let second_root = RootId::new(604);
    let first_items = [
        ItemId::new(610),
        ItemId::new(611),
        ItemId::new(612),
        ItemId::new(613),
    ];
    let second_items = [
        ItemId::new(620),
        ItemId::new(621),
        ItemId::new(622),
        ItemId::new(623),
    ];
    let mut builder = Workspace::builder();
    let first_tabs =
        builder.insert_node(Node::tabs_with_selection(first_items, Some(first_items[3])));
    let second_tabs = builder.insert_node(Node::tabs_with_selection(
        second_items,
        Some(second_items[2]),
    ));
    builder.set_root(
        first_root,
        RootRecord::new(first_tabs).with_central(first_tabs),
    );
    builder.set_root(
        second_root,
        RootRecord::new(second_tabs).with_central(second_tabs),
    );
    builder.set_surface(first_surface, SurfacePresentation::with_main(first_root));
    builder.set_surface(second_surface, SurfacePresentation::with_main(second_root));
    let mut engine = DockEngine::new(
        builder
            .build()
            .expect("cross-surface tab-strip workspace must validate"),
        DockPolicy::default(),
    )
    .expect("cross-surface tab-strip engine must initialize");
    let first_bar = TabBarSceneId {
        root: first_root,
        tabs: first_tabs,
    };
    let second_bar = TabBarSceneId {
        root: second_root,
        tabs: second_tabs,
    };
    let first_key = TabStripStateKey::new(first_surface, first_bar);
    let second_key = TabStripStateKey::new(second_surface, second_bar);
    let selected_before = (first_items[3], second_items[2]);

    let first_measurements = tab_strip_reducer_measurements_for(&engine, first_surface, true);
    let first_plan =
        compile_tab_strip_reducer_plan_for(&engine, first_surface, &first_measurements);
    let first_control = TabStripControlId::TabListMenu(first_bar);
    let first_session = match engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&first_plan, first_key, first_control),
            &first_plan,
        )
        .expect("first menu open must settle")
    {
        InteractionOutcome::TabStripControlActivated {
            control,
            changed: true,
            menu: Some(session),
        } if control == first_control => session,
        outcome => panic!("first menu must open one session, got {outcome:?}"),
    };
    let first_stamp_before_replacement = engine
        .presentation_authority
        .scene
        .surface(first_surface)
        .expect("first surface remains rostered")
        .stamp();
    let second_stamp_before_replacement = engine
        .presentation_authority
        .scene
        .surface(second_surface)
        .expect("second surface remains rostered")
        .stamp();

    let second_measurements = tab_strip_reducer_measurements_for(&engine, second_surface, true);
    let second_plan =
        compile_tab_strip_reducer_plan_for(&engine, second_surface, &second_measurements);
    let second_control = TabStripControlId::TabListMenu(second_bar);
    let second_session = match engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&second_plan, second_key, second_control),
            &second_plan,
        )
        .expect("replacement menu open must settle")
    {
        InteractionOutcome::TabStripControlActivated {
            control,
            changed: true,
            menu: Some(session),
        } if control == second_control => session,
        outcome => panic!("replacement menu must open one session, got {outcome:?}"),
    };

    assert_ne!(second_session, first_session);
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session)
    );
    assert!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu_for(first_key)
            .is_none()
    );
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu_for(second_key)
            .map(|menu| menu.session()),
        Some(second_session)
    );
    assert_ne!(
        engine
            .presentation_authority
            .scene
            .surface(first_surface)
            .expect("first surface remains rostered")
            .stamp(),
        first_stamp_before_replacement
    );
    assert_ne!(
        engine
            .presentation_authority
            .scene
            .surface(second_surface)
            .expect("second surface remains rostered")
            .stamp(),
        second_stamp_before_replacement
    );
    assert_eq!(
        match engine.workspace.node(first_tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("first bar must remain tabs"),
        },
        Some(selected_before.0),
        "opening on another surface cannot select a menu row on the old owner"
    );
    assert_eq!(
        match engine.workspace.node(second_tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("second bar must remain tabs"),
        },
        Some(selected_before.1),
        "the replacement opening edge cannot be reinterpreted as a menu row"
    );

    let first_stamp = engine
        .presentation_authority
        .scene
        .surface(first_surface)
        .expect("first surface remains rostered")
        .stamp();
    let mut changed_surfaces = BTreeSet::new();
    engine
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(701),
            &[SurfaceContributionOutcome::Unavailable {
                surface: first_surface,
                stamp: first_stamp,
                reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                    session: second_session,
                    reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
                },
            }],
            &mut changed_surfaces,
        )
        .expect("a non-owner unavailable outcome must be ignored");
    assert!(changed_surfaces.is_empty());
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session),
        "a non-owner surface cannot close the current menu"
    );

    engine
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(702),
            &[SurfaceContributionOutcome::Unavailable {
                surface: first_surface,
                stamp: first_stamp,
                reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
                    session: first_session,
                    reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
                },
            }],
            &mut changed_surfaces,
        )
        .expect("a stale owner session must be ignored");
    assert!(changed_surfaces.is_empty());
    assert_eq!(
        engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session),
        "an unavailable outcome for a stale session cannot close its successor"
    );

    let host = engine
        .create_presentation_host()
        .expect("cross-surface presentation host must mint");
    let first_measurements = tab_strip_reducer_measurements_for(&engine, first_surface, true);
    publish_surface_projection_with_measurements(
        &mut engine,
        host,
        first_surface,
        first_measurements,
    );
    let second_measurements = tab_strip_reducer_measurements_for(&engine, second_surface, true);
    publish_surface_projection_with_measurements(
        &mut engine,
        host,
        second_surface,
        second_measurements,
    );
    assert!(engine.interaction_projection(first_surface).is_some());
    assert!(engine.interaction_projection(second_surface).is_some());

    let first_stamp = engine
        .presentation_authority
        .scene
        .surface(first_surface)
        .expect("first surface remains rostered")
        .stamp();
    let second_stamp = engine
        .presentation_authority
        .scene
        .surface(second_surface)
        .expect("second surface remains rostered")
        .stamp();
    let stale = SurfaceContributionOutcome::Unavailable {
        surface: first_surface,
        stamp: first_stamp,
        reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
            session: first_session,
            reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
        },
    };
    let non_owner = SurfaceContributionOutcome::Unavailable {
        surface: first_surface,
        stamp: first_stamp,
        reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
            session: second_session,
            reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
        },
    };
    let exact = SurfaceContributionOutcome::Unavailable {
        surface: second_surface,
        stamp: second_stamp,
        reason: SurfaceContributionUnavailableReason::PopupGeometryUnavailable {
            session: second_session,
            reason: PopupGeometryUnavailableReason::AnchorOutsidePlane,
        },
    };
    let mut forward = engine.candidate();
    let mut reverse = engine.candidate();
    let mut forward_changed = BTreeSet::new();
    let mut reverse_changed = BTreeSet::new();
    forward
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(703),
            &[stale.clone(), non_owner.clone(), exact.clone()],
            &mut forward_changed,
        )
        .expect("exact owner unavailability must settle after earlier inert outcomes");
    reverse
        .settle_popup_geometry_unavailability_after_surface_contributions(
            ReducerTickId::new(703),
            &[exact, non_owner, stale],
            &mut reverse_changed,
        )
        .expect("exact owner unavailability must settle before later inert outcomes");
    let expected_changed = BTreeSet::from([first_surface, second_surface]);
    assert_eq!(forward_changed, expected_changed);
    assert_eq!(reverse_changed, expected_changed);
    for settled in [&forward, &reverse] {
        assert!(
            settled
                .presentation_authority
                .tab_strip_states
                .active_menu()
                .is_none()
        );
        assert!(matches!(
            settled.presentation_requirements().popup(),
            crate::tab_strip::PopupPlaneRequirement::Inactive { .. }
        ));
        assert!(settled.interaction_projection(first_surface).is_none());
        assert!(settled.interaction_projection(second_surface).is_none());
    }
    assert_eq!(
        forward.presentation_authority.tab_strip_states,
        reverse.presentation_authority.tab_strip_states
    );
    assert_eq!(
        forward.presentation_authority.presentation_requirements,
        reverse.presentation_authority.presentation_requirements
    );
    assert_eq!(
        forward.presentation_authority.scene,
        reverse.presentation_authority.scene
    );
}

#[test]
fn menu_row_selection_and_close_publish_atomically_and_stale_sessions_reject() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let automatic_plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    let maximum = automatic_plan.tab_bar_records()[0].maximum_scroll_offset();
    assert!(maximum > 0.0);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, maximum)
        .expect("explicit end offset is valid");
    let closed_plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    assert_eq!(
        closed_plan.tab_bar_records()[0]
            .members()
            .first()
            .expect("fixture has a first tab")
            .visibility(),
        TabStripMemberVisibility::Hidden
    );
    let menu_control = TabStripControlId::TabListMenu(fixture.bar);
    let open = fixture
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&closed_plan, key, menu_control),
            &closed_plan,
        )
        .expect("menu open must settle");
    let session = match open {
        InteractionOutcome::TabStripControlActivated {
            menu: Some(session),
            ..
        } => session,
        outcome => panic!("menu must open, got {outcome:?}"),
    };
    let menu_measurements = tab_strip_reducer_measurements(&fixture, true);
    let menu_plan = compile_tab_strip_reducer_plan(&fixture, &menu_measurements);
    let row = menu_plan
        .tab_list_menu_records()
        .iter()
        .find(|menu| menu.session() == session)
        .and_then(|menu| menu.rows().first())
        .copied()
        .expect("open menu must project its first row");
    let frozen = FrozenTabListMenuRowClick {
        session,
        record: row,
        revision: menu_plan.popup().revision(),
    };
    let before = fixture.engine.version();
    let mut events = Vec::new();
    let policy = fixture.engine.policy.clone();
    let outcome = fixture
        .engine
        .finish_journal_tab_list_menu_row(
            tab_strip_test_cause(),
            tab_strip_test_focus_causal(),
            &frozen,
            &menu_plan,
            &policy,
            &mut events,
        )
        .expect("menu row must settle atomically");
    assert!(matches!(
        outcome,
        InteractionOutcome::TabListMenuItemSelected {
            session: actual,
            tab,
            changed: true,
            ..
        } if actual == session && tab == row.tab()
    ));
    assert_ne!(fixture.engine.version(), before);
    assert_eq!(events.len(), 1);
    assert!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .is_none()
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(maximum),
        "menu selection preserves the current effective offset for minimal reveal"
    );
    assert_eq!(
        match fixture.engine.workspace.node(fixture.bar.tabs) {
            Some(Node::Tabs { selected, .. }) => *selected,
            _ => panic!("fixture bar must remain tabs"),
        },
        Some(row.tab().item)
    );
    let selected_measurements = tab_strip_reducer_measurements(&fixture, true);
    let selected_plan = compile_tab_strip_reducer_plan(&fixture, &selected_measurements);
    assert_eq!(
        selected_plan.tab_bar_records()[0]
            .members()
            .first()
            .expect("selected first tab remains in the roster")
            .visibility(),
        TabStripMemberVisibility::Visible
    );

    let workspace_after = fixture.engine.workspace.clone();
    let outcome = fixture
        .engine
        .finish_journal_tab_list_menu_row(
            tab_strip_test_cause(),
            tab_strip_test_focus_causal(),
            &frozen,
            &menu_plan,
            &policy,
            &mut Vec::new(),
        )
        .expect("stale session must reject without mutation");
    assert_eq!(
        outcome,
        InteractionOutcome::Rejected(InteractionRejection::TabListMenuSessionUnavailable {
            session
        })
    );
    assert_eq!(fixture.engine.workspace, workspace_after);
}

#[test]
fn menu_backdrop_rejects_stale_session_and_revision_without_dismissing_current_menu() {
    let mut fixture = tab_strip_reducer_fixture(DockPolicy::default());
    let key = TabStripStateKey::new(fixture.surface, fixture.bar);
    let measurements = tab_strip_reducer_measurements(&fixture, true);
    let closed_plan = compile_tab_strip_reducer_plan(&fixture, &measurements);
    let menu_control = TabStripControlId::TabListMenu(fixture.bar);
    let first_session = match fixture
        .engine
        .finish_journal_tab_strip_control(
            tab_strip_test_cause(),
            &frozen_control(&closed_plan, key, menu_control),
            &closed_plan,
        )
        .expect("first menu open must settle")
    {
        InteractionOutcome::TabStripControlActivated {
            menu: Some(session),
            ..
        } => session,
        outcome => panic!("first menu must open, got {outcome:?}"),
    };
    let first_measurements = tab_strip_reducer_measurements(&fixture, true);
    let first_plan = compile_tab_strip_reducer_plan(&fixture, &first_measurements);
    let first_backdrop = first_plan
        .tab_list_menu_backdrop_records()
        .iter()
        .copied()
        .find(|record| record.session() == first_session)
        .expect("first menu must compile one backdrop");
    let first_frozen = FrozenTabListMenuBackdropClick {
        session: first_session,
        record: first_backdrop,
        revision: first_plan.popup().revision(),
    };

    let close_delta = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .close_tab_list_menu(first_session)
        .expect("first exact session must close");
    fixture
        .engine
        .consume_tab_strip_state_delta(tab_strip_test_cause(), &close_delta)
        .expect("first close must invalidate the popup roster");
    let (second_session, open_delta) = fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .open_tab_list_menu(key, Some(fixture.items[1]))
        .expect("the live strip must open a successor session");
    fixture
        .engine
        .consume_tab_strip_state_delta(tab_strip_test_cause(), &open_delta)
        .expect("successor open must invalidate the popup roster");
    assert_ne!(second_session, first_session);
    let current_measurements = tab_strip_reducer_measurements(&fixture, true);
    let current_plan = compile_tab_strip_reducer_plan(&fixture, &current_measurements);

    let stale_session = fixture
        .engine
        .finish_journal_tab_list_menu_backdrop(tab_strip_test_cause(), &first_frozen, &current_plan)
        .expect("stale session rejection must settle");
    assert_eq!(
        stale_session,
        InteractionOutcome::Rejected(InteractionRejection::TabListMenuSessionUnavailable {
            session: first_session,
        })
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session)
    );

    let current_backdrop = current_plan
        .tab_list_menu_backdrop_records()
        .iter()
        .copied()
        .find(|record| record.session() == second_session)
        .expect("successor menu must compile one backdrop");
    let actual_revision = current_plan.popup().revision();
    let stale_revision = PopupRoutingRevision::new_for_test(
        actual_revision
            .get()
            .checked_add(1)
            .expect("test routing revision must not exhaust"),
    );
    let stale_revision_frozen = FrozenTabListMenuBackdropClick {
        session: second_session,
        record: current_backdrop,
        revision: stale_revision,
    };
    let rejected = fixture
        .engine
        .finish_journal_tab_list_menu_backdrop(
            tab_strip_test_cause(),
            &stale_revision_frozen,
            &current_plan,
        )
        .expect("stale routing revision rejection must settle");
    assert_eq!(
        rejected,
        InteractionOutcome::Rejected(InteractionRejection::TabListMenuPopupRevisionChanged {
            session: second_session,
            expected: stale_revision,
            actual: actual_revision,
        })
    );
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .active_menu()
            .map(|menu| menu.session()),
        Some(second_session),
        "stale popup authority cannot dismiss the current menu"
    );
}
