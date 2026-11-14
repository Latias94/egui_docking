//! Final-presentation-qualified egui receiver bindings.

use std::collections::BTreeMap;
use std::sync::Arc;

use dockspace::geometry::LogicalPoint;
use dockspace::model::{ItemId, SurfaceId};
use dockspace::runtime::{
    NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeScrollReceiverChallenge, NativeSurfaceBinding, NativeSurfaceReceiverQuery,
    NativeWindowBinding, NativeWindowHit, NativeWindowHitQuery, PaintedSurfaceOutput,
    SurfacePresentationArtifact, SurfacePresentationCommit,
};
use eframe::{NativeOutputToken, egui};
use egui_dockspace::native_support::{NativePaintReceiver, NativeScrollPaintReceiver};

use crate::host_frame::NativeSurfacePaint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReceiverStageError {
    ViewportMismatch,
    BindingMismatch,
    SemanticOutputMismatch,
    HitAuthorityMismatch,
    InvalidPixelsPerPoint,
    PassRegressed,
    StorageUnavailable,
}

/// Hit authority and pixel scale from the same completed output pass.
pub(crate) struct NativeReceiverPass {
    viewport: egui::ViewportId,
    hit_authority: Arc<egui::CompletedPassHitAuthority>,
    pixels_per_point: f64,
}

impl NativeReceiverPass {
    pub(crate) fn capture(
        context: &egui::Context,
        viewport: egui::ViewportId,
        pixels_per_point: f32,
    ) -> Result<Self, NativeReceiverStageError> {
        if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
            return Err(NativeReceiverStageError::InvalidPixelsPerPoint);
        }
        let hit_authority = context
            .capture_completed_pass_hit_authority(viewport)
            .ok_or(NativeReceiverStageError::HitAuthorityMismatch)?;
        Ok(Self {
            viewport,
            hit_authority: Arc::new(hit_authority),
            pixels_per_point: f64::from(pixels_per_point),
        })
    }
}

#[derive(Debug)]
struct PendingReceiverSet {
    surface: SurfaceId,
    binding: NativeSurfaceBinding,
    viewport: egui::ViewportId,
    hit_authority: Arc<egui::CompletedPassHitAuthority>,
    pixels_per_point: f64,
    artifact: SurfacePresentationArtifact,
    receivers: Vec<NativePaintReceiver>,
    scroll_receivers: Vec<PresentedScrollReceiver>,
    focus_bindings: Vec<(ItemId, egui::Id)>,
}

impl PendingReceiverSet {
    fn from_paint(
        pass: &NativeReceiverPass,
        binding: NativeSurfaceBinding,
        output: &PaintedSurfaceOutput,
        paint: &NativeSurfacePaint,
    ) -> Result<Self, NativeReceiverStageError> {
        if pass.viewport != paint.viewport_id() {
            return Err(NativeReceiverStageError::ViewportMismatch);
        }
        if binding.surface() != paint.surface() {
            return Err(NativeReceiverStageError::BindingMismatch);
        }
        let Some(semantic_output) = paint.semantic_output() else {
            return Err(NativeReceiverStageError::SemanticOutputMismatch);
        };
        if !output.matches_native_binding(binding)
            || !output.matches_semantic_output(semantic_output)
        {
            return Err(NativeReceiverStageError::SemanticOutputMismatch);
        }
        let viewport = paint.viewport_id();
        let cumulative_pass_nr = paint.cumulative_pass_nr();
        if pass.hit_authority.cumulative_pass_nr() != cumulative_pass_nr {
            return Err(NativeReceiverStageError::HitAuthorityMismatch);
        }
        let receivers = paint.receivers().collect::<Vec<_>>();
        let scroll_receivers = paint
            .scroll_receivers()
            .zip(paint.scroll_identities().iter().copied())
            .map(|(receiver, identity)| PresentedScrollReceiver { receiver, identity })
            .collect::<Vec<_>>();
        Ok(Self {
            surface: paint.surface(),
            binding,
            viewport,
            hit_authority: Arc::clone(&pass.hit_authority),
            pixels_per_point: pass.pixels_per_point,
            artifact: output.presentation_artifact(),
            receivers,
            scroll_receivers,
            focus_bindings: paint.pane_focus_bindings().to_vec(),
        })
    }
}

