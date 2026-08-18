use super::*;
use crate::geometry::{LogicalRect, LogicalSize};
use crate::graph::{Axis, ContainedFloating, RootRecord, SurfacePresentation};
use crate::ids::{ItemId, WorkspaceEpoch, WorkspaceRevision};
use crate::interaction::{DragGeneration, DragSessionId};
use crate::policy::{DockPolicy, TabBarInteraction, TabBarPolicy, TabBarVisibility};
use crate::presentation_hit::{
    PopupHitRole, PresentationHitManifest, PresentationHitRegionKind, PresentationPlane,
};
use crate::presentation_observation::{PresentationOutputSerial, SurfacePresentationOutputTicket};
use crate::scene::{PresentationPlanValidator, SurfaceSceneStamp, TabStripMemberVisibility};
use crate::scene_manifest::{
    Measurement, SurfaceSceneRevision, TabIntrinsic, TabListMenuMetrics, TabStripControlMetric,
    TabStripControlMetrics, TabStripControlPlacement, TabStripMetrics,
};

fn version() -> WorkspaceVersion {
    WorkspaceVersion::new(WorkspaceEpoch::new(3), WorkspaceRevision::new(5))
}

fn authority_domain() -> EngineAuthorityDomainId {
    EngineAuthorityDomainId::new_for_test(1)
}

fn policy_revision() -> PolicyRevision {
    PolicyRevision::new(8)
}

fn policy_snapshot() -> DockPolicySnapshot {
    DockPolicy::default().snapshot(policy_revision())
}

fn positive_rect_overlap(left: LogicalRect, right: LogicalRect) -> bool {
    left.x() < right.max().x()
        && right.x() < left.max().x()
        && left.y() < right.max().y()
        && right.y() < left.max().y()
}

fn compile_default_ready_plan(
    workspace: &Workspace,
    surface: SurfaceId,
    bounds: LogicalRect,
) -> PresentationPlan {
    let policy = policy_snapshot();
    let config = DockPresentationConfig::default();
    let draft = derive_scene_requirement_draft(
        authority_domain(),
        workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy,
        RequirementRevision::new(9),
        &surface_revisions([surface]),
    )
    .expect("requirements should derive");
    let requirements = draft.surface(surface).expect("surface should exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
        .expect("bounds answer is unique");
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane answer is unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(56.0).expect("intrinsic should be valid")),
            )
            .expect("tab answer is unique");
    }
    let control_extent = config.tab_bar_height();
    let controls = TabStripControlMetrics::new(0.0)
        .expect("control spacing should be valid")
        .with_scroll_backward(
            TabStripControlMetric::new(control_extent, TabStripControlPlacement::OverlayLeading)
                .expect("backward control should be valid"),
        )
        .with_scroll_forward(
            TabStripControlMetric::new(control_extent, TabStripControlPlacement::OverlayTrailing)
                .expect("forward control should be valid"),
        )
        .with_tab_list_menu(
            TabStripControlMetric::new(control_extent, TabStripControlPlacement::ReservedTrailing)
                .expect("menu control should be valid"),
        );
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0)
                        .expect("strip should be valid")
                        .with_controls(controls),
                ),
            )
            .expect("strip answer is unique");
    }
    let manifest = draft
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");
    let plan = compile_surface_measurements(
        workspace,
        version(),
        &policy,
        &config,
        &manifest,
        &measurements,
        &TabStripStateStore::default(),
        &[],
    )
    .expect("complete measurements should compile");
    PresentationPlanValidator::new(workspace, &policy)
        .expect("validator should initialize")
        .validate_and_canonicalize(plan)
        .expect("compiled plan should validate")
}

#[test]
fn main_presentation_menu_anchor_prefers_the_visible_central_tab_bar() {
    let surface = SurfaceId::new(801);
    let root = RootId::new(802);
    let mut builder = Workspace::builder();
    let first = builder.insert_node(Node::tabs([ItemId::new(803)]));
    let central = builder.insert_node(Node::tabs([ItemId::new(804)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [first, central]).expect("split should be valid"),
    );
    builder.set_root(root, RootRecord::new(split).with_central(central));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");

    let plan = compile_default_ready_plan(
        &workspace,
        surface,
        LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("bounds should be valid"),
    );
    let [anchor] = plan.presentation_menu_anchor_records() else {
        panic!("one main root should publish one presentation menu anchor")
    };

    assert_eq!(anchor.root(), root);
    assert_eq!(
        anchor.host(),
        PresentationMenuAnchorHost::TabBar(TabBarSceneId {
            root,
            tabs: central,
        })
    );
    let bar = plan
        .tab_bar_records()
        .iter()
        .find(|bar| {
            *bar.id()
                == TabBarSceneId {
                    root,
                    tabs: central,
                }
        })
        .expect("central tab bar should be compiled");
    let anchor_bounds = anchor
        .ready_bounds()
        .expect("the wide central bar should publish usable menu chrome");
    assert!(rect_contains(bar.bounds(), anchor_bounds));
    assert!(!positive_rect_overlap(bar.viewport(), anchor_bounds));
    assert!(
        bar.group_drag()
            .into_iter()
            .flat_map(|group| group.regions())
            .all(|region| !positive_rect_overlap(region.bounds(), anchor_bounds))
    );
}

#[test]
fn main_presentation_menu_anchor_falls_back_to_left_to_right_dfs_order() {
    let surface = SurfaceId::new(811);
    let root = RootId::new(812);
    let mut builder = Workspace::builder();
    let structurally_smaller_right = builder.insert_node(Node::tabs([ItemId::new(813)]));
    let visual_left = builder.insert_node(Node::tabs([ItemId::new(814)]));
    let split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [visual_left, structurally_smaller_right])
            .expect("split should be valid"),
    );
    builder.set_root(root, RootRecord::new(split));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");

    let plan = compile_default_ready_plan(
        &workspace,
        surface,
        LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("bounds should be valid"),
    );
    let [anchor] = plan.presentation_menu_anchor_records() else {
        panic!("one main root should publish one presentation menu anchor")
    };

    assert_eq!(
        anchor.host(),
        PresentationMenuAnchorHost::TabBar(TabBarSceneId {
            root,
            tabs: visual_left,
        })
    );
}

