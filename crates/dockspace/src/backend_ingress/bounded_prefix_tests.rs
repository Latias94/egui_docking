use super::tests::authority_and_recorder;
use super::*;

#[test]
fn bounded_prefix_continues_257_records_as_256_then_one_without_consuming() {
    let (mut authority, mut recorder) = authority_and_recorder(1);
    let max_records = NonZeroUsize::new(256).expect("the record cap must be nonzero");
    for _ in 0..257 {
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("each captured record must fit");
    }

    let prefix = recorder
        .batch_prefix_after(authority.committed_through(), max_records)
        .expect("the first bounded prefix must freeze");
    assert_eq!(prefix.len(), 256);
    assert_eq!(prefix.lease(), recorder.lease());
    assert_eq!(prefix.previous(), BackendIngressOrdinal::ORIGIN);
    assert_eq!(prefix.through(), BackendIngressOrdinal(256));
    assert!(
        prefix
            .records()
            .iter()
            .map(|record| record.ordinal().get())
            .eq(1..=256)
    );
    assert_eq!(recorder.retained_record_count(), 257);
    assert_eq!(recorder.recorded_through(), BackendIngressOrdinal(257));
    assert_eq!(
        recorder
            .batch_prefix_after(authority.committed_through(), max_records)
            .expect("an unchanged recorder must return the same prefix"),
        prefix,
    );

    authority
        .commit_batch(&prefix)
        .expect("the first prefix must commit at its actual last record");
    let suffix = recorder
        .batch_prefix_after(authority.committed_through(), max_records)
        .expect("the next prefix must resume at the committed ordinal");
    assert_eq!(suffix.len(), 1);
    assert_eq!(suffix.previous(), prefix.through());
    assert_eq!(suffix.through(), BackendIngressOrdinal(257));
    assert_eq!(suffix.records()[0].ordinal(), suffix.through());
    assert_eq!(
        suffix.previous_record_identity,
        prefix.records()[255].identity,
    );
    authority
        .commit_batch(&suffix)
        .expect("the final record must commit without a gap");

    let empty = recorder
        .batch_prefix_after(authority.committed_through(), max_records)
        .expect("the exhausted suffix must remain a valid empty batch");
    assert!(empty.is_empty());
    assert_eq!(empty.previous(), suffix.through());
    assert_eq!(empty.through(), suffix.through());
    authority
        .validate_batch(&empty)
        .expect("the empty suffix must retain the committed record identity");
    assert_eq!(recorder.retained_record_count(), 257);
}

#[test]
fn bounded_prefix_preserves_empty_and_retired_boundary_identities_after_rollback() {
    let (mut authority, mut recorder) = authority_and_recorder(1);
    let empty = recorder
        .batch_prefix_after(authority.committed_through(), NonZeroUsize::MIN)
        .expect("a fresh recorder must expose an empty prefix");
    assert!(empty.is_empty());
    assert_eq!(empty.previous(), BackendIngressOrdinal::ORIGIN);
    assert_eq!(empty.through(), BackendIngressOrdinal::ORIGIN);
    authority
        .validate_batch(&empty)
        .expect("the origin identity must remain valid");

    let savepoint = recorder.savepoint();
    recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("the abandoned record must fit");
    recorder
        .rollback_to(savepoint)
        .expect("the abandoned ordinal may be reused with a fresh identity");
    recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("the replacement predecessor must fit");
    let predecessor = recorder
        .pending_batch()
        .expect("the predecessor must freeze");
    authority
        .commit_batch(&predecessor)
        .expect("the replacement predecessor must commit");
    let watermark = authority
        .commit_watermark()
        .expect("the commit must be exact");
    assert_ne!(watermark.record_identity, BackendIngressRecordIdentity(1));
    assert!(
        recorder
            .retire_committed_prefix(watermark)
            .expect("the committed predecessor must retire")
            .is_none()
    );

    let empty = recorder
        .batch_prefix_after(watermark.through(), NonZeroUsize::MIN)
        .expect("an entirely retired stream must expose an empty prefix");
    assert!(empty.is_empty());
    assert_eq!(empty.previous(), watermark.through());
    assert_eq!(empty.through(), watermark.through());
    assert_eq!(empty.previous_record_identity, watermark.record_identity);
    authority
        .validate_batch(&empty)
        .expect("retirement must preserve the exact predecessor identity");

    let next = recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("the first post-retirement record must fit");
    recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("a later record must remain outside a one-record prefix");
    let prefix = recorder
        .batch_prefix_after(watermark.through(), NonZeroUsize::MIN)
        .expect("selection must resume from the retired boundary");
    assert_eq!(prefix.len(), 1);
    assert_eq!(prefix.previous(), watermark.through());
    assert_eq!(prefix.through(), next);
    assert_eq!(prefix.previous_record_identity, watermark.record_identity);
    authority
        .validate_batch(&prefix)
        .expect("the retired identity must authenticate the new prefix");
    assert_eq!(recorder.retained_record_count(), 2);
}

