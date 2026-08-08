//! Event-loop-ordered renderer result correlation.

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicU64, Ordering};

use dockspace::backend::presentation_observation::{
    HostFrameKey, HostPresentationContinuation, HostPresentationOutput, HostPresentationStreamId,
    PresentationHostLease, SurfacePresentationOutputTicket,
};
use eframe::{
    HostedNativeStagingPresentation, HostedPresentationFollowUp, HostedViewportOutput,
    NativeHoveredWindow, NativePointerDeliveryOwner, NativePointerEdge, NativePresentationResult,
    NativeViewportBinding,
};
use egui::{FullOutput, PaintOutcome, PointerHitGraphSnapshot, UserData};
use egui_dockspace::Dockspace;
use egui_dockspace::backend::{
    EguiOuterSurfaceOutput, EguiPresentationResult, EguiPresentationSettlement,
    ExactNativeViewport, NativeViewportIncarnation,
};

use crate::NativeRuntimeError;
use crate::ingress::BoundNativeRoute;

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Copy, Debug)]
struct NativePresentationToken {
    runtime: u64,
    serial: u64,
    native: ExactNativeViewport,
}

struct PendingNativePresentation {
    native: ExactNativeViewport,
    output: Option<HostPresentationOutput>,
    settlement: EguiPresentationSettlement,
}

struct PreparedNativePresentationOutput {
    native: ExactNativeViewport,
    hosted_index: usize,
    serial: u64,
    staging: Option<HostedNativeStagingPresentation>,
}

/// Fully validated native output routing awaiting an infallible post-core apply.
#[must_use = "dropping a prepared native output batch aborts publication"]
pub(crate) struct PreparedNativePresentationBatch {
    runtime: u64,
    base_serial: u64,
    next_serial: u64,
    presentation_host: PresentationHostLease,
    outputs: BTreeMap<dockspace::ids::SurfaceId, PreparedNativePresentationOutput>,
}

#[derive(Clone)]
struct SettledNativePresentation {
    native: ExactNativeViewport,
    presented_graph: Option<PresentedNativePointerGraph>,
}

/// Affine boundary for fallible native-ingress preparation.
///
/// Renderer settlement is a terminal physical fact and is intentionally not
/// rolled back. Destructive quiescence cleanup is staged separately so a
/// failed prepare cannot discard the replay evidence for that fact.
#[must_use = "a native presentation prepare savepoint must be committed or rolled back"]
pub(crate) struct NativePresentationPrepareSavepoint {
    runtime: u64,
    staged_quiescence_len: usize,
}

#[derive(Clone)]
pub(crate) struct PresentedNativePointerGraph {
    native: ExactNativeViewport,
    scene: SurfacePresentationOutputTicket,
    coordinate_generation: dockspace::viewport::CoordinateGeneration,
    emission: HostFrameKey,
    graph: PointerHitGraphSnapshot,
}

impl PresentedNativePointerGraph {
    pub(crate) const fn native(&self) -> ExactNativeViewport {
        self.native
    }

    pub(crate) const fn scene(&self) -> SurfacePresentationOutputTicket {
        self.scene
    }

    pub(crate) const fn coordinate_generation(&self) -> dockspace::viewport::CoordinateGeneration {
        self.coordinate_generation
    }

    pub(crate) const fn emission(&self) -> HostFrameKey {
        self.emission
    }

    pub(crate) const fn graph(&self) -> &PointerHitGraphSnapshot {
        &self.graph
    }
}

#[derive(Clone, Default)]
pub(crate) struct EdgePointerGraphs {
    delivery: Option<PresentedNativePointerGraph>,
    hover: Option<PresentedNativePointerGraph>,
}

impl EdgePointerGraphs {
    pub(crate) const fn delivery(&self) -> Option<&PresentedNativePointerGraph> {
        self.delivery.as_ref()
    }

    pub(crate) const fn hover(&self) -> Option<&PresentedNativePointerGraph> {
        self.hover.as_ref()
    }
}