#[test]
fn main_presentation_menu_anchor_falls_back_when_the_central_bar_cannot_host_it() {
    let surface = SurfaceId::new(819);
    let root = RootId::new(820);
    let mut builder = Workspace::builder();
    let wide = builder.insert_node(Node::tabs([ItemId::new(821)]));
    let narrow_central = builder.insert_node(Node::tabs([ItemId::new(822)]));
    let split = builder.insert_node(
        Node::split(Axis::Horizontal, [wide, narrow_central], [0.9, 0.1])
            .expect("weighted split should be valid"),
    );
    builder.set_root(root, RootRecord::new(split).with_central(narrow_central));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");

    let plan = compile_default_ready_plan(
        &workspace,
        surface,
        LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("bounds should be valid"),
    );
    let [anchor] = plan.presentation_menu_anchor_records() else {
        panic!("the wider fallback bar should publish the presentation menu anchor")
    };

    assert_eq!(
        anchor.host(),
        PresentationMenuAnchorHost::TabBar(TabBarSceneId { root, tabs: wide })
    );
    assert!(anchor.is_ready());
}

#[test]
fn narrow_main_tab_bar_preserves_an_interactive_tab_before_the_presentation_menu() {
    let surface = SurfaceId::new(815);
    let root = RootId::new(816);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(817), ItemId::new(818)]));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");

    let plan = compile_default_ready_plan(
        &workspace,
        surface,
        LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("bounds should be valid"),
    );

    let [anchor] = plan.presentation_menu_anchor_records() else {
        panic!("the compact root must retain one explicit menu availability record")
    };
    assert!(
        !anchor.is_ready(),
        "compact chrome must mark the optional menu unavailable before consuming the last usable tab"
    );
    assert!(
        plan.tab_records().iter().any(|tab| {
            let drag = tab.drag_hit().rect();
            drag.width() > 0.0 && drag.height() > 0.0 && tab.close_bounds().is_some()
        }),
        "at least one tab must retain positive drag and close interaction geometry"
    );
    let bar = plan
        .tab_bar_records()
        .iter()
        .find(|bar| *bar.id() == TabBarSceneId { root, tabs })
        .expect("the compact tab bar should still be compiled");
    assert!(
        bar.group_grip_bounds().is_some(),
        "optional menu chrome must compact before the primary group-drag affordance"
    );
}

#[test]
fn contained_presentation_menu_anchor_owns_the_title_partition_exclusively() {
    let surface = SurfaceId::new(821);
    let root = RootId::new(822);
    let floating = FloatingPresentationId::new(823);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(824)]));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(
            root,
            LogicalRect::new(40.0, 30.0, 320.0, 240.0).expect("floating rect should be valid"),
        ),
    );
    builder
        .attach_contained(surface, floating)
        .expect("surface should exist");
    let workspace = builder.build().expect("fixture should validate");

    let plan = compile_default_ready_plan(
        &workspace,
        surface,
        LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("bounds should be valid"),
    );
    let [anchor] = plan.presentation_menu_anchor_records() else {
        panic!("one contained root should publish one presentation menu anchor")
    };
    let contained = plan
        .contained_record(floating)
        .expect("contained chrome should be compiled");

    assert_eq!(anchor.root(), root);
    assert_eq!(
        anchor.host(),
        PresentationMenuAnchorHost::ContainedTitle(floating)
    );
    let anchor_bounds = anchor
        .ready_bounds()
        .expect("the contained title should publish usable menu chrome");
    assert!(rect_contains(contained.title_bounds(), anchor_bounds));
    assert!(!positive_rect_overlap(
        contained.title_drag_hit().rect(),
        anchor_bounds
    ));
    assert!(
        contained
            .close_bounds()
            .is_none_or(|close| !positive_rect_overlap(close, anchor_bounds))
    );
    assert!(
        plan.presentation_menu_anchor_records()
            .iter()
            .all(|anchor| !matches!(anchor.host(), PresentationMenuAnchorHost::TabBar(_)))
    );
}

#[test]
fn selected_tab_reveal_preserves_visible_offsets_and_moves_only_as_needed() {
    assert_eq!(reveal_tab_range(40.0, 200.0, 100.0, 50.0, 90.0), 40.0);
    assert_eq!(reveal_tab_range(80.0, 200.0, 100.0, 20.0, 60.0), 20.0);
    assert_eq!(reveal_tab_range(20.0, 200.0, 100.0, 120.0, 160.0), 60.0);
    assert_eq!(
        reveal_tab_range(40.0, 200.0, 100.0, 50.0, 180.0),
        50.0,
        "an oversized selected tab aligns its leading edge"
    );
}

fn hit_manifest(
    plan: &PresentationPlan,
    measurements: &SurfaceMeasurements,
) -> PresentationHitManifest {
    let scene = SurfaceSceneStamp::new(measurements.ticket(), SurfaceSceneRevision::new(1));
    let output = SurfacePresentationOutputTicket::mint(
        authority_domain(),
        PresentationOutputSerial::new_for_test(1),
        scene,
    );
    PresentationHitManifest::compile(output, plan)
}

fn surface_revisions(
    surfaces: impl IntoIterator<Item = SurfaceId>,
) -> BTreeMap<SurfaceId, SurfaceRequirementRevision> {
    surfaces
        .into_iter()
        .enumerate()
        .map(|(index, surface)| {
            (
                surface,
                SurfaceRequirementRevision::new(
                    u64::try_from(index).expect("fixture index should fit") + 1,
                ),
            )
        })
        .collect()
}

