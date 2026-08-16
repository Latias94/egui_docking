//! Exact receiver evidence for provisional pointer-edge candidates.
//!
//! One physical pointer edge carries independent click and drag delivery
//! routes plus an optional hover-drop hit at the edge's exact logical point.
//! The core freezes the requested probe set for one host-frame attempt and
//! adapters answer that set exactly once without deriving core semantics from
//! framework geometry.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::atomic::{AtomicU64, Ordering};

use thiserror::Error;

use crate::geometry::LogicalPoint;
use crate::ids::{EngineAuthorityDomainId, SurfaceId};
use crate::pointer_journal::{FiniteScrollVector, PointerEdgeSequence, PointerInputLease};
use crate::presentation_hit::{PresentationHitRegionId, PresentationPointerLane};
use crate::presentation_observation::{PresentedSurfaceAuthority, SurfacePresentationOutputTicket};
use crate::scene::SurfaceInteractionProjection;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
struct PointerReceiverAttemptSerial(u64);

/// Opaque identity of one non-replayable host-frame receiver attempt.
///
/// A failed host frame must not reuse this identity. The engine-owned issuer is
/// therefore advanced when the host frame begins, outside any rollbackable
/// engine candidate clone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerReceiverFrameAttemptId {
    authority_domain: EngineAuthorityDomainId,
    serial: PointerReceiverAttemptSerial,
}

impl PointerReceiverFrameAttemptId {
    /// Returns the engine authority domain which minted this attempt.
    #[must_use]
    pub const fn authority_domain(self) -> EngineAuthorityDomainId {
        self.authority_domain
    }

    /// Returns the process-local diagnostic representation.
    #[must_use]
    pub const fn serial(self) -> u64 {
        self.serial.0
    }
}

/// Engine-owned issuer for host-frame receiver attempt identities.
///
/// This type deliberately does not implement `Clone`: integrating it into
/// `DockEngine` must keep issuance outside rollbackable candidate state.
#[derive(Debug)]
#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
pub(crate) struct PointerReceiverAttemptIssuer {
    authority_domain: EngineAuthorityDomainId,
    last_attempt: AtomicU64,
}

impl PartialEq for PointerReceiverAttemptIssuer {
    fn eq(&self, other: &Self) -> bool {
        self.authority_domain == other.authority_domain
            && self.last_attempt.load(Ordering::Acquire)
                == other.last_attempt.load(Ordering::Acquire)
    }
}

impl Eq for PointerReceiverAttemptIssuer {}

#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
impl PointerReceiverAttemptIssuer {
    pub(crate) const fn new(authority_domain: EngineAuthorityDomainId) -> Self {
        Self {
            authority_domain,
            last_attempt: AtomicU64::new(0),
        }
    }

    /// Consumes one attempt serial even if later roster construction fails.
    pub(crate) fn issue(
        &self,
        lease: PointerInputLease,
    ) -> Result<PointerReceiverFrameAttempt, PointerReceiverAttemptError> {
        if lease.authority_domain() != self.authority_domain {
            return Err(PointerReceiverAttemptError::ForeignPointerLease {
                expected: self.authority_domain,
                submitted: lease.authority_domain(),
            });
        }
        let previous = self
            .last_attempt
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |last| {
                last.checked_add(1)
            })
            .map_err(|_| PointerReceiverAttemptError::AttemptSpaceExhausted)?;
        let serial = previous
            .checked_add(1)
            .map(PointerReceiverAttemptSerial)
            .ok_or(PointerReceiverAttemptError::AttemptSpaceExhausted)?;
        Ok(PointerReceiverFrameAttempt {
            id: PointerReceiverFrameAttemptId {
                authority_domain: self.authority_domain,
                serial,
            },
            lease,
        })
    }
}

/// Failure to mint one core-owned receiver attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerReceiverAttemptError {
    /// The pointer provider belongs to another engine authority domain.
    #[error(
        "pointer receiver attempt lease belongs to authority domain {submitted:?}, expected {expected:?}"
    )]
    ForeignPointerLease {
        /// Engine authority domain owned by the issuer.
        expected: EngineAuthorityDomainId,
        /// Authority domain embedded in the submitted provider lease.
        submitted: EngineAuthorityDomainId,
    },
    /// The engine-local attempt sequence cannot advance without wrapping.
    #[error("pointer receiver frame-attempt sequence is exhausted")]
    AttemptSpaceExhausted,
}

/// Single-use core token consumed when freezing a candidate roster.
#[derive(Debug, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
pub(crate) struct PointerReceiverFrameAttempt {
    id: PointerReceiverFrameAttemptId,
    lease: PointerInputLease,
}

/// Opaque identity of one edge candidate in one exact frame attempt.
///
/// Including both the provider incarnation and frame attempt prevents an
/// adapter from replaying a receipt after provider reset, failed frame retry,
/// or a later frame that happens to contain the same provider sequence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PointerReceiverCandidateId {
    attempt: PointerReceiverFrameAttemptId,
    lease: PointerInputLease,
    sequence: PointerEdgeSequence,
}

impl PointerReceiverCandidateId {
    const fn new(
        attempt: PointerReceiverFrameAttemptId,
        lease: PointerInputLease,
        sequence: PointerEdgeSequence,
    ) -> Self {
        Self {
            attempt,
            lease,
            sequence,
        }
    }

    /// Returns the exact host-frame attempt which owns this candidate.
    #[must_use]
    pub const fn attempt(self) -> PointerReceiverFrameAttemptId {
        self.attempt
    }

    /// Returns the exact provider incarnation which owns this candidate.
    #[must_use]
    pub const fn lease(self) -> PointerInputLease {
        self.lease
    }

    /// Returns the provider-owned edge sequence represented by this candidate.
    #[must_use]
    pub const fn sequence(self) -> PointerEdgeSequence {
        self.sequence
    }
}

/// One independent fact requested for one physical pointer edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PointerReceiverProbe {
    /// The independent click and drag receivers or blockers for physical delivery.
    Delivery,
    /// The hover-drop target hit at the edge's exact logical point.
    HoverHit,
}

/// Exact receiver-probe set requested for one pointer edge.
///
/// This is an explicit finite set of physical questions. A delivery answer
/// contains the framework's independent click and drag lane facts; hover-drop
/// remains a separate point-bound question.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerReceiverProbeRequest {
    /// The edge has no receiver role.
    NotApplicable,
    /// Request the physical click and drag delivery receivers or blockers.
    Delivery,
    /// Request the point-bound hover-drop hit only.
    HoverHit,
}

/// Exact delivery lanes requested from one renderer receiver observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PointerReceiverDeliveryRequest {
    /// This candidate has no delivery-receiver role.
    None,
    /// Only the click lane must be resolved.
    Click,
    /// Click and drag lanes must be resolved independently.
    ClickAndDrag,
    /// Only the scroll lane must be resolved.
    Scroll,
}

/// Core-owned receiver and derivative requirement for one scroll edge.
///
/// This type keeps framework derivative ownership separate from renderer hit
/// evidence. Adapters must not infer either fact from a missing probe vector or
/// from the receiver returned by their own hit test.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollReceiverChallenge {
    /// An unowned scroll sample needs one spatial receiver proof.
    Spatial {
        /// Modifier-projected direction used for receiver admission, when the
        /// provider has already emitted a directional sample.
        ///
        /// `None` still requires an exact top-receiver proof. Smooth `Begin`
        /// freezes that receiver before any directional delta arrives.
        projected_delta: Option<FiniteScrollVector>,
    },
    /// An active docking sequence must prove its frozen receiver.
    Locked {
        /// Stable receiver identity frozen when the sequence began.
        receiver: PresentationHitRegionId,
        /// Stable point inside the frozen receiver, independent of the current pointer.
        probe_point: LogicalPoint,
        /// Direction for blocker and axis-admission checks, when the edge has a delta.
        projected_delta: Option<FiniteScrollVector>,
    },
    /// Docking already terminated semantically but still owns the provider sequence.
    OwnedTerminal,
    /// The framework owns this exact modified scroll sample.
    FrameworkReserved,
    /// Receiver or conversion authority is unavailable, or the sequence never became dock-owned.
    Unavailable,
}

impl ScrollReceiverChallenge {
    const fn requires_receiver_probe(self) -> bool {
        matches!(self, Self::Spatial { .. } | Self::Locked { .. })
    }

    const fn locked_receiver(self) -> Option<PresentationHitRegionId> {
        match self {
            Self::Locked { receiver, .. } => Some(receiver),
            Self::Spatial { .. }
            | Self::OwnedTerminal
            | Self::FrameworkReserved
            | Self::Unavailable => None,
        }
    }
}

