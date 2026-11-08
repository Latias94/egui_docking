//! Finite public-command journey in the real native editor host.

use std::time::{Duration, Instant};

use dockspace::model::{DockAnchor, DockPlacement};

use super::*;

pub(super) fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            MAIN_ROOT,
            DockspaceNode::central_tabs_with_selection(
                [OUTLINE, PREVIEW, CONSOLE, HISTORY, PROFILER],
                Some(OUTLINE),
            )
            .expect("the outline is present"),
        ),
    )
    .with_contained(DockspaceContainedLayout::new(
        INSPECTOR_FLOATING,
        DockspaceRootLayout::new(INSPECTOR_ROOT, DockspaceNode::tabs([INSPECTOR, EDITOR])),
        LogicalRect::new(5000.0, 4000.0, 360.0, 300.0).unwrap(),
    ))
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(2),
        DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([ASSETS])),
        LogicalRect::new(120.0, 100.0, 360.0, 300.0).unwrap(),
    ))])
    .expect("the finite command layout is valid")
}

pub(super) struct CommandSmoke {
    started: Instant,
    step: usize,
    waiting: bool,
    done: bool,
    expected: Option<BTreeMap<ItemId, String>>,
    child: Option<SurfaceId>,
}

impl CommandSmoke {
    pub(super) fn new() -> Self {
        Self {
            started: Instant::now(),
            step: 0,
            waiting: false,
            done: false,
            expected: None,
            child: None,
        }
    }

    pub(super) fn advance(
        &mut self,
        app: &NativeDockspaceApp<ExamplePanes>,
        context: &egui::Context,
    ) {
        if self.done {
            return;
        }
        match self.advance_inner(app) {
            Ok(false) => context.request_repaint_after(Duration::from_millis(16)),
            result => {
                self.done = true;
                match result {
                    Ok(true) => eprintln!(
                        "command-smoke: complete; nine commands, four pane operations, two singleton native-child round trips, eight stable panes and retained document data; presented and quiescent"
                    ),
                    Err(error) => eprintln!("command-smoke: FAIL {error}"),
                    Ok(false) => unreachable!(),
                }
                context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
            }
        }
    }

