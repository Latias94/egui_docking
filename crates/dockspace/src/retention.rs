//! Core-owned retention authority for cross-frame runtime resources.

use std::collections::HashSet;

use crate::presentation_observation::{HostFrameKey, HostPresentationStreamId};

/// Diagnostic class of external evidence still missing before terminal data can be compacted.
///
/// This enum is not a proof or a compaction capability. These barriers are protocol facts, not
/// timers or capacity policies. Joined backend replacement discharges pointer, effect-provider,
/// and platform-observation barriers by consuming the sole recorder. Published terminal effect
/// and close detail is compacted on the next atomic engine candidate; monotonic identity frontiers
/// continue to classify late replay without retaining the payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RuntimeRetentionReleaseBarrier {
    /// The retired effect producer is quiescent, allowing its provider guard to be released.
    EffectIngressQuiesced,
    /// The producer was joined and no prepared journal for the retired lease can still commit.
    PointerIngressQuiesced,
    /// No accepted platform snapshot can still contain facts for the destroyed binding.
    PlatformObservationIngressQuiesced,
}

/// Detailed presentation-host and stream structures retained by the core ledger.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PresentationHostRetentionManifest {
    live_host_states: usize,
    detailed_retired_host_states: usize,
    retained_stream_states: usize,
    compacted_retirement_ranges: usize,
    logical_compacted_hosts: u64,
}

impl PresentationHostRetentionManifest {
    pub(crate) const fn new(
        live_host_states: usize,
        detailed_retired_host_states: usize,
        retained_stream_states: usize,
        compacted_retirement_ranges: usize,
        logical_compacted_hosts: u64,
    ) -> Self {
        Self {
            live_host_states,
            detailed_retired_host_states,
            retained_stream_states,
            compacted_retirement_ranges,
            logical_compacted_hosts,
        }
    }

    /// Returns live host-state records which can still open or own presentation streams.
    #[must_use]
    pub const fn live_host_states(self) -> usize {
        self.live_host_states
    }

    /// Returns retired host records still preserving exact roster and retirement detail.
    #[must_use]
    pub const fn detailed_retired_host_states(self) -> usize {
        self.detailed_retired_host_states
    }

    /// Returns live or terminal stream records still retained by a detailed host record.
    #[must_use]
    pub const fn retained_stream_states(self) -> usize {
        self.retained_stream_states
    }

    /// Returns interval records used to classify compacted retired host identities.
    #[must_use]
    pub const fn compacted_retirement_ranges(self) -> usize {
        self.compacted_retirement_ranges
    }

    /// Returns the logical number of retired hosts represented by compacted intervals.
    #[must_use]
    pub const fn logical_compacted_hosts(self) -> u64 {
        self.logical_compacted_hosts
    }

    /// Returns the actual map and interval records cloned with an engine candidate.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.live_host_states
            + self.detailed_retired_host_states
            + self.retained_stream_states
            + self.compacted_retirement_ranges
    }
}

/// Retained effect-ledger structures and their semantic state.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EffectRetentionManifest {
    unsettled_records: usize,
    terminal_record_guards: usize,
    revoked_provider_guards: usize,
}

impl EffectRetentionManifest {
    pub(crate) const fn new(
        unsettled_records: usize,
        terminal_record_guards: usize,
        revoked_provider_guards: usize,
    ) -> Self {
        Self {
            unsettled_records,
            terminal_record_guards,
            revoked_provider_guards,
        }
    }

    /// Returns records which can still receive a semantic or platform transition.
    #[must_use]
    pub const fn unsettled_records(self) -> usize {
        self.unsettled_records
    }

    /// Returns terminal records retained through their publication boundary or by a live owner.
    #[must_use]
    pub const fn terminal_record_guards(self) -> usize {
        self.terminal_record_guards
    }

    /// Returns retired provider identities retained to reject late provider results.
    #[must_use]
    pub const fn revoked_provider_guards(self) -> usize {
        self.revoked_provider_guards
    }

