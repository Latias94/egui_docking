//! Real egui runs must publish only their intact terminal docking output.

use std::num::NonZeroUsize;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use dockspace::geometry::LogicalPoint;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::{
    DockspaceReceiverRole, HostPointerInput, HostPointerRoute, PointerDeviceKind,
    PointerEventMetadata, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceUnavailableReason, SurfaceWork,
    SurfaceWorkDeferred,
};
use egui::accesskit::Role;
use egui::{
    Color32, Context, Event, FullOutput, PointerButton, Pos2, RawInput, Rect, RepaintCause, Shape,
    Ui,
};

use super::super::Dockspace;
use crate::DockspaceErrorKind;
use crate::pane::PaneView;
use crate::pass_settlement::{ProductPassSettlement, ReadyProductPassSettlementBatch};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const PANE_COLOR: Color32 = Color32::from_rgb(11, 97, 173);
const OUTPUT_HOOK_PANIC: &str = "owned run output hook panicked after pane painting";

#[derive(Default)]
struct RunPanes {
    calls: Vec<(ItemId, u64)>,
}

impl PaneView for RunPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        self.calls.push((item, ui.ctx().cumulative_pass_nr()));
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap().shrink(8.0), 0.0, PANE_COLOR);
    }
}

fn input(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(640.0, 480.0))),
        time: Some(time),
        events,
        ..RawInput::default()
    }
}

fn press(position: Pos2) -> Vec<Event> {
    vec![
        Event::PointerMoved(position),
        Event::PointerButton {
            pos: position,
            button: PointerButton::Primary,
            pressed: true,
            modifiers: egui::Modifiers::NONE,
        },
    ]
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the product renderer supplies finite bounds inside the 640x480 viewport"
)]
fn inactive_tab_center(output: &FullOutput) -> Pos2 {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("the product renderer exports its actual tab geometry")
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == Role::Tab && node.label() == Some("Second"))
                .then(|| node.bounds().expect("the inactive tab has real bounds"))
        })
        .expect("the real inactive tab was painted");
    Rect::from_min_max(
        Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
        Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
    )
    .center()
}

fn presented_tabs(id: &'static str) -> (Context, Dockspace, RunPanes, Pos2) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap();
    let mut dockspace = Dockspace::builder(id, layout).build().unwrap();
    dockspace
        .session
        .configure_output_capacity(NonZeroUsize::new(1).unwrap())
        .unwrap();
    let mut panes = RunPanes::default();

    // Publish P0 before reading geometry. Consecutive real outputs must then
    // agree, so the next raw press uses the renderer's stable registrations.
    for time in [0.0, 0.1] {
        let (output, _) = dockspace
            .run_single_surface(&context, input(time, Vec::new()), SURFACE, &mut panes)
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    assert!(dockspace.session.presented_surface(SURFACE).is_some());
    let mut position = None;
    for time in [0.2, 0.3] {
        let (output, _) = dockspace
            .run_single_surface(&context, input(time, Vec::new()), SURFACE, &mut panes)
            .unwrap();
        let (output, receipt) = output.into_parts();
        let current = inactive_tab_center(&output);
        if let Some(previous) = position {
            assert_eq!(current, previous, "P0 tab geometry must be stable");
        }
        position = Some(current);
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    let position = position.unwrap();
    let hit = dockspace
        .session
        .host_pointer_hit(
            SURFACE,
            LogicalPoint::new(f64::from(position.x), f64::from(position.y)).unwrap(),
        )
        .and_then(|hit| hit.delivery())
        .expect("the stable rendered position hits a real presented receiver");
    assert_eq!(hit.descriptor().role(), DockspaceReceiverRole::TabBody);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(!dockspace.measurement_deferred);
    panes.calls.clear();
    (context, dockspace, panes, position)
}

struct LateDiscard {
    remaining: usize,
}

impl egui::plugin::Plugin for LateDiscard {
    fn debug_name(&self) -> &'static str {
        "product_owned_run_tests::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.remaining > 0 {
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(RepaintCause::new_reason("discard the owned encoding pass"));
        }
    }
}

struct PanicAfterPaint {
    armed: bool,
}

