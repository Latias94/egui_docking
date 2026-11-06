//! Complete CPU product frames without a native window or GPU renderer.

use std::io::{self, Write};
use std::ops::Range;
use std::process::ExitCode;
use std::time::Instant;

#[global_allocator]
static ALLOCATOR: dhat::Alloc = dhat::Alloc;

#[derive(Clone, Copy)]
struct Allocation {
    events: u64,
    bytes: u64,
    live_before: usize,
    live_after: usize,
    prefix_peak: usize,
}


use dockspace::policy::{CloseCapability, DockPolicy};
use egui::{
    Color32, Context, Event, FontId, Modifiers, PointerButton, Pos2, RawInput, Rect, Sense,
    TextStyle, Ui, Vec2, WidgetText,
};
use egui_dockspace::{
    DockStyle, Dockspace, DockspaceAxis, DockspaceCapability, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceCommitStatus, DockspaceSurfaceLayout,
    DockspaceSurfaceStatus, ItemId, PaneView, RootId, SurfaceId, SurfacePresentationResult,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const WIDTH: f32 = 16_384.0;
const RESIZED_WIDTH: f32 = 16_448.0;
const HEIGHT: f32 = 16_384.0;
const READY_LIMIT: usize = 32;
const HEADER: &str = "product,panes,tabs_per_pane,scenario,sample,frames,elapsed_ns,passes,content_calls,visible_contents,vertices,indices,draw_commands,allocation_events,requested_bytes,live_before_bytes,live_after_bytes,case_prefix_peak_bytes";

type Result<T> = std::result::Result<T, String>;

struct Options {
    samples: usize,
    warmup: usize,
    panes: Vec<usize>,
    scenario: Option<Scenario>,
}

impl Options {
    fn parse() -> Result<Self> {
        let mut options = Self {
            samples: 24,
            warmup: 6,
            panes: vec![16, 128, 1024],
            scenario: None,
        };
        let mut arguments = std::env::args().skip(1);
        while let Some(argument) = arguments.next() {
            let (flag, inline) = argument
                .split_once('=')
                .map_or((argument.as_str(), None), |(flag, value)| {
                    (flag, Some(value.to_owned()))
                });
            let value = match inline {
                Some(value) => value,
                None => arguments
                    .next()
                    .ok_or_else(|| format!("missing value for {flag}"))?,
            };
            match flag {
                "--samples" => {
                    options.samples = value
                        .parse()
                        .map_err(|_| format!("invalid --samples value {value:?}"))?;
                }
                "--warmup" => {
                    options.warmup = value
                        .parse()
                        .map_err(|_| format!("invalid --warmup value {value:?}"))?;
                }
                "--panes" => {
                    options.panes = value
                        .split(',')
                        .map(|value| {
                            value
                                .parse::<usize>()
                                .map_err(|_| format!("invalid --panes entry {value:?}"))
                        })
                        .collect::<Result<_>>()?;
                }
                "--scenario" => {
                    options.scenario = Some(match value.as_str() {
                        "idle" => Scenario::Idle,
                        "tab_click" => Scenario::TabClick,
                        "surface_resize" => Scenario::SurfaceResize,
                        _ => return Err(format!("unknown scenario {value:?}")),
                    });
                }
                _ => return Err(format!("unknown option {flag:?}")),
            }
        }
        if options.samples == 0 {
            return Err("--samples must be positive".into());
        }
        if options.panes.is_empty()
            || options
                .panes
                .iter()
                .any(|&panes| panes < 2 || !panes.is_power_of_two())
        {
            return Err("--panes requires powers of two of at least 2".into());
        }
        Ok(options)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scenario {
    Idle,
    TabClick,
    SurfaceResize,
}

impl Scenario {
    const fn name(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::TabClick => "tab_click",
            Self::SurfaceResize => "surface_resize",
        }
    }
}

struct ContentObservation {
    pass: u64,
    item: ItemId,
    bounds: Rect,
    clip: Rect,
    label: Rect,
    square: Rect,
    opacity: f32,
}

struct Panes {
    titles: Vec<String>,
    observations: Vec<ContentObservation>,
}

impl Panes {
    fn new(count: usize) -> Result<Self> {
        let tabs = count.checked_mul(2).ok_or("pane count overflows")?;
        let capacity = count.checked_mul(4).ok_or("pane count overflows")?;
        let titles = (0..tabs)
            .map(|index| {
                format!(
                    "Pane {:04} {}",
                    index / 2 + 1,
                    if index % 2 == 0 { 'A' } else { 'B' },
                )
            })
            .collect();
        Ok(Self {
            titles,
            observations: Vec::with_capacity(capacity),
        })
    }
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<WidgetText> {
        let index = usize::try_from(item.get().checked_sub(1)?).ok()?;
        self.titles.get(index).map(WidgetText::from)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        let bounds = ui.available_rect_before_wrap();
        let label = ui.label("Content").rect;
        let (square, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        ui.painter()
            .rect_filled(square, 0.0, Color32::from_rgb(80, 160, 220));
        self.observations.push(ContentObservation {
            pass: ui.ctx().cumulative_pass_nr(),
            item,
            bounds,
            clip: ui.clip_rect(),
            label,
            square,
            opacity: ui.opacity(),
        });
    }
}

fn balanced_node(first_pane: usize, count: usize, depth: usize) -> Result<DockspaceNode> {
    if count == 1 {
        let first = first_pane
            .checked_mul(2)
            .and_then(|index| index.checked_add(1))
            .and_then(|index| u64::try_from(index).ok())
            .ok_or("item identity overflows")?;
        return Ok(DockspaceNode::tabs([
            ItemId::new(first),
            ItemId::new(first.checked_add(1).ok_or("item identity overflows")?),
        ]));
    }
    let half = count / 2;
    DockspaceNode::equal_split(
        if depth % 2 == 0 {
            DockspaceAxis::Horizontal
        } else {
            DockspaceAxis::Vertical
        },
        [
            balanced_node(first_pane, half, depth + 1)?,
            balanced_node(first_pane + half, half, depth + 1)?,
        ],
    )
    .map_err(|error| error.to_string())
}

#[derive(Clone, Copy, Default)]
struct Counts {
    frames: usize,
    passes: usize,
    content_calls: usize,
    visible_contents: usize,
    vertices: usize,
    indices: usize,
    draw_commands: usize,
}

impl Counts {
    fn add(&mut self, other: Self) {
        self.frames += other.frames;
        self.passes += other.passes;
        self.content_calls += other.content_calls;
        self.visible_contents += other.visible_contents;
        self.vertices += other.vertices;
        self.indices += other.indices;
        self.draw_commands += other.draw_commands;
    }
}

struct Frame {
    allocation: Allocation,
    elapsed_ns: u128,
    meshes: Vec<egui::ClippedPrimitive>,
    counts: Counts,
    observations: Range<usize>,
    first_titles: [Option<Rect>; 2],
    callbacks: usize,
    pixels_per_point: f32,
    accessibility_output: bool,
    response: egui_dockspace::DockspaceResponse,
}

struct Harness {
    context: Context,
    dockspace: Dockspace,
    panes: Panes,
    count: usize,
    frame_number: u64,
    width: f32,
    right_edge: Option<f32>,
}

impl Harness {
    fn new(count: usize, scenario: Scenario) -> Result<Self> {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, balanced_node(0, count, 0)?),
        )])
        .map_err(|error| error.to_string())?;
        let mut policy = DockPolicy::default();
        policy.set_close_capability(CloseCapability::Disabled);
        let style = DockStyle {
            minimum_pane_size: Vec2::new(64.0, 48.0),
            ..DockStyle::default()
        };
        let dockspace = Dockspace::builder(("product_cost", count, scenario.name()), layout)
            .policy(policy)
            .style(style)
            .build()
            .map_err(|error| error.to_string())?;
        let context = Context::default();
        context.set_pixels_per_point(1.0);
        context.global_style_mut(|style| {
            style
                .text_styles
                .insert(TextStyle::Body, FontId::proportional(14.0));
        });
        Ok(Self {
            context,
            dockspace,
            panes: Panes::new(count)?,
            count,
            frame_number: 0,
            width: WIDTH,
            right_edge: None,
        })
    }

    fn frame(
        &mut self,
        width: f32,
        pointer: Option<(Pos2, Option<bool>)>,
        inspect_titles: bool,
    ) -> Result<Frame> {
        let heap_before = dhat::HeapStats::get();
        let started = Instant::now();
        let mut events = Vec::new();
        if let Some((position, pressed)) = pointer {
            events.push(Event::PointerMoved(position));
            if let Some(pressed) = pressed {
                events.push(Event::PointerButton {
                    pos: position,
                    button: PointerButton::Primary,
                    pressed,
                    modifiers: Modifiers::NONE,
                });
            }
        }
        self.frame_number += 1;
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, Vec2::new(width, HEIGHT))),
            time: Some(self.frame_number as f64 / 60.0),
            predicted_dt: 1.0 / 60.0,
            events,
            focused: true,
            ..RawInput::default()
        };
        let start = self.panes.observations.len();
        let (output, response) = self
            .dockspace
            .run_single_surface(&self.context, input, SURFACE, &mut self.panes)
            .map_err(|error| error.to_string())?;
        let (output, receipt) = output.into_parts();
        let first_titles = if inspect_titles {
            [
                title_bounds(&output.shapes, &self.panes.titles[0])?,
                title_bounds(&output.shapes, &self.panes.titles[1])?,
            ]
        } else {
            [None, None]
        };
        let passes = output.platform_output.num_completed_passes;
        let pixels_per_point = output.pixels_per_point;
        let accessibility_output = output.platform_output.accesskit_update.is_some();
        let meshes = self.context.tessellate(output.shapes, pixels_per_point);
        receipt.report(SurfacePresentationResult::Presented);
        let elapsed_ns = started.elapsed().as_nanos();
        let heap_after = dhat::HeapStats::get();
        let allocation = Allocation {
            events: heap_after.total_blocks - heap_before.total_blocks,
            bytes: heap_after.total_bytes - heap_before.total_bytes,
            live_before: heap_before.curr_bytes,
            live_after: heap_after.curr_bytes,
            prefix_peak: heap_after.max_bytes,
        };
        let mut counts = Counts {
            frames: 1,
            passes,
            content_calls: self.panes.observations.len() - start,
            ..Counts::default()
        };
        let mut callbacks = 0;
        for primitive in &meshes {
            match &primitive.primitive {
                egui::epaint::Primitive::Mesh(mesh) => {
                    counts.vertices += mesh.vertices.len();
                    counts.indices += mesh.indices.len();
                    counts.draw_commands += 1;
                }
                egui::epaint::Primitive::Callback(_) => callbacks += 1,
            }
        }
        Ok(Frame {
            allocation,
            elapsed_ns,
            meshes,
            counts,
            observations: start..self.panes.observations.len(),
            first_titles,
            callbacks,
            pixels_per_point,
            accessibility_output,
            response,
        })
    }

    fn validate_frame(&self, frame: &Frame, width: f32) -> Result<(Counts, f32)> {
        if frame.meshes.is_empty()
            || frame.counts.passes == 0
            || frame.counts.vertices == 0
            || frame.counts.indices == 0
            || frame.counts.draw_commands == 0
            || frame.callbacks != 0
        {
            return Err("the complete frame must produce positive meshes without callbacks".into());
        }
        if frame.accessibility_output || frame.pixels_per_point != 1.0 {
            return Err("the frame must use scale 1 without an accessibility tree".into());
        }
        if frame.response.surface_status() != DockspaceSurfaceStatus::Ready
            || !matches!(
                frame.response.surface_commit_status(),
                DockspaceSurfaceCommitStatus::Ready | DockspaceSurfaceCommitStatus::Retained
            )
            || !frame.response.missing_panes().is_empty()
            || frame.response.presentation_settlement_capability() != DockspaceCapability::Supported
        {
            return Err(format!(
                "surface not ready: paint={:?}, commit={:?}, settlement={:?}, missing={:?}",
                frame.response.surface_status(),
                frame.response.surface_commit_status(),
                frame.response.presentation_settlement_capability(),
                frame.response.missing_panes(),
            ));
        }
        let expected = Rect::from_min_size(Pos2::ZERO, Vec2::new(width, HEIGHT));
        let viewport_bounds = self.context.input(egui::InputState::content_rect);
        if viewport_bounds != expected {
            return Err(format!(
                "actual viewport bounds {viewport_bounds:?} differ from requested {expected:?}",
            ));
        }
        let observations = &self.panes.observations[frame.observations.clone()];
        let mut offset = 0;
        let mut passes = 0;
        let mut visible_contents = 0;
        let mut right_edge = f32::NEG_INFINITY;
        let mut seen = vec![false; self.count];
        while offset < observations.len() {
            let pass = observations[offset].pass;
            seen.fill(false);
            let mut visible = 0;
            right_edge = f32::NEG_INFINITY;
            while offset < observations.len() && observations[offset].pass == pass {
                let observation = &observations[offset];
                offset += 1;
                // Geometry is counted for every actual pass, including a pass
                // whose output egui later discards in favor of a repeated pass.
                if observation.opacity <= 0.0
                    || !observation.bounds.is_positive()
                    || !observation.bounds.intersect(observation.clip).is_positive()
                {
                    continue;
                }
                if !observation.bounds.is_finite() {
                    return Err(format!(
                        "pane {:?} has invalid content bounds",
                        observation.item
                    ));
                }
                let pane = observation
                    .item
                    .get()
                    .checked_sub(1)
                    .and_then(|item| usize::try_from(item / 2).ok())
                    .filter(|&pane| pane < self.count)
                    .ok_or_else(|| format!("unexpected pane item {:?}", observation.item))?;
                if seen[pane] {
                    return Err(format!("pane {pane} rendered twice visibly in pass {pass}"));
                }
                if !expected.contains_rect(observation.bounds)
                    || !observation.clip.contains_rect(observation.bounds)
                    || !observation.label.intersect(observation.clip).is_positive()
                    || !observation.clip.contains_rect(observation.square)
                    || observation.square.size() != Vec2::splat(8.0)
                {
                    return Err(format!(
                        "pane {pane} did not expose its complete content payload"
                    ));
                }
                seen[pane] = true;
                visible += 1;
                right_edge = right_edge.max(observation.bounds.max.x);
            }
            if visible != self.count {
                return Err(format!(
                    "pass {pass} exposed {visible} selected panes, expected {}",
                    self.count,
                ));
            }
            visible_contents += visible;
            passes += 1;
        }
        if passes != frame.counts.passes {
            return Err(format!(
                "observed {passes} content passes, egui completed {}",
                frame.counts.passes,
            ));
        }
        let mut counts = frame.counts;
        counts.visible_contents = visible_contents;
        Ok((counts, right_edge))
    }

    fn warm_ready(&mut self) -> Result<()> {
        let mut last = String::new();
        for _ in 0..READY_LIMIT {
            self.panes.observations.clear();
            let frame = self.frame(WIDTH, None, false)?;
            match self.validate_frame(&frame, WIDTH) {
                Ok((_, right_edge)) => {
                    self.right_edge = Some(right_edge);
                    return Ok(());
                }
                Err(error) => last = error,
            }
        }
        Err(format!("readiness exceeded {READY_LIMIT} frames: {last}"))
    }

    fn selected_first(&self) -> Result<ItemId> {
        [ItemId::new(1), ItemId::new(2)]
            .into_iter()
            .find(|&item| {
                self.dockspace
                    .view()
                    .item(item)
                    .is_some_and(|item| item.is_selected())
            })
            .ok_or_else(|| "the first pane has no selected tab".into())
    }

    fn sample(&mut self, scenario: Scenario) -> Result<(u128, Counts, Allocation)> {
        let mut target = None;
        let mut position = None;
        if matches!(scenario, Scenario::TabClick) {
            self.panes.observations.clear();
            let preparation = self.frame(self.width, None, true)?;
            self.validate_frame(&preparation, self.width)?;
            let selected = self.selected_first()?;
            let inactive = if selected == ItemId::new(1) { 1 } else { 0 };
            let bounds = preparation.first_titles[inactive]
                .ok_or("the inactive first-pane title is not present in the actual output")?;
            let point = bounds.center();
            drop(preparation);
            self.panes.observations.clear();
            let hover = self.frame(self.width, Some((point, None)), true)?;
            let (_, right_edge) = self.validate_frame(&hover, self.width)?;
            self.right_edge = Some(right_edge);
            let hover_bounds = hover.first_titles[inactive]
                .ok_or("the inactive first-pane title disappeared during hover")?;
            if !hover_bounds.contains(point) {
                return Err("the actual hovered title no longer contains the pointer".into());
            }
            if self.selected_first()? != selected {
                return Err("a hover-only preparation frame changed the selected tab".into());
            }
            position = Some(point);
            target = Some(ItemId::new(inactive as u64 + 1));
        }
        let width = if matches!(scenario, Scenario::SurfaceResize) {
            if self.width == WIDTH {
                RESIZED_WIDTH
            } else {
                WIDTH
            }
        } else {
            self.width
        };
        self.panes.observations.clear();
        let first = self.frame(
            width,
            position.map(|position| (position, Some(true))),
            false,
        )?;
        let mut elapsed_ns = first.elapsed_ns;
        let mut allocation = first.allocation;
        let (mut counts, mut right_edge) = self.validate_frame(&first, width)?;
        if matches!(scenario, Scenario::SurfaceResize) {
            let previous = self
                .right_edge
                .ok_or("missing preceding resized content bounds")?;
            if ((right_edge - previous) - (width - self.width)).abs() > 0.01 {
                return Err(format!(
                    "content right edge did not follow resize: {previous} -> {right_edge}, width {} -> {width}",
                    self.width,
                ));
            }
        }
        // CPU-ready draw data stays live through validation. Retire it before
        // the next frame, outside both frame timers, as the synchronous host does.
        drop(first);
        if let Some(target) = target {
            self.panes.observations.clear();
            let second = self.frame(
                width,
                position.map(|position| (position, Some(false))),
                false,
            )?;
            elapsed_ns += second.elapsed_ns;
            allocation.events += second.allocation.events;
            allocation.bytes += second.allocation.bytes;
            allocation.live_after = second.allocation.live_after;
            allocation.prefix_peak = allocation.prefix_peak.max(second.allocation.prefix_peak);
            let (second_counts, second_right_edge) = self.validate_frame(&second, width)?;
            counts.add(second_counts);
            right_edge = second_right_edge;
            if self.selected_first()? != target {
                return Err(format!("the real pointer click did not select {target:?}"));
            }
            let content = self.panes.observations[second.observations.clone()]
                .iter()
                .rev()
                .find(|observation| {
                    observation.item.get() <= 2
                        && observation.opacity > 0.0
                        && observation.bounds.intersect(observation.clip).is_positive()
                })
                .ok_or("the clicked pane did not render content")?;
            if content.item != target {
                return Err("the selected item and actual rendered pane disagree".into());
            }
            drop(second);
        }
        self.width = width;
        self.right_edge = Some(right_edge);
        Ok((elapsed_ns, counts, allocation))
    }
}

