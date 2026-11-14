use super::*;
use crate::graph::Axis;
use crate::policy::DockPolicy;
use crate::presentation::manifest::PolicyRevision;
use crate::presentation::scene::SurfaceProgramBlock;

const SURFACE: SurfaceId = SurfaceId::new(1);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn splitter(
    id: SplitterSceneId,
    axis: Axis,
    draw: LogicalRect,
    hit: LogicalRect,
    layer: SceneLayerKey,
    operable: bool,
) -> SplitterRecord {
    SplitterRecord::new(
        id,
        draw,
        HitRegion::new(hit),
        axis,
        draw,
        draw,
        0.0,
        0.0,
        1024.0,
        1024.0,
        vec![100.0, 100.0],
        None,
        SplitWeight::normalize([1.0, 1.0]).unwrap(),
        layer,
        operable,
    )
}

fn draft(roots: &[RootId], allowed: [bool; 2]) -> PresentationPlan {
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(crate::model::DockspaceAxis::Horizontal, allowed[0]);
    policy.set_allow_resize_axis(crate::model::DockspaceAxis::Vertical, allowed[1]);
    let policy = policy.snapshot(PolicyRevision::new(1));
    let mut ready = PresentationPlan::new(SURFACE, rect(0.0, 0.0, 1024.0, 1024.0));
    let mut nodes = Workspace::builder();
    for (ordinal, &root) in roots.iter().enumerate() {
        let presentation = if ordinal == 0 {
            SurfaceProgramRootPresentation::Main
        } else {
            SurfaceProgramRootPresentation::Contained(FloatingPresentationId::new(1))
        };
        ready
            .begin_surface_program_root(root, presentation)
            .unwrap();
        let layer = SceneLayerKey::new(u64::try_from(ordinal).unwrap() + 1);
        let ids = [0, 1, 2].map(|_| SplitterSceneId {
            root,
            split: nodes.insert_node(Node::tabs([])),
            index: 0,
        });
        let horizontal = policy.splitter_resize_is_allowed(Axis::Horizontal, SURFACE);
        let vertical = policy.splitter_resize_is_allowed(Axis::Vertical, SURFACE);
        // Two distinct north/south handles meet one through east/west handle.
        let records = [
            splitter(
                ids[0],
                Axis::Horizontal,
                rect(98.0, 0.0, 4.0, 100.0),
                rect(94.0, 0.0, 12.0, 104.0),
                layer,
                horizontal,
            ),
            splitter(
                ids[1],
                Axis::Horizontal,
                rect(98.0, 100.0, 4.0, 100.0),
                rect(94.0, 96.0, 12.0, 104.0),
                layer,
                horizontal,
            ),
            splitter(
                ids[2],
                Axis::Vertical,
                rect(0.0, 98.0, 200.0, 4.0),
                rect(0.0, 94.0, 200.0, 12.0),
                layer,
                vertical,
            ),
        ];
        // Construction order is intentionally different from ID order.
        for record in records.into_iter().rev() {
            ready
                .push_surface_program_root_operation(SurfaceProgramRootOperation::Splitter(
                    *record.id(),
                ))
                .unwrap();
            ready.push_splitter_record(record);
        }
        ready.end_surface_program_root(root).unwrap();
    }
    ready
}

// Freeze the original function, including first-match lookup and mutation order.
fn frozen_compile_splitter_junctions(
    ready: &mut PresentationPlan,
) -> Result<(), SceneCompilationError> {
    let candidates = derive_splitter_junction_candidates(ready.splitter_records())?;
    for candidate in candidates {
        if ready.region_is_operable(candidate.hit, candidate.layer) {
            let operable = candidate.id().splitters().into_iter().all(|splitter| {
                ready
                    .splitter_record(splitter)
                    .is_some_and(SplitterRecord::operable)
            });
            let id = candidate.id();
            ready.push_splitter_junction_record(SplitterJunctionRecord::new(
                id,
                HitRegion::new(candidate.hit),
                candidate.layer,
                operable,
            ));
            ready.push_surface_program_derived_root_operation(
                id.root(),
                SurfaceProgramRootOperation::SplitterJunction(id),
            )?;
        }
    }
    Ok(())
}

