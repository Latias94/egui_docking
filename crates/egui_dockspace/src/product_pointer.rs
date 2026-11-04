//! Ordered transport for the official host-local pointer lane.

use std::collections::HashSet;
use std::time::Duration;

use dockspace::geometry::LogicalPoint;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceInteractionError, DockspacePointerContinuation,
    DockspaceRuntimeError, DockspaceSession, HostPointerInput, HostPointerRoute, PointerDeviceKind,
    PointerEventMetadata, PresentedDockReceiver, PresentedDockspaceSurface, SurfacePointerButton,
    SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationArtifact, SurfacePresentationCommit,
};
use egui::{Context, Event, Id, Key, LayerId, PointerButton, Pos2, Rect, Ui, WidgetRects};

use crate::pass_settlement::{ProductPointerPacket, pointer_packet};
use crate::product_render::ProductReceiverBinding;

mod touch;

#[cfg(test)]
mod tests;

use touch::TouchContacts;

/// Framework receiver authority frozen before any application callback runs.
pub(crate) struct ProductPointerInputSnapshot {
    completed_pass: u64,
    widgets: WidgetRects,
    interactive: Vec<Rect>,
    transformed_layers: HashSet<LayerId>,
    layer_at: Box<dyn Fn(Pos2) -> Option<LayerId> + Send + Sync>,
}

impl ProductPointerInputSnapshot {
    pub(crate) fn capture(context: &Context) -> Self {
        let (transformed_layers, layer_at) = context.memory(|memory| {
            let areas = memory.areas().clone();
            let transforms = memory.to_global.clone();
            let transformed_layers = transforms
                .iter()
                .filter(|(_, transform)| **transform != egui::emath::TSTransform::IDENTITY)
                .map(|(layer, _)| *layer)
                .collect();
            let blocked = memory
                .layer_ids()
                .filter(|layer| !memory.is_above_modal_layer(*layer))
                .collect::<HashSet<_>>();
            let modal = memory.top_modal_layer();
            // Erase the private Areas type while using egui's own hit test.
            // Capturing Memory itself would retain unrelated user data and the run portal.
            let layer_at: Box<dyn Fn(Pos2) -> Option<LayerId> + Send + Sync> =
                Box::new(move |position| {
                    let layer = areas.layer_id_at(position, &transforms)?;
                    if blocked.contains(&layer) {
                        modal
                    } else {
                        Some(layer)
                    }
                });
            (transformed_layers, layer_at)
        });
        Self {
            completed_pass: context.cumulative_pass_nr(),
            widgets: context.viewport(|viewport| viewport.prev_pass.widgets.clone()),
            interactive: context.interactive_rects_last_pass(),
            transformed_layers,
            layer_at,
        }
    }

    fn identity_layer(&self, layer: LayerId) -> bool {
        !self.transformed_layers.contains(&layer)
    }
}

#[derive(Clone)]
struct PointerScene {
    artifact: SurfacePresentationArtifact,
    layer: LayerId,
    completed_pass: u64,
    coordinates_supported: bool,
    receivers: Vec<ProductReceiverBinding>,
    focus_bindings: Vec<(dockspace::model::ItemId, Id)>,
}

#[derive(Clone, Default)]
pub(crate) struct ProductPointer {
    pending: Vec<PointerScene>,
    presented: Option<PointerScene>,
    consumed_run: Option<ProductPointerPacket>,
    rejected_run: Option<ProductPointerPacket>,
    position: Option<Pos2>,
    // Adapter input enrollment only; core remains the sole gesture owner.
    generic_live: bool,
    magnetic_targeting: Option<bool>,
    touches: TouchContacts,
}

#[derive(Clone, Copy)]
struct PointerSample {
    pointer: SurfacePointerId,
    device: PointerDeviceKind,
    event: SurfacePointerEvent,
    position: Option<Pos2>,
    magnetic_targeting: Option<bool>,
}

