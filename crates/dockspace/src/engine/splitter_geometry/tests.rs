use std::sync::Arc;

use super::*;
use crate::drop_target::SceneLayerKey;
use crate::geometry::Constraints;
use crate::graph::{RootRecord, SurfacePresentation, WorkspaceBuilder};
use crate::hit_region::HitRegion;
use crate::ids::{EngineAuthorityDomainId, ItemId, SurfaceId, WorkspaceEpoch};
use crate::layout::{
    LayoutMetrics, SplitWeightOverride, project_root, project_root_with_overrides,
};
use crate::policy::PolicyRevision;
use crate::presentation_config::{DockPresentationConfig, PresentationConfigRevision};
use crate::scene::{RootLayoutFacts, SplitterSceneId};
use crate::scene_manifest::{
    Measurement, SurfaceMeasurementTicket, SurfaceMeasurements, SurfaceRequirementRevision,
    SurfaceRequirements,
};

const ROOT: RootId = RootId::new(1);
const SURFACE: SurfaceId = SurfaceId::new(1);

struct Fixture {
    workspace: Workspace,
    plan: PresentationPlan,
    projection: Arc<LayoutProjection>,
    constraints: BTreeMap<NodeId, Constraints>,
    bounds: LogicalRect,
    metrics: LayoutMetrics,
}

impl Fixture {
    fn new(
        workspace: Workspace,
        bounds: LogicalRect,
        constraints: BTreeMap<NodeId, Constraints>,
        thickness: f64,
    ) -> Self {
        let metrics = LayoutMetrics::new(thickness).unwrap();
        let projection =
            Arc::new(project_root(&workspace, ROOT, bounds, &constraints, metrics).unwrap());
        let ticket = SurfaceMeasurementTicket::new(
            EngineAuthorityDomainId::new_for_test(1),
            WorkspaceEpoch::new(1),
            PresentationConfigRevision::new(1),
            PolicyRevision::new(1),
            SurfaceRequirementRevision::new(1),
            SURFACE,
        );
        let requirements = SurfaceRequirements::new(
            ticket,
            Default::default(),
            Default::default(),
            Default::default(),
            Default::default(),
        )
        .unwrap();
        let mut measurements = SurfaceMeasurements::new(ticket);
        measurements
            .set_bounds(requirements.bounds(), Measurement::Measured(bounds))
            .unwrap();
        let measurements = requirements
            .validate(&measurements)
            .unwrap()
            .require_authoritative()
            .unwrap();
        let config = DockPresentationConfig::builder()
            .splitter_thickness(thickness)
            .build()
            .unwrap();
        let mut plan = PresentationPlan::new(SURFACE, bounds);
        plan.install_layout_facts(config, measurements);
        plan.push_root_layout_facts(RootLayoutFacts::new(
            ROOT,
            bounds,
            BTreeMap::new(),
            Arc::clone(&projection),
        ));
        Self {
            workspace,
            plan,
            projection,
            constraints,
            bounds,
            metrics,
        }
    }

    fn handle(&self, node: NodeId, index: usize) -> (NodeSource, SplitterRecord) {
        let Node::Split {
            axis,
            children,
            weights,
        } = self.workspace.node(node).unwrap()
        else {
            unreachable!();
        };
        let split = self.projection.splits.get(&node).unwrap();
        let draw = split.splitter_rects[index];
        let record = SplitterRecord::new(
            SplitterSceneId {
                root: ROOT,
                split: node,
                index,
            },
            draw,
            HitRegion::new(draw),
            *axis,
            self.projection.node_rects[&children[index]],
            self.projection.node_rects[&children[index + 1]],
            split.child_minimum_extents[index],
            split.child_minimum_extents[index + 1],
            split.child_maximum_extents[index],
            split.child_maximum_extents[index + 1],
            split.child_extents.to_vec(),
            split.central_index,
            weights.clone(),
            SceneLayerKey::surface_base(),
            true,
        );
        (
            self.workspace.capture_node_source(ROOT, node).unwrap(),
            record,
        )
    }

    fn groups(&self, handles: &[(NodeId, usize)]) -> Vec<FrozenResizeAxisGroup> {
        prepare_resize_axis_groups(
            &self.workspace,
            &self.plan,
            handles
                .iter()
                .map(|&(node, index)| self.handle(node, index))
                .collect(),
        )
        .unwrap()
    }

