use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::model::{
    DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout,
};
use crate::policy::{
    DockItemRule, DockPolicy, DockSourceRule, DockSurfaceRule, DockTargetRule, DockTargetRuleKey,
    TabBarInteraction, TabBarPolicy, TabBarVisibility,
};
use crate::runtime::{
    ContainedResizeDirection, DockspaceDragDecoration, DockspaceDragPreview,
    DockspaceDragSourceKind, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    DockspacePaneFocusObservation, DockspacePresentationCommandKind,
    DockspacePresentationCommandUnavailable, DockspacePresentationMenuAnchorKind,
    DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceRuntimeErrorKind,
    DockspaceSession, DockspaceVisualId, DockspaceVisualKind, HostCloseRequestOrigin,
    HostFrameReport, HostInputOutcome, HostPointerInput, HostPointerRoute, PreparedSurfaceAction,
    SurfaceContainedResizeAdjustment, SurfaceMeasurementAnswer, SurfaceMeasurementRequest,
    SurfacePaintDisposition, SurfacePaintPlan, SurfacePaintProgramStep, SurfacePointerButton,
    SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerInput, SurfacePointerPosition, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceSplitterAdjustment, SurfaceTabNavigation,
    SurfaceUnavailableReason, TabGroupDragRegionKind, TabListMenuMetrics, TabStripControlKind,
    TabStripControlMetric, TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
    UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const TARGET_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const FLOATING_ROOT: RootId = RootId::new(2);
const TARGET_ROOT: RootId = RootId::new(3);
const FRONT_FLOATING_ROOT: RootId = RootId::new(4);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const FRONT_FLOATING: FloatingPresentationId = FloatingPresentationId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const FIFTH: ItemId = ItemId::new(5);
const SIXTH: ItemId = ItemId::new(6);
const SEVENTH: ItemId = ItemId::new(7);
const EIGHTH: ItemId = ItemId::new(8);

fn program_drag_decoration<'frame>(
    plan: SurfacePaintPlan<'frame>,
) -> Option<DockspaceDragDecoration<'frame>> {
    let mut decoration = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::DragDecoration(current) = step {
            assert!(decoration.replace(current).is_none());
        }
        Ok::<(), ()>(())
    })
    .expect("paint plan should expose one valid surface program");
    decoration
}

fn program_drag_preview<'frame>(
    plan: SurfacePaintPlan<'frame>,
) -> Option<DockspaceDragPreview<'frame>> {
    let mut preview = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::DragPreview(current) = step {
            assert!(preview.replace(current).is_none());
        }
        Ok::<(), ()>(())
    })
    .expect("paint plan should expose one valid surface program");
    preview
}

fn program_paint_disposition(
    plan: SurfacePaintPlan<'_>,
    visual: DockspaceVisualId,
) -> Option<SurfacePaintDisposition> {
    let mut disposition = None;
    plan.visit_surface_program(|step| {
        let current = match step {
            SurfacePaintProgramStep::RootBegin { root, paint }
                if root
                    .contained()
                    .is_some_and(|record| record.visual_id() == visual) =>
            {
                Some(paint)
            }
            SurfacePaintProgramStep::Operation {
                operation, paint, ..
            } if operation.visual_id() == visual => Some(paint),
            SurfacePaintProgramStep::PopupMenu { menu, paint } if menu.visual_id() == visual => {
                Some(paint)
            }
            _ => None,
        };
        if let Some(current) = current {
            assert!(
                disposition.is_none_or(|existing| existing == current),
                "one visual must not receive conflicting paint dispositions"
            );
            disposition = Some(current);
        }
        Ok::<(), ()>(())
    })
    .expect("paint plan should expose one valid surface program");
    disposition
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("surface-action test layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("surface-action test session initializes")
}

fn paint_only_tab_bar_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .expect("paint-only tab-bar layout validates");
    let mut policy = DockPolicy::default();
    let mut target = DockTargetRule::default();
    target.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    policy.set_target_rule(DockTargetRuleKey::Item(FIRST), target);
    DockspaceSession::from_layout(layout, policy).expect("paint-only tab-bar session initializes")
}

fn multiview_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            TARGET_SURFACE,
            DockspaceRootLayout::new(TARGET_ROOT, DockspaceNode::central_tabs([THIRD])),
        ),
    ])
    .expect("multiview drag-decoration layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("multiview drag-decoration session initializes")
}

fn split_session() -> DockspaceSession {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::central_tabs([FIRST]),
            DockspaceNode::tabs([SECOND]),
        ],
    )
    .expect("surface-action split validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .expect("surface-action split layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("surface-action split session initializes")
}

fn splitter_junction_session() -> DockspaceSession {
    splitter_junction_session_with_policy(DockPolicy::default())
}

fn splitter_junction_session_with_policy(policy: DockPolicy) -> DockspaceSession {
    let column = |top, bottom| {
        DockspaceNode::equal_split(
            DockspaceAxis::Vertical,
            [DockspaceNode::tabs([top]), DockspaceNode::tabs([bottom])],
        )
        .expect("surface-action junction column validates")
    };
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [column(FIRST, SECOND), column(THIRD, FOURTH)],
    )
    .expect("surface-action junction validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .expect("surface-action junction layout validates");
    DockspaceSession::from_layout(layout, policy)
        .expect("surface-action junction session initializes")
}

fn overflow_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            ROOT,
            DockspaceNode::central_tabs([FIRST, SECOND, THIRD, FOURTH]),
        ),
    )])
    .expect("overflow surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("overflow surface-action session initializes")
}

fn multiview_overflow_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                ROOT,
                DockspaceNode::central_tabs([FIRST, SECOND, THIRD, FOURTH]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            TARGET_SURFACE,
            DockspaceRootLayout::new(
                TARGET_ROOT,
                DockspaceNode::central_tabs([FIFTH, SIXTH, SEVENTH, EIGHTH]),
            ),
        ),
    ])
    .expect("multiview overflow surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("multiview overflow surface-action session initializes")
}

fn contained_session(policy: DockPolicy) -> DockspaceSession {
    let contained_rect =
        LogicalRect::new(160.0, 120.0, 320.0, 240.0).expect("contained rectangle validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        contained_rect,
    ))])
    .expect("contained surface-action layout validates");
    DockspaceSession::from_layout(layout, policy)
        .expect("contained surface-action session initializes")
}

fn stacked_contained_session() -> DockspaceSession {
    let rear =
        LogicalRect::new(120.0, 100.0, 280.0, 220.0).expect("rear contained rectangle validates");
    let front =
        LogicalRect::new(440.0, 280.0, 280.0, 220.0).expect("front contained rectangle validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("stacked contained surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("stacked contained surface-action session initializes")
}

fn fully_occluded_stacked_contained_session() -> DockspaceSession {
    let rear =
        LogicalRect::new(220.0, 180.0, 240.0, 180.0).expect("rear contained rectangle validates");
    let front =
        LogicalRect::new(180.0, 140.0, 320.0, 260.0).expect("front contained rectangle validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("fully occluded contained surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("fully occluded contained surface-action session initializes")
}

fn partially_occluded_stacked_contained_session() -> DockspaceSession {
    let rear =
        LogicalRect::new(200.0, 160.0, 300.0, 220.0).expect("rear contained rectangle validates");
    let front =
        LogicalRect::new(360.0, 120.0, 250.0, 300.0).expect("front contained rectangle validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, THIRD])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, DockspaceNode::tabs([SECOND])),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("partially occluded contained surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("partially occluded contained surface-action session initializes")
}

fn stacked_split_contained_session() -> DockspaceSession {
    let rear =
        LogicalRect::new(120.0, 100.0, 280.0, 220.0).expect("rear contained rectangle validates");
    let front =
        LogicalRect::new(440.0, 280.0, 280.0, 220.0).expect("front contained rectangle validates");
    let rear_content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([SECOND]), DockspaceNode::tabs([THIRD])],
    )
    .expect("rear contained split validates");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(FLOATING_ROOT, rear_content),
        rear,
    ))
    .with_contained(DockspaceContainedLayout::new(
        FRONT_FLOATING,
        DockspaceRootLayout::new(FRONT_FLOATING_ROOT, DockspaceNode::tabs([FOURTH])),
        front,
    ))])
    .expect("stacked split-contained surface-action layout validates");
    DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("stacked split-contained surface-action session initializes")
}

fn contained_roster(session: &DockspaceSession) -> Vec<FloatingPresentationId> {
    session
        .view()
        .surface(SURFACE)
        .expect("the test surface remains present")
        .contained()
        .map(|contained| contained.id())
        .collect()
}

fn split_weights(session: &DockspaceSession) -> Vec<f32> {
    session
        .view()
        .surface(SURFACE)
        .and_then(|surface| surface.main_root())
        .and_then(|root| root.content())
        .and_then(|content| content.split())
        .expect("the test main root remains split")
        .weights()
        .collect()
}

fn splitter_junction_weights(session: &DockspaceSession) -> (Vec<f32>, Vec<Vec<f32>>) {
    let root = session
        .view()
        .surface(SURFACE)
        .and_then(|surface| surface.main_root())
        .and_then(|root| root.content())
        .and_then(|content| content.split())
        .expect("the test junction root remains split");
    let root_weights = root.weights().collect();
    let column_weights = root
        .children()
        .map(|child| {
            child
                .split()
                .expect("each junction column remains split")
                .weights()
                .collect()
        })
        .collect();
    (root_weights, column_weights)
}

fn contained_rect(session: &DockspaceSession) -> LogicalRect {
    session
        .view()
        .contained(FLOATING)
        .expect("the test contained presentation remains open")
        .rect()
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("test bounds validate"),
        LogicalSize::new(64.0, 48.0).expect("test minimum validates"),
        96.0,
    )
    .expect("test metrics validate")
}

fn install_ready_candidate(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("surface measurement succeeds");
    frame.commit().expect("ready candidate commits");
}

