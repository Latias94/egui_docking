//! Product-level surface measurement seam.

pub use crate::scene_manifest::{
    MeasurementValueError, TabListMenuMetrics, TabStripControlMetric, TabStripControlMetrics,
    TabStripControlPlacement, TabStripMetrics,
};

use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{ItemId, SurfaceId};
use crate::policy::TabBarVisibility;
use crate::scene::PaneSceneId;
use crate::scene_manifest::{Measurement, SurfaceMeasurements, TabIntrinsic};

use super::{
    DockspaceHostFrame, DockspaceInteractionError, DockspaceRuntimeError, DockspaceVisualId,
    SurfaceUnavailableReason,
};

/// Uniform measurements for a renderer whose panes and tabs share one metric.
///
/// Rich adapters should use [`DockspaceHostFrame::measure_surface_with`] so
/// pane, tab, popup, and tab-strip values can vary independently.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UniformSurfaceMetrics {
    pub(super) bounds: LogicalRect,
    pub(super) pane_minimum: LogicalSize,
    pub(super) tab_intrinsic: TabIntrinsic,
    pub(super) tab_strip: TabStripMetrics,
}

impl UniformSurfaceMetrics {
    /// Validates one complete uniform measurement profile.
    ///
    /// # Errors
    ///
    /// Returns an error when the tab content width is negative or non-finite.
    pub fn new(
        bounds: LogicalRect,
        pane_minimum: LogicalSize,
        tab_content_width: f64,
    ) -> Result<Self, DockspaceInteractionError> {
        let tab_intrinsic = TabIntrinsic::new(tab_content_width)
            .map_err(|_| DockspaceInteractionError::InvalidMeasurementProfile)?;
        let tab_strip = TabStripMetrics::new(0.0, 0.0)
            .map_err(|_| DockspaceInteractionError::InvalidMeasurementProfile)?;
        Ok(Self {
            bounds,
            pane_minimum,
            tab_intrinsic,
            tab_strip,
        })
    }
}

/// One core-derived product measurement question.
///
/// Structural node identities and exact-set manifest keys remain private. The
/// adapter sees only stable product items, opaque visual identities, and the
/// policy fact needed to measure renderer chrome.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceMeasurementRequest {
    /// Measure the logical rectangle occupied by the dock layout.
    DockBounds {
        /// Logical surface being measured.
        surface: SurfaceId,
    },
    /// Measure the logical rectangle in which transient popup chrome may appear.
    PopupPlaneBounds {
        /// Logical surface being measured.
        surface: SurfaceId,
    },
    /// Measure one pane content minimum.
    PaneMinimum {
        /// Stable renderer identity of the tabs leaf.
        visual: DockspaceVisualId,
        /// Pane item selected for the candidate layout, or `None` for an empty leaf.
        item: Option<ItemId>,
    },
    /// Measure one tab's text-and-icon content width before core padding.
    TabIntrinsic {
        /// Stable renderer identity of the tab.
        visual: DockspaceVisualId,
        /// Product pane item represented by the tab.
        item: ItemId,
    },
    /// Measure renderer-owned chrome around one tab strip.
    TabStrip {
        /// Stable renderer identity of the tab strip.
        visual: DockspaceVisualId,
        /// Frozen tab-bar visibility policy for this frame.
        visibility: TabBarVisibility,
    },
}

/// One answer to a [`SurfaceMeasurementRequest`].
///
/// The answer variant must match the request variant. Core performs that join,
/// validates scalar values, builds the exact measurement roster, and rejects
/// the complete contribution on any mismatch.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SurfaceMeasurementAnswer {
    /// Logical bounds for either a dock layout or popup plane request.
    Bounds(LogicalRect),
    /// Minimum logical content size for one pane request.
    PaneMinimum(LogicalSize),
    /// Text-and-icon content width for one tab request.
    TabIntrinsic(f64),
    /// Renderer-owned tab-strip chrome measurements.
    TabStrip(TabStripMetrics),
    /// The adapter participated but could not authoritatively measure the value.
    Unavailable(SurfaceUnavailableReason),
}

impl SurfaceMeasurementAnswer {
    fn into_bounds(self) -> Result<Measurement<LogicalRect>, DockspaceInteractionError> {
        match self {
            Self::Bounds(value) => Ok(Measurement::Measured(value)),
            Self::Unavailable(reason) => Ok(Measurement::Unavailable(reason.into())),
            Self::PaneMinimum(_) | Self::TabIntrinsic(_) | Self::TabStrip(_) => {
                Err(DockspaceInteractionError::MeasurementAnswerMismatch)
            }
        }
    }

