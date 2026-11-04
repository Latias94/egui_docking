//! Exact final-output ownership for caller-owned official egui runs.

use std::cell::RefCell;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use dockspace::runtime::{
    PaintedSurfaceOutput, PreparedPaneFocusObservation, SurfacePresentationResult,
};
use egui::{
    Context, FullOutput, Id, RawInput, TexturesDelta, Ui, UiBuilder, UiStackInfo, ViewportId,
};

use crate::output_proof::{
    SurfaceOutputProof, final_paint_layer_order, scrub_private_markers, spans_follow_paint_order,
};
use crate::pane::PaneFocusState;
use crate::product_pointer::ProductPointerInputSnapshot;

pub(super) const RUN_SCOPE_TAG: &str = "egui_dockspace::official_run_scope";

/// Complete egui output and the affine receipt for its docking emissions.
///
/// Split this value at the renderer boundary with [`Self::into_parts`]. Returning
/// from egui does not imply presentation. Dropping the receipt retires its exact
/// emissions without granting interaction or pane-focus authority.
#[must_use = "render the output and report its exact final disposition"]
pub struct DockspaceOutput {
    output: FullOutput,
    receipt: DockspaceOutputReceipt,
}

impl DockspaceOutput {
    pub(super) fn new(output: FullOutput, receipt: DockspaceOutputReceipt) -> Self {
        Self { output, receipt }
    }
    /// Transfers the output and its one-use settlement receipt to the host.
    ///
    /// The host must report `Presented` only after this complete output reaches
    /// presentation. Filtering or replacing its shapes requires `Dropped`.
    pub fn into_parts(self) -> (FullOutput, DockspaceOutputReceipt) {
        (self.output, self.receipt)
    }
}

/// One-use final-disposition port for the exact output returned by [`run_ui`].
///
/// A host may retain several receipts and report them in any order. Core settles
/// exact emission order at its next frame boundary. Reporting presentation is a
/// trusted host contract; this value does not observe GPU submission itself.
#[must_use = "report presentation, or drop this receipt to retire its emissions"]
pub struct DockspaceOutputReceipt {
    context: Context,
    viewport: ViewportId,
    deliveries: Vec<SurfaceDelivery>,
    discarded: Vec<PaintedSurfaceOutput>,
}

impl DockspaceOutputReceipt {
    /// Reports the final renderer result for this exact complete egui output.
    pub fn report(mut self, result: SurfacePresentationResult) {
        self.deliver(result);
    }

    fn deliver(&mut self, result: SurfacePresentationResult) {
        // Retire discarded predecessors together with the terminal result.
        // Retiring them during the next speculative pass would revoke the
        // stream while egui is still producing its replacement output.
        self.discarded.clear();
        let mut needs_repaint = false;
        for delivery in self.deliveries.drain(..) {
            let Some(state) = delivery.state.upgrade() else {
                continue;
            };
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            if state
                .lane
                .as_ref()
                .is_some_and(|lane| Arc::ptr_eq(&lane.identity, &delivery.lane))
            {
                needs_repaint |= delivery.needs_repaint;
                state.ready.push(ReadySurfacePresentation {
                    output: delivery.output,
                    result,
                    pane_focus_observation: if result == SurfacePresentationResult::Presented {
                        delivery.pane_focus_observation
                    } else {
                        None
                    },
                });
            }
        }
        if needs_repaint {
            self.context.request_repaint_of(self.viewport);
        }
    }
}

impl Drop for DockspaceOutputReceipt {
    fn drop(&mut self) {
        self.deliver(SurfacePresentationResult::Dropped);
    }
}

