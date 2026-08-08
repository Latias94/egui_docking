//! Ordered ownership of egui renderer outputs and texture namespaces.

use std::sync::{Arc, Mutex};

use dockspace::ids::SurfaceId;
use egui::{Context, FullOutput, TexturesDelta};

use crate::error::DockspaceErrorSource;

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
            let mut texture_interval = std::mem::take(&mut previous_output.textures_delta);
            let newer_textures = std::mem::take(&mut output.textures_delta);
            previous_output.append(output);
            append_texture_interval(&mut texture_interval, newer_textures);
            previous_output.textures_delta = texture_interval;
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
    outstanding: Vec<OutstandingTextureBatch>,
    deferred: Vec<DeferredTextureNamespace>,
}

impl Drop for OutputTextureLedgerState {
    fn drop(&mut self) {
        for namespace in &mut self.deferred {
            namespace.delta.clear();
        }
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

impl OutputTextureLedger {
    pub(crate) fn defer_output(&self, context: &Context, mut output: FullOutput) {
        let delta = std::mem::take(&mut output.textures_delta);
        self.defer_delta(context.clone(), delta);
    }

    fn defer_delta(&self, context: Context, delta: TexturesDelta) {
        if delta.is_empty() {
            return;
        }
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        append_namespace_delta(&mut state.deferred, context, delta);
    }

    pub(crate) fn reserve(
        &self,
        contexts: Vec<Context>,
    ) -> Result<OutputBatchReservation, DockspaceErrorSource> {
        debug_assert!(!contexts.is_empty());
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.outstanding.iter().any(|outstanding| {
            outstanding
                .contexts
                .iter()
                .any(|active| contexts.iter().any(|context| context == active))
        }) {
            return Err(DockspaceErrorSource::RendererOutputBatchOutstanding);
        }
        let batch = state
            .next_batch
            .checked_add(1)
            .ok_or(DockspaceErrorSource::RendererOutputBatchIdentityExhausted)?;
        state.next_batch = batch;
        state
            .outstanding
            .push(OutstandingTextureBatch { batch, contexts });
        Ok(OutputBatchReservation {
            ledger: self.clone(),
            batch,
            finished: false,
        })
    }

    fn take_deferred_for(&self, batch: u64) -> Vec<DeferredTextureNamespace> {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let contexts = state
            .outstanding
            .iter()
            .find(|outstanding| outstanding.batch == batch)
            .map(|outstanding| outstanding.contexts.clone())
            .expect("a live output reservation remains registered");
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
        matching
    }

    fn finish(&self, batch: u64, mut earlier: Vec<DeferredTextureNamespace>) {
        let mut state = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = state
            .outstanding
            .iter()
            .position(|outstanding| outstanding.batch == batch)
            .expect("a live output reservation remains registered");
        state.outstanding.remove(index);
        for deferred in std::mem::take(&mut state.deferred) {
            append_namespace_delta(&mut earlier, deferred.context, deferred.delta);
        }
        state.deferred = earlier;
    }
}

/// Affine reservation for one renderer output batch.
#[must_use = "an output batch reservation must be finished or dropped"]
pub(crate) struct OutputBatchReservation {
    ledger: OutputTextureLedger,
    batch: u64,
    finished: bool,
}

impl OutputBatchReservation {
    pub(crate) fn normalize(&self, outputs: &mut [ConfirmedSurfaceOutput]) {
        let deferred = self.ledger.take_deferred_for(self.batch);
        normalize_texture_deltas(outputs, deferred);
    }

    pub(crate) fn finish(mut self, dropped: Vec<(Context, TexturesDelta)>) {
        let dropped = dropped
            .into_iter()
            .map(|(context, delta)| DeferredTextureNamespace { context, delta })
            .collect();
        self.ledger.finish(self.batch, dropped);
        self.finished = true;
    }
}

impl Drop for OutputBatchReservation {
    fn drop(&mut self) {
        if !self.finished {
            self.ledger.finish(self.batch, Vec::new());
            self.finished = true;
        }
    }
}

fn append_namespace_delta(
    deferred: &mut Vec<DeferredTextureNamespace>,
    context: Context,
    delta: TexturesDelta,
) {
    if let Some(namespace) = deferred
        .iter_mut()
        .find(|namespace| namespace.context == context)
    {
        append_texture_interval(&mut namespace.delta, delta);
    } else {
        deferred.push(DeferredTextureNamespace { context, delta });
    }
}

fn append_texture_interval(earlier: &mut TexturesDelta, mut later: TexturesDelta) {
    for (texture, deltas) in std::mem::take(&mut later.set) {
        for image in deltas {
            earlier.push(texture, image);
        }
    }
    earlier.free.extend(std::mem::take(&mut later.free));
}

fn normalize_texture_deltas(
    outputs: &mut [ConfirmedSurfaceOutput],
    deferred: Vec<DeferredTextureNamespace>,
) {
    let mut plans = deferred;
    for output in outputs.iter_mut() {
        let delta = std::mem::take(&mut output.output_mut().textures_delta);
        append_namespace_delta(&mut plans, output.context.clone(), delta);
    }
    for mut plan in plans {
        let first = outputs
            .iter()
            .position(|output| output.context == plan.context)
            .expect("every reserved texture namespace owns a final output");
        let last = outputs
            .iter()
            .rposition(|output| output.context == plan.context)
            .expect("every reserved texture namespace owns a final output");
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

    #[test]
    fn deferred_textures_stay_in_their_context_namespace() {
        let ledger = OutputTextureLedger::default();
        let context_a = Context::default();
        let context_b = Context::default();
        let texture = TextureId::Managed(7);
        ledger.defer_output(&context_a, output_with_free(texture));

        let reservation = ledger
            .reserve(vec![context_b.clone()])
            .expect("the first batch reserves");
        let mut context_b_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(1),
            context_b,
            1,
            FullOutput::default(),
        )];
        reservation.normalize(&mut context_b_outputs);
        assert!(context_b_outputs[0].output.textures_delta.is_empty());
        reservation.finish(Vec::new());

        let reservation = ledger
            .reserve(vec![context_a.clone()])
            .expect("the second batch reserves");
        let mut context_a_outputs = vec![ConfirmedSurfaceOutput::new(
            SurfaceId::new(2),
            context_a,
            1,
            FullOutput::default(),
        )];
        reservation.normalize(&mut context_a_outputs);
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
        let reservation_a = ledger
            .reserve(vec![context_a.clone()])
            .expect("the first namespace reserves");
        let reservation_b = ledger
            .reserve(vec![context_b])
            .expect("an independent namespace may reserve concurrently");
        let conflict = ledger.reserve(vec![context_a]);
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
    fn superseded_pass_keeps_a_set_that_is_freed_after_other_surface_paints() {
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
        let second = output_with_free(texture);
        let mut outputs = ConfirmedSurfaceOutputs::default();
        outputs.retain(surface, context.clone(), 1, first);
        outputs.retain(surface, context, 2, second);

        let mut retained = outputs.take().pop().expect("one final output remains");
        assert!(retained.output.textures_delta.set.contains_key(&texture));
        assert!(retained.output.textures_delta.free.contains(&texture));
        retained.output.textures_delta.clear();
    }

    #[test]
    fn one_context_applies_sets_before_all_outputs_and_frees_after_them() {
        let context = Context::default();
        let set_texture = TextureId::Managed(11);
        let free_texture = TextureId::Managed(12);
        let first = output_with_free(free_texture);
        let mut second = FullOutput::default();
        second.textures_delta.push(
            set_texture,
            ImageDelta::full(
                ColorImage::filled([1, 1], Color32::WHITE),
                TextureOptions::default(),
            ),
        );
        let mut outputs = vec![
            ConfirmedSurfaceOutput::new(SurfaceId::new(1), context.clone(), 1, first),
            ConfirmedSurfaceOutput::new(SurfaceId::new(2), context, 1, second),
        ];

        normalize_texture_deltas(&mut outputs, Vec::new());

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
    }
}