fn inspect_surface<R>(
    session: &mut DockspaceSession,
    inspect: impl FnOnce(SurfacePaintPlan<'_>) -> R,
) -> R {
    let mut frame = session.begin_host_frame().expect("inspection frame begins");
    let result = inspect(
        frame
            .paint_plan(SURFACE)
            .expect("inspection plan lookup succeeds")
            .expect("the current candidate is paintable"),
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("inspection retains the surface roster");
    frame.commit().expect("inspection finishes before input");
    result
}

fn paint_pointer_surface(session: &mut DockspaceSession) -> crate::runtime::PaintedSurfaceOutput {
    let mut frame = session.begin_host_frame().expect("output frame begins");
    let receipt = frame
        .paint_plan(SURFACE)
        .expect("output plan lookup succeeds")
        .expect("the current candidate is paintable")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("the recording host accepts the complete surface program");
    frame
        .confirm_surface_painted(receipt)
        .expect("the exact program execution is confirmed");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("output retains other surfaces");
    let mut report = frame.commit().expect("the exact output commits");
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    outputs.pop().expect("one complete output was emitted")
}

fn present_pointer_surface(session: &mut DockspaceSession) {
    let output = paint_pointer_surface(session);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the recording host presents its exact output");
    let mut settle = session
        .begin_host_frame()
        .expect("output settlement begins");
    settle
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("output settlement retains current candidates");
    let report = settle.commit().expect("core accepts the presentation");
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
}

fn submit_host_edges(
    session: &mut DockspaceSession,
    edges: &[(SurfacePointerEvent, Option<LogicalPoint>)],
) -> HostFrameReport {
    session
        .enable_host_pointer()
        .expect("one host producer owns the ordered input");
    let hits = edges
        .iter()
        .map(|(_, point)| {
            point
                .and_then(|point| session.host_pointer_hit(SURFACE, point))
                .map(|hit| {
                    (
                        hit.surface(),
                        hit.delivery(),
                        hit.drag_delivery(),
                        hit.hover(),
                    )
                })
        })
        .collect::<Vec<_>>();
    // This recording host has no framework overlays; core resolves every lane.
    let inputs = edges.iter().zip(&hits).map(|((event, point), hit)| {
        let mut facts = SurfacePointerReceiverFacts::unknown();
        let route = if let (Some((surface, click, drag, hover)), Some(point)) = (hit, point) {
            facts = click.as_ref().map_or_else(
                || facts.with_no_delivery(surface),
                |receiver| facts.with_delivery(receiver),
            );
            facts = drag.as_ref().map_or_else(
                || facts.with_no_drag_delivery(surface),
                |receiver| facts.with_drag_delivery(receiver),
            );
            facts = hover.as_ref().map_or_else(
                || facts.with_no_hover(surface),
                |receiver| facts.with_hover(receiver),
            );
            HostPointerRoute::new(*surface, *point)
        } else {
            HostPointerRoute::Unknown
        };
        HostPointerInput::new(
            SurfacePointerId::new(1),
            *event,
            route,
            route,
            SurfacePointerCapture::None,
            facts,
        )
    });
    let mut frame = session.begin_host_frame().expect("host input frame begins");
    frame
        .submit_host_pointer_batch(inputs)
        .expect("the complete ordered host batch is accepted");
    if frame
        .paint_plan(SURFACE)
        .expect("post-input readiness lookup succeeds")
        .is_none()
    {
        // Raise invalidates measurements; an existing Ready preview must remain untouched.
        frame
            .measure_surface(SURFACE, metrics())
            .expect("the stale post-input surface receives current measurements");
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("input retains current surface geometry");
    frame.commit().expect("host input commits atomically")
}

fn present_multiview_source_for_pointer(
    session: &mut DockspaceSession,
) -> (
    DockspaceReceiverDescriptor,
    DockspaceReceiverDescriptor,
    DockspaceVisualId,
    DockspaceVisualId,
    LogicalPoint,
    LogicalPoint,
) {
    let mut measure = session
        .begin_host_frame()
        .expect("multiview measurement frame begins");
    measure
        .measure_surface(SURFACE, metrics())
        .expect("source surface measurement succeeds");
    measure
        .measure_surface(TARGET_SURFACE, metrics())
        .expect("target surface measurement succeeds");
    measure.commit().expect("multiview candidate commits");

    let mut paint = session.begin_host_frame().expect("paint frame begins");
    let (descriptor, hover_descriptor, source_visual, press, moved) = {
        let plan = paint
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .expect("ready candidate is paintable");
        let tab = plan
            .tabs()
            .find(|tab| tab.item() == FIRST)
            .expect("the first tab is visible");
        let descriptor = plan
            .receiver_for_tab_body(tab)
            .expect("the first tab has an exact body receiver");
        let bounds = descriptor.bounds();
        let press = LogicalPoint::new(
            bounds.x() + bounds.width() / 2.0,
            bounds.y() + bounds.height() / 2.0,
        )
        .expect("the tab-center point validates");
        let hover_descriptor = plan
            .receivers()
            .filter(|receiver| receiver.role() == DockspaceReceiverRole::DropTarget)
            .find(|receiver| {
                let bounds = receiver.bounds();
                (bounds.x() + bounds.width() / 2.0 - press.x()).abs() >= 12.0
                    || (bounds.y() + bounds.height() / 2.0 - press.y()).abs() >= 12.0
            })
            .expect("the source surface exposes a threshold-crossing drop target");
        let hover_bounds = hover_descriptor.bounds();
        let moved = LogicalPoint::new(
            hover_bounds.x() + hover_bounds.width() / 2.0,
            hover_bounds.y() + hover_bounds.height() / 2.0,
        )
        .expect("the exact hover-target center validates");
        (descriptor, hover_descriptor, tab.visual_id(), press, moved)
    };
    let target_visual = paint
        .paint_plan(TARGET_SURFACE)
        .expect("target paint plan lookup succeeds")
        .expect("target candidate is paintable")
        .tabs()
        .find(|tab| tab.item() == THIRD)
        .expect("the target tab is visible")
        .visual_id();
    let receipt = paint
        .paint_plan(SURFACE)
        .expect("source paint plan lookup succeeds")
        .expect("source candidate remains paintable")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .expect("the complete source surface program executes");
    paint
        .confirm_surface_painted(receipt)
        .expect("the exact surface output was painted");
    paint
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unpainted target retains its ready candidate");
    let mut report = paint.commit().expect("paint frame commits");
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1, "one exact surface output was painted");
    let output = outputs.pop().expect("one output was asserted");
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the exact output reaches final presentation");

    let mut settle = session
        .begin_host_frame()
        .expect("presentation settlement frame begins");
    settle
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the settlement frame retains the ready surface");
    settle.commit().expect("presentation authority settles");

    (
        descriptor,
        hover_descriptor,
        source_visual,
        target_visual,
        press,
        moved,
    )
}

fn overflow_measurement(request: SurfaceMeasurementRequest) -> SurfaceMeasurementAnswer {
    match request {
        SurfaceMeasurementRequest::DockBounds { .. }
        | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => SurfaceMeasurementAnswer::Bounds(
            LogicalRect::new(0.0, 0.0, 260.0, 180.0).expect("overflow bounds validate"),
        ),
        SurfaceMeasurementRequest::PaneMinimum { .. } => SurfaceMeasurementAnswer::PaneMinimum(
            LogicalSize::new(64.0, 48.0).expect("overflow pane minimum validates"),
        ),
        SurfaceMeasurementRequest::TabIntrinsic { .. } => {
            SurfaceMeasurementAnswer::TabIntrinsic(88.0)
        }
        SurfaceMeasurementRequest::TabStrip { .. } => {
            let controls = TabStripControlMetrics::new(4.0)
                .expect("overflow control spacing validates")
                .with_scroll_backward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                        .expect("backward control validates"),
                )
                .with_scroll_forward(
                    TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                        .expect("forward control validates"),
                )
                .with_tab_list_menu(
                    TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                        .expect("menu control validates"),
                );
            let menu = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                .expect("menu metrics validate");
            SurfaceMeasurementAnswer::TabStrip(
                TabStripMetrics::new(0.0, 0.0)
                    .expect("overflow strip metrics validate")
                    .with_controls(controls)
                    .with_tab_list_menu(menu),
            )
        }
    }
}

fn measure_overflow_surface(frame: &mut crate::runtime::DockspaceHostFrame<'_>) {
    measure_overflow_surface_on(frame, SURFACE);
}

fn measure_overflow_surface_on(
    frame: &mut crate::runtime::DockspaceHostFrame<'_>,
    surface: SurfaceId,
) {
    frame
        .measure_surface_with(surface, overflow_measurement)
        .expect("overflow surface measurement succeeds");
}

fn install_overflow_candidate(session: &mut DockspaceSession) {
    let mut frame = session
        .begin_host_frame()
        .expect("overflow measurement frame begins");
    measure_overflow_surface(&mut frame);
    frame.commit().expect("overflow candidate commits");
}

fn prepare_tab_select(session: &mut DockspaceSession, item: ItemId) -> PreparedSurfaceAction {
    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let action = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("ready candidate is paintable")
        .prepare_tab_select(item)
        .expect("tab exists in the exact candidate");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    frame.commit().expect("paint pass commits");
    action
}

fn prepare_tab_close(session: &mut DockspaceSession, item: ItemId) -> PreparedSurfaceAction {
    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let action = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("ready candidate is paintable")
        .prepare_tab_close(item)
        .expect("tab close control exists in the exact candidate");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    frame.commit().expect("paint pass commits");
    action
}

#[test]
fn external_journal_drag_exposes_core_drag_decoration_without_local_response() {
    let mut session = multiview_session();
    let (descriptor, hover_descriptor, source_visual, target_visual, press, moved) =
        present_multiview_source_for_pointer(&mut session);
    let receiver = session
        .bind_presented_receiver(&descriptor)
        .expect("the descriptor binds to the exact presented output");
    let hover_receiver = session
        .bind_presented_receiver(&hover_descriptor)
        .expect("the hover descriptor binds to the exact presented output");
    session
        .enable_surface_pointer(SURFACE)
        .expect("the presented surface enables its pointer producer");

    let pointer = SurfacePointerId::new(1);
    let mut drag = session.begin_host_frame().expect("drag frame begins");
    drag.submit_surface_pointer_batch([
        SurfacePointerInput::new(
            pointer,
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            SurfacePointerPosition::Known(press),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::delivery(&receiver),
        ),
        SurfacePointerInput::new(
            pointer,
            SurfacePointerEvent::Moved,
            SurfacePointerPosition::Known(moved),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::hover(&hover_receiver),
        ),
    ])
    .expect("the lossless journal crosses the core drag threshold");
    {
        let plan = drag
            .paint_plan(SURFACE)
            .expect("source drag paint plan lookup succeeds")
            .expect("the presented source candidate remains paintable");
        let decoration = program_drag_decoration(plan)
            .expect("journal reduction owns the active source decoration");
        assert_eq!(decoration.source_surface(), SURFACE);
        assert_eq!(decoration.kind(), DockspaceDragSourceKind::Item);
        assert_eq!(decoration.item(), Some(FIRST));
        assert_eq!(decoration.source_visual(), source_visual);
        assert_eq!(
            program_paint_disposition(plan, source_visual),
            Some(SurfacePaintDisposition::SourceGap),
            "the exact source operation owns the source-gap decision"
        );
        let debug = format!("{decoration:?}");
        assert!(!debug.contains("NodeId"));
        assert!(!debug.contains("fingerprint"));
    }
    {
        let plan = drag
            .paint_plan(TARGET_SURFACE)
            .expect("target drag paint plan lookup succeeds")
            .expect("the target candidate remains paintable");
        let decoration = program_drag_decoration(plan)
            .expect("the same core decoration reaches the second ready surface");
        assert_eq!(decoration.source_surface(), SURFACE);
        assert_eq!(decoration.kind(), DockspaceDragSourceKind::Item);
        assert_eq!(decoration.item(), Some(FIRST));
        assert_eq!(decoration.source_visual(), source_visual);
        assert_eq!(
            program_paint_disposition(plan, target_visual),
            Some(SurfacePaintDisposition::Visible),
            "the target surface keeps its own tab operation visible"
        );
    }
    drag.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    drag.commit().expect("the drag frame commits");

    let mut cancel = session
        .begin_host_frame()
        .expect("cancellation frame begins");
    cancel
        .submit_surface_pointer(SurfacePointerInput::new(
            pointer,
            SurfacePointerEvent::StreamCancelled(
                SurfacePointerCancelReason::ExplicitPlatformCancellation,
            ),
            SurfacePointerPosition::Known(moved),
            SurfacePointerCapture::None,
            SurfacePointerReceiverFacts::unknown(),
        ))
        .expect("the provider explicitly cancels the active stream");
    for surface in [SURFACE, TARGET_SURFACE] {
        let plan = cancel
            .paint_plan(surface)
            .expect("cancelled paint plan lookup succeeds")
            .expect("the retained candidate remains paintable");
        assert!(
            program_drag_decoration(plan).is_none(),
            "the core decoration disappears from every cancellation record"
        );
    }
    cancel
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the cancelled candidate is retained");
    cancel.commit().expect("the cancellation frame commits");

    let mut restart = session.begin_host_frame().expect("restart frame begins");
    restart
        .submit_surface_pointer_batch([
            SurfacePointerInput::new(
                pointer,
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                SurfacePointerPosition::Known(press),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&receiver),
            ),
            SurfacePointerInput::new(
                pointer,
                SurfacePointerEvent::Moved,
                SurfacePointerPosition::Known(moved),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::hover(&hover_receiver),
            ),
        ])
        .expect("a fresh journal stream starts after explicit cancellation");
    let restarted = restart
        .paint_plan(SURFACE)
        .expect("restarted paint plan lookup succeeds")
        .expect("the retained source remains paintable");
    assert!(
        program_drag_decoration(restarted).is_some(),
        "cancellation leaves no core or adapter owner blocking the next press"
    );
    restart
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the restarted candidate is retained");
    restart.commit().expect("the restart frame commits");
}

