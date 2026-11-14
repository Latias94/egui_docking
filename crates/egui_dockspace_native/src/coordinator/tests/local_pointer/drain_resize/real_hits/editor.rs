//! Actual egui measurement, painting, hit publication and raw callback fixture.

use std::collections::BTreeMap;

use egui_dockspace::native_support::NativePaintReceiver;
use egui_dockspace::{DockStyle, PaneResponse, PaneView};

use super::*;
use crate::host_frame::NativeSurfacePaint;
use crate::receiver::test_support::present_completed_output;

struct Panes(BTreeMap<ItemId, String>);

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let background = ui.interact(
            ui.max_rect(),
            ui.scope_id().with("content"),
            egui::Sense::hover(),
        );
        let editor = ui.text_edit_singleline(
            self.0
                .entry(item)
                .or_insert_with(|| format!("document {}", item.get())),
        );
        PaneResponse::from(&background).with_content(&editor)
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct Divider {
    pub(super) axis: DockspaceAxis,
    pub(super) visual: dockspace::runtime::DockspaceVisualId,
    receiver_visual: dockspace::runtime::DockspaceVisualId,
    hit: LogicalRect,
    pub(super) draw: LogicalRect,
}

pub(super) struct RenderedScene {
    tabs: BTreeMap<ItemId, LogicalRect>,
    pub(super) outline: LogicalPoint,
    pub(super) dividers: Vec<Divider>,
    junctions: Vec<LogicalRect>,
    receivers: Vec<NativePaintReceiver>,
}

impl RenderedScene {
    pub(super) fn tab_point(&self, item: ItemId) -> LogicalPoint {
        let bounds = self.tabs[&item];
        LogicalPoint::new(
            bounds.x() + bounds.width() * 0.3,
            bounds.y() + bounds.height() * 0.5,
        )
        .unwrap()
    }

    pub(super) fn divider_point(&self, divider: Divider) -> LogicalPoint {
        [0.25, 0.75, 0.125, 0.875]
            .into_iter()
            .map(|fraction| {
                let (x, y) = match divider.axis {
                    DockspaceAxis::Horizontal => (0.25, fraction),
                    DockspaceAxis::Vertical => (fraction, 0.25),
                };
                LogicalPoint::new(
                    divider.hit.x() + divider.hit.width() * x,
                    divider.hit.y() + divider.hit.height() * y,
                )
                .unwrap()
            })
            .find(|point| {
                !divider.draw.contains(*point)
                    && !self.junctions.iter().any(|bounds| bounds.contains(*point))
                    && !self
                        .dividers
                        .iter()
                        .any(|other| other.visual != divider.visual && other.hit.contains(*point))
            })
            .expect("a fresh divider exposes hit padding outside peer handles and junctions")
    }

    pub(super) fn assert_divider_hit(
        &self,
        context: &egui::Context,
        divider: Divider,
        point: LogicalPoint,
    ) {
        #[allow(clippy::cast_possible_truncation)]
        let point = egui::pos2(point.x() as f32, point.y() as f32);
        let authority = context
            .capture_completed_pass_hit_authority(ViewportId::ROOT)
            .unwrap();
        let hit = authority.hit_test(point).unwrap();
        assert!(
            hit.click().is_none(),
            "drag-only padding must occlude clicks: {hit:?}"
        );
        let drag = hit
            .drag()
            .expect("the fresh splitter must receive dragging");
        let receiver = self
            .receivers
            .iter()
            .find(|receiver| {
                receiver.widget_id() == drag.id() && receiver.layer_id() == drag.layer_id()
            })
            .expect("the actual egui drag winner must belong to this exact paint");
        assert_eq!(
            receiver.role(),
            dockspace::runtime::DockspaceReceiverRole::Splitter
        );
        assert_eq!(receiver.visual_id(), divider.receiver_visual);
    }
}

