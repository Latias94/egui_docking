use dockspace::geometry::{
    Constraints, GeometryError, LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint,
    PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};
use slotmap::SlotMap;

fn assert_close(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() <= f64::EPSILON,
        "expected {actual} to equal {expected}"
    );
}

#[test]
fn stable_ids_preserve_their_numeric_identity() {
    let item = ItemId::new(42);
    let root = RootId::new(42);
    let surface = SurfaceId::new(42);
    let floating = FloatingPresentationId::new(42);

    assert_eq!(item.get(), 42);
    assert_eq!(root.get(), 42);
    assert_eq!(surface.get(), 42);
    assert_eq!(floating.get(), 42);
    assert_eq!(item.to_string(), "42");
    assert_eq!(u64::from(root), 42);
}

#[test]
fn runtime_node_ids_are_generational() {
    let mut nodes = SlotMap::<NodeId, &'static str>::with_key();
    let first = nodes.insert("first");
    assert_eq!(nodes.remove(first), Some("first"));
    let replacement = nodes.insert("replacement");

    assert_ne!(first, replacement);
    assert_eq!(nodes.get(first), None);
    assert_eq!(nodes.get(replacement), Some(&"replacement"));
}

#[test]
fn geometry_constructors_reject_invalid_components() {
    assert!(matches!(
        LogicalPoint::new(f64::NAN, 0.0),
        Err(GeometryError::NonFinite { component: "x", .. })
    ));
    assert!(matches!(
        LogicalSize::new(-1.0, 2.0),
        Err(GeometryError::Negative {
            component: "width",
            ..
        })
    ));
    assert!(matches!(
        ScaleFactor::new(0.0),
        Err(GeometryError::ZeroScaleFactor)
    ));

    let min = LogicalPoint::new(10.0, 20.0).expect("point is valid");
    let max = LogicalPoint::new(9.0, 20.0).expect("point is valid");
    assert!(matches!(
        LogicalRect::from_min_max(min, max),
        Err(GeometryError::InvertedBounds { component: "x", .. })
    ));
}

#[test]
fn negative_desktop_origins_are_valid_but_sizes_are_not() {
    let point = PhysicalPoint::new(-1_920.0, -200.0).expect("desktop origins may be negative");
    assert_close(point.x(), -1_920.0);
    assert_close(point.y(), -200.0);
    assert!(PhysicalSize::new(-1.0, 200.0).is_err());
}

#[test]
fn rects_use_half_open_edges_and_reject_overflow() {
    let rect = LogicalRect::new(-10.0, 5.0, 20.0, 15.0).expect("rect is valid");
    assert!(rect.contains(LogicalPoint::new(-10.0, 5.0).expect("point is valid")));
    assert!(!rect.contains(LogicalPoint::new(10.0, 20.0).expect("point is valid")));
    assert_close(rect.width(), 20.0);
    assert_close(rect.height(), 15.0);

    assert!(LogicalRect::new(f64::MAX, 0.0, f64::MAX, 1.0).is_err());
}

#[test]
fn target_scale_performs_a_single_origin_aware_conversion() {
    let target_scale = ScaleFactor::new(1.5).expect("scale is valid");
    let logical = LogicalRect::new(-100.0, 20.0, 320.0, 180.0).expect("rect is valid");
    let target_origin = PhysicalPoint::new(2_000.0, -300.0).expect("origin is valid");
    let physical = logical
        .to_desktop_physical(target_origin, target_scale)
        .expect("conversion is finite");

    assert_eq!(
        physical,
        PhysicalRect::new(1_850.0, -270.0, 480.0, 270.0).expect("rect is valid")
    );
    assert_eq!(
        physical
            .to_target_logical(target_origin, target_scale)
            .expect("conversion is finite"),
        logical
    );
}

#[test]
fn constraints_validate_order_and_clamp_each_axis() {
    let min = LogicalSize::new(100.0, 80.0).expect("size is valid");
    let max = LogicalSize::new(500.0, 400.0).expect("size is valid");
    let constraints = Constraints::new(min, max).expect("constraints are ordered");

    assert_eq!(constraints.min(), min);
    assert_eq!(constraints.max(), max);
    assert_eq!(
        constraints.clamp(LogicalSize::new(50.0, 450.0).expect("size is valid")),
        LogicalSize::new(100.0, 400.0).expect("size is valid")
    );
    assert!(Constraints::new(max, min).is_err());
}