impl egui::plugin::Plugin for PanicAfterPaint {
    fn debug_name(&self) -> &'static str {
        "product_owned_run_tests::panic_after_paint"
    }

    fn output_hook(&mut self, _context: &Context, _output: &mut FullOutput) {
        if std::mem::take(&mut self.armed) {
            panic!("{OUTPUT_HOOK_PANIC}");
        }
    }
}

struct RemovePaneGeometry {
    removed: Arc<AtomicUsize>,
}

impl egui::plugin::Plugin for RemovePaneGeometry {
    fn debug_name(&self) -> &'static str {
        "product_owned_run_tests::remove_pane_geometry"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        output.shapes.retain(|clipped| {
            let remove = matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == PANE_COLOR);
            if remove {
                self.removed.fetch_add(1, Ordering::Relaxed);
            }
            !remove
        });
    }
}

fn take_ready(
    settlement: &ProductPassSettlement,
    context: &Context,
) -> ReadyProductPassSettlementBatch {
    let mut ready = None;
    context
        .run_ui(input(4.0, Vec::new()), |ui| {
            ready = Some(settlement.take_ready(ui));
        })
        .drop_without_applying_deltas();
    ready.expect("the renderer report queue is read through a real egui UI")
}

#[test]
fn late_discards_paint_once_per_actual_pass_and_publish_one_selection_and_output() {
    let (context, mut dockspace, mut panes, position) = presented_tabs("owned-run-late-discards");
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(3).unwrap());
    context.add_plugin(LateDiscard { remaining: 2 });
    let before = dockspace.version();
    let first_pass = context.cumulative_pass_nr();

    let (output, response) = dockspace
        .run_single_surface(&context, input(3.0, press(position)), SURFACE, &mut panes)
        .expect("discarded encoding must not consume the only output slot");
    let (output, receipt) = output.into_parts();
    assert_eq!(output.platform_output.num_completed_passes, 3);
    assert_eq!(context.cumulative_pass_nr(), first_pass + 3);
    assert_eq!(
        panes.calls,
        [
            (SECOND, first_pass),
            (SECOND, first_pass + 1),
            (SECOND, first_pass + 2),
        ],
        "pane UI must run exactly once in each actual pass, including both discards"
    );
    output.drop_without_applying_deltas();
    assert_eq!(response.mutation().before(), before);
    assert_eq!(response.mutation().after(), dockspace.version());
    assert!(response.mutation().workspace_changed());
    assert_ne!(dockspace.version(), before);
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(dockspace.session.has_active_gesture());
    {
        let mut probe = dockspace.session.begin_host_frame().unwrap();
        assert!(matches!(
            probe.surface_work(SURFACE).unwrap(),
            SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity)
        ));
    }

    receipt.report(SurfacePresentationResult::Presented);
    let mut ready = take_ready(&dockspace.pass_settlement, &context);
    assert!(!ready.lane_changed);
    assert_eq!(ready.presentation_failure, None);
    assert_eq!(
        ready.presentations.len(),
        1,
        "only the terminal output exists"
    );
    let presentation = ready.presentations.pop().unwrap();
    assert_eq!(presentation.result, SurfacePresentationResult::Presented);
    let artifact = presentation.output.presentation_artifact();
    assert!(
        !dockspace
            .session
            .presented_surface(SURFACE)
            .unwrap()
            .matches_presentation_artifact(artifact),
        "a renderer report alone cannot publish presentation authority"
    );
    dockspace
        .session
        .report_surface_presentation(presentation.output, presentation.result)
        .unwrap();
    let after = dockspace.version();
    let mut frame = dockspace.session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    assert_eq!(report.before(), after);
    assert_eq!(report.after(), after);
    assert!(!report.workspace_changed());
    assert!(report.take_painted_outputs().is_empty());
    assert_eq!(report.surface_presentation_commits().len(), 1);
    assert!(report.surface_presentation_commits()[0].presents(artifact));
    assert!(
        dockspace
            .session
            .presented_surface(SURFACE)
            .unwrap()
            .matches_presentation_artifact(artifact)
    );
    assert!(
        take_ready(&dockspace.pass_settlement, &context)
            .presentations
            .is_empty()
    );
}