    fn project_updates(&self, updates: &[SplitResize]) -> LayoutProjection {
        let overrides = updates
            .iter()
            .map(|update| SplitWeightOverride::new(update.split(), update.weights()))
            .collect::<Vec<_>>();
        project_root_with_overrides(
            &self.workspace,
            ROOT,
            self.bounds,
            &self.constraints,
            self.metrics,
            &overrides,
        )
        .unwrap()
    }
}

fn rect(width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(0.0, 0.0, width, height).unwrap()
}

fn constraints(minimum_width: f64, maximum_width: f64) -> Constraints {
    Constraints::new(
        LogicalSize::new(minimum_width, 20.0).unwrap(),
        LogicalSize::new(maximum_width, 2000.0).unwrap(),
    )
    .unwrap()
}

fn tabs(builder: &mut WorkspaceBuilder, item: u64) -> NodeId {
    builder.insert_node(Node::tabs([ItemId::new(item)]))
}

fn finish(mut builder: WorkspaceBuilder, node: NodeId, central: Option<NodeId>) -> Workspace {
    let record = match central {
        Some(central) => RootRecord::new(node).with_central(central),
        None => RootRecord::new(node),
    };
    builder.set_root(ROOT, record);
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().unwrap()
}

fn nested(central: usize) -> (Fixture, [NodeId; 7]) {
    let mut builder = Workspace::builder();
    let a = tabs(&mut builder, 1);
    let b = tabs(&mut builder, 2);
    let c = tabs(&mut builder, 3);
    let d = tabs(&mut builder, 4);
    let inner = builder.insert_node(Node::split(Axis::Horizontal, [c, d], [0.5, 0.5]).unwrap());
    let across = builder.insert_node(Node::split(Axis::Vertical, [b, inner], [0.5, 0.5]).unwrap());
    let outer =
        builder.insert_node(Node::split(Axis::Horizontal, [a, across], [0.5, 0.5]).unwrap());
    let central = match central {
        1 => Some(c),
        2 => Some(d),
        _ => None,
    };
    let workspace = finish(builder, outer, central);
    let fixture = Fixture::new(
        workspace,
        rect(1001.0, 601.0),
        BTreeMap::from([
            (a, constraints(100.0, 2000.0)),
            (b, constraints(100.0, 510.0)),
            (c, constraints(200.0, 2000.0)),
            (d, constraints(50.0, 2000.0)),
        ]),
        1.0,
    );
    (fixture, [a, b, c, d, inner, across, outer])
}

fn close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= 0.0002,
        "{actual} != {expected}"
    );
}

