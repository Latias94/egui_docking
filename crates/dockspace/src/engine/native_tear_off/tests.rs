//! Geometry edge cases for selection from an already authoritative work-area roster.

use super::select_native_work_area;
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::platform::ObservedWorkArea;
use crate::platform::viewport::WorkAreaToken;

fn rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).unwrap()
}

fn area(token: u64, bounds: PhysicalRect) -> ObservedWorkArea {
    ObservedWorkArea::new(
        WorkAreaToken::new(token),
        bounds,
        ScaleFactor::new(1.0).unwrap(),
    )
}

#[test]
fn empty_authoritative_roster_cannot_select_an_area() {
    assert_eq!(
        select_native_work_area(rect(10.0, 20.0, 100.0, 80.0), []),
        None,
    );
}

#[test]
fn nearest_area_uses_euclidean_distance_between_rectangles() {
    let requested = rect(0.0, 0.0, 100.0, 100.0);
    // The diagonal gap is hypot(6, 8) = 10; the horizontal gap is 11.
    // Manhattan distance and distance between centers would both choose incorrectly.
    let diagonal = area(9, rect(106.0, 108.0, 1_000.0, 1_000.0));
    let horizontal = area(1, rect(111.0, 0.0, 100.0, 100.0));
    for roster in [[horizontal, diagonal], [diagonal, horizontal]] {
        assert_eq!(select_native_work_area(requested, roster), Some(diagonal));
    }
}

#[test]
fn any_positive_intersection_beats_a_touching_edge() {
    let requested = rect(0.0, 0.0, 1.0, 1.0);
    let touching = area(1, rect(1.0, 0.0, 100.0, 100.0));
    let intersecting = area(9, rect(0.75, 0.0, 1.0, 1.0));
    assert_eq!(
        select_native_work_area(requested, [touching, intersecting]),
        Some(intersecting),
        "positive intersections below ImGui's threshold must still participate",
    );
}

#[test]
fn corner_contact_has_zero_distance_without_positive_intersection() {
    let requested = rect(-100.0, -100.0, 100.0, 100.0);
    let touching = area(9, rect(0.0, 0.0, 100.0, 100.0));
    let separated = area(1, rect(1.0, -100.0, 100.0, 100.0));
    assert_eq!(
        select_native_work_area(requested, [separated, touching]),
        Some(touching),
    );
}
