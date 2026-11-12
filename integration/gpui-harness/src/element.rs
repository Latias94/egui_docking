//! A single retained Element consuming the core's complete ordered paint program.

use std::rc::Rc;

use anyhow::{Context as _, Result, anyhow};
use dockspace::{
    geometry::{LogicalRect, LogicalSize},
    model::ItemId,
    runtime::{
        DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceUpdate,
        SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Request,
        SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerEvent, SurfaceWork,
        SurfaceWorkDeferred, TabStripMetrics,
    },
};
use open_gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, HitboxBehavior,
    HitboxId, Hsla, InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, PointerCaptureHandle, Style, TextRun,
    Window, div, fill, point, prelude::*, px, relative, size,
};

use crate::{HostState, SharedHost, continuation::InputFrame, input, presentation};

mod recording;

use recording::record;

pub(super) struct Packet {
    draws: Vec<Draw>,
}

impl Packet {
    pub(super) fn contains_pane(&self, item: ItemId) -> bool {
        self.pane_bounds(item).is_some()
    }

    pub(super) fn pane_bounds(&self, item: ItemId) -> Option<LogicalRect> {
        self.draws.iter().find_map(|draw| match draw {
            Draw::Pane(bounds, candidate) if *candidate == item => Some(*bounds),
            _ => None,
        })
    }

    pub(super) fn tab_bounds(&self, item: ItemId) -> Option<LogicalRect> {
        self.draws.iter().find_map(|draw| match draw {
            Draw::Tab(bounds, candidate, _) if *candidate == item => Some(*bounds),
            _ => None,
        })
    }

    pub(super) fn receiver_bounds(&self, role: DockspaceReceiverRole) -> Option<LogicalRect> {
        self.draws.iter().find_map(|draw| match draw {
            Draw::Receiver(receiver) if receiver.role() == role => Some(receiver.bounds()),
            _ => None,
        })
    }
}

enum Draw {
    Fill(LogicalRect, Hsla),
    Tab(LogicalRect, ItemId, Hsla),
    Outline(LogicalRect, Hsla),
    Text(LogicalRect, String, Hsla),
    TabText(LogicalRect, ItemId, String, Hsla),
    Pane(LogicalRect, ItemId),
    Receiver(DockspaceReceiverDescriptor),
    Preview(LogicalRect),
}

enum PreparedDraw {
    Fill(Bounds<Pixels>, Hsla),
    Outline(Bounds<Pixels>, Hsla),
    Element(AnyElement),
    Preview(Bounds<Pixels>, Hsla, Hsla),
}

pub(crate) struct DockspaceElement {
    host: SharedHost,
}

pub(crate) struct PreparedFrame {
    draws: Vec<PreparedDraw>,
    hitboxes: Vec<HitboxId>,
    capture: PointerCaptureHandle,
    serial: u64,
    bounds: Bounds<Pixels>,
}

pub(crate) enum PreparedState {
    Frame(PreparedFrame),
    Failure(AnyElement),
}

impl DockspaceElement {
    pub(super) fn new(host: SharedHost) -> Self {
        Self { host }
    }
}

impl IntoElement for DockspaceElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for DockspaceElement {
    type RequestLayoutState = ();
    type PrepaintState = PreparedState;

