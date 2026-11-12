//! Pane content and observations from the real viewport currently painting it.

use std::time::{Duration, Instant};

use eframe::egui;
use egui_dockspace::{PaneResponse, PaneView};

use super::{
    ItemId, ORBIT_INTERVAL, PLANETS, ROOT_SURFACE, SUN, SurfaceId, orbit::Orbit,
    orbit::WindowGeometry,
};

#[derive(Clone, Copy, Debug)]
pub(super) struct Observation {
    pub viewport: egui::ViewportId,
    pub window: u64,
    pub geometry: WindowGeometry,
    pub movements: usize,
}

#[derive(Default)]
pub(super) struct Panes {
    pub orbit: Orbit,
    pub observations: [Option<Observation>; 3],
    pub orbiting: [bool; 3],
    surfaces: [Option<SurfaceId>; 3],
    last_move: [Option<Instant>; 3],
}

impl Panes {
    pub fn reconcile_surfaces(
        &mut self,
        surfaces: [Option<SurfaceId>; 3],
        context: &egui::Context,
    ) {
        for (planet, surface) in surfaces.iter().copied().enumerate() {
            let retired = self.observations[planet].is_some_and(|observation| {
                !context.input(|input| input.raw.viewports.contains_key(&observation.viewport))
            });
            if self.surfaces[planet] != surface
                || surface == Some(ROOT_SURFACE)
                || surface.is_none()
                || retired
            {
                self.observations[planet] = None;
                self.last_move[planet] = None;
                self.orbiting[planet] = false;
            }
        }
        self.surfaces = surfaces;
        for (planet, surface) in surfaces.iter().enumerate() {
            if surfaces.iter().filter(|other| *other == surface).count() != 1 {
                self.orbiting[planet] = false;
            }
        }
    }

    pub fn any_pointer_down(&self, context: &egui::Context) -> bool {
        pointer_down_if_live(context, egui::ViewportId::ROOT)
            || self
                .observations
                .iter()
                .zip(self.surfaces)
                .any(|(observation, surface)| {
                    surface.is_some_and(|surface| surface != ROOT_SURFACE)
                        && observation.is_some_and(|observation| {
                            pointer_down_if_live(context, observation.viewport)
                        })
                })
    }

    fn observe_and_move(&mut self, planet: usize, ui: &egui::Ui) {
        let context = ui.ctx();
        let viewport = context.viewport_id();
        if viewport == egui::ViewportId::ROOT {
            self.observations[planet] = None;
            self.last_move[planet] = None;
            return;
        }
        let Some(geometry) = WindowGeometry::observed(
            context.input(|input| input.viewport().outer_rect),
            context.pixels_per_point(),
        ) else {
            return;
        };
        let Some(token) = eframe::current_native_output_token() else {
            return;
        };
        let window = u64::from(token.window_id());
        let previous = self.observations[planet]
            .filter(|previous| previous.viewport == viewport && previous.window == window);
        if previous.is_none() {
            self.last_move[planet] = None;
        }
        let movements = previous.map_or(0, |previous| {
            previous.movements
                + usize::from(
                    previous
                        .geometry
                        .physical
                        .min
                        .distance(geometry.physical.min)
                        >= 1.0,
                )
        });
        self.observations[planet] = Some(Observation {
            viewport,
            window,
            geometry,
            movements,
        });

        let moving = self.orbiting[planet] && !self.orbit.paused && !self.any_pointer_down(context);
        if moving
            && self.last_move[planet].is_none_or(|last| last.elapsed() >= ORBIT_INTERVAL)
            && let Some(physical_position) = self
                .orbit
                .physical_position(planet, geometry.physical.size())
            && geometry.physical.min.distance(physical_position) >= 1.0
        {
            // This is an ordinary application window command. Native admission and
            // presentation still depend on fresh observations from the OS provider.
            context.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                physical_position / geometry.pixels_per_point,
            ));
            self.last_move[planet] = Some(Instant::now());
        }
        context.request_repaint_after(if moving {
            ORBIT_INTERVAL
        } else {
            Duration::from_millis(100)
        });
    }
}