fn balanced_hot_root_fixture(
    leaf_count: usize,
) -> (
    Workspace,
    SceneRequirementManifest,
    SurfaceMeasurements,
    usize,
) {
    assert!(leaf_count.is_power_of_two());
    let surface = SurfaceId::new(700);
    let root = RootId::new(701);
    let mut builder = Workspace::builder();
    let mut level = Vec::with_capacity(leaf_count);
    let mut central = None;
    for index in 0..leaf_count {
        let item = ItemId::new(
            u64::try_from(index)
                .expect("fixture index should fit")
                .saturating_add(1),
        );
        let tabs = builder.insert_node(Node::tabs([item]));
        central.get_or_insert(tabs);
        level.push(tabs);
    }
    let mut axis = Axis::Horizontal;
    while level.len() > 1 {
        let mut parents = Vec::with_capacity(level.len() / 2);
        for children in level.chunks_exact(2) {
            parents.push(
                builder.insert_node(
                    Node::equal_split(axis, children.iter().copied())
                        .expect("two fixture children form one valid split"),
                ),
            );
        }
        level = parents;
        axis = match axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
    }
    builder.set_root(
        root,
        RootRecord::new(level[0]).with_central(central.expect("fixture has one leaf")),
    );
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("balanced fixture should validate");
    let node_count = workspace.nodes().count();
    let draft = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy_snapshot(),
        RequirementRevision::new(9),
        &surface_revisions([surface]),
    )
    .expect("requirements should derive");
    let requirements = draft.surface(surface).expect("surface should exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(
                LogicalRect::new(0.0, 0.0, 32_768.0, 32_768.0)
                    .expect("fixture bounds should be valid"),
            ),
        )
        .expect("bounds answer is unique");
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane answer is unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(56.0).expect("intrinsic should be valid")),
            )
            .expect("tab answer is unique");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0).expect("strip should be valid"),
                ),
            )
            .expect("strip answer is unique");
    }
    let manifest = draft
        .finalize(PopupPlaneRequirement::default())
        .expect("inactive requirements should finalize");
    (workspace, manifest, measurements, node_count)
}

#[test]
fn scene_target_fingerprints_are_built_once_per_root_at_scale() {
    for leaf_count in [16, 128, 1_024] {
        crate::drop_resolver::structural_work::reset();
        let (workspace, manifest, measurements, node_count) = balanced_hot_root_fixture(leaf_count);
        compile_surface_measurements(
            &workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &TabStripStateStore::default(),
            &[],
        )
        .expect("complete measurements should compile");

        let work = crate::drop_resolver::structural_work::snapshot();
        assert_eq!(
            work.root_fingerprint_builds, 1,
            "a {leaf_count}-leaf hot root must build one shared fingerprint"
        );
        assert_eq!(
            work.root_fingerprint_node_visits, node_count,
            "a {leaf_count}-leaf hot root must visit every root node exactly once"
        );
    }
}

#[test]
fn exact_presentation_hit_lookup_scales_with_manifest_height() {
    for leaf_count in [16, 128, 1_024] {
        let (workspace, manifest, measurements, _) = balanced_hot_root_fixture(leaf_count);
        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &TabStripStateStore::default(),
            &[],
        )
        .expect("complete measurements should compile");
        let hit_manifest = hit_manifest(&plan, &measurements);
        let kinds = hit_manifest
            .regions()
            .iter()
            .map(|region| region.id().kind())
            .collect::<Vec<_>>();
        let region_count = kinds.len();
        assert!(region_count > 0);

        crate::drop_resolver::structural_work::reset();
        for kind in kinds {
            assert!(hit_manifest.region_for_kind(kind).is_some());
        }
        let comparisons =
            crate::drop_resolver::structural_work::snapshot().presentation_hit_lookup_comparisons;
        let binary_search_height =
            usize::try_from(region_count.ilog2()).expect("lookup height should fit usize") + 2;
        assert!(
            comparisons >= region_count,
            "every exact lookup performs at least one comparison"
        );
        assert!(
            comparisons <= region_count * binary_search_height,
            "{region_count} exact receiver lookups used {comparisons} comparisons at {leaf_count} leaves"
        );
    }
}

#[test]
fn measured_future_drop_preview_does_not_reindex_the_candidate_workspace() {
    for leaf_count in [16, 128, 1_024] {
        let (workspace, manifest, measurements, node_count) = balanced_hot_root_fixture(leaf_count);
        let policy = policy_snapshot();
        let plan = compile_surface_measurements(
            &workspace,
            version(),
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &TabStripStateStore::default(),
            &[],
        )
        .expect("complete measurements should compile");
        assert!(plan.layout_facts().is_some());

        let source_item = ItemId::new(u64::try_from(leaf_count).expect("fixture size fits u64"));
        let source = workspace
            .capture_item_source_by_id(source_item)
            .expect("fixture source lookup should remain valid")
            .expect("fixture source item should exist");
        let target = plan
            .drop_guide_clusters()
            .iter()
            .find(|cluster| matches!(cluster.id().scope, crate::drop_guide::DropGuideScope::Outer))
            .and_then(|cluster| cluster.target(crate::drop_guide::DropGuideSlot::Edge(Edge::Left)))
            .expect("compiled root should expose its outer-left guide");
        let hit = target.target().region().rect();
        let point = LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5)
            .expect("guide center should be finite");
        let stamp = SurfaceSceneStamp::new(measurements.ticket(), SurfaceSceneRevision::new(1));

        crate::drop_resolver::structural_work::reset();
        let query = crate::drop_resolver::resolve_presented_drop(
            stamp,
            &plan,
            plan.layout_facts(),
            &workspace,
            version(),
            manifest.workspace_index(),
            &policy,
            DragSessionId::new(version().epoch(), DragGeneration::new(1)),
            crate::command::MovePayload::Item(source),
            None,
            point,
        )
        .expect("measured edge preview should resolve");
        assert!(matches!(
            query.resolution(),
            crate::drop_resolver::DropResolution::Resolved(_)
        ));

        let work = crate::drop_resolver::structural_work::snapshot();
        assert_eq!(work.transaction_prepares, 1);
        assert_eq!(work.workspace_deep_clones.transaction_candidates.calls, 1);
        assert!(
            work.root_fingerprint_builds <= 8,
            "future projection must not add a ninth full-root build at {leaf_count} leaves"
        );
        assert!(
            work.root_fingerprint_node_visits <= node_count * 8,
            "future projection must not add another root traversal at {leaf_count} leaves"
        );
    }
}

#[test]
fn stale_manifest_index_cannot_compile_a_changed_topology_revision() {
    let (workspace, manifest, measurements, _) = balanced_hot_root_fixture(16);
    let mut changed = workspace.clone();
    let root = RootId::new(701);
    let root_node = changed.root(root).expect("fixture root should exist").node;
    let Node::Split {
        children, weights, ..
    } = changed
        .nodes
        .get_mut(root_node)
        .expect("fixture root node should exist")
    else {
        panic!("balanced fixture root must be a split")
    };
    children.reverse();
    weights.reverse();
    changed
        .validate()
        .expect("changed topology should validate");
    let current = WorkspaceVersion::new(version().epoch(), WorkspaceRevision::new(6));

    assert!(matches!(
        compile_surface_measurements(
            &changed,
            current,
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &TabStripStateStore::default(),
            &[],
        ),
        Err(PresentationCompilationError::WorkspaceVersionMismatch {
            manifest: stale,
            current: actual,
        }) if stale == version() && actual == current
    ));
}