impl PointerSample {
    const fn generic(
        event: SurfacePointerEvent,
        position: Option<Pos2>,
        magnetic_targeting: Option<bool>,
    ) -> Self {
        Self {
            pointer: SurfacePointerId::new(1),
            device: PointerDeviceKind::Unknown,
            event,
            position,
            magnetic_targeting,
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
    manipulation_surface: Option<PresentedDockspaceSurface>,
    delivery: Option<PresentedDockReceiver>,
    drag_delivery: Option<PresentedDockReceiver>,
    blocked: bool,
    magnetic_targeting: Option<bool>,
}

/// One normalized run, prepared against this update's presented output.
///
/// Prepare on a pointer draft and install that draft only after publication.
/// Normalization errors travel with the affine input, so a failed run cannot
/// be prepared again with different contact identities or receiver facts.
#[must_use = "submit the prepared input into its update before publishing the pointer draft"]
pub(crate) struct PreparedProductPointerInput {
    input: Result<PreparedPointerEdges, DockspaceRuntimeError>,
    observed_at: Option<Duration>,
}

#[derive(Default)]
struct PreparedPointerEdges {
    raw: Vec<Edge>,
    continuation: Option<QualifiedPointerContinuation>,
    deferred_focus: Vec<Id>,
}

struct QualifiedPointerContinuation {
    request: DockspacePointerContinuation,
    edge: Edge,
}

impl PreparedProductPointerInput {
    /// Returns the run's original clock sample for post-publication scheduling.
    pub(crate) const fn observed_at(&self) -> Option<Duration> {
        self.observed_at
    }

    #[cfg(test)]
    pub(crate) fn is_empty(&self) -> bool {
        self.input
            .as_ref()
            .is_ok_and(|input| input.raw.is_empty() && input.continuation.is_none())
    }

    /// Appends raw input and then an exact surviving stationary request.
    ///
    /// The returned widget IDs request focus only after this update publishes.
    /// No framework hit test is performed against post-input geometry. A new
    /// or changed request remains core-owned for the next update; the host must
    /// call `schedule_pointer_continuation` after publication, even when this
    /// input is empty. Raw events must never be replayed to service that wake.
    pub(crate) fn submit_into(
        self,
        frame: &mut DockspaceHostFrame<'_>,
    ) -> Result<Vec<Id>, DockspaceRuntimeError> {
        let input = self.input?;
        if !input.raw.is_empty() {
            frame.submit_host_pointer_batch(
                input
                    .raw
                    .iter()
                    .map(|edge| pointer_input(edge, self.observed_at)),
            )?;
        }
        if let Some(probe) = input.continuation
            && let Some(request) = frame.pointer_continuation()
            && request.pointer() == probe.request.pointer()
            && request.surface() == probe.request.surface()
            && request.deadline() == probe.request.deadline()
            && request.position() == probe.request.position()
            && probe.edge.point == Some(probe.request.position())
            && probe.edge.surface == frame.presented_surface(probe.request.surface())
        {
            // A release or cancellation in the raw prefix revokes the request
            // before this check. Motion can refresh its token while retaining
            // the same dwell deadline. Reuse only the exact pre-input framework
            // proof at the unchanged point and output, with the current token.
            let edge = Edge {
                event: request.event(),
                ..probe.edge
            };
            frame.submit_host_pointer(pointer_input(&edge, self.observed_at))?;
        }
        Ok(input.deferred_focus)
    }
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
        focus_bindings: Vec<(dockspace::model::ItemId, Id)>,
    ) {
        self.pending.push(PointerScene {
            artifact,
            layer,
            completed_pass,
            coordinates_supported,
            receivers,
            focus_bindings,
        });
    }

    pub(crate) fn presented_focus_bindings(
        &self,
        frame: &DockspaceHostFrame<'_>,
        surface: SurfaceId,
    ) -> &[(dockspace::model::ItemId, Id)] {
        let Some(current) = frame.presented_surface(surface) else {
            return &[];
        };
        self.presented
            .iter()
            .chain(self.pending.iter())
            .find(|scene| current.matches_presentation_artifact(scene.artifact))
            .map_or(&[], |scene| scene.focus_bindings.as_slice())
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

    fn sync_presented(&mut self, frame: &DockspaceHostFrame<'_>, surface: SurfaceId) {
        let Some(current) = frame.presented_surface(surface) else {
            return;
        };
        if self
            .presented
            .as_ref()
            .is_some_and(|scene| current.matches_presentation_artifact(scene.artifact))
        {
            return;
        }
        if let Some(scene) = self
            .pending
            .iter()
            .rev()
            .find(|scene| current.matches_presentation_artifact(scene.artifact))
        {
            // The prelude may have accepted this artifact without publishing
            // it yet. Keep pending intact until the committed report settles it.
            self.presented = Some(scene.clone());
        }
    }

    pub(crate) fn prepare(
        &mut self,
        frame: &DockspaceHostFrame<'_>,
        surface: SurfaceId,
        ui: &Ui,
        cancel: bool,
    ) -> PreparedProductPointerInput {
        self.sync_presented(frame, surface);
        let Some(packet) = pointer_packet(ui) else {
            return PreparedProductPointerInput {
                input: Ok(PreparedPointerEdges::default()),
                observed_at: None,
            };
        };
        let observed_at = packet.observed_at();
        let input = if self
            .consumed_run
            .as_ref()
            .is_some_and(|previous| previous.same_run(&packet))
        {
            Ok(PreparedPointerEdges::default())
        } else if self
            .rejected_run
            .as_ref()
            .is_some_and(|previous| previous.same_run(&packet))
        {
            Err(DockspaceInteractionError::PointerInputAlreadySubmitted.into())
        } else {
            let input = self.prepare_packet(frame, surface, ui, cancel, &packet);
            if input.is_ok() {
                self.consumed_run = Some(packet);
                self.rejected_run = None;
            }
            input
        };
        PreparedProductPointerInput { input, observed_at }
    }

    /// A failed frame may not reinterpret the same raw run on a later pass.
    pub(crate) fn record_rejected_run(&mut self, draft: &Self) {
        if let Some(packet) = draft.rejected_run.as_ref().or(draft.consumed_run.as_ref())
            && self
                .consumed_run
                .as_ref()
                .is_none_or(|previous| !previous.same_run(packet))
        {
            self.rejected_run = Some(packet.clone());
        }
    }

    fn prepare_packet(
        &mut self,
        frame: &DockspaceHostFrame<'_>,
        surface: SurfaceId,
        ui: &Ui,
        cancel: bool,
        packet: &ProductPointerPacket,
    ) -> Result<PreparedPointerEdges, DockspaceRuntimeError> {
        let current = frame.presented_surface(surface);
        let scene = self.presented.as_ref().filter(|scene| {
            current.is_some_and(|surface| surface.matches_presentation_artifact(scene.artifact))
        });
        let snapshot = packet.receivers();
        let current_identity_layer = |layer| {
            ui.ctx()
                .layer_transform_to_global(layer)
                .is_none_or(|transform| transform == egui::emath::TSTransform::IDENTITY)
        };
        let coordinates_supported = snapshot.identity_layer(ui.layer_id())
            && current_identity_layer(ui.layer_id())
            && scene.is_none_or(|scene| {
                scene.coordinates_supported
                    && snapshot.identity_layer(scene.layer)
                    && current_identity_layer(scene.layer)
            });
        let rejected_previous = self.rejected_run.is_some();
        // Normalization may also fail. Neither boundary may replay this run
        // with a new contact identity or different receiver facts.
        self.rejected_run = Some(packet.clone());
        let mut position = self.position;
        let mut generic_live = self.generic_live;
        let mut magnetic_targeting = self.magnetic_targeting;
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
                    magnetic_targeting,
                ));
                generic_live = false;
            }
            records.extend(
                touches.cancel_all(SurfacePointerCancelReason::ExplicitPlatformCancellation),
            );
        }
        for event in packet.events().iter().cloned() {
            if let Some(enabled) = update_magnetic_targeting(&mut magnetic_targeting, &event)
                && generic_live
            {
                records.push(PointerSample::generic(
                    SurfacePointerEvent::MagneticTargetingChanged { enabled },
                    None,
                    magnetic_targeting,
                ));
            }
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
                    magnetic_targeting,
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
                records.push(PointerSample::generic(event, position, magnetic_targeting));
            }
        }
        let edge_at = |sample: PointerSample| {
            if matches!(
                sample.event,
                SurfacePointerEvent::MagneticTargetingChanged { .. }
            ) {
                // A semantic marker has no coordinate or receiver observation.
                return Edge {
                    pointer: sample.pointer,
                    device: sample.device,
                    event: sample.event,
                    point: None,
                    surface: None,
                    manipulation_surface: None,
                    delivery: None,
                    drag_delivery: None,
                    blocked: false,
                    magnetic_targeting: sample.magnetic_targeting,
                };
            }
            let position = sample.position;
            let point = position
                .filter(|_| coordinates_supported)
                .and_then(|position| {
                    LogicalPoint::new(f64::from(position.x), f64::from(position.y)).ok()
                });
            let hit = point
                .and_then(|point| frame.host_pointer_hit_for_device(surface, point, sample.device));
            let qualified = scene.zip(position).is_some_and(|(scene, position)| {
                if !ui.is_enabled()
                    || snapshot.completed_pass != scene.completed_pass
                    || (snapshot.layer_at)(position) != Some(scene.layer)
                    || !coordinates_supported
                {
                    return false;
                }
                // Unknown interactive widgets are blockers. Remove only exact
                // registrations owned by this output's interactive dock chrome.
                let mut unknown = snapshot
                    .interactive
                    .iter()
                    .copied()
                    .filter(|rect| rect.contains(position))
                    .collect::<Vec<_>>();
                let mut credited_widgets = std::collections::HashSet::new();
                for binding in scene.receivers.iter().filter(|binding| {
                    binding.interactive
                        && binding.layer_id == scene.layer
                        && snapshot
                            .widgets
                            .get(binding.widget_id)
                            .is_some_and(|widget| {
                                widget.enabled
                                    && widget.sense.interactive()
                                    && widget.layer_id == binding.layer_id
                                    && widget.interact_rect == binding.interact_rect
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
                manipulation_surface: frame
                    .host_pointer_manipulation_surface(sample.pointer)
                    .filter(|source| coordinates_supported && source.surface() == surface)
                    .or_else(|| hit.map(|hit| hit.surface())),
                delivery: qualified
                    .then(|| hit.and_then(|hit| hit.delivery()))
                    .flatten(),
                drag_delivery: qualified
                    .then(|| hit.and_then(|hit| hit.drag_delivery()))
                    .flatten(),
                blocked: !qualified,
                magnetic_targeting: sample.magnetic_targeting,
            }
        };
        let edges = records.into_iter().map(&edge_at).collect::<Vec<_>>();
        let continuation = frame.pointer_continuation().and_then(|request| {
            if request.surface() != surface
                || !packet
                    .observed_at()
                    .is_some_and(|now| now >= request.deadline())
            {
                return None;
            }
            let sample = continuation_sample(
                &touches,
                position,
                magnetic_targeting,
                request.pointer(),
                request.event(),
            )?;
            let edge = edge_at(sample);
            (!edge.blocked
                && edge.point == Some(request.position())
                && edge.surface.is_some()
                && edge.surface == current)
                .then_some(QualifiedPointerContinuation { request, edge })
        });
        let mut deferred_focus = Vec::new();
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
                    deferred_focus.push(binding.widget_id);
                }
            }
        }
        self.position = position;
        self.generic_live = generic_live;
        self.magnetic_targeting = magnetic_targeting;
        self.touches = touches;
        Ok(PreparedPointerEdges {
            raw: edges,
            continuation,
            deferred_focus,
        })
    }
}

