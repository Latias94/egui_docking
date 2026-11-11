//! Runtime native viewport placement preferences and document-internal wire data.
//!
//! Placement snapshots are not an independent durable boundary. They are encoded,
//! hashed, restored, and published only through [`crate::runtime::DockspaceSession`].
//! Public callers receive validated runtime preferences and register them through
//! the session which owns the workspace, external item identities, and lineage.
//!
//! Scale factors are durable hints from the observation which confirmed the
//! outer rectangle. Provider-local work-area tokens are deliberately excluded
//! from the wire schema and must be reacquired from the current authoritative
//! inventory after restore.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::geometry::{GeometryError, PhysicalRect, PhysicalSize, ScaleFactor};
use crate::ids::SurfaceId;
use crate::viewport::WorkAreaToken;

/// Application-declared native hosting preference for one logical surface.
///
/// This value describes a document's intended host. It grants no native binding,
/// recovery anchor, coordinate authority or permission to create an OS window.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeSurfaceHostPreference {
    /// The application owns this native root window.
    #[serde(deserialize_with = "deserialize_root_host_preference")]
    Root,
    /// Dockspace owns a child window and recovers its content to this host.
    Child {
        /// Logical application-root surface which receives recovered content.
        recovery_host: SurfaceId,
    },
}

fn deserialize_root_host_preference<'de, D>(deserializer: D) -> Result<(), D::Error>
where
    D: serde::Deserializer<'de>,
{
    // Serde's internally tagged unit variant decoder ignores unknown fields.
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct RootHostPreference {}

    RootHostPreference::deserialize(deserializer).map(|_| ())
}

/// A durable window presentation preference.
///
/// Hidden and minimized states are intentionally absent because they are
/// transient lifecycle observations rather than restoration preferences.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowPresentationPreference {
    /// Restore the last-confirmed outer rectangle as a normal window.
    Normal,
    /// Ask the platform to maximize the restored window.
    Maximized,
    /// Ask the platform to present the restored window fullscreen.
    Fullscreen,
}

/// One validated placement preference keyed by a stable logical surface.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ViewportPlacementPreference {
    surface: SurfaceId,
    outer_rect: PhysicalRect,
    inner_size: Option<PhysicalSize>,
    work_area: Option<WorkAreaToken>,
    scale_factor: Option<ScaleFactor>,
    presentation: Option<WindowPresentationPreference>,
}

impl ViewportPlacementPreference {
    /// Creates a preference from a last-confirmed, non-empty physical outer rectangle.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportPlacementRestoreError::EmptyOuterRect`] when the
    /// rectangle has zero width or height.
    pub fn new(
        surface: SurfaceId,
        outer_rect: PhysicalRect,
    ) -> Result<Self, ViewportPlacementRestoreError> {
        if outer_rect.width() == 0.0 || outer_rect.height() == 0.0 {
            return Err(ViewportPlacementRestoreError::EmptyOuterRect { surface });
        }
        Ok(Self {
            surface,
            outer_rect,
            inner_size: None,
            work_area: None,
            scale_factor: None,
            presentation: None,
        })
    }

    /// Associates the last-confirmed native inner size.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportPlacementRestoreError::EmptyInnerSize`] when either
    /// dimension is zero and therefore cannot restore a native window.
    pub fn try_with_inner_size(
        mut self,
        inner_size: PhysicalSize,
    ) -> Result<Self, ViewportPlacementRestoreError> {
        if inner_size.width() == 0.0 || inner_size.height() == 0.0 {
            return Err(ViewportPlacementRestoreError::EmptyInnerSize {
                surface: self.surface,
            });
        }
        self.inner_size = Some(inner_size);
        Ok(self)
    }

    /// Associates the work-area identity which confirmed this placement.
    #[must_use]
    pub const fn with_work_area(mut self, work_area: WorkAreaToken) -> Self {
        self.work_area = Some(work_area);
        self
    }

    /// Associates the scale factor which confirmed this placement.
    #[must_use]
    pub const fn with_scale_factor(mut self, scale_factor: ScaleFactor) -> Self {
        self.scale_factor = Some(scale_factor);
        self
    }

