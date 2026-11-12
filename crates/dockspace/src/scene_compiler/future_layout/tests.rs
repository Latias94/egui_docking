//! Drop previews compared with the independently compiled committed layout.

use std::collections::BTreeMap;

use crate::command::MovePayload;
use crate::drop_resolver::{DropResolution, ResolvedDrop, resolve_presented_drop};
use crate::drop_target::{DropTargetId, DropTargetKind};
use crate::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use crate::graph::{Axis, ContainedFloating, Node, RootRecord, SurfacePresentation, Workspace};
use crate::ids::{
    EngineAuthorityDomainId, FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId,
};
use crate::interaction::{DragGeneration, DragSessionId};
use crate::policy::{
    DockPolicy, DockPolicySnapshot, DockTargetRule, DockTargetRuleKey, PolicyRevision,
    TabBarInteraction, TabBarPolicy, TabBarVisibility,
};
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::scene::{PresentationPlan, SurfaceSceneStamp, TabBarRecord, TabBarSceneId};
use crate::scene_compiler::{compile_surface_plan, derive_scene_requirement_draft};
use crate::scene_manifest::{
    Measurement, RequirementRevision, SurfaceMeasurements, SurfaceRequirementRevision,
    SurfaceSceneRevision, TabIntrinsic, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics,
};
use crate::tab_strip::{PopupPlaneRequirement, TabStripStateKey, TabStripStateStore};
use crate::transaction::WorkspaceTransaction;
use crate::transition::WorkspaceVersion;
use crate::workspace::WorkspaceIndex;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const MOVED: ItemId = ItemId::new(1);

fn compile(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    strip: TabStripMetrics,
    states: &mut TabStripStateStore,
) -> PresentationPlan {
    compile_on_surface(workspace, policy, config, strip, states, SURFACE)
}

fn compile_on_surface(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    strip: TabStripMetrics,
    states: &mut TabStripStateStore,
    surface: SurfaceId,
) -> PresentationPlan {
    compile_on_surface_with_minimums(
        workspace,
        policy,
        config,
        strip,
        states,
        surface,
        &BTreeMap::new(),
    )
}

fn compile_on_surface_with_minimums(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    config: &DockPresentationConfig,
    strip: TabStripMetrics,
    states: &mut TabStripStateStore,
    surface: SurfaceId,
    minimums: &BTreeMap<ItemId, LogicalSize>,
) -> PresentationPlan {
    let manifest = derive_scene_requirement_draft(
        EngineAuthorityDomainId::new_for_test(1),
        workspace,
        WorkspaceVersion::initial(),
        PresentationConfigRevision::new(1),
        policy,
        RequirementRevision::new(1),
        &workspace
            .surfaces()
            .map(|(surface, _)| (surface, SurfaceRequirementRevision::new(1)))
            .collect::<BTreeMap<_, _>>(),
    )
    .unwrap()
    .finalize(PopupPlaneRequirement::default())
    .unwrap();
    states
        .reconcile_exact_for_surface_roster(
            manifest.surfaces().flat_map(|(surface, requirements)| {
                requirements.tab_strips().filter_map(move |key| {
                    match workspace.node(key.bar().tabs) {
                        Some(Node::Tabs { items, .. }) => {
                            Some((TabStripStateKey::new(surface, key.bar()), items.as_slice()))
                        }
                        _ => None,
                    }
                })
            }),
            false,
        )
        .unwrap();
    let requirements = manifest.surface(surface).unwrap();
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(LogicalRect::new(0.0, 0.0, 1000.0, 600.0).unwrap()),
        )
        .unwrap();
    for key in requirements.pane_minimums() {
        let minimum = key
            .selected()
            .and_then(|item| minimums.get(&item).copied())
            .unwrap_or_else(|| LogicalSize::new(0.0, 0.0).unwrap());
        measurements
            .insert_pane_minimum(key, Measurement::Measured(minimum))
            .unwrap();
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(
                key,
                Measurement::Measured(TabIntrinsic::new(150.0).unwrap()),
            )
            .unwrap();
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(key, Measurement::Measured(strip))
            .unwrap();
        states.state_mut(TabStripStateKey::new(surface, key.bar()));
    }
    let plan = compile_surface_plan(
        workspace,
        WorkspaceVersion::initial(),
        policy,
        config,
        &manifest,
        &measurements,
        states,
        &[],
        None,
    )
    .unwrap()
    .finalize()
    .unwrap();
    states
        .adopt_resolved_scroll_offsets(
            surface,
            plan.tab_bar_records().iter().map(|bar| {
                (
                    *bar.id(),
                    bar.scroll_offset(),
                    plan.selected_in_tabs(bar.id().tabs),
                    bar.viewport().width(),
                )
            }),
        )
        .unwrap();
    plan
}

