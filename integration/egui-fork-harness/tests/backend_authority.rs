use std::{
    cell::RefCell,
    sync::{Arc, Mutex},
};

use egui::{
    BackendEventDerivation, BackendEventSequence, Context, Event, EventCorrelation, Id, Modifiers,
    PaintOutcome, PointerButton, PointerEventKind, PointerReceiverAuthority, Pos2, RawInput, Rect,
    Sense, UserData, ViewportId,
};

fn screen_rect() -> Rect {
    Rect::from_min_max(Pos2::ZERO, Pos2::new(160.0, 100.0))
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn hosted_input(viewport_id: ViewportId, sequence: u128, event: Event) -> RawInput {
    let mut derivation = BackendEventDerivation::known(BackendEventSequence::new(sequence));
    RawInput {
        viewport_id,
        events: vec![derivation.envelope(event)],
        ..Default::default()
    }
}

fn register_receiver(ui: &mut egui::Ui) {
    let _ = ui.interact(
        Rect::from_min_max(Pos2::new(10.0, 10.0), Pos2::new(80.0, 80.0)),
        Id::new("fork-harness-receiver"),
        Sense::click_and_drag(),
    );
}

struct HarnessHostedCycleDriver<'a> {
    phases: &'a RefCell<Vec<&'static str>>,
    child: ViewportId,
}

impl eframe::HostedViewportCycleDriver for HarnessHostedCycleDriver<'_> {
    type Output = ViewportId;

    fn begin(
        &mut self,
        cycle: &eframe::HostedViewportCycle,
    ) -> eframe::HostedViewportAppResult<()> {
        assert_eq!(cycle.len(), 2);
        self.phases.borrow_mut().push("begin");
        Ok(())
    }

    fn run_viewport(
        &mut self,
        input: eframe::HostedViewportInput,
    ) -> eframe::HostedViewportAppResult<Self::Output> {
        let prior_phases = self.phases.borrow();
        assert_eq!(prior_phases.first(), Some(&"begin"));
        assert!(!prior_phases.contains(&"end"));
        drop(prior_phases);
        self.phases
            .borrow_mut()
            .push(if input.viewport_id() == self.child {
                "child"
            } else {
                "root"
            });
        Ok(input.viewport_id())
    }

    fn end(
        &mut self,
        outputs: &mut [eframe::HostedViewportOutput<Self::Output>],
    ) -> eframe::HostedViewportAppResult<()> {
        assert_eq!(outputs.len(), 2);
        self.phases.borrow_mut().push("end");
        Ok(())
    }
}

#[test]
fn hook_reordering_downgrades_the_causal_suffix() {
    let mut derivation = BackendEventDerivation::known(BackendEventSequence::new(41));
    let mut input = RawInput {
        events: vec![
            derivation.envelope(Event::Copy),
            derivation.envelope(Event::Cut),
        ],
        ..Default::default()
    };

    let before = input.event_provenance_snapshot();
    input.events.swap(0, 1);
    input.sanitize_hook_events(&before);

    assert!(
        input
            .events
            .iter()
            .all(|event| event.correlation() == EventCorrelation::Unknown)
    );
}

#[test]
fn receiver_journal_preserves_event_identity_position_and_order() {
    let context = Context::default();
    let _ = context.run_ui(
        RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
        register_receiver,
    );

    let position = Pos2::new(40.0, 40.0);
    let sequence = BackendEventSequence::new(73);
    let mut derivation = BackendEventDerivation::known(sequence);
    let output = context.run_ui(
        RawInput {
            screen_rect: Some(screen_rect()),
            events: vec![derivation.envelope(pointer_button(position, true))],
            ..Default::default()
        },
        register_receiver,
    );

    let [record] = output.pointer_receiver_journal.records.as_slice() else {
        panic!("one pointer edge must produce one receiver record");
    };
    assert_eq!(record.event.raw_event_index, 0);
    assert_eq!(record.event.correlation.sequence(), Some(sequence));
    assert_eq!(
        record.kind,
        PointerEventKind::Pressed(PointerButton::Primary)
    );
    assert_eq!(record.position, position);
}

#[test]
fn eframe_reports_the_exact_terminal_renderer_outcome() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let hook_observed = Arc::clone(&observed);
    let mut options = eframe::NativeOptions::default();
    options.presentation_result_hook = Some(Arc::new(move |result| {
        hook_observed
            .lock()
            .expect("the test hook mutex is not poisoned")
            .push(result);
    }));

    let result = egui::PresentationResult::new(
        egui::ViewportId::ROOT,
        UserData::new(97_u64),
        PaintOutcome::SubmittedToSwapchain,
        None,
    );
    options
        .presentation_result_hook
        .as_ref()
        .expect("the fork exposes the presentation result hook")(result.clone());

    let observed = observed
        .lock()
        .expect("the test hook mutex is not poisoned");
    assert_eq!(observed.as_slice(), &[result]);
}

