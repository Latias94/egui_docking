use std::collections::BTreeMap;

use dockspace::engine::{
    CoreHostPresentationFrame, EngineError, HostPresentationDisposition,
    HostPresentationUnavailableReason,
};
use dockspace::geometry::LogicalPoint;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SourceSequence, StableInputSourceId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::interaction::{InteractionCancelReason, InteractionEventKind, InteractionStatus};
use dockspace::pointer_journal::{
    PointerCaptureOwner, PointerEdge, PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation,
    PointerEdgeSequence, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverObservation,
    PointerReceiverProbeReceipt, PointerReceiverReceiptBatch, PresentedPointerReceiverObservation,
};
use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::presentation_observation::{
    HostPresentationCaptureGeneration, HostPresentationObservation,
    HostPresentationObservationOutcome, HostPresentationObservationRejection,
    PresentationHostRetirementReason,
};
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::transition::SurfaceContributionOutcome;
use egui::{Context, Event, Id, Key, Modifiers, Pos2, RawInput, Rect, Ui, vec2};

use super::{
    Dockspace, DockspaceResponse, EguiEngineOwner, EguiFrameScheduleKey, HostFrameResponse,
    should_request_presentation_follow_up_pass,
};
use crate::projection::{TabStripStateMap, load_tab_strip_states};
use crate::render::{EguiDockRenderer, EguiRendererError, EguiSurfaceDraft};
use crate::renderer::consume_gesture_escape;
use crate::style::DockStyle;
use crate::{DockspaceError, PaneView};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(10);
const ITEM: ItemId = ItemId::new(100);
const ESCAPE_TEST_SOURCE: StableInputSourceId = StableInputSourceId::new(0x6573_6361_7065_5f74);

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("test workspace is valid")
}

fn context() -> Context {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    context
}

fn multipass_context() -> Context {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 3.try_into().expect("three is non-zero");
    });
    context
}

fn input() -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        ..RawInput::default()
    }
}

fn input_with_events(events: Vec<Event>) -> RawInput {
    #[cfg(not(egui_backend_event_envelope))]
    let events = events.into_iter().map(Into::into).collect();
    #[cfg(egui_backend_event_envelope)]
    let events = {
        let mut derivation =
            egui::BackendEventDerivation::known(egui::BackendEventSequence::new(1));
        events
            .into_iter()
            .map(|event| derivation.envelope(event))
            .collect()
    };
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        events,
        ..RawInput::default()
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) -> HostFrameResponse {
    let mut host = dockspace.begin_host_frame(key).expect("host frame begins");
    let mut paint = None;
    let _ = context.run_ui(input(), |ui| {
        paint = Some(host.show_surface(SURFACE, ui, panes));
    });
    paint
        .expect("egui invokes the surface callback")
        .expect("surface paints");
    host.end_host_frame().expect("host frame commits")
}

fn prepare_owned_surface_draft(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    key: EguiFrameScheduleKey,
) -> (
    CoreHostPresentationFrame,
    BTreeMap<SurfaceId, EguiSurfaceDraft>,
) {
    let mut host = dockspace.begin_host_frame(key).expect("host frame begins");
    let mut paint = None;
    let _ = context.run_ui(input(), |ui| {
        paint = Some(host.show_surface(SURFACE, ui, panes));
    });
    paint
        .expect("egui invokes the surface callback")
        .expect("surface paints");
    let core_frame = host
        .state
        .take_core_frame()
        .into_input()
        .expect("the framework host retains an input capability");
    let mut core_frame = core_frame
        .into_presentation()
        .expect("owned draft test closes semantic input before publication");
    let mut obligations = core_frame
        .take_presentation_obligations()
        .expect("owned draft test issues the exact physical roster")
        .into_iter()
        .map(|obligation| (obligation.slot().surface(), obligation))
        .collect::<BTreeMap<_, _>>();
    let mut drafts = host.state.take_drafts();
    for draft in drafts.values_mut() {
        if draft.paint().is_some() {
            draft.force_paint_only();
            let obligation = obligations
                .remove(&draft.surface())
                .expect("owned draft surface has one physical obligation");
            draft
                .stage_painted_output(&mut core_frame, obligation)
                .expect("input-free owned draft stages its publication once");
        }
    }
    for (_, obligation) in obligations {
        core_frame
            .resolve_presentation_obligation(
                obligation,
                HostPresentationDisposition::Unavailable(
                    HostPresentationUnavailableReason::OutputNotProduced,
                ),
            )
            .expect("unpainted owned draft slot settles explicitly");
    }
    drop(host);
    (core_frame, drafts)
}

fn stage_draft_contributions(
    core_frame: &mut CoreHostPresentationFrame,
    drafts: &mut BTreeMap<SurfaceId, EguiSurfaceDraft>,
) {
    for draft in drafts.values_mut() {
        draft
            .stage_core_contribution(core_frame)
            .expect("owned draft contributes once");
    }
}

fn run_automatic_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
) -> DockspaceResponse {
    let mut response = None;
    let _ = context.run_ui(input(), |ui| {
        response = Some(dockspace.show_single_surface(SURFACE, ui, panes));
    });
    response
        .expect("egui invokes the automatic surface callback")
        .expect("automatic surface paints")
}

fn run_authoritative_automatic_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
) -> DockspaceResponse {
    run_authoritative_automatic_input(context, dockspace, panes, input())
}