#[test]
fn paint_plan_exposes_distinct_group_drag_region_and_receiver_identities() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let paint = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("ready candidate is paintable");
    let bar = paint
        .tab_bars()
        .next()
        .expect("the surface has one tab bar");
    let regions = bar.group_drag_regions().collect::<Vec<_>>();

    assert_eq!(regions.len(), 2);
    assert_eq!(regions[0].kind(), TabGroupDragRegionKind::LeadingGrip);
    assert_eq!(regions[1].kind(), TabGroupDragRegionKind::TrailingEmpty);
    assert_ne!(regions[0].visual_id(), regions[1].visual_id());

    let receivers = regions
        .iter()
        .copied()
        .map(|region| {
            paint
                .receiver_for_tab_group_drag_region(region)
                .expect("each exact group region owns a receiver")
        })
        .collect::<Vec<_>>();
    assert_eq!(receivers[0].role(), DockspaceReceiverRole::TabGroupGrip);
    assert_eq!(
        receivers[1].role(),
        DockspaceReceiverRole::TabGroupTrailingEmpty
    );
    assert_eq!(receivers[0].bounds(), regions[0].bounds());
    assert_eq!(receivers[1].bounds(), regions[1].bounds());
    assert_ne!(receivers[0].visual_id(), receivers[1].visual_id());

    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    frame.commit().expect("paint frame commits");
}

