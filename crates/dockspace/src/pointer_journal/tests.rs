use super::*;
use crate::backend_ingress::BackendIngressRecorder;
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::intent::AuthorityUnavailableReason;
use crate::platform::{
    CapabilityRosterObservation, InputEffectAcknowledgement, PlatformCapabilities,
    PlatformCapability, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCoordinateObservation, WindowInputObservation, WindowInputState,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use crate::platform_provider::PlatformObservationAuthority;
use crate::presentation_observation::PresentationLedger;
use crate::viewport::{
    CapabilityObservationGeneration, CoordinateObservationGeneration, InputObservationGeneration,
    InventoryObservationGeneration, PresentationObservationGeneration, ViewportBinding,
    ViewportRole, WindowIncarnation, WindowToken, WorkAreaObservationGeneration,
};
use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};
use crate::viewport_registry::ViewportRegistry;

#[derive(Debug, Clone, Copy)]
enum PointerWindow {
    Dock(ViewportBinding),
    Foreign,
    None,
}

fn domain(value: u64) -> EngineAuthorityDomainId {
    EngineAuthorityDomainId::new_for_test(value)
}

fn binding(domain: EngineAuthorityDomainId, surface: u64, token: u64) -> ViewportBinding {
    binding_with_incarnation(domain, surface, token, 1)
}

fn binding_with_incarnation(
    domain: EngineAuthorityDomainId,
    surface: u64,
    token: u64,
    incarnation: u64,
) -> ViewportBinding {
    ViewportBinding::new(
        domain,
        WorkspaceEpoch::new(1),
        SurfaceId::new(surface),
        WindowToken::new(token),
        WindowIncarnation::new(incarnation),
    )
}

fn host(domain: EngineAuthorityDomainId) -> PresentationHostLease {
    PresentationLedger::new(domain)
        .create_host()
        .expect("presentation host")
}

fn desktop_lease(domain: EngineAuthorityDomainId, incarnation: u64) -> PointerInputLease {
    PointerInputLease::new(domain, incarnation, PointerProviderScope::DesktopGlobal)
}

fn surface_local_provider(
    ledger: &mut PointerJournalLedger,
    committed_through: u64,
) -> SurfaceLocalPointerProvider {
    let scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
        host(ledger.authority_domain),
        SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(9)),
    ));
    let committed_through = PointerEdgeSequence::new(committed_through);
    let lease = ledger
        .create_provider(scope, committed_through)
        .expect("surface-local provider");
    SurfaceLocalPointerProvider::new(lease, committed_through)
}

fn route_capabilities() -> PlatformCapabilities {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);
    capabilities
}

fn routeable_registry(
    domain: EngineAuthorityDomainId,
    surface: SurfaceId,
    token: WindowToken,
    origin_x: f64,
    scale: f64,
) -> (ViewportRegistry, ViewportBinding, CoordinateGeneration) {
    let mut registry = ViewportRegistry::new(domain);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(1), surface, token, ViewportRole::Child)
        .expect("test native binding registers");
    let window = crate::platform::ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(1),
            Authority::Known(
                PhysicalRect::new(origin_x, 0.0, 600.0, 400.0)
                    .expect("test physical bounds are valid"),
            ),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor is valid")),
            Authority::Known(ScaleFactor::new(scale).expect("test scale factor is valid")),
        ))
        .with_input_observation(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(1),
            Authority::Known(WindowInputState::ReceivesInput),
            InputEffectAcknowledgement::known(None),
        ))
        .with_presentation_observation(WindowPresentationObservation::new(
            binding,
            PresentationObservationGeneration::new(1),
            Authority::Known(WindowPresentationState::Visible),
            PresentationEffectAcknowledgement::known(None),
        ))
        .with_close_requested(Authority::Known(false));
    let snapshot = PlatformSnapshot::new(
        crate::viewport::PlatformSnapshotGeneration::new(1),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(1),
            Authority::Known(route_capabilities()),
        ),
        unknown_focus_observation(
            FocusObservationGeneration::new(1),
            AuthorityUnavailableReason::NotReported,
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(1),
            Authority::Known(vec![binding]),
        )
        .expect("test inventory is canonical"),
        vec![window],
        Vec::new(),
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(1),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .expect("test work-area tombstone is canonical"),
    )
    .expect("test platform snapshot is canonical");
    registry
        .apply_snapshot_for_test(&snapshot)
        .expect("test registry receives authoritative facts");
    let generation = registry
        .record(surface)
        .expect("registered surface remains present")
        .coordinate_generation();
    (registry, binding, generation)
}

fn desktop_location(hovered: Authority<PointerWindow>) -> PointerEdgeLocation {
    let position = PhysicalPoint::new(10.0, 20.0).expect("finite desktop point");
    let route = match hovered {
        Authority::Known(PointerWindow::Dock(binding)) => {
            DesktopRouteFact::dock(DesktopDockRoute::new(
                binding,
                CoordinateGeneration::new(0),
                position,
                LogicalPoint::new(10.0, 20.0).expect("finite logical point"),
            ))
        }
        Authority::Known(PointerWindow::Foreign) => {
            DesktopRouteFact::foreign(Authority::Known(position))
        }
        Authority::Known(PointerWindow::None) => DesktopRouteFact::no_window(
            Authority::Known(position),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        ),
        Authority::Unknown(reason) => DesktopRouteFact::unknown(Authority::Known(position), reason),
    };
    PointerEdgeLocation::Desktop { route }
}

fn local_location() -> PointerEdgeLocation {
    PointerEdgeLocation::SurfaceLocal {
        position: Authority::Known(LogicalPoint::new(10.0, 20.0).expect("finite logical point")),
    }
}

fn edge(
    sequence: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdge {
    edge_for_pointer(sequence, 7, kind, location, capture)
}

fn edge_for_pointer(
    sequence: u64,
    pointer: u64,
    kind: PointerEdgeKind,
    location: PointerEdgeLocation,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdge {
    PointerEdge::new(
        PointerEdgeSequence::new(sequence),
        PointerId::new(pointer),
        kind,
        location,
        capture,
    )
}

fn moved_journal(previous: u64, through: u64) -> PointerEdgeJournal {
    let edges = (previous..through)
        .map(|sequence| {
            edge(
                sequence + 1,
                PointerEdgeKind::Moved,
                desktop_location(Authority::Known(PointerWindow::None)),
                Authority::Known(PointerCaptureOwner::None),
            )
        })
        .collect();
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        PointerEdgeSequence::new(through),
        edges,
    )
    .expect("complete moved journal")
}

fn local_moved_journal(previous: u64, through: u64) -> PointerEdgeJournal {
    let edges = (previous..through)
        .map(|sequence| {
            edge(
                sequence + 1,
                PointerEdgeKind::Moved,
                local_location(),
                Authority::Known(PointerCaptureOwner::None),
            )
        })
        .collect();
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(previous),
        PointerEdgeSequence::new(through),
        edges,
    )
    .expect("complete local moved journal")
}

fn commit_candidate(
    ledger: &mut PointerJournalLedger,
    lease: PointerInputLease,
    journal: PointerEdgeJournal,
) -> Result<PointerJournalCommit, PointerJournalLedgerError> {
    let prepared = ledger.prepare_candidate(lease, journal)?;
    ledger.commit_prepared(prepared)
}