fn establish_outer_pointer_provider(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
) {
    let bootstrap = run_authoritative_automatic_frame(context, dockspace, panes);
    assert!(
        bootstrap
            .transitions()
            .iter()
            .all(|transition| transition.presentation_emissions().is_empty()),
        "the bootstrap frame measures without claiming a painted output"
    );
    let painted = run_authoritative_automatic_frame(context, dockspace, panes);
    assert!(
        painted
            .transitions()
            .iter()
            .any(|transition| !transition.presentation_emissions().is_empty()),
        "the next frame must establish one concrete presentation stream"
    );
    let _ = run_authoritative_automatic_frame(context, dockspace, panes);
    assert!(
        dockspace.pointer_input.provider().is_none(),
        "automatic presentation does not own the explicit outer pointer lane"
    );
    EguiEngineOwner::validate_surface_local_pointer_provider_scope(
        &dockspace.engine,
        SurfaceLocalPointerScope::new(
            dockspace.presentation_host,
            SurfaceLocalPointerEndpoint::Logical(SURFACE),
        ),
    )
    .expect("the exact active headless presentation stream authorizes outer enrollment");
}

fn run_authoritative_automatic_input(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    input: RawInput,
) -> DockspaceResponse {
    let mut response = None;
    let _ = crate::test_support::run_ui(context, input, |ui| {
        response = Some(dockspace.show_single_surface(SURFACE, ui, panes));
    });
    response
        .expect("egui invokes the authoritative automatic surface callback")
        .expect("authoritative automatic surface paints")
}

fn run_authoritative_automatic_passes(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
) -> Vec<DockspaceResponse> {
    let mut responses = Vec::new();
    let _ = crate::test_support::run_ui(context, input(), |ui| {
        responses.push(
            dockspace
                .show_single_surface(SURFACE, ui, panes)
                .expect("authoritative automatic surface paints"),
        );
    });
    assert!(!responses.is_empty(), "egui invokes the surface callback");
    responses
}

fn automatic_emission_count(dockspace: &Dockspace) -> usize {
    dockspace
        .presentation_ledger
        .diagnostics()
        .automatic_outputs()
}

fn escape_input() -> RawInput {
    let event = Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    #[cfg(not(egui_backend_event_envelope))]
    let events = vec![event.into()];
    #[cfg(egui_backend_event_envelope)]
    let events = {
        let mut derivation =
            egui::BackendEventDerivation::known(egui::BackendEventSequence::new(1));
        vec![derivation.envelope(event)]
    };
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        events,
        ..RawInput::default()
    }
}

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
}

fn arm_real_journal_click(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
) -> (InteractionStatus, SurfaceLocalPointerProvider) {
    for _ in 0..3 {
        let _ = run_authoritative_automatic_frame(context, dockspace, panes);
    }
    let projection = dockspace
        .engine
        .interaction_projection(SURFACE)
        .expect("the test provider publishes one interactive output");
    let close = projection
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabClose(_)))
        .expect("the fixture tab exposes one close receiver");
    let close_rect = close.hit().rect();
    let point = LogicalPoint::new(
        close_rect.min().x() + close_rect.width() * 0.5,
        close_rect.min().y() + close_rect.height() * 0.5,
    )
    .expect("the close receiver center is finite");
    let delivery = PointerReceiverDelivery::new(
        projection,
        PointerReceiverDeliveryDisposition::Dock(close.id()),
    )
    .expect("the close delivery binds to the current output");
    let provider = EguiEngineOwner::create_surface_local_pointer_provider(
        &mut dockspace.engine,
        SurfaceLocalPointerScope::new(
            dockspace.presentation_host,
            SurfaceLocalPointerEndpoint::Logical(SURFACE),
        ),
        PointerEdgeSequence::new(0),
    )
    .expect("the real surface-local provider is admitted");
    let sequence = PointerEdgeSequence::new(1);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        sequence,
        vec![PointerEdge::new(
            sequence,
            PointerId::new(7),
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Known(point),
            },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("the press journal is contiguous");
    let next_sequence = dockspace.last_host_frame.map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("fixture sequence advances")
    });
    let mut frame = dockspace
        .begin_host_frame(EguiFrameScheduleKey::new(next_sequence, 0))
        .expect("the journal host frame begins");
    let core_frame = frame
        .state
        .input_core_frame_mut()
        .expect("the framework facade owns an input capability");
    core_frame
        .submit_surface_pointer_journal(&provider, journal)
        .expect("the real journal stages");
    let candidate = core_frame
        .pointer_receiver_candidates()
        .expect("the press freezes a receiver candidate")
        .candidates()[0]
        .clone();
    core_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([candidate.receipt(
                PointerReceiverObservation::Presented(
                    PresentedPointerReceiverObservation::new([
                        PointerReceiverProbeReceipt::Delivery(delivery),
                    ])
                    .expect("the press answers its delivery probe"),
                ),
            )])
            .expect("the press receipt batch is exact"),
        )
        .expect("the press receipt stages");
    let mut paint = None;
    let _ = context.run_ui(input(), |ui| {
        paint = Some(frame.show_surface(SURFACE, ui, panes));
    });
    paint
        .expect("egui invokes the journal surface callback")
        .expect("the journal surface paints");
    frame
        .end_host_frame()
        .expect("the real journal press commits");
    assert_eq!(provider.committed_through(), sequence);
    (dockspace.engine.interaction().status(), provider)
}

