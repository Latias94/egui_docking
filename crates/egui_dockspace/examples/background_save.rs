//! Runtime-independent background saving with an exact deferred-close token.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{self, Write};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::{SystemTime, UNIX_EPOCH};

use dockspace::policy::{CloseCapability, DockPolicy};
use eframe::egui;
use egui_dockspace::{
    CloseDecision, CloseItemDecisionState, CloseRequestId, DeferredCloseDecision,
    DeferredCloseToken, Dockspace, DockspaceCloseOutcome, DockspaceClosePlan,
    DockspaceCloseResolution, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const DOCUMENT: ItemId = ItemId::new(1);

fn main() -> eframe::Result {
    eframe::run_native(
        "Dockspace background save",
        eframe::NativeOptions::default(),
        Box::new(|_| Ok(Box::new(SaveApp::new()))),
    )
}

struct SaveApp {
    dockspace: Dockspace,
    document: Document,
    saving: Option<SaveJob>,
    status: String,
}

struct SaveJob {
    request: CloseRequestId,
    token: DeferredCloseToken,
    destination: PathBuf,
    receiver: Receiver<io::Result<()>>,
    // Retain completion if committing its exact close decision needs a retry.
    completed: Option<io::Result<()>>,
}

impl SaveApp {
    fn new() -> Self {
        let mut policy = DockPolicy::default();
        policy.set_close_capability(CloseCapability::DeferredAllowed);
        Self {
            dockspace: Dockspace::builder("background-save", layout())
                .policy(policy)
                .build()
                .expect("the static document layout is valid"),
            document: Document {
                text: "Edit this document, then close its tab to save a copy in the temporary directory.\n".to_owned(),
                saving: false,
            },
            saving: None,
            status: "Close the document tab to save it in the background.".to_owned(),
        }
    }

    fn defer(&mut self, plan: &DockspaceClosePlan) -> Option<(CloseRequestId, DeferredCloseToken)> {
        if self.saving.is_some() {
            return None;
        }
        let item = plan.items().iter().find(|item| {
            item.item() == DOCUMENT && item.state() == CloseItemDecisionState::Pending
        })?;
        match self
            .dockspace
            .resolve_close(plan.request(), item.token(), CloseDecision::Deferred)
        {
            Ok(result) => match result.outcome() {
                DockspaceCloseOutcome::Processed {
                    resolution: DockspaceCloseResolution::Deferred { continuation, .. },
                    ..
                } => Some((plan.request(), *continuation)),
                _ => {
                    self.status = "The close request changed; no save was started.".to_owned();
                    None
                }
            },
            Err(error) => {
                self.status = error.to_string();
                None
            }
        }
    }

    fn start_save(&mut self, plan: &DockspaceClosePlan, context: &egui::Context) {
        let Some((request, token)) = self.defer(plan) else {
            return;
        };
        let path = save_path();
        let destination = path.clone();
        let text = self.document.text.clone();
        let wake = context.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        // The worker owns only a snapshot, destination and wake handle: no Ui,
        // docking frame, pane registry or native runtime lock crosses the task.
        let spawned = std::thread::Builder::new()
            .name("document-save".to_owned())
            .spawn(move || {
                let result = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(path)
                    .and_then(|mut file| {
                        file.write_all(text.as_bytes())?;
                        file.sync_all()
                    });
                let _ = sender.send(result);
                wake.request_repaint();
            });
        self.status = format!("Saving to {}", destination.display());
        self.saving = Some(SaveJob {
            request,
            token,
            destination,
            receiver,
            completed: spawned.err().map(Err),
        });
        // This example freezes editing until the saved snapshot is resolved.
        // An editor that allows edits must also compare document revisions.
        self.document.saving = true;
        // A failed spawn also needs another UI turn to veto its deferred close.
        context.request_repaint();
    }

    fn poll_save(&mut self) {
        let Some(job) = self.saving.as_mut() else {
            return;
        };
        if job.completed.is_none() {
            job.completed = match job.receiver.try_recv() {
                Ok(result) => Some(result),
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    Some(Err(io::Error::other("save worker stopped")))
                }
            };
        }
        let result = job.completed.as_ref().expect("completion was retained");
        let decision = if result.is_ok() {
            DeferredCloseDecision::Allow
        } else {
            DeferredCloseDecision::Veto
        };
        let message = match result {
            Ok(()) => format!("Saved copy to {}.", job.destination.display()),
            Err(error) => format!("Save failed; document kept open: {error}"),
        };
        match self
            .dockspace
            .resolve_deferred_close(job.request, job.token, decision)
        {
            Ok(result) => {
                self.status = match result.outcome() {
                    DockspaceCloseOutcome::Processed {
                        resolution: DockspaceCloseResolution::Approved { .. },
                        application: Some(Ok(_)),
                        ..
                    } => format!("{message} Document closed."),
                    DockspaceCloseOutcome::Processed {
                        resolution: DockspaceCloseResolution::Vetoed { .. },
                        ..
                    } => message,
                    _ => {
                        format!("{message} The close request changed; the document was not closed.")
                    }
                };
                self.saving = None;
                self.document.saving = false;
            }
            Err(error) => self.status = format!("{message} Close decision needs a retry: {error}"),
        }
    }
}

