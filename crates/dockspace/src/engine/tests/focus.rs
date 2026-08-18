//! Pane focus, reveal, payload focus, and effect acknowledgement tests.

use super::*;

struct FocusRevealFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    tabs: crate::ids::NodeId,
    binding: ViewportBinding,
    focus_generation: u64,
}

fn focus_reveal_fixture() -> FocusRevealFixture {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([
        ItemId::new(1),
        ItemId::new(2),
        ItemId::new(3),
        ItemId::new(4),
    ]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let workspace = builder
        .build()
        .expect("focus reveal workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("focus presentation host must mint");
    let token = WindowToken::new(1);
    let provider = test_platform_provider(&mut engine);
    let expected = engine.version();
    let registered = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::RegisterViewport {
            provider,
            expected,
            surface: SOURCE_SURFACE,
            token,
            role: ViewportRole::Root,
            recovery_target: None,
        },
    )
    .expect("viewport registration must reduce");
    let InputOutcome::ViewportRegistered { binding } = registered.reduced_inputs()[0].outcome()
    else {
        panic!("viewport registration must publish its exact binding");
    };
    let binding = *binding;
    let mut fixture = FocusRevealFixture {
        engine,
        presentation_host,
        tabs,
        binding,
        focus_generation: 0,
    };
    publish_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding)),
    );
    fixture
}

fn publish_focus_snapshot(
    fixture: &mut FocusRevealFixture,
    focused: Authority<GlobalFocusedWindow>,
) -> EngineTransition {
    fixture.focus_generation += 1;
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    let snapshot = test_platform_snapshot(
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(fixture.focus_generation),
            focused,
            Authority::Known(None),
        ),
        vec![
            ObservedWindow::new(fixture.binding).with_presentation_observation(
                WindowPresentationObservation::new(
                    fixture.binding,
                    crate::viewport::PresentationObservationGeneration::new(
                        fixture.focus_generation,
                    ),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
        ],
        Vec::new(),
        unknown_work_area_observation(fixture.focus_generation),
    )
    .expect("focus snapshot must be canonical");
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    let input = EngineInput::PublishPlatformSnapshot {
        provider,
        expected_epoch,
        snapshot,
    };
    let Some(pointer_provider) = fixture.engine.pointer_provider() else {
        return submit_test_input(&mut fixture.engine, fixture.presentation_host, input)
            .expect("focus snapshot must reduce");
    };
    let watermark = PointerEdgeSequence::new(0);
    let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("idle focus-frame journal must preserve its watermark");
    let mut stream = TestInputStream::resume(&fixture.engine, ENGINE_TEST_INPUT_SOURCE);
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    frame
        .submit_pointer_journal(pointer_provider, journal)
        .expect("active pointer provider must submit every host frame");
    let receipts = frame
        .pointer_receiver_candidates()
        .expect("idle pointer journal must freeze an exact candidate roster")
        .candidates()
        .iter()
        .cloned()
        .map(|candidate| candidate.receipt(PointerReceiverObservation::NotApplicable));
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(receipts)
                .expect("idle pointer receipts must be exact"),
        )
        .expect("idle pointer receipts must stage");
    stream
        .append(&mut frame, input)
        .expect("focus snapshot must fit the host-frame semantic phase");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("focus snapshot with pointer authority must reduce")
}

fn establish_known_all_released_pointer_authority(fixture: &mut FocusRevealFixture) {
    let provider = fixture
        .engine
        .create_pointer_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop-global pointer provider must mint");
    let checkpoint = PointerAuthorityCheckpoint::known(PointerEdgeSequence::new(0), Vec::new())
        .expect("empty complete pointer checkpoint must be canonical");
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(0),
        Vec::new(),
    )
    .expect("empty pointer journal must preserve its watermark")
    .with_authority_checkpoint(checkpoint)
    .expect("checkpoint must bind the journal predecessor");
    let mut frame = begin_test_host_frame(&fixture.engine, fixture.presentation_host);
    frame
        .submit_pointer_journal(provider, journal)
        .expect("pointer checkpoint must stage");
    let receipts = frame
        .pointer_receiver_candidates()
        .expect("pointer provider must freeze an exact candidate roster")
        .candidates()
        .iter()
        .cloned()
        .map(|candidate| candidate.receipt(PointerReceiverObservation::NotApplicable));
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new(receipts)
                .expect("empty-journal receipts must be exact"),
        )
        .expect("pointer checkpoint receipts must stage");
    complete_host_frame_with_explicit_surface_roster(&fixture.engine, &mut frame);
    frame
        .finish(&mut fixture.engine)
        .expect("complete pointer checkpoint must reduce");
}