    /// Returns all currently stored effect identities and provider tombstones.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.unsettled_records + self.terminal_record_guards + self.revoked_provider_guards
    }

    /// Returns the exact proof required before a revoked provider guard may be compacted.
    ///
    /// Published terminal records use the monotonic effect frontier as their replay guard and
    /// therefore need no external quiescence proof.
    #[must_use]
    pub const fn provider_release_barrier(self) -> Option<RuntimeRetentionReleaseBarrier> {
        if self.revoked_provider_guards > 0 {
            Some(RuntimeRetentionReleaseBarrier::EffectIngressQuiesced)
        } else {
            None
        }
    }
}

/// Retained close-plan structures and copied-token replay guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CloseRetentionManifest {
    active_plans: usize,
    terminal_plan_guards: usize,
    decision_token_guards: usize,
    deferred_token_guards: usize,
}

impl CloseRetentionManifest {
    pub(crate) const fn new(
        active_plans: usize,
        terminal_plan_guards: usize,
        decision_token_guards: usize,
        deferred_token_guards: usize,
    ) -> Self {
        Self {
            active_plans,
            terminal_plan_guards,
            decision_token_guards,
            deferred_token_guards,
        }
    }

    /// Returns close plans which still own semantic or native work.
    #[must_use]
    pub const fn active_plans(self) -> usize {
        self.active_plans
    }

    /// Returns final plan snapshots retained through their publication boundary.
    #[must_use]
    pub const fn terminal_plan_guards(self) -> usize {
        self.terminal_plan_guards
    }

    /// Returns initial decision-token ownership entries still accepted by the protocol.
    #[must_use]
    pub const fn decision_token_guards(self) -> usize {
        self.decision_token_guards
    }

    /// Returns deferred continuation-token ownership entries still accepted by the protocol.
    #[must_use]
    pub const fn deferred_token_guards(self) -> usize {
        self.deferred_token_guards
    }

    /// Returns all currently stored close-plan and token identities.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.active_plans
            + self.terminal_plan_guards
            + self.decision_token_guards
            + self.deferred_token_guards
    }
}

/// Retained pointer-provider state and retired-lease replay guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerRetentionManifest {
    active_provider_count: usize,
    active_stream_count: usize,
    detailed_retired_lease_guards: usize,
    compacted_retirement_ranges: usize,
    logical_compacted_leases: u64,
}

impl PointerRetentionManifest {
    pub(crate) const fn new(
        active_provider_count: usize,
        active_stream_count: usize,
        detailed_retired_lease_guards: usize,
        compacted_retirement_ranges: usize,
        logical_compacted_leases: u64,
    ) -> Self {
        Self {
            active_provider_count,
            active_stream_count,
            detailed_retired_lease_guards,
            compacted_retirement_ranges,
            logical_compacted_leases,
        }
    }

    /// Returns whether a live pointer provider is retained.
    #[must_use]
    pub const fn active_provider_count(self) -> usize {
        self.active_provider_count
    }

    /// Returns active device streams owned by the current provider.
    #[must_use]
    pub const fn active_stream_count(self) -> usize {
        self.active_stream_count
    }

    /// Returns retired leases retaining their exact final watermark while producer quiescence is
    /// not yet proven.
    #[must_use]
    pub const fn retired_lease_guards(self) -> usize {
        self.detailed_retired_lease_guards
    }

    /// Returns interval guards which classify quiesced historical provider incarnations.
    #[must_use]
    pub const fn compacted_retirement_ranges(self) -> usize {
        self.compacted_retirement_ranges
    }

    /// Returns the logical number of retired provider leases represented by compacted intervals.
    #[must_use]
    pub const fn logical_compacted_leases(self) -> u64 {
        self.logical_compacted_leases
    }

    /// Returns all currently stored pointer provider, stream, and retirement identities.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.active_provider_count
            + self.active_stream_count
            + self.detailed_retired_lease_guards
            + self.compacted_retirement_ranges
    }

    /// Returns the exact proof required before retired pointer leases may be compacted.
    #[must_use]
    pub const fn terminal_release_barrier(self) -> Option<RuntimeRetentionReleaseBarrier> {
        if self.detailed_retired_lease_guards > 0 {
            Some(RuntimeRetentionReleaseBarrier::PointerIngressQuiesced)
        } else {
            None
        }
    }
}

