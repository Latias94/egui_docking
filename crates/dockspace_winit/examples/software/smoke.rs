//! Synthetic winit events through real software presentation, not OS mouse automation.

use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use dockspace::{
    geometry::{LogicalPoint, LogicalRect},
    model::{DockspaceAxis, ItemId, WorkspaceVersion},
    runtime::{
        DockspaceManipulationCancelReason, DockspaceManipulationStatus, DockspacePreviewVisual,
        DockspaceSession, HostFrameReport, SurfacePaintPlan,
        SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
        SurfacePointerPosition,
    },
};
use dockspace_winit::{
    MouseInput, MouseSample, SurfaceCoordinates,
    backend::{
        dpi::PhysicalPosition,
        event::{DeviceId, ElementState, MouseButton, WindowEvent},
        keyboard::ModifiersState,
    },
};

use super::{AppResult, editor};

#[derive(Default)]
pub(super) struct Snapshot {
    tabs: BTreeMap<ItemId, LogicalRect>,
    panes: BTreeMap<ItemId, LogicalRect>,
    splitter: Option<LogicalRect>,
    centers: Vec<(LogicalRect, LogicalRect)>,
    preview: Option<DockspacePreviewVisual>,
    drag: bool,
}

/// Copies public geometry only; inspection never accepts or presents an output.
pub(super) fn inspect(plan: SurfacePaintPlan<'_>) -> AppResult<Snapshot> {
    let mut snapshot = Snapshot::default();
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                root,
                operation,
                paint,
            } if !paint.is_source_gap() => match operation {
                Operation::Tab(tab) => {
                    snapshot.tabs.insert(tab.item(), tab.visible_bounds());
                }
                Operation::Pane(pane) => {
                    if let Some(item) = pane.selected() {
                        snapshot.panes.insert(item, pane.content_bounds());
                    }
                }
                Operation::Splitter(splitter)
                    if root.contained().is_none()
                        && splitter.axis() == DockspaceAxis::Horizontal =>
                {
                    snapshot.splitter.get_or_insert(splitter.draw_bounds());
                }
                _ => {}
            },
            Step::DragPreview(preview) => snapshot.preview = Some(preview.visual()),
            Step::DragDecoration(_) => snapshot.drag = true,
            Step::DropGuide {
                affordance: Some(cluster),
                ..
            } => {
                for target in cluster.targets() {
                    if target.direction() == dockspace::runtime::DockspaceDropDirection::Center {
                        snapshot
                            .centers
                            .push((cluster.activation_bounds(), target.draw_bounds()));
                    }
                }
            }
            _ => {}
        }
        Ok::<(), std::convert::Infallible>(())
    })?;
    Ok(snapshot)
}

impl Snapshot {
    fn tab(&self, item: u64) -> AppResult<LogicalPoint> {
        center(
            *self
                .tabs
                .get(&ItemId::new(item))
                .ok_or("smoke tab is not visible")?,
        )
    }

    fn target(&self, item: u64) -> AppResult<LogicalPoint> {
        let point = center(
            *self
                .panes
                .get(&ItemId::new(item))
                .ok_or("smoke target pane is not visible")?,
        )?;
        self.centers
            .iter()
            .find(|(activation, _)| activation.contains(point))
            .map_or(Ok(point), |(_, bounds)| center(*bounds))
    }

    fn dock_preview(&self) -> AppResult<()> {
        require(
            self.drag && matches!(self.preview, Some(DockspacePreviewVisual::Dock { .. })),
            "expected an active docking preview",
        )
    }
}

pub(super) struct Smoke {
    stage: u8,
    started: Instant,
    splitter: Option<LogicalPoint>,
    before_release: Option<WorkspaceVersion>,
}

impl Smoke {
    pub(super) fn new() -> Self {
        Self {
            stage: 0,
            started: Instant::now(),
            splitter: None,
            before_release: None,
        }
    }

    pub(super) fn deadline(&self) -> Instant {
        self.started + Duration::from_secs(30)
    }

