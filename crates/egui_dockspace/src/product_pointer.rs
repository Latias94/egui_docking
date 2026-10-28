//! Ordered transport for the official host-local pointer lane.

use dockspace::geometry::LogicalPoint;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceInteractionError, DockspaceRuntimeError, DockspaceSession, HostFrameReport,
    HostPointerInput, HostPointerRoute, PointerDeviceKind, PointerEventMetadata,
    PresentedDockReceiver, PresentedDockspaceSurface, SurfacePointerButton,
    SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationArtifact, SurfacePresentationCommit,
    SurfaceUnavailableReason,
};
use egui::{Event, Key, LayerId, PointerButton, Pos2, Ui};

use crate::pass_settlement::{ProductPointerPacket, pointer_packet};
use crate::product_render::ProductReceiverBinding;

mod touch;

#[cfg(test)]
mod tests;

use touch::TouchContacts;

struct PointerScene {
    artifact: SurfacePresentationArtifact,
    layer: LayerId,
    completed_pass: u64,
    coordinates_supported: bool,
    receivers: Vec<ProductReceiverBinding>,
}

#[derive(Default)]
pub(crate) struct ProductPointer {
    pending: Vec<PointerScene>,
    presented: Option<PointerScene>,
    consumed_run: Option<ProductPointerPacket>,
    rejected_run: Option<ProductPointerPacket>,
    position: Option<Pos2>,
    // Adapter input enrollment only; core remains the sole gesture owner.
    generic_live: bool,
    touches: TouchContacts,
}

#[derive(Clone, Copy)]
struct PointerSample {
    pointer: SurfacePointerId,
    device: PointerDeviceKind,
    event: SurfacePointerEvent,
    position: Option<Pos2>,
}

impl PointerSample {
    const fn generic(event: SurfacePointerEvent, position: Option<Pos2>) -> Self {
        Self {
            pointer: SurfacePointerId::new(1),
            device: PointerDeviceKind::Unknown,
            event,
            position,
        }
    }
}

#[derive(Clone, Copy)]
struct Edge {
    pointer: SurfacePointerId,
    device: PointerDeviceKind,
    event: SurfacePointerEvent,
    point: Option<LogicalPoint>,
    surface: Option<PresentedDockspaceSurface>,
    delivery: Option<PresentedDockReceiver>,
    drag_delivery: Option<PresentedDockReceiver>,
    blocked: bool,
}

impl ProductPointer {
    pub(crate) fn clear(&mut self) {
        let mut touches = std::mem::take(&mut self.touches);
        touches.clear();
        *self = Self {
            touches,
            ..Self::default()
        };
    }

    pub(crate) fn emitted(
        &mut self,
        artifact: SurfacePresentationArtifact,
        layer: LayerId,
        completed_pass: u64,
        coordinates_supported: bool,
        receivers: Vec<ProductReceiverBinding>,
    ) {
        self.pending.push(PointerScene {
            artifact,
            layer,
            completed_pass,
            coordinates_supported,
            receivers,
        });
    }

    pub(crate) fn observe(&mut self, commits: &[SurfacePresentationCommit]) {
        for scene in std::mem::take(&mut self.pending) {
            if commits.iter().any(|commit| commit.presents(scene.artifact)) {
                self.presented = Some(scene);
            } else if !commits.iter().any(|commit| commit.settles(scene.artifact)) {
                self.pending.push(scene);
            }
        }
    }

