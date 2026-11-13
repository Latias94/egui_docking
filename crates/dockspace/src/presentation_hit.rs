//! Output-bound, renderer-neutral presentation hit inventory.
//!
//! This module indexes geometry already compiled into a
//! [`crate::scene::PresentationPlan`]. It does not project layout, choose drop
//! semantics, or prove that a UI framework actually delivered an event to a
//! docking receiver.

use std::cmp::Ordering;
use std::convert::Infallible;
use std::sync::Arc;

use crate::drop_guide::{DropGuideClusterRecord, DropGuideScope};
use crate::drop_target::{DropSourceSuppression, DropTargetKind, SceneLayerKey};
use crate::geometry::LogicalRect;
use crate::hit_region::HitRegion;
#[cfg(test)]
use crate::ids::RootId;
use crate::ids::{FloatingPresentationId, SurfaceId};
use crate::pointer_journal::PointerDeviceKind;
use crate::presentation_observation::SurfacePresentationOutputTicket;
#[cfg(feature = "motion")]
use crate::scene::PresentationMenuAnchorHost;
use crate::scene::{
    ContainedResizeDirection, PaneSceneId, PresentationPlan, ResolvedSurfaceProgramOperation,
    ResolvedSurfaceProgramStep, SceneBuildError, SplitterJunctionId, SplitterSceneId,
    TabBarSceneId, TabGroupDragRegionKind, TabListMenuBackdropRecord, TabListMenuRecord,
    TabSceneId,
};
use crate::tab_strip::{TabListMenuSessionId, TabStripControlId};
use crate::{drop_guide::DropGuideClusterId, drop_target::DropTargetId};

mod geometry_reuse;
mod region_order;
mod touch;

#[cfg(test)]
#[path = "presentation_hit/row_tests.rs"]
mod row_tests;

/// Stable semantic identity of one core-compiled hit region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationHitRegionId {
    surface: SurfaceId,
    kind: PresentationHitRegionKind,
}

impl PresentationHitRegionId {
    const fn new(surface: SurfaceId, kind: PresentationHitRegionKind) -> Self {
        Self { surface, kind }
    }

    /// Returns the sole surface whose logical coordinates define this region.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the stable semantic control or affordance identity.
    #[must_use]
    pub const fn kind(self) -> PresentationHitRegionKind {
        self.kind
    }
}

/// Renderer-neutral class and structural identity of one hit region.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresentationHitRegionKind {
    /// Selected pane content acts as the fallback receiver below docking chrome.
    PaneBody(PaneSceneId),
    /// Select or begin dragging one visible tab.
    TabBody(TabSceneId),
    /// Request closing one visible tab.
    TabClose(TabSceneId),
    /// Consume activation for one core-compiled tab-strip control.
    ///
    /// The exact [`crate::scene::TabStripControlRecord`] retains whether the
    /// control is enabled. Disabled controls remain receivers so input cannot
    /// fall through to workspace content.
    TabStripControl(TabStripControlId),
    /// Scroll the exact overflowing viewport of one tab strip.
    TabStripScroll(TabBarSceneId),
    /// Activate one visible row in an exact tab-list menu instance.
    TabListMenuRow {
        /// Core-owned popup instance which owns this row.
        menu: TabListMenuSessionId,
        /// Stable tab identity selected by the row.
        tab: TabSceneId,
    },
    /// Scroll the exact viewport of one active tab-list menu.
    TabListMenuScroll(TabListMenuSessionId),
    /// Consume all otherwise-unclaimed pointer lanes inside one menu frame.
    TabListMenuBlocker(TabListMenuSessionId),
    /// Consume pointer input outside the menu frame across one exact surface.
    TabListMenuBackdrop(TabListMenuSessionId),
    /// Begin dragging a complete tabs group from one exact empty strip region.
    TabGroupDrag {
        /// Stable tab-bar identity shared by every region for the same group.
        bar: TabBarSceneId,
        /// Distinct leading or trailing region identity.
        region: TabGroupDragRegionKind,
    },
    /// Begin resizing one split boundary.
    SplitterHandle(SplitterSceneId),
    /// Begin an atomic multi-handle resize at one splitter junction.
    SplitterJunction(SplitterJunctionId),
    /// Lowest same-floating fallback below contained chrome and child content.
    ContainedFrameBlocker(FloatingPresentationId),
    /// Begin moving one contained floating presentation.
    ContainedTitle(FloatingPresentationId),
    /// Request closing one contained floating presentation.
    ContainedClose(FloatingPresentationId),
    /// Begin resizing one contained floating presentation.
    ContainedResize {
        /// Stable contained presentation identity.
        floating: FloatingPresentationId,
        /// Exact edge or corner resize direction.
        direction: ContainedResizeDirection,
    },
    /// Passive region which may expose one docking-guide cluster.
    DropGuideActivation(DropGuideClusterId),
    /// Exact guide button, tab gap, topology target, or rootless background.
    DropTarget(DropTargetId),
}

/// Independent pointer-receiver lane used when validating an event receipt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresentationPointerLane {
    /// Click/select/close activation.
    Click,
    /// Drag or resize activation.
    Drag,
    /// Active-drag hover and drop delivery.
    HoverDrop,
    /// Wheel or phaseful trackpad delivery.
    Scroll,
}

/// Exact set of pointer lanes supported by one presentation region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct PresentationPointerLanes(u8);

impl PresentationPointerLanes {
    const CLICK_BIT: u8 = 1 << 0;
    const DRAG_BIT: u8 = 1 << 1;
    const HOVER_DROP_BIT: u8 = 1 << 2;
    const SCROLL_BIT: u8 = 1 << 3;

    /// Click-only receiver lanes.
    pub const CLICK: Self = Self(Self::CLICK_BIT);
    /// Drag-only receiver lanes.
    pub const DRAG: Self = Self(Self::DRAG_BIT);
    /// Active-drag hover/drop-only receiver lanes.
    pub const HOVER_DROP: Self = Self(Self::HOVER_DROP_BIT);
    /// Scroll-only receiver lane.
    pub const SCROLL: Self = Self(Self::SCROLL_BIT);
    /// Shared click and drag receiver lanes.
    pub const CLICK_AND_DRAG: Self = Self(Self::CLICK_BIT | Self::DRAG_BIT);
    /// Every pointer receiver lane.
    pub const ALL: Self =
        Self(Self::CLICK_BIT | Self::DRAG_BIT | Self::HOVER_DROP_BIT | Self::SCROLL_BIT);