    /// Associates a durable presentation preference.
    #[must_use]
    pub const fn with_presentation(mut self, presentation: WindowPresentationPreference) -> Self {
        self.presentation = Some(presentation);
        self
    }

    /// Returns the stable logical surface identity.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the last-confirmed outer rectangle in desktop physical pixels.
    #[must_use]
    pub const fn outer_rect(self) -> PhysicalRect {
        self.outer_rect
    }

    /// Returns the last-confirmed native inner size, when available.
    #[must_use]
    pub const fn inner_size(self) -> Option<PhysicalSize> {
        self.inner_size
    }

    /// Returns the work-area hint recorded with the rectangle.
    #[must_use]
    pub const fn work_area(self) -> Option<WorkAreaToken> {
        self.work_area
    }

    /// Returns the scale-factor hint recorded with the rectangle.
    #[must_use]
    pub const fn scale_factor(self) -> Option<ScaleFactor> {
        self.scale_factor
    }

    /// Returns the durable presentation preference, when one was recorded.
    #[must_use]
    pub const fn presentation(self) -> Option<WindowPresentationPreference> {
        self.presentation
    }
}

/// Validated viewport placement preferences indexed by stable surface identity.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViewportPlacementPreferences {
    surfaces: BTreeMap<SurfaceId, SurfacePresentationPreference>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct SurfacePresentationPreference {
    native_host: Option<NativeSurfaceHostPreference>,
    placement: Option<ViewportPlacementPreference>,
}

impl ViewportPlacementPreferences {
    /// Creates an empty preference set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            surfaces: BTreeMap::new(),
        }
    }

    /// Inserts or replaces the preference for one stable surface.
    pub fn set(
        &mut self,
        preference: ViewportPlacementPreference,
    ) -> Option<ViewportPlacementPreference> {
        self.surfaces
            .entry(preference.surface())
            .or_default()
            .placement
            .replace(preference)
    }

    /// Removes and returns the preference for one stable surface.
    pub fn remove(&mut self, surface: SurfaceId) -> Option<ViewportPlacementPreference> {
        self.surfaces.remove(&surface)?.placement
    }

    /// Returns one placement preference.
    #[must_use]
    pub fn get(&self, surface: SurfaceId) -> Option<&ViewportPlacementPreference> {
        self.surfaces.get(&surface)?.placement.as_ref()
    }

    /// Returns the preferences in stable surface order.
    pub fn iter(&self) -> impl Iterator<Item = &ViewportPlacementPreference> {
        self.surfaces
            .values()
            .filter_map(|entry| entry.placement.as_ref())
    }

    pub(crate) fn set_native_host(
        &mut self,
        surface: SurfaceId,
        hosting: NativeSurfaceHostPreference,
    ) {
        self.surfaces.entry(surface).or_default().native_host = Some(hosting);
    }

    pub(crate) fn native_host(&self, surface: SurfaceId) -> Option<NativeSurfaceHostPreference> {
        self.surfaces.get(&surface)?.native_host
    }

    pub(crate) fn clear_native_host(&mut self, surface: SurfaceId) {
        let Some(entry) = self.surfaces.get_mut(&surface) else {
            return;
        };
        entry.native_host = None;
        if entry.placement.is_none() {
            self.surfaces.remove(&surface);
        }
    }

    pub(crate) fn surface_ids(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.surfaces.keys().copied()
    }

    pub(crate) fn entries(
        &self,
    ) -> impl ExactSizeIterator<
        Item = (
            SurfaceId,
            Option<NativeSurfaceHostPreference>,
            Option<ViewportPlacementPreference>,
        ),
    > + '_ {
        self.surfaces
            .iter()
            .map(|(&surface, entry)| (surface, entry.native_host, entry.placement))
    }

    /// Returns the number of stored surface preferences.
    #[must_use]
    pub fn len(&self) -> usize {
        self.surfaces.len()
    }

    /// Returns whether no surface preferences are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.surfaces.is_empty()
    }

    pub(crate) fn retain_surfaces(&mut self, surfaces: &BTreeSet<SurfaceId>) {
        self.surfaces
            .retain(|surface, _| surfaces.contains(surface));
    }
}