fn target_bar(plan: &PresentationPlan, tabs: NodeId) -> &TabBarRecord {
    plan.tab_bar_records()
        .iter()
        .find(|bar| bar.id().tabs == tabs)
        .unwrap()
}

fn resolve_target(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    target_plan: &PresentationPlan,
    source_plan: &PresentationPlan,
    payload: MovePayload,
    target: DropTargetId,
) -> ResolvedDrop {
    let hit = target_plan.drop_target(target).unwrap().region().rect();
    let point =
        LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5).unwrap();
    let version = WorkspaceVersion::initial();
    let query = resolve_presented_drop(
        SurfaceSceneStamp::new(
            target_plan.measurement_ticket().unwrap(),
            SurfaceSceneRevision::new(1),
        ),
        target_plan,
        source_plan.layout_facts(),
        workspace,
        version,
        &WorkspaceIndex::build(workspace, version).unwrap(),
        policy,
        DragSessionId::new(version.epoch(), DragGeneration::new(1)),
        payload,
        None,
        point,
    )
    .unwrap();
    match query.into_evaluation_parts().0 {
        DropResolution::Resolved(resolved) => {
            assert_eq!(resolved.target_id(), target);
            resolved
        }
        resolution => panic!("the measured target must resolve: {resolution:?}"),
    }
}

fn assert_future_tabs_match_committed(
    resolved: &ResolvedDrop,
    committed: &PresentationPlan,
    target: NodeId,
    payload: &[ItemId],
) {
    let future = resolved
        .preview_tab_strip()
        .expect("the measured visible target has future tab facts");
    let bar = target_bar(committed, target);
    assert_eq!(future.bounds, bar.bounds());
    assert_eq!(future.viewport, bar.viewport());
    assert_eq!(future.scroll_offset, bar.scroll_offset());
    assert_eq!(future.maximum_scroll_offset, bar.maximum_scroll_offset());
    assert_eq!(future.tabs.len(), bar.members().len());
    assert_eq!(
        future
            .tabs
            .iter()
            .filter(|tab| tab.payload)
            .map(|tab| tab.item)
            .collect::<Vec<_>>(),
        payload,
    );
    for (future, member) in future.tabs.iter().zip(bar.members()) {
        assert_eq!(future.item, member.tab().item);
        assert_eq!(future.ordinal, member.ordinal());
        assert_eq!(future.full_bounds, member.full_bounds());
        assert_eq!(future.clip_bounds, bar.viewport());
        assert_eq!(
            future.selected,
            committed.selected_in_tabs(target) == Some(future.item)
        );
        let visible = committed
            .tab_records()
            .iter()
            .find(|tab| *tab.id() == member.tab());
        assert_eq!(
            future.visible_bounds,
            visible.map(|tab| tab.visible_bounds())
        );
        assert_eq!(
            future.text_layout_bounds,
            visible.map(|tab| tab.text_layout_bounds())
        );
        assert_eq!(future.text_bounds, visible.map(|tab| tab.text_bounds()));
        assert_eq!(
            future.close_visual_bounds,
            visible.and_then(|tab| tab.close_visual_bounds())
        );
    }
}

#[test]
fn after_tabs_compressed_preview_matches_the_committed_tab_position() {
    check_after_tabs_preview(false);
}

#[test]
fn after_tabs_tail_scrolled_preview_matches_the_committed_tab_position() {
    check_after_tabs_preview(true);
}

#[test]
fn contained_hidden_bar_merge_preview_matches_the_committed_content() {
    for target_kind in [DropTargetKind::TabGap, DropTargetKind::Center] {
        check_contained_bar_merge_preview(
            target_kind,
            TabBarVisibility::Visible,
            TabBarVisibility::Hidden,
        );
    }
}

