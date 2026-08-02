//! Transactional eframe application facade for the native dockspace runtime.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::engine::BackendIngressProgress;
use dockspace::ids::SurfaceId;
use dockspace::intent::Authority;
use dockspace::policy::DockPolicy;
use dockspace::presentation_observation::{
    HostPresentationObservationOutcome, PresentationHostLease,
};
use dockspace::scene_manifest::MeasurementUnavailableReason;
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::{CloseDecisionToken, CloseItemDecisionState, DeferredCloseToken, NativeCloseEdge};
use eframe::{
    HostedNativeStagingPresentation, HostedViewportCycle, HostedViewportMode, HostedViewportOutput,
    HostedViewportUiDisposition, NativeEffectSink, NativeViewportCreateSink,
};
use egui::{FullOutput, ViewportId};
use egui_dockspace::{
    DockStyle, Dockspace, EguiFrameScheduleKey, EguiNativePresentationSession, ExactNativeViewport,
    PaneView, PreparedEguiOuterFrameCommit,
};

use crate::NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY;
use crate::configuration::NativeConfigurationQueue;
use crate::error::HostedHookError;
use crate::ingress::{
    BoundNativeRoute, NativeEffectCycle, NativeIngressBridge, NativeIngressTransaction,
    PreparedNativeIngress,
};
use crate::presentation::{NativePresentationLedger, PreparedNativePresentationBatch};
use crate::receiver::pointer_receiver_receipts;
use crate::{
    NativeCloseHandler, NativeCloseItemRequest, NativeDeferredCloseRequest,
    NativeSurfaceCloseContext, VetoNativeClose,
};
use crate::{NativeRuntimeError, NativeViewportRoster};

/// Read-only counters describing the native runtime's latest committed state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeRuntimeStatus {
    /// Number of complete hosted cycles published by the runtime.
    pub committed_cycles: u64,
    /// Number of exact live native-to-core routes in the latest cycle.
    pub live_viewports: usize,
    /// Number of renderer outputs awaiting an ordered terminal result.
    pub pending_presentations: usize,
    /// Number of core presentation obligations emitted to the native renderer.
    pub emitted_presentations: u64,
    /// Number of presentation observations reduced by the core.
    pub presentation_observations: u64,
    /// Number of presentation observations rejected as stale or malformed.
    pub presentation_rejections: u64,
    /// Number of presented outputs which were eligible to mint interaction authority.
    pub promoted_presentations: u64,
    /// Number of presented outputs settled after their stream was superseded.
    pub ineligible_presentations: u64,
}

