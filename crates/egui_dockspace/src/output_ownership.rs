//! Ordered ownership of egui renderer outputs and texture namespaces.

use std::sync::{Arc, Mutex};

use dockspace::ids::SurfaceId;
use egui::{Context, FullOutput, TexturesDelta};

use crate::error::DockspaceErrorSource;

/// Texture commands captured in exact host-frame production order.
///
/// Commands are grouped only by egui [`Context`]. Appending a later delta to a
/// namespace preserves its command order while keeping independent texture
/// namespaces isolated.
#[derive(Default)]
pub(crate) struct FrameTextureCaptures {
    namespaces: Vec<DeferredTextureNamespace>,
}

impl FrameTextureCaptures {
    pub(crate) fn capture_output(&mut self, context: &Context, output: &mut FullOutput) {
        let delta = std::mem::take(&mut output.textures_delta);
        append_namespace_delta(&mut self.namespaces, context.clone(), delta);
    }

    fn into_namespaces(mut self) -> Vec<DeferredTextureNamespace> {
        std::mem::take(&mut self.namespaces)
    }
}

impl Drop for FrameTextureCaptures {
    fn drop(&mut self) {
        clear_namespaces(&mut self.namespaces);
    }
}

/// One final surface output retained with its texture namespace and pass identity.
pub(crate) struct ConfirmedSurfaceOutput {
    surface: SurfaceId,
    context: Context,
    completed_pass: u64,
    output: FullOutput,
}

impl ConfirmedSurfaceOutput {
    pub(crate) fn new(
        surface: SurfaceId,
        context: Context,
        completed_pass: u64,
        output: FullOutput,
    ) -> Self {
        Self {
            surface,
            context,
            completed_pass,
            output,
        }
    }

    pub(crate) const fn completed_pass(&self) -> u64 {
        self.completed_pass
    }

    pub(crate) const fn output_mut(&mut self) -> &mut FullOutput {
        &mut self.output
    }

    pub(crate) fn into_parts(self) -> (SurfaceId, Context, FullOutput) {
        (self.surface, self.context, self.output)
    }
}

/// Final surface outputs in their real confirmation order.
#[derive(Default)]
pub(crate) struct ConfirmedSurfaceOutputs {
    outputs: Vec<ConfirmedSurfaceOutput>,
}

impl ConfirmedSurfaceOutputs {
    pub(crate) fn contains(&self, surface: SurfaceId) -> bool {
        self.outputs.iter().any(|output| output.surface == surface)
    }

    pub(crate) fn completed_pass(&self, surface: SurfaceId) -> Option<u64> {
        self.outputs
            .iter()
            .find(|output| output.surface == surface)
            .map(ConfirmedSurfaceOutput::completed_pass)
    }

    pub(crate) fn retain(
        &mut self,
        surface: SurfaceId,
        context: Context,
        completed_pass: u64,
        mut output: FullOutput,
    ) {
        debug_assert!(
            output.textures_delta.is_empty(),
            "host-frame texture commands must be captured before output retention"
        );
        if let Some(index) = self
            .outputs
            .iter()
            .position(|confirmed| confirmed.surface == surface)
        {
            let previous = self.outputs.remove(index);
            assert_eq!(
                previous.context, context,
                "one surface cannot switch egui texture namespaces within a host frame"
            );
            let (_, _, mut previous_output) = previous.into_parts();
            previous_output.append(output);
            output = previous_output;
        }
        self.outputs.push(ConfirmedSurfaceOutput::new(
            surface,
            context,
            completed_pass,
            output,
        ));
    }

    pub(crate) fn take(&mut self) -> Vec<ConfirmedSurfaceOutput> {
        std::mem::take(&mut self.outputs)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }

    pub(crate) fn contexts(&self) -> Vec<Context> {
        let mut contexts = Vec::new();
        for output in &self.outputs {
            if !contexts.iter().any(|context| *context == output.context) {
                contexts.push(output.context.clone());
            }
        }
        contexts
    }

