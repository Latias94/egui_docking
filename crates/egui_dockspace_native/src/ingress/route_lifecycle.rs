//! Native viewport route, registration, retirement, and replacement lineage state.

use super::*;

#[derive(Clone, Copy, Debug)]
pub(crate) struct BoundNativeRoute {
    pub(super) native_binding: NativeViewportBinding,
    pub(super) exact: ExactNativeViewport,
    pub(super) surface: SurfaceId,
    pub(super) core: ViewportBinding,
}

impl BoundNativeRoute {
    pub(crate) const fn exact(self) -> ExactNativeViewport {
        self.exact
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(crate) const fn core(self) -> ViewportBinding {
        self.core
    }

    pub(crate) const fn native_binding(self) -> NativeViewportBinding {
        self.native_binding
    }

    pub(super) const fn adapter_route(self) -> NativeCoreRoute {
        NativeCoreRoute::new(self.exact, self.surface, self.core)
    }
}

/// Keeps one exact native lifetime in the host viewport roster without granting core authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct RouteLessNativeKeepalive {
    pub(super) exact: ExactNativeViewport,
    pub(super) surface: SurfaceId,
}

impl RouteLessNativeKeepalive {
    pub(crate) const fn new(exact: ExactNativeViewport, surface: SurfaceId) -> Self {
        Self { exact, surface }
    }

    pub(crate) const fn exact(self) -> ExactNativeViewport {
        self.exact
    }

    pub(crate) const fn surface(self) -> SurfaceId {
        self.surface
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RetiredNativeRoute {
    pub(super) exact: ExactNativeViewport,
    pub(super) surface: SurfaceId,
    pub(super) core: ViewportBinding,
    // An unpublished bootstrap has no entry in egui's route registry to retire.
    pub(super) adapter_route_was_published: bool,
    pub(super) close_acknowledgement: CloseEffectAcknowledgement,
}

#[derive(Clone, Copy, Debug)]
pub(super) enum NativeRetirementAuthority {
    Published(BoundNativeRoute),
    PendingRegistration(PendingNativeRegistration, ViewportBinding),
    Provisional(crate::effects::PendingEffectRoute),
    AdoptedProvisional(
        PendingNativeRegistration,
        crate::effects::PendingEffectRoute,
    ),
}

impl NativeRetirementAuthority {
    pub(super) const fn surface(self) -> SurfaceId {
        match self {
            Self::Published(route) => route.surface(),
            Self::PendingRegistration(pending, _) => pending.surface,
            Self::Provisional(route) => route.surface(),
            Self::AdoptedProvisional(pending, _) => pending.surface,
        }
    }

    pub(super) const fn core(self) -> ViewportBinding {
        match self {
            Self::Published(route) => route.core(),
            Self::PendingRegistration(_, binding) => binding,
            Self::Provisional(route) => route.core(),
            Self::AdoptedProvisional(_, route) => route.core(),
        }
    }

    pub(super) const fn adapter_route_was_published(self) -> bool {
        matches!(self, Self::Published(_))
    }
}

impl RetiredNativeRoute {
    pub(super) const fn surface(self) -> SurfaceId {
        self.surface
    }

    pub(super) const fn adapter_retirement(self) -> Option<ExactNativeViewport> {
        if self.adapter_route_was_published {
            Some(self.exact)
        } else {
            None
        }
    }
}

pub(super) fn existing_restored_core_route(
    dockspace: &Dockspace,
    surface: SurfaceId,
    retirements: &[RetiredNativeRoute],
) -> Option<(SurfaceId, ViewportBinding)> {
    if retirements
        .iter()
        .any(|retired| retired.surface() == surface)
    {
        return None;
    }
    dockspace
        .native_viewport_binding(surface)
        .map(|binding| (surface, binding))
}

pub(super) fn configured_child_accepts_predecessor(
    spec: &crate::NativeSurfaceSpec,
    surface: SurfaceId,
    predecessor: ViewportBinding,
) -> bool {
    spec.role() == ViewportRole::Child
        && spec.surface() == surface
        && spec
            .bootstrap_token()
            .is_none_or(|token| token == predecessor.token())
}

#[derive(Debug, Default)]
pub(super) struct DeferredReplacementLineagePlan {
    pub(super) successor_after_retirement: BTreeMap<ExactNativeViewport, PendingNativeRegistration>,
    pub(super) intermediate_retirements: BTreeSet<ExactNativeViewport>,
    pub(super) route_less_lifetimes: BTreeSet<ExactNativeViewport>,
    pub(super) live_keepalives: BTreeMap<ViewportId, RouteLessNativeKeepalive>,
    pub(super) terminal_viewports: BTreeSet<ViewportId>,
}

impl DeferredReplacementLineagePlan {
    pub(super) fn take_successor(
        &mut self,
        retired: ExactNativeViewport,
    ) -> Option<PendingNativeRegistration> {
        self.successor_after_retirement.remove(&retired)
    }

    pub(super) fn take_intermediate(&mut self, retired: ExactNativeViewport) -> bool {
        self.intermediate_retirements.remove(&retired)
    }

    pub(super) fn is_route_less(&self, exact: ExactNativeViewport) -> bool {
        self.route_less_lifetimes.contains(&exact)
    }

    pub(super) fn finish(
        self,
    ) -> Result<
        (
            BTreeMap<ViewportId, RouteLessNativeKeepalive>,
            BTreeSet<ViewportId>,
        ),
        NativeRuntimeError,
    > {
        if !self.successor_after_retirement.is_empty() || !self.intermediate_retirements.is_empty()
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native replacement lineage omitted an ordered retirement",
            ));
        }
        Ok((self.live_keepalives, self.terminal_viewports))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PendingNativeRegistrationPhase {
    /// The replacement native lifetime is live, but its predecessor still owns the core binding.
    AwaitingPredecessorRetirement(ViewportBinding),
    /// Core committed its replacement effect and the existing native lifetime adopted that binding.
    AdoptedReplacement {
        predecessor: ViewportBinding,
        replacement: ViewportBinding,
    },
    /// The registration record exists only in the recorder's uncommitted suffix.
    Staged(BackendIngressOrdinal),
    /// Core committed the exact registration, but no adapter route has been published yet.
    Committed(ViewportBinding),
}

/// Correlates a bootstrap window lifetime with its core registration boundary.
///
/// This sidecar must survive registration commit until either the first route is published or an
/// ordered retirement provides the window's terminal fact.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PendingNativeRegistration {
    pub(super) exact: ExactNativeViewport,
    pub(super) surface: SurfaceId,
    pub(super) phase: PendingNativeRegistrationPhase,
}

