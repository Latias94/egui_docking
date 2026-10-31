//! Rollback regressions for terminal document replacement authority.

use std::sync::Arc;

use super::host_frame::APPLICATION_INPUT_SOURCE;
use super::*;
use crate::engine::{EngineInput, ValidatedWorkspaceRestore};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::ItemId;
use crate::model::{
    DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
};
use crate::policy::DockPolicy;
use crate::presentation_observation::HostPresentationObservation;

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x72; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const KNOWN: ItemId = ItemId::new(2);
const INCOMING: ItemId = ItemId::new(3);

fn persistent_session(include_incoming: bool) -> DockspaceSession {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    assert_eq!(bootstrap.ensure_item("pane:first").unwrap(), FIRST);
    assert_eq!(bootstrap.ensure_item("pane:known").unwrap(), KNOWN);
    let items = if include_incoming {
        assert_eq!(bootstrap.ensure_item("pane:incoming").unwrap(), INCOMING);
        [FIRST, INCOMING]
    } else {
        [FIRST, KNOWN]
    };
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs(items)),
    )])
    .unwrap();
    DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap()
}

#[test]
fn duplicate_sequence_after_scope_expansion_poison_rolls_back_and_restore_retries() {
    let mut incoming = persistent_session(true);
    let bytes = incoming.save_document_json().unwrap();
    let mut session = persistent_session(false);

    // Seed a published watermark so rollback must preserve an existing sequence.
    let mut initial = session.begin_host_frame().unwrap();
    initial.select_item_current(FIRST).unwrap();
    initial
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    initial.commit().unwrap();

    let prepared = session
        .prepare_document_restore_json(&bytes, |document, key| {
            document == DOCUMENT && matches!(key, "pane:first" | "pane:known" | "pane:incoming")
        })
        .unwrap();
    let before_workspace = session.engine.workspace().clone();
    let before_version = session.version();
    let before_frontier = session.engine.presentation_identity_frontier();
    let before_tick = session.engine.last_reducer_tick();
    let before_input = session.engine.last_input_sequence();
    let before_watermark = session.engine.semantic_input_watermark();
    let before_source_sequence = session.committed_source_sequence;
    let before_document = session.document.as_ref().unwrap().clone();
    let source_scope = before_document.item_identity_scope();
    let target_scope = incoming.document.as_ref().unwrap().item_identity_scope();
    assert!(!source_scope.contains(&INCOMING));
    assert!(target_scope.is_superset(source_scope.as_ref()));
    assert!(target_scope.contains(&INCOMING));
    assert_eq!(
        before_watermark,
        Some(SourceSequence::new(before_source_sequence))
    );

    // Use the real document scopes and workspace without exposing prepared.inner.
    let mut prelude = session
        .engine
        .begin_host_frame(session.presentation_host)
        .unwrap();
    prelude.restrict_item_identity_scope(Arc::clone(&source_scope));
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .unwrap();
    let mut candidate = prelude.seal(&session.engine).unwrap();
    let sequence = SourceSequence::new(before_source_sequence + 1);
    candidate
        .append_input(
            APPLICATION_INPUT_SOURCE,
            sequence,
            EngineInput::SelectItem {
                expected: before_version,
                item: KNOWN,
            },
        )
        .unwrap();
    let prefix_version = candidate.view().version();
    assert_ne!(prefix_version, before_version);
    assert_ne!(candidate.view().workspace(), &before_workspace);

    let replacement = ValidatedWorkspaceRestore::new(
        incoming.engine.workspace().clone(),
        incoming.engine.presentation_identity_frontier(),
    );
    assert_eq!(
        candidate.append_document_replacement(
            APPLICATION_INPUT_SOURCE,
            sequence,
            EngineInput::RestoreWorkspace(replacement),
            source_scope.as_ref(),
            Arc::clone(&target_scope),
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed)
    );
    // This exact diagnostic proves append ran after the scope checks and expansion.
    assert!(matches!(
        candidate.input_prefix_error(),
        Some(EngineError::SourceSequenceNotIncreasing {
            input_source,
            previous,
            submitted,
        }) if *input_source == APPLICATION_INPUT_SOURCE
            && *previous == sequence
            && *submitted == sequence
    ));
    assert_eq!(
        candidate.append_input(
            APPLICATION_INPUT_SOURCE,
            SourceSequence::new(sequence.get() + 1),
            EngineInput::OpenItem {
                expected: prefix_version,
                item: INCOMING,
                placement: DockPlacement::After(FIRST),
            },
        ),
        Err(CoreHostFrameError::InputPrefixReductionFailed),
        "expanded scope must not authorize further input after a failed append"
    );
    assert!(matches!(
        candidate.finish(&mut session.engine),
        Err(EngineError::SourceSequenceNotIncreasing {
            input_source: APPLICATION_INPUT_SOURCE,
            previous,
            submitted,
        }) if previous == sequence && submitted == sequence
    ));

    assert_eq!(session.engine.workspace(), &before_workspace);
    assert_eq!(session.version(), before_version);
    assert_eq!(
        session.engine.presentation_identity_frontier(),
        before_frontier
    );
    assert_eq!(session.engine.last_reducer_tick(), before_tick);
    assert_eq!(session.engine.last_input_sequence(), before_input);
    assert_eq!(session.engine.semantic_input_watermark(), before_watermark);
    assert_eq!(session.committed_source_sequence, before_source_sequence);
    assert_eq!(session.document.as_ref(), Some(&before_document));
    assert_eq!(
        session.document.as_ref().unwrap().item_identity_scope(),
        source_scope
    );

    // A new runtime frame must still admit only the published document's keys.
    let mut ordinary = session.begin_host_frame().unwrap();
    assert_eq!(ordinary.external_key_for_item(INCOMING), None);
    let error = ordinary
        .open_item_current(INCOMING, DockPlacement::After(FIRST))
        .unwrap_err();
    let DockspaceRuntimeErrorSource::HostFrame(error) = error.source else {
        panic!("incoming identity must be rejected by the core item scope");
    };
    assert_eq!(
        *error,
        CoreHostFrameError::ItemIdentityOutsideScope { item: INCOMING }
    );
    drop(ordinary);
    assert_eq!(session.engine.workspace(), &before_workspace);
    assert_eq!(session.engine.semantic_input_watermark(), before_watermark);
    assert_eq!(session.committed_source_sequence, before_source_sequence);
    assert_eq!(session.document.as_ref(), Some(&before_document));

    // Retry through the normal runtime path, including document publication checks.
    let mut retry = session.begin_host_frame().unwrap();
    retry.submit_document_restore(&prepared).unwrap();
    assert!(retry.view().item(INCOMING).is_some());
    assert_eq!(retry.external_key_for_item(INCOMING), Some("pane:incoming"));
    assert_eq!(retry.session.engine.workspace(), &before_workspace);
    assert_eq!(retry.session.document.as_ref(), Some(&before_document));
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 900.0, 650.0).unwrap(),
        LogicalSize::new(48.0, 32.0).unwrap(),
        96.0,
    )
    .unwrap();
    retry.measure_surface(SURFACE, metrics).unwrap();
    let report = retry.commit().unwrap();

    assert_eq!(
        report.inputs(),
        &[HostInputOutcome::DocumentRestored {
            previous: before_version,
            current: session.version(),
        }]
    );
    assert!(session.view().item(INCOMING).is_some());
    assert!(session.view().item(KNOWN).is_none());
    assert_eq!(session.external_key_for_item(KNOWN), Some("pane:known"));
    assert_eq!(
        session.external_key_for_item(INCOMING),
        Some("pane:incoming")
    );
    assert_eq!(
        session.next_document_generation(),
        incoming.next_document_generation()
    );
    assert_eq!(
        session.document.as_ref().unwrap().item_identity_scope(),
        target_scope
    );
    assert_eq!(session.engine.semantic_input_watermark(), Some(sequence));
    assert_eq!(session.committed_source_sequence, sequence.get());
    assert_eq!(
        session.engine.last_input_sequence().get(),
        before_input.get() + 1
    );
}