    /// Returns whether this region participates in `lane`.
    #[must_use]
    pub const fn contains(self, lane: PresentationPointerLane) -> bool {
        let bit = match lane {
            PresentationPointerLane::Click => Self::CLICK_BIT,
            PresentationPointerLane::Drag => Self::DRAG_BIT,
            PresentationPointerLane::HoverDrop => Self::HOVER_DROP_BIT,
            PresentationPointerLane::Scroll => Self::SCROLL_BIT,
        };
        self.0 & bit != 0
    }
}

/// Semantic presentation plane used for deterministic receiver comparison.
///
/// Popup identities are core-minted. Adapters may inspect a plane but cannot
/// construct a popup session from a numeric priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PresentationPlane {
    /// Normal workspace chrome and content.
    Workspace,
    /// One exact core-owned popup instance above every workspace layer.
    Popup(TabListMenuSessionId),
}

/// Semantic precedence within one popup receiver plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PopupHitRole {
    /// Full-surface outside-dismiss receiver below the menu frame.
    Backdrop,
    /// Menu-frame padding and otherwise unclaimed frame content.
    FrameBlocker,
    /// One activatable menu row.
    Row,
    /// Exact menu viewport which owns scroll delivery.
    Scroll,
}

/// Opaque, core-owned stack key for deterministic receiver comparison.
///
/// Larger keys are frontmost. Adapters may compare keys, but cannot construct a
/// numeric priority.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PresentationHitStackKey {
    plane: PresentationPlane,
    popup_role: Option<PopupHitRole>,
    layer: SceneLayerKey,
    #[cfg(feature = "motion")]
    pane_rank: (bool, usize),
    precedence: PresentationHitPrecedence,
}

impl PresentationHitStackKey {
    const fn new(
        plane: PresentationPlane,
        layer: SceneLayerKey,
        precedence: PresentationHitPrecedence,
    ) -> Self {
        Self {
            plane,
            popup_role: None,
            layer,
            #[cfg(feature = "motion")]
            pane_rank: (false, 0),
            precedence,
        }
    }

    const fn popup(
        session: TabListMenuSessionId,
        role: PopupHitRole,
        precedence: PresentationHitPrecedence,
    ) -> Self {
        Self {
            plane: PresentationPlane::Popup(session),
            popup_role: Some(role),
            layer: SceneLayerKey::surface_base(),
            #[cfg(feature = "motion")]
            pane_rank: (false, 0),
            precedence,
        }
    }

    /// Returns the semantic plane compared before structural workspace layers.
    #[must_use]
    #[cfg(test)]
    pub const fn plane(self) -> PresentationPlane {
        self.plane
    }

    /// Returns semantic popup precedence independent of workspace structure.
    #[must_use]
    #[cfg(test)]
    pub const fn popup_role(self) -> Option<PopupHitRole> {
        self.popup_role
    }