#[test]
fn nested_touching_frontier_uses_descendant_limits_and_keeps_the_far_side_fixed() {
    for central in 0..=2 {
        let (fixture, [a, b, c, d, inner, across, outer]) = nested(central);
        let groups = fixture.groups(&[(outer, 0)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups[0]
                .sources()
                .map(NodeSource::node)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([outer, across, inner])
        );
        assert_eq!(
            groups[0]
                .resized_sources()
                .map(NodeSource::node)
                .collect::<BTreeSet<_>>(),
            BTreeSet::from([outer, inner])
        );
        close(groups[0].common_delta.minimum, -10.0);
        close(groups[0].common_delta.maximum, 49.5);
        let sources = groups[0].sources().collect::<Vec<_>>();
        assert!(
            sources
                .iter()
                .all(|source| sources[0].reference() == source.reference())
        );
        for (requested, effective) in [(1e6, 49.5), (-1e6, -10.0), (18.0, 18.0)] {
            let updates = resize_axis_group_updates(&groups, [requested, 0.0]).unwrap();
            assert_eq!(updates.len(), 2);
            let projected = fixture.project_updates(&updates);
            close(projected.node_rects[&a].width(), 500.0 + effective);
            close(projected.node_rects[&b].width(), 500.0 - effective);
            close(projected.node_rects[&c].width(), 249.5 - effective);
            close(projected.node_rects[&d].width(), 249.5);
            close(
                projected.node_rects[&d].x(),
                fixture.projection.node_rects[&d].x(),
            );
        }
    }
}

#[test]
fn two_axes_keep_orthogonal_sources_but_emit_each_split_once() {
    let (fixture, [_, b, _, d, inner, across, outer]) = nested(0);
    let groups = fixture.groups(&[(outer, 0), (across, 0)]);
    assert_eq!(groups.len(), 2);
    for group in &groups {
        assert!(group.sources().any(|source| source.node() == inner));
        assert!(group.resized_sources().all(|source| {
            matches!(fixture.workspace.node(source.node()), Some(Node::Split { axis, .. }) if *axis == group.axis)
        }));
    }
    let updates = resize_axis_group_updates(&groups, [18.0, 24.0]).unwrap();
    assert_eq!(updates.len(), 3);
    assert_eq!(
        updates
            .iter()
            .map(|update| update.split().node())
            .collect::<BTreeSet<_>>(),
        BTreeSet::from([outer, across, inner])
    );
    let projected = fixture.project_updates(&updates);
    close(projected.node_rects[&b].width(), 482.0);
    close(projected.node_rects[&b].height(), 324.0);
    close(projected.node_rects[&d].width(), 249.5);
    close(projected.node_rects[&d].height(), 276.0);
    close(
        projected.node_rects[&d].x(),
        fixture.projection.node_rects[&d].x(),
    );
}

#[test]
fn adjacent_incident_gaps_merge_before_solving_an_nary_split() {
    let mut builder = Workspace::builder();
    let a = tabs(&mut builder, 1);
    let b = tabs(&mut builder, 2);
    let c = tabs(&mut builder, 3);
    let outer =
        builder.insert_node(Node::split(Axis::Horizontal, [a, b, c], [0.25, 0.5, 0.25]).unwrap());
    let fixture = Fixture::new(
        finish(builder, outer, None),
        rect(1002.0, 300.0),
        [a, b, c]
            .into_iter()
            .map(|node| (node, constraints(20.0, 2000.0)))
            .collect(),
        1.0,
    );
    let groups = fixture.groups(&[(outer, 0), (outer, 1), (outer, 0)]);
    let updates = resize_axis_group_updates(&groups, [20.0, 0.0]).unwrap();
    assert_eq!(updates.len(), 1);
    let projected = fixture.project_updates(&updates);
    close(projected.node_rects[&a].width(), 270.0);
    close(projected.node_rects[&b].width(), 500.0);
    close(projected.node_rects[&b].x(), 271.0);
    close(projected.node_rects[&c].width(), 230.0);
    let mut edge = Some(1.0);
    assert!(merge_edge(&mut edge, -1.0).is_none());
}

fn constrained() -> (Fixture, NodeId) {
    let mut builder = Workspace::builder();
    let a = tabs(&mut builder, 1);
    let b = tabs(&mut builder, 2);
    let split = builder.insert_node(Node::split(Axis::Horizontal, [a, b], [0.25, 0.75]).unwrap());
    (
        Fixture::new(
            finish(builder, split, None),
            rect(401.0, 300.0),
            BTreeMap::from([
                (a, constraints(200.0, 400.0)),
                (b, constraints(100.0, 400.0)),
            ]),
            1.0,
        ),
        split,
    )
}

#[test]
fn zero_effective_motion_preserves_constrained_split_intent_bitwise() {
    let (fixture, split) = constrained();
    let groups = fixture.groups(&[(split, 0)]);
    assert_eq!(
        fixture.projection.splits.get(&split).unwrap().child_extents,
        [200.0, 200.0]
    );
    for requested in [0.0, -48.0] {
        let updates = resize_axis_group_updates(&groups, [requested, 0.0]).unwrap();
        assert_eq!(
            updates[0]
                .weights()
                .iter()
                .map(|weight| weight.get().to_bits())
                .collect::<Vec<_>>(),
            [0.25_f32.to_bits(), 0.75_f32.to_bits()]
        );
    }
}

#[test]
fn returning_to_the_press_boundary_restores_the_original_intent() {
    let (fixture, split) = constrained();
    let groups = fixture.groups(&[(split, 0)]);
    let moved = resize_axis_group_updates(&groups, [50.0, 0.0]).unwrap();
    assert_eq!(
        moved[0]
            .weights()
            .iter()
            .map(|weight| weight.get())
            .collect::<Vec<_>>(),
        [0.625, 0.375]
    );
    let returned = resize_axis_group_updates(&groups, [0.0, 0.0]).unwrap();
    assert_eq!(
        returned[0].weights(),
        groups[0].splits[0].weights.as_slice()
    );
}

#[test]
fn missing_facts_overflow_unallocated_and_compressed_gaps_are_unavailable() {
    let (fixture, split) = constrained();
    let plan = PresentationPlan::new(SURFACE, fixture.bounds);
    assert!(
        prepare_resize_axis_groups(&fixture.workspace, &plan, vec![fixture.handle(split, 0)])
            .is_none()
    );
    for (width, thickness, minimum, maximum) in [
        (101.0, 1.0, 100.0, 1000.0),
        (401.0, 1.0, 0.0, 100.0),
        (0.5, 1.0, 0.0, 1000.0),
    ] {
        let mut builder = Workspace::builder();
        let a = tabs(&mut builder, 1);
        let b = tabs(&mut builder, 2);
        let split = builder.insert_node(Node::split(Axis::Horizontal, [a, b], [0.5, 0.5]).unwrap());
        let fixture = Fixture::new(
            finish(builder, split, None),
            rect(width, 300.0),
            [a, b]
                .into_iter()
                .map(|node| (node, constraints(minimum, maximum)))
                .collect(),
            thickness,
        );
        assert!(
            prepare_resize_axis_groups(
                &fixture.workspace,
                &fixture.plan,
                vec![fixture.handle(split, 0)]
            )
            .is_none()
        );
    }
}

#[test]
fn invalid_displacements_and_duplicate_axis_groups_are_unavailable() {
    let (fixture, split) = constrained();
    let groups = fixture.groups(&[(split, 0)]);
    assert!(resize_axis_group_updates(&groups, [f64::NAN, 0.0]).is_none());
    assert!(resize_axis_group_updates(&groups, [0.0, f64::INFINITY]).is_none());
    assert!(
        resize_axis_group_updates(&[groups[0].clone(), groups[0].clone()], [1.0, 0.0]).is_none()
    );
}

#[test]
fn fractional_origins_bounds_and_gap_thickness_keep_valid_geometry_available() {
    let mut rounded_gap = false;
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for (x, y) in [(127.3, 91.7), (-127.3, -91.7), (1_000_000.3, 1_000_000.7)] {
            for thickness in [1.0, 0.3, 1.25] {
                let mut builder = Workspace::builder();
                let a = tabs(&mut builder, 1);
                let b = tabs(&mut builder, 2);
                let split = builder.insert_node(Node::split(axis, [a, b], [0.25, 0.75]).unwrap());
                let fixture = Fixture::new(
                    finish(builder, split, None),
                    LogicalRect::new(x, y, 401.25, 303.375).unwrap(),
                    [a, b]
                        .into_iter()
                        .map(|node| (node, constraints(20.0, 2000.0)))
                        .collect(),
                    thickness,
                );
                let initial = fixture.projection.splits.get(&split).unwrap();
                rounded_gap |= rect_extent(initial.splitter_rects[0], axis) != thickness;
                let groups = fixture.groups(&[(split, 0)]);
                let delta = match axis {
                    Axis::Horizontal => [17.125, 0.0],
                    Axis::Vertical => [0.0, 17.125],
                };
                let updates = resize_axis_group_updates(&groups, delta).unwrap();
                let projected = fixture.project_updates(&updates);
                close(
                    rect_extent(projected.node_rects[&a], axis),
                    initial.child_extents[0] + 17.125,
                );
                close(
                    rect_extent(projected.node_rects[&b], axis),
                    initial.child_extents[1] - 17.125,
                );
            }
        }
    }
    assert!(
        rounded_gap,
        "the regression must exercise rounded max-minus-min gap extents"
    );
}

