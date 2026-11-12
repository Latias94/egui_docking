//! Ordered transport for the official host-local pointer lane.

use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
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
use egui::{Context, Event, Id, LayerId, Modifiers, PointerButton, Pos2, Rect, Ui, WidgetRects};

use crate::input::DockInputConfig;
use crate::pass_settlement::{ProductPointerPacket, pointer_packet};
use crate::product_focus::DeferredTabFocus;
use crate::product_render::ProductReceiverBinding;

mod touch;

#[cfg(test)]
mod tests;

#[cfg(test)]
mod ownership_tests;

use touch::TouchContacts;

/// Framework receiver authority frozen before any application callback runs.
pub(crate) struct ProductPointerInputSnapshot {
    completed_pass: u64,
    widgets: WidgetRects,
    interactive: Vec<Rect>,
    transformed_layers: HashSet<LayerId>,
    layer_at: Box<dyn Fn(Pos2) -> Option<LayerId> + Send + Sync>,
    foreign_drag: bool,
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
            foreign_drag: has_foreign_drag(context),
        }
    }

    fn identity_layer(&self, layer: LayerId) -> bool {
        !self.transformed_layers.contains(&layer)
    }

    /// Subtracts exact owned registrations once for this packet's frozen output.
    fn unowned_interactive_rects(&self, scene: &PointerScene) -> Vec<Rect> {
        #[cfg(test)]
        ownership_tests::record_join(scene.receivers.len(), self.interactive.len());

        let mut credited_widgets = HashSet::new();
        let mut owned_rectangles = HashMap::<[u32; 4], usize>::new();
        for binding in scene.receivers.iter().filter(|binding| {
            binding.interactive
                && binding.layer_id == scene.layer
                && self.widgets.get(binding.widget_id).is_some_and(|widget| {
                    widget.enabled
                        && widget.sense.interactive()
                        && widget.layer_id == binding.layer_id
                        && widget.interact_rect == binding.interact_rect
                })
                && credited_widgets.insert(binding.widget_id)
        }) {
            if let Some(key) = rectangle_key(binding.interact_rect) {
                *owned_rectangles.entry(key).or_default() += 1;
            }
        }
        self.interactive
            .iter()
            .copied()
            .filter(|rect| {
                let count = rectangle_key(*rect).and_then(|key| owned_rectangles.get_mut(&key));
                if let Some(count) = count
                    && *count > 0
                {
                    *count -= 1;
                    false
                } else {
                    true
                }
            })
            .collect()
    }
}

fn rectangle_key(rect: Rect) -> Option<[u32; 4]> {
    let values = [rect.min.x, rect.min.y, rect.max.x, rect.max.y];
    // Rect uses float PartialEq: NaNs never match and both signed zeros are equal.
    // Keep those semantics instead of treating raw float bits as Rect equality.
    if values.iter().any(|value| value.is_nan()) {
        return None;
    }
    Some(values.map(|value| {
        let bits = value.to_bits();
        if bits == (-0.0_f32).to_bits() {
            0
        } else {
            bits
        }
    }))
}

#[derive(Clone)]
struct PointerScene {
    artifact: SurfacePresentationArtifact,
    layer: LayerId,
    completed_pass: u64,
    coordinates_supported: bool,
    receivers: Arc<[ProductReceiverBinding]>,
    focus_bindings: Arc<[(dockspace::model::ItemId, Id)]>,
}

#[derive(Clone, Default)]
pub(crate) struct ProductPointer {
    config: DockInputConfig,
    pending: Vec<PointerScene>,
    presented: Option<PointerScene>,
    consumed_run: Option<ProductPointerPacket>,
    rejected_run: Option<ProductPointerPacket>,
    position: Option<Pos2>,
    // Adapter input enrollment only; core remains the sole gesture owner.
    generic_live: bool,
    observed_modifiers: Option<Modifiers>,
    docking_enabled: Option<bool>,
    foreign_capture: bool,
    touches: TouchContacts,
}

#[derive(Clone, Copy)]
struct PointerSample {
    pointer: SurfacePointerId,
    device: PointerDeviceKind,
    event: SurfacePointerEvent,
    position: Option<Pos2>,
    docking_enabled: Option<bool>,
}