#[derive(Debug)]
struct PresentedReceiverSet {
    binding: NativeSurfaceBinding,
    viewport: egui::ViewportId,
    hit_authority: Arc<egui::CompletedPassHitAuthority>,
    pixels_per_point: f64,
    artifact: SurfacePresentationArtifact,
    receivers: Vec<NativePaintReceiver>,
    scroll_receivers: Vec<PresentedScrollReceiver>,
    focus_bindings: Vec<(ItemId, egui::Id)>,
}

#[derive(Debug, Clone, Copy)]
struct PresentedScrollReceiver {
    receiver: NativeScrollPaintReceiver,
    identity: egui::WidgetHitIdentity,
}

#[derive(Debug, Default)]
pub(crate) struct NativeReceiverStore {
    pending: BTreeMap<NativeOutputToken, Vec<PendingReceiverSet>>,
    awaiting_core: Vec<PendingReceiverSet>,
    presented: BTreeMap<SurfaceId, Vec<PresentedReceiverSet>>,
}

impl NativeReceiverStore {
    pub(crate) fn presented_focus_bindings(
        &self,
        surface: SurfaceId,
        viewport: egui::ViewportId,
        current: Option<dockspace::runtime::PresentedDockspaceSurface>,
    ) -> &[(ItemId, egui::Id)] {
        let Some(current) = current else {
            return &[];
        };
        let Some(presented) = self.presented.get(&surface) else {
            return &[];
        };
        let mut exact = presented.iter().filter(|entry| {
            entry.viewport == viewport && current.matches_presentation_artifact(entry.artifact)
        });
        let Some(entry) = exact.next() else {
            return &[];
        };
        if exact.next().is_some() {
            return &[];
        }
        &entry.focus_bindings
    }

    pub(crate) fn has_pending_commit(&self) -> bool {
        !self.awaiting_core.is_empty()
    }

    pub(crate) fn stage_outputs(
        &mut self,
        pass: &NativeReceiverPass,
        token: NativeOutputToken,
        window: NativeWindowBinding,
        bindings: &[NativeSurfaceBinding],
        outputs: &[PaintedSurfaceOutput],
        paints: &[NativeSurfacePaint],
    ) -> Result<(), NativeReceiverStageError> {
        if outputs.is_empty() || outputs.len() != paints.len() {
            return Err(NativeReceiverStageError::SemanticOutputMismatch);
        }
        if pass.viewport != token.viewport_id() {
            return Err(NativeReceiverStageError::ViewportMismatch);
        }
        let mut next = Vec::new();
        next.try_reserve_exact(outputs.len())
            .map_err(|_| NativeReceiverStageError::StorageUnavailable)?;
        for output in outputs {
            if next
                .iter()
                .any(|entry: &PendingReceiverSet| entry.surface == output.surface())
            {
                return Err(NativeReceiverStageError::SemanticOutputMismatch);
            }
            let mut matching = paints
                .iter()
                .filter(|paint| paint.surface() == output.surface());
            let paint = matching
                .next()
                .ok_or(NativeReceiverStageError::SemanticOutputMismatch)?;
            if matching.next().is_some() {
                return Err(NativeReceiverStageError::SemanticOutputMismatch);
            }
            let binding = bindings
                .iter()
                .copied()
                .find(|binding| binding.surface() == output.surface() && binding.window() == window)
                .ok_or(NativeReceiverStageError::BindingMismatch)?;
            next.push(PendingReceiverSet::from_paint(
                pass, binding, output, paint,
            )?);
        }
        if let Some(current) = self.pending.get(&token) {
            if current.len() != next.len()
                || current.iter().zip(&next).any(|(current, next)| {
                    current.surface != next.surface
                        || current.binding != next.binding
                        || current.viewport != next.viewport
                        || current.artifact != next.artifact
                        || next.hit_authority.cumulative_pass_nr()
                            < current.hit_authority.cumulative_pass_nr()
                })
            {
                return Err(NativeReceiverStageError::PassRegressed);
            }
        }
        self.pending.insert(token, next);
        Ok(())
    }

    pub(crate) fn renderer_presented(
        &mut self,
        token: NativeOutputToken,
        binding: NativeWindowBinding,
    ) {
        let pending = self.pending.remove(&token);
        self.finish_renderer_presented_outputs(pending, token.viewport_id(), binding);
    }

