//! Opaque framework gesture preparation over one exact paint plan.

use crate::ids::{FloatingPresentationId, ItemId};
use crate::scene::{
    ContainedResizeDirection as CoreContainedResizeDirection, SplitterResizeTarget,
};

use super::super::{
    PreparedSurfaceAction, SurfaceGesturePhase, SurfaceSplitterAdjustment, SurfaceTabNavigation,
};
use super::{DockspaceVisualId, SurfacePaintPlan, VisualIdentity};

/// Product-facing direction of one contained-floating resize handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ContainedResizeDirection {
    /// Top edge.
    North,
    /// Top-right corner.
    NorthEast,
    /// Right edge.
    East,
    /// Bottom-right corner.
    SouthEast,
    /// Bottom edge.
    South,
    /// Bottom-left corner.
    SouthWest,
    /// Left edge.
    West,
    /// Top-left corner.
    NorthWest,
}

impl ContainedResizeDirection {
    pub(super) const fn from_core(direction: CoreContainedResizeDirection) -> Self {
        match direction {
            CoreContainedResizeDirection::North => Self::North,
            CoreContainedResizeDirection::NorthEast => Self::NorthEast,
            CoreContainedResizeDirection::East => Self::East,
            CoreContainedResizeDirection::SouthEast => Self::SouthEast,
            CoreContainedResizeDirection::South => Self::South,
            CoreContainedResizeDirection::SouthWest => Self::SouthWest,
            CoreContainedResizeDirection::West => Self::West,
            CoreContainedResizeDirection::NorthWest => Self::NorthWest,
        }
    }

    const fn into_core(self) -> CoreContainedResizeDirection {
        match self {
            Self::North => CoreContainedResizeDirection::North,
            Self::NorthEast => CoreContainedResizeDirection::NorthEast,
            Self::East => CoreContainedResizeDirection::East,
            Self::SouthEast => CoreContainedResizeDirection::SouthEast,
            Self::South => CoreContainedResizeDirection::South,
            Self::SouthWest => CoreContainedResizeDirection::SouthWest,
            Self::West => CoreContainedResizeDirection::West,
            Self::NorthWest => CoreContainedResizeDirection::NorthWest,
        }
    }
}

impl SurfacePaintPlan<'_> {
    /// Prepares navigation within the exact tab strip which owns `item`.
    #[must_use]
    pub fn prepare_tab_navigation(
        self,
        item: ItemId,
        navigation: SurfaceTabNavigation,
    ) -> Option<PreparedSurfaceAction> {
        let current = self
            .plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)?;
        let selected = self
            .plan
            .tab_records()
            .iter()
            .find(|record| {
                record.selected()
                    && record.id().root == current.id().root
                    && record.id().tabs == current.id().tabs
            })
            .map(|record| record.id().item);
        let destination = crate::tab_strip::tab_navigation_destination(
            self.plan,
            *current.id(),
            selected,
            navigation.into_core(),
        )?;
        (destination != *current.id()).then(|| {
            PreparedSurfaceAction::select_tab(
                self.authority_domain,
                self.version,
                self.scene,
                destination,
            )
        })
    }

    /// Prepares one keyboard or accessibility adjustment for an exact splitter.
    #[must_use]
    pub fn prepare_splitter_adjustment(
        self,
        splitter: DockspaceVisualId,
        adjustment: SurfaceSplitterAdjustment,
    ) -> Option<PreparedSurfaceAction> {
        let VisualIdentity::Splitter(id) = splitter.0 else {
            return None;
        };
        self.plan
            .splitter_record(id)
            .is_some_and(crate::scene::SplitterRecord::operable)
            .then(|| {
                PreparedSurfaceAction::adjust_splitter(
                    self.authority_domain,
                    self.version,
                    self.scene,
                    id,
                    self.splitter_keyboard_step * adjustment.direction(),
                )
            })
    }

    /// Prepares Escape cancellation only for this surface's active local gesture.
    #[must_use]
    pub fn prepare_escape_cancel(self) -> Option<PreparedSurfaceAction> {
        self.escape_available.then(|| {
            PreparedSurfaceAction::cancel_with_escape(
                self.authority_domain,
                self.version,
                self.surface,
            )
        })
    }

    /// Prepares an exact acknowledgement after the renderer painted this plan's
    /// current docking preview.
    #[must_use]
    pub fn prepare_drag_preview_painted(self) -> Option<PreparedSurfaceAction> {
        let preview = self.drag_preview?;
        Some(PreparedSurfaceAction::acknowledge_preview(
            self.authority_domain,
            self.version,
            self.surface,
            preview.acknowledgement(),
        ))
    }

    /// Prepares an exact acknowledgement after the renderer painted this plan's
    /// current contained-floating transform preview.
    #[must_use]
    pub fn prepare_contained_transform_preview_painted(self) -> Option<PreparedSurfaceAction> {
        let preview = self.contained_transform_preview?;
        Some(
            PreparedSurfaceAction::acknowledge_contained_transform_preview(
                self.authority_domain,
                self.version,
                self.surface,
                preview.acknowledgement(),
            ),
        )
    }

    /// Prepares one tab drag phase from this exact candidate.
    #[must_use]
    pub fn prepare_tab_gesture(
        self,
        item: ItemId,
        phase: SurfaceGesturePhase,
    ) -> Option<PreparedSurfaceAction> {
        let tab = self
            .plan
            .tab_records()
            .iter()
            .find(|record| record.id().item == item)?;
        Some(PreparedSurfaceAction::local_tab_gesture(
            self.authority_domain,
            self.version,
            self.surface,
            crate::intent::TabGestureSource::Item(*tab.id()),
            tab_gesture_phase(self.scene, phase),
        ))
    }

    /// Prepares one whole-tab-group drag phase from this exact candidate.
    #[must_use]
    pub fn prepare_tab_group_gesture(
        self,
        bar: DockspaceVisualId,
        phase: SurfaceGesturePhase,
    ) -> Option<PreparedSurfaceAction> {
        let VisualIdentity::TabBar(bar) = bar.0 else {
            return None;
        };
        self.plan
            .tab_bar_records()
            .iter()
            .any(|record| *record.id() == bar)
            .then(|| {
                PreparedSurfaceAction::local_tab_gesture(
                    self.authority_domain,
                    self.version,
                    self.surface,
                    crate::intent::TabGestureSource::Group(bar),
                    tab_gesture_phase(self.scene, phase),
                )
            })
    }

    /// Prepares one contained title drag phase from this exact candidate.
    #[must_use]
    pub fn prepare_contained_title_gesture(
        self,
        floating: FloatingPresentationId,
        phase: SurfaceGesturePhase,
    ) -> Option<PreparedSurfaceAction> {
        let contained = self
            .plan
            .contained_records()
            .iter()
            .find(|record| record.floating() == floating)?;
        Some(PreparedSurfaceAction::local_tab_gesture(
            self.authority_domain,
            self.version,
            self.surface,
            crate::intent::TabGestureSource::ContainedTitle {
                root: contained.root(),
                floating,
            },
            tab_gesture_phase(self.scene, phase),
        ))
    }

    /// Prepares one splitter-handle or junction gesture phase.
    #[must_use]
    pub fn prepare_splitter_gesture(
        self,
        splitter: DockspaceVisualId,
        phase: SurfaceGesturePhase,
    ) -> Option<PreparedSurfaceAction> {
        let target = match splitter.0 {
            VisualIdentity::Splitter(id)
                if self
                    .plan
                    .splitter_records()
                    .iter()
                    .any(|record| *record.id() == id && record.operable()) =>
            {
                SplitterResizeTarget::Handle(id)
            }
            VisualIdentity::SplitterJunction(id)
                if self
                    .plan
                    .splitter_junction_records()
                    .iter()
                    .any(|record| record.id() == id) =>
            {
                SplitterResizeTarget::Junction(id)
            }
            _ => return None,
        };
        Some(PreparedSurfaceAction::local_splitter_gesture(
            self.authority_domain,
            self.version,
            self.surface,
            target,
            splitter_gesture_phase(self.scene, phase),
        ))
    }

    /// Prepares one contained-floating resize gesture phase.
    #[must_use]
    pub fn prepare_contained_resize_gesture(
        self,
        floating: FloatingPresentationId,
        direction: ContainedResizeDirection,
        phase: SurfaceGesturePhase,
    ) -> Option<PreparedSurfaceAction> {
        let direction = direction.into_core();
        self.plan.contained_records().iter().find(|record| {
            record.floating() == floating
                && record
                    .resize()
                    .iter()
                    .any(|resize| resize.direction() == direction)
        })?;
        Some(PreparedSurfaceAction::local_contained_gesture(
            self.authority_domain,
            self.version,
            self.surface,
            floating,
            crate::intent::ContainedGestureKind::Resize(direction),
            contained_gesture_phase(self.scene, phase),
        ))
    }
}

