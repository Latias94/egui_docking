//! Finite application-input journey through the demo's real owned-run path.
//! Driver observations and CSV formatting stay outside frame timing windows.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::path::PathBuf;
use std::time::{Duration, Instant};

use egui::{Event, Modifiers, PointerButton, Pos2, RawInput, Rect};
use egui_dockspace::{Dockspace, SurfacePresentationResult, WorkspaceVersion};

use super::editor::{CONSOLE, DemoPanes, EDITOR, INSPECTOR, MAIN_ROOT, OUTLINE, PREVIEW, PROFILER};

const WARMUP: usize = 8;
const SAMPLES: usize = 64;
const PHASE_FRAMES: usize = WARMUP + SAMPLES;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Phase {
    Setup,
    Idle,
    Hover,
    FocusText,
    Edit,
    Scroll,
    PressSplitter,
    Resize,
    ReleaseSplitter,
    PressTab,
    DragTab,
    ReleaseTab,
    Complete,
}

impl Phase {
    fn scenario(self) -> Option<&'static str> {
        match self {
            Self::Idle => Some("painted_idle"),
            Self::Hover => Some("hover"),
            Self::Edit => Some("text_edit"),
            Self::Scroll => Some("scroll"),
            Self::Resize => Some("splitter_drag"),
            Self::DragTab => Some("tab_drag"),
            _ => None,
        }
    }
}

pub(super) struct Timing {
    pub update_ns: u128,
    pub tessellate_ns: u128,
    pub paint_present_ns: u128,
    pub ui_to_present_ns: u128,
}

struct Sample {
    scenario: &'static str,
    sample: usize,
    timing: Timing,
    vertices: usize,
    indices: usize,
}

pub(super) struct Exercise {
    path: PathBuf,
    phase: Phase,
    step: usize,
    frames: usize,
    setup_drops: usize,
    started: Instant,
    samples: Vec<Sample>,
    tabs: BTreeMap<String, Rect>,
    splitter: Option<Pos2>,
    pointer: Option<Pos2>,
    version: Option<WorkspaceVersion>,
    original_text: String,
    original_width: f32,
    original_weights: [u32; 2],
    resized: bool,
}

impl Exercise {
    pub(super) fn from_args() -> Result<Option<Self>, Box<dyn std::error::Error>> {
        let mut args = std::env::args().skip(1);
        let Some(flag) = args.next() else {
            return Ok(None);
        };
        if flag != "--exercise" {
            return Err("usage: workbench [--exercise NEW.csv]".into());
        }
        let path = args.next().ok_or("--exercise requires a new CSV path")?;
        if args.next().is_some() {
            return Err("usage: workbench [--exercise NEW.csv]".into());
        }
        if PathBuf::from(&path).exists() {
            return Err("exercise CSV already exists".into());
        }
        Ok(Some(Self::new(path.into())))
    }

    fn new(path: PathBuf) -> Self {
        Self {
            path,
            phase: Phase::Setup,
            step: 0,
            frames: 0,
            setup_drops: 0,
            started: Instant::now(),
            samples: Vec::with_capacity(6 * SAMPLES),
            tabs: BTreeMap::new(),
            splitter: None,
            pointer: None,
            version: None,
            original_text: String::new(),
            original_width: 0.0,
            original_weights: [0; 2],
            resized: false,
        }
    }

    pub(super) fn require_complete(&self) -> Result<(), String> {
        if self.phase == Phase::Complete {
            Ok(())
        } else {
            Err(format!(
                "exercise stopped early at {:?}/{}",
                self.phase, self.step
            ))
        }
    }

    pub(super) fn first_drop(&self) -> bool {
        self.setup_drops == 0
    }

