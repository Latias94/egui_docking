//! Renderer-facing ownership settlement for exact egui outputs.

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use dockspace::backend::ids::SurfaceId;
use dockspace::backend::presentation_observation::HostPresentationOutput;
use egui::{Context, FullOutput, TexturesDelta};

use crate::facade::{ExactNativeViewport, NativeCoreRoute};
use crate::output_ownership::OutputBatchReservation;

/// Renderer ownership disposition for one exact outer-host output.
///
/// This reports command ownership, not physical GPU presentation. Native hosts
/// report real window presentation through their platform observation lane.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EguiRendererOutputDisposition {
    /// The renderer irreversibly accepted and consumed the output commands.
    Accepted,
    /// The renderer rejected the output without consuming commands or side effects.
    RejectedUnconsumed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum EguiRendererCompletion {
    Accepted,
    Dropped,
}

const RENDERER_PENDING: u8 = 0;
const RENDERER_ACCEPTED: u8 = 1;
const RENDERER_DROPPED: u8 = 2;

pub(crate) struct OuterPresentationCompletion {
    state: AtomicU8,
}

impl OuterPresentationCompletion {
    pub(crate) fn pending() -> Self {
        Self {
            state: AtomicU8::new(RENDERER_PENDING),
        }
    }

    pub(crate) fn result(&self) -> Option<EguiRendererCompletion> {
        match self.state.load(Ordering::Acquire) {
            RENDERER_PENDING => None,
            RENDERER_ACCEPTED => Some(EguiRendererCompletion::Accepted),
            RENDERER_DROPPED => Some(EguiRendererCompletion::Dropped),
            _ => None,
        }
    }

    fn complete(&self, result: EguiRendererCompletion) -> bool {
        let state = match result {
            EguiRendererCompletion::Accepted => RENDERER_ACCEPTED,
            EguiRendererCompletion::Dropped => RENDERER_DROPPED,
        };
        self.state
            .compare_exchange(RENDERER_PENDING, state, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
    }

    fn drop_if_pending(&self) {
        let _ = self.state.compare_exchange(
            RENDERER_PENDING,
            RENDERER_DROPPED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Private capability minted for an exact core presentation output.
#[must_use = "the renderer must accept or terminally drop this output"]
pub(crate) struct PendingEguiPresentation {
    output: HostPresentationOutput,
    completion: Arc<OuterPresentationCompletion>,
}

impl Debug for PendingEguiPresentation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PendingEguiPresentation")
            .field("output", &self.output)
            .finish_non_exhaustive()
    }
}

impl PendingEguiPresentation {
    pub(crate) fn new(
        output: HostPresentationOutput,
        completion: Arc<OuterPresentationCompletion>,
    ) -> Self {
        Self { output, completion }
    }

    pub(crate) const fn surface(&self) -> SurfaceId {
        self.output.surface()
    }

    const fn output(&self) -> HostPresentationOutput {
        self.output
    }

    fn complete(self, result: EguiRendererCompletion) {
        let _ = self.completion.complete(result);
    }
}

impl Drop for PendingEguiPresentation {
    fn drop(&mut self) {
        self.completion.drop_if_pending();
    }
}

/// One exact egui output bound to its renderer admission obligation.
///
/// Instances remain owned by [`EguiOuterOutputBatch`], so renderer commands and
/// terminal admission disposition cannot be separated accidentally.
#[must_use = "the bound output must be accepted or terminally dropped"]
pub struct EguiOuterSurfaceOutput {
    surface: SurfaceId,
    native: Option<NativeCoreRoute>,
    context: Context,
    full_output: Option<FullOutput>,
    presentation: Option<PendingEguiPresentation>,
}

impl Debug for EguiOuterSurfaceOutput {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EguiOuterSurfaceOutput")
            .field("surface", &self.surface)
            .field("native", &self.native.map(NativeCoreRoute::native))
            .field("presentation_pending", &self.presentation.is_some())
            .finish_non_exhaustive()
    }
}

impl EguiOuterSurfaceOutput {
    pub(crate) fn new(
        surface: SurfaceId,
        native: Option<NativeCoreRoute>,
        context: Context,
        full_output: FullOutput,
        presentation: Option<PendingEguiPresentation>,
    ) -> Self {
        Self {
            surface,
            native,
            context,
            full_output: Some(full_output),
            presentation,
        }
    }

    /// Returns the logical surface rendered by this exact output.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact native lifetime that produced this output, if any.
    #[must_use]
    pub const fn native_binding(&self) -> Option<ExactNativeViewport> {
        match self.native {
            Some(route) => Some(route.native()),
            None => None,
        }
    }

    /// Returns the exact native-to-core route that produced this output.
    #[must_use]
    pub const fn native_route(&self) -> Option<NativeCoreRoute> {
        self.native
    }

    /// Returns the core-minted identity of the presentation obligation, if any.
    #[must_use]
    pub fn presentation_output(&self) -> Option<HostPresentationOutput> {
        self.presentation
            .as_ref()
            .map(PendingEguiPresentation::output)
    }

    /// Borrows the exact egui output before renderer submission.
    #[must_use]
    pub fn full_output(&self) -> &FullOutput {
        self.full_output
            .as_ref()
            .expect("a live outer output retains its egui FullOutput")
    }

