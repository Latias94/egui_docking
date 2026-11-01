//! Modifier-only magnetic controls through real host input and paint programs.

use crate::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalSize, ScaleFactor,
};
use crate::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use crate::policy::DockPolicy;
use crate::runtime::{
    DockspaceHostFrame, DockspacePreviewVisual, DockspaceSession, HostFrameReport,
    HostInputOutcome, HostPointerInput, HostPointerRoute, HostWindowToken, NativeCloseState,
    NativeCreateStrategy, NativeGlobalFocus, NativeHostCapabilities, NativeHostCapability,
    NativeLocalPointerFact, NativeLocalPointerLocation, NativePointerButton, NativePointerEvent,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeReceiverAnswer, NativeReceiverPurpose, NativeSurfaceBinding, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    SurfacePaintDisposition, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const HOST: SurfaceId = SurfaceId::new(1);
const CHILD: SurfaceId = SurfaceId::new(2);
const UNFOCUSED: SurfaceId = SurfaceId::new(3);
const HOST_ROOT: RootId = RootId::new(1);
const SOURCE_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const BACKGROUND: ItemId = ItemId::new(3);
const TARGET: ItemId = ItemId::new(4);

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

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn measure_and_present(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        rect(0.0, 0.0, 960.0, 720.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap();
    let mut measured = session.begin_host_frame().unwrap();
    for surface in measured.surfaces() {
        measured.measure_surface(surface, metrics).unwrap();
    }
    commit(measured);

    let mut painted = session.begin_host_frame().unwrap();
    for surface in painted.surfaces() {
        let receipt = painted
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        painted.confirm_surface_painted(receipt).unwrap();
    }
    let mut report = commit(painted);
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), session.view().surfaces().count());
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    commit(session.begin_host_frame().unwrap());
}

#[derive(Debug, PartialEq)]
struct ContainedGeometry {
    outer: LogicalRect,
    title: LogicalRect,
    panes: Vec<(ItemId, LogicalRect)>,
}

fn contained_geometry(frame: &mut DockspaceHostFrame<'_>) -> ContainedGeometry {
    let plan = frame.paint_plan(HOST).unwrap().unwrap();
    let mut outer = None;
    let mut title = None;
    let mut panes = Vec::new();
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, .. } if root.root() == SOURCE_ROOT => {
                let contained = root.contained().unwrap();
                assert_eq!(contained.floating(), FLOATING);
                assert!(outer.replace(contained.outer_bounds()).is_none());
                title = Some(
                    plan.receiver_for_contained_title(contained)
                        .unwrap()
                        .bounds(),
                );
            }
            Step::Operation {
                operation: Operation::Pane(pane),
                paint,
                ..
            } if pane.root() == SOURCE_ROOT => {
                assert_eq!(paint, SurfacePaintDisposition::Visible);
                panes.push((pane.selected().unwrap(), pane.content_bounds()));
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert_eq!(
        panes.iter().map(|(item, _)| *item).collect::<Vec<_>>(),
        [FIRST, SECOND]
    );
    ContainedGeometry {
        outer: outer.unwrap(),
        title: title.unwrap(),
        panes,
    }
}