impl PointerReceiverProbeRequest {
    /// Returns whether this exact probe is required.
    #[must_use]
    pub const fn requires(self, probe: PointerReceiverProbe) -> bool {
        matches!(
            (self, probe),
            (Self::Delivery, PointerReceiverProbe::Delivery)
                | (Self::HoverHit, PointerReceiverProbe::HoverHit)
        )
    }

    /// Returns whether receiver evidence is intentionally absent.
    #[must_use]
    pub const fn is_not_applicable(self) -> bool {
        matches!(self, Self::NotApplicable)
    }

    /// Returns requested probes in canonical order.
    #[must_use]
    pub const fn probes(self) -> &'static [PointerReceiverProbe] {
        match self {
            Self::NotApplicable => &[],
            Self::Delivery => &[PointerReceiverProbe::Delivery],
            Self::HoverHit => &[PointerReceiverProbe::HoverHit],
        }
    }
}

/// One core-frozen receiver question for a provisional pointer edge.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerReceiverCandidate {
    id: PointerReceiverCandidateId,
    probes: PointerReceiverProbeRequest,
    delivery_surface: Option<SurfaceId>,
    route_point: Option<LogicalPoint>,
    hover_surface: Option<SurfaceId>,
    hover_point: Option<LogicalPoint>,
    delivery_request: PointerReceiverDeliveryRequest,
    scroll_challenge: Option<ScrollReceiverChallenge>,
}

impl PointerReceiverCandidate {
    /// Returns the opaque identity adapters must echo in a receipt.
    #[must_use]
    pub const fn id(&self) -> PointerReceiverCandidateId {
        self.id
    }

    /// Returns the exact independent facts requested for this edge.
    #[must_use]
    pub const fn probes(&self) -> PointerReceiverProbeRequest {
        self.probes
    }

    /// Returns the exact logical surface which received the physical edge.
    ///
    /// This is independent from [`Self::hover_surface`]. A captured native
    /// pointer may be delivered through one window while hovering another.
    #[must_use]
    pub const fn delivery_surface(&self) -> Option<SurfaceId> {
        self.delivery_surface
    }

    /// Returns the surface-local point validated by core for this edge route.
    ///
    /// This is available independently from [`Self::hover_point`]. Delivery-only
    /// probes still need the exact point to query a retained renderer hit graph,
    /// while only hover probes echo the point in their receipt.
    #[must_use]
    pub const fn route_point(&self) -> Option<LogicalPoint> {
        self.route_point
    }

    /// Returns the exact logical surface under the pointer for the hover probe.
    #[must_use]
    pub const fn hover_surface(&self) -> Option<SurfaceId> {
        self.hover_surface
    }

    /// Returns the exact edge point a known hover hit must echo.
    ///
    /// `None` means the provider's surface-local position was unavailable. A
    /// known hover hit is then forbidden; adapters may submit a per-probe
    /// `Unknown` or make the entire candidate `Unknown`.
    #[must_use]
    pub const fn hover_point(&self) -> Option<LogicalPoint> {
        self.hover_point
    }

    pub(crate) const fn delivery_request(&self) -> PointerReceiverDeliveryRequest {
        self.delivery_request
    }

    /// Returns the core-owned scroll receiver and derivative requirement.
    ///
    /// `None` identifies a non-scroll edge. The adapter must obey the explicit
    /// challenge and must not derive derivative ownership from its receipt.
    #[must_use]
    pub const fn scroll_challenge(&self) -> Option<ScrollReceiverChallenge> {
        self.scroll_challenge
    }

    /// Returns whether this candidate requires actual receiver evidence.
    #[must_use]
    pub const fn receiver_is_applicable(&self) -> bool {
        !self.probes.is_not_applicable()
    }

    /// Answers this exact candidate with one receiver observation.
    #[must_use]
    pub fn receipt(&self, observation: PointerReceiverObservation) -> PointerReceiverReceipt {
        PointerReceiverReceipt {
            candidate: self.id,
            observation,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
pub(crate) struct PointerReceiverCandidateSpec {
    sequence: PointerEdgeSequence,
    probes: PointerReceiverProbeRequest,
    delivery_surface: Option<SurfaceId>,
    route_point: Option<LogicalPoint>,
    hover_surface: Option<SurfaceId>,
    hover_point: Option<LogicalPoint>,
    delivery_request: PointerReceiverDeliveryRequest,
    scroll_challenge: Option<ScrollReceiverChallenge>,
}

#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
impl PointerReceiverCandidateSpec {
    pub(crate) const fn not_applicable(sequence: PointerEdgeSequence) -> Self {
        Self {
            sequence,
            probes: PointerReceiverProbeRequest::NotApplicable,
            delivery_surface: None,
            route_point: None,
            hover_surface: None,
            hover_point: None,
            delivery_request: PointerReceiverDeliveryRequest::None,
            scroll_challenge: None,
        }
    }

    pub(crate) const fn delivery(
        sequence: PointerEdgeSequence,
        delivery_surface: Option<SurfaceId>,
        route_point: Option<LogicalPoint>,
        delivery_request: PointerReceiverDeliveryRequest,
    ) -> Self {
        Self {
            sequence,
            probes: if delivery_surface.is_some() {
                PointerReceiverProbeRequest::Delivery
            } else {
                PointerReceiverProbeRequest::NotApplicable
            },
            delivery_surface,
            route_point,
            hover_surface: None,
            hover_point: None,
            delivery_request: if delivery_surface.is_some() {
                delivery_request
            } else {
                PointerReceiverDeliveryRequest::None
            },
            scroll_challenge: None,
        }
    }

    pub(crate) const fn scroll_delivery(
        sequence: PointerEdgeSequence,
        delivery_surface: Option<SurfaceId>,
        route_point: Option<LogicalPoint>,
        challenge: ScrollReceiverChallenge,
    ) -> Self {
        Self {
            sequence,
            probes: if challenge.requires_receiver_probe() && delivery_surface.is_some() {
                PointerReceiverProbeRequest::Delivery
            } else {
                PointerReceiverProbeRequest::NotApplicable
            },
            delivery_surface,
            route_point,
            hover_surface: None,
            hover_point: None,
            delivery_request: if challenge.requires_receiver_probe() && delivery_surface.is_some() {
                PointerReceiverDeliveryRequest::Scroll
            } else {
                PointerReceiverDeliveryRequest::None
            },
            scroll_challenge: Some(challenge),
        }
    }

    pub(crate) const fn hover_hit(
        sequence: PointerEdgeSequence,
        hover_surface: Option<SurfaceId>,
        hover_point: Option<LogicalPoint>,
    ) -> Self {
        Self {
            sequence,
            probes: PointerReceiverProbeRequest::HoverHit,
            delivery_surface: None,
            route_point: None,
            hover_surface,
            hover_point,
            delivery_request: PointerReceiverDeliveryRequest::None,
            scroll_challenge: None,
        }
    }

    const fn referenced_presented_surface(&self) -> Option<SurfaceId> {
        match self.probes {
            PointerReceiverProbeRequest::NotApplicable => None,
            PointerReceiverProbeRequest::Delivery => self.delivery_surface,
            PointerReceiverProbeRequest::HoverHit => self.hover_surface,
        }
    }
}

/// Exact interactive output admitted into one candidate roster.
///
/// It can only be derived from an indivisible interaction projection, never a
/// paint-only projection or naked output ticket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
pub(crate) struct PointerReceiverPresentedOutput {
    ticket: SurfacePresentationOutputTicket,
    authority: PresentedSurfaceAuthority,
}

#[allow(
    dead_code,
    reason = "reserved for CoreHostFrame pointer-receiver integration"
)]
impl PointerReceiverPresentedOutput {
    pub(crate) fn from_interaction(projection: SurfaceInteractionProjection<'_>) -> Self {
        let ticket = projection.output_ticket();
        let authority = projection.authority();
        debug_assert_eq!(projection.hit_manifest().output(), ticket);
        debug_assert!(authority.matches_output(ticket));
        Self { ticket, authority }
    }
}

/// Canonical exact set of provisional edge candidates and current outputs.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerReceiverCandidateRoster {
    attempt: PointerReceiverFrameAttemptId,
    lease: PointerInputLease,
    candidates: Vec<PointerReceiverCandidate>,
    presented_outputs: BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
}

impl PointerReceiverCandidateRoster {
    /// Freezes only outputs referenced by this segment's exact receiver probes.
    ///
    /// A missing output leaves its candidate intact, but prevents that
    /// candidate from submitting known presentation authority.
    pub(crate) fn freeze_referenced_outputs(
        attempt: PointerReceiverFrameAttempt,
        specs: Vec<PointerReceiverCandidateSpec>,
        available_outputs: &BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
    ) -> Result<Self, PointerReceiverCandidateRosterError> {
        let specs = Self::canonicalize_specs(specs)?;
        let presented_outputs = specs
            .iter()
            .filter_map(PointerReceiverCandidateSpec::referenced_presented_surface)
            .filter_map(|surface| {
                available_outputs
                    .get(&surface)
                    .copied()
                    .map(|output| (surface, output))
            })
            .collect();
        Ok(Self::from_canonical_parts(
            attempt,
            specs,
            presented_outputs,
        ))
    }

