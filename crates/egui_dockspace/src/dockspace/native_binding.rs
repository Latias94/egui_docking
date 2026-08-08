//! Exact native-viewport to core-binding routing.
//!
//! The native runtime owns viewport identities and incarnations. The docking
//! core owns [`ViewportBinding`]. This module joins those independently opaque
//! identities without deriving either one from a surface or recycling a route
//! after a native viewport is recreated.

use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;

use dockspace::backend::ingress::BackendIngressLease;
use dockspace::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
use dockspace::transition::{EngineTransition, InputOutcome};
use dockspace::viewport::{ViewportBinding, WindowIncarnation, WindowToken};
use egui::ViewportId;
use thiserror::Error;

/// Opaque typed failure at the native/core binding boundary.
///
/// Detailed route identities remain private to the unpublished native runtime
/// seam so the ordinary egui facade does not stabilize protocol internals.
#[derive(Debug, Error)]
#[error("{message}")]
pub struct NativeBindingError {
    message: String,
}

impl From<NativeBindingRegistryError> for NativeBindingError {
    fn from(error: NativeBindingRegistryError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

impl From<NativeRouteLookupError> for NativeBindingError {
    fn from(error: NativeRouteLookupError) -> Self {
        Self {
            message: error.to_string(),
        }
    }
}

/// Runtime-owned non-wrapping incarnation of one native viewport slot.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct NativeViewportIncarnation(u64);

impl NativeViewportIncarnation {
    /// Wraps a runtime-owned native viewport incarnation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the diagnostic representation supplied by the runtime.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    /// Advances the incarnation without permitting integer-wrap ABA.
    #[must_use]
    pub const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Exact identity of one native viewport lifetime.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ExactNativeViewport {
    viewport: ViewportId,
    incarnation: NativeViewportIncarnation,
}

impl ExactNativeViewport {
    /// Joins one native viewport slot to one exact runtime incarnation.
    #[must_use]
    pub const fn new(viewport: ViewportId, incarnation: NativeViewportIncarnation) -> Self {
        Self {
            viewport,
            incarnation,
        }
    }

    /// Returns the runtime-owned viewport slot.
    #[must_use]
    pub const fn viewport(self) -> ViewportId {
        self.viewport
    }

    /// Returns the runtime-owned viewport incarnation.
    #[must_use]
    pub const fn incarnation(self) -> NativeViewportIncarnation {
        self.incarnation
    }
}

/// Exact backend and workspace scope in which native routes were observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct NativeRouteAuthority {
    provider: BackendIngressLease,
    workspace_epoch: WorkspaceEpoch,
}

impl NativeRouteAuthority {
    /// Captures the exact joined provider and workspace epoch for one roster.
    #[must_use]
    pub(super) const fn new(
        provider: BackendIngressLease,
        workspace_epoch: WorkspaceEpoch,
    ) -> Self {
        Self {
            provider,
            workspace_epoch,
        }
    }

    #[cfg(test)]
    pub(super) const fn provider(self) -> BackendIngressLease {
        self.provider
    }

    #[cfg(test)]
    pub(super) const fn workspace_epoch(self) -> WorkspaceEpoch {
        self.workspace_epoch
    }
}

/// Exact, comparable identity extracted from an unforgeable core binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(super) struct CoreBindingIdentity {
    authority_domain: EngineAuthorityDomainId,
    workspace_epoch: WorkspaceEpoch,
    surface: SurfaceId,
    token: WindowToken,
    incarnation: WindowIncarnation,
}

impl CoreBindingIdentity {
    const fn from_viewport_binding(binding: ViewportBinding) -> Self {
        Self {
            authority_domain: binding.authority_domain(),
            workspace_epoch: binding.epoch(),
            surface: binding.surface(),
            token: binding.token(),
            incarnation: binding.incarnation(),
        }
    }
}

trait ExactCoreBinding: Copy + Debug + PartialEq + Eq {
    fn exact_identity(self) -> CoreBindingIdentity;
}

impl ExactCoreBinding for ViewportBinding {
    fn exact_identity(self) -> CoreBindingIdentity {
        CoreBindingIdentity::from_viewport_binding(self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RouteRecord<B> {
    native: ExactNativeViewport,
    declared_surface: SurfaceId,
    core: B,
}

impl<B: ExactCoreBinding> RouteRecord<B> {
    const fn new(native: ExactNativeViewport, declared_surface: SurfaceId, core: B) -> Self {
        Self {
            native,
            declared_surface,
            core,
        }
    }

    fn core_identity(self) -> CoreBindingIdentity {
        self.core.exact_identity()
    }
}

/// One exact live native/core route.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeCoreRoute(RouteRecord<ViewportBinding>);

impl NativeCoreRoute {
    /// Captures a runtime-observed surface and its exact core binding.
    ///
    /// Candidate reconciliation rejects this value when `surface` differs from
    /// `binding.surface()`.
    #[must_use]
    pub const fn new(
        native: ExactNativeViewport,
        surface: SurfaceId,
        binding: ViewportBinding,
    ) -> Self {
        Self(RouteRecord::new(native, surface, binding))
    }

    /// Returns the exact native viewport lifetime.
    #[must_use]
    pub const fn native(self) -> ExactNativeViewport {
        self.0.native
    }

    /// Returns the logical surface explicitly reported by the runtime roster.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.0.declared_surface
    }

    /// Returns the exact unforgeable core binding.
    #[must_use]
    pub const fn core(self) -> ViewportBinding {
        self.0.core
    }
}

/// Complete native viewport route roster frozen before one hosted cycle.
///
/// Retirements are exact lifetime tombstones, not absences inferred from the
/// live roster. The adapter consumes this value atomically with the hosted
/// cycle candidate.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NativeBindingRoster {
    routes: Vec<NativeCoreRoute>,
    retirements: Vec<ExactNativeViewport>,
}

impl NativeBindingRoster {
    /// Creates one complete live route roster and its exact retirement facts.
    #[must_use]
    pub fn new(
        routes: impl IntoIterator<Item = NativeCoreRoute>,
        retirements: impl IntoIterator<Item = ExactNativeViewport>,
    ) -> Self {
        Self {
            routes: routes.into_iter().collect(),
            retirements: retirements.into_iter().collect(),
        }
    }