fn selected_item(fixture: &FocusRevealFixture) -> Option<ItemId> {
    let Node::Tabs { selected, .. } = fixture
        .engine
        .workspace()
        .node(fixture.tabs)
        .expect("focus tabs must remain current")
    else {
        panic!("focus fixture node must remain tabs");
    };
    *selected
}

struct FocusEffectFixture {
    engine: DockEngine,
    presentation_host: PresentationHostLease,
    binding_a: ViewportBinding,
    binding_b: ViewportBinding,
    platform_generation: u64,
    focus_generation: u64,
}

fn focus_effect_fixture() -> FocusEffectFixture {
    let mut builder = Workspace::builder();
    let tabs_a = builder.insert_node(Node::tabs([ItemId::new(1)]));
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs_a));
    builder.set_root(TARGET_ROOT, RootRecord::new(tabs_b));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let workspace = builder
        .build()
        .expect("focus effect workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("focus presentation host must mint");
    let provider = test_platform_provider(&mut engine);
    let expected = engine.version();
    submit_test_batch(
        &mut engine,
        presentation_host,
        [
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: SOURCE_SURFACE,
                token: WindowToken::new(1),
                role: ViewportRole::Root,
                recovery_target: None,
            },
            EngineInput::RegisterViewport {
                provider,
                expected,
                surface: TARGET_SURFACE,
                token: WindowToken::new(2),
                role: ViewportRole::Root,
                recovery_target: None,
            },
        ],
    )
    .expect("viewport registrations must reduce");
    let binding_a = engine
        .viewport()
        .viewport(SOURCE_SURFACE)
        .expect("first viewport must be current")
        .binding();
    let binding_b = engine
        .viewport()
        .viewport(TARGET_SURFACE)
        .expect("second viewport must be current")
        .binding();
    let mut fixture = FocusEffectFixture {
        engine,
        presentation_host,
        binding_a,
        binding_b,
        platform_generation: 0,
        focus_generation: 0,
    };
    publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(None),
    );
    fixture
}

fn publish_effect_focus_snapshot(
    fixture: &mut FocusEffectFixture,
    focused: Authority<GlobalFocusedWindow>,
    acknowledged_effect: Authority<Option<crate::effect::EffectId>>,
) -> EngineTransition {
    fixture.focus_generation += 1;
    publish_effect_focus_snapshot_at(
        fixture,
        fixture.focus_generation,
        focused,
        acknowledged_effect,
    )
}

fn publish_effect_focus_snapshot_at(
    fixture: &mut FocusEffectFixture,
    generation: u64,
    focused: Authority<GlobalFocusedWindow>,
    acknowledged_effect: Authority<Option<crate::effect::EffectId>>,
) -> EngineTransition {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_global_focus_observation(PlatformCapability::Supported);
    capabilities.set_window_activation_control(PlatformCapability::Supported);
    fixture.platform_generation += 1;
    let provider_generation = fixture.platform_generation;
    let snapshot = test_platform_snapshot_at(
        provider_generation,
        capabilities,
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            focused,
            acknowledged_effect,
        ),
        vec![
            ObservedWindow::new(fixture.binding_a).with_presentation_observation(
                WindowPresentationObservation::new(
                    fixture.binding_a,
                    crate::viewport::PresentationObservationGeneration::new(generation),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
            ObservedWindow::new(fixture.binding_b).with_presentation_observation(
                WindowPresentationObservation::new(
                    fixture.binding_b,
                    crate::viewport::PresentationObservationGeneration::new(generation),
                    Authority::Known(WindowPresentationState::Visible),
                    PresentationEffectAcknowledgement::known(None),
                ),
            ),
        ],
        Vec::new(),
        unknown_work_area_observation(provider_generation),
    )
    .expect("focus effect snapshot must be canonical");
    let provider = test_platform_provider(&mut fixture.engine);
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPlatformSnapshot {
            provider,
            expected_epoch,
            snapshot,
        },
    )
    .expect("focus effect snapshot must reduce")
}

