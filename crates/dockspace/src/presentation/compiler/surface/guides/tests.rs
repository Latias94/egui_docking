use super::*;

fn rect(x1: f64, y1: f64, x2: f64, y2: f64) -> LogicalRect {
    LogicalRect::from_min_max(
        LogicalPoint::new(x1, y1).unwrap(),
        LogicalPoint::new(x2, y2).unwrap(),
    )
    .unwrap()
}

fn button(bounds: LogicalRect) -> GuideButtonGeometry {
    GuideButtonGeometry {
        draw: bounds,
        hit: bounds,
    }
}

#[test]
fn inner_hit_cells_keep_shared_fractional_boundaries_exact() {
    let bounds = rect(5.6, 5.6, 60.0, 60.0);
    let center = button(rect(26.8, 26.8, 38.0, 38.0));
    let mut edges = GuideEdgeGeometry {
        left: button(rect(5.6, 26.8, 30.0, 38.0)),
        right: button(rect(35.0, 26.8, 60.0, 38.0)),
        top: button(rect(26.8, 5.6, 38.0, 30.0)),
        bottom: button(rect(26.8, 35.0, 38.0, 60.0)),
    };
    separate_inner_cross_hits(bounds, center, &mut edges).unwrap();
    let mut buttons = Vec::from(edges.buttons());
    buttons.push(center);
    assert!(!guide_hits_overlap(&buttons));
    assert_eq!(edges.left.hit.max().x(), center.hit.x());
    assert_eq!(edges.top.hit.max().y(), center.hit.y());
}

#[test]
fn outer_hit_cells_keep_shared_fractional_boundaries_exact() {
    let bounds = rect(5.6, 5.6, 60.0, 60.0);
    let mut edges = GuideEdgeGeometry {
        left: button(rect(5.6, 26.8, 30.0, 38.0)),
        right: button(rect(35.0, 26.8, 60.0, 38.0)),
        top: button(rect(26.8, 5.6, 38.0, 30.0)),
        bottom: button(rect(26.8, 35.0, 38.0, 60.0)),
    };
    separate_outer_cross_hits(bounds, &mut edges).unwrap();
    assert!(!guide_hits_overlap(&edges.buttons()));
    assert_eq!(edges.left.hit.max().x(), edges.top.hit.x());
    assert_eq!(edges.top.hit.max().y(), edges.left.hit.y());
}