    /// Creates an explicitly empty native route roster.
    #[must_use]
    pub const fn empty() -> Self {
        Self {
            routes: Vec::new(),
            retirements: Vec::new(),
        }
    }

    pub(super) fn into_parts(self) -> (Vec<NativeCoreRoute>, Vec<ExactNativeViewport>) {
        (self.routes, self.retirements)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct RouteState<B> {
    live_by_native: BTreeMap<ExactNativeViewport, RouteRecord<B>>,
    native_by_core: BTreeMap<CoreBindingIdentity, ExactNativeViewport>,
    native_by_surface: BTreeMap<SurfaceId, ExactNativeViewport>,
    retired_by_native: BTreeMap<ExactNativeViewport, RouteRecord<B>>,
    retired_by_core: BTreeMap<CoreBindingIdentity, ExactNativeViewport>,
}

impl<B> RouteState<B> {
    fn empty() -> Self {
        Self {
            live_by_native: BTreeMap::new(),
            native_by_core: BTreeMap::new(),
            native_by_surface: BTreeMap::new(),
            retired_by_native: BTreeMap::new(),
            retired_by_core: BTreeMap::new(),
        }
    }
}

struct ExactRouteRegistry<B> {
    identity: Arc<()>,
    authority: NativeRouteAuthority,
    revision: u64,
    state: RouteState<B>,
}

impl<B: ExactCoreBinding> ExactRouteRegistry<B> {
    fn new(authority: NativeRouteAuthority) -> Self {
        Self {
            identity: Arc::new(()),
            authority,
            revision: 0,
            state: RouteState::empty(),
        }
    }

    fn reconcile_candidate(
        &self,
        authority: NativeRouteAuthority,
        routes: impl IntoIterator<Item = RouteRecord<B>>,
        retirements: impl IntoIterator<Item = ExactNativeViewport>,
    ) -> Result<ExactRouteCandidate<B>, NativeBindingRegistryError> {
        self.validate_authority(authority)?;
        let next_revision = self
            .revision
            .checked_add(1)
            .ok_or(NativeBindingRegistryError::RevisionExhausted)?;
        let mut next = RouteState::empty();
        next.retired_by_native = self.state.retired_by_native.clone();
        next.retired_by_core = self.state.retired_by_core.clone();
        let mut live_by_viewport = BTreeMap::<ViewportId, ExactNativeViewport>::new();
        let mut retirements = retirements.into_iter().collect::<Vec<_>>();
        retirements.sort_unstable();
        for pair in retirements.windows(2) {
            if pair[0] == pair[1] {
                return Err(NativeBindingRegistryError::DuplicateRetirement { native: pair[0] });
            }
        }

        for route in routes {
            self.validate_route(authority, route)?;
            let native = route.native;
            let core = route.core_identity();

            if self.state.retired_by_native.contains_key(&native) {
                return Err(NativeBindingRegistryError::RetiredNativeBinding { native });
            }
            if self.state.retired_by_core.contains_key(&core) {
                return Err(NativeBindingRegistryError::RetiredCoreBinding { core });
            }
            if let Some(previous) = self.state.live_by_native.get(&native)
                && previous != &route
            {
                return Err(NativeBindingRegistryError::ExactNativeRouteChanged { native });
            }
            if let Some(previous) = self.state.native_by_core.get(&core)
                && *previous != native
            {
                return Err(NativeBindingRegistryError::ExactCoreRouteChanged {
                    core,
                    previous: *previous,
                    submitted: native,
                });
            }
            if next.live_by_native.contains_key(&native) {
                return Err(NativeBindingRegistryError::DuplicateNativeBinding { native });
            }
            if let Some(previous) = live_by_viewport.get(&native.viewport()) {
                return Err(NativeBindingRegistryError::DuplicateLiveNativeViewport {
                    viewport: native.viewport(),
                    first: *previous,
                    second: native,
                });
            }
            if let Some(previous) = next.native_by_core.get(&core) {
                return Err(NativeBindingRegistryError::DuplicateCoreBinding {
                    core,
                    first: *previous,
                    second: native,
                });
            }
            if let Some(previous) = next.native_by_surface.get(&route.declared_surface) {
                return Err(NativeBindingRegistryError::DuplicateSurface {
                    surface: route.declared_surface,
                    first: *previous,
                    second: native,
                });
            }

            live_by_viewport.insert(native.viewport(), native);
            next.live_by_native.insert(native, route);
            next.native_by_core.insert(core, native);
            next.native_by_surface
                .insert(route.declared_surface, native);
        }

        for native in retirements {
            if next.live_by_native.contains_key(&native) {
                return Err(NativeBindingRegistryError::RetiredBindingStillLive { native });
            }
            if self.state.retired_by_native.contains_key(&native) {
                continue;
            }
            let Some(route) = self.state.live_by_native.get(&native) else {
                return Err(NativeBindingRegistryError::UnknownRetirement { native });
            };
            let core = route.core_identity();
            next.retired_by_native.insert(native, *route);
            next.retired_by_core.insert(core, native);
        }

        for (native, route) in &self.state.live_by_native {
            if next.live_by_native.contains_key(native) {
                continue;
            }
            if !next.retired_by_native.contains_key(native) {
                return Err(NativeBindingRegistryError::MissingRetirementTombstone {
                    native: *native,
                    core: route.core_identity(),
                });
            }
        }

        Ok(ExactRouteCandidate {
            registry_identity: Arc::clone(&self.identity),
            authority,
            base_revision: self.revision,
            next_revision,
            state: next,
        })
    }

    #[cfg(test)]
    fn commit(
        &mut self,
        candidate: ExactRouteCandidate<B>,
    ) -> Result<(), NativeBindingRegistryError> {
        if !Arc::ptr_eq(&self.identity, &candidate.registry_identity) {
            return Err(NativeBindingRegistryError::CandidateRegistryMismatch);
        }
        self.validate_authority(candidate.authority)?;
        if candidate.base_revision != self.revision {
            return Err(NativeBindingRegistryError::CandidateRevisionMismatch {
                expected: self.revision,
                submitted: candidate.base_revision,
            });
        }

        self.revision = candidate.next_revision;
        self.state = candidate.state;
        Ok(())
    }

    #[cfg(test)]
    fn resolve_native(
        &self,
        authority: NativeRouteAuthority,
        native: ExactNativeViewport,
        use_case: NativeRouteUse,
    ) -> Result<RouteRecord<B>, NativeRouteLookupError> {
        self.validate_lookup_authority(authority)?;
        if let Some(route) = self.state.live_by_native.get(&native) {
            return Ok(*route);
        }
        if self.state.retired_by_native.contains_key(&native) {
            return Err(NativeRouteLookupError::NativeBindingRetired { native, use_case });
        }
        Err(NativeRouteLookupError::NativeBindingUnknown { native, use_case })
    }

    #[cfg(test)]
    fn resolve_core(
        &self,
        authority: NativeRouteAuthority,
        core: B,
    ) -> Result<ExactNativeViewport, NativeRouteLookupError> {
        self.validate_lookup_authority(authority)?;
        let core = core.exact_identity();
        if let Some(native) = self.state.native_by_core.get(&core) {
            return Ok(*native);
        }
        if self.state.retired_by_core.contains_key(&core) {
            return Err(NativeRouteLookupError::CoreBindingRetired { core });
        }
        Err(NativeRouteLookupError::CoreBindingUnknown { core })
    }

    #[cfg(test)]
    fn canonical_roster(&self) -> impl Iterator<Item = RouteRecord<B>> + '_ {
        self.state.live_by_native.values().copied()
    }

    fn validate_authority(
        &self,
        submitted: NativeRouteAuthority,
    ) -> Result<(), NativeBindingRegistryError> {
        if submitted == self.authority {
            Ok(())
        } else {
            Err(NativeBindingRegistryError::AuthorityMismatch {
                expected: self.authority,
                submitted,
            })
        }
    }

    #[cfg(test)]
    fn validate_lookup_authority(
        &self,
        submitted: NativeRouteAuthority,
    ) -> Result<(), NativeRouteLookupError> {
        if submitted == self.authority {
            Ok(())
        } else {
            Err(NativeRouteLookupError::AuthorityMismatch {
                expected: self.authority,
                submitted,
            })
        }
    }

    fn validate_route(
        &self,
        authority: NativeRouteAuthority,
        route: RouteRecord<B>,
    ) -> Result<(), NativeBindingRegistryError> {
        let core = route.core_identity();
        let expected_domain = authority.provider.authority_domain();
        if core.authority_domain != expected_domain {
            return Err(NativeBindingRegistryError::CoreProviderDomainMismatch {
                native: route.native,
                expected: expected_domain,
                submitted: core.authority_domain,
            });
        }
        if core.workspace_epoch != authority.workspace_epoch {
            return Err(NativeBindingRegistryError::CoreWorkspaceEpochMismatch {
                native: route.native,
                expected: authority.workspace_epoch,
                submitted: core.workspace_epoch,
            });
        }
        if core.surface != route.declared_surface {
            return Err(NativeBindingRegistryError::SurfaceBindingMismatch {
                native: route.native,
                declared: route.declared_surface,
                bound: core.surface,
            });
        }
        Ok(())
    }
}

struct ExactRouteCandidate<B> {
    registry_identity: Arc<()>,
    authority: NativeRouteAuthority,
    base_revision: u64,
    next_revision: u64,
    state: RouteState<B>,
}

/// Candidate exact roster which has not yet changed committed routing.
#[must_use = "a validated native binding candidate must be committed or discarded"]
pub(super) struct NativeBindingCandidate(ExactRouteCandidate<ViewportBinding>);

impl NativeBindingCandidate {
    pub(super) fn input_receiver_surface(
        &self,
        native: ExactNativeViewport,
    ) -> Result<SurfaceId, NativeRouteLookupError> {
        resolve_input_candidate(&self.0, native).map(|route| route.declared_surface)
    }