    #[allow(
        dead_code,
        reason = "reserved for CoreHostFrame pointer-receiver integration"
    )]
    pub(crate) fn freeze(
        attempt: PointerReceiverFrameAttempt,
        specs: Vec<PointerReceiverCandidateSpec>,
        outputs: Vec<PointerReceiverPresentedOutput>,
    ) -> Result<Self, PointerReceiverCandidateRosterError> {
        let specs = Self::canonicalize_specs(specs)?;
        let mut presented_outputs = BTreeMap::new();
        for output in outputs {
            let surface = output.ticket.surface();
            if presented_outputs.insert(surface, output).is_some() {
                return Err(
                    PointerReceiverCandidateRosterError::DuplicatePresentedSurface { surface },
                );
            }
        }

        Ok(Self::from_canonical_parts(
            attempt,
            specs,
            presented_outputs,
        ))
    }

    fn canonicalize_specs(
        mut specs: Vec<PointerReceiverCandidateSpec>,
    ) -> Result<Vec<PointerReceiverCandidateSpec>, PointerReceiverCandidateRosterError> {
        specs.sort_unstable_by_key(|spec| spec.sequence);
        if let Some(pair) = specs
            .windows(2)
            .find(|pair| pair[0].sequence == pair[1].sequence)
        {
            return Err(
                PointerReceiverCandidateRosterError::DuplicateCandidateSequence {
                    sequence: pair[0].sequence,
                },
            );
        }
        Ok(specs)
    }

    fn from_canonical_parts(
        attempt: PointerReceiverFrameAttempt,
        specs: Vec<PointerReceiverCandidateSpec>,
        presented_outputs: BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
    ) -> Self {
        let candidates = specs
            .into_iter()
            .map(|spec| PointerReceiverCandidate {
                id: PointerReceiverCandidateId::new(attempt.id, attempt.lease, spec.sequence),
                probes: spec.probes,
                delivery_surface: spec.delivery_surface,
                route_point: spec.route_point,
                hover_surface: spec.hover_surface,
                hover_point: spec.hover_point,
                delivery_request: spec.delivery_request,
                scroll_challenge: spec.scroll_challenge,
            })
            .collect();
        Self {
            attempt: attempt.id,
            lease: attempt.lease,
            candidates,
            presented_outputs,
        }
    }

    /// Returns the exact non-replayable frame-attempt identity.
    #[must_use]
    pub const fn attempt(&self) -> PointerReceiverFrameAttemptId {
        self.attempt
    }

    /// Returns the frozen provider incarnation for every candidate.
    #[must_use]
    pub const fn lease(&self) -> PointerInputLease {
        self.lease
    }

    /// Returns candidates in canonical provider-sequence order.
    ///
    /// Input `Vec` order has no semantic meaning; construction canonicalizes it.
    #[must_use]
    pub fn candidates(&self) -> &[PointerReceiverCandidate] {
        &self.candidates
    }

    #[allow(
        dead_code,
        reason = "reserved for CoreHostFrame pointer-receiver integration"
    )]
    pub(crate) fn validate(
        &self,
        batch: PointerReceiverReceiptBatch,
    ) -> Result<ValidatedPointerReceiverReceiptBatch, PointerReceiverReceiptValidationError> {
        self.validate_against_presented_outputs(batch, &self.presented_outputs)
    }

    /// Validates receipts against the interactive outputs which remain current
    /// after the host frame reduces its presentation observations.
    ///
    /// Candidate identities and requested probes are frozen at frame begin,
    /// but a receipt for an output retired or superseded by the observation
    /// prelude cannot authorize an edge later in that same frame.
    pub(crate) fn validate_against_presented_outputs(
        &self,
        batch: PointerReceiverReceiptBatch,
        presented_outputs: &BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
    ) -> Result<ValidatedPointerReceiverReceiptBatch, PointerReceiverReceiptValidationError> {
        for receipt in &batch.receipts {
            if receipt.candidate.lease() != self.lease {
                return Err(PointerReceiverReceiptValidationError::ForeignPointerLease {
                    expected: self.lease,
                    submitted: receipt.candidate.lease(),
                });
            }
            if receipt.candidate.attempt() != self.attempt {
                return Err(PointerReceiverReceiptValidationError::ForeignFrameAttempt {
                    expected: self.attempt,
                    submitted: receipt.candidate.attempt(),
                });
            }
        }

        let expected = self
            .candidates
            .iter()
            .map(PointerReceiverCandidate::id)
            .collect::<BTreeSet<_>>();
        let submitted = batch
            .receipts
            .iter()
            .map(PointerReceiverReceipt::candidate)
            .collect::<BTreeSet<_>>();
        if let Some(candidate) = expected.difference(&submitted).next().copied() {
            return Err(PointerReceiverReceiptValidationError::MissingCandidate { candidate });
        }
        if let Some(candidate) = submitted.difference(&expected).next().copied() {
            return Err(PointerReceiverReceiptValidationError::UnexpectedCandidate { candidate });
        }

        let receipts = batch
            .receipts
            .into_iter()
            .zip(&self.candidates)
            .map(|(receipt, candidate)| {
                debug_assert_eq!(receipt.candidate, candidate.id);
                self.validate_observation(candidate, &receipt.observation, presented_outputs)?;
                Ok(ValidatedPointerReceiverReceipt {
                    candidate: receipt.candidate,
                    observation: receipt.observation,
                })
            })
            .collect::<Result<Vec<_>, PointerReceiverReceiptValidationError>>()?;
        Ok(ValidatedPointerReceiverReceiptBatch { receipts })
    }

    fn validate_observation(
        &self,
        candidate: &PointerReceiverCandidate,
        observation: &PointerReceiverObservation,
        presented_outputs: &BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
    ) -> Result<(), PointerReceiverReceiptValidationError> {
        match observation {
            PointerReceiverObservation::NotApplicable if candidate.probes.is_not_applicable() => {
                Ok(())
            }
            PointerReceiverObservation::NotApplicable => Err(
                PointerReceiverReceiptValidationError::NotApplicableForRequiredCandidate {
                    candidate: candidate.id,
                },
            ),
            PointerReceiverObservation::Unknown(_) if !candidate.probes.is_not_applicable() => {
                Ok(())
            }
            PointerReceiverObservation::Unknown(_) => Err(
                PointerReceiverReceiptValidationError::UnknownForNotApplicableCandidate {
                    candidate: candidate.id,
                },
            ),
            PointerReceiverObservation::Presented(_) if candidate.probes.is_not_applicable() => {
                Err(
                    PointerReceiverReceiptValidationError::PresentedForNotApplicableCandidate {
                        candidate: candidate.id,
                    },
                )
            }
            PointerReceiverObservation::Presented(presented) => {
                let mut submitted = BTreeSet::new();
                for receipt in presented.probes() {
                    if !submitted.insert(receipt.probe()) {
                        return Err(PointerReceiverReceiptValidationError::DuplicateProbe {
                            candidate: candidate.id,
                            probe: receipt.probe(),
                        });
                    }
                }
                for probe in candidate.probes.probes() {
                    if !submitted.contains(probe) {
                        return Err(PointerReceiverReceiptValidationError::MissingProbe {
                            candidate: candidate.id,
                            probe: *probe,
                        });
                    }
                }
                for probe in submitted {
                    if !candidate.probes.requires(probe) {
                        return Err(PointerReceiverReceiptValidationError::UnexpectedProbe {
                            candidate: candidate.id,
                            probe,
                        });
                    }
                }

                for receipt in presented.probes() {
                    match receipt {
                        PointerReceiverProbeReceipt::Delivery(delivery) => {
                            if let (
                                Some(expected),
                                PointerReceiverDeliveryDisposition::Dock(submitted),
                            ) = (
                                candidate
                                    .scroll_challenge
                                    .and_then(ScrollReceiverChallenge::locked_receiver),
                                delivery.scroll(),
                            ) && submitted != expected
                            {
                                return Err(
                                    PointerReceiverReceiptValidationError::LockedScrollReceiverMismatch {
                                        candidate: candidate.id,
                                        expected,
                                        submitted,
                                    },
                                );
                            }
                            if let Some((output, authority)) = delivery.known_authority() {
                                self.validate_expected_surface(
                                    candidate.id,
                                    PointerReceiverProbe::Delivery,
                                    candidate.delivery_surface,
                                    output.surface(),
                                )?;
                                self.validate_known_authority(
                                    candidate.id,
                                    PointerReceiverProbe::Delivery,
                                    output,
                                    authority,
                                    presented_outputs,
                                )?;
                            }
                        }
                        PointerReceiverProbeReceipt::HoverHit(hover_hit) => {
                            if let Some((output, authority, point)) = hover_hit.known_authority() {
                                let Some(expected) = candidate.hover_point else {
                                    return Err(
                                        PointerReceiverReceiptValidationError::HoverHitPointUnavailable {
                                            candidate: candidate.id,
                                        },
                                    );
                                };
                                if point != expected {
                                    return Err(
                                        PointerReceiverReceiptValidationError::HoverHitPointMismatch {
                                            candidate: candidate.id,
                                            expected,
                                            submitted: point,
                                        },
                                    );
                                }
                                self.validate_expected_surface(
                                    candidate.id,
                                    PointerReceiverProbe::HoverHit,
                                    candidate.hover_surface,
                                    output.surface(),
                                )?;
                                self.validate_known_authority(
                                    candidate.id,
                                    PointerReceiverProbe::HoverHit,
                                    output,
                                    authority,
                                    presented_outputs,
                                )?;
                            }
                        }
                    }
                }
                Ok(())
            }
        }
    }

    fn validate_expected_surface(
        &self,
        candidate: PointerReceiverCandidateId,
        probe: PointerReceiverProbe,
        expected: Option<SurfaceId>,
        submitted: SurfaceId,
    ) -> Result<(), PointerReceiverReceiptValidationError> {
        if expected.is_some_and(|expected| expected != submitted) {
            return Err(
                PointerReceiverReceiptValidationError::ProbeSurfaceMismatch {
                    candidate,
                    probe,
                    expected: expected.expect("mismatch requires an expected surface"),
                    submitted,
                },
            );
        }
        Ok(())
    }

    fn validate_known_authority(
        &self,
        candidate: PointerReceiverCandidateId,
        probe: PointerReceiverProbe,
        submitted_output: SurfacePresentationOutputTicket,
        submitted_authority: PresentedSurfaceAuthority,
        presented_outputs: &BTreeMap<SurfaceId, PointerReceiverPresentedOutput>,
    ) -> Result<(), PointerReceiverReceiptValidationError> {
        let frozen = self
            .presented_outputs
            .get(&submitted_output.surface())
            .copied();
        let current = presented_outputs.get(&submitted_output.surface()).copied();
        if !submitted_authority.matches_output(submitted_output)
            || frozen.is_none_or(|frozen| {
                frozen.ticket != submitted_output || frozen.authority != submitted_authority
            })
            || current.is_none_or(|current| {
                current.ticket != submitted_output
                    || !current
                        .authority
                        .same_interaction_semantics(submitted_authority)
            })
        {
            return Err(
                PointerReceiverReceiptValidationError::PresentationAuthorityNotCurrent {
                    candidate,
                    probe,
                    submitted_output,
                    submitted_authority,
                    current_output: current.map(|current| current.ticket),
                    current_authority: current.map(|current| current.authority),
                },
            );
        }
        Ok(())
    }
}