fn constraints_along(axis: Axis, minimum: f64, maximum: f64) -> Constraints {
    match axis {
        Axis::Horizontal => constraints(minimum, maximum),
        Axis::Vertical => Constraints::new(
            LogicalSize::new(20.0, minimum).unwrap(),
            LogicalSize::new(2000.0, maximum).unwrap(),
        )
        .unwrap(),
    }
}

fn bounds_along(axis: Axis, origin: f64, extent: f64) -> LogicalRect {
    match axis {
        Axis::Horizontal => LogicalRect::new(origin, 0.0, extent, 300.0).unwrap(),
        Axis::Vertical => LogicalRect::new(0.0, origin, 300.0, extent).unwrap(),
    }
}

fn delta_along(axis: Axis, delta: f64) -> [f64; 2] {
    match axis {
        Axis::Horizontal => [delta, 0.0],
        Axis::Vertical => [0.0, delta],
    }
}

#[test]
fn small_far_central_keeps_nary_resize_available_on_both_axes() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let mut builder = Workspace::builder();
        let a = tabs(&mut builder, 1);
        let b = tabs(&mut builder, 2);
        let central = tabs(&mut builder, 3);
        let split =
            builder.insert_node(Node::split(axis, [a, b, central], [0.5, 0.5, 1e-10]).unwrap());
        let fixture = Fixture::new(
            finish(builder, split, Some(central)),
            bounds_along(axis, 0.0, 500.0),
            BTreeMap::from([
                (a, constraints_along(axis, 20.0, 2000.0)),
                (b, constraints_along(axis, 20.0, 2000.0)),
                (central, constraints_along(axis, 1e-6, 2000.0)),
            ]),
            1.0,
        );
        let initial = fixture.projection.splits.get(&split).unwrap();
        assert_eq!(initial.central_index, Some(2));
        assert_eq!(initial.child_extents[2], 1e-6);
        let original_far = fixture.projection.node_rects[&central];
        let groups = fixture.groups(&[(split, 0)]);
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].splits.len(), 1);
        assert_eq!(groups[0].splits[0].child_coefficients, [1.0, -1.0, 0.0]);
        assert!(groups[0].common_delta.minimum <= -64.0);
        assert!(groups[0].common_delta.maximum >= 64.0);
        let original_bits = groups[0].splits[0]
            .weights
            .iter()
            .map(|weight| weight.get().to_bits())
            .collect::<Vec<_>>();
        assert_eq!(
            original_bits,
            [0.5_f32.to_bits(), 0.5_f32.to_bits(), 1e-10_f32.to_bits()]
        );

        for delta in [0.0, 64.0, -64.0, 0.0] {
            let updates = resize_axis_group_updates(&groups, delta_along(axis, delta))
                .unwrap_or_else(|| panic!("{axis:?}: delta={delta} must remain available"));
            assert_eq!(updates.len(), 1);
            assert_eq!(updates[0].split().node(), split);
            let weights = updates[0].weights();
            assert!(
                weights
                    .iter()
                    .all(|weight| weight.get().is_finite() && weight.get() > 0.0)
            );
            let sum = weights
                .iter()
                .map(|weight| f64::from(weight.get()))
                .sum::<f64>();
            assert!((sum - 1.0).abs() <= crate::graph::NORMALIZED_WEIGHT_TOLERANCE);
            if delta == 0.0 {
                assert_eq!(
                    weights
                        .iter()
                        .map(|weight| weight.get().to_bits())
                        .collect::<Vec<_>>(),
                    original_bits,
                );
            }

            let projected = fixture.project_updates(&updates);
            let resized = projected.splits.get(&split).unwrap();
            assert!(
                validate_split_projection(
                    &projected,
                    &resized,
                    axis,
                    fixture.bounds,
                    &[a, b, central],
                    weights,
                    1.0,
                )
                .is_some()
            );
            close(
                rect_extent(projected.node_rects[&a], axis),
                initial.child_extents[0] + delta,
            );
            close(
                rect_extent(projected.node_rects[&b], axis),
                initial.child_extents[1] - delta,
            );
            assert_eq!(resized.child_extents[2], 1e-6);
            let far = projected.node_rects[&central];
            for (actual, expected) in [
                (far.min().x(), original_far.min().x()),
                (far.min().y(), original_far.min().y()),
                (far.max().x(), original_far.max().x()),
                (far.max().y(), original_far.max().y()),
            ] {
                assert!((actual - expected).abs() <= tolerance(actual, expected));
            }
        }
    }
}