/// Runs a complete official egui frame and returns owned renderer settlement.
///
/// Repeated callbacks retire the discarded pass. Only the final pass, after
/// every output hook, can produce eligible receipts. Surface drawing must remain
/// synchronous on the supplied Ui layer; cross-layer writes conservatively make
/// that pass unavailable for presentation authority. Private markers are removed
/// before the output is returned. Unwinding retires the exact open pass.
///
/// An explicit, representable [`RawInput::time`] is retained as the observation
/// time of this entire ordered input batch. Missing or invalid time stays unknown;
/// egui's predicted frame time is not an input observation. This boundary does
/// not supply individual OS event timestamps or classify generic pointer events.
///
/// # Panics
///
/// Panics on recursive use with the same Context. Catching that panic does not
/// make the poisoned outer pass eligible for presentation.
pub fn run_ui(
    context: &Context,
    input: RawInput,
    mut add_contents: impl FnMut(&mut Ui),
) -> DockspaceOutput {
    let run = ProductPassSettlementRun::begin(context, &input);
    let mut output = context.run_ui(input, |ui| {
        let scope = run
            .begin_pass(ui)
            .expect("egui_dockspace::run_ui used a foreign context or viewport");
        let _callback = scope.enter(ui);
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(RUN_SCOPE_TAG)
                .ui_stack_info(UiStackInfo::default().with_tag_value(RUN_SCOPE_TAG, scope)),
            &mut add_contents,
        );
    });
    let receipt = run.complete(&mut output);
    DockspaceOutput { output, receipt }
}

#[derive(Default)]
pub(super) struct ProductPassSettlementBatch {
    pub(super) output: Option<PaintedSurfaceOutput>,
    pub(super) proof: Option<SurfaceOutputProof>,
    pub(super) pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pub(super) pane_focus_pending: Option<PendingPaneFocusObservation>,
    pub(super) needs_repaint: bool,
    pub(super) emission_incomplete: bool,
}

#[derive(Default)]
pub(super) struct ReadyProductPassSettlementBatch {
    pub(super) presentations: Vec<ReadySurfacePresentation>,
    pub(super) presentation_failure: Option<PresentationSettlementFailure>,
    pub(super) lane_changed: bool,
}

pub(super) struct ReadySurfacePresentation {
    pub(super) output: PaintedSurfaceOutput,
    pub(super) result: SurfacePresentationResult,
    pub(super) pane_focus_observation: Option<PreparedPaneFocusObservation>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PresentationSettlementFailure {
    OutputMismatch,
    Abandoned,
}

pub(super) struct PendingPaneFocusObservation {
    target: Option<Id>,
    observed_during_callback: PaneFocusState,
    focused: PreparedPaneFocusObservation,
    not_focused: PreparedPaneFocusObservation,
    unavailable: PreparedPaneFocusObservation,
}

impl PendingPaneFocusObservation {
    pub(super) fn new(
        target: Option<Id>,
        observed_during_callback: PaneFocusState,
        focused: PreparedPaneFocusObservation,
        not_focused: PreparedPaneFocusObservation,
        unavailable: PreparedPaneFocusObservation,
    ) -> Self {
        Self {
            target,
            observed_during_callback,
            focused,
            not_focused,
            unavailable,
        }
    }

    fn resolve(
        self,
        context: &Context,
        viewport: ViewportId,
        output: &FullOutput,
    ) -> PreparedPaneFocusObservation {
        let terminal = self.target.and_then(|target| {
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .filter(|update| {
                    update
                        .nodes
                        .iter()
                        .any(|(node, _)| *node == target.accesskit_id())
                })
                .map(|update| {
                    if update.focus == target.accesskit_id() {
                        PaneFocusState::Focused
                    } else {
                        PaneFocusState::Unfocused
                    }
                })
                .or_else(|| {
                    (viewport == ViewportId::ROOT).then(|| {
                        if context.input(|input| input.focused)
                            && context.memory(|memory| memory.has_focus(target))
                        {
                            PaneFocusState::Focused
                        } else {
                            PaneFocusState::Unfocused
                        }
                    })
                })
        });
        match (self.observed_during_callback, terminal) {
            (PaneFocusState::Focused, Some(PaneFocusState::Focused)) => self.focused,
            (PaneFocusState::Unfocused, Some(PaneFocusState::Unfocused)) => self.not_focused,
            _ => self.unavailable,
        }
    }
}

/// Per-Dockspace pending output ownership. Core remains the ordering authority.
#[derive(Default)]
pub(super) struct ProductPassSettlement {
    state: Arc<Mutex<PassSettlementState>>,
}

impl ProductPassSettlement {
    pub(super) fn enroll(&self, ui: &Ui) -> ProductPassEnrollment {
        let Some(portal) = active_run_portal(ui.ctx()) else {
            return ProductPassEnrollment::Unowned;
        };
        let scope = ui_run_scope(ui);
        let context = context_identity(ui.ctx());
        let viewport = ui.ctx().viewport_id();
        let mut portal = portal.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(pass) = portal.current_pass else {
            portal.poisoned = true;
            return ProductPassEnrollment::Conflict;
        };
        if portal.poisoned
            || portal.viewport != viewport
            || scope
                .as_ref()
                .is_none_or(|scope| scope.pass != pass || !Arc::ptr_eq(&scope.run, &portal.run))
        {
            portal.poisoned = true;
            return ProductPassEnrollment::Conflict;
        }
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.active.is_some() {
            portal.poisoned = true;
            return ProductPassEnrollment::Conflict;
        }
        if !state.lane_matches(&context, viewport) {
            state.lane_changed = state.lane.is_some();
            if state.lane.is_some() && !state.ready.is_empty() {
                state.failure = Some(PresentationSettlementFailure::Abandoned);
            }
            state.ready.clear();
            state.lane = Some(SettlementLane {
                context: Arc::downgrade(&context),
                viewport,
                identity: Arc::new(LaneIdentity),
            });
        }
        state.active = Some(OpenProductPass {
            run: Arc::clone(&portal.run),
            pass,
            batch: None,
        });
        portal.participants.push(Arc::downgrade(&self.state));
        ProductPassEnrollment::Acquired(ProductPassClaim {
            state: Arc::clone(&self.state),
            run: Arc::clone(&portal.run),
            pass,
        })
    }