fn title_bounds(shapes: &[egui::epaint::ClippedShape], title: &str) -> Result<Option<Rect>> {
    let mut found = None;
    for shape in shapes {
        let egui::Shape::Text(text) = &shape.shape else {
            continue;
        };
        if text.galley.text() != title {
            continue;
        }
        let bounds = text
            .galley
            .rect
            .translate(text.pos.to_vec2())
            .intersect(shape.clip_rect);
        if text.angle != 0.0 || !bounds.is_finite() || !bounds.is_positive() {
            return Err(format!("title {title:?} has unusable actual geometry"));
        }
        if found.replace(bounds).is_some() {
            return Err(format!("title {title:?} was painted more than once"));
        }
    }
    Ok(found)
}

fn run() -> Result<()> {
    let options = Options::parse()?;
    let stdout = io::stdout();
    let mut output = io::BufWriter::new(stdout.lock());
    writeln!(output, "{HEADER}").map_err(|error| error.to_string())?;
    for count in options.panes {
        for scenario in [Scenario::Idle, Scenario::TabClick, Scenario::SurfaceResize] {
            if options
                .scenario
                .is_some_and(|selected| selected != scenario)
            {
                continue;
            }
            let _profiler = dhat::Profiler::builder().testing().trim_backtraces(Some(4)).build();
            let mut harness = Harness::new(count, scenario)?;
            harness
                .warm_ready()
                .map_err(|error| format!("panes={count} {}: {error}", scenario.name()))?;
            for _ in 0..options.warmup {
                harness.sample(scenario).map_err(|error| {
                    format!("panes={count} {} warmup: {error}", scenario.name())
                })?;
            }
            for sample in 0..options.samples {
                let (elapsed_ns, counts, allocation) = harness.sample(scenario).map_err(|error| {
                    format!("panes={count} {} sample={sample}: {error}", scenario.name())
                })?;
                writeln!(
                    output,
                    "egui_dockspace,{count},2,{},{sample},{},{elapsed_ns},{},{},{},{},{},{},{},{},{},{},{}",
                    scenario.name(),
                    counts.frames,
                    counts.passes,
                    counts.content_calls,
                    counts.visible_contents,
                    counts.vertices,
                    counts.indices,
                    counts.draw_commands,
                    allocation.events,
                    allocation.bytes,
                    allocation.live_before,
                    allocation.live_after,
                    allocation.prefix_peak,
                )
                .map_err(|error| error.to_string())?;
            }
        }
    }
    output.flush().map_err(|error| error.to_string())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("product_cost: {error}");
            ExitCode::FAILURE
        }
    }
}