struct ActiveNativeCycle {
    session: EguiNativePresentationSession,
    presentation_host: PresentationHostLease,
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    expected_surfaces: BTreeSet<SurfaceId>,
    callbacks: BTreeSet<ViewportId>,
    presentation_callbacks: BTreeSet<ViewportId>,
    staging: BTreeMap<ViewportId, HostedNativeStagingPresentation>,
    staging_callbacks: BTreeSet<ViewportId>,
    effect_sink: NativeEffectSink,
    create_sink: NativeViewportCreateSink,
    effects: NativeEffectCycle,
    pending_restored_viewports: BTreeSet<ViewportId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RestoredViewportPublication {
    AwaitingMaterialization,
    HiddenBootstrap,
    RoutedLive,
}

const fn restored_viewport_publication(
    routed: bool,
    materialized: bool,
) -> RestoredViewportPublication {
    if routed {
        RestoredViewportPublication::RoutedLive
    } else if materialized {
        RestoredViewportPublication::HiddenBootstrap
    } else {
        RestoredViewportPublication::AwaitingMaterialization
    }
}

struct PreparedNativeCycle {
    frame: PreparedEguiOuterFrameCommit,
    presentations: PreparedNativePresentationBatch,
    routes: BTreeMap<ViewportId, BoundNativeRoute>,
    effects: NativeEffectCycle,
    pending_restored_viewports: BTreeSet<ViewportId>,
}

/// Fork-backed application driver for one dockspace and its pane registry.
///
/// The app selects eframe's transactional hosted-cycle mode. All native input
/// is reduced before any pane callback, and all outputs are sealed before the
/// renderer receives them.
pub struct NativeDockspaceApp<P> {
    dockspace: Dockspace,
    panes: P,
    catalog: NativeViewportRoster,
    ingress: NativeIngressBridge,
    presentations: NativePresentationLedger,
    pending_restored_viewports: BTreeSet<ViewportId>,
    active: Option<ActiveNativeCycle>,
    prepared: Option<PreparedNativeCycle>,
    ingress_transaction: Option<NativeIngressTransaction>,
    next_cycle: u64,
    status: NativeRuntimeStatus,
    close_handler: Box<dyn NativeCloseHandler>,
    queued_close_tokens: BTreeSet<CloseDecisionToken>,
    queued_deferred_tokens: BTreeSet<DeferredCloseToken>,
    pending_configuration: NativeConfigurationQueue,
    last_persistence_error: Option<String>,
}

impl<P: PaneView> NativeDockspaceApp<P> {
    /// Creates a native runtime around an existing semantic dockspace.
    ///
    /// The viewport roster is a bootstrap catalog. Only its root entry may be
    /// enrolled before core emits a child-window creation request.
    pub fn new(
        dockspace: Dockspace,
        panes: P,
        catalog: NativeViewportRoster,
    ) -> Result<Self, NativeRuntimeError> {
        catalog.validate_workspace(dockspace.engine().workspace())?;
        let pending_restored_viewports = catalog
            .restored_children()
            .map(|spec| spec.viewport())
            .collect();
        Ok(Self {
            dockspace,
            panes,
            catalog,
            ingress: NativeIngressBridge::new()?,
            presentations: NativePresentationLedger::new()?,
            pending_restored_viewports,
            active: None,
            prepared: None,
            ingress_transaction: None,
            next_cycle: 0,
            status: NativeRuntimeStatus::default(),
            close_handler: Box::new(VetoNativeClose),
            queued_close_tokens: BTreeSet::new(),
            queued_deferred_tokens: BTreeSet::new(),
            pending_configuration: NativeConfigurationQueue::default(),
            last_persistence_error: None,
        })
    }

    /// Installs the application policy for native child close plans.
    #[must_use]
    pub fn with_close_handler(mut self, handler: impl NativeCloseHandler + 'static) -> Self {
        self.close_handler = Box::new(handler);
        self
    }

    /// Returns the latest committed runtime counters.
    #[must_use]
    pub const fn status(&self) -> NativeRuntimeStatus {
        self.status
    }

    /// Returns read-only access to the semantic dockspace.
    #[must_use]
    pub const fn dockspace(&self) -> &Dockspace {
        &self.dockspace
    }

    /// Returns the most recent background persistence failure, if any.
    ///
    /// Explicit calls to [`Self::save_document_to_storage`] return their error
    /// directly. This diagnostic records failures from eframe's infallible
    /// [`eframe::App::save`] callback.
    #[must_use]
    pub fn last_persistence_error(&self) -> Option<&str> {
        self.last_persistence_error.as_deref()
    }

    /// Captures the complete atomic document into eframe storage.
    ///
    /// An unbound dockspace returns `Ok(false)` without modifying storage. A
    /// capture failure also leaves the previous valid value untouched.
    ///
    /// # Errors
    ///
    /// Returns a cycle-state or strict document-capture failure.
    pub fn save_document_to_storage(
        &mut self,
        storage: &mut dyn eframe::Storage,
    ) -> Result<bool, NativeRuntimeError> {
        if self.cycle_in_flight() {
            return Err(NativeRuntimeError::CycleAlreadyActive);
        }
        if self.dockspace.document_id().is_none() {
            self.last_persistence_error = None;
            return Ok(false);
        }
        let json = self.dockspace.save_document_json()?;
        storage.set_string(NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY, json);
        self.last_persistence_error = None;
        Ok(true)
    }

    /// Returns mutable access to the application pane registry while no hosted
    /// cycle is active.
    pub fn panes_mut(&mut self) -> Result<&mut P, NativeRuntimeError> {
        if self.cycle_in_flight() {
            return Err(NativeRuntimeError::CycleAlreadyActive);
        }
        Ok(&mut self.panes)
    }