#[test]
fn contained_resize_operability_honors_exact_root_item_and_surface_policy() {
    let mut policies = Vec::new();

    let mut global = DockPolicy::default();
    global.set_allow_contained_transform(false);
    policies.push(("global", global));

    let mut root = DockPolicy::default();
    let mut root_rule = DockSourceRule::new();
    root_rule.set_enabled(false);
    root.set_source_rule(FLOATING_ROOT, root_rule);
    policies.push(("root", root));

    let mut item = DockPolicy::default();
    let mut item_rule = DockItemRule::new();
    item_rule.set_source_enabled(false);
    item.set_item_rule(SECOND, item_rule);
    policies.push(("item", item));

    let mut surface = DockPolicy::default();
    let mut surface_rule = DockSurfaceRule::new();
    surface_rule.set_target_enabled(false);
    surface.set_surface_rule(SURFACE, surface_rule);
    policies.push(("surface", surface));

    for (scope, policy) in policies {
        let mut session = contained_session(policy);
        install_ready_candidate(&mut session);
        let mut frame = session.begin_host_frame().expect("paint frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .expect("contained candidate is paintable");
        let contained = plan
            .contained()
            .next()
            .expect("the exact contained presentation is painted");
        let cardinal = contained
            .resize()
            .filter(|resize| {
                matches!(
                    resize.direction(),
                    ContainedResizeDirection::North
                        | ContainedResizeDirection::East
                        | ContainedResizeDirection::South
                        | ContainedResizeDirection::West
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(cardinal.len(), 4);
        assert!(
            cardinal.iter().all(|resize| !resize.operable()),
            "{scope} policy denial must be frozen into every cardinal resize record",
        );
        assert!(
            cardinal.iter().all(|resize| plan
                .receiver_for_contained_resize(contained, *resize)
                .is_none()),
            "{scope} policy denial must remove every cardinal resize receiver",
        );
        assert!(
            cardinal.iter().all(|resize| {
                plan.prepare_contained_resize_adjustment(
                    contained.floating(),
                    resize.direction(),
                    SurfaceContainedResizeAdjustment::Increment,
                )
                .is_none()
            }),
            "{scope} policy denial must reject every cardinal resize action",
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the unchanged candidate is retained");
        frame.commit().expect("paint pass commits");
    }
}

#[test]
fn contained_resize_adjustment_uses_current_ready_geometry_for_every_cardinal_edge() {
    let source = LogicalRect::new(160.0, 120.0, 320.0, 240.0).expect("source rectangle validates");
    let cases = [
        (
            ContainedResizeDirection::North,
            SurfaceContainedResizeAdjustment::Decrement,
            LogicalRect::new(160.0, 104.0, 320.0, 256.0).expect("north rectangle validates"),
        ),
        (
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Increment,
            LogicalRect::new(160.0, 120.0, 336.0, 240.0).expect("east rectangle validates"),
        ),
        (
            ContainedResizeDirection::South,
            SurfaceContainedResizeAdjustment::Increment,
            LogicalRect::new(160.0, 120.0, 320.0, 256.0).expect("south rectangle validates"),
        ),
        (
            ContainedResizeDirection::West,
            SurfaceContainedResizeAdjustment::Decrement,
            LogicalRect::new(144.0, 120.0, 336.0, 240.0).expect("west rectangle validates"),
        ),
    ];

    for (direction, adjustment, expected) in cases {
        let mut session = contained_session(DockPolicy::default());
        install_ready_candidate(&mut session);
        assert_eq!(contained_rect(&session), source);

        let mut frame = session.begin_host_frame().expect("adjustment frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .expect("contained candidate is paintable");
        let contained = plan
            .contained()
            .next()
            .expect("the contained presentation is painted");
        let resize = contained
            .resize()
            .find(|resize| resize.direction() == direction)
            .expect("the requested cardinal edge is painted");
        assert!(resize.operable());
        let action = plan
            .prepare_contained_resize_adjustment(
                contained.floating(),
                resize.direction(),
                adjustment,
            )
            .expect("the current Ready edge prepares an adjustment");
        frame
            .submit_surface_action(action)
            .expect("the same-frame action is accepted");
        frame
            .measure_surface(SURFACE, metrics())
            .expect("the adjusted surface measures");
        frame.commit().expect("the adjustment frame commits");

        assert_eq!(contained_rect(&session), expected, "{direction:?}");
    }
}

#[test]
fn stale_contained_resize_adjustment_is_inert() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);
    let before = contained_rect(&session);

    let mut prepare = session.begin_host_frame().expect("prepare frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("contained candidate is paintable");
    let contained = plan
        .contained()
        .next()
        .expect("the contained presentation is painted");
    let action = plan
        .prepare_contained_resize_adjustment(
            contained.floating(),
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Increment,
        )
        .expect("the current edge prepares an adjustment");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the candidate is retained");
    prepare.commit().expect("prepare frame commits");

    let mut mutate = session.begin_host_frame().expect("mutation frame begins");
    mutate
        .select_item_current(THIRD)
        .expect("the main-root selection changes the revision");
    mutate
        .measure_surface(SURFACE, metrics())
        .expect("the mutated surface measures");
    mutate.commit().expect("mutation frame commits");

    let mut stale = session.begin_host_frame().expect("stale frame begins");
    stale
        .submit_surface_action(action)
        .expect("same-session stale action is structurally accepted");
    stale
        .measure_surface(SURFACE, metrics())
        .expect("the stale surface measures");
    let report = stale.commit().expect("stale frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert_eq!(contained_rect(&session), before);
}

#[test]
fn exact_surface_tab_action_selects_without_exposing_scene_identity() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let action = prepare_tab_select(&mut session, SECOND);
    assert_eq!(action.surface(), SURFACE);
    assert_eq!(action.expected_version(), session.version());

    let mut frame = session.begin_host_frame().expect("action frame begins");
    frame
        .submit_surface_action(action)
        .expect("same-session action is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("post-action surface measurement succeeds");
    frame.commit().expect("selection frame commits");

    assert!(
        session
            .view()
            .item(SECOND)
            .expect("second item remains open")
            .is_selected()
    );
}

#[test]
fn paint_only_tabs_expose_no_receiver_or_prepared_semantic_action() {
    let mut session = paint_only_tab_bar_session();
    install_ready_candidate(&mut session);

    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("paint-only candidate remains paintable");
    let tabs = plan.tabs().collect::<Vec<_>>();
    assert_eq!(tabs.len(), 2);
    assert!(tabs.iter().all(|tab| !tab.operable()));
    assert!(
        tabs.iter()
            .all(|tab| plan.receiver_for_tab_body(*tab).is_none())
    );
    assert!(plan.prepare_tab_select(SECOND).is_none());
    assert!(
        plan.prepare_tab_navigation(FIRST, SurfaceTabNavigation::Next)
            .is_none()
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the paint-only candidate is retained");
    frame.commit().expect("paint-only inspection frame commits");
}

#[test]
fn host_tab_drag_into_its_source_pane_center_publishes_a_contained_preview() {
    let mut session = session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    let (source, target) = inspect_surface(&mut session, |plan| {
        let tab = plan
            .tabs()
            .find(|tab| tab.item() == FIRST)
            .expect("the source tab is visible");
        let source = plan
            .receiver_for_tab_body(tab)
            .expect("the tab has an exact receiver")
            .center();
        let pane = plan
            .panes()
            .find(|pane| pane.root() == tab.root())
            .expect("the source pane is visible");
        let bounds = pane.content_bounds();
        let target = LogicalPoint::new(
            bounds.x() + bounds.width() * 0.65,
            bounds.y() + bounds.height() * 0.65,
        )
        .expect("the pane center validates");
        (source, target)
    });
    submit_host_edges(
        &mut session,
        &[
            (
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                Some(source),
            ),
            (SurfacePointerEvent::Moved, Some(target)),
        ],
    );
    assert!(session.has_active_gesture());
    inspect_surface(&mut session, |plan| {
        let preview =
            program_drag_preview(plan).expect("the source pane publishes a contained preview");
        assert!(matches!(
            preview.visual(),
            crate::runtime::DockspacePreviewVisual::Contained {
                surface: SURFACE,
                fallback: false,
                ..
            }
        ));
    });
}

#[test]
fn host_tab_drag_with_unknown_release_cancels_without_late_delivery() {
    let mut session = session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    let original_version = session.version();
    let source = inspect_surface(&mut session, |plan| {
        plan.tab_receiver(FIRST)
            .expect("the source tab has an exact receiver")
            .center()
    });
    let threshold =
        LogicalPoint::new(source.x() + 16.0, source.y() + 8.0).expect("threshold point validates");
    submit_host_edges(
        &mut session,
        &[
            (
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                Some(source),
            ),
            (SurfacePointerEvent::Moved, Some(threshold)),
        ],
    );
    assert!(session.has_active_gesture());
    // Leaving the qualified host route does not prove desktop outside-all authority.
    submit_host_edges(&mut session, &[(SurfacePointerEvent::Moved, None)]);
    inspect_surface(&mut session, |plan| {
        assert!(
            program_drag_preview(plan).is_none(),
            "an unknown route cannot authorize contained placement"
        );
    });
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            None,
        )],
    );
    assert_eq!(session.version(), original_version);
    assert!(!session.has_active_gesture());
    present_pointer_surface(&mut session);
    inspect_surface(&mut session, |plan| {
        assert!(program_drag_preview(plan).is_none());
        assert!(program_drag_decoration(plan).is_none());
    });
    assert_eq!(
        session.version(),
        original_version,
        "new presentation cannot revive an unknown-authority release"
    );
    let item = session.view().item(FIRST).expect("the source remains open");
    assert_eq!(item.root(), ROOT);
    assert_eq!(item.contained(), None);
    assert_eq!(
        session
            .view()
            .surface(SURFACE)
            .expect("the source surface remains present")
            .contained()
            .count(),
        0
    );
}

#[test]
fn surface_focus_request_stays_pending_until_an_exact_focused_observation() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let select = prepare_tab_select(&mut session, SECOND);

    let mut select_frame = session.begin_host_frame().expect("selection frame begins");
    select_frame
        .submit_surface_action(select)
        .expect("the exact tab selection is accepted");
    select_frame
        .measure_surface(SURFACE, metrics())
        .expect("the selected surface measures");
    select_frame.commit().expect("selection frame commits");

    let mut not_focused_frame = session
        .begin_host_frame()
        .expect("not-focused observation frame begins");
    let plan = not_focused_frame
        .paint_plan(SURFACE)
        .expect("focus paint plan lookup succeeds")
        .expect("the focused surface remains paintable");
    let request = plan
        .pane_focus_request()
        .expect("selection publishes an exact pane-focus request");
    assert_eq!(request.surface(), SURFACE);
    assert_eq!(request.item(), Some(SECOND));
    assert_eq!(plan.focused_pane(), None);
    let not_focused = plan
        .prepare_pane_focus_observation(request, DockspacePaneFocusObservation::NotFocused)
        .expect("the exact request accepts a not-focused observation");
    not_focused_frame
        .submit_pane_focus_observation(not_focused)
        .expect("the not-focused observation is accepted");
    not_focused_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged focus surface is retained");
    not_focused_frame
        .commit()
        .expect("not-focused observation commits");

    let mut focused_frame = session
        .begin_host_frame()
        .expect("focused observation frame begins");
    let plan = focused_frame
        .paint_plan(SURFACE)
        .expect("pending focus plan lookup succeeds")
        .expect("the pending focus surface remains paintable");
    let retained = plan
        .pane_focus_request()
        .expect("not-focused keeps the exact request pending");
    assert_eq!(retained, request);
    assert_eq!(plan.focused_pane(), None);
    let focused = plan
        .prepare_pane_focus_observation(retained, DockspacePaneFocusObservation::Focused)
        .expect("the retained request accepts a focused observation");
    focused_frame
        .submit_pane_focus_observation(focused)
        .expect("the focused observation is accepted");
    focused_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the focused surface is retained");
    focused_frame.commit().expect("focused observation commits");

    let mut settled = session
        .begin_host_frame()
        .expect("settled focus inspection frame begins");
    let plan = settled
        .paint_plan(SURFACE)
        .expect("settled focus plan lookup succeeds")
        .expect("the settled focus surface remains paintable");
    assert!(plan.pane_focus_request().is_none());
    assert_eq!(plan.focused_pane(), Some(SECOND));
    settled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the settled surface is retained");
    settled.commit().expect("settled inspection frame commits");
}

#[test]
fn rear_contained_title_press_raises_before_any_drag_preview() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);
    let point = inspect_surface(&mut session, |plan| {
        let rear = plan
            .contained()
            .find(|contained| contained.floating() == FLOATING)
            .expect("the rear presentation is painted");
        plan.receiver_for_contained_title(rear)
            .expect("the exposed rear title has a receiver")
            .center()
    });
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            Some(point),
        )],
    );
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    inspect_surface(&mut session, |plan| {
        assert!(
            program_drag_decoration(plan).is_none(),
            "a press alone must not hide the source"
        );
        assert!(
            program_drag_preview(plan).is_none(),
            "the title must cross its drag threshold first"
        );
    });
    present_pointer_surface(&mut session);
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            Some(point),
        )],
    );
    assert!(!session.has_active_gesture());
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
}

#[test]
fn fully_occluded_rear_contained_chrome_has_no_surface_action_authority() {
    let mut session = fully_occluded_stacked_contained_session();
    install_ready_candidate(&mut session);

    let mut frame = session
        .begin_host_frame()
        .expect("contained occlusion inspection frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("contained occlusion plan lookup succeeds")
        .expect("stacked contained candidate is paintable");
    let rear = plan
        .contained()
        .find(|contained| contained.floating() == FLOATING)
        .expect("rear contained presentation is painted");
    let front = plan
        .contained()
        .find(|contained| contained.floating() == FRONT_FLOATING)
        .expect("front contained presentation is painted");

    assert!(!rear.title_operable());
    assert!(!rear.close_operable());
    assert!(rear.resize().all(|resize| !resize.operable()));
    assert!(plan.receiver_for_contained_title(rear).is_none());
    assert!(plan.receiver_for_contained_close(rear).is_none());
    assert!(
        rear.resize()
            .all(|resize| plan.receiver_for_contained_resize(rear, resize).is_none())
    );
    assert!(plan.prepare_contained_close(FLOATING).is_none());
    assert!(
        plan.prepare_contained_resize_adjustment(
            FLOATING,
            ContainedResizeDirection::East,
            SurfaceContainedResizeAdjustment::Increment,
        )
        .is_none()
    );

    assert!(front.title_operable());
    assert!(front.close_operable());
    assert!(front.resize().all(|resize| resize.operable()));

    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("contained occlusion inspection retains the ready candidate");
    frame
        .commit()
        .expect("contained occlusion inspection frame commits");
}

#[test]
fn partially_exposed_rear_contained_chrome_retains_only_exposed_authority() {
    let mut session = partially_occluded_stacked_contained_session();
    install_ready_candidate(&mut session);

    let mut frame = session
        .begin_host_frame()
        .expect("partial contained occlusion inspection frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("partial contained occlusion plan lookup succeeds")
        .expect("partially stacked contained candidate is paintable");
    let rear = plan
        .contained()
        .find(|contained| contained.floating() == FLOATING)
        .expect("rear contained presentation is painted");
    let resize = rear
        .resize()
        .map(|record| (record.direction(), record.operable()))
        .collect::<Vec<_>>();

    assert!(rear.title_operable());
    assert!(!rear.close_operable());
    assert_eq!(
        resize.iter().find_map(|(direction, operable)| {
            (*direction == ContainedResizeDirection::West).then_some(*operable)
        }),
        Some(true),
    );
    assert_eq!(
        resize.iter().find_map(|(direction, operable)| {
            (*direction == ContainedResizeDirection::East).then_some(*operable)
        }),
        Some(false),
    );

    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("partial contained occlusion inspection retains the ready candidate");
    frame
        .commit()
        .expect("partial contained occlusion inspection frame commits");
}

#[test]
fn rear_contained_pane_press_raises_and_requests_the_exact_selected_pane_focus() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    let point = inspect_surface(&mut session, |plan| {
        let pane = plan
            .panes()
            .find(|pane| pane.root() == FLOATING_ROOT && pane.selected() == Some(SECOND))
            .expect("the selected rear pane is painted");
        plan.receiver_for_pane(pane)
            .expect("the pane owns an exact fallback receiver")
            .center()
    });
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            Some(point),
        )],
    );
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(!session.has_active_gesture());
    inspect_surface(&mut session, |plan| {
        assert_eq!(
            plan.pane_focus_request().and_then(|request| request.item()),
            Some(SECOND)
        );
    });
}

