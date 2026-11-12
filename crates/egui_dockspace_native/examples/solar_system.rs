//! Three real managed child windows orbit an application-owned central Sun.
//! Run with `--smoke` for a finite create, move, programmatic recovery and recreate journey.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dockspace::geometry::{LogicalRect, PhysicalRect};
use dockspace::model::{
    DockspaceActionOutcome, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    NativeTearOffSource, NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::DockspaceSession;
use eframe::egui;
use egui_dockspace::{DockStyle, DockspaceActionStatus};
use egui_dockspace_native::{NativeActionStatus, NativeCommand, NativeDockspaceApp};
use winit::raw_window_handle::HasDisplayHandle;

#[path = "solar_system/orbit.rs"]
mod orbit;
#[path = "solar_system/panes.rs"]
mod panes;
#[path = "solar_system/smoke.rs"]
mod smoke;

use orbit::{AnimationClock, GeometryAvailability, WindowGeometry};
use panes::{Observation, Panes};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const SUN: ItemId = ItemId::new(1);
const CHILD_SIZE: egui::Vec2 = egui::vec2(210.0, 150.0);

struct Planet {
    item: ItemId,
    name: &'static str,
    color: egui::Color32,
    phase: f32,
    lane: f32,
}

const PLANETS: [Planet; 3] = [
    Planet {
        item: ItemId::new(2),
        name: "Mercury",
        color: egui::Color32::from_rgb(146, 144, 158),
        phase: 0.0,
        lane: 0.0,
    },
    Planet {
        item: ItemId::new(3),
        name: "Venus",
        color: egui::Color32::from_rgb(203, 131, 77),
        phase: std::f32::consts::TAU / 3.0,
        lane: 8.0,
    },
    Planet {
        item: ItemId::new(4),
        name: "Earth",
        color: egui::Color32::from_rgb(65, 141, 188),
        phase: std::f32::consts::TAU * 2.0 / 3.0,
        lane: 16.0,
    },
];

type SmokeResult = Arc<Mutex<Option<Result<(), String>>>>;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let smoke_mode = std::env::args().any(|argument| argument == "--smoke");
    let outcome: SmokeResult = Arc::default();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let session = DockspaceSession::from_layout(initial_layout(), policy)?;
    let dockspace = NativeDockspaceApp::new(
        egui::Id::unique("solar-system"),
        session,
        ROOT_SURFACE,
        Panes::default(),
        DockStyle::default(),
    )?;
    let native_host = dockspace.native_host_handler();
    let app = SolarApp {
        dockspace,
        queue: (0..PLANETS.len()).collect(),
        launch: None,
        last_frame: Instant::now(),
        animation: AnimationClock::default(),
        status: "Launching three planet windows…".to_owned(),
        smoke: smoke_mode.then(|| smoke::Smoke::new(Arc::clone(&outcome))),
    };
    eframe::run_native(
        "Dockspace · Solar system",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([500.0, 300.0])
                .with_min_inner_size([400.0, 250.0]),
            centered: true,
            native_host: Some(native_host),
            ..Default::default()
        },
        Box::new(move |creation| {
            creation.egui_ctx.set_theme(egui::Theme::Light);
            Ok(Box::new(app))
        }),
    )?;
    if smoke_mode {
        let result = outcome
            .lock()
            .map_err(|_| std::io::Error::other("solar smoke result lock was poisoned"))?
            .take()
            .unwrap_or_else(|| Err("application closed before the smoke journey completed".into()));
        result.map_err(std::io::Error::other)?;
    }
    Ok(())
}

fn initial_layout() -> DockspaceLayout {
    let mut surface = DockspaceSurfaceLayout::new(
        ROOT_SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([SUN])),
    );
    for (planet, offset) in PLANETS.iter().zip([0.0, 1.0, 2.0]) {
        let id = planet.item.get();
        surface = surface.with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(id),
            DockspaceRootLayout::new(RootId::new(id), DockspaceNode::tabs([planet.item])),
            LogicalRect::new(45.0 + offset * 65.0, 40.0 + offset * 30.0, 210.0, 150.0)
                .expect("the initial contained bounds are valid"),
        ));
    }
    DockspaceLayout::new([surface]).expect("the Sun and three distinct planets form a valid layout")
}

struct SolarApp {
    dockspace: NativeDockspaceApp<Panes>,
    queue: VecDeque<usize>,
    launch: Option<Launch>,
    last_frame: Instant,
    animation: AnimationClock,
    status: String,
    smoke: Option<smoke::Smoke>,
}

#[derive(Clone, Copy)]
enum Launch {
    AwaitingOutcome { planet: usize },
    AwaitingPresentation { planet: usize, surface: SurfaceId },
}

#[derive(Clone, Copy, Debug)]
struct LivePlanet {
    surface: SurfaceId,
    observation: Observation,
}

impl SolarApp {
    fn surfaces(&self) -> [Option<SurfaceId>; 3] {
        self.dockspace.with_view(|view| {
            PLANETS
                .each_ref()
                .map(|planet| view.item(planet.item).map(|item| item.surface()))
        })
    }

