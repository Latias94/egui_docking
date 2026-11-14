use super::*;
use crate::graph::command::{DockFraction, WorkspaceCommand};
use crate::graph::transaction::WorkspaceTransaction;
use crate::graph::{RootRecord, SurfacePresentation};
use crate::interaction::targeting::guide::{DropGuideEdgeSet, DropGuideTargetRecord};
use crate::interaction::targeting::targets::{DropTargetLocation, DropVisual};
use crate::policy::DockPolicy;
use crate::presentation::hit::region::HitRegion;
use crate::presentation::manifest::PolicyRevision;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

struct Fixture {
    workspace: Workspace,
    tabs: NodeId,
    targets: [Arc<DropTargetRecord>; 5],
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn bounds() -> LogicalRect {
    rect(0.0, 0.0, 200.0, 200.0)
}

fn fixture(overlap: bool) -> Fixture {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([FIRST, SECOND]));
    builder.set_root(ROOT, RootRecord::new(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let edges = [Edge::Left, Edge::Right, Edge::Top, Edge::Bottom];
    let placements = [
        (80.0, 80.0),
        (40.0, 80.0),
        (120.0, 80.0),
        (80.0, 40.0),
        (80.0, 120.0),
    ];
    let targets = std::array::from_fn(|index| {
        let location = if index == 0 {
            DropTargetLocation::Center {
                surface: SURFACE,
                root: ROOT,
                tabs,
            }
        } else {
            let edge = edges[index - 1];
            DropTargetLocation::InnerEdge {
                surface: SURFACE,
                root: ROOT,
                node: tabs,
                edge,
                fraction: DockFraction::new(0.3).unwrap(),
            }
        };
        let (x, y) = if overlap {
            placements[0]
        } else {
            placements[index]
        };
        let hit = rect(x, y, 20.0, 20.0);
        Arc::new(DropTargetRecord::new(
            location,
            DropTargetAvailability::Available,
            HitRegion::new(hit),
            SceneLayerKey::surface_base(),
            DropVisual::new(hit),
        ))
    });
    Fixture {
        workspace,
        tabs,
        targets,
    }
}

fn guides(targets: &[Arc<DropTargetRecord>; 5]) -> [DropGuideTargetRecord; 5] {
    std::array::from_fn(|index| {
        DropGuideTargetRecord::new(Arc::clone(&targets[index]), targets[index].region().rect())
    })
}

fn plan(fixture: &Fixture, guides: [DropGuideTargetRecord; 5]) -> PresentationPlan {
    let [center, left, right, top, bottom] = guides;
    let mut plan = PresentationPlan::new(SURFACE, bounds());
    for target in fixture.targets.iter().rev() {
        plan.push_drop_target(Arc::clone(target));
    }
    plan.push_drop_guide_cluster(DropGuideClusterRecord::inner(
        SURFACE,
        ROOT,
        fixture.tabs,
        HitRegion::new(bounds()),
        SceneLayerKey::surface_base(),
        center,
        DropGuideEdgeSet::new(left, right, top, bottom),
    ));
    plan
}

fn validate(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    plan: PresentationPlan,
) -> Result<PresentationPlan, SceneBuildError> {
    PresentationPlanValidator::new(workspace, policy)
        .unwrap()
        .validate_and_canonicalize(plan)
}

#[test]
fn slot_rejections_precede_earlier_pairs_with_overlapping_hits() {
    let fixture = fixture(true);
    let policy = DockPolicySnapshot::default();
    let cluster = DropGuideClusterId::inner(SURFACE, ROOT, fixture.tabs);
    let bottom_id = fixture.targets[4].id();

    let mut missing = plan(&fixture, guides(&fixture.targets));
    missing
        .facts_mut()
        .drop_targets
        .retain(|target| target.id() != bottom_id);
    let mut foreign_guides = guides(&fixture.targets);
    foreign_guides[4] = DropGuideTargetRecord::new(
        Arc::new(fixture.targets[4].as_ref().clone()),
        fixture.targets[4].region().rect(),
    );
    let foreign = plan(&fixture, foreign_guides);
    for plan in [missing, foreign] {
        assert_eq!(
            validate(&fixture.workspace, &policy, plan),
            Err(SceneBuildError::TargetSemanticMismatch {
                surface: SURFACE,
                target: bottom_id
            })
        );
    }

    let mut wrong_slot_guides = guides(&fixture.targets);
    wrong_slot_guides.swap(3, 4);
    assert_eq!(
        validate(
            &fixture.workspace,
            &policy,
            plan(&fixture, wrong_slot_guides)
        ),
        Err(SceneBuildError::DropGuideTargetSlotMismatch {
            cluster,
            slot: DropGuideSlot::Edge(Edge::Top),
            target: bottom_id,
        })
    );

    let mut invalid_draw_guides = guides(&fixture.targets);
    invalid_draw_guides[4] =
        DropGuideTargetRecord::new(Arc::clone(&fixture.targets[4]), rect(0.0, 0.0, 10.0, 10.0));
    assert_eq!(
        validate(
            &fixture.workspace,
            &policy,
            plan(&fixture, invalid_draw_guides)
        ),
        Err(SceneBuildError::DropGuideDrawOutsideHitRegion {
            surface: SURFACE,
            cluster,
            target: bottom_id,
        })
    );
}

#[test]
fn overlapping_pairs_keep_canonical_slot_order_across_catalog_orders() {
    let fixture = fixture(true);
    let policy = DockPolicySnapshot::default();
    let mut plan = plan(&fixture, guides(&fixture.targets));
    for _ in 0..2 {
        assert_eq!(
            validate(&fixture.workspace, &policy, plan.clone()),
            Err(SceneBuildError::OverlappingDropGuideHitRegions {
                surface: SURFACE,
                cluster: DropGuideClusterId::inner(SURFACE, ROOT, fixture.tabs),
                first: DropGuideSlot::Center,
                second: DropGuideSlot::Edge(Edge::Left),
            })
        );
        plan.facts_mut().drop_targets.reverse();
    }
}

#[test]
fn canonicalization_retains_unchanged_owners_and_guide_snapshots() {
    let fixture = fixture(false);
    let policy = DockPolicySnapshot::default();
    let old = Arc::new(
        validate(
            &fixture.workspace,
            &policy,
            plan(&fixture, guides(&fixture.targets)),
        )
        .unwrap(),
    );
    let mut current = old.as_ref().clone();

    for _ in 0..2 {
        current.facts_mut().drop_targets.reverse();
        current = validate(&fixture.workspace, &policy, current).unwrap();
        assert_eq!(current.drop_targets(), old.drop_targets());
        assert_eq!(current.drop_guide_clusters(), old.drop_guide_clusters());
        for target in current.drop_targets() {
            let original = fixture
                .targets
                .iter()
                .find(|original| original.id() == target.id())
                .unwrap();
            assert!(Arc::ptr_eq(target, original));
        }
        for (_, guide) in current.drop_guide_clusters()[0].targets() {
            let index = current
                .drop_targets()
                .binary_search_by_key(&guide.id(), |target| target.id())
                .unwrap();
            let target = &current.drop_targets()[index];
            assert!(guide.shares_target(target));
            assert_eq!(guide.hit(), target.region());
            assert_eq!(guide.preview(), target.visual().rect());
        }
    }
}

#[test]
fn canonical_rebinding_preserves_old_owners_and_binds_current_selection() {
    let mut fixture = fixture(false);
    let policy = DockPolicySnapshot::default();
    let old = Arc::new(
        validate(
            &fixture.workspace,
            &policy,
            plan(&fixture, guides(&fixture.targets)),
        )
        .unwrap(),
    );
    let mut disabled = DockPolicy::default();
    disabled.set_allow_tab_merge(false);
    disabled.set_allow_edge_split(false);
    let disabled = disabled.snapshot(PolicyRevision::new(1));
    let stale_source = fixture
        .workspace
        .capture_item_source(ROOT, fixture.tabs, SECOND)
        .unwrap();

    for select in [false, true] {
        if select {
            WorkspaceTransaction::from_commands([WorkspaceCommand::Select {
                source: stale_source.clone(),
            }])
            .apply(&mut fixture.workspace, &policy)
            .unwrap();
        }
        let current = validate(&fixture.workspace, &disabled, old.as_ref().clone()).unwrap();
        assert_eq!(
            current.command_basis(ROOT),
            Some(&fixture.workspace.root_fingerprint(ROOT).unwrap())
        );
        for target in current.drop_targets() {
            assert_eq!(
                target.availability(),
                DropTargetAvailability::Unavailable(DropTargetUnavailable::PolicyDisabled)
            );
            let old_target = old
                .drop_targets()
                .iter()
                .find(|old| old.id() == target.id())
                .unwrap();
            assert!(!Arc::ptr_eq(target, old_target));
            assert_eq!(old_target.availability(), DropTargetAvailability::Available);
        }
        for (_, guide) in current.drop_guide_clusters()[0].targets() {
            let index = current
                .drop_targets()
                .binary_search_by_key(&guide.id(), |target| target.id())
                .unwrap();
            assert!(guide.shares_target(&current.drop_targets()[index]));
        }
        for (_, guide) in old.drop_guide_clusters()[0].targets() {
            assert_eq!(
                guide.target().availability(),
                DropTargetAvailability::Available
            );
        }
    }
}