impl eframe::App for SaveApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_save();
        egui::Panel::top("save-status").show(ui, |ui| {
            ui.label(&self.status);
            if self.saving.is_some() {
                ui.label("Saving a snapshot; editing is temporarily paused.");
                if self
                    .saving
                    .as_ref()
                    .is_some_and(|job| job.completed.is_some())
                    && ui.button("Retry close decision").clicked()
                {
                    self.poll_save();
                }
            }
        });
        egui::CentralPanel::default().show(ui, |ui| {
            match self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.document)
            {
                Ok(response) => {
                    for request in response.close_request_events() {
                        if !request.reused() {
                            self.start_save(request.plan(), ui.ctx());
                        }
                    }
                }
                Err(error) => self.status = error.to_string(),
            }
        });
    }
}

struct Document {
    text: String,
    saving: bool,
}

impl PaneView for Document {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == DOCUMENT).then(|| "Document".into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let editor = ui.add_enabled(
            !self.saving,
            egui::TextEdit::multiline(&mut self.text).desired_width(f32::INFINITY),
        );
        PaneResponse::from(&editor)
    }
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([DOCUMENT])),
    )])
    .expect("the static document layout is valid")
}

fn save_path() -> PathBuf {
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    std::env::temp_dir().join(format!(
        "dockspace-document-{}-{stamp}.txt",
        std::process::id()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_dockspace::DockspaceCloseRequestStatus;
    use std::sync::mpsc::SyncSender;

    fn pending_save() -> (SaveApp, SyncSender<io::Result<()>>) {
        let mut app = SaveApp::new();
        let result = app.dockspace.request_close_item_current(DOCUMENT).unwrap();
        let DockspaceCloseRequestStatus::Requested(request) = result.status() else {
            panic!("the document close request must be admitted");
        };
        let (request, token) = app.defer(request.plan()).unwrap();
        let (sender, receiver) = mpsc::sync_channel(1);
        app.saving = Some(SaveJob {
            request,
            token,
            destination: PathBuf::from("application-owned-copy.txt"),
            receiver,
            completed: None,
        });
        app.document.saving = true;
        (app, sender)
    }

    #[test]
    fn worker_writes_the_snapshot_before_the_ui_allows_closure() {
        let mut app = SaveApp::new();
        let snapshot = app.document.text.clone();
        let result = app.dockspace.request_close_item_current(DOCUMENT).unwrap();
        let DockspaceCloseRequestStatus::Requested(request) = result.status() else {
            panic!("the document close request must be admitted");
        };
        app.start_save(request.plan(), &egui::Context::default());
        let job = app
            .saving
            .as_mut()
            .expect("the worker owns a deferred close");
        let destination = job.destination.clone();
        job.completed = Some(
            job.receiver
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("the save worker must report completion"),
        );
        assert!(app.dockspace.view().item(DOCUMENT).is_some());
        app.poll_save();
        let stored = std::fs::read_to_string(&destination);
        let removed = std::fs::remove_file(destination);
        assert_eq!(stored.unwrap(), snapshot);
        removed.unwrap();
        assert!(app.dockspace.view().item(DOCUMENT).is_none());
        assert!(app.saving.is_none());
    }

    #[test]
    fn document_stays_open_until_save_completes() {
        let (mut app, sender) = pending_save();
        app.poll_save();
        assert!(app.dockspace.view().item(DOCUMENT).is_some());
        assert!(app.document.saving);
        sender.send(Ok(())).unwrap();
        app.poll_save();
        assert!(app.dockspace.view().item(DOCUMENT).is_none());
        assert!(app.saving.is_none());
        assert!(!app.document.saving);
    }

    #[test]
    fn failed_or_disconnected_save_keeps_the_document_open() {
        for disconnected in [false, true] {
            let (mut app, sender) = pending_save();
            if !disconnected {
                sender
                    .send(Err(io::Error::other("storage unavailable")))
                    .unwrap();
            }
            drop(sender);
            app.poll_save();
            assert!(app.dockspace.view().item(DOCUMENT).is_some());
            assert!(app.saving.is_none());
            assert!(!app.document.saving);
            assert!(app.status.contains("Save failed"));
        }
    }

    #[test]
    fn old_save_completion_cannot_close_a_replaced_layout() {
        let (mut app, sender) = pending_save();
        app.dockspace.reset_layout(layout()).unwrap();
        sender.send(Ok(())).unwrap();
        app.poll_save();
        assert!(app.dockspace.view().item(DOCUMENT).is_some());
        assert!(app.saving.is_none());
        assert!(app.status.contains("close request changed"));
    }
}
