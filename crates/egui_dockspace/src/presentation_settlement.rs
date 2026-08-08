//! Renderer-facing settlement for exact egui presentation outputs.

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use dockspace::backend::presentation_observation::HostPresentationOutput;
use dockspace::ids::SurfaceId;
use egui::{Context, FullOutput, TexturesDelta};

use crate::facade::{ExactNativeViewport, NativeCoreRoute};
use crate::output_ownership::OutputBatchReservation;

/// Terminal renderer result for one exact outer-host output.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EguiPresentationResult {
    /// The renderer successfully presented the exact output.
    Presented,
    /// The renderer terminally discarded the exact output without presenting it.
    Dropped,
}

const PRESENTATION_PENDING: u8 = 0;
const PRESENTATION_PRESENTED: u8 = 1;
const PRESENTATION_DROPPED: u8 = 2;

pub(crate) struct OuterPresentationCompletion {
    state: AtomicU8,
}

impl OuterPresentationCompletion {
    pub(crate) fn pending() -> Self {
        Self {
            state: AtomicU8::new(PRESENTATION_PENDING),
        }
    }

    pub(crate) fn result(&self) -> Option<EguiPresentationResult> {
        match self.state.load(Ordering::Acquire) {
            PRESENTATION_PENDING => None,
            PRESENTATION_PRESENTED => Some(EguiPresentationResult::Presented),
            PRESENTATION_DROPPED => Some(EguiPresentationResult::Dropped),
            _ => None,
        }
    }

    fn complete(&self, result: EguiPresentationResult) -> bool {
        let state = match result {
            EguiPresentationResult::Presented => PRESENTATION_PRESENTED,
            EguiPresentationResult::Dropped => PRESENTATION_DROPPED,
        };
        self.state
            .compare_exchange(
                PRESENTATION_PENDING,
                state,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn drop_if_pending(&self) {
        let _ = self.state.compare_exchange(
            PRESENTATION_PENDING,
            PRESENTATION_DROPPED,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }
}

/// Private capability minted for an exact core presentation output.
#[must_use = "the renderer should report Presented or Dropped for this output"]
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

    fn complete(self, result: EguiPresentationResult) {
        debug_assert!(
            self.completion.complete(result),
            "an affine output capability completes exactly once"
        );
    }
}

impl Drop for PendingEguiPresentation {
    fn drop(&mut self) {
        self.completion.drop_if_pending();
    }
}

/// One exact egui output bound to its renderer settlement obligation.
///
/// Instances remain owned by [`EguiOuterOutputBatch`], so renderer commands and
/// terminal presentation disposition cannot be separated accidentally.
#[must_use = "the bound output must be presented or terminally dropped"]
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

    /// Returns whether this output must report a terminal renderer result.
    #[must_use]
    pub const fn has_presentation_obligation(&self) -> bool {
        self.presentation.is_some()
    }

    fn settle_presented(&mut self) {
        self.full_output
            .as_mut()
            .expect("a live output retains renderer commands")
            .textures_delta
            .clear();
        if let Some(presentation) = self.presentation.take() {
            presentation.complete(EguiPresentationResult::Presented);
        }
    }

    fn drop_output(&mut self) -> Option<(Context, TexturesDelta)> {
        if let Some(presentation) = self.presentation.take() {
            presentation.complete(EguiPresentationResult::Dropped);
        }
        let mut output = self.full_output.take()?;
        let delta = std::mem::take(&mut output.textures_delta);
        (!delta.is_empty()).then(|| (self.context.clone(), delta))
    }
}

impl Drop for EguiOuterSurfaceOutput {
    fn drop(&mut self) {
        let _ = self.drop_output();
    }
}

/// Ordered renderer batch for all final outputs from one host frame.
///
/// The callback is invoked in exact confirmation order. Returning `Dropped`
/// stops the batch; later outputs are not exposed because their shapes may rely
/// on texture commands owned by an earlier output in the same namespace.
#[must_use = "the renderer output batch must be settled or intentionally dropped"]
pub struct EguiOuterOutputBatch {
    outputs: Vec<EguiOuterSurfaceOutput>,
    reservation: Option<OutputBatchReservation>,
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

    /// Renders and settles every output in exact generation order.
    pub fn settle_with(
        mut self,
        mut settle: impl FnMut(SurfaceId, &FullOutput) -> EguiPresentationResult,
    ) {
        let mut dropped = false;
        for output in &mut self.outputs {
            if dropped {
                continue;
            }
            let result = settle(
                output.surface,
                output
                    .full_output
                    .as_ref()
                    .expect("an unsettled batch output retains renderer commands"),
            );
            match result {
                EguiPresentationResult::Presented => output.settle_presented(),
                EguiPresentationResult::Dropped => dropped = true,
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
            debug_assert!(dropped.is_empty());
            for (_, mut delta) in dropped {
                delta.clear();
            }
        }
    }
}

impl Drop for EguiOuterOutputBatch {
    fn drop(&mut self) {
        self.finish();
    }
}