/// Event-loop-local owner of every affine adapter presentation capability.
pub(crate) struct NativePresentationLedger {
    runtime: u64,
    next_serial: u64,
    committed_serial: u64,
    pending: BTreeMap<u64, PendingNativePresentation>,
    settled: BTreeMap<u64, SettledNativePresentation>,
    retired_bindings: BTreeSet<ExactNativeViewport>,
    presented_graphs: BTreeMap<ExactNativeViewport, PresentedNativePointerGraph>,
    live_streams:
        BTreeMap<ExactNativeViewport, BTreeSet<(PresentationHostLease, HostPresentationStreamId)>>,
    retired_streams:
        BTreeMap<ExactNativeViewport, BTreeSet<(PresentationHostLease, HostPresentationStreamId)>>,
    staged_quiescence: Vec<ExactNativeViewport>,
    committed_quiescence: BTreeSet<ExactNativeViewport>,
}

impl NativePresentationLedger {
    pub(crate) fn new() -> Result<Self, NativeRuntimeError> {
        let runtime = NEXT_RUNTIME_ID
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |current| {
                current.checked_add(1)
            })
            .map_err(|_| NativeRuntimeError::IdentityExhausted)?;
        Ok(Self {
            runtime,
            next_serial: 0,
            committed_serial: 0,
            pending: BTreeMap::new(),
            settled: BTreeMap::new(),
            retired_bindings: BTreeSet::new(),
            presented_graphs: BTreeMap::new(),
            live_streams: BTreeMap::new(),
            retired_streams: BTreeMap::new(),
            staged_quiescence: Vec::new(),
            committed_quiescence: BTreeSet::new(),
        })
    }

    pub(crate) fn prepare_savepoint(&self) -> NativePresentationPrepareSavepoint {
        NativePresentationPrepareSavepoint {
            runtime: self.runtime,
            staged_quiescence_len: self.staged_quiescence.len(),
        }
    }

    pub(crate) fn rollback_prepare(&mut self, savepoint: NativePresentationPrepareSavepoint) {
        assert_eq!(savepoint.runtime, self.runtime);
        assert!(savepoint.staged_quiescence_len <= self.staged_quiescence.len());
        self.staged_quiescence
            .truncate(savepoint.staged_quiescence_len);
    }

    pub(crate) fn commit_prepare(&self, savepoint: NativePresentationPrepareSavepoint) {
        assert_eq!(savepoint.runtime, self.runtime);
        assert!(savepoint.staged_quiescence_len <= self.staged_quiescence.len());
    }

    pub(crate) fn prepare_output_batch(
        &self,
        presentation_host: PresentationHostLease,
        routes: impl IntoIterator<Item = BoundNativeRoute>,
        hosted_outputs: &[HostedViewportOutput<FullOutput>],
        mut staging: BTreeMap<egui::ViewportId, HostedNativeStagingPresentation>,
    ) -> Result<PreparedNativePresentationBatch, NativeRuntimeError> {
        let mut next_serial = self.next_serial;
        let mut prepared = BTreeMap::new();
        for route in routes {
            let viewport = route.exact().viewport();
            let hosted_index = hosted_outputs
                .iter()
                .position(|output| output.viewport_id() == viewport)
                .ok_or(NativeRuntimeError::AdapterOutputMissing { viewport })?;
            let hosted = &hosted_outputs[hosted_index];
            if hosted.native_binding() != Some(route.native_binding()) {
                return Err(NativeRuntimeError::HostedOutputBindingMismatch { viewport });
            }
            if hosted.output().platform_output.presentation_token.is_some() {
                return Err(NativeRuntimeError::PresentationTokenOccupied { viewport });
            }
            let authorization = staging.remove(&viewport);
            if let Some(authorization) = authorization.as_ref() {
                hosted
                    .validate_native_staging_presentation(authorization)
                    .map_err(|error| NativeRuntimeError::HostedProtocol(error.to_string()))?;
            }
            next_serial = next_serial
                .checked_add(1)
                .ok_or(NativeRuntimeError::IdentityExhausted)?;
            let previous = prepared.insert(
                route.surface(),
                PreparedNativePresentationOutput {
                    native: route.exact(),
                    hosted_index,
                    serial: next_serial,
                    staging: authorization,
                },
            );
            if previous.is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "one native output batch repeated a logical surface",
                ));
            }
        }
        Ok(PreparedNativePresentationBatch {
            runtime: self.runtime,
            base_serial: self.next_serial,
            next_serial,
            presentation_host,
            outputs: prepared,
        })
    }

    /// Applies an already validated output batch after the core commit.
    ///
    /// Every assertion is protected by the affine batch and by the adapter's
    /// complete-roster finish contract; there is no recoverable failure after
    /// semantic ownership publishes.
    pub(crate) fn commit_output_batch(
        &mut self,
        mut prepared: PreparedNativePresentationBatch,
        adapter_outputs: Vec<EguiOuterSurfaceOutput>,
        hosted_outputs: &mut [HostedViewportOutput<FullOutput>],
    ) {
        assert_eq!(prepared.runtime, self.runtime);
        assert_eq!(prepared.base_serial, self.next_serial);
        assert_eq!(prepared.outputs.len(), adapter_outputs.len());
        for adapter_output in adapter_outputs {
            let (surface, mut output, settlement) = adapter_output.into_parts();
            let output_plan = prepared
                .outputs
                .remove(&surface)
                .expect("the prepared native batch covers every adapter output");
            assert_eq!(settlement.native_binding(), Some(output_plan.native));
            let hosted = hosted_outputs
                .get_mut(output_plan.hosted_index)
                .expect("the prepared hosted-output index remains in the frozen roster");
            assert_eq!(hosted.viewport_id(), output_plan.native.viewport());
            assert_eq!(
                hosted.native_binding().map(exact_native),
                Some(output_plan.native)
            );
            assert!(output.platform_output.presentation_token.is_none());

            let presentation_output = settlement.presentation_output();
            debug_assert_eq!(settlement.is_required(), presentation_output.is_some());
            let follow_up = presentation_output.and_then(|output| match output.continuation() {
                HostPresentationContinuation::None => None,
                HostPresentationContinuation::Presented => {
                    Some(HostedPresentationFollowUp::SuccessfulSubmission)
                }
                HostPresentationContinuation::Terminal => {
                    Some(HostedPresentationFollowUp::AnyResult)
                }
            });
            if let Some(presentation_output) = presentation_output {
                let token = NativePresentationToken {
                    runtime: self.runtime,
                    serial: output_plan.serial,
                    native: output_plan.native,
                };
                output.platform_output.presentation_token = Some(UserData::new(token));
                self.live_streams
                    .entry(output_plan.native)
                    .or_default()
                    .insert((prepared.presentation_host, presentation_output.stream()));
                let pending = PendingNativePresentation {
                    native: output_plan.native,
                    output: Some(presentation_output),
                    settlement,
                };
                assert!(self.pending.insert(output_plan.serial, pending).is_none());
            }
            *hosted.output_mut() = output;
            if let Some(follow_up) = follow_up {
                hosted
                    .require_presentation_result_follow_up(follow_up)
                    .expect("a committed presentation obligation retains its renderer token");
            }
            if let Some(authorization) = output_plan.staging {
                hosted
                    .authorize_native_staging_presentation(authorization)
                    .expect("a prevalidated hidden-staging authorization remains valid");
            }
        }
        assert!(prepared.outputs.is_empty());
        self.next_serial = prepared.next_serial;
    }

    pub(crate) fn consume_ordered(
        &mut self,
        dockspace: &mut Dockspace,
        recorder: &mut dockspace::backend::ingress::BackendIngressRecorder,
        native_result: &NativePresentationResult,
    ) -> Result<(), NativeRuntimeError> {
        let result = native_result.result();
        let Some(token) = result.token().downcast_ref::<NativePresentationToken>() else {
            return Ok(());
        };
        if token.runtime != self.runtime {
            return Ok(());
        }
        let submitted = native_result.binding();
        let submitted_exact = ExactNativeViewport::new(
            submitted.viewport_id(),
            NativeViewportIncarnation::new(submitted.incarnation().get()),
        );
        if let Some(settled) = self.settled.get(&token.serial) {
            if submitted_exact != settled.native
                || result.viewport_id() != settled.native.viewport()
                || token.native != settled.native
            {
                return Err(NativeRuntimeError::PresentationViewportMismatch {
                    serial: token.serial,
                    expected: settled.native.viewport(),
                    submitted: result.viewport_id(),
                });
            }
            if let Some(graph) = settled.presented_graph.clone() {
                self.presented_graphs.insert(settled.native, graph);
            }
            let _ = dockspace.record_ready_backend_presentation_observations(recorder)?;
            return Ok(());
        }
        let Some(pending) = self.pending.get(&token.serial) else {
            if token.serial <= self.committed_serial
                && submitted_exact == token.native
                && result.viewport_id() == token.native.viewport()
            {
                return Ok(());
            }
            if self.retired_bindings.contains(&token.native) {
                if submitted_exact == token.native
                    && result.viewport_id() == token.native.viewport()
                {
                    return Ok(());
                }
                return Err(NativeRuntimeError::PresentationViewportMismatch {
                    serial: token.serial,
                    expected: token.native.viewport(),
                    submitted: result.viewport_id(),
                });
            }
            return Err(NativeRuntimeError::UnknownPresentationToken {
                serial: token.serial,
            });
        };
        if submitted_exact != pending.native
            || result.viewport_id() != pending.native.viewport()
            || token.native != pending.native
        {
            return Err(NativeRuntimeError::PresentationViewportMismatch {
                serial: token.serial,
                expected: pending.native.viewport(),
                submitted: result.viewport_id(),
            });
        }
        let pending = self
            .pending
            .remove(&token.serial)
            .expect("the validated pending token remains present until it is consumed");
        let adapter_result = native_presentation_result(result.outcome());
        let presented_graph = match (adapter_result, pending.output) {
            (EguiPresentationResult::Presented, Some(output)) => result
                .presented_pointer_hit_graph()
                .cloned()
                .and_then(|graph| {
                    output
                        .payload()
                        .scene()
                        .zip(output.payload().coordinate_generation())
                        .map(
                            |(scene, coordinate_generation)| PresentedNativePointerGraph {
                                native: pending.native,
                                scene,
                                coordinate_generation,
                                emission: output.key(),
                                graph,
                            },
                        )
                }),
            (EguiPresentationResult::Presented | EguiPresentationResult::Dropped, None)
            | (EguiPresentationResult::Dropped, Some(_)) => None,
        };
        pending.settlement.settle(adapter_result);
        self.settled.insert(
            token.serial,
            SettledNativePresentation {
                native: pending.native,
                presented_graph: presented_graph.clone(),
            },
        );
        let _ = dockspace.record_ready_backend_presentation_observations(recorder)?;
        if let Some(graph) = presented_graph {
            self.presented_graphs.insert(pending.native, graph);
        }
        Ok(())
    }

    pub(crate) fn capture_edge(&self, edge: &NativePointerEdge) -> EdgePointerGraphs {
        let delivery = match edge.delivery_owner().value() {
            Some(NativePointerDeliveryOwner::Viewport(binding)) => {
                self.graph_for_native(*binding).cloned()
            }
            Some(NativePointerDeliveryOwner::Foreign | NativePointerDeliveryOwner::None) | None => {
                None
            }
        };
        let hover = match edge.hovered().value() {
            Some(NativeHoveredWindow::Viewport(binding)) => {
                self.graph_for_native(*binding).cloned()
            }
            Some(NativeHoveredWindow::Foreign | NativeHoveredWindow::None) | None => None,
        };
        EdgePointerGraphs { delivery, hover }
    }

    pub(crate) fn semantic_graph(
        &self,
        binding: NativeViewportBinding,
        graph: &PointerHitGraphSnapshot,
    ) -> Option<&PresentedNativePointerGraph> {
        let presented = self.graph_for_native(binding)?;
        (presented.graph() == graph).then_some(presented)
    }

    pub(crate) fn retire(
        &mut self,
        dockspace: &mut Dockspace,
        recorder: &mut dockspace::backend::ingress::BackendIngressRecorder,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        self.presented_graphs.remove(&exact);
        self.retired_bindings.insert(exact);
        if let Some(streams) = self.live_streams.remove(&exact) {
            self.retired_streams
                .entry(exact)
                .or_default()
                .extend(streams);
        }

        let pending = self
            .pending
            .keys()
            .copied()
            .filter(|serial| {
                self.pending
                    .get(serial)
                    .is_some_and(|pending| pending.native == exact)
            })
            .collect::<Vec<_>>();
        for serial in pending {
            let pending = self
                .pending
                .remove(&serial)
                .expect("a pending native presentation remains present until retirement");
            pending.settlement.settle(EguiPresentationResult::Dropped);
            self.settled.insert(
                serial,
                SettledNativePresentation {
                    native: exact,
                    presented_graph: None,
                },
            );
        }
        let _ = dockspace.record_ready_backend_presentation_observations(recorder)?;
        Ok(())
    }

    /// Stages the fork proof that no renderer result can still name this exact native lifetime.
    ///
    /// Core stream reclamation remains a separate authority boundary. The tombstone is retained
    /// until a later cycle confirms every stream archived for this binding.
    pub(crate) fn retirement_quiesced(&mut self, exact: ExactNativeViewport) {
        debug_assert!(
            self.pending.values().all(|pending| pending.native != exact),
            "native retirement settles every adapter presentation before quiescence"
        );
        if self.retired_bindings.contains(&exact) && !self.staged_quiescence.contains(&exact) {
            self.staged_quiescence.push(exact);
        }
    }

    pub(crate) fn accept_commit(&mut self) {
        if let Some(serial) = self.settled.keys().next_back().copied() {
            self.committed_serial = self.committed_serial.max(serial);
        }
        self.settled.clear();
        for exact in self.staged_quiescence.drain(..) {
            self.committed_quiescence.insert(exact);
        }
    }

    /// Reclaims core and adapter state for fork-confirmed native retirements.
    ///
    /// This runs before the next hosted transaction begins. A temporarily retained core stream
    /// keeps its exact native proof and requests another cycle; only complete reclamation removes
    /// the native tombstone.
    pub(crate) fn reclaim_committed_quiescence(
        &mut self,
        dockspace: &mut Dockspace,
    ) -> Result<(), NativeRuntimeError> {
        let candidates = self
            .committed_quiescence
            .iter()
            .copied()
            .collect::<Vec<_>>();
        for exact in candidates {
            let streams = self
                .retired_streams
                .get(&exact)
                .cloned()
                .unwrap_or_default();
            if !dockspace.adapter_reclaim_quiesced_presentation_streams(&streams)? {
                continue;
            }
            self.retired_streams.remove(&exact);
            self.retired_bindings.remove(&exact);
            self.committed_quiescence.remove(&exact);
        }
        Ok(())
    }

    pub(crate) fn has_committed_quiescence_work(&self) -> bool {
        !self.committed_quiescence.is_empty()
    }

    fn graph_for_native(
        &self,
        binding: NativeViewportBinding,
    ) -> Option<&PresentedNativePointerGraph> {
        let exact = ExactNativeViewport::new(
            binding.viewport_id(),
            NativeViewportIncarnation::new(binding.incarnation().get()),
        );
        self.presented_graphs.get(&exact)
    }

    pub(crate) fn pending_count(&self) -> usize {
        self.pending.len()
    }

    #[cfg(test)]
    pub(crate) fn retired_binding_count(&self) -> usize {
        self.retired_bindings.len()
    }
}

