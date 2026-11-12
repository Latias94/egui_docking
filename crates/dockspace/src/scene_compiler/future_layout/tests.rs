//! Drop previews compared with the independently compiled committed layout.

use std::collections::BTreeMap;

use crate::command::MovePayload;
use crate::drop_resolver::{DropResolution, resolve_presented_drop};
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
    SurfaceSceneRevision, TabIntrinsic, TabStripMetrics,
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
    let manifest = derive_scene_requirement_draft(
        EngineAuthorityDomainId::new_for_test(1),
        workspace,
        WorkspaceVersion::initial(),
        PresentationConfigRevision::new(1),
        policy,
        RequirementRevision::new(1),
        &BTreeMap::from([(SURFACE, SurfaceRequirementRevision::new(1))]),
    )
    .unwrap()
    .finalize(PopupPlaneRequirement::default())
    .unwrap();
    let requirements = manifest.surface(SURFACE).unwrap();
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(LogicalRect::new(0.0, 0.0, 1000.0, 600.0).unwrap()),
        )
        .unwrap();
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(
                key,
                Measurement::Measured(LogicalSize::new(0.0, 0.0).unwrap()),
            )
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
        states.state_mut(TabStripStateKey::new(SURFACE, key.bar()));
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
            SURFACE,
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
