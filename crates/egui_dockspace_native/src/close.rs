//! Application decisions for core-owned native close plans.

use dockspace::backend::graph::Workspace;
use dockspace::backend::ids::ItemId;
use dockspace::policy::CloseCapability;
use dockspace::{
    CloseDecision, CloseDecisionToken, ClosePlanTarget, CloseRequestId, DeferredCloseDecision,
    DeferredCloseToken, NativeCloseEdge, SurfaceCloseRequest,
};

/// Exact facts available when an application selects the semantic disposition
/// for one native child close edge.
#[derive(Debug)]
pub struct NativeSurfaceCloseContext<'a> {
    edge: NativeCloseEdge,
    workspace: &'a Workspace,
    configured: Option<&'a SurfaceCloseRequest>,
}

impl<'a> NativeSurfaceCloseContext<'a> {
    pub(crate) const fn new(
        edge: NativeCloseEdge,
        workspace: &'a Workspace,
        configured: Option<&'a SurfaceCloseRequest>,
    ) -> Self {
        Self {
            edge,
            workspace,
            configured,
        }
    }

    /// Returns the exact provider close edge being resolved.
    #[must_use]
    pub const fn edge(&self) -> NativeCloseEdge {
        self.edge
    }

    /// Returns the current read-only graph from which an explicit rehome
    /// program may be constructed.
    #[must_use]
    pub const fn workspace(&self) -> &'a Workspace {
        self.workspace
    }

    /// Returns the optional static request attached to this surface template.
    #[must_use]
    pub const fn configured(&self) -> Option<&'a SurfaceCloseRequest> {
        self.configured
    }
}

/// One initial decision requested for a frozen native close-plan item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeCloseItemRequest {
    request: CloseRequestId,
    target: ClosePlanTarget,
    item: ItemId,
    capability: CloseCapability,
    token: CloseDecisionToken,
}

impl NativeCloseItemRequest {
    pub(crate) const fn new(
        request: CloseRequestId,
        target: ClosePlanTarget,
        item: ItemId,
        capability: CloseCapability,
        token: CloseDecisionToken,
    ) -> Self {
        Self {
            request,
            target,
            item,
            capability,
            token,
        }
    }

    #[must_use]
    pub const fn request(self) -> CloseRequestId {
        self.request
    }

    #[must_use]
    pub const fn target(self) -> ClosePlanTarget {
        self.target
    }

    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    #[must_use]
    pub const fn capability(self) -> CloseCapability {
        self.capability
    }

    #[must_use]
    pub const fn token(self) -> CloseDecisionToken {
        self.token
    }
}

/// One terminal decision requested for a deferred native close item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NativeDeferredCloseRequest {
    request: CloseRequestId,
    target: ClosePlanTarget,
    item: ItemId,
    token: DeferredCloseToken,
}

impl NativeDeferredCloseRequest {
    pub(crate) const fn new(
        request: CloseRequestId,
        target: ClosePlanTarget,
        item: ItemId,
        token: DeferredCloseToken,
    ) -> Self {
        Self {
            request,
            target,
            item,
            token,
        }
    }

    #[must_use]
    pub const fn request(self) -> CloseRequestId {
        self.request
    }

    #[must_use]
    pub const fn target(self) -> ClosePlanTarget {
        self.target
    }

    #[must_use]
    pub const fn item(self) -> ItemId {
        self.item
    }

    #[must_use]
    pub const fn token(self) -> DeferredCloseToken {
        self.token
    }
}

/// Application policy for resolving core-frozen native close plans.
pub trait NativeCloseHandler {
    /// Selects the complete semantic disposition for one exact native edge.
    ///
    /// A runtime-created tear-off surface has no static template. Implementors
    /// must therefore choose its close behavior explicitly instead of inheriting
    /// a destructive fallback. A configured template is available through
    /// [`NativeSurfaceCloseContext::configured`].
    fn surface_request(&mut self, context: NativeSurfaceCloseContext<'_>) -> SurfaceCloseRequest;

    /// Resolves one initial item token exactly once.
    fn decide(&mut self, request: NativeCloseItemRequest) -> CloseDecision;

    /// Polls one deferred continuation without inventing a timeout.
    fn resolve_deferred(
        &mut self,
        _request: NativeDeferredCloseRequest,
    ) -> Option<DeferredCloseDecision> {
        None
    }
}

/// Fail-closed default for applications without native close integration.
#[derive(Clone, Copy, Debug, Default)]
pub struct VetoNativeClose;

impl NativeCloseHandler for VetoNativeClose {
    fn surface_request(&mut self, _context: NativeSurfaceCloseContext<'_>) -> SurfaceCloseRequest {
        SurfaceCloseRequest::CloseContent
    }

    fn decide(&mut self, _request: NativeCloseItemRequest) -> CloseDecision {
        CloseDecision::Veto
    }
}

/// Simple policy for applications whose panes never require a close veto.
#[derive(Clone, Copy, Debug, Default)]
pub struct AllowNativeClose;

impl NativeCloseHandler for AllowNativeClose {
    fn surface_request(&mut self, context: NativeSurfaceCloseContext<'_>) -> SurfaceCloseRequest {
        context
            .configured()
            .cloned()
            .unwrap_or(SurfaceCloseRequest::CloseContent)
    }

    fn decide(&mut self, _request: NativeCloseItemRequest) -> CloseDecision {
        CloseDecision::Allow
    }
}