#[test]
fn release_then_press_across_windows_preserves_provider_order() {
    let domain = domain(1);
    let window_a = binding(domain, 10, 100);
    let window_b = binding(domain, 20, 200);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(40),
        PointerEdgeSequence::new(42),
        vec![
            edge(
                41,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                desktop_location(Authority::Known(PointerWindow::Dock(window_a))),
                Authority::Known(PointerCaptureOwner::None),
            ),
            edge(
                42,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                desktop_location(Authority::Known(PointerWindow::Dock(window_b))),
                Authority::Known(PointerCaptureOwner::Native(window_b)),
            ),
        ],
    )
    .expect("complete journal");

    assert_eq!(journal.len(), 2);
    assert_eq!(
        journal.edges()[0].location(),
        desktop_location(Authority::Known(PointerWindow::Dock(window_a)))
    );
    assert_eq!(
        journal.edges()[1].location(),
        desktop_location(Authority::Known(PointerWindow::Dock(window_b)))
    );
    assert_eq!(
        journal
            .edges()
            .iter()
            .map(PointerEdge::kind)
            .collect::<Vec<_>>(),
        vec![
            PointerEdgeKind::ButtonReleased(PointerButton::Primary),
            PointerEdgeKind::ButtonPressed(PointerButton::Primary),
        ]
    );
}

#[test]
fn equal_watermarks_encode_an_authoritative_empty_interval() {
    let watermark = PointerEdgeSequence::new(9);
    let journal = PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("equal watermarks permit an empty journal");

    assert!(journal.is_empty());
    assert_eq!(journal.previous(), watermark);
    assert_eq!(journal.through(), watermark);
}

#[test]
fn duplicate_and_descending_sequences_are_rejected() {
    let unknown_window = Authority::Unknown(AuthorityUnavailableReason::NotReported);
    let unknown_location = desktop_location(unknown_window);
    let unknown_capture = Authority::Unknown(AuthorityUnavailableReason::NotReported);

    let duplicate = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(2),
        vec![
            edge(1, PointerEdgeKind::Moved, unknown_location, unknown_capture),
            edge(1, PointerEdgeKind::Moved, unknown_location, unknown_capture),
        ],
    );
    assert_eq!(
        duplicate,
        Err(PointerJournalError::SequenceNotIncreasing {
            previous: PointerEdgeSequence::new(1),
            actual: PointerEdgeSequence::new(1),
        })
    );

    let descending = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(2),
        vec![
            edge(1, PointerEdgeKind::Moved, unknown_location, unknown_capture),
            edge(0, PointerEdgeKind::Moved, unknown_location, unknown_capture),
        ],
    );
    assert_eq!(
        descending,
        Err(PointerJournalError::SequenceNotIncreasing {
            previous: PointerEdgeSequence::new(1),
            actual: PointerEdgeSequence::new(0),
        })
    );
}

#[test]
fn a_gap_inside_the_declared_interval_is_rejected() {
    let result = PointerEdgeJournal::new(
        PointerEdgeSequence::new(5),
        PointerEdgeSequence::new(7),
        vec![edge(
            7,
            PointerEdgeKind::Moved,
            desktop_location(Authority::Known(PointerWindow::None)),
            Authority::Known(PointerCaptureOwner::None),
        )],
    );

    assert_eq!(
        result,
        Err(PointerJournalError::SequenceGap {
            expected: PointerEdgeSequence::new(6),
            actual: PointerEdgeSequence::new(7),
        })
    );
}

#[test]
fn unavailable_capture_authority_is_not_no_capture() {
    let unknown = Authority::Unknown(AuthorityUnavailableReason::ProviderUnavailable);
    let no_capture = Authority::Known(PointerCaptureOwner::None);

    assert_ne!(unknown, no_capture);
}

#[test]
fn capture_change_preserves_unknown_capture_authority() {
    let unknown = Authority::Unknown(AuthorityUnavailableReason::SurfaceUnavailable);
    let edge = PointerEdge::new(
        PointerEdgeSequence::new(1),
        PointerId::new(1),
        PointerEdgeKind::CaptureChanged,
        PointerEdgeLocation::Desktop {
            route: DesktopRouteFact::foreign(Authority::Unknown(
                AuthorityUnavailableReason::NotReported,
            )),
        },
        unknown,
    );

    assert_eq!(edge.kind(), PointerEdgeKind::CaptureChanged);
    assert_eq!(edge.capture_owner(), unknown);
    assert_ne!(
        edge.capture_owner(),
        Authority::Known(PointerCaptureOwner::None)
    );
}

#[test]
fn tickets_include_the_exact_provider_incarnation() {
    let sequence = PointerEdgeSequence::new(11);
    let first_lease = desktop_lease(domain(1), 1);
    let next_lease = desktop_lease(domain(1), 2);
    let first = PointerEdgeTicket::new(first_lease, sequence);
    let next = PointerEdgeTicket::new(next_lease, sequence);

    assert_ne!(first, next);
    assert_eq!(first.lease(), first_lease);
    assert_eq!(first.lease().incarnation(), 1);
    assert_eq!(first.sequence(), sequence);
}

#[test]
fn tickets_from_foreign_engine_domains_are_distinct() {
    let sequence = PointerEdgeSequence::new(11);
    let local = PointerEdgeTicket::new(desktop_lease(domain(1), 1), sequence);
    let foreign = PointerEdgeTicket::new(desktop_lease(domain(2), 1), sequence);

    assert_ne!(local, foreign);
    assert_eq!(local.authority_domain(), domain(1));
}

#[test]
fn ledger_allows_only_one_live_pointer_provider() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let first = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(10),
        )
        .expect("first provider");

    assert_eq!(
        ledger.create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(10),
        ),
        Err(PointerJournalLedgerError::ProviderAlreadyActive { active: first })
    );

    ledger.retire_provider(first).expect("retire first");
    let second = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(10),
        )
        .expect("successor provider");
    assert_eq!(first.incarnation(), 1);
    assert_eq!(second.incarnation(), 2);
}

#[test]
fn ledger_advances_one_exact_watermark_across_host_frames() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");

    let first =
        commit_candidate(&mut ledger, lease, moved_journal(0, 2)).expect("first frame journal");
    assert_eq!(first.lease(), lease);
    assert_eq!(first.journal().previous(), PointerEdgeSequence::new(0));
    assert_eq!(first.journal().through(), PointerEdgeSequence::new(2));
    assert_eq!(
        first
            .accepted_edges()
            .iter()
            .map(|accepted| accepted.ticket().sequence())
            .collect::<Vec<_>>(),
        vec![PointerEdgeSequence::new(1), PointerEdgeSequence::new(2)]
    );
    assert!(
        first
            .accepted_edges()
            .iter()
            .all(|accepted| accepted.ticket().lease() == lease)
    );

    let second = commit_candidate(&mut ledger, lease, moved_journal(2, 3))
        .expect("next frame journal begins at committed watermark");
    assert_eq!(second.accepted_edges().len(), 1);
    assert_eq!(
        second.accepted_edges()[0].ticket(),
        PointerEdgeTicket::new(lease, PointerEdgeSequence::new(3))
    );
}

