use egui::{Context, Event, FullOutput, Modifiers, PointerButton, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    CloseDecision, Dockspace, DockspaceAxis, DockspaceCloseRequestStatus, DockspaceContainedLayout,
    DockspaceLayout, DockspaceManipulationCancelReason as CancelReason,
    DockspaceManipulationDelivery as Delivery, DockspaceManipulationFeedback as Feedback,
    DockspaceManipulationPendingReason as PendingReason, DockspaceManipulationStatus as Status,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    LogicalRect, PaneView, RootId, SurfaceId, SurfacePresentationResult, run_ui,
};

const SURFACE: SurfaceId = SurfaceId::new(1);

struct Panes;

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(
            if item == ItemId::new(1) {
                "First"
            } else {
                "Second"
            }
            .into(),
        )
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        ui.label(format!("Pane {}", item.get()));
    }
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn frame(
    context: &Context,
    dockspace: &mut Dockspace,
    events: Vec<Event>,
    disposition: SurfacePresentationResult,
) -> (FullOutput, Vec<Feedback>) {
    let input = RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..RawInput::default()
    };
    let mut feedback = Vec::new();
    let (mut output, receipt) = run_ui(context, input, |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut Panes)
            .unwrap();
        feedback.extend_from_slice(response.manipulation_feedback());
    })
    .into_parts();
    receipt.report(disposition);
    // This recording host inspects semantics without uploading GPU textures.
    output.textures_delta.clear();
    (output, feedback)
}

fn fixture() -> (Context, Dockspace, Pos2) {
    let split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([ItemId::new(1)]),
            DockspaceNode::tabs([ItemId::new(2)]),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), split),
    )])
    .unwrap();
    let (context, dockspace, output) = ready(layout);
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::Tab && node.label() == Some("Second"))
                .then(|| node.bounds().unwrap())
        })
        .unwrap();
    let source = Pos2::new(
        ((bounds.x0 + bounds.x1) * 0.5) as f32,
        ((bounds.y0 + bounds.y1) * 0.5) as f32,
    );
    (context, dockspace, source)
}

fn ready(layout: DockspaceLayout) -> (Context, Dockspace, FullOutput) {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("manipulation-feedback", layout)
        .build()
        .unwrap();
    let _ = frame(
        &context,
        &mut dockspace,
        Vec::new(),
        SurfacePresentationResult::Presented,
    );
    let (output, feedback) = frame(
        &context,
        &mut dockspace,
        Vec::new(),
        SurfacePresentationResult::Presented,
    );
    assert!(feedback.is_empty());
    (context, dockspace, output)
}

#[test]
fn escape_and_context_retirement_publish_one_terminal_without_focus_theft() {
    for escape in [false, true] {
        let (context, mut dockspace, source) = fixture();
        frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(source), button(source, true)],
            SurfacePresentationResult::Presented,
        );
        let (_, motion) = frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(Pos2::new(200.0, 340.0))],
            SurfacePresentationResult::Presented,
        );
        assert!(motion.is_empty());
        let version = dockspace.version();
        let focus_before = context.memory(|memory| memory.focused());
        let replacement = Context::default();
        replacement.enable_accesskit();
        let context = if escape { &context } else { &replacement };
        let events = if escape {
            vec![Event::Key {
                key: egui::Key::Escape,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            }]
        } else {
            Vec::new()
        };
        let (_, terminal) = frame(
            context,
            &mut dockspace,
            events,
            SurfacePresentationResult::Presented,
        );
        let [terminal] = terminal.as_slice() else {
            panic!("one committed cancellation: {terminal:?}")
        };
        assert_eq!(
            terminal.status(),
            Status::Cancelled(if escape {
                CancelReason::Escape
            } else {
                CancelReason::PointerProviderRetired
            })
        );
        assert_eq!(dockspace.version(), version);
        if escape {
            assert!(focus_before.is_some());
            // egui clears focus on Escape before Dockspace receives the input.
            // Cancellation feedback must not request a substitute focus target.
            assert_eq!(context.memory(|memory| memory.focused()), None);
        }
        for _ in 0..3 {
            let (_, repeated) = frame(
                context,
                &mut dockspace,
                Vec::new(),
                SurfacePresentationResult::Presented,
            );
            assert!(repeated.is_empty());
            assert_eq!(context.memory(|memory| memory.focused()), None);
            assert_eq!(dockspace.version(), version);
        }
    }
}

