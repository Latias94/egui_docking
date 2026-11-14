use super::*;
use crate::input::ingress::BackendIngressPayload;

fn journal() -> PointerEdgeJournal {
    PointerEdgeJournal::new(
        PointerEdgeSequence::new(0),
        PointerEdgeSequence::new(1),
        vec![PointerEdge::new(
            PointerEdgeSequence::new(1),
            PointerId::new(1),
            PointerEdgeKind::Moved,
            PointerEdgeLocation::SurfaceLocal {
                position: Authority::Unknown(AuthorityUnavailableReason::NotReported),
            },
            Authority::Known(PointerCaptureOwner::None),
        )],
    )
    .expect("the journal must cover its complete interval")
}

fn checkpoint(button: PointerButton) -> PointerAuthorityCheckpoint {
    PointerAuthorityCheckpoint::known(
        PointerEdgeSequence::new(0),
        vec![
            PointerStateObservation::new(
                PointerId::new(1),
                vec![button],
                Authority::Known(PointerCaptureOwner::None),
            )
            .expect("the pointer state must be canonical"),
        ],
    )
    .expect("the checkpoint must have one complete pointer roster")
}

#[test]
fn attaching_checkpoint_preserves_cloned_branch_and_reuses_unique_storage() {
    let original = journal();
    let original_owner = Arc::as_ptr(&original.contents);
    let original_edges = original.edges().as_ptr();
    let branch = original.clone();
    assert_eq!(branch, original);
    assert!(Arc::ptr_eq(&branch.contents, &original.contents));

    let attached = branch
        .with_authority_checkpoint(checkpoint(PointerButton::Primary))
        .expect("the branch must accept its first checkpoint");
    assert!(original.authority_checkpoint().is_none());
    assert!(!Arc::ptr_eq(&attached.contents, &original.contents));
    assert_eq!(attached.edges(), original.edges());
    assert_ne!(attached.edges().as_ptr(), original_edges);
    assert_eq!(attached.previous(), original.previous());
    assert_eq!(attached.through(), original.through());

    let frozen = attached.clone();
    assert!(Arc::ptr_eq(&frozen.contents, &attached.contents));
    assert!(std::ptr::eq(
        frozen
            .authority_checkpoint()
            .expect("the clone has a checkpoint"),
        attached
            .authority_checkpoint()
            .expect("the branch has a checkpoint"),
    ));
    let original = original
        .with_authority_checkpoint(checkpoint(PointerButton::Secondary))
        .expect("the unique original must accept its independent checkpoint");
    assert_eq!(Arc::as_ptr(&original.contents), original_owner);
    assert_eq!(original.edges().as_ptr(), original_edges);
    assert_eq!(
        original.authority_checkpoint(),
        Some(&checkpoint(PointerButton::Secondary)),
    );
    assert_eq!(
        attached.authority_checkpoint(),
        Some(&checkpoint(PointerButton::Primary)),
    );
    assert_eq!(frozen, attached);
}

#[test]
fn backend_payload_clone_shares_journal_edges_and_checkpoint_owner() {
    let journal = journal()
        .with_authority_checkpoint(checkpoint(PointerButton::Primary))
        .expect("the journal must retain its authority baseline");
    let owner = Arc::downgrade(&journal.contents);
    let payload = BackendIngressPayload::PointerSegment(journal);
    let cloned = payload.clone();
    let retry = cloned.clone();

    let BackendIngressPayload::PointerSegment(original) = &payload else {
        panic!("the payload must contain a pointer journal");
    };
    for copy in [&cloned, &retry] {
        let BackendIngressPayload::PointerSegment(journal) = copy else {
            panic!("the cloned payload must retain its pointer journal");
        };
        assert_eq!(copy, &payload);
        assert!(Arc::ptr_eq(&original.contents, &journal.contents));
        assert_eq!(original.edges().as_ptr(), journal.edges().as_ptr());
        assert_eq!(journal.len(), 1);
        assert!(!journal.is_empty());
        let original_checkpoint = original
            .authority_checkpoint()
            .expect("the original baseline");
        let cloned_checkpoint = journal.authority_checkpoint().expect("the cloned baseline");
        assert!(std::ptr::eq(original_checkpoint, cloned_checkpoint));
        let Authority::Known(original_pointers) = original_checkpoint.pointers() else {
            panic!("the original baseline must retain its nonempty roster");
        };
        let Authority::Known(cloned_pointers) = cloned_checkpoint.pointers() else {
            panic!("the cloned baseline must retain its nonempty roster");
        };
        assert_eq!(original_pointers.len(), 1);
        assert_eq!(original_pointers.as_ptr(), cloned_pointers.as_ptr());
        assert_eq!(original_pointers[0].pressed_buttons().len(), 1);
        assert_eq!(
            original_pointers[0].pressed_buttons().as_ptr(),
            cloned_pointers[0].pressed_buttons().as_ptr(),
        );
    }

    drop(payload);
    drop(cloned);
    assert!(owner.upgrade().is_some());
    let BackendIngressPayload::PointerSegment(journal) = &retry else {
        panic!("the retry must retain its pointer journal");
    };
    assert_eq!(journal.edges().len(), 1);
    assert_eq!(
        journal.authority_checkpoint(),
        Some(&checkpoint(PointerButton::Primary)),
    );
    drop(retry);
    assert!(owner.upgrade().is_none());
}

#[test]
fn into_edges_moves_unique_vector_and_copies_shared_vector() {
    let unique = journal()
        .with_authority_checkpoint(checkpoint(PointerButton::Primary))
        .expect("the unique journal must retain its authority baseline");
    let unique_edges = unique.edges().as_ptr();
    let moved = unique.into_edges();
    assert_eq!(moved.as_ptr(), unique_edges);
    assert_eq!(moved.len(), 1);

    let shared = journal()
        .with_authority_checkpoint(checkpoint(PointerButton::Primary))
        .expect("the shared journal must retain its authority baseline");
    let retained = shared.clone();
    let mut copied = shared.into_edges();
    assert_eq!(copied, moved);
    assert_eq!(copied.as_slice(), retained.edges());
    assert_ne!(copied.as_ptr(), retained.edges().as_ptr());
    copied.clear();
    assert_eq!(retained.edges(), moved.as_slice());
    assert_eq!(
        retained.authority_checkpoint(),
        Some(&checkpoint(PointerButton::Primary)),
    );
    let retained_edges = retained.edges().as_ptr();
    let moved = retained.into_edges();
    assert_eq!(moved.as_ptr(), retained_edges);
}