fn submit_formal_click_escape(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    provider: &dockspace::pointer_journal::SurfaceLocalPointerProvider,
) -> HostFrameResponse {
    assert_eq!(
        dockspace.engine.pointer_provider(),
        Some(provider.lease()),
        "the click retains its exact affine producer",
    );
    let watermark = PointerEdgeSequence::new(1);
    let next_sequence = dockspace.last_host_frame.map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("fixture sequence advances")
    });
    let mut frame = dockspace
        .begin_host_frame(EguiFrameScheduleKey::new(next_sequence, 0))
        .expect("the formal Escape host frame begins");
    let mut formal_input = None;
    let _ = context.run_ui(escape_input(), |ui| {
        let escape_events = ui.input(|input| input.events.clone());
        formal_input = Some(frame.escape_input(SURFACE, ui.ctx(), &escape_events));
    });
    let formal_input = formal_input
        .expect("egui invokes the formal Escape input callback")
        .expect("the exact Escape envelope remains valid")
        .expect("the source surface maps Escape to one formal input")
        .into_input();
    let core_frame = frame
        .state
        .input_core_frame_mut()
        .expect("the framework facade owns an input capability");
    core_frame
        .submit_surface_pointer_journal(
            provider,
            PointerEdgeJournal::new(watermark, watermark, Vec::new())
                .expect("the no-edge journal preserves the real watermark"),
        )
        .expect("the mandatory no-edge journal stages");
    assert!(
        core_frame
            .pointer_receiver_candidates()
            .expect("the empty journal freezes an empty candidate roster")
            .candidates()
            .is_empty(),
    );
    core_frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([]).expect("the empty receipt batch is exact"),
        )
        .expect("the mandatory empty receipts stage");
    core_frame
        .append_input(ESCAPE_TEST_SOURCE, SourceSequence::new(1), formal_input)
        .expect("the formal Escape input stages");
    let mut paint = None;
    let _ = context.run_ui(input(), |ui| {
        paint = Some(frame.show_surface(SURFACE, ui, panes));
    });
    paint
        .expect("egui invokes the formal Escape surface callback")
        .expect("the formal Escape surface paints");
    frame
        .end_host_frame()
        .expect("the formal Escape frame commits")
}