    pub(super) fn prepare(
        &mut self,
        input: &mut RawInput,
        panes: &DemoPanes,
    ) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(90) || self.frames >= 640 {
            return Err(format!(
                "exercise exceeded its bound at {:?}/{}",
                self.phase, self.step
            ));
        }
        input.events.clear();
        input.focused = true;
        input.events.push(Event::ModifiersChanged(Modifiers::NONE));
        let point = match self.phase {
            Phase::Hover => Some(panes.panes[&OUTLINE].content.unwrap().center()),
            Phase::FocusText => Some(panes.panes[&EDITOR].editor.unwrap().center()),
            Phase::Scroll => Some(panes.panes[&CONSOLE].scroll_bounds.unwrap().center()),
            Phase::PressSplitter => self.splitter,
            Phase::Resize => self
                .splitter
                .map(|p| p + egui::vec2(12.0 + (self.step % 24) as f32 * 2.0, 0.0)),
            Phase::ReleaseSplitter | Phase::ReleaseTab => self.pointer,
            Phase::PressTab => self.tabs.get("Preview").map(|r| r.min + r.size() * 0.35),
            Phase::DragTab => {
                let target = panes.panes[&OUTLINE].content.unwrap();
                Some(target.min + target.size() * 0.65 + egui::vec2((self.step % 3) as f32, 0.0))
            }
            _ => None,
        };
        if let Some(point) = point {
            input.events.push(Event::PointerMoved(point));
            self.pointer = Some(point);
        }
        match self.phase {
            Phase::FocusText => input.events.push(button(point.unwrap(), self.step == 0)),
            Phase::PressSplitter | Phase::PressTab => {
                if self.step != 0 {
                    input.events.push(button(point.unwrap(), true));
                }
            }
            Phase::ReleaseSplitter | Phase::ReleaseTab if self.step == 0 => {
                input.events.push(button(point.unwrap(), false));
            }
            Phase::Edit => input.events.push(Event::Text("~".into())),
            Phase::Scroll => input.events.push(Event::MouseWheel {
                unit: egui::MouseWheelUnit::Point,
                delta: egui::vec2(0.0, -24.0),
                phase: egui::TouchPhase::Move,
                source: egui::MouseWheelSource::Unknown,
                modifiers: Modifiers::NONE,
            }),
            _ => {}
        }
        Ok(())
    }

    pub(super) fn observe(&mut self, update: Option<&egui::accesskit::TreeUpdate>) {
        let Some(update) = update else { return };
        for (_, node) in &update.nodes {
            let Some(bounds) = node.bounds() else {
                continue;
            };
            // Dockspace's semantic node bounds are in egui logical coordinates.
            let rect = Rect::from_min_max(
                egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
            );
            if node.role() == egui::accesskit::Role::Tab
                && let Some(label) = node.label()
            {
                self.tabs.insert(label.into(), rect);
            }
            if self.phase == Phase::Setup
                && node.role() == egui::accesskit::Role::Splitter
                && node.label() == Some("Resize panes")
                && node.orientation() == Some(egui::accesskit::Orientation::Vertical)
                && rect.height() > 200.0
                && rect.center().x < 500.0
            {
                self.splitter = Some(rect.center());
            }
        }
    }

    pub(super) fn finish_frame(
        &mut self,
        panes: &DemoPanes,
        dock: &Dockspace,
        presentation: SurfacePresentationResult,
        primitives: &[egui::ClippedPrimitive],
        timing: Timing,
    ) -> Result<bool, String> {
        self.frames += 1;
        if presentation != SurfacePresentationResult::Presented {
            if self.phase == Phase::Setup {
                self.setup_drops += 1;
                return Ok(false);
            }
            return Err(format!(
                "exercise output dropped at {:?}/{}",
                self.phase, self.step
            ));
        }
        if matches!(
            self.phase,
            Phase::Idle | Phase::Hover | Phase::Edit | Phase::Scroll
        ) && self.version != Some(dock.version())
        {
            return Err(format!(
                "content-only phase {:?} changed docking version",
                self.phase
            ));
        }
        let (vertices, indices) = primitives.iter().fold((0, 0), |(v, i), primitive| {
            if let egui::epaint::Primitive::Mesh(mesh) = &primitive.primitive {
                (v + mesh.vertices.len(), i + mesh.indices.len())
            } else {
                (v, i)
            }
        });
        if vertices == 0 || indices == 0 {
            return Err("exercise produced no actual mesh data".into());
        }
        if let Some(scenario) = self.phase.scenario()
            && self.step >= WARMUP
        {
            self.samples.push(Sample {
                scenario,
                sample: self.step - WARMUP,
                timing,
                vertices,
                indices,
            });
        }
        if self.phase == Phase::Resize {
            self.resized |=
                (panes.panes[&OUTLINE].content.unwrap().width() - self.original_width).abs() > 2.0;
        }
        let length = if self.phase.scenario().is_some() {
            PHASE_FRAMES
        } else if matches!(
            self.phase,
            Phase::FocusText | Phase::PressSplitter | Phase::PressTab
        ) {
            2
        } else {
            8
        };
        self.step += 1;
        if self.step < length {
            return Ok(false);
        }
        self.phase = match self.phase {
            Phase::Setup => {
                if self.splitter.is_none()
                    || !self.tabs.contains_key("Preview")
                    || [EDITOR, OUTLINE, CONSOLE, PROFILER, INSPECTOR]
                        .iter()
                        .any(|id| panes.panes[id].content.is_none())
                {
                    return Err("initial editor geometry is incomplete".into());
                }
                self.original_text = panes.panes[&EDITOR].text.clone();
                self.version = Some(dock.version());
                self.original_width = panes.panes[&OUTLINE].content.unwrap().width();
                Phase::Idle
            }
            Phase::Idle => Phase::Hover,
            Phase::Hover => Phase::FocusText,
            Phase::FocusText => Phase::Edit,
            Phase::Edit => {
                let text = &panes.panes[&EDITOR].text;
                if text.matches('~').count() != PHASE_FRAMES
                    || text.replace('~', "") != self.original_text
                {
                    return Err("text input did not preserve and edit the actual document".into());
                }
                Phase::Scroll
            }
            Phase::Scroll => {
                if panes.panes[&CONSOLE].scroll_offset <= 0.0 {
                    return Err(format!(
                        "wheel input did not scroll the actual Console: bounds={:?}, content={:?}, pointer={:?}",
                        panes.panes[&CONSOLE].scroll_bounds,
                        panes.panes[&CONSOLE].content,
                        self.pointer
                    ));
                }
                self.original_weights = top_weights(dock);
                Phase::PressSplitter
            }
            Phase::PressSplitter => Phase::Resize,
            Phase::Resize => Phase::ReleaseSplitter,
            Phase::ReleaseSplitter => {
                if !self.resized
                    || self.version == Some(dock.version())
                    || top_weights(dock) == self.original_weights
                    || (panes.panes[&OUTLINE].content.unwrap().width() - self.original_width).abs()
                        <= 2.0
                {
                    return Err("splitter gesture did not change geometry and commit".into());
                }
                Phase::PressTab
            }
            Phase::PressTab => Phase::DragTab,
            Phase::DragTab => Phase::ReleaseTab,
            Phase::ReleaseTab => {
                let view = dock.view();
                let preview = view.item(PREVIEW).ok_or("Preview disappeared")?;
                if preview.tabs().items() != [OUTLINE, PREVIEW]
                    || !preview.is_selected()
                    || view.item(EDITOR).unwrap().tabs().items() != [EDITOR]
                    || panes.panes.len() != 8
                    || panes.panes.keys().any(|id| view.item(*id).is_none())
                    || panes.panes[&EDITOR].text.matches('~').count() != PHASE_FRAMES
                    || panes.panes[&EDITOR].text.replace('~', "") != self.original_text
                {
                    return Err(
                        "tab gesture did not produce the exact merge with retained content".into(),
                    );
                }
                self.write_csv()?;
                eprintln!(
                    "egui-host-exercise: complete; text, scroll, resize, exact tab merge, eight items and {} UI frame samples verified; startup_dropped_outputs={}",
                    self.samples.len(),
                    self.setup_drops
                );
                Phase::Complete
            }
            Phase::Complete => Phase::Complete,
        };
        self.step = 0;
        Ok(self.phase == Phase::Complete)
    }

    fn write_csv(&self) -> Result<(), String> {
        let file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&self.path)
            .map_err(|e| format!("create exercise CSV: {e}"))?;
        let mut out = io::BufWriter::new(file);
        writeln!(out, "scenario,sample,update_ns,tessellate_ns,paint_present_ns,ui_to_present_ns,vertices,indices")
            .map_err(|e| e.to_string())?;
        for sample in &self.samples {
            let t = &sample.timing;
            writeln!(
                out,
                "{},{},{},{},{},{},{},{}",
                sample.scenario,
                sample.sample,
                t.update_ns,
                t.tessellate_ns,
                t.paint_present_ns,
                t.ui_to_present_ns,
                sample.vertices,
                sample.indices
            )
            .map_err(|e| e.to_string())?;
        }
        out.flush().map_err(|e| e.to_string())
    }
}

fn button(pos: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::NONE,
    }
}

fn top_weights(dock: &Dockspace) -> [u32; 2] {
    let rows = dock
        .view()
        .root(MAIN_ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap();
    let top = rows.children().next().unwrap().split().unwrap();
    let mut weights = top.weights();
    [
        weights.next().unwrap().to_bits(),
        weights.next().unwrap().to_bits(),
    ]
}

#[cfg(test)]
#[path = "tests.rs"]
mod tests;