    /// Returns the workspace-roster-derived structural layer.
    #[must_use]
    pub const fn layer(self) -> SceneLayerKey {
        self.layer
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
enum PresentationHitPrecedence {
    FallbackBlocker,
    PopupRow,
    PopupScroll,
    PaneBody,
    TabBody,
    TabGroupDrag,
    TabClose,
    TabStripControl,
    TabStripScroll,
    SplitterHandle,
    SplitterJunction,
    ContainedTitle,
    ContainedClose,
    ContainedResize,
    StandaloneSurfaceBackground,
    StandaloneOuterEdge,
    StandaloneInnerEdge,
    StandaloneCenter,
    StandaloneTabGap,
    InnerGuideTarget,
    OuterGuideTarget,
    PassiveGuideActivation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PresentationHitBehavior {
    ExclusiveReceiver,
    FallbackBlocker,
    PassiveAffordance,
}

impl DropSourceSuppression {
    fn excludes(self, id: PresentationHitRegionId) -> bool {
        match id.kind() {
            PresentationHitRegionKind::ContainedFrameBlocker(floating) => {
                self.excludes_occlusion(floating)
            }
            PresentationHitRegionKind::DropGuideActivation(cluster) => {
                self.excludes_cluster(cluster)
            }
            PresentationHitRegionKind::DropTarget(target) => self.excludes_target(target),
            PresentationHitRegionKind::PaneBody(_)
            | PresentationHitRegionKind::TabBody(_)
            | PresentationHitRegionKind::TabClose(_)
            | PresentationHitRegionKind::TabStripControl(_)
            | PresentationHitRegionKind::TabStripScroll(_)
            | PresentationHitRegionKind::TabListMenuRow { .. }
            | PresentationHitRegionKind::TabListMenuScroll(_)
            | PresentationHitRegionKind::TabListMenuBlocker(_)
            | PresentationHitRegionKind::TabListMenuBackdrop(_)
            | PresentationHitRegionKind::TabGroupDrag { .. }
            | PresentationHitRegionKind::SplitterHandle(_)
            | PresentationHitRegionKind::SplitterJunction(_)
            | PresentationHitRegionKind::ContainedTitle(_)
            | PresentationHitRegionKind::ContainedClose(_)
            | PresentationHitRegionKind::ContainedResize { .. } => false,
        }
    }
}

/// One exact region in a core-compiled presentation hit inventory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PresentationHitRegion {
    id: PresentationHitRegionId,
    hit: HitRegion,
    anchor: LogicalRect,
    lanes: PresentationPointerLanes,
    stack: PresentationHitStackKey,
    behavior: PresentationHitBehavior,
}

impl PresentationHitRegion {
    fn new(
        id: PresentationHitRegionId,
        hit: HitRegion,
        lanes: PresentationPointerLanes,
        stack: PresentationHitStackKey,
        behavior: PresentationHitBehavior,
    ) -> Self {
        Self {
            id,
            hit,
            anchor: hit.rect(),
            lanes,
            stack,
            behavior,
        }
    }

    const fn with_anchor(mut self, anchor: LogicalRect) -> Self {
        self.anchor = anchor;
        self
    }

    /// Returns the stable semantic identity of this region.
    #[must_use]
    pub const fn id(self) -> PresentationHitRegionId {
        self.id
    }

    /// Returns the half-open base hit geometry.
    ///
    /// This may be empty when only the device-specific touch target survives clipping.
    #[must_use]
    pub const fn hit(self) -> HitRegion {
        self.hit
    }

    /// Returns the supported receiver lanes.
    #[must_use]
    pub const fn lanes(self) -> PresentationPointerLanes {
        self.lanes
    }

    /// Returns the opaque deterministic stack key.
    #[must_use]
    pub const fn stack(self) -> PresentationHitStackKey {
        self.stack
    }

    /// Returns whether this entry is a passive affordance predicate rather
    /// than an exclusive receiver candidate.
    #[must_use]
    pub const fn is_passive(self) -> bool {
        matches!(self.behavior, PresentationHitBehavior::PassiveAffordance)
    }
}

/// Canonical hit inventory bound to one exact semantic presentation output.
#[derive(Debug, Clone)]
pub struct PresentationHitManifest {
    output: SurfacePresentationOutputTicket,
    geometry: Arc<geometry_reuse::HitGeometry>,
}

impl PartialEq for PresentationHitManifest {
    fn eq(&self, other: &Self) -> bool {
        self.output == other.output
            && (Arc::ptr_eq(&self.geometry, &other.geometry) || self.geometry == other.geometry)
    }
}

impl PresentationHitManifest {
    pub(crate) fn rebind_output(&self, output: SurfacePresentationOutputTicket) -> Self {
        Self {
            output,
            geometry: Arc::clone(&self.geometry),
        }
    }

    #[cfg(test)]
    pub(crate) fn compile(
        output: SurfacePresentationOutputTicket,
        plan: &PresentationPlan,
    ) -> Result<Self, SceneBuildError> {
        Self::compile_with_order(output, plan, None)
    }

    fn compile_with_order(
        output: SurfacePresentationOutputTicket,
        plan: &PresentationPlan,
        previous: Option<&geometry_reuse::HitGeometry>,
    ) -> Result<Self, SceneBuildError> {
        debug_assert_eq!(output.surface(), plan.surface());
        let surface = plan.surface();
        let mut regions = Vec::new();
        emit_regions(plan, &mut |region| regions.push(region))?;
        let emission_rows = region_order::canonicalize_reusing_order(&mut regions, previous);
        debug_assert!(regions.windows(2).all(|pair| pair[0].id() != pair[1].id()));
        let touch = touch::compile_reusing_order(
            plan,
            &regions,
            previous.map(|previous| previous.touch_order.as_ref()),
        );
        let mut touch_regions = touch.fragments;
        compact_region_rows(&mut regions, &mut touch_regions)
            .map_err(|()| SceneBuildError::InvalidSurfaceProgram { surface })?;
        Ok(Self {
            output,
            geometry: Arc::new(geometry_reuse::HitGeometry {
                touch_order: touch.order,
                emission_rows: (emission_rows.len() == regions.len()).then_some(emission_rows),
                regions: regions.into_boxed_slice(),
                touch_regions: touch_regions.into_boxed_slice(),
            }),
        })
    }

    /// Returns the exact semantic output to which every region belongs.
    #[must_use]
    pub const fn output(&self) -> SurfacePresentationOutputTicket {
        self.output
    }

    /// Returns all entries in canonical semantic-ID order.
    ///
    /// This order is serialization and diff order only. Receiver precedence is
    /// carried by [`PresentationHitRegion::stack`].
    #[must_use]
    pub fn regions(&self) -> &[PresentationHitRegion] {
        &self.geometry.regions
    }

    /// Returns one exact region by stable semantic identity.
    #[must_use]
    pub fn region(&self, id: PresentationHitRegionId) -> Option<&PresentationHitRegion> {
        self.region_index(id)
            .map(|index| &self.geometry.regions[index])
    }

    fn region_index(&self, id: PresentationHitRegionId) -> Option<usize> {
        self.geometry
            .regions
            .binary_search_by(|region| {
                #[cfg(test)]
                crate::drop_resolver::structural_work::record_presentation_hit_lookup_comparison();
                region.id.cmp(&id)
            })
            .ok()
    }

    pub(crate) fn region_for_kind(
        &self,
        kind: PresentationHitRegionKind,
    ) -> Option<&PresentationHitRegion> {
        self.region(PresentationHitRegionId::new(self.output.surface(), kind))
    }

    pub(crate) fn region_index_for_kind(&self, kind: PresentationHitRegionKind) -> Option<usize> {
        self.region_index(PresentationHitRegionId::new(self.output.surface(), kind))
    }

    #[allow(
        dead_code,
        reason = "used by the pointer receiver receipt reducer added in the next protocol slice"
    )]
    pub(crate) fn resolve_exclusive(
        &self,
        lane: PresentationPointerLane,
        point: crate::geometry::LogicalPoint,
    ) -> Result<Option<&PresentationHitRegion>, PresentationHitResolutionError> {
        self.resolve_exclusive_with_suppression(lane, point, DropSourceSuppression::default())
    }

    pub(crate) fn resolve_exclusive_with_suppression(
        &self,
        lane: PresentationPointerLane,
        point: crate::geometry::LogicalPoint,
        suppression: DropSourceSuppression,
    ) -> Result<Option<&PresentationHitRegion>, PresentationHitResolutionError> {
        self.resolve_exclusive_for_device(lane, point, suppression, PointerDeviceKind::Unknown)
    }

    pub(crate) fn region_contains_for_device(
        &self,
        id: PresentationHitRegionId,
        point: crate::geometry::LogicalPoint,
        device: PointerDeviceKind,
    ) -> bool {
        let Some(index) = self.region_index(id) else {
            return false;
        };
        self.region_contains(index, &self.geometry.regions[index], point, device)
    }