#[test]
fn contained_visible_bar_center_merge_preview_matches_the_committed_content() {
    check_contained_bar_merge_preview(
        DropTargetKind::Center,
        TabBarVisibility::Hidden,
        TabBarVisibility::Visible,
    );
}

#[test]
fn auto_hidden_single_merge_then_split_previews_match_committed_layouts() {
    let floating_root = RootId::new(2);
    let floating = FloatingPresentationId::new(1);
    let retained = ItemId::new(10);
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([MOVED]));
    let target = builder.insert_node(Node::tabs([retained]));
    builder.set_root(ROOT, RootRecord::new(target).with_central(target));
    builder.set_root(floating_root, RootRecord::new(source));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(
            floating_root,
            LogicalRect::new(600.0, 200.0, 300.0, 300.0).unwrap(),
        ),
    );
    builder.attach_contained(SURFACE, floating).unwrap();
    let mut workspace = builder.build().unwrap();
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::AutoHideSingle,
        TabBarInteraction::Enabled,
    ));
    let policy = policy.snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let mut states = TabStripStateStore::default();
    let plan = compile(&workspace, &policy, &config, strip, &mut states);
    assert!(plan.tab_bar_records().is_empty());
    assert!(
        plan.contained_record(floating).is_some(),
        "the hidden source retains its window title"
    );
    assert_eq!(plan.tab_strip_control_records().len(), 2);

    let merged = resolve_target(
        &workspace,
        &policy,
        &plan,
        &plan,
        MovePayload::Subtree(
            workspace
                .capture_node_source(floating_root, source)
                .unwrap(),
        ),
        DropTargetId::Center {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
        },
    );
    WorkspaceTransaction::from_commands([merged.command().clone()])
        .apply(&mut workspace, &policy)
        .unwrap();
    let merged_plan = compile(&workspace, &policy, &config, strip, &mut states);
    assert_future_tabs_match_committed(&merged, &merged_plan, target, &[MOVED]);
    assert_eq!(
        merged.visual().rect(),
        merged_plan
            .pane_records()
            .iter()
            .find(|pane| pane.id().tabs == target)
            .unwrap()
            .content_bounds()
    );

    let edge = DropTargetId::OuterEdge {
        surface: SURFACE,
        root: ROOT,
        node: target,
        edge: crate::command::Edge::Right,
    };
    let separated = resolve_target(
        &workspace,
        &policy,
        &merged_plan,
        &merged_plan,
        MovePayload::Item(workspace.capture_item_source(ROOT, target, MOVED).unwrap()),
        edge,
    );
    WorkspaceTransaction::from_commands([separated.command().clone()])
        .apply(&mut workspace, &policy)
        .unwrap();
    let separated_plan = compile(&workspace, &policy, &config, strip, &mut states);
    assert!(
        separated_plan.tab_bar_records().is_empty(),
        "both single-member leaves hide after the split"
    );
    assert_eq!(separated_plan.tab_strip_control_records().len(), 2);
    let moved_pane = separated_plan
        .pane_records()
        .iter()
        .find(|pane| separated_plan.pane_selected(pane) == Some(MOVED))
        .unwrap();
    assert_eq!(
        separated.visual().rect(),
        moved_pane.content_bounds(),
        "2→1 membership changes must use hidden-bar minimums in the future layout"
    );
}

