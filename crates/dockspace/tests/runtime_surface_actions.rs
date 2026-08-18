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
    ContainedResizeDirection, DockspaceDragSourceKind, DockspacePaneFocusObservation,
    DockspacePresentationCommandKind, DockspacePresentationCommandUnavailable,
    DockspacePresentationMenuAnchorKind, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceRuntimeErrorKind, DockspaceSession, DockspaceVisualId, DockspaceVisualKind,
    HostCloseRequestOrigin, HostInputOutcome, PreparedSurfaceAction,
    SurfaceContainedResizeAdjustment, SurfaceGesturePhase, SurfaceMeasurementAnswer,
    SurfaceMeasurementRequest, SurfacePointerButton, SurfacePointerCancelReason,
    SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId, SurfacePointerInput,
    SurfacePointerPosition, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceSplitterAdjustment, SurfaceTabNavigation, SurfaceUnavailableReason,
    TabGroupDragRegionKind, TabListMenuMetrics, TabStripControlKind, TabStripControlMetric,
    TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics, UniformSurfaceMetrics,
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
    paint
        .confirm_surface_painted(SURFACE)
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
    frame
        .measure_surface_with(SURFACE, overflow_measurement)
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
        let decoration = drag
            .paint_plan(SURFACE)
            .expect("source drag paint plan lookup succeeds")
            .expect("the presented source candidate remains paintable")
            .drag_decoration()
            .expect("journal reduction owns the active source decoration");
        assert_eq!(decoration.source_surface(), SURFACE);
        assert_eq!(decoration.kind(), DockspaceDragSourceKind::Item);
        assert_eq!(decoration.item(), Some(FIRST));
        assert_eq!(decoration.source_visual(), source_visual);
        assert!(decoration.omits_visual(source_visual));
        let debug = format!("{decoration:?}");
        assert!(!debug.contains("NodeId"));
        assert!(!debug.contains("fingerprint"));
    }
    {
        let decoration = drag
            .paint_plan(TARGET_SURFACE)
            .expect("target drag paint plan lookup succeeds")
            .expect("the target candidate remains paintable")
            .drag_decoration()
            .expect("the same core decoration reaches the second ready surface");
        assert_eq!(decoration.source_surface(), SURFACE);
        assert_eq!(decoration.kind(), DockspaceDragSourceKind::Item);
        assert_eq!(decoration.item(), Some(FIRST));
        assert_eq!(decoration.source_visual(), source_visual);
        assert!(decoration.omits_visual(source_visual));
        assert!(
            !decoration.omits_visual(target_visual),
            "only the exact source visual is omitted on a target surface"
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
        assert!(
            cancel
                .paint_plan(surface)
                .expect("cancelled paint plan lookup succeeds")
                .expect("the retained candidate remains paintable")
                .drag_decoration()
                .is_none(),
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
    assert!(
        restart
            .paint_plan(SURFACE)
            .expect("restarted paint plan lookup succeeds")
            .expect("the retained source remains paintable")
            .drag_decoration()
            .is_some(),
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
    assert!(
        plan.prepare_tab_gesture(
            FIRST,
            SurfaceGesturePhase::Begin {
                initial: LogicalPoint::new(16.0, 16.0).expect("initial point validates"),
                current: LogicalPoint::new(24.0, 16.0).expect("current point validates"),
            },
        )
        .is_none()
    );
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the paint-only candidate is retained");
    frame.commit().expect("paint-only inspection frame commits");
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
    assert_eq!(request.item(), SECOND);
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
    assert!(
        settled
            .paint_plan(SURFACE)
            .expect("settled focus plan lookup succeeds")
            .expect("the settled focus surface remains paintable")
            .pane_focus_request()
            .is_none()
    );
    settled
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the settled surface is retained");
    settled.commit().expect("settled inspection frame commits");
}

#[test]
fn rear_contained_title_press_atomically_raises_without_starting_a_gesture() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);

    let mut frame = session
        .begin_host_frame()
        .expect("contained activation frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("contained activation plan lookup succeeds")
        .expect("stacked contained candidate is paintable");
    let rear = plan
        .contained()
        .find(|contained| contained.floating() == FLOATING)
        .expect("rear contained presentation is painted");
    let bounds = rear.title_drag_bounds();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("rear title activation point validates");
    let activate = plan
        .prepare_contained_activation(FLOATING, point)
        .expect("rear contained title prepares exact activation");
    frame
        .submit_surface_action(activate)
        .expect("rear contained activation is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised contained surface measures");
    frame
        .commit()
        .expect("rear contained activation commits atomically");

    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(!session.has_active_gesture());
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
    assert!(
        plan.prepare_contained_title_gesture(
            FLOATING,
            SurfaceGesturePhase::Begin {
                initial: rear.title_drag_bounds().min(),
                current: rear.title_drag_bounds().min(),
            },
        )
        .is_none()
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

    let mut frame = session
        .begin_host_frame()
        .expect("contained pane activation frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("contained pane activation plan lookup succeeds")
        .expect("stacked contained candidate is paintable");
    let pane = plan
        .panes()
        .find(|pane| pane.root() == FLOATING_ROOT && pane.selected() == Some(SECOND))
        .expect("rear contained selected pane is painted");
    let bounds = pane.content_bounds();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("rear pane activation point validates");
    let activate = plan
        .prepare_contained_activation(FLOATING, point)
        .expect("rear contained pane prepares exact activation");
    frame
        .submit_surface_action(activate)
        .expect("rear contained pane activation is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised contained pane surface measures");
    frame
        .commit()
        .expect("rear contained pane activation commits atomically");

    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(!session.has_active_gesture());

    let mut inspection = session
        .begin_host_frame()
        .expect("contained pane focus inspection frame begins");
    let plan = inspection
        .paint_plan(SURFACE)
        .expect("contained pane focus plan lookup succeeds")
        .expect("raised contained pane remains paintable");
    assert_eq!(
        plan.pane_focus_request().map(|request| request.item()),
        Some(SECOND)
    );
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("contained pane focus inspection retains the current surface");
    inspection
        .commit()
        .expect("contained pane focus inspection commits");
}

#[test]
fn rear_contained_title_begin_atomically_raises_and_starts_the_local_drag() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);

    let mut frame = session
        .begin_host_frame()
        .expect("contained title gesture frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("contained paint plan lookup succeeds")
        .expect("stacked contained candidate is paintable");
    let rear = plan
        .contained()
        .find(|contained| contained.floating() == FLOATING)
        .expect("rear contained presentation is painted");
    let bounds = rear.title_bounds();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("rear title center validates");
    let moved = LogicalPoint::new(point.x() + 18.0, point.y())
        .expect("rear title threshold movement validates");
    let begin = plan
        .prepare_contained_title_gesture(
            FLOATING,
            SurfaceGesturePhase::Begin {
                initial: point,
                current: moved,
            },
        )
        .expect("rear contained title prepares a local drag");
    frame
        .submit_surface_action(begin)
        .expect("rear contained title begin is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised contained surface measures");
    frame
        .commit()
        .expect("rear contained title begin commits atomically");

    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());

    let mut inspection = session
        .begin_host_frame()
        .expect("contained drag feedback inspection frame begins");
    let plan = inspection
        .paint_plan(SURFACE)
        .expect("contained drag feedback plan lookup succeeds")
        .expect("raised contained candidate remains paintable");
    assert!(
        plan.drag_decoration().is_some(),
        "the raised source remains decorated in the terminal scene"
    );
    assert!(
        plan.drag_preview().is_some(),
        "the threshold-crossing preview is rebased onto the terminal scene"
    );
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("drag feedback inspection retains the current surface");
    inspection
        .commit()
        .expect("drag feedback inspection commits");
}

#[test]
fn rear_contained_resize_begin_atomically_raises_and_starts_the_local_transform() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);

    let mut frame = session
        .begin_host_frame()
        .expect("contained resize gesture frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("contained paint plan lookup succeeds")
        .expect("stacked contained candidate is paintable");
    let rear = plan
        .contained()
        .find(|contained| contained.floating() == FLOATING)
        .expect("rear contained presentation is painted");
    let east = rear
        .resize()
        .find(|resize| resize.direction() == ContainedResizeDirection::East)
        .expect("rear contained east resize handle is painted");
    let bounds = east.hit_bounds();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("rear east resize center validates");
    let moved = LogicalPoint::new(point.x() + 18.0, point.y())
        .expect("rear east resize movement validates");
    let begin = plan
        .prepare_contained_resize_gesture(
            FLOATING,
            ContainedResizeDirection::East,
            SurfaceGesturePhase::Begin {
                initial: point,
                current: moved,
            },
        )
        .expect("rear contained resize prepares a local transform");
    frame
        .submit_surface_action(begin)
        .expect("rear contained resize begin is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised contained surface measures");
    frame
        .commit()
        .expect("rear contained resize begin commits atomically");

    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());
}