#[test]
fn rear_contained_title_press_raises_before_host_drag_motion() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);
    let point = inspect_surface(&mut session, |plan| {
        let rear = plan
            .contained()
            .find(|contained| contained.floating() == FLOATING)
            .expect("the rear presentation is painted");
        plan.receiver_for_contained_title(rear)
            .expect("the rear title has an exact receiver")
            .center()
    });
    let moved = LogicalPoint::new(point.x() + 18.0, point.y()).expect("title movement validates");
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            Some(point),
        )],
    );
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());
    present_pointer_surface(&mut session);
    submit_host_edges(&mut session, &[(SurfacePointerEvent::Moved, Some(moved))]);
    assert!(session.has_active_gesture());
    inspect_surface(&mut session, |plan| {
        assert!(
            program_drag_decoration(plan).is_some(),
            "the raised source remains decorated"
        );
        assert!(
            program_drag_preview(plan).is_some(),
            "the threshold-crossing preview reaches the terminal candidate"
        );
    });
}

#[test]
fn contained_tab_exits_its_source_rect_then_redocks_through_the_pane_basin() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    let mut expected_items = session
        .view()
        .item(FIRST)
        .expect("the main group exists before redocking")
        .tabs()
        .items()
        .to_vec();
    expected_items.push(SECOND);
    let (initial, target) = inspect_surface(&mut session, |plan| {
        let contained = plan
            .contained()
            .find(|contained| contained.floating() == FLOATING)
            .expect("the contained source is painted");
        let initial = plan
            .tab_receiver(SECOND)
            .expect("the contained tab has an exact receiver")
            .center();
        let pane = plan
            .panes()
            .find(|pane| pane.root() == ROOT)
            .expect("the main pane basin is painted");
        let bounds = pane.content_bounds();
        let target = LogicalPoint::new(
            bounds.x() + bounds.width() * 0.75,
            bounds.y() + bounds.height() * 0.75,
        )
        .expect("the pane basin point validates");
        assert!(
            !contained.outer_bounds().contains(target),
            "the first magnetic observation must leave the source rectangle"
        );
        assert!(
            plan.drop_guides()
                .flat_map(|cluster| cluster.targets())
                .all(|guide| !guide.hit_bounds().contains(target)),
            "the journey must not aim at an exact guide button"
        );
        (initial, target)
    });
    let version = session.version();
    let report = submit_host_edges(
        &mut session,
        &[
            (
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                Some(initial),
            ),
            (SurfacePointerEvent::Moved, Some(target)),
            (
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                Some(target),
            ),
        ],
    );
    assert!(report.workspace_changed());
    assert_ne!(session.version(), version);
    let [terminal] = report.manipulation_feedback() else {
        panic!("the Burst must deliver one redock terminal: {report:?}");
    };
    assert_eq!(
        terminal.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert!(
        session.view().contained(FLOATING).is_none(),
        "a successful Burst commits its ordinary redock before any new paint"
    );
    let moved = session
        .view()
        .item(SECOND)
        .expect("the redocked item remains open");
    assert_eq!(moved.surface(), SURFACE);
    assert_eq!(moved.root(), ROOT);
    assert_eq!(moved.contained(), None);
    assert_eq!(moved.tabs().items(), expected_items.as_slice());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    let delivered_version = session.version();
    inspect_surface(&mut session, |plan| {
        assert!(program_drag_preview(plan).is_none());
    });
    let output = paint_pointer_surface(&mut session);
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the successor redocked layout reaches presentation");
    let mut settle = session
        .begin_host_frame()
        .expect("successor output settlement begins");
    settle
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("output settlement retains the committed layout");
    let report = settle
        .commit()
        .expect("the redocked layout presentation settles");
    assert!(!report.workspace_changed());
    assert!(report.manipulation_feedback().is_empty());
    assert_eq!(session.version(), delivered_version);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact)),
        "the successor paint grants authority without repeating the accepted edit"
    );
    assert!(
        session.view().contained(FLOATING).is_none(),
        "presentation preserves the already committed redock"
    );
    let moved = session
        .view()
        .item(SECOND)
        .expect("the redocked item remains open");
    assert_eq!(moved.surface(), SURFACE);
    assert_eq!(moved.root(), ROOT);
    assert_eq!(moved.contained(), None);
    assert!(!session.has_active_gesture());
}

#[test]
fn contained_release_commits_before_presentation_and_successor_disposition_cannot_undo_it() {
    for disposition in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let mut session = contained_session(DockPolicy::default());
        install_ready_candidate(&mut session);
        present_pointer_surface(&mut session);
        let original = contained_rect(&session);
        let point = inspect_surface(&mut session, |plan| {
            let contained = plan
                .contained()
                .find(|contained| contained.floating() == FLOATING)
                .unwrap();
            let east = contained
                .resize()
                .find(|resize| resize.direction() == ContainedResizeDirection::East)
                .unwrap();
            plan.receiver_for_contained_resize(contained, east)
                .unwrap()
                .center()
        });
        let moved = LogicalPoint::new(point.x() + 18.0, point.y()).unwrap();
        let expected = LogicalRect::new(
            original.x(),
            original.y(),
            original.width() + 18.0,
            original.height(),
        )
        .unwrap();
        let version = session.version();
        let report = submit_host_edges(
            &mut session,
            &[
                (
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    Some(point),
                ),
                (SurfacePointerEvent::Moved, Some(moved)),
                (
                    SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                    Some(moved),
                ),
            ],
        );
        assert!(!session.has_active_gesture());
        assert!(!session.has_pending_presentation());
        assert!(!session.surface_requires_presentation(SURFACE));
        assert!(!session.surface_requires_presentation(TARGET_SURFACE));
        assert!(report.repaint_surfaces().contains(&SURFACE));
        assert!(report.workspace_changed());
        assert_ne!(session.version(), version);
        assert_eq!(contained_rect(&session), expected);
        let [terminal] = report.manipulation_feedback() else {
            panic!("the release must deliver one contained transform: {report:?}");
        };
        assert_eq!(
            terminal.status(),
            DockspaceManipulationStatus::Delivered(
                DockspaceManipulationDelivery::ContainedTransform
            )
        );
        let delivered_version = session.version();

        let output = paint_pointer_surface(&mut session);
        assert!(!session.has_pending_presentation());
        assert_eq!(contained_rect(&session), expected);
        session
            .report_surface_presentation(output, disposition)
            .unwrap();
        let mut settle = session.begin_host_frame().unwrap();
        settle
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = settle.commit().unwrap();
        assert!(!report.workspace_changed());
        assert!(report.manipulation_feedback().is_empty());
        assert!(!session.has_pending_presentation());
        assert!(!session.surface_requires_presentation(SURFACE));
        assert_eq!(contained_rect(&session), expected);
        assert_eq!(session.version(), delivered_version);
    }
}

#[test]
fn rear_contained_resize_press_raises_before_host_transform_motion() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);
    let original = contained_rect(&session);
    let point = inspect_surface(&mut session, |plan| {
        let rear = plan
            .contained()
            .find(|contained| contained.floating() == FLOATING)
            .expect("the rear presentation is painted");
        let east = rear
            .resize()
            .find(|resize| resize.direction() == ContainedResizeDirection::East)
            .expect("the east handle is painted");
        plan.receiver_for_contained_resize(rear, east)
            .expect("the east handle has an exact receiver")
            .center()
    });
    let moved = LogicalPoint::new(point.x() + 18.0, point.y()).expect("resize movement validates");
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            Some(point),
        )],
    );
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());
    present_pointer_surface(&mut session);
    submit_host_edges(&mut session, &[(SurfacePointerEvent::Moved, Some(moved))]);
    assert!(session.has_active_gesture());
    assert_eq!(
        contained_rect(&session),
        original,
        "resize motion stays transient until release"
    );
    inspect_surface(&mut session, |plan| {
        let mut preview = None;
        plan.visit_surface_program(|step| {
            if let SurfacePaintProgramStep::ContainedTransformPreview(current) = step {
                assert_eq!(current.floating(), FLOATING);
                assert!(preview.replace(current.rect()).is_none());
            }
            Ok::<(), ()>(())
        })
        .expect("the post-Raise resize program is valid");
        let preview = preview.expect("post-Raise motion publishes the actual resize preview");
        assert!(preview.width() > original.width());
        assert_eq!(preview.height(), original.height());
    });
}

#[test]
fn rear_contained_tab_press_raises_before_drag_motion_and_requests_pane_focus() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);
    let point = inspect_surface(&mut session, |plan| {
        plan.tab_receiver(SECOND)
            .expect("the rear tab has an exact receiver")
            .center()
    });
    let moved = LogicalPoint::new(point.x() + 18.0, point.y()).expect("tab movement validates");
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            Some(point),
        )],
    );
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());
    present_pointer_surface(&mut session);
    submit_host_edges(&mut session, &[(SurfacePointerEvent::Moved, Some(moved))]);
    assert!(session.has_active_gesture());
    inspect_surface(&mut session, |plan| {
        let request = plan
            .pane_focus_request()
            .expect("contained tab activation requests pane focus");
        assert_eq!(request.surface(), SURFACE);
        assert_eq!(request.item(), Some(SECOND));
        assert!(
            program_drag_decoration(plan).is_some(),
            "the post-Raise motion crosses the real drag threshold"
        );
    });
}

#[test]
fn rear_split_contained_tab_batch_raises_the_complete_root_before_dragging_an_item() {
    let mut session = stacked_split_contained_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);
    let point = inspect_surface(&mut session, |plan| {
        plan.tab_receiver(SECOND)
            .expect("the rear split tab has an exact receiver")
            .center()
    });
    let moved =
        LogicalPoint::new(point.x() + 18.0, point.y()).expect("split tab movement validates");
    submit_host_edges(
        &mut session,
        &[
            (
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                Some(point),
            ),
            (SurfacePointerEvent::Moved, Some(moved)),
        ],
    );
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());
    for item in [SECOND, THIRD] {
        let item = session
            .view()
            .item(item)
            .expect("the raised root retains both panes");
        assert_eq!(item.root(), FLOATING_ROOT);
        assert_eq!(item.contained(), Some(FLOATING));
    }
}

