use super::*;
use crate::{
    DockPlacement, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    SurfacePresentationResult,
};
use dockspace::{
    geometry::LogicalRect,
    motion::MotionMode,
    runtime::{SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step},
};
use std::{
    num::NonZeroUsize,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

struct Panes;
impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Editor {item}").into())
    }
    fn ui(&mut self, _: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        PaneResponse::from(&ui.label("Editor content"))
    }
}

fn frame(context: &egui::Context, dock: &mut Dockspace, time: f64) -> crate::DockspaceOutput {
    crate::run_ui(
        context,
        egui::RawInput {
            time: Some(time),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            dock.show_single_surface(SurfaceId::new(1), ui, &mut Panes)
                .unwrap();
        },
    )
}

fn fixture() -> (egui::Context, Dockspace) {
    let context = egui::Context::default();
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let nodes = [1, 2, 3].map(|id| DockspaceNode::tabs([ItemId::new(id)]));
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::equal_split(DockspaceAxis::Horizontal, nodes).unwrap(),
        ),
    )])
    .unwrap();
    let mut dock = Dockspace::builder("motion-defaults", layout)
        .build()
        .unwrap();
    for time in [0.0, 0.1, 0.2] {
        present_frame(&context, &mut dock, time);
    }
    (context, dock)
}

fn present_frame(context: &egui::Context, dock: &mut Dockspace, time: f64) {
    let (output, receipt) = frame(context, dock, time).into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
}

fn reflow(dock: &mut Dockspace) {
    dock.dock_item_current(ItemId::new(3), DockPlacement::After(ItemId::new(1)))
        .unwrap();
}

fn sample(dock: &mut Dockspace) -> (LogicalRect, bool, usize) {
    let mut frame = dock.session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SurfaceId::new(1)).unwrap().unwrap();
    let mut pane = None;
    let mut splitters = 0;
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Pane(record),
                ..
            } if record.selected() == Some(ItemId::new(2)) => pane = Some(record.bounds()),
            Step::Operation {
                operation: Operation::Splitter(_),
                ..
            } => splitters += 1,
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    (
        pane.unwrap(),
        plan.partition_available(RootId::new(1)),
        splitters,
    )
}

#[test]
fn default_unknown_preference_animates_and_restores_dividers_at_two_hundred_milliseconds() {
    let (context, mut dock) = fixture();
    assert_eq!(dock.motion_preference(), MotionPreference::Unknown);
    let (before, partition, splitters) = sample(&mut dock);
    assert!(partition);
    assert_eq!(splitters, 2);
    reflow(&mut dock);

    present_frame(&context, &mut dock, 1.0);
    assert!(dock.session.surface_motion_is_active(SurfaceId::new(1)));
    let (initial, partition, splitters) = sample(&mut dock);
    assert_eq!(initial, before);
    assert!(!partition);
    assert_eq!(splitters, 0);

    present_frame(&context, &mut dock, 1.199);
    assert!(dock.session.surface_motion_is_active(SurfaceId::new(1)));
    let (moving, partition, splitters) = sample(&mut dock);
    assert_ne!(moving, before);
    assert!(!partition);
    assert_eq!(splitters, 0);

    present_frame(&context, &mut dock, 1.2);
    assert!(!dock.session.surface_motion_is_active(SurfaceId::new(1)));
    let (completed, partition, splitters) = sample(&mut dock);
    assert_ne!(completed, before);
    assert!(partition);
    assert_eq!(splitters, 1);
}

#[test]
fn runtime_off_system_reduction_and_zero_egui_animation_time_settle_active_motion() {
    for (mode, preference, zero_time) in [
        (MotionMode::Off, MotionPreference::Unknown, false),
        (MotionMode::System, MotionPreference::Reduced, false),
        (MotionMode::System, MotionPreference::Full, true),
    ] {
        let (context, mut dock) = fixture();
        let theme = context.theme();
        let animation_time = context.style_of(theme).animation_time;
        reflow(&mut dock);
        present_frame(&context, &mut dock, 1.0);
        assert!(dock.session.surface_motion_is_active(SurfaceId::new(1)));
        let mut settings = *dock.motion_settings();
        settings.mode = mode;
        dock.set_motion_settings(settings);
        dock.set_motion_preference(preference);
        if zero_time {
            context.style_mut_of(theme, |style| style.animation_time = 0.0);
        }
        present_frame(&context, &mut dock, 1.01);
        assert!(
            !dock.session.surface_motion_is_active(SurfaceId::new(1)),
            "{mode:?}/{preference:?}/zero_time={zero_time} must stop motion immediately"
        );
        let (completed, partition, splitters) = sample(&mut dock);
        assert!(partition);
        assert_eq!(splitters, 1);
        dock.set_motion_settings(MotionSettings::default());
        dock.set_motion_preference(MotionPreference::Unknown);
        context.style_mut_of(theme, |style| style.animation_time = animation_time);
        present_frame(&context, &mut dock, 1.02);
        assert!(
            !dock.session.surface_motion_is_active(SurfaceId::new(1)),
            "reenabling motion must not replay the interrupted layout"
        );
        assert_eq!(sample(&mut dock).0, completed);
    }
}