    /// Replaces docking policy at the next complete hosted-cycle boundary.
    ///
    /// Repeated calls before that boundary use last-wins semantics. A failed
    /// hosted cycle retains the candidate for an exact retry.
    pub fn set_policy(&mut self, policy: DockPolicy) -> Result<(), NativeRuntimeError> {
        if self.cycle_in_flight() {
            return Err(NativeRuntimeError::CycleAlreadyActive);
        }
        self.pending_configuration.replace_policy(policy);
        Ok(())
    }

    /// Replaces semantic geometry and egui style at the next complete
    /// hosted-cycle boundary.
    ///
    /// Validation occurs before the candidate replaces any prior valid style.
    /// A failed hosted cycle retains the candidate for an exact retry.
    pub fn set_style(&mut self, style: DockStyle) -> Result<(), NativeRuntimeError> {
        if self.cycle_in_flight() {
            return Err(NativeRuntimeError::CycleAlreadyActive);
        }
        self.pending_configuration.replace_style(style)?;
        Ok(())
    }

    fn begin_cycle(&mut self, cycle: &HostedViewportCycle) -> Result<(), NativeRuntimeError> {
        if self.cycle_in_flight() {
            return Err(NativeRuntimeError::CycleAlreadyActive);
        }
        self.presentations
            .reclaim_committed_quiescence(&mut self.dockspace)?;
        let native_ingress = cycle
            .native_host_ingress()
            .ok_or(NativeRuntimeError::NativeIngressMissing)?;
        let effect_sink =
            cycle
                .native_effect_sink()
                .cloned()
                .ok_or(NativeRuntimeError::IngressUnavailable(
                    "transactional native effect sink is required",
                ))?;
        let create_sink = cycle.native_viewport_create_sink().cloned().ok_or(
            NativeRuntimeError::IngressUnavailable(
                "transactional native viewport-create sink is required",
            ),
        )?;

        let prepared = self.ingress.prepare_cycle(
            &mut self.dockspace,
            native_ingress,
            &self.catalog,
            &mut self.presentations,
        )?;
        let PreparedNativeIngress {
            batch,
            bindings,
            routes,
            pointer_edges,
            presentation_host,
            transaction,
        } = prepared;
        debug_assert!(self.ingress_transaction.is_none());
        self.ingress_transaction = Some(transaction);
        let pending_restored_viewports = self
            .pending_restored_viewports
            .iter()
            .copied()
            .filter(|viewport| {
                !routes.contains_key(viewport)
                    && !self.ingress.restored_viewport_is_terminal(*viewport)
            })
            .collect();
        self.next_cycle = self
            .next_cycle
            .checked_add(1)
            .ok_or(NativeRuntimeError::IdentityExhausted)?;
        let mut input = self
            .dockspace
            .begin_native_cycle(EguiFrameScheduleKey::new(self.next_cycle, 0), bindings)?;
        let mut progress = match input.submit_ingress(batch) {
            Ok(progress) => progress,
            Err(source) => return Err(core_ingress_error(&input, source)),
        };
        while progress == BackendIngressProgress::ReceiverReceiptsRequired {
            let receipts = pointer_receiver_receipts(&self.dockspace, &input, &pointer_edges)?;
            progress = match input.submit_pointer_receiver_receipts(receipts) {
                Ok(progress) => progress,
                Err(source) => return Err(core_ingress_error(&input, source)),
            };
        }

        let mut configuration = input.into_configuration()?;
        self.pending_configuration
            .apply(&mut configuration, &self.dockspace)?;
        let session = configuration.into_presentation()?;
        let expected_surfaces = session.expected_surfaces().collect();
        let staging_requests = session
            .native_staging_requests()
            .map(|request| request.binding())
            .collect::<BTreeSet<_>>();
        let mut staging = BTreeMap::new();
        for route in routes.values() {
            if staging_requests.contains(&route.core()) {
                let authorization = cycle
                    .take_native_staging_presentation(route.exact().viewport())
                    .map_err(|error| NativeRuntimeError::HostedProtocol(error.to_string()))?;
                staging.insert(route.exact().viewport(), authorization);
            }
        }
        self.active = Some(ActiveNativeCycle {
            session,
            presentation_host,
            routes,
            expected_surfaces,
            callbacks: BTreeSet::new(),
            presentation_callbacks: BTreeSet::new(),
            staging,
            staging_callbacks: BTreeSet::new(),
            effect_sink,
            create_sink,
            effects: self.ingress.begin_effect_cycle(),
            pending_restored_viewports,
        });
        Ok(())
    }

