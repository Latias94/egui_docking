//! Opt-in application diagnostics; these observations are not a replay protocol.

use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::*;

#[derive(Clone)]
pub(super) struct OperationLog(Arc<Mutex<LogFile>>);

struct LogFile {
    file: Option<File>,
    started: Instant,
    sequence: u64,
    last_move: Duration,
    viewport_geometry: BTreeMap<egui::ViewportId, String>,
    error_recorded: bool,
    cleanup_recorded: usize,
}

impl OperationLog {
    pub(super) fn from_args() -> io::Result<Option<Self>> {
        let mut args = std::env::args_os().skip(1);
        let mut path = None;
        while let Some(argument) = args.next() {
            if argument == "--event-log" {
                if path.is_some() {
                    return Err(io::Error::other("--event-log may be supplied only once"));
                }
                path = Some(PathBuf::from(args.next().ok_or_else(|| {
                    io::Error::other("--event-log requires a new log file path")
                })?));
            }
        }
        let Some(path) = path else { return Ok(None) };
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let log = Self(Arc::new(Mutex::new(LogFile {
            file: Some(file),
            started: Instant::now(),
            sequence: 0,
            last_move: Duration::ZERO,
            viewport_geometry: BTreeMap::new(),
            error_recorded: false,
            cleanup_recorded: 0,
        })));
        log.record("start", format!(
            "pid={} unix_ms={} version={} os={} arch={} serde={}; input observations and committed model snapshots; no replay authority",
            std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis(),
            env!("CARGO_PKG_VERSION"), std::env::consts::OS, std::env::consts::ARCH, cfg!(feature = "serde"),
        ));
        eprintln!("native-editor: operation log {}", path.display());
        Ok(Some(log))
    }

    pub(super) fn install(&self, context: &egui::Context) {
        context.add_plugin(InputLog(self.clone()));
    }

    pub(super) fn record(&self, kind: &str, detail: impl std::fmt::Display) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record(kind, detail);
    }

    pub(super) fn observe(&self, app: &NativeDockspaceApp<ExamplePanes>) {
        if app.error_kind().is_none() {
            return;
        }
        let mut log = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        app.with_errors(|primary, cleanup| {
            if !log.error_recorded {
                log.record("fatal", format!("{primary:#?}"));
            }
            for error in &cleanup[log.cleanup_recorded..] {
                log.record("cleanup-error", format!("{error:#?}"));
            }
            log.cleanup_recorded = cleanup.len();
        });
        if !log.error_recorded {
            log.error_recorded = true;
            log.record("state", app.with_view(|view| format!("{view:?}")));
            log.record(
                "pending",
                format!(
                    "{:?}; outputs={:?}",
                    app.pending_work(),
                    app.output_backlog()
                ),
            );
        }
    }
}

impl LogFile {
    fn record(&mut self, kind: &str, detail: impl std::fmt::Display) {
        let Some(file) = &mut self.file else { return };
        self.sequence += 1;
        // Direct writes keep the last interaction available while a failed UI
        // remains open. Motion is sampled below; content and typing are omitted.
        if let Err(error) = writeln!(
            file,
            "{}\t{}us\t{kind}\t{detail}",
            self.sequence,
            self.started.elapsed().as_micros()
        ) {
            eprintln!("native-editor: operation log failed: {error}");
            self.file = None;
        }
    }
}

struct InputLog(OperationLog);

impl egui::plugin::Plugin for InputLog {
    fn debug_name(&self) -> &'static str {
        "native-editor-operation-log"
    }

    fn input_hook(&mut self, _: &egui::Context, input: &mut egui::RawInput) {
        let mut log = self
            .0
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let geometry = format!(
            "screen={:?} viewport={:?}",
            input.screen_rect,
            input.viewports.get(&input.viewport_id)
        );
        if log.viewport_geometry.get(&input.viewport_id) != Some(&geometry) {
            log.record("viewport", format!("id={:?} {geometry}", input.viewport_id));
            log.viewport_geometry.insert(input.viewport_id, geometry);
        }
        for event in &input.events {
            let record = match event {
                egui::Event::PointerMoved(_) => {
                    let elapsed = log.started.elapsed();
                    let sampled =
                        elapsed.saturating_sub(log.last_move) >= Duration::from_millis(32);
                    if sampled {
                        log.last_move = elapsed;
                    }
                    sampled
                }
                egui::Event::PointerButton { .. }
                | egui::Event::PointerGone
                | egui::Event::WindowFocused(_)
                | egui::Event::ModifiersChanged(_)
                | egui::Event::MouseWheel { .. }
                | egui::Event::Key {
                    key: egui::Key::Escape,
                    ..
                } => true,
                _ => false,
            };
            if record {
                log.record(
                    "input",
                    format!(
                        "viewport={:?} output={:?} time={:?} {event:?}",
                        input.viewport_id,
                        eframe::current_native_output_token(),
                        input.time
                    ),
                );
            }
        }
    }
}
