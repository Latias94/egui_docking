#![cfg(feature = "serde")]

use dockspace::geometry::{PhysicalRect, ScaleFactor};
use dockspace::ids::SurfaceId;
use dockspace::viewport::WorkAreaToken;
use dockspace::viewport_persistence::{
    ViewportPlacementPreference, ViewportPlacementPreferences, ViewportPlacementRestoreError,
    WindowPresentationPreference,
};

const FIRST_SURFACE: SurfaceId = SurfaceId::new(7);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(11);

fn rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}
#[test]
fn runtime_preferences_preserve_canonical_surface_identity_and_metadata() {
    let mut preferences = ViewportPlacementPreferences::new();
    let first = ViewportPlacementPreference::new(FIRST_SURFACE, rect(-800.0, 20.0, 800.0, 600.0))
        .expect("non-empty placement must be valid");
    let second =
        ViewportPlacementPreference::new(SECOND_SURFACE, rect(1_920.0, -120.0, 1_200.0, 800.0))
            .expect("non-empty placement must be valid")
            .with_work_area(WorkAreaToken::new(42))
            .with_scale_factor(ScaleFactor::new(2.0).expect("test scale must be valid"))
            .with_presentation(WindowPresentationPreference::Fullscreen);

    assert_eq!(preferences.set(second), None);
    assert_eq!(preferences.set(first), None);
    assert_eq!(
        preferences
            .iter()
            .map(|preference| preference.surface())
            .collect::<Vec<_>>(),
        vec![FIRST_SURFACE, SECOND_SURFACE],
        "runtime iteration remains canonical by stable surface identity"
    );
    assert_eq!(
        preferences
            .get(SECOND_SURFACE)
            .expect("second preference must exist")
            .work_area(),
        Some(WorkAreaToken::new(42))
    );
    assert_eq!(
        preferences
            .get(SECOND_SURFACE)
            .expect("second preference must exist")
            .presentation(),
        Some(WindowPresentationPreference::Fullscreen)
    );
    assert_eq!(preferences.remove(FIRST_SURFACE), Some(first));
    assert_eq!(preferences.len(), 1);
}

#[test]
fn runtime_preference_rejects_empty_outer_rect() {
    assert_eq!(
        ViewportPlacementPreference::new(FIRST_SURFACE, rect(0.0, 0.0, 0.0, 480.0),),
        Err(ViewportPlacementRestoreError::EmptyOuterRect {
            surface: FIRST_SURFACE,
        })
    );
}