impl PendingNativeRegistration {
    pub(super) const fn staged(
        exact: ExactNativeViewport,
        surface: SurfaceId,
        registration_ordinal: BackendIngressOrdinal,
    ) -> Self {
        Self {
            exact,
            surface,
            phase: PendingNativeRegistrationPhase::Staged(registration_ordinal),
        }
    }

    pub(super) const fn awaiting_predecessor(
        exact: ExactNativeViewport,
        surface: SurfaceId,
        predecessor: ViewportBinding,
    ) -> Self {
        Self {
            exact,
            surface,
            phase: PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(predecessor),
        }
    }

    pub(super) const fn predecessor_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding) => Some(binding),
            PendingNativeRegistrationPhase::AdoptedReplacement { predecessor, .. } => {
                Some(predecessor)
            }
            PendingNativeRegistrationPhase::Staged(_)
            | PendingNativeRegistrationPhase::Committed(_) => None,
        }
    }

    pub(super) const fn awaiting_predecessor_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(binding) => Some(binding),
            PendingNativeRegistrationPhase::AdoptedReplacement { .. }
            | PendingNativeRegistrationPhase::Staged(_)
            | PendingNativeRegistrationPhase::Committed(_) => None,
        }
    }

    pub(super) const fn adopted_replacement_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AdoptedReplacement { replacement, .. } => {
                Some(replacement)
            }
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(_)
            | PendingNativeRegistrationPhase::Staged(_)
            | PendingNativeRegistrationPhase::Committed(_) => None,
        }
    }

    pub(super) const fn committed_binding(self) -> Option<ViewportBinding> {
        match self.phase {
            PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(_)
            | PendingNativeRegistrationPhase::Staged(_) => None,
            PendingNativeRegistrationPhase::AdoptedReplacement { replacement, .. } => {
                Some(replacement)
            }
            PendingNativeRegistrationPhase::Committed(binding) => Some(binding),
        }
    }
}

