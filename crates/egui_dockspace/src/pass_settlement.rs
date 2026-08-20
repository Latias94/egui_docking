//! Final-pass settlement for callback-only product rendering.

use std::sync::{Arc, Mutex, PoisonError, Weak};

use dockspace::runtime::{PreparedPaneFocusObservation, PreparedSurfaceAction};
use egui::{Context, Id, Ui, ViewportId};

#[derive(Default)]
pub(super) struct ProductPassSettlementBatch {
    pub(super) presentation_actions: Vec<PreparedSurfaceAction>,
    pub(super) pane_focus_observation: Option<PreparedPaneFocusObservation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum PreviousPassSettlement {
    None,
    SameRun,
    Settled,
    Rejected,
}

pub(super) struct ProductPassSettlementReady {
    pub(super) batch: ProductPassSettlementBatch,
    pub(super) previous: PreviousPassSettlement,
}

#[derive(Default)]
pub(super) struct ProductPassSettlement {
    state: Mutex<PassSettlementState>,
}

impl ProductPassSettlement {
    pub(super) fn take_ready(&self, ui: &Ui) -> ProductPassSettlementReady {
        let identity = context_identity(ui.ctx());
        let stamp = PassStamp::from_ui(ui);
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.take_ready(&identity, ui.ctx().viewport_id(), stamp)
    }

    pub(super) fn stage(&self, ui: &Ui, batch: ProductPassSettlementBatch) -> bool {
        let identity = context_identity(ui.ctx());
        let Some(stamp) = PassStamp::from_ui(ui) else {
            return false;
        };
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        state.stage(&identity, ui.ctx().viewport_id(), stamp, batch);
        true
    }
}

#[derive(Default)]
struct PassSettlementState {
    lane: Option<SettlementLane>,
}

impl PassSettlementState {
    fn take_ready(
        &mut self,
        identity: &Arc<ContextIdentity>,
        viewport: ViewportId,
        current: Option<PassStamp>,
    ) -> ProductPassSettlementReady {
        let replaced_pending = self.activate_lane(identity, viewport);
        let lane = self
            .lane
            .as_mut()
            .expect("activating a settlement lane installs it");
        let Some(staged) = lane.staged.take() else {
            return ProductPassSettlementReady {
                batch: ProductPassSettlementBatch::default(),
                previous: if replaced_pending {
                    PreviousPassSettlement::Rejected
                } else {
                    PreviousPassSettlement::None
                },
            };
        };
        let Some(current) = current else {
            return ProductPassSettlementReady {
                batch: ProductPassSettlementBatch::default(),
                previous: PreviousPassSettlement::Rejected,
            };
        };

        if current.frame == staged.stamp.frame
            && current.run_start_pass == staged.stamp.run_start_pass
        {
            lane.staged = Some(staged);
            return ProductPassSettlementReady {
                batch: ProductPassSettlementBatch::default(),
                previous: PreviousPassSettlement::SameRun,
            };
        }

        let terminal_pass = current
            .run_start_pass
            .checked_sub(staged.stamp.run_start_pass)
            .and_then(|passes| passes.checked_sub(1));
        let exact_successor = staged
            .stamp
            .frame
            .checked_add(1)
            .is_some_and(|frame| current.frame == frame);
        if exact_successor && terminal_pass == Some(staged.stamp.pass_index) {
            ProductPassSettlementReady {
                batch: staged.batch,
                previous: PreviousPassSettlement::Settled,
            }
        } else {
            ProductPassSettlementReady {
                batch: ProductPassSettlementBatch::default(),
                previous: PreviousPassSettlement::Rejected,
            }
        }
    }

    fn stage(
        &mut self,
        identity: &Arc<ContextIdentity>,
        viewport: ViewportId,
        stamp: PassStamp,
        batch: ProductPassSettlementBatch,
    ) {
        self.activate_lane(identity, viewport);
        self.lane
            .as_mut()
            .expect("activating a settlement lane installs it")
            .staged = Some(StagedPass { stamp, batch });
    }

    fn activate_lane(&mut self, identity: &Arc<ContextIdentity>, viewport: ViewportId) -> bool {
        let matches = self.lane.as_ref().is_some_and(|lane| {
            lane.viewport == viewport
                && lane
                    .context
                    .upgrade()
                    .is_some_and(|current| Arc::ptr_eq(&current, identity))
        });
        if matches {
            return false;
        }

        let replaced_pending = self.lane.take().is_some_and(|lane| lane.staged.is_some());
        self.lane = Some(SettlementLane {
            context: Arc::downgrade(identity),
            viewport,
            staged: None,
        });
        replaced_pending
    }
}

struct SettlementLane {
    context: Weak<ContextIdentity>,
    viewport: ViewportId,
    staged: Option<StagedPass>,
}

struct StagedPass {
    stamp: PassStamp,
    batch: ProductPassSettlementBatch,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PassStamp {
    frame: u64,
    run_start_pass: u64,
    pass_index: u64,
}

impl PassStamp {
    fn from_ui(ui: &Ui) -> Option<Self> {
        let pass_index = u64::try_from(ui.ctx().current_pass_index()).ok()?;
        let cumulative_pass = ui.ctx().cumulative_pass_nr();
        Some(Self {
            frame: ui.ctx().cumulative_frame_nr(),
            run_start_pass: cumulative_pass.checked_sub(pass_index)?,
            pass_index,
        })
    }
}

struct ContextIdentity;

fn context_identity(context: &Context) -> Arc<ContextIdentity> {
    let id = Id::new(("egui_dockspace", "pass_settlement_context_identity"));
    context.data_mut(|data| {
        if let Some(identity) = data.get_temp::<Arc<ContextIdentity>>(id) {
            identity
        } else {
            let identity = Arc::new(ContextIdentity);
            data.insert_temp(id, Arc::clone(&identity));
            identity
        }
    })
}

#[cfg(test)]
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use egui::{Id, Pos2, RawInput, RepaintCause, UiBuilder, Vec2};

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

