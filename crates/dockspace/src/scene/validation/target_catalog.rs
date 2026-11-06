//! One sorted row inventory for duplicate checks, target joins, and final order.

use std::sync::Arc;

use crate::drop_target::{DropTargetId, DropTargetKind, DropTargetRecord};

pub(super) struct DropTargetCatalog<'a> {
    targets: &'a [Arc<DropTargetRecord>],
    entries: Vec<(DropTargetId, usize)>,
}

pub(super) struct DropTargetOrder(Vec<usize>);

// Mark the already joined catalog rows instead of sorting a second inventory
// of large identities. Repeated expected identities are naturally idempotent.
pub(super) struct MagneticTargetCoverage<'catalog, 'targets> {
    catalog: &'catalog DropTargetCatalog<'targets>,
    expected: Vec<bool>,
}

impl<'a> DropTargetCatalog<'a> {
    pub(super) fn new(
        targets: &'a [Arc<DropTargetRecord>],
        background: Option<&DropTargetRecord>,
    ) -> Result<Self, DropTargetId> {
        let mut entries = targets
            .iter()
            .enumerate()
            .map(|(row, target)| (target.id(), row))
            .collect::<Vec<_>>();
        entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
        let duplicate = entries.windows(2).find_map(|pair| {
            let first = pair[0].0;
            (first == pair[1].0).then_some(first)
        });
        let background_duplicate = background.and_then(|background| {
            let id = background.id();
            entries
                .binary_search_by(|(key, _)| key.cmp(&id))
                .ok()
                .map(|_| id)
        });
        // The old combined catalog reported the smallest duplicate, including
        // collisions with the separate rootless-background record.
        if let Some(id) = duplicate.into_iter().chain(background_duplicate).min() {
            return Err(id);
        }
        Ok(Self { targets, entries })
    }

    #[cfg(test)]
    fn get(&self, id: &DropTargetId) -> Option<&'a Arc<DropTargetRecord>> {
        self.entry_index(id)
            .map(|index| &self.targets[self.entries[index].1])
    }

    fn entry_index(&self, id: &DropTargetId) -> Option<usize> {
        self.entries.binary_search_by(|(key, _)| key.cmp(id)).ok()
    }

    #[cfg(test)]
    fn keys(&self) -> impl ExactSizeIterator<Item = DropTargetId> + '_ {
        self.entries.iter().map(|(id, _)| *id)
    }

    pub(super) fn into_order(self) -> DropTargetOrder {
        DropTargetOrder(self.entries.into_iter().map(|(_, row)| row).collect())
    }
}

impl<'catalog, 'targets> MagneticTargetCoverage<'catalog, 'targets> {
    pub(super) fn new(catalog: &'catalog DropTargetCatalog<'targets>) -> Self {
        Self {
            catalog,
            expected: vec![false; catalog.entries.len()],
        }
    }

    pub(super) fn get(&mut self, id: &DropTargetId) -> Option<&'targets Arc<DropTargetRecord>> {
        let index = self.catalog.entry_index(id)?;
        self.expected[index] = true;
        Some(&self.catalog.targets[self.catalog.entries[index].1])
    }

    pub(super) fn is_complete(self) -> bool {
        self.catalog
            .entries
            .iter()
            .zip(self.expected)
            .all(|((id, _), expected)| {
                expected
                    == matches!(
                        id.kind(),
                        DropTargetKind::Center
                            | DropTargetKind::InnerEdge
                            | DropTargetKind::OuterEdge
                    )
            })
    }
}