    pub(crate) fn as_mut_slice(&mut self) -> &mut [ConfirmedSurfaceOutput] {
        &mut self.outputs
    }

    pub(crate) fn as_slice(&self) -> &[ConfirmedSurfaceOutput] {
        &self.outputs
    }
}

/// Dockspace-local texture namespace ledger.
///
/// Only one renderer batch per [`Context`] may remain unsettled at a time. This
/// deliberately favors a small, exact interface over speculative same-namespace
/// pipelining. Independent texture namespaces may still advance independently.
#[derive(Clone, Default)]
pub(crate) struct OutputTextureLedger(Arc<Mutex<OutputTextureLedgerState>>);

#[derive(Default)]
struct OutputTextureLedgerState {
    next_batch: u64,
    enrolled: Vec<Context>,
    outstanding: Vec<OutstandingTextureBatch>,
    deferred: Vec<DeferredTextureNamespace>,
    poisoned: Vec<Context>,
}

impl Drop for OutputTextureLedgerState {
    fn drop(&mut self) {
        clear_namespaces(&mut self.deferred);
    }
}

struct OutstandingTextureBatch {
    batch: u64,
    contexts: Vec<Context>,
}

struct DeferredTextureNamespace {
    context: Context,
    delta: TexturesDelta,
}

/// Early reservation of one egui texture namespace before input is consumed.
#[must_use = "an output namespace enrollment must enter a batch or be dropped"]
pub(crate) struct OutputNamespaceEnrollment {
    ledger: OutputTextureLedger,
    context: Context,
    active: bool,
}

impl OutputNamespaceEnrollment {
    pub(crate) fn context(&self) -> &Context {
        &self.context
    }
}

impl Drop for OutputNamespaceEnrollment {
    fn drop(&mut self) {
        if self.active {
            self.ledger.release_enrollment(&self.context);
            self.active = false;
        }
    }
}