#[test]
fn future_split_preserves_an_unchanged_revealed_single_sibling() {
    let retained = ItemId::new(2);
    let revealed = ItemId::new(10);
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([MOVED, retained]));
    let sibling = builder.insert_node(Node::tabs([revealed]));
    let split = builder.insert_node(Node::equal_split(Axis::Vertical, [source, sibling]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(sibling));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut workspace = builder.build().unwrap();
    let mut policy = DockPolicy::default();
    policy.set_tab_bar(TabBarPolicy::new(
        TabBarVisibility::AutoHideSingle,
        TabBarInteraction::Enabled,
    ));
    let policy = policy.snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let minimums = BTreeMap::from([
        (MOVED, LogicalSize::new(100.0, 250.0).unwrap()),
        (retained, LogicalSize::new(100.0, 250.0).unwrap()),
        (revealed, LogicalSize::new(100.0, 310.0).unwrap()),
    ]);
    let mut states = TabStripStateStore::default();
    compile_on_surface_with_minimums(
        &workspace,
        &policy,
        &config,
        strip,
        &mut states,
        SURFACE,
        &minimums,
    );
    let sibling_key = TabStripStateKey::new(
        SURFACE,
        TabBarSceneId {
            root: ROOT,
            tabs: sibling,
        },
    );
    states.reveal_single_tab_bar(sibling_key).unwrap();
    let plan = compile_on_surface_with_minimums(
        &workspace,
        &policy,
        &config,
        strip,
        &mut states,
        SURFACE,
        &minimums,
    );
    assert!(
        plan.tab_bar_records()
            .iter()
            .any(|bar| bar.id().tabs == sibling)
    );
    let edge = plan.drop_targets().iter().map(|record| record.id()).find(|id| {
        matches!(id, DropTargetId::InnerEdge { node, edge: crate::command::Edge::Right, .. } if *node == source)
    }).unwrap();
    let resolved = resolve_target(
        &workspace,
        &policy,
        &plan,
        &plan,
        MovePayload::Item(workspace.capture_item_source(ROOT, source, MOVED).unwrap()),
        edge,
    );
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut workspace, &policy)
        .unwrap();
    let committed = compile_on_surface_with_minimums(
        &workspace,
        &policy,
        &config,
        strip,
        &mut states,
        SURFACE,
        &minimums,
    );
    assert!(
        committed
            .tab_bar_records()
            .iter()
            .any(|bar| bar.id().tabs == sibling)
    );
    assert_eq!(states.revealed_single_item(sibling_key), Some(revealed));
    let moved_pane = committed
        .pane_records()
        .iter()
        .find(|pane| committed.pane_selected(pane) == Some(MOVED))
        .unwrap();
    assert_eq!(
        resolved.visual().rect(),
        moved_pane.content_bounds(),
        "candidate minimums must retain the actual presented reveal of an unchanged sibling"
    );
}

fn check_contained_bar_merge_preview(
    target_kind: DropTargetKind,
    target_visibility: TabBarVisibility,
    payload_visibility: TabBarVisibility,
) {
    let floating_root = RootId::new(2);
    let floating = FloatingPresentationId::new(1);
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([MOVED, ItemId::new(2)]));
    let target = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    builder.set_root(ROOT, RootRecord::new(target).with_central(target));
    builder.set_root(floating_root, RootRecord::new(source));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_contained_floating(
        floating,
        ContainedFloating::new(
            floating_root,
            LogicalRect::new(600.0, 200.0, 300.0, 300.0).unwrap(),
        ),
    );
    builder.attach_contained(SURFACE, floating).unwrap();
    let workspace = builder.build().unwrap();
    let mut policy = DockPolicy::default();
    for (item, visibility) in [
        (ItemId::new(10), target_visibility),
        (MOVED, payload_visibility),
    ] {
        let mut rule = DockTargetRule::new();
        rule.set_tab_bar(TabBarPolicy::new(visibility, TabBarInteraction::Enabled));
        policy.set_target_rule(DockTargetRuleKey::Item(item), rule);
    }
    let policy = policy.snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let mut states = TabStripStateStore::default();
    let plan = compile(&workspace, &policy, &config, strip, &mut states);
    assert!(plan.contained_record(floating).is_some());
    assert_eq!(
        plan.tab_bar_records()
            .iter()
            .any(|bar| bar.id().root == floating_root),
        payload_visibility == TabBarVisibility::Visible,
    );
    assert_eq!(
        plan.tab_bar_records()
            .iter()
            .any(|bar| bar.id().root == ROOT),
        target_visibility == TabBarVisibility::Visible,
    );
    let target_id = match target_kind {
        DropTargetKind::TabGap => DropTargetId::TabGap {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
            index: 1,
        },
        DropTargetKind::Center => DropTargetId::Center {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
        },
        _ => panic!("the regression requires a merge target: {target_kind:?}"),
    };
    let hit = plan.drop_target(target_id).unwrap().region().rect();
    let point =
        LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5).unwrap();
    let version = WorkspaceVersion::initial();
    let query = resolve_presented_drop(
        SurfaceSceneStamp::new(
            plan.measurement_ticket().unwrap(),
            SurfaceSceneRevision::new(1),
        ),
        &plan,
        plan.layout_facts(),
        &workspace,
        version,
        &WorkspaceIndex::build(&workspace, version).unwrap(),
        &policy,
        DragSessionId::new(version.epoch(), DragGeneration::new(1)),
        MovePayload::Subtree(
            workspace
                .capture_node_source(floating_root, source)
                .unwrap(),
        ),
        None,
        point,
    )
    .expect("a tab-bar visibility change must preserve a legal whole-window merge preview");
    let DropResolution::Resolved(resolved) = query.resolution() else {
        panic!("the floating group must merge through {target_kind:?}: {query:?}");
    };
    assert_eq!(resolved.target_id(), target_id);
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile(&committed, &policy, &config, strip, &mut states);
    assert_eq!(
        committed_plan
            .tab_bar_records()
            .iter()
            .any(|bar| bar.id().tabs == target),
        payload_visibility == TabBarVisibility::Visible,
    );
    assert!(committed_plan.contained_record(floating).is_none());
    let pane = committed_plan
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == target)
        .unwrap();
    assert_eq!(committed_plan.pane_selected(pane), Some(MOVED));
    assert_eq!(
        resolved.visual().rect(),
        pane.content_bounds(),
        "{target_kind:?} must preview the committed content area when selection changes the bar from {target_visibility:?} to {payload_visibility:?}",
    );
    if payload_visibility == TabBarVisibility::Hidden
        || target_visibility == TabBarVisibility::Hidden
    {
        assert!(
            resolved.preview_tab_strip().is_none(),
            "hidden or unmeasured tab chrome must not be fabricated"
        );
    } else {
        assert_future_tabs_match_committed(
            resolved,
            &committed_plan,
            target,
            &[MOVED, ItemId::new(2)],
        );
    }
}