    pub(super) fn take_ready(&self, ui: &Ui) -> ReadyProductPassSettlementBatch {
        let context = context_identity(ui.ctx());
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if !state.lane_matches(&context, ui.ctx().viewport_id()) {
            return ReadyProductPassSettlementBatch {
                lane_changed: true,
                ..ReadyProductPassSettlementBatch::default()
            };
        }
        ReadyProductPassSettlementBatch {
            presentations: std::mem::take(&mut state.ready),
            presentation_failure: state.failure.take(),
            lane_changed: std::mem::take(&mut state.lane_changed),
        }
    }
}

pub(super) enum ProductPassEnrollment {
    Acquired(ProductPassClaim),
    Unowned,
    Conflict,
}

pub(super) struct ProductPassClaim {
    state: Arc<Mutex<PassSettlementState>>,
    run: Arc<RunIdentity>,
    pass: u64,
}

impl ProductPassClaim {
    pub(super) fn stage(self, batch: ProductPassSettlementBatch) -> bool {
        let mut state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(active) = state
            .active
            .as_mut()
            .filter(|active| active.matches(&self.run, self.pass))
        else {
            return false;
        };
        if active.batch.is_some() {
            return false;
        }
        active.batch = Some(batch);
        true
    }
}

struct SurfaceDelivery {
    state: Weak<Mutex<PassSettlementState>>,
    lane: Arc<LaneIdentity>,
    output: PaintedSurfaceOutput,
    pane_focus_observation: Option<PreparedPaneFocusObservation>,
    needs_repaint: bool,
}

pub(super) struct ProductPassSettlementRun {
    context: Context,
    context_identity: Arc<ContextIdentity>,
    viewport: ViewportId,
    portal: Arc<Mutex<ActiveRunPortal>>,
    completed: bool,
    discarded: RefCell<Vec<PaintedSurfaceOutput>>,
}

impl ProductPassSettlementRun {
    pub(super) fn begin(context: &Context, input: &RawInput) -> Self {
        if let Some(active) = active_run_portal(context) {
            active
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .poisoned = true;
            panic!("nested egui_dockspace::run_ui calls on one Context are unsupported");
        }
        context.add_plugin(RunInputSnapshot);
        let portal = Arc::new(Mutex::new(ActiveRunPortal {
            run: Arc::new(RunIdentity),
            input: None,
            observed_at: input
                .time
                .and_then(|time| std::time::Duration::try_from_secs_f64(time).ok()),
            viewport: input.viewport_id,
            next_pass: 0,
            current_pass: None,
            poisoned: false,
            participants: Vec::new(),
        }));
        context.data_mut(|data| data.insert_temp(active_run_portal_id(), Arc::clone(&portal)));
        Self {
            context: context.clone(),
            context_identity: context_identity(context),
            viewport: input.viewport_id,
            portal,
            completed: false,
            discarded: RefCell::new(Vec::new()),
        }
    }

