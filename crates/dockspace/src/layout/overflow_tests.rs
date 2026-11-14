use super::*;
use proptest::prelude::*;

fn allocate(
    extent: f64,
    constraints: &[AxisConstraint],
    policy: PaneOverflowPolicy,
) -> (Vec<f64>, AxisOutcome) {
    let weights = vec![1.0; constraints.len()];
    let mut sizes = vec![0.0; constraints.len()];
    let result = solve_axis_into(
        extent,
        &weights,
        constraints,
        None,
        &mut sizes,
        &mut Vec::new(),
        policy,
    )
    .unwrap();
    (sizes, result)
}

#[test]
fn compression_preserves_zero_minima_and_fits_zero_or_tiny_extents() {
    let constraints = [
        AxisConstraint::new(150.0, 500.0).unwrap(),
        AxisConstraint::new(0.0, 500.0).unwrap(),
        AxisConstraint::new(100.0, 500.0).unwrap(),
    ];
    for extent in [0.0, 1.0e-300, 10.0, 249.999] {
        let (sizes, outcome) = allocate(extent, &constraints, PaneOverflowPolicy::Compress);
        assert!(sizes.iter().all(|size| size.is_finite() && *size >= 0.0));
        assert_eq!(sizes[1], 0.0);
        assert!((sizes.iter().sum::<f64>() - extent).abs() <= extent * 1.0e-14);
        assert_eq!(outcome.overflow, 250.0 - extent);
        assert_eq!(outcome.unallocated, 0.0);
    }
}

#[test]
fn compression_is_inert_when_minima_fit() {
    let constraints = [
        AxisConstraint::new(150.0, 500.0).unwrap(),
        AxisConstraint::new(100.0, 300.0).unwrap(),
    ];
    for extent in [250.0, 250.001, 300.0, 700.0, 900.0] {
        let (clipped, clip_outcome) = allocate(extent, &constraints, PaneOverflowPolicy::Clip);
        let (compressed, compress_outcome) =
            allocate(extent, &constraints, PaneOverflowPolicy::Compress);
        assert_eq!(compressed, clipped);
        assert_eq!(compress_outcome.overflow, clip_outcome.overflow);
        assert_eq!(compress_outcome.unallocated, clip_outcome.unallocated);
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    #[test]
    fn compressed_allocations_conserve_space_without_mutating_constraints(
        minima in prop::collection::vec(1.0_f64..100_000.0, 2..17),
        fraction in 0.0_f64..0.999_999,
    ) {
        let total: f64 = minima.iter().sum();
        let extent = total * fraction;
        let constraints: Vec<_> = minima.iter()
            .map(|&minimum| AxisConstraint::new(minimum, total).unwrap())
            .collect();
        let original = constraints.clone();
        let (sizes, outcome) = allocate(extent, &constraints, PaneOverflowPolicy::Compress);
        prop_assert_eq!(&constraints, &original);
        let tolerance = total * f64::EPSILON * 32.0;
        prop_assert!((sizes.iter().sum::<f64>() - extent).abs() <= tolerance);
        prop_assert_eq!(outcome.overflow, total - extent);
        prop_assert_eq!(outcome.unallocated, 0.0);
        for (&size, &minimum) in sizes.iter().zip(&minima) {
            prop_assert!(size.is_finite() && size >= 0.0 && size <= minimum + tolerance);
            prop_assert!((size - minimum * fraction).abs() <= tolerance);
        }
    }
}