    fn id(&self) -> Option<ElementId> {
        Some(ElementId::named_usize(
            "dockspace-gpui-host",
            Rc::as_ptr(&self.host) as usize,
        ))
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let style = Style {
            size: size(relative(1.0).into(), relative(1.0).into()),
            flex_grow: 1.0,
            ..Default::default()
        };
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PreparedState {
        let mount = self.host.borrow_mut().ensure_mount(id, window);
        match mount {
            Ok(true) => {
                let weak = Rc::downgrade(&self.host);
                let owner = window.window_handle().window_id();
                cx.on_window_closed(move |_, closed| {
                    if closed == owner {
                        if let Some(host) = weak.upgrade() {
                            presentation::close(&mut host.borrow_mut());
                        }
                    }
                })
                .detach();
            }
            Ok(false) => {}
            Err(error) => return self.failure(error, bounds, window, cx),
        }
        let capture = *self
            .host
            .borrow_mut()
            .capture
            .get_or_insert_with(|| window.new_pointer_capture_handle());
        // Claim the framework's once-per-frame capture binding before touching
        // a pending core candidate. Failed duplicate mounts retract their hitbox.
        let background = window.transact(|window| {
            let background = window.insert_hitbox(bounds, HitboxBehavior::Normal);
            window.bind_pointer_capture(&capture, background.id)?;
            Ok::<_, open_gpui::PointerCaptureError>(background.id)
        });
        let background = match background {
            Ok(background) => background,
            Err(error) => return self.failure(error, bounds, window, cx),
        };
        window.set_focus_handle(&self.host.borrow().focus, cx);
        match self.prepare(bounds, background, capture, window, cx) {
            Ok(prepared) => PreparedState::Frame(prepared),
            Err(error) => {
                self.host.borrow_mut().fail(&error);
                self.failure(error, bounds, window, cx)
            }
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request: &mut (),
        prepared: &mut PreparedState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let background = self.host.borrow().config.colors.background;
        window.paint_quad(fill(bounds, background));
        let prepared = match prepared {
            PreparedState::Frame(prepared) => prepared,
            PreparedState::Failure(error) => {
                error.paint(window, cx);
                return;
            }
        };
        if let Some(probe) = &self.host.borrow().surface_probe {
            probe.set(Some(bounds));
        }
        // Child paint changes GPUI's active dispatch node. Register this
        // element's listeners while its own node is still active.
        self.listen(prepared, window);
        // Keep the core order, including pane widgets between chrome and overlays.
        for draw in &mut prepared.draws {
            match draw {
                PreparedDraw::Fill(bounds, color) => window.paint_quad(fill(*bounds, *color)),
                PreparedDraw::Outline(bounds, color) => outline(*bounds, *color, window),
                PreparedDraw::Preview(bounds, color, border) => {
                    window.paint_quad(fill(*bounds, *color));
                    outline(*bounds, *border, window);
                }
                PreparedDraw::Element(element) => element.paint(window, cx),
            }
        }
    }
}

impl DockspaceElement {
    fn failure(
        &self,
        error: impl std::fmt::Display,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> PreparedState {
        let config = self.host.borrow().config.clone();
        let mut diagnostic = div()
            .w(bounds.size.width)
            .h(bounds.size.height)
            .overflow_hidden()
            .font(config.font)
            .text_size(config.text_size)
            .text_color(config.colors.text)
            .child(format!("Cannot mount dockspace: {error}"))
            .into_any_element();
        diagnostic.prepaint_as_root(bounds.origin, definite(bounds), window, cx);
        PreparedState::Failure(diagnostic)
    }

    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        background: HitboxId,
        capture: PointerCaptureHandle,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<PreparedFrame> {
        let logical = LogicalRect::new(
            0.0,
            0.0,
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        )?;
        let (packet, serial, publication) = {
            let mut host = self.host.borrow_mut();
            presentation::abandon_unaccepted(&mut host, window);
            let publication = host.publication;
            if let Some(pending) = &host.pending {
                (pending.packet.clone(), pending.serial, publication)
            } else if !host.dirty && host.inputs.is_empty() && host.bounds == Some(logical) {
                (
                    host.packet
                        .clone()
                        .context("a reusable frame has a retained packet")?,
                    host.serial,
                    publication,
                )
            } else {
                let inputs = std::mem::take(&mut host.inputs);
                let candidate = build_candidate(&mut host, &inputs, logical, window);
                let (packet, update) = match candidate {
                    Ok(candidate) => candidate,
                    Err(error) => {
                        let mut inputs = inputs;
                        inputs.append(&mut host.inputs);
                        host.inputs = inputs;
                        return Err(error);
                    }
                };
                let ticket = match window.begin_exact_frame_submission(cx) {
                    Ok(ticket) => ticket,
                    Err(error) => {
                        drop(update);
                        let mut inputs = inputs;
                        inputs.append(&mut host.inputs);
                        host.inputs = inputs;
                        return Err(error);
                    }
                };
                host.serial = host
                    .serial
                    .checked_add(1)
                    .context("frame serial exhausted")?;
                let serial = host.serial;
                host.bounds = Some(logical);
                host.dirty = false;
                host.pending = Some(presentation::PendingSubmission::new(
                    serial,
                    ticket.clone(),
                    packet.clone(),
                    update,
                    inputs,
                ));
                presentation::observe(&self.host, &ticket, serial, window);
                (packet, serial, publication)
            }
        };

        let mut hitboxes = vec![background];
        let mut draws = Vec::new();
        let config = self.host.borrow().config.clone();
        for draw in &packet.draws {
            match draw {
                Draw::Fill(rect, color) | Draw::Tab(rect, _, color) => {
                    draws.push(PreparedDraw::Fill(to_gpui(*rect, bounds), *color))
                }
                Draw::Outline(rect, color) => {
                    draws.push(PreparedDraw::Outline(to_gpui(*rect, bounds), *color))
                }
                Draw::Preview(rect) => draws.push(PreparedDraw::Preview(
                    to_gpui(*rect, bounds),
                    config.colors.preview,
                    config.colors.accent,
                )),
                Draw::Receiver(receiver) => {
                    let hitbox = window
                        .insert_hitbox(to_gpui(receiver.bounds(), bounds), HitboxBehavior::Normal);
                    hitboxes.push(hitbox.id);
                }
                Draw::Text(rect, title, color) | Draw::TabText(rect, _, title, color) => {
                    let rect = to_gpui(*rect, bounds);
                    let mut element = div()
                        .w(rect.size.width)
                        .h(rect.size.height)
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .font(config.font.clone())
                        .text_size(config.text_size)
                        .text_color(*color)
                        .when_some(
                            match draw {
                                Draw::TabText(_, item, _, _) => Some(*item),
                                _ => None,
                            },
                            |text, item| {
                                text.debug_selector(move || format!("dockspace:tab:{}", item.get()))
                            },
                        )
                        .child(title.clone())
                        .into_any_element();
                    element.prepaint_as_root(rect.origin, definite(rect), window, cx);
                    draws.push(PreparedDraw::Element(element));
                }
                Draw::Pane(rect, item) => {
                    let rect = to_gpui(*rect, bounds);
                    let panel = self.host.borrow().panels.get(item).cloned();
                    let mut element = match panel {
                        Some(panel) => div()
                            .id(format!("pane-{}", item.get()))
                            .w(rect.size.width)
                            .h(rect.size.height)
                            .overflow_hidden()
                            .child(panel.view)
                            .into_any_element(),
                        None => div()
                            .w(rect.size.width)
                            .h(rect.size.height)
                            .child("Pane unavailable")
                            .into_any_element(),
                    };
                    element.prepaint_as_root(rect.origin, definite(rect), window, cx);
                    draws.push(PreparedDraw::Element(element));
                }
            }
        }

        let shared = self.host.clone();
        let discard = self.host.clone();
        let input_frame = InputFrame {
            serial,
            bounds,
            hitboxes: hitboxes.clone(),
            capture,
        };
        window.record_prepaint_window_transaction(
            publication,
            move |fence, window, cx| {
                presentation::accept(&shared, serial, input_frame.clone(), fence, window, cx)
            },
            move |_, window, _| presentation::discard(&discard, window),
        );
        let shared = self.host.clone();
        window.record_prepaint_focus_stable_commit(move |_, window, cx| {
            presentation::accept_focus_stable(&shared, serial, window, cx);
        });
        Ok(PreparedFrame {
            draws,
            hitboxes,
            capture,
            serial,
            bounds,
        })
    }

    fn listen(&self, prepared: &PreparedFrame, window: &mut Window) {
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        let capture = prepared.capture;
        let serial = prepared.serial;
        let bounds = prepared.bounds;
        window.on_mouse_event(move |event: &MouseDownEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left {
                return;
            }
            let clear = hitboxes
                .iter()
                .any(|id| id.window_point_target_rank(event.position, window) == Some(0));
            if !clear || !input::is_presented(&shared.borrow(), serial) {
                return;
            }
            if window.capture_pointer(&capture, MouseButton::Left).is_err() {
                return;
            }
            if input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                event.position,
                bounds,
                &hitboxes,
                true,
                event.modifiers,
                window,
            ) {
                let focus = shared.borrow().focus.clone();
                window.focus(&focus, cx);
                window.refresh();
                cx.stop_propagation();
            } else {
                let _ = window.release_pointer(&capture);
            }
        });
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
            if !phase.bubble() || !capture.is_captured(window) {
                return;
            }
            if input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::Moved,
                event.position,
                bounds,
                &hitboxes,
                true,
                event.modifiers,
                window,
            ) {
                window.refresh();
            }
        });
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() || event.button != MouseButton::Left || !capture.is_captured(window)
            {
                return;
            }
            if input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                event.position,
                bounds,
                &hitboxes,
                true,
                event.modifiers,
                window,
            ) {
                window.refresh();
                cx.stop_propagation();
            }
        });
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        window.on_modifiers_changed(move |event, window, _| {
            if !capture.is_captured(window) {
                return;
            }
            let position = window.mouse_position();
            let enabled = shared
                .borrow()
                .config
                .input
                .docking_enabled(event.modifiers);
            if input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::DockingEnabledChanged { enabled },
                position,
                bounds,
                &hitboxes,
                true,
                event.modifiers,
                window,
            ) {
                window.refresh();
            }
        });
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if !phase.bubble()
                || !capture.is_captured(window)
                || !shared
                    .borrow()
                    .config
                    .input
                    .cancel_matches(&event.keystroke)
            {
                return;
            }
            input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::StreamCancelled(
                    SurfacePointerCancelReason::ExplicitCancellation,
                ),
                window.mouse_position(),
                bounds,
                &hitboxes,
                true,
                event.keystroke.modifiers,
                window,
            );
            let _ = window.release_pointer(&capture);
            window.refresh();
            cx.stop_propagation();
        });
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        window.on_pointer_cancel(move |_, phase, window, _| {
            if !phase.bubble() {
                return;
            }
            let mut host = shared.borrow_mut();
            let pending_input = host
                .pending
                .as_ref()
                .is_some_and(|pending| !pending.inputs.is_empty());
            if !host.session.has_active_gesture() && host.inputs.is_empty() && !pending_input {
                return;
            }
            let position = window.mouse_position();
            input::queue(
                &mut host,
                serial,
                SurfacePointerEvent::StreamCancelled(
                    SurfacePointerCancelReason::ExplicitPlatformCancellation,
                ),
                position,
                bounds,
                &hitboxes,
                false,
                window.modifiers(),
                window,
            );
            window.refresh();
        });
    }
}