/// Structural failure while freezing one core-owned candidate roster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerReceiverCandidateRosterError {
    /// Two candidate specifications named the same provider edge.
    #[error("pointer receiver roster contains duplicate edge sequence {sequence}")]
    DuplicateCandidateSequence {
        /// Duplicated provider sequence.
        sequence: PointerEdgeSequence,
    },
    /// More than one interactive output was supplied for one logical surface.
    #[error("pointer receiver roster contains duplicate presented surface {surface}")]
    DuplicatePresentedSurface {
        /// Duplicated logical surface.
        surface: SurfaceId,
    },
}

/// Actual disposition of one framework delivery lane.
///
/// Click and drag routing are independent framework facts. A single physical
/// edge can therefore name different receivers in the two lanes without
/// granting either receiver a capability which is absent from the core-owned
/// hit manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerReceiverDeliveryDisposition {
    /// One core-compiled docking region received the edge.
    Dock(PresentationHitRegionId),
    /// The presentation-owned canvas received the edge outside a control.
    DockCanvas,
    /// A higher framework receiver blocked docking delivery.
    Blocked,
    /// The framework authoritatively proved no receiver accepted delivery.
    NoReceiver,
    /// Framework delivery could not be authoritatively determined.
    Unknown(PointerReceiverUnknownReason),
}

/// Point-bound disposition of the hover-drop probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerReceiverHoverHitDisposition {
    /// One core-compiled hover-drop target was hit.
    Dock(PresentationHitRegionId),
    /// A higher framework receiver blocks hover-drop interaction.
    Blocked,
    /// No hover-drop receiver exists at the exact point.
    NoReceiver,
    /// The framework could not authoritatively determine the hover hit.
    Unknown(PointerReceiverUnknownReason),
}

/// Delivery evidence for one exact candidate probe.
///
/// Click and drag dispositions are independent, but every known disposition is
/// bound to the same final-presentation output authority. This prevents an
/// adapter from combining lane facts from different surfaces or presentation
/// generations while preserving frameworks whose click and drag routes differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PointerReceiverDelivery {
    output: Option<SurfacePresentationOutputTicket>,
    authority: Option<PresentedSurfaceAuthority>,
    click: PointerReceiverDeliveryDisposition,
    drag: PointerReceiverDeliveryDisposition,
    scroll: PointerReceiverDeliveryDisposition,
}

impl PointerReceiverDelivery {
    /// Binds one receiver to every delivery lane it owns in the manifest.
    ///
    /// This is a strict shorthand for frameworks which report one receiver for
    /// both lanes. A lane absent from the region remains typed `Unknown`, never
    /// an inferred `NoReceiver`. Use [`Self::from_lanes`] when routing differs.
    pub fn new(
        projection: SurfaceInteractionProjection<'_>,
        disposition: PointerReceiverDeliveryDisposition,
    ) -> Result<Self, PointerReceiverObservationError> {
        let (click, drag, scroll) = match disposition {
            PointerReceiverDeliveryDisposition::Dock(region) => {
                validate_region_surface_and_behavior(projection, region)?;
                let region = projection
                    .hit_manifest()
                    .region(region)
                    .expect("validated manifest region remains available");
                let click = region
                    .lanes()
                    .contains(PresentationPointerLane::Click)
                    .then_some(disposition)
                    .unwrap_or(PointerReceiverDeliveryDisposition::Unknown(
                        PointerReceiverUnknownReason::NotReported,
                    ));
                let drag = region
                    .lanes()
                    .contains(PresentationPointerLane::Drag)
                    .then_some(disposition)
                    .unwrap_or(PointerReceiverDeliveryDisposition::Unknown(
                        PointerReceiverUnknownReason::NotReported,
                    ));
                let scroll = region
                    .lanes()
                    .contains(PresentationPointerLane::Scroll)
                    .then_some(disposition)
                    .unwrap_or(PointerReceiverDeliveryDisposition::Unknown(
                        PointerReceiverUnknownReason::NotReported,
                    ));
                if matches!(
                    (click, drag, scroll),
                    (
                        PointerReceiverDeliveryDisposition::Unknown(_),
                        PointerReceiverDeliveryDisposition::Unknown(_),
                        PointerReceiverDeliveryDisposition::Unknown(_)
                    )
                ) {
                    return Err(
                        PointerReceiverObservationError::DeliveryRegionDoesNotSupportAnyLane {
                            region: region.id(),
                        },
                    );
                }
                (click, drag, scroll)
            }
            _ => (disposition, disposition, disposition),
        };
        Self::from_lanes_with_scroll(projection, click, drag, scroll)
    }

    /// Binds independent click and drag dispositions to one exact output.
    ///
    /// Each `Dock` claim is checked against its corresponding manifest lane.
    /// If both lanes are unknown, the result intentionally carries no output
    /// authority; otherwise all known lane facts share the supplied projection.
    pub fn from_lanes(
        projection: SurfaceInteractionProjection<'_>,
        click: PointerReceiverDeliveryDisposition,
        drag: PointerReceiverDeliveryDisposition,
    ) -> Result<Self, PointerReceiverObservationError> {
        Self::from_lanes_with_scroll(
            projection,
            click,
            drag,
            PointerReceiverDeliveryDisposition::Unknown(PointerReceiverUnknownReason::NotReported),
        )
    }