#[test]
fn preparation_is_pure_and_a_rejected_receipt_can_retry_the_same_interval() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let before = ledger.active;
    let before_version = ledger.version;

    let rejected_by_receipts = ledger
        .prepare_candidate(lease, moved_journal(0, 1))
        .expect("candidate is valid before receipt validation");
    assert_eq!(rejected_by_receipts.lease(), lease);
    assert_eq!(
        rejected_by_receipts.journal().through(),
        PointerEdgeSequence::new(1)
    );
    assert_eq!(ledger.active, before);
    assert_eq!(ledger.version, before_version);

    drop(rejected_by_receipts);
    let retry = ledger
        .prepare_candidate(lease, moved_journal(0, 1))
        .expect("receipt rejection did not consume the interval");
    let commit = ledger
        .commit_prepared(retry)
        .expect("retry commits after the receipt join succeeds");

    assert_eq!(commit.accepted_edges().len(), 1);
    assert_eq!(
        commit.accepted_edges()[0].ticket(),
        PointerEdgeTicket::new(lease, PointerEdgeSequence::new(1))
    );
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease,
            committed_through: PointerEdgeSequence::new(1),
        })
    );
    assert_ne!(ledger.version, before_version);
}

#[test]
fn an_empty_prepared_interval_cannot_be_replayed_after_another_commit() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(5),
        )
        .expect("provider");
    let first = ledger
        .prepare_candidate(lease, moved_journal(5, 5))
        .expect("first empty candidate");
    let replay = ledger
        .prepare_candidate(lease, moved_journal(5, 5))
        .expect("second empty candidate observes the same state");
    let prepared_version = first.ledger_version.0;

    ledger
        .commit_prepared(first)
        .expect("first empty interval commits");
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease,
            committed_through: PointerEdgeSequence::new(5),
        })
    );

    assert_eq!(
        ledger.commit_prepared(replay),
        Err(PointerJournalLedgerError::PreparedJournalStale {
            lease,
            prepared_version,
            active_version: ledger.version.0,
            prepared_committed_through: PointerEdgeSequence::new(5),
            active_committed_through: PointerEdgeSequence::new(5),
        })
    );
}

#[test]
fn retiring_a_provider_invalidates_every_prepared_interval() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let prepared = ledger
        .prepare_candidate(lease, moved_journal(0, 1))
        .expect("candidate before provider retirement");

    ledger.retire_provider(lease).expect("retire provider");
    assert_eq!(
        ledger.commit_prepared(prepared),
        Err(PointerJournalLedgerError::RetiredLease {
            lease,
            committed_through: PointerEdgeSequence::new(0),
        })
    );
    assert!(ledger.active.is_none());
}

#[test]
fn prepared_journal_cannot_cross_to_an_equivalent_but_distinct_ledger() {
    let authority_domain = domain(1);
    let mut source = PointerJournalLedger::new(authority_domain);
    let source_lease = source
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("source provider");
    let prepared = source
        .prepare_candidate(source_lease, moved_journal(0, 1))
        .expect("source candidate");

    let mut target = PointerJournalLedger::new(authority_domain);
    let target_lease = target
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("target provider");
    assert_eq!(source_lease, target_lease);

    assert_eq!(
        target.commit_prepared(prepared),
        Err(PointerJournalLedgerError::ForeignPreparedJournal)
    );
    assert_eq!(
        target.active,
        Some(ActivePointerProvider {
            lease: target_lease,
            committed_through: PointerEdgeSequence::new(0),
        })
    );
    assert_eq!(
        source.active,
        Some(ActivePointerProvider {
            lease: source_lease,
            committed_through: PointerEdgeSequence::new(0),
        })
    );
}

#[test]
fn prepare_revalidates_a_structurally_complete_journal_before_freezing_it() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let malformed = PointerEdgeJournal {
        previous: PointerEdgeSequence::new(0),
        through: PointerEdgeSequence::new(2),
        edges: vec![edge(
            2,
            PointerEdgeKind::Moved,
            desktop_location(Authority::Known(PointerWindow::None)),
            Authority::Known(PointerCaptureOwner::None),
        )],
        authority_checkpoint: None,
    };

    let error = ledger
        .prepare_candidate(lease, malformed)
        .expect_err("malformed journal cannot be frozen");
    assert_eq!(
        error,
        PointerJournalLedgerError::InvalidJournal(PointerJournalError::SequenceGap {
            expected: PointerEdgeSequence::new(1),
            actual: PointerEdgeSequence::new(2),
        })
    );
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease,
            committed_through: PointerEdgeSequence::new(0),
        })
    );
}

#[test]
fn surface_local_scope_is_frozen_across_committed_watermarks() {
    let authority_domain = domain(1);
    let presentation_host = host(authority_domain);
    let endpoint = SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(9));
    let local = SurfaceLocalPointerScope::new(presentation_host, endpoint);
    let scope = PointerProviderScope::SurfaceLocal(local);
    let mut ledger = PointerJournalLedger::new(authority_domain);

    assert_eq!(local.host(), presentation_host);
    assert_eq!(local.surface(), SurfaceId::new(9));
    assert_eq!(local.endpoint(), endpoint);
    assert_eq!(endpoint.surface(), SurfaceId::new(9));
    assert_eq!(endpoint.native_binding(), None);
    assert_eq!(scope.surface_local(), Some(local));

    let lease = ledger
        .create_provider(scope, PointerEdgeSequence::new(0))
        .expect("surface-local provider");
    let first = commit_candidate(&mut ledger, lease, local_moved_journal(0, 2))
        .expect("first local journal");
    let second = commit_candidate(&mut ledger, lease, local_moved_journal(2, 3))
        .expect("second local journal");

    assert_eq!(lease.scope(), scope);
    assert_eq!(first.lease().scope(), scope);
    assert!(
        first
            .accepted_edges()
            .iter()
            .all(|accepted| accepted.ticket().lease().scope() == scope)
    );
    assert_eq!(second.lease().scope(), scope);
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease,
            committed_through: PointerEdgeSequence::new(3),
        })
    );
}

#[test]
fn lost_surface_local_frame_attempt_rejects_before_core_publication() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let provider = surface_local_provider(&mut ledger, 7);
    let lease = provider.lease();
    let mut commit = provider
        .begin_frame_submission(PointerEdgeSequence::new(7), PointerEdgeSequence::new(8))
        .expect("the exact producer reserves one frame attempt");
    lock_surface_local_pointer_state(&provider.state).in_flight = None;
    let mut core_published = false;

    assert_eq!(
        commit.publish_with(|| core_published = true),
        Err(SurfaceLocalPointerProviderError::FrameSubmissionLost { lease, attempt: 1 })
    );
    assert!(!core_published);
    assert_eq!(provider.committed_through(), PointerEdgeSequence::new(7));
}

#[test]
fn active_surface_local_quiescence_compacts_without_a_detailed_tombstone() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let provider = surface_local_provider(&mut ledger, 7);
    let lease = provider.lease();
    let receipt = provider
        .drain()
        .expect("provider has no host-frame submission in flight");

    assert_eq!(
        ledger.retire_quiesced_surface_local(&receipt),
        Ok(SurfaceLocalPointerQuiescenceDisposition::RetiredActive)
    );
    assert_eq!(ledger.active_lease(), None);
    let retention = ledger.retention_manifest();
    assert_eq!(retention.retired_lease_guards(), 0);
    assert_eq!(retention.compacted_retirement_ranges(), 1);
    assert_eq!(retention.logical_compacted_leases(), 1);
    assert!(matches!(
        ledger.prepare_candidate(lease, local_moved_journal(7, 7)),
        Err(PointerJournalLedgerError::CompactedLease { lease: submitted })
            if submitted == lease
    ));
}