fn pointer_down_if_live(context: &egui::Context, viewport: egui::ViewportId) -> bool {
    // input_for creates missing viewport state, so only query the current roster.
    context.input(|input| input.raw.viewports.contains_key(&viewport))
        && context.input_for(viewport, |input| input.pointer.any_down())
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        if item == SUN {
            return Some("Sun".into());
        }
        PLANETS
            .iter()
            .find(|planet| planet.item == item)
            .map(|planet| planet.name.into())
    }

    fn minimum_size(&self, _item: ItemId) -> egui::Vec2 {
        egui::vec2(160.0, 110.0)
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let planet = PLANETS.iter().position(|planet| planet.item == item);
        if let Some(planet) = planet {
            self.observe_and_move(planet, ui);
        }
        let (rect, response) = ui.allocate_exact_size(ui.available_size(), egui::Sense::hover());
        let (name, color) = planet
            .map_or(("Sun", egui::Color32::from_rgb(244, 174, 57)), |index| {
                (PLANETS[index].name, PLANETS[index].color)
            });
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, color.gamma_multiply(0.08));
        let radius = (rect.height().min(rect.width()) * 0.23).clamp(10.0, 50.0);
        let center = rect.center() - egui::vec2(0.0, 12.0);
        painter.circle_stroke(center, radius + 8.0, egui::Stroke::new(1.0, color));
        painter.circle_filled(center, radius, color);
        painter.circle_filled(
            center - egui::vec2(radius * 0.28, radius * 0.28),
            radius * 0.24,
            egui::Color32::WHITE.gamma_multiply(0.25),
        );
        painter.text(
            center + egui::vec2(0.0, radius + 21.0),
            egui::Align2::CENTER_CENTER,
            name,
            egui::FontId::proportional(17.0),
            ui.visuals().text_color(),
        );
        if item == SUN && rect.height() > 190.0 {
            painter.text(
                rect.center_bottom() - egui::vec2(0.0, 18.0),
                egui::Align2::CENTER_BOTTOM,
                "Pause the orbit to arrange windows by hand.",
                egui::FontId::proportional(12.0),
                ui.visuals().weak_text_color(),
            );
        }
        PaneResponse::default().with_content(&response)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observation(viewport: egui::ViewportId, window: u64) -> Observation {
        Observation {
            viewport,
            window,
            geometry: WindowGeometry::observed(
                Some(egui::Rect::from_min_size(
                    egui::pos2(10.0, 20.0),
                    egui::vec2(210.0, 150.0),
                )),
                1.0,
            )
            .unwrap(),
            movements: 4,
        }
    }

    fn input(active: egui::ViewportId, live: &[egui::ViewportId]) -> egui::RawInput {
        let mut input = egui::RawInput {
            viewport_id: active,
            ..Default::default()
        };
        for viewport in live {
            input.viewports.insert(
                *viewport,
                egui::ViewportInfo {
                    parent: (*viewport != egui::ViewportId::ROOT).then_some(egui::ViewportId::ROOT),
                    ..Default::default()
                },
            );
        }
        input
    }

    #[test]
    fn stale_pointer_observation_does_not_create_a_retired_viewport_in_a_child_pass() {
        let context = egui::Context::default();
        let retired = egui::ViewportId::from_hash_of("retired-mercury");
        let active = egui::ViewportId::from_hash_of("live-earth");
        let panes = Panes {
            surfaces: [
                Some(SurfaceId::new(3)),
                Some(SurfaceId::new(3)),
                Some(SurfaceId::new(4)),
            ],
            observations: [Some(observation(retired, 2)), None, None],
            ..Default::default()
        };
        let output = context.run_ui(input(active, &[egui::ViewportId::ROOT, active]), |_| {
            assert!(!panes.any_pointer_down(&context));
        });
        assert!(
            !output.viewport_output.contains_key(&retired),
            "a read of retired pointer state must not resurrect a window"
        );
        output.drop_without_applying_deltas();
    }

    #[test]
    fn a_hidden_pane_moved_by_the_core_drops_its_old_window_before_that_window_retires() {
        use dockspace::model::{
            DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
            DockspaceSurfaceLayout, RootId,
        };
        use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason};

        let original = [SurfaceId::new(2), SurfaceId::new(3), SurfaceId::new(4)];
        let layout = DockspaceLayout::new(
            std::iter::once(DockspaceSurfaceLayout::new(
                ROOT_SURFACE,
                DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([SUN])),
            ))
            .chain(PLANETS.iter().zip(original).map(|(planet, surface)| {
                DockspaceSurfaceLayout::new(
                    surface,
                    DockspaceRootLayout::new(
                        RootId::new(surface.get()),
                        DockspaceNode::tabs([planet.item]),
                    ),
                )
            })),
        )
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, Default::default()).unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .dock_item_current(PLANETS[0].item, DockPlacement::After(PLANETS[1].item))
            .unwrap();
        frame.select_item_current(PLANETS[1].item).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
        let surfaces = PLANETS
            .each_ref()
            .map(|planet| session.view().item(planet.item).map(|item| item.surface()));
        assert_eq!(
            surfaces,
            [Some(original[1]), Some(original[1]), Some(original[2])]
        );
        assert!(!session.view().item(PLANETS[0].item).unwrap().is_selected());
        assert!(session.view().surface(original[0]).is_none());

        let viewports = ["mercury", "venus", "earth"].map(egui::ViewportId::from_hash_of);
        let mut panes = Panes {
            surfaces: original.map(Some),
            observations: std::array::from_fn(|planet| {
                Some(observation(
                    viewports[planet],
                    u64::try_from(planet).unwrap() + 2,
                ))
            }),
            last_move: [Some(Instant::now()); 3],
            orbiting: [true; 3],
            ..Default::default()
        };
        let context = egui::Context::default();
        let output = context.run_ui(input(egui::ViewportId::ROOT, &viewports), |_| {
            // The native roster can still contain the source during asynchronous retirement.
            panes.reconcile_surfaces(surfaces, &context);
            assert!(panes.observations[0].is_none());
            assert!(panes.last_move[0].is_none());
            assert!(panes.observations[1].is_some());
            assert!(panes.observations[2].is_some());
            assert_eq!(panes.orbiting, [false, false, true]);
            assert!(!panes.any_pointer_down(&context));
        });
        assert!(!output.viewport_output.contains_key(&viewports[0]));
        output.drop_without_applying_deltas();
    }
}