fn request_focus_effect(
    fixture: &mut FocusEffectFixture,
    binding: ViewportBinding,
    focus: PanelFocus,
) -> (
    crate::effect::EffectId,
    crate::viewport_focus::ActivationGeneration,
) {
    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(binding, focus),
        },
    )
    .expect("activation must reduce");
    let activation = transition
        .reduced_inputs()
        .iter()
        .find_map(|input| match input.outcome() {
            InputOutcome::ViewportActivationRequested { activation } => Some(*activation),
            _ => None,
        })
        .expect("activation input must publish its generation");
    let effect = transition
        .platform_effects()
        .iter()
        .find_map(|request| match request.effect() {
            crate::effect::PlatformEffect::RequestFocus {
                binding: requested, ..
            } if *requested == binding => Some(request.id()),
            _ => None,
        })
        .expect("activation must emit one exact focus effect");
    (effect, activation.generation())
}

fn observed_focus_effect_ids(transition: &EngineTransition) -> Vec<crate::effect::EffectId> {
    transition
        .focus_delta()
        .effects()
        .iter()
        .filter_map(|change| {
            change
                .observed()
                .map(crate::viewport_focus::ObservedPlatformFocusEffect::effect)
        })
        .collect()
}

fn assert_focus_effect_observed(engine: &DockEngine, effect: crate::effect::EffectId) {
    assert!(matches!(
        engine
            .viewport()
            .effects()
            .record(effect)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::ObservedApplied { .. })
    ));
}

#[test]
fn late_superseded_focus_ack_settles_only_its_effect_before_successor_completion() {
    let mut fixture = focus_effect_fixture();
    let binding_a = fixture.binding_a;
    let binding_b = fixture.binding_b;
    let (effect_a, activation_a) =
        request_focus_effect(&mut fixture, binding_a, PanelFocus::Item(ItemId::new(1)));
    let (effect_b, activation_b) =
        request_focus_effect(&mut fixture, binding_b, PanelFocus::Item(ItemId::new(2)));

    let late_a = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert_eq!(observed_focus_effect_ids(&late_a), vec![effect_a]);
    assert_focus_effect_observed(&fixture.engine, effect_a);
    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .map(crate::viewport_focus::PendingViewportActivation::generation),
        Some(activation_b),
        "late predecessor acknowledgement must not complete or cancel the successor"
    );
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .is_none()
    );

    let completed_b = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
        Authority::Known(Some(effect_b)),
    );
    assert_eq!(observed_focus_effect_ids(&completed_b), vec![effect_b]);
    assert_focus_effect_observed(&fixture.engine, effect_b);
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .is_none()
    );
    let intent = fixture
        .engine
        .viewport_focus()
        .pending_pane_intent()
        .expect("successor target observation must install its pane intent");
    assert_eq!(intent.activation(), Some(activation_b));
    assert_eq!(intent.surface(), binding_b.surface());
    assert_eq!(intent.native_guard(), Some(binding_b));
    assert_eq!(intent.focus(), PanelFocus::Item(ItemId::new(2)));
    assert_ne!(intent.activation(), Some(activation_a));
}

#[test]
fn one_envelope_can_settle_late_predecessor_and_complete_current_target() {
    let mut fixture = focus_effect_fixture();
    let binding_a = fixture.binding_a;
    let binding_b = fixture.binding_b;
    let (effect_a, _) =
        request_focus_effect(&mut fixture, binding_a, PanelFocus::Item(ItemId::new(1)));
    let (effect_b, activation_b) =
        request_focus_effect(&mut fixture, binding_b, PanelFocus::Item(ItemId::new(2)));

    let transition = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding_b)),
        Authority::Known(Some(effect_a)),
    );
    assert_eq!(
        observed_focus_effect_ids(&transition),
        vec![effect_a, effect_b],
        "FocusDelta must retain both exact-A and target-B settlements in effect order"
    );
    assert_focus_effect_observed(&fixture.engine, effect_a);
    assert_focus_effect_observed(&fixture.engine, effect_b);
    assert_eq!(
        transition.focus_delta().effects()[0]
            .observed()
            .map(crate::viewport_focus::ObservedPlatformFocusEffect::evidence),
        Some(crate::viewport_focus::PlatformFocusEvidence::ExactEffectAcknowledgement)
    );
    assert_eq!(
        transition.focus_delta().effects()[1]
            .observed()
            .map(crate::viewport_focus::ObservedPlatformFocusEffect::evidence),
        Some(crate::viewport_focus::PlatformFocusEvidence::NewerMatchingObservation)
    );
    let intent = fixture
        .engine
        .viewport_focus()
        .pending_pane_intent()
        .expect("current target evidence must complete the successor");
    assert_eq!(intent.activation(), Some(activation_b));
    assert_eq!(intent.surface(), binding_b.surface());
    assert_eq!(intent.native_guard(), Some(binding_b));
}