#[test]
fn surface_local_quiescence_compacts_a_previously_retired_lease_without_touching_successor() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let provider = surface_local_provider(&mut ledger, 3);
    let retired = provider.lease();
    ledger
        .retire_provider(retired)
        .expect("core retires the predecessor first");
    let receipt = provider
        .drain()
        .expect("retired provider has no host-frame submission in flight");
    let successor = surface_local_provider(&mut ledger, 11).lease();

    assert_eq!(
        ledger.retire_quiesced_surface_local(&receipt),
        Ok(SurfaceLocalPointerQuiescenceDisposition::CompactedPreviouslyRetired)
    );
    assert_eq!(ledger.active_lease(), Some(successor));
    let retention = ledger.retention_manifest();
    assert_eq!(retention.active_provider_count(), 1);
    assert_eq!(retention.retired_lease_guards(), 0);
    assert_eq!(retention.compacted_retirement_ranges(), 1);
}

#[test]
fn rejected_surface_local_quiescence_preserves_the_exact_active_watermark() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let provider = surface_local_provider(&mut ledger, 4);
    let lease = provider.lease();
    commit_candidate(&mut ledger, lease, local_moved_journal(4, 5))
        .expect("core advances beyond the producer's stale watermark");
    let receipt = provider
        .drain()
        .expect("stale producer watermark remains drainable without an in-flight frame");
    let before = ledger.clone();

    assert_eq!(
        ledger.retire_quiesced_surface_local(&receipt),
        Err(
            PointerJournalLedgerError::SurfaceLocalQuiescenceWatermarkMismatch {
                lease,
                committed_through: PointerEdgeSequence::new(5),
                submitted: PointerEdgeSequence::new(4),
            }
        )
    );
    assert_eq!(ledger, before);
    assert!(!receipt.is_consumed());
}

#[test]
fn surface_local_provider_rejects_foreign_host_and_binding_before_minting() {
    let local_domain = domain(1);
    let foreign_domain = domain(2);
    let mut ledger = PointerJournalLedger::new(local_domain);
    let foreign_host = host(foreign_domain);
    let foreign_host_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
        foreign_host,
        SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(7)),
    ));

    assert_eq!(
        ledger.create_provider(foreign_host_scope, PointerEdgeSequence::new(0)),
        Err(PointerJournalLedgerError::ForeignSurfaceLocalHost {
            host: foreign_host,
            expected: local_domain,
            submitted: foreign_domain,
        })
    );
    assert!(ledger.active.is_none());
    assert_eq!(ledger.last_incarnation, PointerProviderIncarnation(0));

    let local_host = host(local_domain);
    let foreign_binding = binding(foreign_domain, 7, 70);
    let foreign_binding_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
        local_host,
        SurfaceLocalPointerEndpoint::Native(foreign_binding),
    ));
    assert_eq!(
        ledger.create_provider(foreign_binding_scope, PointerEdgeSequence::new(0)),
        Err(PointerJournalLedgerError::ForeignSurfaceLocalBinding {
            binding: foreign_binding,
            expected: local_domain,
            submitted: foreign_domain,
        })
    );
    assert!(ledger.active.is_none());
    assert_eq!(ledger.last_incarnation, PointerProviderIncarnation(0));

    let local_binding = binding(local_domain, 7, 70);
    let endpoint = SurfaceLocalPointerEndpoint::Native(local_binding);
    let valid_scope =
        PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(local_host, endpoint));
    let lease = ledger
        .create_provider(valid_scope, PointerEdgeSequence::new(0))
        .expect("valid local provider after rejected candidates");
    assert_eq!(lease.incarnation(), 1);
    assert_eq!(endpoint.surface(), SurfaceId::new(7));
    assert_eq!(endpoint.native_binding(), Some(local_binding));
}

#[test]
fn location_scope_mismatch_is_atomic_in_both_directions() {
    let authority_domain = domain(1);
    let mut ledger = PointerJournalLedger::new(authority_domain);
    let desktop = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop provider");

    assert_eq!(
        commit_candidate(&mut ledger, desktop, local_moved_journal(0, 1)),
        Err(PointerJournalLedgerError::LocationScopeMismatch {
            lease: desktop,
            sequence: PointerEdgeSequence::new(1),
            submitted: PointerEdgeLocationLane::SurfaceLocal,
        })
    );
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease: desktop,
            committed_through: PointerEdgeSequence::new(0),
        })
    );
    commit_candidate(&mut ledger, desktop, moved_journal(0, 1))
        .expect("desktop watermark was not advanced by rejection");
    ledger
        .retire_provider(desktop)
        .expect("retire desktop provider");

    let local_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
        host(authority_domain),
        SurfaceLocalPointerEndpoint::Logical(SurfaceId::new(5)),
    ));
    let local = ledger
        .create_provider(local_scope, PointerEdgeSequence::new(10))
        .expect("local provider");
    assert_eq!(
        commit_candidate(&mut ledger, local, moved_journal(10, 11)),
        Err(PointerJournalLedgerError::LocationScopeMismatch {
            lease: local,
            sequence: PointerEdgeSequence::new(11),
            submitted: PointerEdgeLocationLane::Desktop,
        })
    );
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease: local,
            committed_through: PointerEdgeSequence::new(10),
        })
    );
    commit_candidate(&mut ledger, local, local_moved_journal(10, 11))
        .expect("local watermark was not advanced by rejection");
}