fn build_candidate(
    host: &mut HostState,
    inputs: &[input::QueuedPointer],
    bounds: LogicalRect,
    window: &Window,
) -> Result<(Rc<Packet>, DockspaceUpdate)> {
    let panels = &host.panels;
    let config = &host.config;
    let surface = host.surface;
    let pointer = host.pointer;
    let mut frame = host.session.begin_host_frame()?;
    for input in inputs {
        input.submit(&mut frame)?;
    }
    frame.begin_encoding_pass()?;
    let measure = match frame.surface_work(surface)? {
        SurfaceWork::Ready(plan) => plan.measurement_refresh().is_allowed(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured) => true,
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity) => {
            return Err(anyhow!("renderer output capacity is exhausted"));
        }
    };
    if measure {
        frame.measure_surface_with(surface, |request| match request {
            Request::DockBounds { .. } | Request::PopupPlaneBounds { .. } => Answer::Bounds(bounds),
            Request::PaneMinimum { item, .. } => {
                Answer::PaneMinimum(item.and_then(|item| panels.get(&item)).map_or_else(
                    || LogicalSize::new(120.0, 90.0).expect("default minimum is valid"),
                    |pane| pane.minimum_size,
                ))
            }
            Request::TabIntrinsic { item, .. } => {
                let width = panels.get(&item).map_or(120.0, |pane| {
                    let line = window.text_system().shape_line(
                        pane.title.clone(),
                        config.text_size,
                        &[TextRun {
                            len: pane.title.len(),
                            font: config.font.clone(),
                            color: config.colors.text,
                            ..Default::default()
                        }],
                        None,
                    );
                    f64::from(f32::from(line.width))
                });
                Answer::TabIntrinsic(width)
            }
            Request::TabStrip { .. } => Answer::TabStrip(
                TabStripMetrics::new(0.0, 0.0).expect("zero chrome reservation is valid"),
            ),
        })?;
    }
    let plan = frame
        .paint_plan(surface)?
        .context("measured surface has no ready paint plan")?;
    let (packet, receipt) = record(plan, panels, pointer, &config.colors)?;
    frame.confirm_surface_painted(receipt)?;
    Ok((Rc::new(packet), frame.suspend()))
}

fn to_gpui(rect: LogicalRect, surface: Bounds<Pixels>) -> Bounds<Pixels> {
    Bounds::new(
        point(
            surface.origin.x + px(rect.x() as f32),
            surface.origin.y + px(rect.y() as f32),
        ),
        size(px(rect.width() as f32), px(rect.height() as f32)),
    )
}

fn definite(bounds: Bounds<Pixels>) -> open_gpui::Size<AvailableSpace> {
    size(
        AvailableSpace::Definite(bounds.size.width),
        AvailableSpace::Definite(bounds.size.height),
    )
}

fn outline(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let x = bounds.origin.x;
    let y = bounds.origin.y;
    let w = bounds.size.width;
    let h = bounds.size.height;
    for bounds in [
        Bounds::new(point(x, y), size(w, px(1.0))),
        Bounds::new(point(x, y + h - px(1.0)), size(w, px(1.0))),
        Bounds::new(point(x, y), size(px(1.0), h)),
        Bounds::new(point(x + w - px(1.0), y), size(px(1.0), h)),
    ] {
        window.paint_quad(fill(bounds, color));
    }
}
