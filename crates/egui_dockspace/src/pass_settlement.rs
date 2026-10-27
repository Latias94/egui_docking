//! Final-pass settlement for caller-owned official egui runs.

use std::any::Any;
use std::sync::{Arc, Mutex, PoisonError, Weak};

use dockspace::runtime::{PreparedPaneFocusObservation, PreparedSurfaceAction};
use egui::epaint::PaintCallback;
use egui::{
    Context, FullOutput, Id, Painter, RawInput, Shape, Ui, UiBuilder, UiStackInfo, ViewportId,
};

use crate::pane::PaneFocusState;
use crate::renderer_pass_bridge::{
    OpenPassBinding, RendererPassBridge, RendererPassReceiptIssuer, RendererPassSettlement,
};

const RUN_SCOPE_TAG: &str = "egui_dockspace::official_run_scope";

/// Runs one complete caller-owned egui frame with a private terminal-output port.
///
/// Every [`crate::Dockspace`] painted by `add_contents` may enroll once in each
/// egui pass. Repeated callbacks discard their predecessor, the final callback
/// settles only after all output hooks return, and unwind abandons the exact
/// open pass. The returned output contains no Dockspace bookkeeping shapes.
///
/// # Panics
///
/// Panics when called recursively for the same [`Context`]. The outer run is
/// poisoned before the panic so catching it cannot promote outer evidence.
#[must_use]
pub fn run_ui(
    context: &Context,
    input: RawInput,
    mut add_contents: impl FnMut(&mut Ui),
) -> FullOutput {
    let run = ProductPassSettlementRun::begin(context, input.viewport_id);
    let mut output = context.run_ui(input, |ui| {
        let scope = run
            .begin_pass(ui)
            .expect("egui_dockspace::run_ui callback used a foreign context or viewport");
        ui.scope_builder(
            UiBuilder::new()
                .id_salt(RUN_SCOPE_TAG)
                .ui_stack_info(UiStackInfo::default().with_tag_value(RUN_SCOPE_TAG, scope)),
            &mut add_contents,
        );
    });
    run.complete(&mut output);
    output
}

#[derive(Default)]
pub(super) struct ProductPassSettlementBatch {
    pub(super) presentation_evidence: Vec<PresentationEvidence>,
    pub(super) pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pub(super) pane_focus_pending: Option<PendingPaneFocusObservation>,
}

#[derive(Default)]
pub(super) struct ReadyProductPassSettlementBatch {
    pub(super) presentation_actions: Vec<PreparedSurfaceAction>,
    pub(super) pane_focus_observation: Option<PreparedPaneFocusObservation>,
    pub(super) presentation_failure: Option<PresentationSettlementFailure>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PresentationSettlementFailure {
    OutputMismatch,
    Abandoned,
}

pub(super) struct PresentationEvidence {
    pub(super) action: PreparedSurfaceAction,
    pub(super) proof: Option<FinalOutputShapeProof>,
}

#[derive(Clone)]
pub(super) struct PresentationProofScope {
    run: Arc<RunIdentity>,
    pass: u64,
}

impl ProductPassSettlementBatch {
    fn into_ready(
        self,
        context: &Context,
        viewport: ViewportId,
        output: &FullOutput,
    ) -> ReadyProductPassSettlementBatch {
        let output_mismatch = self.presentation_evidence.iter().any(|evidence| {
            evidence
                .proof
                .as_ref()
                .is_none_or(|proof| !proof.is_present_once(output))
        });
        if output_mismatch {
            return ReadyProductPassSettlementBatch {
                presentation_failure: Some(PresentationSettlementFailure::OutputMismatch),
                ..ReadyProductPassSettlementBatch::default()
            };
        }
        ReadyProductPassSettlementBatch {
            presentation_actions: self
                .presentation_evidence
                .into_iter()
                .map(|evidence| evidence.action)
                .collect(),
            pane_focus_observation: self
                .pane_focus_pending
                .map_or(self.pane_focus_observation, |observation| {
                    Some(observation.resolve(context, viewport, output))
                }),
            presentation_failure: None,
        }
    }
}

impl ReadyProductPassSettlementBatch {
    fn merge(&mut self, mut newer: Self) {
        self.presentation_actions
            .append(&mut newer.presentation_actions);
        if self.pane_focus_observation.is_none() {
            self.pane_focus_observation = newer.pane_focus_observation;
        }
        if self.presentation_failure.is_none() {
            self.presentation_failure = newer.presentation_failure;
        }
        if self.presentation_failure.is_some() {
            self.presentation_actions.clear();
            self.pane_focus_observation = None;
        }
    }
}

/// Exact private proof that one preview compound survived into final output.
pub(super) struct FinalOutputShapeProof {
    expected: egui::epaint::ClippedShape,
    marker: Arc<dyn Any + Send + Sync>,
}

impl FinalOutputShapeProof {
    pub(super) fn paint(
        scope: &PresentationProofScope,
        painter: &Painter,
        marker_rect: egui::Rect,
        mut shapes: Vec<Shape>,
    ) -> Option<Self> {
        if !painter.is_visible() {
            return None;
        }
        let marker: Arc<dyn Any + Send + Sync> = Arc::new(PresentationMarker {
            run: Arc::downgrade(&scope.run),
            pass: scope.pass,
        });
        shapes.push(Shape::Callback(PaintCallback {
            rect: marker_rect,
            callback: Arc::clone(&marker),
        }));
        painter.add(Shape::Vec(shapes));

        let mut expected = None;
        painter.for_each_shape(|shape| {
            if shape_contains_marker(&shape.shape, &marker) {
                expected = Some(shape.clone());
            }
        });
        let mut expected = expected?;
        if let Some(transform) = painter.ctx().layer_transform_to_global(painter.layer_id()) {
            expected.transform(transform);
        }
        Some(Self { expected, marker })
    }