    pub(super) fn resolve_input_receiver(
        &self,
        native: ExactNativeViewport,
        current: Option<ViewportBinding>,
    ) -> Result<NativeCoreRoute, NativeRouteLookupError> {
        resolve_current_input_candidate(&self.0, native, current).map(NativeCoreRoute)
    }

    pub(super) fn callback_surface(
        &self,
        native: ExactNativeViewport,
    ) -> Result<SurfaceId, NativeRouteLookupError> {
        resolve_candidate(&self.0, native, NativeRouteUse::Callback)
            .map(|route| route.declared_surface)
    }

    pub(super) fn presentation_surface(
        &self,
        native: ExactNativeViewport,
    ) -> Result<SurfaceId, NativeRouteLookupError> {
        resolve_candidate(&self.0, native, NativeRouteUse::Presentation)
            .map(|route| route.declared_surface)
    }

    pub(super) fn resolve_callback(
        &self,
        native: ExactNativeViewport,
        current: Option<ViewportBinding>,
    ) -> Result<NativeCoreRoute, NativeRouteLookupError> {
        resolve_current_candidate(&self.0, native, current, NativeRouteUse::Callback)
            .map(NativeCoreRoute)
    }

    pub(super) fn resolve_presentation(
        &self,
        native: ExactNativeViewport,
        current: Option<ViewportBinding>,
    ) -> Result<NativeCoreRoute, NativeRouteLookupError> {
        resolve_current_candidate(&self.0, native, current, NativeRouteUse::Presentation)
            .map(NativeCoreRoute)
    }
}

fn resolve_current_input_candidate<B: ExactCoreBinding>(
    candidate: &ExactRouteCandidate<B>,
    native: ExactNativeViewport,
    current: Option<B>,
) -> Result<RouteRecord<B>, NativeRouteLookupError> {
    let route = resolve_input_candidate(candidate, native)?;
    if current != Some(route.core) {
        return Err(NativeRouteLookupError::CoreBindingNoLongerCurrent {
            native,
            expected: route.core_identity(),
            observed: current.map(ExactCoreBinding::exact_identity),
            use_case: NativeRouteUse::Callback,
        });
    }
    Ok(route)
}

fn resolve_input_candidate<B: ExactCoreBinding>(
    candidate: &ExactRouteCandidate<B>,
    native: ExactNativeViewport,
) -> Result<RouteRecord<B>, NativeRouteLookupError> {
    candidate
        .state
        .live_by_native
        .get(&native)
        .or_else(|| candidate.state.retired_by_native.get(&native))
        .copied()
        .ok_or(NativeRouteLookupError::NativeBindingUnknown {
            native,
            use_case: NativeRouteUse::Callback,
        })
}

fn resolve_current_candidate<B: ExactCoreBinding>(
    candidate: &ExactRouteCandidate<B>,
    native: ExactNativeViewport,
    current: Option<B>,
    use_case: NativeRouteUse,
) -> Result<RouteRecord<B>, NativeRouteLookupError> {
    let route = resolve_candidate(candidate, native, use_case)?;
    if current != Some(route.core) {
        return Err(NativeRouteLookupError::CoreBindingNoLongerCurrent {
            native,
            expected: route.core_identity(),
            observed: current.map(ExactCoreBinding::exact_identity),
            use_case,
        });
    }
    Ok(route)
}

fn resolve_candidate<B: ExactCoreBinding>(
    candidate: &ExactRouteCandidate<B>,
    native: ExactNativeViewport,
    use_case: NativeRouteUse,
) -> Result<RouteRecord<B>, NativeRouteLookupError> {
    if let Some(route) = candidate.state.live_by_native.get(&native) {
        return Ok(*route);
    }
    if candidate.state.retired_by_native.contains_key(&native) {
        return Err(NativeRouteLookupError::NativeBindingRetired { native, use_case });
    }
    Err(NativeRouteLookupError::NativeBindingUnknown { native, use_case })
}

/// Exact registry mutation preflighted before the core transaction publishes.
pub(super) struct PreparedNativeBindingCommit {
    registry_identity: Arc<()>,
    base_authority: NativeRouteAuthority,
    next_authority: NativeRouteAuthority,
    base_revision: u64,
    next_revision: u64,
    state: RouteState<ViewportBinding>,
}

/// Committed exact native/core route registry.
///
/// Every mutation is a complete-roster reconcile followed by an atomic commit.
/// No API resolves callbacks by viewport slot or logical surface alone.
pub(super) struct NativeBindingRegistry {
    inner: ExactRouteRegistry<ViewportBinding>,
}

impl NativeBindingRegistry {
    /// Creates an empty registry scoped to one exact joined provider and epoch.
    #[must_use]
    pub(super) fn new(provider: BackendIngressLease, workspace_epoch: WorkspaceEpoch) -> Self {
        Self {
            inner: ExactRouteRegistry::new(NativeRouteAuthority::new(provider, workspace_epoch)),
        }
    }