#[test]
fn capture_owner_is_fail_closed_against_the_provider_scope() {
    let authority_domain = domain(1);
    let mut ledger = PointerJournalLedger::new(authority_domain);
    let desktop = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop provider");
    let desktop_endpoint_capture = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![edge(
            1,
            PointerEdgeKind::CaptureChanged,
            desktop_location(Authority::Known(PointerWindow::None)),
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    )
    .expect("complete desktop journal");

    assert_eq!(
        commit_candidate(&mut ledger, desktop, desktop_endpoint_capture),
        Err(PointerJournalLedgerError::CaptureScopeMismatch {
            lease: desktop,
            sequence: PointerEdgeSequence::new(1),
            submitted: PointerCaptureOwner::ProviderEndpoint,
        })
    );
    assert_eq!(ledger.last_stream_incarnation, PointerStreamIncarnation(0));

    let foreign_binding = binding(domain(2), 7, 70);
    let foreign_capture = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![edge(
            1,
            PointerEdgeKind::CaptureChanged,
            desktop_location(Authority::Known(PointerWindow::Foreign)),
            Authority::Known(PointerCaptureOwner::Native(foreign_binding)),
        )],
    )
    .expect("complete foreign capture journal");
    assert_eq!(
        commit_candidate(&mut ledger, desktop, foreign_capture),
        Err(PointerJournalLedgerError::ForeignCaptureBinding {
            lease: desktop,
            sequence: PointerEdgeSequence::new(1),
            binding: foreign_binding,
            expected: authority_domain,
            submitted: domain(2),
        })
    );

    let local_binding = binding(authority_domain, 7, 70);
    let valid_desktop_capture = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![edge(
            1,
            PointerEdgeKind::CaptureChanged,
            desktop_location(Authority::Known(PointerWindow::Dock(local_binding))),
            Authority::Known(PointerCaptureOwner::Native(local_binding)),
        )],
    )
    .expect("complete exact native capture journal");
    commit_candidate(&mut ledger, desktop, valid_desktop_capture)
        .expect("desktop provider may report an exact local-domain native binding");
    ledger
        .retire_provider(desktop)
        .expect("retire desktop provider");

    let local_scope = PointerProviderScope::SurfaceLocal(SurfaceLocalPointerScope::new(
        host(authority_domain),
        SurfaceLocalPointerEndpoint::Native(local_binding),
    ));
    let local = ledger
        .create_provider(local_scope, PointerEdgeSequence::new(10))
        .expect("native-backed surface-local provider");
    let forged_native_capture = PointerEdgeJournal::new(
        PointerEdgeSequence::new(10),
        PointerEdgeSequence::new(11),
        vec![edge(
            11,
            PointerEdgeKind::CaptureChanged,
            local_location(),
            Authority::Known(PointerCaptureOwner::Native(local_binding)),
        )],
    )
    .expect("complete local capture journal");
    assert_eq!(
        commit_candidate(&mut ledger, local, forged_native_capture),
        Err(PointerJournalLedgerError::CaptureScopeMismatch {
            lease: local,
            sequence: PointerEdgeSequence::new(11),
            submitted: PointerCaptureOwner::Native(local_binding),
        })
    );

    let endpoint_capture = PointerEdgeJournal::new(
        PointerEdgeSequence::new(10),
        PointerEdgeSequence::new(11),
        vec![edge(
            11,
            PointerEdgeKind::CaptureChanged,
            local_location(),
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    )
    .expect("complete endpoint capture journal");
    commit_candidate(&mut ledger, local, endpoint_capture)
        .expect("surface-local provider reports its frozen endpoint symbolically");
}

#[test]
fn native_capture_identity_preserves_window_incarnation_across_token_reuse() {
    let authority_domain = domain(1);
    let stale = binding_with_incarnation(authority_domain, 7, 70, 1);
    let successor = binding_with_incarnation(authority_domain, 7, 70, 2);
    assert_ne!(stale, successor);

    let mut ledger = PointerJournalLedger::new(authority_domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("desktop provider");
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(2),
        vec![
            edge(
                1,
                PointerEdgeKind::CaptureChanged,
                desktop_location(Authority::Known(PointerWindow::Dock(stale))),
                Authority::Known(PointerCaptureOwner::Native(stale)),
            ),
            edge(
                2,
                PointerEdgeKind::CaptureChanged,
                desktop_location(Authority::Known(PointerWindow::Dock(successor))),
                Authority::Known(PointerCaptureOwner::Native(successor)),
            ),
        ],
    )
    .expect("complete capture journal");

    let commit = commit_candidate(&mut ledger, lease, journal).expect("exact native captures");
    assert_eq!(
        commit.journal().edges()[0].capture_owner(),
        Authority::Known(PointerCaptureOwner::Native(stale))
    );
    assert_eq!(
        commit.journal().edges()[1].capture_owner(),
        Authority::Known(PointerCaptureOwner::Native(successor))
    );
    assert_eq!(
        commit.accepted_edges()[0].stream(),
        commit.accepted_edges()[1].stream()
    );
}

#[test]
fn desktop_route_from_another_engine_domain_is_rejected_before_watermark_commit() {
    let local_domain = domain(1);
    let foreign_domain = domain(2);
    let mut ledger = PointerJournalLedger::new(local_domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(4),
        )
        .expect("desktop provider");
    let foreign = binding(foreign_domain, 7, 70);
    let desktop = PhysicalPoint::new(12.0, 34.0).expect("finite desktop point");
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(4),
        PointerEdgeSequence::new(5),
        vec![edge(
            5,
            PointerEdgeKind::Moved,
            PointerEdgeLocation::Desktop {
                route: DesktopRouteFact::dock_without_coordinate_route(
                    foreign,
                    Authority::Known(desktop),
                    AuthorityUnavailableReason::CoordinateUnavailable,
                ),
            },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("journal is structurally complete");

    assert_eq!(
        commit_candidate(&mut ledger, lease, journal),
        Err(PointerJournalLedgerError::ForeignDesktopRouteBinding {
            lease,
            sequence: PointerEdgeSequence::new(5),
            binding: foreign,
            expected: local_domain,
            submitted: foreign_domain,
        })
    );
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease,
            committed_through: PointerEdgeSequence::new(4),
        })
    );
}

#[test]
fn cancellation_and_pointer_reuse_in_one_batch_get_distinct_streams() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let location = desktop_location(Authority::Known(PointerWindow::None));
    let capture = Authority::Known(PointerCaptureOwner::None);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(3),
        vec![
            edge(1, PointerEdgeKind::Moved, location, capture),
            edge(
                2,
                PointerEdgeKind::StreamCancelled(
                    PointerStreamCancelReason::ExplicitPlatformCancellation,
                ),
                location,
                capture,
            ),
            edge(
                3,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                location,
                capture,
            ),
        ],
    )
    .expect("complete reused-pointer journal");

    let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
    let accepted = commit.accepted_edges();
    assert_eq!(accepted.len(), 3);
    assert_eq!(accepted[0].stream(), accepted[1].stream());
    assert_ne!(accepted[1].stream(), accepted[2].stream());
    assert_eq!(accepted[0].stream().incarnation(), 1);
    assert_eq!(accepted[1].stream().incarnation(), 1);
    assert_eq!(accepted[2].stream().incarnation(), 2);
    assert_eq!(accepted[1].stream().pointer(), PointerId::new(7));
}

#[test]
fn normal_terminal_release_retires_touch_stream_without_becoming_cancellation() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let location = desktop_location(Authority::Known(PointerWindow::None));
    let capture = Authority::Known(PointerCaptureOwner::None);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(2),
        vec![
            edge(
                1,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                location,
                capture,
            )
            .ending_stream(),
            edge(
                2,
                PointerEdgeKind::ButtonPressed(PointerButton::Primary),
                location,
                capture,
            ),
        ],
    )
    .expect("complete reused touch journal");

    let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
    assert!(matches!(
        commit.journal().edges()[0].kind(),
        PointerEdgeKind::ButtonReleased(PointerButton::Primary)
    ));
    assert!(commit.journal().edges()[0].ends_stream());
    assert_ne!(
        commit.accepted_edges()[0].stream(),
        commit.accepted_edges()[1].stream()
    );
}

#[test]
fn ten_thousand_terminal_touch_releases_leave_no_active_streams() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let location = desktop_location(Authority::Known(PointerWindow::None));
    let capture = Authority::Known(PointerCaptureOwner::None);
    let edges = (1..=10_000)
        .map(|sequence| {
            edge(
                sequence,
                PointerEdgeKind::ButtonReleased(PointerButton::Primary),
                location,
                capture,
            )
            .ending_stream()
        })
        .collect::<Vec<_>>();
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(10_000),
        edges,
    )
    .expect("complete terminal touch journal");

    let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
    assert_eq!(commit.accepted_edges().len(), 10_000);
    assert!(ledger.active_streams.is_empty());
    assert_eq!(ledger.last_stream_incarnation.0, 10_000);
}