    pub(super) fn begin_pass(&self, ui: &Ui) -> Option<RunScope> {
        if ui.ctx().viewport_id() != self.viewport
            || !Arc::ptr_eq(&context_identity(ui.ctx()), &self.context_identity)
        {
            return None;
        }
        self.retire_terminal(false);
        let mut portal = self.portal.lock().unwrap_or_else(PoisonError::into_inner);
        portal.next_pass = portal
            .next_pass
            .checked_add(1)
            .expect("egui pass identity exhausted");
        portal.current_pass = Some(portal.next_pass);
        // A caught recursive run poisons the whole run, including later passes.
        Some(RunScope {
            run: Arc::clone(&portal.run),
            pass: portal.next_pass,
        })
    }

    pub(super) fn layer_order_witness(&self) -> Vec<egui::LayerId> {
        final_paint_layer_order(&self.context)
    }

    pub(super) fn complete(mut self, output: &mut FullOutput) -> DockspaceOutputReceipt {
        let terminal = self.take_terminal();
        let mut deliveries = Vec::new();
        if let Some((run, pass, poisoned, participants)) = terminal {
            let mut staged = Vec::new();
            let mut invalid = poisoned;
            let mut output_spans = Vec::new();
            for (participant_order, participant) in participants.into_iter().enumerate() {
                let Some(state) = participant.upgrade() else {
                    continue;
                };
                let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
                let Some(open) = state.take_open(&run, pass) else {
                    invalid = true;
                    continue;
                };
                let Some(batch) = open.batch else {
                    invalid = true;
                    state.failure = Some(PresentationSettlementFailure::Abandoned);
                    continue;
                };
                invalid |= batch.emission_incomplete;
                if batch.output.is_some() {
                    match batch
                        .proof
                        .as_ref()
                        .and_then(|proof| proof.present_spans(output))
                    {
                        Some(evidence) => {
                            output_spans.extend(
                                evidence
                                    .spans
                                    .into_iter()
                                    .enumerate()
                                    .map(|(span_order, span)| {
                                        (participant_order, span_order, span)
                                    }),
                            );
                        }
                        _ => invalid = true,
                    }
                }
                let lane = Arc::clone(
                    &state
                        .lane
                        .as_ref()
                        .expect("enrollment installs a lane")
                        .identity,
                );
                staged.push((participant, lane, batch));
            }
            invalid |= !spans_follow_paint_order(&mut output_spans, &self.layer_order_witness());
            for (participant, lane, batch) in staged {
                if invalid {
                    if let Some(state) = participant.upgrade() {
                        state.lock().unwrap_or_else(PoisonError::into_inner).failure =
                            Some(if poisoned {
                                PresentationSettlementFailure::Abandoned
                            } else {
                                PresentationSettlementFailure::OutputMismatch
                            });
                    }
                    continue;
                }
                let focus = batch
                    .pane_focus_pending
                    .map_or(batch.pane_focus_observation, |pending| {
                        Some(pending.resolve(&self.context, self.viewport, output))
                    });
                if let Some(output) = batch.output {
                    deliveries.push(SurfaceDelivery {
                        state: participant,
                        lane,
                        output,
                        pane_focus_observation: focus,
                        needs_repaint: batch.needs_repaint,
                    });
                }
            }
        }
        restore_retained_textures(&self.context, output);
        scrub_private_markers(output);
        self.remove_portal();
        self.completed = true;
        DockspaceOutputReceipt {
            context: self.context.clone(),
            viewport: self.viewport,
            deliveries,
            discarded: self.discarded.take(),
        }
    }

    pub(super) fn owns_only(&self, settlement: &ProductPassSettlement) -> bool {
        let portal = self.portal.lock().unwrap_or_else(PoisonError::into_inner);
        !portal.poisoned
            && portal.participants.len() == 1
            && portal.participants[0]
                .upgrade()
                .is_some_and(|state| Arc::ptr_eq(&state, &settlement.state))
    }

