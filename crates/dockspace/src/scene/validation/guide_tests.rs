use super::*;
use crate::command::{DockFraction, WorkspaceCommand};
use crate::drop_guide::{DropGuideEdgeSet, DropGuideTargetRecord};
use crate::drop_target::DropVisual;
use crate::graph::{RootRecord, SurfacePresentation};
use crate::hit_region::HitRegion;
use crate::policy::DockPolicy;
use crate::scene_manifest::PolicyRevision;
use crate::transaction::WorkspaceTransaction;

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
        let (id, target) = if index == 0 {
            (
                DropTargetId::Center {
                    surface: SURFACE,
                    root: ROOT,
                    tabs,
                },
                DockTarget::Center(workspace.capture_tab_target(ROOT, tabs).unwrap()),
            )
        } else {
            let edge = edges[index - 1];
            (
                DropTargetId::InnerEdge {
                    surface: SURFACE,
                    root: ROOT,
                    node: tabs,
                    edge,
                },
                DockTarget::InnerEdge(
                    workspace
                        .capture_inner_edge_target(
                            ROOT,
                            tabs,
                            edge,
                            DockFraction::new(0.3).unwrap(),
                        )
                        .unwrap(),
                ),
            )
        };
        let (x, y) = if overlap {
            placements[0]
        } else {
            placements[index]
        };
        let hit = rect(x, y, 20.0, 20.0);
        Arc::new(DropTargetRecord::new(
            id,
            target,
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
        plan.drop_targets.reverse();
    }
}

#[test]
fn canonical_rebinding_preserves_old_owners_and_stale_before_policy() {
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

    for stale in [false, true] {
        if stale {
            WorkspaceTransaction::from_commands([WorkspaceCommand::Select {
                source: stale_source.clone(),
            }])
            .apply(&mut fixture.workspace, &policy)
            .unwrap();
        }
        let current = validate(&fixture.workspace, &disabled, old.as_ref().clone()).unwrap();
        for target in current.drop_targets() {
            assert_eq!(
                target.availability(),
                DropTargetAvailability::Unavailable(if stale {
                    DropTargetUnavailable::Stale
                } else {
                    DropTargetUnavailable::PolicyDisabled
                })
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
