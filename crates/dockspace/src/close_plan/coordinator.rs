use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;
use std::sync::Arc;

use thiserror::Error;

use super::*;

/// Failure to create a plan or mint a required protocol identity.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub(crate) enum CloseCoordinatorError {
    #[error("close authority domain mismatch: expected {expected:?}, got {actual:?}")]
    AuthorityDomainMismatch {
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    #[error("native close edge domain mismatch: expected {expected:?}, got {actual:?}")]
    NativeEdgeDomainMismatch {
        expected: EngineAuthorityDomainId,
        actual: EngineAuthorityDomainId,
    },
    #[error("surface close plan requires an exact native close edge")]
    SurfaceRequiresNativeEdge,
    #[error("close request identity space exhausted")]
    RequestIdExhausted,
    #[error("close decision token identity space exhausted")]
    DecisionTokenExhausted,
    #[error("deferred close token identity space exhausted")]
    DeferredTokenExhausted,
    #[error("close plan contains duplicate item {item}")]
    DuplicateItem { item: ItemId },
    #[error("close plan contains disabled item {item}")]
    DisabledItem { item: ItemId },
    #[error("item close target {target} requires exactly that item, got {actual:?}")]
    ItemTargetMismatch { target: ItemId, actual: Vec<ItemId> },
    #[error("root close target {root} requires a non-empty item decision roster")]
    EmptyRootHasNoItemDecisions { root: RootId },
    #[error("surface close-content target {surface} requires a non-empty item decision roster")]
    SurfaceCloseContentHasNoItemDecisions { surface: SurfaceId },
    #[error("surface rehome target {surface} cannot request {item_count} pane-close decisions")]
    SurfaceRehomeHasItemDecisions {
        surface: SurfaceId,
        item_count: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenOwner {
    request: CloseRequestId,
    item_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeCloseCausality {
    binding: ViewportBinding,
    edge_observed_at: CloseObservationGeneration,
    edge_received_at: InventoryGeneration,
    close: Option<NativeEffectCausality>,
    cancellation: Option<NativeEffectCausality>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct NativeEffectCausality {
    effect: EffectId,
    issued_after: CloseObservationGeneration,
    received_after: InventoryGeneration,
    after_effect: Option<EffectId>,
}

/// Core-private pair of a public plan and its exact checked command material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PreparedClose<P> {
    plan: ClosePlan,
    prepared: P,
    native: Option<NativeCloseCausality>,
    terminal_published: bool,
}

impl<P> PreparedClose<P> {
    const fn plan(&self) -> &ClosePlan {
        &self.plan
    }

    const fn prepared(&self) -> &P {
        &self.prepared
    }

    fn refresh_resolution_phase(&mut self) {
        self.plan.phase = if self
            .plan
            .items
            .iter()
            .all(|item| item.state == CloseItemDecisionState::Allowed)
        {
            ClosePlanPhase::Approved
        } else if self
            .plan
            .items
            .iter()
            .any(|item| matches!(item.state, CloseItemDecisionState::Deferred { .. }))
        {
            ClosePlanPhase::Deferred
        } else if self
            .plan
            .items
            .iter()
            .any(|item| item.state == CloseItemDecisionState::Allowed)
        {
            ClosePlanPhase::Resolving
        } else {
            ClosePlanPhase::Requested
        };
    }
}

/// Borrow proving that the last required vote allowed one exact prepared plan.
pub(crate) struct ApprovedClose<'a, P> {
    plan: &'a ClosePlan,
    prepared: &'a P,
}

impl<'a, P> ApprovedClose<'a, P> {
    pub(crate) const fn plan(&self) -> &'a ClosePlan {
        self.plan
    }

    pub(crate) const fn prepared(&self) -> &'a P {
        self.prepared
    }
}

/// Exactly-once close-plan coordinator with independent monotonic token spaces.
///
/// The coordinator never evaluates frames or elapsed time. Deferred and
/// indeterminate plans remain pending until an exact continuation, authority
/// change, cancellation, or platform fact resolves them.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct CloseCoordinator<P> {
    domain: EngineAuthorityDomainId,
    last_request: u64,
    last_decision: u64,
    last_deferred: u64,
    requests: BTreeMap<CloseRequestId, PreparedClose<P>>,
    decision_owners: BTreeMap<CloseDecisionToken, TokenOwner>,
    deferred_owners: BTreeMap<DeferredCloseToken, TokenOwner>,
}

impl<P> CloseCoordinator<P> {
    pub(crate) const fn new(domain: EngineAuthorityDomainId) -> Self {
        Self {
            domain,
            last_request: 0,
            last_decision: 0,
            last_deferred: 0,
            requests: BTreeMap::new(),
            decision_owners: BTreeMap::new(),
            deferred_owners: BTreeMap::new(),
        }
    }

    /// Accounts for every retained close plan and token ownership entry.
    ///
    /// Terminal plans stay detailed through their successful publication boundary. The next
    /// atomic candidate compacts their payload and copied-token indexes while monotonic identity
    /// frontiers continue to classify late replay.
    pub(crate) fn retention_manifest(&self) -> CloseRetentionManifest {
        let terminal_plan_guards = self
            .requests
            .values()
            .filter(|record| record.plan.is_terminal())
            .count();
        CloseRetentionManifest::new(
            self.requests.len() - terminal_plan_guards,
            terminal_plan_guards,
            self.decision_owners.len(),
            self.deferred_owners.len(),
        )
    }