impl OutputTextureLedger {
    pub(crate) fn enroll(
        &self,
        context: &Context,
    ) -> Result<OutputNamespaceEnrollment, DockspaceErrorSource> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.poisoned.iter().any(|poisoned| poisoned == context) {
            return Err(DockspaceErrorSource::RendererTextureNamespacePoisoned);
        }
        let enrolled = state.enrolled.iter().any(|active| active == context);
        let outstanding = state
            .outstanding
            .iter()
            .any(|batch| batch.contexts.iter().any(|active| active == context));
        if enrolled || outstanding {
            return Err(DockspaceErrorSource::RendererOutputBatchOutstanding);
        }
        state.enrolled.push(context.clone());
        Ok(OutputNamespaceEnrollment {
            ledger: self.clone(),
            context: context.clone(),
            active: true,
        })
    }

    pub(crate) fn defer_captures(&self, captures: FrameTextureCaptures) {
        self.defer_namespaces(captures.into_namespaces());
    }

    fn defer_namespaces(&self, namespaces: Vec<DeferredTextureNamespace>) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for namespace in namespaces {
            append_namespace_delta(&mut state.deferred, namespace.context, namespace.delta);
        }
    }

    pub(crate) fn reserve(
        &self,
        contexts: Vec<Context>,
        mut enrollments: Vec<OutputNamespaceEnrollment>,
        captures: FrameTextureCaptures,
    ) -> Result<OutputBatchReservation, DockspaceErrorSource> {
        debug_assert!(!contexts.is_empty());
        let output_contexts_enrolled = contexts.iter().all(|context| {
            enrollments
                .iter()
                .any(|enrollment| enrollment.context() == context)
        }) && enrollments
            .iter()
            .filter(|enrollment| {
                contexts
                    .iter()
                    .any(|context| context == enrollment.context())
            })
            .all(|enrollment| Arc::ptr_eq(&self.0, &enrollment.ledger.0));
        if !output_contexts_enrolled {
            self.defer_captures(captures);
            return Err(DockspaceErrorSource::RendererOutputContextNotEnrolled);
        }
        let mut matching_captures = Vec::new();
        let mut unused_captures = Vec::new();
        for namespace in captures.into_namespaces() {
            if contexts.iter().any(|context| *context == namespace.context) {
                matching_captures.push(namespace);
            } else {
                unused_captures.push(namespace);
            }
        }
        if !unused_captures.is_empty() {
            self.defer_namespaces(unused_captures);
        }
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if contexts
            .iter()
            .any(|context| state.poisoned.iter().any(|poisoned| poisoned == context))
        {
            drop(state);
            clear_namespaces(&mut matching_captures);
            return Err(DockspaceErrorSource::RendererTextureNamespacePoisoned);
        }
        if contexts
            .iter()
            .any(|context| !state.enrolled.iter().any(|enrolled| enrolled == context))
        {
            drop(state);
            self.defer_namespaces(matching_captures);
            return Err(DockspaceErrorSource::RendererOutputContextNotEnrolled);
        }
        let batch = state
            .next_batch
            .checked_add(1)
            .ok_or(DockspaceErrorSource::RendererOutputBatchIdentityExhausted);
        let batch = match batch {
            Ok(batch) => batch,
            Err(error) => {
                drop(state);
                self.defer_namespaces(matching_captures);
                return Err(error);
            }
        };
        state.next_batch = batch;
        state
            .enrolled
            .retain(|enrolled| !contexts.iter().any(|context| context == enrolled));
        state.outstanding.push(OutstandingTextureBatch {
            batch,
            contexts: contexts.clone(),
        });
        for enrollment in &mut enrollments {
            if contexts
                .iter()
                .any(|context| context == enrollment.context())
            {
                enrollment.active = false;
            }
        }
        Ok(OutputBatchReservation {
            ledger: self.clone(),
            batch,
            contexts,
            pending: matching_captures,
            active: true,
            attached: false,
        })
    }

    fn release_enrollment(&self, context: &Context) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.enrolled.retain(|enrolled| enrolled != context);
    }

    fn is_outstanding(&self, batch: u64, contexts: &[Context]) -> bool {
        let state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.outstanding.iter().any(|outstanding| {
            outstanding.batch == batch
                && outstanding.contexts.len() == contexts.len()
                && contexts.iter().all(|context| {
                    outstanding
                        .contexts
                        .iter()
                        .any(|outstanding| outstanding == context)
                })
        })
    }

    fn take_deferred_for(&self, batch: u64) -> Option<Vec<DeferredTextureNamespace>> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let contexts = state
            .outstanding
            .iter()
            .find(|outstanding| outstanding.batch == batch)
            .map(|outstanding| outstanding.contexts.clone())?;
        let mut matching = Vec::new();
        let mut retained = Vec::new();
        for deferred in std::mem::take(&mut state.deferred) {
            if contexts.iter().any(|context| *context == deferred.context) {
                matching.push(deferred);
            } else {
                retained.push(deferred);
            }
        }
        state.deferred = retained;
        Some(matching)
    }

    fn finish_partitioned(
        &self,
        batch: u64,
        mut earlier: Vec<DeferredTextureNamespace>,
        poisoned: &[Context],
    ) -> bool {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(index) = state
            .outstanding
            .iter()
            .position(|outstanding| outstanding.batch == batch)
        else {
            clear_namespaces(&mut earlier);
            return false;
        };
        state.outstanding.remove(index);
        earlier.retain_mut(|namespace| {
            let keep = !poisoned.iter().any(|context| *context == namespace.context);
            if !keep {
                namespace.delta.clear();
            }
            keep
        });
        for mut deferred in std::mem::take(&mut state.deferred) {
            if poisoned.iter().any(|context| *context == deferred.context) {
                deferred.delta.clear();
            } else {
                append_namespace_delta(&mut earlier, deferred.context, deferred.delta);
            }
        }
        state.deferred = earlier;
        for context in poisoned {
            if !state.poisoned.iter().any(|existing| existing == context) {
                state.poisoned.push(context.clone());
            }
        }
        true
    }

    fn finish(&self, batch: u64, earlier: Vec<DeferredTextureNamespace>) -> bool {
        self.finish_partitioned(batch, earlier, &[])
    }
}

