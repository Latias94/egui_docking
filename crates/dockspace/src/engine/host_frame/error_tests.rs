//! Reduction failures retain one typed cause across poisoning and facade conversion.

use std::error::Error;

use super::*;
use crate::graph::{Node, RootRecord, SurfacePresentation};
use crate::ids::{ItemId, RootId, SurfaceId};
use crate::runtime::{DockspaceRuntimeError, DockspaceRuntimeErrorKind};

const SOURCE: StableInputSourceId = StableInputSourceId::new(0xe220);

fn fixture() -> (DockEngine, PresentationHostLease) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
    builder.set_root(RootId::new(1), RootRecord::new(tabs));
    builder.set_surface(
        SurfaceId::new(1),
        SurfacePresentation::with_main(RootId::new(1)),
    );
    let mut engine = DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap();
    let host = engine.create_presentation_host().unwrap();
    (engine, host)
}

fn begin(engine: &DockEngine, host: PresentationHostLease) -> CoreHostFrame {
    let mut prelude = engine.begin_host_frame(host).unwrap();
    prelude
        .submit_presentation_observation(HostPresentationObservation::NoUpdate)
        .unwrap();
    prelude.seal(engine).unwrap()
}

fn assert_runtime_source(
    error: CoreHostFrameError,
    expected: &EngineError,
    kind: DockspaceRuntimeErrorKind,
) {
    let runtime = DockspaceRuntimeError::from(error);
    assert_eq!(runtime.kind(), kind);
    let mut source = runtime.source();
    while let Some(current) = source {
        let engine = current
            .downcast_ref::<EngineError>()
            .or_else(|| current.downcast_ref::<Box<EngineError>>().map(Box::as_ref));
        if let Some(engine) = engine {
            assert_eq!(engine, expected);
            assert!(runtime.to_string().contains(&engine.to_string()));
            return;
        }
        source = current.source();
    }
    panic!("the public runtime error lost its typed engine cause: {runtime:?}");
}

#[test]
fn input_prefix_failure_retains_its_typed_source_and_first_poison() {
    let (mut engine, host) = fixture();
    let before = engine.candidate();
    let sequence = SourceSequence::new(1);
    let mut frame = begin(&engine, host);
    frame
        .append_input(SOURCE, sequence, EngineInput::ValidateWorkspace)
        .unwrap();
    let expected = EngineError::SourceSequenceNotIncreasing {
        input_source: SOURCE,
        previous: sequence,
        submitted: sequence,
    };
    let error = frame
        .append_input(SOURCE, sequence, EngineInput::ValidateWorkspace)
        .unwrap_err();
    assert_eq!(frame.input_prefix_error(), Some(&expected));
    assert_runtime_source(
        error.clone(),
        &expected,
        DockspaceRuntimeErrorKind::HostProtocol,
    );
    assert_eq!(
        frame.append_input(
            SOURCE,
            SourceSequence::new(2),
            EngineInput::ValidateWorkspace
        ),
        Err(error.clone()),
        "a later operation must retain the first rejection, without advancing the candidate"
    );
    assert_eq!(frame.begin_presentation_phase(), Err(error));
    assert_eq!(frame.finish(&mut engine), Err(expected));
    assert_eq!(engine, before);

    let mut retry = begin(&engine, host);
    retry
        .append_input(SOURCE, sequence, EngineInput::ValidateWorkspace)
        .unwrap();
    assert_eq!(retry.view().version(), engine.version());
}

#[test]
fn exhausted_input_source_survives_presentation_entry_and_candidate_abandonment() {
    let (engine, host) = fixture();
    let before = engine.candidate();
    let mut frame = begin(&engine, host);
    frame.candidate.last_input = InputSequence::new(u64::MAX);
    let error = frame
        .append_input(
            SOURCE,
            SourceSequence::new(1),
            EngineInput::ValidateWorkspace,
        )
        .unwrap_err();
    assert_eq!(
        frame.input_prefix_error(),
        Some(&EngineError::InputSequenceExhausted)
    );
    assert_runtime_source(
        error.clone(),
        &EngineError::InputSequenceExhausted,
        DockspaceRuntimeErrorKind::Internal,
    );
    assert_eq!(frame.refresh_presentation_layout(), Err(error.clone()));
    assert_eq!(frame.into_presentation().unwrap_err(), error);
    assert_eq!(engine, before);
    assert!(begin(&engine, host).input_prefix_error().is_none());
}