#[test]
fn contained_modifier_only_pause_and_reenable_preserve_complete_moved_geometry() {
    let initial = rect(200.0, 140.0, 400.0, 300.0);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        HOST,
        DockspaceRootLayout::new(HOST_ROOT, DockspaceNode::central_tabs([BACKGROUND])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(
            SOURCE_ROOT,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [DockspaceNode::tabs([FIRST]), DockspaceNode::tabs([SECOND])],
            )
            .unwrap(),
        ),
        initial,
    ))])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    measure_and_present(&mut session);
    let version = session.version();
    let mut frame = session.begin_host_frame().unwrap();
    let before = contained_geometry(&mut frame);
    commit(frame);
    assert_eq!(before.outer, initial);

    let start = center(before.title);
    let hit = session.host_pointer_hit(HOST, start).unwrap();
    let source = hit.drag_delivery().unwrap();
    let surface = hit.surface();
    let moved = LogicalPoint::new(start.x() + 80.0, start.y() + 48.0).unwrap();
    let shifted = |bounds: LogicalRect| {
        rect(
            bounds.x() + 80.0,
            bounds.y() + 48.0,
            bounds.width(),
            bounds.height(),
        )
    };
    let expected = ContainedGeometry {
        outer: shifted(before.outer),
        title: shifted(before.title),
        panes: before
            .panes
            .iter()
            .map(|(item, bounds)| (*item, shifted(*bounds)))
            .collect(),
    };
    let route = HostPointerRoute::new(surface, start);
    let moved_route = HostPointerRoute::new(surface, moved);
    let mut drag = session.begin_host_frame().unwrap();
    drag.submit_host_pointer_batch([
        HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&source).with_clear_hover_lane(&surface),
        ),
        HostPointerInput::new(
            SurfacePointerId::new(1),
            SurfacePointerEvent::Moved,
            moved_route,
            moved_route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
        ),
    ])
    .unwrap();
    assert_eq!(contained_geometry(&mut drag), expected);
    assert!(!commit(drag).workspace_changed());

    for enabled in [false, true] {
        let mut control = session.begin_host_frame().unwrap();
        control
            .submit_host_pointer(HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::MagneticTargetingChanged { enabled },
                HostPointerRoute::Unknown,
                HostPointerRoute::Unknown,
                SurfacePointerCapture::Unknown,
                SurfacePointerReceiverFacts::unknown(),
            ))
            .unwrap();
        assert_eq!(contained_geometry(&mut control), expected);
        let report = commit(control);
        assert_eq!(
            report.inputs(),
            [HostInputOutcome::MagneticTargetingUpdated {
                enabled,
                changed: true
            }],
        );
        assert!(!report.workspace_changed());
        assert!(session.has_active_gesture());
        assert_eq!(session.version(), version);
        assert_eq!(session.view().contained(FLOATING).unwrap().rect(), initial);

        let mut idle = session.begin_host_frame().unwrap();
        assert_eq!(contained_geometry(&mut idle), expected);
        commit(idle);
    }
}

fn local_native_input(
    session: &DockspaceSession,
    binding: NativeSurfaceBinding,
    event: NativePointerEvent,
    point: LogicalPoint,
) -> NativePointerInput {
    let coordinates = session.native_local_coordinates(binding).unwrap();
    let geometry = coordinates.geometry();
    let sample = coordinates
        .capture(
            binding,
            NativeLocalPointerFact::new(
                PhysicalPoint::new(point.x(), point.y()).unwrap(),
                geometry.content_size(),
                geometry.native_scale_factor(),
            ),
            geometry,
        )
        .unwrap();
    NativePointerInput::new_local(
        NativePointerId::new(1),
        event,
        NativeLocalPointerLocation::Exact(sample),
        NativePointerOwner::Native(binding),
        NativePointerOwner::Native(binding),
    )
}