    pub(crate) fn submit(
        &mut self,
        session: &mut DockspaceSession,
        surface: SurfaceId,
        ui: &Ui,
        cancel: bool,
    ) -> Result<Vec<HostFrameReport>, DockspaceRuntimeError> {
        let Some(packet) = pointer_packet(ui) else {
            return Ok(Vec::new());
        };
        if self
            .consumed_run
            .as_ref()
            .is_some_and(|previous| previous.same_run(&packet))
        {
            return Ok(Vec::new());
        }
        if self
            .rejected_run
            .as_ref()
            .is_some_and(|previous| previous.same_run(&packet))
        {
            return Err(DockspaceInteractionError::PointerInputAlreadySubmitted.into());
        }
        let current = session.presented_surface(surface);
        let scene = self.presented.as_ref().filter(|scene| {
            current.is_some_and(|surface| surface.matches_presentation_artifact(scene.artifact))
        });
        let identity_layer = |layer| {
            ui.ctx()
                .layer_transform_to_global(layer)
                .is_none_or(|transform| transform == egui::emath::TSTransform::IDENTITY)
        };
        let coordinates_supported = identity_layer(ui.layer_id())
            && scene.is_none_or(|scene| scene.coordinates_supported && identity_layer(scene.layer));
        let rejected_previous = self.rejected_run.is_some();
        // Normalization may also fail. Neither boundary may replay this run
        // with a new contact identity or different receiver facts.
        self.rejected_run = Some(packet.clone());
        let mut position = self.position;
        let mut generic_live = self.generic_live;
        let mut touches = self.touches.clone();
        let touch_packet = !touches.is_empty()
            || packet
                .events()
                .iter()
                .any(|event| matches!(event, Event::Touch { .. }));
        let mut records = Vec::new();
        if cancel || rejected_previous || !coordinates_supported {
            if generic_live {
                records.push(PointerSample::generic(
                    SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    ),
                    position,
                ));
                generic_live = false;
            }
            records.extend(
                touches.cancel_all(SurfacePointerCancelReason::ExplicitPlatformCancellation),
            );
        }
        for event in packet.events().iter().cloned() {
            if touch_packet
                && generic_live
                && matches!(
                    &event,
                    Event::Touch { .. }
                        | Event::PointerMoved(_)
                        | Event::PointerButton { .. }
                        | Event::PointerGone
                )
            {
                // Touch input suppresses ambiguous synthesized generic tails.
                // Retire an enrolled generic stream at that exact raw boundary
                // before its terminal could disappear or a contact requests ownership.
                records.push(PointerSample::generic(
                    SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    ),
                    position,
                ));
                generic_live = false;
            }
            let event = match event {
                Event::Touch {
                    device_id,
                    id,
                    phase,
                    pos,
                    ..
                } => {
                    if coordinates_supported
                        && let Some(sample) = touches.observe(device_id, id, phase, pos)?
                    {
                        records.push(sample);
                    }
                    None
                }
                Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone
                    if touch_packet =>
                {
                    None
                }
                Event::PointerMoved(point) => {
                    position = Some(point);
                    Some(SurfacePointerEvent::Moved)
                }
                Event::PointerButton {
                    pos,
                    button: PointerButton::Primary,
                    pressed,
                    ..
                } => {
                    position = Some(pos);
                    Some(if pressed {
                        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary)
                    } else {
                        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary)
                    })
                }
                Event::PointerGone => {
                    position = None;
                    Some(SurfacePointerEvent::Moved)
                }
                Event::WindowFocused(false) => {
                    records.extend(
                        touches
                            .cancel_all(SurfacePointerCancelReason::ExplicitPlatformCancellation),
                    );
                    Some(SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    ))
                }
                Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => {
                    records.extend(touches.cancel_all(SurfacePointerCancelReason::Escape));
                    Some(SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::Escape,
                    ))
                }
                _ => None,
            };
            if let Some(event) = event {
                if matches!(event, SurfacePointerEvent::StreamCancelled(_)) {
                    if !generic_live {
                        continue;
                    }
                    generic_live = false;
                } else {
                    generic_live = true;
                }
                records.push(PointerSample::generic(event, position));
            }
        }
        if records.is_empty()
            && let Some(request) = session.pointer_continuation()
            && request.surface() == surface
            && packet
                .observed_at()
                .is_some_and(|now| now >= request.deadline())
        {
            // This is an explicit stationary recheck. It does not claim that
            // the pointer moved; the ordinary receiver path below must prove
            // current framework clearance at its retained logical point.
            if let Some(sample) =
                continuation_sample(&touches, position, request.pointer(), request.event())
            {
                records.push(sample);
            }
        }
        if records.is_empty() {
            schedule_pointer_continuation(session, ui, packet.observed_at());
            self.consumed_run = Some(packet);
            self.rejected_run = None;
            return Ok(Vec::new());
        }
        let interactive = ui.ctx().interactive_rects_last_pass();
        let edge_at = |sample: PointerSample, session: &DockspaceSession| {
            let position = sample.position;
            let point = position
                .filter(|_| coordinates_supported)
                .and_then(|position| {
                    LogicalPoint::new(f64::from(position.x), f64::from(position.y)).ok()
                });
            let hit = point.and_then(|point| session.host_pointer_hit(surface, point));
            let qualified = scene.zip(position).is_some_and(|(scene, position)| {
                if !ui.is_enabled()
                    || ui.ctx().cumulative_pass_nr() != scene.completed_pass
                    || ui.ctx().layer_id_at(position) != Some(scene.layer)
                    || !coordinates_supported
                {
                    return false;
                }
                // Unknown interactive widgets are blockers. Remove only exact
                // registrations owned by this output's interactive dock chrome.
                let mut unknown = interactive
                    .iter()
                    .copied()
                    .filter(|rect| rect.contains(position))
                    .collect::<Vec<_>>();
                let mut credited_widgets = std::collections::HashSet::new();
                for binding in scene.receivers.iter().filter(|binding| {
                    binding.interactive
                        && binding.layer_id == scene.layer
                        && ui.ctx().viewport(|viewport| {
                            viewport
                                .prev_pass
                                .widgets
                                .get(binding.widget_id)
                                .is_some_and(|widget| {
                                    widget.enabled
                                        && widget.sense.interactive()
                                        && widget.layer_id == binding.layer_id
                                        && widget.interact_rect == binding.interact_rect
                                })
                        })
                        && credited_widgets.insert(binding.widget_id)
                }) {
                    if let Some(index) = unknown
                        .iter()
                        .position(|rect| *rect == binding.interact_rect)
                    {
                        unknown.remove(index);
                    }
                }
                unknown.is_empty()
            });
            Edge {
                pointer: sample.pointer,
                device: sample.device,
                event: sample.event,
                point,
                surface: hit.map(|hit| hit.surface()),
                delivery: qualified
                    .then(|| hit.and_then(|hit| hit.delivery()))
                    .flatten(),
                drag_delivery: qualified
                    .then(|| hit.and_then(|hit| hit.drag_delivery()))
                    .flatten(),
                blocked: !qualified,
            }
        };
        let edges = records
            .into_iter()
            .map(|sample| edge_at(sample, session))
            .collect::<Vec<_>>();
        let observed_at = packet.observed_at();
        let inputs = edges.iter().map(|edge| pointer_input(edge, observed_at));
        let mut frame = session.begin_host_frame()?;
        frame.submit_host_pointer_batch(inputs)?;
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let report = frame.commit()?;
        // The raw boundary accepted these identities and positions. A later
        // continuation failure must not roll their roster back.
        self.position = position;
        self.generic_live = generic_live;
        self.touches = touches;
        let mut reports = vec![report];
        if let (Some(request), Some(now)) = (session.pointer_continuation(), observed_at)
            && now >= request.deadline()
            && request.surface() == surface
            && current.is_some()
            && current == session.presented_surface(surface)
            && let Some(sample) = continuation_sample(
                &self.touches,
                self.position,
                request.pointer(),
                request.event(),
            )
        {
            // Complete accepted raw input first. A release in that batch ends
            // the gesture before any deadline task can change its target scene.
            // A surviving due action uses the same current framework facts in
            // an explicit next journal edge; moving pointers need not go idle.
            let edge = edge_at(sample, session);
            if edge.point == Some(request.position()) && edge.surface == current {
                let mut frame = session.begin_host_frame()?;
                frame.submit_host_pointer(pointer_input(&edge, observed_at))?;
                frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
                reports.push(frame.commit()?);
            }
        }
        if let Some(scene) = scene {
            for edge in &edges {
                if matches!(
                    edge.event,
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary)
                ) && edge.device == PointerDeviceKind::Unknown
                    && let Some(receiver) = edge.delivery
                    && receiver.descriptor().role()
                        == dockspace::runtime::DockspaceReceiverRole::TabBody
                    && let Some(binding) = scene
                        .receivers
                        .iter()
                        .find(|binding| binding.receiver == receiver.descriptor())
                {
                    ui.ctx()
                        .memory_mut(|memory| memory.request_focus(binding.widget_id));
                }
            }
        }
        schedule_pointer_continuation(session, ui, packet.observed_at());
        self.consumed_run = Some(packet);
        self.rejected_run = None;
        Ok(reports)
    }
}