#[test]
fn wrong_stale_incarnation_and_duplicate_focus_acks_do_not_settle_again() {
    let mut fixture = focus_effect_fixture();
    let binding_a = fixture.binding_a;
    let binding_b = fixture.binding_b;
    let (effect_a, _) = request_focus_effect(&mut fixture, binding_a, PanelFocus::None);
    let (_, activation_b) = request_focus_effect(&mut fixture, binding_b, PanelFocus::None);

    let wrong = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(crate::effect::EffectId::new(9_999))),
    );
    assert!(observed_focus_effect_ids(&wrong).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(effect_a)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::Requested)
    );

    let stale_generation = fixture.focus_generation - 1;
    let stale = publish_effect_focus_snapshot_at(
        &mut fixture,
        stale_generation,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert!(observed_focus_effect_ids(&stale).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(effect_a)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::Requested)
    );

    let first = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert_eq!(observed_focus_effect_ids(&first), vec![effect_a]);
    let duplicate = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(effect_a)),
    );
    assert!(observed_focus_effect_ids(&duplicate).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport_focus()
            .pending_activation()
            .map(crate::viewport_focus::PendingViewportActivation::generation),
        Some(activation_b)
    );

    let stale_binding = ViewportBinding::new(
        binding_a.authority_domain(),
        binding_a.epoch(),
        binding_a.surface(),
        binding_a.token(),
        WindowIncarnation::new(binding_a.incarnation().get() + 1),
    );
    let stale_effect = fixture
        .engine
        .viewport
        .request_focus_binding(stale_binding)
        .expect("stale-incarnation effect must allocate for the invariant test");
    let stale_incarnation = publish_effect_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Foreign),
        Authority::Known(Some(stale_effect)),
    );
    assert!(observed_focus_effect_ids(&stale_incarnation).is_empty());
    assert_eq!(
        fixture
            .engine
            .viewport()
            .effects()
            .record(stale_effect)
            .map(crate::effect::EffectRecord::phase),
        Some(crate::effect::EffectPhase::Requested)
    );
}

#[test]
fn explicit_focus_atomically_reveals_hidden_item_without_acknowledging_it() {
    let mut fixture = focus_reveal_fixture();
    assert_eq!(selected_item(&fixture), Some(ItemId::new(1)));

    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(
                fixture.binding,
                PanelFocus::Item(ItemId::new(2)),
            ),
        },
    )
    .expect("explicit activation must reduce");

    assert_eq!(selected_item(&fixture), Some(ItemId::new(2)));
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .is_some()
    );
    assert_eq!(
        fixture.engine.viewport_focus().panel_focus(SOURCE_SURFACE),
        PanelFocusRecord::NoHistory,
        "selection and pane rendering cannot acknowledge focus"
    );
    assert!(transition.focus_delta().pane_intent().is_some());
}

