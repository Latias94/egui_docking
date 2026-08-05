//! Core-owned authority for platform observation providers.
//!
//! Platform observations may outlive the native runtime instance that captured
//! them. A lease binds every observation batch to one exact provider
//! incarnation so a delayed batch cannot regain authority after provider
//! replacement or retirement.

use thiserror::Error;

use crate::ids::EngineAuthorityDomainId;

/// Non-wrapping engine-local incarnation of one platform observation provider.
///
/// This counter remains private because an adapter may retain an incarnation
/// only through a core-minted [`PlatformObservationLease`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PlatformProviderIncarnation(u64);

impl PlatformProviderIncarnation {
    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Monotonic publication frontier for platform-provider authority changes.
///
/// The value is core-owned and deliberately distinct from provider
/// incarnation: replacement activation changes authority without allocating a
/// new provider identity.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub(crate) struct PlatformProviderAuthorityFrontier(u64);

impl PlatformProviderAuthorityFrontier {
    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    pub(crate) const fn get(self) -> u64 {
        self.0
    }
}

/// Opaque core-minted identity of one platform observation provider lifetime.
///
/// The same lease may span many host frames. Its fields and constructor are
/// private, so an adapter can retain and return a lease but cannot manufacture
/// one, advance its incarnation, or rebind it to another engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PlatformObservationLease {
    authority_domain: EngineAuthorityDomainId,
    incarnation: PlatformProviderIncarnation,
}

impl PlatformObservationLease {
    const fn new(
        authority_domain: EngineAuthorityDomainId,
        incarnation: PlatformProviderIncarnation,
    ) -> Self {
        Self {
            authority_domain,
            incarnation,
        }
    }

    /// Returns the engine authority domain which minted this lease.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the process-local diagnostic representation of this incarnation.
    #[must_use]
    pub const fn incarnation(self) -> u64 {
        self.incarnation.0
    }
}

/// Opaque authority to finish one exact platform-provider handoff.
///
/// Beginning replacement revokes the predecessor immediately, but the
/// successor remains inactive until the runtime has stopped and joined the old
/// dispatch lane and returns this ticket. The private fields prevent an
/// adapter from manufacturing a quiescence boundary.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlatformProviderReplacementTicket {
    authority_domain: EngineAuthorityDomainId,
    predecessor: PlatformObservationLease,
    successor: PlatformObservationLease,
}

impl PlatformProviderReplacementTicket {
    const fn new(
        authority_domain: EngineAuthorityDomainId,
        predecessor: PlatformObservationLease,
        successor: PlatformObservationLease,
    ) -> Self {
        Self {
            authority_domain,
            predecessor,
            successor,
        }
    }

    /// Returns the provider which was revoked when this handoff began.
    #[must_use]
    pub const fn predecessor(self) -> PlatformObservationLease {
        self.predecessor
    }
}

/// Classification of a submitted platform observation lease.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PlatformObservationLeaseClassification {
    /// The exact sole provider currently owns observation authority.
    Active,
    /// A replacement permanently superseded this provider incarnation.
    Superseded { successor: PlatformObservationLease },
    /// The provider explicitly relinquished observation authority.
    Retired,
    /// The lease belongs to another engine authority domain.
    Foreign {
        expected: EngineAuthorityDomainId,
        submitted: EngineAuthorityDomainId,
    },
    /// This engine never minted the submitted lease.
    Unknown,
}

