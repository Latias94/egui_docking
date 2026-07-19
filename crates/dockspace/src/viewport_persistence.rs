//! Versioned persistence for native viewport placement preferences.
//!
//! This sidecar is deliberately independent from
//! [`crate::persistence::WorkspaceSnapshot`]. It stores only stable logical
//! surface identities and last-confirmed placement hints. Native window tokens,
//! incarnations, focus, hover, scene proofs, and other session authority never
//! cross this boundary.
//!
//! Work-area tokens and scale factors are hints from the observation which
//! confirmed the outer rectangle. A platform adapter must revalidate them
//! against its current authoritative inventory before applying a restored
//! preference.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
use serde::ser::SerializeTuple;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use thiserror::Error;

use crate::geometry::{GeometryError, PhysicalRect, ScaleFactor};
use crate::ids::SurfaceId;
use crate::viewport::WorkAreaToken;

/// The viewport-placement sidecar schema emitted and accepted by this release.
pub const VIEWPORT_PLACEMENT_SNAPSHOT_VERSION: u32 = 1;

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
            work_area: None,
            scale_factor: None,
            presentation: None,
        })
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
    placements: BTreeMap<SurfaceId, ViewportPlacementPreference>,
}

impl ViewportPlacementPreferences {
    /// Creates an empty preference set.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            placements: BTreeMap::new(),
        }
    }

    /// Inserts or replaces the preference for one stable surface.
    pub fn set(
        &mut self,
        preference: ViewportPlacementPreference,
    ) -> Option<ViewportPlacementPreference> {
        self.placements.insert(preference.surface(), preference)
    }

    /// Removes and returns the preference for one stable surface.
    pub fn remove(&mut self, surface: SurfaceId) -> Option<ViewportPlacementPreference> {
        self.placements.remove(&surface)
    }

    /// Returns one placement preference.
    #[must_use]
    pub fn get(&self, surface: SurfaceId) -> Option<&ViewportPlacementPreference> {
        self.placements.get(&surface)
    }

    /// Returns the preferences in stable surface order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &ViewportPlacementPreference> {
        self.placements.values()
    }

    /// Returns the number of stored surface preferences.
    #[must_use]
    pub fn len(&self) -> usize {
        self.placements.len()
    }

    /// Returns whether no surface preferences are stored.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.placements.is_empty()
    }
}

/// A versioned, renderer-neutral viewport placement sidecar.
///
/// Snapshot records intentionally expose scalar geometry so callers can inspect
/// or migrate untrusted data. Call [`Self::restore`] before using any record as
/// a platform placement preference.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewportPlacementSnapshot {
    /// Snapshot schema version.
    pub version: u32,
    /// One record per stable logical surface.
    pub placements: Vec<SnapshotViewportPlacementRecord>,
}

#[derive(Serialize)]
struct ViewportPlacementSnapshotPayloadRef<'snapshot> {
    placements: &'snapshot [SnapshotViewportPlacementRecord],
}

impl Serialize for ViewportPlacementSnapshot {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut document = serializer.serialize_tuple(2)?;
        document.serialize_element(&self.version)?;
        document.serialize_element(&ViewportPlacementSnapshotPayloadRef {
            placements: &self.placements,
        })?;
        document.end()
    }
}

impl ViewportPlacementSnapshot {
    /// Captures a canonical sidecar from validated preferences.
    #[must_use]
    pub fn capture(preferences: &ViewportPlacementPreferences) -> Self {
        Self {
            version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
            placements: preferences
                .iter()
                .copied()
                .map(SnapshotViewportPlacementRecord::from)
                .collect(),
        }
    }

    /// Validates and restores this snapshot without mutating external state.
    ///
    /// # Errors
    ///
    /// Returns a structured error for unsupported versions, duplicate surfaces,
    /// non-finite or invalid geometry, empty outer rectangles, and invalid scale
    /// factors.
    pub fn restore(&self) -> Result<ViewportPlacementPreferences, ViewportPlacementRestoreError> {
        if self.version != VIEWPORT_PLACEMENT_SNAPSHOT_VERSION {
            return Err(ViewportPlacementRestoreError::UnsupportedVersion {
                found: self.version,
                supported: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
            });
        }

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
            let preference = record.restore()?;
            restored.set(preference);
        }
        Ok(restored)
    }
}

/// One untrusted persisted placement record.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotViewportPlacementRecord {
    /// Stable logical surface identity.
    pub surface: SurfaceId,
    /// Last-confirmed native outer rectangle in desktop physical pixels.
    pub outer_rect: SnapshotPhysicalRect,
    /// Optional provider-defined work-area identity hint.
    pub work_area: Option<u64>,
    /// Optional scale-factor hint from the confirming observation.
    pub scale_factor: Option<f64>,
    /// Optional durable window presentation preference.
    pub presentation: Option<WindowPresentationPreference>,
}

