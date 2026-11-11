use super::*;
use crate::graph::{Axis, Node, RootRecord, SurfacePresentation};
use crate::ids::{ItemId, NodeId, RootId, SurfaceId};
use slotmap::Key as _;

struct Fixture {
    engine: DockEngine,
    root: RootId,
    split: NodeId,
    tabs: NodeId,
    other_root: RootId,
    other_tabs: NodeId,
    empty_root: RootId,
    empty_tabs: NodeId,
}

fn fixture() -> Fixture {
    let root = RootId::new(1);
    let other_root = RootId::new(2);
    let empty_root = RootId::new(3);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let right = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [tabs, right]).unwrap());
    let other_tabs = builder.insert_node(Node::tabs([ItemId::new(4)]));
    let empty_tabs = builder.insert_node(Node::tabs([]));
    builder.set_root(root, RootRecord::new(split));
    builder.set_root(other_root, RootRecord::new(other_tabs));
    builder.set_root(
        empty_root,
        RootRecord::new(empty_tabs).with_central(empty_tabs),
    );
    builder.set_surface(SurfaceId::new(1), SurfacePresentation::with_main(root));
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(other_root),
    );
    builder.set_surface(
        SurfaceId::new(3),
        SurfacePresentation::with_main(empty_root),
    );
    Fixture {
        engine: DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap(),
        root,
        split,
        tabs,
        other_root,
        other_tabs,
        empty_root,
        empty_tabs,
    }
}

type PreparedFacts = (SurfaceId, Option<NodeSource>, bool);

fn prepare(
    engine: &DockEngine,
    payload: &MovePayload,
) -> Result<PreparedFacts, InteractionRejection> {
    engine.prepare_drag_source(payload).map(|prepared| {
        (
            prepared.source_surface,
            prepared.complete_root,
            prepared.partial_detachable,
        )
    })
}

// Preserve the original live preparation sequence independently of the index gate.
fn prepare_live(
    engine: &DockEngine,
    payload: &MovePayload,
) -> Result<PreparedFacts, InteractionRejection> {
    engine
        .validate_payload(payload)
        .map_err(InteractionRejection::CommandRejected)?;
    let complete_root = engine
        .complete_root_source(payload)
        .map_err(InteractionRejection::CommandRejected)?;
    let partial_detachable =
        complete_root.is_some() || engine.partial_payload_is_detachable(payload);
    let surface = engine
        .payload_surface(payload)
        .ok_or(InteractionRejection::CommandRejected(
            CommandError::Invariant {
                stage: "freeze drag source surface",
            },
        ))?;
    Ok((surface, complete_root, partial_detachable))
}

#[test]
fn indexed_drag_source_preparation_matches_live_payload_rejections() {
    let fixture = fixture();
    let engine = &fixture.engine;
    let item = engine
        .workspace
        .capture_item_source(fixture.root, fixture.tabs, ItemId::new(1))
        .unwrap();
    let indexed = engine
        .presentation_authority
        .workspace_index_for(engine.workspace.as_arc(), engine.version)
        .unwrap()
        .capture_item_source(
            &engine.workspace,
            engine.version,
            fixture.root,
            fixture.tabs,
            ItemId::new(1),
        )
        .unwrap();
    assert_eq!(item, indexed);
    assert_eq!(item.reference(), indexed.reference());
    let node = engine
        .workspace
        .capture_node_source(fixture.root, fixture.tabs)
        .unwrap();
    let subtree = engine
        .workspace
        .capture_node_source(fixture.root, fixture.split)
        .unwrap();
    let singleton = engine
        .workspace
        .capture_item_source(fixture.other_root, fixture.other_tabs, ItemId::new(4))
        .unwrap();
    let empty = engine
        .workspace
        .capture_node_source(fixture.empty_root, fixture.empty_tabs)
        .unwrap();
    let payloads = [
        MovePayload::Item(item.clone()),
        MovePayload::Tabs(node),
        MovePayload::Subtree(subtree.clone()),
        MovePayload::Item(singleton.clone()),
        MovePayload::Item(crate::command::ItemSource {
            item: ItemId::new(99),
            ..item.clone()
        }),
        MovePayload::Item(crate::command::ItemSource {
            root: RootId::new(99),
            ..item.clone()
        }),
        MovePayload::Item(crate::command::ItemSource {
            tabs: NodeId::null(),
            ..item.clone()
        }),
        MovePayload::Item(crate::command::ItemSource {
            tabs: fixture.other_tabs,
            ..item.clone()
        }),
        MovePayload::Item(crate::command::ItemSource {
            reference: singleton.reference,
            ..item.clone()
        }),
        MovePayload::Item(crate::command::ItemSource {
            tabs: fixture.split,
            ..item
        }),
        MovePayload::Tabs(subtree),
        MovePayload::Tabs(empty.clone()),
        MovePayload::Subtree(empty),
    ];
    for payload in payloads {
        assert_eq!(
            prepare(engine, &payload),
            prepare_live(engine, &payload),
            "{payload:?}"
        );
    }
}

