//! Drag decoration survives strip clipping without granting pointer authority.

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceDragSourceKind, DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceSession,
    DockspaceVisualId, HostFrameReport, HostPointerInput, HostPointerRoute, PresentedDockReceiver,
    PresentedDockspaceSurface, SurfacePaintDisposition as Paint, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton as Button, SurfacePointerCapture, SurfacePointerEvent as Event,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, TabStripMemberVisibility, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SOURCE: SurfaceId = SurfaceId::new(1);
const TARGET: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const CONTAINED_ROOT: RootId = RootId::new(3);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn center(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .unwrap()
}

fn layout(contained: bool) -> DockspaceLayout {
    let mut source = DockspaceSurfaceLayout::new(
        SOURCE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs((1..=8).map(ItemId::new)),
        ),
    );
    if contained {
        source = source.with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(
                CONTAINED_ROOT,
                DockspaceNode::equal_split(
                    DockspaceAxis::Horizontal,
                    [
                        DockspaceNode::tabs([ItemId::new(20)]),
                        DockspaceNode::tabs([ItemId::new(21)]),
                    ],
                )
                .unwrap(),
            ),
            rect(10.0, 160.0, 220.0, 240.0),
        ));
    }
    DockspaceLayout::new([
        source,
        DockspaceSurfaceLayout::new(
            TARGET,
            DockspaceRootLayout::new(
                RootId::new(2),
                DockspaceNode::central_tabs([ItemId::new(9), ItemId::new(10)]),
            ),
        ),
    ])
    .unwrap()
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    for (surface, width) in [(SOURCE, 240.0), (TARGET, 800.0)] {
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    rect(0.0, 0.0, width, 600.0),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
    }
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn present(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    for surface in [SOURCE, TARGET] {
        let (_, receipt) =
            RecordingRenderer::execute(frame.paint_plan(surface).unwrap().unwrap()).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
    }
    let outputs = finish(frame).take_painted_outputs();
    assert_eq!(outputs.len(), 2);
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    finish(session.begin_host_frame().unwrap());
}

fn fixture(contained: bool) -> DockspaceSession {
    let mut session =
        DockspaceSession::from_layout(layout(contained), DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    measure(&mut frame);
    finish(frame);
    present(&mut session);
    session
}

fn source(session: &mut DockspaceSession) -> PresentedDockReceiver {
    let mut frame = session.begin_host_frame().unwrap();
    let descriptor = frame
        .paint_plan(SOURCE)
        .unwrap()
        .unwrap()
        .tab_receiver(FIRST)
        .unwrap();
    drop(frame);
    session.bind_presented_receiver(&descriptor).unwrap()
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Decoration {
    surface: SurfaceId,
    kind: DockspaceDragSourceKind,
    visual: DockspaceVisualId,
    bounds: LogicalRect,
    item: Option<ItemId>,
    ghost: Option<ItemId>,
    projected: Option<LogicalRect>,
    is_projected: bool,
}

fn decoration(plan: SurfacePaintPlan<'_>) -> Option<Decoration> {
    let mut result = None;
    plan.visit_surface_program(|step| {
        if let Step::DragDecoration(source) = step {
            assert!(
                result
                    .replace(Decoration {
                        surface: source.source_surface(),
                        kind: source.kind(),
                        visual: source.source_visual(),
                        bounds: source.source_bounds(),
                        item: source.item(),
                        ghost: source.ghost_item(),
                        projected: source.projected_root_bounds(),
                        is_projected: source.source_is_projected(),
                    })
                    .is_none(),
                "one gesture emits one source decoration per program"
            );
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    result
}

fn drag_inputs<'a>(
    source: PresentedDockspaceSurface,
    receiver: &'a PresentedDockReceiver,
    hover: &'a PresentedDockspaceSurface,
    point: LogicalPoint,
) -> [HostPointerInput<'a>; 2] {
    let press = HostPointerRoute::new(source, receiver.center());
    let moved =
        LogicalPoint::new(receiver.center().x() + 48.0, receiver.center().y() + 32.0).unwrap();
    [
        HostPointerInput::new(
            SurfacePointerId::new(1),
            Event::ButtonPressed(Button::Primary),
            press,
            press,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(receiver).with_drag_delivery(receiver),
        ),
        HostPointerInput::new(
            SurfacePointerId::new(1),
            Event::Moved,
            HostPointerRoute::new(source, moved),
            HostPointerRoute::new(*hover, point),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(hover),
        ),
    ]
}

#[test]
fn scrolling_the_source_tab_out_of_its_own_strip_preserves_the_drag_ghost() {
    let mut session = fixture(false);
    let receiver = source(&mut session);
    let a = session.presented_surface(SOURCE).unwrap();
    let b = session.presented_surface(TARGET).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(drag_inputs(
            a,
            &receiver,
            &b,
            center(rect(0.0, 0.0, 800.0, 600.0)),
        ))
        .unwrap();
    finish(frame);
    assert!(session.has_active_gesture());
    let version = session.version();

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SOURCE).unwrap().unwrap();
    let before = decoration(plan).unwrap();
    assert_eq!(before.kind, DockspaceDragSourceKind::Item);
    assert_eq!(before.item, Some(FIRST));
    assert_eq!(before.ghost, Some(FIRST));
    assert!(!before.is_projected);
    let mut scroll = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Tab(tab),
                paint,
                ..
            } if tab.item() == FIRST => {
                assert_eq!(paint, Paint::SourceGap);
                assert_eq!(tab.visual_id(), before.visual);
            }
            Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } => {
                assert!(bar.maximum_scroll_offset() > before.bounds.width());
                scroll = plan.prepare_tab_strip_scroll_at(
                    bar,
                    center(bar.viewport()),
                    bar.maximum_scroll_offset(),
                );
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let scroll = scroll.unwrap();
    drop(frame);

    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_surface_action(scroll).unwrap();
    measure(&mut frame);
    assert!(!finish(frame).workspace_changed());
    assert_eq!(session.version(), version);
    assert!(session.has_active_gesture());

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SOURCE).unwrap().unwrap();
    assert!(plan.tab_receiver(FIRST).is_none());
    let after = decoration(plan).expect("a clipped source still owns its pointer ghost");
    let mut hidden_bounds = None;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            operation: Operation::TabBar(bar),
            ..
        } = step
        {
            let member = bar.members().find(|member| member.item() == FIRST).unwrap();
            assert_eq!(member.visibility(), TabStripMemberVisibility::Hidden);
            assert!(member.full_bounds().max().x() <= bar.viewport().x());
            hidden_bounds = Some(member.full_bounds());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(after.bounds, hidden_bounds.unwrap());
    assert!(after.bounds.x() < before.bounds.x());
    assert_eq!(after.bounds.width(), before.bounds.width());
    assert_eq!(after.bounds.height(), before.bounds.height());
    assert_eq!(
        Decoration {
            bounds: before.bounds,
            ..after
        },
        before
    );
    assert_eq!(
        decoration(frame.paint_plan(TARGET).unwrap().unwrap()),
        Some(after)
    );
    finish(frame);
    present(&mut session);

    // Retaining the source ghost cannot substitute for an absent hover proof.
    for event in [Event::Moved, Event::ButtonReleased(Button::Primary)] {
        let a = session.presented_surface(SOURCE).unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(HostPointerInput::new(
                SurfacePointerId::new(1),
                event,
                HostPointerRoute::new(a, LogicalPoint::new(180.0, 500.0).unwrap()),
                HostPointerRoute::Unknown,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            ))
            .unwrap();
        let plan = frame.paint_plan(TARGET).unwrap().unwrap();
        if event == Event::Moved {
            assert_eq!(decoration(plan), Some(after));
            plan.visit_surface_program(|step| {
                assert!(!matches!(step, Step::DragPreview(_)));
                Ok::<(), ()>(())
            })
            .unwrap();
        } else {
            assert!(decoration(plan).is_none());
        }
        assert!(!finish(frame).workspace_changed());
    }
    assert!(!session.has_active_gesture());
    assert_eq!(session.version(), version);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), SOURCE);
}

