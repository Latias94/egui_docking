//! Device-specific resize targets through real egui contacts and presentation receipts.

use super::*;
use dockspace::geometry::LogicalRect;
use dockspace::model::{DockspaceContainedLayout, FloatingPresentationId};
use dockspace::runtime::{
    ContainedResizeDirection, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    DockspaceTouchEvent, PointerDeviceKind, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step,
};

const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);

#[derive(Clone, Copy, Debug)]
enum Target {
    Splitter,
    Contained,
}

impl Target {
    fn role(self) -> DockspaceReceiverRole {
        match self {
            Self::Splitter => DockspaceReceiverRole::Splitter,
            Self::Contained => DockspaceReceiverRole::ContainedResize,
        }
    }
}

#[derive(Clone, Copy, Debug)]
enum Barrier {
    None,
    ExternalWidget(Rect),
    DisabledUi,
}

#[derive(Default)]
struct FrameEvents {
    touch: Vec<DockspaceTouchEvent>,
    feedback: Vec<DockspaceManipulationStatus>,
    closes: usize,
}

fn frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    time: f64,
    events: Vec<Event>,
    barrier: Barrier,
) -> FrameEvents {
    let mut raw = input(events);
    raw.time = Some(time);
    let mut result = FrameEvents::default();
    let output = crate::run_ui(context, raw, |ui| {
        let mut show = |ui: &mut Ui| {
            let response = dockspace.show_single_surface(SURFACE, ui, panes).unwrap();
            result.touch.extend_from_slice(response.touch_events());
            result.feedback.extend(
                response
                    .manipulation_feedback()
                    .iter()
                    .map(|feedback| feedback.status()),
            );
            result.closes += response.close_requests().count();
        };
        if matches!(barrier, Barrier::DisabledUi) {
            ui.add_enabled_ui(false, show);
        } else {
            show(ui);
        }
        if let Barrier::ExternalWidget(rect) = barrier {
            let response = ui.interact(
                rect,
                ui.make_persistent_id("touch-target-external-widget"),
                egui::Sense::click_and_drag(),
            );
            assert!(response.enabled() && response.sense.interactive());
        }
    });
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(crate::SurfacePresentationResult::Presented);
    result
}

fn contact(phase: egui::TouchPhase, pos: Pos2) -> Event {
    Event::Touch {
        device_id: egui::TouchDeviceId(1),
        id: egui::TouchId(7),
        phase,
        pos,
        force: None,
    }
}

fn setup(target: Target) -> (Context, Dockspace, MagneticPanes, Rect) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let surface = match target {
        Target::Splitter => DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                ROOT,
                DockspaceNode::equal_split(
                    DockspaceAxis::Horizontal,
                    [DockspaceNode::tabs([FIRST]), DockspaceNode::tabs([SECOND])],
                )
                .unwrap(),
            ),
        ),
        Target::Contained => DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
            LogicalRect::new(120.0, 110.0, 320.0, 260.0).unwrap(),
        )),
    };
    let mut dockspace = Dockspace::builder(
        format!("touch-resize-target-{target:?}"),
        DockspaceLayout::new([surface]).unwrap(),
    )
    .build()
    .unwrap();
    let mut panes = MagneticPanes::new();
    for index in 0..8 {
        frame(
            &context,
            &mut dockspace,
            &mut panes,
            f64::from(index) * 0.05,
            Vec::new(),
            Barrier::None,
        );
    }
    let hit = base_resize_hit(&mut dockspace, target);
    (context, dockspace, panes, hit)
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the actual paint geometry is bounded by the finite egui test surface"
)]
fn egui_rect(rect: LogicalRect) -> Rect {
    Rect::from_min_max(
        Pos2::new(rect.x() as f32, rect.y() as f32),
        Pos2::new(rect.max().x() as f32, rect.max().y() as f32),
    )
}

