#![cfg(feature = "serde")]

use dockspace::geometry::{GeometryError, PhysicalRect, ScaleFactor};
use dockspace::ids::SurfaceId;
use dockspace::viewport::WorkAreaToken;
use dockspace::viewport_persistence::{
    SnapshotPhysicalRect, SnapshotViewportPlacementRecord, VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
    ViewportPlacementPreference, ViewportPlacementPreferences, ViewportPlacementRestoreError,
    ViewportPlacementSnapshot, ViewportPlacementSnapshotEnvelope, WindowPresentationPreference,
};

const FIRST_SURFACE: SurfaceId = SurfaceId::new(7);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(11);

fn rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).expect("test physical rectangle must be valid")
}

fn record(surface: SurfaceId) -> SnapshotViewportPlacementRecord {
    SnapshotViewportPlacementRecord {
        surface,
        outer_rect: SnapshotPhysicalRect {
            x: 100.0,
            y: -40.0,
            width: 900.0,
            height: 700.0,
        },
        work_area: Some(31),
        scale_factor: Some(1.5),
        presentation: Some(WindowPresentationPreference::Maximized),
    }
}

#[test]
fn json_round_trip_preserves_only_durable_placement_preferences() {
    let mut preferences = ViewportPlacementPreferences::new();
    preferences.set(
        ViewportPlacementPreference::new(SECOND_SURFACE, rect(1_920.0, -120.0, 1_200.0, 800.0))
            .expect("non-empty placement must be valid")
            .with_work_area(WorkAreaToken::new(42))
            .with_scale_factor(ScaleFactor::new(2.0).expect("test scale must be valid"))
            .with_presentation(WindowPresentationPreference::Fullscreen),
    );
    preferences.set(
        ViewportPlacementPreference::new(FIRST_SURFACE, rect(-800.0, 20.0, 800.0, 600.0))
            .expect("non-empty placement must be valid"),
    );

    let snapshot = ViewportPlacementSnapshot::capture(&preferences);
    assert_eq!(snapshot.version, VIEWPORT_PLACEMENT_SNAPSHOT_VERSION);
    assert_eq!(
        snapshot
            .placements
            .iter()
            .map(|placement| placement.surface)
            .collect::<Vec<_>>(),
        vec![FIRST_SURFACE, SECOND_SURFACE],
        "capture order must be canonical by stable surface identity"
    );

    let json = serde_json::to_string_pretty(&snapshot).expect("sidecar JSON must encode");
    for forbidden in [
        "window_token",
        "incarnation",
        "focused",
        "hovered",
        "scene",
        "proof",
    ] {
        assert!(
            !json.contains(forbidden),
            "transient field {forbidden} must not cross the persistence boundary"
        );
    }

    let restored = serde_json::from_str::<ViewportPlacementSnapshotEnvelope>(&json)
        .expect("sidecar envelope must decode")
        .into_preferences()
        .expect("sidecar must validate");
    assert_eq!(restored, preferences);

    let second = restored
        .get(SECOND_SURFACE)
        .expect("second surface must survive");
    assert_eq!(second.work_area(), Some(WorkAreaToken::new(42)));
    assert_eq!(
        second.scale_factor(),
        Some(ScaleFactor::new(2.0).expect("test scale must be valid"))
    );
    assert_eq!(
        second.presentation(),
        Some(WindowPresentationPreference::Fullscreen)
    );
}

#[test]
fn unsupported_version_is_typed_before_future_payload_shape() {
    let envelope = serde_json::from_str::<ViewportPlacementSnapshotEnvelope>(
        r#"[9,{"future":{"shape":[1,2,3]}}]"#,
    )
    .expect("future payload must be skipped by the version-first envelope");
    assert_eq!(envelope.version(), 9);
    assert_eq!(
        envelope
            .into_preferences()
            .expect_err("future version must not restore"),
        ViewportPlacementRestoreError::UnsupportedVersion {
            found: 9,
            supported: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
        }
    );
}

#[test]
fn duplicate_surface_is_a_structured_restore_error() {
    let snapshot = ViewportPlacementSnapshot {
        version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
        placements: vec![record(FIRST_SURFACE), record(FIRST_SURFACE)],
    };
    assert_eq!(
        snapshot
            .restore()
            .expect_err("duplicate stable surface must be rejected"),
        ViewportPlacementRestoreError::DuplicateSurface {
            surface: FIRST_SURFACE,
        }
    );
}