#[test]
fn preceding_presentation_report_preserves_feedback_identity_and_terminal_once() {
    for disposition in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let (context, mut dockspace, source) = fixture();
        let (_, press) = frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(source), button(source, true)],
            SurfacePresentationResult::Presented,
        );
        assert!(press.is_empty());
        let painted = Pos2::new(400.0, 72.0);
        for _ in 0..3 {
            let (_, motion) = frame(
                &context,
                &mut dockspace,
                vec![Event::PointerMoved(painted)],
                SurfacePresentationResult::Presented,
            );
            assert!(motion.is_empty());
        }
        let target = Pos2::new(400.0, 528.0);
        let (_, released) = frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(target), button(target, false)],
            disposition,
        );
        let [pending] = released.as_slice() else {
            panic!("the release must publish one pending feedback record: {released:?}");
        };
        assert_eq!(
            pending.status(),
            Status::Pending(PendingReason::AwaitingPresentation)
        );
        // This invocation consumes the prior output observation before its paint
        // report. include_preceding_reports must preserve that terminal feedback.
        let (_, settled) = frame(
            &context,
            &mut dockspace,
            Vec::new(),
            SurfacePresentationResult::Presented,
        );
        let [terminal] = settled.as_slice() else {
            panic!("the preceding report must forward exactly one terminal record: {settled:?}");
        };
        assert_eq!(pending.manipulation_id(), terminal.manipulation_id());
        assert_ne!(pending.id(), terminal.id());
        assert_eq!(
            terminal.status(),
            if disposition == SurfacePresentationResult::Presented {
                Status::Delivered(Delivery::Dock)
            } else {
                Status::Cancelled(CancelReason::SceneUnavailable)
            }
        );
        for _ in 0..2 {
            let (_, repeated) = frame(
                &context,
                &mut dockspace,
                Vec::new(),
                SurfacePresentationResult::Presented,
            );
            assert!(repeated.is_empty());
        }
    }
}

#[test]
fn programmatic_dock_back_and_close_return_pending_cancellation_in_their_mutation() {
    for close in [false, true] {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
            LogicalRect::new(400.0, 140.0, 300.0, 240.0).unwrap(),
        ))])
        .unwrap();
        let (context, mut dockspace, output) = ready(layout);
        let bounds = output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .find_map(|(_, node)| {
                (node.label() == Some("Resize floating right edge")).then(|| node.bounds().unwrap())
            })
            .unwrap();
        let source = Pos2::new(
            ((bounds.x0 + bounds.x1) * 0.5) as f32,
            ((bounds.y0 + bounds.y1) * 0.5) as f32,
        );
        frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(source), button(source, true)],
            SurfacePresentationResult::Presented,
        );
        let moved = source + vec2(24.0, 0.0);
        frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(moved)],
            SurfacePresentationResult::Presented,
        );
        let released = source + vec2(48.0, 0.0);
        let (_, feedback) = frame(
            &context,
            &mut dockspace,
            vec![Event::PointerMoved(released), button(released, false)],
            SurfacePresentationResult::Presented,
        );
        let [pending] = feedback.as_slice() else {
            panic!("release must wait: {feedback:?}")
        };
        assert_eq!(
            pending.status(),
            Status::Pending(PendingReason::AwaitingPresentation)
        );
        let cancelled = if close {
            let request = dockspace
                .request_close_root_current(RootId::new(2))
                .unwrap();
            let DockspaceCloseRequestStatus::Requested(request) = request.status() else {
                panic!("application root close must open a plan")
            };
            let plan = request.plan();
            dockspace
                .resolve_close(
                    plan.request(),
                    plan.items()[0].token(),
                    CloseDecision::Allow,
                )
                .unwrap()
                .mutation()
                .manipulation_feedback()
                .to_vec()
        } else {
            dockspace
                .dock_root_back_current(RootId::new(2))
                .unwrap()
                .mutation()
                .manipulation_feedback()
                .to_vec()
        };
        let [terminal] = cancelled.as_slice() else {
            panic!("command must return one terminal: {cancelled:?}")
        };
        assert_eq!(terminal.manipulation_id(), pending.manipulation_id());
        assert_ne!(terminal.id(), pending.id());
        assert_eq!(
            terminal.status(),
            Status::Cancelled(CancelReason::WorkspaceChanged)
        );
        assert_eq!(dockspace.view().item(ItemId::new(2)).is_none(), close);
        for _ in 0..3 {
            let (_, repeated) = frame(
                &context,
                &mut dockspace,
                Vec::new(),
                SurfacePresentationResult::Presented,
            );
            assert!(
                repeated.is_empty(),
                "direct results are not replayed by show"
            );
        }
    }
}