impl NativeIngressBridge {
    pub(super) fn retire_native_binding(
        &mut self,
        dockspace: &mut Dockspace,
        presentations: &mut NativePresentationLedger,
        routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
        provisional_routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
        exact: ExactNativeViewport,
        close_acknowledgement: CloseEffectAcknowledgement,
    ) -> Result<Option<RetiredNativeRoute>, NativeRuntimeError> {
        if self.retired_routes.contains_key(&exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native ingress repeated one retired viewport lifetime",
            ));
        }

        let published = routes
            .get(&exact.viewport())
            .copied()
            .filter(|route| route.exact() == exact);
        let pending = self.pending_registrations.get(&exact).copied();
        let provisional = self.effects.route_for_provisional_retirement(exact);
        if published.is_some() && (pending.is_some() || provisional.is_some()) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "one native lifetime has multiple retirement authorities",
            ));
        }
        if let (Some(pending), Some(provisional)) = (pending, provisional)
            && (pending.surface != provisional.surface()
                || pending.adopted_replacement_binding() != Some(provisional.core()))
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "adopted provisional retirement authorities disagree",
            ));
        }

        let authority = published
            .map(NativeRetirementAuthority::Published)
            .or_else(|| {
                Some(NativeRetirementAuthority::AdoptedProvisional(
                    pending?,
                    provisional?,
                ))
            })
            .or_else(|| {
                let pending = pending?;
                pending
                    .committed_binding()
                    .map(|binding| NativeRetirementAuthority::PendingRegistration(pending, binding))
            })
            .or_else(|| provisional.map(NativeRetirementAuthority::Provisional));
        let Some(authority) = authority else {
            if pending.is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "native viewport retired before its core registration committed",
                ));
            }
            return Ok(None);
        };
        let surface = authority.surface();
        let core = authority.core();
        let adapter_route_was_published = authority.adapter_route_was_published();

        if matches!(
            authority,
            NativeRetirementAuthority::Provisional(_)
                | NativeRetirementAuthority::AdoptedProvisional(..)
        ) {
            self.effects.retain_initialization_receipts(exact)?;
        } else {
            self.effects.forget_pending_native(exact);
        }
        if matches!(
            authority,
            NativeRetirementAuthority::PendingRegistration(..)
        ) {
            self.effects
                .retire_restored_viewport(exact.viewport(), surface)?;
        }
        let recorder = self.recorder.as_mut().expect("provider enrolled above");
        presentations.retire(dockspace, recorder, exact)?;
        match authority {
            NativeRetirementAuthority::Published(_) => {
                routes.remove(&exact.viewport());
            }
            NativeRetirementAuthority::PendingRegistration(..) => {
                self.pending_registrations.remove(&exact);
            }
            NativeRetirementAuthority::Provisional(_) => {}
            NativeRetirementAuthority::AdoptedProvisional(..) => {
                self.pending_registrations.remove(&exact);
            }
        }
        if matches!(
            authority,
            NativeRetirementAuthority::Provisional(_)
                | NativeRetirementAuthority::AdoptedProvisional(..)
        ) && provisional_routes
            .remove(&exact.viewport())
            .is_some_and(|route| {
                route.exact() != exact || route.surface() != surface || route.core() != core
            })
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "provisional retirement changed its cycle-local route",
            ));
        }
        if self.retired_routes.insert(exact, core).is_some() {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native ingress repeated one retired viewport route",
            ));
        }
        Ok(Some(RetiredNativeRoute {
            exact,
            surface,
            core,
            adapter_route_was_published,
            close_acknowledgement,
        }))
    }

    pub(super) fn retire_deferred_replacement(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(pending) = self.pending_registrations.get(&exact).copied() else {
            return Ok(false);
        };
        if pending.awaiting_predecessor_binding().is_none() {
            return Ok(false);
        }
        self.pending_registrations.remove(&exact);
        self.effects.forget_pending_native(exact);
        if !self.deferred_replacement_retirements.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "deferred native replacement repeated one retirement lifetime",
            ));
        }
        Ok(true)
    }

    pub(super) fn retire_quarantined_native_lifetime(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<bool, NativeRuntimeError> {
        if !self.quarantined_native_lifetimes.remove(&exact) {
            return Ok(false);
        }
        self.pending_registrations.remove(&exact);
        if !self.deferred_replacement_retirements.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "quarantined native lifetime repeated one retirement",
            ));
        }
        Ok(true)
    }

    pub(super) fn retire_materialized_restored_bootstrap(
        &mut self,
        configured: &NativeViewportRoster,
        exact: ExactNativeViewport,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(spec) = configured
            .get(exact.viewport())
            .filter(|spec| spec.role() == ViewportRole::Child && spec.bootstrap_token().is_some())
        else {
            return Ok(false);
        };
        if !self
            .effects
            .retire_materialized_restored_viewport(exact.viewport(), spec.surface())?
        {
            return Ok(false);
        }
        if !self.retiring_restored_bootstraps.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "materialized restored bootstrap repeated one retirement lifetime",
            ));
        }
        Ok(true)
    }

    pub(super) fn retire_intermediate_replacement(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        self.effects.forget_pending_native(exact);
        if !self.deferred_replacement_retirements.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native replacement lineage repeated one intermediate lifetime",
            ));
        }
        Ok(())
    }

    pub(super) fn quarantine_failed_native_lifetime(
        &mut self,
        exact: ExactNativeViewport,
        provisional_routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
    ) -> Result<bool, NativeRuntimeError> {
        let Some(effect_route) = self.effects.route_for_provisional_retirement(exact) else {
            return Ok(false);
        };
        if !effect_route.is_failed() {
            return Ok(false);
        }

        if let Some(pending) = self.pending_registrations.get(&exact).copied() {
            if pending.surface != effect_route.surface()
                || pending.adopted_replacement_binding() != Some(effect_route.core())
            {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "failed native initialization changed its pending core authority",
                ));
            }
            self.pending_registrations.remove(&exact);
        }
        if let Some(route) = provisional_routes.remove(&exact.viewport())
            && (route.exact() != exact
                || route.surface() != effect_route.surface()
                || route.core() != effect_route.core())
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "failed native initialization changed its provisional route",
            ));
        }
        self.effects.retain_initialization_receipts(exact)?;
        if !self.quarantined_native_lifetimes.insert(exact) {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native initialization repeated one quarantine lifetime",
            ));
        }
        Ok(true)
    }

    pub(super) fn route_for_semantic_input(
        &self,
        routes: &BTreeMap<ViewportId, BoundNativeRoute>,
        native: NativeViewportBinding,
        replacement_lineages: &DeferredReplacementLineagePlan,
    ) -> Result<Option<BoundNativeRoute>, NativeRuntimeError> {
        let route = routes
            .get(&native.viewport_id())
            .filter(|route| route.native_binding() == native)
            .copied();
        if route.is_some()
            || self.route_less_replacement(exact_native(native))
            || self.effects.has_provisional_native(exact_native(native))
            || replacement_lineages.is_route_less(exact_native(native))
        {
            return Ok(route);
        }
        Err(NativeRuntimeError::IngressUnavailable(
            "native semantic edge has no exact current core binding route",
        ))
    }

    pub(super) fn route_less_replacement(&self, exact: ExactNativeViewport) -> bool {
        self.pending_registrations.contains_key(&exact)
            || self.deferred_replacement_retirements.contains(&exact)
            || self.quarantined_native_lifetimes.contains(&exact)
            || self.retiring_restored_bootstraps.contains(&exact)
            || self
                .effects
                .restored_create_is_materialized(exact.viewport())
    }

    pub(super) fn record_retirement_quiescence(
        &mut self,
        presentations: &mut NativePresentationLedger,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        let Some(binding) = self.retired_routes.get(&exact).copied() else {
            if self.retiring_restored_bootstraps.remove(&exact) {
                self.effects.retire_cleanup_native(exact);
                return Ok(());
            }
            if self.deferred_replacement_retirements.remove(&exact) {
                self.effects.retire_initialization_receipts(exact);
                self.effects.retire_cleanup_native(exact);
                return Ok(());
            }
            if self.external_retirements.remove(&exact) {
                self.effects.retire_cleanup_native(exact);
                return Ok(());
            }
            return Err(NativeRuntimeError::IngressUnavailable(
                "native retirement quiescence omitted its retired route classification",
            ));
        };
        self.recorder
            .as_mut()
            .expect("provider enrolled above")
            .record_platform_binding_quiescence(binding)?;
        presentations.retirement_quiesced(exact);
        self.effects.retire_initialization_receipts(exact);
        self.effects.retire_cleanup_binding(binding, exact);
        Ok(())
    }

    pub(super) fn record_external_retirement(
        &mut self,
        exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        if self.external_retirements.insert(exact) {
            Ok(())
        } else {
            Err(NativeRuntimeError::IngressUnavailable(
                "external native viewport repeated one retirement lifetime",
            ))
        }
    }

    pub(super) fn insert_pending_registration(
        &mut self,
        registration: PendingNativeRegistration,
    ) -> Result<(), NativeRuntimeError> {
        if self.pending_registrations.contains_key(&registration.exact)
            || self.pending_registrations.values().any(|pending| {
                pending.surface == registration.surface
                    || pending.exact.viewport() == registration.exact.viewport()
            })
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "native viewport registration repeated a pending lifetime or surface",
            ));
        }
        self.pending_registrations
            .insert(registration.exact, registration);
        Ok(())
    }

    pub(super) fn insert_deferred_replacement_registration(
        &mut self,
        registration: PendingNativeRegistration,
        predecessor_exact: ExactNativeViewport,
    ) -> Result<(), NativeRuntimeError> {
        let predecessor =
            registration
                .predecessor_binding()
                .ok_or(NativeRuntimeError::IngressUnavailable(
                    "deferred replacement registration omitted its predecessor binding",
                ))?;
        if self.pending_registrations.contains_key(&registration.exact)
            || self.pending_registrations.values().any(|pending| {
                let conflicts = pending.surface == registration.surface
                    || pending.exact.viewport() == registration.exact.viewport();
                let is_exact_predecessor = pending.exact == predecessor_exact
                    && pending.committed_binding() == Some(predecessor);
                conflicts && !is_exact_predecessor
            })
        {
            return Err(NativeRuntimeError::IngressUnavailable(
                "deferred native replacement conflicts with another pending lifetime",
            ));
        }
        self.pending_registrations
            .insert(registration.exact, registration);
        Ok(())
    }

    pub(super) fn pending_registration_for_surface(
        &self,
        surface: SurfaceId,
    ) -> Option<PendingNativeRegistration> {
        self.pending_registrations
            .values()
            .copied()
            .find(|pending| pending.surface == surface)
    }

    pub(super) fn advance_pending_registrations(
        &mut self,
        dockspace: &Dockspace,
        committed_through: Option<BackendIngressOrdinal>,
    ) -> Result<(), NativeRuntimeError> {
        for pending in self.pending_registrations.values_mut() {
            match pending.phase {
                PendingNativeRegistrationPhase::AwaitingPredecessorRetirement(predecessor) => {
                    let current = dockspace.native_viewport_binding(pending.surface);
                    let retained_replacement =
                        dockspace.backend_recovery_replacement_binding(pending.surface);
                    if current.is_some_and(|binding| {
                        binding != predecessor && Some(binding) != retained_replacement
                    }) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "deferred native replacement observed an unrelated core binding",
                        ));
                    }
                }
                PendingNativeRegistrationPhase::AdoptedReplacement {
                    predecessor,
                    replacement,
                } => {
                    let current = dockspace.native_viewport_binding(pending.surface);
                    let retained = dockspace.backend_recovery_replacement_binding(pending.surface);
                    if current
                        .is_some_and(|binding| binding != predecessor && binding != replacement)
                        || (current.is_none() && retained != Some(replacement))
                    {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "adopted native replacement lost its exact core authority",
                        ));
                    }
                }
                PendingNativeRegistrationPhase::Staged(registration_ordinal)
                    if committed_through
                        .is_some_and(|committed| registration_ordinal <= committed) =>
                {
                    let binding = dockspace.native_viewport_binding(pending.surface).ok_or(
                        NativeRuntimeError::IngressUnavailable(
                            "committed native registration did not mint its core binding",
                        ),
                    )?;
                    pending.phase = PendingNativeRegistrationPhase::Committed(binding);
                }
                PendingNativeRegistrationPhase::Staged(_) => {
                    if dockspace.native_viewport_binding(pending.surface).is_some() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "native registration became visible before its committed ordinal",
                        ));
                    }
                }
                PendingNativeRegistrationPhase::Committed(binding) => {
                    if dockspace.native_viewport_binding(pending.surface) != Some(binding) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "pending native registration lost or changed its exact core binding",
                        ));
                    }
                }
            }
        }
        Ok(())
    }

    pub(super) fn plan_deferred_replacement_lineages(
        &self,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
    ) -> Result<DeferredReplacementLineagePlan, NativeRuntimeError> {
        let live = ingress
            .platform()
            .inventory()
            .iter()
            .copied()
            .map(exact_native)
            .collect::<BTreeSet<_>>();
        let mut retired_by_viewport = BTreeMap::<ViewportId, Vec<ExactNativeViewport>>::new();
        for retirement in
            ingress
                .ordered()
                .records()
                .iter()
                .filter_map(|record| match record.event() {
                    NativeIngressEvent::Retirement(retirement) => {
                        Some(exact_native(retirement.binding()))
                    }
                    _ => None,
                })
        {
            retired_by_viewport
                .entry(retirement.viewport())
                .or_default()
                .push(retirement);
        }

        self.plan_deferred_replacement_lineages_from_exacts(retired_by_viewport, &live, configured)
    }

    pub(super) fn plan_deferred_replacement_lineages_from_exacts(
        &self,
        retired_by_viewport: BTreeMap<ViewportId, Vec<ExactNativeViewport>>,
        live: &BTreeSet<ExactNativeViewport>,
        configured: &NativeViewportRoster,
    ) -> Result<DeferredReplacementLineagePlan, NativeRuntimeError> {
        let mut plan = DeferredReplacementLineagePlan::default();
        for (viewport, retired) in retired_by_viewport {
            let authorities = retired
                .iter()
                .enumerate()
                .filter_map(|(index, exact)| {
                    self.replacement_lineage_authority(*exact)
                        .map(|authority| (index, *exact, authority))
                })
                .collect::<Vec<_>>();
            if authorities.len() > 1 {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "native replacement lineage contained multiple core authorities",
                ));
            }
            let Some((authority_index, authority_exact, (surface, predecessor))) =
                authorities.first().copied()
            else {
                continue;
            };
            let Some(spec) = configured.get(viewport) else {
                continue;
            };
            if !configured_child_accepts_predecessor(spec, surface, predecessor) {
                continue;
            }

            let retired_exact = retired.iter().copied().collect::<BTreeSet<_>>();
            let mut live_successors = live.iter().copied().filter(|candidate| {
                candidate.viewport() == viewport && !retired_exact.contains(candidate)
            });
            let live_successor = live_successors.next();
            if live_successors.next().is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "atomic native inventory repeated one replacement viewport lifetime",
                ));
            }

            plan.intermediate_retirements
                .extend(retired.iter().skip(authority_index + 1).copied());
            plan.route_less_lifetimes
                .extend(retired.iter().skip(authority_index + 1).copied());
            if let Some(successor) = live_successor {
                let registration = PendingNativeRegistration::awaiting_predecessor(
                    successor,
                    surface,
                    predecessor,
                );
                if plan
                    .successor_after_retirement
                    .insert(authority_exact, registration)
                    .is_some()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "native replacement lineage repeated its authority retirement",
                    ));
                }
                plan.route_less_lifetimes.insert(successor);
                if plan
                    .live_keepalives
                    .insert(viewport, RouteLessNativeKeepalive::new(successor, surface))
                    .is_some()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "native replacement lineage repeated one live keepalive viewport",
                    ));
                }
            } else {
                plan.terminal_viewports.insert(viewport);
            }
        }
        Ok(plan)
    }

    pub(super) fn replacement_lineage_authority(
        &self,
        exact: ExactNativeViewport,
    ) -> Option<(SurfaceId, ViewportBinding)> {
        self.routes
            .get(&exact.viewport())
            .copied()
            .filter(|route| route.exact() == exact)
            .map(|route| (route.surface(), route.core()))
            .or_else(|| {
                let pending = self.pending_registrations.get(&exact).copied()?;
                pending
                    .committed_binding()
                    .or_else(|| pending.predecessor_binding())
                    .map(|binding| (pending.surface, binding))
            })
    }

    pub(super) fn reconcile_snapshot_routes(
        &mut self,
        dockspace: &Dockspace,
        ingress: &NativeHostIngress,
        configured: &NativeViewportRoster,
        retirements: &[RetiredNativeRoute],
        routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
        provisional_routes: &mut BTreeMap<ViewportId, BoundNativeRoute>,
    ) -> Result<(), NativeRuntimeError> {
        let live = ingress
            .platform()
            .inventory()
            .iter()
            .map(|binding| (binding.viewport_id(), *binding))
            .collect::<BTreeMap<_, _>>();
        for (viewport, route) in routes.iter() {
            if live.get(viewport) != Some(&route.native_binding) {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "atomic native snapshot changed a routed lifetime without an ordered retirement",
                ));
            }
        }

        if let Some(native_binding) = live.get(&ViewportId::ROOT).copied()
            && !routes.contains_key(&ViewportId::ROOT)
        {
            let root = configured.root();
            let exact = exact_native(native_binding);
            if !retirements
                .iter()
                .any(|retired| retired.surface() == root.surface())
            {
                let pending = self.pending_registrations.get(&exact).copied();
                if self
                    .pending_registration_for_surface(root.surface())
                    .is_some_and(|registration| registration.exact != exact)
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "root registration native lifetime changed before routing",
                    ));
                }
                if let Some(registration) = pending
                    && registration.surface != root.surface()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "root registration changed its declared surface",
                    ));
                }
                let core = pending
                    .and_then(PendingNativeRegistration::committed_binding)
                    .or_else(|| {
                        pending
                            .is_none()
                            .then(|| dockspace.native_viewport_binding(root.surface()))
                            .flatten()
                    });
                if let Some(core) = core {
                    if Some(core.token()) != root.bootstrap_token() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "core root binding token differs from the bootstrap token",
                        ));
                    }
                    routes.insert(
                        ViewportId::ROOT,
                        BoundNativeRoute {
                            native_binding,
                            exact,
                            surface: root.surface(),
                            core,
                        },
                    );
                    self.pending_registrations.remove(&exact);
                } else if pending.is_none() {
                    let ordinal = dockspace.record_backend_viewport_registration(
                        self.recorder
                            .as_mut()
                            .expect("the provider was enrolled above"),
                        root.surface(),
                        root.bootstrap_token()
                            .ok_or(NativeRuntimeError::MissingRootViewport)?,
                        ViewportRole::Root,
                        None,
                    )?;
                    self.insert_pending_registration(PendingNativeRegistration::staged(
                        exact,
                        root.surface(),
                        ordinal,
                    ))?;
                }
            }
        }

        for (viewport, native_binding) in live {
            if viewport == ViewportId::ROOT {
                continue;
            }
            if let Some(route) = routes.get(&viewport) {
                if route.native_binding != native_binding {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "native viewport incarnation changed without retirement",
                    ));
                }
                continue;
            }

            let exact = exact_native(native_binding);
            if self.quarantined_native_lifetimes.contains(&exact) {
                continue;
            }
            let restored = configured.get(viewport).filter(|spec| {
                spec.role() == ViewportRole::Child && spec.bootstrap_token().is_some()
            });
            let pending = self.pending_registrations.get(&exact).copied();
            if let Some(spec) = restored {
                if self
                    .pending_registration_for_surface(spec.surface())
                    .is_some_and(|registration| registration.exact != exact)
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "restored registration native lifetime changed before routing",
                    ));
                }
                if let Some(registration) = pending
                    && registration.surface != spec.surface()
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "restored registration changed its declared surface",
                    ));
                }
            }
            if let Some(predecessor) = pending.and_then(|registration| {
                registration
                    .predecessor_binding()
                    .map(|predecessor| (registration.surface, predecessor))
            }) {
                let (surface, predecessor) = predecessor;
                let current = dockspace.native_viewport_binding(surface);
                if current == Some(predecessor) {
                    continue;
                }
                let Some(expected_replacement) =
                    pending.and_then(PendingNativeRegistration::adopted_replacement_binding)
                else {
                    let retained_replacement =
                        dockspace.backend_recovery_replacement_binding(surface);
                    if current.is_some_and(|binding| Some(binding) != retained_replacement) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "deferred native replacement observed an unrelated core binding",
                        ));
                    }
                    continue;
                };
                let Some(effect_route) = self.effects.route_for_new_binding(native_binding) else {
                    if current.is_some() {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "deferred native replacement observed an unrelated core binding",
                        ));
                    }
                    continue;
                };
                let core = effect_route.core();
                if effect_route.surface() != surface
                    || core != expected_replacement
                    || current.is_some_and(|binding| binding != core)
                {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "deferred native replacement changed its core replacement authority",
                    ));
                }
                let route = BoundNativeRoute {
                    native_binding,
                    exact,
                    surface,
                    core,
                };
                if effect_route.is_publishable() {
                    if !self.effects.publish_route(exact) {
                        return Err(NativeRuntimeError::IngressUnavailable(
                            "publishable native replacement lost its initialization proof",
                        ));
                    }
                    routes.insert(viewport, route);
                    self.pending_registrations.remove(&exact);
                } else {
                    provisional_routes.insert(viewport, route);
                }
                continue;
            }
            let effect_route = self.effects.route_for_new_binding(native_binding);
            if effect_route.is_some() && pending.is_some() {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "one native lifetime has both create-effect and bootstrap route authority",
                ));
            }
            let pending_route = pending.and_then(|registration| {
                registration
                    .committed_binding()
                    .map(|binding| (registration.surface, binding))
            });
            let effect_binding = effect_route.map(|route| (route.surface(), route.core()));
            let Some((surface, core)) = effect_binding.or_else(|| {
                let spec = restored?;
                pending_route.or_else(|| {
                    pending
                        .is_none()
                        .then(|| {
                            existing_restored_core_route(dockspace, spec.surface(), retirements)
                        })
                        .flatten()
                })
            }) else {
                if let Some(spec) = restored
                    && !retirements
                        .iter()
                        .any(|retired| retired.surface() == spec.surface())
                    && pending.is_none()
                {
                    let recovery =
                        spec.recovery_bootstrap()
                            .ok_or(NativeRuntimeError::IngressUnavailable(
                                "restored child omitted its recovery bootstrap",
                            ))?;
                    let ordinal = dockspace.record_backend_child_viewport_bootstrap(
                        self.recorder
                            .as_mut()
                            .expect("the provider was enrolled above"),
                        spec.surface(),
                        spec.bootstrap_token().expect("restored child has a token"),
                        recovery,
                    )?;
                    self.insert_pending_registration(PendingNativeRegistration::staged(
                        exact,
                        spec.surface(),
                        ordinal,
                    ))?;
                }
                // Unknown physical children remain outside dockspace authority. A restored
                // child joins only after its core registration commits on a prior cycle.
                continue;
            };
            if let Some(spec) = restored
                && effect_route.is_none()
                && Some(core.token()) != spec.bootstrap_token()
            {
                return Err(NativeRuntimeError::IngressUnavailable(
                    "core child binding token differs from the restored token",
                ));
            }
            let route = BoundNativeRoute {
                native_binding,
                exact,
                surface,
                core,
            };
            if effect_route.is_some_and(|route| !route.is_publishable()) {
                provisional_routes.insert(viewport, route);
            } else {
                if effect_route.is_some() && !self.effects.publish_route(exact) {
                    return Err(NativeRuntimeError::IngressUnavailable(
                        "publishable native create lost its initialization proof",
                    ));
                }
                routes.insert(viewport, route);
                self.pending_registrations.remove(&exact);
            }
        }
        Ok(())
    }
}