#[test]
fn nonfinite_negative_empty_geometry_and_invalid_scale_are_typed() {
    let cases = [
        (
            SnapshotPhysicalRect {
                x: f64::NAN,
                y: 0.0,
                width: 640.0,
                height: 480.0,
            },
            GeometryError::NonFinite {
                component: "physical_x",
                value: f64::NAN,
            },
        ),
        (
            SnapshotPhysicalRect {
                x: 0.0,
                y: 0.0,
                width: -1.0,
                height: 480.0,
            },
            GeometryError::Negative {
                component: "physical_width",
                value: -1.0,
            },
        ),
        (
            SnapshotPhysicalRect {
                x: f64::MAX,
                y: 0.0,
                width: f64::MAX,
                height: 480.0,
            },
            GeometryError::NonFinite {
                component: "physical_x",
                value: f64::INFINITY,
            },
        ),
    ];

    for (outer_rect, expected_source) in cases {
        let mut invalid = record(FIRST_SURFACE);
        invalid.outer_rect = outer_rect;
        let error = ViewportPlacementSnapshot {
            version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
            placements: vec![invalid],
        }
        .restore()
        .expect_err("invalid physical geometry must be rejected");
        match error {
            ViewportPlacementRestoreError::InvalidOuterRect { surface, source } => {
                assert_eq!(surface, FIRST_SURFACE);
                assert_eq!(source.to_string(), expected_source.to_string());
            }
            other => panic!("expected invalid outer rectangle, got {other:?}"),
        }
    }

    let mut empty = record(FIRST_SURFACE);
    empty.outer_rect.width = 0.0;
    assert_eq!(
        ViewportPlacementSnapshot {
            version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
            placements: vec![empty],
        }
        .restore()
        .expect_err("empty placement must be rejected"),
        ViewportPlacementRestoreError::EmptyOuterRect {
            surface: FIRST_SURFACE,
        }
    );

    for invalid_scale in [0.0, -1.0, f64::INFINITY, f64::NAN] {
        let mut invalid = record(FIRST_SURFACE);
        invalid.scale_factor = Some(invalid_scale);
        assert!(matches!(
            ViewportPlacementSnapshot {
                version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
                placements: vec![invalid],
            }
            .restore(),
            Err(ViewportPlacementRestoreError::InvalidScaleFactor {
                surface: FIRST_SURFACE,
                ..
            })
        ));
    }
}

#[test]
fn supported_schema_strictly_rejects_unknown_and_repeated_fields() {
    let snapshot = ViewportPlacementSnapshot {
        version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
        placements: vec![record(FIRST_SURFACE)],
    };
    let base = serde_json::to_value(snapshot).expect("test sidecar must encode");

    let mut unknown_payload = base.clone();
    unknown_payload[1]["future"] = serde_json::json!(true);
    assert!(serde_json::from_value::<ViewportPlacementSnapshotEnvelope>(unknown_payload).is_err());

    let mut unknown_record = base.clone();
    unknown_record[1]["placements"][0]["window_token"] = serde_json::json!(99);
    assert!(serde_json::from_value::<ViewportPlacementSnapshotEnvelope>(unknown_record).is_err());

    let mut unknown_rect = base.clone();
    unknown_rect[1]["placements"][0]["outer_rect"]["monitor"] = serde_json::json!(3);
    assert!(serde_json::from_value::<ViewportPlacementSnapshotEnvelope>(unknown_rect).is_err());

    let json = serde_json::to_string(&base).expect("test value must encode");
    let payload_start = json.find('{').expect("payload object must exist") + 1;
    let duplicate = format!(
        "{}\"placements\":[],{}",
        &json[..payload_start],
        &json[payload_start..]
    );
    assert!(serde_json::from_str::<ViewportPlacementSnapshotEnvelope>(&duplicate).is_err());

    let mut transient_presentation = base;
    transient_presentation[1]["placements"][0]["presentation"] = serde_json::json!("minimized");
    assert!(
        serde_json::from_value::<ViewportPlacementSnapshotEnvelope>(transient_presentation)
            .is_err()
    );
}

#[test]
fn envelope_requires_exactly_version_then_payload() {
    let payload = r#"{"placements":[]}"#;
    for malformed in [
        "[]".to_owned(),
        format!("[{VIEWPORT_PLACEMENT_SNAPSHOT_VERSION}]"),
        format!("[{VIEWPORT_PLACEMENT_SNAPSHOT_VERSION},{payload},null]"),
        format!("[{payload},{VIEWPORT_PLACEMENT_SNAPSHOT_VERSION}]"),
    ] {
        assert!(serde_json::from_str::<ViewportPlacementSnapshotEnvelope>(&malformed).is_err());
    }
}

#[test]
fn replacing_and_removing_preferences_are_explicit() {
    let first = ViewportPlacementPreference::new(FIRST_SURFACE, rect(0.0, 0.0, 400.0, 300.0))
        .expect("first preference must be valid");
    let replacement =
        ViewportPlacementPreference::new(FIRST_SURFACE, rect(40.0, 50.0, 800.0, 600.0))
            .expect("replacement preference must be valid");
    let mut preferences = ViewportPlacementPreferences::new();

    assert_eq!(preferences.set(first), None);
    assert_eq!(preferences.set(replacement), Some(first));
    assert_eq!(preferences.len(), 1);
    assert_eq!(preferences.get(FIRST_SURFACE), Some(&replacement));
    assert_eq!(preferences.remove(FIRST_SURFACE), Some(replacement));
    assert!(preferences.is_empty());
}