/// Affine reservation for one renderer output batch.
#[must_use = "an output batch reservation must be finished or dropped"]
pub(crate) struct OutputBatchReservation {
    ledger: OutputTextureLedger,
    batch: u64,
    contexts: Vec<Context>,
    pending: Vec<DeferredTextureNamespace>,
    active: bool,
    attached: bool,
}

impl OutputBatchReservation {
    pub(crate) fn preflight(
        &self,
        outputs: &[ConfirmedSurfaceOutput],
    ) -> Result<(), DockspaceErrorSource> {
        if !self.active || self.attached {
            return Err(DockspaceErrorSource::RendererOutputReservationUnavailable);
        }
        if !self.ledger.is_outstanding(self.batch, &self.contexts) {
            return Err(DockspaceErrorSource::RendererOutputReservationUnavailable);
        }
        let exact_context_coverage = self
            .contexts
            .iter()
            .all(|context| outputs.iter().any(|output| output.context == *context))
            && outputs.iter().all(|output| {
                self.contexts
                    .iter()
                    .any(|context| *context == output.context)
            });
        if !exact_context_coverage {
            return Err(DockspaceErrorSource::RendererOutputTextureNamespaceMissing);
        }
        Ok(())
    }

    /// Attaches the reserved texture interval after the core commit succeeds.
    ///
    /// `preflight` and the affine reservation make this step infallible unless
    /// the private ledger invariants are violated.
    pub(crate) fn attach(&mut self, outputs: &mut [ConfirmedSurfaceOutput]) {
        self.preflight(outputs)
            .expect("a preflighted output reservation remains attachable");
        let mut plans = self
            .ledger
            .take_deferred_for(self.batch)
            .expect("an active output reservation remains registered");
        for namespace in std::mem::take(&mut self.pending) {
            append_namespace_delta(&mut plans, namespace.context, namespace.delta);
        }
        normalize_texture_deltas(outputs, plans);
        self.attached = true;
    }

    pub(crate) fn finish(mut self, dropped: Vec<(Context, TexturesDelta)>) {
        self.active = false;
        let replay = self.take_replay_interval(dropped);
        let _ = self.ledger.finish(self.batch, replay);
    }

    pub(crate) fn finish_after_panic(
        mut self,
        dropped: Vec<(Context, TexturesDelta)>,
        poisoned: &Context,
    ) {
        self.active = false;
        let replay = self.take_replay_interval(dropped);
        let _ = self
            .ledger
            .finish_partitioned(self.batch, replay, std::slice::from_ref(poisoned));
    }

    fn take_replay_interval(
        &mut self,
        dropped: Vec<(Context, TexturesDelta)>,
    ) -> Vec<DeferredTextureNamespace> {
        let mut replay = if self.attached {
            Vec::new()
        } else {
            self.ledger
                .take_deferred_for(self.batch)
                .unwrap_or_default()
        };
        for namespace in std::mem::take(&mut self.pending) {
            append_namespace_delta(&mut replay, namespace.context, namespace.delta);
        }
        for (context, delta) in dropped {
            append_namespace_delta(&mut replay, context, delta);
        }
        replay
    }
}

impl Drop for OutputBatchReservation {
    fn drop(&mut self) {
        if self.active {
            self.active = false;
            let replay = self.take_replay_interval(Vec::new());
            let _ = self.ledger.finish(self.batch, replay);
        }
    }
}

fn append_namespace_delta(
    deferred: &mut Vec<DeferredTextureNamespace>,
    context: Context,
    delta: TexturesDelta,
) {
    if delta.is_empty() {
        return;
    }
    if let Some(namespace) = deferred
        .iter_mut()
        .find(|namespace| namespace.context == context)
    {
        append_texture_interval(&mut namespace.delta, delta);
    } else {
        deferred.push(DeferredTextureNamespace { context, delta });
    }
}

fn clear_namespaces(namespaces: &mut Vec<DeferredTextureNamespace>) {
    for namespace in namespaces.iter_mut() {
        namespace.delta.clear();
    }
    namespaces.clear();
}