    fn region_contains(
        &self,
        index: usize,
        region: &PresentationHitRegion,
        point: crate::geometry::LogicalPoint,
        device: PointerDeviceKind,
    ) -> bool {
        if device == PointerDeviceKind::Touch && touch::replaces(region.id().kind()) {
            // An empty replacement stays empty; it never falls back to mouse geometry.
            let first = self
                .geometry
                .touch_regions
                .partition_point(|(row, _)| *row < index);
            self.geometry.touch_regions[first..]
                .iter()
                .take_while(|(row, _)| *row == index)
                .any(|(_, rect)| rect.contains(point))
        } else {
            region.hit().contains(point)
        }
    }

    pub(crate) fn resolve_exclusive_for_device(
        &self,
        lane: PresentationPointerLane,
        point: crate::geometry::LogicalPoint,
        suppression: DropSourceSuppression,
        device: PointerDeviceKind,
    ) -> Result<Option<&PresentationHitRegion>, PresentationHitResolutionError> {
        resolve_exclusive_matching(
            &self.geometry.regions,
            lane,
            point,
            suppression,
            |index, region| self.region_contains(index, region, point, device),
        )
    }
}

fn emit_regions(
    plan: &PresentationPlan,
    regions: &mut impl FnMut(PresentationHitRegion),
) -> Result<(), SceneBuildError> {
    let surface = plan.surface();
    plan.visit_surface_program(|step| {
        match step {
            ResolvedSurfaceProgramStep::Operation { operation, .. } => {
                #[cfg(feature = "motion")]
                push_ranked_operation_regions(plan, regions, surface, operation);
                #[cfg(not(feature = "motion"))]
                push_operation_regions(regions, surface, operation);
            }
            ResolvedSurfaceProgramStep::PopupBackdrop(backdrop) => {
                push_popup_backdrop_region(regions, surface, backdrop);
            }
            ResolvedSurfaceProgramStep::PopupMenu(menu) => {
                push_popup_menu_regions(regions, surface, menu);
            }
            ResolvedSurfaceProgramStep::DropTarget(target) => {
                push_drop_target_region(regions, surface, target);
            }
            ResolvedSurfaceProgramStep::DropGuide(cluster) => {
                push_guide_regions(regions, surface, cluster);
            }
            ResolvedSurfaceProgramStep::RootBegin(_)
            | ResolvedSurfaceProgramStep::RootEnd(_)
            | ResolvedSurfaceProgramStep::DragPreview
            | ResolvedSurfaceProgramStep::ContainedTransformPreview
            | ResolvedSurfaceProgramStep::DragDecoration
            | ResolvedSurfaceProgramStep::InteractionFeedback
            | ResolvedSurfaceProgramStep::SurfaceEnd => {}
        }
        Ok::<(), Infallible>(())
    })
    .map_err(|_| SceneBuildError::InvalidSurfaceProgram { surface })
}

#[cfg(feature = "motion")]
fn push_ranked_operation_regions(
    plan: &PresentationPlan,
    regions: &mut impl FnMut(PresentationHitRegion),
    surface: SurfaceId,
    operation: ResolvedSurfaceProgramOperation<'_>,
) {
    let pane_rank = match operation {
        ResolvedSurfaceProgramOperation::Pane(pane) => {
            (false, plan.pane_rank(pane.id().root, pane.id().tabs))
        }
        ResolvedSurfaceProgramOperation::Tab(tab) => {
            (false, plan.pane_rank(tab.id().root, tab.id().tabs))
        }
        ResolvedSurfaceProgramOperation::TabBar(bar) => {
            (false, plan.pane_rank(bar.id().root, bar.id().tabs))
        }
        ResolvedSurfaceProgramOperation::TabStripControl(control) => {
            let bar = control.id().bar();
            (false, plan.pane_rank(bar.root, bar.tabs))
        }
        ResolvedSurfaceProgramOperation::PresentationMenuAnchor(anchor) => match anchor.host() {
            PresentationMenuAnchorHost::TabBar(bar) => (false, plan.pane_rank(bar.root, bar.tabs)),
            PresentationMenuAnchorHost::ContainedTitle(_) => {
                plan.root_control_pane_rank(anchor.root())
            }
        },
        ResolvedSurfaceProgramOperation::Splitter(splitter) => {
            if !plan.partition_available(splitter.id().root) {
                return;
            }
            plan.root_control_pane_rank(splitter.id().root)
        }
        ResolvedSurfaceProgramOperation::SplitterJunction(junction) => {
            if !plan.partition_available(junction.id().root()) {
                return;
            }
            plan.root_control_pane_rank(junction.id().root())
        }
        ResolvedSurfaceProgramOperation::ContainedControls(contained) => {
            plan.root_control_pane_rank(contained.root())
        }
    };
    push_operation_regions(
        &mut |mut region| {
            region.stack.pane_rank = if matches!(
                region.id().kind(),
                PresentationHitRegionKind::ContainedFrameBlocker(_)
            ) {
                (false, 0)
            } else {
                pane_rank
            };
            regions(region);
        },
        surface,
        operation,
    );
}

// Touch rows arrive in input-region order. Compact both inventories together so
// empty mouse geometry can keep its touch fragments without an identity join.
fn compact_region_rows(
    regions: &mut Vec<PresentationHitRegion>,
    touch_regions: &mut [(usize, LogicalRect)],
) -> Result<(), ()> {
    let mut next_fragment = 0;
    let mut retained = 0;
    for original in 0..regions.len() {
        if touch_regions
            .get(next_fragment)
            .is_some_and(|(row, _)| *row < original)
        {
            return Err(());
        }
        let start = next_fragment;
        while let Some((row, _)) = touch_regions.get_mut(next_fragment) {
            if *row != original {
                break;
            }
            *row = retained;
            next_fragment += 1;
        }
        let bounds = regions[original].hit().rect();
        if (bounds.width() > 0.0 && bounds.height() > 0.0) || next_fragment > start {
            regions[retained] = regions[original];
            retained += 1;
        }
    }
    if next_fragment != touch_regions.len() {
        return Err(());
    }
    regions.truncate(retained);
    Ok(())
}