    fn advance_inner(&mut self, app: &NativeDockspaceApp<ExamplePanes>) -> Result<bool, String> {
        if self.started.elapsed() > Duration::from_secs(60) {
            return Err(format!(
                "timed out at command {} (waiting={})",
                self.step, self.waiting
            ));
        }
        if let Some(kind) = app.error_kind() {
            return Err(app.with_errors(|primary, cleanup| {
                format!("runtime stopped: {kind:?}; {primary:?}; {cleanup:?}")
            }));
        }
        if self.expected.is_none() {
            app.with_panes_mut(|panes| {
                panes
                    .panes
                    .get_mut(&EDITOR)
                    .unwrap()
                    .text
                    .push_str("\nCOMMAND_JOURNEY")
            });
            self.expected = Some(app.with_panes(|panes| {
                panes
                    .panes
                    .iter()
                    .map(|(&item, pane)| (item, pane.text.clone()))
                    .collect()
            }));
        }
        let unchanged = app.with_panes(|panes| {
            let expected = self.expected.as_ref().unwrap();
            panes.panes.len() == 8
                && expected.iter().all(|(item, text)| {
                    panes.panes.get(item).is_some_and(|pane| &pane.text == text)
                })
        });
        if !unchanged {
            return Err("application pane data changed during a presentation command".to_owned());
        }

        if self.waiting {
            if let Some(result) = app.take_action_status() {
                let status = result.map_err(|error| error.to_string())?;
                self.check_status(&status)?;
                if self.step == 0 {
                    let extracted = app.with_view(|view| {
                        view.item(INSPECTOR).is_some_and(|pane| {
                            pane.root() == INSPECTOR_ROOT && pane.tabs().items() == [INSPECTOR]
                        }) && view.item(EDITOR).is_some_and(|pane| {
                            pane.root() != INSPECTOR_ROOT
                                && pane.contained().is_some()
                                && pane.tabs().items() == [EDITOR]
                        })
                    });
                    if !extracted {
                        return Err("contained extraction did not preserve the source group and create a separate singleton".to_owned());
                    }
                    eprintln!(
                        "command-smoke: contained extraction preserved Inspector's original group and created a separate Editor singleton"
                    );
                }
                eprintln!("command-smoke: step={} result={status:?}", self.step);
                self.waiting = false;
                self.step += 1;
            }
            return Ok(false);
        }
        let host = app.root_surface();
        let surfaces = app.with_view(|view| {
            view.surfaces()
                .map(|surface| surface.id())
                .collect::<Vec<_>>()
        });
        if !app.is_quiescent()
            || !surfaces
                .into_iter()
                .all(|surface| app.is_surface_presented(surface))
        {
            return Ok(false);
        }
        if let Some(child) = self.child
            && !app.is_surface_presented(child)
        {
            return Ok(false);
        }
        if self.step == 9 {
            if !app.with_view(|view| {
                view.surfaces().count() == 1
                    && view
                        .item(EDITOR)
                        .is_some_and(|pane| pane.root() == MAIN_ROOT)
            }) {
                return Err("final docking did not restore the single host surface".to_owned());
            }
            return Ok(true);
        }
        let command = match self.step {
            0 | 4 | 6 => NativeCommand::FloatItem {
                item: EDITOR,
                surface: host,
            },
            1 => NativeCommand::RaiseContained { item: INSPECTOR },
            2 => NativeCommand::BringContainedIntoView { item: INSPECTOR },
            3 | 8 => NativeCommand::DockItem {
                item: EDITOR,
                placement: DockPlacement::Center(DockAnchor::Item(OUTLINE)),
            },
            5 | 7 => {
                let root = app
                    .with_view(|view| view.item(EDITOR).map(|pane| pane.root()))
                    .ok_or("Editor disappeared")?;
                NativeCommand::MoveRootToNewWindow {
                    root,
                    placement: NativeWindowPlacement::new(
                        PhysicalRect::new(760.0, 120.0, 440.0, 340.0).unwrap(),
                    ),
                }
            }
            _ => return Err("unexpected command step".to_owned()),
        };
        app.request_command(command)
            .map_err(|error| error.to_string())?;
        self.waiting = true;
        Ok(false)
    }

    fn check_status(&mut self, status: &NativeActionStatus) -> Result<(), String> {
        let NativeActionStatus::Dock(DockspaceActionStatus::Applied(outcome)) = status else {
            return Err(format!("command {} did not apply: {status:?}", self.step));
        };
        let valid = match (self.step, outcome) {
            (
                0 | 4,
                DockspaceActionOutcome::Floated {
                    item: EDITOR,
                    surface: SURFACE,
                    changed: true,
                    ..
                },
            )
            | (1, DockspaceActionOutcome::ContainedRaised { changed: true, .. })
            | (2, DockspaceActionOutcome::ContainedBoundsUpdated { changed: true, .. })
            | (
                3,
                DockspaceActionOutcome::Docked {
                    item: EDITOR,
                    target_root: MAIN_ROOT,
                    changed: true,
                    ..
                },
            ) => true,
            (
                5 | 7,
                DockspaceActionOutcome::NativeRootTearOffRequested {
                    target_surface,
                    items,
                    ..
                },
            ) if items == &[EDITOR] => {
                self.child = Some(*target_surface);
                true
            }
            (
                6,
                DockspaceActionOutcome::Floated {
                    item: EDITOR,
                    surface: SURFACE,
                    changed: true,
                    ..
                },
            ) => {
                self.child = None;
                true
            }
            (
                8,
                DockspaceActionOutcome::Docked {
                    item: EDITOR,
                    target_root: MAIN_ROOT,
                    changed: true,
                    ..
                },
            ) => {
                self.child = None;
                true
            }
            _ => false,
        };
        if valid {
            Ok(())
        } else {
            Err(format!(
                "command {} produced an unexpected outcome: {outcome:?}",
                self.step
            ))
        }
    }
}