    /// Rebinds an unchanged physical route roster to a joined provider successor.
    ///
    /// Provider replacement does not recreate native windows or core bindings.
    /// Keeping the route state preserves exact native incarnations and retirement
    /// tombstones, while changing the authority invalidates every predecessor
    /// candidate before it can be committed.
    pub(super) fn rebind_provider(
        &mut self,
        provider: BackendIngressLease,
        workspace_epoch: WorkspaceEpoch,
    ) {
        let previous = self.inner.authority;
        assert_eq!(
            previous.provider.authority_domain(),
            provider.authority_domain(),
            "a joined provider successor must retain the engine authority domain"
        );
        assert_eq!(
            previous.workspace_epoch, workspace_epoch,
            "provider replacement cannot cross a workspace epoch"
        );
        self.inner.authority = NativeRouteAuthority::new(provider, workspace_epoch);
    }

    /// Builds a complete validated candidate without mutating committed routes.
    pub(super) fn reconcile_candidate(
        &self,
        provider: BackendIngressLease,
        workspace_epoch: WorkspaceEpoch,
        routes: impl IntoIterator<Item = NativeCoreRoute>,
        retirements: impl IntoIterator<Item = ExactNativeViewport>,
    ) -> Result<NativeBindingCandidate, NativeBindingRegistryError> {
        let authority = NativeRouteAuthority::new(provider, workspace_epoch);
        self.inner
            .reconcile_candidate(
                authority,
                routes.into_iter().map(|route| route.0),
                retirements,
            )
            .map(NativeBindingCandidate)
    }

    /// Converts a candidate into an infallible post-core sidecar commit.
    pub(super) fn prepare_commit(
        &self,
        candidate: NativeBindingCandidate,
        transition: &EngineTransition,
    ) -> Result<PreparedNativeBindingCommit, NativeBindingRegistryError> {
        let mut candidate = candidate.0;
        if !Arc::ptr_eq(&self.inner.identity, &candidate.registry_identity) {
            return Err(NativeBindingRegistryError::CandidateRegistryMismatch);
        }
        self.inner.validate_authority(candidate.authority)?;
        if candidate.base_revision != self.inner.revision {
            return Err(NativeBindingRegistryError::CandidateRevisionMismatch {
                expected: self.inner.revision,
                submitted: candidate.base_revision,
            });
        }
        let base_authority = candidate.authority;
        for reduced in transition.reduced_inputs() {
            let InputOutcome::WorkspaceReplaced {
                before,
                after,
                reconciliation,
                ..
            } = reduced.outcome()
            else {
                continue;
            };
            if candidate.authority.workspace_epoch != before.epoch() {
                return Err(NativeBindingRegistryError::WorkspaceRebaseEpochMismatch {
                    expected: candidate.authority.workspace_epoch,
                    submitted: before.epoch(),
                });
            }
            candidate.state = rebase_route_state(
                candidate.state,
                candidate.authority.provider,
                after.epoch(),
                reconciliation.rebound(),
            )?;
            candidate.authority =
                NativeRouteAuthority::new(candidate.authority.provider, after.epoch());
            for route in candidate.state.live_by_native.values().copied() {
                self.inner.validate_route(candidate.authority, route)?;
            }
        }
        Ok(PreparedNativeBindingCommit {
            registry_identity: candidate.registry_identity,
            base_authority,
            next_authority: candidate.authority,
            base_revision: candidate.base_revision,
            next_revision: candidate.next_revision,
            state: candidate.state,
        })
    }