impl PointerSample {
    const fn generic(
        event: SurfacePointerEvent,
        position: Option<Pos2>,
        docking_enabled: Option<bool>,
    ) -> Self {
        Self {
            pointer: SurfacePointerId::new(1),
            device: PointerDeviceKind::Unknown,
            event,
            position,
            docking_enabled,
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
    docking_enabled: Option<bool>,
    capture: SurfacePointerCapture,
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
    deferred_focus: DeferredTabFocus,
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
    /// The returned intent applies framework tab focus once and preserves whether
    /// this input handled a tab press, even if a later application press replaced
    /// its focus destination. It does not acknowledge core presentation. No new
    /// hit test selects a destination against post-input geometry. The host must
    /// call `schedule_pointer_continuation` after publication, even when this
    /// input is empty. Raw events must never be replayed to service that wake.
    pub(crate) fn submit_into(
        self,
        frame: &mut DockspaceHostFrame<'_>,
    ) -> Result<DeferredTabFocus, DockspaceRuntimeError> {
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
    pub(crate) const fn input(&self) -> DockInputConfig {
        self.config
    }

    pub(crate) fn set_input(&mut self, config: DockInputConfig) {
        self.config = config;
    }

    pub(crate) fn clear(&mut self) {
        let mut touches = std::mem::take(&mut self.touches);
        touches.clear();
        *self = Self {
            config: self.config,
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
            receivers: receivers.into(),
            focus_bindings: focus_bindings.into(),
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
            .map_or(&[], |scene| scene.focus_bindings.as_ref())
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
            let input = self.prepare_packet(frame, surface, ui, &packet);
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
        packet: &ProductPointerPacket,
    ) -> Result<PreparedPointerEdges, DockspaceRuntimeError> {
        let current = frame.presented_surface(surface);
        let scene = self.presented.as_ref().filter(|scene| {
            current.is_some_and(|surface| surface.matches_presentation_artifact(scene.artifact))
        });
        let snapshot = packet.receivers();
        // Keep original-run ownership even when an earlier application callback
        // consumes the payload. Also honor a source started before this call.
        let foreign_drag = snapshot.foreign_drag || has_foreign_drag(ui.ctx());
        let capture = if foreign_drag {
            SurfacePointerCapture::Foreign
        } else {
            SurfacePointerCapture::None
        };
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
        let mut observed_modifiers = self.observed_modifiers;
        let mut docking_enabled = self.docking_enabled;
        let mut touches = self.touches.clone();
        let touch_packet = !touches.is_empty()
            || packet
                .events()
                .iter()
                .any(|event| matches!(event, Event::Touch { .. }));
        let mut records = Vec::new();
        if rejected_previous || !coordinates_supported {
            if generic_live {
                records.push(PointerSample::generic(
                    SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    ),
                    position,
                    docking_enabled,
                ));
                generic_live = false;
            }
            records.extend(touches.cancel_all(
                SurfacePointerCancelReason::ExplicitPlatformCancellation,
                docking_enabled,
            ));
        }
        if foreign_drag && !self.foreign_capture {
            if generic_live {
                records.push(PointerSample::generic(
                    SurfacePointerEvent::CaptureChanged,
                    position,
                    docking_enabled,
                ));
            }
            records.extend(touches.capture_changed(docking_enabled));
        }
        // Re-evaluate the last actual modifier observation after a configuration
        // change, without fabricating pointer motion or enrolling a new stream.
        if let Some(enabled) = update_docking_enabled(
            &mut docking_enabled,
            self.config,
            observed_modifiers.unwrap_or(Modifiers::NONE),
        ) {
            records.extend(docking_change_samples(&touches, generic_live, enabled));
        }
        for event in packet.events().iter().cloned() {
            if let Some(modifiers) = event_modifiers(&event) {
                observed_modifiers = Some(modifiers);
                if let Some(enabled) =
                    update_docking_enabled(&mut docking_enabled, self.config, modifiers)
                {
                    records.extend(docking_change_samples(&touches, generic_live, enabled));
                }
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
                    docking_enabled,
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
                        && let Some(sample) =
                            touches.observe(device_id, id, phase, pos, docking_enabled)?
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
                    records.extend(touches.cancel_all(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                        docking_enabled,
                    ));
                    Some(SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    ))
                }
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if self.config.cancel_matches(key, modifiers) => {
                    records.extend(touches.cancel_all(
                        SurfacePointerCancelReason::ExplicitCancellation,
                        docking_enabled,
                    ));
                    Some(SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitCancellation,
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
                records.push(PointerSample::generic(event, position, docking_enabled));
            }
        }
        // Empty packets and blocked layers need no join. All qualified edges
        // and stationary probes in this packet share the same frozen result.
        let blockers = OnceCell::new();
        let edge_at = |sample: PointerSample| {
            if matches!(
                sample.event,
                SurfacePointerEvent::DockingEnabledChanged { .. }
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
                    docking_enabled: sample.docking_enabled,
                    capture,
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
                !foreign_drag
                    && ui.is_enabled()
                    && snapshot.completed_pass == scene.completed_pass
                    && (snapshot.layer_at)(position) == Some(scene.layer)
                    && coordinates_supported
                    && !blockers
                        .get_or_init(|| snapshot.unowned_interactive_rects(scene))
                        .iter()
                        .any(|rect| rect.contains(position))
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
                docking_enabled: sample.docking_enabled,
                capture,
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
                docking_enabled,
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
        let mut deferred_focus = DeferredTabFocus::default();
        if let Some(scene) = scene {
            for edge in &edges {
                if !matches!(
                    edge.event,
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary)
                ) {
                    continue;
                }
                let target = if edge.device == PointerDeviceKind::Unknown
                    && let Some(receiver) = edge.delivery
                    && receiver.descriptor().role()
                        == dockspace::runtime::DockspaceReceiverRole::TabBody
                    && let Some(binding) = scene
                        .receivers
                        .iter()
                        .find(|binding| binding.receiver == receiver.descriptor())
                {
                    Some(binding.widget_id)
                } else {
                    None
                };
                deferred_focus.observe_primary_press(target);
            }
        }
        self.position = position;
        self.generic_live = generic_live;
        self.observed_modifiers = observed_modifiers;
        self.docking_enabled = docking_enabled;
        self.foreign_capture = foreign_drag;
        self.touches = touches;
        Ok(PreparedPointerEdges {
            raw: edges,
            continuation,
            deferred_focus,
        })
    }
}

fn has_foreign_drag(context: &Context) -> bool {
    egui::DragAndDrop::has_any_payload(context)
        || context.input(|input| {
            !input.raw.hovered_files.is_empty() || !input.raw.dropped_files.is_empty()
        })
}

fn continuation_sample(
    touches: &TouchContacts,
    position: Option<Pos2>,
    docking_enabled: Option<bool>,
    pointer: SurfacePointerId,
    event: SurfacePointerEvent,
) -> Option<PointerSample> {
    if pointer == SurfacePointerId::new(1) {
        Some(PointerSample::generic(event, position, docking_enabled))
    } else {
        touches.continuation(pointer, event, docking_enabled)
    }
}

fn event_modifiers(event: &Event) -> Option<Modifiers> {
    match event {
        Event::ModifiersChanged(modifiers)
        | Event::Key { modifiers, .. }
        | Event::PointerButton { modifiers, .. } => Some(*modifiers),
        _ => None,
    }
}

fn update_docking_enabled(
    current: &mut Option<bool>,
    config: DockInputConfig,
    modifiers: Modifiers,
) -> Option<bool> {
    let enabled = config.docking_enabled(modifiers);
    let changed = *current != Some(enabled);
    *current = Some(enabled);
    changed.then_some(enabled)
}

fn docking_change_samples(
    touches: &TouchContacts,
    generic_live: bool,
    enabled: bool,
) -> impl Iterator<Item = PointerSample> {
    generic_live
        .then(|| {
            PointerSample::generic(
                SurfacePointerEvent::DockingEnabledChanged { enabled },
                None,
                Some(enabled),
            )
        })
        .into_iter()
        .chain(touches.docking_enabled_changed(enabled))
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
    if let Some(enabled) = edge.docking_enabled {
        metadata = metadata.with_docking_enabled(enabled);
    }
    HostPointerInput::new(
        edge.pointer,
        edge.event,
        manipulation,
        route,
        edge.capture,
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
    viewport: egui::ViewportId,
    observed_at: Option<Duration>,
) {
    if let (Some(request), Some(now)) = (session.pointer_continuation(), observed_at) {
        context.request_repaint_after_for(request.deadline().saturating_sub(now), viewport);
    }
}