    /// Called only after the inspected output was really presented and settled.
    pub(super) fn step(
        &mut self,
        session: &mut DockspaceSession,
        mouse: &mut MouseInput,
        coordinates: SurfaceCoordinates,
        pixels_per_point: f64,
        snapshot: &Snapshot,
    ) -> AppResult<bool> {
        if Instant::now() >= self.deadline() {
            return Err(format!("software smoke timed out at stage {}", self.stage).into());
        }
        let motion = |point: LogicalPoint| WindowEvent::CursorMoved {
            device_id: DeviceId::dummy(),
            position: PhysicalPosition::new(
                point.x() * pixels_per_point,
                point.y() * pixels_per_point,
            ),
        };
        let button = |state| WindowEvent::MouseInput {
            device_id: DeviceId::dummy(),
            state,
            button: MouseButton::Left,
        };
        let event = match self.stage {
            0 | 7 | 16 | 21 | 27 => {
                match self.stage {
                    7 => require(
                        center(snapshot.splitter.ok_or("main splitter disappeared")?)?.x()
                            - self.splitter.ok_or("splitter baseline is absent")?.x()
                            > 16.0,
                        "splitter drag did not resize the real layout",
                    )?,
                    16 => {
                        require(
                            !session.has_active_gesture() && snapshot.preview.is_none(),
                            "unknown-position release retained its gesture",
                        )?;
                        require(
                            self.before_release == Some(session.version()),
                            "unknown-position release reused an old target",
                        )?;
                        require(
                            session
                                .view()
                                .item(ItemId::new(2))
                                .ok_or("item 2 disappeared")?
                                .tabs()
                                .items()
                                == [ItemId::new(1), ItemId::new(2)],
                            "invalidated release changed the source group",
                        )?;
                    }
                    21 => require(
                        session
                            .view()
                            .item(ItemId::new(2))
                            .ok_or("item 2 disappeared")?
                            .tabs()
                            .items()
                            == [ItemId::new(3), ItemId::new(2)],
                        "dock release did not produce the exact target group",
                    )?,
                    27 => {
                        require(
                            session
                                .view()
                                .item(ItemId::new(2))
                                .ok_or("item 2 disappeared")?
                                .contained()
                                .is_some(),
                            "Shift drag did not create a contained window",
                        )?;
                        dispatch(
                            session,
                            mouse,
                            coordinates,
                            &WindowEvent::ModifiersChanged(ModifiersState::empty().into()),
                        )?;
                    }
                    _ => {}
                }
                motion(snapshot.tab(2)?)
            }
            1 | 4 | 8 | 17 | 22 | 28 => button(ElementState::Pressed),
            2 | 6 | 15 | 20 | 26 => {
                if self.stage == 15 {
                    require(
                        snapshot.preview.is_none() && snapshot.drag,
                        "geometry invalidation did not clear the preview while retaining the drag",
                    )?;
                } else if self.stage == 20 {
                    snapshot.dock_preview()?;
                } else if self.stage == 26 {
                    require(
                        matches!(
                            snapshot.preview,
                            Some(DockspacePreviewVisual::Contained { .. })
                        ),
                        "Shift drag has no contained preview",
                    )?;
                }
                button(ElementState::Released)
            }
            3 => {
                require(
                    session
                        .view()
                        .item(ItemId::new(2))
                        .is_some_and(|item| item.is_selected()),
                    "tab click did not select Outline",
                )?;
                let point = center(snapshot.splitter.ok_or("main splitter is absent")?)?;
                self.splitter = Some(point);
                motion(point)
            }
            5 => {
                let point = self.splitter.ok_or("splitter baseline is absent")?;
                motion(LogicalPoint::new(point.x() + 48.0, point.y())?)
            }
            9 | 10 | 13 | 18 | 19 | 29 | 30 => motion(snapshot.target(3)?),
            11 => {
                snapshot.dock_preview()?;
                WindowEvent::CursorLeft {
                    device_id: DeviceId::dummy(),
                }
            }
            12 => {
                require(
                    snapshot.preview.is_none() && snapshot.drag && session.has_active_gesture(),
                    "CursorLeft did not clear the preview while retaining the drag",
                )?;
                motion(snapshot.target(3)?)
            }
            14 => {
                snapshot.dock_preview()?;
                self.before_release = Some(session.version());
                submit(
                    session,
                    mouse
                        .invalidate_position()
                        .ok_or("no position to invalidate")?,
                    coordinates,
                )?;
                self.stage += 1;
                return Ok(false);
            }
            23 => motion(snapshot.target(1)?),
            24 => {
                snapshot.dock_preview()?;
                // The stationary directive withdraws docking without inventing motion.
                WindowEvent::ModifiersChanged(ModifiersState::SHIFT.into())
            }
            25 => {
                require(
                    snapshot.preview.is_none() && snapshot.drag && session.has_active_gesture(),
                    "stationary Shift did not withdraw the dock preview while retaining the drag",
                )?;
                motion(snapshot.target(1)?)
            }
            31 => {
                snapshot.dock_preview()?;
                let report = dispatch(session, mouse, coordinates, &WindowEvent::Focused(false))?
                    .ok_or("focus loss did not emit a cancellation")?;
                require(
                    report.manipulation_feedback().iter().any(|feedback| {
                        feedback.status()
                            == DockspaceManipulationStatus::Cancelled(
                                DockspaceManipulationCancelReason::PointerStreamCancelled,
                            )
                    }),
                    "focus loss used the wrong core cancellation reason",
                )?;
                self.stage += 1;
                return Ok(false);
            }
            32 => {
                require(
                    !session.has_active_gesture()
                        && !session.has_pending_presentation()
                        && snapshot.preview.is_none(),
                    "focus loss left a gesture or presentation pending",
                )?;
                require(
                    (1..=5).all(|item| session.view().item(ItemId::new(item)).is_some()),
                    "smoke lost application items",
                )?;
                require(
                    session
                        .view()
                        .item(ItemId::new(2))
                        .is_some_and(|item| item.contained().is_some()),
                    "cancelled drag changed the floating source",
                )?;
                return Ok(true);
            }
            _ => return Err("software smoke advanced past its final stage".into()),
        };
        dispatch(session, mouse, coordinates, &event)?;
        self.stage += 1;
        Ok(false)
    }
}