    /// Binds independent click, drag, and scroll dispositions to one exact output.
    pub fn from_lanes_with_scroll(
        projection: SurfaceInteractionProjection<'_>,
        click: PointerReceiverDeliveryDisposition,
        drag: PointerReceiverDeliveryDisposition,
        scroll: PointerReceiverDeliveryDisposition,
    ) -> Result<Self, PointerReceiverObservationError> {
        let (output, authority) = projection_binding(projection)?;
        validate_delivery_lane(projection, PresentationPointerLane::Click, click)?;
        validate_delivery_lane(projection, PresentationPointerLane::Drag, drag)?;
        validate_delivery_lane(projection, PresentationPointerLane::Scroll, scroll)?;
        if matches!(click, PointerReceiverDeliveryDisposition::Unknown(_))
            && matches!(drag, PointerReceiverDeliveryDisposition::Unknown(_))
            && matches!(scroll, PointerReceiverDeliveryDisposition::Unknown(_))
        {
            return Ok(Self {
                output: None,
                authority: None,
                click,
                drag,
                scroll,
            });
        }
        Ok(Self {
            output: Some(output),
            authority: Some(authority),
            click,
            drag,
            scroll,
        })
    }

    /// Creates delivery evidence for which neither lane can be determined.
    #[must_use]
    pub const fn unknown(reason: PointerReceiverUnknownReason) -> Self {
        Self {
            output: None,
            authority: None,
            click: PointerReceiverDeliveryDisposition::Unknown(reason),
            drag: PointerReceiverDeliveryDisposition::Unknown(reason),
            scroll: PointerReceiverDeliveryDisposition::Unknown(reason),
        }
    }

    /// Returns the click-lane delivery disposition.
    #[must_use]
    pub const fn click(self) -> PointerReceiverDeliveryDisposition {
        self.click
    }

    /// Returns the drag-lane delivery disposition.
    #[must_use]
    pub const fn drag(self) -> PointerReceiverDeliveryDisposition {
        self.drag
    }

    /// Returns the scroll-lane delivery disposition.
    #[must_use]
    pub const fn scroll(self) -> PointerReceiverDeliveryDisposition {
        self.scroll
    }

    /// Returns the output binding for a known delivery fact.
    #[must_use]
    pub const fn output(self) -> Option<SurfacePresentationOutputTicket> {
        self.output
    }

    /// Returns final-presentation authority for a known delivery fact.
    #[must_use]
    pub const fn authority(self) -> Option<PresentedSurfaceAuthority> {
        self.authority
    }

    fn known_authority(
        self,
    ) -> Option<(SurfacePresentationOutputTicket, PresentedSurfaceAuthority)> {
        match (self.output, self.authority) {
            (Some(output), Some(authority)) => Some((output, authority)),
            (None, None) => None,
            _ => {
                unreachable!("pointer receiver delivery constructors keep authority binding atomic")
            }
        }
    }
}

fn validate_delivery_lane(
    projection: SurfaceInteractionProjection<'_>,
    lane: PresentationPointerLane,
    disposition: PointerReceiverDeliveryDisposition,
) -> Result<(), PointerReceiverObservationError> {
    let PointerReceiverDeliveryDisposition::Dock(region) = disposition else {
        return Ok(());
    };
    validate_region_surface_and_behavior(projection, region)?;
    let region = projection
        .hit_manifest()
        .region(region)
        .expect("validated manifest region remains available");
    if !region.lanes().contains(lane) {
        return Err(
            PointerReceiverObservationError::DockRegionDoesNotSupportLane {
                region: region.id(),
                lane,
            },
        );
    }
    Ok(())
}

/// Hover-drop hit evidence for one exact candidate probe.
///
/// A known result carries the exact event-time logical point and the complete
/// final-presentation authority. The candidate join verifies that point against
/// the journal edge before any future source-aware drop resolver selects a
/// destination.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PointerReceiverHoverHit {
    output: Option<SurfacePresentationOutputTicket>,
    authority: Option<PresentedSurfaceAuthority>,
    point: Option<LogicalPoint>,
    disposition: PointerReceiverHoverHitDisposition,
}

impl PointerReceiverHoverHit {
    /// Binds one known hover-drop disposition to an exact output and point.
    ///
    /// A `Unknown` disposition intentionally carries neither point nor output
    /// authority. It is the only legal answer when the candidate lacks a known
    /// surface-local edge point.
    pub fn new(
        projection: SurfaceInteractionProjection<'_>,
        point: LogicalPoint,
        disposition: PointerReceiverHoverHitDisposition,
    ) -> Result<Self, PointerReceiverObservationError> {
        let (output, authority) = projection_binding(projection)?;
        if let PointerReceiverHoverHitDisposition::Unknown(reason) = disposition {
            return Ok(Self::unknown(reason));
        }
        if let PointerReceiverHoverHitDisposition::Dock(region) = disposition {
            validate_region_surface_and_behavior(projection, region)?;
            let region = projection
                .hit_manifest()
                .region(region)
                .expect("validated manifest region remains available");
            if !region.lanes().contains(PresentationPointerLane::HoverDrop) {
                return Err(
                    PointerReceiverObservationError::DockRegionDoesNotSupportLane {
                        region: region.id(),
                        lane: PresentationPointerLane::HoverDrop,
                    },
                );
            }
        }
        Ok(Self {
            output: Some(output),
            authority: Some(authority),
            point: Some(point),
            disposition,
        })
    }

    /// Creates hover evidence which the framework cannot determine.
    #[must_use]
    pub const fn unknown(reason: PointerReceiverUnknownReason) -> Self {
        Self {
            output: None,
            authority: None,
            point: None,
            disposition: PointerReceiverHoverHitDisposition::Unknown(reason),
        }
    }

    /// Returns the hover-drop disposition.
    #[must_use]
    pub const fn disposition(self) -> PointerReceiverHoverHitDisposition {
        self.disposition
    }

    /// Returns the exact logical point for a known hover hit.
    #[must_use]
    pub const fn point(self) -> Option<LogicalPoint> {
        self.point
    }

    /// Returns the output binding for a known hover hit.
    #[must_use]
    pub const fn output(self) -> Option<SurfacePresentationOutputTicket> {
        self.output
    }

    /// Returns final-presentation authority for a known hover hit.
    #[must_use]
    pub const fn authority(self) -> Option<PresentedSurfaceAuthority> {
        self.authority
    }

    fn known_authority(
        self,
    ) -> Option<(
        SurfacePresentationOutputTicket,
        PresentedSurfaceAuthority,
        LogicalPoint,
    )> {
        match (self.output, self.authority, self.point) {
            (Some(output), Some(authority), Some(point)) => Some((output, authority, point)),
            (None, None, None) => None,
            _ => {
                unreachable!("pointer receiver hover constructors keep point and authority atomic")
            }
        }
    }
}

/// One answer for one requested receiver probe.
#[derive(Debug, Clone, PartialEq)]
pub enum PointerReceiverProbeReceipt {
    /// Physical delivery fact.
    Delivery(PointerReceiverDelivery),
    /// Point-bound hover-drop fact.
    HoverHit(PointerReceiverHoverHit),
}

impl PointerReceiverProbeReceipt {
    /// Returns the exact probe represented by this receipt.
    #[must_use]
    pub const fn probe(&self) -> PointerReceiverProbe {
        match self {
            Self::Delivery(_) => PointerReceiverProbe::Delivery,
            Self::HoverHit(_) => PointerReceiverProbe::HoverHit,
        }
    }
}

/// Typed reason why framework receiver evidence is unavailable for one edge or
/// one requested probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PointerReceiverUnknownReason {
    /// The active framework/backend does not expose per-edge delivery facts.
    FrameworkDeliveryUnavailable,
    /// The provider could not retain an event-to-receiver correlation.
    EventCorrelationUnavailable,
    /// Front-to-back layer ownership could not be proven.
    LayerAuthorityUnavailable,
    /// No eligible final-presentation authority was observable for the edge.
    PresentationAuthorityUnavailable,
    /// The provider explicitly did not report this edge's receiver fact.
    NotReported,
}

/// Probe facts paired with final-presentation authority for one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct PresentedPointerReceiverObservation {
    probes: Vec<PointerReceiverProbeReceipt>,
}

impl PresentedPointerReceiverObservation {
    /// Builds a canonical exact probe set.
    ///
    /// Known dock claims are manifest-checked by the individual delivery and
    /// hover constructors. This constructor only canonicalizes independent
    /// probe identities and rejects duplicate answers.
    pub fn new(
        probes: impl IntoIterator<Item = PointerReceiverProbeReceipt>,
    ) -> Result<Self, PointerReceiverObservationError> {
        let mut probes = probes.into_iter().collect::<Vec<_>>();
        probes.sort_unstable_by_key(PointerReceiverProbeReceipt::probe);
        if probes.is_empty() {
            return Err(PointerReceiverObservationError::EmptyPresentedProbeSet);
        }
        if let Some(pair) = probes
            .windows(2)
            .find(|pair| pair[0].probe() == pair[1].probe())
        {
            return Err(PointerReceiverObservationError::DuplicateProbe {
                probe: pair[0].probe(),
            });
        }
        Ok(Self { probes })
    }