/// Retained native-binding cleanup state, indexes, and destroyed-incarnation guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BindingRetentionManifest {
    active_retirements: usize,
    token_index_entries: usize,
    cleanup_lineage_entries: usize,
    destroyed_binding_guards: usize,
}

/// Stable semantic-input source watermarks retained as replay guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct InputSourceRetentionManifest {
    watermark_guards: usize,
}

impl InputSourceRetentionManifest {
    pub(crate) const fn new(watermark_guards: usize) -> Self {
        Self { watermark_guards }
    }

    /// Returns source identities whose last accepted sequence remains authoritative.
    #[must_use]
    pub const fn watermark_guards(self) -> usize {
        self.watermark_guards
    }

    /// Returns all currently stored semantic-input replay guards.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.watermark_guards
    }
}

/// Core-owned smooth-scroll sessions and per-stream sequence replay guards.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ScrollRetentionManifest {
    active_sessions: usize,
    sequence_watermark_guards: usize,
}

impl ScrollRetentionManifest {
    pub(crate) const fn new(active_sessions: usize, sequence_watermark_guards: usize) -> Self {
        Self {
            active_sessions,
            sequence_watermark_guards,
        }
    }

    /// Returns phaseful smooth-scroll sessions which still own or suppress delivery.
    #[must_use]
    pub const fn active_sessions(self) -> usize {
        self.active_sessions
    }

    /// Returns per-device sequence watermarks retained for live pointer streams.
    #[must_use]
    pub const fn sequence_watermark_guards(self) -> usize {
        self.sequence_watermark_guards
    }

    /// Returns all currently stored smooth-scroll session and replay-guard structures.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.active_sessions + self.sequence_watermark_guards
    }
}

impl BindingRetentionManifest {
    pub(crate) const fn new(
        active_retirements: usize,
        token_index_entries: usize,
        cleanup_lineage_entries: usize,
        destroyed_binding_guards: usize,
    ) -> Self {
        Self {
            active_retirements,
            token_index_entries,
            cleanup_lineage_entries,
            destroyed_binding_guards,
        }
    }

    /// Returns retired bindings which still own cleanup or exact-destruction work.
    #[must_use]
    pub const fn active_retirements(self) -> usize {
        self.active_retirements
    }

    /// Returns window-token quarantine index entries for active retirements.
    #[must_use]
    pub const fn token_index_entries(self) -> usize {
        self.token_index_entries
    }

    /// Returns cleanup effect identities retained for late causal settlement.
    #[must_use]
    pub const fn cleanup_lineage_entries(self) -> usize {
        self.cleanup_lineage_entries
    }

    /// Returns exact destroyed binding incarnations retained against delayed snapshots.
    #[must_use]
    pub const fn destroyed_binding_guards(self) -> usize {
        self.destroyed_binding_guards
    }

    /// Returns all currently stored binding retirement identities and indexes.
    #[must_use]
    pub const fn retained_structure_count(self) -> usize {
        self.active_retirements
            + self.token_index_entries
            + self.cleanup_lineage_entries
            + self.destroyed_binding_guards
    }

    /// Returns the proof required before destroyed binding guards may be compacted.
    #[must_use]
    pub const fn terminal_release_barrier(self) -> Option<RuntimeRetentionReleaseBarrier> {
        if self.destroyed_binding_guards > 0 {
            Some(RuntimeRetentionReleaseBarrier::PlatformObservationIngressQuiesced)
        } else {
            None
        }
    }
}

/// Exact concrete presentation emissions whose adapter resources may still be referenced.
///
/// The core derives this manifest from pending presentation settlement, retained scene
/// authority, active interaction sessions, and native staging state. Adapters may reclaim an
/// emission only after it is absent from a newly committed manifest. This is deliberately an
/// exact set rather than an age-based cache policy: renderer latency, provider replacement, and
/// viewport incarnation changes cannot be inferred from frame count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationRetentionManifest {
    emissions: HashSet<HostFrameKey>,
    streams: HashSet<HostPresentationStreamId>,
}