const fn tab_gesture_phase(
    scene: crate::scene::SurfaceSceneStamp,
    phase: SurfaceGesturePhase,
) -> crate::engine::LocalTabGesturePhase {
    match phase {
        SurfaceGesturePhase::Begin { initial, current } => {
            crate::engine::LocalTabGesturePhase::Begin {
                scene,
                initial,
                current,
            }
        }
        SurfaceGesturePhase::Move { current } => {
            crate::engine::LocalTabGesturePhase::Move { scene, current }
        }
        SurfaceGesturePhase::Release { current } => {
            crate::engine::LocalTabGesturePhase::Release { scene, current }
        }
        SurfaceGesturePhase::Cancel => crate::engine::LocalTabGesturePhase::Cancel,
    }
}

const fn splitter_gesture_phase(
    scene: crate::scene::SurfaceSceneStamp,
    phase: SurfaceGesturePhase,
) -> crate::engine::LocalSplitterGesturePhase {
    match phase {
        SurfaceGesturePhase::Begin { initial, current } => {
            crate::engine::LocalSplitterGesturePhase::Press {
                scene,
                initial,
                current,
            }
        }
        SurfaceGesturePhase::Move { current } => {
            crate::engine::LocalSplitterGesturePhase::Move { current }
        }
        SurfaceGesturePhase::Release { current } => {
            crate::engine::LocalSplitterGesturePhase::Release { current }
        }
        SurfaceGesturePhase::Cancel => crate::engine::LocalSplitterGesturePhase::Cancel,
    }
}

const fn contained_gesture_phase(
    scene: crate::scene::SurfaceSceneStamp,
    phase: SurfaceGesturePhase,
) -> crate::engine::LocalContainedGesturePhase {
    match phase {
        SurfaceGesturePhase::Begin { initial, current } => {
            crate::engine::LocalContainedGesturePhase::Begin {
                scene,
                initial,
                current,
            }
        }
        SurfaceGesturePhase::Move { current } => {
            crate::engine::LocalContainedGesturePhase::Move { scene, current }
        }
        SurfaceGesturePhase::Release { current } => {
            crate::engine::LocalContainedGesturePhase::Release { scene, current }
        }
        SurfaceGesturePhase::Cancel => crate::engine::LocalContainedGesturePhase::Cancel,
    }
}
