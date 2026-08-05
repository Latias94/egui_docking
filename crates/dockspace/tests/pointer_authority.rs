mod support;

use dockspace::engine::{CoreHostFrame, DockEngine};
use dockspace::geometry::LogicalPoint;
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::intent::{Authority, AuthorityUnavailableReason, PointerButton, PointerId};
use dockspace::pointer_journal::{
    AnyButtonDownAuthority, PointerAuthorityCheckpoint, PointerCaptureOwner, PointerEdge,
    PointerEdgeJournal, PointerEdgeKind, PointerEdgeLocation, PointerEdgeSequence,
    PointerStateObservation, SurfaceLocalPointerEndpoint, SurfaceLocalPointerProvider,
    SurfaceLocalPointerScope,
};
use dockspace::pointer_receiver::{
    PointerReceiverObservation, PointerReceiverReceipt, PointerReceiverReceiptBatch,
};
use dockspace::policy::DockPolicy;
use dockspace::transition::EngineTransition;
use support::{TestPresentationHost, complete_host_frame_with_retained_or_unavailable};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const POINTER: PointerId = PointerId::new(7);

struct Fixture {
    engine: DockEngine,
    host: TestPresentationHost,
    provider: SurfaceLocalPointerProvider,
}

impl Fixture {
    fn new() -> Self {
        let mut engine =
            DockEngine::new(workspace(), DockPolicy::default()).expect("test engine is valid");
        let host = TestPresentationHost::new(&mut engine);
        let provider = create_provider(&mut engine, &host);
        Self {
            engine,
            host,
            provider,
        }
    }

    fn submit(&mut self, journal: PointerEdgeJournal) -> EngineTransition {
        submit_journal(&mut self.engine, &mut self.host, &self.provider, journal)
    }
}

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("test workspace is valid")
}

fn create_provider(
    engine: &mut DockEngine,
    host: &TestPresentationHost,
) -> SurfaceLocalPointerProvider {
    engine
        .create_surface_local_pointer_provider(
            SurfaceLocalPointerScope::new(
                host.lease(),
                SurfaceLocalPointerEndpoint::Logical(SURFACE),
            ),
            PointerEdgeSequence::new(0),
        )
        .expect("surface-local pointer provider is admitted")
}

fn empty_journal(previous: u64) -> PointerEdgeJournal {
    let watermark = PointerEdgeSequence::new(previous);
    PointerEdgeJournal::new(watermark, watermark, Vec::new())
        .expect("empty journal preserves its watermark")
}

fn journal(previous: u64, edges: Vec<PointerEdge>) -> PointerEdgeJournal {
    let through = edges
        .last()
        .map_or(PointerEdgeSequence::new(previous), PointerEdge::sequence);
    PointerEdgeJournal::new(PointerEdgeSequence::new(previous), through, edges)
        .expect("test journal is contiguous")
}

fn edge(
    sequence: u64,
    kind: PointerEdgeKind,
    capture: Authority<PointerCaptureOwner>,
) -> PointerEdge {
    PointerEdge::new(
        PointerEdgeSequence::new(sequence),
        POINTER,
        kind,
        PointerEdgeLocation::SurfaceLocal {
            position: Authority::Known(LogicalPoint::new(32.0, 24.0).expect("test point is valid")),
        },
        capture,
    )
}

fn state(
    pressed_buttons: impl IntoIterator<Item = PointerButton>,
    capture: Authority<PointerCaptureOwner>,
) -> PointerStateObservation {
    PointerStateObservation::new(POINTER, pressed_buttons.into_iter().collect(), capture)
        .expect("test pointer state is canonical")
}

fn known_checkpoint(
    observed_through: u64,
    observations: impl IntoIterator<Item = PointerStateObservation>,
) -> PointerAuthorityCheckpoint {
    PointerAuthorityCheckpoint::known(
        PointerEdgeSequence::new(observed_through),
        observations.into_iter().collect(),
    )
    .expect("test authority checkpoint is canonical")
}

fn with_checkpoint(
    journal: PointerEdgeJournal,
    checkpoint: PointerAuthorityCheckpoint,
) -> PointerEdgeJournal {
    journal
        .with_authority_checkpoint(checkpoint)
        .expect("checkpoint matches the journal predecessor watermark")
}