type OverflowingTabStripFixture = (
    Workspace,
    SceneRequirementManifest,
    SurfaceMeasurements,
    TabStripStateStore,
    SurfaceId,
    TabBarSceneId,
    [ItemId; 4],
);

fn overflowing_tab_strip_fixture(legacy_scroll: f64) -> OverflowingTabStripFixture {
    overflowing_tab_strip_fixture_with(
        legacy_scroll,
        &policy_snapshot(),
        Some(
            TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
                .expect("menu metrics should be valid"),
        ),
        true,
    )
}

fn overflowing_tab_strip_fixture_with(
    legacy_scroll: f64,
    policy: &DockPolicySnapshot,
    menu: Option<TabListMenuMetrics>,
    open_menu: bool,
) -> OverflowingTabStripFixture {
    let controls = TabStripControlMetrics::new(4.0)
        .expect("control metrics should be valid")
        .with_scroll_backward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                .expect("backward control metric should be valid"),
        )
        .with_scroll_forward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                .expect("forward control metric should be valid"),
        )
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu control metric should be valid"),
        );
    overflowing_tab_strip_fixture_with_controls(legacy_scroll, policy, menu, open_menu, controls)
}

fn overflowing_tab_strip_fixture_with_controls(
    legacy_scroll: f64,
    policy: &DockPolicySnapshot,
    menu: Option<TabListMenuMetrics>,
    open_menu: bool,
    controls: TabStripControlMetrics,
) -> OverflowingTabStripFixture {
    let bounds =
        LogicalRect::new(0.0, 0.0, 260.0, 180.0).expect("default fixture bounds should be valid");
    overflowing_tab_strip_fixture_with_geometry(
        legacy_scroll,
        policy,
        menu,
        open_menu,
        controls,
        bounds,
        bounds,
    )
}

#[allow(clippy::too_many_arguments)]
fn overflowing_tab_strip_fixture_with_geometry(
    legacy_scroll: f64,
    policy: &DockPolicySnapshot,
    menu: Option<TabListMenuMetrics>,
    open_menu: bool,
    controls: TabStripControlMetrics,
    layout_bounds: LogicalRect,
    popup_plane_bounds: LogicalRect,
) -> OverflowingTabStripFixture {
    let surface = SurfaceId::new(41);
    let root = RootId::new(42);
    let items = [
        ItemId::new(50),
        ItemId::new(51),
        ItemId::new(52),
        ItemId::new(53),
    ];
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs_with_selection(items, Some(items[3])));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");
    let draft = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        policy,
        RequirementRevision::new(9),
        &surface_revisions([surface]),
    )
    .expect("requirements should derive");
    let requirements = draft.surface(surface).expect("surface should exist");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(requirements.bounds(), Measurement::Measured(layout_bounds))
        .expect("bounds answer is unique");
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane answer is unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(88.0).expect("intrinsic should be valid")),
            )
            .expect("tab answer is unique");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    menu.map_or_else(
                        || {
                            TabStripMetrics::new(0.0, 0.0)
                                .expect("strip should be valid")
                                .with_controls(controls)
                        },
                        |menu| {
                            TabStripMetrics::new(0.0, 0.0)
                                .expect("strip should be valid")
                                .with_controls(controls)
                                .with_tab_list_menu(menu)
                        },
                    )
                    .with_scroll_offset(legacy_scroll)
                    .expect("legacy observation should be valid"),
                ),
            )
            .expect("strip answer is unique");
    }
    let bar = TabBarSceneId { root, tabs };
    let mut states = TabStripStateStore::default();
    let state_key = TabStripStateKey::new(surface, bar);
    assert!(
        states
            .reconcile_exact_for_surface_roster([(state_key, items.as_slice())], false)
            .expect("tab-strip roster reconciles")
            .state_changed()
    );
    let state = states.state_mut(state_key);
    state
        .set_scroll_offset(36.0)
        .expect("core scroll should be valid");
    if open_menu {
        let (menu, _) = states
            .open_tab_list_menu(state_key, Some(items[2]))
            .expect("the live strip can open its menu");
        states
            .set_active_menu_scroll_offset(menu, 12.0)
            .expect("menu scroll should be valid");
    }
    let manifest = draft
        .finalize(states.popup_requirement())
        .expect("reconciled popup state must finalize coherently");
    if let Some(key) = manifest
        .surface(surface)
        .expect("final surface requirements should exist")
        .popup_plane_bounds()
    {
        measurements
            .set_popup_plane_bounds(key, Measurement::Measured(popup_plane_bounds))
            .expect("popup-plane bounds answer is unique");
    }
    (
        workspace,
        manifest,
        measurements,
        states,
        surface,
        bar,
        items,
    )
}

