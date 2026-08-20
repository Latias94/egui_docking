//! Final-pass settlement for callback-only product rendering.

use std::sync::{Arc, Mutex, PoisonError};

use dockspace::runtime::{PreparedPaneFocusObservation, PreparedSurfaceAction};
use egui::{Context, FullOutput, RawInput, Ui, ViewportId};

#[derive(Default)]
pub(super) struct ProductPassSettlementBatch {
    pub(super) presentation_actions: Vec<PreparedSurfaceAction>,
    pub(super) pane_focus_observation: Option<PreparedPaneFocusObservation>,
}

#[derive(Default)]
pub(super) struct ProductPassSettlement {
    state: Arc<Mutex<PassSettlementState>>,
}

impl ProductPassSettlement {
    pub(super) fn begin_run(&self, context: &Context, input: &RawInput) -> DockspaceEguiRun {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let identity = state.begin_run(context, input.viewport_id);
        DockspaceEguiRun {
            state: Arc::clone(&self.state),
            context: context.clone(),
            viewport: input.viewport_id,
            identity,
            finished: false,
        }
    }

    pub(super) fn take_ready(&self, ui: &Ui) -> ProductPassSettlementBatch {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.take_ready(ui.ctx(), ui.ctx().viewport_id())
    }

    pub(super) fn stage(&self, ui: &Ui, batch: ProductPassSettlementBatch) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.stage(
            ui.ctx(),
            ui.ctx().viewport_id(),
            ui.ctx().current_pass_index(),
            batch,
        )
    }
}

/// One exact official-egui run whose terminal output may settle Dockspace
/// presentation acknowledgements.
///
/// Create this token with [`crate::Dockspace::begin_egui_run`] before calling
/// [`Context::run_ui`] or [`Context::begin_pass`]. Consume it with
/// [`Self::settle`] only after the matching run or manual pass returns its
/// [`FullOutput`]. Dropping the token, including during unwinding, abandons all
/// presentation evidence staged by that run.
#[must_use = "the egui run must be settled with its terminal FullOutput or explicitly dropped"]
pub struct DockspaceEguiRun {
    state: Arc<Mutex<PassSettlementState>>,
    context: Context,
    viewport: ViewportId,
    identity: Arc<RunIdentity>,
    finished: bool,
}

impl DockspaceEguiRun {
    /// Accepts presentation evidence only from the terminal pass of this exact
    /// egui run.
    ///
    /// For [`Context::run_ui`], call this after `run_ui` returns. For the
    /// low-level API, call it after the matching [`Context::end_pass`]. A pass
    /// that did not render Dockspace, or a staged batch from an earlier
    /// discarded pass, is rejected fail-closed.
    pub fn settle(mut self, output: &FullOutput) {
        let terminal_pass = output.platform_output.num_completed_passes.checked_sub(1);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.settle_run(&self.context, self.viewport, &self.identity, terminal_pass);
        self.finished = true;
    }
}

impl Drop for DockspaceEguiRun {
    fn drop(&mut self) {
        if self.finished {
            return;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.abandon_run(&self.context, self.viewport, &self.identity);
    }
}

#[derive(Default)]
struct PassSettlementState {
    active_runs: Vec<ActiveRun>,
    staged: Vec<StagedPass>,
    ready: Vec<ReadyPass>,
}

impl PassSettlementState {
    fn begin_run(&mut self, context: &Context, viewport: ViewportId) -> Arc<RunIdentity> {
        let replaced = self
            .active_runs
            .iter()
            .position(|active| active.context == *context && active.viewport == viewport)
            .map(|position| self.active_runs.swap_remove(position));
        if let Some(replaced) = replaced {
            self.staged
                .retain(|staged| !Arc::ptr_eq(&staged.identity, &replaced.identity));
        }

        let identity = Arc::new(RunIdentity);
        self.active_runs.push(ActiveRun {
            context: context.clone(),
            viewport,
            identity: Arc::clone(&identity),
        });
        identity
    }