fn base_resize_hit(dockspace: &mut Dockspace, target: Target) -> Rect {
    let mut frame = dockspace.session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut hit = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation { operation, .. } = step {
            match (target, operation) {
                (Target::Splitter, Operation::Splitter(splitter)) => {
                    hit = Some(splitter.hit_bounds());
                }
                (Target::Contained, Operation::ContainedControls(contained)) => {
                    assert_eq!(
                        contained.outer_bounds(),
                        LogicalRect::new(120.0, 110.0, 320.0, 260.0).unwrap(),
                        "touch expansion must not enlarge the painted floating window"
                    );
                    hit = contained
                        .resize()
                        .find(|resize| resize.direction() == ContainedResizeDirection::East)
                        .map(|resize| resize.hit_bounds());
                }
                _ => {}
            }
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    egui_rect(hit.unwrap())
}

fn expanded_point(dockspace: &Dockspace, target: Target, hit: Rect) -> Pos2 {
    assert!(hit.width() < 44.0, "the base handle remains narrow");
    let point = Pos2::new(hit.max.x + 12.0, hit.center().y);
    assert!(!hit.contains(point));
    let logical = LogicalPoint::new(f64::from(point.x), f64::from(point.y)).unwrap();
    let touch = dockspace
        .session
        .host_pointer_hit_for_device(SURFACE, logical, PointerDeviceKind::Touch)
        .unwrap();
    assert_eq!(
        touch.drag_delivery().unwrap().descriptor().role(),
        target.role()
    );
    for device in [PointerDeviceKind::Mouse, PointerDeviceKind::Unknown] {
        let ordinary = dockspace
            .session
            .host_pointer_hit_for_device(SURFACE, logical, device)
            .unwrap();
        assert_ne!(
            ordinary
                .drag_delivery()
                .map(|receiver| receiver.descriptor().role()),
            Some(target.role()),
            "only explicit Touch expands resize geometry: {device:?}"
        );
    }
    let base = dockspace
        .session
        .host_pointer_hit(SURFACE, logical)
        .unwrap();
    assert_ne!(
        base.drag_delivery()
            .map(|receiver| receiver.descriptor().role()),
        Some(target.role())
    );
    point
}

fn ordinary_pointer_misses(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    point: Pos2,
) {
    let before = dockspace.version();
    let end = point + vec2(48.0, 0.0);
    for (time, events) in [
        (
            0.6,
            vec![Event::PointerMoved(point), pointer_button(point, true)],
        ),
        (0.7, vec![Event::PointerMoved(end)]),
        (0.8, vec![pointer_button(end, false), Event::PointerGone]),
    ] {
        let result = frame(context, dockspace, panes, time, events, Barrier::None);
        assert!(result.touch.is_empty());
        assert!(
            !result
                .feedback
                .iter()
                .any(|status| matches!(status, DockspaceManipulationStatus::Delivered(_)))
        );
        assert_eq!(dockspace.version(), before);
        assert!(!dockspace.session.has_active_gesture());
    }
}

fn resize_with_touch(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    point: Pos2,
) -> Vec<DockspaceManipulationStatus> {
    let before = dockspace.version();
    let start = frame(
        context,
        dockspace,
        panes,
        1.0,
        vec![contact(egui::TouchPhase::Start, point)],
        Barrier::None,
    );
    assert!(start.touch.is_empty());
    let wake = dockspace.session.pointer_continuation().unwrap();
    assert_eq!(wake.deadline(), std::time::Duration::from_millis(1350));
    let armed = frame(context, dockspace, panes, 1.35, Vec::new(), Barrier::None);
    assert!(matches!(
        armed.touch.as_slice(),
        [DockspaceTouchEvent::Armed { pointer, surface }]
            if *pointer == wake.pointer() && *surface == SURFACE
    ));
    assert_eq!(dockspace.version(), before);
    let end = point + vec2(48.0, 0.0);
    let moved = frame(
        context,
        dockspace,
        panes,
        1.36,
        vec![contact(egui::TouchPhase::Move, end)],
        Barrier::None,
    );
    assert!(moved.touch.is_empty());
    assert!(dockspace.session.has_active_gesture());
    let released = frame(
        context,
        dockspace,
        panes,
        1.4,
        vec![contact(egui::TouchPhase::End, end)],
        Barrier::None,
    );
    assert!(released.touch.is_empty());
    let mut feedback = moved.feedback;
    feedback.extend(released.feedback);
    for index in 0..6 {
        let settled = frame(
            context,
            dockspace,
            panes,
            1.5 + f64::from(index) * 0.05,
            Vec::new(),
            Barrier::None,
        );
        assert!(settled.touch.is_empty());
        feedback.extend(settled.feedback);
    }
    assert!(!dockspace.session.has_active_gesture());
    assert!(dockspace.session.pointer_continuation().is_none());
    feedback
}

#[test]
fn explicit_touch_resizes_splitter_outside_the_unchanged_mouse_hit() {
    let (context, mut dockspace, mut panes, hit) = setup(Target::Splitter);
    let point = expanded_point(&dockspace, Target::Splitter, hit);
    let before = dockspace
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap()
        .weights()
        .collect::<Vec<_>>();
    ordinary_pointer_misses(&context, &mut dockspace, &mut panes, point);
    let feedback = resize_with_touch(&context, &mut dockspace, &mut panes, point);
    let after = dockspace
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap()
        .weights()
        .collect::<Vec<_>>();
    assert!(after[0] > before[0] + 0.03, "{before:?} -> {after:?}");
    assert_eq!(
        feedback
            .iter()
            .filter(|status| **status
                == DockspaceManipulationStatus::Delivered(
                    DockspaceManipulationDelivery::SplitterResize
                ))
            .count(),
        1,
        "the real contact must deliver exactly one splitter resize: {feedback:?}"
    );
}

#[test]
fn explicit_touch_resizes_contained_window_from_outside_its_painted_bounds() {
    let (context, mut dockspace, mut panes, hit) = setup(Target::Contained);
    let point = expanded_point(&dockspace, Target::Contained, hit);
    let before = dockspace.view().contained(FLOATING).unwrap().rect();
    assert!(f64::from(point.x) > before.max().x());
    ordinary_pointer_misses(&context, &mut dockspace, &mut panes, point);
    assert_eq!(dockspace.view().contained(FLOATING).unwrap().rect(), before);
    let feedback = resize_with_touch(&context, &mut dockspace, &mut panes, point);
    let after = dockspace.view().contained(FLOATING).unwrap().rect();
    assert_eq!(
        (after.x(), after.y(), after.height()),
        (before.x(), before.y(), before.height())
    );
    assert!(
        after.width() > before.width() + 30.0,
        "{before:?} -> {after:?}"
    );
    assert_eq!(
        feedback
            .iter()
            .filter(|status| **status
                == DockspaceManipulationStatus::Delivered(
                    DockspaceManipulationDelivery::ContainedTransform
                ))
            .count(),
        1,
        "the real contact must deliver exactly one contained resize: {feedback:?}"
    );
}

#[test]
fn expanded_touch_resize_targets_keep_external_widget_and_disabled_ui_blockers() {
    for target in [Target::Splitter, Target::Contained] {
        for external_widget in [true, false] {
            let (context, mut dockspace, mut panes, hit) = setup(target);
            let point = expanded_point(&dockspace, target, hit);
            let end = point + vec2(48.0, 0.0);
            let barrier = if external_widget {
                Barrier::ExternalWidget(Rect::from_two_pos(point, end).expand(8.0))
            } else {
                Barrier::DisabledUi
            };
            for time in [0.5, 0.55] {
                frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    time,
                    Vec::new(),
                    barrier,
                );
            }
            let before = dockspace.version();
            for (time, events) in [
                (1.0, vec![contact(egui::TouchPhase::Start, point)]),
                (1.35, Vec::new()),
                (1.36, vec![contact(egui::TouchPhase::Move, end)]),
                (1.4, vec![contact(egui::TouchPhase::End, end)]),
                (1.5, Vec::new()),
            ] {
                let result = frame(&context, &mut dockspace, &mut panes, time, events, barrier);
                assert!(
                    result.touch.is_empty(),
                    "{target:?}, {barrier:?}: {:?}",
                    result.touch
                );
                assert_eq!(result.closes, 0);
                assert!(
                    !result
                        .feedback
                        .iter()
                        .any(|status| matches!(status, DockspaceManipulationStatus::Delivered(_)))
                );
                assert_eq!(dockspace.version(), before, "{target:?}, {barrier:?}");
                assert!(!dockspace.session.has_active_gesture());
                assert!(dockspace.session.pointer_continuation().is_none());
            }
        }
    }
}