fn push_operation_regions(
    regions: &mut impl FnMut(PresentationHitRegion),
    surface: SurfaceId,
    operation: ResolvedSurfaceProgramOperation<'_>,
) {
    match operation {
        ResolvedSurfaceProgramOperation::Pane(pane) => {
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::PaneBody(pane.id()),
                    ),
                    HitRegion::new(pane.content_bounds()),
                    PresentationPointerLanes::CLICK_AND_DRAG,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        pane.layer(),
                        PresentationHitPrecedence::PaneBody,
                    ),
                    PresentationHitBehavior::FallbackBlocker,
                ),
            );
        }
        ResolvedSurfaceProgramOperation::Tab(tab) => {
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::TabBody(*tab.id()),
                    ),
                    tab.drag_hit(),
                    PresentationPointerLanes::CLICK_AND_DRAG,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        tab.layer(),
                        PresentationHitPrecedence::TabBody,
                    ),
                    PresentationHitBehavior::ExclusiveReceiver,
                ),
            );
            if let Some(close) = tab.close_bounds() {
                push_region_if_hittable(
                    regions,
                    PresentationHitRegion::new(
                        PresentationHitRegionId::new(
                            surface,
                            PresentationHitRegionKind::TabClose(*tab.id()),
                        ),
                        HitRegion::new(close),
                        PresentationPointerLanes::CLICK,
                        PresentationHitStackKey::new(
                            PresentationPlane::Workspace,
                            tab.layer(),
                            PresentationHitPrecedence::TabClose,
                        ),
                        PresentationHitBehavior::ExclusiveReceiver,
                    ),
                );
            }
        }
        ResolvedSurfaceProgramOperation::TabBar(bar) => {
            if let Some(group) = bar.group_drag() {
                for region in group.regions() {
                    push_region_if_hittable(
                        regions,
                        PresentationHitRegion::new(
                            PresentationHitRegionId::new(
                                surface,
                                PresentationHitRegionKind::TabGroupDrag {
                                    bar: *bar.id(),
                                    region: region.kind(),
                                },
                            ),
                            region.hit(),
                            PresentationPointerLanes::DRAG,
                            PresentationHitStackKey::new(
                                PresentationPlane::Workspace,
                                bar.layer(),
                                PresentationHitPrecedence::TabGroupDrag,
                            ),
                            PresentationHitBehavior::ExclusiveReceiver,
                        ),
                    );
                }
            }
            if bar.interaction() == crate::policy::TabBarInteraction::Enabled
                && bar.maximum_scroll_offset() > 0.0
            {
                push_region_if_hittable(
                    regions,
                    PresentationHitRegion::new(
                        PresentationHitRegionId::new(
                            surface,
                            PresentationHitRegionKind::TabStripScroll(*bar.id()),
                        ),
                        HitRegion::new(bar.viewport()),
                        PresentationPointerLanes::SCROLL,
                        PresentationHitStackKey::new(
                            PresentationPlane::Workspace,
                            bar.layer(),
                            PresentationHitPrecedence::TabStripScroll,
                        ),
                        PresentationHitBehavior::ExclusiveReceiver,
                    ),
                );
            }
        }
        ResolvedSurfaceProgramOperation::TabStripControl(control) => {
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::TabStripControl(control.id()),
                    ),
                    control.hit(),
                    // Overlay controls also occlude the tab's drag lane. The
                    // semantic reducer still treats this as a click action.
                    PresentationPointerLanes::CLICK_AND_DRAG,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        control.layer(),
                        PresentationHitPrecedence::TabStripControl,
                    ),
                    PresentationHitBehavior::ExclusiveReceiver,
                ),
            );
        }
        ResolvedSurfaceProgramOperation::Splitter(splitter) if splitter.operable() => {
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::SplitterHandle(*splitter.id()),
                    ),
                    splitter.hit(),
                    PresentationPointerLanes::DRAG,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        splitter.layer(),
                        PresentationHitPrecedence::SplitterHandle,
                    ),
                    PresentationHitBehavior::ExclusiveReceiver,
                )
                .with_anchor(splitter.draw_bounds()),
            );
        }
        ResolvedSurfaceProgramOperation::SplitterJunction(junction) if junction.operable() => {
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::SplitterJunction(junction.id()),
                    ),
                    junction.hit(),
                    PresentationPointerLanes::DRAG,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        junction.layer(),
                        PresentationHitPrecedence::SplitterJunction,
                    ),
                    PresentationHitBehavior::ExclusiveReceiver,
                )
                .with_anchor(junction.hit().rect()),
            );
        }
        ResolvedSurfaceProgramOperation::ContainedControls(contained) => {
            let floating = contained.floating();
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::ContainedFrameBlocker(floating),
                    ),
                    HitRegion::new(contained.outer_bounds()),
                    PresentationPointerLanes::ALL,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        contained.layer(),
                        PresentationHitPrecedence::FallbackBlocker,
                    ),
                    PresentationHitBehavior::FallbackBlocker,
                ),
            );
            push_region_if_hittable(
                regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::ContainedTitle(floating),
                    ),
                    contained.title_drag_hit(),
                    PresentationPointerLanes::DRAG,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        contained.layer(),
                        PresentationHitPrecedence::ContainedTitle,
                    ),
                    PresentationHitBehavior::ExclusiveReceiver,
                ),
            );
            if let Some(close) = contained.close_bounds() {
                push_region_if_hittable(
                    regions,
                    PresentationHitRegion::new(
                        PresentationHitRegionId::new(
                            surface,
                            PresentationHitRegionKind::ContainedClose(floating),
                        ),
                        HitRegion::new(close),
                        PresentationPointerLanes::CLICK,
                        PresentationHitStackKey::new(
                            PresentationPlane::Workspace,
                            contained.layer(),
                            PresentationHitPrecedence::ContainedClose,
                        ),
                        PresentationHitBehavior::ExclusiveReceiver,
                    ),
                );
            }
            if contained.transform_operable() {
                for resize in contained.resize() {
                    push_region_if_hittable(
                        regions,
                        PresentationHitRegion::new(
                            PresentationHitRegionId::new(
                                surface,
                                PresentationHitRegionKind::ContainedResize {
                                    floating,
                                    direction: resize.direction(),
                                },
                            ),
                            resize.hit(),
                            PresentationPointerLanes::DRAG,
                            PresentationHitStackKey::new(
                                PresentationPlane::Workspace,
                                contained.layer(),
                                PresentationHitPrecedence::ContainedResize,
                            ),
                            PresentationHitBehavior::ExclusiveReceiver,
                        ),
                    );
                }
            }
        }
        ResolvedSurfaceProgramOperation::Splitter(_)
        | ResolvedSurfaceProgramOperation::SplitterJunction(_)
        | ResolvedSurfaceProgramOperation::PresentationMenuAnchor(_) => {}
    }
}