#[test]
fn local_tab_selection_without_a_native_binding_publishes_pane_focus() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    let workspace = builder
        .build()
        .expect("local focus workspace must be valid");
    let mut engine =
        DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let presentation_host = engine
        .create_presentation_host()
        .expect("local focus presentation host must mint");
    let bounds =
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("local focus surface bounds must be valid");
    let scene = publish_surface_projection(&mut engine, presentation_host, SOURCE_SURFACE, bounds);
    let tab = engine
        .presentation_authority
        .scene
        .surface(SOURCE_SURFACE)
        .and_then(SurfaceScene::ready)
        .expect("the local focus surface must be ready")
        .candidate()
        .plan()
        .tab_records()
        .iter()
        .find(|record| record.id().item == ItemId::new(2))
        .map(|record| *record.id())
        .expect("the second tab must be presented");
    assert_eq!(engine.viewport_focus_binding(SOURCE_SURFACE), None);

    let expected = engine.version();
    let transition = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::SelectLocalSceneTab {
            expected,
            scene,
            tab,
        },
    )
    .expect("local tab selection must reduce");

    let Node::Tabs { selected, .. } = engine
        .workspace()
        .node(tabs)
        .expect("local focus tabs must remain current")
    else {
        panic!("local focus fixture node must remain tabs");
    };
    assert_eq!(*selected, Some(ItemId::new(2)));
    let request = engine
        .viewport_focus()
        .published_pane_intent()
        .expect("the committed boundary must publish pane focus");
    assert_eq!(request.surface(), SOURCE_SURFACE);
    assert_eq!(request.item(), Some(ItemId::new(2)));
    assert_eq!(request.native_guard(), None);
    assert!(transition.focus_delta().pane_intent().is_some());

    let expected_epoch = engine.version().epoch();
    let not_focused = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::PublishPaneFocusRequestObservation {
            expected_epoch,
            observation: PaneFocusRequestObservation::new(
                PaneFocusObservationGeneration::new(1),
                request,
                PaneFocusRequestObservationState::NotFocused,
            ),
        },
    )
    .expect("not-focused observation must reduce");
    assert!(matches!(
        not_focused.reduced_inputs()[0].outcome(),
        InputOutcome::PaneFocusObservationPublished {
            transition: PaneFocusObservationTransition::NotFocused { intent },
        } if *intent == request.id()
    ));
    assert_eq!(
        engine.viewport_focus().published_pane_intent(),
        Some(request)
    );

    let focused = submit_test_input(
        &mut engine,
        presentation_host,
        EngineInput::PublishPaneFocusRequestObservation {
            expected_epoch,
            observation: PaneFocusRequestObservation::new(
                PaneFocusObservationGeneration::new(2),
                request,
                PaneFocusRequestObservationState::Focused,
            ),
        },
    )
    .expect("focused observation must reduce");
    assert!(matches!(
        focused.reduced_inputs()[0].outcome(),
        InputOutcome::PaneFocusObservationPublished {
            transition: PaneFocusObservationTransition::Focused { cleared_intent },
        } if *cleared_intent == request.id()
    ));
    assert_eq!(engine.viewport_focus().published_pane_intent(), None);
    assert_eq!(
        engine.viewport_focus().panel_focus(SOURCE_SURFACE),
        PanelFocusRecord::Item(ItemId::new(2))
    );
}

#[test]
fn explicit_focus_minimally_reveals_from_the_current_strip_scroll() {
    let mut fixture = focus_reveal_fixture();
    let bar = TabBarSceneId {
        root: SOURCE_ROOT,
        tabs: fixture.tabs,
    };
    let key = TabStripStateKey::new(SOURCE_SURFACE, bar);
    fixture
        .engine
        .presentation_authority
        .tab_strip_states
        .set_tab_strip_scroll_offset(key, 0.0)
        .expect("explicit beginning offset is valid");
    let measurements = tab_strip_reducer_measurements_for(&fixture.engine, SOURCE_SURFACE, true);
    let before = compile_tab_strip_reducer_plan_for(&fixture.engine, SOURCE_SURFACE, &measurements);
    assert_eq!(
        before.tab_bar_records()[0]
            .members()
            .iter()
            .find(|member| member.tab().item == ItemId::new(4))
            .expect("focus target remains in the complete roster")
            .visibility(),
        TabStripMemberVisibility::Hidden
    );

    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(
                fixture.binding,
                PanelFocus::Item(ItemId::new(4)),
            ),
        },
    )
    .expect("explicit focus must reduce");
    assert_eq!(selected_item(&fixture), Some(ItemId::new(4)));
    assert!(transition.focus_delta().pane_intent().is_some());
    assert_eq!(
        fixture
            .engine
            .presentation_authority
            .tab_strip_states
            .state(key)
            .and_then(|state| state.scroll_offset()),
        Some(0.0)
    );

    let measurements = tab_strip_reducer_measurements_for(&fixture.engine, SOURCE_SURFACE, true);
    let after = compile_tab_strip_reducer_plan_for(&fixture.engine, SOURCE_SURFACE, &measurements);
    assert_eq!(
        after.tab_bar_records()[0]
            .members()
            .iter()
            .find(|member| member.tab().item == ItemId::new(4))
            .expect("focused target remains in the complete roster")
            .visibility(),
        TabStripMemberVisibility::Visible
    );
}