    /// Applies an already-preflighted candidate without an ordinary failure path.
    pub(super) fn commit_prepared(&mut self, prepared: PreparedNativeBindingCommit) {
        self.validate_prepared(&prepared)
            .expect("a prepared native binding commit remains current");
        self.inner.authority = prepared.next_authority;
        self.inner.revision = prepared.next_revision;
        self.inner.state = prepared.state;
    }

    pub(super) fn validate_prepared(
        &self,
        prepared: &PreparedNativeBindingCommit,
    ) -> Result<(), NativeBindingRegistryError> {
        if !Arc::ptr_eq(&self.inner.identity, &prepared.registry_identity) {
            return Err(NativeBindingRegistryError::CandidateRegistryMismatch);
        }
        if self.inner.authority != prepared.base_authority {
            return Err(NativeBindingRegistryError::AuthorityMismatch {
                expected: self.inner.authority,
                submitted: prepared.base_authority,
            });
        }
        if self.inner.revision != prepared.base_revision {
            return Err(NativeBindingRegistryError::CandidateRevisionMismatch {
                expected: self.inner.revision,
                submitted: prepared.base_revision,
            });
        }
        Ok(())
    }
}

fn rebase_route_state(
    state: RouteState<ViewportBinding>,
    provider: BackendIngressLease,
    workspace_epoch: WorkspaceEpoch,
    rebound: &[(ViewportBinding, ViewportBinding)],
) -> Result<RouteState<ViewportBinding>, NativeBindingRegistryError> {
    let rebound = rebound.iter().copied().collect::<BTreeMap<_, _>>();
    let mut next = RouteState::empty();
    next.retired_by_native = state.retired_by_native;
    next.retired_by_core = state.retired_by_core;

    for (native, mut route) in state.live_by_native {
        let previous = route.core;
        let replacement = rebound.get(&previous).copied().ok_or(
            NativeBindingRegistryError::LiveRouteNotRebound {
                native,
                binding: previous,
            },
        )?;
        route.core = replacement;
        let core = route.core_identity();
        if core.authority_domain != provider.authority_domain() {
            return Err(NativeBindingRegistryError::CoreProviderDomainMismatch {
                native,
                expected: provider.authority_domain(),
                submitted: core.authority_domain,
            });
        }
        if core.workspace_epoch != workspace_epoch {
            return Err(NativeBindingRegistryError::CoreWorkspaceEpochMismatch {
                native,
                expected: workspace_epoch,
                submitted: core.workspace_epoch,
            });
        }
        if core.surface != route.declared_surface {
            return Err(NativeBindingRegistryError::SurfaceBindingMismatch {
                native,
                declared: route.declared_surface,
                bound: core.surface,
            });
        }
        if let Some(previous) = next.native_by_core.insert(core, native) {
            return Err(NativeBindingRegistryError::DuplicateCoreBinding {
                core,
                first: previous,
                second: native,
            });
        }
        if let Some(previous) = next
            .native_by_surface
            .insert(route.declared_surface, native)
        {
            return Err(NativeBindingRegistryError::DuplicateSurface {
                surface: route.declared_surface,
                first: previous,
                second: native,
            });
        }
        next.live_by_native.insert(native, route);
    }

    Ok(next)
}

/// Native operation which attempted an exact route lookup.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NativeRouteUse {
    /// Dispatch of one native callback.
    Callback,
    /// Settlement of one native presentation result.
    Presentation,
    /// Cross-window input or output routing.
    #[cfg(test)]
    Routing,
}

/// Failure to construct or commit an exact native/core roster.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub(super) enum NativeBindingRegistryError {
    /// A roster was captured by another provider or workspace epoch.
    #[error("native route authority mismatch: expected {expected:?}, submitted {submitted:?}")]
    AuthorityMismatch {
        /// Exact registry authority.
        expected: NativeRouteAuthority,
        /// Authority attached to the candidate roster.
        submitted: NativeRouteAuthority,
    },
    /// A core binding belongs to another engine authority domain.
    #[error(
        "native viewport {native:?} core domain mismatch: expected {expected:?}, submitted {submitted:?}"
    )]
    CoreProviderDomainMismatch {
        native: ExactNativeViewport,
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    /// A core binding belongs to another workspace epoch.
    #[error(
        "native viewport {native:?} core epoch mismatch: expected {expected:?}, submitted {submitted:?}"
    )]
    CoreWorkspaceEpochMismatch {
        native: ExactNativeViewport,
        expected: WorkspaceEpoch,
        submitted: WorkspaceEpoch,
    },
    /// A workspace replacement did not begin at the route candidate's exact epoch.
    #[error("native route workspace rebase expected epoch {expected:?}, submitted {submitted:?}")]
    WorkspaceRebaseEpochMismatch {
        /// Epoch currently carried by the candidate route authority.
        expected: WorkspaceEpoch,
        /// Epoch named by the workspace replacement outcome.
        submitted: WorkspaceEpoch,
    },
    /// A live native route was not retained by the workspace replacement.
    #[error("native route {native:?} for {binding:?} was not rebound by workspace replacement")]
    LiveRouteNotRebound {
        /// Exact native viewport which remained live in the submitted roster.
        native: ExactNativeViewport,
        /// Pre-replacement core binding which lacked a successor.
        binding: ViewportBinding,
    },
    /// The runtime surface fact disagrees with the exact core binding.
    #[error(
        "native viewport {native:?} surface mismatch: roster declared {declared}, core bound {bound}"
    )]
    SurfaceBindingMismatch {
        native: ExactNativeViewport,
        declared: SurfaceId,
        bound: SurfaceId,
    },
    /// One exact native binding appeared twice in a candidate roster.
    #[error("duplicate exact native viewport binding {native:?}")]
    DuplicateNativeBinding { native: ExactNativeViewport },
    /// Two live incarnations claimed the same native viewport slot.
    #[error(
        "native viewport slot {viewport:?} has two live incarnations: {first:?} and {second:?}"
    )]
    DuplicateLiveNativeViewport {
        viewport: ViewportId,
        first: ExactNativeViewport,
        second: ExactNativeViewport,
    },
    /// One retirement tombstone appeared twice in the same atomic envelope.
    #[error("duplicate native retirement tombstone {native:?}")]
    DuplicateRetirement { native: ExactNativeViewport },
    /// A tombstone named a native lifetime that this registry never observed.
    #[error("native retirement tombstone names unknown binding {native:?}")]
    UnknownRetirement { native: ExactNativeViewport },
    /// A tombstoned lifetime also appeared in the same live roster.
    #[error("retired native viewport binding {native:?} also appears in the live roster")]
    RetiredBindingStillLive { native: ExactNativeViewport },
    /// A previously live route disappeared without an exact retirement fact.
    #[error(
        "native viewport binding {native:?} for core binding {core:?} disappeared without an exact retirement tombstone"
    )]
    MissingRetirementTombstone {
        native: ExactNativeViewport,
        core: CoreBindingIdentity,
    },
    /// One exact core binding appeared twice in a candidate roster.
    #[error("core binding {core:?} is routed by both {first:?} and {second:?}")]
    DuplicateCoreBinding {
        core: CoreBindingIdentity,
        first: ExactNativeViewport,
        second: ExactNativeViewport,
    },
    /// Two live routes claimed the same logical surface.
    #[error("surface {surface} is routed by both {first:?} and {second:?}")]
    DuplicateSurface {
        surface: SurfaceId,
        first: ExactNativeViewport,
        second: ExactNativeViewport,
    },
    /// A retired exact native lifetime cannot become live again.
    #[error("retired exact native viewport binding {native:?} cannot be reused")]
    RetiredNativeBinding { native: ExactNativeViewport },
    /// A retired exact core binding cannot be routed again.
    #[error("retired exact core binding {core:?} cannot be reused")]
    RetiredCoreBinding { core: CoreBindingIdentity },
    /// An unchanged native lifetime was paired with different core facts.
    #[error("exact native viewport binding {native:?} cannot be relabeled")]
    ExactNativeRouteChanged { native: ExactNativeViewport },
    /// An unchanged core binding was paired with another native lifetime.
    #[error("exact core binding {core:?} cannot move from {previous:?} to {submitted:?}")]
    ExactCoreRouteChanged {
        core: CoreBindingIdentity,
        previous: ExactNativeViewport,
        submitted: ExactNativeViewport,
    },
    /// A candidate created by another registry cannot be spliced into this one.
    #[error("native binding candidate belongs to another registry")]
    CandidateRegistryMismatch,
    /// A candidate was based on an older committed registry revision.
    #[error(
        "native binding candidate revision mismatch: expected {expected}, submitted {submitted}"
    )]
    CandidateRevisionMismatch { expected: u64, submitted: u64 },
    /// The registry revision cannot advance without wrapping.
    #[error("native binding registry revision exhausted")]
    RevisionExhausted,
}

