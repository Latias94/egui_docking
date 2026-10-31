//! Finite real-window workflow with six OS edits, or seven with JSON persistence.
//!
//! Close, reopen, locking, default reset and JSON restore use public application APIs.
//! Diagnostic content rectangles locate input only; presented surfaces and native
//! quiescence remain the product readiness checks.

use std::time::{Duration, Instant};

use dockspace::close::{CloseDecision, CloseItemDecisionState, ClosePlanTarget, CloseRequestId};
use dockspace::model::{DockAnchor, DockPlacement, DockspaceActionRejection};
use dockspace::runtime::{DockspaceCloseOutcome, DockspaceCloseResolution};

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
    OriginalQuiescence,
    DefaultReset,
    DefaultPresented,
    SelectResetEditor,
    ResetInput,
    #[cfg(feature = "serde")]
    DocumentRestore,
    #[cfg(feature = "serde")]
    RestoredPlaceholder,
    #[cfg(feature = "serde")]
    SelectRestoredInspector,
    #[cfg(feature = "serde")]
    RestoredInput,
    FinalQuiescence,
    Complete,
}

pub(super) struct EditorSmoke {
    stage: Stage,
    started: Instant,
    expected: BTreeMap<ItemId, String>,
    restore_after_close: bool,
    announced: bool,
    focus_announced: bool,
}