/// A renderer-neutral viewport placement sidecar payload.
///
/// Snapshot records use scalar geometry for untrusted data. Call [`Self::restore`]
/// before using any record as a platform placement preference.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ViewportPlacementSnapshot {
    /// One record per stable logical surface.
    pub(crate) placements: Vec<SnapshotViewportPlacementRecord>,
}

impl ViewportPlacementSnapshot {
    /// Captures a canonical sidecar from validated preferences.
    #[must_use]
    pub(crate) fn capture(preferences: &ViewportPlacementPreferences) -> Self {
        Self {
            placements: preferences
                .entries()
                .map(|(surface, native_host, placement)| {
                    SnapshotViewportPlacementRecord::capture(surface, native_host, placement)
                })
                .collect(),
        }
    }

    /// Validates and restores this snapshot without mutating external state.
    ///
    /// # Errors
    ///
    /// Returns a structured error for duplicate surfaces,
    /// non-finite or invalid geometry, empty outer rectangles, and invalid scale
    /// factors.
    pub(crate) fn restore(
        &self,
    ) -> Result<ViewportPlacementPreferences, ViewportPlacementRestoreError> {
        let mut surfaces = BTreeSet::new();
        for record in &self.placements {
            if !surfaces.insert(record.surface) {
                return Err(ViewportPlacementRestoreError::DuplicateSurface {
                    surface: record.surface,
                });
            }
        }

        let mut restored = ViewportPlacementPreferences::new();
        for record in &self.placements {
            if let Some(preference) = record.restore()? {
                restored.set(preference);
            }
            if let Some(hosting) = record.native_host {
                restored.set_native_host(record.surface, hosting);
            }
            if record.outer_rect.is_none() && record.native_host.is_none() {
                return Err(ViewportPlacementRestoreError::EmptyPreference {
                    surface: record.surface,
                });
            }
        }
        Ok(restored)
    }
}

/// One untrusted persisted placement record.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotViewportPlacementRecord {
    /// Stable logical surface identity.
    pub(crate) surface: SurfaceId,
    /// Explicit hosting intent; a null value keeps the surface renderer-local.
    #[serde(deserialize_with = "crate::persistence::deserialize_required_option")]
    pub(crate) native_host: Option<NativeSurfaceHostPreference>,
    /// Last-confirmed native outer rectangle in desktop physical pixels.
    #[serde(deserialize_with = "crate::persistence::deserialize_required_option")]
    pub(crate) outer_rect: Option<SnapshotPhysicalRect>,
    /// Optional native inner size captured with the outer rectangle.
    #[serde(deserialize_with = "crate::persistence::deserialize_required_option")]
    pub(crate) inner_size: Option<SnapshotPhysicalSize>,
    /// Optional scale-factor hint from the confirming observation.
    #[serde(deserialize_with = "crate::persistence::deserialize_required_option")]
    pub(crate) scale_factor: Option<f64>,
    /// Optional durable window presentation preference.
    #[serde(deserialize_with = "crate::persistence::deserialize_required_option")]
    pub(crate) presentation: Option<WindowPresentationPreference>,
}

impl SnapshotViewportPlacementRecord {
    fn restore(self) -> Result<Option<ViewportPlacementPreference>, ViewportPlacementRestoreError> {
        let Some(outer) = self.outer_rect else {
            if self.inner_size.is_some()
                || self.scale_factor.is_some()
                || self.presentation.is_some()
            {
                return Err(
                    ViewportPlacementRestoreError::PlacementHintWithoutOuterRect {
                        surface: self.surface,
                    },
                );
            }
            return Ok(None);
        };
        let outer_rect =
            outer
                .restore()
                .map_err(|source| ViewportPlacementRestoreError::InvalidOuterRect {
                    surface: self.surface,
                    source,
                })?;
        let mut preference = ViewportPlacementPreference::new(self.surface, outer_rect)?;
        if let Some(value) = self.inner_size {
            let inner_size = value.restore().map_err(|source| {
                ViewportPlacementRestoreError::InvalidInnerSize {
                    surface: self.surface,
                    source,
                }
            })?;
            preference = preference.try_with_inner_size(inner_size)?;
        }
        if let Some(value) = self.scale_factor {
            let scale_factor = ScaleFactor::new(value).map_err(|source| {
                ViewportPlacementRestoreError::InvalidScaleFactor {
                    surface: self.surface,
                    source,
                }
            })?;
            preference = preference.with_scale_factor(scale_factor);
        }
        if let Some(presentation) = self.presentation {
            preference = preference.with_presentation(presentation);
        }
        Ok(Some(preference))
    }

