//! Output-bound, renderer-neutral presentation hit inventory.
//!
//! This module indexes geometry already compiled into a
//! [`crate::scene::PresentationPlan`]. It does not project layout, choose drop
//! semantics, or prove that a UI framework actually delivered an event to a
//! docking receiver.

use std::cmp::Ordering;

use crate::drop_guide::DropGuideScope;
use crate::drop_target::{DropTargetKind, SceneLayerKey};
use crate::hit_region::HitRegion;
use crate::ids::{FloatingPresentationId, RootId, SurfaceId};
use crate::presentation_observation::SurfacePresentationOutputTicket;
use crate::scene::{
    ContainedResizeDirection, PaneSceneId, PresentationPlan, SplitterJunctionId, SplitterSceneId,
    TabBarSceneId, TabSceneId,
};
use crate::tab_strip::{TabListMenuSessionId, TabStripControlId};
use crate::{drop_guide::DropGuideClusterId, drop_target::DropTargetId};

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
    /// Begin dragging a complete tabs group.
    TabGroupGrip(TabBarSceneId),
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
            precedence,
        }
    }

    /// Returns the semantic plane compared before structural workspace layers.
    #[must_use]
    pub const fn plane(self) -> PresentationPlane {
        self.plane
    }

    /// Returns semantic popup precedence independent of workspace structure.
    #[must_use]
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
    TabGroupGrip,
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

/// Drag-source records which must not occlude or target their own complete root.
///
/// This is an interaction-time filter over the immutable presentation manifest.
/// It deliberately mirrors complete-root source suppression in the drop resolver:
/// partial item or tab payloads receive no suppression.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[allow(
    dead_code,
    reason = "consumed by the pointer receiver reducer in the next protocol slice"
)]
pub(crate) struct PresentationHitSuppression {
    root: Option<RootId>,
    contained: Option<FloatingPresentationId>,
}