fn push_popup_backdrop_region(
    regions: &mut impl FnMut(PresentationHitRegion),
    surface: SurfaceId,
    backdrop: &TabListMenuBackdropRecord,
) {
    let session = backdrop.session();
    push_region_if_hittable(
        regions,
        PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::TabListMenuBackdrop(session),
            ),
            HitRegion::new(backdrop.bounds()),
            PresentationPointerLanes::ALL,
            PresentationHitStackKey::popup(
                session,
                PopupHitRole::Backdrop,
                PresentationHitPrecedence::FallbackBlocker,
            ),
            PresentationHitBehavior::ExclusiveReceiver,
        ),
    );
}

fn push_popup_menu_regions(
    regions: &mut impl FnMut(PresentationHitRegion),
    surface: SurfaceId,
    menu: &TabListMenuRecord,
) {
    let session = menu.session();
    push_region_if_hittable(
        regions,
        PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::TabListMenuBlocker(session),
            ),
            HitRegion::new(menu.bounds()),
            PresentationPointerLanes::ALL,
            PresentationHitStackKey::popup(
                session,
                PopupHitRole::FrameBlocker,
                PresentationHitPrecedence::FallbackBlocker,
            ),
            PresentationHitBehavior::FallbackBlocker,
        ),
    );
    push_region_if_hittable(
        regions,
        PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::TabListMenuScroll(session),
            ),
            HitRegion::new(menu.viewport()),
            PresentationPointerLanes::SCROLL,
            PresentationHitStackKey::popup(
                session,
                PopupHitRole::Scroll,
                PresentationHitPrecedence::PopupScroll,
            ),
            PresentationHitBehavior::ExclusiveReceiver,
        ),
    );
    for row in menu.rows() {
        let Some(hit) = row.hit() else {
            continue;
        };
        push_region_if_hittable(
            regions,
            PresentationHitRegion::new(
                PresentationHitRegionId::new(
                    surface,
                    PresentationHitRegionKind::TabListMenuRow {
                        menu: session,
                        tab: row.tab(),
                    },
                ),
                hit,
                PresentationPointerLanes::CLICK,
                PresentationHitStackKey::popup(
                    session,
                    PopupHitRole::Row,
                    PresentationHitPrecedence::PopupRow,
                ),
                PresentationHitBehavior::ExclusiveReceiver,
            ),
        );
    }
}

fn push_guide_regions(
    regions: &mut impl FnMut(PresentationHitRegion),
    surface: SurfaceId,
    cluster: &DropGuideClusterRecord,
) {
    push_region_if_hittable(
        regions,
        PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::DropGuideActivation(cluster.id()),
            ),
            cluster.activation(),
            PresentationPointerLanes::HOVER_DROP,
            PresentationHitStackKey::new(
                PresentationPlane::Workspace,
                cluster.layer(),
                PresentationHitPrecedence::PassiveGuideActivation,
            ),
            PresentationHitBehavior::PassiveAffordance,
        ),
    );
    let precedence = match cluster.id().scope {
        DropGuideScope::Inner(_) => PresentationHitPrecedence::InnerGuideTarget,
        DropGuideScope::Outer => PresentationHitPrecedence::OuterGuideTarget,
    };
    for (_, target) in cluster.targets() {
        push_region_if_hittable(
            regions,
            PresentationHitRegion::new(
                PresentationHitRegionId::new(
                    surface,
                    PresentationHitRegionKind::DropTarget(target.id()),
                ),
                target.hit(),
                PresentationPointerLanes::HOVER_DROP,
                PresentationHitStackKey::new(
                    PresentationPlane::Workspace,
                    cluster.layer(),
                    precedence,
                ),
                PresentationHitBehavior::ExclusiveReceiver,
            ),
        );
    }
}

#[cfg(test)]
fn resolve_exclusive_regions(
    regions: &[PresentationHitRegion],
    lane: PresentationPointerLane,
    point: crate::geometry::LogicalPoint,
    suppression: DropSourceSuppression,
) -> Result<Option<&PresentationHitRegion>, PresentationHitResolutionError> {
    resolve_exclusive_matching(regions, lane, point, suppression, |_, region| {
        region.hit().contains(point)
    })
}

fn resolve_exclusive_matching(
    regions: &[PresentationHitRegion],
    lane: PresentationPointerLane,
    point: crate::geometry::LogicalPoint,
    suppression: DropSourceSuppression,
    contains: impl Fn(usize, &PresentationHitRegion) -> bool,
) -> Result<Option<&PresentationHitRegion>, PresentationHitResolutionError> {
    let mut winner: Option<&PresentationHitRegion> = None;
    let mut ambiguity = None;
    for (index, candidate) in regions.iter().enumerate() {
        if candidate.is_passive()
            || suppression.excludes(candidate.id())
            || !candidate.lanes().contains(lane)
            || !contains(index, candidate)
        {
            continue;
        }
        match winner {
            None => winner = Some(candidate),
            Some(current) => match candidate.stack().cmp(&current.stack()) {
                Ordering::Greater => {
                    winner = Some(candidate);
                    ambiguity = None;
                }
                Ordering::Less => {}
                Ordering::Equal if candidate.id() != current.id() => {
                    match splitter_tie_break(candidate, current, point) {
                        Ordering::Greater => {
                            winner = Some(candidate);
                            ambiguity = None;
                        }
                        Ordering::Less => {}
                        Ordering::Equal => {
                            ambiguity = Some(PresentationHitResolutionError::Ambiguous {
                                lane,
                                first: current.id(),
                                second: candidate.id(),
                            });
                        }
                    }
                }
                Ordering::Equal => {}
            },
        }
    }
    match ambiguity {
        Some(error) => Err(error),
        None => Ok(winner),
    }
}

