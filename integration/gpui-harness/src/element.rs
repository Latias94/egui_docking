//! A single retained Element consuming the core's complete ordered paint program.

use std::{collections::BTreeMap, convert::Infallible, rc::Rc};

use anyhow::{Context as _, Result, anyhow};
use dockspace::{
    geometry::{LogicalRect, LogicalSize},
    model::ItemId,
    runtime::{
        DockspacePreviewVisual, DockspaceReceiverDescriptor, DockspaceUpdate,
        SurfaceMeasurementAnswer as Answer, SurfaceMeasurementRequest as Request, SurfacePaintPlan,
        SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
        SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerEvent, SurfaceWork,
        SurfaceWorkDeferred, TabStripMetrics,
    },
};
use open_gpui::{
    AnyElement, App, AvailableSpace, Bounds, Element, ElementId, GlobalElementId, HitboxBehavior,
    HitboxId, InspectorElementId, IntoElement, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, Pixels, PointerCaptureHandle, Style, TextRun, Window, div, fill,
    font, point, prelude::*, px, relative, rgb, rgba, size,
};

use crate::{HostState, SURFACE, SharedHost, input, presentation};

const BACKGROUND: u32 = 0xe6eaf0;
const TAB_BAR: u32 = 0xd8dee8;
const TAB: u32 = 0xcbd4e1;
const SELECTED: u32 = 0xf5f6f8;
const INK: u32 = 0x253247;
const ACCENT: u32 = 0x2b7b99;

pub(super) struct Packet {
    draws: Vec<Draw>,
}

impl Packet {
    pub(super) fn contains_pane(&self, item: ItemId) -> bool {
        self.draws
            .iter()
            .any(|draw| matches!(draw, Draw::Pane(_, candidate) if *candidate == item))
    }
}

enum Draw {
    Fill(LogicalRect, u32),
    Outline(LogicalRect, u32),
    Text(LogicalRect, String, u32),
    Pane(LogicalRect, ItemId),
    Receiver(DockspaceReceiverDescriptor),
    Preview(LogicalRect),
}

enum PreparedDraw {
    Fill(Bounds<Pixels>, u32),
    Outline(Bounds<Pixels>, u32),
    Element(AnyElement),
    Preview(Bounds<Pixels>),
}

pub struct DockspaceElement {
    host: SharedHost,
}

pub struct PreparedFrame {
    draws: Vec<PreparedDraw>,
    hitboxes: Vec<HitboxId>,
    capture: PointerCaptureHandle,
    serial: u64,
    bounds: Bounds<Pixels>,
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
    type PrepaintState = Option<PreparedFrame>;