fn check_after_tabs_preview(tail_scrolled: bool) {
    let target_items = (10..if tail_scrolled { 16 } else { 13 })
        .map(ItemId::new)
        .collect::<Vec<_>>();
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([MOVED, ItemId::new(2)]));
    let target = builder.insert_node(Node::tabs_with_selection(
        target_items.iter().copied(),
        target_items.last().copied(),
    ));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [source, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(target));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::builder()
        .tab_max_width(if tail_scrolled { 72.0 } else { 220.0 })
        .build()
        .unwrap();
    let strip = TabStripMetrics::new(0.0, 0.0)
        .unwrap()
        .with_after_tabs_width(if tail_scrolled { 96.0 } else { 64.0 })
        .unwrap();
    let mut states = TabStripStateStore::default();
    let mut plan = compile(&workspace, &policy, &config, strip, &mut states);
    if tail_scrolled {
        let maximum = target_bar(&plan, target).maximum_scroll_offset();
        assert!(maximum > strip.after_tabs_width() * 0.5);
        states
            .state_mut(TabStripStateKey::new(
                SURFACE,
                TabBarSceneId {
                    root: ROOT,
                    tabs: target,
                },
            ))
            .set_scroll_offset(maximum)
            .unwrap();
        plan = compile(&workspace, &policy, &config, strip, &mut states);
        assert_eq!(target_bar(&plan, target).scroll_offset(), maximum);
    }
    let index = if tail_scrolled { 4 } else { 1 };
    let target_id = DropTargetId::TabGap {
        surface: SURFACE,
        root: ROOT,
        tabs: target,
        index,
    };
    let hit = plan.drop_target(target_id).unwrap().region().rect();
    let point =
        LogicalPoint::new(hit.x() + hit.width() * 0.5, hit.y() + hit.height() * 0.5).unwrap();
    let version = WorkspaceVersion::initial();
    let query = resolve_presented_drop(
        SurfaceSceneStamp::new(
            plan.measurement_ticket().unwrap(),
            SurfaceSceneRevision::new(1),
        ),
        &plan,
        plan.layout_facts(),
        &workspace,
        version,
        &WorkspaceIndex::build(&workspace, version).unwrap(),
        &policy,
        DragSessionId::new(version.epoch(), DragGeneration::new(1)),
        MovePayload::Item(workspace.capture_item_source(ROOT, source, MOVED).unwrap()),
        None,
        point,
    )
    .unwrap();
    let DropResolution::Resolved(resolved) = query.resolution() else {
        panic!("the presented tab gap must resolve: {query:?}");
    };
    assert_eq!(resolved.target_id(), target_id);
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile(&committed, &policy, &config, strip, &mut states);
    assert_future_tabs_match_committed(resolved, &committed_plan, target, &[MOVED]);
    let bar = target_bar(&committed_plan, target);
    let inserted = bar
        .members()
        .iter()
        .find(|member| member.tab().item == MOVED)
        .unwrap()
        .full_bounds();
    let marker_width = config.splitter_thickness().min(bar.viewport().width());
    let expected_x = (inserted.x() - marker_width * 0.5).clamp(
        bar.viewport().x(),
        (bar.viewport().max().x() - marker_width).max(bar.viewport().x()),
    );
    assert_eq!(
        resolved.visual().rect().x(),
        expected_x,
        "the insertion preview must use the committed strip's after-tabs width and scroll extent",
    );
}