    const fn cycle_in_flight(&self) -> bool {
        self.active.is_some() || self.prepared.is_some() || self.ingress_transaction.is_some()
    }

    fn run_viewport(
        &mut self,
        viewport: ViewportId,
        ui: &mut egui::Ui,
    ) -> Result<(), NativeRuntimeError> {
        let active = self
            .active
            .as_mut()
            .ok_or(NativeRuntimeError::CycleMissing)?;
        if !active.callbacks.insert(viewport) {
            return Err(NativeRuntimeError::DuplicateViewportCallback { viewport });
        }
        if viewport == ViewportId::ROOT {
            NativeIngressBridge::schedule_restored_viewports(
                &mut active.effects,
                &active.pending_restored_viewports,
                &active.routes,
                &self.catalog,
                &active.create_sink,
            )?;
            let mut declared = active
                .routes
                .values()
                .filter(|route| route.exact().viewport() != ViewportId::ROOT)
                .map(|route| (route.exact().viewport(), route.surface()))
                .collect::<BTreeMap<_, _>>();
            for viewport in &active.pending_restored_viewports {
                let publication = restored_viewport_publication(
                    active.routes.contains_key(viewport),
                    self.ingress.restored_viewport_is_materialized(*viewport),
                );
                if publication == RestoredViewportPublication::HiddenBootstrap {
                    let surface = self
                        .catalog
                        .get(*viewport)
                        .expect("pending restored viewport belongs to the validated catalog")
                        .surface();
                    declared.entry(*viewport).or_insert(surface);
                }
            }
            for (viewport, surface) in declared {
                let builder = if active.routes.contains_key(&viewport) {
                    self.catalog
                        .builder(viewport)
                        .unwrap_or_else(|| self.catalog.retained_builder(surface))
                } else {
                    self.catalog
                        .restored_staging_builder(viewport)
                        .expect("materialized restored viewport must retain its staging builder")
                };
                ui.ctx()
                    .show_viewport_deferred(viewport, builder, |_ui, _class| {});
            }
        }
        let Some(route) = active.routes.get(&viewport).copied() else {
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
            return Ok(());
        };
        if active.staging.contains_key(&viewport) {
            active
                .session
                .show_native_staging(&mut self.dockspace, route.exact(), ui)?;
            active.staging_callbacks.insert(viewport);
            active.presentation_callbacks.insert(viewport);
        } else if active.expected_surfaces.contains(&route.surface()) {
            active.session.show_native_surface(
                &mut self.dockspace,
                route.exact(),
                ui,
                &mut self.panes,
            )?;
            active.presentation_callbacks.insert(viewport);
        } else {
            ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
        }
        Ok(())
    }

