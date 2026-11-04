//! Opt-in complete-update samples, written as CSV without timing thresholds.
//!
//! Timing and heap accounting run separately; no local unsafe code or lint override.
//! See docs/performance/2026-11-03-engine-first-methodology.md for the boundaries.

use std::convert::Infallible;
use std::hint::black_box;
use std::io::{self, BufWriter, Write};
use std::num::NonZeroUsize;
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceManipulationDelivery, DockspaceManipulationStatus, DockspaceReceiverDescriptor,
    DockspaceSession, DockspaceVisualId, HostPointerInput, HostPointerRoute, PaintedSurfaceOutput,
    PointerDeviceKind, PointerEventMetadata, SurfacePaintDisposition, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePointerButton, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerReceiverFacts, SurfacePresentationResult, SurfaceProgramExecutionReceipt,
    SurfaceUnavailableReason, SurfaceWork, UniformSurfaceMetrics,
};
use dockspace_host_harness::OwnedRecord;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

// The dependency owns GlobalAlloc's unsafe implementation. The harness keeps
// unsafe_code = "forbid" and uses only dhat's safe construction/snapshot APIs.
#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Idle,
    Select,
    Drag,
    MeasurementChange,
    OutputsInFlight,
}

impl Scenario {
    const ALL: [Self; 5] = [
        Self::Idle,
        Self::Select,
        Self::Drag,
        Self::MeasurementChange,
        Self::OutputsInFlight,
    ];

    fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Select => "select",
            Self::Drag => "drag",
            Self::MeasurementChange => "measurement_change",
            Self::OutputsInFlight => "outputs_in_flight",
        }
    }
}

struct Options {
    samples: usize,
    warmup: usize,
    in_flight: usize,
    panes: Vec<usize>,
    timing_only: bool,
}

impl Options {
    fn parse() -> io::Result<Option<Self>> {
        let mut options = Self {
            samples: 24,
            warmup: 4,
            in_flight: 3,
            panes: vec![16, 128, 1_024],
            timing_only: false,
        };
        let mut args = std::env::args().skip(1);
        while let Some(flag) = args.next() {
            if flag == "--help" || flag == "-h" {
                eprintln!(
                    "Usage: frame_cost [--samples 24] [--warmup 4] [--in-flight 3] \
                     [--panes 16,128,1024] [--timing-only]\n\
                     CSV goes to stdout; times are nanoseconds per update/settlement.\n\
                     --in-flight is the number of older outputs retained before the timed update.\n\
                     --timing-only skips active heap profiling for CPU sampling."
                );
                return Ok(None);
            }
            if flag == "--timing-only" {
                options.timing_only = true;
                continue;
            }
            let value = args.next().ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("missing value for {flag}"),
                )
            })?;
            let parse_number = |value: &str| {
                value.parse::<usize>().map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("invalid value for {flag}: {error}"),
                    )
                })
            };
            match flag.as_str() {
                "--samples" => options.samples = parse_number(&value)?,
                "--warmup" => options.warmup = parse_number(&value)?,
                "--in-flight" => options.in_flight = parse_number(&value)?,
                "--panes" => {
                    options.panes = value
                        .split(',')
                        .map(parse_number)
                        .collect::<io::Result<_>>()?;
                }
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        format!("unknown option {flag}"),
                    ));
                }
            }
        }
        if options.samples == 0
            || options.in_flight == 0
            || options.in_flight.checked_add(1).is_none()
            || options.panes.is_empty()
            || options
                .panes
                .iter()
                .any(|panes| ![16, 128, 1_024].contains(panes))
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "samples and in-flight must be positive; in-flight + 1 must fit usize; \
                 panes must be a comma-separated subset of 16,128,1024",
            ));
        }
        Ok(Some(options))
    }
}

fn tree(first: u64, count: usize, axis: DockspaceAxis) -> DockspaceNode {
    if count == 1 {
        return DockspaceNode::tabs([ItemId::new(first * 2), ItemId::new(first * 2 + 1)]);
    }
    let half = count / 2;
    let next = match axis {
        DockspaceAxis::Horizontal => DockspaceAxis::Vertical,
        DockspaceAxis::Vertical => DockspaceAxis::Horizontal,
    };
    DockspaceNode::equal_split(
        axis,
        [
            tree(first, half, next),
            tree(first + u64::try_from(half).unwrap(), count - half, next),
        ],
    )
    .unwrap()
}