/// Failure to resolve one exact callback, presentation, or route identity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Error)]
pub(super) enum NativeRouteLookupError {
    /// The lookup came from another provider or workspace epoch.
    #[cfg(test)]
    #[error("native route authority mismatch: expected {expected:?}, submitted {submitted:?}")]
    AuthorityMismatch {
        expected: NativeRouteAuthority,
        submitted: NativeRouteAuthority,
    },
    /// The exact native lifetime is a committed tombstone.
    #[error("{use_case:?} rejected retired native viewport binding {native:?}")]
    NativeBindingRetired {
        native: ExactNativeViewport,
        use_case: NativeRouteUse,
    },
    /// The exact native lifetime was never committed.
    #[error("{use_case:?} rejected unknown native viewport binding {native:?}")]
    NativeBindingUnknown {
        native: ExactNativeViewport,
        use_case: NativeRouteUse,
    },
    /// The exact core binding is a committed tombstone.
    #[cfg(test)]
    #[error("retired exact core binding {core:?} has no live native route")]
    CoreBindingRetired { core: CoreBindingIdentity },
    /// The exact core binding was never committed.
    #[cfg(test)]
    #[error("unknown exact core binding {core:?} has no live native route")]
    CoreBindingUnknown { core: CoreBindingIdentity },
    /// The candidate route no longer matches the post-ingress core authority.
    #[error(
        "{use_case:?} rejected native viewport {native:?}: expected core binding {expected:?}, current binding is {observed:?}"
    )]
    CoreBindingNoLongerCurrent {
        native: ExactNativeViewport,
        expected: CoreBindingIdentity,
        observed: Option<CoreBindingIdentity>,
        use_case: NativeRouteUse,
    },
}

#[cfg(test)]
mod tests {
    use dockspace::backend::engine::DockEngine;
    use dockspace::backend::pointer_journal::PointerEdgeSequence;
    use dockspace::graph::Workspace;
    use dockspace::policy::DockPolicy;

    use super::*;

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    struct TestCoreBinding(CoreBindingIdentity);

    impl ExactCoreBinding for TestCoreBinding {
        fn exact_identity(self) -> CoreBindingIdentity {
            self.0
        }
    }