#[test]
fn overflow_compiles_stable_controls_membership_and_menu_roster() {
    let (workspace, manifest, measurements, states, surface, bar, items) =
        overflowing_tab_strip_fixture(900.0);
    let plan = compile_surface_measurements(
        &workspace,
        version(),
        &policy_snapshot(),
        &DockPresentationConfig::default(),
        &manifest,
        &measurements,
        &states,
        &[],
    )
    .expect("complete measurements should compile");
    let plan = PresentationPlanValidator::new(&workspace, &policy_snapshot())
        .expect("validator should initialize")
        .validate_and_canonicalize(plan)
        .expect("compiled strip records should be strictly valid");

    assert_eq!(plan.surface(), surface);
    assert_eq!(
        plan.tab_strip_control_records()
            .iter()
            .map(|record| record.id())
            .collect::<Vec<_>>(),
        TabStripControlId::for_bar(bar)
    );
    let members = plan.tab_bar_records()[0].members();
    assert_eq!(
        members
            .iter()
            .map(|member| member.tab().item)
            .collect::<Vec<_>>(),
        items
    );
    let visibilities = members
        .iter()
        .map(|member| member.visibility())
        .collect::<BTreeSet<_>>();
    assert!(visibilities.contains(&TabStripMemberVisibility::Visible));
    assert!(visibilities.contains(&TabStripMemberVisibility::PartiallyVisible));

    let menu = &plan.tab_list_menu_records()[0];
    let [backdrop] = plan.tab_list_menu_backdrop_records() else {
        panic!("an active popup must cover the exact surface")
    };
    assert_eq!(backdrop.session(), menu.session());
    assert_eq!(backdrop.revision(), manifest.popup().revision());
    assert_eq!(backdrop.bounds(), plan.bounds());
    assert_eq!(menu.bar(), bar);
    assert_eq!(menu.session().key(), TabStripStateKey::new(surface, bar));
    assert_eq!(
        menu.rows()
            .iter()
            .map(|row| row.tab().item)
            .collect::<Vec<_>>(),
        items
    );
    assert_eq!(
        menu.rows()
            .iter()
            .filter(|row| row.focused())
            .map(|row| row.tab().item)
            .collect::<Vec<_>>(),
        [items[2]]
    );
    assert!(menu.scroll_offset() <= menu.maximum_scroll_offset());

    let hit_manifest = hit_manifest(&plan, &measurements);
    let ids = hit_manifest
        .regions()
        .iter()
        .map(|region| region.id())
        .collect::<BTreeSet<_>>();
    assert_eq!(ids.len(), hit_manifest.regions().len());
    assert_eq!(
        hit_manifest
            .regions()
            .iter()
            .filter(|region| matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabStripControl(_)
            ))
            .count(),
        3
    );
    let blocker = hit_manifest
        .regions()
        .iter()
        .find(|region| {
            region.id().kind() == PresentationHitRegionKind::TabListMenuBlocker(menu.session())
        })
        .expect("the complete popup frame has a blocker");
    assert_eq!(
        blocker.stack().plane(),
        PresentationPlane::Popup(menu.session())
    );
    assert_eq!(
        blocker.stack().popup_role(),
        Some(PopupHitRole::FrameBlocker)
    );
    let backdrop_hit = hit_manifest
        .regions()
        .iter()
        .find(|region| {
            region.id().kind() == PresentationHitRegionKind::TabListMenuBackdrop(menu.session())
        })
        .expect("the popup covers the complete surface");
    assert_eq!(
        backdrop_hit.stack().popup_role(),
        Some(PopupHitRole::Backdrop)
    );
    let row_hits = hit_manifest
        .regions()
        .iter()
        .filter(|region| {
            matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabListMenuRow { menu: actual, .. }
                    if actual == menu.session()
            )
        })
        .count();
    assert_eq!(
        row_hits,
        menu.rows().iter().filter(|row| row.hit().is_some()).count()
    );
}

#[test]
fn bottom_dock_menu_opens_upward_within_the_authoritative_popup_plane() {
    let controls = TabStripControlMetrics::new(4.0)
        .expect("control metrics should be valid")
        .with_scroll_backward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                .expect("backward control metric should be valid"),
        )
        .with_scroll_forward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                .expect("forward control metric should be valid"),
        )
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu control metric should be valid"),
        );
    let layout_bounds =
        LogicalRect::new(0.0, 108.0, 180.0, 32.0).expect("bottom dock bounds should be valid");
    let popup_plane_bounds =
        LogicalRect::new(0.0, 0.0, 180.0, 140.0).expect("viewport popup plane should be valid");
    let menu_metrics = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
        .expect("menu metrics should be valid");
    let (workspace, manifest, measurements, states, _, _, _) =
        overflowing_tab_strip_fixture_with_geometry(
            0.0,
            &policy_snapshot(),
            Some(menu_metrics),
            true,
            controls,
            layout_bounds,
            popup_plane_bounds,
        );

    let plan = compile_surface_measurements(
        &workspace,
        version(),
        &policy_snapshot(),
        &DockPresentationConfig::default(),
        &manifest,
        &measurements,
        &states,
        &[],
    )
    .expect("popup-plane measurements should compile");
    let plan = PresentationPlanValidator::new(&workspace, &policy_snapshot())
        .expect("validator should initialize")
        .validate_and_canonicalize(plan)
        .expect("separate layout and popup bounds should validate");
    let [bar] = plan.tab_bar_records() else {
        panic!("fixture should compile one tab bar")
    };
    let [menu] = plan.tab_list_menu_records() else {
        panic!("active fixture should compile one menu")
    };
    let [backdrop] = plan.tab_list_menu_backdrop_records() else {
        panic!("active fixture should compile one backdrop")
    };

    assert_eq!(plan.bounds(), layout_bounds);
    assert_eq!(plan.popup_plane_bounds(), Some(popup_plane_bounds));
    assert_eq!(backdrop.bounds(), popup_plane_bounds);
    assert!(rect_contains(popup_plane_bounds, menu.bounds()));
    assert!(menu.bounds().max().y() <= bar.bounds().y());
}

#[test]
fn active_popup_rejects_an_empty_measured_plane_before_projection() {
    let controls = TabStripControlMetrics::new(4.0)
        .expect("control metrics should be valid")
        .with_scroll_backward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                .expect("backward control metric should be valid"),
        )
        .with_scroll_forward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                .expect("forward control metric should be valid"),
        )
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu control metric should be valid"),
        );
    let layout_bounds =
        LogicalRect::new(0.0, 0.0, 180.0, 140.0).expect("layout bounds should be valid");
    let empty_popup_plane = LogicalRect::new(0.0, 0.0, 0.0, 140.0)
        .expect("zero-width popup plane is representable but not authoritative");
    let menu_metrics = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
        .expect("menu metrics should be valid");
    let (workspace, manifest, measurements, states, surface, _, _) =
        overflowing_tab_strip_fixture_with_geometry(
            0.0,
            &policy_snapshot(),
            Some(menu_metrics),
            true,
            controls,
            layout_bounds,
            empty_popup_plane,
        );

    assert_eq!(
        compile_surface_measurements(
            &workspace,
            version(),
            &policy_snapshot(),
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        ),
        Err(PresentationCompilationError::Scene(
            SceneCompilationError::EmptyPopupPlaneBounds { surface },
        ))
    );
}

