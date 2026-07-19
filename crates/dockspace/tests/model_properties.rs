use dockspace::geometry::{Constraints, LogicalRect, LogicalSize};
use dockspace::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::layout::{AxisConstraint, LayoutMetrics, project_root, solve_axis};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn axis_solver_is_deterministic_and_conserves_reported_extent(
        extent in 0.0_f64..1_000_000.0,
        inputs in prop::collection::vec(
            (0.001_f64..100.0, 0.0_f64..1_000.0, 0.0_f64..1_000.0),
            1..24,
        ),
    ) {
        let weights = inputs.iter().map(|(weight, _, _)| *weight).collect::<Vec<_>>();
        let constraints = inputs
            .iter()
            .map(|(_, minimum, span)| {
                AxisConstraint::new(*minimum, *minimum + *span)
                    .expect("generated constraints are finite and ordered")
            })
            .collect::<Vec<_>>();

        let first = solve_axis(extent, &weights, &constraints, None)
            .expect("generated axis input is valid");
        let second = solve_axis(extent, &weights, &constraints, None)
            .expect("replaying generated input is valid");

        prop_assert_eq!(&first, &second);
        prop_assert!(first.overflow.is_finite() && first.overflow >= 0.0);
        prop_assert!(first.unallocated.is_finite() && first.unallocated >= 0.0);
        for (index, (size, constraint)) in first.sizes.iter().zip(&constraints).enumerate() {
            prop_assert!(size.is_finite());
            prop_assert!(
                *size >= constraint.min(),
                "child {index} size {size:?} is below minimum {:?}",
                constraint.min()
            );
            prop_assert!(
                *size <= constraint.max(),
                "child {index} size {size:?} exceeds maximum {:?}",
                constraint.max()
            );
        }

        let assigned = first.sizes.iter().sum::<f64>();
        let expected = extent + first.overflow - first.unallocated;
        let tolerance = extent.max(assigned).max(1.0) * f64::EPSILON * 256.0;
        prop_assert!((assigned - expected).abs() <= tolerance);
    }

    #[test]
    fn canonical_workspace_and_projection_are_deterministic(
        axes in prop::collection::vec(any::<bool>(), 0..32),
        splitter_thickness in 0.0_f64..0.1,
    ) {
        let mut builder = Workspace::builder();
        let central = builder.insert_node(Node::tabs([]));
        let mut subtree = central;
        let mut leaf_constraints = std::collections::BTreeMap::from([(
            central,
            Constraints::new(
                LogicalSize::new(0.0, 0.0).expect("valid minimum"),
                LogicalSize::new(1_000_000.0, 1_000_000.0).expect("valid maximum"),
            )
            .expect("valid constraints"),
        )]);

        for (index, horizontal) in axes.iter().copied().enumerate() {
            let item = ItemId::new(u64::try_from(index + 1).expect("fixture index fits u64"));
            let side = builder.insert_node(Node::tabs([item]));
            leaf_constraints.insert(side, leaf_constraints[&central]);
            subtree = builder.insert_node(
                Node::equal_split(
                    if horizontal { Axis::Horizontal } else { Axis::Vertical },
                    [subtree, side],
                )
                .expect("two-child split is valid"),
            );
        }

        let root = RootId::new(1);
        builder.set_root(root, RootRecord::new(subtree).with_central(central));
        builder.set_surface(SurfaceId::new(1), SurfacePresentation::new(root));
        let workspace = builder.build().expect("generated workspace canonicalizes");
        workspace.validate().expect("published workspace is strict");

        let expected_items = axes
            .iter()
            .enumerate()
            .map(|(index, _)| {
                (ItemId::new(u64::try_from(index + 1).expect("fixture index fits u64")), 1)
            })
            .collect::<std::collections::BTreeMap<_, _>>();
        prop_assert_eq!(workspace.item_multiset(), expected_items);

        let metrics = LayoutMetrics::new(splitter_thickness).expect("generated metric is valid");
        let bounds = LogicalRect::new(0.0, 0.0, 100_000.0, 100_000.0)
            .expect("fixture bounds are valid");
        let first = project_root(&workspace, root, bounds, &leaf_constraints, metrics)
            .expect("generated workspace projects");
        let second = project_root(&workspace, root, bounds, &leaf_constraints, metrics)
            .expect("generated workspace replays");
        prop_assert_eq!(first, second);
    }
}