#[test]
fn terminal_receipt_wakes_caller_owned_motion_after_output_backpressure() {
    let context = egui::Context::default();
    context.options_mut(|options| options.max_passes = NonZeroUsize::new(1).unwrap());
    let nodes = [1, 2, 3].map(|id| DockspaceNode::tabs([ItemId::new(id)]));
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::equal_split(DockspaceAxis::Horizontal, nodes).unwrap(),
        ),
    )])
    .unwrap();
    let mut dock = Dockspace::builder("motion-capacity", layout)
        .build()
        .unwrap();
    dock.session
        .configure_output_capacity(NonZeroUsize::new(1).unwrap())
        .unwrap();
    dock.set_motion_preference(MotionPreference::Full);
    for time in [0.0, 0.1, 0.2] {
        let (output, receipt) = frame(&context, &mut dock, time).into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
    dock.dock_item_current(ItemId::new(3), DockPlacement::After(ItemId::new(1)))
        .unwrap();
    let (output, active) = frame(&context, &mut dock, 1.0).into_parts();
    output.drop_without_applying_deltas();
    assert!(dock.session.surface_motion_is_active(SurfaceId::new(1)));
    for time in [1.01, 1.02, 1.03] {
        let (output, receipt) = frame(&context, &mut dock, time).into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Dropped);
    }
    let wakeups = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&wakeups);
    context.set_request_repaint_callback(move |_| {
        observed.fetch_add(1, Ordering::Relaxed);
    });
    active.report(SurfacePresentationResult::Presented);
    assert!(
        wakeups.load(Ordering::Relaxed) > 0,
        "terminal settlement must resume the paused trajectory"
    );
}

fn idle_context(context: &egui::Context, time: f64) {
    for step in 0..4 {
        let output = context.run_ui(
            egui::RawInput {
                time: Some(time + f64::from(step) * 0.001),
                ..Default::default()
            },
            |_| {},
        );
        output.drop_without_applying_deltas();
        if !context.has_requested_repaint() {
            return;
        }
    }
    panic!("the context must be idle before observing a delayed receipt wakeup");
}

#[test]
fn settled_motion_receipt_wakes_backpressured_owned_input_without_a_repaint_loop() {
    for result in [
        Some(SurfacePresentationResult::Presented),
        Some(SurfacePresentationResult::Dropped),
        None,
    ] {
        let (context, mut dock) = fixture();
        let surface = SurfaceId::new(1);
        dock.session
            .configure_output_capacity(NonZeroUsize::new(1).unwrap())
            .unwrap();
        dock.set_motion_preference(MotionPreference::Full);
        reflow(&mut dock);
        let (output, _) = dock
            .run_single_surface(
                &context,
                egui::RawInput {
                    time: Some(1.0),
                    ..Default::default()
                },
                surface,
                &mut Panes,
            )
            .unwrap();
        assert!(dock.session.surface_motion_is_active(surface));
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);

        let (output, _) = dock
            .run_single_surface(
                &context,
                egui::RawInput {
                    time: Some(10.0),
                    ..Default::default()
                },
                surface,
                &mut Panes,
            )
            .unwrap();
        assert!(!dock.session.surface_motion_is_active(surface));
        let (output, final_receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        idle_context(&context, 10.1);

        let wakeups = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&wakeups);
        context.set_request_repaint_callback(move |info| {
            assert_eq!(info.viewport_id, egui::ViewportId::ROOT);
            observed.fetch_add(1, Ordering::Relaxed);
        });
        let events = vec![egui::Event::Text("deferred input".into())];
        let mut deferred = egui::RawInput {
            time: Some(11.0),
            events: events.clone(),
            ..Default::default()
        };
        let frame_before = context.cumulative_frame_nr();
        let mut composition_calls = 0;
        for _ in 0..2 {
            let error = dock
                .run_single_surface_with_ui(&context, deferred, surface, &mut Panes, |_| {
                    composition_calls += 1;
                })
                .err()
                .expect("the final unsettled output must retain its capacity slot");
            assert_eq!(error.kind(), crate::DockspaceErrorKind::OutputBackpressure);
            deferred = error.into_deferred_input().unwrap();
            assert_eq!(deferred.events, events);
            assert_eq!(deferred.time, Some(11.0));
            assert_eq!(context.cumulative_frame_nr(), frame_before);
            assert_eq!(composition_calls, 0);
            assert_eq!(wakeups.load(Ordering::Relaxed), 0);
        }

        match result {
            Some(result) => final_receipt.report(result),
            None => drop(final_receipt),
        }
        assert_eq!(
            wakeups.load(Ordering::Relaxed),
            1,
            "{result:?} must wake deferred input even after motion has settled"
        );
        let (output, _) = dock
            .run_single_surface_with_ui(&context, deferred, surface, &mut Panes, |ui| {
                composition_calls += 1;
                assert_eq!(ui.input(|input| input.events.clone()), events);
            })
            .unwrap();
        assert_eq!(composition_calls, 1);
        assert!(!dock.session.surface_motion_is_active(surface));
        let (output, static_receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        idle_context(&context, 11.1);
        wakeups.store(0, Ordering::Relaxed);
        static_receipt.report(SurfacePresentationResult::Presented);
        assert_eq!(wakeups.load(Ordering::Relaxed), 0);
        assert!(!context.has_requested_repaint());
    }
}