    fn is_present_once(&self, output: &FullOutput) -> bool {
        output
            .shapes
            .iter()
            .filter(|shape| {
                *shape == &self.expected && shape_contains_marker(&shape.shape, &self.marker)
            })
            .count()
            == 1
    }
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
            (PaneFocusState::Unknown, _)
            | (_, None)
            | (PaneFocusState::Focused, Some(PaneFocusState::Unfocused))
            | (PaneFocusState::Unfocused, Some(PaneFocusState::Focused)) => self.unavailable,
            (_, Some(PaneFocusState::Unknown)) => self.unavailable,
        }
    }
}

/// Per-Dockspace adapter state enrolled into the context-owned run portal.
#[derive(Default)]
pub(super) struct ProductPassSettlement {
    state: Arc<Mutex<PassSettlementState>>,
}

impl ProductPassSettlement {
    #[cfg(test)]
    pub(super) fn begin_run(
        &self,
        context: &Context,
        viewport: ViewportId,
    ) -> ProductPassSettlementRun {
        ProductPassSettlementRun::begin(context, viewport)
    }

    #[cfg(test)]
    pub(super) fn is_active(&self, ui: &Ui) -> bool {
        let Some(portal) = active_run_portal(ui.ctx()) else {
            return false;
        };
        let portal = portal.lock().unwrap_or_else(PoisonError::into_inner);
        ui_run_scope(ui).is_some_and(|scope| {
            portal.viewport == ui.ctx().viewport_id()
                && portal.current_pass == Some(scope.pass)
                && Arc::ptr_eq(&portal.run, &scope.run)
                && !portal.current_pass_poisoned
        })
    }

    #[cfg(test)]
    pub(super) fn stage(&self, ui: &Ui, batch: ProductPassSettlementBatch) -> bool {
        match self.enroll(ui) {
            ProductPassEnrollment::Acquired(claim) => claim.stage(batch),
            ProductPassEnrollment::Unowned | ProductPassEnrollment::Conflict => false,
        }
    }

    pub(super) fn enroll(&self, ui: &Ui) -> ProductPassEnrollment {
        let Some(portal) = active_run_portal(ui.ctx()) else {
            return ProductPassEnrollment::Unowned;
        };
        let scope = ui_run_scope(ui);
        let context = context_identity(ui.ctx());
        let viewport = ui.ctx().viewport_id();
        let mut portal_state = portal.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(pass) = portal_state.current_pass else {
            portal_state.current_pass_poisoned = true;
            return ProductPassEnrollment::Conflict;
        };
        if portal_state.current_pass_poisoned
            || portal_state.viewport != viewport
            || scope.as_ref().is_none_or(|scope| {
                scope.pass != pass || !Arc::ptr_eq(&scope.run, &portal_state.run)
            })
        {
            portal_state.current_pass_poisoned = true;
            return ProductPassEnrollment::Conflict;
        }
        let run = Arc::clone(&portal_state.run);
        let claimed = self
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .claim(&context, viewport, &run, pass);
        if !claimed {
            portal_state.current_pass_poisoned = true;
            return ProductPassEnrollment::Conflict;
        }
        let participant = Arc::downgrade(&self.state);
        if !portal_state
            .participants
            .iter()
            .any(|current| Weak::ptr_eq(current, &participant))
        {
            portal_state.participants.push(participant);
        }
        ProductPassEnrollment::Acquired(ProductPassClaim {
            state: Arc::clone(&self.state),
            context,
            viewport,
            run,
            pass,
        })
    }

    pub(super) fn take_ready(&self, ui: &Ui) -> ReadyProductPassSettlementBatch {
        let context = context_identity(ui.ctx());
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take_ready(&context, ui.ctx().viewport_id())
    }
}

pub(super) enum ProductPassEnrollment {
    Acquired(ProductPassClaim),
    Unowned,
    Conflict,
}

pub(super) struct ProductPassClaim {
    state: Arc<Mutex<PassSettlementState>>,
    context: Arc<ContextIdentity>,
    viewport: ViewportId,
    run: Arc<RunIdentity>,
    pass: u64,
}

impl ProductPassClaim {
    pub(super) fn proof_scope(&self) -> PresentationProofScope {
        PresentationProofScope {
            run: Arc::clone(&self.run),
            pass: self.pass,
        }
    }

    pub(super) fn stage(self, batch: ProductPassSettlementBatch) -> bool {
        self.state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .stage(&self.context, self.viewport, &self.run, self.pass, batch)
    }
}

pub(super) struct ProductPassSettlementRun {
    context: Context,
    context_identity: Arc<ContextIdentity>,
    viewport: ViewportId,
    portal: Arc<Mutex<ActiveRunPortal>>,
    completed: bool,
}