fn exact_native(binding: NativeViewportBinding) -> ExactNativeViewport {
    ExactNativeViewport::new(
        binding.viewport_id(),
        NativeViewportIncarnation::new(binding.incarnation().get()),
    )
}

fn native_presentation_result(outcome: &PaintOutcome) -> EguiPresentationResult {
    match outcome {
        PaintOutcome::SubmittedToSwapchain | PaintOutcome::Swapped => {
            EguiPresentationResult::Presented
        }
        PaintOutcome::SubmittedToBrowserCanvas
        | PaintOutcome::Skipped(_)
        | PaintOutcome::Failed(_) => EguiPresentationResult::Dropped,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId, SurfaceId};
    fn test_dockspace() -> Dockspace {
        let surface = SurfaceId::new(1);
        let root = RootId::new(1);
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(root, RootRecord::new(tabs));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        Dockspace::builder(
            "native-presentation-retention",
            builder.build().expect("the test workspace is valid"),
        )
        .build()
        .expect("the test dockspace is valid")
    }

    #[test]
    fn browser_canvas_submission_cannot_authorize_native_presentation() {
        assert_eq!(
            native_presentation_result(&PaintOutcome::SubmittedToBrowserCanvas),
            EguiPresentationResult::Dropped
        );
        assert_eq!(
            native_presentation_result(&PaintOutcome::SubmittedToSwapchain),
            EguiPresentationResult::Presented
        );
        assert_eq!(
            native_presentation_result(&PaintOutcome::Swapped),
            EguiPresentationResult::Presented
        );
    }

    #[test]
    fn exact_quiescence_releases_only_the_named_retirement_after_core_reclamation() {
        let first = ExactNativeViewport::new(
            egui::ViewportId::from_hash_of("first-retired-viewport"),
            NativeViewportIncarnation::new(1),
        );
        let second = ExactNativeViewport::new(
            egui::ViewportId::from_hash_of("second-retired-viewport"),
            NativeViewportIncarnation::new(2),
        );
        let mut ledger = NativePresentationLedger::new().expect("runtime identity is available");
        ledger.retired_bindings.insert(first);
        ledger.retired_bindings.insert(second);

        ledger.retirement_quiesced(first);
        ledger.retirement_quiesced(first);
        assert_eq!(ledger.retired_binding_count(), 2);
        ledger.accept_commit();
        assert_eq!(ledger.retired_binding_count(), 2);
        assert!(ledger.has_committed_quiescence_work());

        ledger
            .reclaim_committed_quiescence(&mut test_dockspace())
            .expect("a binding with no core stream archive reclaims immediately");
        assert_eq!(ledger.retired_binding_count(), 1);
        assert!(ledger.retired_bindings.contains(&second));
        assert!(!ledger.has_committed_quiescence_work());
    }

    #[test]
    fn failed_prepare_preserves_quiesced_retirement_replay_evidence() {
        let exact = ExactNativeViewport::new(
            egui::ViewportId::from_hash_of("failed-prepare-retired-viewport"),
            NativeViewportIncarnation::new(1),
        );
        let mut ledger = NativePresentationLedger::new().expect("runtime identity is available");
        ledger.retired_bindings.insert(exact);
        ledger.settled.insert(
            7,
            SettledNativePresentation {
                native: exact,
                presented_graph: None,
            },
        );

        let savepoint = ledger.prepare_savepoint();
        ledger.retirement_quiesced(exact);
        ledger.rollback_prepare(savepoint);

        assert!(ledger.retired_bindings.contains(&exact));
        assert!(ledger.settled.contains_key(&7));
        assert!(ledger.staged_quiescence.is_empty());
        assert!(ledger.committed_quiescence.is_empty());
    }
}