    fn capture(
        surface: SurfaceId,
        native_host: Option<NativeSurfaceHostPreference>,
        placement: Option<ViewportPlacementPreference>,
    ) -> Self {
        Self {
            surface,
            native_host,
            outer_rect: placement
                .map(|preference| SnapshotPhysicalRect::from(preference.outer_rect())),
            inner_size: placement
                .and_then(ViewportPlacementPreference::inner_size)
                .map(SnapshotPhysicalSize::from),
            scale_factor: placement
                .and_then(ViewportPlacementPreference::scale_factor)
                .map(ScaleFactor::get),
            presentation: placement.and_then(ViewportPlacementPreference::presentation),
        }
    }
}

impl From<ViewportPlacementPreference> for SnapshotViewportPlacementRecord {
    fn from(preference: ViewportPlacementPreference) -> Self {
        Self::capture(preference.surface(), None, Some(preference))
    }
}

/// An untrusted physical size represented by scalar components.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotPhysicalSize {
    pub(crate) width: f64,
    pub(crate) height: f64,
}

impl SnapshotPhysicalSize {
    fn restore(self) -> Result<PhysicalSize, GeometryError> {
        PhysicalSize::new(self.width, self.height)
    }
}

impl From<PhysicalSize> for SnapshotPhysicalSize {
    fn from(size: PhysicalSize) -> Self {
        Self {
            width: size.width(),
            height: size.height(),
        }
    }
}

/// An untrusted physical rectangle represented by scalar components.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SnapshotPhysicalRect {
    /// Minimum desktop-physical horizontal coordinate.
    pub(crate) x: f64,
    /// Minimum desktop-physical vertical coordinate.
    pub(crate) y: f64,
    /// Physical width, which must be finite and strictly positive for placement.
    pub(crate) width: f64,
    /// Physical height, which must be finite and strictly positive for placement.
    pub(crate) height: f64,
}

impl SnapshotPhysicalRect {
    fn restore(self) -> Result<PhysicalRect, GeometryError> {
        PhysicalRect::new(self.x, self.y, self.width, self.height)
    }
}

impl From<PhysicalRect> for SnapshotPhysicalRect {
    fn from(rect: PhysicalRect) -> Self {
        Self {
            x: rect.x(),
            y: rect.y(),
            width: rect.width(),
            height: rect.height(),
        }
    }
}

