//! Final-pass settlement for callback-only product rendering.

use std::sync::{Arc, Mutex, PoisonError, Weak};

use dockspace::runtime::PreparedSurfaceAction;
use egui::{Context, FullOutput, Ui, ViewportId};

#[derive(Default)]
pub(super) struct ProductPassSettlement {
    state: Arc<Mutex<PassSettlementState>>,
}

impl ProductPassSettlement {
    pub(super) fn take_ready(&self, ui: &Ui) -> Vec<PreparedSurfaceAction> {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.take_ready(ui.ctx(), ui.ctx().viewport_id())
    }

    pub(super) fn stage(&self, ui: &Ui, actions: Vec<PreparedSurfaceAction>) {
        let weak = Arc::downgrade(&self.state);
        ui.ctx()
            .plugin_or_default::<DockspacePassSettlementPlugin>()
            .lock()
            .register(weak);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.stage(ui.ctx(), ui.ctx().viewport_id(), actions);
    }
}

#[derive(Default)]
struct PassSettlementState {
    staged: Vec<StagedPass>,
    ready: Vec<ReadyPass>,
}

impl PassSettlementState {
    fn stage(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        actions: Vec<PreparedSurfaceAction>,
    ) {
        let pass_index = context.current_pass_index();
        let frame_nr = context.cumulative_frame_nr_for(viewport);
        if let Some(staged) = self
            .staged
            .iter_mut()
            .find(|staged| staged.context == *context && staged.viewport == viewport)
        {
            *staged = StagedPass {
                context: context.clone(),
                viewport,
                pass_index,
                frame_nr,
                observed_output: false,
                actions,
            };
            return;
        }

        self.staged.push(StagedPass {
            context: context.clone(),
            viewport,
            pass_index,
            frame_nr,
            observed_output: false,
            actions,
        });
    }

    fn begin_pass(&mut self, context: &Context, viewport: ViewportId, pass_index: usize) {
        // Reaching pass zero proves that the prior run returned without another
        // multipass iteration. Non-zero means the prior candidate was discarded.
        let mut retained = Vec::with_capacity(self.staged.len());
        for staged in std::mem::take(&mut self.staged) {
            if staged.context == *context && staged.viewport == viewport {
                let completed_run = staged
                    .frame_nr
                    .checked_add(1)
                    .is_some_and(|next| context.cumulative_frame_nr_for(viewport) == next);
                if pass_index == 0 && staged.observed_output && completed_run {
                    self.ready.push(ReadyPass {
                        context: staged.context,
                        viewport: staged.viewport,
                        actions: staged.actions,
                    });
                }
            } else {
                retained.push(staged);
            }
        }
        self.staged = retained;
    }

    fn observe_finished_pass(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        finished_pass_index: usize,
    ) {
        // Plugins registered from inside a pass are absent from that run's
        // begin-pass snapshot, but end-pass hooks use the current registry.
        // Observing a later pass therefore closes that first-registration gap.
        let mut retained = Vec::with_capacity(self.staged.len());
        for mut staged in std::mem::take(&mut self.staged) {
            if staged.context == *context && staged.viewport == viewport {
                if staged.pass_index == finished_pass_index {
                    staged.observed_output = true;
                    retained.push(staged);
                } else if staged.pass_index > finished_pass_index {
                    retained.push(staged);
                }
            } else {
                retained.push(staged);
            }
        }
        self.staged = retained;
    }

    fn take_ready(
        &mut self,
        context: &Context,
        viewport: ViewportId,
    ) -> Vec<PreparedSurfaceAction> {
        let mut actions = Vec::new();
        let mut retained = Vec::with_capacity(self.ready.len());
        for ready in std::mem::take(&mut self.ready) {
            if ready.context == *context && ready.viewport == viewport {
                actions.extend(ready.actions);
            } else {
                retained.push(ready);
            }
        }
        self.ready = retained;
        actions
    }
}

struct StagedPass {
    context: Context,
    viewport: ViewportId,
    pass_index: usize,
    frame_nr: u64,
    observed_output: bool,
    actions: Vec<PreparedSurfaceAction>,
}

struct ReadyPass {
    context: Context,
    viewport: ViewportId,
    actions: Vec<PreparedSurfaceAction>,
}

#[derive(Default)]
struct DockspacePassSettlementPlugin {
    states: Vec<Weak<Mutex<PassSettlementState>>>,
}

impl DockspacePassSettlementPlugin {
    fn register(&mut self, state: Weak<Mutex<PassSettlementState>>) {
        if !self
            .states
            .iter()
            .any(|registered| Weak::ptr_eq(registered, &state))
        {
            self.states.push(state);
        }
    }

    fn for_each_state(&mut self, mut visit: impl FnMut(&mut PassSettlementState)) {
        self.states.retain(|state| {
            let Some(state) = state.upgrade() else {
                return false;
            };
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            visit(&mut state);
            true
        });
    }
}

impl egui::plugin::Plugin for DockspacePassSettlementPlugin {
    fn debug_name(&self) -> &'static str {
        "egui_dockspace::final_pass_settlement"
    }

    fn on_begin_pass(&mut self, ui: &mut Ui) {
        let context = ui.ctx();
        let viewport = context.viewport_id();
        let pass_index = context.current_pass_index();
        self.for_each_state(|state| state.begin_pass(context, viewport, pass_index));
    }

    fn output_hook(&mut self, context: &Context, output: &mut FullOutput) {
        let viewport = context.viewport_id();
        let Some(finished_pass_index) = output.platform_output.num_completed_passes.checked_sub(1)
        else {
            return;
        };
        self.for_each_state(|state| {
            state.observe_finished_pass(context, viewport, finished_pass_index);
        });
    }
}