    fn end_cycle(
        &mut self,
        context: &egui::Context,
        outputs: &mut [HostedViewportOutput<FullOutput>],
    ) -> Result<(), NativeRuntimeError> {
        let mut active = self.active.take().ok_or(NativeRuntimeError::CycleMissing)?;
        for output in outputs.iter() {
            let viewport = output.viewport_id();
            let Some(route) = active.routes.get(&viewport).copied() else {
                continue;
            };
            if !active.presentation_callbacks.contains(&viewport) {
                continue;
            }
            if active.staging_callbacks.contains(&viewport) {
                active.session.confirm_native_staging_output(
                    &mut self.dockspace,
                    route.exact(),
                    context,
                    output.output().clone(),
                )?;
            } else {
                active.session.confirm_native_surface_output(
                    &mut self.dockspace,
                    route.exact(),
                    context,
                    output.output().clone(),
                )?;
            }
        }

        let painted = active
            .routes
            .values()
            .filter(|route| {
                active
                    .presentation_callbacks
                    .contains(&route.exact().viewport())
            })
            .map(|route| route.surface())
            .collect::<BTreeSet<_>>();
        let missing = active
            .session
            .expected_surfaces()
            .filter(|surface| !painted.contains(surface))
            .collect::<Vec<_>>();
        for surface in missing {
            active.session.mark_surface_unavailable(
                &mut self.dockspace,
                surface,
                MeasurementUnavailableReason::Deferred,
            )?;
        }

        let output_routes = active
            .routes
            .values()
            .filter(|route| {
                active
                    .presentation_callbacks
                    .contains(&route.exact().viewport())
            })
            .copied()
            .collect::<Vec<_>>();
        let prepared_presentations = self.presentations.prepare_output_batch(
            active.presentation_host,
            output_routes,
            outputs,
            std::mem::take(&mut active.staging),
        )?;
        let frame = active.session.prepare_finish(&mut self.dockspace)?;
        if let Some(surface) = frame
            .transition()
            .reduced_inputs()
            .iter()
            .find_map(|input| match input.outcome() {
                InputOutcome::ViewportRegistrationRejected { surface } => Some(*surface),
                _ => None,
            })
        {
            frame.abort(&mut self.dockspace);
            return Err(NativeRuntimeError::ViewportRegistrationRejected { surface });
        }
        NativeIngressBridge::dispatch_effects(
            &mut active.effects,
            frame.transition().platform_effects(),
            &active.routes,
            &self.catalog,
            &active.effect_sink,
            &active.create_sink,
        );
        self.prepared = Some(PreparedNativeCycle {
            frame,
            presentations: prepared_presentations,
            routes: active.routes,
            effects: active.effects,
            pending_restored_viewports: active.pending_restored_viewports,
        });
        Ok(())
    }

    fn commit_cycle(
        &mut self,
        context: &egui::Context,
        outputs: &mut [HostedViewportOutput<FullOutput>],
    ) -> Result<(), NativeRuntimeError> {
        let prepared = self
            .prepared
            .take()
            .ok_or(NativeRuntimeError::CycleMissing)?;
        let commit = prepared.frame.commit(&mut self.dockspace)?;
        let (host, adapter_outputs) = commit.into_parts();
        let emitted_presentations = adapter_outputs
            .iter()
            .filter(|output| output.has_presentation_obligation())
            .count();
        self.pending_configuration.accept_commit();
        self.presentations
            .commit_output_batch(prepared.presentations, adapter_outputs, outputs);
        self.status.emitted_presentations = self
            .status
            .emitted_presentations
            .saturating_add(u64::try_from(emitted_presentations).unwrap_or(u64::MAX));

        self.ingress.commit_effect_cycle(prepared.effects);
        let transaction = self
            .ingress_transaction
            .take()
            .expect("a prepared native cycle retains its ingress transaction");
        self.ingress
            .commit_transaction(&self.presentations, transaction);
        self.pending_restored_viewports = prepared.pending_restored_viewports;
        self.queue_native_close_requests(host.transition());
        self.queue_native_close_decisions();
        for observation in host.transition().presentation_observations() {
            self.status.presentation_observations =
                self.status.presentation_observations.saturating_add(1);
            match observation {
                HostPresentationObservationOutcome::Rejected { .. } => {
                    self.status.presentation_rejections =
                        self.status.presentation_rejections.saturating_add(1);
                }
                HostPresentationObservationOutcome::Retired {
                    presented: Authority::Known(Some(_)),
                    promotion_eligible: true,
                    ..
                } => {
                    self.status.promoted_presentations =
                        self.status.promoted_presentations.saturating_add(1);
                }
                HostPresentationObservationOutcome::Retired {
                    presented: Authority::Known(Some(_)),
                    promotion_eligible: false,
                    ..
                } => {
                    self.status.ineligible_presentations =
                        self.status.ineligible_presentations.saturating_add(1);
                }
                HostPresentationObservationOutcome::NoUpdate { .. }
                | HostPresentationObservationOutcome::CapturedUnknown { .. }
                | HostPresentationObservationOutcome::Retired { .. } => {}
            }
        }
        self.presentations.accept_commit();
        self.status.committed_cycles = self.status.committed_cycles.saturating_add(1);
        self.status.live_viewports = prepared.routes.len();
        self.status.pending_presentations = self.presentations.pending_count();
        request_follow_up_cycle(
            context,
            self.ingress.has_post_commit_records()
                || self.presentations.has_committed_quiescence_work(),
        );
        Ok(())
    }