    fn route_authority() -> NativeRouteAuthority {
        let mut engine = DockEngine::new(Workspace::new(), DockPolicy::default())
            .expect("empty workspace must construct a route-authority fixture");
        let host = engine
            .create_presentation_host()
            .expect("fixture must create an exact presentation host");
        let recorder = engine
            .create_backend_ingress_provider(host, PointerEdgeSequence::new(0))
            .expect("fixture must enroll an exact joined provider");
        NativeRouteAuthority::new(recorder.lease(), engine.version().epoch())
    }

    fn native(viewport: u64, incarnation: u64) -> ExactNativeViewport {
        ExactNativeViewport::new(
            ViewportId::from_hash_of(viewport),
            NativeViewportIncarnation::new(incarnation),
        )
    }

    fn core(
        authority: NativeRouteAuthority,
        surface: u64,
        token: u64,
        incarnation: u64,
    ) -> TestCoreBinding {
        TestCoreBinding(CoreBindingIdentity {
            authority_domain: authority.provider().authority_domain(),
            workspace_epoch: authority.workspace_epoch(),
            surface: SurfaceId::new(surface),
            token: WindowToken::new(token),
            incarnation: WindowIncarnation::new(incarnation),
        })
    }

    fn route(
        authority: NativeRouteAuthority,
        viewport: u64,
        native_incarnation: u64,
        surface: u64,
        core_incarnation: u64,
    ) -> RouteRecord<TestCoreBinding> {
        RouteRecord::new(
            native(viewport, native_incarnation),
            SurfaceId::new(surface),
            core(
                authority,
                surface,
                viewport.saturating_mul(100) + native_incarnation,
                core_incarnation,
            ),
        )
    }

    #[test]
    fn duplicate_native_core_and_surface_routes_are_rejected() {
        let authority = route_authority();
        let registry = ExactRouteRegistry::new(authority);
        let first = route(authority, 1, 1, 10, 1);
        let second = route(authority, 2, 1, 20, 2);

        assert!(matches!(
            registry.reconcile_candidate(authority, [first, first], []),
            Err(NativeBindingRegistryError::DuplicateNativeBinding { native: duplicate })
                if duplicate == first.native
        ));

        let duplicate_core = RouteRecord::new(second.native, first.declared_surface, first.core);
        assert!(matches!(
            registry.reconcile_candidate(authority, [first, duplicate_core], []),
            Err(NativeBindingRegistryError::DuplicateCoreBinding { core, .. })
                if core == first.core_identity()
        ));

        let duplicate_surface = RouteRecord::new(
            second.native,
            first.declared_surface,
            TestCoreBinding(CoreBindingIdentity {
                surface: first.declared_surface,
                ..second.core_identity()
            }),
        );
        assert!(matches!(
            registry.reconcile_candidate(authority, [first, duplicate_surface], []),
            Err(NativeBindingRegistryError::DuplicateSurface { surface, .. })
                if surface == first.declared_surface
        ));
    }

    #[test]
    fn provider_domain_epoch_and_surface_are_exact_validation_facts() {
        let authority = route_authority();
        let foreign = route_authority();
        let registry = ExactRouteRegistry::new(authority);
        let valid = route(authority, 1, 1, 10, 1);

        let foreign_core = RouteRecord::new(
            valid.native,
            valid.declared_surface,
            core(foreign, 10, 101, 1),
        );
        assert!(matches!(
            registry.reconcile_candidate(authority, [foreign_core], []),
            Err(NativeBindingRegistryError::CoreProviderDomainMismatch { .. })
        ));

        let stale_core = RouteRecord::new(
            valid.native,
            valid.declared_surface,
            TestCoreBinding(CoreBindingIdentity {
                workspace_epoch: WorkspaceEpoch::new(
                    authority.workspace_epoch().get().saturating_add(1),
                ),
                ..valid.core_identity()
            }),
        );
        assert!(matches!(
            registry.reconcile_candidate(authority, [stale_core], []),
            Err(NativeBindingRegistryError::CoreWorkspaceEpochMismatch { .. })
        ));

        let mislabeled = RouteRecord::new(valid.native, SurfaceId::new(11), valid.core);
        assert!(matches!(
            registry.reconcile_candidate(authority, [mislabeled], []),
            Err(NativeBindingRegistryError::SurfaceBindingMismatch { .. })
        ));

        assert!(matches!(
            registry.reconcile_candidate(foreign, [valid], []),
            Err(NativeBindingRegistryError::AuthorityMismatch { .. })
        ));
    }

    #[test]
    fn exact_tombstone_allows_same_viewport_id_a1_to_a2_recreation() {
        let authority = route_authority();
        let mut registry = ExactRouteRegistry::new(authority);
        let a1 = route(authority, 7, 1, 10, 1);
        let a2 = route(authority, 7, 2, 10, 2);

        let initial = registry
            .reconcile_candidate(authority, [a1], [])
            .expect("A1 roster must validate");
        registry.commit(initial).expect("A1 roster must commit");
        let recreated = registry
            .reconcile_candidate(authority, [a2], [a1.native])
            .expect("A2 may replace A1 atomically");
        registry.commit(recreated).expect("A2 roster must commit");

        assert_eq!(
            registry.resolve_native(authority, a2.native, NativeRouteUse::Routing),
            Ok(a2)
        );
        assert!(matches!(
            registry.resolve_native(authority, a1.native, NativeRouteUse::Routing),
            Err(NativeRouteLookupError::NativeBindingRetired { native, .. })
                if native == a1.native
        ));
        assert_eq!(registry.resolve_core(authority, a2.core), Ok(a2.native));
        assert!(matches!(
            registry.resolve_core(authority, a1.core),
            Err(NativeRouteLookupError::CoreBindingRetired { .. })
        ));
    }