impl ProductPassSettlementRun {
    pub(super) fn begin(context: &Context, viewport: ViewportId) -> Self {
        let context_identity = context_identity(context);
        if let Some(active) = active_run_portal(context) {
            active
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .current_pass_poisoned = true;
            panic!("nested egui_dockspace::run_ui calls on one Context are unsupported");
        }
        let portal = Arc::new(Mutex::new(ActiveRunPortal {
            run: Arc::new(RunIdentity),
            viewport,
            next_pass: 0,
            current_pass: None,
            current_pass_poisoned: false,
            participants: Vec::new(),
        }));
        context.data_mut(|data| {
            data.insert_temp(active_run_portal_id(), Arc::clone(&portal));
        });
        Self {
            context: context.clone(),
            context_identity,
            viewport,
            portal,
            completed: false,
        }
    }

    pub(super) fn begin_pass(&self, ui: &Ui) -> Option<PresentationProofScope> {
        let context = context_identity(ui.ctx());
        if ui.ctx().viewport_id() != self.viewport || !Arc::ptr_eq(&context, &self.context_identity)
        {
            return None;
        }
        let (previous, scope) = {
            let mut portal = self.portal.lock().unwrap_or_else(PoisonError::into_inner);
            let previous = portal.current_pass.map(|pass| {
                (
                    Arc::clone(&portal.run),
                    pass,
                    portal.current_pass_poisoned,
                    std::mem::take(&mut portal.participants),
                )
            });
            portal.next_pass = portal
                .next_pass
                .checked_add(1)
                .expect("egui pass identity space exhausted");
            portal.current_pass = Some(portal.next_pass);
            portal.current_pass_poisoned = false;
            let scope = PresentationProofScope {
                run: Arc::clone(&portal.run),
                pass: portal.next_pass,
            };
            (previous, scope)
        };
        if let Some((run, pass, poisoned, participants)) = previous {
            settle_participants(
                participants,
                &run,
                pass,
                if poisoned {
                    PassDisposition::Poisoned
                } else {
                    PassDisposition::Discarded
                },
                None,
            );
        }
        Some(scope)
    }

    pub(super) fn complete(mut self, output: &mut FullOutput) {
        let terminal = self.take_terminal_participants();
        self.remove_portal();
        if let Some((run, pass, poisoned, participants)) = terminal {
            if poisoned {
                settle_participants(participants, &run, pass, PassDisposition::Poisoned, None);
                scrub_private_markers(output, None);
            } else {
                settle_participants(
                    participants,
                    &run,
                    pass,
                    PassDisposition::Presented,
                    Some((&self.context, output)),
                );
                scrub_private_markers(output, Some((&run, pass)));
            }
        } else {
            scrub_private_markers(output, None);
        }
        self.completed = true;
    }

    fn take_terminal_participants(
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
                portal.current_pass_poisoned,
                std::mem::take(&mut portal.participants),
            )
        })
    }

    fn remove_portal(&self) {
        self.context.data_mut(|data| {
            let matches = data
                .get_temp::<Arc<Mutex<ActiveRunPortal>>>(active_run_portal_id())
                .is_some_and(|current| Arc::ptr_eq(&current, &self.portal));
            if matches {
                data.remove::<Arc<Mutex<ActiveRunPortal>>>(active_run_portal_id());
            }
        });
    }
}

impl Drop for ProductPassSettlementRun {
    fn drop(&mut self) {
        if self.completed {
            return;
        }
        let terminal = self.take_terminal_participants();
        self.remove_portal();
        if let Some((run, pass, _poisoned, participants)) = terminal {
            settle_participants(participants, &run, pass, PassDisposition::Unwind, None);
        }
    }
}

struct ActiveRunPortal {
    run: Arc<RunIdentity>,
    viewport: ViewportId,
    next_pass: u64,
    current_pass: Option<u64>,
    current_pass_poisoned: bool,
    participants: Vec<Weak<Mutex<PassSettlementState>>>,
}

fn active_run_portal(context: &Context) -> Option<Arc<Mutex<ActiveRunPortal>>> {
    context.data(|data| data.get_temp(active_run_portal_id()))
}

fn active_run_portal_id() -> Id {
    Id::unique(("egui_dockspace", "active_official_run_portal"))
}

fn ui_run_scope(ui: &Ui) -> Option<PresentationProofScope> {
    ui.stack().iter().find_map(|stack| {
        stack
            .tags()
            .get_downcast::<PresentationProofScope>(RUN_SCOPE_TAG)
            .cloned()
    })
}

fn settle_participants(
    participants: Vec<Weak<Mutex<PassSettlementState>>>,
    run: &Arc<RunIdentity>,
    pass: u64,
    disposition: PassDisposition,
    mut presented: Option<(&Context, &mut FullOutput)>,
) {
    for participant in participants {
        let Some(participant) = participant.upgrade() else {
            continue;
        };
        let mut state = participant.lock().unwrap_or_else(PoisonError::into_inner);
        match (&mut presented, disposition) {
            (Some((context, output)), PassDisposition::Presented) => {
                state.settle(run, pass, disposition, Some((context, output)));
            }
            _ => state.settle(run, pass, disposition, None),
        }
    }
}

struct PassSettlementState {
    bridge: RendererPassBridge<ProductPassSettlementBatch>,
    lane: Option<SettlementLane>,
    pending_failure: Option<PresentationSettlementFailure>,
}