/// Typed failure while changing or validating platform provider authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PlatformObservationAuthorityError {
    /// A second provider cannot be created while one remains active.
    #[error("platform observation provider {active:?} is already active")]
    ProviderAlreadyActive {
        /// Exact provider currently holding observation authority.
        active: PlatformObservationLease,
    },
    /// A provider cannot be created while a revoked predecessor is awaiting a
    /// typed dispatch-lane quiescence boundary.
    #[error("platform provider replacement from {predecessor:?} is awaiting quiescence")]
    ProviderReplacementPending {
        /// Revoked predecessor whose dispatch lane must become quiescent.
        predecessor: PlatformObservationLease,
    },
    /// Terminal provider retirement closes this engine authority permanently.
    #[error("platform observation provider authority is terminally retired")]
    ProviderAuthorityRetired,
    /// The submitted lease belongs to another engine.
    #[error(
        "platform observation lease belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignLease {
        /// Engine authority domain owned by this ledger.
        expected: EngineAuthorityDomainId,
        /// Authority domain embedded in the submitted lease.
        submitted: EngineAuthorityDomainId,
    },
    /// This engine did not mint the submitted lease.
    #[error("platform observation lease {lease:?} was never minted by this authority")]
    UnknownLease {
        /// Unrecognized lease.
        lease: PlatformObservationLease,
    },
    /// A replacement permanently superseded the submitted lease.
    #[error("platform observation lease {lease:?} was superseded by provider {successor:?}")]
    SupersededLease {
        /// Superseded provider lease.
        lease: PlatformObservationLease,
        /// Exact successor which replaced it.
        successor: PlatformObservationLease,
    },
    /// The submitted lease was explicitly retired.
    #[error("platform observation lease {lease:?} is retired")]
    RetiredLease {
        /// Retired provider lease.
        lease: PlatformObservationLease,
    },
    /// A replacement ticket belongs to another engine authority domain.
    #[error(
        "platform provider replacement ticket belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignReplacementTicket {
        /// Engine authority domain owned by this ledger.
        expected: EngineAuthorityDomainId,
        /// Authority domain embedded in the submitted ticket.
        submitted: EngineAuthorityDomainId,
    },
    /// The submitted ticket is not the exact pending replacement.
    #[error("platform provider replacement ticket is not the pending handoff")]
    UnknownReplacementTicket,
    /// No further provider incarnation can be allocated without wrapping.
    #[error("platform observation provider incarnation counter is exhausted")]
    ProviderIncarnationExhausted,
    /// No further provider-authority transition can be recorded without wrapping.
    #[error("platform observation provider authority frontier is exhausted")]
    ProviderAuthorityFrontierExhausted,
}

/// Core-owned authority ledger for the sole live platform observation provider.
///
/// This type is deliberately crate-private. Adapters receive only the opaque
/// lease; the engine owns creation, replacement, retirement, and validation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct PlatformObservationAuthority {
    authority_domain: EngineAuthorityDomainId,
    last_incarnation: PlatformProviderIncarnation,
    frontier: PlatformProviderAuthorityFrontier,
    active: Option<PlatformObservationLease>,
    pending_replacement: Option<PlatformProviderReplacementTicket>,
    terminal_retirement: Option<PlatformObservationLease>,
}

