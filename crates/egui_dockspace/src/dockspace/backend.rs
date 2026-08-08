//! Narrow read-only state required by an ordered native host backend.

use dockspace::ClosePlan;
use dockspace::backend_ingress::BackendIngressCommitWatermark;
use dockspace::interaction::InteractionPreview;
use dockspace::platform::ObservedWorkArea;
use dockspace::scene::PresentationPlan;
use dockspace::viewport::{ViewportBinding, WorkAreaGeneration};
use dockspace::{PlatformObservationLease, ids::SurfaceId};

use super::Dockspace;

/// Exact core-owned work-area state accepted at the last committed backend boundary.
#[derive(Clone, Debug)]
pub struct EguiNativeWorkAreaState {
    provider: PlatformObservationLease,
    generation: WorkAreaGeneration,
    work_areas: Vec<ObservedWorkArea>,
}

impl EguiNativeWorkAreaState {
    /// Returns the provider which owns this observation namespace.
    #[must_use]
    pub const fn provider(&self) -> PlatformObservationLease {
        self.provider
    }

    /// Returns the accepted core work-area generation.
    #[must_use]
    pub const fn generation(&self) -> WorkAreaGeneration {
        self.generation
    }

    /// Returns the complete accepted work-area roster.
    #[must_use]
    pub fn work_areas(&self) -> &[ObservedWorkArea] {
        &self.work_areas
    }
}

impl Dockspace {
    /// Returns the current committed backend prefix watermark.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_ingress_commit_watermark(&self) -> Option<BackendIngressCommitWatermark> {
        self.core_engine().backend_ingress_commit_watermark()
    }

    /// Returns the retained replacement binding for one recovery-owned surface.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_recovery_replacement_binding(
        &self,
        surface: SurfaceId,
    ) -> Option<ViewportBinding> {
        self.core_engine()
            .viewport()
            .recovery_pending(surface)
            .and_then(dockspace::frame::RecoveryPending::replacement_binding)
    }

    /// Returns the exact binding currently permitted to publish pane-focus facts.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_viewport_focus_binding(&self, surface: SurfaceId) -> Option<ViewportBinding> {
        self.core_engine().viewport_focus_binding(surface)
    }

    /// Captures the complete current work-area authority, when supported and observed.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_work_area_state(&self) -> Option<EguiNativeWorkAreaState> {
        let engine = self.core_engine();
        let viewport = engine.viewport();
        if !viewport.capabilities().work_area().is_supported() {
            return None;
        }
        Some(EguiNativeWorkAreaState {
            provider: engine.platform_provider()?,
            generation: viewport.work_area_generation(),
            work_areas: viewport.work_areas().map(|(_, area)| area).collect(),
        })
    }

    /// Iterates the currently non-terminal core-owned close plans.
    #[doc(hidden)]
    pub fn backend_active_close_plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.core_engine().active_close_plans()
    }

    /// Returns whether one surface has exact current interaction authority.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_surface_is_interactive(&self, surface: SurfaceId) -> bool {
        self.core_engine().interaction_authority(surface).is_some()
    }

    /// Returns the current core-owned drag preview for backend diagnostics.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_presentation_preview(&self) -> Option<&InteractionPreview> {
        self.core_engine().presentation_preview()
    }

    /// Returns one ready core-owned presentation plan for backend rendering diagnostics.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_ready_presentation_plan(&self, surface: SurfaceId) -> Option<&PresentationPlan> {
        self.core_engine()
            .scene()
            .surface(surface)?
            .ready()
            .map(|ready| ready.plan())
    }

    /// Returns the number of destroyed-binding guards retained until exact host quiescence.
    #[doc(hidden)]
    #[must_use]
    pub fn backend_destroyed_binding_guard_count(&self) -> usize {
        self.core_engine()
            .runtime_retention_manifest()
            .bindings()
            .destroyed_binding_guards()
    }
}