// This example owns its sink; the shared harness fixture is not modified.
// OwnedRecord is the same public, owned record format used by the public tests.
#[derive(Default)]
struct RecordingSink {
    records: Vec<OwnedRecord>,
    splitter: Option<DockspaceReceiverDescriptor>,
    pane_contents: usize,
}

impl RecordingSink {
    fn encode(plan: SurfacePaintPlan<'_>) -> (Self, SurfaceProgramExecutionReceipt) {
        let mut sink = Self::default();
        let visible = SurfacePaintDisposition::Visible;
        let receipt = plan
            .execute_surface_program(|step| {
                match step {
                    Step::RootBegin { root, paint } => {
                        sink.records.push(OwnedRecord::RootBegin(root.root()));
                        if let Some(frame) = root.contained() {
                            sink.geometry(frame.visual_id(), frame.outer_bounds(), paint);
                        }
                    }
                    Step::Operation {
                        operation, paint, ..
                    } => {
                        let bounds = match operation {
                            Operation::Pane(pane) => {
                                sink.pane_contents += 1;
                                sink.records.push(OwnedRecord::PaneContent {
                                    visual: pane.visual_id(),
                                    item: pane.selected(),
                                    bounds: pane.content_bounds(),
                                    paint,
                                });
                                pane.bounds()
                            }
                            Operation::TabBar(bar) => bar.bounds(),
                            Operation::Tab(tab) => {
                                if let Some(receiver) = plan.receiver_for_tab_body(tab) {
                                    sink.records.push(OwnedRecord::Receiver(receiver));
                                }
                                tab.visible_bounds()
                            }
                            Operation::Splitter(splitter) => {
                                if splitter.axis() == DockspaceAxis::Horizontal
                                    && splitter.operable()
                                {
                                    let receiver = plan.receiver_for_splitter(splitter).unwrap();
                                    // The tallest horizontal splitter belongs to the root split.
                                    if sink.splitter.is_none_or(|current| {
                                        receiver.bounds().height() > current.bounds().height()
                                    }) {
                                        sink.splitter = Some(receiver);
                                    }
                                }
                                splitter.draw_bounds()
                            }
                            Operation::SplitterJunction(junction) => junction.hit_bounds(),
                            Operation::TabStripControl(control) => control.bounds(),
                            Operation::ContainedControls(frame) => frame.title_bounds(),
                            Operation::PresentationMenuAnchor(anchor) => anchor.bounds(),
                        };
                        sink.geometry(operation.visual_id(), bounds, paint);
                    }
                    Step::RootEnd(root) => sink.records.push(OwnedRecord::RootEnd(root.root())),
                    Step::PopupBackdrop(backdrop) => {
                        sink.geometry(backdrop.visual_id(), backdrop.bounds(), visible);
                    }
                    Step::PopupMenu { menu, paint } => {
                        sink.geometry(menu.visual_id(), menu.bounds(), paint);
                        for row in menu.rows() {
                            sink.geometry(row.visual_id(), row.bounds(), paint);
                        }
                    }
                    Step::DragPreview(preview) => {
                        sink.records.push(OwnedRecord::Preview(preview.visual()));
                    }
                    Step::DropTarget(receiver) => {
                        sink.records.push(OwnedRecord::Receiver(receiver))
                    }
                    Step::DropGuide {
                        structural,
                        affordance,
                    } => {
                        sink.geometry(
                            structural.visual_id(),
                            structural.activation_bounds(),
                            visible,
                        );
                        for target in structural.targets() {
                            sink.geometry(target.visual_id(), target.draw_bounds(), visible);
                        }
                        if let Some(cluster) = affordance {
                            for target in cluster.targets() {
                                sink.geometry(target.visual_id(), target.draw_bounds(), visible);
                            }
                        }
                    }
                    Step::ContainedTransformPreview(preview) => sink.records.push(
                        OwnedRecord::TransformPreview(preview.floating(), preview.rect()),
                    ),
                    Step::DragDecoration(decoration) => {
                        sink.geometry(
                            decoration.source_visual(),
                            decoration.source_bounds(),
                            visible,
                        );
                        if let Some(bounds) = decoration.projected_root_bounds() {
                            sink.geometry(decoration.source_visual(), bounds, visible);
                        }
                    }
                    Step::InteractionFeedback => {
                        sink.records.push(OwnedRecord::InteractionFeedback)
                    }
                    Step::SurfaceEnd { semantic_output } => {
                        sink.records.push(OwnedRecord::SurfaceEnd(semantic_output));
                    }
                }
                Ok::<(), Infallible>(())
            })
            .unwrap();
        (sink, receipt)
    }