fn compare(mut actual: PresentationPlan) -> (Result<(), SceneCompilationError>, PresentationPlan) {
    let mut expected = actual.clone();
    let expected_result = frozen_compile_splitter_junctions(&mut expected);
    let actual_result = compile_splitter_junctions(&mut actual);
    assert_eq!(actual_result, expected_result);
    assert_eq!(
        actual.splitter_junction_records(),
        expected.splitter_junction_records()
    );
    assert_eq!(
        actual, expected,
        "draft operations and partial failures stay identical"
    );
    if actual_result.is_ok() {
        assert_eq!(
            actual.finish_surface_program(),
            expected.finish_surface_program()
        );
        assert_eq!(
            actual.surface_program().blocks(),
            expected.surface_program().blocks()
        );
    }
    (actual_result, actual)
}

#[test]
fn junction_compile_preserves_multi_root_order_and_axis_policy() {
    let roots = [RootId::new(30), RootId::new(10)];
    for allowed in [[true, true], [false, true], [true, false], [false, false]] {
        let (result, ready) = compare(draft(&roots, allowed));
        result.unwrap();
        assert_eq!(ready.splitter_junction_records().len(), 2);
        assert!(
            ready
                .splitter_junction_records()
                .iter()
                .all(|record| record.operable() == (allowed[0] && allowed[1]))
        );
        assert_eq!(ready.splitter_junction_records()[0].id().root(), roots[1]);
        let blocks = ready.surface_program().blocks();
        let root_order = blocks
            .iter()
            .filter_map(|block| match block {
                SurfaceProgramBlock::RootBegin { root, .. } => Some(*root),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(root_order, roots);
        assert_eq!(
            blocks
                .iter()
                .filter(|block| matches!(
                    block,
                    SurfaceProgramBlock::RootOperation(
                        SurfaceProgramRootOperation::SplitterJunction(_)
                    )
                ))
                .count(),
            2
        );
    }
}

#[test]
fn duplicate_splitter_ids_keep_the_first_operability_answer() {
    let root = RootId::new(2);
    let enabled = draft(&[root], [true, true]);
    let disabled = draft(&[root], [false, true]);
    let target = *enabled.splitter_records()[1].id();
    for first_enabled in [false, true] {
        let mut ready = PresentationPlan::new(SURFACE, enabled.bounds());
        ready
            .begin_surface_program_root(root, SurfaceProgramRootPresentation::Main)
            .unwrap();
        for (index, record) in enabled.splitter_records().iter().enumerate() {
            let record = if *record.id() == target && !first_enabled {
                &disabled.splitter_records()[index]
            } else {
                record
            };
            ready.push_splitter_record(record.clone());
            ready
                .push_surface_program_root_operation(SurfaceProgramRootOperation::Splitter(
                    *record.id(),
                ))
                .unwrap();
        }
        let duplicate = if first_enabled {
            &disabled.splitter_records()[1]
        } else {
            &enabled.splitter_records()[1]
        };
        assert_eq!(*duplicate.id(), target);
        ready.push_splitter_record(duplicate.clone());
        ready.end_surface_program_root(root).unwrap();
        let (result, ready) = compare(ready);
        result.unwrap();
        assert_eq!(ready.splitter_junction_records().len(), 1);
        assert_eq!(
            ready.splitter_junction_records()[0].operable(),
            first_enabled
        );
    }
}

#[test]
fn absent_or_finished_root_program_preserves_failure_and_partial_append() {
    let root = RootId::new(2);
    let template = draft(&[root], [true, true]);
    let mut missing_root = PresentationPlan::new(SURFACE, template.bounds());
    for record in template.splitter_records() {
        missing_root.push_splitter_record(record.clone());
    }
    let mut finished = template.clone();
    finished.finish_surface_program().unwrap();
    for ready in [missing_root, finished] {
        let (result, ready) = compare(ready);
        assert_eq!(result, Err(SceneCompilationError::SurfaceProgramInvariant));
        assert_eq!(ready.splitter_junction_records().len(), 1);
    }
    let mut empty = PresentationPlan::new(SURFACE, template.bounds());
    empty.finish_surface_program().unwrap();
    let (result, ready) = compare(empty);
    result.expect("no candidates require no program mutation");
    assert!(ready.splitter_junction_records().is_empty());
}
