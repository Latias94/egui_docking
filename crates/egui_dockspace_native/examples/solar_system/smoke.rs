//! Real OS-window movement, programmatic root recovery and fresh window creation.

use std::time::{Duration, Instant};

use eframe::egui;

use super::{
    DockspaceActionOutcome, DockspaceActionStatus, LivePlanet, NativeActionStatus, NativeCommand,
    PLANETS, ROOT_SURFACE, RootId, SUN, SmokeResult, SolarApp,
};

const MOVEMENTS: usize = 4;

pub(super) struct Smoke {
    started: Instant,
    phase: Phase,
    result: SmokeResult,
}

#[derive(Clone, Copy)]
enum Phase {
    ObserveInitial,
    Pause {
        old: LivePlanet,
        movements: usize,
        stable_since: Instant,
    },
    AwaitRecoveryOutcome {
        old: LivePlanet,
        root: RootId,
    },
    Recover {
        old: LivePlanet,
    },
    ObserveReplacement {
        old: LivePlanet,
    },
    MoveReplacement {
        replacement: LivePlanet,
    },
    SettleReplacement {
        replacement: LivePlanet,
    },
    Complete,
}

impl Smoke {
    pub fn new(result: SmokeResult) -> Self {
        Self {
            started: Instant::now(),
            phase: Phase::ObserveInitial,
            result,
        }
    }

    pub fn fail(&mut self, context: &egui::Context, error: String) {
        self.finish(context, Err(error));
    }

    pub fn advance(&mut self, app: &mut SolarApp, context: &egui::Context) {
        if matches!(self.phase, Phase::Complete) {
            return;
        }
        match self.advance_inner(app) {
            Ok(true) => self.finish(context, Ok(())),
            Ok(false) => {}
            Err(error) => self.finish(context, Err(error)),
        }
    }