#[test]
fn platform_restore_reveals_the_exact_hidden_focus_history_item() {
    let mut fixture = focus_reveal_fixture();
    let expected_epoch = fixture.engine.version().epoch();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::PublishPaneFocusObservation {
            expected_epoch,
            observation: PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(1),
                fixture.binding,
                PanelFocus::Item(ItemId::new(2)),
            ),
        },
    )
    .expect("pane focus history must reduce");
    let source = fixture
        .engine
        .workspace()
        .capture_item_source(SOURCE_ROOT, fixture.tabs, ItemId::new(1))
        .expect("first item source must be current");
    let expected = fixture.engine.version();
    submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::WorkspaceCommand {
            expected,
            command: WorkspaceCommand::Select { source },
        },
    )
    .expect("selection must reduce");
    assert_eq!(selected_item(&fixture), Some(ItemId::new(1)));

    publish_focus_snapshot(&mut fixture, Authority::Known(GlobalFocusedWindow::Foreign));
    establish_known_all_released_pointer_authority(&mut fixture);
    let binding = fixture.binding;
    let restored = publish_focus_snapshot(
        &mut fixture,
        Authority::Known(GlobalFocusedWindow::Dock(binding)),
    );

    assert_eq!(selected_item(&fixture), Some(ItemId::new(2)));
    assert!(
        fixture
            .engine
            .viewport_focus()
            .pending_pane_intent()
            .is_some()
    );
    assert!(restored.focus_delta().pane_intent().is_some());
}

struct PayloadFocusFixture {
    engine: DockEngine,
    binding: ViewportBinding,
    item_payload: MovePayload,
    stale_item_payload: MovePayload,
    tabs_payload: MovePayload,
    subtree_payload: MovePayload,
}

fn payload_focus_fixture() -> PayloadFocusFixture {
    let mut builder = Workspace::builder();
    let left_tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(3)]));
    let right_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let source_split = builder.insert_node(
        Node::split(
            crate::graph::Axis::Horizontal,
            [left_tabs, right_tabs],
            [0.5, 0.5],
        )
        .expect("source split must be valid"),
    );
    let target_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
    builder.set_root(SOURCE_ROOT, RootRecord::new(source_split));
    builder.set_root(TARGET_ROOT, RootRecord::new(target_tabs));
    builder.set_surface(SOURCE_SURFACE, SurfacePresentation::with_main(SOURCE_ROOT));
    builder.set_surface(TARGET_SURFACE, SurfacePresentation::with_main(TARGET_ROOT));
    let workspace = builder
        .build()
        .expect("payload focus workspace must be valid");
    let item_payload = MovePayload::Item(
        workspace
            .capture_item_source(SOURCE_ROOT, left_tabs, ItemId::new(1))
            .expect("item payload must be current"),
    );
    let tabs_payload = MovePayload::Tabs(
        workspace
            .capture_node_source(SOURCE_ROOT, left_tabs)
            .expect("tabs payload must be current"),
    );
    let subtree_payload = MovePayload::Subtree(
        workspace
            .capture_node_source(SOURCE_ROOT, source_split)
            .expect("subtree payload must be current"),
    );
    let mut stale_item_payload = item_payload.clone();
    let wrong_fingerprint = workspace
        .capture_node_source(TARGET_ROOT, target_tabs)
        .expect("target source must be current")
        .fingerprint()
        .clone();
    let MovePayload::Item(stale_source) = &mut stale_item_payload else {
        unreachable!("fixture creates an item payload");
    };
    stale_source.fingerprint = wrong_fingerprint;

    let engine = DockEngine::new(workspace, DockPolicy::default()).expect("engine must be valid");
    let binding = ViewportBinding::new(
        engine.authority_domain,
        WorkspaceEpoch::new(0),
        SOURCE_SURFACE,
        WindowToken::new(1),
        WindowIncarnation::new(1),
    );
    PayloadFocusFixture {
        engine,
        binding,
        item_payload,
        stale_item_payload,
        tabs_payload,
        subtree_payload,
    }
}