#[test]
fn rear_contained_tab_select_atomically_raises_and_requests_pane_focus() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);
    let select = prepare_tab_select(&mut session, SECOND);

    let mut frame = session
        .begin_host_frame()
        .expect("contained tab selection frame begins");
    frame
        .submit_surface_action(select)
        .expect("rear contained tab selection is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised contained surface measures");
    frame
        .commit()
        .expect("rear contained tab selection commits atomically");

    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);

    let mut inspection = session
        .begin_host_frame()
        .expect("contained tab focus inspection frame begins");
    let request = inspection
        .paint_plan(SURFACE)
        .expect("focus paint plan lookup succeeds")
        .expect("raised contained candidate remains paintable")
        .pane_focus_request()
        .expect("contained tab selection requests pane focus");
    assert_eq!(request.surface(), SURFACE);
    assert_eq!(request.item(), Some(SECOND));
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("focus inspection retains the current surface");
    inspection
        .commit()
        .expect("contained tab focus inspection commits");
}

#[test]
fn exact_surface_close_action_opens_the_shared_close_workflow() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let action = prepare_tab_close(&mut session, SECOND);

    let mut frame = session.begin_host_frame().expect("close frame begins");
    frame
        .submit_surface_action(action)
        .expect("same-session close action is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("post-close-request measurement succeeds");
    let report = frame.commit().expect("close request frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseRequested {
            origin: HostCloseRequestOrigin::Interaction,
            ..
        }]
    ));
    assert!(session.view().item(SECOND).is_some());
}

#[test]
fn completed_surface_tab_close_keeps_its_successor_pending_until_exact_focus_observation() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let select = prepare_tab_select(&mut session, SECOND);
    let mut frame = session.begin_host_frame().expect("selection frame begins");
    frame
        .submit_surface_action(select)
        .expect("exact tab selects");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("selection measures");
    frame.commit().expect("selection commits");

    let original = inspect_surface(&mut session, |plan| {
        let request = plan
            .pane_focus_request()
            .expect("selected pane has a focus intent");
        assert_eq!(request.item(), Some(SECOND));
        request
    });
    let action = prepare_tab_close(&mut session, SECOND);
    let mut frame = session
        .begin_host_frame()
        .expect("close request frame begins");
    frame
        .submit_surface_action(action)
        .expect("exact close requests a plan");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("close request measures");
    let report = frame.commit().expect("close request commits");
    let [HostInputOutcome::CloseRequested { plan, .. }] = report.inputs() else {
        panic!("exact close must expose one application decision plan");
    };
    inspect_surface(&mut session, |paint| {
        assert_eq!(paint.pane_focus_request(), Some(original));
    });

    let mut frame = session
        .begin_host_frame()
        .expect("close decision frame begins");
    frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            crate::close::CloseDecision::Allow,
        )
        .expect("close decision stages");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("closed surface measures");
    let report = frame.commit().expect("close decision commits");
    assert!(report.workspace_changed());
    assert!(session.view().item(SECOND).is_none());

    let successor = inspect_surface(&mut session, |plan| {
        let request = plan
            .pane_focus_request()
            .expect("close publishes a visual successor");
        assert_eq!(request.item(), Some(FIRST));
        assert_ne!(request, original);
        assert!(
            plan.prepare_pane_focus_observation(original, DockspacePaneFocusObservation::Focused)
                .is_none()
        );
        request
    });
    for observation in [
        DockspacePaneFocusObservation::NotFocused,
        DockspacePaneFocusObservation::Focused,
    ] {
        let mut frame = session
            .begin_host_frame()
            .expect("successor observation frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("paint lookup succeeds")
            .expect("successor surface is paintable");
        assert_eq!(plan.pane_focus_request(), Some(successor));
        let exact = plan
            .prepare_pane_focus_observation(successor, observation)
            .expect("exact successor observation prepares");
        frame
            .submit_pane_focus_observation(exact)
            .expect("successor observation stages");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("observation retains the surface");
        frame.commit().expect("successor observation commits");
    }
    inspect_surface(&mut session, |plan| {
        assert!(plan.pane_focus_request().is_none());
    });
}

#[test]
fn exact_contained_close_action_requests_the_root_without_mutating_content() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);

    let mut frame = session.begin_host_frame().expect("dock-back frame begins");
    let action = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("contained candidate is paintable")
        .prepare_contained_close(FLOATING)
        .expect("contained close control prepares a dock-back action");
    frame
        .submit_surface_action(action)
        .expect("same-frame dock-back action is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("post-dock-back surface measurement succeeds");
    let report = frame.commit().expect("dock-back frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::CloseRequested { plan, reused: false, origin: HostCloseRequestOrigin::Interaction }]
            if plan.target() == crate::close::ClosePlanTarget::Root { root: FLOATING_ROOT }
    ));
    assert!(!report.workspace_changed());
    assert!(session.view().contained(FLOATING).is_some());
    assert!(session.view().item(SECOND).is_some());
}

#[test]
fn candidate_paint_plan_exposes_opaque_presentation_menu_anchors() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);

    let mut frame = session
        .begin_host_frame()
        .expect("presentation-anchor paint frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("contained candidate is paintable");
    let anchors = plan.presentation_menu_anchors().collect::<Vec<_>>();

    assert_eq!(anchors.len(), 2);
    let main = anchors
        .iter()
        .copied()
        .find(|anchor| anchor.root() == ROOT)
        .expect("main root anchor is exposed");
    assert_eq!(main.kind(), DockspacePresentationMenuAnchorKind::TabBar);
    assert_eq!(
        main.visual_id().kind(),
        DockspaceVisualKind::PresentationMenuAnchor
    );
    assert_eq!(main.host_visual_id().kind(), DockspaceVisualKind::TabBar);
    assert!(main.bounds().width() > 0.0 && main.bounds().height() > 0.0);
    assert!(main.operable());
    let main_debug = format!("{main:?}");
    assert!(!main_debug.contains("RootId"));
    assert!(!main_debug.contains("NodeId"));
    assert!(!main_debug.contains("TabBarSceneId"));

    let contained = anchors
        .iter()
        .copied()
        .find(|anchor| anchor.root() == FLOATING_ROOT)
        .expect("contained root anchor is exposed");
    assert_eq!(
        contained.kind(),
        DockspacePresentationMenuAnchorKind::ContainedTitle
    );
    assert_eq!(
        contained.host_visual_id().kind(),
        DockspaceVisualKind::Contained
    );
    assert!(contained.bounds().width() > 0.0 && contained.bounds().height() > 0.0);
    assert!(contained.operable());

    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("anchor inspection retains the ready candidate");
    frame.commit().expect("anchor inspection frame commits");
}

#[test]
fn candidate_bound_presentation_commands_report_truthful_availability_and_execute() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);

    let mut prepare = session
        .begin_host_frame()
        .expect("presentation-command paint frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("contained candidate is paintable");

    let float = plan
        .presentation_command(ROOT, DockspacePresentationCommandKind::Float)
        .expect("float availability is queryable")
        .expect("the docked root belongs to this exact surface");
    assert!(
        float.is_ready(),
        "float unexpectedly unavailable: {:?}",
        float.unavailable_reason()
    );
    assert_eq!(float.root(), ROOT);
    assert_eq!(float.kind(), DockspacePresentationCommandKind::Float);

    let dock_back = plan
        .presentation_command(ROOT, DockspacePresentationCommandKind::DockBack)
        .expect("dock-back availability is queryable")
        .expect("the docked root belongs to this exact surface");
    assert_eq!(
        dock_back.unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::Action(
            crate::model::DockspaceActionRejection::DockBackUnavailable { root: ROOT }
        ))
    );

    let contained_float = plan
        .presentation_command(FLOATING_ROOT, DockspacePresentationCommandKind::Float)
        .expect("contained float availability is queryable")
        .expect("the contained root belongs to this exact surface");
    assert_eq!(
        contained_float.unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::AlreadyContained)
    );

    let contained_dock_back = plan
        .presentation_command(FLOATING_ROOT, DockspacePresentationCommandKind::DockBack)
        .expect("contained dock-back availability is queryable")
        .expect("the contained root belongs to this exact surface");
    assert!(contained_dock_back.is_ready());

    let move_to_new_window = plan
        .presentation_command(ROOT, DockspacePresentationCommandKind::MoveToNewWindow)
        .expect("native promotion availability is queryable")
        .expect("the docked root belongs to this exact surface");
    assert_eq!(
        move_to_new_window.unavailable_reason(),
        Some(DockspacePresentationCommandUnavailable::Action(
            crate::model::DockspaceActionRejection::NativeUnavailable
        ))
    );

    let action = plan
        .prepare_presentation_command(float)
        .expect("the exact ready command prepares one opaque surface action");
    assert!(
        plan.prepare_presentation_command(dock_back).is_none(),
        "an unavailable command cannot mint an action"
    );
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the command preparation frame retains the ready candidate");
    prepare
        .commit()
        .expect("the command preparation frame commits");

    let mut execute = session
        .begin_host_frame()
        .expect("presentation-command execution frame begins");
    execute
        .submit_surface_action(action)
        .expect("the exact scene-bound command is accepted");
    execute
        .measure_surface(SURFACE, metrics())
        .expect("the floated surface is remeasured");
    let report = execute.commit().expect("the presentation command commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::RootFloated {
                root: ROOT,
                surface: SURFACE,
                changed: true,
                ..
            }
        )]
    ));
    assert!(
        session
            .view()
            .surface(SURFACE)
            .expect("the floated surface remains available")
            .contained()
            .any(|contained| contained.root().is_some_and(|root| root.id() == ROOT))
    );
}

