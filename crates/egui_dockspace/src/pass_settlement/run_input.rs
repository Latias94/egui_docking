//! Frozen input and synchronous callback scope for one official egui run.

use std::cell::RefCell;
use std::sync::{Arc, PoisonError};

use egui::{Context, RawInput, Ui, ViewportId};

use crate::pointer::ProductPointerInputSnapshot;

use super::{RunIdentity, active_run_portal};

pub(crate) const RUN_SCOPE_TAG: &str = "egui_dockspace::official_run_scope";

pub(super) struct RunInput {
    events: Vec<egui::Event>,
    receivers: ProductPointerInputSnapshot,
}

pub(super) struct RunInputSnapshot;

impl egui::plugin::Plugin for RunInputSnapshot {
    fn debug_name(&self) -> &'static str {
        "egui_dockspace::run_input_snapshot"
    }

    fn input_hook(&mut self, context: &Context, _input: &mut RawInput) {
        // egui can reject recursive root borrowing before on_begin_pass runs.
        if in_run_callback(context)
            && let Some(portal) = active_run_portal(context)
        {
            portal
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .poisoned = true;
        }
    }

    fn on_begin_pass(&mut self, ui: &mut Ui) {
        let Some(portal) = active_run_portal(ui.ctx()) else {
            return;
        };
        let mut portal = portal.lock().unwrap_or_else(PoisonError::into_inner);
        if in_run_callback(ui.ctx()) {
            // A raw recursive Context::run_ui has no inherited Ui stack tag.
            portal.poisoned = true;
            return;
        }
        if portal.viewport == ui.ctx().viewport_id() && portal.input.is_none() {
            portal.input = Some(Arc::new(RunInput {
                events: ui.ctx().input(|input| input.raw.events.clone()),
                receivers: ProductPointerInputSnapshot::capture(ui.ctx()),
            }));
        }
    }
}

#[derive(Clone)]
pub(crate) struct ProductPointerPacket {
    run: Arc<RunIdentity>,
    input: Arc<RunInput>,
    observed_at: Option<std::time::Duration>,
}

impl ProductPointerPacket {
    pub(crate) fn same_run(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.run, &other.run)
    }

    pub(crate) fn events(&self) -> &[egui::Event] {
        &self.input.events
    }

    pub(crate) fn receivers(&self) -> &ProductPointerInputSnapshot {
        &self.input.receivers
    }

    pub(crate) const fn observed_at(&self) -> Option<std::time::Duration> {
        self.observed_at
    }
}

pub(crate) fn pointer_packet(ui: &Ui) -> Option<ProductPointerPacket> {
    let portal = active_run_portal(ui.ctx())?;
    let portal = portal.lock().unwrap_or_else(PoisonError::into_inner);
    let scope = ui_run_scope(ui)?;
    if portal.poisoned
        || portal.current_pass != Some(scope.pass)
        || portal.viewport != ui.ctx().viewport_id()
        || !Arc::ptr_eq(&scope.run, &portal.run)
    {
        return None;
    }
    let input = Arc::clone(portal.input.as_ref()?);
    Some(ProductPointerPacket {
        run: Arc::clone(&portal.run),
        input,
        observed_at: portal.observed_at,
    })
}

#[derive(Clone)]
pub(crate) struct RunScope {
    pub(super) run: Arc<RunIdentity>,
    pub(super) pass: u64,
}

thread_local! {
    static RUN_CALLBACKS: RefCell<Vec<RunCallback>> = const { RefCell::new(Vec::new()) };
}

struct RunCallback {
    context: Context,
    viewport: ViewportId,
    frame: u64,
    completed_pass: u64,
    pass_index: usize,
    scope: RunScope,
}

fn in_run_callback(context: &Context) -> bool {
    RUN_CALLBACKS.with(|callbacks| {
        callbacks
            .borrow()
            .iter()
            .any(|callback| callback.context == *context)
    })
}

#[must_use = "keep the guard alive until the synchronous application callback returns"]
pub(crate) struct RunCallbackGuard;

impl RunScope {
    pub(crate) fn enter(&self, ui: &Ui) -> RunCallbackGuard {
        RUN_CALLBACKS.with(|callbacks| {
            callbacks.borrow_mut().push(RunCallback {
                context: ui.ctx().clone(),
                viewport: ui.ctx().viewport_id(),
                frame: ui.ctx().cumulative_frame_nr(),
                completed_pass: ui.ctx().cumulative_pass_nr(),
                pass_index: ui.ctx().current_pass_index(),
                scope: self.clone(),
            });
        });
        RunCallbackGuard
    }
}

impl Drop for RunCallbackGuard {
    fn drop(&mut self) {
        RUN_CALLBACKS.with(|callbacks| {
            callbacks.borrow_mut().pop();
        });
    }
}

pub(super) fn ui_run_scope(ui: &Ui) -> Option<RunScope> {
    let tagged = ui.stack().iter().find_map(|stack| {
        stack
            .tags()
            .get_downcast::<RunScope>(RUN_SCOPE_TAG)
            .cloned()
    });
    RUN_CALLBACKS.with(|callbacks| {
        let callbacks = callbacks.borrow();
        let callback = callbacks.last()?;
        let source_matches = match tagged {
            Some(scope) => {
                scope.pass == callback.scope.pass && Arc::ptr_eq(&scope.run, &callback.scope.run)
            }
            None => {
                // Areas create independent root Uis. Limit the callback-local
                // fallback to their declared area root and matching layer ID.
                ui.stack()
                    .iter()
                    .last()
                    .is_some_and(|root| root.is_area_ui() && root.unique_id == ui.layer_id().id)
            }
        };
        (callback.context == *ui.ctx()
            && callback.viewport == ui.ctx().viewport_id()
            && callback.frame == ui.ctx().cumulative_frame_nr()
            && callback.completed_pass == ui.ctx().cumulative_pass_nr()
            && callback.pass_index == ui.ctx().current_pass_index()
            && source_matches)
            .then(|| callback.scope.clone())
    })
}