    fn finish_renderer_presented_outputs(
        &mut self,
        pending: Option<Vec<PendingReceiverSet>>,
        viewport: egui::ViewportId,
        binding: NativeWindowBinding,
    ) {
        let Some(pending) = pending else {
            return;
        };
        // Validate the whole token before promoting any receiver contribution.
        if pending.is_empty()
            || pending.iter().enumerate().any(|(index, entry)| {
                entry.binding.window() != binding
                    || entry.surface != entry.binding.surface()
                    || entry.viewport != viewport
                    || !Arc::ptr_eq(&pending[0].hit_authority, &entry.hit_authority)
                    || pending[..index]
                        .iter()
                        .any(|previous| previous.surface == entry.surface)
                    || self
                        .awaiting_core
                        .iter()
                        .any(|candidate| candidate.artifact == entry.artifact)
            })
        {
            return;
        }
        self.awaiting_core.extend(pending);
    }

    #[cfg(test)]
    fn finish_renderer_presented(
        &mut self,
        pending: Option<PendingReceiverSet>,
        viewport: egui::ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        self.finish_renderer_presented_outputs(
            pending.map(|pending| vec![pending]),
            viewport,
            binding.window(),
        );
    }

    pub(crate) fn dropped(&mut self, token: NativeOutputToken) {
        self.pending.remove(&token);
    }

