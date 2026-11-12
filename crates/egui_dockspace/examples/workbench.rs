//! Complete docking through the public optional winit/WGPU host.
//!
//! Application layout and pane rendering live in workbench/editor.rs.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::time::Duration;

use egui::{Context, RawInput};
use egui_dockspace::winit::{
    AppResult, FrameControl, NativeApp, NativeFrameReport, NativeOptions, backend, run_native,
};
use egui_dockspace::{Dockspace, DockspaceOutput, DockspaceResponse, SurfacePresentationResult};

#[path = "workbench/editor.rs"]
mod editor;
#[path = "workbench/exercise.rs"]
mod exercise;

use editor::{DemoPanes, apply_editor_commands, demo_layout, run_editor};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let exercise = exercise::Exercise::from_args()?;
    let context = editor::new_context();
    if exercise.is_some() {
        context.enable_accesskit();
    }
    let options = NativeOptions {
        window: backend::window::WindowAttributes::default()
            .with_title("Dockspace Workbench")
            .with_inner_size(backend::dpi::LogicalSize::new(1280.0, 820.0)),
        ..Default::default()
    };
    let mut app = Workbench {
        dockspace: Dockspace::builder("workbench", demo_layout())
            .style(editor::dock_style())
            .build()?,
        panes: DemoPanes::new(),
        response: None,
        restore_layout: false,
        exercise,
    };
    run_native(options, context, &mut app)?;
    if let Some(exercise) = app.exercise {
        exercise.require_complete()?;
    }
    Ok(())
}

struct Workbench {
    dockspace: Dockspace,
    panes: DemoPanes,
    response: Option<DockspaceResponse>,
    restore_layout: bool,
    exercise: Option<exercise::Exercise>,
}

impl NativeApp for Workbench {
    fn prepare_input(&mut self, input: &mut RawInput) -> AppResult<()> {
        if let Some(exercise) = &mut self.exercise {
            exercise.prepare(input, &self.panes)?;
        }
        Ok(())
    }

    fn update(&mut self, context: &Context, input: RawInput) -> AppResult<DockspaceOutput> {
        self.restore_layout = false;
        let (output, response) = run_editor(
            &mut self.dockspace,
            context,
            input,
            &mut self.panes,
            &mut self.restore_layout,
        )?;
        self.response = response;
        Ok(output)
    }

    fn frame_finished(&mut self, frame: NativeFrameReport<'_>) -> AppResult<FrameControl> {
        let changed = apply_editor_commands(
            &mut self.dockspace,
            self.response.as_ref(),
            self.restore_layout,
        )?;
        let Some(exercise) = &mut self.exercise else {
            return Ok(if changed {
                FrameControl::Repaint
            } else {
                FrameControl::Continue
            });
        };
        exercise.observe(frame.accesskit_update);
        if frame.presentation == SurfacePresentationResult::Dropped && exercise.first_drop() {
            eprintln!(
                "egui-host-exercise: initial dropped output: {:?}",
                frame.drop_reason
            );
        }
        if exercise.finish_frame(
            &self.panes,
            &self.dockspace,
            frame.presentation,
            frame.primitives,
            exercise::Timing {
                update_ns: frame.update_duration.as_nanos(),
                tessellate_ns: frame.tessellation_duration.as_nanos(),
                paint_present_ns: frame.paint_duration.as_nanos(),
                ui_to_present_ns: frame.frame_duration.as_nanos(),
            },
        )? {
            return Ok(FrameControl::Exit);
        }
        if frame.presentation == SurfacePresentationResult::Dropped {
            frame
                .context
                .request_repaint_after(Duration::from_millis(16));
            Ok(FrameControl::Continue)
        } else {
            Ok(FrameControl::Repaint)
        }
    }
}
