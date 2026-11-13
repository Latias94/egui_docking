//! A deferred context rebind must retire the preceding host's input contacts.

use std::num::NonZeroUsize;
use std::sync::{Arc, Mutex};

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use egui::accesskit::Role;
use egui::{
    Context, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, TouchDeviceId,
    TouchId, TouchPhase, Ui, ViewportId,
};

use crate::{
    Dockspace, DockspaceErrorKind, DockspaceOutput, PaneResponse, PaneView,
    SurfacePresentationResult,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(if item == FIRST { "First" } else { "Second" }.into())
    }

    fn ui(&mut self, _: ItemId, ui: &mut Ui) -> PaneResponse {
        PaneResponse::from(&ui.label("Application-owned pane"))
    }
}

fn context() -> Context {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    context
}

fn input(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
        focused: true,
        time: Some(time),
        events,
        ..RawInput::default()
    }
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the actual tab bounds lie inside the finite 640 by 480 egui viewport"
)]
fn second_tab_center(output: &FullOutput) -> Pos2 {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Tab && node.label() == Some("Second"))
                .then(|| node.bounds().unwrap())
        })
        .expect("the actual egui output contains the second tab");
    Pos2::new(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    )
}

fn present(output: DockspaceOutput) -> Pos2 {
    let (output, receipt) = output.into_parts();
    let position = second_tab_center(&output);
    output.drop_without_applying_deltas();
    // This test owns a logical sink for the complete, unmodified egui output.
    receipt.report(SurfacePresentationResult::Presented);
    position
}

fn button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