impl SnapshotViewportPlacementRecord {
    fn restore(self) -> Result<ViewportPlacementPreference, ViewportPlacementRestoreError> {
        let outer_rect = self.outer_rect.restore().map_err(|source| {
            ViewportPlacementRestoreError::InvalidOuterRect {
                surface: self.surface,
                source,
            }
        })?;
        let mut preference = ViewportPlacementPreference::new(self.surface, outer_rect)?;
        if let Some(work_area) = self.work_area {
            preference = preference.with_work_area(WorkAreaToken::new(work_area));
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
        Ok(preference)
    }
}

impl From<ViewportPlacementPreference> for SnapshotViewportPlacementRecord {
    fn from(preference: ViewportPlacementPreference) -> Self {
        Self {
            surface: preference.surface(),
            outer_rect: SnapshotPhysicalRect::from(preference.outer_rect()),
            work_area: preference.work_area().map(WorkAreaToken::get),
            scale_factor: preference.scale_factor().map(ScaleFactor::get),
            presentation: preference.presentation(),
        }
    }
}

/// An untrusted physical rectangle represented by scalar components.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotPhysicalRect {
    /// Minimum desktop-physical horizontal coordinate.
    pub x: f64,
    /// Minimum desktop-physical vertical coordinate.
    pub y: f64,
    /// Physical width, which must be finite and strictly positive for placement.
    pub width: f64,
    /// Physical height, which must be finite and strictly positive for placement.
    pub height: f64,
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ViewportPlacementSnapshotV1 {
    placements: Vec<SnapshotViewportPlacementRecord>,
}

impl ViewportPlacementSnapshotV1 {
    fn into_snapshot(self) -> ViewportPlacementSnapshot {
        ViewportPlacementSnapshot {
            version: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
            placements: self.placements,
        }
    }
}

/// Version-first decoder for a self-describing viewport placement sidecar.
///
/// Deserialize external data into this envelope, then call
/// [`Self::into_snapshot`] or [`Self::into_preferences`]. Unsupported versions
/// accept any payload shape so callers receive a typed version error instead of
/// a misleading current-schema decoding error.
#[derive(Clone, Debug, PartialEq)]
pub struct ViewportPlacementSnapshotEnvelope {
    version: u32,
    snapshot: Option<ViewportPlacementSnapshot>,
}

impl ViewportPlacementSnapshotEnvelope {
    /// Returns the schema version declared by the serialized document.
    #[must_use]
    pub const fn version(&self) -> u32 {
        self.version
    }

    /// Returns the strictly decoded snapshot for the supported schema version.
    ///
    /// # Errors
    ///
    /// Returns [`ViewportPlacementRestoreError::UnsupportedVersion`] for every
    /// version other than [`VIEWPORT_PLACEMENT_SNAPSHOT_VERSION`].
    pub fn into_snapshot(self) -> Result<ViewportPlacementSnapshot, ViewportPlacementRestoreError> {
        self.snapshot
            .ok_or(ViewportPlacementRestoreError::UnsupportedVersion {
                found: self.version,
                supported: VIEWPORT_PLACEMENT_SNAPSHOT_VERSION,
            })
    }

    /// Decodes and validates the complete sidecar without mutating external state.
    ///
    /// # Errors
    ///
    /// Returns a structured version, identity, geometry, or scale-factor error.
    pub fn into_preferences(
        self,
    ) -> Result<ViewportPlacementPreferences, ViewportPlacementRestoreError> {
        self.into_snapshot()?.restore()
    }
}

impl<'de> Deserialize<'de> for ViewportPlacementSnapshotEnvelope {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_tuple(2, ViewportPlacementSnapshotEnvelopeVisitor)
    }
}

struct ViewportPlacementSnapshotEnvelopeVisitor;

impl<'de> Visitor<'de> for ViewportPlacementSnapshotEnvelopeVisitor {
    type Value = ViewportPlacementSnapshotEnvelope;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a [version, payload] viewport placement document")
    }

    fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let version = sequence
            .next_element::<u32>()?
            .ok_or_else(|| A::Error::invalid_length(0, &self))?;
        let snapshot = if version == VIEWPORT_PLACEMENT_SNAPSHOT_VERSION {
            let payload = sequence
                .next_element::<ViewportPlacementSnapshotV1>()?
                .ok_or_else(|| A::Error::invalid_length(1, &self))?;
            Some(payload.into_snapshot())
        } else {
            sequence
                .next_element::<IgnoredAny>()?
                .ok_or_else(|| A::Error::invalid_length(1, &self))?;
            None
        };
        if sequence.next_element::<IgnoredAny>()?.is_some() {
            return Err(A::Error::invalid_length(3, &self));
        }
        Ok(ViewportPlacementSnapshotEnvelope { version, snapshot })
    }
}

/// Failure to restore a viewport placement sidecar.
#[derive(Clone, Debug, PartialEq, Error)]
pub enum ViewportPlacementRestoreError {
    /// The snapshot version is not implemented by this crate release.
    #[error(
        "unsupported viewport placement snapshot version {found}; supported version is {supported}"
    )]
    UnsupportedVersion {
        /// Version read from the snapshot.
        found: u32,
        /// Version implemented by this crate release.
        supported: u32,
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
    /// A record contains an invalid scale-factor hint.
    #[error("surface {surface} has an invalid scale-factor hint: {source}")]
    InvalidScaleFactor {
        /// Stable logical surface owning the record.
        surface: SurfaceId,
        /// Scale validation failure.
        source: GeometryError,
    },
}