impl PlatformObservationAuthority {
    /// Creates an empty provider authority for one exact engine domain.
    pub(crate) fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            last_incarnation: PlatformProviderIncarnation::default(),
            frontier: PlatformProviderAuthorityFrontier::default(),
            active: None,
            pending_replacement: None,
            terminal_retirement: None,
        }
    }

    /// Returns the sole live provider lease, when one exists.
    pub(crate) const fn active(&self) -> Option<PlatformObservationLease> {
        self.active
    }

    /// Returns the exact pending replacement reserved by the core.
    pub(crate) const fn pending_replacement(&self) -> Option<PlatformProviderReplacementTicket> {
        self.pending_replacement
    }

    /// Returns the monotonic frontier of successfully published authority changes.
    pub(crate) const fn frontier(&self) -> PlatformProviderAuthorityFrontier {
        self.frontier
    }

    /// Creates the sole provider when no provider is active.
    pub(crate) fn create(
        &mut self,
    ) -> Result<PlatformObservationLease, PlatformObservationAuthorityError> {
        if let Some(active) = self.active {
            return Err(PlatformObservationAuthorityError::ProviderAlreadyActive { active });
        }
        if let Some(pending) = self.pending_replacement {
            return Err(
                PlatformObservationAuthorityError::ProviderReplacementPending {
                    predecessor: pending.predecessor,
                },
            );
        }
        if self.terminal_retirement.is_some() {
            return Err(PlatformObservationAuthorityError::ProviderAuthorityRetired);
        }

        let incarnation = self.next_incarnation()?;
        let frontier = self.next_frontier()?;
        let lease = PlatformObservationLease::new(self.authority_domain, incarnation);
        self.last_incarnation = incarnation;
        self.frontier = frontier;
        self.active = Some(lease);
        Ok(lease)
    }

    /// Revokes one exact provider and reserves its inactive successor.
    ///
    /// The returned ticket is not a lease and grants no observation or effect
    /// authority. The runtime must first quiesce the predecessor dispatch lane,
    /// then return it through [`Self::finish_replacement`].
    pub(crate) fn begin_replacement(
        &mut self,
        lease: PlatformObservationLease,
    ) -> Result<PlatformProviderReplacementTicket, PlatformObservationAuthorityError> {
        self.require_active(lease)?;
        let incarnation = self.next_incarnation()?;
        let frontier = self.next_frontier()?;
        let successor = PlatformObservationLease::new(self.authority_domain, incarnation);
        let ticket =
            PlatformProviderReplacementTicket::new(self.authority_domain, lease, successor);

        self.last_incarnation = incarnation;
        self.frontier = frontier;
        self.active = None;
        self.pending_replacement = Some(ticket);
        Ok(ticket)
    }

    /// Activates the reserved successor after typed predecessor quiescence.
    pub(crate) fn finish_replacement(
        &mut self,
        ticket: PlatformProviderReplacementTicket,
    ) -> Result<PlatformObservationLease, PlatformObservationAuthorityError> {
        if ticket.authority_domain != self.authority_domain {
            return Err(
                PlatformObservationAuthorityError::ForeignReplacementTicket {
                    expected: self.authority_domain,
                    submitted: ticket.authority_domain,
                },
            );
        }
        if self.pending_replacement != Some(ticket) {
            return Err(PlatformObservationAuthorityError::UnknownReplacementTicket);
        }
        let frontier = self.next_frontier()?;
        debug_assert!(self.active.is_none());
        self.frontier = frontier;
        self.pending_replacement = None;
        self.active = Some(ticket.successor);
        Ok(ticket.successor)
    }

    /// Abandons one pending handoff without reviving either provider.
    ///
    /// The predecessor remains superseded and the reserved successor never
    /// gains observation authority. A later explicit provider enrollment mints
    /// a fresh incarnation.
    pub(crate) fn abort_replacement(
        &mut self,
        ticket: PlatformProviderReplacementTicket,
    ) -> Result<(), PlatformObservationAuthorityError> {
        if ticket.authority_domain != self.authority_domain {
            return Err(
                PlatformObservationAuthorityError::ForeignReplacementTicket {
                    expected: self.authority_domain,
                    submitted: ticket.authority_domain,
                },
            );
        }
        if self.pending_replacement != Some(ticket) {
            return Err(PlatformObservationAuthorityError::UnknownReplacementTicket);
        }
        let frontier = self.next_frontier()?;
        debug_assert!(self.active.is_none());
        self.frontier = frontier;
        self.pending_replacement = None;
        Ok(())
    }

    /// Permanently retires one exact active provider.
    #[allow(
        dead_code,
        reason = "the native runtime will expose terminal provider retirement"
    )]
    pub(crate) fn retire(
        &mut self,
        lease: PlatformObservationLease,
    ) -> Result<(), PlatformObservationAuthorityError> {
        self.require_active(lease)?;
        let frontier = self.next_frontier()?;

        self.frontier = frontier;
        self.active = None;
        self.terminal_retirement = Some(lease);
        Ok(())
    }

    /// Classifies a lease without changing provider authority.
    pub(crate) fn classify(
        &self,
        lease: PlatformObservationLease,
    ) -> PlatformObservationLeaseClassification {
        if lease.authority_domain() != self.authority_domain {
            return PlatformObservationLeaseClassification::Foreign {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            };
        }
        if self.active == Some(lease) {
            return PlatformObservationLeaseClassification::Active;
        }
        if lease.incarnation.0 == 0 || lease.incarnation > self.last_incarnation {
            return PlatformObservationLeaseClassification::Unknown;
        }
        if self.terminal_retirement == Some(lease) {
            return PlatformObservationLeaseClassification::Retired;
        }
        if lease.incarnation < self.last_incarnation {
            let successor = PlatformObservationLease::new(
                self.authority_domain,
                lease
                    .incarnation
                    .checked_next()
                    .expect("a historical incarnation always has a successor"),
            );
            return PlatformObservationLeaseClassification::Superseded { successor };
        }
        PlatformObservationLeaseClassification::Unknown
    }

    /// Requires the exact current provider lease.
    pub(crate) fn require_active(
        &self,
        lease: PlatformObservationLease,
    ) -> Result<(), PlatformObservationAuthorityError> {
        match self.classify(lease) {
            PlatformObservationLeaseClassification::Active => Ok(()),
            PlatformObservationLeaseClassification::Superseded { successor } => {
                Err(PlatformObservationAuthorityError::SupersededLease { lease, successor })
            }
            PlatformObservationLeaseClassification::Retired => {
                Err(PlatformObservationAuthorityError::RetiredLease { lease })
            }
            PlatformObservationLeaseClassification::Foreign {
                expected,
                submitted,
            } => Err(PlatformObservationAuthorityError::ForeignLease {
                expected,
                submitted,
            }),
            PlatformObservationLeaseClassification::Unknown => {
                Err(PlatformObservationAuthorityError::UnknownLease { lease })
            }
        }
    }

    fn next_incarnation(
        &self,
    ) -> Result<PlatformProviderIncarnation, PlatformObservationAuthorityError> {
        self.last_incarnation
            .checked_next()
            .ok_or(PlatformObservationAuthorityError::ProviderIncarnationExhausted)
    }

    fn next_frontier(
        &self,
    ) -> Result<PlatformProviderAuthorityFrontier, PlatformObservationAuthorityError> {
        self.frontier
            .checked_next()
            .ok_or(PlatformObservationAuthorityError::ProviderAuthorityFrontierExhausted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn domain(value: u64) -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(value)
    }

    fn forged_lease(
        authority_domain: EngineAuthorityDomainId,
        incarnation: u64,
    ) -> PlatformObservationLease {
        PlatformObservationLease::new(authority_domain, PlatformProviderIncarnation(incarnation))
    }

    #[test]
    fn create_mints_one_opaque_active_lease() {
        let authority_domain = domain(1);
        let mut authority = PlatformObservationAuthority::new(authority_domain);

        assert_eq!(authority.active(), None);
        assert_eq!(authority.frontier().get(), 0);
        let lease = authority.create().expect("first provider must be created");

        assert_eq!(lease.authority_domain(), authority_domain);
        assert_eq!(lease.incarnation(), 1);
        assert_eq!(authority.frontier().get(), 1);
        assert_eq!(authority.active(), Some(lease));
        assert_eq!(
            authority.classify(lease),
            PlatformObservationLeaseClassification::Active
        );
    }

    #[test]
    fn create_rejects_a_second_live_provider_without_mutation() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let active = authority.create().expect("first provider must be created");
        let before = authority.clone();

        assert_eq!(
            authority.create(),
            Err(PlatformObservationAuthorityError::ProviderAlreadyActive { active })
        );
        assert_eq!(authority, before);
    }

    #[test]
    fn replacement_requires_quiescence_before_the_successor_becomes_active() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let first = authority.create().expect("first provider must be created");
        let ticket = authority
            .begin_replacement(first)
            .expect("exact active provider must begin replacement");
        let successor = ticket.successor;

        assert_eq!(authority.frontier().get(), 2);
        assert_eq!(successor.incarnation(), 2);
        assert_eq!(authority.active(), None);
        assert_eq!(
            authority.classify(first),
            PlatformObservationLeaseClassification::Superseded { successor }
        );
        assert_eq!(
            authority.require_active(first),
            Err(PlatformObservationAuthorityError::SupersededLease {
                lease: first,
                successor,
            })
        );
        assert_eq!(
            authority.require_active(successor),
            Err(PlatformObservationAuthorityError::UnknownLease { lease: successor })
        );
        assert_eq!(
            authority.create(),
            Err(
                PlatformObservationAuthorityError::ProviderReplacementPending {
                    predecessor: first,
                }
            )
        );

        assert_eq!(
            authority
                .finish_replacement(ticket)
                .expect("quiesced replacement must finish"),
            successor
        );
        assert_eq!(authority.active(), Some(successor));
        assert_eq!(authority.frontier().get(), 3);
        assert_eq!(authority.require_active(successor), Ok(()));
    }

    #[test]
    fn retire_requires_the_exact_active_lease_and_is_terminal() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let lease = authority.create().expect("provider must be created");

        authority
            .retire(lease)
            .expect("exact active provider must retire");

        assert_eq!(authority.active(), None);
        assert_eq!(authority.frontier().get(), 2);
        assert_eq!(
            authority.classify(lease),
            PlatformObservationLeaseClassification::Retired
        );
        assert_eq!(
            authority.retire(lease),
            Err(PlatformObservationAuthorityError::RetiredLease { lease })
        );
    }

    #[test]
    fn foreign_engine_leases_never_gain_authority() {
        let mut first_authority = PlatformObservationAuthority::new(domain(1));
        let mut second_authority = PlatformObservationAuthority::new(domain(2));
        let first = first_authority
            .create()
            .expect("first engine provider must be created");
        let second = second_authority
            .create()
            .expect("second engine provider must be created");
        let first_before = first_authority.clone();
        let second_before = second_authority.clone();

        assert_eq!(
            first_authority.classify(second),
            PlatformObservationLeaseClassification::Foreign {
                expected: domain(1),
                submitted: domain(2),
            }
        );
        assert_eq!(
            first_authority.begin_replacement(second),
            Err(PlatformObservationAuthorityError::ForeignLease {
                expected: domain(1),
                submitted: domain(2),
            })
        );
        assert_eq!(
            second_authority.retire(first),
            Err(PlatformObservationAuthorityError::ForeignLease {
                expected: domain(2),
                submitted: domain(1),
            })
        );
        assert_eq!(first_authority, first_before);
        assert_eq!(second_authority, second_before);
    }

    #[test]
    fn same_domain_unminted_lease_is_unknown_and_cannot_replace_active() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let active = authority.create().expect("provider must be created");
        let unknown = forged_lease(domain(1), 99);
        let before = authority.clone();

        assert_eq!(
            authority.classify(unknown),
            PlatformObservationLeaseClassification::Unknown
        );
        assert_eq!(
            authority.begin_replacement(unknown),
            Err(PlatformObservationAuthorityError::UnknownLease { lease: unknown })
        );
        assert_eq!(authority, before);
        assert_eq!(authority.active(), Some(active));
    }

    #[test]
    fn old_lease_cannot_replace_or_retire_its_successor() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let first = authority.create().expect("first provider must be created");
        let ticket = authority
            .begin_replacement(first)
            .expect("replacement must begin");
        let successor = authority
            .finish_replacement(ticket)
            .expect("replacement must finish");
        let before = authority.clone();
        let error = PlatformObservationAuthorityError::SupersededLease {
            lease: first,
            successor,
        };

        assert_eq!(authority.begin_replacement(first), Err(error));
        assert_eq!(authority.retire(first), Err(error));
        assert_eq!(authority, before);
        assert_eq!(authority.active(), Some(successor));
    }

    #[test]
    fn long_replacement_chain_reconstructs_the_oldest_exact_successor() {
        let authority_domain = domain(1);
        let mut authority = PlatformObservationAuthority::new(authority_domain);
        let first = authority.create().expect("first provider must be created");
        let mut active = first;

        for _ in 0..1_024 {
            let ticket = authority
                .begin_replacement(active)
                .expect("active provider begins replacement");
            active = authority
                .finish_replacement(ticket)
                .expect("reserved successor becomes active");
        }

        assert_eq!(
            authority.classify(first),
            PlatformObservationLeaseClassification::Superseded {
                successor: forged_lease(authority_domain, 2),
            }
        );
        assert_eq!(authority.active(), Some(active));
        assert_eq!(active.incarnation(), 1_025);
    }

    #[test]
    fn foreign_or_non_pending_replacement_ticket_cannot_activate_a_provider() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let first = authority.create().expect("provider must be created");
        let pending = authority
            .begin_replacement(first)
            .expect("replacement must begin");
        let before = authority.clone();
        let foreign = PlatformProviderReplacementTicket::new(
            domain(2),
            forged_lease(domain(2), 1),
            forged_lease(domain(2), 2),
        );

        assert_eq!(
            authority.finish_replacement(foreign),
            Err(
                PlatformObservationAuthorityError::ForeignReplacementTicket {
                    expected: domain(1),
                    submitted: domain(2),
                }
            )
        );
        assert_eq!(authority, before);

        let successor = authority
            .finish_replacement(pending)
            .expect("exact pending replacement must finish");
        let finished = authority.clone();
        assert_eq!(
            authority.finish_replacement(pending),
            Err(PlatformObservationAuthorityError::UnknownReplacementTicket)
        );
        assert_eq!(authority, finished);
        assert_eq!(authority.active(), Some(successor));
    }

    #[test]
    fn create_incarnation_exhaustion_is_atomic() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        authority.last_incarnation = PlatformProviderIncarnation(u64::MAX);
        let before = authority.clone();

        assert_eq!(
            authority.create(),
            Err(PlatformObservationAuthorityError::ProviderIncarnationExhausted)
        );
        assert_eq!(authority, before);
    }

    #[test]
    fn replacement_incarnation_exhaustion_preserves_the_exact_active_provider() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let active = authority.create().expect("provider must be created");
        authority.last_incarnation = PlatformProviderIncarnation(u64::MAX);
        let before = authority.clone();

        assert_eq!(
            authority.begin_replacement(active),
            Err(PlatformObservationAuthorityError::ProviderIncarnationExhausted)
        );
        assert_eq!(authority, before);
        assert_eq!(authority.active(), Some(active));
        assert_eq!(
            authority.classify(active),
            PlatformObservationLeaseClassification::Active
        );
    }

    #[test]
    fn authority_frontier_exhaustion_is_atomic_at_replacement_activation() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let active = authority.create().expect("provider must be created");
        let ticket = authority
            .begin_replacement(active)
            .expect("replacement must begin");
        authority.frontier = PlatformProviderAuthorityFrontier(u64::MAX);
        let before = authority.clone();

        assert_eq!(
            authority.finish_replacement(ticket),
            Err(PlatformObservationAuthorityError::ProviderAuthorityFrontierExhausted)
        );
        assert_eq!(authority, before);
    }

    #[test]
    fn terminal_retirement_never_allows_a_successor_provider() {
        let mut authority = PlatformObservationAuthority::new(domain(1));
        let active = authority.create().expect("provider must be created");
        authority.last_incarnation = PlatformProviderIncarnation(u64::MAX);

        authority
            .retire(active)
            .expect("retirement does not allocate another provider");
        assert_eq!(authority.active(), None);
        assert_eq!(
            authority.create(),
            Err(PlatformObservationAuthorityError::ProviderAuthorityRetired)
        );
        assert_eq!(
            authority.classify(active),
            PlatformObservationLeaseClassification::Retired
        );
    }
}