    fn into_pane_minimum(self) -> Result<Measurement<LogicalSize>, DockspaceInteractionError> {
        match self {
            Self::PaneMinimum(value) => Ok(Measurement::Measured(value)),
            Self::Unavailable(reason) => Ok(Measurement::Unavailable(reason.into())),
            Self::Bounds(_) | Self::TabIntrinsic(_) | Self::TabStrip(_) => {
                Err(DockspaceInteractionError::MeasurementAnswerMismatch)
            }
        }
    }

    fn into_tab_intrinsic(self) -> Result<Measurement<TabIntrinsic>, DockspaceInteractionError> {
        match self {
            Self::TabIntrinsic(content_width) => TabIntrinsic::new(content_width)
                .map(Measurement::Measured)
                .map_err(|_| DockspaceInteractionError::InvalidMeasurementProfile),
            Self::Unavailable(reason) => Ok(Measurement::Unavailable(reason.into())),
            Self::Bounds(_) | Self::PaneMinimum(_) | Self::TabStrip(_) => {
                Err(DockspaceInteractionError::MeasurementAnswerMismatch)
            }
        }
    }

    fn into_tab_strip(self) -> Result<Measurement<TabStripMetrics>, DockspaceInteractionError> {
        match self {
            Self::TabStrip(value) => Ok(Measurement::Measured(value)),
            Self::Unavailable(reason) => Ok(Measurement::Unavailable(reason.into())),
            Self::Bounds(_) | Self::PaneMinimum(_) | Self::TabIntrinsic(_) => {
                Err(DockspaceInteractionError::MeasurementAnswerMismatch)
            }
        }
    }
}

impl DockspaceHostFrame<'_> {
    /// Resolves every exact surface measurement through product-level questions.
    ///
    /// Core owns deterministic traversal, structural identities, exact-set
    /// assembly, and validation. The resolver may return an explicit
    /// [`SurfaceMeasurementAnswer::Unavailable`] for any individual value.
    ///
    /// # Errors
    ///
    /// Returns an error when the surface is outside the frozen roster, already
    /// answered, an answer variant does not match its request, or the resulting
    /// exact contribution is invalid. Dropping the host frame rolls the entire
    /// contribution back.
    pub fn measure_surface_with(
        &mut self,
        surface: SurfaceId,
        mut resolve: impl FnMut(SurfaceMeasurementRequest) -> SurfaceMeasurementAnswer,
    ) -> Result<(), DockspaceRuntimeError> {
        self.complete_pointer_input()?;
        if self.surface_answered(surface) {
            return Err(DockspaceInteractionError::SurfaceAlreadyAnswered { surface }.into());
        }
        let requirements = self
            .frame
            .view()
            .presentation_requirements()
            .surface(surface)
            .ok_or(DockspaceInteractionError::SurfaceOutsideRoster { surface })?;
        let mut measurements = SurfaceMeasurements::new(requirements.ticket());
        measurements
            .set_bounds(
                requirements.bounds(),
                resolve(SurfaceMeasurementRequest::DockBounds { surface }).into_bounds()?,
            )
            .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        if let Some(key) = requirements.popup_plane_bounds() {
            measurements
                .set_popup_plane_bounds(
                    key,
                    resolve(SurfaceMeasurementRequest::PopupPlaneBounds { surface })
                        .into_bounds()?,
                )
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        for key in requirements.pane_minimums() {
            let visual = DockspaceVisualId::from_pane_scene(PaneSceneId {
                root: key.root(),
                tabs: key.tabs(),
            });
            measurements
                .insert_pane_minimum(
                    key,
                    resolve(SurfaceMeasurementRequest::PaneMinimum {
                        visual,
                        item: key.selected(),
                    })
                    .into_pane_minimum()?,
                )
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        for key in requirements.tab_intrinsics() {
            let tab = key.tab();
            measurements
                .insert_tab_intrinsic(
                    key,
                    resolve(SurfaceMeasurementRequest::TabIntrinsic {
                        visual: DockspaceVisualId::from_tab_scene(tab),
                        item: tab.item,
                    })
                    .into_tab_intrinsic()?,
                )
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        for key in requirements.tab_strips() {
            let bar = key.bar();
            let visibility = requirements
                .tab_bar(bar)
                .ok_or(DockspaceInteractionError::MeasurementRosterInvariant)?
                .policy()
                .visibility();
            measurements
                .insert_tab_strip(
                    key,
                    resolve(SurfaceMeasurementRequest::TabStrip {
                        visual: DockspaceVisualId::from_tab_bar_scene(bar),
                        visibility,
                    })
                    .into_tab_strip()?,
                )
                .map_err(|_| DockspaceInteractionError::MeasurementRosterInvariant)?;
        }
        let token = self.frame.view().begin_surface_contribution(surface)?;
        let contribution = self
            .frame
            .view()
            .prepare_surface_contribution(token, measurements)?;
        self.frame.push_surface_contribution(contribution)?;
        Ok(())
    }
}