#[test]
fn rear_contained_tab_begin_atomically_raises_starts_drag_and_requests_pane_focus() {
    let mut session = stacked_contained_session();
    install_ready_candidate(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);

    let mut frame = session
        .begin_host_frame()
        .expect("contained tab gesture frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("contained paint plan lookup succeeds")
        .expect("stacked contained candidate is paintable");
    let tab = plan
        .tabs()
        .find(|tab| tab.item() == SECOND)
        .expect("rear contained tab is painted");
    let bounds = tab.drag_bounds();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("rear contained tab center validates");
    let begin = plan
        .prepare_tab_gesture(
            SECOND,
            SurfaceGesturePhase::Begin {
                initial: point,
                current: point,
            },
        )
        .expect("rear contained tab prepares a local drag");
    frame
        .submit_surface_action(begin)
        .expect("rear contained tab begin is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised contained surface measures");
    frame
        .commit()
        .expect("rear contained tab begin commits atomically");

    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());

    let mut inspection = session
        .begin_host_frame()
        .expect("contained tab focus inspection frame begins");
    let request = inspection
        .paint_plan(SURFACE)
        .expect("focus paint plan lookup succeeds")
        .expect("raised contained candidate remains paintable")
        .pane_focus_request()
        .expect("contained tab activation requests pane focus");
    assert_eq!(request.surface(), SURFACE);
    assert_eq!(request.item(), SECOND);
    inspection
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("focus inspection retains the current surface");
    inspection
        .commit()
        .expect("contained tab focus inspection commits");
}

