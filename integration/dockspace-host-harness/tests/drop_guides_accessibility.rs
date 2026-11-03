//! Visible drop guide buttons select their advertised operation.

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceDropDirection as Direction, DockspaceGuideScope as Scope,
    DockspaceHostFrame, DockspaceSession, HostPointerInput, HostPointerRoute,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);

fn center(bounds: LogicalRect) -> LogicalPoint {
    LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .unwrap()
}

#[test]
fn every_visible_guide_center_keeps_its_identity_in_compact_scaled_and_occluded_layouts() {
    let main_root = RootId::new(1);
    for (bounds, scale, occluded) in [
        (
            LogicalRect::new(0.0, 0.0, 1321.0, 600.0).unwrap(),
            1.0,
            false,
        ),
        (
            LogicalRect::new(0.0, 0.0, 362.0, 120.0).unwrap(),
            1.0,
            false,
        ),
        (
            LogicalRect::new(13.25, -37.5, 480.0, 240.0).unwrap(),
            2.5,
            false,
        ),
        (
            LogicalRect::new(0.0, 0.0, 1321.0, 600.0).unwrap(),
            1.0,
            true,
        ),
    ] {
        let node = DockspaceNode::split(
            DockspaceAxis::Horizontal,
            [
                (DockspaceNode::tabs([FIRST, SECOND]), 0.2),
                (DockspaceNode::central_tabs([THIRD, FOURTH]), 0.8),
            ],
        )
        .unwrap();
        let covered = LogicalRect::new(40.0, 170.0, 460.0, 300.0).unwrap();
        let mut surface =
            DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(main_root, node));
        if occluded {
            surface = surface.with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(1),
                DockspaceRootLayout::new(
                    RootId::new(2),
                    DockspaceNode::tabs([ItemId::new(20), ItemId::new(21)]),
                ),
                covered,
            ));
        }
        let config = DockPresentationConfig::builder()
            .guide_extent(48.0 * scale)
            .guide_gap(12.0 * scale)
            .guide_hit_padding(6.0 * scale)
            .guide_outer_inset(64.0 * scale)
            .build()
            .unwrap();
        let mut session = DockspaceSession::from_layout_with_presentation_config(
            DockspaceLayout::new([surface]).unwrap(),
            DockPolicy::default(),
            config,
        )
        .unwrap();
        session.enable_host_pointer().unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(bounds, LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
                    .unwrap(),
            )
            .unwrap();
        frame.commit().unwrap();

        let mut frame = session.begin_host_frame().unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let source = plan.tab_receiver(THIRD).unwrap();
        let mut buttons = Vec::new();
        let receipt = plan
            .execute_surface_program(|step| {
                if let Step::DropGuide { structural, .. } = step {
                    buttons.extend(structural.targets().map(|target| {
                        (
                            structural.root(),
                            structural.scope(),
                            target.direction(),
                            target.visual_id(),
                            target.draw_bounds(),
                            target.hit_bounds(),
                        )
                    }));
                }
                Ok::<(), ()>(())
            })
            .unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let mut outputs = frame.commit().unwrap().take_painted_outputs();
        assert_eq!(outputs.len(), 1);
        session
            .report_surface_presentation(
                outputs.pop().unwrap(),
                SurfacePresentationResult::Presented,
            )
            .unwrap();
        commit_deferred(session.begin_host_frame().unwrap());
        assert!(buttons.iter().any(|(_, scope, direction, ..)| {
            *scope == Scope::Inner && *direction == Direction::Left
        }));
        assert!(buttons.iter().any(|(_, scope, ..)| *scope == Scope::Outer));
        let presented = session.presented_surface(SURFACE).unwrap();
        let receiver = session.bind_presented_receiver(&source).unwrap();
        let press = HostPointerRoute::new(presented, source.center());
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer(HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::ButtonPressed(Button::Primary),
                press,
                press,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&presented),
            ))
            .unwrap();
        commit_deferred(frame);
        for (root, scope, direction, visual, draw, hit) in &buttons {
            assert!(draw.width() > 0.0 && draw.height() > 0.0);
            let aim = center(*draw);
            assert!(hit.contains(aim));
            if occluded && *root == main_root {
                assert!(
                    hit.max().x() <= covered.x()
                        || hit.x() >= covered.max().x()
                        || hit.max().y() <= covered.y()
                        || hit.y() >= covered.max().y(),
                    "a rear guide's draw and hit geometry must avoid the front contained root"
                );
            }
            assert_eq!(
                buttons
                    .iter()
                    .filter(|(other_root, _, _, _, _, hit)| {
                        other_root == root && hit.contains(aim)
                    })
                    .count(),
                1,
                "a button center must not also belong to another same-root guide"
            );
            let presented = session.presented_surface(SURFACE).unwrap();
            let route = HostPointerRoute::new(presented, aim);
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .submit_host_pointer(HostPointerInput::new(
                    SurfacePointerId::new(1),
                    Event::Moved,
                    route,
                    route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&presented),
                ))
                .unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            let mut active = Vec::new();
            let mut expected = None;
            plan.visit_surface_program(|step| {
                if let Step::DropGuide {
                    affordance: Some(cluster),
                    ..
                } = step
                {
                    for target in cluster.targets() {
                        if target.visual_id() == *visual {
                            assert_eq!(target.draw_bounds(), *draw);
                            expected = plan.receiver_for_drop_target(target);
                        }
                        if target.is_active() {
                            active.push(target.visual_id());
                        }
                    }
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            let expected = expected.expect("the aimed guide must be in the visible affordance");
            assert_eq!(
                active,
                [*visual],
                "{bounds:?}, scale={scale}, {root:?}/{scope:?}/{direction:?} at {aim:?}"
            );
            assert_eq!(expected.bounds(), *hit);
            commit_deferred(frame);
            let winner = session
                .host_pointer_hit(SURFACE, aim)
                .expect("the output's guide centers must have unambiguous input")
                .hover()
                .expect("the aimed guide must retain its hover receiver")
                .descriptor();
            assert_eq!(winner.visual_id(), expected.visual_id());
            assert_eq!(winner.bounds(), *hit);
        }
    }
}