fn splitter_tie_break(
    candidate: &PresentationHitRegion,
    current: &PresentationHitRegion,
    point: crate::geometry::LogicalPoint,
) -> Ordering {
    let candidate_kind = candidate.id().kind();
    let current_kind = current.id().kind();
    let splitter_pair = matches!(
        (candidate_kind, current_kind),
        (
            PresentationHitRegionKind::SplitterHandle(_),
            PresentationHitRegionKind::SplitterHandle(_)
        ) | (
            PresentationHitRegionKind::SplitterJunction(_),
            PresentationHitRegionKind::SplitterJunction(_)
        )
    );
    if !splitter_pair {
        return Ordering::Equal;
    }

    let candidate_distance = splitter_anchor_distance(candidate, point);
    let current_distance = splitter_anchor_distance(current, point);
    current_distance
        .total_cmp(&candidate_distance)
        .then_with(|| current.id().cmp(&candidate.id()))
}

fn splitter_anchor_distance(
    region: &PresentationHitRegion,
    point: crate::geometry::LogicalPoint,
) -> f64 {
    let anchor = region.anchor;
    match region.id().kind() {
        PresentationHitRegionKind::SplitterJunction(_) => {
            let dx = midpoint(anchor.x(), anchor.max().x()) - point.x();
            let dy = midpoint(anchor.y(), anchor.max().y()) - point.y();
            dx * dx + dy * dy
        }
        PresentationHitRegionKind::SplitterHandle(_) => {
            let dx = if point.x() < anchor.x() {
                anchor.x() - point.x()
            } else if point.x() > anchor.max().x() {
                point.x() - anchor.max().x()
            } else {
                0.0
            };
            let dy = if point.y() < anchor.y() {
                anchor.y() - point.y()
            } else if point.y() > anchor.max().y() {
                point.y() - anchor.max().y()
            } else {
                0.0
            };
            dx * dx + dy * dy
        }
        _ => 0.0,
    }
}