#[test]
fn a_backpressured_context_rebind_cannot_keep_old_touch_contacts_and_suppress_mouse() {
    let original_context = context();
    let rebound_context = context();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder("backpressured-context-rebind", layout)
        .build()
        .unwrap();
    let mut panes = Panes;
    let mut point = Pos2::ZERO;
    for pass in 0..4 {
        let (output, _) = dockspace
            .run_single_surface(
                &original_context,
                input(f64::from(pass) * 0.1, Vec::new()),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        point = present(output);
    }
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());

    // Keep the contact live while filling the public facade's default 64 slots.
    // These frames finish before the contact's long-press deadline.
    let mut retained = Vec::with_capacity(64);
    for pass in 0..64 {
        let events = if pass == 0 {
            vec![
                Event::Touch {
                    device_id: TouchDeviceId(1),
                    id: TouchId(7),
                    phase: TouchPhase::Start,
                    pos: point,
                    force: None,
                },
                Event::PointerMoved(point),
                button(point, true),
            ]
        } else {
            Vec::new()
        };
        let (output, _) = dockspace
            .run_single_surface(
                &original_context,
                input(1.0 + f64::from(pass) * 0.001, events),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        retained.push(receipt);
    }
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    let before_rebind = dockspace.version();
    let original_input = input(2.0, vec![Event::PointerMoved(point)]);
    let error = dockspace
        .run_single_surface(
            &rebound_context,
            original_input.clone(),
            SURFACE,
            &mut panes,
        )
        .err()
        .expect("the context rebind must defer input while all 64 outputs are retained");
    assert_eq!(error.kind(), DockspaceErrorKind::OutputBackpressure);
    assert_eq!(rebound_context.cumulative_pass_nr(), 0);
    assert_eq!(dockspace.version(), before_rebind);
    let deferred = error.into_deferred_input().unwrap();
    assert_eq!(deferred.events, original_input.events);

    // Their old lane can no longer deliver presentation into the new context.
    // Reporting releases the capabilities, whose unmatched outputs retire on Drop.
    for receipt in retained {
        receipt.report(SurfacePresentationResult::Presented);
    }
    let (output, _) = dockspace
        .run_single_surface(&rebound_context, deferred, SURFACE, &mut panes)
        .expect("retiring the old lane's outputs admits the original input");
    present(output);
    for time in [2.1, 2.2] {
        let (output, _) = dockspace
            .run_single_surface(
                &rebound_context,
                input(time, Vec::new()),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        point = present(output);
    }
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());

    let (output, _) = dockspace
        .run_single_surface(
            &rebound_context,
            input(
                2.3,
                vec![
                    Event::PointerMoved(point),
                    button(point, true),
                    button(point, false),
                ],
            ),
            SURFACE,
            &mut panes,
        )
        .unwrap();
    present(output);
    assert!(
        dockspace.view().item(SECOND).unwrap().is_selected(),
        "the new context's mouse must select its presented tab without an old Touch End"
    );
}

fn idle_viewport(context: &Context, viewport: ViewportId, time: f64) {
    for pass in 0..4 {
        let mut raw = input(time + f64::from(pass) * 0.001, Vec::new());
        raw.viewport_id = viewport;
        raw.viewports.entry(viewport).or_default();
        context.run_ui(raw, |_| {}).drop_without_applying_deltas();
        if !context.has_requested_repaint_for(&viewport) {
            return;
        }
    }
    panic!("the viewport must be idle before observing a delayed receipt wakeup");
}

fn assert_old_lane_wakes_rebound_input(same_context: bool) {
    for result in [
        Some(SurfacePresentationResult::Presented),
        Some(SurfacePresentationResult::Dropped),
        None,
    ] {
        let original_context = context();
        let rebound_context = if same_context {
            original_context.clone()
        } else {
            context()
        };
        let rebound_viewport = if same_context {
            ViewportId::from_hash_of("backpressured-rebound-viewport")
        } else {
            ViewportId::ROOT
        };
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder(("rebound-output-wakeup", same_context), layout)
            .build()
            .unwrap();
        dockspace
            .session
            .configure_output_capacity(NonZeroUsize::new(1).unwrap())
            .unwrap();
        for pass in 0..3 {
            let (output, _) = dockspace
                .run_single_surface(
                    &original_context,
                    input(f64::from(pass) * 0.1, Vec::new()),
                    SURFACE,
                    &mut Panes,
                )
                .unwrap();
            present(output);
        }
        let (output, _) = dockspace
            .run_single_surface(
                &original_context,
                input(0.4, Vec::new()),
                SURFACE,
                &mut Panes,
            )
            .unwrap();
        let (output, old_receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        idle_viewport(&original_context, ViewportId::ROOT, 0.5);
        idle_viewport(&rebound_context, rebound_viewport, 0.6);

        let old_wakeups = Arc::new(Mutex::new(Vec::new()));
        if !same_context {
            let observed = Arc::clone(&old_wakeups);
            original_context.set_request_repaint_callback(move |info| {
                observed.lock().unwrap().push(info.viewport_id);
            });
        }
        let wakeups = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&wakeups);
        rebound_context.set_request_repaint_callback(move |info| {
            observed.lock().unwrap().push(info.viewport_id);
        });
        let mut original_input = input(
            1.0,
            vec![
                Event::Text("deferred text".into()),
                Event::Ime(egui::ImeEvent::Commit("deferred IME".into())),
            ],
        );
        original_input.viewport_id = rebound_viewport;
        original_input
            .viewports
            .entry(rebound_viewport)
            .or_default()
            .title = Some("Waiting output owner".into());
        let pass_before = rebound_context.cumulative_pass_nr_for(rebound_viewport);
        let mut deferred = original_input.clone();
        let mut composition_calls = 0;
        for _ in 0..2 {
            let error = dockspace
                .run_single_surface_with_ui(&rebound_context, deferred, SURFACE, &mut Panes, |_| {
                    composition_calls += 1
                })
                .err()
                .expect("the old lane's output must retain its capacity slot");
            assert_eq!(error.kind(), DockspaceErrorKind::OutputBackpressure);
            deferred = error.into_deferred_input().unwrap();
            assert_eq!(deferred.events, original_input.events);
            assert_eq!(deferred.time, original_input.time);
            assert_eq!(deferred.viewport_id, original_input.viewport_id);
            assert_eq!(deferred.viewports, original_input.viewports);
            assert_eq!(
                rebound_context.cumulative_pass_nr_for(rebound_viewport),
                pass_before
            );
            assert_eq!(composition_calls, 0);
            assert!(wakeups.lock().unwrap().is_empty());
        }

        match result {
            Some(result) => old_receipt.report(result),
            None => drop(old_receipt),
        }
        assert_eq!(
            wakeups.lock().unwrap().as_slice(),
            &[rebound_viewport],
            "{result:?} must wake the waiting lane after rebind"
        );
        assert!(old_wakeups.lock().unwrap().is_empty());
        assert_eq!(
            rebound_context.cumulative_pass_nr_for(rebound_viewport),
            pass_before
        );
        assert_eq!(composition_calls, 0);
        assert!(
            dockspace
                .pass_settlement
                .take_ready_before_run(&rebound_context, rebound_viewport)
                .unwrap()
                .presentations
                .is_empty(),
            "an old lane's presentation must not enter the rebound lane"
        );
        let (output, _) = dockspace
            .run_single_surface_with_ui(&rebound_context, deferred, SURFACE, &mut Panes, |ui| {
                composition_calls += 1;
                assert_eq!(
                    ui.input(|input| input.events.clone()),
                    original_input.events
                );
            })
            .unwrap();
        assert_eq!(composition_calls, 1);
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

#[test]
fn old_context_receipt_wakes_the_backpressured_rebound_context() {
    assert_old_lane_wakes_rebound_input(false);
}

#[test]
fn old_viewport_receipt_wakes_the_backpressured_rebound_viewport() {
    assert_old_lane_wakes_rebound_input(true);
}