fn record_payload_focus(
    engine: &mut DockEngine,
    binding: ViewportBinding,
    observation_generation: &mut u64,
    focus: PanelFocus,
) {
    *observation_generation += 1;
    assert!(matches!(
        engine.viewport_focus.publish_pane_focus_observation(
            PaneFocusObservation::new(
                PaneFocusObservationGeneration::new(*observation_generation),
                binding,
                focus,
            ),
            |candidate| candidate == binding,
            |surface, item| {
                surface == SOURCE_SURFACE
                    && [ItemId::new(1), ItemId::new(3), ItemId::new(4)].contains(&item)
            },
        ),
        PaneFocusObservationTransition::Applied { .. }
    ));
}

#[test]
fn payload_focus_is_frozen_only_for_an_exact_payload_member() {
    let PayloadFocusFixture {
        mut engine,
        binding,
        item_payload,
        stale_item_payload,
        tabs_payload,
        subtree_payload,
    } = payload_focus_fixture();
    let mut observation_generation = 0_u64;

    assert_eq!(
        engine
            .freeze_payload_focus(&item_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Preserve,
        "missing pane-focus history must remain an explicit no-op"
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::Item(ItemId::new(1)),
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&item_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Set(ItemId::new(1))
    );
    assert!(
        engine.freeze_payload_focus(&stale_item_payload).is_err(),
        "a stale payload must reject instead of fabricating a clear-focus command"
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::Item(ItemId::new(3)),
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&item_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Clear,
        "an inactive item drag must not invent pane focus"
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&tabs_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Set(ItemId::new(3))
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::Item(ItemId::new(4)),
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&tabs_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Clear,
        "a sibling item on the same surface is outside the exact tabs payload"
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&subtree_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Set(ItemId::new(4))
    );

    record_payload_focus(
        &mut engine,
        binding,
        &mut observation_generation,
        PanelFocus::None,
    );
    assert_eq!(
        engine
            .freeze_payload_focus(&subtree_payload)
            .expect("current payload must freeze"),
        PaneFocusDisposition::Clear
    );
}

#[test]
fn private_focus_generations_do_not_publish_state_or_focus_delta() {
    let mut fixture = counter_fixture();
    publish_counter_scene(&mut fixture);
    let stale_binding = ViewportBinding::new(
        fixture.engine.authority_domain,
        WorkspaceEpoch::new(0),
        SOURCE_SURFACE,
        WindowToken::new(99),
        WindowIncarnation::new(99),
    );
    let expected = fixture.engine.version();
    let transition = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ActivateViewport {
            expected,
            request: ViewportActivationRequest::explicit(stale_binding, PanelFocus::None),
        },
    )
    .expect("suppressed activation must reduce");
    let InputOutcome::ViewportActivationRequested { activation } =
        transition.reduced_inputs()[0].outcome()
    else {
        panic!("explicit activation must produce an activation outcome");
    };
    assert_eq!(
        activation.outcome(),
        ActivationStartOutcome::Suppressed(
            crate::viewport_focus::ActivationSuppression::StaleBinding
        )
    );
    assert!(transition.focus_delta().is_empty());
    assert!(!transition.published_state_changed());

    let maintenance = submit_test_input(
        &mut fixture.engine,
        fixture.presentation_host,
        EngineInput::ValidateWorkspace,
    )
    .expect("maintenance input must reduce");
    assert!(maintenance.focus_delta().is_empty());
    assert!(!maintenance.published_state_changed());
}

#[test]
fn action_batch_barrier_allows_an_unrelated_target_mutation() {
    let mut fixture = counter_fixture();
    let before = fixture.engine.workspace().clone();
    let roster =
        SurfaceRosterDisposition::capture(fixture.engine.workspace(), SOURCE_SURFACE, None)
            .expect("direct edge roster must freeze without coordinate authority");
    let barrier = BTreeMap::from([(SOURCE_SURFACE, roster)]);
    let target = fixture
        .engine
        .workspace()
        .capture_tab_target(TARGET_ROOT, fixture.target_tabs)
        .expect("target tabs must be current");
    let mut events = Vec::new();

    let application = fixture
        .engine
        .apply_interaction_command_with_barrier(
            InputSequence::new(1),
            &WorkspaceCommand::Open {
                item: ItemId::new(99),
                target: crate::command::DockTarget::Center(target),
            },
            Some(&barrier),
            &mut events,
        )
        .expect("unrelated target mutation must reduce");

    assert!(matches!(
        application,
        CommandApplication::Applied { changed: true, .. }
    ));
    assert_ne!(fixture.engine.workspace(), &before);
    assert!(!events.is_empty());
}
