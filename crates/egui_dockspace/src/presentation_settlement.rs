//! Renderer-facing settlement for exact egui presentation outputs.

use std::fmt::Debug;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use dockspace::backend::presentation_observation::HostPresentationOutput;
use dockspace::ids::SurfaceId;
use egui::FullOutput;

use crate::facade::{DeferredTextureDeltas, ExactNativeViewport, NativeCoreRoute};

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

/// Affine capability for reporting one exact renderer result after egui returns.
///
/// This value can be carried independently from [`FullOutput`] to match the
/// ordinary egui integration boundary. Calling [`Self::settle`] consumes the
/// capability. Dropping a required, unsettled capability records
/// [`EguiPresentationResult::Dropped`], so an abandoned output cannot block its
/// presentation stream. A capability for an output without an obligation is an
/// explicit no-op and reports that state through [`Self::is_required`].
#[must_use = "a required settlement must be reported or intentionally dropped"]
pub struct EguiPresentationSettlement {
    surface: SurfaceId,
    native: Option<NativeCoreRoute>,
    presentation: Option<PendingEguiPresentation>,
}

impl Debug for EguiPresentationSettlement {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EguiPresentationSettlement")
            .field("surface", &self.surface)
            .field("native", &self.native.map(NativeCoreRoute::native))
            .field("required", &self.is_required())
            .finish_non_exhaustive()
    }
}

impl EguiPresentationSettlement {
    fn new(
        surface: SurfaceId,
        native: Option<NativeCoreRoute>,
        presentation: Option<PendingEguiPresentation>,
    ) -> Self {
        Self {
            surface,
            native,
            presentation,
        }
    }

    /// Returns the logical surface associated with the renderer output.
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

    /// Returns whether this capability represents a core settlement obligation.
    #[must_use]
    pub const fn is_required(&self) -> bool {
        self.presentation.is_some()
    }

    /// Records the exact terminal renderer result once.
    ///
    /// This is an explicit no-op when [`Self::is_required`] is `false`.
    pub fn settle(mut self, result: EguiPresentationResult) {
        if let Some(presentation) = self.presentation.take() {
            presentation.complete(result);
        }
    }

    /// Records a renderer result only for the exact native lifetime that produced it.
    ///
    /// A mismatch returns an error which still owns this affine settlement, so
    /// the caller may route it to the correct renderer result or intentionally
    /// drop it. No presentation result is recorded on the error path.
    pub fn settle_native(
        self,
        submitted: ExactNativeViewport,
        result: EguiPresentationResult,
    ) -> Result<(), EguiNativePresentationSettlementError> {
        if self.native_binding() != Some(submitted) {
            return Err(EguiNativePresentationSettlementError {
                submitted,
                settlement: self,
            });
        }
        self.settle(result);
        Ok(())
    }
}

/// A renderer result was reported for another native viewport lifetime.
///
/// This error retains the affine settlement capability. Consuming the error
/// with [`Self::into_settlement`] lets a native runtime retry exact routing;
/// dropping it terminally records `Dropped` for a required obligation.
#[must_use = "the retained settlement must be rerouted or intentionally dropped"]
pub struct EguiNativePresentationSettlementError {
    submitted: ExactNativeViewport,
    settlement: EguiPresentationSettlement,
}

impl Debug for EguiNativePresentationSettlementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("EguiNativePresentationSettlementError")
            .field("expected", &self.expected())
            .field("submitted", &self.submitted)
            .finish_non_exhaustive()
    }
}

impl std::fmt::Display for EguiNativePresentationSettlementError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            formatter,
            "native presentation settlement expected {:?}, submitted {:?}",
            self.expected(),
            self.submitted
        )
    }
}

impl std::error::Error for EguiNativePresentationSettlementError {}

impl EguiNativePresentationSettlementError {
    /// Returns the native lifetime carried by the output capability.
    #[must_use]
    pub const fn expected(&self) -> Option<ExactNativeViewport> {
        self.settlement.native_binding()
    }

    /// Returns the native lifetime attached to the renderer result.
    #[must_use]
    pub const fn submitted(&self) -> ExactNativeViewport {
        self.submitted
    }

    /// Recovers the still-unsettled affine capability.
    #[must_use]
    pub fn into_settlement(self) -> EguiPresentationSettlement {
        self.settlement
    }
}

/// One exact egui output bound to its renderer settlement obligation.
///
/// Use [`Self::into_parts`] when the renderer owns [`FullOutput`] and reports
/// presentation later. Use [`Self::settle_with`] for the synchronous case.
/// Dropping this value terminally records `Dropped` when an obligation exists.
#[must_use = "the bound output must be presented or terminally dropped"]
pub struct EguiOuterSurfaceOutput {
    surface: SurfaceId,
    native: Option<NativeCoreRoute>,
    full_output: Option<FullOutput>,
    presentation: Option<PendingEguiPresentation>,
    deferred_texture_deltas: DeferredTextureDeltas,
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
        full_output: FullOutput,
        presentation: Option<PendingEguiPresentation>,
        deferred_texture_deltas: DeferredTextureDeltas,
    ) -> Self {
        Self {
            surface,
            native,
            full_output: Some(full_output),
            presentation,
            deferred_texture_deltas,
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

    /// Separates the egui output from its late renderer-settlement capability.
    ///
    /// Splitting does not complete the obligation. The returned settlement is
    /// affine and must be consumed with [`EguiPresentationSettlement::settle`]
    /// or dropped, which terminally records `Dropped` when required.
    #[must_use = "the returned settlement capability must be handled"]
    pub fn into_parts(mut self) -> (SurfaceId, FullOutput, EguiPresentationSettlement) {
        let surface = self.surface;
        let native = self.native;
        let full_output = self
            .full_output
            .take()
            .expect("an outer output can be split exactly once");
        let presentation = self.presentation.take();
        (
            surface,
            full_output,
            EguiPresentationSettlement::new(surface, native, presentation),
        )
    }

    /// Consumes the exact output inside the renderer boundary and records its result.
    pub fn settle_with(self, settle: impl FnOnce(SurfaceId, FullOutput) -> EguiPresentationResult) {
        let (surface, full_output, presentation) = self.into_parts();
        let result = settle(surface, full_output);
        presentation.settle(result);
    }
}

impl Drop for EguiOuterSurfaceOutput {
    fn drop(&mut self) {
        if let Some(full_output) = self.full_output.take() {
            self.deferred_texture_deltas.defer_output(full_output);
        }
    }
}