fn append_texture_interval(earlier: &mut TexturesDelta, mut later: TexturesDelta) {
    // A renderer batch has one set phase before all surface paints and one
    // free phase after them. Preserve both sides of that interval even when a
    // later surface frees a texture set by an earlier surface; egui's own
    // `TexturesDelta::append` is intentionally frame-local and would remove
    // that set before the earlier surface had a chance to paint.
    for (texture, deltas) in std::mem::take(&mut later.set) {
        for image in deltas {
            earlier.push(texture, image);
        }
    }
    earlier.free.extend(std::mem::take(&mut later.free));
}

fn normalize_texture_deltas(
    outputs: &mut [ConfirmedSurfaceOutput],
    plans: Vec<DeferredTextureNamespace>,
) {
    for mut plan in plans {
        let first = outputs
            .iter()
            .position(|output| output.context == plan.context)
            .expect("texture namespace coverage was preflighted");
        let last = outputs
            .iter()
            .rposition(|output| output.context == plan.context)
            .expect("texture namespace coverage was preflighted");
        let free = std::mem::take(&mut plan.delta.free);
        outputs[first]
            .output_mut()
            .textures_delta
            .set
            .extend(std::mem::take(&mut plan.delta.set));
        outputs[last].output_mut().textures_delta.free.extend(free);
    }
}

#[cfg(test)]
mod tests {
    use egui::epaint::ImageDelta;
    use egui::{Color32, ColorImage, TextureId, TextureOptions};

    use super::*;

    fn output_with_free(texture: TextureId) -> FullOutput {
        let mut output = FullOutput::default();
        output.textures_delta.free.insert(texture);
        output
    }

    fn reserve(
        ledger: &OutputTextureLedger,
        contexts: &[Context],
        captures: FrameTextureCaptures,
    ) -> OutputBatchReservation {
        let enrollments = contexts
            .iter()
            .map(|context| ledger.enroll(context).expect("namespace enrolls"))
            .collect();
        ledger
            .reserve(contexts.to_vec(), enrollments, captures)
            .expect("output batch reserves")
    }

