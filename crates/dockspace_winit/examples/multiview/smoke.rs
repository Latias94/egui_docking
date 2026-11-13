//! A finite real-window lifecycle; commands are programmatic, not OS input.

use std::time::{Duration, Instant};

use dockspace::{
    model::NativeWindowPlacement,
    runtime::{DockspaceSession, NativeTearOffSource},
};

use super::{AppResult, host::Host, workspace};

#[derive(Clone, Copy)]
pub(super) enum Command {
    Detach,
    DockBack,
    CloseChild,
    Quit,
    MinimizeMain,
}

pub(super) struct Smoke {
    stage: u8,
    deadline: Instant,
}

impl Smoke {
    pub fn new() -> Self {
        Self {
            stage: 0,
            deadline: Instant::now() + Duration::from_secs(20),
        }
    }

    pub fn deadline(&self) -> Instant {
        self.deadline
    }

    pub fn finished(&self) -> bool {
        self.stage == 8
    }

    pub fn next(
        &mut self,
        session: &DockspaceSession,
        host: &Host,
        root_presented: bool,
    ) -> AppResult<Option<Command>> {
        let item = session
            .view()
            .item(workspace::INSPECTOR)
            .ok_or("Inspector content was lost")?;
        let in_main = item.surface() == workspace::MAIN;
        let child_presented = host.child().is_some()
            && !session.has_pending_native_create()
            && !in_main
            && session
                .presentation_commands(item.root())
                .dock_back()?
                .is_ready();
        let detach_ready = if matches!(self.stage, 0 | 1 | 3 | 5) && host.child().is_none() {
            session
                .native_window_command(
                    NativeTearOffSource::Item(workspace::INSPECTOR),
                    NativeWindowPlacement::new(host.child_placement()?),
                )?
                .is_ready()
        } else {
            false
        };
        let command = match self.stage {
            0 if root_presented && detach_ready => Some(Command::Detach),
            1 if in_main
                && !session.has_pending_native_create()
                && host.idle()
                && detach_ready
                && host.destructions == 1 =>
            {
                Some(Command::Detach)
            }
            2 if child_presented => Some(Command::DockBack),
            3 if in_main && host.idle() && detach_ready && host.destructions == 2 => {
                Some(Command::Detach)
            }
            4 if child_presented => Some(Command::CloseChild),
            5 if in_main && host.idle() && detach_ready && host.destructions == 3 => {
                Some(Command::Detach)
            }
            6 if child_presented => {
                if host.creations != 4 || host.windows.len() != 2 {
                    return Err("native recreation did not retain exactly the main window and a fresh child".into());
                }
                Some(Command::MinimizeMain)
            }
            7 if host.root_minimized() => Some(Command::Quit),
            _ => None,
        };
        if command.is_some() {
            let action = match (self.stage, command) {
                (0, _) => "fail renderer preparation and compensate",
                (_, Some(Command::Detach)) => "create and present a fresh child",
                (_, Some(Command::DockBack)) => "Dock Back after recovery scene is ready",
                (_, Some(Command::CloseChild)) => "close child and recover its content",
                (_, Some(Command::Quit)) => "drain child and root before exit",
                (_, Some(Command::MinimizeMain)) => "minimize the recovery window before shutdown",
                _ => unreachable!(),
            };
            println!("multiview smoke: {action}");
            self.stage += 1;
        }
        Ok(command)
    }
}