    fn abort_cycle(&mut self) {
        self.active.take();
        if let Some(prepared) = self.prepared.take() {
            prepared.frame.abort(&mut self.dockspace);
        }
        if let Some(transaction) = self.ingress_transaction.take() {
            self.ingress.rollback_transaction(
                &mut self.dockspace,
                &mut self.presentations,
                transaction,
            );
        }
    }

    fn queue_native_close_requests(&mut self, transition: &EngineTransition) {
        let edges = transition
            .reduced_inputs()
            .iter()
            .flat_map(|input| match input.outcome() {
                InputOutcome::PlatformSnapshotPublished {
                    native_close_edges, ..
                }
                | InputOutcome::NativeCloseObservationPublished {
                    native_close_edges, ..
                } => native_close_edges.as_slice(),
                _ => &[],
            })
            .copied()
            .collect::<Vec<NativeCloseEdge>>();
        for edge in edges {
            let request = self
                .close_handler
                .surface_request(NativeSurfaceCloseContext::new(
                    edge,
                    self.dockspace.engine().workspace(),
                    self.catalog.close_request(edge.binding().surface()),
                ));
            self.ingress
                .queue_surface_close_request(&self.dockspace, edge, request);
        }
    }

    fn queue_native_close_decisions(&mut self) {
        let initial = self
            .dockspace
            .engine()
            .active_close_plans()
            .filter(|plan| matches!(plan.target(), dockspace::ClosePlanTarget::Surface { .. }))
            .flat_map(|plan| {
                plan.items().iter().filter_map(move |item| {
                    (item.state() == CloseItemDecisionState::Pending).then(|| {
                        NativeCloseItemRequest::new(
                            plan.request(),
                            plan.target(),
                            item.item(),
                            item.capability(),
                            item.token(),
                        )
                    })
                })
            })
            .collect::<Vec<_>>();
        let current_initial = initial
            .iter()
            .map(|request| request.token())
            .collect::<BTreeSet<_>>();
        self.queued_close_tokens
            .retain(|token| current_initial.contains(token));
        for request in initial {
            if self.queued_close_tokens.contains(&request.token()) {
                continue;
            }
            let requested = self.close_handler.decide(request);
            let decision = if requested == dockspace::CloseDecision::Deferred
                && !request.capability().allows_deferred()
            {
                // A handler cannot widen the core-frozen close capability. A
                // malformed decision therefore fails closed without aborting
                // a host cycle whose semantic commit already succeeded.
                dockspace::CloseDecision::Veto
            } else {
                requested
            };
            self.ingress
                .queue_close_resolution(request.request(), request.token(), decision);
            self.queued_close_tokens.insert(request.token());
        }

        let deferred = self
            .dockspace
            .engine()
            .active_close_plans()
            .filter(|plan| matches!(plan.target(), dockspace::ClosePlanTarget::Surface { .. }))
            .flat_map(|plan| {
                plan.items().iter().filter_map(move |item| {
                    let CloseItemDecisionState::Deferred { continuation } = item.state() else {
                        return None;
                    };
                    Some(NativeDeferredCloseRequest::new(
                        plan.request(),
                        plan.target(),
                        item.item(),
                        continuation,
                    ))
                })
            })
            .collect::<Vec<_>>();
        let current_deferred = deferred
            .iter()
            .map(|request| request.token())
            .collect::<BTreeSet<_>>();
        self.queued_deferred_tokens
            .retain(|token| current_deferred.contains(token));
        for request in deferred {
            if self.queued_deferred_tokens.contains(&request.token()) {
                continue;
            }
            let Some(decision) = self.close_handler.resolve_deferred(request) else {
                continue;
            };
            self.ingress.queue_deferred_close_resolution(
                request.request(),
                request.token(),
                decision,
            );
            self.queued_deferred_tokens.insert(request.token());
        }
    }
}

fn core_ingress_error(
    input: &egui_dockspace::EguiNativeInputSession,
    source: egui_dockspace::DockspaceError,
) -> NativeRuntimeError {
    let detail = input
        .input_prefix_error()
        .map_or_else(|| source.to_string(), ToString::to_string);
    NativeRuntimeError::CoreIngressRejected { detail, source }
}

impl<P: PaneView> eframe::App for NativeDockspaceApp<P> {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Err(error) = self.save_document_to_storage(storage) {
            self.last_persistence_error = Some(error.to_string());
        }
    }

