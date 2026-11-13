//! Rollbackable layout trajectories over already validated target plans.

use std::collections::BTreeMap;
use std::sync::Arc;

use super::*;
use crate::layout::SplitWeightOverride;
use crate::motion::{MotionFrame, RectMotion, Transition};
use crate::scene::{ContainedLayoutProjection, SurfacePlanScene};

#[derive(Clone, Debug, Default, PartialEq)]
pub(super) struct LayoutMotionState {
    pub(super) frame: Option<MotionFrame>,
    pub(super) surfaces: BTreeMap<SurfaceId, Arc<SurfaceMotionState>>,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct SurfaceMotionState {
    pub(super) target: Arc<PresentationPlan>,
    coordinates: SurfaceCoordinateCapture,
    frame: MotionFrame,
    tracks: BTreeMap<ItemId, PaneMotion>,
    active: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct PaneMotion {
    root: RootId,
    motion: RectMotion,
    rank: usize,
}

impl DockEngine {
    pub(crate) fn set_motion_frame(&mut self, frame: MotionFrame) {
        self.presentation_authority.motion.frame = Some(frame);
        let epoch = self.version.epoch();
        let workspace = &self.workspace;
        self.presentation_authority
            .motion
            .surfaces
            .retain(|surface, state| {
                workspace.surface(*surface).is_some()
                    && state
                        .target
                        .measurement_ticket()
                        .is_some_and(|ticket| ticket.workspace_epoch() == epoch)
            });
    }

    pub(crate) fn surface_motion_is_active(&self, surface: SurfaceId) -> bool {
        self.presentation_authority
            .motion
            .surfaces
            .get(&surface)
            .is_some_and(|state| state.active)
    }

    pub(super) fn motion_needs_refresh(&self, surface: SurfaceId) -> bool {
        let motion = &self.presentation_authority.motion;
        let (Some(frame), Some(state)) = (motion.frame, motion.surfaces.get(&surface)) else {
            return false;
        };
        frame.settings != state.frame.settings
            || frame.preference != state.frame.preference
            || (state.active && frame.now != state.frame.now)
    }

    pub(super) fn cached_motion_target(
        &self,
        token: SurfaceContributionToken,
        measurements: &SurfaceMeasurements,
        contained: Option<ContainedLayoutProjection>,
        resize: &[SplitWeightOverride<'_>],
    ) -> Option<Arc<PresentationPlan>> {
        // Local tab-strip edits invalidate Ready without changing measurements.
        // Reuse only while the scene still retains that prepared geometry.
        self.scene().ready_candidate(token.surface())?;
        let target = &self
            .presentation_authority
            .motion
            .surfaces
            .get(&token.surface())?
            .target;
        let layout = target.layout_facts()?;
        (target.measurement_ticket() == Some(token.ticket())
            && target.popup()
                == self
                    .presentation_authority
                    .tab_strip_states
                    .popup_requirement()
            && target.popup()
                == self
                    .presentation_authority
                    .presentation_requirements
                    .popup()
            && layout.config() == &self.presentation_authority.presentation_config
            && layout.measurements().as_ref() == measurements
            && target.contained_projection() == contained
            && target.resize_projection().len() == resize.len()
            && target
                .resize_projection()
                .iter()
                .zip(resize)
                .all(|(old, current)| {
                    old.split() == current.source() && old.weights() == current.weights()
                }))
        .then(|| Arc::clone(target))
    }

    pub(super) fn prepare_layout_motion(
        &self,
        mut contribution: PreparedSurfaceContribution,
    ) -> Result<PreparedSurfaceContribution, SurfaceContributionPrepareError> {
        let surface = contribution.surface();
        let Some(frame) = self.presentation_authority.motion.frame else {
            return Ok(contribution);
        };
        let PreparedSurfaceContributionState::Ready { plan: target } = &contribution.state else {
            return Ok(contribution);
        };
        // Prospective native content must retain one exact first-live candidate.
        if self.scene().native_first_live_projection(surface).is_some() {
            return Ok(contribution);
        }
        let old = self.presentation_authority.motion.surfaces.get(&surface);
        let coordinates = contribution.token.coordinates;
        let prior = self
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::confirmed_presentation)
            .filter(|scene| scene.coordinate_capture() == coordinates)
            .filter(|scene| scene.plan().bounds() == target.bounds());
        let transition = if self.interaction.active_surface() == Some(surface)
            || target.popup().visual_for_surface(surface).is_some()
            || target.contained_projection().is_some()
            || !target.resize_projection().is_empty()
            || prior.is_some_and(|scene| {
                scene.plan().contained_projection().is_some()
                    || !scene.plan().resize_projection().is_empty()
            }) {
            Transition::Instant
        } else {
            frame.settings.layout_transition(frame.preference)
        };
        if transition == Transition::Instant {
            return Ok(contribution);
        }
        if let Some(old) = old
            && Arc::ptr_eq(target, &old.target)
            && !old.active
            && frame.settings == old.frame.settings
            && frame.preference == old.frame.preference
        {
            contribution.next_motion = Some(Arc::clone(old));
            return Ok(contribution);
        }
        let prior_panes =
            std::cell::LazyCell::new(|| prior.map(presented_panes).unwrap_or_default());
        let continuing = old.filter(|state| {
            state.coordinates == coordinates
                && state.target.bounds() == target.bounds()
                && state
                    .target
                    .measurement_ticket()
                    .is_some_and(|ticket| ticket.workspace_epoch() == self.version.epoch())
        });
        let mut tracks = BTreeMap::new();
        let mut rectangles = BTreeMap::new();
        let mut ranks = BTreeMap::new();
        let mut active = false;
        let mut geometry_changed = false;
        let count = target.pane_records().len();
        for (ordinal, pane) in target.pane_records().iter().enumerate() {
            let Some(item) = target.pane_selected(pane) else {
                continue;
            };
            let mut motion = continuing
                .and_then(|state| state.tracks.get(&item))
                .filter(|track| track.root == pane.id().root)
                .filter(|_| {
                    continuing
                        .is_some_and(|state| Arc::ptr_eq(target, &state.target) || state.active)
                })
                .map(|track| track.motion)
                .or_else(|| {
                    prior_panes
                        .get(&item)
                        .filter(|(root, _)| *root == pane.id().root)
                        .map(|(_, rect)| RectMotion::new(*rect))
                })
                .unwrap_or_else(|| RectMotion::new(pane.bounds()));
            motion.retarget(pane.bounds(), transition, frame.now);
            let sample = motion.sample(frame.now);
            let moving = sample.rect() != pane.bounds();
            active |= sample.is_active();
            geometry_changed |= moving;
            rectangles.insert(pane.id(), sample.rect());
            // Preserve target roster order; move each complete travelling pane
            // above stationary peers rather than ranking controls separately.
            let rank = continuing
                .filter(|state| Arc::ptr_eq(target, &state.target))
                .and_then(|state| state.tracks.get(&item))
                .map_or_else(
                    || ordinal.saturating_add(if sample.is_active() { count } else { 0 }),
                    |track| track.rank,
                );
            ranks.insert(pane.id(), rank);
            tracks.insert(
                item,
                PaneMotion {
                    root: pane.id().root,
                    motion,
                    rank,
                },
            );
        }
        let state = Arc::new(SurfaceMotionState {
            target: Arc::clone(target),
            coordinates,
            frame,
            tracks,
            active,
        });
        if geometry_changed {
            let sampled = crate::scene_compiler::materialize_presentation_sample(
                &self.workspace,
                &self.policy,
                &self.presentation_authority.presentation_requirements,
                &self.presentation_authority.tab_strip_states,
                target,
                &rectangles,
                &ranks,
            )
            .map_err(SurfaceContributionPrepareError::Compilation)?;
            contribution.state = PreparedSurfaceContributionState::Ready {
                plan: Arc::new(sampled),
            };
        }
        contribution.next_motion = Some(state);
        Ok(contribution)
    }
}

fn presented_panes(scene: &SurfacePlanScene) -> BTreeMap<ItemId, (RootId, LogicalRect)> {
    scene
        .plan()
        .pane_records()
        .iter()
        .filter_map(|pane| {
            scene
                .plan()
                .pane_selected(pane)
                .map(|item| (item, (pane.id().root, pane.bounds())))
        })
        .collect()
}

impl CoreHostFrame {
    pub(crate) fn set_motion_frame(&mut self, frame: MotionFrame) {
        self.candidate.set_motion_frame(frame);
    }
}

impl HostFrameView<'_> {
    pub(crate) fn motion_frame(self) -> Option<MotionFrame> {
        self.engine.presentation_authority.motion.frame
    }
}

#[cfg(test)]
mod tests;