impl EditorSmoke {
    pub(super) fn new() -> Self {
        eprintln!(
            "editor-smoke: profile={} edits={}",
            if cfg!(feature = "serde") {
                "serde"
            } else {
                "basic"
            },
            if cfg!(feature = "serde") { 7 } else { 6 },
        );
        let panes = ExamplePanes::new();
        Self {
            stage: Stage::RootInput,
            started: Instant::now(),
            expected: [INSPECTOR, EDITOR]
                .into_iter()
                .map(|item| (item, panes.panes[&item].text.clone()))
                .collect(),
            restore_after_close: false,
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
                "editor-smoke: complete; eight stable panes, {} real edits, two child lifetimes, first-live, Dock Back, OS-close recovery, programmatic close/Allow/reopen, lock/rejected move/select/unlock and Default reset{}, fresh presentation and final quiescence verified",
                if cfg!(feature = "serde") { 7 } else { 6 },
                if cfg!(feature = "serde") {
                    ", named JSON restore with a drawn missing placeholder and explicit reload/select"
                } else {
                    ""
                },
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
        #[cfg(feature = "serde")]
        let may_be_closed = may_be_closed || matches!(self.stage, Stage::DocumentRestore);
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
                if self.input(app, INSPECTOR, "ROOT_EDIT", false)? {
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
                if self.input(app, INSPECTOR, "CHILD_EDIT", true)? {
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
                if self.input(app, INSPECTOR, "BACK_EDIT", false)? {
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
                if self.input(app, INSPECTOR, "RECOVERED_EDIT", false)? {
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
                        "editor-smoke: programmatic={} item={} awaiting=Allow",
                        if self.restore_after_close {
                            "restore-close-requested"
                        } else {
                            "close-requested"
                        },
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
                        "editor-smoke: programmatic={} item={}",
                        if self.restore_after_close {
                            "restore-allow-close"
                        } else {
                            "allow-close"
                        },
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
                        "editor-smoke: programmatic={} item={} registry=8 text=preserved",
                        if self.restore_after_close {
                            "restore-closed"
                        } else {
                            "closed"
                        },
                        INSPECTOR.get()
                    );
                    #[cfg(feature = "serde")]
                    if self.restore_after_close {
                        app.dockspace.with_panes_mut(|panes| {
                            panes
                                .panes
                                .get_mut(&INSPECTOR)
                                .expect("Inspector")
                                .set_loaded(false);
                        });
                        self.check_text(app)?;
                        eprintln!(
                            "editor-smoke: programmatic=unload item={} key=pane/inspector data=retained",
                            INSPECTOR.get()
                        );
                        let request = app.request_named_preset();
                        self.submit(
                            app,
                            request,
                            "Smoke: restore named all-root JSON preset",
                            Stage::DocumentRestore,
                        )?;
                        return Ok(());
                    }
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
                if self.input(app, INSPECTOR, "REOPENED_EDIT", false)? {
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
                    self.set_stage(Stage::OriginalQuiescence);
                }
            }
            Stage::OriginalQuiescence
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
                let request = app
                    .dockspace
                    .request_layout_reset(app.default_layout.clone());
                self.submit(app, request, "Smoke: Default layout", Stage::DefaultReset)?;
            }
            Stage::DefaultReset => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(status, NativeActionStatus::LayoutReset { previous, current } if previous != current)
                    {
                        return Err(format!("Default did not replace the layout: {status:?}"));
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=default-reset logically-committed=true fresh-presented=required"
                    );
                    self.set_stage(Stage::DefaultPresented);
                }
            }
            Stage::DefaultPresented if self.settled(app) => {
                self.check_text(app)?;
                let default_panes = app.dockspace.with_view(|view| {
                    view.item(EDITOR).is_some_and(|pane| {
                        pane.is_selected() && pane.tabs().items() == [EDITOR, HISTORY, PROFILER]
                    }) && view.item(INSPECTOR).is_some_and(|pane| {
                        pane.is_selected()
                            && pane.contained().is_some()
                            && pane.tabs().items() == [PREVIEW, INSPECTOR]
                    })
                }) == Some(true);
                if !app.all_root_profile() || !default_panes {
                    return Err("Default did not restore all eight IDs and the default tab groups to the root host".to_owned());
                }
                eprintln!(
                    "editor-smoke: programmatic=default-presented registry=8 text=preserved fresh-presented=true quiescent=true"
                );
                let request = app.dockspace.request_select_item(EDITOR);
                self.submit(
                    app,
                    request,
                    "Smoke: focus Document after Default",
                    Stage::SelectResetEditor,
                )?;
            }
            Stage::SelectResetEditor => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                            DockspaceActionOutcome::Selected { item: EDITOR, .. }
                        ))
                    ) {
                        return Err(format!("Document select after Default failed: {status:?}"));
                    }
                    eprintln!(
                        "editor-smoke: programmatic=default-select item={}",
                        EDITOR.get()
                    );
                    self.set_stage(Stage::ResetInput);
                }
            }
            Stage::ResetInput if self.settled(app) => {
                if self.input(app, EDITOR, "RESET_EDIT", false)? {
                    self.check_text(app)?;
                    #[cfg(feature = "serde")]
                    {
                        "Default all-root".clone_into(&mut app.preset_name);
                        app.save_named_preset()?;
                        eprintln!(
                            "editor-smoke: programmatic=capture-preset profile=default-all-root slots=1 registry=8"
                        );
                        self.restore_after_close = true;
                        let request = app.dockspace.request_close_item(INSPECTOR);
                        self.submit(
                            app,
                            request,
                            "Smoke: close Inspector before JSON restore",
                            Stage::CloseInspector,
                        )?;
                    }
                    #[cfg(not(feature = "serde"))]
                    self.set_stage(Stage::FinalQuiescence);
                }
            }
            #[cfg(feature = "serde")]
            Stage::DocumentRestore => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(status, NativeActionStatus::DocumentRestored { previous, current } if previous != current)
                    {
                        return Err(format!("JSON document restore did not commit: {status:?}"));
                    }
                    // Require a placeholder callback after this terminal result, too.
                    app.dockspace.with_panes_mut(|panes| {
                        panes
                            .panes
                            .get_mut(&INSPECTOR)
                            .expect("Inspector")
                            .missing_painted = false;
                    });
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=document-restored logically-committed=true fresh-presented=required"
                    );
                    self.set_stage(Stage::RestoredPlaceholder);
                }
            }
            #[cfg(feature = "serde")]
            Stage::RestoredPlaceholder if self.settled(app) => {
                self.check_text(app)?;
                if !app.all_root_profile() || !location.is_some_and(InspectorLocation::is_contained)
                {
                    return Err(
                        "JSON restore did not restore the all-root Inspector profile".to_owned(),
                    );
                }
                let missing_drawn = app.dockspace.with_panes(|panes| {
                    let pane = &panes.panes[&INSPECTOR];
                    !pane.loaded
                        && pane.key == "pane/inspector"
                        && pane.title == "Inspector"
                        && pane.missing_painted
                        && pane.input.is_none()
                        && panes.focus_target(INSPECTOR).is_none()
                });
                if !missing_drawn {
                    return Ok(());
                }
                eprintln!(
                    "editor-smoke: programmatic=missing-placeholder item={} actual-draw=true fresh-presented=true focus-target=none",
                    INSPECTOR.get()
                );
                app.dockspace.with_panes_mut(|panes| {
                    panes
                        .panes
                        .get_mut(&INSPECTOR)
                        .expect("Inspector")
                        .set_loaded(true);
                });
                eprintln!(
                    "editor-smoke: programmatic=reload item={} data=retained",
                    INSPECTOR.get()
                );
                let request = app.dockspace.request_select_item(INSPECTOR);
                self.submit(
                    app,
                    request,
                    "Smoke: new Inspector focus after reload",
                    Stage::SelectRestoredInspector,
                )?;
            }
            #[cfg(feature = "serde")]
            Stage::SelectRestoredInspector => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(
                        status,
                        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                            DockspaceActionOutcome::Selected {
                                item: INSPECTOR,
                                ..
                            }
                        ))
                    ) {
                        return Err(format!("Inspector select after reload failed: {status:?}"));
                    }
                    eprintln!(
                        "editor-smoke: programmatic=restore-select item={}",
                        INSPECTOR.get()
                    );
                    self.set_stage(Stage::RestoredInput);
                }
            }
            #[cfg(feature = "serde")]
            Stage::RestoredInput
                if location.is_some_and(|location| self.ready(app, location, false)) =>
            {
                if self.input(app, INSPECTOR, "RESTORED_EDIT", false)? {
                    self.set_stage(Stage::FinalQuiescence);
                }
            }
            Stage::FinalQuiescence if self.settled(app) => {
                self.check_text(app)?;
                if !app.all_root_profile()
                    || app.dockspace.arrangement_locked()
                    || !app.dockspace.pending_content_close_plans().is_empty()
                    || !app
                        .dockspace
                        .with_panes(|panes| panes.panes.values().all(|pane| pane.loaded))
                {
                    return Err(
                        "the final all-root profile has missing panes, a lock or pending decisions"
                            .to_owned(),
                    );
                }
                eprintln!(
                    "editor-smoke: programmatic=final-quiescence registry=8 text=preserved fresh-presented=true"
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

    fn submit<E: std::fmt::Display>(
        &mut self,
        app: &mut NativeExampleApp,
        request: Result<(), E>,
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
        let status =
            status.map_err(|error| format!("programmatic smoke action interrupted: {error}"))?;
        app.record_action_status(&status);
        Ok(Some(status))
    }

    fn check_text(&self, app: &NativeExampleApp) -> Result<(), String> {
        app.dockspace.with_panes(|panes| {
            self.expected
                .iter()
                .all(|(item, expected)| panes.panes[item].text == *expected)
                .then_some(())
                .ok_or_else(|| {
                    "a layout or presentation action changed application-owned text".to_owned()
                })
        })
    }

    fn input(
        &mut self,
        app: &NativeExampleApp,
        item: ItemId,
        marker: &str,
        native: bool,
    ) -> Result<bool, String> {
        let (text, input) = app.dockspace.with_panes(|panes| {
            let pane = &panes.panes[&item];
            (pane.text.clone(), pane.input)
        });
        let Some((viewport, rect, focused, scale)) = input else {
            return Ok(false);
        };
        if (viewport != egui::ViewportId::ROOT) != native {
            return Ok(false);
        }
        if !self.announced {
            eprintln!(
                "editor-smoke: input={marker} item={} window={} x={:.1} y={:.1}",
                item.get(),
                if native { "child" } else { "root" },
                rect.center().x * scale,
                rect.center().y * scale,
            );
            self.announced = true;
        }
        if focused && !self.focus_announced {
            eprintln!("editor-smoke: focused={marker} item={}", item.get());
            self.focus_announced = true;
        }
        if text == format!("{}{marker}", self.expected[&item]) && focused {
            self.expected.insert(item, text);
            eprintln!(
                "editor-smoke: accepted real focused edit {marker} item={}",
                item.get()
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