    #[test]
    fn input_candidate_keeps_a_pre_retirement_route_until_core_reduces_the_tombstone() {
        let authority = route_authority();
        let mut registry = ExactRouteRegistry::new(authority);
        let a1 = route(authority, 7, 1, 10, 1);
        let initial = registry
            .reconcile_candidate(authority, [a1], [])
            .expect("A1 roster must validate");
        registry.commit(initial).expect("A1 roster must commit");

        let retiring = registry
            .reconcile_candidate(authority, [], [a1.native])
            .expect("the exact A1 retirement must validate");
        assert_eq!(
            resolve_current_input_candidate(&retiring, a1.native, Some(a1.core)),
            Ok(a1)
        );
        assert!(matches!(
            resolve_candidate(&retiring, a1.native, NativeRouteUse::Callback),
            Err(NativeRouteLookupError::NativeBindingRetired { native, .. })
                if native == a1.native
        ));
        assert!(matches!(
            resolve_current_input_candidate(&retiring, a1.native, None),
            Err(NativeRouteLookupError::CoreBindingNoLongerCurrent {
                native,
                observed: None,
                ..
            }) if native == a1.native
        ));
    }

    #[test]
    fn late_a1_route_presentation_and_callback_are_all_rejected() {
        let authority = route_authority();
        let mut registry = ExactRouteRegistry::new(authority);
        let a1 = route(authority, 7, 1, 10, 1);
        let a2 = route(authority, 7, 2, 10, 2);

        let initial = registry
            .reconcile_candidate(authority, [a1], [])
            .expect("A1 roster must validate");
        registry.commit(initial).expect("A1 roster must commit");
        let recreated = registry
            .reconcile_candidate(authority, [a2], [a1.native])
            .expect("A2 roster must validate");
        registry.commit(recreated).expect("A2 roster must commit");

        for use_case in [
            NativeRouteUse::Routing,
            NativeRouteUse::Presentation,
            NativeRouteUse::Callback,
        ] {
            assert_eq!(
                registry.resolve_native(authority, a1.native, use_case),
                Err(NativeRouteLookupError::NativeBindingRetired {
                    native: a1.native,
                    use_case,
                })
            );
        }
    }

    #[test]
    fn live_route_cannot_disappear_without_an_exact_retirement_tombstone() {
        let authority = route_authority();
        let mut registry = ExactRouteRegistry::new(authority);
        let a1 = route(authority, 7, 1, 10, 1);
        let initial = registry
            .reconcile_candidate(authority, [a1], [])
            .expect("A1 roster must validate");
        registry.commit(initial).expect("A1 roster must commit");

        assert!(matches!(
            registry.reconcile_candidate(authority, [], []),
            Err(NativeBindingRegistryError::MissingRetirementTombstone {
                native,
                core,
            }) if native == a1.native && core == a1.core_identity()
        ));
        assert_eq!(
            registry.resolve_native(authority, a1.native, NativeRouteUse::Callback),
            Ok(a1)
        );
    }

    #[test]
    fn retirement_tombstones_are_exact_unique_and_absent_from_the_live_roster() {
        let authority = route_authority();
        let mut registry = ExactRouteRegistry::new(authority);
        let a1 = route(authority, 7, 1, 10, 1);
        let unknown = native(99, 1);
        let initial = registry
            .reconcile_candidate(authority, [a1], [])
            .expect("A1 roster must validate");
        registry.commit(initial).expect("A1 roster must commit");

        assert!(matches!(
            registry.reconcile_candidate(authority, [a1], [a1.native]),
            Err(NativeBindingRegistryError::RetiredBindingStillLive { native })
                if native == a1.native
        ));
        assert!(matches!(
            registry.reconcile_candidate(authority, [], [unknown]),
            Err(NativeBindingRegistryError::UnknownRetirement { native })
                if native == unknown
        ));
        assert!(matches!(
            registry.reconcile_candidate(authority, [], [a1.native, a1.native]),
            Err(NativeBindingRegistryError::DuplicateRetirement { native })
                if native == a1.native
        ));
    }

    #[test]
    fn failed_candidate_does_not_change_committed_registry() {
        let authority = route_authority();
        let mut registry = ExactRouteRegistry::new(authority);
        let first = route(authority, 1, 1, 10, 1);
        let second = route(authority, 2, 1, 20, 2);
        let initial = registry
            .reconcile_candidate(authority, [first], [])
            .expect("initial roster must validate");
        registry
            .commit(initial)
            .expect("initial roster must commit");
        let before = registry.canonical_roster().collect::<Vec<_>>();

        let invalid = RouteRecord::new(second.native, first.declared_surface, first.core);
        assert!(
            registry
                .reconcile_candidate(authority, [first, invalid], [])
                .is_err()
        );

        assert_eq!(registry.canonical_roster().collect::<Vec<_>>(), before);
        assert_eq!(
            registry.resolve_native(authority, first.native, NativeRouteUse::Callback),
            Ok(first)
        );
        assert!(matches!(
            registry.resolve_native(authority, second.native, NativeRouteUse::Callback),
            Err(NativeRouteLookupError::NativeBindingUnknown { .. })
        ));
    }

    #[test]
    fn callback_order_permutations_commit_the_same_canonical_roster() {
        let authority = route_authority();
        let first = route(authority, 20, 4, 10, 2);
        let second = route(authority, 3, 9, 20, 8);
        let third = route(authority, 11, 1, 30, 5);
        let mut forward = ExactRouteRegistry::new(authority);
        let mut permuted = ExactRouteRegistry::new(authority);

        let forward_candidate = forward
            .reconcile_candidate(authority, [first, second, third], [])
            .expect("forward callback order must validate");
        forward
            .commit(forward_candidate)
            .expect("forward roster must commit");
        let permuted_candidate = permuted
            .reconcile_candidate(authority, [third, first, second], [])
            .expect("permuted callback order must validate");
        permuted
            .commit(permuted_candidate)
            .expect("permuted roster must commit");

        assert_eq!(
            forward.canonical_roster().collect::<Vec<_>>(),
            permuted.canonical_roster().collect::<Vec<_>>()
        );
    }
}