fn continuation_sample(
    touches: &TouchContacts,
    position: Option<Pos2>,
    magnetic_targeting: Option<bool>,
    pointer: SurfacePointerId,
    event: SurfacePointerEvent,
) -> Option<PointerSample> {
    if pointer == SurfacePointerId::new(1) {
        Some(PointerSample::generic(event, position, magnetic_targeting))
    } else {
        touches.continuation(pointer, event)
    }
}

fn update_magnetic_targeting(current: &mut Option<bool>, event: &Event) -> Option<bool> {
    let modifiers = match event {
        Event::ModifiersChanged(modifiers)
        | Event::Key { modifiers, .. }
        | Event::PointerButton { modifiers, .. } => modifiers,
        _ => return None,
    };
    let enabled = !modifiers.shift;
    let changed = *current != Some(enabled);
    *current = Some(enabled);
    changed.then_some(enabled)
}

fn pointer_input(edge: &Edge, observed_at: Option<Duration>) -> HostPointerInput<'_> {
    let route = edge
        .surface
        .zip(edge.point)
        .map_or(HostPointerRoute::Unknown, |(surface, point)| {
            HostPointerRoute::new(surface, point)
        });
    let manipulation = edge
        .manipulation_surface
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
    let mut metadata = observed_at.map_or_else(
        || PointerEventMetadata::untimed(edge.device),
        |time| PointerEventMetadata::new(edge.device, time),
    );
    if let Some(enabled) = edge.magnetic_targeting {
        metadata = metadata.with_magnetic_targeting(enabled);
    }
    HostPointerInput::new(
        edge.pointer,
        edge.event,
        manipulation,
        route,
        SurfacePointerCapture::None,
        facts,
    )
    .with_metadata(metadata)
}

/// Schedules retained stationary work from the state that actually published.
///
/// Call after every successful update, including one that could not safely
/// append a continuation. The next run rechecks fresh output facts without
/// replaying the previous run's raw packet.
pub(crate) fn schedule_pointer_continuation(
    session: &DockspaceSession,
    context: &egui::Context,
    observed_at: Option<Duration>,
) {
    if let (Some(request), Some(now)) = (session.pointer_continuation(), observed_at) {
        context.request_repaint_after(request.deadline().saturating_sub(now));
    }
}