    fn live_planets(&self) -> [Option<LivePlanet>; 3] {
        let surfaces = self.surfaces();
        self.dockspace.with_panes(|panes| {
            std::array::from_fn(|planet| {
                let surface = surfaces[planet].filter(|surface| *surface != ROOT_SURFACE)?;
                Some(LivePlanet {
                    surface,
                    observation: panes.observations[planet]?,
                })
            })
        })
    }

    fn launch_returned(&mut self) {
        let surfaces = self.surfaces();
        for (planet, surface) in surfaces.into_iter().enumerate() {
            if surface == Some(ROOT_SURFACE) && !self.queue.contains(&planet) {
                self.queue.push_back(planet);
            }
        }
        if !self.queue.is_empty() {
            self.status = "Launching returned planets…".into();
        }
    }

    fn cancel_queued_launches(&mut self) {
        self.queue.clear();
        self.status = "Queued launches cancelled; use Launch returned planets to retry".into();
    }

    fn set_paused(&self, paused: bool) {
        self.dockspace
            .with_panes_mut(|panes| panes.orbit.paused = paused);
    }

    fn advance_launch(&mut self, geometry: GeometryAvailability) -> Result<(), String> {
        if let Some(kind) = self.dockspace.error_kind() {
            return Err(self.dockspace.with_errors(|primary, cleanup| {
                format!("native runtime stopped: {kind:?}; {primary:?}; {cleanup:?}")
            }));
        }
        if let Some(Launch::AwaitingOutcome { planet }) = self.launch {
            let Some(status) = self.dockspace.take_action_status() else {
                return Ok(());
            };
            match status.map_err(|error| error.to_string())? {
                NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::NativeRootTearOffRequested {
                        target_surface,
                        items,
                        ..
                    },
                )) if items == [PLANETS[planet].item] => {
                    if self.smoke.is_some() {
                        eprintln!(
                            "solar-smoke: {} launch accepted for {target_surface:?}",
                            PLANETS[planet].name
                        );
                    }
                    self.launch = Some(Launch::AwaitingPresentation {
                        planet,
                        surface: target_surface,
                    });
                }
                unexpected => return Err(format!("launch returned {unexpected:?}")),
            }
        }
        if let Some(Launch::AwaitingPresentation { planet, surface }) = self.launch {
            if !self.dockspace.is_surface_presented(surface) || !self.dockspace.is_quiescent() {
                return Ok(());
            }
            self.status = format!("{} is in orbit", PLANETS[planet].name);
            if self.smoke.is_some() {
                eprintln!(
                    "solar-smoke: {} presented and quiescent on {surface:?}",
                    PLANETS[planet].name
                );
            }
            self.launch = None;
        }
        if self.launch.is_none() && !self.queue.is_empty() {
            match geometry {
                GeometryAvailability::Unsupported => return Err(
                    "Orbital windows require global window positions, which Wayland does not provide. Use an X11 session or another desktop backend.".into(),
                ),
                GeometryAvailability::Waiting => {
                    self.status = "Waiting for the desktop window position; launches resume when geometry is observed, or cancel the queue to retry later".into();
                    return Ok(());
                }
                GeometryAvailability::Ready => {}
            }
        }
        if self.launch.is_some()
            || !self.dockspace.is_quiescent()
            || !self.dockspace.is_surface_presented(ROOT_SURFACE)
        {
            return Ok(());
        }
        let Some(&planet) = self.queue.front() else {
            return Ok(());
        };
        if self.surfaces()[planet] != Some(ROOT_SURFACE) {
            self.queue.pop_front();
            return Ok(());
        }
        let placement = self.dockspace.with_panes(|panes| {
            let root = panes.orbit.root?;
            // Creation dimensions are a request, not an inferred child scale.
            // Once live, every movement uses that child's observed geometry.
            let size = CHILD_SIZE * root.pixels_per_point;
            let position = panes.orbit.physical_position(planet, size)?;
            PhysicalRect::new(
                f64::from(position.x),
                f64::from(position.y),
                f64::from(size.x),
                f64::from(size.y),
            )
            .ok()
            .map(NativeWindowPlacement::new)
        });
        let Some(placement) = placement else {
            return Ok(());
        };
        self.dockspace
            .request_command(NativeCommand::MoveToNewWindow {
                source: NativeTearOffSource::Item(PLANETS[planet].item),
                placement,
            })
            .map_err(|error| error.to_string())?;
        self.dockspace
            .with_panes_mut(|panes| panes.observations[planet] = None);
        self.queue.pop_front();
        self.launch = Some(Launch::AwaitingOutcome { planet });
        Ok(())
    }
}

