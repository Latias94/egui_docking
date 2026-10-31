//! Finite real-window workflow with five edits supplied by an external OS-input owner.
//!
//! Close decisions, reopening and arrangement locking use public application APIs.
//! Diagnostic content rectangles locate input only; presented surfaces and native
//! quiescence remain the product readiness checks.

use std::time::{Duration, Instant};

use dockspace::close::{CloseDecision, CloseItemDecisionState, ClosePlanTarget, CloseRequestId};
use dockspace::model::{DockAnchor, DockPlacement, DockspaceActionRejection};
use dockspace::runtime::{DockspaceCloseOutcome, DockspaceCloseResolution};
use egui_dockspace_native::NativeActionRequestError;

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
    CloseInspector,
    AllowInspector(CloseRequestId),
    ClosedInspector,
    ReopenInspector,
    ReopenedInput,
    LockArrangement,
    LockedArrangement,
    RejectLockedMove(InspectorLocation),
    SelectWhileLocked,
    UnlockArrangement,
    FinalQuiescence,
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
                "editor-smoke: complete; eight stable panes, five real edits, two child lifetimes, first-live, Dock Back, OS-close recovery, programmatic close/Allow/reopen and lock/rejected move/select/unlock, final quiescence verified"
            );
            context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
        } else {
            // Exceed the ingress age limit before the first real OS edit. Later
            // root polls also observe state changed by deferred child UIs.
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
        let inventory_intact = app.dockspace.with_panes(|panes| {
            panes.panes.len() == 8
                && [
                    OUTLINE, EDITOR, PREVIEW, INSPECTOR, CONSOLE, HISTORY, PROFILER, ASSETS,
                ]
                .iter()
                .all(|item| panes.panes.contains_key(item))
        });
        if !inventory_intact {
            return Err("the application registry lost one of its eight stable panes".to_owned());
        }
        let may_be_closed = matches!(
            self.stage,
            Stage::AllowInspector(_) | Stage::ClosedInspector | Stage::ReopenInspector
        );
        let location = app
            .dockspace
            .with_view(|view| {
                for item in [
                    OUTLINE, EDITOR, PREVIEW, INSPECTOR, CONSOLE, HISTORY, PROFILER, ASSETS,
                ] {
                    if view.item(item).is_none() && !(item == INSPECTOR && may_be_closed) {
                        return Err(format!("open pane {} unexpectedly disappeared", item.get()));
                    }
                }
                Ok(view.item(INSPECTOR).map(InspectorLocation::from))
            })
            .ok_or("the current product view is unavailable")??;
        match self.stage {
            Stage::RootInput
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                if self.input(app, "ROOT_EDIT", false)? {
                    app.request_inspector_window(location.expect("ready Inspector").root);
                    self.expect_presentation_request(app)?;
                    self.set_stage(Stage::FirstChild);
                }
            }
            Stage::FirstChild
                if location.is_some_and(|location| self.ready(app, location, true)) =>
            {
                self.check_text(app)?;
                self.set_stage(Stage::ChildInput);
            }
            Stage::ChildInput
                if location.is_some_and(|location| self.ready(app, location, true)) =>
            {
                if self.input(app, "CHILD_EDIT", true)? {
                    app.request_inspector_dock_back(location.expect("ready Inspector").root);
                    self.expect_presentation_request(app)?;
                    self.set_stage(Stage::DockBack);
                }
            }
            Stage::DockBack
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                self.check_text(app)?;
                self.set_stage(Stage::BackInput);
            }
            Stage::BackInput
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                if self.input(app, "BACK_EDIT", false)? {
                    app.request_inspector_window(location.expect("ready Inspector").root);
                    self.expect_presentation_request(app)?;
                    self.set_stage(Stage::SecondChild);
                }
            }
            Stage::SecondChild
                if location.is_some_and(|location| self.ready(app, location, true)) =>
            {
                self.check_text(app)?;
                eprintln!(
                    "editor-smoke: close-child; send a real OS close request to Dockspace panel"
                );
                self.set_stage(Stage::CloseChild(
                    location.expect("ready Inspector").surface,
                ));
            }
            Stage::CloseChild(old_surface)
                if location.is_some_and(|location| self.ready(app, location, false))
                    && !app.dockspace.is_surface_presented(old_surface) =>
            {
                self.check_text(app)?;
                self.set_stage(Stage::RecoveredInput);
            }
            Stage::RecoveredInput
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                if self.input(app, "RECOVERED_EDIT", false)? {
                    let request = app.dockspace.request_close_item(INSPECTOR);
                    self.submit(
                        app,
                        request,
                        "Smoke: request Inspector close",
                        Stage::CloseInspector,
                    )?;
                }
            }
            Stage::CloseInspector => {
                if let Some(status) = self.take_status(app)? {
                    let NativeActionStatus::CloseRequested {
                        plan,
                        reused: false,
                    } = status
                    else {
                        return Err(format!(
                            "expected a fresh Inspector close plan, got {status:?}"
                        ));
                    };
                    let plans = app.dockspace.pending_content_close_plans();
                    if plans.len() != 1
                        || plans.first() != Some(&plan)
                        || plan.target() != (ClosePlanTarget::Item { item: INSPECTOR })
                        || plan.items().len() != 1
                        || plan.items()[0].item() != INSPECTOR
                        || plan.items()[0].state() != CloseItemDecisionState::Pending
                    {
                        return Err(
                            "Inspector close did not project one exact pending core plan"
                                .to_owned(),
                        );
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=close-requested item={} awaiting=Allow",
                        INSPECTOR.get()
                    );
                    let request = app.dockspace.request_close_decision(
                        plan.request(),
                        plan.items()[0].token(),
                        CloseDecision::Allow,
                    );
                    self.submit(
                        app,
                        request,
                        "Smoke: Allow Inspector close",
                        Stage::AllowInspector(plan.request()),
                    )?;
                }
            }
            Stage::AllowInspector(expected_request) => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(status,
                        NativeActionStatus::CloseDecisionProcessed {
                            resolution: DockspaceCloseResolution::Approved { request },
                            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: INSPECTOR, .. })),
                            changed: true,
                            ..
                        } if request == expected_request
                    ) {
                        return Err(format!(
                            "explicit Allow did not close Inspector: {status:?}"
                        ));
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=allow-close item={}",
                        INSPECTOR.get()
                    );
                    self.set_stage(Stage::ClosedInspector);
                }
            }
            Stage::ClosedInspector => {
                self.check_text(app)?;
                if location.is_some() {
                    return Err("Inspector remained open after its approved close".to_owned());
                }
                if self.settled(app) {
                    if !app.dockspace.pending_content_close_plans().is_empty() {
                        return Err(
                            "the applied Inspector close retained a pending decision".to_owned()
                        );
                    }
                    eprintln!(
                        "editor-smoke: programmatic=closed item={} registry=8 text=preserved",
                        INSPECTOR.get()
                    );
                    let request = app.dockspace.request_open_item(
                        INSPECTOR,
                        DockPlacement::Center(DockAnchor::Item(EDITOR)),
                    );
                    self.submit(
                        app,
                        request,
                        "Smoke: reopen Inspector beside Document",
                        Stage::ReopenInspector,
                    )?;
                }
            }
            Stage::ReopenInspector => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                            DockspaceActionOutcome::Opened {
                                item: INSPECTOR,
                                ..
                            }
                        ))
                    ) {
                        return Err(format!(
                            "Inspector did not reopen as the same item: {status:?}"
                        ));
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=reopen item={} anchor=EDITOR",
                        INSPECTOR.get()
                    );
                    self.set_stage(Stage::ReopenedInput);
                }
            }
            Stage::ReopenedInput
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                let centered = app.dockspace.with_view(|view| {
                    view.item(INSPECTOR).is_some_and(|pane| {
                        pane.item() == INSPECTOR
                            && pane.is_selected()
                            && pane.tabs().items().contains(&EDITOR)
                    })
                }) == Some(true);
                if !centered {
                    return Err(
                        "the reopened Inspector is not selected in the current Document center"
                            .to_owned(),
                    );
                }
                let focused = app.dockspace.with_panes(|panes| {
                    panes.panes[&INSPECTOR]
                        .input
                        .is_some_and(|(viewport, _, focused, _)| {
                            viewport == egui::ViewportId::ROOT && focused
                        })
                });
                if !focused {
                    if self.focus_announced {
                        return Err(
                            "the reopened TextEdit lost its restored focus before the fifth edit"
                                .to_owned(),
                        );
                    }
                    return Ok(());
                }
                if !self.announced {
                    eprintln!(
                        "editor-smoke: programmatic=reopen-focus item={} actual-widget=true",
                        INSPECTOR.get()
                    );
                }
                if self.input(app, "REOPENED_EDIT", false)? {
                    let request = app.dockspace.request_set_arrangement_locked(true);
                    self.submit(
                        app,
                        request,
                        "Smoke: lock arrangement",
                        Stage::LockArrangement,
                    )?;
                }
            }
            Stage::LockArrangement => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::ArrangementLockChanged {
                            locked: true,
                            changed: true,
                            ..
                        }
                    ) || !app.dockspace.arrangement_locked()
                    {
                        return Err(format!("arrangement lock did not commit: {status:?}"));
                    }
                    self.check_text(app)?;
                    eprintln!("editor-smoke: programmatic=lock locked=true");
                    self.set_stage(Stage::LockedArrangement);
                }
            }
            Stage::LockedArrangement
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                let location = location.expect("ready Inspector");
                let placement = NativeWindowPlacement::new(
                    PhysicalRect::new(760.0, 120.0, 440.0, 340.0)
                        .expect("the static child placement is valid"),
                );
                let request = app
                    .dockspace
                    .request_move_root_to_new_window(location.root, placement);
                self.submit(
                    app,
                    request,
                    "Smoke: attempt native move while locked",
                    Stage::RejectLockedMove(location),
                )?;
            }
            Stage::RejectLockedMove(original) => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
                            DockspaceActionRejection::PolicyDenied
                        ))
                    ) || !app.dockspace.arrangement_locked()
                        || location != Some(original)
                    {
                        return Err(format!(
                            "locked native move was not rejected without moving Inspector: {status:?}"
                        ));
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=locked-move-rejected item={}",
                        INSPECTOR.get()
                    );
                    let request = app.dockspace.request_select_item(EDITOR);
                    self.submit(
                        app,
                        request,
                        "Smoke: select Document while locked",
                        Stage::SelectWhileLocked,
                    )?;
                }
            }
            Stage::SelectWhileLocked => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                            DockspaceActionOutcome::Selected {
                                item: EDITOR,
                                changed: true
                            }
                        ))
                    ) || !app.dockspace.arrangement_locked()
                        || app.dockspace.with_view(|view| {
                            view.item(EDITOR).is_some_and(|pane| pane.is_selected())
                        }) != Some(true)
                    {
                        return Err(format!(
                            "content selection did not work while locked: {status:?}"
                        ));
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=select-while-locked item={}",
                        EDITOR.get()
                    );
                    let request = app.dockspace.request_set_arrangement_locked(false);
                    self.submit(
                        app,
                        request,
                        "Smoke: unlock arrangement",
                        Stage::UnlockArrangement,
                    )?;
                }
            }
            Stage::UnlockArrangement => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::ArrangementLockChanged {
                            locked: false,
                            changed: true,
                            ..
                        }
                    ) || app.dockspace.arrangement_locked()
                    {
                        return Err(format!("arrangement unlock did not commit: {status:?}"));
                    }
                    eprintln!("editor-smoke: programmatic=unlock locked=false");
                    self.set_stage(Stage::FinalQuiescence);
                }
            }
            Stage::FinalQuiescence
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                self.check_text(app)?;
                if app.dockspace.arrangement_locked()
                    || !app.dockspace.pending_content_close_plans().is_empty()
                {
                    return Err(
                        "the final layout still has a lock or pending close decision".to_owned(),
                    );
                }
                eprintln!(
                    "editor-smoke: programmatic=quiescent item={} registry=8",
                    INSPECTOR.get()
                );
                self.set_stage(Stage::Complete);
            }
            _ => {}
        }
        Ok(())
    }

    fn settled(&self, app: &NativeExampleApp) -> bool {
        !app.action.is_pending()
            && app.dockspace.is_surface_presented(SURFACE)
            && app.dockspace.is_quiescent()
    }

    fn ready(&self, app: &NativeExampleApp, location: InspectorLocation, native: bool) -> bool {
        location.is_native_child() == native
            && !app.action.is_pending()
            && app.dockspace.is_surface_presented(location.surface)
            && app.dockspace.is_quiescent()
    }

    fn expect_presentation_request(&self, app: &NativeExampleApp) -> Result<(), String> {
        app.action
            .is_pending()
            .then_some(())
            .ok_or_else(|| format!("presentation request was not accepted: {:?}", app.status))
    }

    fn submit(
        &mut self,
        app: &mut NativeExampleApp,
        request: Result<(), NativeActionRequestError>,
        operation: &'static str,
        stage: Stage,
    ) -> Result<(), String> {
        request.map_err(|error| format!("{operation}: {error}"))?;
        // The smoke owns this result through the same example action state and
        // native single slot; the UI must not consume it or enqueue another action.
        app.action = ExampleActionState::AwaitingSmokeOutcome(operation);
        app.status = Some(format!("{operation}: programmatic request accepted."));
        self.set_stage(stage);
        Ok(())
    }

    fn take_status(
        &self,
        app: &mut NativeExampleApp,
    ) -> Result<Option<NativeActionStatus>, String> {
        let Some(status) = app.dockspace.take_action_status() else {
            return Ok(None);
        };
        app.action = ExampleActionState::Idle;
        let status =
            status.map_err(|error| format!("programmatic smoke action interrupted: {error}"))?;
        app.status = Some(super::layout::describe_status(&status));
        Ok(Some(status))
    }

    fn check_text(&self, app: &NativeExampleApp) -> Result<(), String> {
        app.dockspace.with_panes(|panes| {
            (panes.panes[&INSPECTOR].text == self.expected)
                .then_some(())
                .ok_or_else(|| {
                    "a layout or presentation action changed application-owned text".to_owned()
                })
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