#[test]
fn local_dock_back_without_focus_history_does_not_guess_from_selection() {
    let mut session = contained_session(DockPolicy::default());
    install_ready_candidate(&mut session);

    let mut prepare = session
        .begin_host_frame()
        .expect("contained dock-back preparation frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("contained candidate is paintable");
    assert!(plan.pane_focus_request().is_none());
    let command = plan
        .presentation_command(FLOATING_ROOT, DockspacePresentationCommandKind::DockBack)
        .expect("dock-back availability is queryable")
        .expect("the exact contained root exposes a dock-back command");
    assert!(command.is_ready());
    assert_eq!(command.root(), FLOATING_ROOT);
    assert_eq!(command.kind(), DockspacePresentationCommandKind::DockBack);
    let action = plan
        .prepare_presentation_command(command)
        .expect("the exact presentation DockBack prepares an action");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the preparation frame retains the current candidate");
    prepare
        .commit()
        .expect("the dock-back preparation frame commits");

    let mut execute = session
        .begin_host_frame()
        .expect("contained dock-back execution frame begins");
    execute
        .submit_surface_action(action)
        .expect("the exact dock-back action is accepted");
    execute
        .measure_surface(SURFACE, metrics())
        .expect("the recovered surface measures");
    let report = execute.commit().expect("the contained dock-back commits");
    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::RootDocked {
                root: FLOATING_ROOT,
                target_root: ROOT,
                items: vec![SECOND],
                changed: true,
            }
        )]
    );
    assert!(report.workspace_changed());
    assert!(
        report
            .inputs()
            .iter()
            .all(|input| !matches!(input, HostInputOutcome::CloseRequested { .. }))
    );
    assert!(session.view().contained(FLOATING).is_none());
    assert!(session.view().root(FLOATING_ROOT).is_none());
    let docked = session
        .view()
        .item(SECOND)
        .expect("docked content survives");
    assert_eq!(docked.surface(), SURFACE);
    assert_eq!(docked.root(), ROOT);
    assert_eq!(docked.contained(), None);
    assert!(docked.is_selected());
    assert_eq!(docked.tabs().items(), &[FIRST, THIRD, SECOND]);

    let mut inspect = session
        .begin_host_frame()
        .expect("focus inspection frame begins");
    let plan = inspect
        .paint_plan(SURFACE)
        .expect("focus paint plan lookup succeeds")
        .expect("the recovered surface remains paintable");
    let request = plan
        .pane_focus_request()
        .expect("explicit DockBack requests the current selected pane without focus history");
    assert_eq!(request.surface(), SURFACE);
    assert_eq!(request.item(), Some(SECOND));
    assert_eq!(
        request.target(),
        crate::runtime::DockspaceFocusTarget::Pane(SECOND)
    );
    inspect
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the focus inspection retains the current surface");
    inspect
        .commit()
        .expect("the focus inspection frame commits");
}

#[test]
fn directional_presentation_commands_are_core_derived_and_execute() {
    let directions = [
        DockspacePresentationCommandKind::DockLeft,
        DockspacePresentationCommandKind::DockRight,
        DockspacePresentationCommandKind::DockTop,
        DockspacePresentationCommandKind::DockBottom,
        DockspacePresentationCommandKind::DockCenter,
    ];

    for kind in directions {
        let mut session = contained_session(DockPolicy::default());
        install_ready_candidate(&mut session);
        let mut prepare = session
            .begin_host_frame()
            .expect("directional-command paint frame begins");
        let plan = prepare
            .paint_plan(SURFACE)
            .expect("paint plan lookup succeeds")
            .expect("contained candidate is paintable");
        let docked = plan
            .presentation_command(ROOT, kind)
            .expect("docked directional availability is queryable")
            .expect("the docked root belongs to this exact surface");
        assert_eq!(
            docked.unavailable_reason(),
            Some(DockspacePresentationCommandUnavailable::Action(
                crate::model::DockspaceActionRejection::DockBackUnavailable { root: ROOT }
            )),
            "a main root cannot fabricate a second target for {kind:?}",
        );

        let floating = plan
            .presentation_command(FLOATING_ROOT, kind)
            .expect("contained directional availability is queryable")
            .expect("the contained root belongs to this exact surface");
        assert!(
            floating.is_ready(),
            "contained {kind:?} unexpectedly unavailable: {:?}",
            floating.unavailable_reason(),
        );
        let action = plan
            .prepare_presentation_command(floating)
            .expect("the exact directional command prepares one opaque action");
        prepare
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("directional command preparation retains the current candidate");
        prepare
            .commit()
            .expect("directional command preparation commits");

        let mut execute = session
            .begin_host_frame()
            .expect("directional-command execution frame begins");
        execute
            .submit_surface_action(action)
            .expect("the exact directional command is accepted");
        execute
            .measure_surface(SURFACE, metrics())
            .expect("the directionally docked surface is remeasured");
        let report = execute
            .commit()
            .expect("the directional docking command commits");

        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::ProductActionApplied(
                crate::model::DockspaceActionOutcome::RootDocked {
                    root: FLOATING_ROOT,
                    target_root: ROOT,
                    items,
                    changed: true,
                }
            )] if items == &[SECOND]
        ));
        assert!(session.view().contained(FLOATING).is_none());
        let content = session
            .view()
            .root(ROOT)
            .and_then(|root| root.content())
            .expect("directional docking preserves the target root");
        match kind {
            DockspacePresentationCommandKind::DockLeft => {
                let split = content.split().expect("Dock Left creates a split");
                assert_eq!(split.axis(), DockspaceAxis::Horizontal);
                assert_eq!(
                    split
                        .children()
                        .map(|child| child.tabs().unwrap().items().to_vec())
                        .collect::<Vec<_>>(),
                    vec![vec![SECOND], vec![FIRST, THIRD]],
                );
            }
            DockspacePresentationCommandKind::DockRight => {
                let split = content.split().expect("Dock Right creates a split");
                assert_eq!(split.axis(), DockspaceAxis::Horizontal);
                assert_eq!(
                    split
                        .children()
                        .map(|child| child.tabs().unwrap().items().to_vec())
                        .collect::<Vec<_>>(),
                    vec![vec![FIRST, THIRD], vec![SECOND]],
                );
            }
            DockspacePresentationCommandKind::DockTop => {
                let split = content.split().expect("Dock Top creates a split");
                assert_eq!(split.axis(), DockspaceAxis::Vertical);
                assert_eq!(
                    split
                        .children()
                        .map(|child| child.tabs().unwrap().items().to_vec())
                        .collect::<Vec<_>>(),
                    vec![vec![SECOND], vec![FIRST, THIRD]],
                );
            }
            DockspacePresentationCommandKind::DockBottom => {
                let split = content.split().expect("Dock Bottom creates a split");
                assert_eq!(split.axis(), DockspaceAxis::Vertical);
                assert_eq!(
                    split
                        .children()
                        .map(|child| child.tabs().unwrap().items().to_vec())
                        .collect::<Vec<_>>(),
                    vec![vec![FIRST, THIRD], vec![SECOND]],
                );
            }
            DockspacePresentationCommandKind::DockCenter => assert_eq!(
                content
                    .tabs()
                    .expect("Dock Center merges into the central tab group")
                    .items(),
                &[FIRST, THIRD, SECOND],
            ),
            DockspacePresentationCommandKind::Float
            | DockspacePresentationCommandKind::MoveToNewWindow
            | DockspacePresentationCommandKind::DockBack => {
                unreachable!("the table contains only directional commands")
            }
        }
    }
}

#[test]
fn stale_surface_action_is_inert_after_a_new_workspace_revision() {
    let mut session = session();
    install_ready_candidate(&mut session);
    let stale = prepare_tab_select(&mut session, FIRST);

    let mut mutate = session.begin_host_frame().expect("mutation frame begins");
    mutate
        .select_item_current(SECOND)
        .expect("current selection action is accepted");
    mutate
        .measure_surface(SURFACE, metrics())
        .expect("mutated surface measurement succeeds");
    mutate.commit().expect("mutation frame commits");

    let mut frame = session
        .begin_host_frame()
        .expect("stale action frame begins");
    frame
        .submit_surface_action(stale)
        .expect("same-session stale action is structurally accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("stale action surface measurement succeeds");
    let report = frame.commit().expect("stale action frame commits");

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert!(
        session
            .view()
            .item(SECOND)
            .expect("second item remains open")
            .is_selected()
    );
}

#[test]
fn cross_session_surface_action_is_rejected_without_poisoning_the_frame() {
    let mut first = session();
    install_ready_candidate(&mut first);
    let foreign = prepare_tab_select(&mut first, SECOND);

    let mut second = session();
    install_ready_candidate(&mut second);
    let mut frame = second
        .begin_host_frame()
        .expect("second session frame begins");
    let error = frame
        .submit_surface_action(foreign)
        .expect_err("foreign surface action must be rejected");
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::OperationConflict);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unaffected frame remains usable");
    frame.commit().expect("the unaffected frame commits");

    assert!(
        second
            .view()
            .item(FIRST)
            .expect("first item remains open")
            .is_selected()
    );
}

#[test]
fn presented_host_splitter_edges_drive_the_core_resize_session() {
    let mut session = split_session();
    install_ready_candidate(&mut session);
    present_pointer_surface(&mut session);
    assert!(!session.has_active_gesture());
    let before = split_weights(&session);
    let initial = inspect_surface(&mut session, |plan| {
        let splitter = plan
            .splitters()
            .next()
            .expect("the equal split has one splitter");
        plan.receiver_for_splitter(splitter)
            .expect("the splitter has an exact receiver")
            .center()
    });
    let released =
        LogicalPoint::new(initial.x() + 48.0, initial.y()).expect("release point validates");
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
            Some(initial),
        )],
    );
    assert!(session.has_active_gesture());
    assert_eq!(split_weights(&session), before);
    submit_host_edges(
        &mut session,
        &[(
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            Some(released),
        )],
    );
    assert!(!session.has_active_gesture());
    assert_ne!(split_weights(&session), before);
}