    fn advance_inner(&mut self, app: &mut SolarApp) -> Result<bool, String> {
        if self.started.elapsed() > Duration::from_secs(75) {
            return Err(format!(
                "timed out during {}; live={:?}, quiescent={}",
                self.phase_name(),
                app.live_planets(),
                app.dockspace.is_quiescent()
            ));
        }
        if !app.dockspace.with_view(|view| {
            view.item(SUN).is_some()
                && PLANETS
                    .iter()
                    .all(|planet| view.item(planet.item).is_some())
        }) {
            return Err("a presentation lifecycle operation removed application content".into());
        }
        match self.phase {
            Phase::ObserveInitial => {
                let live = app.live_planets();
                if app.launch.is_some() || !app.queue.is_empty() {
                    return Ok(false);
                }
                if !live.iter().all(|planet| {
                    planet.is_some_and(|planet| planet.observation.movements >= MOVEMENTS)
                }) {
                    return Ok(false);
                }
                let windows = live.map(|planet| planet.expect("all three planets were observed"));
                for (index, planet) in windows.iter().enumerate() {
                    for other in &windows[..index] {
                        if planet.observation.window == other.observation.window
                            || planet.observation.viewport == other.observation.viewport
                        {
                            return Err(
                                "three planets did not receive three distinct real windows".into(),
                            );
                        }
                    }
                    eprintln!(
                        "solar-smoke: {} observed surface={:?} viewport={:?} window={} actual_outer={:?} observed_moves={}",
                        PLANETS[index].name,
                        planet.surface,
                        planet.observation.viewport,
                        planet.observation.window,
                        planet.observation.geometry.physical,
                        planet.observation.movements
                    );
                }
                app.set_paused(true);
                app.status = "Smoke: paused; recovering Mercury with DockRootBack…".into();
                self.phase = Phase::Pause {
                    old: windows[0],
                    movements: windows[0].observation.movements,
                    stable_since: Instant::now(),
                };
            }
            Phase::Pause {
                old,
                movements,
                stable_since,
            } => {
                let Some(current) = app.live_planets()[0] else {
                    return Ok(false);
                };
                if current.observation.movements != movements {
                    self.phase = Phase::Pause {
                        old,
                        movements: current.observation.movements,
                        stable_since: Instant::now(),
                    };
                } else if stable_since.elapsed() >= Duration::from_millis(250)
                    && app.dockspace.is_quiescent()
                    && app.live_planets().iter().all(|planet| {
                        planet.is_some_and(|planet| {
                            app.dockspace.is_surface_presented(planet.surface)
                        })
                    })
                {
                    eprintln!(
                        "solar-smoke: all three child windows presented and quiescent after pausing"
                    );
                    let root = app
                        .dockspace
                        .with_view(|view| view.item(PLANETS[0].item).map(|item| item.root()))
                        .ok_or("Mercury disappeared before programmatic recovery")?;
                    app.dockspace
                        .request_command(NativeCommand::DockRootBack { root })
                        .map_err(|error| error.to_string())?;
                    self.phase = Phase::AwaitRecoveryOutcome { old, root };
                }
            }
            Phase::AwaitRecoveryOutcome { old, root } => {
                let Some(result) = app.dockspace.take_action_status() else {
                    return Ok(false);
                };
                match result.map_err(|error| error.to_string())? {
                    NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                        DockspaceActionOutcome::RootFloated {
                            root: recovered,
                            surface: ROOT_SURFACE,
                            ..
                        },
                    )) if recovered == root => self.phase = Phase::Recover { old },
                    unexpected => {
                        return Err(format!("programmatic recovery returned {unexpected:?}"));
                    }
                }
            }
            Phase::Recover { old } => {
                let recovered = app.dockspace.with_view(|view| {
                    view.item(PLANETS[0].item)
                        .is_some_and(|item| item.surface() == ROOT_SURFACE)
                        && view.surface(old.surface).is_none()
                });
                if recovered
                    && !app.dockspace.is_surface_presented(old.surface)
                    && app.dockspace.is_quiescent()
                {
                    eprintln!(
                        "solar-smoke: DockRootBack programmatic recovery returned Mercury to the host; old surface={:?} retired, viewport={:?}, window={}",
                        old.surface, old.observation.viewport, old.observation.window
                    );
                    app.queue.push_back(0);
                    app.status = "Smoke: Mercury recovered; launching a fresh window…".into();
                    self.phase = Phase::ObserveReplacement { old };
                }
            }
            Phase::ObserveReplacement { old } => {
                let Some(new) = app.live_planets()[0] else {
                    return Ok(false);
                };
                if app.launch.is_some()
                    || !app.queue.is_empty()
                    || !app.dockspace.is_surface_presented(new.surface)
                    || !app.dockspace.is_quiescent()
                {
                    return Ok(false);
                }
                if new.surface == old.surface
                    || new.observation.viewport == old.observation.viewport
                {
                    return Err(format!(
                        "recreated window reused its retired incarnation: old={old:?}, new={new:?}"
                    ));
                }
                // Native IDs may be recycled after destruction. The fresh surface
                // and viewport identify the new logical/native incarnation.
                app.set_paused(false);
                self.phase = Phase::MoveReplacement { replacement: new };
            }
            Phase::MoveReplacement { replacement } => {
                let Some(new) = app.live_planets()[0] else {
                    return Ok(false);
                };
                if new.surface != replacement.surface
                    || new.observation.viewport != replacement.observation.viewport
                    || new.observation.window != replacement.observation.window
                {
                    return Err("the replacement changed identity during its orbit".into());
                }
                if new.observation.movements < replacement.observation.movements + MOVEMENTS {
                    return Ok(false);
                }
                app.set_paused(true);
                self.phase = Phase::SettleReplacement { replacement };
            }
            Phase::SettleReplacement { replacement } => {
                let Some(new) = app.live_planets()[0] else {
                    return Ok(false);
                };
                if new.surface != replacement.surface
                    || new.observation.viewport != replacement.observation.viewport
                    || new.observation.window != replacement.observation.window
                {
                    return Err("the replacement changed identity before final presentation".into());
                }
                if !app.dockspace.is_surface_presented(new.surface) || !app.dockspace.is_quiescent()
                {
                    return Ok(false);
                }
                eprintln!(
                    "solar-smoke: recreated Mercury surface={:?} viewport={:?} window={} actual_outer={:?} observed_moves_after_resume={}; three real children moved, paused, one programmatically recovered and a fresh incarnation moved again",
                    new.surface,
                    new.observation.viewport,
                    new.observation.window,
                    new.observation.geometry.physical,
                    new.observation.movements - replacement.observation.movements
                );
                return Ok(true);
            }
            Phase::Complete => return Ok(true),
        }
        Ok(false)
    }

    fn phase_name(&self) -> &'static str {
        match self.phase {
            Phase::ObserveInitial => "initial presentation and movement",
            Phase::Pause { .. } => "pause stabilization",
            Phase::AwaitRecoveryOutcome { .. } => "programmatic root recovery outcome",
            Phase::Recover { .. } => "child retirement after programmatic recovery",
            Phase::ObserveReplacement { .. } => "replacement presentation and movement",
            Phase::MoveReplacement { .. } => "replacement movement after resume",
            Phase::SettleReplacement { .. } => "replacement presentation after pausing",
            Phase::Complete => "completion",
        }
    }

    fn finish(&mut self, context: &egui::Context, result: Result<(), String>) {
        if matches!(self.phase, Phase::Complete) {
            return;
        }
        match &result {
            Ok(()) => eprintln!("solar-smoke: PASS"),
            Err(error) => eprintln!("solar-smoke: FAIL {error}"),
        }
        *self
            .result
            .lock()
            .expect("the smoke result lock is not poisoned") = Some(result);
        self.phase = Phase::Complete;
        context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
    }
}