impl PassSettlementState {
    fn claim(
        &mut self,
        context: &Arc<ContextIdentity>,
        viewport: ViewportId,
        run: &Arc<RunIdentity>,
        pass: u64,
    ) -> bool {
        if !self.lane_matches(context, viewport) {
            if self.lane.as_ref().is_some_and(|lane| lane.active.is_some()) {
                return false;
            }
            self.retire_idle_lane();
            self.lane = Some(SettlementLane {
                context: Arc::downgrade(context),
                viewport,
                active: None,
                ready: None,
            });
        }
        if self.lane.as_ref().is_some_and(|lane| lane.active.is_some()) {
            return false;
        }
        let Ok((binding, receipt_issuer)) = self.bridge.open_pass(viewport) else {
            return false;
        };
        self.lane
            .as_mut()
            .expect("claiming a pass installs its settlement lane")
            .active = Some(OpenProductPass {
            run: Arc::clone(run),
            pass,
            binding: Some(binding),
            receipt_issuer,
        });
        true
    }

    fn stage(
        &mut self,
        context: &Arc<ContextIdentity>,
        viewport: ViewportId,
        run: &Arc<RunIdentity>,
        pass: u64,
        batch: ProductPassSettlementBatch,
    ) -> bool {
        let binding = self
            .matching_lane_mut(context, viewport)
            .and_then(|lane| lane.active.as_mut())
            .filter(|active| active.pass == pass && Arc::ptr_eq(&active.run, run))
            .and_then(|active| active.binding.take());
        let Some(binding) = binding else {
            return false;
        };
        match self.bridge.stage(binding, batch) {
            Ok(()) => true,
            Err(error) => {
                let _reason = error.reason;
                let _artifact = error.artifact;
                false
            }
        }
    }

    fn settle(
        &mut self,
        run: &Arc<RunIdentity>,
        pass: u64,
        disposition: PassDisposition,
        presented: Option<(&Context, &mut FullOutput)>,
    ) {
        let viewport = self.lane.as_ref().map(|lane| lane.viewport);
        let open = self
            .lane
            .as_mut()
            .and_then(|lane| lane.active.take())
            .filter(|active| active.pass == pass && Arc::ptr_eq(&active.run, run));
        let Some(open) = open else {
            return;
        };
        let receipt = match disposition {
            PassDisposition::Presented => open.receipt_issuer.presented(),
            PassDisposition::Discarded => open.receipt_issuer.discarded(),
            PassDisposition::Unwind | PassDisposition::Poisoned => open.receipt_issuer.unwind(),
        };
        match self.bridge.settle(receipt) {
            Ok(RendererPassSettlement::Presented(batch)) => {
                if let (Some(viewport), Some((context, output))) = (viewport, presented)
                    && let Some(lane) = self.lane.as_mut()
                {
                    let ready = batch.into_ready(context, viewport, output);
                    if let Some(current) = &mut lane.ready {
                        current.merge(ready);
                    } else {
                        lane.ready = Some(ready);
                    }
                }
            }
            Ok(RendererPassSettlement::Discarded(_)) => {}
            Ok(RendererPassSettlement::Unwound(Some(_))) => {}
            Ok(RendererPassSettlement::Unwound(None)) => {}
            Err(error) => {
                let _receipt = error.receipt;
                let _reason = error.reason;
                self.record_ready_failure(PresentationSettlementFailure::Abandoned);
            }
        }
        if matches!(
            disposition,
            PassDisposition::Unwind | PassDisposition::Poisoned
        ) {
            self.record_ready_failure(PresentationSettlementFailure::Abandoned);
        }
    }

    fn take_ready(
        &mut self,
        context: &Arc<ContextIdentity>,
        viewport: ViewportId,
    ) -> ReadyProductPassSettlementBatch {
        if !self.lane_matches(context, viewport) {
            if self.lane.as_ref().is_some_and(|lane| lane.active.is_some()) {
                return ReadyProductPassSettlementBatch::default();
            }
            self.retire_idle_lane();
            return self.take_pending_failure();
        }
        let ready = self
            .matching_lane_mut(context, viewport)
            .and_then(|lane| lane.ready.take())
            .unwrap_or_default();
        self.merge_pending_failure(ready)
    }

    fn lane_matches(&self, context: &Arc<ContextIdentity>, viewport: ViewportId) -> bool {
        self.lane.as_ref().is_some_and(|lane| {
            lane.viewport == viewport
                && lane
                    .context
                    .upgrade()
                    .is_some_and(|current| Arc::ptr_eq(&current, context))
        })
    }

    fn matching_lane_mut(
        &mut self,
        context: &Arc<ContextIdentity>,
        viewport: ViewportId,
    ) -> Option<&mut SettlementLane> {
        self.lane.as_mut().filter(|lane| {
            lane.viewport == viewport
                && lane
                    .context
                    .upgrade()
                    .is_some_and(|current| Arc::ptr_eq(&current, context))
        })
    }

    fn record_ready_failure(&mut self, failure: PresentationSettlementFailure) {
        if let Some(lane) = self.lane.as_mut() {
            let ready = lane
                .ready
                .get_or_insert_with(ReadyProductPassSettlementBatch::default);
            ready.presentation_actions.clear();
            ready.pane_focus_observation = None;
            ready.presentation_failure = Some(failure);
        } else if self.pending_failure.is_none() {
            self.pending_failure = Some(failure);
        }
    }