#[test]
fn rounded_nested_minimum_overflow_allows_reverse_expansion_on_both_axes() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let mut builder = Workspace::builder();
        let a = tabs(&mut builder, 1);
        let c = tabs(&mut builder, 2);
        let d = tabs(&mut builder, 3);
        let b = tabs(&mut builder, 4);
        let inner = builder.insert_node(Node::split(axis, [c, d], [0.5, 0.5]).unwrap());
        let orthogonal = match axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
        let across = builder.insert_node(Node::split(orthogonal, [b, inner], [0.5, 0.5]).unwrap());
        let outer = builder.insert_node(Node::split(axis, [a, across], [0.9, 0.1]).unwrap());
        let fixture = Fixture::new(
            finish(builder, outer, None),
            bounds_along(axis, 0.0, 1001.0),
            BTreeMap::from([
                (a, constraints_along(axis, 20.0, 2000.0)),
                (b, constraints_along(axis, 20.0, 2000.0)),
                (c, constraints_along(axis, 200.3, 2000.0)),
                (d, constraints_along(axis, 50.0, 2000.0)),
            ]),
            1.0,
        );
        close(
            rect_extent(fixture.projection.node_rects[&inner], axis),
            251.3,
        );
        let residual = fixture.projection.splits.get(&inner).unwrap().overflow;
        assert!(residual > 0.0 && residual < 1e-10, "{axis:?}: {residual}");
        let groups = fixture.groups(&[(outer, 0)]);
        let updates = resize_axis_group_updates(&groups, delta_along(axis, -10.0)).unwrap();
        assert_eq!(updates.len(), 2);
        let projected = fixture.project_updates(&updates);
        close(rect_extent(projected.node_rects[&c], axis), 210.3);
        close(rect_extent(projected.node_rects[&d], axis), 50.0);
        let far = projected.node_rects[&d];
        let original_far = fixture.projection.node_rects[&d];
        close(far.x(), original_far.x());
        close(far.y(), original_far.y());
        for update in resize_axis_group_updates(&groups, delta_along(axis, 10.0)).unwrap() {
            let Node::Split { weights, .. } =
                fixture.workspace.node(update.split().node()).unwrap()
            else {
                unreachable!();
            };
            assert_eq!(update.weights(), weights.as_slice());
        }
    }
}

