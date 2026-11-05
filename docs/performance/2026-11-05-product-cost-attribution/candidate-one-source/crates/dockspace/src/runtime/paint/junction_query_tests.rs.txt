use super::*;
use crate::graph::{Node, RootRecord, Workspace};
use crate::runtime::{DockspaceSession, UniformSurfaceMetrics};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(2);
const AXES: [DockspaceAxis; 2] = [DockspaceAxis::Horizontal, DockspaceAxis::Vertical];

fn measured_grid(panes: u64, root: RootId, allowed: [bool; 2]) -> DockspaceSession {
    let mut builder = Workspace::builder();
    let mut nodes = (1..=panes)
        .map(|item| builder.insert_node(Node::tabs([ItemId::new(item)])))
        .collect::<Vec<_>>();
    let mut axis = Axis::Horizontal;
    while nodes.len() > 1 {
        nodes = nodes
            .chunks_exact(2)
            .map(|pair| builder.insert_node(Node::equal_split(axis, pair.iter().copied()).unwrap()))
            .collect();
        axis = match axis {
            Axis::Horizontal => Axis::Vertical,
            Axis::Vertical => Axis::Horizontal,
        };
    }
    builder.set_root(root, RootRecord::new(nodes[0]));
    builder.set_surface(SURFACE, crate::graph::SurfacePresentation::with_main(root));
    let mut policy = crate::policy::DockPolicy::default();
    for (axis, allowed) in AXES.into_iter().zip(allowed) {
        policy.set_allow_resize_axis(axis, allowed);
    }
    let mut session = DockspaceSession::from_workspace_for_test(builder.build().unwrap(), policy)
        .expect("the alternating grid initializes");
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 2048.0, 2048.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                48.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    session
}

// Preserve the original scan and sorted/deduplicated arm traversal independently.
fn frozen_axis_operable(
    plan: SurfacePaintPlan<'_>,
    junction: SplitterJunctionPaintRecord<'_>,
    axis: DockspaceAxis,
) -> bool {
    let axis: Axis = axis.into();
    let Some(record) = plan
        .plan
        .splitter_junction_records()
        .iter()
        .find(|record| record.id() == junction.record.id())
    else {
        return false;
    };
    let directions = match axis {
        Axis::Horizontal => [
            SplitterJunctionDirection::North,
            SplitterJunctionDirection::South,
        ],
        Axis::Vertical => [
            SplitterJunctionDirection::East,
            SplitterJunctionDirection::West,
        ],
    };
    let mut ids = directions
        .into_iter()
        .filter_map(|direction| record.id().arm(direction))
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    !ids.is_empty()
        && ids.into_iter().all(|id| {
            plan.plan
                .splitter_record(id)
                .is_some_and(|splitter| splitter.axis() == axis && splitter.operable())
        })
}

#[test]
fn canonical_grid_junction_queries_match_frozen_scans_on_both_axes() {
    let mut saw_shared_arm = false;
    let mut saw_distinct_arms = false;
    for panes in [4, 16] {
        for allowed in [[true, true], [false, true], [true, false], [false, false]] {
            let mut session = measured_grid(panes, ROOT, allowed);
            let mut frame = session.begin_host_frame().unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            assert!(
                plan.plan
                    .splitter_records()
                    .windows(2)
                    .all(|pair| pair[0].id() < pair[1].id())
            );
            assert!(
                plan.plan
                    .splitter_junction_records()
                    .windows(2)
                    .all(|pair| pair[0].id() < pair[1].id())
            );
            assert!(plan.splitter_junctions().len() > 0);
            for junction in plan.splitter_junctions() {
                let id = junction.record.id();
                for (axis, expected) in AXES.into_iter().zip(allowed) {
                    assert!(junction.has_axis(axis));
                    assert_eq!(
                        plan.splitter_junction_axis_operable(junction, axis),
                        frozen_axis_operable(plan, junction, axis)
                    );
                    assert_eq!(
                        plan.splitter_junction_axis_operable(junction, axis),
                        expected
                    );
                }
                for (first, second) in [
                    (
                        SplitterJunctionDirection::North,
                        SplitterJunctionDirection::South,
                    ),
                    (
                        SplitterJunctionDirection::East,
                        SplitterJunctionDirection::West,
                    ),
                ] {
                    if let (Some(first), Some(second)) = (id.arm(first), id.arm(second)) {
                        saw_shared_arm |= first == second;
                        saw_distinct_arms |= first != second;
                    }
                }
            }
        }
    }
    assert!(
        saw_shared_arm,
        "a through handle occupies two opposite arms"
    );
    assert!(saw_distinct_arms, "separate handles meet on the same axis");
}