    fn stage(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        pass_index: usize,
        batch: ProductPassSettlementBatch,
    ) -> bool {
        let Some(identity) = self
            .active_runs
            .iter()
            .find(|active| active.context == *context && active.viewport == viewport)
            .map(|active| Arc::clone(&active.identity))
        else {
            return false;
        };
        if let Some(staged) = self
            .staged
            .iter_mut()
            .find(|staged| Arc::ptr_eq(&staged.identity, &identity))
        {
            *staged = StagedPass {
                context: context.clone(),
                viewport,
                identity,
                pass_index,
                batch,
            };
            return true;
        }

        self.staged.push(StagedPass {
            context: context.clone(),
            viewport,
            identity,
            pass_index,
            batch,
        });
        true
    }

    fn settle_run(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        identity: &Arc<RunIdentity>,
        terminal_pass: Option<usize>,
    ) {
        let Some(active_position) = self.active_runs.iter().position(|active| {
            active.context == *context
                && active.viewport == viewport
                && Arc::ptr_eq(&active.identity, identity)
        }) else {
            return;
        };
        self.active_runs.swap_remove(active_position);

        let mut retained = Vec::with_capacity(self.staged.len());
        for staged in std::mem::take(&mut self.staged) {
            if Arc::ptr_eq(&staged.identity, identity) {
                if terminal_pass == Some(staged.pass_index) {
                    self.ready.push(ReadyPass {
                        context: staged.context,
                        viewport: staged.viewport,
                        batch: staged.batch,
                    });
                }
            } else {
                retained.push(staged);
            }
        }
        self.staged = retained;
    }

    fn abandon_run(
        &mut self,
        context: &Context,
        viewport: ViewportId,
        identity: &Arc<RunIdentity>,
    ) {
        self.active_runs.retain(|active| {
            active.context != *context
                || active.viewport != viewport
                || !Arc::ptr_eq(&active.identity, identity)
        });
        self.staged
            .retain(|staged| !Arc::ptr_eq(&staged.identity, identity));
    }

    fn take_ready(
        &mut self,
        context: &Context,
        viewport: ViewportId,
    ) -> ProductPassSettlementBatch {
        let mut batch = ProductPassSettlementBatch::default();
        let mut retained = Vec::with_capacity(self.ready.len());
        for ready in std::mem::take(&mut self.ready) {
            if ready.context == *context && ready.viewport == viewport {
                batch
                    .presentation_actions
                    .extend(ready.batch.presentation_actions);
                if ready.batch.pane_focus_observation.is_some() {
                    batch.pane_focus_observation = ready.batch.pane_focus_observation;
                }
            } else {
                retained.push(ready);
            }
        }
        self.ready = retained;
        batch
    }
}

struct RunIdentity;

struct ActiveRun {
    context: Context,
    viewport: ViewportId,
    identity: Arc<RunIdentity>,
}

struct StagedPass {
    context: Context,
    viewport: ViewportId,
    identity: Arc<RunIdentity>,
    pass_index: usize,
    batch: ProductPassSettlementBatch,
}

struct ReadyPass {
    context: Context,
    viewport: ViewportId,
    batch: ProductPassSettlementBatch,
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use egui::{Id, Pos2, RepaintCause, UiBuilder, Vec2};

    use super::*;

    #[derive(Default)]
    struct LateOutputHook {
        discard_remaining: usize,
        panic_once: bool,
    }

    impl egui::plugin::Plugin for LateOutputHook {
        fn debug_name(&self) -> &'static str {
            "egui_dockspace::test::late_output_hook"
        }

        fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
            if self.panic_once {
                self.panic_once = false;
                panic!("late output hook interrupted the egui run");
            }
            if self.discard_remaining == 0 {
                return;
            }
            self.discard_remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(RepaintCause::new_reason(
                    "late output hook requested another pass",
                ));
        }
    }

    fn input_for(viewport: ViewportId) -> RawInput {
        let mut input = RawInput {
            viewport_id: viewport,
            screen_rect: Some(egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(128.0))),
            ..RawInput::default()
        };
        input.viewports.entry(viewport).or_default();
        input
    }

    fn clear_textures(output: &mut FullOutput) {
        output.textures_delta.clear();
    }

    fn assert_state_counts(
        settlement: &ProductPassSettlement,
        active: usize,
        staged: usize,
        ready: usize,
    ) {
        let state = settlement
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert_eq!(state.active_runs.len(), active);
        assert_eq!(state.staged.len(), staged);
        assert_eq!(state.ready.len(), ready);
    }

    #[test]
    fn explicit_root_run_settles_its_terminal_output() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let input = input_for(ViewportId::ROOT);
        let run = settlement.begin_run(&context, &input);

        let mut output = context.run_ui(input, |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        run.settle(&output);
        clear_textures(&mut output);

        assert_state_counts(&settlement, 0, 0, 1);
    }

    #[test]
    fn explicit_manual_pass_settles_after_end_pass() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let input = input_for(ViewportId::ROOT);
        let run = settlement.begin_run(&context, &input);

        context.begin_pass(input);
        let ui = Ui::new(
            context.clone(),
            Id::new("dockspace-explicit-manual-pass"),
            UiBuilder::new().max_rect(context.viewport_rect()),
        );
        assert!(settlement.stage(&ui, ProductPassSettlementBatch::default()));
        let mut output = context.end_pass();
        run.settle(&output);
        clear_textures(&mut output);

        assert_state_counts(&settlement, 0, 0, 1);
    }

    #[test]
    fn explicit_non_root_run_keeps_the_input_viewport() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let child = ViewportId::from_hash_of("dockspace-explicit-child-run");
        let input = input_for(child);
        let run = settlement.begin_run(&context, &input);

        let mut output = context.run_ui(input, |ui| {
            assert_eq!(ui.ctx().viewport_id(), child);
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        run.settle(&output);
        clear_textures(&mut output);

        let state = settlement
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        assert!(state.active_runs.is_empty());
        assert!(state.staged.is_empty());
        assert_eq!(state.ready.len(), 1);
        assert_eq!(state.ready[0].viewport, child);
    }

    #[test]
    fn explicit_run_accepts_only_the_terminal_late_discard_pass() {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 3.try_into().expect("three is non-zero");
        });
        context
            .plugin_or_default::<LateOutputHook>()
            .lock()
            .discard_remaining = 1;
        let settlement = ProductPassSettlement::default();
        let input = input_for(ViewportId::ROOT);
        let run = settlement.begin_run(&context, &input);
        let mut observed_passes = Vec::new();

        let mut output = context.run_ui(input, |ui| {
            observed_passes.push(ui.ctx().current_pass_index());
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        assert_eq!(observed_passes, [0, 1]);
        {
            let state = settlement
                .state
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            assert_eq!(state.staged.len(), 1);
            assert_eq!(state.staged[0].pass_index, 1);
        }
        run.settle(&output);
        clear_textures(&mut output);

        assert_state_counts(&settlement, 0, 0, 1);
    }

    #[test]
    fn omitted_terminal_pass_rejects_discarded_evidence() {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 3.try_into().expect("three is non-zero");
        });
        context
            .plugin_or_default::<LateOutputHook>()
            .lock()
            .discard_remaining = 1;
        let settlement = ProductPassSettlement::default();
        let input = input_for(ViewportId::ROOT);
        let run = settlement.begin_run(&context, &input);

        let mut output = context.run_ui(input, |ui| {
            if ui.ctx().current_pass_index() == 0 {
                assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
            }
        });
        run.settle(&output);
        clear_textures(&mut output);

        assert_state_counts(&settlement, 0, 0, 0);
    }

    #[test]
    fn late_output_panic_abandons_the_exact_run() {
        let context = Context::default();
        let mut warm_up = context.run_ui(input_for(ViewportId::ROOT), |_| {});
        clear_textures(&mut warm_up);
        context
            .plugin_or_default::<LateOutputHook>()
            .lock()
            .panic_once = true;
        let settlement = ProductPassSettlement::default();

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let input = input_for(ViewportId::ROOT);
            let run = settlement.begin_run(&context, &input);
            let mut output = context.run_ui(input, |ui| {
                assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
            });
            run.settle(&output);
            clear_textures(&mut output);
        }));

        assert!(interrupted.is_err());
        assert_state_counts(&settlement, 0, 0, 0);
    }

    #[test]
    fn show_without_an_explicit_run_cannot_stage_evidence() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert!(!settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);

        assert_state_counts(&settlement, 0, 0, 0);
    }
}