#[test]
fn queued_accessibility_event_survives_a_later_update_hook_panic_until_publication() {
    let (context, mut dockspace, mut panes, _) = presented_tabs("owned-run-queued-event-retry");
    let (output, _) = dockspace
        .run_single_surface(&context, input(1.0, Vec::new()), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    let target = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(id, node)| {
            (node.role() == Role::Tab && node.label() == Some("Second")).then_some(*id)
        })
        .unwrap();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    let event = Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
        action: egui::accesskit::Action::Click,
        target_tree: egui::accesskit::TreeId::ROOT,
        target_node: target,
        data: None,
    });
    let (output, _) = dockspace
        .run_single_surface(&context, input(2.0, vec![event]), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(
        !dockspace.ui_actions.is_empty(),
        "the real UI event entered deferred ingress"
    );
    let before = dockspace.version();
    panes.calls.clear();
    context.add_plugin(PanicAfterPaint { armed: true });
    assert!(
        catch_unwind(AssertUnwindSafe(|| {
            let _ =
                dockspace.run_single_surface(&context, input(3.0, Vec::new()), SURFACE, &mut panes);
        }))
        .is_err()
    );
    assert!(panes.calls.iter().any(|(item, _)| *item == SECOND));
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(
        !dockspace.ui_actions.is_empty(),
        "an unpublished attempt cannot acknowledge the queued event"
    );
    let (output, response) = dockspace
        .run_single_surface(&context, input(4.0, Vec::new()), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(response.mutation().workspace_changed());
    assert!(dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(dockspace.ui_actions.is_empty());
    let selected = dockspace.version();
    let (output, response) = dockspace
        .run_single_surface(&context, input(5.0, Vec::new()), SURFACE, &mut panes)
        .unwrap();
    output.into_parts().0.drop_without_applying_deltas();
    assert_eq!(dockspace.version(), selected);
    assert!(!response.mutation().workspace_changed());
}

#[test]
fn real_ui_action_ingress_accepts_64_events_and_rejects_65_without_partial_publication() {
    struct ManyPanes;
    impl PaneView for ManyPanes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Pane {}", item.get()).into())
        }
        fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
            ui.label("Bounded action ingress");
        }
    }
    for count in [64, 65] {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs((1..=count).map(ItemId::new)),
            ),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder(("bounded-owned-actions", count), layout)
            .build()
            .unwrap();
        let mut panes = ManyPanes;
        let wide_input = |time, events| {
            let mut raw = input(time, events);
            raw.screen_rect = Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(16_384.0, 480.0)));
            raw
        };
        let mut events = Vec::new();
        for time in [0.0, 0.1, 0.2] {
            let (output, _) = dockspace
                .run_single_surface(&context, wide_input(time, Vec::new()), SURFACE, &mut panes)
                .unwrap();
            let (output, receipt) = output.into_parts();
            events = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .filter(|(_, node)| node.role() == Role::Tab)
                .map(|(id, _)| {
                    Event::AccessKitActionRequest(egui::accesskit::ActionRequest {
                        action: egui::accesskit::Action::Click,
                        target_tree: egui::accesskit::TreeId::ROOT,
                        target_node: *id,
                        data: None,
                    })
                })
                .collect();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }
        assert_eq!(events.len(), usize::try_from(count).unwrap());
        let before = dockspace.version();
        let result =
            dockspace.run_single_surface(&context, wide_input(1.0, events), SURFACE, &mut panes);
        assert_eq!(dockspace.version(), before);
        if count == 65 {
            assert!(
                result.is_err(),
                "overflow is rejected before any new action batch publishes"
            );
            assert!(dockspace.ui_actions.is_empty());
        } else {
            let (output, _) = result.unwrap();
            let (output, receipt) = output.into_parts();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
            assert_eq!(dockspace.ui_actions.before(u64::MAX).count(), 64);
            let (output, _) = dockspace
                .run_single_surface(&context, wide_input(2.0, Vec::new()), SURFACE, &mut panes)
                .unwrap();
            output.into_parts().0.drop_without_applying_deltas();
            assert!(
                dockspace
                    .view()
                    .item(ItemId::new(64))
                    .unwrap()
                    .is_selected()
            );
            assert!(dockspace.ui_actions.is_empty());
        }
    }
}

