//! Ordered transport for the official host-local pointer lane.

use dockspace::geometry::LogicalPoint;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceInteractionError, DockspaceRuntimeError, DockspaceSession, HostFrameReport,
    HostPointerInput, HostPointerRoute, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationArtifact,
    SurfacePresentationCommit, SurfaceUnavailableReason,
};
use egui::{Event, Key, LayerId, PointerButton, Pos2, Ui};

use crate::pass_settlement::{ProductPointerPacket, pointer_packet};
use crate::product_render::ProductReceiverBinding;

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
}

struct Edge {
    event: SurfacePointerEvent,
    point: Option<LogicalPoint>,
    surface: Option<PresentedDockspaceSurface>,
    delivery: Option<PresentedDockReceiver>,
    drag_delivery: Option<PresentedDockReceiver>,
    blocked: bool,
}

impl ProductPointer {
    pub(crate) fn clear(&mut self) {
        *self = Self::default();
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
    ) -> Result<Option<HostFrameReport>, DockspaceRuntimeError> {
        let Some(packet) = pointer_packet(ui) else {
            return Ok(None);
        };
        if self
            .consumed_run
            .as_ref()
            .is_some_and(|previous| previous.same_run(&packet))
        {
            return Ok(None);
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
        let mut position = self.position;
        let mut records = Vec::new();
        if cancel || self.rejected_run.is_some() || !coordinates_supported {
            records.push((
                SurfacePointerEvent::StreamCancelled(
                    SurfacePointerCancelReason::ExplicitPlatformCancellation,
                ),
                position,
            ));
        }
        for event in packet.events().iter().cloned() {
            let event = match event {
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
                Event::Touch {
                    phase: egui::TouchPhase::Cancel,
                    ..
                }
                | Event::WindowFocused(false)
                | Event::Key {
                    key: Key::Escape,
                    pressed: true,
                    ..
                } => Some(SurfacePointerEvent::StreamCancelled(
                    SurfacePointerCancelReason::ExplicitPlatformCancellation,
                )),
                _ => None,
            };
            if let Some(event) = event {
                records.push((event, position));
            }
        }
        if records.is_empty() {
            self.consumed_run = Some(packet);
            return Ok(None);
        }
        let interactive = ui.ctx().interactive_rects_last_pass();
        let edges = records
            .into_iter()
            .map(|(event, position)| {
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
                    event,
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
            })
            .collect::<Vec<_>>();
        let inputs = edges.iter().map(|edge| {
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
                SurfacePointerId::new(1),
                edge.event,
                route,
                route,
                SurfacePointerCapture::None,
                facts,
            )
        });
        // A failed packet must not acquire different facts during an egui rerun.
        // The next run cancels the prior owner before accepting fresh input.
        self.rejected_run = Some(packet.clone());
        let mut frame = session.begin_host_frame()?;
        frame.submit_host_pointer_batch(inputs)?;
        frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
        let report = frame.commit()?;
        if let Some(scene) = scene {
            for edge in &edges {
                if matches!(
                    edge.event,
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary)
                ) && let Some(receiver) = edge.delivery
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
        self.position = position;
        self.consumed_run = Some(packet);
        self.rejected_run = None;
        Ok(Some(report))
    }
}
