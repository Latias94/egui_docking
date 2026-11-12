//! Private state kernel for direct-manipulation target selection.

use crate::drop_guide::{DropGuideClusterId, DropGuideSlot};
use crate::drop_target::{DropTargetId, SceneLayerKey};
use crate::geometry::LogicalPoint;
use crate::hit_region::HitRegion;
use crate::scene::SurfaceSceneStamp;
use crate::target_field::MagneticTargetCandidate;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DirectTargetProvenance {
    Guide {
        cluster: DropGuideClusterId,
        slot: DropGuideSlot,
    },
    TabGap,
    SurfaceBackground,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DirectTargetCandidate {
    target: DropTargetId,
    provenance: DirectTargetProvenance,
    hit: HitRegion,
}

impl DirectTargetCandidate {
    pub(crate) const fn new(
        target: DropTargetId,
        provenance: DirectTargetProvenance,
        hit: HitRegion,
    ) -> Self {
        Self {
            target,
            provenance,
            hit,
        }
    }

    pub(crate) const fn target(self) -> DropTargetId {
        self.target
    }

    fn retains_at(self, point: LogicalPoint, margin: f64) -> bool {
        let rect = self.hit.rect();
        let outside_x = if point.x() < rect.x() {
            rect.x() - point.x()
        } else if point.x() > rect.max().x() {
            point.x() - rect.max().x()
        } else {
            0.0
        };
        let outside_y = if point.y() < rect.y() {
            rect.y() - point.y()
        } else if point.y() > rect.max().y() {
            point.y() - rect.max().y()
        } else {
            0.0
        };
        outside_x.hypot(outside_y) <= margin
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TargetCandidate {
    Direct(DirectTargetCandidate),
    Magnetic(MagneticTargetCandidate),
}

impl TargetCandidate {
    pub(crate) const fn target(self) -> DropTargetId {
        match self {
            Self::Direct(candidate) => candidate.target(),
            Self::Magnetic(candidate) => candidate.target(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct TargetAcquisition {
    scene: SurfaceSceneStamp,
    layer: SceneLayerKey,
    candidate: TargetCandidate,
}

impl TargetAcquisition {
    pub(crate) const fn new(
        scene: SurfaceSceneStamp,
        layer: SceneLayerKey,
        candidate: TargetCandidate,
    ) -> Self {
        Self {
            scene,
            layer,
            candidate,
        }
    }

    pub(crate) const fn scene(self) -> SurfaceSceneStamp {
        self.scene
    }

    pub(crate) const fn target(self) -> DropTargetId {
        self.candidate.target()
    }

    const fn candidate(self) -> TargetCandidate {
        self.candidate
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum TargetObservation {
    Surface {
        scene: SurfaceSceneStamp,
        layer: SceneLayerKey,
        candidate: Option<TargetCandidate>,
    },
    OutsideSurface,
    OutsideAll,
    Unavailable,
}

impl TargetObservation {
    pub(crate) const fn surface(
        scene: SurfaceSceneStamp,
        layer: SceneLayerKey,
        candidate: Option<TargetCandidate>,
    ) -> Self {
        Self::Surface {
            scene,
            layer,
            candidate,
        }
    }

    pub(crate) const fn acquisition(self) -> Option<TargetAcquisition> {
        match self {
            Self::Surface {
                scene,
                layer,
                candidate: Some(candidate),
            } => Some(TargetAcquisition::new(scene, layer, candidate)),
            Self::Surface {
                candidate: None, ..
            }
            | Self::OutsideSurface
            | Self::OutsideAll
            | Self::Unavailable => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TargetLatchDecision {
    AcceptCandidate,
    RetainCurrent,
}

pub(crate) fn reconcile_target(
    current: Option<TargetAcquisition>,
    observation: TargetObservation,
    point: Option<LogicalPoint>,
    release_hysteresis: f64,
) -> TargetLatchDecision {
    let Some(current) = current else {
        return TargetLatchDecision::AcceptCandidate;
    };
    let TargetObservation::Surface {
        scene,
        layer,
        candidate,
    } = observation
    else {
        return TargetLatchDecision::AcceptCandidate;
    };
    // A geometric release envelope cannot cross the frontmost occlusion layer.
    if current.scene() != scene || current.layer != layer {
        return TargetLatchDecision::AcceptCandidate;
    }
    let Some(point) = point else {
        return TargetLatchDecision::AcceptCandidate;
    };
    match (current.candidate(), candidate) {
        (_, Some(TargetCandidate::Direct(_))) => TargetLatchDecision::AcceptCandidate,
        (TargetCandidate::Direct(current), None | Some(TargetCandidate::Magnetic(_)))
            if current.retains_at(point, release_hysteresis) =>
        {
            TargetLatchDecision::RetainCurrent
        }
        (TargetCandidate::Direct(_), None | Some(TargetCandidate::Magnetic(_))) => {
            TargetLatchDecision::AcceptCandidate
        }
        (TargetCandidate::Magnetic(current), Some(TargetCandidate::Magnetic(next)))
            if next.target() == current.target() =>
        {
            TargetLatchDecision::AcceptCandidate
        }
        (TargetCandidate::Magnetic(current), Some(TargetCandidate::Magnetic(next)))
            if next.is_deeply_acquired_at(point, release_hysteresis)
                || !current.retains_at(point, release_hysteresis) =>
        {
            TargetLatchDecision::AcceptCandidate
        }
        (TargetCandidate::Magnetic(current), Some(TargetCandidate::Magnetic(_)) | None)
            if current.retains_at(point, release_hysteresis) =>
        {
            TargetLatchDecision::RetainCurrent
        }
        (TargetCandidate::Magnetic(_), Some(TargetCandidate::Magnetic(_)) | None) => {
            TargetLatchDecision::AcceptCandidate
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::LogicalRect;
    use crate::ids::{EngineAuthorityDomainId, NodeId, RootId, SurfaceId, WorkspaceEpoch};
    use crate::policy::PolicyRevision;
    use crate::presentation_config::PresentationConfigRevision;
    use crate::scene_manifest::{
        SurfaceMeasurementTicket, SurfaceRequirementRevision, SurfaceSceneRevision,
    };

    const SURFACE: SurfaceId = SurfaceId::new(1);

    fn point(x: f64, y: f64) -> LogicalPoint {
        LogicalPoint::new(x, y).expect("test point must be finite")
    }

    fn scene(revision: u64) -> SurfaceSceneStamp {
        SurfaceSceneStamp::new(
            SurfaceMeasurementTicket::new(
                EngineAuthorityDomainId::new_for_test(1),
                WorkspaceEpoch::new(1),
                PresentationConfigRevision::new(1),
                PolicyRevision::new(1),
                SurfaceRequirementRevision::new(1),
                SURFACE,
            ),
            SurfaceSceneRevision::new(revision),
        )
    }

    fn tab_gap(scene: SurfaceSceneStamp) -> TargetAcquisition {
        TargetAcquisition::new(
            scene,
            SceneLayerKey::surface_base(),
            TargetCandidate::Direct(DirectTargetCandidate::new(
                DropTargetId::TabGap {
                    surface: SURFACE,
                    root: RootId::new(2),
                    tabs: NodeId::default(),
                    index: 1,
                },
                DirectTargetProvenance::TabGap,
                HitRegion::new(
                    LogicalRect::new(0.0, 0.0, 10.0, 10.0).expect("test hit region must be valid"),
                ),
            )),
        )
    }

    #[test]
    fn direct_target_retains_only_inside_its_configured_release_envelope() {
        let scene = scene(1);
        let current = Some(tab_gap(scene));

        assert_eq!(
            reconcile_target(
                current,
                TargetObservation::surface(scene, SceneLayerKey::surface_base(), None),
                Some(point(17.5, 5.0)),
                8.0,
            ),
            TargetLatchDecision::RetainCurrent
        );
        assert_eq!(
            reconcile_target(
                current,
                TargetObservation::surface(scene, SceneLayerKey::surface_base(), None),
                Some(point(18.5, 5.0)),
                8.0,
            ),
            TargetLatchDecision::AcceptCandidate
        );
    }

    #[test]
    fn scene_replacement_and_outside_all_never_retain_a_direct_target() {
        let current_scene = scene(1);
        let current = Some(tab_gap(current_scene));

        assert_eq!(
            reconcile_target(
                current,
                TargetObservation::surface(scene(2), SceneLayerKey::surface_base(), None),
                Some(point(5.0, 5.0)),
                8.0,
            ),
            TargetLatchDecision::AcceptCandidate
        );
        assert_eq!(
            reconcile_target(
                current,
                TargetObservation::OutsideAll,
                Some(point(5.0, 5.0)),
                8.0,
            ),
            TargetLatchDecision::AcceptCandidate
        );
    }

    #[test]
    fn another_occlusion_layer_or_outside_surface_never_retains_a_target() {
        let scene = scene(1);
        let current = Some(tab_gap(scene));
        for observation in [
            TargetObservation::surface(scene, SceneLayerKey::contained(0).unwrap(), None),
            TargetObservation::OutsideSurface,
        ] {
            assert_eq!(
                reconcile_target(current, observation, Some(point(5.0, 5.0)), 8.0),
                TargetLatchDecision::AcceptCandidate,
            );
        }
    }
}