#[test]
fn real_pressed_click_escape_is_callback_order_independent() {
    for source_order in [[false, true], [true, false]] {
        let host_context = multipass_context();
        let mut dockspace = Dockspace::builder("pressed-escape-order", workspace())
            .build()
            .expect("facade builds");
        let mut panes = TestPanes;
        let (status, provider) = arm_real_journal_click(&host_context, &mut dockspace, &mut panes);
        let InteractionStatus::Pressed { .. } = status else {
            panic!("the real journal press must own one click session");
        };
        let callback_context = context();
        let mut escape_consumptions = 0;
        let _ = callback_context.run_ui(escape_input(), |ui| {
            for is_source_surface in source_order {
                if consume_gesture_escape(ui, status, is_source_surface) {
                    escape_consumptions += 1;
                }
            }
        });
        assert_eq!(
            escape_consumptions, 1,
            "only the core-proven source callback may consume Escape",
        );
        let response =
            submit_formal_click_escape(&host_context, &mut dockspace, &mut panes, &provider);
        let transition = response.transition();
        assert_eq!(
            dockspace.engine.interaction().status(),
            InteractionStatus::Idle,
        );
        assert_eq!(dockspace.engine.active_close_plans().count(), 0);
        assert_eq!(
            transition
                .interaction_events()
                .iter()
                .filter_map(|event| match event.kind() {
                    InteractionEventKind::Cancelled { reason, .. } => Some(*reason),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            [InteractionCancelReason::Escape],
        );
    }
}

#[test]
fn already_retired_pointer_abort_compacts_the_exact_tombstone() {
    let context = multipass_context();
    let mut dockspace = Dockspace::builder("retired-pointer-abort", workspace())
        .build()
        .expect("facade builds");
    let mut panes = TestPanes;
    let (status, provider) = arm_real_journal_click(&context, &mut dockspace, &mut panes);
    assert!(matches!(status, InteractionStatus::Pressed { .. }));
    let workspace_epoch = dockspace.engine.version().epoch();
    let reservation = dockspace
        .pointer_input
        .reserve_install()
        .expect("the adapter incarnation remains available");
    dockspace
        .pointer_input
        .install_unbound(reservation, provider, SURFACE, workspace_epoch);
    let outcome = EguiEngineOwner::retire_presentation_host_for_test(
        &mut dockspace.engine,
        dockspace.presentation_host,
        PresentationHostRetirementReason::RuntimeDestroyed,
    )
    .expect("retiring the host also retires its surface-local pointer lease");
    let dockspace::transition::PresentationHostRetirementOutcome::Retired {
        affected_surfaces,
        transition,
        ..
    } = outcome
    else {
        panic!("the first host retirement must publish its complete transition");
    };
    assert_eq!(affected_surfaces, [SURFACE]);
    assert!(transition.interaction_events().iter().any(|event| {
        matches!(
            event.kind(),
            InteractionEventKind::Cancelled {
                reason: InteractionCancelReason::SceneUnavailable,
                ..
            }
        )
    }));
    let retired_at = dockspace.engine.last_reducer_tick();
    assert_eq!(
        dockspace
            .engine
            .runtime_retention_manifest()
            .pointer()
            .retired_lease_guards(),
        1
    );

    dockspace
        .abort_pointer_input()
        .expect("the drained adapter producer compacts an existing tombstone");

    assert_eq!(dockspace.engine.last_reducer_tick(), retired_at);
    assert_eq!(dockspace.pointer_input.provider(), None);
    let retention = dockspace.engine.runtime_retention_manifest().pointer();
    assert_eq!(retention.retired_lease_guards(), 0);
    assert_eq!(retention.compacted_retirement_ranges(), 1);
}

#[test]
fn dropped_outer_frame_retires_captured_pointer_instead_of_discarding_release() {
    let context = multipass_context();
    let mut dockspace = Dockspace::builder("dropped-outer-release", workspace())
        .build()
        .expect("facade builds");
    let mut panes = TestPanes;
    let (status, provider) = arm_real_journal_click(&context, &mut dockspace, &mut panes);
    assert!(matches!(status, InteractionStatus::Pressed { .. }));

    let reservation = dockspace
        .pointer_input
        .reserve_install()
        .expect("the adapter can enroll the exact provider");
    dockspace.pointer_input.install_unbound(
        reservation,
        provider,
        SURFACE,
        dockspace.engine.version().epoch(),
    );

    let next_sequence = dockspace
        .last_host_frame
        .map_or(1, |key| key.sequence() + 1);
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(next_sequence, 0))
        .expect("the outer frame begins");
    let release = Event::PointerButton {
        pos: Pos2::new(32.0, 32.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    };
    frame
        .run_surface(
            SURFACE,
            &context,
            input_with_events(vec![release]),
            &mut panes,
        )
        .expect("the release is retained by the outer frame before its seal");

    // Dropping the host frame must retire the provider after its core candidate
    // is dropped. It must not merely delete the staged release from the adapter.
    drop(frame);
    assert_eq!(dockspace.pointer_input.provider(), None);
    assert_eq!(dockspace.core_engine().pointer_provider(), None);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn post_pointer_prepare_error_drops_core_guard_before_retiring_provider() {
    let context = multipass_context();
    let mut dockspace = Dockspace::builder("post-pointer-prepare-error", workspace())
        .build()
        .expect("facade builds");
    let mut panes = TestPanes;
    let (status, provider) = arm_real_journal_click(&context, &mut dockspace, &mut panes);
    assert!(matches!(status, InteractionStatus::Pressed { .. }));

    let reservation = dockspace
        .pointer_input
        .reserve_install()
        .expect("the adapter can enroll the exact provider");
    dockspace.pointer_input.install_unbound(
        reservation,
        provider,
        SURFACE,
        dockspace.engine.version().epoch(),
    );
    dockspace.semantic_source_sequence = SourceSequence::new(u64::MAX);

    let next_sequence = dockspace
        .last_host_frame
        .map_or(1, |key| key.sequence() + 1);
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(next_sequence, 0))
        .expect("the outer frame begins");
    let release = Event::PointerButton {
        pos: Pos2::new(32.0, 32.0),
        button: egui::PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    };
    let escape = Event::Key {
        key: Key::Escape,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    };
    frame
        .run_surface(
            SURFACE,
            &context,
            input_with_events(vec![release, escape]),
            &mut panes,
        )
        .expect("the release and later semantic action are staged");

    let error = frame
        .finish()
        .expect_err("the post-pointer semantic sequence is exhausted");
    assert!(matches!(
        error,
        DockspaceError::InputSourceSequenceExhausted { .. }
    ));
    assert_eq!(dockspace.pointer_input.provider(), None);
    assert_eq!(dockspace.core_engine().pointer_provider(), None);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn deferred_pointer_abort_retries_before_the_next_public_boundary() {
    let context = multipass_context();
    let mut dockspace = Dockspace::builder("deferred-pointer-abort", workspace())
        .build()
        .expect("facade builds");
    let mut panes = TestPanes;
    let (status, provider) = arm_real_journal_click(&context, &mut dockspace, &mut panes);
    assert!(matches!(status, InteractionStatus::Pressed { .. }));

    let reservation = dockspace
        .pointer_input
        .reserve_install()
        .expect("the adapter can enroll the exact provider");
    dockspace.pointer_input.install_unbound(
        reservation,
        provider,
        SURFACE,
        dockspace.engine.version().epoch(),
    );

    let mut prelude =
        EguiEngineOwner::begin_host_frame(&mut dockspace.engine, dockspace.presentation_host)
            .expect("a low-level frame begins");
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .expect("the low-level frame submits its observation");
    let mut core_frame = prelude
        .seal(EguiEngineOwner::engine(&dockspace.engine))
        .expect("the low-level frame seals");
    dockspace
        .pointer_input
        .submit_empty_interval(&mut core_frame)
        .expect("the low-level frame owns one in-flight producer attempt");

    let error = dockspace
        .abort_pointer_input()
        .expect_err("an in-flight core guard blocks provider retirement");
    assert!(matches!(error, DockspaceError::PointerInputFrameInFlight));
    assert!(dockspace.pending_pointer_abort);
    assert!(dockspace.pointer_input.provider().is_some());

    drop(core_frame);
    dockspace
        .ensure_native_session_idle()
        .expect("the next public boundary reaps the deferred exact retirement");
    assert!(!dockspace.pending_pointer_abort);
    assert_eq!(dockspace.pointer_input.provider(), None);
    assert_eq!(dockspace.core_engine().pointer_provider(), None);
    assert_eq!(
        dockspace.core_engine().interaction().status(),
        InteractionStatus::Idle
    );
}

#[test]
fn outer_frame_without_a_surface_callback_submits_an_empty_pointer_interval() {
    let mut dockspace = Dockspace::builder("empty-outer-pointer-interval", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    establish_outer_pointer_provider(&context, &mut dockspace, &mut panes);
    let next_sequence = dockspace.last_host_frame.map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("fixture sequence advances")
    });
    let frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(next_sequence, 0))
        .expect("outer frame derives its physical roster");
    let provider = frame
        .inner
        .dockspace
        .pointer_input
        .provider()
        .expect("the sole scheduled surface enrolls a pointer provider");

    let commit = frame
        .finish()
        .expect("an absent callback reduces as unavailable with an empty pointer interval");

    assert_eq!(commit.outputs().len(), 0);
    assert_eq!(dockspace.engine.pointer_provider(), Some(provider));
    assert_eq!(dockspace.pointer_input.provider(), Some(provider));
}

#[test]
fn confirmed_outer_surface_without_pointer_capture_fails_closed() {
    let context = context();
    let mut panes = TestPanes;
    let mut dockspace = Dockspace::builder("missing-outer-pointer-capture", workspace())
        .build()
        .expect("facade builds");
    establish_outer_pointer_provider(&context, &mut dockspace, &mut panes);
    let next_sequence = dockspace.last_host_frame.map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("fixture sequence advances")
    });
    let mut frame = dockspace
        .begin_outer_frame(EguiFrameScheduleKey::new(next_sequence, 0))
        .expect("outer frame derives its physical roster");
    let mut paint = None;
    let raw_input = input();
    let viewport = raw_input.viewport_id;
    let output = context.run_ui(raw_input, |ui| {
        paint = Some(frame.inner.show_surface(SURFACE, ui, &mut panes));
    });
    paint
        .expect("egui invokes the surface callback")
        .expect("surface paints");
    frame
        .inner
        .confirm_surface_output(SURFACE, &context, viewport, output)
        .expect("the split-output test confirms the exact painted output");
    let provider = frame
        .inner
        .dockspace
        .pointer_input
        .provider()
        .expect("the outer surface retains its pointer provider");

    let error = frame
        .finish()
        .expect_err("paint without an exact pointer epoch must fail closed");

    assert!(matches!(
        error,
        DockspaceError::CoreHostFrame(
            dockspace::engine::CoreHostFrameError::PointerJournalMissingBeforePresentation {
                provider: missing,
            },
        ) if missing == provider
    ));
}