#[test]
fn interleaved_pointers_retain_independent_monotonic_streams() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let location = desktop_location(Authority::Known(PointerWindow::None));
    let capture = Authority::Known(PointerCaptureOwner::None);
    let journal = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(5),
        vec![
            edge_for_pointer(1, 7, PointerEdgeKind::Moved, location, capture),
            edge_for_pointer(2, 9, PointerEdgeKind::Moved, location, capture),
            edge_for_pointer(
                3,
                7,
                PointerEdgeKind::StreamCancelled(
                    PointerStreamCancelReason::ExplicitPlatformCancellation,
                ),
                location,
                capture,
            ),
            edge_for_pointer(4, 9, PointerEdgeKind::Moved, location, capture),
            edge_for_pointer(5, 7, PointerEdgeKind::Moved, location, capture),
        ],
    )
    .expect("complete interleaved journal");

    let commit = commit_candidate(&mut ledger, lease, journal).expect("journal commit");
    assert_eq!(
        commit
            .accepted_edges()
            .iter()
            .map(|accepted| accepted.stream().incarnation())
            .collect::<Vec<_>>(),
        vec![1, 2, 1, 2, 3]
    );
    assert_eq!(
        commit.accepted_edges()[1].stream(),
        commit.accepted_edges()[3].stream()
    );
}

#[test]
fn failed_commit_and_retry_do_not_consume_stream_incarnations() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let cancelled = PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![edge(
            1,
            PointerEdgeKind::StreamCancelled(
                PointerStreamCancelReason::ExplicitPlatformCancellation,
            ),
            desktop_location(Authority::Known(PointerWindow::None)),
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("complete cancellation journal");
    let accepted = ledger
        .prepare_candidate(lease, cancelled.clone())
        .expect("accepted candidate");
    let stale = ledger
        .prepare_candidate(lease, cancelled)
        .expect("parallel candidate");
    assert_eq!(ledger.last_stream_incarnation, PointerStreamIncarnation(0));

    let first = ledger
        .commit_prepared(accepted)
        .expect("first candidate commits");
    assert_eq!(first.accepted_edges()[0].stream().incarnation(), 1);
    assert!(matches!(
        ledger.commit_prepared(stale),
        Err(PointerJournalLedgerError::PreparedJournalStale { .. })
    ));
    assert_eq!(ledger.last_stream_incarnation, PointerStreamIncarnation(1));

    let successor = commit_candidate(&mut ledger, lease, moved_journal(1, 2))
        .expect("same provider pointer starts a successor stream");
    assert_eq!(successor.accepted_edges()[0].stream().incarnation(), 2);
}

#[test]
fn candidate_clone_commits_stream_state_without_mutating_the_source_snapshot() {
    let mut source = PointerJournalLedger::new(domain(1));
    let lease = source
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    let prepared = source
        .prepare_candidate(lease, moved_journal(0, 1))
        .expect("candidate before speculative clone");
    let mut candidate = source.clone();

    let commit = candidate
        .commit_prepared(prepared)
        .expect("shared ledger identity permits atomic candidate commit");
    assert_eq!(commit.accepted_edges()[0].stream().incarnation(), 1);
    assert_eq!(source.last_stream_incarnation, PointerStreamIncarnation(0));
    assert!(source.active_streams.is_empty());
    assert_eq!(
        candidate.last_stream_incarnation,
        PointerStreamIncarnation(1)
    );
    assert_eq!(candidate.active_streams.len(), 1);
}

#[test]
fn replay_and_ahead_candidates_are_rejected_atomically() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    commit_candidate(&mut ledger, lease, moved_journal(0, 2)).expect("initial journal");

    assert_eq!(
        commit_candidate(&mut ledger, lease, moved_journal(0, 1)),
        Err(PointerJournalLedgerError::JournalReplay {
            lease,
            committed_through: PointerEdgeSequence::new(2),
            submitted_previous: PointerEdgeSequence::new(0),
        })
    );
    assert_eq!(
        commit_candidate(&mut ledger, lease, moved_journal(3, 4)),
        Err(PointerJournalLedgerError::JournalAhead {
            lease,
            committed_through: PointerEdgeSequence::new(2),
            submitted_previous: PointerEdgeSequence::new(3),
        })
    );

    let accepted = commit_candidate(&mut ledger, lease, moved_journal(2, 3))
        .expect("rejections did not advance the committed watermark");
    assert_eq!(accepted.journal().through(), PointerEdgeSequence::new(3));
}

#[test]
fn foreign_unknown_and_retired_leases_are_typed_rejections() {
    let local_domain = domain(1);
    let mut ledger = PointerJournalLedger::new(local_domain);
    let active = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(5),
        )
        .expect("provider");
    let foreign = desktop_lease(domain(2), active.incarnation());
    let unknown = desktop_lease(local_domain, 99);

    assert_eq!(
        commit_candidate(&mut ledger, foreign, moved_journal(5, 5)),
        Err(PointerJournalLedgerError::ForeignLease {
            expected: local_domain,
            submitted: domain(2),
        })
    );
    assert_eq!(
        commit_candidate(&mut ledger, unknown, moved_journal(5, 5)),
        Err(PointerJournalLedgerError::UnknownLease { lease: unknown })
    );

    ledger.retire_provider(active).expect("retire active");
    assert_eq!(
        commit_candidate(&mut ledger, active, moved_journal(5, 5)),
        Err(PointerJournalLedgerError::RetiredLease {
            lease: active,
            committed_through: PointerEdgeSequence::new(5),
        })
    );
}

#[test]
fn quiesced_pointer_leases_compact_to_one_fail_closed_incarnation_range() {
    let domain = domain(1);
    let mut platform_authority = PlatformObservationAuthority::new(domain);
    let platform = platform_authority
        .create()
        .expect("test platform provider must mint");
    let presentation_host = host(domain);
    let mut ledger = PointerJournalLedger::new(domain);
    let mut producers = Vec::with_capacity(10_000);

    for _ in 0..10_000 {
        let lease = ledger
            .create_provider(
                PointerProviderScope::DesktopGlobal,
                PointerEdgeSequence::new(0),
            )
            .expect("provider incarnation must remain available");
        ledger
            .retire_provider(lease)
            .expect("the active provider must retire exactly once");
        let recorder = BackendIngressRecorder::new(
            platform,
            lease,
            presentation_host,
            PointerEdgeSequence::new(0),
        )
        .expect("the joined producer must bind the exact pointer lease");
        producers.push((lease, recorder));
    }

    let before = ledger.retention_manifest();
    assert_eq!(before.active_provider_count(), 0);
    assert_eq!(before.active_stream_count(), 0);
    assert_eq!(before.retired_lease_guards(), 10_000);
    assert_eq!(before.compacted_retirement_ranges(), 0);
    assert_eq!(before.logical_compacted_leases(), 0);
    assert_eq!(before.retained_structure_count(), 10_000);
    assert_eq!(
        before.terminal_release_barrier(),
        Some(crate::retention::RuntimeRetentionReleaseBarrier::PointerIngressQuiesced),
    );

    let first = producers[0].0;
    for (_, recorder) in producers {
        let receipt = recorder.drain();
        ledger
            .compact_quiesced_backend(&receipt)
            .expect("a joined producer releases its detailed tombstone");
    }

    let after = ledger.retention_manifest();
    assert_eq!(after.retired_lease_guards(), 0);
    assert_eq!(after.compacted_retirement_ranges(), 1);
    assert_eq!(after.logical_compacted_leases(), 10_000);
    assert_eq!(after.retained_structure_count(), 1);
    assert_eq!(after.terminal_release_barrier(), None);
    assert!(matches!(
        ledger.prepare_candidate(first, moved_journal(0, 0)),
        Err(PointerJournalLedgerError::CompactedLease { lease }) if lease == first
    ));
}