#[test]
fn menu_only_control_reserves_only_its_trailing_extent() {
    let bar = LogicalRect::new(10.0, 20.0, 200.0, 24.0).expect("bar is valid");
    let controls = TabStripControlMetrics::new(4.0)
        .expect("spacing is valid")
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu extent is valid"),
        );
    let layout = tab_strip_control_layout(bar.x(), bar, bar.width(), controls)
        .expect("control geometry compiles")
        .expect("menu-only roster is allocatable");

    assert_eq!(layout.reserved_leading, 0.0);
    assert_eq!(layout.reserved_trailing, 20.0);
    assert_eq!(layout.rects[0], None);
    assert_eq!(layout.rects[1], None);
    let menu = layout.rects[2].expect("menu control is present");
    assert_eq!(menu.x(), bar.max().x() - 20.0);
    assert_eq!(menu.max().x(), bar.max().x());
}

#[test]
fn menu_only_control_compiles_as_an_exact_scene_subset() {
    let policy = policy_snapshot();
    let menu_metrics =
        TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0).expect("menu metrics are valid");
    let controls = TabStripControlMetrics::new(4.0)
        .expect("spacing is valid")
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu extent is valid"),
        );
    let (workspace, manifest, measurements, states, _, bar, _) =
        overflowing_tab_strip_fixture_with_controls(
            0.0,
            &policy,
            Some(menu_metrics),
            false,
            controls,
        );
    let plan = compile_surface_measurements(
        &workspace,
        version(),
        &policy,
        &DockPresentationConfig::default(),
        &manifest,
        &measurements,
        &states,
        &[],
    )
    .expect("menu-only measurements compile");
    let plan = PresentationPlanValidator::new(&workspace, &policy)
        .expect("validator initializes")
        .validate_and_canonicalize(plan)
        .expect("optional control subset is valid");

    let [control] = plan.tab_strip_control_records() else {
        panic!("menu-only metrics publish exactly one control")
    };
    assert_eq!(control.id(), TabStripControlId::TabListMenu(bar));
    let bar = plan
        .tab_bar_records()
        .iter()
        .find(|record| *record.id() == bar)
        .expect("compiled bar exists");
    let anchor = plan
        .presentation_menu_anchor_records()
        .iter()
        .find(|anchor| anchor.root() == bar.id().root)
        .expect("main root presentation anchor exists");
    let anchor_bounds = anchor
        .ready_bounds()
        .expect("menu-only layout should retain usable presentation chrome");
    assert_eq!(bar.viewport().max().x(), control.bounds().x());
    assert_eq!(control.bounds().max().x(), anchor_bounds.x());
    assert_eq!(anchor_bounds.max().x(), bar.bounds().max().x());
}

#[test]
fn overlay_controls_never_overlap_when_the_viewport_is_too_narrow() {
    let bar = LogicalRect::new(0.0, 0.0, 40.0, 24.0).expect("bar is valid");
    let controls = TabStripControlMetrics::new(4.0)
        .expect("spacing is valid")
        .with_scroll_backward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayLeading)
                .expect("backward extent is valid"),
        )
        .with_scroll_forward(
            TabStripControlMetric::new(18.0, TabStripControlPlacement::OverlayTrailing)
                .expect("forward extent is valid"),
        )
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu extent is valid"),
        );
    let layout = tab_strip_control_layout(bar.x(), bar, bar.width(), controls)
        .expect("control geometry compiles")
        .expect("reserved menu remains allocatable");

    assert_eq!(layout.rects[0], None);
    assert_eq!(layout.rects[1], None);
    assert!(layout.rects[2].is_some());
    assert_eq!(layout.reserved_trailing, 20.0);
}

#[test]
fn tiny_host_omits_an_unallocatable_reserved_control_roster() {
    let bar = LogicalRect::new(0.0, 0.0, 15.0, 24.0).expect("bar is valid");
    let controls = TabStripControlMetrics::new(0.0)
        .expect("spacing is valid")
        .with_tab_list_menu(
            TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing)
                .expect("menu extent is valid"),
        );

    assert!(
        tab_strip_control_layout(bar.x(), bar, bar.width(), controls)
            .expect("tiny geometry is a valid unavailable allocation")
            .is_none()
    );
}

#[test]
fn disabled_tab_bar_cannot_publish_an_open_menu_or_row_hits() {
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::Visible,
        TabBarInteraction::Disabled,
    ));
    let policy = policy.snapshot(policy_revision());
    let menu = TabListMenuMetrics::new(24.0, 8.0, 8.0, 2.0, 92.0, 10.0)
        .expect("menu metrics should be valid");
    let (workspace, manifest, measurements, states, _, _, _) =
        overflowing_tab_strip_fixture_with(0.0, &policy, Some(menu), false);

    let plan = compile_surface_measurements(
        &workspace,
        version(),
        &policy,
        &DockPresentationConfig::default(),
        &manifest,
        &measurements,
        &states,
        &[],
    )
    .expect("disabled strip measurements should compile");
    let plan = PresentationPlanValidator::new(&workspace, &policy)
        .expect("validator should initialize")
        .validate_and_canonicalize(plan)
        .expect("disabled strip output should remain valid");

    assert_eq!(plan.tab_strip_control_records().len(), 3);
    assert!(
        plan.tab_strip_control_records()
            .iter()
            .all(|control| !control.enabled())
    );
    assert!(plan.tab_list_menu_records().is_empty());
    assert!(plan.tab_list_menu_backdrop_records().is_empty());
    let hit_manifest = hit_manifest(&plan, &measurements);
    assert_eq!(
        hit_manifest
            .regions()
            .iter()
            .filter(|region| matches!(
                region.id().kind(),
                PresentationHitRegionKind::TabStripControl(_)
            ))
            .count(),
        3,
        "disabled controls remain blockers in the click lane"
    );
    assert!(!hit_manifest.regions().iter().any(|region| matches!(
        region.id().kind(),
        PresentationHitRegionKind::TabListMenuRow { .. }
            | PresentationHitRegionKind::TabListMenuBlocker(_)
    )));
}