#[test]
fn center_and_gap_merges_project_every_payload_title_after_the_source_pane_disappears() {
    for gap in [false, true] {
        let payload_items = [MOVED, ItemId::new(2), ItemId::new(3)];
        let mut builder = Workspace::builder();
        let source = builder.insert_node(Node::tabs_with_selection(
            payload_items,
            Some(ItemId::new(2)),
        ));
        let middle = builder.insert_node(Node::tabs([ItemId::new(4)]));
        let target = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
        let split = builder
            .insert_node(Node::equal_split(Axis::Horizontal, [source, middle, target]).unwrap());
        builder.set_root(ROOT, RootRecord::new(split).with_central(target));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        let workspace = builder.build().unwrap();
        let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
        let config = DockPresentationConfig::default();
        let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
        let mut states = TabStripStateStore::default();
        let plan = compile(&workspace, &policy, &config, strip, &mut states);
        let resolved = resolve_target(
            &workspace,
            &policy,
            &plan,
            &plan,
            MovePayload::Tabs(workspace.capture_node_source(ROOT, source).unwrap()),
            if gap {
                DropTargetId::TabGap {
                    surface: SURFACE,
                    root: ROOT,
                    tabs: target,
                    index: 1,
                }
            } else {
                DropTargetId::Center {
                    surface: SURFACE,
                    root: ROOT,
                    tabs: target,
                }
            },
        );
        let mut committed = workspace.clone();
        WorkspaceTransaction::from_commands([resolved.command().clone()])
            .apply(&mut committed, &policy)
            .unwrap();
        let committed_plan = compile(&committed, &policy, &config, strip, &mut states);
        assert_future_tabs_match_committed(&resolved, &committed_plan, target, &payload_items);
        assert_ne!(
            target_bar(&plan, target).bounds(),
            target_bar(&committed_plan, target).bounds()
        );
        assert!(committed.node(source).is_none());
        let future = resolved.preview_tab_strip().unwrap();
        let expected = if gap {
            [
                ItemId::new(10),
                MOVED,
                ItemId::new(2),
                ItemId::new(3),
                ItemId::new(11),
            ]
        } else {
            [
                ItemId::new(10),
                ItemId::new(11),
                MOVED,
                ItemId::new(2),
                ItemId::new(3),
            ]
        };
        assert_eq!(
            future.tabs.iter().map(|tab| tab.item).collect::<Vec<_>>(),
            expected
        );
        assert_eq!(
            future.tabs.iter().find(|tab| tab.selected).unwrap().item,
            ItemId::new(2)
        );
    }
}