#[test]
fn successful_pointer_abort_cancels_the_active_gesture_and_compacts_its_guard() {
    let context = multipass_context();
    let mut dockspace = Dockspace::builder("successful-pointer-abort", workspace())
        .build()
        .expect("facade builds");
    let mut panes = TestPanes;
    let (status, provider_owner) = arm_real_journal_click(&context, &mut dockspace, &mut panes);
    assert!(matches!(status, InteractionStatus::Pressed { .. }));
    let provider = dockspace
        .engine
        .pointer_provider()
        .expect("active provider");
    assert_eq!(provider_owner.lease(), provider);
    let workspace_epoch = dockspace.engine.version().epoch();
    let reservation = dockspace
        .pointer_input
        .reserve_install()
        .expect("the adapter incarnation remains available");
    dockspace
        .pointer_input
        .install_unbound(reservation, provider_owner, SURFACE, workspace_epoch);
    let next_sequence = dockspace.last_host_frame.map_or(1, |key| {
        key.sequence()
            .checked_add(1)
            .expect("fixture sequence advances")
    });

    drop(
        dockspace
            .begin_host_frame(EguiFrameScheduleKey::new(next_sequence, 0))
            .expect("retirement succeeds before the next host frame"),
    );

    assert_eq!(dockspace.pointer_input.provider(), None);
    assert_eq!(
        dockspace.engine.interaction().status(),
        InteractionStatus::Idle
    );
    let retention = dockspace.engine.runtime_retention_manifest().pointer();
    assert_eq!(retention.retired_lease_guards(), 0);
    assert_eq!(retention.compacted_retirement_ranges(), 1);
}

#[test]
fn repeated_surface_local_abort_keeps_pointer_retention_constant() {
    let mut dockspace = Dockspace::builder("pointer-abort-soak", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    establish_outer_pointer_provider(&context, &mut dockspace, &mut panes);

    for _ in 0..1_000 {
        dockspace
            .ensure_outer_pointer_provider()
            .expect("the surface-local producer enrolls");
        dockspace
            .abort_pointer_input()
            .expect("the drained producer retires and compacts atomically");
    }

    let retention = dockspace.engine.runtime_retention_manifest().pointer();
    assert_eq!(retention.active_provider_count(), 0);
    assert_eq!(retention.retired_lease_guards(), 0);
    assert_eq!(retention.compacted_retirement_ranges(), 1);
    assert_eq!(retention.logical_compacted_leases(), 1_000);
    assert_eq!(retention.retained_structure_count(), 1);
}

#[test]
fn explicit_host_frames_do_not_infer_final_presentation() {
    let mut dockspace = Dockspace::builder("paint-ticket", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;

    let first = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let ticket = match first.transition().surface_contributions() {
        [SurfaceContributionOutcome::Ready { ticket, .. }] => *ticket,
        outcomes => panic!("first paint must publish one ready output: {outcomes:?}"),
    };
    assert!(first.transition().presentation_emissions().is_empty());
    assert!(first.transition().presentation_observations().is_empty());

    let second = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(2, 0),
    );
    assert!(second.transition().presentation_emissions().is_empty());
    assert!(second.transition().presentation_observations().is_empty());
    assert!(matches!(
        second.transition().surface_contributions(),
        [SurfaceContributionOutcome::Retained { ticket: actual, .. }] if *actual == ticket
    ));
    let third = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(3, 0),
    );
    assert!(third.transition().presentation_emissions().is_empty());
    assert!(third.transition().presentation_observations().is_empty());
    assert!(matches!(
        third.transition().surface_contributions(),
        [SurfaceContributionOutcome::Retained { ticket: actual, .. }] if *actual == ticket
    ));
}