#[test]
fn foreign_and_retired_source_proofs_cannot_create_a_drag_decoration() {
    for foreign in [false, true] {
        let mut session = fixture(false);
        let (invalid, receiver) = if foreign {
            let mut other = fixture(false);
            let receiver = source(&mut other);
            (other.presented_surface(SOURCE).unwrap(), receiver)
        } else {
            let receiver = source(&mut session);
            let old = session.presented_surface(SOURCE).unwrap();
            let reset = session.prepare_layout_reset(layout(false)).unwrap();
            let mut frame = session.begin_layout_reset_frame(&reset).unwrap();
            measure(&mut frame);
            finish(frame);
            present(&mut session);
            (old, receiver)
        };
        assert_ne!(invalid, session.presented_surface(SOURCE).unwrap());
        assert!(
            session
                .bind_presented_receiver(&receiver.descriptor())
                .is_none()
        );
        let b = session.presented_surface(TARGET).unwrap();
        let version = session.version();
        let mut frame = session.begin_host_frame().unwrap();
        if frame
            .submit_host_pointer_batch(drag_inputs(
                invalid,
                &receiver,
                &b,
                LogicalPoint::new(400.0, 300.0).unwrap(),
            ))
            .is_ok()
        {
            for surface in [SOURCE, TARGET] {
                assert!(decoration(frame.paint_plan(surface).unwrap().unwrap()).is_none());
            }
            assert!(!finish(frame).workspace_changed());
        } else {
            drop(frame);
        }
        assert!(!session.has_active_gesture());
        assert_eq!(session.version(), version);
    }
}

#[test]
fn a_projected_contained_root_keeps_live_content_and_suppresses_a_duplicate_ghost() {
    let mut session = fixture(true);
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SOURCE).unwrap().unwrap();
    let mut title: Option<DockspaceReceiverDescriptor> = None;
    plan.visit_surface_program(|step| {
        if let Step::RootBegin { root, .. } = step
            && root.root() == CONTAINED_ROOT
        {
            title = plan.receiver_for_contained_title(root.contained().unwrap());
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    drop(frame);
    let receiver = session.bind_presented_receiver(&title.unwrap()).unwrap();
    let a = session.presented_surface(SOURCE).unwrap();
    let moved =
        LogicalPoint::new(receiver.center().x() + 48.0, receiver.center().y() + 32.0).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch(drag_inputs(a, &receiver, &a, moved))
        .unwrap();
    finish(frame);

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SOURCE).unwrap().unwrap();
    let source = decoration(plan).unwrap();
    assert_eq!(source.kind, DockspaceDragSourceKind::Root);
    assert_eq!(source.surface, SOURCE);
    assert!(
        source.is_projected,
        "the renderer must suppress a second pointer ghost"
    );
    // The 220px root remains inside its 240px source surface.
    assert_eq!(source.projected, Some(rect(20.0, 192.0, 220.0, 240.0)));
    let mut panes = 0;
    plan.visit_surface_program(|step| {
        if let Step::Operation {
            root,
            operation,
            paint,
        } = step
            && root.root() == CONTAINED_ROOT
        {
            assert_eq!(paint, Paint::Visible);
            if let Operation::Pane(_) = operation {
                panes += 1;
            }
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(panes, 2, "the projected content is painted exactly once");
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    finish(frame);
}