    pub(crate) fn abandon(&mut self, token: NativeOutputToken) {
        self.pending.remove(&token);
    }

    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) {
        self.pending
            .retain(|_, pending| pending.iter().all(|entry| entry.binding != binding));
        self.awaiting_core
            .retain(|pending| pending.binding != binding);
        self.presented.retain(|_, presented| {
            presented.retain(|candidate| candidate.binding != binding);
            !presented.is_empty()
        });
    }

    pub(crate) fn retire_window(&mut self, window: NativeWindowBinding) {
        self.pending
            .retain(|_, pending| pending.iter().all(|entry| entry.binding.window() != window));
        self.awaiting_core
            .retain(|pending| pending.binding.window() != window);
        self.presented.retain(|_, presented| {
            presented.retain(|candidate| candidate.binding.window() != window);
            !presented.is_empty()
        });
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .values()
            .flatten()
            .any(|pending| pending.binding == binding)
            || self
                .awaiting_core
                .iter()
                .any(|pending| pending.binding == binding)
            || self
                .presented
                .get(&binding.surface())
                .is_some_and(|presented| {
                    presented
                        .iter()
                        .any(|candidate| candidate.binding == binding)
                })
    }

    pub(crate) fn commit_presentations(&mut self, commits: &[SurfacePresentationCommit]) {
        for commit in commits {
            self.presented.retain(|_, presented| {
                presented.retain(|candidate| !commit.retires(candidate.artifact));
                !presented.is_empty()
            });

            for candidate in self
                .awaiting_core
                .extract_if(.., |candidate| commit.settles(candidate.artifact))
            {
                if !commit.presents(candidate.artifact) {
                    continue;
                }
                let candidate = PresentedReceiverSet {
                    binding: candidate.binding,
                    viewport: candidate.viewport,
                    hit_authority: candidate.hit_authority,
                    pixels_per_point: candidate.pixels_per_point,
                    artifact: candidate.artifact,
                    receivers: candidate.receivers,
                    scroll_receivers: candidate.scroll_receivers,
                    focus_bindings: candidate.focus_bindings,
                };
                let presented = self
                    .presented
                    .entry(candidate.binding.surface())
                    .or_default();
                if let Some(existing) = presented
                    .iter_mut()
                    .find(|existing| existing.artifact == candidate.artifact)
                {
                    *existing = candidate;
                } else {
                    presented.push(candidate);
                }
            }
        }
    }

    pub(crate) fn resolve(&self, query: NativeReceiverQuery) -> NativeReceiverAnswer {
        match query {
            NativeReceiverQuery::WindowHit(query) => {
                NativeReceiverAnswer::WindowHit(self.resolve_window_hit(query))
            }
            NativeReceiverQuery::Surface(query) => self.resolve_surface(query),
        }
    }

    fn resolve_window_hit(&self, query: NativeWindowHitQuery) -> NativeWindowHit {
        let candidates = || {
            self.presented
                .values()
                .flatten()
                .filter(|entry| entry.binding.window() == query.window())
        };
        let Some(latest) =
            candidates().max_by_key(|entry| entry.hit_authority.cumulative_pass_nr())
        else {
            return NativeWindowHit::Unknown;
        };
        let Some(point) = physical_pos(query.point(), latest.pixels_per_point) else {
            return NativeWindowHit::Unknown;
        };
        let Some(hit) = latest.hit_authority.hit_test(point) else {
            return NativeWindowHit::Unknown;
        };
        let Some(identity) = hit.contains_pointer() else {
            return NativeWindowHit::WindowBackground;
        };
        let matching = |entry: &&PresentedReceiverSet| {
            Arc::ptr_eq(&entry.hit_authority, &latest.hit_authority)
                && entry.has_hit_identity(identity)
        };
        let mut matching = candidates().filter(matching);
        let Some(selected) = matching.next() else {
            return NativeWindowHit::Blocked;
        };
        if matching.next().is_some() {
            return NativeWindowHit::Unknown;
        }
        for lane in [hit.click(), hit.drag()].into_iter().flatten() {
            if !selected.has_hit_identity(lane) {
                return NativeWindowHit::Blocked;
            }
        }
        NativeWindowHit::Surface(selected.artifact)
    }

    fn resolve_surface(&self, query: NativeSurfaceReceiverQuery) -> NativeReceiverAnswer {
        let Some(candidates) = self.presented.get(&query.surface()) else {
            return NativeReceiverAnswer::Unknown;
        };
        let mut candidates = candidates.iter().filter(|presented| {
            query.matches_native_binding(presented.binding)
                && query.matches_presentation_artifact(presented.artifact)
        });
        let Some(presented) = candidates.next() else {
            return NativeReceiverAnswer::Unknown;
        };
        if candidates.next().is_some() {
            return NativeReceiverAnswer::Unknown;
        }
        let Some(point) = query
            .client_position()
            .and_then(|point| physical_pos(point, presented.pixels_per_point))
        else {
            return NativeReceiverAnswer::Unknown;
        };
        if let NativeReceiverPurpose::ScrollDelivery(challenge) = query.purpose() {
            return resolve_scroll(query, presented, point, *challenge);
        }
        let Some(hit) = presented.hit_authority.hit_test(point) else {
            return NativeReceiverAnswer::Unknown;
        };
        let identity = match query.purpose() {
            NativeReceiverPurpose::ClickDelivery => hit.click(),
            NativeReceiverPurpose::DragDelivery => hit.drag(),
            NativeReceiverPurpose::HoverHit => hit.contains_pointer(),
            NativeReceiverPurpose::ScrollDelivery(_) => unreachable!("scroll resolved above"),
        };
        let receiver_for = |identity: egui::WidgetHitIdentity| {
            let mut matches = presented.receivers.iter().copied().filter(|receiver| {
                receiver.viewport_id() == presented.viewport
                    && receiver.cumulative_pass_nr() == presented.hit_authority.cumulative_pass_nr()
                    && receiver.widget_id() == identity.id()
                    && receiver.layer_id() == identity.layer_id()
            });
            let receiver = matches
                .next()
                .ok_or_else(|| NativeReceiverAnswer::Blocked(query.presented_surface()))?;
            if matches.next().is_some() {
                return Err(NativeReceiverAnswer::Unknown);
            }
            Ok(receiver)
        };
        let Some(identity) = identity else {
            if let Some(identity) = hit.contains_pointer() {
                if let Err(answer) = receiver_for(identity) {
                    return answer;
                }
                // egui can suppress one capture lane behind a widget owning
                // the other, including registered docking controls. This is
                // lane occlusion, not evidence of empty core hit geometry.
                // Keep the other lane independently available to its receiver.
                let occluding = match query.purpose() {
                    NativeReceiverPurpose::ClickDelivery => hit.drag(),
                    NativeReceiverPurpose::DragDelivery => hit.click(),
                    NativeReceiverPurpose::HoverHit => None,
                    NativeReceiverPurpose::ScrollDelivery(_) => {
                        unreachable!("scroll resolved above")
                    }
                };
                if let Some(identity) = occluding {
                    return match receiver_for(identity) {
                        Ok(_) => NativeReceiverAnswer::Blocked(query.presented_surface()),
                        Err(answer) => answer,
                    };
                }
            }
            return NativeReceiverAnswer::EmptyLane(query.presented_surface());
        };
        let receiver = match receiver_for(identity) {
            Ok(receiver) => receiver,
            Err(answer) => return answer,
        };
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            // A hover-only label can be in front while a different widget still
            // owns click or drag. Clear the pane only when every active hit lane
            // belongs to this exact output's declared docking/content widgets.
            for identity in [hit.click(), hit.drag()].into_iter().flatten() {
                if let Err(answer) = receiver_for(identity) {
                    return answer;
                }
            }
        }
        resolve_matching_receiver(query, receiver)
    }
}