    /// Returns the egui texture namespace that owns this output.
    #[must_use]
    pub const fn texture_context(&self) -> &Context {
        &self.context
    }

    /// Returns whether this output must report terminal renderer admission.
    #[must_use]
    pub const fn has_renderer_admission_obligation(&self) -> bool {
        self.presentation.is_some()
    }

    fn accept_renderer_output(&mut self) {
        self.full_output
            .as_mut()
            .expect("a live output retains renderer commands")
            .textures_delta
            .clear();
        if let Some(presentation) = self.presentation.take() {
            presentation.complete(EguiRendererCompletion::Accepted);
        }
    }

    fn drop_output(&mut self) -> Option<(Context, TexturesDelta)> {
        if let Some(presentation) = self.presentation.take() {
            presentation.complete(EguiRendererCompletion::Dropped);
        }
        let mut output = self.full_output.take()?;
        let delta = std::mem::take(&mut output.textures_delta);
        (!delta.is_empty()).then(|| (self.context.clone(), delta))
    }

    fn discard_poisoned(&mut self) {
        if let Some(presentation) = self.presentation.take() {
            presentation.complete(EguiRendererCompletion::Dropped);
        }
        if let Some(mut output) = self.full_output.take() {
            output.textures_delta.clear();
            output.drop_without_applying_deltas();
        }
    }
}

impl Drop for EguiOuterSurfaceOutput {
    fn drop(&mut self) {
        if let Some((_, mut delta)) = self.drop_output() {
            delta.clear();
        }
    }
}

/// Ordered renderer batch for all final outputs from one host frame.
///
/// The callback is invoked in exact confirmation order. Returning
/// [`EguiRendererOutputDisposition::RejectedUnconsumed`] stops the batch; later
/// outputs are not exposed because their shapes may rely on commands owned by
/// an earlier output in the same namespace.
#[must_use = "the renderer output batch must be submitted or intentionally dropped"]
pub struct EguiOuterOutputBatch {
    outputs: Vec<EguiOuterSurfaceOutput>,
    reservation: Option<OutputBatchReservation>,
    active_context: Option<Context>,
}

impl Debug for EguiOuterOutputBatch {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EguiOuterOutputBatch")
            .field("outputs", &self.outputs)
            .finish_non_exhaustive()
    }
}

impl EguiOuterOutputBatch {
    pub(crate) fn new(
        outputs: Vec<EguiOuterSurfaceOutput>,
        reservation: Option<OutputBatchReservation>,
    ) -> Self {
        debug_assert_eq!(outputs.is_empty(), reservation.is_none());
        Self {
            outputs,
            reservation,
            active_context: None,
        }
    }

    /// Returns the number of final surface outputs in generation order.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.outputs.len()
    }

    /// Returns whether this host frame produced no renderer outputs.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.outputs.is_empty()
    }

    /// Iterates final surface outputs without separating their renderer ownership.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &EguiOuterSurfaceOutput> {
        self.outputs.iter()
    }

    /// Submits every output to the renderer in exact generation order.
    ///
    /// `Accepted` means the callback has irreversibly consumed the renderer
    /// commands. It does not assert that a swapchain image reached the screen.
    /// `RejectedUnconsumed` promises that the callback performed no renderer
    /// side effects, allowing this batch to replay the commands later.
    pub fn submit_with(
        mut self,
        mut submit: impl FnMut(SurfaceId, &Context, &FullOutput) -> EguiRendererOutputDisposition,
    ) {
        let mut rejected = false;
        for output in &mut self.outputs {
            if rejected {
                continue;
            }
            self.active_context = Some(output.context.clone());
            let result = submit(
                output.surface,
                &output.context,
                output
                    .full_output
                    .as_ref()
                    .expect("an unsettled batch output retains renderer commands"),
            );
            self.active_context = None;
            match result {
                EguiRendererOutputDisposition::Accepted => output.accept_renderer_output(),
                EguiRendererOutputDisposition::RejectedUnconsumed => rejected = true,
            }
        }
        self.finish();
    }

    fn finish(&mut self) {
        let mut dropped = Vec::new();
        for output in &mut self.outputs {
            if let Some(delta) = output.drop_output() {
                dropped.push(delta);
            }
        }
        if let Some(reservation) = self.reservation.take() {
            reservation.finish(dropped);
        } else {
            for (_, mut delta) in dropped {
                delta.clear();
            }
        }
    }

    fn poison_after_renderer_panic(&mut self) {
        let Some(poisoned) = self.active_context.take() else {
            self.finish();
            return;
        };
        let mut dropped = Vec::new();
        for output in &mut self.outputs {
            if output.context == poisoned {
                output.discard_poisoned();
            } else if let Some(delta) = output.drop_output() {
                dropped.push(delta);
            }
        }
        if let Some(reservation) = self.reservation.take() {
            reservation.finish_after_panic(dropped, &poisoned);
        } else {
            for (_, mut delta) in dropped {
                delta.clear();
            }
        }
    }
}

impl Drop for EguiOuterOutputBatch {
    fn drop(&mut self) {
        if self.active_context.is_some() {
            self.poison_after_renderer_panic();
        } else {
            self.finish();
        }
    }
}