    /// Returns probe answers in canonical probe order.
    #[must_use]
    pub fn probes(&self) -> &[PointerReceiverProbeReceipt] {
        &self.probes
    }
}

/// Receiver answer for one exact pointer-edge candidate.
#[derive(Debug, Clone, PartialEq)]
pub enum PointerReceiverObservation {
    /// Receiver delivery has no semantic role for this edge.
    NotApplicable,
    /// The framework could not authoritatively determine any requested probe.
    Unknown(PointerReceiverUnknownReason),
    /// Per-probe facts, with every known fact bound to a final output.
    Presented(PresentedPointerReceiverObservation),
}

/// Failure while binding a known receiver probe to a hit manifest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerReceiverObservationError {
    /// The core supplied an internally inconsistent plan/manifest projection.
    #[error("pointer receiver projection manifest belongs to another output")]
    ProjectionOutputMismatch,
    /// The core supplied presentation authority for another output.
    #[error("pointer receiver projection authority belongs to another output")]
    ProjectionAuthorityMismatch,
    /// A presented observation contained no probe fact.
    #[error("presented pointer receiver observation has no probe facts")]
    EmptyPresentedProbeSet,
    /// One probe was answered more than once.
    #[error("presented pointer receiver observation contains duplicate probe {probe:?}")]
    DuplicateProbe {
        /// Duplicated independent probe identity.
        probe: PointerReceiverProbe,
    },
    /// A claimed docking receiver belongs to another logical surface.
    #[error(
        "docking receiver {region:?} belongs to another surface; output is for {output_surface}"
    )]
    DockRegionFromDifferentSurface {
        /// Surface bound to the exact presented output.
        output_surface: SurfaceId,
        /// Cross-surface region claim.
        region: PresentationHitRegionId,
    },
    /// A claimed region is absent from the exact output's manifest.
    #[error("docking receiver {region:?} is absent from the exact presented output")]
    DockRegionAbsentFromOutput {
        /// Missing semantic hit identity.
        region: PresentationHitRegionId,
    },
    /// A passive affordance predicate was incorrectly reported as a receiver.
    #[error("passive presentation region {region:?} cannot receive pointer delivery")]
    PassiveRegionCannotReceive {
        /// Passive semantic hit identity.
        region: PresentationHitRegionId,
    },
    /// The exact output does not expose the claimed region on this lane.
    #[error("docking receiver {region:?} does not support lane {lane:?}")]
    DockRegionDoesNotSupportLane {
        /// Semantic hit identity.
        region: PresentationHitRegionId,
        /// Unsupported receiver lane.
        lane: PresentationPointerLane,
    },
    /// A delivery region has no executable lane capability in the manifest.
    #[error("delivery receiver {region:?} supports no delivery lane")]
    DeliveryRegionDoesNotSupportAnyLane {
        /// Semantic hit identity.
        region: PresentationHitRegionId,
    },
}

fn projection_binding(
    projection: SurfaceInteractionProjection<'_>,
) -> Result<
    (SurfacePresentationOutputTicket, PresentedSurfaceAuthority),
    PointerReceiverObservationError,
> {
    let output = projection.output_ticket();
    let authority = projection.authority();
    if projection.hit_manifest().output() != output {
        return Err(PointerReceiverObservationError::ProjectionOutputMismatch);
    }
    if !authority.matches_output(output) {
        return Err(PointerReceiverObservationError::ProjectionAuthorityMismatch);
    }
    Ok((output, authority))
}

fn validate_region_surface_and_behavior(
    projection: SurfaceInteractionProjection<'_>,
    region_id: PresentationHitRegionId,
) -> Result<(), PointerReceiverObservationError> {
    let output = projection.output_ticket();
    if region_id.surface() != output.surface() {
        return Err(
            PointerReceiverObservationError::DockRegionFromDifferentSurface {
                output_surface: output.surface(),
                region: region_id,
            },
        );
    }
    let Some(region) = projection.hit_manifest().region(region_id) else {
        return Err(
            PointerReceiverObservationError::DockRegionAbsentFromOutput { region: region_id },
        );
    };
    if region.is_passive() {
        return Err(
            PointerReceiverObservationError::PassiveRegionCannotReceive { region: region_id },
        );
    }
    Ok(())
}

/// One candidate identity paired with its adapter observation.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerReceiverReceipt {
    candidate: PointerReceiverCandidateId,
    observation: PointerReceiverObservation,
}

impl PointerReceiverReceipt {
    /// Returns the exact provisional candidate being answered.
    #[must_use]
    pub const fn candidate(&self) -> PointerReceiverCandidateId {
        self.candidate
    }

    /// Returns the submitted receiver evidence.
    #[must_use]
    pub const fn observation(&self) -> &PointerReceiverObservation {
        &self.observation
    }
}

/// Canonical receipt set submitted for one frozen candidate roster.
#[derive(Debug, Clone, PartialEq)]
pub struct PointerReceiverReceiptBatch {
    receipts: Vec<PointerReceiverReceipt>,
}

impl PointerReceiverReceiptBatch {
    /// Builds a canonical batch, rejecting duplicate candidate answers.
    ///
    /// Submission `Vec` order has no semantic meaning.
    pub fn new(
        receipts: impl IntoIterator<Item = PointerReceiverReceipt>,
    ) -> Result<Self, PointerReceiverReceiptBatchError> {
        let mut receipts = receipts.into_iter().collect::<Vec<_>>();
        receipts.sort_unstable_by_key(PointerReceiverReceipt::candidate);
        if let Some(pair) = receipts
            .windows(2)
            .find(|pair| pair[0].candidate == pair[1].candidate)
        {
            return Err(PointerReceiverReceiptBatchError::DuplicateCandidate {
                candidate: pair[0].candidate,
            });
        }
        Ok(Self { receipts })
    }

    /// Returns receipts in canonical candidate identity order.
    #[must_use]
    pub fn receipts(&self) -> &[PointerReceiverReceipt] {
        &self.receipts
    }

    /// Returns whether the submitted exact set is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.receipts.is_empty()
    }
}

/// Structural failure while building one receipt batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum PointerReceiverReceiptBatchError {
    /// The adapter answered one candidate more than once.
    #[error("pointer receiver receipt batch contains duplicate candidate {candidate:?}")]
    DuplicateCandidate {
        /// Duplicated core-issued candidate identity.
        candidate: PointerReceiverCandidateId,
    },
}