#[test]
fn retention_accounts_for_the_live_provider_and_device_stream_index() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    commit_candidate(&mut ledger, lease, moved_journal(0, 1))
        .expect("one pointer stream must be accepted");

    let retention = ledger.retention_manifest();
    assert_eq!(retention.active_provider_count(), 1);
    assert_eq!(retention.active_stream_count(), 1);
    assert_eq!(retention.retired_lease_guards(), 0);
    assert_eq!(retention.compacted_retirement_ranges(), 0);
    assert_eq!(retention.logical_compacted_leases(), 0);
    assert_eq!(retention.retained_structure_count(), 2);
    assert_eq!(retention.terminal_release_barrier(), None);
}

#[test]
fn quiesced_compaction_version_exhaustion_preserves_the_detailed_tombstone() {
    let domain = domain(1);
    let mut platform_authority = PlatformObservationAuthority::new(domain);
    let platform = platform_authority
        .create()
        .expect("test platform provider must mint");
    let presentation_host = host(domain);
    let mut ledger = PointerJournalLedger::new(domain);
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(7),
        )
        .expect("provider incarnation must remain available");
    ledger
        .retire_provider(lease)
        .expect("the active provider must retire exactly once");
    let receipt = BackendIngressRecorder::new(
        platform,
        lease,
        presentation_host,
        PointerEdgeSequence::new(7),
    )
    .expect("the joined producer must bind the exact pointer lease")
    .drain();
    ledger.version = PointerJournalLedgerVersion(u64::MAX);
    let before = ledger.clone();

    assert_eq!(
        ledger.compact_quiesced_backend(&receipt),
        Err(PointerJournalLedgerError::LedgerVersionExhausted)
    );
    assert_eq!(ledger, before);
    assert_eq!(ledger.retention_manifest().retired_lease_guards(), 1);
    assert_eq!(ledger.retention_manifest().compacted_retirement_ranges(), 0);
}

#[test]
fn retirement_rejections_preserve_the_active_lease_and_watermark() {
    let local_domain = domain(1);
    let mut ledger = PointerJournalLedger::new(local_domain);
    let first = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(5),
        )
        .expect("first provider");
    commit_candidate(&mut ledger, first, moved_journal(5, 7)).expect("advance first watermark");
    let expected_first = ActivePointerProvider {
        lease: first,
        committed_through: PointerEdgeSequence::new(7),
    };
    let foreign = desktop_lease(domain(2), first.incarnation());
    let unknown = desktop_lease(local_domain, 99);

    assert_eq!(
        ledger.retire_provider(foreign),
        Err(PointerJournalLedgerError::ForeignLease {
            expected: local_domain,
            submitted: domain(2),
        })
    );
    assert_eq!(ledger.active, Some(expected_first));
    assert!(ledger.retired.is_empty());

    assert_eq!(
        ledger.retire_provider(unknown),
        Err(PointerJournalLedgerError::UnknownLease { lease: unknown })
    );
    assert_eq!(ledger.active, Some(expected_first));
    assert!(ledger.retired.is_empty());

    ledger
        .retire_provider(first)
        .expect("retire exact first lease");
    let successor = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(20),
        )
        .expect("successor provider");
    commit_candidate(&mut ledger, successor, moved_journal(20, 21))
        .expect("advance successor watermark");
    let expected_successor = ActivePointerProvider {
        lease: successor,
        committed_through: PointerEdgeSequence::new(21),
    };

    assert_eq!(
        ledger.retire_provider(first),
        Err(PointerJournalLedgerError::RetiredLease {
            lease: first,
            committed_through: PointerEdgeSequence::new(7),
        })
    );
    assert_eq!(ledger.active, Some(expected_successor));
    assert_eq!(ledger.retired.len(), 1);
    assert_eq!(
        ledger.retired.get(&first),
        Some(&RetiredPointerInputLease {
            lease: first,
            committed_through: PointerEdgeSequence::new(7),
        })
    );
}

#[test]
fn successor_incarnation_cannot_collide_with_old_tickets_or_streams() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let first_lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("first provider");
    let first_commit =
        commit_candidate(&mut ledger, first_lease, moved_journal(0, 1)).expect("first journal");
    let first_ticket = first_commit.accepted_edges()[0].ticket();
    let pointer = PointerId::new(7);
    let first_stream = first_commit.accepted_edges()[0].stream();
    ledger
        .retire_provider(first_lease)
        .expect("retire first provider");

    let next_lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("successor provider");
    let next_commit =
        commit_candidate(&mut ledger, next_lease, moved_journal(0, 1)).expect("successor journal");
    let next_ticket = next_commit.accepted_edges()[0].ticket();
    let next_stream = next_commit.accepted_edges()[0].stream();

    assert_eq!(first_ticket.sequence(), next_ticket.sequence());
    assert_ne!(first_ticket.lease(), next_ticket.lease());
    assert_ne!(first_ticket, next_ticket);
    assert_eq!(first_stream.pointer(), pointer);
    assert_eq!(first_stream.lease(), first_lease);
    assert_eq!(first_stream.incarnation(), 1);
    assert_eq!(next_stream.incarnation(), 2);
    assert_ne!(first_stream, next_stream);
}

#[test]
fn provider_incarnation_counter_never_wraps() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    ledger.last_incarnation = PointerProviderIncarnation(u64::MAX);

    assert_eq!(
        ledger.create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        ),
        Err(PointerJournalLedgerError::ProviderIncarnationExhausted)
    );
    assert!(ledger.active.is_none());
    assert_eq!(
        ledger.last_incarnation,
        PointerProviderIncarnation(u64::MAX)
    );
}

#[test]
fn stream_incarnation_exhaustion_is_atomic() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(0),
        )
        .expect("provider");
    ledger.last_stream_incarnation = PointerStreamIncarnation(u64::MAX);
    let before_version = ledger.version;
    let prepared = ledger
        .prepare_candidate(lease, moved_journal(0, 1))
        .expect("journal is valid before stream identity planning");

    assert_eq!(
        ledger.commit_prepared(prepared),
        Err(PointerJournalLedgerError::StreamIncarnationExhausted)
    );
    assert_eq!(ledger.version, before_version);
    assert_eq!(
        ledger.active,
        Some(ActivePointerProvider {
            lease,
            committed_through: PointerEdgeSequence::new(0),
        })
    );
    assert!(ledger.active_streams.is_empty());
    assert_eq!(
        ledger.last_stream_incarnation,
        PointerStreamIncarnation(u64::MAX)
    );
}

