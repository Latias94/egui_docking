//! Event-time GPUI clearance and capture translated into the public host pointer lane.

use dockspace::{
    geometry::LogicalPoint,
    runtime::{
        DockspaceHostFrame, HostPointerInput, HostPointerRoute, PointerDeviceKind,
        PointerEventMetadata, PresentedDockReceiver, PresentedDockspaceSurface,
        SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts,
    },
};
use open_gpui::{Bounds, HitboxId, Pixels, Point, Window};

use crate::{HostState, SURFACE};

/// Owned event-time facts; delayed input never re-hits a newer layout.
#[derive(Clone, Copy)]
pub(super) struct QueuedPointer {
    event: SurfacePointerEvent,
    route: HostPointerRoute,
    surface: Option<PresentedDockspaceSurface>,
    delivery: Option<PresentedDockReceiver>,
    drag: Option<PresentedDockReceiver>,
    clear: bool,
    capture: SurfacePointerCapture,
    enabled: bool,
}

impl QueuedPointer {
    pub(super) fn submit(&self, frame: &mut DockspaceHostFrame<'_>) -> anyhow::Result<()> {
        let mut facts = SurfacePointerReceiverFacts::unknown();
        if let Some(surface) = &self.surface {
            if self.clear {
                facts = match &self.delivery {
                    Some(delivery) => facts.with_delivery(delivery),
                    None => facts.with_no_delivery(surface),
                };
                facts = match &self.drag {
                    Some(drag) => facts.with_drag_delivery(drag),
                    None => facts.with_no_drag_delivery(surface),
                };
            } else {
                facts = facts.with_blocked_delivery(surface);
            }
            facts = if self.clear {
                facts.with_clear_hover_lane(surface)
            } else {
                facts.with_blocked_hover(surface)
            };
        }
        frame.submit_host_pointer_batch([HostPointerInput::new(
            SurfacePointerId::new(1),
            self.event,
            self.route,
            self.route,
            self.capture,
            facts,
        )
        .with_metadata(
            PointerEventMetadata::untimed(PointerDeviceKind::Mouse)
                .with_docking_enabled(self.enabled),
        )])?;
        Ok(())
    }
}

pub(super) fn queue(
    host: &mut HostState,
    serial: u64,
    event: SurfacePointerEvent,
    position: Point<Pixels>,
    bounds: Bounds<Pixels>,
    hitboxes: &[HitboxId],
    captured: bool,
    shift: bool,
    window: &Window,
) -> bool {
    let Ok(point) = LogicalPoint::new(
        f64::from(f32::from(position.x - bounds.origin.x)),
        f64::from(f32::from(position.y - bounds.origin.y)),
    ) else {
        return false;
    };
    let clear = hitboxes
        .iter()
        .any(|hitbox| hitbox.window_point_target_rank(position, window) == Some(0));
    let cancellation = matches!(event, SurfacePointerEvent::StreamCancelled(_));
    if !captured && !clear && !cancellation {
        return false;
    }
    let hit = is_presented(host, serial)
        .then(|| {
            host.session
                .host_pointer_hit_for_device(SURFACE, point, PointerDeviceKind::Mouse)
        })
        .flatten();
    if hit.is_none() && !captured && !cancellation {
        return false;
    }
    let surface = hit.map(|hit| hit.surface());
    host.inputs.push(QueuedPointer {
        event,
        route: surface.map_or(HostPointerRoute::Unknown, |surface| {
            HostPointerRoute::new(surface, point)
        }),
        surface,
        delivery: clear.then(|| hit.and_then(|hit| hit.delivery())).flatten(),
        drag: clear
            .then(|| hit.and_then(|hit| hit.drag_delivery()))
            .flatten(),
        clear,
        capture: if captured {
            SurfacePointerCapture::ProviderEndpoint
        } else {
            SurfacePointerCapture::None
        },
        enabled: !shift,
    });
    host.pointer = Some(point);
    host.dirty = true;
    true
}

pub(super) fn is_presented(host: &HostState, serial: u64) -> bool {
    host.presented.is_some_and(|(presented_serial, artifact)| {
        serial == presented_serial
            && host
                .session
                .presented_surface(SURFACE)
                .is_some_and(|surface| surface.matches_presentation_artifact(artifact))
    })
}