    fn retire_idle_lane(&mut self) {
        let retired = self.lane.take().and_then(|lane| lane.ready);
        if let Some(retired) = retired
            && self.pending_failure.is_none()
        {
            self.pending_failure = Some(
                retired
                    .presentation_failure
                    .unwrap_or(PresentationSettlementFailure::Abandoned),
            );
        }
        self.bridge = RendererPassBridge::new();
    }

    fn take_pending_failure(&mut self) -> ReadyProductPassSettlementBatch {
        ReadyProductPassSettlementBatch {
            presentation_failure: self.pending_failure.take(),
            ..ReadyProductPassSettlementBatch::default()
        }
    }

    fn merge_pending_failure(
        &mut self,
        mut ready: ReadyProductPassSettlementBatch,
    ) -> ReadyProductPassSettlementBatch {
        if let Some(failure) = self.pending_failure.take() {
            ready.presentation_actions.clear();
            ready.pane_focus_observation = None;
            ready.presentation_failure = Some(failure);
        }
        ready
    }
}

impl Default for PassSettlementState {
    fn default() -> Self {
        Self {
            bridge: RendererPassBridge::new(),
            lane: None,
            pending_failure: None,
        }
    }
}

struct SettlementLane {
    context: Weak<ContextIdentity>,
    viewport: ViewportId,
    active: Option<OpenProductPass>,
    ready: Option<ReadyProductPassSettlementBatch>,
}

struct OpenProductPass {
    run: Arc<RunIdentity>,
    pass: u64,
    binding: Option<OpenPassBinding>,
    receipt_issuer: RendererPassReceiptIssuer,
}

#[derive(Clone, Copy)]
enum PassDisposition {
    Presented,
    Discarded,
    Unwind,
    Poisoned,
}

struct PresentationMarker {
    run: Weak<RunIdentity>,
    pass: u64,
}
struct RunIdentity;
struct ContextIdentity;

fn shape_contains_marker(shape: &Shape, marker: &Arc<dyn Any + Send + Sync>) -> bool {
    match shape {
        Shape::Vec(shapes) => shapes
            .iter()
            .any(|shape| shape_contains_marker(shape, marker)),
        Shape::Callback(callback) => Arc::ptr_eq(&callback.callback, marker),
        Shape::Noop
        | Shape::Circle(_)
        | Shape::Ellipse(_)
        | Shape::LineSegment { .. }
        | Shape::Path(_)
        | Shape::Band(_)
        | Shape::Rect(_)
        | Shape::Text(_)
        | Shape::Mesh(_)
        | Shape::QuadraticBezier(_)
        | Shape::CubicBezier(_) => false,
    }
}

fn scrub_private_markers(output: &mut FullOutput, current: Option<(&Arc<RunIdentity>, u64)>) {
    for clipped in &mut output.shapes {
        scrub_shape(&mut clipped.shape, current);
    }
}

fn scrub_shape(shape: &mut Shape, current: Option<(&Arc<RunIdentity>, u64)>) {
    match shape {
        Shape::Vec(shapes) => {
            if shapes
                .iter()
                .any(|shape| contains_stale_marker(shape, current))
            {
                *shape = Shape::Noop;
                return;
            }
            for shape in shapes.iter_mut() {
                scrub_shape(shape, current);
            }
            shapes.retain(|shape| !matches!(shape, Shape::Noop));
            if shapes.is_empty() {
                *shape = Shape::Noop;
            }
        }
        Shape::Callback(callback) if callback.callback.as_ref().is::<PresentationMarker>() => {
            *shape = Shape::Noop;
        }
        Shape::Noop
        | Shape::Circle(_)
        | Shape::Ellipse(_)
        | Shape::LineSegment { .. }
        | Shape::Path(_)
        | Shape::Band(_)
        | Shape::Rect(_)
        | Shape::Text(_)
        | Shape::Mesh(_)
        | Shape::QuadraticBezier(_)
        | Shape::CubicBezier(_)
        | Shape::Callback(_) => {}
    }
}