/// Exact-set or authority failure while validating receiver receipts.
#[derive(Debug, Clone, PartialEq, Error)]
pub enum PointerReceiverReceiptValidationError {
    /// A receipt belongs to another pointer provider incarnation.
    #[error("pointer receiver receipt uses lease {submitted:?}, expected {expected:?}")]
    ForeignPointerLease {
        /// Frozen candidate-roster lease.
        expected: PointerInputLease,
        /// Lease embedded in the submitted candidate.
        submitted: PointerInputLease,
    },
    /// A receipt belongs to another frame attempt, including a failed retry.
    #[error("pointer receiver receipt uses frame attempt {submitted:?}, expected {expected:?}")]
    ForeignFrameAttempt {
        /// Frozen candidate-roster attempt.
        expected: PointerReceiverFrameAttemptId,
        /// Attempt embedded in the submitted candidate.
        submitted: PointerReceiverFrameAttemptId,
    },
    /// The exact-set batch omitted one candidate.
    #[error("pointer receiver receipt batch is missing candidate {candidate:?}")]
    MissingCandidate {
        /// First missing candidate in canonical order.
        candidate: PointerReceiverCandidateId,
    },
    /// The exact-set batch contains an unrequested candidate.
    #[error("pointer receiver receipt batch contains unexpected candidate {candidate:?}")]
    UnexpectedCandidate {
        /// First unexpected candidate in canonical order.
        candidate: PointerReceiverCandidateId,
    },
    /// A receiver-applicable edge was incorrectly marked not applicable.
    #[error("required pointer receiver candidate {candidate:?} was marked not applicable")]
    NotApplicableForRequiredCandidate {
        /// Candidate requiring one or more probes.
        candidate: PointerReceiverCandidateId,
    },
    /// An edge with no receiver role was reported as unknown.
    #[error("not-applicable pointer receiver candidate {candidate:?} was marked unknown")]
    UnknownForNotApplicableCandidate {
        /// Candidate with no probe request.
        candidate: PointerReceiverCandidateId,
    },
    /// An edge with no receiver role claimed presented evidence.
    #[error("not-applicable pointer receiver candidate {candidate:?} claimed presentation")]
    PresentedForNotApplicableCandidate {
        /// Candidate with no probe request.
        candidate: PointerReceiverCandidateId,
    },
    /// A presented observation answered one requested probe more than once.
    #[error("pointer receiver candidate {candidate:?} contains duplicate probe {probe:?}")]
    DuplicateProbe {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// Repeated independent probe.
        probe: PointerReceiverProbe,
    },
    /// A presented observation omitted one required probe.
    #[error("pointer receiver candidate {candidate:?} is missing probe {probe:?}")]
    MissingProbe {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// First missing probe in canonical order.
        probe: PointerReceiverProbe,
    },
    /// A presented observation answered a probe the candidate did not request.
    #[error("pointer receiver candidate {candidate:?} contains unexpected probe {probe:?}")]
    UnexpectedProbe {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// Unexpected independent probe.
        probe: PointerReceiverProbe,
    },
    /// A known probe names output or concrete provenance outside the current roster.
    #[error(
        "pointer receiver candidate {candidate:?} names non-current presentation authority for {probe:?}"
    )]
    PresentationAuthorityNotCurrent {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// Known probe carrying the stale or foreign authority.
        probe: PointerReceiverProbe,
        /// Submitted exact semantic output.
        submitted_output: SurfacePresentationOutputTicket,
        /// Submitted exact concrete presentation provenance.
        submitted_authority: PresentedSurfaceAuthority,
        /// Current interactive output for that surface, when one exists.
        current_output: Option<SurfacePresentationOutputTicket>,
        /// Current exact concrete presentation provenance, when one exists.
        current_authority: Option<PresentedSurfaceAuthority>,
    },
    /// A known hover hit was submitted although the edge had no known local point.
    #[error("pointer receiver candidate {candidate:?} has no known local point for a hover hit")]
    HoverHitPointUnavailable {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
    },
    /// A known hover hit did not echo the exact event-time point.
    #[error("pointer receiver candidate {candidate:?} used a different hover point")]
    HoverHitPointMismatch {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// Exact point frozen from the journal edge.
        expected: LogicalPoint,
        /// Point supplied with the known hover result.
        submitted: LogicalPoint,
    },
    /// A known probe named a surface other than its exact event-time route.
    #[error(
        "pointer receiver candidate {candidate:?} answered {probe:?} for surface {submitted}, expected {expected}"
    )]
    ProbeSurfaceMismatch {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// Independent probe carrying the wrong surface.
        probe: PointerReceiverProbe,
        /// Event-time surface frozen for this probe.
        expected: SurfaceId,
        /// Surface carried by the submitted output authority.
        submitted: SurfaceId,
    },
    /// A smooth-scroll continuation claimed a receiver other than its frozen owner.
    #[error("pointer receiver candidate {candidate:?} changed its locked scroll receiver")]
    LockedScrollReceiverMismatch {
        /// Candidate being answered.
        candidate: PointerReceiverCandidateId,
        /// Core-owned receiver frozen for the active sequence.
        expected: PresentationHitRegionId,
        /// Different receiver submitted by the adapter.
        submitted: PresentationHitRegionId,
    },
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    dead_code,
    reason = "reserved for the journal-driven interaction reducer"
)]
pub(crate) struct ValidatedPointerReceiverReceiptBatch {
    receipts: Vec<ValidatedPointerReceiverReceipt>,
}

#[allow(
    dead_code,
    reason = "reserved for the journal-driven interaction reducer"
)]
impl ValidatedPointerReceiverReceiptBatch {
    pub(crate) fn receipts(&self) -> &[ValidatedPointerReceiverReceipt] {
        &self.receipts
    }
}

#[derive(Debug, Clone, PartialEq)]
#[allow(
    dead_code,
    reason = "reserved for the journal-driven interaction reducer"
)]
pub(crate) struct ValidatedPointerReceiverReceipt {
    candidate: PointerReceiverCandidateId,
    observation: PointerReceiverObservation,
}

#[allow(
    dead_code,
    reason = "reserved for the journal-driven interaction reducer"
)]
impl ValidatedPointerReceiverReceipt {
    pub(crate) const fn candidate(&self) -> PointerReceiverCandidateId {
        self.candidate
    }

    pub(crate) const fn observation(&self) -> &PointerReceiverObservation {
        &self.observation
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::WorkspaceEpoch;
    use crate::pointer_journal::PointerProviderScope;
    use crate::policy::PolicyRevision;
    use crate::presentation_config::PresentationConfigRevision;
    use crate::presentation_observation::{PresentationOutputSerial, PresentedSurfaceAuthority};
    use crate::scene::SurfaceSceneStamp;
    use crate::scene_manifest::{
        SurfaceMeasurementTicket, SurfaceRequirementRevision, SurfaceSceneRevision,
    };
    use crate::viewport::CoordinateGeneration;

    fn domain(value: u64) -> EngineAuthorityDomainId {
        EngineAuthorityDomainId::new_for_test(value)
    }

    fn lease(authority_domain: EngineAuthorityDomainId, incarnation: u64) -> PointerInputLease {
        PointerInputLease::new(
            authority_domain,
            incarnation,
            PointerProviderScope::DesktopGlobal,
        )
    }

    fn output(
        authority_domain: EngineAuthorityDomainId,
        surface: SurfaceId,
        serial: u64,
    ) -> SurfacePresentationOutputTicket {
        let requirement = SurfaceMeasurementTicket::new(
            authority_domain,
            WorkspaceEpoch::new(1),
            PresentationConfigRevision::new(1),
            PolicyRevision::new(1),
            SurfaceRequirementRevision::new(1),
            surface,
        );
        SurfacePresentationOutputTicket::mint(
            authority_domain,
            PresentationOutputSerial::new_for_test(serial),
            SurfaceSceneStamp::new(requirement, SurfaceSceneRevision::new(1)),
        )
    }

    fn authority(
        ticket: SurfacePresentationOutputTicket,
        emission: u64,
    ) -> PresentedSurfaceAuthority {
        PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
            ticket,
            CoordinateGeneration::new(1),
            emission,
        )
    }

    fn presented_output(
        ticket: SurfacePresentationOutputTicket,
        emission: u64,
    ) -> PointerReceiverPresentedOutput {
        PointerReceiverPresentedOutput {
            ticket,
            authority: authority(ticket, emission),
        }
    }

