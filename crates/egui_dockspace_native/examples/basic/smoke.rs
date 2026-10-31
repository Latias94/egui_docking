//! Finite real-window workflow with six OS edits, or eight with JSON persistence.
//!
//! Close, reopen, locking, default reset and JSON restore use public application APIs.
//! Diagnostic content rectangles locate input only; presented surfaces and native
//! quiescence remain the product readiness checks.

use std::time::{Duration, Instant};

use dockspace::close::{CloseDecision, CloseItemDecisionState, ClosePlanTarget, CloseRequestId};
use dockspace::model::{DockAnchor, DockPlacement, DockspaceActionRejection};
use dockspace::runtime::{DockspaceCloseOutcome, DockspaceCloseResolution};

use super::*;

#[cfg(feature = "serde")]
const NATIVE_RESTORE_ITEMS: [ItemId; 2] = [INSPECTOR, ASSETS];

#[cfg(feature = "serde")]
#[derive(PartialEq, Eq)]
struct SavedTabs {
    item: ItemId,
    items: Vec<ItemId>,
    selected: Option<ItemId>,
}

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
    #[cfg(feature = "serde")]
    NativeRestoreLayout,
    #[cfg(feature = "serde")]
    NativeRestoreReady,
    #[cfg(feature = "serde")]
    NativeRestoreChild(usize),
    #[cfg(feature = "serde")]
    NativeRestoreObserved,
    #[cfg(feature = "serde")]
    NativeDocumentRestore,
    #[cfg(feature = "serde")]
    NativeRestoredPresented,
    #[cfg(feature = "serde")]
    NativeRestoredInput,
    #[cfg(feature = "serde")]
    NativeRestoredDockBack(usize),
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
    #[cfg(feature = "serde")]
    saved_tabs: Vec<SavedTabs>,
    #[cfg(feature = "serde")]
    old_windows: [u64; 2],
    #[cfg(feature = "serde")]
    children_observed: std::sync::mpsc::Receiver<bool>,
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
            if cfg!(feature = "serde") { 8 } else { 6 },
        );
        #[cfg(feature = "serde")]
        let children_observed = {
            let (sender, receiver) = std::sync::mpsc::sync_channel(1);
            // One finite barrier lets the X11 owner verify both predecessors
            // still exist before the application submits their replacement.
            std::thread::spawn(move || {
                use std::io::Read as _;

                let mut acknowledgement = [0; b"multi-children-observed\n".len()];
                let observed = std::io::stdin().read_exact(&mut acknowledgement).is_ok()
                    && acknowledgement == *b"multi-children-observed\n";
                let _ = sender.send(observed);
            });
            receiver
        };
        let panes = ExamplePanes::new();
        Self {
            stage: Stage::RootInput,
            started: Instant::now(),
            expected: panes
                .panes
                .into_iter()
                .map(|(item, pane)| (item, pane.text))
                .collect(),
            restore_after_close: false,
            announced: false,
            focus_announced: false,
            #[cfg(feature = "serde")]
            saved_tabs: Vec::new(),
            #[cfg(feature = "serde")]
            old_windows: [0; 2],
            #[cfg(feature = "serde")]
            children_observed,
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
                "editor-smoke: complete; eight stable panes, {} real edits, {} child lifetimes, first-live, Dock Back, OS-close recovery, programmatic close/Allow/reopen, lock/rejected move/select/unlock and Default reset{}, fresh presentation and final quiescence verified",
                if cfg!(feature = "serde") { 8 } else { 6 },
                if cfg!(feature = "serde") { 6 } else { 2 },
                if cfg!(feature = "serde") {
                    ", named JSON restore with a drawn missing placeholder and explicit reload/select, two live Child replacements with preserved tabs/selection and an eighth edit in a fresh OS child"
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
        if self.started.elapsed()
            > Duration::from_secs(if cfg!(feature = "serde") { 90 } else { 60 })
        {
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
                    app.request_root_dock_back(location.expect("ready Inspector").root);
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
                    let request = app.dockspace.request_layout_reset(editor_layout(true));
                    self.submit(
                        app,
                        request,
                        "Smoke: prepare independent Inspector and Assets roots",
                        Stage::NativeRestoreLayout,
                    )?;
                }
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoreLayout => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(status, NativeActionStatus::LayoutReset { previous, current } if previous != current)
                    {
                        return Err(format!("independent root preparation failed: {status:?}"));
                    }
                    self.check_text(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=multi-layout registry=8 independent-roots=Inspector,Assets"
                    );
                    self.set_stage(Stage::NativeRestoreReady);
                }
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoreReady if self.settled(app) => {
                let locations = self.native_restore_locations(app)?;
                if !locations.iter().all(|location| location.is_contained()) {
                    return Err("the two source roots are not independently contained".to_owned());
                }
                self.check_text(app)?;
                app.request_inspector_window(locations[0].root);
                self.expect_presentation_request(app)?;
                self.set_stage(Stage::NativeRestoreChild(0));
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoreChild(index) if self.settled(app) => {
                let locations = self.native_restore_locations(app)?;
                if !self.ready(app, locations[index], true) {
                    return Ok(());
                }
                let Some(window_id) = self.native_window_id(app, NATIVE_RESTORE_ITEMS[index])
                else {
                    return Ok(());
                };
                self.check_text(app)?;
                if index == 0 {
                    eprintln!(
                        "editor-smoke: programmatic=multi-inspector-presented item={} window_id={window_id} first-live=true quiescent=true",
                        INSPECTOR.get()
                    );
                    app.request_native_window(
                        locations[1].root,
                        PhysicalRect::new(120.0, 120.0, 440.0, 340.0)
                            .expect("the static Assets child placement is valid"),
                    );
                    self.expect_presentation_request(app)?;
                    self.set_stage(Stage::NativeRestoreChild(1));
                } else {
                    let Some(windows) = self.native_restore_windows(app) else {
                        return Ok(());
                    };
                    if locations[0].surface == locations[1].surface || windows[0] == windows[1] {
                        return Err(
                            "Inspector and Assets did not reach two distinct OS children"
                                .to_owned(),
                        );
                    }
                    self.old_windows = windows;
                    self.saved_tabs = self.capture_tabs(app)?;
                    self.check_native_restore_layout(app)?;
                    eprintln!(
                        "editor-smoke: programmatic=multi-children-ready window_ids={},{} first-live=true quiescent=true registry=8",
                        windows[0], windows[1]
                    );
                    "Two native windows".clone_into(&mut app.preset_name);
                    app.save_named_preset()?;
                    eprintln!(
                        "editor-smoke: programmatic=capture-multi-preset profile=root-two-children slots=2 registry=8 awaiting=old-window-observation"
                    );
                    self.set_stage(Stage::NativeRestoreObserved);
                }
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoreObserved if self.settled(app) => {
                match self.children_observed.try_recv() {
                    Ok(true) => {}
                    Err(std::sync::mpsc::TryRecvError::Empty) => return Ok(()),
                    _ => {
                        return Err(
                            "the X11 owner did not confirm both live predecessors".to_owned()
                        );
                    }
                }
                self.check_native_restore_layout(app)?;
                if self.native_restore_windows(app) != Some(self.old_windows) {
                    return Err(
                        "a predecessor changed before document restore was requested".to_owned(),
                    );
                }
                let request = app.request_named_preset();
                self.submit(
                    app,
                    request,
                    "Smoke: replace two live Child windows from a named JSON preset",
                    Stage::NativeDocumentRestore,
                )?;
                eprintln!(
                    "editor-smoke: programmatic=multi-restore-requested old-window_ids={},{} observed-alive=true",
                    self.old_windows[0], self.old_windows[1]
                );
            }
            #[cfg(feature = "serde")]
            Stage::NativeDocumentRestore => {
                if let Some(status) = self.take_status(app)? {
                    if !matches!(status, NativeActionStatus::DocumentRestored { previous, current } if previous != current)
                    {
                        return Err(format!(
                            "multi-surface JSON restore did not commit: {status:?}"
                        ));
                    }
                    self.check_native_restore_layout(app)?;
                    // Only callbacks from the replacement windows may locate
                    // the next input; predecessor widget bounds are obsolete.
                    app.dockspace.with_panes_mut(|panes| {
                        for pane in panes.panes.values_mut() {
                            pane.input = None;
                            pane.window_id = None;
                        }
                    });
                    eprintln!(
                        "editor-smoke: programmatic=multi-document-restored logically-committed=true registry=8 tabs=preserved selected=preserved fresh-presented=required"
                    );
                    self.set_stage(Stage::NativeRestoredPresented);
                }
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoredPresented if self.settled(app) => {
                self.check_native_restore_layout(app)?;
                let Some(windows) = self.native_restore_windows(app) else {
                    return Ok(());
                };
                if windows[0] == windows[1]
                    || windows
                        .iter()
                        .any(|window| self.old_windows.contains(window))
                {
                    return Err(
                        "the restored children reused predecessor OS window identities".to_owned(),
                    );
                }
                eprintln!(
                    "editor-smoke: programmatic=multi-children-presented window_ids={},{} first-live=true quiescent=true registry=8 tabs=preserved selected=preserved",
                    windows[0], windows[1]
                );
                self.set_stage(Stage::NativeRestoredInput);
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoredInput if self.settled(app) => {
                if self.input(app, INSPECTOR, "NATIVE_RESTORED_EDIT", true)? {
                    let locations = self.check_native_restore_layout(app)?;
                    app.request_root_dock_back(locations[0].root);
                    self.expect_presentation_request(app)?;
                    self.set_stage(Stage::NativeRestoredDockBack(0));
                }
            }
            #[cfg(feature = "serde")]
            Stage::NativeRestoredDockBack(index) if self.settled(app) => {
                let locations = self.native_restore_locations(app)?;
                if !self.ready(app, locations[index], false) {
                    return Ok(());
                }
                self.check_text(app)?;
                eprintln!(
                    "editor-smoke: programmatic={} item={} quiescent=true",
                    if index == 0 {
                        "multi-inspector-docked-back"
                    } else {
                        "multi-assets-docked-back"
                    },
                    NATIVE_RESTORE_ITEMS[index].get()
                );
                if index == 0 {
                    app.request_root_dock_back(locations[1].root);
                    self.expect_presentation_request(app)?;
                    self.set_stage(Stage::NativeRestoredDockBack(1));
                } else {
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
        !app.action.is_pending() && app.all_surfaces_presented() && app.dockspace.is_quiescent()
    }

    #[cfg(feature = "serde")]
    fn native_restore_locations(
        &self,
        app: &NativeExampleApp,
    ) -> Result<[InspectorLocation; 2], String> {
        app.dockspace
            .with_view(|view| {
                let inspector = view.item(INSPECTOR).ok_or("Inspector disappeared")?;
                let assets = view.item(ASSETS).ok_or("Assets disappeared")?;
                if inspector.root() == assets.root() {
                    return Err("Inspector and Assets no longer have independent roots");
                }
                Ok([
                    InspectorLocation::from(inspector),
                    InspectorLocation::from(assets),
                ])
            })
            .ok_or("the multi-surface layout is unavailable")?
            .map_err(str::to_owned)
    }

    #[cfg(feature = "serde")]
    fn capture_tabs(&self, app: &NativeExampleApp) -> Result<Vec<SavedTabs>, String> {
        app.dockspace
            .with_view(|view| {
                self.expected
                    .keys()
                    .map(|item| {
                        let pane = view.item(*item).ok_or("a saved pane disappeared")?;
                        Ok(SavedTabs {
                            item: pane.item(),
                            items: pane.tabs().items().to_vec(),
                            selected: pane.tabs().selected(),
                        })
                    })
                    .collect::<Result<Vec<_>, &str>>()
            })
            .ok_or("the saved tab groups are unavailable")?
            .map_err(str::to_owned)
    }

    #[cfg(feature = "serde")]
    fn check_native_restore_layout(
        &self,
        app: &NativeExampleApp,
    ) -> Result<[InspectorLocation; 2], String> {
        self.check_text(app)?;
        let locations = self.native_restore_locations(app)?;
        let layout_intact = app.dockspace.with_view(|view| {
            // This checks logical membership only. Native outputs supply the
            // OS window identities and each surface must separately be ready.
            view.surfaces().count() == 3
                && self.expected.keys().all(|item| {
                    view.item(*item).is_some_and(|pane| {
                        pane.surface()
                            == match *item {
                                INSPECTOR | PREVIEW => locations[0].surface,
                                ASSETS => locations[1].surface,
                                _ => SURFACE,
                            }
                    })
                })
        }) == Some(true);
        if !layout_intact
            || !locations.iter().all(|location| location.is_native_child())
            || locations[0].surface == locations[1].surface
            || self.capture_tabs(app)? != self.saved_tabs
        {
            return Err("multi-surface restore changed pane IDs, tab groups, selection or surface membership".to_owned());
        }
        Ok(locations)
    }

    #[cfg(feature = "serde")]
    fn native_window_id(&self, app: &NativeExampleApp, item: ItemId) -> Option<u64> {
        app.dockspace.with_panes(|panes| {
            let pane = &panes.panes[&item];
            let (viewport, _, _, _) = pane.input?;
            (viewport != egui::ViewportId::ROOT)
                .then_some(pane.window_id)
                .flatten()
        })
    }

    #[cfg(feature = "serde")]
    fn native_restore_windows(&self, app: &NativeExampleApp) -> Option<[u64; 2]> {
        Some([
            self.native_window_id(app, INSPECTOR)?,
            self.native_window_id(app, ASSETS)?,
        ])
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
        let (text, input, window_id) = app.dockspace.with_panes(|panes| {
            let pane = &panes.panes[&item];
            (pane.text.clone(), pane.input, pane.window_id)
        });
        let Some((viewport, rect, focused, scale)) = input else {
            return Ok(false);
        };
        let Some(window_id) = window_id else {
            return Ok(false);
        };
        if (viewport != egui::ViewportId::ROOT) != native {
            return Ok(false);
        }
        if !self.announced {
            eprintln!(
                "editor-smoke: input={marker} item={} window={} x={:.1} y={:.1} window_id={window_id} left={:.1} top={:.1} right={:.1} bottom={:.1}",
                item.get(),
                if native { "child" } else { "root" },
                rect.center().x * scale,
                rect.center().y * scale,
                rect.left() * scale,
                rect.top() * scale,
                rect.right() * scale,
                rect.bottom() * scale,
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