#[test]
fn indexed_drag_source_preparation_reuses_current_root_fingerprints() {
    let fixture = fixture();
    let engine = &fixture.engine;
    let payloads = [
        MovePayload::Item(
            engine
                .workspace
                .capture_item_source(fixture.root, fixture.tabs, ItemId::new(1))
                .unwrap(),
        ),
        MovePayload::Tabs(
            engine
                .workspace
                .capture_node_source(fixture.root, fixture.tabs)
                .unwrap(),
        ),
        MovePayload::Subtree(
            engine
                .workspace
                .capture_node_source(fixture.root, fixture.split)
                .unwrap(),
        ),
        MovePayload::Item(
            engine
                .workspace
                .capture_item_source(fixture.other_root, fixture.other_tabs, ItemId::new(4))
                .unwrap(),
        ),
    ];
    crate::drop_resolver::structural_work::reset();
    PARTIAL_DETACHABILITY_EVALUATIONS.with(|evaluations| evaluations.set(0));
    for _ in 0..64 {
        for payload in &payloads {
            prepare(engine, payload).unwrap();
        }
    }
    let work = crate::drop_resolver::structural_work::snapshot();
    assert_eq!(work.root_fingerprint_builds, 0);
    assert_eq!(work.root_fingerprint_node_visits, 0);
    assert_eq!(
        PARTIAL_DETACHABILITY_EVALUATIONS.with(std::cell::Cell::get),
        128
    );
}

#[test]
fn published_workspace_before_revision_uses_live_drag_source_validation() {
    let mut fixture = fixture();
    let engine = &mut fixture.engine;
    let payload = MovePayload::Item(
        engine
            .workspace
            .capture_item_source(fixture.root, fixture.tabs, ItemId::new(1))
            .unwrap(),
    );
    let previous_version = engine.version;
    let previous_workspace = Arc::downgrade(engine.workspace.as_arc());
    let mut candidate = engine.workspace.as_ref().clone();
    let source = candidate
        .capture_item_source(fixture.root, fixture.tabs, ItemId::new(2))
        .unwrap();
    crate::transaction::WorkspaceTransaction::from_commands([WorkspaceCommand::Select { source }])
        .apply(&mut candidate, &engine.policy)
        .unwrap();
    let publication = engine
        .stage_workspace_publication(candidate, &engine.policy)
        .unwrap();
    engine.publish_workspace(publication);
    assert_eq!(engine.version, previous_version);
    assert!(previous_workspace.upgrade().is_none());
    assert!(
        engine
            .presentation_authority
            .workspace_index_for(engine.workspace.as_arc(), engine.version)
            .is_none()
    );
    crate::drop_resolver::structural_work::reset();
    assert!(matches!(
        prepare(engine, &payload),
        Err(InteractionRejection::CommandRejected(
            CommandError::StaleNode { .. }
        ))
    ));
    assert_eq!(
        crate::drop_resolver::structural_work::snapshot().root_fingerprint_builds,
        0
    );
    assert_eq!(prepare(engine, &payload), prepare_live(engine, &payload));

    let current = MovePayload::Item(
        engine
            .workspace
            .capture_item_source(fixture.root, fixture.tabs, ItemId::new(2))
            .unwrap(),
    );
    assert_eq!(prepare(engine, &current), prepare_live(engine, &current));
    engine.advance_revision(InputSequence::default()).unwrap();
    assert!(
        engine
            .presentation_authority
            .workspace_index_for(engine.workspace.as_arc(), engine.version)
            .is_some()
    );
    crate::drop_resolver::structural_work::reset();
    prepare(engine, &current).unwrap();
    assert_eq!(
        crate::drop_resolver::structural_work::snapshot().root_fingerprint_builds,
        0
    );
    assert!(matches!(
        prepare(engine, &payload),
        Err(InteractionRejection::CommandRejected(
            CommandError::StaleNode { .. }
        ))
    ));
}

#[test]
fn workspace_index_source_or_version_mismatch_keeps_live_drag_source_validation() {
    for replace_source in [false, true] {
        let mut fixture = fixture();
        let engine = &mut fixture.engine;
        let payload = MovePayload::Item(
            engine
                .workspace
                .capture_item_source(fixture.root, fixture.tabs, ItemId::new(1))
                .unwrap(),
        );
        if replace_source {
            let previous = Arc::downgrade(engine.workspace.as_arc());
            engine.workspace = engine.workspace.as_ref().clone().into();
            assert!(previous.upgrade().is_none());
        } else {
            engine.version = WorkspaceVersion::new(
                engine.version.epoch(),
                engine.version.revision().checked_next().unwrap(),
            );
        }
        assert!(
            engine
                .presentation_authority
                .workspace_index_for(engine.workspace.as_arc(), engine.version)
                .is_none()
        );
        crate::drop_resolver::structural_work::reset();
        prepare(engine, &payload).unwrap();
        assert_eq!(
            crate::drop_resolver::structural_work::snapshot().root_fingerprint_builds,
            0
        );
        assert_eq!(prepare(engine, &payload), prepare_live(engine, &payload));
    }
}