fn midpoint(minimum: f64, maximum: f64) -> f64 {
    minimum * 0.5 + maximum * 0.5
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{LogicalPoint, LogicalRect};
    use crate::graph::{Node, Workspace};
    use crate::ids::ItemId;
    use crate::scene::{TabBarSceneId, TabSceneId};
    use crate::tab_strip::{TabListMenuSessionId, TabStripStateKey};

    fn tabs_node() -> crate::ids::NodeId {
        let mut builder = Workspace::builder();
        builder.insert_node(Node::tabs([]))
    }

    fn region(
        id: PresentationHitRegionId,
        layer: SceneLayerKey,
        precedence: PresentationHitPrecedence,
    ) -> PresentationHitRegion {
        PresentationHitRegion::new(
            id,
            HitRegion::new(LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rect is valid")),
            PresentationPointerLanes::HOVER_DROP,
            PresentationHitStackKey::new(PresentationPlane::Workspace, layer, precedence),
            PresentationHitBehavior::FallbackBlocker,
        )
    }

    #[test]
    fn hover_drop_blocker_is_frontmost_except_for_the_complete_source_root() {
        let surface = SurfaceId::new(1);
        let source_root = RootId::new(10);
        let target_root = RootId::new(20);
        let floating = FloatingPresentationId::new(30);
        let target_tabs = tabs_node();
        let blocker = region(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::ContainedFrameBlocker(floating),
            ),
            SceneLayerKey::new(2),
            PresentationHitPrecedence::FallbackBlocker,
        );
        let lower_target = region(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::DropTarget(DropTargetId::Center {
                    surface,
                    root: target_root,
                    tabs: target_tabs,
                }),
            ),
            SceneLayerKey::new(1),
            PresentationHitPrecedence::StandaloneCenter,
        );
        let point = LogicalPoint::new(50.0, 50.0).expect("test point is valid");
        let regions = [lower_target, blocker];

        assert_eq!(
            resolve_exclusive_regions(
                &regions,
                PresentationPointerLane::HoverDrop,
                point,
                DropSourceSuppression::default(),
            )
            .expect("the stack is unambiguous")
            .map(|region| region.id()),
            Some(blocker.id()),
        );
        assert_eq!(
            resolve_exclusive_regions(
                &regions,
                PresentationPointerLane::HoverDrop,
                point,
                DropSourceSuppression::complete_root(source_root, Some(floating)),
            )
            .expect("source suppression keeps one lower target")
            .map(|region| region.id()),
            Some(lower_target.id()),
        );
    }

    #[test]
    fn complete_source_suppression_excludes_its_targets_and_guide_activation() {
        let surface = SurfaceId::new(1);
        let source_root = RootId::new(10);
        let floating = FloatingPresentationId::new(30);
        let source_tabs = tabs_node();
        let suppression = DropSourceSuppression::complete_root(source_root, Some(floating));

        assert!(suppression.excludes(PresentationHitRegionId::new(
            surface,
            PresentationHitRegionKind::DropTarget(DropTargetId::Center {
                surface,
                root: source_root,
                tabs: source_tabs,
            }),
        )));
        assert!(suppression.excludes(PresentationHitRegionId::new(
            surface,
            PresentationHitRegionKind::DropGuideActivation(DropGuideClusterId::outer(
                surface,
                source_root,
            )),
        )));
    }

    #[test]
    fn disappearing_source_leaf_keeps_its_roots_outer_hover_receivers() {
        use crate::command::{Edge, MovePayload};
        use crate::graph::{Axis, RootRecord, SurfacePresentation};

        let surface = SurfaceId::new(1);
        let root = RootId::new(10);
        let item = ItemId::new(1);
        let mut builder = Workspace::builder();
        let source_tabs = builder.insert_node(Node::tabs([item]));
        let remaining_tabs = builder.insert_node(Node::tabs([ItemId::new(2)]));
        let split = builder.insert_node(
            Node::equal_split(Axis::Horizontal, [source_tabs, remaining_tabs])
                .expect("source and remaining leaf form a valid split"),
        );
        builder.set_root(root, RootRecord::new(split));
        builder.set_surface(surface, SurfacePresentation::with_main(root));
        let workspace = builder.build().expect("self-docking workspace is valid");
        let payload = MovePayload::Item(
            workspace
                .capture_item_source(root, source_tabs, item)
                .expect("source tab is current"),
        );
        let suppression = DropSourceSuppression::from_source(&workspace, &payload, None);
        let outer = region(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::DropTarget(DropTargetId::OuterEdge {
                    surface,
                    root,
                    node: split,
                    edge: Edge::Right,
                }),
            ),
            SceneLayerKey::surface_base(),
            PresentationHitPrecedence::StandaloneOuterEdge,
        );
        let point = LogicalPoint::new(50.0, 50.0).expect("test point is valid");
        for kind in [
            PresentationHitRegionKind::DropTarget(DropTargetId::Center {
                surface,
                root,
                tabs: source_tabs,
            }),
            PresentationHitRegionKind::DropTarget(DropTargetId::InnerEdge {
                surface,
                root,
                node: source_tabs,
                edge: Edge::Right,
            }),
            PresentationHitRegionKind::DropTarget(DropTargetId::TabGap {
                surface,
                root,
                tabs: source_tabs,
                index: 0,
            }),
            PresentationHitRegionKind::DropGuideActivation(DropGuideClusterId::inner(
                surface,
                root,
                source_tabs,
            )),
        ] {
            let source = region(
                PresentationHitRegionId::new(surface, kind),
                SceneLayerKey::surface_base(),
                PresentationHitPrecedence::StandaloneCenter,
            );
            let regions = [outer, source];
            assert_eq!(
                resolve_exclusive_regions(
                    &regions,
                    PresentationPointerLane::HoverDrop,
                    point,
                    suppression,
                )
                .expect("removing source hits exposes the outer receiver")
                .map(|winner| winner.id()),
                Some(outer.id()),
                "removed source geometry must not mask a valid root receiver: {kind:?}",
            );
        }
        assert!(!suppression.excludes(PresentationHitRegionId::new(
            surface,
            PresentationHitRegionKind::DropGuideActivation(DropGuideClusterId::outer(
                surface, root
            )),
        )));
    }

    #[test]
    fn popup_row_beats_its_blocker_and_popup_blocker_beats_workspace() {
        let surface = SurfaceId::new(1);
        let tabs = tabs_node();
        let bar = TabBarSceneId {
            root: RootId::new(10),
            tabs,
        };
        let session = TabListMenuSessionId::new_for_test(TabStripStateKey::new(surface, bar), 7);
        let point = LogicalPoint::new(50.0, 50.0).expect("test point is valid");
        let workspace = PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::ContainedFrameBlocker(FloatingPresentationId::new(3)),
            ),
            HitRegion::new(LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rect is valid")),
            PresentationPointerLanes::CLICK,
            PresentationHitStackKey::new(
                PresentationPlane::Workspace,
                SceneLayerKey::new(100),
                PresentationHitPrecedence::FallbackBlocker,
            ),
            PresentationHitBehavior::FallbackBlocker,
        );
        let blocker = PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::TabListMenuBlocker(session),
            ),
            HitRegion::new(LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rect is valid")),
            PresentationPointerLanes::ALL,
            PresentationHitStackKey::new(
                PresentationPlane::Popup(session),
                SceneLayerKey::surface_base(),
                PresentationHitPrecedence::FallbackBlocker,
            ),
            PresentationHitBehavior::FallbackBlocker,
        );
        let tab = TabSceneId {
            root: bar.root,
            tabs: bar.tabs,
            item: ItemId::new(20),
        };
        let row = PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::TabListMenuRow { menu: session, tab },
            ),
            HitRegion::new(LogicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test rect is valid")),
            PresentationPointerLanes::CLICK,
            PresentationHitStackKey::new(
                PresentationPlane::Popup(session),
                SceneLayerKey::surface_base(),
                PresentationHitPrecedence::PopupRow,
            ),
            PresentationHitBehavior::ExclusiveReceiver,
        );

        assert_eq!(workspace.stack().plane(), PresentationPlane::Workspace);
        assert_eq!(blocker.stack().plane(), PresentationPlane::Popup(session));
        assert_eq!(
            resolve_exclusive_regions(
                &[workspace, blocker],
                PresentationPointerLane::Click,
                point,
                DropSourceSuppression::default(),
            )
            .expect("different planes are never ambiguous")
            .map(|region| region.id()),
            Some(blocker.id()),
        );
        assert_eq!(
            resolve_exclusive_regions(
                &[workspace, blocker, row],
                PresentationPointerLane::Click,
                point,
                DropSourceSuppression::default(),
            )
            .expect("the popup row has unique precedence within its plane")
            .map(|region| region.id()),
            Some(row.id()),
        );
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "used by the pointer receiver receipt reducer added in the next protocol slice"
)]
pub(crate) enum PresentationHitResolutionError {
    Ambiguous {
        lane: PresentationPointerLane,
        first: PresentationHitRegionId,
        second: PresentationHitRegionId,
    },
}

fn push_region_if_hittable(
    regions: &mut impl FnMut(PresentationHitRegion),
    region: PresentationHitRegion,
) {
    let rect = region.hit().rect();
    // A clipped base band may be empty while its touch footprint is visible.
    // Compilation removes the identity if neither profile can hit it.
    if (rect.width() > 0.0 && rect.height() > 0.0) || touch::replaces(region.id().kind()) {
        regions(region);
    }
}

fn push_drop_target_region(
    regions: &mut impl FnMut(PresentationHitRegion),
    surface: SurfaceId,
    target: &crate::drop_target::DropTargetRecord,
) {
    let precedence = match target.id().kind() {
        DropTargetKind::TabGap => PresentationHitPrecedence::StandaloneTabGap,
        DropTargetKind::Center => PresentationHitPrecedence::StandaloneCenter,
        DropTargetKind::InnerEdge => PresentationHitPrecedence::StandaloneInnerEdge,
        DropTargetKind::OuterEdge => PresentationHitPrecedence::StandaloneOuterEdge,
        DropTargetKind::SurfaceBackground => PresentationHitPrecedence::StandaloneSurfaceBackground,
    };
    push_region_if_hittable(
        regions,
        PresentationHitRegion::new(
            PresentationHitRegionId::new(
                surface,
                PresentationHitRegionKind::DropTarget(target.id()),
            ),
            target.region(),
            PresentationPointerLanes::HOVER_DROP,
            PresentationHitStackKey::new(PresentationPlane::Workspace, target.layer(), precedence),
            PresentationHitBehavior::ExclusiveReceiver,
        ),
    );
}
