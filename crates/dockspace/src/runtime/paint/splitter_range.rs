//! On-demand resize limits from the same touching-frontier solver as gestures.

use crate::graph::Axis;
use crate::presentation::scene::SplitterResizeTarget;

use super::super::SurfaceSplitterAdjustment;
use super::{DockspaceVisualId, SurfacePaintPlan, VisualIdentity};

/// Current and reachable sizes of the pane immediately before a splitter.
///
/// Values are fractions of the two adjacent panes' combined extent, excluding
/// their separator. Limits include all nested panes touching the moving edge,
/// not just the adjacent panes' own minimum sizes. They describe this exact
/// candidate and grant no input or presentation authority.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SurfaceSplitterRange {
    value: f64,
    minimum: f64,
    maximum: f64,
}

impl SurfaceSplitterRange {
    /// Returns the current fraction occupied by the preceding pane.
    #[must_use]
    pub const fn value(self) -> f64 {
        self.value
    }

    /// Returns the smallest reachable fraction under the full layout constraints.
    #[must_use]
    pub const fn minimum(self) -> f64 {
        self.minimum
    }

    /// Returns the largest reachable fraction under the full layout constraints.
    #[must_use]
    pub const fn maximum(self) -> f64 {
        self.maximum
    }
}

impl SurfacePaintPlan<'_> {
    /// Resolves the exact reachable range for a splitter in this candidate.
    ///
    /// Returns `None` when resizing is unavailable or the layout cannot prove
    /// its limits. This walks the root's touching layout and allocates temporary
    /// solver state. Query it when numeric accessibility values or application
    /// controls need the range, rather than for ordinary splitter drawing.
    #[must_use]
    pub fn splitter_resize_range(
        self,
        splitter: DockspaceVisualId,
    ) -> Option<SurfaceSplitterRange> {
        let VisualIdentity::Splitter(id) = splitter.0 else {
            return None;
        };
        let record = self
            .plan
            .splitter_record(id)
            .filter(|record| record.operable())?;
        let limits = self.candidate.splitter_resize_delta_interval(
            self.plan,
            SplitterResizeTarget::Handle(id),
            record.axis(),
        )?;
        let before = *record.child_extents().get(id.index)?;
        let after = *record.child_extents().get(id.index + 1)?;
        let total = before + after;
        if !total.is_finite() || total <= 0.0 {
            return None;
        }
        Some(SurfaceSplitterRange {
            value: before / total,
            minimum: ((before + limits.minimum) / total).clamp(0.0, 1.0),
            maximum: ((before + limits.maximum) / total).clamp(0.0, 1.0),
        })
    }

    pub(super) fn splitter_adjustment_delta(
        self,
        target: SplitterResizeTarget,
        axis: Axis,
        adjustment: SurfaceSplitterAdjustment,
    ) -> Option<f64> {
        match adjustment {
            SurfaceSplitterAdjustment::Decrement => Some(-self.splitter_keyboard_step),
            SurfaceSplitterAdjustment::Increment => Some(self.splitter_keyboard_step),
            SurfaceSplitterAdjustment::Minimum | SurfaceSplitterAdjustment::Maximum => {
                let limits = self
                    .candidate
                    .splitter_resize_delta_interval(self.plan, target, axis)?;
                Some(match adjustment {
                    SurfaceSplitterAdjustment::Minimum => limits.minimum,
                    _ => limits.maximum,
                })
            }
        }
    }
}
