//! Finite real-window workflow. An external OS-input owner supplies the edits and close.
//!
//! The diagnostic content rectangles locate input only; presented surfaces and
//! native quiescence remain the product readiness checks.

use std::time::{Duration, Instant};

use super::*;

#[derive(Clone, Copy)]
enum Stage {
    RootInput,
    FirstChild,
    ChildInput,
    DockBack,
    BackInput,
    SecondChild,
    CloseChild(SurfaceId),
    RecoveredInput,
    Complete,
}

pub(super) struct EditorSmoke {
    stage: Stage,
    started: Instant,
    expected: String,
    announced: bool,
    focus_announced: bool,
}

impl EditorSmoke {
    pub(super) fn new() -> Self {
        Self {
            stage: Stage::RootInput,
            started: Instant::now(),
            expected: ExamplePanes::new().panes[&INSPECTOR].text.clone(),
            announced: false,
            focus_announced: false,
        }
    }

    pub(super) fn advance(&mut self, app: &mut NativeExampleApp, context: &egui::Context) {
        if matches!(self.stage, Stage::Complete) {
            return;
        }
        if let Err(error) = self.advance_inner(app) {
            eprintln!("editor-smoke: FAIL {error}");
            self.stage = Stage::Complete;
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else if matches!(self.stage, Stage::Complete) {
            eprintln!(
                "editor-smoke: complete; eight stable panes, four real edits, first-live, Dock Back, OS-close recovery and quiescence verified"
            );
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else {
            // The initial content edit waits on real OS input after an idle
            // interval longer than the ingress age limit. Later root polls
            // also observe application state changed by deferred child UIs.
            let delay = if matches!(self.stage, Stage::RootInput) && self.announced {
                Duration::from_secs(10)
            } else {
                Duration::from_millis(16)
            };
            context.request_repaint_after(delay);
        }
    }

    fn advance_inner(&mut self, app: &mut NativeExampleApp) -> Result<(), String> {
        if self.started.elapsed() > Duration::from_secs(60) {
            return Err("the bounded editor workflow timed out".to_owned());
        }
        if let Some(kind) = app.dockspace.error_kind() {
            return Err(app.dockspace.with_errors(|primary, cleanup| {
                format!(
                    "native runtime stopped: {kind:?}; primary={primary:?}; cleanup={cleanup:?}"
                )
            }));
        }
        let location = app
            .dockspace
            .with_view(|view| {
                for item in [
                    OUTLINE, EDITOR, PREVIEW, INSPECTOR, CONSOLE, HISTORY, PROFILER, ASSETS,
                ] {
                    if view.item(item).is_none() {
                        return None;
                    }
                }
                view.item(INSPECTOR).map(InspectorLocation::from)
            })
            .flatten()
            .ok_or("one of the eight stable panes disappeared")?;
        match self.stage {
            Stage::RootInput if self.ready(app, location, false) => {
                if self.input(app, "ROOT_EDIT", false)? {
                    app.request_inspector_window(location.root);
                    self.set_stage(Stage::FirstChild);
                }
            }
            Stage::FirstChild if self.ready(app, location, true) => {
                self.check_text(app)?;
                self.set_stage(Stage::ChildInput);
            }
            Stage::ChildInput if self.ready(app, location, true) => {
                if self.input(app, "CHILD_EDIT", true)? {
                    app.request_inspector_dock_back(location.root);
                    self.set_stage(Stage::DockBack);
                }
            }
            Stage::DockBack if self.ready(app, location, false) => {
                self.check_text(app)?;
                self.set_stage(Stage::BackInput);
            }
            Stage::BackInput if self.ready(app, location, false) => {
                if self.input(app, "BACK_EDIT", false)? {
                    app.request_inspector_window(location.root);
                    self.set_stage(Stage::SecondChild);
                }
            }
            Stage::SecondChild if self.ready(app, location, true) => {
                self.check_text(app)?;
                eprintln!(
                    "editor-smoke: close-child; send a real OS close request to Dockspace panel"
                );
                self.set_stage(Stage::CloseChild(location.surface));
            }
            Stage::CloseChild(old_surface)
                if self.ready(app, location, false)
                    && !app.dockspace.is_surface_presented(old_surface) =>
            {
                self.check_text(app)?;
                self.set_stage(Stage::RecoveredInput);
            }
            Stage::RecoveredInput if self.ready(app, location, false) => {
                if self.input(app, "RECOVERED_EDIT", false)? {
                    self.stage = Stage::Complete;
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn ready(&self, app: &NativeExampleApp, location: InspectorLocation, native: bool) -> bool {
        location.is_native_child() == native
            && !app.action.is_pending()
            && app.dockspace.is_surface_presented(location.surface)
            && app.dockspace.is_quiescent()
    }

    fn check_text(&self, app: &NativeExampleApp) -> Result<(), String> {
        app.dockspace.with_panes(|panes| {
            (panes.panes[&INSPECTOR].text == self.expected)
                .then_some(())
                .ok_or_else(|| "presentation changed application-owned text".to_owned())
        })
    }

    fn input(
        &mut self,
        app: &NativeExampleApp,
        marker: &str,
        native: bool,
    ) -> Result<bool, String> {
        let (text, input) = app.dockspace.with_panes(|panes| {
            let inspector = &panes.panes[&INSPECTOR];
            (inspector.text.clone(), inspector.input)
        });
        let Some((viewport, rect, focused, scale)) = input else {
            return Ok(false);
        };
        if (viewport != egui::ViewportId::ROOT) != native {
            return Ok(false);
        }
        if !self.announced {
            eprintln!(
                "editor-smoke: input={marker} window={} x={:.1} y={:.1}",
                if native { "child" } else { "root" },
                rect.center().x * scale,
                rect.center().y * scale,
            );
            self.announced = true;
        }
        if focused && !self.focus_announced {
            eprintln!("editor-smoke: focused={marker}");
            self.focus_announced = true;
        }
        if text == format!("{}{marker}", self.expected) && focused {
            self.expected = text;
            eprintln!(
                "editor-smoke: accepted real focused edit {marker} item={}",
                INSPECTOR.get()
            );
            return Ok(true);
        }
        Ok(false)
    }

    fn set_stage(&mut self, stage: Stage) {
        self.stage = stage;
        self.announced = false;
        self.focus_announced = false;
    }
}