fn commit_deferred(mut frame: DockspaceHostFrame<'_>) {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn review_aiming_at_the_visible_inner_left_guide_selects_the_inner_left_operation() {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::split(
                DockspaceAxis::Horizontal,
                [
                    (DockspaceNode::tabs([FIRST, SECOND]), 0.2),
                    (DockspaceNode::central_tabs([THIRD, FOURTH]), 0.8),
                ],
            )
            .unwrap(),
        ),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 1321.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();

    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let source = plan.tab_receiver(THIRD).unwrap();
    let mut inner = None;
    let receipt = plan
        .execute_surface_program(|step| {
            if let Step::DropGuide { structural, .. } = step
                && structural.scope() == Scope::Inner
                && structural.activation_bounds().width() < 300.0
            {
                inner = structural
                    .targets()
                    .find(|target| target.direction() == Direction::Left)
                    .map(|target| (target.visual_id(), target.draw_bounds()));
            }
            Ok::<(), ()>(())
        })
        .unwrap();
    let (inner_visual, inner) = inner.expect("the narrow left pane must offer an inner-left guide");
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    session
        .report_surface_presentation(outputs.pop().unwrap(), SurfacePresentationResult::Presented)
        .unwrap();
    commit_deferred(session.begin_host_frame().unwrap());

    let presented = session.presented_surface(SURFACE).unwrap();
    let receiver = session.bind_presented_receiver(&source).unwrap();
    let press = HostPointerRoute::new(presented, source.center());
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            Event::ButtonPressed(Button::Primary),
            press,
            press,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver).with_clear_hover_lane(&presented),
        ))
        .unwrap();
    commit_deferred(frame);

    let presented = session.presented_surface(SURFACE).unwrap();
    let route = HostPointerRoute::new(presented, center(inner));
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(HostPointerInput::new(
            SurfacePointerId::new(1),
            Event::Moved,
            route,
            route,
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&presented),
        ))
        .unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut active = Vec::new();
    let mut visible_inner = false;
    plan.visit_surface_program(|step| {
        if let Step::DropGuide {
            affordance: Some(cluster),
            ..
        } = step
        {
            for target in cluster
                .targets()
                .filter(|target| target.direction() == Direction::Left)
            {
                if cluster.scope() == Scope::Inner
                    && target.visual_id() == inner_visual
                    && target.draw_bounds() == inner
                {
                    visible_inner = true;
                }
                if target.is_active() {
                    active.push((cluster.scope(), target.visual_id()));
                }
            }
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    assert!(
        visible_inner,
        "the inner-left guide must actually be emitted at the aimed button"
    );
    assert_eq!(
        active,
        [(Scope::Inner, inner_visual)],
        "aiming at the visible inner-left guide must select its own operation"
    );
}