    #[test]
    fn candidate_roster_freezes_only_referenced_presented_outputs() {
        let authority_domain = domain(7);
        let provider = lease(authority_domain, 1);
        let issuer = PointerReceiverAttemptIssuer::new(authority_domain);
        let outputs = (1..=1_024)
            .map(|identity| {
                let surface = SurfaceId::new(identity);
                let ticket = output(authority_domain, surface, identity);
                (surface, presented_output(ticket, 1))
            })
            .collect::<BTreeMap<_, _>>();
        let delivery_surface = SurfaceId::new(17);
        let hover_surface = SurfaceId::new(997);
        let missing_surface = SurfaceId::new(2_048);
        let missing_ticket = output(authority_domain, missing_surface, 2_048);
        let missing_authority = authority(missing_ticket, 1);

        let roster = PointerReceiverCandidateRoster::freeze_referenced_outputs(
            issuer.issue(provider).expect("attempt"),
            vec![
                PointerReceiverCandidateSpec::delivery(
                    PointerEdgeSequence::new(1),
                    Some(delivery_surface),
                    None,
                    PointerReceiverDeliveryRequest::Click,
                ),
                PointerReceiverCandidateSpec::hover_hit(
                    PointerEdgeSequence::new(2),
                    Some(hover_surface),
                    None,
                ),
                PointerReceiverCandidateSpec::delivery(
                    PointerEdgeSequence::new(3),
                    Some(missing_surface),
                    None,
                    PointerReceiverDeliveryRequest::Click,
                ),
                PointerReceiverCandidateSpec::not_applicable(PointerEdgeSequence::new(4)),
            ],
            &outputs,
        )
        .expect("roster");

        assert_eq!(
            roster.presented_outputs.keys().copied().collect::<Vec<_>>(),
            vec![delivery_surface, hover_surface],
        );

        let known_missing =
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                PointerReceiverDelivery {
                    output: Some(missing_ticket),
                    authority: Some(missing_authority),
                    click: PointerReceiverDeliveryDisposition::Blocked,
                    drag: PointerReceiverDeliveryDisposition::Blocked,
                    scroll: PointerReceiverDeliveryDisposition::Blocked,
                },
            )])
            .expect("missing output claim is structurally valid");
        let candidates = roster.candidates();
        let receipts = PointerReceiverReceiptBatch::new([
            candidates[0].receipt(PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::NotReported,
            )),
            candidates[1].receipt(PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::NotReported,
            )),
            candidates[2].receipt(PointerReceiverObservation::Presented(known_missing)),
            candidates[3].receipt(PointerReceiverObservation::NotApplicable),
        ])
        .expect("receipt batch remains exact");

        assert!(matches!(
            roster.validate(receipts),
            Err(
                PointerReceiverReceiptValidationError::PresentationAuthorityNotCurrent {
                    candidate,
                    submitted_output,
                    current_output: None,
                    current_authority: None,
                    ..
                }
            ) if candidate == candidates[2].id() && submitted_output == missing_ticket
        ));
    }

    #[test]
    fn known_delivery_must_match_its_event_time_surface() {
        let authority_domain = domain(7);
        let provider = lease(authority_domain, 1);
        let first_output = output(authority_domain, SurfaceId::new(1), 1);
        let second_output = output(authority_domain, SurfaceId::new(2), 2);
        let second_authority = authority(second_output, 1);
        let issuer = PointerReceiverAttemptIssuer::new(authority_domain);
        let point = LogicalPoint::new(10.0, 20.0).expect("finite test point");
        let roster = PointerReceiverCandidateRoster::freeze(
            issuer.issue(provider).expect("attempt"),
            vec![PointerReceiverCandidateSpec::delivery(
                PointerEdgeSequence::new(1),
                Some(SurfaceId::new(1)),
                Some(point),
                PointerReceiverDeliveryRequest::Click,
            )],
            vec![
                presented_output(first_output, 1),
                presented_output(second_output, 1),
            ],
        )
        .expect("roster");
        let candidate = &roster.candidates()[0];
        let observation =
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                PointerReceiverDelivery {
                    output: Some(second_output),
                    authority: Some(second_authority),
                    click: PointerReceiverDeliveryDisposition::Blocked,
                    drag: PointerReceiverDeliveryDisposition::Blocked,
                    scroll: PointerReceiverDeliveryDisposition::Blocked,
                },
            )])
            .expect("one delivery answer");

        assert!(matches!(
            roster.validate(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::Presented(observation)),
                ])
                .expect("batch")
            ),
            Err(PointerReceiverReceiptValidationError::ProbeSurfaceMismatch {
                candidate: submitted,
                probe: PointerReceiverProbe::Delivery,
                expected,
                submitted: actual,
            }) if submitted == candidate.id()
                && expected == SurfaceId::new(1)
                && actual == SurfaceId::new(2)
        ));
    }

    #[test]
    fn unknown_local_hover_point_accepts_only_a_per_probe_unknown() {
        let authority_domain = domain(7);
        let provider = lease(authority_domain, 1);
        let ticket = output(authority_domain, SurfaceId::new(1), 1);
        let issuer = PointerReceiverAttemptIssuer::new(authority_domain);
        let roster = PointerReceiverCandidateRoster::freeze(
            issuer.issue(provider).expect("attempt"),
            vec![PointerReceiverCandidateSpec::hover_hit(
                PointerEdgeSequence::new(1),
                Some(SurfaceId::new(1)),
                None,
            )],
            vec![presented_output(ticket, 1)],
        )
        .expect("roster");
        let candidate = &roster.candidates()[0];
        let unknown =
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                PointerReceiverHoverHit::unknown(PointerReceiverUnknownReason::NotReported),
            )])
            .expect("per-probe unknown is one exact hover answer");
        roster
            .validate(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::Presented(unknown))
                ])
                .expect("batch"),
            )
            .expect("unknown point accepts a per-probe unknown result");

        let known =
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::HoverHit(
                PointerReceiverHoverHit {
                    output: Some(ticket),
                    authority: Some(authority(ticket, 1)),
                    point: Some(LogicalPoint::new(10.0, 20.0).expect("finite test point")),
                    disposition: PointerReceiverHoverHitDisposition::NoReceiver,
                },
            )])
            .expect("known hover is structurally valid");
        assert!(matches!(
            roster.validate(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::Presented(known)),
                ])
                .expect("batch")
            ),
            Err(PointerReceiverReceiptValidationError::HoverHitPointUnavailable {
                candidate: submitted
            }) if submitted == candidate.id()
        ));
    }

    #[test]
    fn known_blocked_or_no_receiver_delivery_must_bind_the_current_output() {
        let authority_domain = domain(7);
        let provider = lease(authority_domain, 1);
        let ticket = output(authority_domain, SurfaceId::new(1), 1);
        let current = authority(ticket, 2);
        let stale = authority(ticket, 1);
        let issuer = PointerReceiverAttemptIssuer::new(authority_domain);
        let route_point = LogicalPoint::new(12.0, 34.0).expect("finite route point");
        let roster = PointerReceiverCandidateRoster::freeze(
            issuer.issue(provider).expect("attempt"),
            vec![PointerReceiverCandidateSpec::delivery(
                PointerEdgeSequence::new(1),
                Some(SurfaceId::new(1)),
                Some(route_point),
                PointerReceiverDeliveryRequest::Click,
            )],
            vec![PointerReceiverPresentedOutput {
                ticket,
                authority: current,
            }],
        )
        .expect("roster");
        let candidate = &roster.candidates()[0];
        assert_eq!(candidate.route_point(), Some(route_point));
        let stale_blocked =
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                PointerReceiverDelivery {
                    output: Some(ticket),
                    authority: Some(stale),
                    click: PointerReceiverDeliveryDisposition::Blocked,
                    drag: PointerReceiverDeliveryDisposition::Blocked,
                    scroll: PointerReceiverDeliveryDisposition::Blocked,
                },
            )])
            .expect("known blocker is structurally valid");
        assert!(matches!(
            roster.validate(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::Presented(stale_blocked)),
                ])
                .expect("batch")
            ),
            Err(PointerReceiverReceiptValidationError::PresentationAuthorityNotCurrent {
                probe: PointerReceiverProbe::Delivery,
                submitted_authority,
                ..
            }) if submitted_authority == stale
        ));

        let current_no_receiver =
            PresentedPointerReceiverObservation::new([PointerReceiverProbeReceipt::Delivery(
                PointerReceiverDelivery {
                    output: Some(ticket),
                    authority: Some(current),
                    click: PointerReceiverDeliveryDisposition::NoReceiver,
                    drag: PointerReceiverDeliveryDisposition::NoReceiver,
                    scroll: PointerReceiverDeliveryDisposition::NoReceiver,
                },
            )])
            .expect("known no-receiver is structurally valid");
        roster
            .validate(
                PointerReceiverReceiptBatch::new([
                    candidate.receipt(PointerReceiverObservation::Presented(current_no_receiver))
                ])
                .expect("batch"),
            )
            .expect("current no-receiver authority is accepted");
    }

    #[test]
    fn candidate_receipt_set_is_exact_and_not_applicable_stays_distinct() {
        let authority_domain = domain(7);
        let provider = lease(authority_domain, 1);
        let issuer = PointerReceiverAttemptIssuer::new(authority_domain);
        let roster = PointerReceiverCandidateRoster::freeze(
            issuer.issue(provider).expect("attempt"),
            vec![
                PointerReceiverCandidateSpec::not_applicable(PointerEdgeSequence::new(1)),
                PointerReceiverCandidateSpec::delivery(
                    PointerEdgeSequence::new(2),
                    Some(SurfaceId::new(1)),
                    None,
                    PointerReceiverDeliveryRequest::Click,
                ),
                PointerReceiverCandidateSpec::hover_hit(PointerEdgeSequence::new(3), None, None),
            ],
            Vec::new(),
        )
        .expect("roster");
        let first = roster.candidates()[0].receipt(PointerReceiverObservation::NotApplicable);
        assert_eq!(
            PointerReceiverReceiptBatch::new([first.clone(), first]),
            Err(PointerReceiverReceiptBatchError::DuplicateCandidate {
                candidate: roster.candidates()[0].id(),
            })
        );

        let incomplete = PointerReceiverReceiptBatch::new([
            roster.candidates()[0].receipt(PointerReceiverObservation::NotApplicable),
            roster.candidates()[1].receipt(PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::NotReported,
            )),
        ])
        .expect("partial exact set is structurally valid");
        assert_eq!(
            roster.validate(incomplete),
            Err(PointerReceiverReceiptValidationError::MissingCandidate {
                candidate: roster.candidates()[2].id(),
            })
        );

        let invalid_not_applicable = PointerReceiverReceiptBatch::new([
            roster.candidates()[0].receipt(PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::NotReported,
            )),
            roster.candidates()[1].receipt(PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::NotReported,
            )),
            roster.candidates()[2].receipt(PointerReceiverObservation::Unknown(
                PointerReceiverUnknownReason::NotReported,
            )),
        ])
        .expect("exact candidate set");
        assert_eq!(
            roster.validate(invalid_not_applicable),
            Err(
                PointerReceiverReceiptValidationError::UnknownForNotApplicableCandidate {
                    candidate: roster.candidates()[0].id(),
                }
            )
        );
    }
}