#[test]
fn exact_surface_splitter_adjustment_uses_the_local_ready_candidate() {
    let mut session = split_session();
    install_ready_candidate(&mut session);
    let before = split_weights(&session);

    let mut frame = session.begin_host_frame().expect("adjustment frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("split candidate is paintable");
    let splitter = plan
        .splitters()
        .next()
        .expect("the equal split has one splitter");
    let action = plan
        .prepare_splitter_adjustment(splitter.visual_id(), SurfaceSplitterAdjustment::Increment)
        .expect("the operable splitter prepares an adjustment");
    frame
        .submit_surface_action(action)
        .expect("the same-frame local response is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("the adjusted surface measures");
    let report = frame.commit().expect("the adjustment frame commits");

    assert!(split_weights(&session)[0] > before[0]);
    assert!(report.events().is_empty());
    let change = report
        .layout_change()
        .expect("split resizing is a layout change");
    assert_eq!(change.before(), report.before());
    assert_eq!(change.after(), report.after());
    assert_ne!(change.before(), change.after());
    assert!(!report.notification().is_empty());

    let mut idle = session.begin_host_frame().unwrap();
    idle.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(idle.commit().unwrap().notification().is_empty());
}

#[test]
fn exact_junction_axis_adjustment_updates_only_that_axis_in_one_revision() {
    let mut session = splitter_junction_session();
    install_ready_candidate(&mut session);
    let before_version = session.version();
    let (before_root, before_columns) = splitter_junction_weights(&session);

    let mut frame = session
        .begin_host_frame()
        .expect("junction adjustment frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("junction paint plan lookup succeeds")
        .expect("junction candidate is paintable");
    let junction = plan
        .splitter_junctions()
        .next()
        .expect("the perpendicular fixture exposes one junction");
    let debug = format!("{junction:?}");
    assert!(!debug.contains("SplitterJunctionId"));
    assert!(!debug.contains("NodeId"));
    assert!(!debug.contains("north"));
    assert!(junction.has_axis(DockspaceAxis::Horizontal));
    assert!(junction.has_axis(DockspaceAxis::Vertical));
    assert!(
        plan.splitter_junction_axis_operable(junction, DockspaceAxis::Horizontal),
        "the horizontal junction axis is independently operable"
    );
    let action = plan
        .prepare_splitter_junction_adjustment(
            junction.visual_id(),
            DockspaceAxis::Horizontal,
            SurfaceSplitterAdjustment::Increment,
        )
        .expect("the exact horizontal junction axis prepares an adjustment");
    frame
        .submit_surface_action(action)
        .expect("the same-frame junction adjustment is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("the adjusted junction surface measures");
    frame
        .commit()
        .expect("the junction adjustment frame commits");

    let (after_root, after_columns) = splitter_junction_weights(&session);
    assert!(after_root[0] > before_root[0]);
    assert_eq!(after_columns, before_columns);
    assert_eq!(
        session.version().revision().get(),
        before_version.revision().get() + 1,
        "one axis-wide ResizeSplits command advances exactly one revision"
    );
}

#[test]
fn junction_axis_action_is_absent_when_that_axis_is_not_operable() {
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(DockspaceAxis::Vertical, false);
    let mut session = splitter_junction_session_with_policy(policy);
    install_ready_candidate(&mut session);

    let mut frame = session
        .begin_host_frame()
        .expect("axis-policy inspection frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("axis-policy plan lookup succeeds")
        .expect("axis-policy candidate is paintable");
    let junction = plan
        .splitter_junctions()
        .next()
        .expect("the structural junction remains available for axis semantics");
    assert!(!plan.splitter_junction_operable(junction));
    assert!(plan.receiver_for_splitter_junction(junction).is_none());
    assert!(plan.splitter_junction_axis_operable(junction, DockspaceAxis::Horizontal));
    assert!(!plan.splitter_junction_axis_operable(junction, DockspaceAxis::Vertical));
    assert!(
        plan.prepare_splitter_junction_adjustment(
            junction.visual_id(),
            DockspaceAxis::Horizontal,
            SurfaceSplitterAdjustment::Increment,
        )
        .is_some()
    );
    assert!(
        plan.prepare_splitter_junction_adjustment(
            junction.visual_id(),
            DockspaceAxis::Vertical,
            SurfaceSplitterAdjustment::Increment,
        )
        .is_none(),
        "a denied axis must publish no semantic adjustment"
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the axis-policy inspection retains the candidate");
    frame.commit().expect("the inspection frame commits");
}

#[test]
fn exact_surface_tab_list_actions_open_and_select_from_the_current_candidate() {
    let mut session = overflow_session();
    install_overflow_candidate(&mut session);

    let mut prepare = session
        .begin_host_frame()
        .expect("overflow control paint frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("overflow control plan lookup succeeds")
        .expect("overflow control plan is paintable");
    let control = plan
        .tab_strip_controls()
        .find(|control| control.kind() == TabStripControlKind::TabListMenu)
        .expect("overflow plan exposes the menu control");
    let open = plan
        .prepare_tab_strip_control_activation(control)
        .expect("enabled menu control prepares an action");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the control plan is retained");
    prepare.commit().expect("control paint frame commits");

    let mut open_frame = session
        .begin_host_frame()
        .expect("menu open action frame begins");
    open_frame
        .submit_surface_action(open)
        .expect("menu open action is accepted");
    open_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("menu open defers remeasurement");
    open_frame.commit().expect("menu open action commits");

    let mut publish_menu = session
        .begin_host_frame()
        .expect("menu publication frame begins");
    measure_overflow_surface(&mut publish_menu);
    publish_menu.commit().expect("menu candidate commits");

    let mut prepare_row = session
        .begin_host_frame()
        .expect("menu row paint frame begins");
    let plan = prepare_row
        .paint_plan(SURFACE)
        .expect("menu plan lookup succeeds")
        .expect("menu plan is paintable");
    let menu = plan
        .tab_list_menus()
        .next()
        .expect("the tab-list menu is open");
    let row = menu
        .rows()
        .find(|row| row.item() == FOURTH)
        .expect("the fourth item has a menu row");
    let select = plan
        .prepare_tab_list_menu_row_activation(row)
        .expect("the visible menu row prepares an action");
    prepare_row
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the menu plan is retained");
    prepare_row.commit().expect("menu row paint frame commits");

    let mut select_frame = session
        .begin_host_frame()
        .expect("menu selection frame begins");
    select_frame
        .submit_surface_action(select)
        .expect("menu row action is accepted");
    select_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("menu selection defers remeasurement");
    select_frame.commit().expect("menu row selection commits");

    assert!(
        session
            .view()
            .item(FOURTH)
            .expect("fourth item remains open")
            .is_selected()
    );
}

#[test]
fn only_the_popup_owner_draws_and_prepares_its_backdrop_dismissal() {
    let mut session = multiview_overflow_session();
    let mut initial = session
        .begin_host_frame()
        .expect("multiview overflow measurement frame begins");
    for surface in [SURFACE, TARGET_SURFACE] {
        measure_overflow_surface_on(&mut initial, surface);
    }
    initial
        .commit()
        .expect("multiview overflow candidates commit");

    let mut prepare_open = session
        .begin_host_frame()
        .expect("target menu control paint frame begins");
    let target_plan = prepare_open
        .paint_plan(TARGET_SURFACE)
        .expect("target menu control plan lookup succeeds")
        .expect("target menu control plan is paintable");
    let target_control = target_plan
        .tab_strip_controls()
        .find(|control| control.kind() == TabStripControlKind::TabListMenu)
        .expect("target overflow plan exposes the menu control");
    let open = target_plan
        .prepare_tab_strip_control_activation(target_control)
        .expect("target menu control prepares an action");
    prepare_open
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("target control plan retains all surfaces");
    prepare_open
        .commit()
        .expect("target control paint frame commits");

    let mut open_frame = session
        .begin_host_frame()
        .expect("target menu open frame begins");
    open_frame
        .submit_surface_action(open)
        .expect("target menu open action is accepted");
    open_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("target menu open defers remeasurement");
    open_frame.commit().expect("target menu open commits");

    let mut publish_popup = session
        .begin_host_frame()
        .expect("cross-surface popup publication frame begins");
    for surface in [SURFACE, TARGET_SURFACE] {
        measure_overflow_surface_on(&mut publish_popup, surface);
    }
    publish_popup
        .commit()
        .expect("cross-surface popup candidates commit");

    let mut prepare_dismiss = session
        .begin_host_frame()
        .expect("owner backdrop paint frame begins");
    {
        let non_owner = prepare_dismiss
            .paint_plan(SURFACE)
            .expect("non-owner popup plan lookup succeeds")
            .expect("non-owner popup plan is paintable");
        let control = non_owner
            .tab_strip_controls()
            .find(|control| control.kind() == TabStripControlKind::TabListMenu)
            .expect("non-owner overflow plan exposes the menu control");
        assert!(
            !control.menu_open(),
            "only the popup owner may expose an expanded menu control"
        );
        let mut backdrop = None;
        let mut popup_menu_seen = false;
        non_owner
            .visit_surface_program(|step| {
                match step {
                    SurfacePaintProgramStep::PopupBackdrop(current) => {
                        assert!(backdrop.replace(current).is_none());
                    }
                    SurfacePaintProgramStep::PopupMenu { .. } => popup_menu_seen = true,
                    _ => {}
                }
                Ok::<(), ()>(())
            })
            .expect("the non-owner popup program is valid");
        assert!(
            !popup_menu_seen,
            "a non-owner surface must not reconstruct the owner's menu"
        );
        assert!(
            backdrop.is_none(),
            "an unrelated surface has no popup visual"
        );
    }
    let dismiss = {
        let owner = prepare_dismiss
            .paint_plan(TARGET_SURFACE)
            .expect("owner popup plan lookup succeeds")
            .expect("owner popup plan is paintable");
        let control = owner
            .tab_strip_controls()
            .find(|control| control.kind() == TabStripControlKind::TabListMenu)
            .expect("owner overflow plan exposes the menu control");
        assert!(
            control.menu_open(),
            "the popup owner must expose an expanded menu control"
        );
        let mut backdrop = None;
        owner
            .visit_surface_program(|step| {
                if let SurfacePaintProgramStep::PopupBackdrop(current) = step {
                    assert!(backdrop.replace(current).is_none());
                }
                Ok::<(), ()>(())
            })
            .unwrap();
        owner
            .prepare_tab_list_menu_dismiss(backdrop.expect("the owner paints its backdrop"))
            .unwrap()
    };
    prepare_dismiss
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("popup inspection retains both surface candidates");
    prepare_dismiss
        .commit()
        .expect("popup inspection frame commits");

    let mut dismiss_frame = session
        .begin_host_frame()
        .expect("owner backdrop dismiss frame begins");
    dismiss_frame
        .submit_surface_action(dismiss)
        .expect("owner backdrop dismiss action is accepted");
    dismiss_frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("owner backdrop dismissal defers remeasurement");
    dismiss_frame
        .commit()
        .expect("owner backdrop dismissal commits");

    let mut publish_closed = session
        .begin_host_frame()
        .expect("closed popup publication frame begins");
    for surface in [SURFACE, TARGET_SURFACE] {
        measure_overflow_surface_on(&mut publish_closed, surface);
    }
    publish_closed
        .commit()
        .expect("closed popup candidates commit");

    let mut verify = session
        .begin_host_frame()
        .expect("closed popup verification frame begins");
    for surface in [SURFACE, TARGET_SURFACE] {
        let plan = verify
            .paint_plan(surface)
            .expect("closed popup plan lookup succeeds")
            .expect("closed popup plan is paintable");
        let control = plan
            .tab_strip_controls()
            .find(|control| control.kind() == TabStripControlKind::TabListMenu)
            .expect("closed overflow plan exposes the menu control");
        assert!(!control.menu_open());
        plan.visit_surface_program(|step| {
            assert!(!matches!(
                step,
                SurfacePaintProgramStep::PopupBackdrop(_)
                    | SurfacePaintProgramStep::PopupMenu { .. }
            ));
            Ok::<(), ()>(())
        })
        .expect("the closed popup program is valid");
    }
    verify
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("closed popup verification retains both surfaces");
    verify.commit().expect("closed popup verification commits");
}