    fn take_terminal(
        &self,
    ) -> Option<(
        Arc<RunIdentity>,
        u64,
        bool,
        Vec<Weak<Mutex<PassSettlementState>>>,
    )> {
        let mut portal = self.portal.lock().unwrap_or_else(PoisonError::into_inner);
        portal.current_pass.take().map(|pass| {
            (
                Arc::clone(&portal.run),
                pass,
                portal.poisoned,
                std::mem::take(&mut portal.participants),
            )
        })
    }

    fn retire_terminal(&self, abandoned: bool) {
        let Some((run, pass, poisoned, participants)) = self.take_terminal() else {
            return;
        };
        for participant in participants {
            let Some(state) = participant.upgrade() else {
                continue;
            };
            let mut state = state.lock().unwrap_or_else(PoisonError::into_inner);
            if let Some(open) = state.take_open(&run, pass) {
                if let Some(output) = open.batch.and_then(|batch| batch.output) {
                    self.discarded.borrow_mut().push(output);
                }
                if abandoned || poisoned {
                    state.failure = Some(PresentationSettlementFailure::Abandoned);
                }
            }
        }
    }

    fn remove_portal(&self) {
        self.context.data_mut(|data| {
            if data
                .get_temp::<Arc<Mutex<ActiveRunPortal>>>(active_run_portal_id())
                .is_some_and(|current| Arc::ptr_eq(&current, &self.portal))
            {
                data.remove::<Arc<Mutex<ActiveRunPortal>>>(active_run_portal_id());
            }
        });
    }
}

impl Drop for ProductPassSettlementRun {
    fn drop(&mut self) {
        if !self.completed {
            self.retire_terminal(true);
            self.remove_portal();
        }
    }
}

struct ActiveRunPortal {
    input: Option<Arc<RunInput>>,
    observed_at: Option<std::time::Duration>,
    run: Arc<RunIdentity>,
    viewport: ViewportId,
    next_pass: u64,
    current_pass: Option<u64>,
    poisoned: bool,
    participants: Vec<Weak<Mutex<PassSettlementState>>>,
}

struct RunInput {
    events: Vec<egui::Event>,
    receivers: ProductPointerInputSnapshot,
}

struct RunInputSnapshot;

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
pub(super) struct ProductPointerPacket {
    run: Arc<RunIdentity>,
    input: Arc<RunInput>,
    observed_at: Option<std::time::Duration>,
}

impl ProductPointerPacket {
    pub(super) fn same_run(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.run, &other.run)
    }

    pub(super) fn events(&self) -> &[egui::Event] {
        &self.input.events
    }

    pub(super) fn receivers(&self) -> &ProductPointerInputSnapshot {
        &self.input.receivers
    }

    pub(super) const fn observed_at(&self) -> Option<std::time::Duration> {
        self.observed_at
    }
}