    fn geometry(
        &mut self,
        visual: DockspaceVisualId,
        bounds: LogicalRect,
        paint: SurfacePaintDisposition,
    ) {
        self.records.push(OwnedRecord::Geometry {
            visual,
            bounds,
            paint,
        });
    }
}

struct EncodedOutput {
    sink: RecordingSink,
    permit: PaintedSurfaceOutput,
}

#[derive(Clone, Copy)]
struct Drag {
    start: LogicalPoint,
    end: LogicalPoint,
}

struct Harness {
    session: DockspaceSession,
    pending: Vec<EncodedOutput>,
    presented_splitter: Option<DockspaceReceiverDescriptor>,
    drag: Option<Drag>,
    panes: usize,
    step: u64,
}

impl Harness {
    fn new(panes: usize, slots: usize) -> Self {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, tree(1, panes, DockspaceAxis::Horizontal)),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session.enable_host_pointer().unwrap();
        session
            .configure_output_capacity(NonZeroUsize::new(slots).unwrap())
            .unwrap();
        let mut harness = Self {
            session,
            pending: Vec::with_capacity(slots),
            presented_splitter: None,
            drag: None,
            panes,
            step: 0,
        };
        harness.update(Scenario::Idle, None);
        harness.settle();
        harness
    }

    fn update(&mut self, scenario: Scenario, pointer_event: Option<SurfacePointerEvent>) {
        let alternate = self.step % 2 == 1;
        let pointer = pointer_event.map(|event| {
            let drag = self.drag.unwrap();
            let point = if matches!(event, SurfacePointerEvent::ButtonPressed(_)) {
                drag.start
            } else {
                drag.end
            };
            (
                event,
                point,
                self.session.host_pointer_hit(SURFACE, point).unwrap(),
            )
        });
        let mut frame = self.session.begin_host_frame().unwrap();
        if matches!(scenario, Scenario::Select | Scenario::OutputsInFlight) {
            frame
                .select_item_current(ItemId::new(if alternate { 3 } else { 2 }))
                .unwrap();
        }
        if let Some((event, point, hit)) = pointer {
            let surface = hit.surface();
            let click = hit.delivery();
            let drag = hit.drag_delivery();
            let facts = click.as_ref().map_or_else(
                || SurfacePointerReceiverFacts::unknown().with_no_delivery(&surface),
                SurfacePointerReceiverFacts::delivery,
            );
            let facts = drag.as_ref().map_or_else(
                || facts.with_no_drag_delivery(&surface),
                |receiver| facts.with_drag_delivery(receiver),
            );
            let route = HostPointerRoute::new(surface, point);
            let capture = if matches!(event, SurfacePointerEvent::ButtonReleased(_)) {
                SurfacePointerCapture::None
            } else {
                SurfacePointerCapture::ProviderEndpoint
            };
            frame
                .submit_host_pointer(
                    HostPointerInput::new(
                        SurfacePointerId::new(1),
                        event,
                        route,
                        route,
                        capture,
                        facts.with_clear_hover_lane(&surface),
                    )
                    .with_metadata(PointerEventMetadata::new(
                        PointerDeviceKind::Mouse,
                        Duration::from_millis(self.step),
                    )),
                )
                .unwrap();
        }
        let changed_measurement = matches!(
            scenario,
            Scenario::MeasurementChange | Scenario::OutputsInFlight
        ) && alternate;
        frame
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(
                        0.0,
                        0.0,
                        if changed_measurement {
                            16_448.0
                        } else {
                            16_384.0
                        },
                        16_384.0,
                    )
                    .unwrap(),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    if changed_measurement { 72.0 } else { 64.0 },
                )
                .unwrap(),
            )
            .unwrap();
        let plan = match frame.surface_work(SURFACE).unwrap() {
            SurfaceWork::Ready(plan) => plan,
            SurfaceWork::Deferred(reason) => {
                panic!("complete-update sample unexpectedly deferred: {reason:?}")
            }
        };
        let (sink, receipt) = RecordingSink::encode(plan);
        frame.confirm_surface_painted(receipt).unwrap();
        let mut report = frame.commit().unwrap();
        let released = matches!(pointer_event, Some(SurfacePointerEvent::ButtonReleased(_)));
        assert_eq!(
            report.workspace_changed(),
            matches!(scenario, Scenario::Select | Scenario::OutputsInFlight) || released
        );
        if scenario == Scenario::Drag {
            assert_eq!(self.session.has_active_gesture(), !released);
            if released {
                assert!(report.manipulation_feedback().iter().any(|feedback| {
                    feedback.status()
                        == DockspaceManipulationStatus::Delivered(
                            DockspaceManipulationDelivery::SplitterResize,
                        )
                }));
            }
        }
        let [permit]: [PaintedSurfaceOutput; 1] = report.take_painted_outputs().try_into().unwrap();
        let Some(OwnedRecord::SurfaceEnd(semantic)) = sink.records.last() else {
            panic!("encoding must reach SurfaceEnd");
        };
        assert!(permit.matches_semantic_output(*semantic));
        assert_eq!(sink.pane_contents, self.panes);
        black_box(&sink.records);
        self.pending.push(EncodedOutput { sink, permit });
        self.step += 1;
    }

    fn settle(&mut self) {
        let newest = self.pending.last().unwrap();
        let artifact = newest.permit.presentation_artifact();
        self.presented_splitter = newest.sink.splitter;
        // Reverse results exercise the exact output prefix, not a synthetic drop.
        while let Some(EncodedOutput { sink, permit }) = self.pending.pop() {
            self.session
                .report_surface_presentation(permit, SurfacePresentationResult::Presented)
                .unwrap();
            drop(sink);
        }
        let mut frame = self.session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert!(!report.workspace_changed());
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(artifact))
        );
        assert!(
            self.session
                .presented_surface(SURFACE)
                .unwrap()
                .matches_presentation_artifact(artifact)
        );
    }

    fn sample(&mut self, scenario: Scenario, older_outputs: usize, heap: bool) -> Sample {
        assert!(self.pending.is_empty());
        if scenario == Scenario::Drag {
            let source = self
                .session
                .bind_presented_receiver(&self.presented_splitter.unwrap())
                .expect("drag starts from the exact last presented splitter");
            // Stay away from junctions at the centers of balanced child splits.
            let start = LogicalPoint::new(
                source.center().x(),
                source.bounds().y() + source.bounds().height() * 0.37,
            )
            .unwrap();
            assert_eq!(
                self.session
                    .host_pointer_hit(SURFACE, start)
                    .unwrap()
                    .drag_delivery()
                    .unwrap()
                    .descriptor(),
                source.descriptor()
            );
            self.drag = Some(Drag {
                start,
                end: LogicalPoint::new(
                    start.x() + if self.step % 2 == 1 { 64.0 } else { -64.0 },
                    start.y(),
                )
                .unwrap(),
            });
            self.update(
                scenario,
                Some(SurfacePointerEvent::ButtonPressed(
                    SurfacePointerButton::Primary,
                )),
            );
            self.settle();
        }
        // Prefill is outside both measured windows. Every older output has its
        // own changed selection/measurement and owned recording payload.
        for _ in 0..older_outputs {
            self.update(Scenario::OutputsInFlight, None);
        }
        let event = (scenario == Scenario::Drag).then_some(SurfacePointerEvent::Moved);
        let update = measure(heap, || self.update(scenario, event));
        assert_eq!(self.pending.len(), older_outputs + 1);
        let settlement = measure(heap, || self.settle());
        if scenario == Scenario::Drag {
            // Complete and present the terminal outside both measurement windows.
            self.update(
                scenario,
                Some(SurfacePointerEvent::ButtonReleased(
                    SurfacePointerButton::Primary,
                )),
            );
            self.settle();
            self.drag = None;
        }
        Sample { update, settlement }
    }
}