fn child_preview(frame: &mut DockspaceHostFrame<'_>) -> Option<DockspacePreviewVisual> {
    let mut preview = None;
    frame
        .paint_plan(CHILD)
        .unwrap()
        .unwrap()
        .visit_surface_program(|step| {
            if let Step::DragPreview(record) = step {
                assert!(preview.replace(record.visual()).is_none());
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    preview
}

#[test]
fn focused_root_controls_child_magnetism_but_unfocused_binding_is_inert() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(HOST_ROOT, DockspaceNode::central_tabs([BACKGROUND])),
        ),
        DockspaceSurfaceLayout::new(
            CHILD,
            DockspaceRootLayout::new(
                SOURCE_ROOT,
                DockspaceNode::equal_split(
                    DockspaceAxis::Horizontal,
                    [
                        DockspaceNode::tabs([FIRST, SECOND]),
                        DockspaceNode::tabs([TARGET]),
                    ],
                )
                .unwrap(),
            ),
        ),
        DockspaceSurfaceLayout::new(
            UNFOCUSED,
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ItemId::new(5)])),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host(
            NativeCreateStrategy::Staged,
            NativePointerRoster::Exact(Vec::new()),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::NativeWindowLifecycle)
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::GlobalFocusObservation)
                .with(NativeHostCapability::PointerHitTestObservation),
        )
        .unwrap();
    for (surface, token) in [(HOST, 11), (UNFOCUSED, 33)] {
        session
            .register_native_root(surface, HostWindowToken::new(token))
            .unwrap();
    }
    commit(session.begin_host_frame().unwrap());
    session
        .register_owned_native_child(CHILD, HostWindowToken::new(22), HOST)
        .unwrap();
    let registration = commit(session.begin_host_frame().unwrap());
    let binding = |surface| {
        registration
            .native_bindings()
            .iter()
            .copied()
            .find(|binding| binding.surface() == surface)
            .unwrap()
    };
    let root = binding(HOST);
    let child = binding(CHILD);
    let unfocused = binding(UNFOCUSED);
    let scale = ScaleFactor::new(1.0).unwrap();
    let facts = NativeWindowFacts::live()
        .with_content_size(PhysicalSize::new(960.0, 720.0).unwrap())
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None);
    session
        .report_managed_native_snapshot(
            [root, child, unfocused].map(|binding| (binding, facts)),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    commit(session.begin_host_frame().unwrap());
    session
        .report_native_global_focus(NativeGlobalFocus::Dock(root))
        .unwrap();
    commit(session.begin_host_frame().unwrap());
    measure_and_present(&mut session);

    let mut inspect = session.begin_host_frame().unwrap();
    let plan = inspect.paint_plan(CHILD).unwrap().unwrap();
    let source = plan
        .receiver_for_tab_body(plan.tabs().find(|tab| tab.item() == FIRST).unwrap())
        .unwrap();
    let mut magnetic = None;
    let mut guides = Vec::new();
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Pane(pane),
                ..
            } if pane.selected() == Some(TARGET) => {
                let bounds = pane.content_bounds();
                magnetic = Some(
                    LogicalPoint::new(
                        bounds.x() + bounds.width() * 0.65,
                        bounds.y() + bounds.height() * 0.65,
                    )
                    .unwrap(),
                );
            }
            Step::DropGuide { structural, .. } => {
                guides.extend(structural.targets().map(|target| target.hit_bounds()));
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let magnetic = magnetic.unwrap();
    assert!(guides.iter().all(|bounds| !bounds.contains(magnetic)));
    commit(inspect);
    let version = session.version();
    let press = local_native_input(
        &session,
        child,
        NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
        source.center(),
    );
    let moved = local_native_input(&session, child, NativePointerEvent::Moved, magnetic);
    session.record_native_pointer_batch([press, moved]).unwrap();
    let mut dragging = session
        .begin_native_host_frame(|query| {
            assert!(query.matches_native_binding(child));
            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
                NativeReceiverAnswer::ClearHoverLane(query.presented_surface())
            } else {
                NativeReceiverAnswer::Dock(query.bind_receiver(&source).unwrap())
            }
        })
        .unwrap();
    let acquired = child_preview(&mut dragging);
    assert!(matches!(
        acquired,
        Some(DockspacePreviewVisual::Dock { surface: CHILD, .. })
    ));
    assert!(!commit(dragging).workspace_changed());
    assert!(session.has_active_gesture());

    for (binding, enabled, changed, expected) in [
        (unfocused, false, false, acquired),
        (root, false, true, None),
        (unfocused, true, false, None),
        (root, true, true, None),
    ] {
        session
            .report_native_magnetic_targeting(binding, enabled)
            .unwrap();
        let mut control = session
            .begin_native_host_frame(|_| {
                panic!("modifier-only input must not query pointer receivers")
            })
            .unwrap();
        assert_eq!(child_preview(&mut control), expected);
        let report = commit(control);
        assert_eq!(
            report.inputs(),
            [HostInputOutcome::MagneticTargetingUpdated { enabled, changed }]
        );
        assert!(!report.workspace_changed());
        assert!(session.has_active_gesture());
        assert_eq!(session.version(), version);
    }

    let moved = local_native_input(&session, child, NativePointerEvent::Moved, magnetic);
    session.record_native_pointer(moved).unwrap();
    let mut reacquired = session
        .begin_native_host_frame(|query| {
            assert!(matches!(query.purpose(), NativeReceiverPurpose::HoverHit));
            NativeReceiverAnswer::ClearHoverLane(query.presented_surface())
        })
        .unwrap();
    assert_eq!(child_preview(&mut reacquired), acquired);
    assert!(!commit(reacquired).workspace_changed());
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
}