fn continuation_sample(
    touches: &TouchContacts,
    position: Option<Pos2>,
    pointer: SurfacePointerId,
    event: SurfacePointerEvent,
) -> Option<PointerSample> {
    if pointer == SurfacePointerId::new(1) {
        Some(PointerSample::generic(event, position))
    } else {
        touches.continuation(pointer, event)
    }
}

fn pointer_input(edge: &Edge, observed_at: Option<std::time::Duration>) -> HostPointerInput<'_> {
    let route = edge
        .surface
        .zip(edge.point)
        .map_or(HostPointerRoute::Unknown, |(surface, point)| {
            HostPointerRoute::new(surface, point)
        });
    let mut facts = SurfacePointerReceiverFacts::unknown();
    if let Some(surface) = edge.surface.as_ref() {
        if edge.blocked {
            facts = facts
                .with_blocked_delivery(surface)
                .with_blocked_hover(surface);
        } else {
            facts = edge.delivery.as_ref().map_or_else(
                || facts.with_no_delivery(surface),
                |receiver| facts.with_delivery(receiver),
            );
            facts = edge.drag_delivery.as_ref().map_or_else(
                || facts.with_no_drag_delivery(surface),
                |receiver| facts.with_drag_delivery(receiver),
            );
            facts = facts.with_clear_hover_lane(surface);
        }
    }
    HostPointerInput::new(
        edge.pointer,
        edge.event,
        route,
        route,
        SurfacePointerCapture::None,
        facts,
    )
    .with_metadata(observed_at.map_or_else(
        || PointerEventMetadata::untimed(edge.device),
        |time| PointerEventMetadata::new(edge.device, time),
    ))
}

fn schedule_pointer_continuation(
    session: &DockspaceSession,
    ui: &Ui,
    observed_at: Option<std::time::Duration>,
) {
    if let (Some(request), Some(now)) = (session.pointer_continuation(), observed_at) {
        ui.ctx()
            .request_repaint_after(request.deadline().saturating_sub(now));
    }
}