#[derive(Clone, Copy)]
struct HeapCost {
    allocation_events: u64,
    requested_bytes: u64,
    live_before_tracked_bytes: usize,
    live_after_tracked_bytes: usize,
    // dhat cannot reset its high water mark. This is a case-lifetime prefix
    // peak including setup, warmup and earlier samples, never a window peak.
    case_prefix_peak_tracked_bytes: usize,
}

#[derive(Clone, Copy)]
enum Cost {
    Time(u128),
    Heap(HeapCost),
}

struct Sample {
    update: Cost,
    settlement: Cost,
}

fn measure(heap: bool, run: impl FnOnce()) -> Cost {
    if heap {
        let before = dhat::HeapStats::get();
        run();
        let after = dhat::HeapStats::get();
        Cost::Heap(HeapCost {
            allocation_events: after.total_blocks - before.total_blocks,
            requested_bytes: after.total_bytes - before.total_bytes,
            live_before_tracked_bytes: before.curr_bytes,
            live_after_tracked_bytes: after.curr_bytes,
            case_prefix_peak_tracked_bytes: after.max_bytes,
        })
    } else {
        let start = Instant::now();
        run();
        Cost::Time(start.elapsed().as_nanos())
    }
}

fn write_case(
    out: &mut impl Write,
    panes: usize,
    scenario: Scenario,
    older_outputs: usize,
    options: &Options,
    samples: &[Sample],
) -> io::Result<()> {
    let heap = matches!(samples[0].update, Cost::Heap(_));
    let metrics: &[(&str, fn(Cost) -> u128)] = if heap {
        &[
            ("allocation_events", |cost| match cost {
                Cost::Heap(cost) => u128::from(cost.allocation_events),
                _ => unreachable!(),
            }),
            ("requested_bytes", |cost| match cost {
                Cost::Heap(cost) => u128::from(cost.requested_bytes),
                _ => unreachable!(),
            }),
            ("live_before_tracked_bytes", |cost| match cost {
                Cost::Heap(cost) => cost.live_before_tracked_bytes as u128,
                _ => unreachable!(),
            }),
            ("live_after_tracked_bytes", |cost| match cost {
                Cost::Heap(cost) => cost.live_after_tracked_bytes as u128,
                _ => unreachable!(),
            }),
            ("case_prefix_peak_tracked_bytes", |cost| match cost {
                Cost::Heap(cost) => cost.case_prefix_peak_tracked_bytes as u128,
                _ => unreachable!(),
            }),
        ]
    } else {
        &[("time_ns", |cost| match cost {
            Cost::Time(ns) => ns,
            _ => unreachable!(),
        })]
    };
    let instrumentation = if heap { "dhat_active" } else { "dhat_inactive" };
    for update in [true, false] {
        let (phase, state, outputs_before, outputs_after) = if update {
            (
                "update",
                "retained_in_flight",
                older_outputs,
                older_outputs + 1,
            )
        } else {
            ("settlement", "settled", older_outputs + 1, 0)
        };
        for &(metric, read) in metrics {
            let mut values: Vec<_> = samples
                .iter()
                .map(|sample| {
                    read(if update {
                        sample.update
                    } else {
                        sample.settlement
                    })
                })
                .collect();
            values.sort_unstable();
            let percentile = |percent: usize| values[(values.len() * percent).div_ceil(100) - 1];
            writeln!(
                out,
                "{panes},2,{},{instrumentation},{phase},{state},{},{},{outputs_before},{outputs_after},{metric},{},{},{}",
                scenario.name(),
                options.samples,
                options.warmup,
                percentile(50),
                percentile(95),
                values.last().unwrap()
            )?;
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let Some(options) = Options::parse()? else {
        return Ok(());
    };
    let mut out = BufWriter::new(io::stdout().lock());
    writeln!(
        out,
        "panes,tabs_per_pane,scenario,instrumentation,phase,end_state,samples,warmup,outputs_before,outputs_after,metric,p50,p95,max"
    )?;
    for &panes in &options.panes {
        for scenario in Scenario::ALL {
            for &heap in if options.timing_only {
                &[false][..]
            } else {
                &[false, true][..]
            } {
                // Sample storage and output formatting are outside profiling.
                let mut samples = Vec::with_capacity(options.samples);
                // One profiler per pane-count/scenario case, not per frame.
                // testing() suppresses JSON/summary output, not heap backtraces.
                let profiler = heap.then(|| {
                    dhat::Profiler::builder()
                        .testing()
                        .trim_backtraces(Some(4))
                        .build()
                });
                let mut harness = Harness::new(panes, options.in_flight + 1);
                let older_outputs = if scenario == Scenario::OutputsInFlight {
                    options.in_flight
                } else {
                    0
                };
                for _ in 0..options.warmup {
                    black_box(harness.sample(scenario, older_outputs, heap));
                }
                for _ in 0..options.samples {
                    samples.push(harness.sample(scenario, older_outputs, heap));
                }
                drop(harness);
                drop(profiler);
                write_case(&mut out, panes, scenario, older_outputs, &options, &samples)?;
            }
        }
    }
    out.flush()
}