    /// Freezes one close plan while preserving the supplied item order exactly.
    ///
    /// `prepared` remains core-private and may contain checked graph sources,
    /// complete surface rosters, recovery proofs, and focus dispositions.
    pub(crate) fn open(
        &mut self,
        authority: CloseAuthority,
        target: ClosePlanTarget,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
        prepared: P,
    ) -> Result<ClosePlan, CloseCoordinatorError> {
        if target.kind() == ClosePlanTargetKind::Surface {
            return Err(CloseCoordinatorError::SurfaceRequiresNativeEdge);
        }
        self.open_unbound(authority, target, requirements, prepared)
    }

    /// Atomically freezes a surface close plan with the exact native edge that
    /// created its external close obligation.
    pub(crate) fn open_surface(
        &mut self,
        authority: CloseAuthority,
        edge: NativeCloseEdge,
        request: SurfaceCloseRequest,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
        prepared: P,
    ) -> Result<ClosePlan, CloseCoordinatorError> {
        if edge.domain != self.domain {
            return Err(CloseCoordinatorError::NativeEdgeDomainMismatch {
                expected: self.domain,
                actual: edge.domain,
            });
        }

        let plan = self.open_unbound(
            authority,
            ClosePlanTarget::Surface {
                surface: edge.binding.surface(),
                disposition: request.disposition(),
            },
            requirements,
            prepared,
        )?;
        let record = self
            .requests
            .get_mut(&plan.request)
            .expect("a successfully opened close plan is retained");
        record.native = Some(NativeCloseCausality {
            binding: edge.binding,
            edge_observed_at: edge.observed_at,
            edge_received_at: edge.received_at,
            close: None,
            cancellation: None,
        });
        Ok(plan)
    }

    fn open_unbound(
        &mut self,
        authority: CloseAuthority,
        target: ClosePlanTarget,
        requirements: impl IntoIterator<Item = CloseItemRequirement>,
        prepared: P,
    ) -> Result<ClosePlan, CloseCoordinatorError> {
        if authority.domain() != self.domain {
            return Err(CloseCoordinatorError::AuthorityDomainMismatch {
                expected: self.domain,
                actual: authority.domain(),
            });
        }
        let requirements = requirements.into_iter().collect::<Vec<_>>();
        Self::validate_requirements(target, &requirements)?;

        let request_raw = self
            .last_request
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(CloseCoordinatorError::RequestIdExhausted)?;
        let decision_count = u64::try_from(requirements.len())
            .map_err(|_| CloseCoordinatorError::DecisionTokenExhausted)?;
        let decision_end = self
            .last_decision
            .checked_add(decision_count)
            .ok_or(CloseCoordinatorError::DecisionTokenExhausted)?;

        let request = CloseRequestId::from_sequence(self.domain, request_raw);
        let mut items = Vec::with_capacity(requirements.len());
        for (offset, requirement) in (1..=decision_count).zip(requirements.iter().copied()) {
            let raw = self
                .last_decision
                .checked_add(offset)
                .and_then(NonZeroU64::new)
                .ok_or(CloseCoordinatorError::DecisionTokenExhausted)?;
            items.push(ClosePlanItem {
                item: requirement.item,
                capability: requirement.capability,
                token: CloseDecisionToken::from_sequence(self.domain, raw),
                state: CloseItemDecisionState::Pending,
            });
        }

        let phase = if items.is_empty() {
            ClosePlanPhase::Approved
        } else {
            ClosePlanPhase::Requested
        };
        let plan = ClosePlan {
            request,
            authority,
            target,
            items: Arc::from(items),
            destruction: CloseDestructionState::None,
            cancellation: CloseCancellationState::None,
            phase,
        };

        self.last_request = request.sequence();
        self.last_decision = decision_end;
        for (item_index, item) in plan.items.iter().copied().enumerate() {
            self.decision_owners.insert(
                item.token,
                TokenOwner {
                    request,
                    item_index,
                },
            );
        }
        self.requests.insert(
            request,
            PreparedClose {
                plan: plan.clone(),
                prepared,
                native: None,
                terminal_published: false,
            },
        );

        Ok(plan)
    }

    /// Returns the latest public view for a known request.
    pub(crate) fn plan(&self, request: CloseRequestId) -> Option<&ClosePlan> {
        self.requests.get(&request).map(PreparedClose::plan)
    }