#[test]
fn rounded_nested_maximum_unallocated_allows_reverse_shrink_on_both_axes() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let mut builder = Workspace::builder();
        let a = tabs(&mut builder, 1);
        let c = tabs(&mut builder, 2);
        let d = tabs(&mut builder, 3);
        let b = tabs(&mut builder, 4);
        let inner = builder.insert_node(Node::split(axis, [c, d], [0.5, 0.5]).unwrap());
        let orthogonal = match axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
        let across = builder.insert_node(Node::split(orthogonal, [b, inner], [0.5, 0.5]).unwrap());
        let outer = builder.insert_node(Node::split(axis, [a, across], [0.5, 0.5]).unwrap());
        let fixture = Fixture::new(
            finish(builder, outer, None),
            bounds_along(axis, 0.0, 1001.0),
            BTreeMap::from([
                (a, constraints_along(axis, 20.0, 2000.0)),
                (b, constraints_along(axis, 20.0, 2000.0)),
                (c, constraints_along(axis, 20.0, 200.1)),
                (d, constraints_along(axis, 20.0, 50.0)),
            ]),
            1.0,
        );
        close(
            rect_extent(fixture.projection.node_rects[&inner], axis),
            251.1,
        );
        let residual = fixture.projection.splits.get(&inner).unwrap().unallocated;
        assert!(residual > 0.0 && residual < 1e-10, "{axis:?}: {residual}");
        let groups = fixture.groups(&[(outer, 0)]);
        let updates = resize_axis_group_updates(&groups, delta_along(axis, 10.0)).unwrap();
        assert_eq!(updates.len(), 2);
        let projected = fixture.project_updates(&updates);
        close(rect_extent(projected.node_rects[&c], axis), 190.1);
        close(rect_extent(projected.node_rects[&d], axis), 50.0);
        let far = projected.node_rects[&d];
        let original_far = fixture.projection.node_rects[&d];
        close(far.x(), original_far.x());
        close(far.y(), original_far.y());
        for update in resize_axis_group_updates(&groups, delta_along(axis, -10.0)).unwrap() {
            let Node::Split { weights, .. } =
                fixture.workspace.node(update.split().node()).unwrap()
            else {
                unreachable!();
            };
            assert_eq!(update.weights(), weights.as_slice());
        }
    }
}