#[test]
fn successful_presentation_exposes_the_immutable_hit_graph() {
    let context = Context::default();
    let output = context.run_ui(
        RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
        register_receiver,
    );

    let result = egui::PresentationResult::new(
        ViewportId::ROOT,
        UserData::new(101_u64),
        PaintOutcome::SubmittedToSwapchain,
        output.pointer_hit_graph_candidate,
    );
    let graph = result
        .presented_pointer_hit_graph()
        .expect("successful presentation must expose its exact hit graph");

    assert_eq!(graph.viewport_id(), ViewportId::ROOT);
    assert!(matches!(
        graph.probe(Pos2::new(40.0, 40.0)),
        PointerReceiverAuthority::Known(_)
    ));
}

#[test]
fn viewport_mismatch_terminally_rejects_the_candidate() {
    let context = Context::default();
    let output = context.run_ui(
        RawInput {
            screen_rect: Some(screen_rect()),
            ..Default::default()
        },
        register_receiver,
    );
    let candidate = output
        .pointer_hit_graph_candidate
        .expect("completed pass must emit a hit graph candidate");
    let retained_candidate = candidate.clone();

    let result = egui::PresentationResult::new(
        ViewportId::from_hash_of("different viewport"),
        UserData::new(102_u64),
        PaintOutcome::Swapped,
        Some(candidate),
    );

    assert!(result.presented_pointer_hit_graph().is_none());
    assert!(!retained_candidate.settle(&PaintOutcome::Swapped));
}

#[test]
fn dockspace_adapter_and_fork_resolve_one_egui_type() {
    fn accepts_adapter_context(_: &egui::Context) {}

    let context = Context::default();
    accepts_adapter_context(&context);
    let _ = std::mem::size_of::<egui_dockspace::Dockspace>();
}

#[test]
fn hosted_cycle_exposes_the_complete_roster_before_any_viewport_callback() {
    let child = ViewportId::from_hash_of("fork harness hosted child");
    let cycle = eframe::HostedViewportCycle::new([
        hosted_input(ViewportId::ROOT, 11, Event::Copy),
        hosted_input(child, 12, Event::Cut),
    ])
    .expect("the complete physical roster is valid");
    let phases = RefCell::new(Vec::new());
    let mut driver = HarnessHostedCycleDriver {
        phases: &phases,
        child,
    };

    let outputs = cycle
        .drive([child, ViewportId::ROOT], &mut driver)
        .expect("a complete callback permutation must run");

    assert_eq!(phases.into_inner(), ["begin", "child", "root", "end"]);
    assert_eq!(outputs[0].viewport_id(), child);
    assert_eq!(outputs[1].viewport_id(), ViewportId::ROOT);
}

#[test]
fn hosted_cycle_preserves_cross_viewport_release_then_press_order() {
    let child = ViewportId::from_hash_of("fork harness hosted target");
    let release_position = Pos2::new(17.0, 19.0);
    let press_position = Pos2::new(71.0, 73.0);
    let cycle = eframe::HostedViewportCycle::new([
        hosted_input(
            ViewportId::ROOT,
            41,
            pointer_button(release_position, false),
        ),
        hosted_input(child, 42, pointer_button(press_position, true)),
    ])
    .expect("the complete physical roster is valid");

    cycle
        .run(
            [child, ViewportId::ROOT],
            |inputs| {
                let mut edges = inputs
                    .inputs()
                    .flat_map(|(viewport_id, input)| {
                        input.events.iter().map(move |event| {
                            (
                                event
                                    .correlation()
                                    .sequence()
                                    .expect("backend sequence is known"),
                                viewport_id,
                                event.event().clone(),
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                edges.sort_by_key(|(sequence, _, _)| *sequence);

                assert!(matches!(
                    &edges[0],
                    (sequence, viewport_id, Event::PointerButton { pos, pressed: false, .. })
                        if *sequence == BackendEventSequence::new(41)
                            && *viewport_id == ViewportId::ROOT
                            && *pos == release_position
                ));
                assert!(matches!(
                    &edges[1],
                    (sequence, viewport_id, Event::PointerButton { pos, pressed: true, .. })
                        if *sequence == BackendEventSequence::new(42)
                            && *viewport_id == child
                            && *pos == press_position
                ));
                Ok(())
            },
            |_| Ok(()),
            |_| Ok(()),
        )
        .expect("viewport callback order must not rewrite physical edge order");
}