fn dispatch(
    session: &mut DockspaceSession,
    mouse: &mut MouseInput,
    coordinates: SurfaceCoordinates,
    event: &WindowEvent,
) -> AppResult<Option<HostFrameReport>> {
    editor::window_input(session, mouse, Some(coordinates), event)
}

fn submit(
    session: &mut DockspaceSession,
    sample: MouseSample,
    coordinates: SurfaceCoordinates,
) -> AppResult<HostFrameReport> {
    let position = match sample.logical_position(coordinates)? {
        SurfacePointerPosition::Known(point) => Some(point),
        SurfacePointerPosition::Unknown => None,
    };
    editor::input(
        session,
        sample.event,
        position,
        !sample.modifiers.shift_key(),
    )
}

fn center(bounds: LogicalRect) -> AppResult<LogicalPoint> {
    Ok(LogicalPoint::new(
        bounds.x() + bounds.width() * 0.5,
        bounds.y() + bounds.height() * 0.5,
    )?)
}

fn require(condition: bool, message: &str) -> AppResult<()> {
    if condition {
        Ok(())
    } else {
        Err(message.into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::{
        geometry::{PhysicalPoint, ScaleFactor},
        runtime::SurfacePresentationResult,
    };

    #[test]
    fn synthetic_journey_also_preserves_core_semantics_in_a_headless_surface() -> AppResult<()> {
        for (scale, size) in [(1.0, [960, 640]), (2.5, [2400, 1600])] {
            run_headless(scale, size)?;
        }
        Ok(())
    }

    fn run_headless(scale: f64, size: [u32; 2]) -> AppResult<()> {
        let mut session = editor::session()?;
        let mut mouse = MouseInput::default();
        let coordinates =
            SurfaceCoordinates::new(PhysicalPoint::new(0.0, 0.0)?, ScaleFactor::new(scale)?);
        let mut smoke = Smoke::new();
        let mut pixels = vec![0; usize::try_from(size[0])? * usize::try_from(size[1])?];
        for _ in 0..40 {
            let snapshot = present_headless(&mut session, &mut pixels, size, scale)?;
            if smoke.step(&mut session, &mut mouse, coordinates, scale, &snapshot)? {
                return Ok(());
            }
        }
        Err("headless smoke exceeded its finite frame bound".into())
    }

    #[test]
    fn pending_coordinate_mapping_cannot_activate_an_old_presented_tab() -> AppResult<()> {
        let mut session = editor::session()?;
        let mut mouse = MouseInput::default();
        let mut pixels = vec![0; 960 * 640];
        let old = present_headless(&mut session, &mut pixels, [960, 640], 1.0)?;
        let old_tab = old.tab(2)?;
        let version = session.version();
        let click = |position| {
            [
                WindowEvent::CursorMoved {
                    device_id: DeviceId::dummy(),
                    position,
                },
                WindowEvent::MouseInput {
                    device_id: DeviceId::dummy(),
                    state: ElementState::Pressed,
                    button: MouseButton::Left,
                },
                WindowEvent::MouseInput {
                    device_id: DeviceId::dummy(),
                    state: ElementState::Released,
                    button: MouseButton::Left,
                },
            ]
        };
        // With the proposed zoom, these pixels would hit the old Outline record.
        // The new mapping has not presented, so every edge must remain unknown.
        for event in click(PhysicalPosition::new(old_tab.x() * 2.5, old_tab.y() * 2.5)) {
            editor::window_input(&mut session, &mut mouse, None, &event)?
                .ok_or("an unknown-position edge was dropped")?;
            require(
                !session.has_active_gesture(),
                "mapping gap started a gesture",
            )?;
            require(
                session.version() == version,
                "mapping gap changed the old output's selection",
            )?;
            require(
                session
                    .view()
                    .item(ItemId::new(1))
                    .is_some_and(|item| item.is_selected()),
                "mapping gap selected the old Outline tab",
            )?;
        }

        let fresh = present_headless(&mut session, &mut pixels, [960, 640], 2.5)?;
        let coordinates =
            SurfaceCoordinates::new(PhysicalPoint::new(0.0, 0.0)?, ScaleFactor::new(2.5)?);
        if let Some(sample) = mouse.invalidate_position() {
            submit(&mut session, sample, coordinates)?;
        }
        let point = fresh.tab(2)?;
        for event in click(PhysicalPosition::new(point.x() * 2.5, point.y() * 2.5)) {
            editor::window_input(&mut session, &mut mouse, Some(coordinates), &event)?
                .ok_or("a fresh mapped edge was dropped")?;
        }
        require(
            session
                .view()
                .item(ItemId::new(2))
                .is_some_and(|item| item.is_selected()),
            "fresh mapped click did not select Outline",
        )?;
        require(
            !session.has_active_gesture(),
            "fresh mapped click retained a gesture",
        )
    }

    fn present_headless(
        session: &mut DockspaceSession,
        pixels: &mut [u32],
        size: [u32; 2],
        scale: f64,
    ) -> AppResult<Snapshot> {
        let mut frame = session.begin_host_frame()?;
        frame.begin_encoding_pass()?;
        editor::measure(
            &mut frame,
            LogicalRect::new(
                0.0,
                0.0,
                f64::from(size[0]) / scale,
                f64::from(size[1]) / scale,
            )?,
        )?;
        let plan = frame
            .paint_plan(editor::SURFACE)?
            .ok_or("headless plan deferred")?;
        let snapshot = inspect(plan)?;
        let receipt = super::super::paint::render(plan, pixels, size, scale)?;
        frame.confirm_surface_painted(receipt)?;
        let mut report = frame.commit()?;
        // This test supplies a logical presentation sink; only --smoke covers a real window.
        for output in report.take_painted_outputs() {
            session.report_surface_presentation(output, SurfacePresentationResult::Presented)?;
        }
        session.begin_host_frame()?.commit()?;
        Ok(snapshot)
    }
}