#[test]
fn idle_overflow_without_menu_metrics_is_ready_with_a_disabled_menu_receiver() {
    let policy = policy_snapshot();
    let (workspace, manifest, measurements, states, _, bar, _) =
        overflowing_tab_strip_fixture_with(0.0, &policy, None, false);

    let plan = compile_surface_measurements(
        &workspace,
        version(),
        &policy,
        &DockPresentationConfig::default(),
        &manifest,
        &measurements,
        &states,
        &[],
    )
    .expect("optional menu metrics cannot make an idle overflow scene unavailable");
    let plan = PresentationPlanValidator::new(&workspace, &policy)
        .expect("validator should initialize")
        .validate_and_canonicalize(plan)
        .expect("the typed unavailable capability must validate strictly");

    let bar_record = plan
        .tab_bar_records()
        .iter()
        .find(|record| *record.id() == bar)
        .expect("overflowing strip has one exact bar record");
    assert_eq!(
        bar_record.menu_geometry_availability(),
        TabListMenuGeometryAvailability::Unavailable
    );
    let menu_control = TabStripControlId::TabListMenu(bar);
    let control = plan
        .tab_strip_control_records()
        .iter()
        .find(|record| record.id() == menu_control)
        .expect("overflow controls retain an exact menu receiver");
    assert!(!control.enabled());
    assert!(plan.tab_list_menu_records().is_empty());
    assert!(plan.tab_list_menu_backdrop_records().is_empty());

    let hit_manifest = hit_manifest(&plan, &measurements);
    assert_eq!(
        hit_manifest
            .regions()
            .iter()
            .filter(|region| {
                region.id().kind() == PresentationHitRegionKind::TabStripControl(menu_control)
            })
            .count(),
        1,
        "a disabled menu command remains the exclusive click receiver"
    );
}

#[test]
fn active_menu_without_allocatable_geometry_cannot_publish_a_ready_scene() {
    let policy = policy_snapshot();
    let menu = TabListMenuMetrics::new(24.0, 8.0, 20.0, 2.0, 20.0, 10.0)
        .expect("degenerate menu metrics remain a valid measurement");
    let (workspace, manifest, measurements, states, _, _, _) =
        overflowing_tab_strip_fixture_with(0.0, &policy, Some(menu), true);

    assert!(matches!(
        compile_surface_measurements(
            &workspace,
            version(),
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        ),
        Err(PresentationCompilationError::Scene(
            SceneCompilationError::ActiveTabListMenuProjectionUnavailable { .. }
        ))
    ));
}

#[test]
fn overflowing_menu_aggregate_is_rejected_before_scene_publication() {
    let policy = policy_snapshot();
    let menu = TabListMenuMetrics::new(f64::MAX, 8.0, 8.0, 2.0, 92.0, 10.0)
        .expect("finite menu metrics should be accepted as measurements");
    let (workspace, manifest, measurements, states, _, _, _) =
        overflowing_tab_strip_fixture_with(0.0, &policy, Some(menu), true);

    assert!(matches!(
        compile_surface_measurements(
            &workspace,
            version(),
            &policy,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &states,
            &[],
        ),
        Err(PresentationCompilationError::Scene(
            SceneCompilationError::NonFiniteAggregate {
                aggregate: "tab-list row heights"
            }
        ))
    ));
}

#[test]
fn adapter_scroll_observation_does_not_change_core_compilation() {
    let (first_workspace, first_manifest, first, first_states, _, _, _) =
        overflowing_tab_strip_fixture(0.0);
    let (second_workspace, second_manifest, second, second_states, _, _, _) =
        overflowing_tab_strip_fixture(9_000.0);
    let first_plan = compile_surface_measurements(
        &first_workspace,
        version(),
        &policy_snapshot(),
        &DockPresentationConfig::default(),
        &first_manifest,
        &first,
        &first_states,
        &[],
    )
    .expect("first measurements should compile");
    let second_plan = compile_surface_measurements(
        &second_workspace,
        version(),
        &policy_snapshot(),
        &DockPresentationConfig::default(),
        &second_manifest,
        &second,
        &second_states,
        &[],
    )
    .expect("second measurements should compile");

    assert_eq!(first_plan, second_plan);
}

#[test]
fn edge_previews_reuse_the_exact_activation_boundary() {
    let bounds = LogicalRect::new(0.1, 60.2, 179.7, 159.6).expect("valid bounds");
    let fraction = DockFraction::new(0.3).expect("valid fraction");

    let left = edge_preview(bounds, Edge::Left, fraction).expect("left preview");
    let right = edge_preview(bounds, Edge::Right, fraction).expect("right preview");
    let top = edge_preview(bounds, Edge::Top, fraction).expect("top preview");
    let bottom = edge_preview(bounds, Edge::Bottom, fraction).expect("bottom preview");

    assert_eq!(left.min(), bounds.min());
    assert_eq!(left.max().y(), bounds.max().y());
    assert_eq!(right.max(), bounds.max());
    assert_eq!(right.min().y(), bounds.min().y());
    assert_eq!(top.min(), bounds.min());
    assert_eq!(top.max().x(), bounds.max().x());
    assert_eq!(bottom.max(), bounds.max());
    assert_eq!(bottom.min().x(), bounds.min().x());
}

#[test]
fn single_central_leaf_derives_every_exact_question() {
    let surface = SurfaceId::new(1);
    let root = RootId::new(2);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs_with_selection(
        [ItemId::new(10), ItemId::new(11)],
        Some(ItemId::new(11)),
    ));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");

    let manifest = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy_snapshot(),
        RequirementRevision::new(9),
        &surface_revisions([surface]),
    )
    .expect("requirements should derive")
    .finalize(PopupPlaneRequirement::default())
    .expect("inactive requirements should finalize");
    let requirements = manifest
        .surface(surface)
        .expect("surface should be present");

    assert_eq!(manifest.surfaces().len(), 1);
    assert_eq!(manifest.authority_domain(), authority_domain());
    assert_eq!(manifest.policy(), policy_revision());
    assert_eq!(requirements.ticket().workspace_epoch(), version().epoch());
    assert_eq!(
        requirements.ticket().surface_requirement(),
        SurfaceRequirementRevision::new(1)
    );
    assert_eq!(
        requirements.pane_minimums().collect::<Vec<_>>(),
        [
            PaneMinimumKey::new(root, tabs, Some(ItemId::new(10))),
            PaneMinimumKey::new(root, tabs, Some(ItemId::new(11))),
        ]
    );
    assert_eq!(requirements.tab_intrinsics().len(), 2);
    assert_eq!(
        requirements.tab_strips().collect::<Vec<_>>(),
        [TabStripKey::new(surface, TabBarSceneId { root, tabs })]
    );
}