        fn output_hook(&mut self, _context: &Context, output: &mut egui::FullOutput) {
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

    fn clear_textures(output: &mut egui::FullOutput) {
        output.textures_delta.clear();
    }

    fn staged_count(settlement: &ProductPassSettlement) -> usize {
        settlement
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .lane
            .as_ref()
            .and_then(|lane| lane.staged.as_ref())
            .map_or(0, |_| 1)
    }

    #[test]
    fn successful_root_run_settles_on_the_next_frame() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::None
            );
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        assert_eq!(staged_count(&settlement), 1);

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::Settled
            );
        });
        clear_textures(&mut output);
        assert_eq!(staged_count(&settlement), 0);
    }

    #[test]
    fn non_root_run_uses_the_exact_viewport_counters() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let child = ViewportId::from_hash_of("dockspace-automatic-child-run");
        let mut output = context.run_ui(input_for(child), |ui| {
            assert_eq!(ui.ctx().viewport_id(), child);
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);

        let mut output = context.run_ui(input_for(child), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::Settled
            );
        });
        clear_textures(&mut output);
    }

    #[test]
    fn late_discard_keeps_only_the_terminal_pass() {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 3.try_into().expect("three is non-zero");
        });
        context
            .plugin_or_default::<LateOutputHook>()
            .lock()
            .discard_remaining = 1;
        let settlement = ProductPassSettlement::default();
        let mut observed = Vec::new();

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            observed.push(ui.ctx().current_pass_index());
            let previous = settlement.take_ready(ui).previous;
            assert!(matches!(
                previous,
                PreviousPassSettlement::None | PreviousPassSettlement::SameRun
            ));
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        assert_eq!(observed, [0, 1]);

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::Settled
            );
        });
        clear_textures(&mut output);
    }

    #[test]
    fn omitted_terminal_pass_rejects_discarded_evidence() {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 3.try_into().expect("three is non-zero");
        });
        let settlement = ProductPassSettlement::default();

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            if ui.ctx().current_pass_index() == 0 {
                assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
                ui.ctx()
                    .request_discard("omit Dockspace from the terminal pass");
            }
        });
        clear_textures(&mut output);

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::Rejected
            );
        });
        clear_textures(&mut output);
    }

    #[test]
    fn late_output_panic_rejects_the_interrupted_run() {
        let context = Context::default();
        let mut warm_up = context.run_ui(input_for(ViewportId::ROOT), |_| {});
        clear_textures(&mut warm_up);
        context
            .plugin_or_default::<LateOutputHook>()
            .lock()
            .panic_once = true;
        let settlement = ProductPassSettlement::default();

        let interrupted = catch_unwind(AssertUnwindSafe(|| {
            let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
                assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
            });
            clear_textures(&mut output);
        }));
        assert!(interrupted.is_err());

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::Rejected
            );
        });
        clear_textures(&mut output);
    }

    #[test]
    fn independent_manual_passes_do_not_impersonate_a_completed_run() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();

        context.begin_pass(input_for(ViewportId::ROOT));
        let ui = Ui::new(
            context.clone(),
            Id::new("dockspace-manual-pass-one"),
            UiBuilder::new().max_rect(context.viewport_rect()),
        );
        assert!(settlement.stage(&ui, ProductPassSettlementBatch::default()));
        let mut output = context.end_pass();
        clear_textures(&mut output);

        context.begin_pass(input_for(ViewportId::ROOT));
        let ui = Ui::new(
            context.clone(),
            Id::new("dockspace-manual-pass-two"),
            UiBuilder::new().max_rect(context.viewport_rect()),
        );
        assert_eq!(
            settlement.take_ready(&ui).previous,
            PreviousPassSettlement::Rejected
        );
        let mut output = context.end_pass();
        clear_textures(&mut output);
    }

    #[test]
    fn context_churn_abandons_old_evidence_without_retaining_the_context() {
        let settlement = ProductPassSettlement::default();
        let old = Context::default();
        let old_identity = context_identity(&old);
        let old_weak = Arc::downgrade(&old_identity);
        let mut output = old.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        drop(old_identity);
        drop(old);
        assert!(old_weak.upgrade().is_none());

        let current = Context::default();
        let mut output = current.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).previous,
                PreviousPassSettlement::Rejected
            );
        });
        clear_textures(&mut output);
        assert_eq!(staged_count(&settlement), 0);
    }
}