#[test]
fn foreign_junction_uses_current_plan_membership_and_axis_policy() {
    let mut origin = measured_grid(4, ROOT, [true, true]);
    let mut current = measured_grid(4, ROOT, [false, true]);
    let mut unrelated = measured_grid(4, RootId::new(99), [true, true]);
    let mut origin_frame = origin.begin_host_frame().unwrap();
    let mut current_frame = current.begin_host_frame().unwrap();
    let mut unrelated_frame = unrelated.begin_host_frame().unwrap();
    let origin = origin_frame.paint_plan(SURFACE).unwrap().unwrap();
    let current = current_frame.paint_plan(SURFACE).unwrap().unwrap();
    let unrelated = unrelated_frame.paint_plan(SURFACE).unwrap().unwrap();
    let foreign = origin.splitter_junctions().next().unwrap();
    assert!(foreign.record.operable());
    for (axis, expected) in AXES.into_iter().zip([false, true]) {
        assert_eq!(
            current.splitter_junction_axis_operable(foreign, axis),
            expected
        );
        assert_eq!(
            current.splitter_junction_axis_operable(foreign, axis),
            frozen_axis_operable(current, foreign, axis)
        );
        assert!(!unrelated.splitter_junction_axis_operable(foreign, axis));
        assert_eq!(
            unrelated.splitter_junction_axis_operable(foreign, axis),
            frozen_axis_operable(unrelated, foreign, axis)
        );
    }
}

#[test]
fn either_distinct_arm_must_exist_and_match_the_axis() {
    let mut session = measured_grid(4, ROOT, [true, true]);
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let junction = plan.splitter_junctions().next().unwrap();
    let id = junction.record.id();
    let (axis, arms) = [
        (
            DockspaceAxis::Horizontal,
            [
                id.arm(SplitterJunctionDirection::North),
                id.arm(SplitterJunctionDirection::South),
            ],
        ),
        (
            DockspaceAxis::Vertical,
            [
                id.arm(SplitterJunctionDirection::East),
                id.arm(SplitterJunctionDirection::West),
            ],
        ),
    ]
    .into_iter()
    .find_map(|(axis, arms)| match arms {
        [Some(first), Some(second)] if first != second => Some((axis, [first, second])),
        _ => None,
    })
    .expect("the grid has two distinct handles on one junction axis");
    for arm in arms {
        for wrong_axis in [false, true] {
            // Keep the canonical order while exercising a missing or mismatched handle.
            let mut altered = PresentationPlan::new(SURFACE, plan.plan.bounds());
            altered.push_splitter_junction_record(*junction.record);
            for splitter in plan.plan.splitter_records() {
                if *splitter.id() != arm {
                    altered.push_splitter_record(splitter.clone());
                } else if wrong_axis {
                    altered.push_splitter_record(SplitterRecord::new(
                        *splitter.id(),
                        splitter.draw_bounds(),
                        splitter.hit(),
                        match splitter.axis() {
                            Axis::Horizontal => Axis::Vertical,
                            Axis::Vertical => Axis::Horizontal,
                        },
                        splitter.before_bounds(),
                        splitter.after_bounds(),
                        splitter.before_minimum_extent(),
                        splitter.after_minimum_extent(),
                        splitter.before_maximum_extent(),
                        splitter.after_maximum_extent(),
                        splitter.child_extents().to_vec(),
                        splitter.central_index(),
                        splitter.weights().to_vec(),
                        splitter.layer(),
                        splitter.operable(),
                    ));
                }
            }
            let altered = SurfacePaintPlan {
                plan: &altered,
                ..plan
            };
            assert!(!altered.splitter_junction_axis_operable(junction, axis));
            assert_eq!(
                altered.splitter_junction_axis_operable(junction, axis),
                frozen_axis_operable(altered, junction, axis)
            );
        }
    }
}