#[test]
fn nested_split_and_rootless_contained_roster_are_complete() {
    let surface = SurfaceId::new(1);
    let root_a = RootId::new(2);
    let root_b = RootId::new(3);
    let floating_a = FloatingPresentationId::new(4);
    let floating_b = FloatingPresentationId::new(5);
    let mut builder = Workspace::builder();
    let a_left = builder.insert_node(Node::tabs([ItemId::new(10)]));
    let a_right = builder.insert_node(Node::tabs([ItemId::new(11)]));
    let a_split = builder.insert_node(
        Node::equal_split(Axis::Horizontal, [a_left, a_right]).expect("split should be valid"),
    );
    let b_tabs = builder.insert_node(Node::tabs([ItemId::new(12), ItemId::new(13)]));
    builder.set_root(root_a, RootRecord::new(a_split).with_central(a_right));
    builder.set_root(root_b, RootRecord::new(b_tabs));
    builder.set_surface(surface, SurfacePresentation::rootless());
    builder.set_contained_floating(
        floating_a,
        ContainedFloating::new(
            root_a,
            LogicalRect::new(10.0, 20.0, 300.0, 240.0).expect("valid rect"),
        ),
    );
    builder.set_contained_floating(
        floating_b,
        ContainedFloating::new(
            root_b,
            LogicalRect::new(350.0, 40.0, 280.0, 220.0).expect("valid rect"),
        ),
    );
    builder
        .attach_contained(surface, floating_a)
        .expect("surface should exist");
    builder
        .attach_contained(surface, floating_b)
        .expect("surface should exist");
    let workspace = builder.build().expect("fixture should validate");

    let manifest = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy_snapshot(),
        RequirementRevision::new(9),
        &surface_revisions([surface]),
    )
    .expect("requirements should derive")
    .finalize(PopupPlaneRequirement::default())
    .expect("inactive requirements should finalize");
    let requirements = manifest
        .surface(surface)
        .expect("surface should be present");

    assert_eq!(requirements.pane_minimums().len(), 4);
    assert_eq!(requirements.tab_intrinsics().len(), 4);
    assert_eq!(requirements.tab_strips().len(), 3);
    assert_eq!(requirements.ticket().surface(), surface);
}

#[test]
fn surface_iteration_and_requirements_are_order_independent() {
    let mut builder = Workspace::builder();
    let tabs_b = builder.insert_node(Node::tabs([ItemId::new(20)]));
    let tabs_a = builder.insert_node(Node::tabs([ItemId::new(10)]));
    builder.set_root(RootId::new(20), RootRecord::new(tabs_b));
    builder.set_root(RootId::new(10), RootRecord::new(tabs_a));
    builder.set_surface(
        SurfaceId::new(20),
        SurfacePresentation::with_main(RootId::new(20)),
    );
    builder.set_surface(
        SurfaceId::new(10),
        SurfacePresentation::with_main(RootId::new(10)),
    );
    let workspace = builder.build().expect("fixture should validate");

    let manifest = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy_snapshot(),
        RequirementRevision::new(9),
        &surface_revisions([SurfaceId::new(10), SurfaceId::new(20)]),
    )
    .expect("requirements should derive")
    .finalize(PopupPlaneRequirement::default())
    .expect("inactive requirements should finalize");

    assert_eq!(
        manifest
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<Vec<_>>(),
        [SurfaceId::new(10), SurfaceId::new(20)]
    );
}

#[test]
fn surface_revision_inventory_requires_live_surfaces_and_tolerates_tombstones() {
    let surface = SurfaceId::new(1);
    let root = RootId::new(2);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(root, RootRecord::new(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");

    assert_eq!(
        derive_scene_requirement_draft(
            authority_domain(),
            &workspace,
            version(),
            PresentationConfigRevision::new(7),
            &policy_snapshot(),
            RequirementRevision::new(9),
            &BTreeMap::new(),
        ),
        Err(SceneRequirementDerivationError::MissingSurfaceRequirementRevision { surface })
    );
    let manifest = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy_snapshot(),
        RequirementRevision::new(9),
        &BTreeMap::from([
            (surface, SurfaceRequirementRevision::new(1)),
            (SurfaceId::new(99), SurfaceRequirementRevision::new(2)),
        ]),
    )
    .expect("removed-surface revision tombstones should remain valid")
    .finalize(PopupPlaneRequirement::default())
    .expect("inactive requirements should finalize");

    assert_eq!(
        manifest
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<Vec<_>>(),
        [surface]
    );
}

#[test]
fn compilation_rejects_a_policy_snapshot_newer_than_the_exact_ticket() {
    let surface = SurfaceId::new(1);
    let root = RootId::new(2);
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(3)]));
    builder.set_root(root, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(surface, SurfacePresentation::with_main(root));
    let workspace = builder.build().expect("fixture should validate");
    let manifest = derive_scene_requirement_draft(
        authority_domain(),
        &workspace,
        version(),
        PresentationConfigRevision::new(7),
        &policy_snapshot(),
        RequirementRevision::new(9),
        &surface_revisions([surface]),
    )
    .expect("requirements should derive")
    .finalize(PopupPlaneRequirement::default())
    .expect("inactive requirements should finalize");
    let requirements = manifest
        .surface(surface)
        .expect("surface should be present");
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(
                LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("bounds should be valid"),
            ),
        )
        .expect("bounds answer is unique");
    let minimum = LogicalSize::new(0.0, 0.0).expect("minimum should be valid");
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .expect("pane answer is unique");
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(56.0).expect("intrinsic should be valid")),
            )
            .expect("tab answer is unique");
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(
                    TabStripMetrics::new(0.0, 0.0).expect("strip should be valid"),
                ),
            )
            .expect("strip answer is unique");
    }
    let newer = DockPolicy::default().snapshot(
        policy_revision()
            .checked_next()
            .expect("fixture revision should advance"),
    );

    assert!(matches!(
        compile_surface_measurements(
            &workspace,
            version(),
            &newer,
            &DockPresentationConfig::default(),
            &manifest,
            &measurements,
            &TabStripStateStore::default(),
            &[],
        ),
        Err(PresentationCompilationError::Scene(
            SceneCompilationError::PolicyRevisionMismatch { ticket, snapshot },
        )) if ticket == policy_revision() && snapshot == newer.revision()
    ));
}