#[test]
fn rear_split_contained_tab_begin_raises_the_complete_root_before_dragging_an_item() {
    let mut session = stacked_split_contained_session();
    install_ready_candidate(&mut session);
    assert_eq!(contained_roster(&session), [FLOATING, FRONT_FLOATING]);

    let mut frame = session
        .begin_host_frame()
        .expect("split-contained tab gesture frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("split-contained paint plan lookup succeeds")
        .expect("split-contained candidate is paintable");
    let tab = plan
        .tabs()
        .find(|tab| tab.item() == SECOND)
        .expect("rear split-contained tab is painted");
    let bounds = tab.drag_bounds();
    let point = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("rear split-contained tab center validates");
    let begin = plan
        .prepare_tab_gesture(
            SECOND,
            SurfaceGesturePhase::Begin {
                initial: point,
                current: point,
            },
        )
        .expect("rear split-contained tab prepares a local drag");
    frame
        .submit_surface_action(begin)
        .expect("rear split-contained tab begin is accepted");
    frame
        .measure_surface(SURFACE, metrics())
        .expect("raised split-contained surface measures");
    frame
        .commit()
        .expect("rear split-contained tab begin commits atomically");
    assert_eq!(contained_roster(&session), [FRONT_FLOATING, FLOATING]);
    assert!(session.has_active_gesture());
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
    assert_eq!(request.item(), SECOND);
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
fn exact_contained_close_action_docks_the_root_back_without_closing_content() {
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
        [HostInputOutcome::ProductActionApplied(
            crate::model::DockspaceActionOutcome::RootDocked {
                root: FLOATING_ROOT,
                items,
                changed: true,
                ..
            }
        )] if items == &[SECOND]
    ));
    assert!(session.view().contained(FLOATING).is_none());
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
fn opaque_splitter_actions_drive_the_core_resize_session() {
    let mut session = split_session();
    install_ready_candidate(&mut session);
    assert!(!session.has_active_gesture());
    let before = split_weights(&session);

    let mut prepare = session.begin_host_frame().expect("paint frame begins");
    let plan = prepare
        .paint_plan(SURFACE)
        .expect("paint plan lookup succeeds")
        .expect("split candidate is paintable");
    let splitter = plan
        .splitters()
        .next()
        .expect("the equal split has one splitter");
    let bounds = splitter.hit_bounds();
    let initial = LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )
    .expect("splitter center validates");
    let released = LogicalPoint::new(initial.x() + 48.0, initial.y())
        .expect("splitter release point validates");
    let press = plan
        .prepare_splitter_gesture(
            splitter.visual_id(),
            SurfaceGesturePhase::Begin {
                initial,
                current: initial,
            },
        )
        .expect("the exact operable splitter prepares a press");
    let release = plan
        .prepare_splitter_gesture(
            splitter.visual_id(),
            SurfaceGesturePhase::Release { current: released },
        )
        .expect("the exact operable splitter prepares a release");
    prepare
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the unchanged candidate is retained");
    prepare.commit().expect("paint pass commits");

    let mut press_frame = session.begin_host_frame().expect("press frame begins");
    press_frame
        .submit_surface_action(press)
        .expect("splitter press is accepted");
    press_frame
        .measure_surface(SURFACE, metrics())
        .expect("pressed surface measures");
    press_frame.commit().expect("splitter press commits");
    assert!(session.has_active_gesture());
    assert_eq!(split_weights(&session), before);

    let mut release_frame = session.begin_host_frame().expect("release frame begins");
    release_frame
        .submit_surface_action(release)
        .expect("splitter release is accepted");
    release_frame
        .measure_surface(SURFACE, metrics())
        .expect("released surface measures");
    release_frame.commit().expect("splitter release commits");

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
    frame.commit().expect("the adjustment frame commits");

    assert!(split_weights(&session)[0] > before[0]);
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