fn submit_journal(
    engine: &mut DockEngine,
    host: &mut TestPresentationHost,
    provider: &SurfaceLocalPointerProvider,
    journal: PointerEdgeJournal,
) -> EngineTransition {
    let mut frame = host.begin(engine);
    let checkpoint = journal.authority_checkpoint().cloned();
    let edges = journal.edges().to_vec();
    if edges.is_empty() {
        frame
            .submit_surface_pointer_journal(provider, journal)
            .expect("empty pointer journal follows the provider watermark");
        let receipts = frame
            .pointer_receiver_candidates()
            .expect("a live provider freezes one candidate roster")
            .candidates()
            .iter()
            .cloned()
            .map(|candidate| candidate.receipt(PointerReceiverObservation::NotApplicable))
            .collect::<Vec<PointerReceiverReceipt>>();
        frame
            .submit_pointer_receiver_receipts(
                PointerReceiverReceiptBatch::new(receipts).expect("receipts form an exact set"),
            )
            .expect("not-applicable receipts stage");
    } else {
        let mut previous = journal.previous();
        for (index, edge) in edges.into_iter().enumerate() {
            let mut segment = PointerEdgeJournal::new(previous, edge.sequence(), vec![edge])
                .expect("single edge segment is contiguous");
            if index == 0 {
                if let Some(checkpoint) = checkpoint.clone() {
                    segment = segment
                        .with_authority_checkpoint(checkpoint)
                        .expect("checkpoint belongs to the first edge segment");
                }
            }
            let through = segment.through();
            frame
                .submit_surface_pointer_journal(provider, segment)
                .expect("pointer edge follows the provider watermark");
            let candidate = frame
                .pointer_receiver_candidates()
                .expect("a live provider freezes one candidate roster")
                .candidates()[0]
                .clone();
            frame
                .submit_pointer_receiver_receipts(
                    PointerReceiverReceiptBatch::new([
                        candidate.receipt(PointerReceiverObservation::NotApplicable)
                    ])
                    .expect("receipt forms an exact set"),
                )
                .expect("not-applicable receipt stages");
            previous = through;
        }
    }
    complete_host_frame_with_retained_or_unavailable(engine, &mut frame);
    host.finish(frame, engine)
}

fn submit_edge(
    frame: &mut CoreHostFrame,
    provider: &SurfaceLocalPointerProvider,
    journal: PointerEdgeJournal,
) {
    frame
        .submit_surface_pointer_journal(provider, journal)
        .expect("single pointer edge follows the provider watermark");
    let candidate = frame
        .pointer_receiver_candidates()
        .expect("a single pointer edge freezes one candidate")
        .candidates()[0]
        .clone();
    frame
        .submit_pointer_receiver_receipts(
            PointerReceiverReceiptBatch::new([
                candidate.receipt(PointerReceiverObservation::NotApplicable)
            ])
            .expect("single edge receipt is exact"),
        )
        .expect("single edge receipt stages");
}