impl PresentationHitSuppression {
    #[allow(
        dead_code,
        reason = "consumed by the pointer receiver reducer in the next protocol slice"
    )]
    pub(crate) const fn complete_root(
        root: RootId,
        contained: Option<FloatingPresentationId>,
    ) -> Self {
        Self {
            root: Some(root),
            contained,
        }
    }

    fn excludes(self, id: PresentationHitRegionId) -> bool {
        match id.kind() {
            PresentationHitRegionKind::ContainedFrameBlocker(floating) => {
                matches!(self.contained, Some(source) if source == floating)
            }
            PresentationHitRegionKind::DropGuideActivation(cluster) => {
                matches!(self.root, Some(source) if source == cluster.root)
            }
            PresentationHitRegionKind::DropTarget(target) => {
                matches!((self.root, drop_target_root(target)), (Some(source), Some(target)) if source == target)
            }
            PresentationHitRegionKind::PaneBody(_)
            | PresentationHitRegionKind::TabBody(_)
            | PresentationHitRegionKind::TabClose(_)
            | PresentationHitRegionKind::TabStripControl(_)
            | PresentationHitRegionKind::TabStripScroll(_)
            | PresentationHitRegionKind::TabListMenuRow { .. }
            | PresentationHitRegionKind::TabListMenuScroll(_)
            | PresentationHitRegionKind::TabListMenuBlocker(_)
            | PresentationHitRegionKind::TabListMenuBackdrop(_)
            | PresentationHitRegionKind::TabGroupGrip(_)
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
    lanes: PresentationPointerLanes,
    stack: PresentationHitStackKey,
    behavior: PresentationHitBehavior,
}

impl PresentationHitRegion {
    const fn new(
        id: PresentationHitRegionId,
        hit: HitRegion,
        lanes: PresentationPointerLanes,
        stack: PresentationHitStackKey,
        behavior: PresentationHitBehavior,
    ) -> Self {
        Self {
            id,
            hit,
            lanes,
            stack,
            behavior,
        }
    }

    /// Returns the stable semantic identity of this region.
    #[must_use]
    pub const fn id(self) -> PresentationHitRegionId {
        self.id
    }

    /// Returns the exact half-open hit geometry.
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
#[derive(Debug, Clone, PartialEq)]
pub struct PresentationHitManifest {
    output: SurfacePresentationOutputTicket,
    regions: Vec<PresentationHitRegion>,
}

impl PresentationHitManifest {
    pub(crate) fn compile(
        output: SurfacePresentationOutputTicket,
        plan: &PresentationPlan,
    ) -> Self {
        debug_assert_eq!(output.surface(), plan.surface());
        let surface = plan.surface();
        let mut regions = Vec::new();

        for pane in plan.pane_records() {
            push_region_if_hittable(
                &mut regions,
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

        for tab in plan.tab_records() {
            push_region_if_hittable(
                &mut regions,
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
                    &mut regions,
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

        for bar in plan.tab_bar_records() {
            if let Some(group) = bar.group_drag() {
                push_region_if_hittable(
                    &mut regions,
                    PresentationHitRegion::new(
                        PresentationHitRegionId::new(
                            surface,
                            PresentationHitRegionKind::TabGroupGrip(*bar.id()),
                        ),
                        group.hit(),
                        PresentationPointerLanes::DRAG,
                        PresentationHitStackKey::new(
                            PresentationPlane::Workspace,
                            bar.layer(),
                            PresentationHitPrecedence::TabGroupGrip,
                        ),
                        PresentationHitBehavior::ExclusiveReceiver,
                    ),
                );
            }
            if bar.interaction() == crate::policy::TabBarInteraction::Enabled
                && bar.maximum_scroll_offset() > 0.0
            {
                push_region_if_hittable(
                    &mut regions,
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

        for control in plan.tab_strip_control_records() {
            push_region_if_hittable(
                &mut regions,
                PresentationHitRegion::new(
                    PresentationHitRegionId::new(
                        surface,
                        PresentationHitRegionKind::TabStripControl(control.id()),
                    ),
                    control.hit(),
                    PresentationPointerLanes::CLICK,
                    PresentationHitStackKey::new(
                        PresentationPlane::Workspace,
                        control.layer(),
                        PresentationHitPrecedence::TabStripControl,
                    ),
                    PresentationHitBehavior::ExclusiveReceiver,
                ),
            );
        }

        for backdrop in plan.tab_list_menu_backdrop_records() {
            let session = backdrop.session();
            push_region_if_hittable(
                &mut regions,
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

        for menu in plan.tab_list_menu_records() {
            let session = menu.session();
            push_region_if_hittable(
                &mut regions,
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
                &mut regions,
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
                    &mut regions,
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

        for splitter in plan
            .splitter_records()
            .iter()
            .filter(|splitter| splitter.operable())
        {
            push_region_if_hittable(
                &mut regions,
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
                ),
            );
        }

        for junction in plan.splitter_junction_records() {
            push_region_if_hittable(
                &mut regions,
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
                ),
            );
        }

        for contained in plan.contained_records() {
            let floating = contained.floating();
            push_region_if_hittable(
                &mut regions,
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
                &mut regions,
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
                    &mut regions,
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
                        &mut regions,
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

        for cluster in plan.drop_guide_clusters() {
            push_region_if_hittable(
                &mut regions,
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
                    &mut regions,
                    PresentationHitRegion::new(
                        PresentationHitRegionId::new(
                            surface,
                            PresentationHitRegionKind::DropTarget(target.id()),
                        ),
                        target.target().region(),
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

        for target in plan.drop_targets() {
            push_drop_target_region(&mut regions, surface, target);
        }
        if let Some(background) = plan.surface_background() {
            push_drop_target_region(&mut regions, surface, background);
        }

        regions.sort_unstable_by_key(|region| region.id());
        debug_assert!(regions.windows(2).all(|pair| pair[0].id() != pair[1].id()));
        Self { output, regions }
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
        &self.regions
    }

    /// Returns one exact region by stable semantic identity.
    #[must_use]
    pub fn region(&self, id: PresentationHitRegionId) -> Option<&PresentationHitRegion> {
        self.regions
            .binary_search_by_key(&id, |region| {
                #[cfg(test)]
                crate::drop_resolver::structural_work::record_presentation_hit_lookup_comparison();
                region.id()
            })
            .ok()
            .map(|index| &self.regions[index])
    }

    pub(crate) fn region_for_kind(
        &self,
        kind: PresentationHitRegionKind,
    ) -> Option<&PresentationHitRegion> {
        self.region(PresentationHitRegionId::new(self.output.surface(), kind))
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
        self.resolve_exclusive_with_suppression(lane, point, PresentationHitSuppression::default())
    }

    pub(crate) fn resolve_exclusive_with_suppression(
        &self,
        lane: PresentationPointerLane,
        point: crate::geometry::LogicalPoint,
        suppression: PresentationHitSuppression,
    ) -> Result<Option<&PresentationHitRegion>, PresentationHitResolutionError> {
        resolve_exclusive_regions(&self.regions, lane, point, suppression)
    }
}

fn resolve_exclusive_regions<'a>(
    regions: &'a [PresentationHitRegion],
    lane: PresentationPointerLane,
    point: crate::geometry::LogicalPoint,
    suppression: PresentationHitSuppression,
) -> Result<Option<&'a PresentationHitRegion>, PresentationHitResolutionError> {
    let mut winner: Option<&PresentationHitRegion> = None;
    for candidate in regions.iter().filter(|candidate| {
        !candidate.is_passive()
            && !suppression.excludes(candidate.id())
            && candidate.lanes().contains(lane)
            && candidate.hit().contains(point)
    }) {
        match winner {
            None => winner = Some(candidate),
            Some(current) => match candidate.stack().cmp(&current.stack()) {
                Ordering::Greater => winner = Some(candidate),
                Ordering::Less => {}
                Ordering::Equal if candidate.id() != current.id() => {
                    return Err(PresentationHitResolutionError::Ambiguous {
                        lane,
                        first: current.id(),
                        second: candidate.id(),
                    });
                }
                Ordering::Equal => {}
            },
        }
    }
    Ok(winner)
}

const fn drop_target_root(target: DropTargetId) -> Option<RootId> {
    match target {
        DropTargetId::TabGap { root, .. }
        | DropTargetId::Center { root, .. }
        | DropTargetId::InnerEdge { root, .. }
        | DropTargetId::OuterEdge { root, .. } => Some(root),
        DropTargetId::SurfaceBackground { .. } => None,
    }
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
                PresentationHitSuppression::default(),
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
                PresentationHitSuppression::complete_root(source_root, Some(floating)),
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
        let suppression = PresentationHitSuppression::complete_root(source_root, Some(floating));

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
                PresentationHitSuppression::default(),
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
                PresentationHitSuppression::default(),
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
    regions: &mut Vec<PresentationHitRegion>,
    region: PresentationHitRegion,
) {
    let rect = region.hit().rect();
    if rect.width() > 0.0 && rect.height() > 0.0 {
        regions.push(region);
    }
}

fn push_drop_target_region(
    regions: &mut Vec<PresentationHitRegion>,
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