#[test]
fn automatic_frames_without_terminal_provider_do_not_create_pending_outputs() {
    let mut dockspace = Dockspace::builder("automatic-presentation", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;

    for _ in 0..64 {
        let response = run_automatic_frame(&context, &mut dockspace, &mut panes);
        assert!(
            response.transitions()[0]
                .presentation_observations()
                .is_empty()
        );
        assert!(
            response.transitions()[0]
                .presentation_emissions()
                .is_empty()
        );
        assert!(!response.interactions_current());
    }
    let diagnostics = dockspace.core_engine().presentation_ledger_diagnostics();
    assert_eq!(diagnostics.active_streams(), 0);
    assert_eq!(diagnostics.pending_streams(), 0);
    assert_eq!(diagnostics.pending_outputs(), 0);
}

#[test]
fn accepted_terminal_watermarks_bound_the_automatic_emission_map() {
    let mut dockspace = Dockspace::builder("automatic-presentation-retirement", workspace())
        .build()
        .expect("facade builds");
    let context = multipass_context();
    let mut panes = TestPanes;
    let mut settled = std::collections::BTreeSet::new();
    let mut observed_multi_emission_boundary = false;

    for _ in 0..128 {
        let responses = run_authoritative_automatic_passes(&context, &mut dockspace, &mut panes);
        observed_multi_emission_boundary |= responses.len() > 1;
        for outcome in responses
            .iter()
            .flat_map(DockspaceResponse::transitions)
            .flat_map(|transition| transition.presentation_observations())
        {
            match outcome {
                HostPresentationObservationOutcome::Retired {
                    stream,
                    settled_through,
                    ..
                } => assert!(
                    settled.insert((*stream, *settled_through)),
                    "one terminal watermark must not be submitted twice",
                ),
                HostPresentationObservationOutcome::Rejected { reason, .. } => {
                    panic!("automatic provider submitted an invalid observation: {reason:?}")
                }
                HostPresentationObservationOutcome::NoUpdate { .. }
                | HostPresentationObservationOutcome::CapturedUnknown { .. } => {}
            }
        }

        let adapter_pending = automatic_emission_count(&dockspace);
        let core_pending = dockspace
            .core_engine()
            .presentation_ledger_diagnostics()
            .pending_outputs();
        assert_eq!(adapter_pending, core_pending);
        assert!(
            adapter_pending <= 2,
            "settled emissions must not accumulate across host boundaries",
        );
    }

    assert!(
        observed_multi_emission_boundary,
        "promotion must exercise more than one egui pass in one host boundary",
    );
}

#[test]
fn unknown_capture_keeps_outputs_for_a_later_terminal_retry() {
    let mut dockspace = Dockspace::builder("automatic-presentation-unknown", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;

    let bootstrap = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    assert!(
        bootstrap.transitions()[0]
            .presentation_emissions()
            .is_empty()
    );
    let first = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    let first_output = first.transitions()[0].presentation_emissions()[0].output();
    assert_eq!(automatic_emission_count(&dockspace), 1);

    crate::test_support::remove_presentation_provider(&context);
    let unknown = run_automatic_frame(&context, &mut dockspace, &mut panes);
    assert!(matches!(
        unknown.transitions()[0].presentation_observations(),
        [HostPresentationObservationOutcome::CapturedUnknown { stream, .. }]
            if *stream == first_output.stream()
    ));
    assert!(
        dockspace
            .presentation_ledger
            .contains_automatic_output(first_output.stream(), first_output.key()),
        "Unknown is not a terminal fact and cannot reclaim the output",
    );
    assert_eq!(
        dockspace
            .core_engine()
            .presentation_ledger_diagnostics()
            .pending_outputs(),
        1,
    );

    let _ = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    let terminal = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    assert!(
        terminal
            .transitions()
            .iter()
            .flat_map(|transition| transition.presentation_observations())
            .any(|outcome| matches!(
                outcome,
                HostPresentationObservationOutcome::Retired {
                    retired_output_count: 2,
                    ..
                }
            ))
    );
    assert!(
        !dockspace
            .presentation_ledger
            .contains_automatic_output(first_output.stream(), first_output.key()),
        "an accepted terminal retry must reclaim the earlier Unknown output",
    );
    assert_eq!(
        automatic_emission_count(&dockspace),
        dockspace
            .core_engine()
            .presentation_ledger_diagnostics()
            .pending_outputs(),
    );
}

#[test]
fn presentation_follow_up_pass_requires_accepted_promotion() {
    let mut dockspace = Dockspace::builder("presentation-follow-up", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let bootstrap = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    assert!(
        bootstrap.transitions()[0]
            .presentation_emissions()
            .is_empty()
    );
    let first = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    let output = first.transitions()[0].presentation_emissions()[0].output();
    let stream = output.stream();
    let key = output.key();
    let non_promoting = vec![
        HostPresentationObservationOutcome::NoUpdate { stream },
        HostPresentationObservationOutcome::CapturedUnknown {
            stream,
            generation: HostPresentationCaptureGeneration::new(1),
            reason: AuthorityUnavailableReason::ProviderUnavailable,
        },
        HostPresentationObservationOutcome::Retired {
            stream,
            generation: HostPresentationCaptureGeneration::new(2),
            settled_through: key,
            presented: Authority::Known(None),
            retired_output_count: 1,
            promotion_eligible: false,
        },
        HostPresentationObservationOutcome::Rejected {
            stream,
            reason: HostPresentationObservationRejection::SettlementNotPending { submitted: key },
        },
    ];
    assert!(!should_request_presentation_follow_up_pass(&non_promoting));
    assert!(should_request_presentation_follow_up_pass(&[
        HostPresentationObservationOutcome::Retired {
            stream,
            generation: HostPresentationCaptureGeneration::new(3),
            settled_through: key,
            presented: Authority::Known(Some(key)),
            retired_output_count: 1,
            promotion_eligible: true,
        },
    ]));
}

#[test]
fn missing_surface_callback_submits_explicit_deferred_unavailable_fact() {
    let mut dockspace = Dockspace::builder("missing-surface", workspace())
        .build()
        .expect("facade builds");
    let response = dockspace
        .begin_host_frame(EguiFrameScheduleKey::new(1, 0))
        .expect("host frame begins")
        .end_host_frame()
        .expect("host frame commits");
    assert!(matches!(
        response.transition().surface_contributions(),
        [SurfaceContributionOutcome::Unavailable { .. }]
    ));
    assert!(response.transition().presentation_emissions().is_empty());
}

#[test]
fn host_frame_key_cannot_repeat_after_a_successful_commit() {
    let mut dockspace = Dockspace::builder("host-key", workspace())
        .build()
        .expect("facade builds");
    let key = EguiFrameScheduleKey::new(1, 0);
    dockspace
        .begin_host_frame(key)
        .expect("host frame begins")
        .mark_surface_unavailable(SURFACE, MeasurementUnavailableReason::Deferred)
        .expect("unavailable contribution prepares");
    // The uncommitted temporary frame is dropped; it cannot consume the key.
    dockspace
        .begin_host_frame(key)
        .expect("dropped host frame does not commit its identity")
        .end_host_frame()
        .expect("host frame commits");
    assert!(dockspace.begin_host_frame(key).is_err());
}

#[test]
fn dropped_owned_surface_draft_does_not_publish_adapter_sidecars() {
    let mut dockspace = Dockspace::builder("owned-draft-abort", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let id = dockspace.id();
    let before = dockspace.renderer.sidecar_diagnostics();

    let (core_frame, drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    assert_eq!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
    drop(drafts);
    drop(core_frame);

    assert_eq!(dockspace.renderer.sidecar_diagnostics(), before);
    assert_eq!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
}

#[test]
fn duplicate_owned_draft_staging_does_not_publish_adapter_sidecars() {
    let mut dockspace = Dockspace::builder("owned-draft-duplicate", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let id = dockspace.id();
    let before = dockspace.renderer.sidecar_diagnostics();
    let (mut core_frame, mut drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    let draft = drafts.get_mut(&SURFACE).expect("surface draft exists");

    draft
        .stage_core_contribution(&mut core_frame)
        .expect("first contribution stages");
    assert!(matches!(
        draft.stage_core_contribution(&mut core_frame),
        Err(DockspaceError::Renderer(
            EguiRendererError::ContributionAlreadySubmitted { surface: SURFACE }
        ))
    ));

    assert_eq!(dockspace.renderer.sidecar_diagnostics(), before);
    assert_eq!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
}

#[test]
fn failed_core_finish_does_not_publish_owned_draft_sidecars() {
    let mut dockspace = Dockspace::builder("owned-draft-finish-error", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let id = dockspace.id();
    let before = dockspace.renderer.sidecar_diagnostics();
    let (mut core_frame, mut drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    stage_draft_contributions(&mut core_frame, &mut drafts);
    EguiEngineOwner::create_presentation_host(&mut dockspace.engine)
        .expect("advancing the host frontier invalidates the prepared frame");
    assert!(
        EguiEngineOwner::prepare_host_presentation_frame(&mut dockspace.engine, core_frame,)
            .is_err()
    );

    assert_eq!(dockspace.renderer.sidecar_diagnostics(), before);
    assert_eq!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
}

#[test]
fn owned_core_candidate_cannot_overwrite_an_advanced_host_frontier() {
    let mut dockspace = Dockspace::builder("owned-candidate-stale-fence", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let (mut core_frame, mut drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    stage_draft_contributions(&mut core_frame, &mut drafts);
    let before_tick = dockspace.engine.last_reducer_tick();
    let prepared =
        EguiEngineOwner::prepare_owned_host_presentation_frame(&dockspace.engine, core_frame)
            .expect("the owned core candidate prepares without publishing");
    EguiEngineOwner::create_presentation_host(&mut dockspace.engine)
        .expect("a later host frontier is minted independently");

    assert!(matches!(
        EguiEngineOwner::commit_owned_host_presentation_frame(&mut dockspace.engine, prepared),
        Err(DockspaceError::Engine(
            EngineError::HostFramePresentationHostFrontierStale { .. }
        ))
    ));
    assert_eq!(dockspace.engine.last_reducer_tick(), before_tick);
}

#[test]
fn cross_renderer_preflight_rejects_before_core_commit() {
    let mut dockspace = Dockspace::builder("owned-draft-cross-renderer", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let id = dockspace.id();
    let (mut core_frame, mut drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    stage_draft_contributions(&mut core_frame, &mut drafts);
    let before_tick = dockspace.engine.last_reducer_tick();
    let prepared_core =
        EguiEngineOwner::prepare_host_presentation_frame(&mut dockspace.engine, core_frame)
            .expect("core frame prepares without publishing");
    let other = EguiDockRenderer::new(Id::new("other-renderer"), DockStyle::default())
        .expect("second renderer builds");
    let before = other.sidecar_diagnostics();

    assert!(matches!(
        other.prepare_frame(prepared_core.transition(), drafts, BTreeMap::new()),
        Err(DockspaceError::Renderer(
            EguiRendererError::RendererBindingMismatch { surface: SURFACE }
        ))
    ));
    drop(prepared_core);
    assert_eq!(dockspace.engine.last_reducer_tick(), before_tick);
    assert_eq!(other.sidecar_diagnostics(), before);
    assert_eq!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
}

#[test]
fn changed_style_preflight_rejects_before_core_commit() {
    let mut dockspace = Dockspace::builder("owned-draft-style", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let id = dockspace.id();
    let (mut core_frame, mut drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    stage_draft_contributions(&mut core_frame, &mut drafts);
    let mut style = dockspace.renderer.style().clone();
    style.tab_bar_height += 1.0;
    dockspace
        .renderer
        .replace_style(style)
        .expect("style revision advances");
    let before = dockspace.renderer.sidecar_diagnostics();
    let before_tick = dockspace.engine.last_reducer_tick();
    let prepared_core =
        EguiEngineOwner::prepare_host_presentation_frame(&mut dockspace.engine, core_frame)
            .expect("core frame prepares without publishing");

    assert!(matches!(
        dockspace
            .renderer
            .prepare_frame(prepared_core.transition(), drafts, BTreeMap::new()),
        Err(DockspaceError::Renderer(
            EguiRendererError::RendererStyleRevisionMismatch {
                surface: SURFACE,
                ..
            }
        ))
    ));
    drop(prepared_core);
    assert_eq!(dockspace.engine.last_reducer_tick(), before_tick);
    assert_eq!(dockspace.renderer.sidecar_diagnostics(), before);
    assert_eq!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
}

#[test]
fn prepared_core_and_renderer_commit_owned_sidecars_once() {
    let mut dockspace = Dockspace::builder("owned-draft-success", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let id = dockspace.id();
    let before = dockspace.renderer.sidecar_diagnostics();
    let (mut core_frame, mut drafts) = prepare_owned_surface_draft(
        &context,
        &mut dockspace,
        &mut panes,
        EguiFrameScheduleKey::new(1, 0),
    );
    stage_draft_contributions(&mut core_frame, &mut drafts);
    let prepared_core =
        EguiEngineOwner::prepare_host_presentation_frame(&mut dockspace.engine, core_frame)
            .expect("core frame prepares");
    let prepared_renderer = dockspace
        .renderer
        .prepare_frame(prepared_core.transition(), drafts, BTreeMap::new())
        .expect("matching renderer preflights owned drafts");
    let transition = prepared_core
        .commit()
        .expect("prepared core and affine pointer state commit together");
    let acceptance = prepared_renderer.commit(
        &mut dockspace.renderer,
        EguiEngineOwner::engine(&dockspace.engine),
    );
    let after = dockspace.renderer.sidecar_diagnostics();
    assert_eq!(dockspace.engine.last_reducer_tick(), transition.tick());
    assert_eq!(acceptance.surfaces().len(), 1);
    assert!(after.paint_resource_set_count > before.paint_resource_set_count);
    assert_eq!(
        after.receiver_presentation_count,
        before.receiver_presentation_count
    );
    assert_ne!(
        load_tab_strip_states(&context, id),
        TabStripStateMap::default()
    );
}

#[test]
fn receiver_resources_remain_queryable_until_core_reclaims_the_exact_emission() {
    let mut dockspace = Dockspace::builder("receiver-retention-authority", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;

    for _ in 0..3 {
        let _ = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
    }
    let (output, authority) = {
        let projection = dockspace
            .core_engine()
            .interaction_projection(SURFACE)
            .expect("the deterministic provider presents one interactive output");
        (projection.output_ticket(), projection.authority())
    };
    let (viewport, widget_pass, receiver) = dockspace
        .renderer
        .first_receiver_registration_for(authority)
        .expect("the presented output retains exact receiver registrations");

    assert!(
        dockspace
            .core_engine()
            .presentation_retention_manifest()
            .retains(authority.emission())
    );
    assert!(matches!(
        dockspace
            .renderer
            .resolve_receiver(output, authority, viewport, widget_pass, receiver,),
        crate::receiver::PaintReceiverLookup::Dock(_)
    ));

    for _ in 0..4 {
        let _ = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
        if !dockspace
            .core_engine()
            .presentation_retention_manifest()
            .retains(authority.emission())
        {
            break;
        }
    }
    assert!(
        !dockspace
            .core_engine()
            .presentation_retention_manifest()
            .retains(authority.emission()),
        "a superseded terminal emission must eventually leave core retention authority",
    );
    assert_eq!(
        dockspace
            .renderer
            .resolve_receiver(output, authority, viewport, widget_pass, receiver,),
        crate::receiver::PaintReceiverLookup::GenerationUnavailable,
    );
}

#[test]
fn ten_thousand_presented_outputs_keep_receiver_sidecars_bounded() {
    let mut dockspace = Dockspace::builder("receiver-retention-soak", workspace())
        .build()
        .expect("facade builds");
    let context = context();
    let mut panes = TestPanes;
    let mut maximum_receivers = 0;
    let mut maximum_retained_emissions = 0;

    for _ in 0..10_000 {
        let _ = run_authoritative_automatic_frame(&context, &mut dockspace, &mut panes);
        let diagnostics = dockspace.renderer.sidecar_diagnostics();
        let manifest = dockspace.core_engine().presentation_retention_manifest();
        let presentation_ledger = dockspace.presentation_ledger.diagnostics();
        maximum_receivers = maximum_receivers.max(diagnostics.receiver_presentation_count);
        maximum_retained_emissions = maximum_retained_emissions.max(manifest.emission_count());
        assert!(
            diagnostics.receiver_presentation_count <= manifest.emission_count(),
            "one-surface receiver storage must be bounded by exact core emission authority",
        );
        assert!(
            presentation_ledger.capture_generation_streams() <= manifest.stream_count(),
            "capture generations must be bounded by exact core stream authority",
        );
    }

    assert!(maximum_receivers <= 2, "observed {maximum_receivers}");
    assert!(
        maximum_retained_emissions <= 2,
        "observed {maximum_retained_emissions}",
    );
}