#[test]
fn new_provider_starts_with_unknown_button_authority() {
    let fixture = Fixture::new();

    assert!(matches!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
}

#[test]
fn complete_checkpoint_is_required_to_prove_all_buttons_released() {
    let mut fixture = Fixture::new();
    let checkpoint = known_checkpoint(0, Vec::<PointerStateObservation>::new());

    let transition = fixture.submit(with_checkpoint(empty_journal(0), checkpoint));

    assert!(transition.reduced_pointer_edges().is_empty());
    assert_eq!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
}

#[test]
fn release_then_press_exposes_edge_local_button_authority_without_lookahead() {
    let mut fixture = Fixture::new();
    let checkpoint = known_checkpoint(
        0,
        [state(
            [PointerButton::Secondary],
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    );
    let mut frame = fixture.host.begin(&fixture.engine);
    submit_edge(
        &mut frame,
        &fixture.provider,
        with_checkpoint(
            journal(
                0,
                vec![edge(
                    1,
                    PointerEdgeKind::ButtonReleased(PointerButton::Secondary),
                    Authority::Known(PointerCaptureOwner::ProviderEndpoint),
                )],
            ),
            checkpoint,
        ),
    );
    submit_edge(
        &mut frame,
        &fixture.provider,
        journal(
            1,
            vec![edge(
                2,
                PointerEdgeKind::ButtonPressed(PointerButton::Middle),
                Authority::Known(PointerCaptureOwner::ProviderEndpoint),
            )],
        ),
    );
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    let reduced = transition.reduced_pointer_edges();

    assert_eq!(reduced.len(), 2);
    assert_eq!(
        reduced[0].button_authority_after(),
        AnyButtonDownAuthority::KnownAllReleased
    );
    assert_eq!(
        reduced[1].button_authority_after(),
        AnyButtonDownAuthority::KnownDown
    );
    assert_eq!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );
}

#[test]
fn newer_unknown_capture_never_reuses_an_older_known_owner() {
    let mut fixture = Fixture::new();
    let checkpoint = known_checkpoint(
        0,
        [state(
            Vec::<PointerButton>::new(),
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    );
    let unknown = Authority::Unknown(AuthorityUnavailableReason::NotReported);
    let mut frame = fixture.host.begin(&fixture.engine);
    submit_edge(
        &mut frame,
        &fixture.provider,
        with_checkpoint(
            journal(
                0,
                vec![edge(
                    1,
                    PointerEdgeKind::Moved,
                    Authority::Known(PointerCaptureOwner::ProviderEndpoint),
                )],
            ),
            checkpoint,
        ),
    );
    submit_edge(
        &mut frame,
        &fixture.provider,
        journal(1, vec![edge(2, PointerEdgeKind::Moved, unknown)]),
    );
    complete_host_frame_with_retained_or_unavailable(&fixture.engine, &mut frame);
    let transition = fixture.host.finish(frame, &mut fixture.engine);
    let reduced = transition.reduced_pointer_edges();

    assert_eq!(reduced.len(), 2);
    assert_eq!(
        reduced[0].capture_authority_after(),
        Authority::Known(PointerCaptureOwner::ProviderEndpoint)
    );
    assert_eq!(reduced[1].capture_authority_after(), unknown);
}

#[test]
fn retired_provider_state_is_not_inherited_by_its_successor() {
    let mut fixture = Fixture::new();
    let checkpoint = known_checkpoint(
        0,
        [state(
            [PointerButton::Secondary],
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    );
    let _ = fixture.submit(with_checkpoint(empty_journal(0), checkpoint));
    assert_eq!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownDown
    );

    let Fixture {
        mut engine,
        host,
        provider,
    } = fixture;
    let mut receipt = provider
        .drain()
        .expect("a committed provider has no in-flight host frame");
    let retired = engine
        .retire_quiesced_surface_local_pointer_provider(&mut receipt)
        .expect("first provider retires atomically");
    assert!(!retired.interaction_changed());
    assert!(receipt.is_consumed());
    assert!(matches!(
        engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));

    let provider = create_provider(&mut engine, &host);
    let mut fixture = Fixture {
        engine,
        host,
        provider,
    };
    assert!(matches!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));

    let unknown = Authority::Unknown(AuthorityUnavailableReason::NotReported);
    let transition = fixture.submit(journal(0, vec![edge(1, PointerEdgeKind::Moved, unknown)]));
    let reduced = &transition.reduced_pointer_edges()[0];
    assert!(matches!(
        reduced.button_authority_after(),
        AnyButtonDownAuthority::Unknown(_)
    ));
    assert_eq!(reduced.capture_authority_after(), unknown);
}

#[test]
fn conflicting_checkpoint_at_the_same_watermark_is_rejected_atomically() {
    let mut fixture = Fixture::new();
    let all_released = known_checkpoint(0, Vec::<PointerStateObservation>::new());
    let _ = fixture.submit(with_checkpoint(empty_journal(0), all_released));

    let conflicting = known_checkpoint(
        0,
        [state(
            [PointerButton::Primary],
            Authority::Known(PointerCaptureOwner::ProviderEndpoint),
        )],
    );
    let mut frame = fixture.host.begin(&fixture.engine);
    let rejected = frame.submit_surface_pointer_journal(
        &fixture.provider,
        with_checkpoint(empty_journal(0), conflicting),
    );

    assert!(rejected.is_err());
    assert_eq!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::KnownAllReleased
    );
}

#[test]
fn explicit_unknown_checkpoint_revokes_all_released_authority() {
    let mut fixture = Fixture::new();
    let all_released = known_checkpoint(
        0,
        [state(
            Vec::<PointerButton>::new(),
            Authority::Known(PointerCaptureOwner::None),
        )],
    );
    let _ = fixture.submit(with_checkpoint(empty_journal(0), all_released));
    let _ = fixture.submit(journal(
        0,
        vec![edge(
            1,
            PointerEdgeKind::Moved,
            Authority::Known(PointerCaptureOwner::None),
        )],
    ));
    let unavailable = PointerAuthorityCheckpoint::unknown(
        PointerEdgeSequence::new(1),
        AuthorityUnavailableReason::NotReported,
    );

    let _ = fixture.submit(with_checkpoint(empty_journal(1), unavailable));

    assert!(matches!(
        fixture.engine.pointer_button_authority(),
        AnyButtonDownAuthority::Unknown(_)
    ));
}