#[test]
fn bounded_prefix_is_exactly_retryable_after_failed_publication() {
    let (authority, mut recorder) = authority_and_recorder(1);
    for _ in 0..2 {
        recorder
            .record_semantic_input(EngineInput::ValidateWorkspace)
            .expect("the retryable records must fit");
    }
    let prefix = recorder
        .batch_prefix_after(authority.committed_through(), NonZeroUsize::MIN)
        .expect("the retryable prefix must freeze");
    assert_eq!(prefix.len(), 1);

    assert_eq!(
        prefix
            .commit_guard()
            .publish_with(|| Err::<(), _>("rejected"))
            .expect("the publication guard must accept the live prefix"),
        Err("rejected"),
    );
    let retry = recorder
        .batch_prefix_after(authority.committed_through(), NonZeroUsize::MIN)
        .expect("failed publication must leave the same prefix selectable");
    assert_eq!(retry, prefix);
    authority
        .validate_batch(&retry)
        .expect("failed publication must leave every prefix record live");
    retry
        .commit_guard()
        .validate()
        .expect("the original publication proof must remain retryable");
    assert_eq!(recorder.retained_record_count(), 2);
    assert_eq!(recorder.recorded_through(), BackendIngressOrdinal(2));
}

#[test]
fn bounded_prefix_rollback_preserves_prefix_and_revokes_frozen_suffix() {
    let (authority, mut recorder) = authority_and_recorder(1);
    let first = recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("the retained record must fit");
    let savepoint = recorder.savepoint();
    recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("the abandoned suffix record must fit");
    let prefix = recorder
        .batch_prefix_after(authority.committed_through(), NonZeroUsize::MIN)
        .expect("the retained prefix must freeze separately from the suffix");
    assert_eq!(prefix.len(), 1);
    assert_eq!(prefix.through(), first);
    let suffix = recorder
        .batch_prefix_after(prefix.through(), NonZeroUsize::MIN)
        .expect("the abandoned suffix must freeze");
    assert_eq!(suffix.len(), 1);

    recorder
        .rollback_to(savepoint)
        .expect("rollback must remove only the later record");
    authority
        .validate_batch(&prefix)
        .expect("the bounded prefix must retain its exact live record");
    assert_eq!(
        suffix
            .commit_guard()
            .validate()
            .expect_err("the frozen suffix must retain the revoked append identity"),
        BackendIngressError::RecordRevoked {
            ordinal: suffix.through(),
        },
    );

    let replacement_ordinal = recorder
        .record_semantic_input(EngineInput::ValidateWorkspace)
        .expect("the replacement record may reuse the abandoned ordinal");
    let replacement = recorder
        .batch_prefix_after(prefix.through(), NonZeroUsize::MIN)
        .expect("the replacement suffix must freeze with its own identity");
    assert_eq!(replacement_ordinal, suffix.through());
    assert_eq!(
        replacement.previous_record_identity,
        prefix.records()[0].identity
    );
    assert_ne!(
        replacement.records()[0].identity,
        suffix.records()[0].identity
    );
    assert_ne!(replacement, suffix);
    replacement
        .commit_guard()
        .validate()
        .expect("the replacement must be live independently of the revoked suffix");
}