impl eframe::App for SolarApp {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let context = ui.ctx().clone();
        let now = Instant::now();
        let elapsed = now.duration_since(std::mem::replace(&mut self.last_frame, now));
        let root = WindowGeometry::observed(
            context.input(|input| {
                input
                    .raw
                    .viewports
                    .get(&egui::ViewportId::ROOT)
                    .and_then(|viewport| viewport.outer_rect)
            }),
            context.pixels_per_point(),
        );
        let geometry = GeometryAvailability::observed(
            frame.display_handle().ok().map(|display| display.as_raw()),
            root,
        );
        let surfaces = self.surfaces();
        let launching = self.launch.is_some() || !self.queue.is_empty();
        self.dockspace.with_panes_mut(|panes| {
            panes.orbit.root = root;
            panes.reconcile_surfaces(surfaces, &context);
            // A manually combined window stays placed instead of receiving two orbits.
            panes.orbiting = std::array::from_fn(|planet| {
                !launching
                    && surfaces[planet].is_some_and(|surface| surface != ROOT_SURFACE)
                    && surfaces
                        .iter()
                        .filter(|surface| **surface == surfaces[planet])
                        .count()
                        == 1
            });
            let pointer_down = panes.any_pointer_down(&context);
            panes.orbit.tick(elapsed, pointer_down || launching);
        });
        let mut paused = self.dockspace.with_panes(|panes| panes.orbit.paused);
        let mut launch = false;
        let mut cancel = false;
        egui::Panel::top("solar-controls").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("Solar system");
                ui.add_enabled_ui(self.smoke.is_none(), |ui| {
                    ui.checkbox(&mut paused, "Pause orbit");
                    launch = ui
                        .add_enabled(
                            self.launch.is_none()
                                && self.queue.is_empty()
                                && geometry != GeometryAvailability::Unsupported,
                            egui::Button::new("Launch returned planets"),
                        )
                        .clicked();
                    if !self.queue.is_empty() {
                        cancel = ui.button("Cancel queued launches").clicked();
                    }
                });
            });
            ui.small(&self.status);
        });
        self.set_paused(paused);
        if launch {
            self.launch_returned();
        }
        if cancel {
            self.cancel_queued_launches();
        }
        self.dockspace.ui(ui, frame);
        let surfaces = self.surfaces();
        self.dockspace
            .with_panes_mut(|panes| panes.reconcile_surfaces(surfaces, &context));
        if let Err(error) = self.advance_launch(geometry) {
            self.set_paused(true);
            self.status.clone_from(&error);
            self.queue.clear();
            if let Some(smoke) = &mut self.smoke {
                smoke.fail(&context, error);
            }
        }
        if let Some(mut smoke) = self.smoke.take() {
            smoke.advance(self, &context);
            self.smoke = Some(smoke);
        }
        let animating = self.dockspace.with_panes(|panes| {
            !panes.orbit.paused && panes.orbiting.into_iter().any(|orbiting| orbiting)
        });
        if self.animation.due(Instant::now(), animating) {
            self.dockspace
                .with_panes(|panes| panes.move_planets(&context));
        }
        if let Some(delay) = self.animation.delay(Instant::now()) {
            context.request_repaint_after(delay);
        } else {
            context.request_repaint_after(if geometry == GeometryAvailability::Waiting {
                Duration::from_millis(500)
            } else {
                Duration::from_millis(100)
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> SolarApp {
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let session = DockspaceSession::from_layout(initial_layout(), policy).unwrap();
        SolarApp {
            dockspace: NativeDockspaceApp::new(
                egui::Id::unique("solar-launch-tests"),
                session,
                ROOT_SURFACE,
                Panes::default(),
                DockStyle::default(),
            )
            .unwrap(),
            queue: (0..PLANETS.len()).collect(),
            launch: None,
            last_frame: Instant::now(),
            animation: AnimationClock::default(),
            status: String::new(),
            smoke: None,
        }
    }

    #[test]
    fn first_frame_without_geometry_preserves_a_cancellable_and_retryable_launch_queue() {
        let mut app = app();
        app.advance_launch(GeometryAvailability::Waiting).unwrap();
        assert_eq!(app.queue.len(), PLANETS.len());
        assert!(
            app.status
                .starts_with("Waiting for the desktop window position")
        );
        assert!(app.launch.is_none());
        assert!(app.dockspace.take_action_status().is_none());
        app.cancel_queued_launches();
        assert!(app.queue.is_empty());
        app.launch_returned();
        assert_eq!(app.queue.len(), PLANETS.len());
        app.advance_launch(GeometryAvailability::Waiting).unwrap();
        assert!(app.launch.is_none());
        assert!(app.dockspace.take_action_status().is_none());
    }

    #[test]
    fn unsupported_global_placement_returns_a_specific_error_without_waiting_for_smoke_timeout() {
        let mut app = app();
        let error = app
            .advance_launch(GeometryAvailability::Unsupported)
            .unwrap_err();
        assert!(error.contains("Wayland does not provide"));
        assert!(app.launch.is_none());
        assert!(app.dockspace.take_action_status().is_none());
        let result: SmokeResult = Arc::default();
        let mut smoke = smoke::Smoke::new(Arc::clone(&result));
        smoke.fail(&egui::Context::default(), error);
        assert!(
            result
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .as_ref()
                .unwrap_err()
                .contains("Wayland does not provide")
        );
    }
}