#[test]
fn touch_resize_expansion_preserves_the_actual_contained_close_control() {
    let (context, mut dockspace, mut panes, _) = setup(Target::Contained);
    let close = {
        let mut frame = dockspace.session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let mut close = None;
        plan.visit_surface_program(|step| {
            if let Step::Operation {
                operation: Operation::ContainedControls(contained),
                ..
            } = step
            {
                close = contained.close_bounds();
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        egui_rect(close.unwrap())
    };
    let point = close.center();
    let logical = LogicalPoint::new(f64::from(point.x), f64::from(point.y)).unwrap();
    let hit = dockspace
        .session
        .host_pointer_hit_for_device(SURFACE, logical, PointerDeviceKind::Touch)
        .unwrap();
    assert_eq!(
        hit.delivery().unwrap().descriptor().role(),
        DockspaceReceiverRole::ContainedClose
    );
    assert_ne!(
        hit.drag_delivery()
            .map(|receiver| receiver.descriptor().role()),
        Some(DockspaceReceiverRole::ContainedResize)
    );
    let before = dockspace.view().contained(FLOATING).unwrap().rect();
    let pressed = frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.0,
        vec![contact(egui::TouchPhase::Start, point)],
        Barrier::None,
    );
    assert!(pressed.touch.is_empty());
    assert_eq!(pressed.closes, 0);
    assert!(dockspace.session.pointer_continuation().is_none());
    let released = frame(
        &context,
        &mut dockspace,
        &mut panes,
        1.1,
        vec![contact(egui::TouchPhase::End, point)],
        Barrier::None,
    );
    assert_eq!(released.closes, 1);
    assert!(released.touch.is_empty());
    assert!(!dockspace.session.has_active_gesture());
    assert_eq!(dockspace.view().contained(FLOATING).unwrap().rect(), before);
}
