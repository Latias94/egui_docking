//! Commit-only events emitted by the headless engine.

use crate::command::{CloseCommitOutcome, CommandOutcome};
use crate::ids::{
    InputSequence, ReducerCausalOrdinal, ReducerTickId, SourceSequence, StableInputSourceId,
    SurfaceId,
};
use crate::pointer_journal::{PointerEdgeTicket, PointerStreamId};
use crate::presentation_observation::{HostPresentationStreamId, PresentationHostLease};
use crate::scene::SurfaceSceneStamp;
use crate::transition::WorkspaceVersion;

/// Core-minted causal identity of one published event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReductionCause {
    /// One producer input reduced in normative batch order.
    Input {
        /// Core reducer boundary which consumed the input.
        tick: ReducerTickId,
        /// Exact position of this input among semantic facts in the reducer boundary.
        ordinal: ReducerCausalOrdinal,
        /// Core-assigned global input sequence.
        input: InputSequence,
        /// Stable producer identity.
        source: StableInputSourceId,
        /// Producer-assigned sequence within that source.
        source_sequence: SourceSequence,
    },
    /// One provider-ordered pointer edge accepted through the journal protocol.
    PointerEdge {
        /// Core reducer boundary which consumed the edge.
        tick: ReducerTickId,
        /// Position of this journal batch among semantic facts in the reducer boundary.
        ordinal: ReducerCausalOrdinal,
        /// Exact stream incarnation affected by the edge.
        stream: PointerStreamId,
        /// Core-minted identity of the accepted provider edge.
        ticket: PointerEdgeTicket,
    },
    /// One pointer-provider incarnation and every gesture it owned terminated atomically.
    PointerProviderRetirement {
        /// Core reducer boundary which recorded the terminal lifecycle fact.
        tick: ReducerTickId,
        /// Exact provider incarnation retired by this boundary.
        provider: crate::pointer_journal::PointerInputLease,
    },
    /// One platform provider lost all authority before its successor was activated.
    PlatformProviderReplacement {
        /// Core reducer boundary which published the authority cutover.
        tick: ReducerTickId,
        /// Exact provider incarnation superseded by this boundary.
        provider: crate::platform_provider::PlatformObservationLease,
    },
    /// One independently delivered surface contribution in a reducer boundary.
    SurfaceContribution {
        /// Core reducer boundary which consumed the contribution.
        tick: ReducerTickId,
        /// Surface measured by the contribution.
        surface: SurfaceId,
        /// Exact surface authority frozen before measurement began.
        base: SurfaceSceneStamp,
    },
    /// The atomic interaction reconciliation following host presentation observations.
    SurfacePresentationObservationBatch {
        /// Core reducer boundary which consumed every observation.
        tick: ReducerTickId,
    },
    /// One presentation stream fact reduced at an exact backend ingress position.
    PresentationObservation {
        /// Core reducer boundary which consumed the observation.
        tick: ReducerTickId,
        /// Position among all causal facts reduced in this host frame.
        ordinal: ReducerCausalOrdinal,
        /// Core-bound presentation host which owns the stream.
        host: PresentationHostLease,
        /// Exact stream observed at this position.
        stream: HostPresentationStreamId,
    },
    /// The atomic interaction reconciliation following all surface contributions.
    SurfaceContributionBatch {
        /// Core reducer boundary which consumed every contribution.
        tick: ReducerTickId,
    },
    /// One presentation host and all of its streams terminated atomically.
    PresentationHostRetirement {
        /// Core reducer boundary which recorded the terminal lifecycle fact.
        tick: ReducerTickId,
        /// Exact host lease retired by this boundary.
        host: PresentationHostLease,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PendingReductionCause {
    Input(InputSequence),
    Bound(ReductionCause),
}

impl PendingReductionCause {
    pub(crate) const fn input(input: InputSequence) -> Self {
        Self::Input(input)
    }

    pub(crate) const fn bound(cause: ReductionCause) -> Self {
        Self::Bound(cause)
    }

    pub(crate) fn bind_input(&mut self, cause: ReductionCause) -> bool {
        let ReductionCause::Input { input, .. } = cause else {
            return false;
        };
        match self {
            Self::Input(pending) if *pending == input => {
                *self = Self::Bound(cause);
                true
            }
            Self::Bound(existing) => *existing == cause,
            Self::Input(_) => false,
        }
    }

    pub(crate) const fn published(self) -> Option<ReductionCause> {
        match self {
            Self::Input(_) => None,
            Self::Bound(cause) => Some(cause),
        }
    }
}

/// Event created after an engine candidate has been published.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkspaceEvent {
    cause: PendingReductionCause,
    version: WorkspaceVersion,
    kind: WorkspaceEventKind,
}

impl WorkspaceEvent {
    pub(crate) const fn new(
        input: InputSequence,
        version: WorkspaceVersion,
        kind: WorkspaceEventKind,
    ) -> Self {
        Self {
            cause: PendingReductionCause::input(input),
            version,
            kind,
        }
    }

    /// Creates an event whose causal authority is already complete.
    ///
    /// Pointer-journal reductions do not have an [`InputSequence`].  Binding
    /// them through the input constructor would manufacture a false semantic
    /// producer identity, so journal-driven workspace changes enter the event
    /// log through this exact-cause constructor instead.
    pub(crate) const fn new_caused(
        cause: ReductionCause,
        version: WorkspaceVersion,
        kind: WorkspaceEventKind,
    ) -> Self {
        Self {
            cause: PendingReductionCause::bound(cause),
            version,
            kind,
        }
    }

    pub(crate) fn bind_input(&mut self, cause: ReductionCause) -> bool {
        self.cause.bind_input(cause)
    }

    /// Returns the exact core-minted cause of this published event.
    #[must_use]
    pub fn cause(&self) -> ReductionCause {
        self.cause
            .published()
            .expect("published workspace events always have a bound reduction cause")
    }

    /// Returns the version after applying the event's change.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the committed change description.
    #[must_use]
    pub const fn kind(&self) -> &WorkspaceEventKind {
        &self.kind
    }
}

/// Durable or policy change which became observable at a commit boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkspaceEventKind {
    /// A complete workspace replacement committed.
    WorkspaceReplaced,
    /// One checked workspace command changed state.
    CommandCommitted(CommandOutcome),
    /// One ClosePlan-approved content-close transaction changed state.
    CloseCommitted(CloseCommitOutcome),
    /// Application docking policy changed.
    PolicyReplaced,
}