impl DropTargetOrder {
    pub(super) fn apply(mut self, targets: &mut [Arc<DropTargetRecord>]) {
        debug_assert_eq!(targets.len(), self.0.len());
        // Until this final permutation, validators observe the original input
        // order and guide targets keep the exact original catalog Arc owners.
        for start in 0..self.0.len() {
            let mut current = start;
            while self.0[current] != start {
                let source = self.0[current];
                targets.swap(current, source);
                self.0[current] = current;
                current = source;
            }
            self.0[current] = current;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    use super::super::{
        PresentationPlanValidator, SceneWorkspaceIndex, validate_drop_target,
        validate_semantic_uniqueness,
    };
    use crate::command::{DockFraction, Edge};
    use crate::drop_guide::{DropGuideClusterRecord, DropGuideEdgeSet, DropGuideTargetRecord};
    use crate::drop_target::{
        DropTargetAvailability, DropTargetLocation, DropVisual, SceneLayerKey, SurfaceBackground,
    };
    use crate::geometry::LogicalRect;
    use crate::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use crate::hit_region::HitRegion;
    use crate::ids::{ItemId, NodeId, RootId, SurfaceId};
    use crate::policy::DockPolicySnapshot;
    use crate::scene::{PaneRecord, PaneSceneId, PresentationPlan, SceneBuildError};

    const SURFACE: SurfaceId = SurfaceId::new(1);
    const ROOT: RootId = RootId::new(1);

    fn bounds() -> LogicalRect {
        LogicalRect::new(0.0, 0.0, 200.0, 200.0).unwrap()
    }

    fn fixture() -> (Workspace, NodeId, Vec<Arc<DropTargetRecord>>) {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([ItemId::new(1)]));
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().unwrap();
        let center = DropTargetLocation::Center {
            surface: SURFACE,
            root: ROOT,
            tabs,
        };
        let edge = DropTargetLocation::InnerEdge {
            surface: SURFACE,
            root: ROOT,
            node: tabs,
            edge: Edge::Left,
            fraction: DockFraction::new(0.3).unwrap(),
        };
        let gap = DropTargetLocation::TabGap {
            surface: SURFACE,
            root: ROOT,
            tabs,
            index: 0,
        };
        let targets = [center, edge, gap]
            .into_iter()
            .map(|location| {
                Arc::new(DropTargetRecord::new(
                    location,
                    DropTargetAvailability::Available,
                    HitRegion::new(bounds()),
                    SceneLayerKey::surface_base(),
                    DropVisual::new(bounds()),
                ))
            })
            .collect();
        (workspace, tabs, targets)
    }

    fn background(surface: u64) -> DropTargetRecord {
        DropTargetRecord::surface_background(
            SurfaceBackground::new(SurfaceId::new(surface)),
            HitRegion::new(bounds()),
            DropVisual::new(bounds()),
        )
    }

    fn validate(
        workspace: &Workspace,
        plan: PresentationPlan,
    ) -> Result<PresentationPlan, SceneBuildError> {
        PresentationPlanValidator::new(workspace, &DockPolicySnapshot::default())
            .unwrap()
            .validate_and_canonicalize(plan)
    }

    #[test]
    fn row_index_and_final_order_match_the_old_btree_catalog_without_replacing_owners() {
        let (workspace, _, owners) = fixture();
        for reversed in [false, true] {
            for rotation in 0..owners.len() {
                let mut targets = owners.clone();
                targets.rotate_left(rotation);
                if reversed {
                    targets.reverse();
                }
                let oracle = targets
                    .iter()
                    .map(|target| (target.id(), target))
                    .collect::<BTreeMap<_, _>>();
                let separate_background = background(9);
                let catalog = DropTargetCatalog::new(&targets, Some(&separate_background)).unwrap();
                assert_eq!(
                    catalog.keys().collect::<Vec<_>>(),
                    oracle.keys().copied().collect::<Vec<_>>()
                );
                for (id, owner) in &oracle {
                    assert!(Arc::ptr_eq(catalog.get(id).unwrap(), owner));
                }
                assert!(catalog.get(&separate_background.id()).is_none());
                let expected = oracle
                    .values()
                    .map(|owner| Arc::clone(owner))
                    .collect::<Vec<_>>();
                let order = catalog.into_order();
                order.apply(&mut targets);
                assert!(
                    targets
                        .iter()
                        .zip(&expected)
                        .all(|(left, right)| Arc::ptr_eq(left, right))
                );

                let mut plan = PresentationPlan::new(SURFACE, bounds());
                for owner in owners.iter().cycle().skip(rotation).take(owners.len()) {
                    plan.push_drop_target(Arc::clone(owner));
                }
                let canonical = validate(&workspace, plan).unwrap();
                assert_eq!(canonical.drop_targets().len(), expected.len());
                assert!(
                    canonical
                        .drop_targets()
                        .iter()
                        .zip(&expected)
                        .all(|(left, right)| Arc::ptr_eq(left, right))
                );
            }
        }
        let mut empty = Vec::new();
        DropTargetCatalog::new(&empty, None)
            .unwrap()
            .into_order()
            .apply(&mut empty);
    }

    #[test]
    fn magnetic_row_coverage_matches_sorted_deduplicated_identity_sets() {
        let (_, _, mut owners) = fixture();
        owners.push(Arc::new(background(1)));
        let ids = owners.iter().map(|owner| owner.id()).collect::<Vec<_>>();
        for count in 0..=owners.len() {
            let catalog = DropTargetCatalog::new(&owners[..count], None).unwrap();
            let actual = catalog
                .keys()
                .filter(|id| {
                    matches!(
                        id.kind(),
                        DropTargetKind::Center
                            | DropTargetKind::InnerEdge
                            | DropTargetKind::OuterEdge
                    )
                })
                .collect::<Vec<_>>();
            for mask in 0..(1 << ids.len()) {
                let mut expected = ids
                    .iter()
                    .enumerate()
                    .filter(|(index, _)| mask & (1 << index) != 0)
                    .map(|(_, &id)| id)
                    .collect::<Vec<_>>();
                for repeated in [false, true] {
                    if repeated {
                        expected.extend(expected.clone());
                        expected.reverse();
                    }
                    let mut oracle = expected.clone();
                    oracle.sort_unstable();
                    oracle.dedup();
                    let mut coverage = MagneticTargetCoverage::new(&catalog);
                    let present = expected.iter().all(|id| coverage.get(id).is_some());
                    assert_eq!(present && coverage.is_complete(), actual == oracle);
                }
            }
        }
    }

    #[test]
    fn smallest_combined_background_duplicate_matches_the_frozen_sorted_id_oracle() {
        let (_, _, mut owners) = fixture();
        owners.push(Arc::new(background(1)));
        owners.push(Arc::new(background(9)));
        for pattern in [
            [3, 3, 2, 2],
            [3, 3, 1, 0],
            [1, 1, 0, 0],
            [0, 1, 2, 3],
            [3, 3, 4, 0],
            [4, 4, 3, 0],
        ] {
            let targets = pattern.map(|index| Arc::clone(&owners[index]));
            for separate_background in [None, Some(background(1)), Some(background(9))] {
                // Freeze the previous combined-catalog sort independently of the row index.
                let mut ids = separate_background
                    .iter()
                    .map(DropTargetRecord::id)
                    .chain(targets.iter().map(|target| target.id()))
                    .collect::<Vec<_>>();
                ids.sort_unstable();
                let expected = ids
                    .windows(2)
                    .find_map(|pair| (pair[0] == pair[1]).then_some(pair[0]));
                assert_eq!(
                    DropTargetCatalog::new(&targets, separate_background.as_ref()).err(),
                    expected
                );
            }
        }
    }

    #[test]
    fn first_invalid_record_stays_in_input_order_until_canonicalization() {
        let (workspace, _, owners) = fixture();
        let empty = LogicalRect::new(0.0, 0.0, 0.0, 0.0).unwrap();
        for order in [[1, 2, 0], [0, 1, 2]] {
            let mut plan = PresentationPlan::new(SURFACE, bounds());
            for index in order {
                let owner = &owners[index];
                plan.push_drop_target(Arc::new(DropTargetRecord::new(
                    owner.location(),
                    owner.availability(),
                    owner.region(),
                    owner.layer(),
                    DropVisual::new(empty),
                )));
            }
            let workspace_index = SceneWorkspaceIndex::new(&workspace).unwrap();
            let expected = plan
                .drop_targets()
                .iter()
                .find_map(|target| {
                    validate_drop_target(&plan, target, &workspace, &workspace_index).err()
                })
                .unwrap();
            assert_eq!(validate(&workspace, plan), Err(expected));
        }
    }

    #[test]
    fn record_class_priority_and_independent_guide_duplicates_are_preserved() {
        let (workspace, tabs, owners) = fixture();
        let mut plan = PresentationPlan::new(SURFACE, bounds());
        for target in &owners {
            plan.push_drop_target(Arc::clone(target));
        }
        let guide = DropGuideTargetRecord::new(Arc::clone(&owners[0]), bounds());
        plan.push_drop_guide_cluster(DropGuideClusterRecord::inner(
            SURFACE,
            ROOT,
            tabs,
            HitRegion::new(bounds()),
            SceneLayerKey::surface_base(),
            guide.clone(),
            DropGuideEdgeSet::new(guide.clone(), guide.clone(), guide.clone(), guide),
        ));
        assert_eq!(
            validate_semantic_uniqueness(&plan).err(),
            Some(SceneBuildError::DuplicateDropTarget { id: owners[0].id() })
        );
        plan.push_drop_target(Arc::clone(&owners[1]));
        assert_eq!(
            validate_semantic_uniqueness(&plan).err(),
            Some(SceneBuildError::DuplicateDropTarget { id: owners[1].id() })
        );
        let pane = PaneRecord::new(
            PaneSceneId { root: ROOT, tabs },
            bounds(),
            bounds(),
            SceneLayerKey::surface_base(),
        );
        plan.facts_mut().pane_records = vec![pane, pane];
        assert_eq!(
            validate(&workspace, plan),
            Err(SceneBuildError::DuplicatePane { id: pane.id() })
        );
    }
}