#[test]
fn empty_candidate_succeeds_without_fabricating_an_edge_ticket() {
    let mut ledger = PointerJournalLedger::new(domain(1));
    let lease = ledger
        .create_provider(
            PointerProviderScope::DesktopGlobal,
            PointerEdgeSequence::new(5),
        )
        .expect("provider");

    let empty = commit_candidate(&mut ledger, lease, moved_journal(5, 5))
        .expect("authoritative empty interval");
    assert!(empty.journal().is_empty());
    assert!(empty.accepted_edges().is_empty());

    let next = commit_candidate(&mut ledger, lease, moved_journal(5, 6))
        .expect("empty interval left watermark at five");
    assert_eq!(next.accepted_edges().len(), 1);
    assert_eq!(
        next.accepted_edges()[0].ticket().sequence(),
        PointerEdgeSequence::new(6)
    );
}

#[test]
fn desktop_dock_route_uses_the_target_binding_generation_and_scale_once() {
    let domain = domain(7);
    let surface = SurfaceId::new(91);
    let (registry, binding, generation) =
        routeable_registry(domain, surface, WindowToken::new(901), 1_000.0, 2.0);
    let desktop = PhysicalPoint::new(1_200.0, 150.0).expect("finite desktop point");
    let surface_point = LogicalPoint::new(100.0, 75.0).expect("finite surface point");
    let fact = DesktopRouteFact::dock(DesktopDockRoute::new(
        binding,
        generation,
        desktop,
        surface_point,
    ));

    assert_eq!(
        fact.validate_against_registry(domain, &registry),
        DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(
            fact.dock_route()
                .expect("dock fact retains its exact route")
        ))
    );
    assert_eq!(fact.position(), Authority::Known(desktop));
    assert_eq!(
        fact.hovered(),
        Authority::Known(DesktopHoveredWindow::Dock(binding))
    );
}

#[test]
fn core_derived_desktop_route_uses_current_registry_coordinates() {
    let domain = domain(71);
    let surface = SurfaceId::new(911);
    let (registry, binding, generation) =
        routeable_registry(domain, surface, WindowToken::new(9_011), 1_000.0, 2.0);
    let desktop = PhysicalPoint::new(1_200.0, 150.0).expect("finite desktop point");
    let expected = DesktopDockRoute::new(
        binding,
        generation,
        desktop,
        LogicalPoint::new(100.0, 75.0).expect("finite surface point"),
    );
    let fact = DesktopRouteFact::dock_from_desktop_position(binding, Authority::Known(desktop));

    assert_eq!(fact.dock_route(), None);
    assert_eq!(
        fact.validate_against_registry(domain, &registry),
        DesktopRouteValidation::Known(ValidatedDesktopRoute::Dock(expected))
    );
}

#[test]
fn core_derived_desktop_route_fails_closed_without_physical_position() {
    let domain = domain(72);
    let surface = SurfaceId::new(912);
    let (registry, binding, _) =
        routeable_registry(domain, surface, WindowToken::new(9_012), 0.0, 1.0);
    let fact = DesktopRouteFact::dock_from_desktop_position(
        binding,
        Authority::Unknown(AuthorityUnavailableReason::CoordinateUnavailable),
    );

    assert_eq!(
        fact.validate_against_registry(domain, &registry),
        DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::DesktopPositionUnknown {
            binding,
            reason: AuthorityUnavailableReason::CoordinateUnavailable,
        })
    );
}

#[test]
fn desktop_unknown_foreign_and_no_window_remain_distinct() {
    let domain = domain(8);
    let registry = ViewportRegistry::new(domain);
    let position = PhysicalPoint::new(12.0, 34.0).expect("finite desktop point");

    assert_eq!(
        DesktopRouteFact::unknown(
            Authority::Known(position),
            AuthorityUnavailableReason::NotReported,
        )
        .validate_against_registry(domain, &registry),
        DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::HoverUnknown {
            position: Authority::Known(position),
            reason: AuthorityUnavailableReason::NotReported,
        })
    );
    assert_eq!(
        DesktopRouteFact::foreign(Authority::Known(position))
            .validate_against_registry(domain, &registry),
        DesktopRouteValidation::Known(ValidatedDesktopRoute::Foreign {
            position: Authority::Known(position),
        })
    );
    assert_eq!(
        DesktopRouteFact::no_window(
            Authority::Known(position),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .validate_against_registry(domain, &registry),
        DesktopRouteValidation::Known(ValidatedDesktopRoute::NoWindow {
            position: Authority::Known(position),
            work_area: Authority::Unknown(AuthorityUnavailableReason::NotReported),
        })
    );
}

#[test]
fn known_dock_hover_without_a_coordinate_route_cannot_become_a_local_target() {
    let domain = domain(81);
    let surface = SurfaceId::new(811);
    let (registry, binding, _) =
        routeable_registry(domain, surface, WindowToken::new(8_110), 0.0, 1.0);
    let position = PhysicalPoint::new(12.0, 34.0).expect("finite desktop point");
    let fact = DesktopRouteFact::dock_without_coordinate_route(
        binding,
        Authority::Known(position),
        AuthorityUnavailableReason::CoordinateUnavailable,
    );

    assert_eq!(
        fact.hovered(),
        Authority::Known(DesktopHoveredWindow::Dock(binding))
    );
    assert_eq!(
        fact.dock_route_authority(),
        Some(Authority::Unknown(
            AuthorityUnavailableReason::CoordinateUnavailable
        ))
    );
    assert_eq!(fact.dock_route(), None);
    assert_eq!(
        fact.validate_against_registry(domain, &registry),
        DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::CoordinateRouteUnknown {
            binding,
            position: Authority::Known(position),
            reason: AuthorityUnavailableReason::CoordinateUnavailable,
        })
    );
}

#[test]
fn desktop_route_rejects_a_reused_window_token_with_an_old_incarnation() {
    let domain = domain(9);
    let surface = SurfaceId::new(92);
    let (registry, current, _) =
        routeable_registry(domain, surface, WindowToken::new(902), 0.0, 1.0);
    let stale = ViewportBinding::new(
        domain,
        current.epoch(),
        surface,
        current.token(),
        WindowIncarnation::new(
            current
                .incarnation()
                .get()
                .checked_add(1)
                .expect("test incarnation has a successor"),
        ),
    );
    let desktop = PhysicalPoint::new(64.0, 48.0).expect("finite desktop point");
    let fact = DesktopRouteFact::dock_from_desktop_position(stale, Authority::Known(desktop));

    assert_eq!(
        fact.validate_against_registry(domain, &registry),
        DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::StaleBinding {
            observed: stale,
            current,
        })
    );
}

#[test]
fn desktop_route_rejects_an_adapter_logical_point_that_disagrees_with_coordinates() {
    let domain = domain(10);
    let surface = SurfaceId::new(93);
    let (registry, binding, generation) =
        routeable_registry(domain, surface, WindowToken::new(903), 1_000.0, 2.0);
    let desktop = PhysicalPoint::new(1_200.0, 150.0).expect("finite desktop point");
    let submitted = LogicalPoint::new(101.0, 75.0).expect("finite surface point");
    let expected = LogicalPoint::new(100.0, 75.0).expect("finite surface point");
    let fact = DesktopRouteFact::dock(DesktopDockRoute::new(
        binding, generation, desktop, submitted,
    ));

    assert_eq!(
        fact.validate_against_registry(domain, &registry),
        DesktopRouteValidation::Unavailable(DesktopRouteUnavailable::CoordinatePointMismatch {
            binding,
            physical: desktop,
            expected,
            submitted,
        })
    );
}