#[test]
fn large_origin_rounded_minimum_allows_reverse_expansion_on_both_axes() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let mut builder = Workspace::builder();
        let a = tabs(&mut builder, 1);
        let b = tabs(&mut builder, 2);
        let split = builder.insert_node(Node::split(axis, [a, b], [0.1, 0.9]).unwrap());
        let fixture = Fixture::new(
            finish(builder, split, None),
            bounds_along(axis, 1_000_000.3, 1001.0),
            BTreeMap::from([
                (a, constraints_along(axis, 200.1, 2000.0)),
                (b, constraints_along(axis, 20.0, 2000.0)),
            ]),
            1.0,
        );
        let actual = rect_extent(fixture.projection.node_rects[&a], axis);
        assert!(
            actual < 200.1 && 200.1 - actual < 1e-7,
            "{axis:?}: {actual}"
        );
        let groups = fixture.groups(&[(split, 0)]);
        let updates = resize_axis_group_updates(&groups, delta_along(axis, 10.0)).unwrap();
        let projected = fixture.project_updates(&updates);
        close(rect_extent(projected.node_rects[&a], axis), 210.1);
        close(rect_extent(projected.node_rects[&b], axis), 789.9);
        let blocked = resize_axis_group_updates(&groups, delta_along(axis, -10.0)).unwrap();
        assert_eq!(blocked[0].weights(), groups[0].splits[0].weights.as_slice());
    }
}

#[test]
fn large_origin_rounded_maximum_allows_reverse_shrink_on_both_axes() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        let mut builder = Workspace::builder();
        let a = tabs(&mut builder, 1);
        let b = tabs(&mut builder, 2);
        let split = builder.insert_node(Node::split(axis, [a, b], [0.5, 0.5]).unwrap());
        let fixture = Fixture::new(
            finish(builder, split, None),
            bounds_along(axis, 1_000_000.3, 1001.0),
            BTreeMap::from([
                (a, constraints_along(axis, 20.0, 200.3)),
                (b, constraints_along(axis, 20.0, 2000.0)),
            ]),
            1.0,
        );
        let actual = rect_extent(fixture.projection.node_rects[&a], axis);
        assert!(
            actual > 200.3 && actual - 200.3 < 1e-7,
            "{axis:?}: {actual}"
        );
        let groups = fixture.groups(&[(split, 0)]);
        let updates = resize_axis_group_updates(&groups, delta_along(axis, -10.0)).unwrap();
        let projected = fixture.project_updates(&updates);
        close(rect_extent(projected.node_rects[&a], axis), 190.3);
        close(rect_extent(projected.node_rects[&b], axis), 809.7);
        let blocked = resize_axis_group_updates(&groups, delta_along(axis, 10.0)).unwrap();
        assert_eq!(blocked[0].weights(), groups[0].splits[0].weights.as_slice());
    }
}

#[test]
fn large_origin_real_overflow_unallocated_and_compressed_gaps_remain_unavailable() {
    for axis in [Axis::Horizontal, Axis::Vertical] {
        for (extent, minimum, maximum) in [
            (401.0, 200.1, 2000.0),
            (401.0, 20.0, 199.9),
            (0.5, 0.0, 2000.0),
        ] {
            let mut builder = Workspace::builder();
            let a = tabs(&mut builder, 1);
            let b = tabs(&mut builder, 2);
            let split = builder.insert_node(Node::split(axis, [a, b], [0.5, 0.5]).unwrap());
            let fixture = Fixture::new(
                finish(builder, split, None),
                bounds_along(axis, 1_000_000.3, extent),
                [a, b]
                    .into_iter()
                    .map(|node| (node, constraints_along(axis, minimum, maximum)))
                    .collect(),
                1.0,
            );
            let projected = fixture.projection.splits.get(&split).unwrap();
            if extent == 0.5 {
                close(rect_extent(projected.splitter_rects[0], axis), 0.5);
            } else if minimum > 200.0 {
                assert!(projected.overflow > 0.1);
            } else {
                assert!(projected.unallocated > 0.1);
            }
            assert!(
                prepare_resize_axis_groups(
                    &fixture.workspace,
                    &fixture.plan,
                    vec![fixture.handle(split, 0)],
                )
                .is_none(),
                "{axis:?}: extent={extent}, min={minimum}, max={maximum}"
            );
        }
    }
}