    #[test]
    fn deferred_textures_stay_in_their_context_namespace() {
        let ledger = OutputTextureLedger::default();
        let context_a = Context::default();
        let context_b = Context::default();
        let texture = TextureId::Managed(7);
        let mut captures = FrameTextureCaptures::default();
        let mut output = output_with_free(texture);
        captures.capture_output(&context_a, &mut output);
        output.drop_without_applying_deltas();
        ledger.defer_captures(captures);

        let mut reservation = reserve(
            &ledger,
            std::slice::from_ref(&context_b),
            FrameTextureCaptures::default(),
        );
        let mut context_b_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(1),
            context_b,
            1,
            FullOutput::default(),
        )];
        reservation
            .preflight(&context_b_outputs)
            .expect("independent namespace preflights");
        reservation.attach(&mut context_b_outputs);
        assert!(context_b_outputs[0].output.textures_delta.is_empty());
        reservation.finish(Vec::new());

        let mut reservation = reserve(
            &ledger,
            std::slice::from_ref(&context_a),
            FrameTextureCaptures::default(),
        );
        let mut context_a_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(2),
            context_a,
            1,
            FullOutput::default(),
        )];
        reservation
            .preflight(&context_a_outputs)
            .expect("deferred namespace preflights");
        reservation.attach(&mut context_a_outputs);
        assert!(
            context_a_outputs[0]
                .output
                .textures_delta
                .free
                .contains(&texture)
        );
        context_a_outputs[0].output.textures_delta.clear();
        reservation.finish(Vec::new());
    }

    #[test]
    fn independent_contexts_can_advance_while_one_namespace_is_unsettled() {
        let ledger = OutputTextureLedger::default();
        let context_a = Context::default();
        let context_b = Context::default();
        let reservation_a = reserve(
            &ledger,
            std::slice::from_ref(&context_a),
            FrameTextureCaptures::default(),
        );
        let reservation_b = reserve(
            &ledger,
            std::slice::from_ref(&context_b),
            FrameTextureCaptures::default(),
        );
        let conflict = ledger.enroll(&context_a);
        assert!(matches!(
            conflict,
            Err(DockspaceErrorSource::RendererOutputBatchOutstanding)
        ));
        reservation_b.finish(Vec::new());
        reservation_a.finish(Vec::new());
    }

    #[test]
    fn confirmed_outputs_follow_latest_confirmation_order() {
        let context = Context::default();
        let mut outputs = ConfirmedSurfaceOutputs::default();
        outputs.retain(SurfaceId::new(2), context.clone(), 1, FullOutput::default());
        outputs.retain(SurfaceId::new(1), context.clone(), 1, FullOutput::default());
        outputs.retain(SurfaceId::new(2), context, 2, FullOutput::default());

        let order = outputs
            .take()
            .into_iter()
            .map(|output| output.surface)
            .collect::<Vec<_>>();
        assert_eq!(order, vec![SurfaceId::new(1), SurfaceId::new(2)]);
    }

    #[test]
    fn superseded_pass_texture_commands_keep_capture_order() {
        let ledger = OutputTextureLedger::default();
        let context = Context::default();
        let surface = SurfaceId::new(1);
        let texture = TextureId::Managed(13);
        let mut first = FullOutput::default();
        first.textures_delta.push(
            texture,
            ImageDelta::full(
                ColorImage::filled([1, 1], Color32::WHITE),
                TextureOptions::default(),
            ),
        );
        let mut second = output_with_free(texture);
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context, &mut first);
        captures.capture_output(&context, &mut second);
        let mut outputs = ConfirmedSurfaceOutputs::default();
        outputs.retain(surface, context.clone(), 1, first);
        outputs.retain(surface, context.clone(), 2, second);

        let mut reservation = reserve(&ledger, std::slice::from_ref(&context), captures);
        reservation
            .preflight(outputs.as_slice())
            .expect("captured interval preflights");
        reservation.attach(outputs.as_mut_slice());
        let mut retained = outputs.take().pop().expect("one final output remains");
        assert!(retained.output.textures_delta.set.contains_key(&texture));
        assert!(retained.output.textures_delta.free.contains(&texture));
        retained.output.textures_delta.clear();
        reservation.finish(Vec::new());
    }

    #[test]
    fn prepared_abort_replays_captured_texture_commands() {
        let ledger = OutputTextureLedger::default();
        let context = Context::default();
        let texture = TextureId::Managed(14);
        let mut produced = FullOutput::default();
        produced.textures_delta.push(
            texture,
            ImageDelta::full(
                ColorImage::filled([1, 1], Color32::LIGHT_GREEN),
                TextureOptions::default(),
            ),
        );
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context, &mut produced);
        let first_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(1),
            context.clone(),
            1,
            produced,
        )];
        let first = reserve(&ledger, std::slice::from_ref(&context), captures);
        first
            .preflight(&first_outputs)
            .expect("prepared batch preflights");
        drop(first);

        let mut replay_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(1),
            context.clone(),
            2,
            FullOutput::default(),
        )];
        let mut replay = reserve(
            &ledger,
            std::slice::from_ref(&context),
            FrameTextureCaptures::default(),
        );
        replay
            .preflight(&replay_outputs)
            .expect("aborted texture interval remains replayable");
        replay.attach(&mut replay_outputs);
        assert!(
            replay_outputs[0]
                .output
                .textures_delta
                .set
                .contains_key(&texture)
        );
        replay_outputs[0].output.textures_delta.clear();
        replay.finish(Vec::new());
    }

    #[test]
    fn subset_reservation_releases_unused_enrollment_and_defers_its_capture() {
        let ledger = OutputTextureLedger::default();
        let context_a = Context::default();
        let context_b = Context::default();
        let texture_b = TextureId::Managed(15);
        let mut output_b = output_with_free(texture_b);
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context_b, &mut output_b);
        output_b.drop_without_applying_deltas();
        let enrollments = vec![
            ledger.enroll(&context_a).expect("A enrolls"),
            ledger.enroll(&context_b).expect("B enrolls"),
        ];
        let reservation_a = ledger
            .reserve(vec![context_a.clone()], enrollments, captures)
            .expect("the final output subset reserves");

        let enrollment_b = ledger
            .enroll(&context_b)
            .expect("unused enrollment was released");
        reservation_a.finish(Vec::new());
        let mut outputs_b = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(2),
            context_b.clone(),
            1,
            FullOutput::default(),
        )];
        let mut reservation_b = ledger
            .reserve(
                vec![context_b.clone()],
                vec![enrollment_b],
                FrameTextureCaptures::default(),
            )
            .expect("deferred namespace reserves independently");
        reservation_b
            .preflight(&outputs_b)
            .expect("deferred namespace preflights");
        reservation_b.attach(&mut outputs_b);
        assert!(outputs_b[0].output.textures_delta.free.contains(&texture_b));
        outputs_b[0].output.textures_delta.clear();
        reservation_b.finish(Vec::new());
    }

    #[test]
    fn one_context_applies_sets_before_all_outputs_and_frees_after_them() {
        let ledger = OutputTextureLedger::default();
        let context = Context::default();
        let set_texture = TextureId::Managed(11);
        let free_texture = TextureId::Managed(12);
        let mut first = output_with_free(free_texture);
        let mut second = FullOutput::default();
        second.textures_delta.push(
            set_texture,
            ImageDelta::full(
                ColorImage::filled([1, 1], Color32::WHITE),
                TextureOptions::default(),
            ),
        );
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context, &mut first);
        captures.capture_output(&context, &mut second);
        let mut outputs = vec![
            ConfirmedSurfaceOutput::new(SurfaceId::new(1), context.clone(), 1, first),
            ConfirmedSurfaceOutput::new(SurfaceId::new(2), context.clone(), 1, second),
        ];

        let mut reservation = reserve(&ledger, std::slice::from_ref(&context), captures);
        reservation
            .preflight(&outputs)
            .expect("batch texture plan preflights");
        reservation.attach(&mut outputs);

        assert!(
            outputs[0]
                .output
                .textures_delta
                .set
                .contains_key(&set_texture)
        );
        assert!(outputs[0].output.textures_delta.free.is_empty());
        assert!(outputs[1].output.textures_delta.set.is_empty());
        assert!(
            outputs[1]
                .output
                .textures_delta
                .free
                .contains(&free_texture)
        );
        for output in &mut outputs {
            output.output.textures_delta.clear();
        }
        reservation.finish(Vec::new());
    }

    #[test]
    fn later_surface_free_does_not_erase_an_earlier_surface_set() {
        let ledger = OutputTextureLedger::default();
        let context = Context::default();
        let texture = TextureId::Managed(18);
        let mut first = FullOutput::default();
        first.textures_delta.push(
            texture,
            ImageDelta::full(
                ColorImage::filled([1, 1], Color32::WHITE),
                TextureOptions::default(),
            ),
        );
        let mut second = output_with_free(texture);
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context, &mut first);
        captures.capture_output(&context, &mut second);
        let mut outputs = vec![
            ConfirmedSurfaceOutput::new(SurfaceId::new(1), context.clone(), 1, first),
            ConfirmedSurfaceOutput::new(SurfaceId::new(2), context.clone(), 1, second),
        ];

        let mut reservation = reserve(&ledger, std::slice::from_ref(&context), captures);
        reservation
            .preflight(&outputs)
            .expect("cross-surface texture interval preflights");
        reservation.attach(&mut outputs);

        assert!(outputs[0].output.textures_delta.set.contains_key(&texture));
        assert!(outputs[0].output.textures_delta.free.is_empty());
        assert!(outputs[1].output.textures_delta.set.is_empty());
        assert!(outputs[1].output.textures_delta.free.contains(&texture));
        for output in &mut outputs {
            output.output.textures_delta.clear();
        }
        reservation.finish(Vec::new());
    }

    #[test]
    fn renderer_panic_poison_blocks_future_namespace_enrollment() {
        let ledger = OutputTextureLedger::default();
        let context = Context::default();
        let reservation = reserve(
            &ledger,
            std::slice::from_ref(&context),
            FrameTextureCaptures::default(),
        );
        reservation.finish_after_panic(Vec::new(), &context);

        assert!(matches!(
            ledger.enroll(&context),
            Err(DockspaceErrorSource::RendererTextureNamespacePoisoned)
        ));
    }

    #[test]
    fn renderer_panic_only_poisons_the_active_texture_namespace() {
        let ledger = OutputTextureLedger::default();
        let context_a = Context::default();
        let context_b = Context::default();
        let texture_a = TextureId::Managed(16);
        let texture_b = TextureId::Managed(17);
        let mut output_a = output_with_free(texture_a);
        let mut output_b = output_with_free(texture_b);
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context_a, &mut output_a);
        captures.capture_output(&context_b, &mut output_b);
        let mut outputs = vec![
            ConfirmedSurfaceOutput::new(SurfaceId::new(1), context_a.clone(), 1, output_a),
            ConfirmedSurfaceOutput::new(SurfaceId::new(2), context_b.clone(), 1, output_b),
        ];
        let mut reservation = reserve(&ledger, &[context_a.clone(), context_b.clone()], captures);
        reservation
            .preflight(&outputs)
            .expect("two namespaces preflight");
        reservation.attach(&mut outputs);
        outputs[0].output.textures_delta.clear();
        let replay_b = std::mem::take(&mut outputs[1].output.textures_delta);
        reservation.finish_after_panic(vec![(context_b.clone(), replay_b)], &context_a);

        assert!(matches!(
            ledger.enroll(&context_a),
            Err(DockspaceErrorSource::RendererTextureNamespacePoisoned)
        ));
        let mut replay_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(2),
            context_b.clone(),
            2,
            FullOutput::default(),
        )];
        let mut replay = reserve(
            &ledger,
            std::slice::from_ref(&context_b),
            FrameTextureCaptures::default(),
        );
        replay
            .preflight(&replay_outputs)
            .expect("independent namespace remains usable");
        replay.attach(&mut replay_outputs);
        assert!(
            replay_outputs[0]
                .output
                .textures_delta
                .free
                .contains(&texture_b)
        );
        replay_outputs[0].output.textures_delta.clear();
        replay.finish(Vec::new());
    }

    #[test]
    fn rejected_unconsumed_texture_commands_replay_in_the_next_batch() {
        let ledger = OutputTextureLedger::default();
        let context = Context::default();
        let texture = TextureId::Managed(21);
        let mut output = FullOutput::default();
        output.textures_delta.push(
            texture,
            ImageDelta::full(
                ColorImage::filled([1, 1], Color32::LIGHT_BLUE),
                TextureOptions::default(),
            ),
        );
        let mut captures = FrameTextureCaptures::default();
        captures.capture_output(&context, &mut output);
        let mut first_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(1),
            context.clone(),
            1,
            output,
        )];
        let mut first = reserve(&ledger, std::slice::from_ref(&context), captures);
        first
            .preflight(&first_outputs)
            .expect("first batch preflights");
        first.attach(&mut first_outputs);
        let rejected = std::mem::take(&mut first_outputs[0].output.textures_delta);
        first.finish(vec![(context.clone(), rejected)]);

        let mut second_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(2),
            context.clone(),
            1,
            FullOutput::default(),
        )];
        let mut second = reserve(
            &ledger,
            std::slice::from_ref(&context),
            FrameTextureCaptures::default(),
        );
        second
            .preflight(&second_outputs)
            .expect("replay batch preflights");
        second.attach(&mut second_outputs);
        assert!(
            second_outputs[0]
                .output
                .textures_delta
                .set
                .contains_key(&texture)
        );
        second_outputs[0].output.textures_delta.clear();
        second.finish(Vec::new());
    }
}
