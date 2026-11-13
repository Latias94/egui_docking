//! Application layout, measurements and exact presented receiver routing.

use dockspace::{
    geometry::{LogicalPoint, LogicalRect},
    model::{
        DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
        DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId,
        SurfaceId,
    },
    policy::{CloseCapability, DockPolicy},
    runtime::{
        DockspaceHostFrame, DockspaceSession, HostFrameReport, HostPointerInput, HostPointerRoute,
        PointerDeviceKind, PointerEventMetadata, SurfacePointerCancelReason, SurfacePointerCapture,
        SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
    },
};
use dockspace_winit::{MouseInput, SurfaceCoordinates, backend::event::WindowEvent};

use super::{AppResult, paint};

pub(super) const SURFACE: SurfaceId = SurfaceId::new(1);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

pub(super) fn layout() -> AppResult<DockspaceLayout> {
    Ok(DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::split(
                DockspaceAxis::Horizontal,
                [
                    (
                        DockspaceNode::central_tabs([ItemId::new(1), ItemId::new(2)]),
                        2.0,
                    ),
                    (
                        DockspaceNode::equal_split(
                            DockspaceAxis::Vertical,
                            [
                                DockspaceNode::tabs([ItemId::new(3)]),
                                DockspaceNode::tabs([ItemId::new(4)]),
                            ],
                        )?,
                        1.0,
                    ),
                ],
            )?,
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(5)])),
        LogicalRect::new(120.0, 270.0, 300.0, 180.0)?,
    ))])?)
}

pub(super) fn session() -> AppResult<DockspaceSession> {
    let mut policy = DockPolicy::default();
    // This example focuses on host wiring; documents have no unsaved-close workflow.
    policy.set_close_capability(CloseCapability::Disabled);
    let mut session = DockspaceSession::from_layout(layout()?, policy)?;
    session.enable_host_pointer()?;
    Ok(session)
}

pub(super) fn measure(frame: &mut DockspaceHostFrame<'_>, bounds: LogicalRect) -> AppResult<()> {
    paint::measure(frame, SURFACE, bounds)
}

/// Applies the example's Shift-to-float policy in the original window-event order.
/// A missing presented mapping forwards edges with an unknown position.
pub(super) fn window_input(
    session: &mut DockspaceSession,
    mouse: &mut MouseInput,
    coordinates: Option<SurfaceCoordinates>,
    event: &WindowEvent,
) -> AppResult<Option<HostFrameReport>> {
    let (event, position, docking_enabled) = if let Some(sample) = mouse.on_window_event(event) {
        let position = match coordinates
            .map(|coordinates| sample.logical_position(coordinates))
            .transpose()?
        {
            Some(dockspace::runtime::SurfacePointerPosition::Known(point)) => Some(point),
            Some(dockspace::runtime::SurfacePointerPosition::Unknown) | None => None,
        };
        (sample.event, position, !sample.modifiers.shift_key())
    } else if let WindowEvent::ModifiersChanged(modifiers) = event {
        let enabled = !modifiers.state().shift_key();
        (
            SurfacePointerEvent::DockingEnabledChanged { enabled },
            None,
            enabled,
        )
    } else {
        return Ok(None);
    };
    input(session, event, position, docking_enabled).map(Some)
}

/// This window owns all its pixels, with no overlays or third-party widgets.
/// Winit delivery proves local input; it does not imply native pointer capture.
pub(super) fn input(
    session: &mut DockspaceSession,
    event: SurfacePointerEvent,
    position: Option<LogicalPoint>,
    docking_enabled: bool,
) -> AppResult<HostFrameReport> {
    let mut frame = session.begin_host_frame()?;
    let hit = position.and_then(|point| {
        frame.host_pointer_hit_for_device(SURFACE, point, PointerDeviceKind::Mouse)
    });
    let surface = hit.map(|hit| hit.surface());
    let delivery = hit.and_then(|hit| hit.delivery());
    let drag = hit.and_then(|hit| hit.drag_delivery());
    let mut facts = SurfacePointerReceiverFacts::unknown();
    let mut route = HostPointerRoute::Unknown;
    if let (Some(surface), Some(point)) = (surface.as_ref(), position) {
        facts = delivery.as_ref().map_or_else(
            || facts.with_no_delivery(surface),
            SurfacePointerReceiverFacts::delivery,
        );
        facts = drag.as_ref().map_or_else(
            || facts.with_no_drag_delivery(surface),
            |receiver| facts.with_drag_delivery(receiver),
        );
        facts = facts.with_clear_hover_lane(surface);
        route = HostPointerRoute::new(*surface, point);
    }
    let manipulation = position
        .zip(frame.host_pointer_manipulation_surface(POINTER))
        .map_or(route, |(point, source)| {
            HostPointerRoute::new(source, point)
        });
    frame.submit_host_pointer(
        HostPointerInput::new(
            POINTER,
            event,
            manipulation,
            route,
            SurfacePointerCapture::None,
            facts,
        )
        .with_metadata(
            PointerEventMetadata::untimed(PointerDeviceKind::Mouse)
                .with_docking_enabled(docking_enabled),
        ),
    )?;
    Ok(frame.commit()?)
}

pub(super) fn cancel(session: &mut DockspaceSession) -> AppResult<HostFrameReport> {
    input(
        session,
        SurfacePointerEvent::StreamCancelled(SurfacePointerCancelReason::ExplicitCancellation),
        None,
        true,
    )
}