/// Failure to restore a viewport placement sidecar.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum ViewportPlacementRestoreError {
    /// A row carries neither a native host declaration nor a placement.
    #[error("surface {surface} has no presentation preference")]
    EmptyPreference {
        /// Stable logical surface owning the empty row.
        surface: SurfaceId,
    },
    /// Geometry-dependent hints cannot stand alone without an outer rectangle.
    #[error("surface {surface} has placement hints without outer bounds")]
    PlacementHintWithoutOuterRect {
        /// Stable logical surface owning the hints.
        surface: SurfaceId,
    },
    /// Two records declare the same stable logical surface.
    #[error("duplicate viewport placement for surface {surface}")]
    DuplicateSurface {
        /// Repeated stable logical surface.
        surface: SurfaceId,
    },
    /// A record contains an invalid outer rectangle.
    #[error("surface {surface} has invalid physical outer rectangle: {source}")]
    InvalidOuterRect {
        /// Stable logical surface owning the record.
        surface: SurfaceId,
        /// Geometry validation failure.
        source: GeometryError,
    },
    /// A record contains a valid but empty rectangle, which cannot restore a window.
    #[error("surface {surface} has an empty physical outer rectangle")]
    EmptyOuterRect {
        /// Stable logical surface owning the record.
        surface: SurfaceId,
    },
    /// A record contains an invalid inner size.
    #[error("surface {surface} has an invalid physical inner size: {source}")]
    InvalidInnerSize {
        /// Stable logical surface owning the record.
        surface: SurfaceId,
        /// Geometry validation failure.
        source: GeometryError,
    },
    /// A record contains a valid but empty inner size.
    #[error("surface {surface} has an empty physical inner size")]
    EmptyInnerSize {
        /// Stable logical surface owning the record.
        surface: SurfaceId,
    },
    /// A record contains an invalid scale-factor hint.
    #[error("surface {surface} has an invalid scale-factor hint: {source}")]
    InvalidScaleFactor {
        /// Stable logical surface owning the record.
        surface: SurfaceId,
        /// Scale validation failure.
        source: GeometryError,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIRST: SurfaceId = SurfaceId::new(7);

    fn record() -> SnapshotViewportPlacementRecord {
        SnapshotViewportPlacementRecord {
            surface: FIRST,
            native_host: Some(NativeSurfaceHostPreference::Root),
            outer_rect: Some(SnapshotPhysicalRect {
                x: 100.0,
                y: -40.0,
                width: 900.0,
                height: 700.0,
            }),
            inner_size: Some(SnapshotPhysicalSize {
                width: 880.0,
                height: 660.0,
            }),
            scale_factor: Some(1.5),
            presentation: Some(WindowPresentationPreference::Maximized),
        }
    }

    #[test]
    fn internal_component_round_trip_drops_provider_local_work_area_identity() {
        let mut preferences = ViewportPlacementPreferences::new();
        preferences.set(
            record()
                .restore()
                .expect("fixture placement must validate")
                .expect("fixture placement has outer bounds")
                .with_work_area(WorkAreaToken::new(31)),
        );
        let snapshot = ViewportPlacementSnapshot::capture(&preferences);
        let json = serde_json::to_string(&snapshot).expect("component must encode");
        let restored = serde_json::from_str::<ViewportPlacementSnapshot>(&json)
            .expect("component payload must decode")
            .restore()
            .expect("component must validate");
        let placement = restored.get(FIRST).expect("placement must restore");
        assert_eq!(placement.work_area(), None);
        assert_eq!(
            placement.inner_size(),
            preferences.get(FIRST).unwrap().inner_size()
        );
        assert_eq!(
            placement.scale_factor(),
            preferences.get(FIRST).unwrap().scale_factor()
        );
        assert_eq!(
            placement.presentation(),
            preferences.get(FIRST).unwrap().presentation()
        );
    }

    #[test]
    fn internal_component_rejects_duplicate_surfaces_and_invalid_records() {
        let duplicate = ViewportPlacementSnapshot {
            placements: vec![record(), record()],
        };
        assert_eq!(
            duplicate
                .restore()
                .expect_err("duplicate surface must be rejected"),
            ViewportPlacementRestoreError::DuplicateSurface { surface: FIRST }
        );

        let mut invalid = record();
        invalid.outer_rect.as_mut().unwrap().width = 0.0;
        assert_eq!(
            ViewportPlacementSnapshot {
                placements: vec![invalid]
            }
            .restore()
            .expect_err("empty placement must be rejected"),
            ViewportPlacementRestoreError::EmptyOuterRect { surface: FIRST }
        );

        let mut invalid = record();
        invalid
            .inner_size
            .as_mut()
            .expect("fixture inner size exists")
            .width = 0.0;
        assert_eq!(
            ViewportPlacementSnapshot {
                placements: vec![invalid]
            }
            .restore()
            .expect_err("empty inner size must be rejected"),
            ViewportPlacementRestoreError::EmptyInnerSize { surface: FIRST }
        );
    }

    #[test]
    fn nullable_placement_fields_are_required_and_accept_explicit_null() {
        let snapshot = ViewportPlacementSnapshot {
            placements: vec![record()],
        };
        let base = serde_json::to_value(snapshot).expect("component must encode");
        for field in [
            "native_host",
            "outer_rect",
            "inner_size",
            "scale_factor",
            "presentation",
        ] {
            let mut explicit_null = base.clone();
            explicit_null["placements"][0][field] = serde_json::Value::Null;
            let decoded =
                serde_json::from_value::<ViewportPlacementSnapshot>(explicit_null.clone())
                    .expect("explicitly null placement fields must decode");
            let encoded = serde_json::to_value(decoded).expect("decoded component must encode");
            assert_eq!(encoded["placements"][0][field], serde_json::Value::Null);

            explicit_null["placements"][0]
                .as_object_mut()
                .expect("placement payload must be an object")
                .remove(field)
                .expect("nullable field must be present before removal");
            let error = serde_json::from_value::<ViewportPlacementSnapshot>(explicit_null)
                .expect_err("an absent nullable field must not become an intentional null");
            assert!(
                error
                    .to_string()
                    .contains(&format!("missing field `{field}`")),
                "{error}"
            );
        }
    }

    #[test]
    fn component_payload_rejects_unknown_and_duplicate_fields() {
        let snapshot = ViewportPlacementSnapshot {
            placements: vec![record()],
        };
        let base = serde_json::to_value(&snapshot).expect("component must encode");
        for path in [
            "",
            "/placements/0",
            "/placements/0/outer_rect",
            "/placements/0/inner_size",
            "/placements/0/native_host",
        ] {
            let mut value = base.clone();
            value
                .pointer_mut(path)
                .and_then(serde_json::Value::as_object_mut)
                .expect("schema boundary must be an object")
                .insert("window_token".into(), serde_json::json!(99));
            assert!(serde_json::from_value::<ViewportPlacementSnapshot>(value).is_err());
        }

        let encoded = serde_json::to_string(&snapshot).expect("component must encode");
        for duplicate in [
            r#"{"placements":[],"placements":[]}"#.to_owned(),
            encoded.replacen(
                r#""native_host":"#,
                r#""native_host":null,"native_host":"#,
                1,
            ),
        ] {
            assert!(serde_json::from_str::<ViewportPlacementSnapshot>(&duplicate).is_err());
        }
    }

    #[test]
    fn native_hosting_without_placement_preserves_identities_through_round_trip_and_pruning() {
        let child = SurfaceId::new(8);
        let mut preferences = ViewportPlacementPreferences::new();
        preferences.set_native_host(FIRST, NativeSurfaceHostPreference::Root);
        preferences.set_native_host(
            child,
            NativeSurfaceHostPreference::Child {
                recovery_host: FIRST,
            },
        );
        let json = serde_json::to_vec(&ViewportPlacementSnapshot::capture(&preferences)).unwrap();
        let mut restored = serde_json::from_slice::<ViewportPlacementSnapshot>(&json)
            .unwrap()
            .restore()
            .unwrap();
        assert_eq!(restored, preferences);
        assert!(restored.iter().next().is_none());
        assert_eq!(restored.surface_ids().collect::<Vec<_>>(), [FIRST, child]);
        assert_eq!(
            restored.native_host(FIRST),
            Some(NativeSurfaceHostPreference::Root)
        );
        assert_eq!(
            restored.native_host(child),
            Some(NativeSurfaceHostPreference::Child {
                recovery_host: FIRST
            })
        );
        restored.retain_surfaces(&BTreeSet::from([FIRST]));
        assert_eq!(restored.len(), 1);
        assert_eq!(restored.remove(FIRST), None);
        assert!(restored.is_empty());
    }

    #[test]
    fn incomplete_rows_and_hints_without_outer_bounds_are_rejected() {
        let mut invalid = record();
        invalid.outer_rect = None;
        assert_eq!(
            ViewportPlacementSnapshot {
                placements: vec![invalid]
            }
            .restore(),
            Err(ViewportPlacementRestoreError::PlacementHintWithoutOuterRect { surface: FIRST })
        );
        invalid.inner_size = None;
        invalid.scale_factor = None;
        invalid.presentation = None;
        invalid.native_host = None;
        assert_eq!(
            ViewportPlacementSnapshot {
                placements: vec![invalid]
            }
            .restore(),
            Err(ViewportPlacementRestoreError::EmptyPreference { surface: FIRST })
        );
    }
}