pub(super) fn pointer_packet(ui: &Ui) -> Option<ProductPointerPacket> {
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
pub(super) struct RunScope {
    run: Arc<RunIdentity>,
    pass: u64,
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
pub(super) struct RunCallbackGuard;

impl RunScope {
    pub(super) fn enter(&self, ui: &Ui) -> RunCallbackGuard {
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

struct RunIdentity;
struct ContextIdentity;
struct LaneIdentity;

#[derive(Default)]
struct PassSettlementState {
    lane: Option<SettlementLane>,
    active: Option<OpenProductPass>,
    ready: Vec<ReadySurfacePresentation>,
    failure: Option<PresentationSettlementFailure>,
    lane_changed: bool,
}

impl PassSettlementState {
    fn lane_matches(&self, context: &Arc<ContextIdentity>, viewport: ViewportId) -> bool {
        self.lane.as_ref().is_some_and(|lane| {
            lane.viewport == viewport
                && lane
                    .context
                    .upgrade()
                    .is_some_and(|current| Arc::ptr_eq(&current, context))
        })
    }

    fn take_open(&mut self, run: &Arc<RunIdentity>, pass: u64) -> Option<OpenProductPass> {
        if self
            .active
            .as_ref()
            .is_some_and(|active| active.matches(run, pass))
        {
            self.active.take()
        } else {
            None
        }
    }
}

struct SettlementLane {
    context: Weak<ContextIdentity>,
    viewport: ViewportId,
    identity: Arc<LaneIdentity>,
}
struct OpenProductPass {
    run: Arc<RunIdentity>,
    pass: u64,
    batch: Option<ProductPassSettlementBatch>,
}
impl OpenProductPass {
    fn matches(&self, run: &Arc<RunIdentity>, pass: u64) -> bool {
        self.pass == pass && Arc::ptr_eq(&self.run, run)
    }
}

fn active_run_portal_id() -> Id {
    Id::unique(("egui_dockspace", "active_official_run_portal"))
}

fn retained_textures_id() -> Id {
    Id::unique(("egui_dockspace", "retained_renderer_textures"))
}

#[derive(Clone, Default)]
struct RetainedTexturesDelta(TexturesDelta);

impl Drop for RetainedTexturesDelta {
    fn drop(&mut self) {
        self.0.clear();
    }
}

fn retain_texture_delta(context: &Context, delta: TexturesDelta) {
    if delta.is_empty() {
        return;
    }
    context.data_mut(|data| {
        data.get_temp_mut_or_default::<RetainedTexturesDelta>(retained_textures_id())
            .0
            .append(delta);
    });
}

pub(super) struct OutputTexturesGuard<'a> {
    context: &'a Context,
    delta: Option<TexturesDelta>,
}

impl<'a> OutputTexturesGuard<'a> {
    pub(super) fn take(context: &'a Context, output: &mut FullOutput) -> Self {
        Self {
            context,
            delta: Some(std::mem::take(&mut output.textures_delta)),
        }
    }

    pub(super) fn restore_into_output(mut self, output: &mut FullOutput) {
        output.textures_delta = self.delta.take().unwrap_or_default();
    }
}

impl Drop for OutputTexturesGuard<'_> {
    fn drop(&mut self) {
        if let Some(delta) = self.delta.take() {
            retain_texture_delta(self.context, delta);
        }
    }
}

fn restore_retained_textures(context: &Context, output: &mut FullOutput) {
    let Some(mut retained) =
        context.data_mut(|data| data.remove_temp::<RetainedTexturesDelta>(retained_textures_id()))
    else {
        return;
    };
    retained
        .0
        .append(std::mem::take(&mut output.textures_delta));
    output.textures_delta = std::mem::take(&mut retained.0);
}

fn active_run_portal(context: &Context) -> Option<Arc<Mutex<ActiveRunPortal>>> {
    context.data(|data| data.get_temp(active_run_portal_id()))
}
fn ui_run_scope(ui: &Ui) -> Option<RunScope> {
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
fn context_identity(context: &Context) -> Arc<ContextIdentity> {
    let id = Id::unique(("egui_dockspace", "pass_settlement_context_identity"));
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
#[path = "pass_settlement_tests.rs"]
mod tests;

#[cfg(test)]
mod output_textures_guard_tests {
    use super::*;

    #[test]
    fn output_delta_is_retained_when_a_later_result_path_returns_early() {
        let context = Context::default();
        let texture = egui::TextureId::User(42);
        let mut output = FullOutput::default();
        output.textures_delta.push(
            texture,
            egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([1, 1], egui::Color32::RED),
                egui::TextureOptions::NEAREST,
            ),
        );

        let result: Result<(), ()> = (|| {
            let _guard = OutputTexturesGuard::take(&context, &mut output);
            Err(())
        })();

        assert!(result.is_err());
        restore_retained_textures(&context, &mut output);
        assert!(output.textures_delta.set.contains_key(&texture));
        output.drop_without_applying_deltas();
    }

    #[test]
    fn consecutive_failed_outputs_preserve_set_and_free_append_semantics() {
        let context = Context::default();
        let texture = egui::TextureId::User(43);
        let mut first = TexturesDelta::default();
        first.push(
            texture,
            egui::epaint::ImageDelta::full(
                egui::ColorImage::filled([1, 1], egui::Color32::RED),
                egui::TextureOptions::NEAREST,
            ),
        );
        retain_texture_delta(&context, first);

        let mut second = TexturesDelta::default();
        second.free(texture);
        retain_texture_delta(&context, second);

        let mut output = FullOutput::default();
        restore_retained_textures(&context, &mut output);
        assert!(!output.textures_delta.set.contains_key(&texture));
        assert!(output.textures_delta.free.contains(&texture));
        output.drop_without_applying_deltas();
    }
}
