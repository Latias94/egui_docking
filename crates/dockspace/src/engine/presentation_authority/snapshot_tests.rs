use super::*;
use crate::drop_resolver::structural_work;
use crate::geometry::LogicalRect;
use crate::graph::{ContainedFloating, RootRecord, SurfacePresentation};
use crate::ids::{FloatingPresentationId, ItemId, RootId};

const SURFACE: SurfaceId = SurfaceId::new(1);
const EXTRA: SurfaceId = SurfaceId::new(99);

fn fixture() -> DockEngine {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    let contained = builder.insert_node(Node::tabs([ItemId::new(3)]));
    let other = builder.insert_node(Node::tabs([ItemId::new(4)]));
    builder.set_root(RootId::new(1), RootRecord::new(tabs));
    builder.set_root(RootId::new(2), RootRecord::new(contained));
    builder.set_root(RootId::new(3), RootRecord::new(other));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(RootId::new(1)));
    builder.set_contained_floating(
        FloatingPresentationId::new(1),
        ContainedFloating::new(
            RootId::new(2),
            LogicalRect::new(10.0, 20.0, 300.0, 200.0).unwrap(),
        ),
    );
    builder
        .attach_contained(SURFACE, FloatingPresentationId::new(1))
        .unwrap();
    builder.set_surface(
        SurfaceId::new(2),
        SurfacePresentation::with_main(RootId::new(3)),
    );
    DockEngine::new(builder.build().unwrap(), DockPolicy::default()).unwrap()
}

#[test]
fn exact_snapshot_reuses_live_semantics_and_excludes_prospective_surfaces() {
    let mut engine = fixture();
    let expected = DockEngine::capture_surface_semantic_snapshots(engine.workspace()).unwrap();
    let extra = expected[&SURFACE].clone();
    engine
        .presentation_authority
        .surface_semantic_snapshots
        .insert(EXTRA, extra);
    structural_work::reset();
    let actual = engine
        .presentation_authority
        .surface_semantic_snapshots_for(engine.workspace.as_arc(), engine.version)
        .unwrap();
    assert_eq!(structural_work::snapshot().root_fingerprint_builds, 0);
    assert_eq!(actual, expected);
    assert!(!actual.contains_key(&EXTRA));
    let original = engine.presentation_authority.surface_semantic_snapshots[&SURFACE]
        .main
        .as_ref()
        .unwrap();
    let cached = actual[&SURFACE].main.as_ref().unwrap();
    assert!(Arc::ptr_eq(&original.0, &cached.0));
    engine
        .presentation_authority
        .surface_semantic_snapshots
        .remove(&SurfaceId::new(2));
    assert!(
        engine
            .presentation_authority
            .surface_semantic_snapshots_for(engine.workspace.as_arc(), engine.version)
            .is_none()
    );
    structural_work::reset();
    engine.try_rebuild_presentation_requirements(None).unwrap();
    assert!(structural_work::snapshot().root_fingerprint_builds > 0);
    assert_eq!(
        engine.presentation_authority.surface_semantic_snapshots,
        expected
    );
}

#[test]
fn snapshot_source_version_and_same_version_mutation_keep_live_fallback() {
    let mut engine = fixture();
    let changed_version = WorkspaceVersion::new(
        engine.version.epoch(),
        engine.version.revision().checked_next().unwrap(),
    );
    assert!(
        engine
            .presentation_authority
            .surface_semantic_snapshots_for(engine.workspace.as_arc(), changed_version)
            .is_none()
    );
    let mut candidate = engine.candidate();
    let tabs = candidate
        .workspace
        .capture_item_source_by_id(ItemId::new(2))
        .unwrap()
        .unwrap()
        .tabs();
    let Node::Tabs { selected, .. } = candidate.workspace.draft_mut().nodes.get_mut(tabs).unwrap()
    else {
        unreachable!()
    };
    *selected = Some(ItemId::new(2));
    candidate
        .workspace
        .draft_mut()
        .tab_mru
        .insert(tabs, vec![ItemId::new(2), ItemId::new(1)]);
    assert!(
        candidate
            .presentation_authority
            .surface_semantic_snapshots_for(candidate.workspace.as_arc(), candidate.version)
            .is_none()
    );
    assert!(
        engine
            .presentation_authority
            .surface_semantic_snapshots_for(engine.workspace.as_arc(), engine.version)
            .is_some()
    );
    structural_work::reset();
    candidate
        .try_rebuild_presentation_requirements(None)
        .unwrap();
    assert!(structural_work::snapshot().root_fingerprint_builds > 0);
    let expected = DockEngine::capture_surface_semantic_snapshots(candidate.workspace()).unwrap();
    structural_work::reset();
    assert_eq!(
        candidate
            .presentation_authority
            .surface_semantic_snapshots_for(candidate.workspace.as_arc(), candidate.version)
            .unwrap(),
        expected
    );
    assert_eq!(structural_work::snapshot().root_fingerprint_builds, 0);
    assert_ne!(candidate.workspace(), engine.workspace());
    engine.workspace = engine.workspace.as_ref().clone().into();
    assert!(
        engine
            .presentation_authority
            .surface_semantic_snapshots_for(engine.workspace.as_arc(), engine.version)
            .is_none()
    );
}