/// Complete core-derived accounting for runtime resources retained across host frames.
///
/// Presentation emissions form an immediately executable allow-list for adapter resource
/// reclamation. Other domains expose every live structure and the external quiescence barrier
/// which prevents safe compaction. Consequently a soak test can distinguish a real leak (stored
/// structures without a represented obligation) from a deliberately retained protocol guard.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RuntimeRetentionManifest {
    presentation: PresentationRetentionManifest,
    presentation_hosts: PresentationHostRetentionManifest,
    effects: EffectRetentionManifest,
    close: CloseRetentionManifest,
    pointer: PointerRetentionManifest,
    bindings: BindingRetentionManifest,
    input_sources: InputSourceRetentionManifest,
    scroll: ScrollRetentionManifest,
}

impl RuntimeRetentionManifest {
    pub(crate) const fn new(
        presentation: PresentationRetentionManifest,
        presentation_hosts: PresentationHostRetentionManifest,
        effects: EffectRetentionManifest,
        close: CloseRetentionManifest,
        pointer: PointerRetentionManifest,
        bindings: BindingRetentionManifest,
        input_sources: InputSourceRetentionManifest,
        scroll: ScrollRetentionManifest,
    ) -> Self {
        Self {
            presentation,
            presentation_hosts,
            effects,
            close,
            pointer,
            bindings,
            input_sources,
            scroll,
        }
    }

    /// Returns exact concrete presentation outputs still referenced by core authority.
    #[must_use]
    pub const fn presentation(&self) -> &PresentationRetentionManifest {
        &self.presentation
    }

    /// Returns presentation host, stream, and compacted retirement accounting.
    #[must_use]
    pub const fn presentation_hosts(&self) -> PresentationHostRetentionManifest {
        self.presentation_hosts
    }

    /// Returns effect-ledger retention accounting.
    #[must_use]
    pub const fn effects(&self) -> EffectRetentionManifest {
        self.effects
    }

    /// Returns close-plan retention accounting.
    #[must_use]
    pub const fn close(&self) -> CloseRetentionManifest {
        self.close
    }

    /// Returns pointer-provider retention accounting.
    #[must_use]
    pub const fn pointer(&self) -> PointerRetentionManifest {
        self.pointer
    }

    /// Returns native-binding lifecycle retention accounting.
    #[must_use]
    pub const fn bindings(&self) -> BindingRetentionManifest {
        self.bindings
    }

    /// Returns semantic-input source watermark accounting.
    #[must_use]
    pub const fn input_sources(&self) -> InputSourceRetentionManifest {
        self.input_sources
    }

    /// Returns smooth-scroll session and sequence-watermark accounting.
    #[must_use]
    pub const fn scroll(&self) -> ScrollRetentionManifest {
        self.scroll
    }

    /// Returns every retained runtime structure except concrete presentation output resources.
    #[must_use]
    pub const fn retained_runtime_structure_count(&self) -> usize {
        self.presentation_hosts.retained_structure_count()
            + self.effects.retained_structure_count()
            + self.close.retained_structure_count()
            + self.pointer.retained_structure_count()
            + self.bindings.retained_structure_count()
            + self.input_sources.retained_structure_count()
            + self.scroll.retained_structure_count()
    }
}

impl PresentationRetentionManifest {
    pub(crate) fn from_resources(
        emissions: impl IntoIterator<Item = HostFrameKey>,
        streams: impl IntoIterator<Item = HostPresentationStreamId>,
    ) -> Self {
        Self {
            emissions: emissions.into_iter().collect(),
            streams: streams.into_iter().collect(),
        }
    }

    /// Returns whether adapter resources for this exact concrete emission remain live.
    #[must_use]
    pub fn retains(&self, emission: HostFrameKey) -> bool {
        self.emissions.contains(&emission)
    }

    /// Returns whether adapter state for this exact stream incarnation remains live.
    #[must_use]
    pub fn retains_stream(&self, stream: HostPresentationStreamId) -> bool {
        self.streams.contains(&stream)
    }

    /// Returns the number of distinct concrete emissions retained by core authority.
    #[doc(hidden)]
    #[must_use]
    pub fn emission_count(&self) -> usize {
        self.emissions.len()
    }

    /// Returns the number of exact presentation stream incarnations retained by core authority.
    #[doc(hidden)]
    #[must_use]
    pub fn stream_count(&self) -> usize {
        self.streams.len()
    }
}