    pub(crate) fn lookup(&self, request: CloseRequestId) -> ClosePlanLookup<'_> {
        match self.plan(request) {
            Some(plan) => ClosePlanLookup::Detailed(plan),
            None if self.request_was_retired_terminal(request) => ClosePlanLookup::RetiredTerminal,
            None => ClosePlanLookup::Unknown,
        }
    }

    /// Returns the exact core-private payload frozen for one known close plan.
    ///
    /// Callers use this after an effect barrier or an authoritative native
    /// settlement to apply the already-validated transaction, never to
    /// reconstruct a close operation from current workspace state.
    pub(crate) fn prepared(&self, request: CloseRequestId) -> Option<&P> {
        self.requests.get(&request).map(PreparedClose::prepared)
    }

    /// Returns every retained plan in stable request order.
    pub(crate) fn plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.requests.values().map(PreparedClose::plan)
    }

    /// Marks terminal snapshots visible at this successful publication boundary.
    pub(crate) fn mark_boundary_published(&mut self) {
        for record in self.requests.values_mut() {
            if record.plan.is_terminal() {
                record.terminal_published = true;
            }
        }
    }

    /// Removes detailed terminal state which was visible at an earlier publication boundary.
    ///
    /// Monotonic request and token frontiers remain authoritative replay guards. Compaction is
    /// performed only on an unpublished engine candidate, so a failed transaction cannot make
    /// terminal history disappear from the live engine.
    pub(crate) fn compact_published_terminal(&mut self) -> usize {
        let retired = self
            .requests
            .iter()
            .filter_map(|(request, record)| {
                (record.terminal_published && record.plan.is_terminal()).then_some(*request)
            })
            .collect::<BTreeSet<_>>();
        if retired.is_empty() {
            return 0;
        }

        self.requests
            .retain(|request, _| !retired.contains(request));
        self.decision_owners
            .retain(|_, owner| !retired.contains(&owner.request));
        self.deferred_owners
            .retain(|_, owner| !retired.contains(&owner.request));
        retired.len()
    }

    fn request_was_retired_terminal(&self, request: CloseRequestId) -> bool {
        request.domain() == self.domain
            && request.sequence() <= self.last_request
            && !self.requests.contains_key(&request)
    }

    /// Returns every non-terminal plan in stable request order.
    pub(crate) fn active_plans(&self) -> impl Iterator<Item = &ClosePlan> {
        self.plans().filter(|plan| !plan.is_terminal())
    }

    pub(crate) fn extend_referenced_effects(&self, effects: &mut BTreeSet<EffectId>) {
        for native in self.requests.values().filter_map(|record| record.native) {
            for causality in [native.close, native.cancellation].into_iter().flatten() {
                effects.insert(causality.effect);
                effects.extend(causality.after_effect);
            }
        }
    }

    /// Returns the newest reusable plan for one exact target and authority.
    ///
    /// UI retries may reuse only a non-terminal request derived from the same
    /// workspace and policy revisions. An authority change requires a newly
    /// prepared plan rather than reinterpreting frozen item capabilities.
    pub(crate) fn active_plan_for_target(
        &self,
        target: ClosePlanTarget,
        authority: CloseAuthority,
    ) -> Option<&ClosePlan> {
        self.requests.values().rev().find_map(|record| {
            let plan = record.plan();
            (plan.target == target && plan.authority == authority && !plan.is_terminal())
                .then_some(plan)
        })
    }

    /// Returns the active surface plan for one exact provider close edge.
    ///
    /// A native binding can observe several close edges over its lifetime. A
    /// superseded edge must never block, settle, or otherwise select a plan
    /// prepared for a later edge on the same binding.
    pub(crate) fn surface_request_for_edge(&self, edge: NativeCloseEdge) -> Option<CloseRequestId> {
        self.requests.values().rev().find_map(|record| {
            (!record.plan.is_terminal() && self.record_native_edge(record) == Some(edge))
                .then_some(record.plan.request)
        })
    }

    /// Returns every non-terminal surface plan still associated with one native binding.
    ///
    /// This is only for terminal cleanup after a typed destruction fact. New
    /// requests and normal settlement must use [`Self::surface_request_for_edge`]
    /// or [`Self::surface_request_for_effect`].
    pub(crate) fn active_surface_requests_for_binding(
        &self,
        binding: ViewportBinding,
    ) -> Vec<CloseRequestId> {
        self.requests
            .values()
            .filter_map(|record| {
                (!record.plan.is_terminal()
                    && record
                        .native
                        .is_some_and(|native| native.binding == binding))
                .then_some(record.plan.request)
            })
            .collect()
    }

    /// Returns the active surface plan named by one exact provider effect acknowledgement.
    ///
    /// Effect identities are engine-global, so this is unambiguous even while
    /// a binding has retained indeterminate plans from older close edges.
    pub(crate) fn surface_request_for_effect(
        &self,
        binding: ViewportBinding,
        effect: EffectId,
    ) -> Option<CloseRequestId> {
        self.requests.values().rev().find_map(|record| {
            let native = record.native?;
            (!record.plan.is_terminal()
                && native.binding == binding
                && (native.close.is_some_and(|close| close.effect == effect)
                    || native
                        .cancellation
                        .is_some_and(|cancellation| cancellation.effect == effect)))
            .then_some(record.plan.request)
        })
    }

    /// Returns the exact native edge frozen into a surface plan.
    pub(crate) fn native_edge(&self, request: CloseRequestId) -> Option<NativeCloseEdge> {
        self.requests
            .get(&request)
            .and_then(|record| self.record_native_edge(record))
    }

    fn record_native_edge(&self, record: &PreparedClose<P>) -> Option<NativeCloseEdge> {
        let native = record.native?;
        Some(NativeCloseEdge {
            domain: self.domain,
            binding: native.binding,
            observed_at: native.edge_observed_at,
            received_at: native.edge_received_at,
        })
    }

    /// Returns the emitted destructive native-close effect, when any.
    pub(crate) fn native_close_effect(&self, request: CloseRequestId) -> Option<EffectId> {
        self.requests
            .get(&request)?
            .native?
            .close
            .map(|effect| effect.effect)
    }

    /// Returns the emitted native-close cancellation effect, when any.
    pub(crate) fn native_cancellation_effect(&self, request: CloseRequestId) -> Option<EffectId> {
        self.requests
            .get(&request)?
            .native?
            .cancellation
            .map(|effect| effect.effect)
    }

    /// Consumes one exact initial decision token.
    pub(crate) fn resolve(
        &mut self,
        request: CloseRequestId,
        token: CloseDecisionToken,
        authority: CloseAuthority,
        decision: CloseDecision,
    ) -> Result<CloseResolutionOutcome, CloseCoordinatorError> {
        if let Err(reason) = self.guard_request(request, authority) {
            return Ok(CloseResolutionOutcome::Inert(reason));
        }

        let Some(owner) = self.decision_owners.get(&token).copied() else {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::UnknownDecisionToken { request, token },
            ));
        };
        if owner.request != request {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::WrongDecisionToken {
                    request,
                    token,
                    owner: owner.request,
                },
            ));
        }

        let phase = self.requests.get(&request).map(|record| record.plan.phase);
        if !phase.is_some_and(|phase| {
            matches!(
                phase,
                ClosePlanPhase::Requested
                    | ClosePlanPhase::Resolving
                    | ClosePlanPhase::Deferred
                    | ClosePlanPhase::Approved
            )
        }) {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::PhaseMismatch {
                    request,
                    action: CloseLifecycleAction::ResolveInitialDecision,
                    actual: phase.unwrap_or(ClosePlanPhase::Stale),
                },
            ));
        }

        let Some((item, capability, state)) = self.item_snapshot(request, owner.item_index) else {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::UnknownDecisionToken { request, token },
            ));
        };
        if state != CloseItemDecisionState::Pending {
            return Ok(CloseResolutionOutcome::Inert(
                CloseInertReason::DuplicateDecision {
                    request,
                    item,
                    state,
                },
            ));
        }

        match decision {
            CloseDecision::Allow => {
                let Some(record) = self.requests.get_mut(&request) else {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::UnknownRequest { request },
                    ));
                };
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Allowed;
                record.refresh_resolution_phase();
                if record.plan.phase == ClosePlanPhase::Approved {
                    Ok(CloseResolutionOutcome::Approved { request })
                } else {
                    Ok(CloseResolutionOutcome::Recorded {
                        request,
                        item,
                        phase: record.plan.phase,
                    })
                }
            }
            CloseDecision::Veto => {
                let Some(record) = self.requests.get_mut(&request) else {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::UnknownRequest { request },
                    ));
                };
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Vetoed;
                record.plan.phase = ClosePlanPhase::Vetoed;
                Ok(CloseResolutionOutcome::Vetoed { request, item })
            }
            CloseDecision::Deferred => {
                if !capability.allows_deferred() {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::DeferredNotAllowed {
                            request,
                            item,
                            capability,
                        },
                    ));
                }
                let continuation = self.mint_deferred()?;
                let Some(record) = self.requests.get_mut(&request) else {
                    return Ok(CloseResolutionOutcome::Inert(
                        CloseInertReason::UnknownRequest { request },
                    ));
                };
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Deferred { continuation };
                record.refresh_resolution_phase();
                self.deferred_owners.insert(continuation, owner);
                Ok(CloseResolutionOutcome::Deferred {
                    request,
                    item,
                    continuation,
                })
            }
        }
    }

    /// Consumes one exact deferred continuation token.
    pub(crate) fn continue_deferred(
        &mut self,
        request: CloseRequestId,
        token: DeferredCloseToken,
        authority: CloseAuthority,
        decision: DeferredCloseDecision,
    ) -> CloseResolutionOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseResolutionOutcome::Inert(reason);
        }

        let Some(owner) = self.deferred_owners.get(&token).copied() else {
            return CloseResolutionOutcome::Inert(CloseInertReason::UnknownDeferredToken {
                request,
                token,
            });
        };
        if owner.request != request {
            return CloseResolutionOutcome::Inert(CloseInertReason::WrongDeferredToken {
                request,
                token,
                owner: owner.request,
            });
        }

        let phase = self.requests.get(&request).map(|record| record.plan.phase);
        if !phase.is_some_and(|phase| {
            matches!(
                phase,
                ClosePlanPhase::Deferred | ClosePlanPhase::Resolving | ClosePlanPhase::Approved
            )
        }) {
            return CloseResolutionOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ResolveDeferredDecision,
                actual: phase.unwrap_or(ClosePlanPhase::Stale),
            });
        }

        let Some((item, _, state)) = self.item_snapshot(request, owner.item_index) else {
            return CloseResolutionOutcome::Inert(CloseInertReason::UnknownDeferredToken {
                request,
                token,
            });
        };
        if !matches!(
            state,
            CloseItemDecisionState::Deferred { continuation } if continuation == token
        ) {
            return CloseResolutionOutcome::Inert(
                CloseInertReason::DuplicateDeferredContinuation {
                    request,
                    item,
                    state,
                },
            );
        }

        let Some(record) = self.requests.get_mut(&request) else {
            return CloseResolutionOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        match decision {
            DeferredCloseDecision::Allow => {
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Allowed;
                record.refresh_resolution_phase();
                if record.plan.phase == ClosePlanPhase::Approved {
                    CloseResolutionOutcome::Approved { request }
                } else {
                    CloseResolutionOutcome::Recorded {
                        request,
                        item,
                        phase: record.plan.phase,
                    }
                }
            }
            DeferredCloseDecision::Veto => {
                Arc::make_mut(&mut record.plan.items)[owner.item_index].state =
                    CloseItemDecisionState::Vetoed;
                record.plan.phase = ClosePlanPhase::Vetoed;
                CloseResolutionOutcome::Vetoed { request, item }
            }
        }
    }

    /// Borrows the exact prepared payload only while the plan is approved and current.
    pub(crate) fn approved(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> Result<ApprovedClose<'_, P>, CloseInertReason> {
        self.guard_request(request, authority)?;
        let phase = self.requests.get(&request).map(|record| record.plan.phase);
        if phase != Some(ClosePlanPhase::Approved) {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::PrepareCommit,
                actual: phase.unwrap_or(ClosePlanPhase::Stale),
            });
        }
        let record = self
            .requests
            .get(&request)
            .ok_or(CloseInertReason::UnknownRequest { request })?;
        Ok(ApprovedClose {
            plan: record.plan(),
            prepared: record.prepared(),
        })
    }

    /// Invalidates a prepared plan after its final checked commit is rejected.
    ///
    /// Item and root plans become stale only before an external effect creates
    /// an irreversible recovery obligation. A surface plan instead preserves
    /// that obligation and requests explicit cancellation, regardless of
    /// whether its close effect was already emitted.
    pub(crate) fn mark_stale(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() == ClosePlanTargetKind::Surface {
            return self.request_cancel(request, authority);
        }
        self.advance_exact(
            request,
            CloseLifecycleAction::InvalidatePrepared,
            &[
                ClosePlanPhase::Requested,
                ClosePlanPhase::Resolving,
                ClosePlanPhase::Deferred,
                ClosePlanPhase::Approved,
            ],
            ClosePlanPhase::Stale,
        )
    }

    /// Marks a successful item/root topology transaction as applied.
    pub(crate) fn mark_local_applied(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() == ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyLocalCommit,
                target: ClosePlanTargetKind::Surface,
            });
        }
        self.advance_exact(
            request,
            CloseLifecycleAction::ApplyLocalCommit,
            &[ClosePlanPhase::Approved],
            ClosePlanPhase::Applied,
        )
    }

    /// Records emission of the exact native close effect for a surface plan.
    pub(crate) fn mark_effect_emitted(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        binding: ViewportBinding,
        effect: EffectId,
        issued_after: CloseObservationGeneration,
        received_after: InventoryGeneration,
        after_effect: Option<EffectId>,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let ClosePlanTarget::Surface { surface, .. } = record.plan.target else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::EmitCloseEffect,
                target: record.plan.target.kind(),
            });
        };
        if binding.surface() != surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeSurfaceMismatch {
                request,
                expected: surface,
                actual: binding.surface(),
            });
        }
        if record.plan.phase != ClosePlanPhase::Approved {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::EmitCloseEffect,
                actual: record.plan.phase,
            });
        }
        let Some(native) = record.native.as_mut() else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        };
        if native.binding != binding {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: binding,
            });
        }
        if issued_after < native.edge_observed_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionObservationPrecedesEdge {
                    request,
                    edge_observed_at: native.edge_observed_at,
                    issued_after,
                },
            );
        }
        if received_after < native.edge_received_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionInventoryPrecedesEdge {
                    request,
                    edge_received_at: native.edge_received_at,
                    received_after,
                },
            );
        }
        if after_effect.is_some() {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeCloseEffectPredecessorMismatch {
                    request,
                    expected: None,
                    actual: after_effect,
                },
            );
        }
        if native.close.is_some() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::EmitCloseEffect,
                actual: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        native.close = Some(NativeEffectCausality {
            effect,
            issued_after,
            received_after,
            after_effect,
        });
        record.plan.destruction = CloseDestructionState::EffectEmitted;
        record.plan.phase = ClosePlanPhase::EffectEmitted;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::EffectEmitted,
        }
    }

    /// Applies a surface plan only after the caller validates exact destruction proof.
    fn mark_destroyed_applied(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                target: record.plan.target.kind(),
            });
        }
        if record.plan.destruction == CloseDestructionState::None {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                actual: record.plan.phase,
            });
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        record.plan.destruction = CloseDestructionState::None;
        record.plan.cancellation = CloseCancellationState::None;
        record.plan.phase = ClosePlanPhase::Applied;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Applied,
        }
    }

    /// Requests explicit cancellation without inferring a timeout.
    ///
    /// Item and root plans terminate immediately because they have no platform
    /// settlement. A surface plan always owns an observed native close edge, so
    /// cancellation remains pending until the provider proves the edge cleared.
    pub(crate) fn request_cancel(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            let from = record.plan.phase;
            record.plan.cancellation = CloseCancellationState::Settled;
            record.plan.phase = ClosePlanPhase::Cancelled;
            return CloseAdvanceOutcome::Advanced {
                request,
                from,
                to: ClosePlanPhase::Cancelled,
            };
        }
        if record.native.is_none() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        }
        if record.plan.cancellation != CloseCancellationState::None {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::RequestCancellation,
                actual: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        record.plan.cancellation = CloseCancellationState::Requested;
        record.plan.phase = ClosePlanPhase::CancelRequested;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::CancelRequested,
        }
    }

    /// Freezes the exact compensating effect and the last provider observation
    /// which causally preceded its emission.
    pub(crate) fn mark_cancellation_effect_emitted(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        binding: ViewportBinding,
        effect: EffectId,
        issued_after: CloseObservationGeneration,
        received_after: InventoryGeneration,
        after_effect: Option<EffectId>,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::EmitCancellationEffect,
                target: record.plan.target.kind(),
            });
        }
        if !matches!(
            record.plan.cancellation,
            CloseCancellationState::Requested | CloseCancellationState::Indeterminate
        ) {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::EmitCancellationEffect,
                actual: record.plan.phase,
            });
        }
        let Some(native) = record.native.as_mut() else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        };
        if binding != native.binding {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: binding,
            });
        }
        if issued_after < native.edge_observed_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionObservationPrecedesEdge {
                    request,
                    edge_observed_at: native.edge_observed_at,
                    issued_after,
                },
            );
        }
        if received_after < native.edge_received_at {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::NativeEmissionInventoryPrecedesEdge {
                    request,
                    edge_received_at: native.edge_received_at,
                    received_after,
                },
            );
        }
        let expected_predecessor = native.close.map(|close| close.effect);
        if after_effect != expected_predecessor {
            return CloseAdvanceOutcome::Inert(
                CloseInertReason::CancellationEffectPredecessorMismatch {
                    request,
                    expected: expected_predecessor,
                    actual: after_effect,
                },
            );
        }
        if let Some(existing) = native.cancellation {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEffectMismatch {
                request,
                expected: existing.effect,
                actual: effect,
            });
        }

        let phase = record.plan.phase;
        native.cancellation = Some(NativeEffectCausality {
            effect,
            issued_after,
            received_after,
            after_effect,
        });
        CloseAdvanceOutcome::Advanced {
            request,
            from: phase,
            to: phase,
        }
    }

    /// Reduces authoritative native settlement from one provider observation batch.
    ///
    /// Destruction wins when both facts occur in the same batch. Callers must not
    /// split one batch into multiple calls because doing so would discard this
    /// explicit ordering rule.
    pub(crate) fn settle_native(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        settlement: CloseNativeSettlement,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }

        match settlement {
            CloseNativeSettlement::DestroyedProved(proof) => {
                if let Err(reason) = self.validate_destroyed_proof(request, proof) {
                    return CloseAdvanceOutcome::Inert(reason);
                }
                self.mark_destroyed_applied(request, authority)
            }
            CloseNativeSettlement::CancellationProved(proof) => {
                self.apply_cancellation_proof(request, authority, proof)
            }
            CloseNativeSettlement::DestroyedProvedWithCancellation {
                destroyed,
                cancellation,
            } => {
                if let Err(reason) = self.validate_destroyed_proof(request, destroyed) {
                    return CloseAdvanceOutcome::Inert(reason);
                }
                if let Err(reason) = self.validate_cancellation_proof(request, cancellation) {
                    return CloseAdvanceOutcome::Inert(reason);
                }
                self.mark_destroyed_applied(request, authority)
            }
        }
    }

    fn validate_destroyed_proof(
        &self,
        request: CloseRequestId,
        proof: CloseDestroyedProof,
    ) -> Result<(), CloseInertReason> {
        if proof.request.domain() != self.domain {
            return Err(CloseInertReason::DestroyedProofDomainMismatch {
                request,
                expected: self.domain,
                actual: proof.request.domain(),
            });
        }
        if proof.request != request {
            return Err(CloseInertReason::DestroyedProofRequestMismatch {
                request,
                proof_request: proof.request,
            });
        }
        let Some(record) = self.requests.get(&request) else {
            return Err(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return Err(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                target: record.plan.target.kind(),
            });
        }
        let Some(native) = record.native else {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                actual: record.plan.phase,
            });
        };
        if proof.binding != native.binding {
            return Err(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: proof.binding,
            });
        }
        let Some(close) = native.close else {
            return Err(CloseInertReason::CloseEffectNotEmitted { request });
        };
        if record.plan.destruction == CloseDestructionState::None {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyDestroyed,
                actual: record.plan.phase,
            });
        }
        if proof.observation_generation <= close.issued_after {
            return Err(CloseInertReason::DestroyedObservationNotNewer {
                request,
                issued_after: close.issued_after,
                actual: proof.observation_generation,
            });
        }
        if proof.inventory_generation <= close.received_after {
            return Err(CloseInertReason::DestroyedInventoryNotNewer {
                request,
                issued_after: close.received_after,
                actual: proof.inventory_generation,
            });
        }
        let cancellation_effect = native.cancellation.map(|cancellation| cancellation.effect);
        if proof.acknowledged_effect != close.effect
            && cancellation_effect != Some(proof.acknowledged_effect)
        {
            return Err(CloseInertReason::DestroyedEffectNotAcknowledged {
                request,
                close_effect: close.effect,
                cancellation_effect,
                actual: proof.acknowledged_effect,
            });
        }
        Ok(())
    }

    fn apply_cancellation_proof(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
        proof: CloseCancellationProof,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        if let Err(reason) = self.validate_cancellation_proof(request, proof) {
            return CloseAdvanceOutcome::Inert(reason);
        }

        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        record.plan.destruction = CloseDestructionState::None;
        record.plan.cancellation = CloseCancellationState::Settled;
        record.plan.phase = ClosePlanPhase::Cancelled;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Cancelled,
        }
    }

    fn validate_cancellation_proof(
        &self,
        request: CloseRequestId,
        proof: CloseCancellationProof,
    ) -> Result<(), CloseInertReason> {
        if proof.request.domain() != self.domain {
            return Err(CloseInertReason::CancellationProofDomainMismatch {
                request,
                expected: self.domain,
                actual: proof.request.domain(),
            });
        }
        if proof.request != request {
            return Err(CloseInertReason::CancellationProofRequestMismatch {
                request,
                proof_request: proof.request,
            });
        }
        let Some(record) = self.requests.get(&request) else {
            return Err(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return Err(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::ApplyCancellationProof,
                target: record.plan.target.kind(),
            });
        }
        if !matches!(
            record.plan.cancellation,
            CloseCancellationState::Requested | CloseCancellationState::Indeterminate
        ) {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyCancellationProof,
                actual: record.plan.phase,
            });
        }
        let Some(native) = record.native else {
            return Err(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::ApplyCancellationProof,
                actual: record.plan.phase,
            });
        };
        if proof.binding != native.binding {
            return Err(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: proof.binding,
            });
        }
        let expected_close_effect = native.close.map(|close| close.effect);
        let expected_cancel_effect = native.cancellation.map(|cancellation| cancellation.effect);
        if native
            .cancellation
            .is_some_and(|cancellation| cancellation.after_effect != expected_close_effect)
        {
            return Err(CloseInertReason::CancellationEffectPredecessorMismatch {
                request,
                expected: expected_close_effect,
                actual: native
                    .cancellation
                    .and_then(|cancellation| cancellation.after_effect),
            });
        }
        if proof.close_effect != expected_close_effect {
            return Err(CloseInertReason::CancellationEffectPredecessorMismatch {
                request,
                expected: expected_close_effect,
                actual: proof.close_effect,
            });
        }
        if proof.cancel_effect != expected_cancel_effect {
            return Err(CloseInertReason::CancellationEffectMismatch {
                request,
                expected: expected_cancel_effect,
                actual: proof.cancel_effect,
            });
        }
        let (expected_frontier, issued_after, received_after) = match native.cancellation {
            Some(cancellation) => (
                Some(cancellation.effect),
                cancellation.issued_after,
                cancellation.received_after,
            ),
            None => match native.close {
                Some(close) => (Some(close.effect), close.issued_after, close.received_after),
                None => (None, native.edge_observed_at, native.edge_received_at),
            },
        };
        if proof.known_effect_frontier != expected_frontier {
            return Err(CloseInertReason::CancellationKnownFrontierMismatch {
                request,
                expected: expected_frontier,
                actual: proof.known_effect_frontier,
            });
        }
        if proof.observation_generation <= issued_after {
            return Err(CloseInertReason::CancellationObservationNotNewer {
                request,
                issued_after,
                actual: proof.observation_generation,
            });
        }
        if proof.inventory_generation <= received_after {
            return Err(CloseInertReason::CancellationInventoryNotNewer {
                request,
                issued_after: received_after,
                actual: proof.inventory_generation,
            });
        }
        Ok(())
    }

    /// Records unknown external settlement while retaining the exact obligation.
    pub(crate) fn mark_indeterminate(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        let has_external_effect = record
            .native
            .is_some_and(|native| native.close.is_some() || native.cancellation.is_some());
        if !has_external_effect
            || !matches!(
                from,
                ClosePlanPhase::EffectEmitted
                    | ClosePlanPhase::AwaitingDestroyed
                    | ClosePlanPhase::CancelRequested
            )
        {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::MarkIndeterminate,
                actual: from,
            });
        }
        if record.plan.cancellation == CloseCancellationState::Requested {
            record.plan.cancellation = CloseCancellationState::Indeterminate;
        }
        record.plan.phase = ClosePlanPhase::Indeterminate;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Indeterminate,
        }
    }

    /// Retains an unresolved surface plan after typed provider facts prove that
    /// its exact close edge is no longer current.
    ///
    /// This differs from [`Self::mark_indeterminate`]: it is not a dispatch
    /// report and therefore may also apply before any platform effect was
    /// emitted. The plan remains auditable, but cannot block a later edge on
    /// the same binding or authorize its frozen topology payload.
    pub(crate) fn mark_native_edge_indeterminate(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> CloseAdvanceOutcome {
        if let Err(reason) = self.guard_request(request, authority) {
            return CloseAdvanceOutcome::Inert(reason);
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::MarkIndeterminate,
                target: record.plan.target.kind(),
            });
        }
        if record.native.is_none() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        }
        if record.plan.is_terminal() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: record.plan.phase,
            });
        }
        if record.plan.phase == ClosePlanPhase::Indeterminate {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action: CloseLifecycleAction::MarkIndeterminate,
                actual: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        record.plan.cancellation = CloseCancellationState::Indeterminate;
        record.plan.phase = ClosePlanPhase::Indeterminate;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::Indeterminate,
        }
    }

    /// Terminates a surface plan when the exact binding was externally destroyed
    /// without a valid causal proof for the plan's emitted effect lane.
    ///
    /// This deliberately bypasses workspace/policy authority freshness: the
    /// native resource is already gone, so retaining an active plan would leak
    /// an unreachable obligation. It never authorizes the frozen topology
    /// payload; the engine must take its explicit unplanned recovery path.
    pub(crate) fn mark_externally_destroyed_unproved(
        &mut self,
        request: CloseRequestId,
        binding: ViewportBinding,
    ) -> CloseAdvanceOutcome {
        if request.domain() != self.domain {
            return CloseAdvanceOutcome::Inert(CloseInertReason::RequestDomainMismatch {
                request,
                expected: self.domain,
                actual: request.domain(),
            });
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let Some(native) = record.native else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeEdgeNotObserved { request });
        };
        if native.binding != binding {
            return CloseAdvanceOutcome::Inert(CloseInertReason::NativeBindingMismatch {
                request,
                expected: native.binding,
                actual: binding,
            });
        }
        if record.plan.target.kind() != ClosePlanTargetKind::Surface {
            return CloseAdvanceOutcome::Inert(CloseInertReason::TargetMismatch {
                request,
                action: CloseLifecycleAction::MarkExternallyDestroyedUnproved,
                target: record.plan.target.kind(),
            });
        }
        if record.plan.is_terminal() {
            return CloseAdvanceOutcome::Inert(CloseInertReason::Terminal {
                request,
                phase: record.plan.phase,
            });
        }
        let from = record.plan.phase;
        record.plan.destruction = CloseDestructionState::None;
        record.plan.cancellation = CloseCancellationState::None;
        record.plan.phase = ClosePlanPhase::ExternallyDestroyedUnproved;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: ClosePlanPhase::ExternallyDestroyedUnproved,
        }
    }

    /// Invalidates every non-terminal plan whose authority no longer matches.
    pub(crate) fn invalidate_stale(&mut self, authority: CloseAuthority) -> Vec<CloseRequestId> {
        if authority.domain() != self.domain {
            return Vec::new();
        }
        let mut invalidated = Vec::new();
        for (request, record) in &mut self.requests {
            let cancels_on_authority_drift = Self::cancels_on_authority_drift(&record.plan);
            if (record.plan.phase.may_become_stale() || cancels_on_authority_drift)
                && record.plan.authority != authority
            {
                if cancels_on_authority_drift {
                    record.plan.cancellation = CloseCancellationState::Requested;
                    record.plan.phase = ClosePlanPhase::CancelRequested;
                } else {
                    record.plan.phase = ClosePlanPhase::Stale;
                }
                invalidated.push(*request);
            }
        }
        invalidated
    }

    fn validate_requirements(
        target: ClosePlanTarget,
        requirements: &[CloseItemRequirement],
    ) -> Result<(), CloseCoordinatorError> {
        let mut seen = BTreeSet::new();
        for requirement in requirements {
            if !seen.insert(requirement.item) {
                return Err(CloseCoordinatorError::DuplicateItem {
                    item: requirement.item,
                });
            }
            if !requirement.capability.allows_close() {
                return Err(CloseCoordinatorError::DisabledItem {
                    item: requirement.item,
                });
            }
        }

        match target {
            ClosePlanTarget::Item { item } => {
                let actual = requirements
                    .iter()
                    .map(|requirement| requirement.item)
                    .collect::<Vec<_>>();
                if actual.as_slice() != [item] {
                    return Err(CloseCoordinatorError::ItemTargetMismatch {
                        target: item,
                        actual,
                    });
                }
            }
            ClosePlanTarget::Root { root } if requirements.is_empty() => {
                return Err(CloseCoordinatorError::EmptyRootHasNoItemDecisions { root });
            }
            ClosePlanTarget::Surface {
                surface,
                disposition: SurfaceCloseDisposition::RehomeAll { .. },
            } if !requirements.is_empty() => {
                return Err(CloseCoordinatorError::SurfaceRehomeHasItemDecisions {
                    surface,
                    item_count: requirements.len(),
                });
            }
            ClosePlanTarget::Surface {
                surface,
                disposition: SurfaceCloseDisposition::CloseContent,
            } if requirements.is_empty() => {
                return Err(
                    CloseCoordinatorError::SurfaceCloseContentHasNoItemDecisions { surface },
                );
            }
            ClosePlanTarget::Root { .. } | ClosePlanTarget::Surface { .. } => {}
        }
        Ok(())
    }

    fn guard_request(
        &mut self,
        request: CloseRequestId,
        authority: CloseAuthority,
    ) -> Result<(), CloseInertReason> {
        if request.domain() != self.domain {
            return Err(CloseInertReason::RequestDomainMismatch {
                request,
                expected: self.domain,
                actual: request.domain(),
            });
        }
        if authority.domain() != self.domain {
            return Err(CloseInertReason::AuthorityDomainMismatch {
                request,
                expected: self.domain,
                actual: authority.domain(),
            });
        }
        let Some(record) = self.requests.get_mut(&request) else {
            return Err(if self.request_was_retired_terminal(request) {
                CloseInertReason::RetiredTerminal { request }
            } else {
                CloseInertReason::UnknownRequest { request }
            });
        };
        if record.plan.is_terminal() {
            return Err(CloseInertReason::Terminal {
                request,
                phase: record.plan.phase,
            });
        }
        let cancels_on_authority_drift = Self::cancels_on_authority_drift(&record.plan);
        if (record.plan.phase.may_become_stale() || cancels_on_authority_drift)
            && record.plan.authority != authority
        {
            let expected = record.plan.authority;
            if cancels_on_authority_drift {
                record.plan.cancellation = CloseCancellationState::Requested;
                record.plan.phase = ClosePlanPhase::CancelRequested;
            } else {
                record.plan.phase = ClosePlanPhase::Stale;
            }
            return Err(CloseInertReason::AuthorityStale {
                request,
                expected,
                actual: authority,
            });
        }
        Ok(())
    }

    fn cancels_on_authority_drift(plan: &ClosePlan) -> bool {
        plan.target.kind() == ClosePlanTargetKind::Surface
            && (plan.phase.may_become_stale() || plan.phase == ClosePlanPhase::Vetoed)
    }

    fn item_snapshot(
        &self,
        request: CloseRequestId,
        item_index: usize,
    ) -> Option<(ItemId, CloseCapability, CloseItemDecisionState)> {
        let record = self.requests.get(&request)?;
        let item = record.plan.items.get(item_index)?;
        Some((item.item, item.capability, item.state))
    }

    fn mint_deferred(&mut self) -> Result<DeferredCloseToken, CloseCoordinatorError> {
        let raw = self
            .last_deferred
            .checked_add(1)
            .and_then(NonZeroU64::new)
            .ok_or(CloseCoordinatorError::DeferredTokenExhausted)?;
        let token = DeferredCloseToken::from_sequence(self.domain, raw);
        self.last_deferred = token.sequence();
        Ok(token)
    }

    fn advance_exact(
        &mut self,
        request: CloseRequestId,
        action: CloseLifecycleAction,
        expected: &[ClosePlanPhase],
        next: ClosePlanPhase,
    ) -> CloseAdvanceOutcome {
        let Some(record) = self.requests.get(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        if !expected.contains(&record.plan.phase) {
            return CloseAdvanceOutcome::Inert(CloseInertReason::PhaseMismatch {
                request,
                action,
                actual: record.plan.phase,
            });
        }
        self.advance_unchecked(request, next)
    }

    fn advance_unchecked(
        &mut self,
        request: CloseRequestId,
        next: ClosePlanPhase,
    ) -> CloseAdvanceOutcome {
        let Some(record) = self.requests.get_mut(&request) else {
            return CloseAdvanceOutcome::Inert(CloseInertReason::UnknownRequest { request });
        };
        let from = record.plan.phase;
        record.plan.phase = next;
        CloseAdvanceOutcome::Advanced {
            request,
            from,
            to: next,
        }
    }
}