fn contains_stale_marker(shape: &Shape, current: Option<(&Arc<RunIdentity>, u64)>) -> bool {
    match shape {
        Shape::Vec(shapes) => shapes
            .iter()
            .any(|shape| contains_stale_marker(shape, current)),
        Shape::Callback(callback) => callback
            .callback
            .as_ref()
            .downcast_ref::<PresentationMarker>()
            .is_some_and(|marker| {
                let Some((current_run, current_pass)) = current else {
                    return true;
                };
                marker.pass != current_pass
                    || marker
                        .run
                        .upgrade()
                        .is_none_or(|run| !Arc::ptr_eq(&run, current_run))
            }),
        Shape::Noop
        | Shape::Circle(_)
        | Shape::Ellipse(_)
        | Shape::LineSegment { .. }
        | Shape::Path(_)
        | Shape::Band(_)
        | Shape::Rect(_)
        | Shape::Text(_)
        | Shape::Mesh(_)
        | Shape::QuadraticBezier(_)
        | Shape::CubicBezier(_) => false,
    }
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
mod tests {
    use std::panic::{AssertUnwindSafe, catch_unwind};

    use dockspace::geometry::{LogicalRect, LogicalSize};
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspacePaneFocusObservation, DockspaceSession, UniformSurfaceMetrics,
    };
    use egui::{Id, Pos2, RawInput, RepaintCause, UiBuilder, Vec2};

    use super::*;

    #[derive(Default)]
    struct LateOutputHook {
        discard_remaining: usize,
        panic_once: bool,
        transform_markers_once: bool,
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
            if self.transform_markers_once {
                self.transform_markers_once = false;
                for clipped in &mut output.shapes {
                    if contains_stale_marker(&clipped.shape, None) {
                        clipped.transform_after_tessellation =
                            egui::emath::TSTransform::from_translation(egui::vec2(8.0, 4.0));
                    }
                }
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

    fn focus_batch(target: Id) -> ProductPassSettlementBatch {
        let surface = SurfaceId::new(1);
        let first = ItemId::new(1);
        let second = ItemId::new(2);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([first, second])),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
            LogicalSize::new(16.0, 16.0).unwrap(),
            24.0,
        )
        .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame.measure_surface(surface, metrics).unwrap();
        frame.commit().unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        let selection = frame
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .prepare_tab_select(second)
            .unwrap();
        frame.submit_surface_action(selection).unwrap();
        frame.measure_surface(surface, metrics).unwrap();
        frame.commit().unwrap();
        let mut inspect = session.begin_host_frame().unwrap();
        let plan = inspect.paint_plan(surface).unwrap().unwrap();
        let request = plan.pane_focus_request().unwrap();
        let observation = |state| plan.prepare_pane_focus_observation(request, state).unwrap();
        ProductPassSettlementBatch {
            presentation_evidence: vec![PresentationEvidence {
                action: plan.prepare_tab_select(first).unwrap(),
                proof: None,
            }],
            pane_focus_observation: Some(observation(DockspacePaneFocusObservation::Focused)),
            pane_focus_pending: Some(PendingPaneFocusObservation::new(
                Some(target),
                PaneFocusState::Focused,
                observation(DockspacePaneFocusObservation::Focused),
                observation(DockspacePaneFocusObservation::NotFocused),
                observation(DockspacePaneFocusObservation::Unavailable),
            )),
        }
    }

    fn run_owned(
        context: &Context,
        settlement: &ProductPassSettlement,
        input: RawInput,
        mut run_ui: impl FnMut(&Ui),
    ) -> egui::FullOutput {
        let run = settlement.begin_run(context, input.viewport_id);
        let mut output = context.run_ui(input, |ui| {
            let scope = run
                .begin_pass(ui)
                .expect("the owned test run uses its exact context and viewport");
            ui.scope_builder(
                UiBuilder::new()
                    .id_salt(RUN_SCOPE_TAG)
                    .ui_stack_info(UiStackInfo::default().with_tag_value(RUN_SCOPE_TAG, scope)),
                |ui| run_ui(ui),
            );
        });
        run.complete(&mut output);
        output
    }

    fn lane_counts(settlement: &ProductPassSettlement) -> (usize, usize, usize) {
        let state = settlement
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let staged = state.bridge.staged_len();
        state.lane.as_ref().map_or((0, 0, 0), |lane| {
            (
                usize::from(lane.active.is_some()),
                staged,
                usize::from(lane.ready.is_some()),
            )
        })
    }

    #[test]
    fn successful_root_run_promotes_terminal_evidence() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.is_active(ui));
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 1));

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            let _ = settlement.take_ready(ui);
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 0));
    }

    #[test]
    fn output_mismatch_drops_both_immediate_and_pending_focus_observations() {
        let context = Context::default();
        let target = Id::unique("mismatched-output-focus");
        for pending in [false, true] {
            let mut batch = focus_batch(target);
            if !pending {
                batch.pane_focus_pending = None;
            }
            let ready = batch.into_ready(&context, ViewportId::ROOT, &FullOutput::default());
            assert_eq!(
                ready.presentation_failure,
                Some(PresentationSettlementFailure::OutputMismatch)
            );
            assert!(ready.presentation_actions.is_empty());
            assert!(ready.pane_focus_observation.is_none());
        }
    }

    #[test]
    fn late_transform_mismatch_drops_focus_when_the_ready_lane_was_empty() {
        let context = Context::default();
        context
            .plugin_or_default::<LateOutputHook>()
            .lock()
            .transform_markers_once = true;
        let settlement = ProductPassSettlement::default();
        let target = Id::unique("late-transform-output-focus");
        let mut batch = Some(focus_batch(target));
        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            let ProductPassEnrollment::Acquired(claim) = settlement.enroll(ui) else {
                panic!("the owned pass acquires its exact claim")
            };
            let mut batch = batch.take().unwrap();
            let rect = egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0));
            ui.interact(rect, target, egui::Sense::click())
                .request_focus();
            batch.presentation_evidence[0].proof = FinalOutputShapeProof::paint(
                &claim.proof_scope(),
                ui.painter(),
                rect,
                vec![Shape::rect_filled(rect, 0.0, egui::Color32::RED)],
            );
            assert!(batch.presentation_evidence[0].proof.is_some());
            assert!(claim.stage(batch));
        });
        assert!(context.memory(|memory| memory.has_focus(target)));
        assert!(output.shapes.iter().any(|clipped| {
            clipped.transform_after_tessellation != egui::emath::TSTransform::IDENTITY
        }));
        clear_textures(&mut output);
        let ready = settlement
            .state
            .lock()
            .unwrap()
            .lane
            .as_mut()
            .unwrap()
            .ready
            .take()
            .unwrap();
        assert_eq!(
            ready.presentation_failure,
            Some(PresentationSettlementFailure::OutputMismatch)
        );
        assert!(ready.presentation_actions.is_empty());
        assert!(ready.pane_focus_observation.is_none());
    }

    #[test]
    fn abandonment_clears_previously_ready_focus_without_a_new_batch() {
        let context = Context::default();
        let target = Id::unique("abandoned-ready-focus");
        let mut batch = focus_batch(target);
        batch.presentation_evidence.clear();
        batch.pane_focus_pending = None;
        let mut state = PassSettlementState::default();
        state.lane = Some(SettlementLane {
            context: Arc::downgrade(&context_identity(&context)),
            viewport: ViewportId::ROOT,
            active: None,
            ready: Some(batch.into_ready(&context, ViewportId::ROOT, &FullOutput::default())),
        });
        assert!(
            state
                .lane
                .as_ref()
                .unwrap()
                .ready
                .as_ref()
                .unwrap()
                .pane_focus_observation
                .is_some()
        );
        let run = Arc::new(RunIdentity);
        assert!(state.claim(&context_identity(&context), ViewportId::ROOT, &run, 1));
        state.settle(&run, 1, PassDisposition::Poisoned, None);
        let ready = state.lane.as_ref().unwrap().ready.as_ref().unwrap();
        assert_eq!(
            ready.presentation_failure,
            Some(PresentationSettlementFailure::Abandoned)
        );
        assert!(ready.presentation_actions.is_empty());
        assert!(ready.pane_focus_observation.is_none());
    }

    #[test]
    fn non_root_run_promotes_only_to_the_exact_viewport() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let child = ViewportId::from_hash_of("dockspace-owned-child-run");
        let mut output = run_owned(&context, &settlement, input_for(child), |ui| {
            assert_eq!(ui.ctx().viewport_id(), child);
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 1));

        let mut consumed = false;
        let mut output = context.run_ui(input_for(child), |ui| {
            let ready = settlement.take_ready(ui);
            consumed = ready.presentation_failure.is_none();
        });
        clear_textures(&mut output);
        assert!(consumed);
        assert_eq!(lane_counts(&settlement), (0, 0, 0));
    }

    #[test]
    fn foreign_viewport_retires_ready_as_abandoned() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let child = ViewportId::from_hash_of("dockspace-owned-child-retirement");
        let mut output = run_owned(&context, &settlement, input_for(child), |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 1));

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            let ready = settlement.take_ready(ui);
            assert!(ready.presentation_actions.is_empty());
            assert!(ready.pane_focus_observation.is_none());
            assert_eq!(
                ready.presentation_failure,
                Some(PresentationSettlementFailure::Abandoned),
            );
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 0));
    }

    #[test]
    fn late_discard_promotes_only_terminal_pass_evidence() {
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

        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            observed.push(ui.ctx().current_pass_index());
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        assert_eq!(observed, [0, 1]);
        assert_eq!(lane_counts(&settlement), (0, 0, 1));
    }

    #[test]
    fn omitted_terminal_pass_rejects_discarded_evidence() {
        let context = Context::default();
        context.options_mut(|options| {
            options.max_passes = 3.try_into().expect("three is non-zero");
        });
        let settlement = ProductPassSettlement::default();

        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            if ui.ctx().current_pass_index() == 0 {
                assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
                ui.ctx()
                    .request_discard("omit Dockspace from the terminal pass");
            }
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 0));
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
            let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
                assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
            });
            clear_textures(&mut output);
        }));
        assert!(interrupted.is_err());
        assert_eq!(lane_counts(&settlement), (0, 0, 1));

        let mut saw_abandonment = false;
        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            let ready = settlement.take_ready(ui);
            saw_abandonment =
                ready.presentation_failure == Some(PresentationSettlementFailure::Abandoned);
            assert!(ready.presentation_actions.is_empty());
            assert!(ready.pane_focus_observation.is_none());
        });
        clear_textures(&mut output);
        assert!(saw_abandonment);
        assert_eq!(lane_counts(&settlement), (0, 0, 0));
    }

    #[test]
    fn terminal_claim_without_a_staged_batch_reports_abandonment() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();

        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            let ProductPassEnrollment::Acquired(_claim) = settlement.enroll(ui) else {
                panic!("the exact wrapper UI acquires a settlement claim");
            };
        });
        clear_textures(&mut output);

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).presentation_failure,
                Some(PresentationSettlementFailure::Abandoned),
            );
        });
        clear_textures(&mut output);
    }

    #[test]
    fn duplicate_enrollment_poisons_the_complete_pass() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
            assert!(!settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);

        let mut saw_poison = false;
        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            let ready = settlement.take_ready(ui);
            saw_poison =
                ready.presentation_failure == Some(PresentationSettlementFailure::Abandoned);
            assert!(ready.presentation_actions.is_empty());
        });
        clear_textures(&mut output);
        assert!(saw_poison);
    }

    #[test]
    fn caught_nested_wrapper_panic_poisons_outer_evidence_and_then_recovers() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();
        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
            let nested = catch_unwind(AssertUnwindSafe(|| {
                let mut nested_output =
                    super::run_ui(&context, input_for(ViewportId::ROOT), |_| {});
                clear_textures(&mut nested_output);
            }));
            assert!(nested.is_err());
        });
        clear_textures(&mut output);

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).presentation_failure,
                Some(PresentationSettlementFailure::Abandoned),
            );
        });
        clear_textures(&mut output);

        let mut output = run_owned(&context, &settlement, input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()))
        });
        clear_textures(&mut output);
        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.take_ready(ui).presentation_failure.is_none());
        });
        clear_textures(&mut output);
    }

    #[test]
    fn direct_or_manual_passes_cannot_stage_evidence() {
        let context = Context::default();
        let settlement = ProductPassSettlement::default();

        let mut output = context.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert!(!settlement.is_active(ui));
            assert!(!settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);

        context.begin_pass(input_for(ViewportId::ROOT));
        let ui = Ui::new(
            context.clone(),
            Id::unique("dockspace-manual-pass"),
            UiBuilder::new().max_rect(context.viewport_rect()),
        );
        assert!(!settlement.is_active(&ui));
        assert!(!settlement.stage(&ui, ProductPassSettlementBatch::default()));
        let mut output = context.end_pass();
        clear_textures(&mut output);
    }

    #[test]
    fn context_churn_abandons_old_evidence_without_retaining_the_context() {
        let settlement = ProductPassSettlement::default();
        let old = Context::default();
        let old_identity = context_identity(&old);
        let old_weak = Arc::downgrade(&old_identity);
        let mut output = run_owned(&old, &settlement, input_for(ViewportId::ROOT), |ui| {
            assert!(settlement.stage(ui, ProductPassSettlementBatch::default()));
        });
        clear_textures(&mut output);
        drop(old_identity);
        drop(old);
        assert!(old_weak.upgrade().is_none());

        let current = Context::default();
        let mut output = current.run_ui(input_for(ViewportId::ROOT), |ui| {
            assert_eq!(
                settlement.take_ready(ui).presentation_failure,
                Some(PresentationSettlementFailure::Abandoned),
            );
        });
        clear_textures(&mut output);
        assert_eq!(lane_counts(&settlement), (0, 0, 0));
    }

    #[test]
    fn current_marker_is_removed_while_visuals_and_unrelated_callbacks_survive() {
        let run = Arc::new(RunIdentity);
        let marker: Arc<dyn Any + Send + Sync> = Arc::new(PresentationMarker {
            run: Arc::downgrade(&run),
            pass: 7,
        });
        let unrelated: Arc<dyn Any + Send + Sync> = Arc::new("application callback");
        let visual = Shape::rect_filled(
            egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0)),
            0.0,
            egui::Color32::RED,
        );
        let mut output = FullOutput {
            shapes: vec![
                egui::epaint::ClippedShape::new(
                    egui::Rect::EVERYTHING,
                    Shape::Vec(vec![
                        visual.clone(),
                        Shape::Callback(PaintCallback {
                            rect: egui::Rect::EVERYTHING,
                            callback: marker,
                        }),
                    ]),
                ),
                egui::epaint::ClippedShape::new(
                    egui::Rect::EVERYTHING,
                    Shape::Callback(PaintCallback {
                        rect: egui::Rect::EVERYTHING,
                        callback: Arc::clone(&unrelated),
                    }),
                ),
            ],
            ..FullOutput::default()
        };

        let transform = egui::emath::TSTransform::from_translation(egui::vec2(8.0, 4.0));
        let clip = egui::Rect::from_min_size(egui::pos2(2.0, 3.0), Vec2::splat(64.0));
        for clipped in &mut output.shapes {
            clipped.clip_rect = clip;
            clipped.transform_after_tessellation = transform;
        }
        scrub_private_markers(&mut output, Some((&run, 7)));

        for clipped in &output.shapes {
            assert_eq!(clipped.clip_rect, clip);
            assert_eq!(clipped.transform_after_tessellation, transform);
        }
        assert_eq!(output.shapes[0].shape, Shape::Vec(vec![visual]));
        assert!(matches!(
            &output.shapes[1].shape,
            Shape::Callback(callback) if Arc::ptr_eq(&callback.callback, &unrelated)
        ));
    }

    #[test]
    fn stale_marker_removes_its_whole_compound_without_touching_unrelated_callbacks() {
        let current_run = Arc::new(RunIdentity);
        let stale_run = Arc::new(RunIdentity);
        let stale_marker: Arc<dyn Any + Send + Sync> = Arc::new(PresentationMarker {
            run: Arc::downgrade(&stale_run),
            pass: 3,
        });
        let unrelated: Arc<dyn Any + Send + Sync> = Arc::new("application callback");
        let mut output = FullOutput {
            shapes: vec![
                egui::epaint::ClippedShape::new(
                    egui::Rect::EVERYTHING,
                    Shape::Vec(vec![
                        Shape::rect_filled(
                            egui::Rect::from_min_size(Pos2::ZERO, Vec2::splat(16.0)),
                            0.0,
                            egui::Color32::RED,
                        ),
                        Shape::Callback(PaintCallback {
                            rect: egui::Rect::EVERYTHING,
                            callback: stale_marker,
                        }),
                    ]),
                ),
                egui::epaint::ClippedShape::new(
                    egui::Rect::EVERYTHING,
                    Shape::Callback(PaintCallback {
                        rect: egui::Rect::EVERYTHING,
                        callback: Arc::clone(&unrelated),
                    }),
                ),
            ],
            ..FullOutput::default()
        };

        scrub_private_markers(&mut output, Some((&current_run, 3)));

        assert!(matches!(output.shapes[0].shape, Shape::Noop));
        assert!(matches!(
            &output.shapes[1].shape,
            Shape::Callback(callback) if Arc::ptr_eq(&callback.callback, &unrelated)
        ));
    }
}