    fn id(&self) -> Option<ElementId> {
        Some("dockspace-gpui-host".into())
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
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> Option<PreparedFrame> {
        let prepared = self.prepare(bounds, window, cx);
        match prepared {
            Ok(prepared) => Some(prepared),
            Err(error) => {
                self.host.borrow_mut().fail(error);
                None
            }
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request: &mut (),
        prepared: &mut Option<PreparedFrame>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.paint_quad(fill(bounds, rgb(BACKGROUND)));
        let Some(prepared) = prepared.as_mut() else {
            return;
        };
        // Keep the core order, including pane widgets between chrome and overlays.
        for draw in &mut prepared.draws {
            match draw {
                PreparedDraw::Fill(bounds, color) => window.paint_quad(fill(*bounds, rgb(*color))),
                PreparedDraw::Outline(bounds, color) => outline(*bounds, *color, window),
                PreparedDraw::Preview(bounds) => {
                    window.paint_quad(fill(*bounds, rgba(0x2b7b993c)));
                    outline(*bounds, ACCENT, window);
                }
                PreparedDraw::Element(element) => element.paint(window, cx),
            }
        }
        self.listen(prepared, window);
    }
}

impl DockspaceElement {
    fn prepare(
        &mut self,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut App,
    ) -> Result<PreparedFrame> {
        let logical = LogicalRect::new(
            0.0,
            0.0,
            f64::from(f32::from(bounds.size.width)),
            f64::from(f32::from(bounds.size.height)),
        )?;
        let (packet, serial, publication, capture) = {
            let mut host = self.host.borrow_mut();
            presentation::abandon_unaccepted(&mut host, window);
            let capture = *host
                .capture
                .get_or_insert_with(|| window.new_pointer_capture_handle());
            let publication = host.publication;
            if let Some(pending) = &host.pending {
                (pending.packet.clone(), pending.serial, publication, capture)
            } else if !host.dirty && host.inputs.is_empty() && host.bounds == Some(logical) {
                (
                    host.packet
                        .clone()
                        .context("a reusable frame has a retained packet")?,
                    host.serial,
                    publication,
                    capture,
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
                (packet, serial, publication, capture)
            }
        };

        let background = window.insert_hitbox(bounds, HitboxBehavior::Normal);
        window.bind_pointer_capture(&capture, background.id)?;
        let mut hitboxes = vec![background.id];
        let mut draws = Vec::new();
        for draw in &packet.draws {
            match draw {
                Draw::Fill(rect, color) => {
                    draws.push(PreparedDraw::Fill(to_gpui(*rect, bounds), *color))
                }
                Draw::Outline(rect, color) => {
                    draws.push(PreparedDraw::Outline(to_gpui(*rect, bounds), *color))
                }
                Draw::Preview(rect) => draws.push(PreparedDraw::Preview(to_gpui(*rect, bounds))),
                Draw::Receiver(receiver) => {
                    let hitbox = window
                        .insert_hitbox(to_gpui(receiver.bounds(), bounds), HitboxBehavior::Normal);
                    hitboxes.push(hitbox.id);
                }
                Draw::Text(rect, title, color) => {
                    let rect = to_gpui(*rect, bounds);
                    let mut element = div()
                        .w(rect.size.width)
                        .h(rect.size.height)
                        .overflow_hidden()
                        .flex()
                        .items_center()
                        .font_family("Arial")
                        .text_size(px(14.0))
                        .text_color(rgb(*color))
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
        window.record_prepaint_window_transaction(
            publication,
            move |fence, window, _| presentation::accept(&shared, serial, fence, window),
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
                event.modifiers.shift,
                window,
            ) {
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
                event.modifiers.shift,
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
                event.modifiers.shift,
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
            if input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::DockingEnabledChanged {
                    enabled: !event.modifiers.shift,
                },
                position,
                bounds,
                &hitboxes,
                true,
                event.modifiers.shift,
                window,
            ) {
                window.refresh();
            }
        });
        let shared = self.host.clone();
        let hitboxes = prepared.hitboxes.clone();
        window.on_key_event(move |event: &KeyDownEvent, phase, window, cx| {
            if !phase.bubble() || event.keystroke.key != "escape" || !capture.is_captured(window) {
                return;
            }
            let position = window.mouse_position();
            input::queue(
                &mut shared.borrow_mut(),
                serial,
                SurfacePointerEvent::StreamCancelled(
                    SurfacePointerCancelReason::ExplicitCancellation,
                ),
                position,
                bounds,
                &hitboxes,
                true,
                false,
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
                false,
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
    let widths: BTreeMap<_, _> = host
        .panels
        .iter()
        .map(|(item, panel)| {
            let title = panel.title;
            let line = window.text_system().shape_line(
                title.into(),
                px(14.0),
                &[TextRun {
                    len: title.len(),
                    font: font("Arial"),
                    color: rgb(INK).into(),
                    ..Default::default()
                }],
                None,
            );
            (*item, f64::from(f32::from(line.width)))
        })
        .collect();
    let titles: BTreeMap<_, _> = host
        .panels
        .iter()
        .map(|(item, panel)| (*item, panel.title))
        .collect();
    let mut frame = host.session.begin_host_frame()?;
    for input in inputs {
        input.submit(&mut frame)?;
    }
    frame.begin_encoding_pass()?;
    let measure = match frame.surface_work(SURFACE)? {
        SurfaceWork::Ready(plan) => plan.measurement_refresh().is_allowed(),
        SurfaceWork::Deferred(SurfaceWorkDeferred::Unmeasured) => true,
        SurfaceWork::Deferred(SurfaceWorkDeferred::OutputCapacity) => {
            return Err(anyhow!("renderer output capacity is exhausted"));
        }
    };
    if measure {
        frame.measure_surface_with(SURFACE, |request| match request {
            Request::DockBounds { .. } | Request::PopupPlaneBounds { .. } => Answer::Bounds(bounds),
            Request::PaneMinimum { .. } => Answer::PaneMinimum(
                LogicalSize::new(120.0, 90.0).expect("constant pane minimum is valid"),
            ),
            Request::TabIntrinsic { item, .. } => {
                Answer::TabIntrinsic(widths.get(&item).copied().unwrap_or(120.0))
            }
            Request::TabStrip { .. } => Answer::TabStrip(
                TabStripMetrics::new(0.0, 0.0).expect("zero chrome reservation is valid"),
            ),
        })?;
    }
    let plan = frame
        .paint_plan(SURFACE)?
        .context("measured surface has no ready paint plan")?;
    let (packet, receipt) = record(plan, &titles, host.pointer)?;
    frame.confirm_surface_painted(receipt)?;
    Ok((Rc::new(packet), frame.suspend()))
}

fn record(
    plan: SurfacePaintPlan<'_>,
    titles: &BTreeMap<ItemId, &'static str>,
    pointer: Option<dockspace::geometry::LogicalPoint>,
) -> Result<(Packet, dockspace::runtime::SurfaceProgramExecutionReceipt)> {
    let mut draws = Vec::new();
    let label = |item: Option<ItemId>| {
        item.and_then(|item| titles.get(&item).copied())
            .unwrap_or("Workspace")
            .to_owned()
    };
    let receiver = |draws: &mut Vec<Draw>, value: Option<DockspaceReceiverDescriptor>| {
        if let Some(value) = value {
            draws.push(Draw::Receiver(value));
        }
    };
    let receipt = plan
        .execute_surface_program(|step| {
            match step {
                Step::RootBegin { root, paint } => {
                    if let Some(contained) = root.contained() {
                        receiver(&mut draws, plan.receiver_for_contained_frame(contained));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(contained.outer_bounds(), TAB_BAR));
                            draws.push(Draw::Outline(contained.outer_bounds(), 0x8391a6));
                        }
                    }
                }
                Step::Operation {
                    operation,
                    paint,
                    root,
                } => match operation {
                    Operation::Pane(pane) => {
                        receiver(&mut draws, plan.receiver_for_pane(pane));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(pane.content_bounds(), SELECTED));
                            if let Some(item) = pane.selected() {
                                draws.push(Draw::Pane(pane.content_bounds(), item));
                            }
                        }
                    }
                    Operation::TabBar(bar) => {
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(bar.bounds(), TAB_BAR));
                        }
                        for region in bar.group_drag_regions() {
                            receiver(&mut draws, plan.receiver_for_tab_group_drag_region(region));
                        }
                    }
                    Operation::Tab(tab) => {
                        receiver(&mut draws, plan.receiver_for_tab_body(tab));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(
                                tab.visible_bounds(),
                                if tab.selected() { SELECTED } else { TAB },
                            ));
                            draws.push(Draw::Text(tab.text_bounds(), label(Some(tab.item())), INK));
                        }
                    }
                    Operation::Splitter(splitter) => {
                        receiver(&mut draws, plan.receiver_for_splitter(splitter));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(splitter.draw_bounds(), 0xa3afbf));
                        }
                    }
                    Operation::SplitterJunction(junction) => {
                        receiver(&mut draws, plan.receiver_for_splitter_junction(junction))
                    }
                    Operation::ContainedControls(contained) => {
                        receiver(&mut draws, plan.receiver_for_contained_title(contained));
                        for resize in contained.resize() {
                            receiver(
                                &mut draws,
                                plan.receiver_for_contained_resize(contained, resize),
                            );
                        }
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(contained.title_bounds(), 0xb9c6d8));
                            draws.push(Draw::Text(
                                contained.title_drag_bounds(),
                                label(root.title_item()),
                                INK,
                            ));
                        }
                    }
                    Operation::TabStripControl(control) => {
                        receiver(&mut draws, plan.receiver_for_tab_strip_control(control));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(control.bounds(), TAB));
                        }
                    }
                    Operation::PresentationMenuAnchor(_) => {}
                },
                Step::DropGuide {
                    affordance: Some(cluster),
                    ..
                } => {
                    for target in cluster.targets() {
                        receiver(&mut draws, plan.receiver_for_drop_target(target));
                        draws.push(Draw::Fill(
                            target.draw_bounds(),
                            if target.is_active() { ACCENT } else { 0xc4dde7 },
                        ));
                        draws.push(Draw::Outline(target.draw_bounds(), ACCENT));
                        let caption = match target.direction() {
                            dockspace::runtime::DockspaceDropDirection::Center => "+",
                            dockspace::runtime::DockspaceDropDirection::Left => "←",
                            dockspace::runtime::DockspaceDropDirection::Right => "→",
                            dockspace::runtime::DockspaceDropDirection::Top => "↑",
                            dockspace::runtime::DockspaceDropDirection::Bottom => "↓",
                        };
                        draws.push(Draw::Text(target.draw_bounds(), caption.into(), INK));
                    }
                }
                Step::DragPreview(preview) => {
                    if let Some(target) = preview.target() {
                        draws.push(Draw::Outline(target.bounds(), ACCENT));
                    }
                    if !preview.is_layout_projected() {
                        match preview.visual() {
                            DockspacePreviewVisual::Dock { rect, .. }
                            | DockspacePreviewVisual::Contained { rect, .. } => {
                                draws.push(Draw::Preview(rect))
                            }
                            DockspacePreviewVisual::Native { .. } => {}
                        }
                    }
                }
                Step::ContainedTransformPreview(preview) => {
                    if !preview.is_layout_projected() {
                        draws.push(Draw::Preview(preview.rect()));
                    }
                }
                Step::DragDecoration(decoration) => {
                    if !decoration.source_is_projected() {
                        if let Some(pointer) = pointer {
                            if let Ok(ghost) = LogicalRect::new(
                                pointer.x() + 12.0,
                                pointer.y() + 14.0,
                                160.0,
                                28.0,
                            ) {
                                draws.push(Draw::Fill(ghost, 0xd3e0ed));
                                draws.push(Draw::Outline(ghost, ACCENT));
                                draws.push(Draw::Text(ghost, label(decoration.ghost_item()), INK));
                            }
                        }
                    }
                }
                Step::PopupBackdrop(backdrop) => receiver(
                    &mut draws,
                    plan.receiver_for_tab_list_menu_backdrop(backdrop),
                ),
                Step::PopupMenu { menu, .. } => {
                    draws.push(Draw::Fill(menu.bounds(), SELECTED));
                    receiver(&mut draws, plan.receiver_for_tab_list_menu_frame(menu));
                    for row in menu.rows() {
                        receiver(&mut draws, plan.receiver_for_tab_list_menu_row(row));
                        draws.push(Draw::Text(row.bounds(), label(Some(row.item())), INK));
                    }
                }
                Step::DropTarget(_)
                | Step::DropGuide {
                    affordance: None, ..
                }
                | Step::InteractionFeedback
                | Step::RootEnd(_)
                | Step::SurfaceEnd { .. } => {}
            }
            Ok::<(), Infallible>(())
        })
        .map_err(|error| anyhow!("core surface program rejected: {error:?}"))?;
    Ok((Packet { draws }, receipt))
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

fn outline(bounds: Bounds<Pixels>, color: u32, window: &mut Window) {
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
        window.paint_quad(fill(bounds, rgb(color)));
    }
}