impl PresentedReceiverSet {
    fn has_hit_identity(&self, identity: egui::WidgetHitIdentity) -> bool {
        self.receivers.iter().any(|receiver| {
            receiver.widget_id() == identity.id()
                && receiver.layer_id() == identity.layer_id()
                && receiver.cumulative_pass_nr() == self.hit_authority.cumulative_pass_nr()
        }) || self.scroll_receivers.iter().any(|candidate| {
            candidate.identity == identity
                && candidate.receiver.cumulative_pass_nr()
                    == self.hit_authority.cumulative_pass_nr()
        })
    }
}

fn resolve_matching_receiver(
    query: NativeSurfaceReceiverQuery,
    receiver: NativePaintReceiver,
) -> NativeReceiverAnswer {
    if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
        receiver.clear_hover_lane_for_query(query).map_or(
            NativeReceiverAnswer::Unknown,
            NativeReceiverAnswer::ClearHoverLane,
        )
    } else {
        receiver
            .bind_for_query(query)
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
    }
}

fn resolve_scroll(
    query: NativeSurfaceReceiverQuery,
    presented: &PresentedReceiverSet,
    point: egui::Pos2,
    challenge: NativeScrollReceiverChallenge,
) -> NativeReceiverAnswer {
    let Some(projected_delta) = widget_scroll_delta(challenge) else {
        return NativeReceiverAnswer::Unknown;
    };
    let candidates = presented
        .scroll_receivers
        .iter()
        .copied()
        .map(|receiver| receiver.identity)
        .collect::<Vec<_>>();
    let challenge = match challenge {
        NativeScrollReceiverChallenge::Spatial { .. } => {
            egui::WidgetScrollHitChallenge::Spatial { projected_delta }
        }
        NativeScrollReceiverChallenge::Locked { receiver, .. } => {
            let mut matches = presented
                .scroll_receivers
                .iter()
                .copied()
                .filter(|candidate| candidate.receiver.bind_for_query(query) == Some(receiver));
            let Some(candidate) = matches.next() else {
                return NativeReceiverAnswer::Unknown;
            };
            if matches.next().is_some() {
                return NativeReceiverAnswer::Unknown;
            }
            egui::WidgetScrollHitChallenge::Locked {
                receiver: candidate.identity,
                projected_delta,
            }
        }
    };
    let Some(hit) =
        presented
            .hit_authority
            .scroll_hit_test(point, candidates.as_slice(), challenge)
    else {
        return NativeReceiverAnswer::Unknown;
    };
    match hit.hit() {
        egui::WidgetScrollHit::Blocked => NativeReceiverAnswer::Blocked(query.presented_surface()),
        egui::WidgetScrollHit::NoReceiver => {
            NativeReceiverAnswer::EmptyLane(query.presented_surface())
        }
        egui::WidgetScrollHit::Candidate(identity) => {
            let mut matches = presented
                .scroll_receivers
                .iter()
                .copied()
                .filter(|candidate| {
                    candidate.receiver.viewport_id() == presented.viewport
                        && candidate.receiver.cumulative_pass_nr()
                            == presented.hit_authority.cumulative_pass_nr()
                        && candidate.identity == identity
                });
            let Some(candidate) = matches.next() else {
                return NativeReceiverAnswer::Unknown;
            };
            if matches.next().is_some() {
                return NativeReceiverAnswer::Unknown;
            }
            candidate
                .receiver
                .bind_for_query(query)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }
}

fn widget_scroll_delta(
    challenge: NativeScrollReceiverChallenge,
) -> Option<Option<egui::WidgetScrollDelta>> {
    challenge.projected_delta().map_or(Some(None), |delta| {
        egui::WidgetScrollDelta::new(delta.x(), delta.y()).map(Some)
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "egui's logical coordinate space is f32 and rejects non-finite conversions"
)]
fn logical_pos(point: LogicalPoint) -> Option<egui::Pos2> {
    let x = point.x() as f32;
    let y = point.y() as f32;
    (x.is_finite() && y.is_finite()).then_some(egui::pos2(x, y))
}

fn physical_pos(
    point: dockspace::geometry::PhysicalPoint,
    pixels_per_point: f64,
) -> Option<egui::Pos2> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return None;
    }
    logical_pos(LogicalPoint::new(point.x() / pixels_per_point, point.y() / pixels_per_point).ok()?)
}

#[cfg(test)]
mod focus_tests;

#[cfg(test)]
mod menu_tests;

#[cfg(test)]
mod no_frame_tests;

#[cfg(test)]
mod lane_tests;

#[cfg(test)]
mod tests;