#[test]
fn same_strip_gap_preview_removes_the_source_before_computing_future_order() {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([MOVED, ItemId::new(2), ItemId::new(3)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let mut states = TabStripStateStore::default();
    let plan = compile(&workspace, &policy, &config, strip, &mut states);
    let resolved = resolve_target(
        &workspace,
        &policy,
        &plan,
        &plan,
        MovePayload::Item(workspace.capture_item_source(ROOT, tabs, MOVED).unwrap()),
        DropTargetId::TabGap {
            surface: SURFACE,
            root: ROOT,
            tabs,
            index: 2,
        },
    );
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile(&committed, &policy, &config, strip, &mut states);
    assert_future_tabs_match_committed(&resolved, &committed_plan, tabs, &[MOVED]);
    assert_eq!(
        resolved
            .preview_tab_strip()
            .unwrap()
            .tabs
            .iter()
            .map(|tab| tab.item)
            .collect::<Vec<_>>(),
        [ItemId::new(2), MOVED, ItemId::new(3)]
    );
}

#[test]
fn cross_surface_group_preview_uses_the_target_strip_and_retained_source_measurements() {
    let source_surface = SurfaceId::new(2);
    let source_root = RootId::new(2);
    let payload_items = [MOVED, ItemId::new(2), ItemId::new(3)];
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs_with_selection(
        payload_items,
        Some(ItemId::new(2)),
    ));
    let target = builder.insert_node(Node::tabs([ItemId::new(10), ItemId::new(11)]));
    builder.set_root(ROOT, RootRecord::new(target).with_central(target));
    builder.set_root(source_root, RootRecord::new(source));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(source_surface, SurfacePresentation::with_main(source_root));
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let target_strip = TabStripMetrics::new(23.0, 41.0)
        .unwrap()
        .with_after_tabs_width(67.0)
        .unwrap();
    let source_strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let mut states = TabStripStateStore::default();
    let target_plan = compile(&workspace, &policy, &config, target_strip, &mut states);
    let source_plan = compile_on_surface(
        &workspace,
        &policy,
        &config,
        source_strip,
        &mut states,
        source_surface,
    );
    let resolved = resolve_target(
        &workspace,
        &policy,
        &target_plan,
        &source_plan,
        MovePayload::Tabs(workspace.capture_node_source(source_root, source).unwrap()),
        DropTargetId::Center {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
        },
    );
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile(&committed, &policy, &config, target_strip, &mut states);
    assert_future_tabs_match_committed(&resolved, &committed_plan, target, &payload_items);
    assert!(committed.surface(source_surface).is_none());
}

#[test]
fn empty_central_merge_projects_the_new_group_chrome_and_all_payload_members() {
    let payload_items = [MOVED, ItemId::new(2)];
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs(payload_items));
    let target = builder.insert_node(Node::tabs([]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [source, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(target));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let mut states = TabStripStateStore::default();
    let plan = compile(&workspace, &policy, &config, strip, &mut states);
    let resolved = resolve_target(
        &workspace,
        &policy,
        &plan,
        &plan,
        MovePayload::Tabs(workspace.capture_node_source(ROOT, source).unwrap()),
        DropTargetId::Center {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
        },
    );
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile(&committed, &policy, &config, strip, &mut states);
    assert_future_tabs_match_committed(&resolved, &committed_plan, target, &payload_items);
    assert!(
        resolved
            .preview_tab_strip()
            .unwrap()
            .tabs
            .iter()
            .all(|tab| tab.payload)
    );
}

#[test]
fn overflowing_group_preview_keeps_every_payload_member_and_matches_committed_clipping() {
    let payload_items = (1..17).map(ItemId::new).collect::<Vec<_>>();
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs_with_selection(
        payload_items.iter().copied(),
        Some(ItemId::new(8)),
    ));
    let target = builder.insert_node(Node::tabs([ItemId::new(20), ItemId::new(21)]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [source, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(target));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::builder()
        .tab_min_width(80.0)
        .tab_max_width(160.0)
        .build()
        .unwrap();
    let control =
        TabStripControlMetric::new(20.0, TabStripControlPlacement::ReservedTrailing).unwrap();
    let controls = TabStripControlMetrics::new(4.0)
        .unwrap()
        .with_scroll_backward(control)
        .with_scroll_forward(control)
        .with_tab_list_menu(control);
    let strip = TabStripMetrics::new(12.0, 8.0)
        .unwrap()
        .with_after_tabs_width(60.0)
        .unwrap()
        .with_controls(controls);
    let mut states = TabStripStateStore::default();
    let plan = compile(&workspace, &policy, &config, strip, &mut states);
    let resolved = resolve_target(
        &workspace,
        &policy,
        &plan,
        &plan,
        MovePayload::Tabs(workspace.capture_node_source(ROOT, source).unwrap()),
        DropTargetId::Center {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
        },
    );
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile(&committed, &policy, &config, strip, &mut states);
    assert_future_tabs_match_committed(&resolved, &committed_plan, target, &payload_items);
    let future = resolved.preview_tab_strip().unwrap();
    assert!(future.maximum_scroll_offset > 0.0);
    assert!(
        future
            .tabs
            .iter()
            .any(|tab| tab.payload && tab.visible_bounds.is_none())
    );
    assert!(
        future
            .tabs
            .iter()
            .any(|tab| tab.payload && tab.visible_bounds.is_some())
    );
    assert!(
        future
            .tabs
            .iter()
            .find(|tab| tab.selected)
            .unwrap()
            .visible_bounds
            .is_some()
    );
}

#[test]
fn center_merge_with_minimum_overflow_clips_content_and_tabs_to_the_same_root_bounds() {
    let source_surface = SurfaceId::new(2);
    let source_root = RootId::new(2);
    let left_item = ItemId::new(20);
    let target_item = ItemId::new(10);
    let mut builder = Workspace::builder();
    let source = builder.insert_node(Node::tabs([MOVED, ItemId::new(2)]));
    let left = builder.insert_node(Node::tabs([left_item]));
    let target = builder.insert_node(Node::tabs([target_item]));
    let split = builder.insert_node(Node::equal_split(Axis::Horizontal, [left, target]).unwrap());
    builder.set_root(ROOT, RootRecord::new(split).with_central(target));
    builder.set_root(source_root, RootRecord::new(source));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(source_surface, SurfacePresentation::with_main(source_root));
    let workspace = builder.build().unwrap();
    let policy = DockPolicy::default().snapshot(PolicyRevision::new(1));
    let config = DockPresentationConfig::default();
    let strip = TabStripMetrics::new(0.0, 0.0).unwrap();
    let minimums = BTreeMap::from([
        (left_item, LogicalSize::new(600.0, 32.0).unwrap()),
        (target_item, LogicalSize::new(32.0, 32.0).unwrap()),
        (MOVED, LogicalSize::new(700.0, 32.0).unwrap()),
    ]);
    let mut states = TabStripStateStore::default();
    let target_plan = compile_on_surface_with_minimums(
        &workspace,
        &policy,
        &config,
        strip,
        &mut states,
        SURFACE,
        &minimums,
    );
    let source_plan = compile_on_surface_with_minimums(
        &workspace,
        &policy,
        &config,
        strip,
        &mut states,
        source_surface,
        &minimums,
    );
    let resolved = resolve_target(
        &workspace,
        &policy,
        &target_plan,
        &source_plan,
        MovePayload::Tabs(workspace.capture_node_source(source_root, source).unwrap()),
        DropTargetId::Center {
            surface: SURFACE,
            root: ROOT,
            tabs: target,
        },
    );
    let mut committed = workspace.clone();
    WorkspaceTransaction::from_commands([resolved.command().clone()])
        .apply(&mut committed, &policy)
        .unwrap();
    let committed_plan = compile_on_surface_with_minimums(
        &committed,
        &policy,
        &config,
        strip,
        &mut states,
        SURFACE,
        &minimums,
    );
    let pane = committed_plan
        .pane_records()
        .iter()
        .find(|pane| pane.id().tabs == target)
        .unwrap();
    assert!(
        pane.bounds().width() < minimums[&MOVED].width(),
        "the regression must exercise an overflowing minimum"
    );
    assert_eq!(resolved.visual().rect(), pane.content_bounds());
    let future = resolved.preview_tab_strip().unwrap();
    assert_eq!(future.bounds.x(), pane.bounds().x());
    assert_eq!(future.bounds.width(), pane.content_bounds().width());
    assert_eq!(future.bounds.max().y(), pane.content_bounds().y());
    assert!(resolved.visual().rect().max().x() <= target_plan.bounds().max().x());
    assert_future_tabs_match_committed(
        &resolved,
        &committed_plan,
        target,
        &[MOVED, ItemId::new(2)],
    );
}