#[test]
fn output_hook_panic_rolls_back_a_real_selection_and_never_replays_its_raw_press() {
    let (context, mut dockspace, mut panes, position) =
        presented_tabs("owned-run-output-hook-panic");
    let before = dockspace.version();
    let presented = dockspace.session.presented_surface(SURFACE);
    let continuation = dockspace.session.pointer_continuation();
    let first_pass = context.cumulative_pass_nr();
    context.add_plugin(PanicAfterPaint { armed: true });

    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let _ = dockspace.run_single_surface(
            &context,
            input(3.0, press(position)),
            SURFACE,
            &mut panes,
        );
    }));
    let panic = interrupted.expect_err("the terminal output hook must unwind the owned run");
    let message = panic
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| panic.downcast_ref::<String>().map(String::as_str));
    assert_eq!(message, Some(OUTPUT_HOOK_PANIC));
    assert_eq!(
        panes.calls,
        [(SECOND, first_pass)],
        "the real inactive-tab press must select and paint before the hook panics"
    );
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert_eq!(dockspace.session.presented_surface(SURFACE), presented);
    assert_eq!(dockspace.session.pointer_continuation(), continuation);

    // Probe core's producer clock directly: egui's clock itself cannot rewind.
    // This earlier event must remain admissible after discarding the 3s batch.
    {
        let mut probe = dockspace.session.begin_host_frame().unwrap();
        probe
            .submit_host_pointer(
                HostPointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    HostPointerRoute::Unknown,
                    HostPointerRoute::Unknown,
                    SurfacePointerCapture::Unknown,
                    SurfacePointerReceiverFacts::unknown(),
                )
                .with_metadata(PointerEventMetadata::new(
                    PointerDeviceKind::Unknown,
                    Duration::from_millis(2500),
                )),
            )
            .expect("the failed run must not advance the producer watermark");
        assert!(matches!(
            probe.surface_work(SURFACE).unwrap(),
            SurfaceWork::Ready(_)
        ));
    }

    panes.calls.clear();
    let next_pass = context.cumulative_pass_nr();
    let (output, response) = dockspace
        .run_single_surface(&context, input(4.0, Vec::new()), SURFACE, &mut panes)
        .expect("unwinding must release the candidate and its encoding slot");
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    assert_eq!(panes.calls, [(FIRST, next_pass)]);
    assert_eq!(response.mutation().before(), before);
    assert_eq!(response.mutation().after(), before);
    assert!(!response.mutation().workspace_changed());
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.ui_actions.is_empty());
    receipt.report(SurfacePresentationResult::Presented);
}

#[test]
fn removing_real_pane_geometry_rejects_the_run_before_core_publication() {
    let (context, mut dockspace, mut panes, position) =
        presented_tabs("owned-run-damaged-pane-output");
    let before = dockspace.version();
    let presented = dockspace.session.presented_surface(SURFACE);
    let first_pass = context.cumulative_pass_nr();
    let removed = Arc::new(AtomicUsize::new(0));
    context.add_plugin(RemovePaneGeometry {
        removed: Arc::clone(&removed),
    });

    let error = dockspace
        .run_single_surface(&context, input(3.0, press(position)), SURFACE, &mut panes)
        .err()
        .expect("an output hook cannot remove pane pixels and still publish selection");
    assert_eq!(error.kind(), DockspaceErrorKind::OperationConflict);
    assert_eq!(removed.load(Ordering::Relaxed), 1);
    assert_eq!(
        panes.calls,
        [(SECOND, first_pass)],
        "the failed output must contain the real newly selected pane"
    );
    assert_eq!(dockspace.version(), before);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert!(!dockspace.view().item(SECOND).unwrap().is_selected());
    assert!(!dockspace.session.has_active_gesture());
    assert_eq!(dockspace.session.presented_surface(SURFACE), presented);
    assert!(dockspace.ui_actions.is_empty());
    let mut probe = dockspace.session.begin_host_frame().unwrap();
    assert!(matches!(
        probe.surface_work(SURFACE).unwrap(),
        SurfaceWork::Ready(_)
    ));
}