struct EncodedScene {
    scene: RenderedScene,
    output: PaintedSurfaceOutput,
    paint: NativeSurfacePaint,
    pixels_per_point: f32,
}

pub(super) struct Editor {
    pub(super) native: NativeCoordinator,
    binding: NativeSurfaceBinding,
    pub(super) context: egui::Context,
    panes: Panes,
    now: Arc<AtomicU64>,
    ordinal: u64,
    pass: u64,
    desktop: bool,
}

impl Editor {
    pub(super) fn new(desktop: bool) -> Self {
        let (native, binding, now) = editor_fixture(desktop);
        let context = egui::Context::default();
        context.options_mut(|options| {
            options.zoom_factor = 0.75;
            options.max_passes = 1.try_into().unwrap();
        });
        Self {
            native,
            binding,
            context,
            panes: Panes(
                (1..=8)
                    .map(|id| (ItemId::new(id), format!("document {id}")))
                    .collect(),
            ),
            now,
            ordinal: 0,
            pass: 0,
            desktop,
        }
    }

    fn encode(&mut self) -> EncodedScene {
        self.pass += 1;
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            time: Some(Duration::from_millis(self.pass * 16).as_secs_f64()),
            ..Default::default()
        };
        input
            .viewports
            .get_mut(&ViewportId::ROOT)
            .unwrap()
            .native_pixels_per_point = Some(2.0);
        let mut encoded = None;
        let output = self.context.run_ui(input, |ui| {
            let outline_selected = self
                .native
                .session
                .view()
                .item(FIRST)
                .unwrap()
                .tabs()
                .selected();
            let mut frame = self
                .native
                .poll_resolved_host_frame(&self.context, &mut test_budget())
                .unwrap()
                .expect("a bounded editor pass must reach its encoding phase");
            let style = DockStyle::default();
            frame
                .measure_surface(
                    FIRST_SURFACE,
                    ui,
                    egui::Rect::from_min_max(egui::pos2(0.0, 94.0), egui::pos2(800.0, 600.0)),
                    ui.max_rect(),
                    &self.panes,
                    &style,
                )
                .unwrap();
            let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
            let mut tabs = BTreeMap::new();
            let mut outline = None;
            let mut dividers = Vec::new();
            let mut junctions = Vec::new();
            plan.visit_surface_program(|step| {
                if let SurfacePaintProgramStep::Operation { operation, .. } = step {
                    match operation {
                        SurfacePaintProgramOperation::Tab(tab) => {
                            tabs.insert(tab.item(), tab.visible_bounds());
                        }
                        SurfacePaintProgramOperation::Pane(pane)
                            if pane.selected() == outline_selected =>
                        {
                            let bounds = pane.content_bounds();
                            outline = Some(
                                LogicalPoint::new(
                                    bounds.x() + bounds.width() * 0.5,
                                    bounds.y() + bounds.height() * 0.75,
                                )
                                .unwrap(),
                            );
                        }
                        SurfacePaintProgramOperation::Splitter(splitter) => {
                            dividers.push(Divider {
                                axis: splitter.axis(),
                                visual: splitter.visual_id(),
                                receiver_visual: plan
                                    .receiver_for_splitter(splitter)
                                    .unwrap()
                                    .visual_id(),
                                hit: splitter.hit_bounds(),
                                draw: splitter.draw_bounds(),
                            })
                        }
                        SurfacePaintProgramOperation::SplitterJunction(junction) => {
                            junctions.push(junction.hit_bounds())
                        }
                        _ => {}
                    }
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            let mut paint = frame
                .paint_surface(
                    egui::Id::unique("native-editor-completed-hits"),
                    FIRST_SURFACE,
                    ui,
                    &mut self.panes,
                    &style,
                )
                .unwrap();
            assert!(paint.confirm_into(&mut frame.frame).unwrap());
            let receivers = paint.receivers().collect();
            let mut report = finish(frame);
            assert!(
                report.manipulation_feedback().is_empty(),
                "a paint must not hide an input terminal: {report:?}"
            );
            assert!(report.take_native_effects().is_empty());
            self.native.settle_host_frame_report(&report).unwrap();
            let [output] = report.take_painted_outputs().try_into().unwrap();
            encoded = Some((
                RenderedScene {
                    tabs,
                    outline: outline.unwrap(),
                    dividers,
                    junctions,
                    receivers,
                },
                output,
                paint,
            ));
        });
        let pixels_per_point = output.pixels_per_point;
        assert_eq!(pixels_per_point, 1.5);
        output.drop_without_applying_deltas();
        let (scene, output, paint) = encoded.unwrap();
        EncodedScene {
            scene,
            output,
            paint,
            pixels_per_point,
        }
    }

    pub(super) fn present(&mut self) -> RenderedScene {
        let EncodedScene {
            scene,
            output,
            paint,
            pixels_per_point,
        } = self.encode();
        present_completed_output(
            &mut self.native.receivers,
            &self.context,
            pixels_per_point,
            self.binding,
            &output,
            &paint,
        )
        .unwrap();
        self.native
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = self.commit(false);
        assert!(
            report.manipulation_feedback().is_empty(),
            "presentation must not replay input: {report:?}"
        );
        scene
    }

    pub(super) fn commit(&mut self, retry: bool) -> HostFrameReport {
        let before = self.native.session.version();
        if retry {
            drop(
                self.native
                    .poll_resolved_host_frame(&self.context, &mut test_budget())
                    .unwrap()
                    .unwrap(),
            );
            assert_eq!(self.native.session.version(), before);
        }
        let frame = self
            .native
            .poll_resolved_host_frame(&self.context, &mut test_budget())
            .unwrap_or_else(|error| {
                panic!(
                    "desktop={}, ordinal={}: {error:?}",
                    self.desktop, self.ordinal
                )
            })
            .expect("a retried input prefix must make progress");
        let mut report = finish(frame);
        assert!(report.take_native_effects().is_empty());
        self.native.settle_host_frame_report(&report).unwrap();
        report
    }

    pub(super) fn callback(&mut self, event: WindowEvent) -> HostFrameReport {
        self.ordinal += 1;
        self.now.store(40 + self.ordinal, Ordering::SeqCst);
        let record = if NativeWindowEventClass::classify(&event) == NativeWindowEventClass::Pointer
        {
            capture(&self.native, self.binding, self.ordinal, event)
        } else {
            NativeWindowEventRecord::for_test(
                self.ordinal,
                WindowId::from(WINDOW),
                Some(ViewportId::ROOT),
                Some(self.binding),
                event,
            )
        };
        self.native
            .bridge
            .push_record(HostRecord::WindowEvent(record));
        self.native
            .reduce_callback_prefix(&self.context, &mut test_budget())
            .unwrap();
        self.commit(true)
    }

    pub(super) fn pointer(
        &mut self,
        point: LogicalPoint,
        state: Option<ElementState>,
    ) -> HostFrameReport {
        self.callback(if self.desktop {
            raw_desktop_event(point, state)
        } else {
            raw_event(point, state, 2.0)
        })
    }

    pub(super) fn hold_output(&mut self) -> PaintedSurfaceOutput {
        self.encode().output
    }

    pub(super) fn assert_quiescent(&mut self) {
        for id in 1..=8 {
            assert_eq!(self.panes.0[&ItemId::new(id)], format!("document {id}"));
        }
        assert!(self.native.session.is_current_native_binding(self.binding));
        assert!(!self.native.session.has_pending_native_input());
        assert!(!self.native.session.has_pending_presentation());
        assert!(self.native.next_window_event().unwrap().is_none());
        assert_eq!(self.native.bridge.input_backlog_for_test(), (0, None));
        assert!(
            self.native
                .pointer_translator
                .can_coalesce_idle_cursor_moves()
        );
    }
}