    fn hosted_viewport_mode(&self) -> HostedViewportMode {
        HostedViewportMode::Transactional
    }

    fn begin_hosted_viewport_cycle(
        &mut self,
        _context: &egui::Context,
        cycle: &HostedViewportCycle,
        _frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<()> {
        self.begin_cycle(cycle)
            .map_err(HostedHookError::from)
            .map_err(|error| Box::new(error) as eframe::HostedViewportAppError)
    }

    fn hosted_viewport_ui(
        &mut self,
        viewport: ViewportId,
        ui: &mut egui::Ui,
        _frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<HostedViewportUiDisposition> {
        self.run_viewport(viewport, ui)
            .map_err(HostedHookError::from)
            .map_err(|error| Box::new(error) as eframe::HostedViewportAppError)?;
        Ok(HostedViewportUiDisposition::Handled)
    }

    fn end_hosted_viewport_cycle(
        &mut self,
        context: &egui::Context,
        outputs: &mut [HostedViewportOutput<FullOutput>],
        _frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<()> {
        self.end_cycle(context, outputs)
            .map_err(HostedHookError::from)
            .map_err(|error| Box::new(error) as eframe::HostedViewportAppError)
    }

    fn commit_hosted_viewport_cycle(
        &mut self,
        context: &egui::Context,
        outputs: &mut [HostedViewportOutput<FullOutput>],
        _frame: &mut eframe::Frame,
    ) -> eframe::HostedViewportAppResult<()> {
        self.commit_cycle(context, outputs)
            .map_err(HostedHookError::from)
            .map_err(|error| Box::new(error) as eframe::HostedViewportAppError)
    }

    fn abort_hosted_viewport_cycle(
        &mut self,
        _context: &egui::Context,
        _frame: &mut eframe::Frame,
    ) {
        self.abort_cycle();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ui.allocate_rect(ui.available_rect_before_wrap(), egui::Sense::hover());
    }
}

fn request_follow_up_cycle(context: &egui::Context, has_post_commit_work: bool) {
    if has_post_commit_work {
        context.request_repaint_of(ViewportId::ROOT);
    }
}

fn _assert_exact_native_is_copy(_: ExactNativeViewport) {}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn post_commit_work_requests_exactly_one_root_follow_up_cycle() {
        let context = egui::Context::default();
        let requests = Arc::new(AtomicUsize::new(0));
        context.set_request_repaint_callback({
            let requests = Arc::clone(&requests);
            move |request| {
                if request.viewport_id == ViewportId::ROOT {
                    requests.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        request_follow_up_cycle(&context, false);
        assert_eq!(requests.load(Ordering::Relaxed), 0);

        request_follow_up_cycle(&context, true);
        assert_eq!(requests.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn restored_viewport_stays_undeclared_until_materialized_then_hidden_until_routed() {
        assert_eq!(
            restored_viewport_publication(false, false),
            RestoredViewportPublication::AwaitingMaterialization
        );
        assert_eq!(
            restored_viewport_publication(false, true),
            RestoredViewportPublication::HiddenBootstrap
        );
        assert_eq!(
            restored_viewport_publication(true, true),
            RestoredViewportPublication::RoutedLive
        );
    }
}
