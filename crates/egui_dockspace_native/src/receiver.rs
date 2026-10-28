//! Final-presentation-qualified egui receiver bindings.

use std::collections::BTreeMap;

use dockspace::geometry::LogicalPoint;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeScrollReceiverChallenge, NativeSurfaceBinding, PaintedSurfaceOutput,
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
    PassRegressed,
}

#[derive(Debug)]
struct PendingReceiverSet {
    surface: SurfaceId,
    binding: NativeSurfaceBinding,
    viewport: egui::ViewportId,
    hit_authority: egui::CompletedPassHitAuthority,
    artifact: SurfacePresentationArtifact,
    receivers: Vec<NativePaintReceiver>,
    scroll_receivers: Vec<PresentedScrollReceiver>,
}

impl PendingReceiverSet {
    fn from_paint(
        context: &egui::Context,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
        output: &PaintedSurfaceOutput,
        paint: &NativeSurfacePaint,
    ) -> Result<Self, NativeReceiverStageError> {
        if token.viewport_id() != paint.viewport_id() {
            return Err(NativeReceiverStageError::ViewportMismatch);
        }
        if binding.surface() != paint.surface() {
            return Err(NativeReceiverStageError::BindingMismatch);
        }
        let Some(semantic_output) = paint.semantic_output() else {
            return Err(NativeReceiverStageError::SemanticOutputMismatch);
        };
        if !output.matches_semantic_output(semantic_output) {
            return Err(NativeReceiverStageError::SemanticOutputMismatch);
        }
        let viewport = paint.viewport_id();
        let cumulative_pass_nr = paint.cumulative_pass_nr();
        let hit_authority = context
            .capture_completed_pass_hit_authority(viewport)
            .filter(|authority| authority.cumulative_pass_nr() == cumulative_pass_nr)
            .ok_or(NativeReceiverStageError::HitAuthorityMismatch)?;
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
            hit_authority,
            artifact: output.presentation_artifact(),
            receivers,
            scroll_receivers,
        })
    }
}

#[derive(Debug)]
struct PresentedReceiverSet {
    binding: NativeSurfaceBinding,
    viewport: egui::ViewportId,
    hit_authority: egui::CompletedPassHitAuthority,
    artifact: SurfacePresentationArtifact,
    receivers: Vec<NativePaintReceiver>,
    scroll_receivers: Vec<PresentedScrollReceiver>,
}

#[derive(Debug, Clone, Copy)]
struct PresentedScrollReceiver {
    receiver: NativeScrollPaintReceiver,
    identity: egui::WidgetHitIdentity,
}

#[derive(Debug, Default)]
pub(crate) struct NativeReceiverStore {
    pending: BTreeMap<NativeOutputToken, PendingReceiverSet>,
    awaiting_core: Vec<PendingReceiverSet>,
    presented: BTreeMap<SurfaceId, Vec<PresentedReceiverSet>>,
}

impl NativeReceiverStore {
    pub(crate) fn has_pending_commit(&self) -> bool {
        !self.awaiting_core.is_empty()
    }

    pub(crate) fn stage(
        &mut self,
        context: &egui::Context,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
        output: &PaintedSurfaceOutput,
        paint: &NativeSurfacePaint,
    ) -> Result<(), NativeReceiverStageError> {
        let next = PendingReceiverSet::from_paint(context, token, binding, output, paint)?;
        if let Some(current) = self.pending.get(&token)
            && (current.surface != next.surface
                || current.binding != next.binding
                || current.viewport != next.viewport
                || current.artifact != next.artifact
                || next.hit_authority.cumulative_pass_nr()
                    < current.hit_authority.cumulative_pass_nr())
        {
            return Err(NativeReceiverStageError::PassRegressed);
        }
        self.pending.insert(token, next);
        Ok(())
    }

    pub(crate) fn renderer_presented(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) {
        let pending = self.pending.remove(&token);
        self.finish_renderer_presented(pending, token.viewport_id(), binding);
    }

    fn finish_renderer_presented(
        &mut self,
        pending: Option<PendingReceiverSet>,
        viewport: egui::ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let Some(pending) = pending else {
            return;
        };
        if pending.surface != binding.surface()
            || pending.binding != binding
            || pending.viewport != viewport
        {
            return;
        }
        if self
            .awaiting_core
            .iter()
            .any(|candidate| candidate.artifact == pending.artifact)
        {
            return;
        }
        self.awaiting_core.push(pending);
    }

    pub(crate) fn dropped(&mut self, token: NativeOutputToken) {
        self.pending.remove(&token);
    }

    pub(crate) fn abandon(&mut self, token: NativeOutputToken) {
        self.pending.remove(&token);
    }

    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) {
        self.pending.retain(|_, pending| pending.binding != binding);
        self.awaiting_core
            .retain(|pending| pending.binding != binding);
        self.presented.retain(|_, presented| {
            presented.retain(|candidate| candidate.binding != binding);
            !presented.is_empty()
        });
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .values()
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

            let mut promoted = Vec::new();
            self.awaiting_core.retain(|candidate| {
                if !commit.settles(candidate.artifact) {
                    return true;
                }
                if commit.presents(candidate.artifact) {
                    promoted.push(PresentedReceiverSet {
                        binding: candidate.binding,
                        viewport: candidate.viewport,
                        hit_authority: candidate.hit_authority.clone(),
                        artifact: candidate.artifact,
                        receivers: candidate.receivers.clone(),
                        scroll_receivers: candidate.scroll_receivers.clone(),
                    });
                }
                false
            });
            for candidate in promoted {
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
        let Some(point) = query.point().and_then(logical_pos) else {
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
        let Some(identity) = identity else {
            return NativeReceiverAnswer::NoReceiver(query.presented_surface());
        };
        let mut matches = presented.receivers.iter().copied().filter(|receiver| {
            receiver.viewport_id() == presented.viewport
                && receiver.cumulative_pass_nr() == presented.hit_authority.cumulative_pass_nr()
                && receiver.widget_id() == identity.id()
                && receiver.layer_id() == identity.layer_id()
        });
        let Some(receiver) = matches.next() else {
            return NativeReceiverAnswer::Blocked(query.presented_surface());
        };
        if matches.next().is_some() {
            return NativeReceiverAnswer::Unknown;
        }
        resolve_matching_receiver(query, receiver)
    }
}

fn resolve_matching_receiver(
    query: NativeReceiverQuery,
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
    query: NativeReceiverQuery,
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
            NativeReceiverAnswer::NoReceiver(query.presented_surface())
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

#[cfg(test)]
mod tests {
    use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
    use dockspace::model::{
        DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceReceiverRole, DockspaceSession, HostInputOutcome, HostWindowToken,
        NativeCloseState, NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerButton,
        NativePointerEvent, NativePointerHover, NativePointerId, NativePointerInput,
        NativePointerOwner, NativePointerRoster, NativeScrollDelta, NativeScrollDeviceId,
        NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum, NativeScrollPhase,
        NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState,
        NativeWorkAreaRoster, SurfacePaintProgramOperation, SurfacePaintProgramStep,
        SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
    };
    use egui::{Id, RawInput, Rect, Sense, Ui, Vec2};
    use egui_dockspace::{DockStyle, PaneView, native_support};

    use super::*;

    const SURFACE: SurfaceId = SurfaceId::new(1);

    fn binding(window: u64) -> NativeSurfaceBinding {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("receiver test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("receiver test session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("managed native host enrolls");
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .expect("managed native capabilities configure");
        session
            .register_native_root(SURFACE, HostWindowToken::new(window))
            .expect("native root registration queues");
        let mut frame = session
            .begin_host_frame()
            .expect("native registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native registration frame settles");
        let report = frame.commit().expect("native registration frame commits");
        report
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .expect("native registration emits one binding")
    }

    fn presentation_artifact() -> SurfacePresentationArtifact {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("semantic-output test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("semantic-output test session initializes");
        let metrics = UniformSurfaceMetrics::new(
            dockspace::geometry::LogicalRect::new(0.0, 0.0, 640.0, 480.0)
                .expect("semantic-output bounds validate"),
            dockspace::geometry::LogicalSize::new(32.0, 24.0)
                .expect("semantic-output minimum validates"),
            80.0,
        )
        .expect("semantic-output metrics validate");
        let mut measured = session
            .begin_host_frame()
            .expect("semantic-output measurement frame begins");
        measured
            .measure_surface(SURFACE, metrics)
            .expect("semantic-output surface measures");
        measured
            .commit()
            .expect("semantic-output measurement commits");
        let mut frame = session
            .begin_host_frame()
            .expect("semantic-output paint frame begins");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("semantic-output plan lookup succeeds")
            .expect("semantic-output plan is ready");
        let receipt = plan
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("semantic-output program executes");
        frame
            .confirm_surface_painted(receipt)
            .expect("semantic-output paint confirms");
        let mut report = frame.commit().expect("semantic-output paint commits");
        let output = report
            .take_painted_outputs()
            .pop()
            .expect("semantic-output paint emits one output");
        output.presentation_artifact()
    }

    fn empty_hit_authority() -> egui::CompletedPassHitAuthority {
        let context = egui::Context::default();
        let mut output = context.run_ui(RawInput::default(), |_| {});
        output.textures_delta.clear();
        context
            .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
            .expect("one completed root pass publishes hit authority")
    }

    fn empty_pending_receiver(
        binding: NativeSurfaceBinding,
        viewport: egui::ViewportId,
    ) -> PendingReceiverSet {
        pending_receiver(binding, viewport, presentation_artifact())
    }

    fn pending_receiver(
        binding: NativeSurfaceBinding,
        viewport: egui::ViewportId,
        artifact: SurfacePresentationArtifact,
    ) -> PendingReceiverSet {
        PendingReceiverSet {
            surface: SURFACE,
            binding,
            viewport,
            hit_authority: empty_hit_authority(),
            artifact,
            receivers: Vec::new(),
            scroll_receivers: Vec::new(),
        }
    }

    fn artifact_commit_sequence() -> (
        SurfacePresentationArtifact,
        SurfacePresentationCommit,
        SurfacePresentationArtifact,
        SurfacePresentationCommit,
    ) {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("artifact commit layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("artifact commit session initializes");
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("artifact commit bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("artifact commit minimum validates"),
            80.0,
        )
        .expect("artifact commit metrics validate");
        let mut measured = session
            .begin_host_frame()
            .expect("artifact commit measurement begins");
        measured
            .measure_surface(SURFACE, metrics)
            .expect("artifact commit surface measures");
        measured
            .commit()
            .expect("artifact commit measurement publishes");

        let mut paint_and_report = |result| {
            let mut painted = session
                .begin_host_frame()
                .expect("artifact paint frame begins");
            let receipt = painted
                .paint_plan(SURFACE)
                .expect("artifact paint plan resolves")
                .expect("artifact surface is paintable")
                .execute_surface_program(|_| Ok::<(), ()>(()))
                .expect("artifact surface program executes");
            painted
                .confirm_surface_painted(receipt)
                .expect("artifact paint confirms");
            let mut report = painted.commit().expect("artifact paint publishes");
            let output = report
                .take_painted_outputs()
                .pop()
                .expect("artifact paint emits one output");
            let artifact = output.presentation_artifact();
            session
                .report_surface_presentation(output, result)
                .expect("artifact renderer result records");
            let mut settled = session
                .begin_host_frame()
                .expect("artifact settlement frame begins");
            settled
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .expect("artifact settlement retains the surface");
            let report = settled.commit().expect("artifact settlement commits");
            let [commit] = report.surface_presentation_commits() else {
                panic!("artifact settlement must emit one exact core commit")
            };
            (artifact, *commit)
        };

        let (first_artifact, first_commit) = paint_and_report(SurfacePresentationResult::Presented);
        let (second_artifact, second_commit) = paint_and_report(SurfacePresentationResult::Dropped);
        (first_artifact, first_commit, second_artifact, second_commit)
    }

    struct TestPanes;

    impl PaneView for TestPanes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Pane {}", item.get()).into())
        }

        fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
            ui.allocate_rect(ui.max_rect(), Sense::hover());
        }
    }

    struct UnclaimedPanes;

    impl PaneView for UnclaimedPanes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Pane {}", item.get()).into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
    }

    #[test]
    fn presentation_menu_hit_blocks_native_docking_receivers() {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("presentation-menu receiver layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("presentation-menu receiver session initializes");
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("surface minimum validates"),
            96.0,
        )
        .expect("surface metrics validate");
        let mut measured = session
            .begin_host_frame()
            .expect("presentation-menu measurement frame begins");
        measured
            .measure_surface(SURFACE, metrics)
            .expect("presentation-menu surface measures");
        measured
            .commit()
            .expect("presentation-menu measurement commits");

        let context = egui::Context::default();
        let mut panes = TestPanes;
        let mut anchor_point = None;
        let mut painted = None;
        let mut full_output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..RawInput::default()
            },
            |ui| {
                let mut frame = session
                    .begin_host_frame()
                    .expect("presentation-menu paint frame begins");
                let plan = frame
                    .paint_plan(SURFACE)
                    .expect("presentation-menu paint plan resolves")
                    .expect("presentation-menu paint plan is ready");
                let mut anchor = None;
                plan.visit_surface_program(|step| {
                    if let SurfacePaintProgramStep::Operation {
                        operation: SurfacePaintProgramOperation::PresentationMenuAnchor(record),
                        ..
                    } = step
                    {
                        assert!(anchor.replace(record).is_none());
                    }
                    Ok::<(), ()>(())
                })
                .expect("presentation-menu paint program resolves");
                let anchor = anchor.expect("main root publishes one presentation-menu anchor");
                let bounds = anchor.bounds();
                let center = LogicalPoint::new(
                    bounds.x() + bounds.width() * 0.5,
                    bounds.y() + bounds.height() * 0.5,
                )
                .expect("presentation-menu anchor center validates");
                anchor_point = logical_pos(center);
                let mut register_scroll_candidate = |ui: &Ui, rect: Rect, id: Id| {
                    let identity = ui.register_scroll_hit_candidate(rect, id);
                    (identity.id(), identity.layer_id())
                };
                let paint = native_support::paint_surface(
                    &mut frame,
                    Id::unique("native-presentation-menu"),
                    SURFACE,
                    ui,
                    &mut panes,
                    &DockStyle::default(),
                    &mut register_scroll_candidate,
                )
                .expect("native presentation-menu surface paints");
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .expect("presentation-menu paint frame settles");
                frame
                    .commit()
                    .expect("presentation-menu paint frame commits");
                painted = Some(paint);
            },
        );
        full_output.textures_delta.clear();

        let painted = painted.expect("presentation-menu paint result is retained");
        let hit = context
            .hit_test_last_pass(
                egui::ViewportId::ROOT,
                anchor_point.expect("presentation-menu anchor center is available"),
            )
            .expect("completed pass publishes one hit snapshot");
        assert_eq!(hit.cumulative_pass_nr(), painted.cumulative_pass_nr());
        let identity = hit
            .click()
            .expect("presentation-menu anchor wins click hit testing");
        assert!(
            painted.receivers().all(|receiver| {
                receiver.widget_id() != identity.id() || receiver.layer_id() != identity.layer_id()
            }),
            "the adapter-owned menu must remain outside the core docking receiver roster so the native resolver answers Blocked instead of clicking through",
        );
    }

    #[test]
    fn front_contained_chrome_blocks_a_background_tab_strip_control() {
        let main_root = RootId::new(1);
        let contained_root = RootId::new(2);
        let contained = FloatingPresentationId::new(1);
        let contained_rect =
            LogicalRect::new(0.0, 0.0, 640.0, 180.0).expect("contained receiver bounds validate");
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                main_root,
                DockspaceNode::central_tabs((1..=24).map(ItemId::new)),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            contained,
            DockspaceRootLayout::new(contained_root, DockspaceNode::tabs([ItemId::new(100)])),
            contained_rect,
        ))])
        .expect("overlapping receiver layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("overlapping receiver session initializes");
        let context = egui::Context::default();
        let instance_id = Id::unique("native-overlapping-tab-control");
        let mut panes = TestPanes;
        let style = DockStyle::default();
        let mut measured_output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..RawInput::default()
            },
            |ui| {
                let mut frame = session
                    .begin_host_frame()
                    .expect("overlapping receiver measurement frame begins");
                native_support::measure_surface(
                    &mut frame,
                    SURFACE,
                    ui,
                    ui.available_rect_before_wrap(),
                    ui.max_rect(),
                    &panes,
                    &style,
                )
                .expect("overlapping receiver surface measures");
                frame
                    .commit()
                    .expect("overlapping receiver measurement commits");
            },
        );
        measured_output.textures_delta.clear();
        let mut control_point = None;
        let mut background_control_id = None;
        let mut painted = None;
        let mut full_output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..RawInput::default()
            },
            |ui| {
                let mut frame = session
                    .begin_host_frame()
                    .expect("overlapping receiver paint frame begins");
                let plan = frame
                    .paint_plan(SURFACE)
                    .expect("overlapping receiver paint plan resolves")
                    .expect("overlapping receiver paint plan is ready");
                let mut control = None;
                plan.visit_surface_program(|step| {
                    if let SurfacePaintProgramStep::Operation {
                        operation: SurfacePaintProgramOperation::TabStripControl(record),
                        ..
                    } = step
                        && record.root() == main_root
                        && control.is_none()
                    {
                        control = Some(record);
                    }
                    Ok::<(), ()>(())
                })
                .expect("overlapping receiver paint program resolves");
                let control = control.expect("the overflowing main strip publishes one control");
                let hit_bounds = control.hit_bounds();
                let point = LogicalPoint::new(
                    hit_bounds.x() + hit_bounds.width() * 0.5,
                    hit_bounds.y() + hit_bounds.height() * 0.5,
                )
                .expect("background control center validates");
                assert!(
                    contained_rect.contains(point),
                    "the foreground contained window must cover the background control"
                );
                control_point = logical_pos(point);
                background_control_id = Some(ui.make_persistent_id((
                    instance_id,
                    "tab-strip-control",
                    control.visual_id(),
                )));
                let mut register_scroll_candidate = |ui: &Ui, rect: Rect, id: Id| {
                    let identity = ui.register_scroll_hit_candidate(rect, id);
                    (identity.id(), identity.layer_id())
                };
                let paint = native_support::paint_surface(
                    &mut frame,
                    instance_id,
                    SURFACE,
                    ui,
                    &mut panes,
                    &style,
                    &mut register_scroll_candidate,
                )
                .expect("overlapping native surface paints");
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .expect("overlapping receiver paint frame settles");
                frame
                    .commit()
                    .expect("overlapping receiver paint frame commits");
                painted = Some(paint);
            },
        );
        full_output.textures_delta.clear();

        let painted = painted.expect("overlapping receiver paint result is retained");
        let hit = context
            .hit_test_last_pass(
                egui::ViewportId::ROOT,
                control_point.expect("background control point is available"),
            )
            .expect("completed pass publishes one hit snapshot");
        assert_eq!(hit.cumulative_pass_nr(), painted.cumulative_pass_nr());
        assert_ne!(
            hit.click().map(|identity| identity.id()),
            background_control_id,
            "foreground contained chrome must win over the background strip control"
        );
    }

    #[test]
    fn committed_host_frame_promotes_receivers_without_follow_up_input() {
        use std::sync::Arc;

        use crate::host_frame::NativeHostFrame;
        use crate::mailbox::NativeHostBridge;
        use crate::viewport_map::NativeViewportMap;

        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("idle settlement layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("idle settlement session initializes");
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("surface minimum validates"),
            80.0,
        )
        .expect("surface metrics validate");
        let mut measurement = session.begin_host_frame().expect("measurement begins");
        measurement
            .measure_surface(SURFACE, metrics)
            .expect("surface measures");
        measurement.commit().expect("measurement commits");
        let mut paint = session.begin_host_frame().expect("paint begins");
        let execution = paint
            .paint_plan(SURFACE)
            .expect("paint plan resolves")
            .expect("surface is ready")
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("the complete program executes");
        paint
            .confirm_surface_painted(execution)
            .expect("complete paint confirms");
        let output = paint
            .commit()
            .expect("paint commits")
            .take_painted_outputs()
            .pop()
            .expect("one output is emitted");
        let artifact = output.presentation_artifact();
        let binding = binding(31);
        let mut receivers = NativeReceiverStore::default();
        receivers.finish_renderer_presented(
            Some(pending_receiver(binding, egui::ViewportId::ROOT, artifact)),
            egui::ViewportId::ROOT,
            binding,
        );
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the output owner reports presentation");

        let bridge = Arc::new(NativeHostBridge::new(NativeViewportMap::default()));
        let mut frame = NativeHostFrame::new(
            session.begin_host_frame().expect("settlement begins"),
            bridge,
            &mut receivers,
            None,
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("settlement does not repaint");
        frame
            .commit()
            .expect("the presentation observation commits");

        assert!(
            !receivers.has_pending_commit(),
            "a committed presentation must not require another input or repaint to promote receivers"
        );
        assert_eq!(receivers.presented[&SURFACE][0].artifact, artifact);
    }

    #[test]
    fn renderer_terminal_waits_for_matching_core_commit_before_atomic_promotion() {
        let binding = binding(31);
        let (first_artifact, first_commit, second_artifact, second_commit) =
            artifact_commit_sequence();
        let mut store = NativeReceiverStore::default();

        store.finish_renderer_presented(
            Some(pending_receiver(
                binding,
                egui::ViewportId::ROOT,
                first_artifact,
            )),
            egui::ViewportId::ROOT,
            binding,
        );
        assert!(store.presented.is_empty());
        assert_eq!(store.awaiting_core.len(), 1);

        store.commit_presentations(&[first_commit]);
        assert!(store.presented.get(&SURFACE).is_some_and(|entries| {
            entries
                .iter()
                .any(|entry| entry.artifact == first_artifact && entry.binding == binding)
        }));
        assert!(store.awaiting_core.is_empty());

        store.finish_renderer_presented(
            Some(pending_receiver(
                binding,
                egui::ViewportId::ROOT,
                second_artifact,
            )),
            egui::ViewportId::ROOT,
            binding,
        );
        assert!(
            store.presented.get(&SURFACE).is_some_and(|entries| entries
                .iter()
                .any(|entry| { entry.artifact == first_artifact && entry.binding == binding })),
            "physical presentation alone must preserve the previous core-committed artifact"
        );

        store.commit_presentations(&[second_commit]);
        assert!(!store.presented.contains_key(&SURFACE));
        assert!(store.awaiting_core.is_empty());
    }

    #[test]
    fn retiring_an_old_binding_preserves_the_same_surface_successor() {
        let predecessor = binding(11);
        let successor = binding(12);
        let mut store = NativeReceiverStore::default();
        store.presented.insert(
            SURFACE,
            vec![PresentedReceiverSet {
                binding: successor,
                viewport: egui::ViewportId::ROOT,
                hit_authority: empty_hit_authority(),
                artifact: presentation_artifact(),
                receivers: Vec::new(),
                scroll_receivers: Vec::new(),
            }],
        );

        store.retire_binding(predecessor);

        assert_eq!(
            store
                .presented
                .get(&SURFACE)
                .and_then(|entries| entries.first())
                .map(|entry| entry.binding),
            Some(successor)
        );
    }

    #[test]
    fn late_terminal_without_pending_token_preserves_the_successor() {
        let predecessor = binding(21);
        let successor = binding(22);
        let mut store = NativeReceiverStore::default();
        store.presented.insert(
            SURFACE,
            vec![PresentedReceiverSet {
                binding: successor,
                viewport: egui::ViewportId::ROOT,
                hit_authority: empty_hit_authority(),
                artifact: presentation_artifact(),
                receivers: Vec::new(),
                scroll_receivers: Vec::new(),
            }],
        );

        store.finish_renderer_presented(None, egui::ViewportId::ROOT, predecessor);

        assert_eq!(
            store
                .presented
                .get(&SURFACE)
                .and_then(|entries| entries.first())
                .map(|entry| entry.binding),
            Some(successor),
            "an A1 terminal without its retired pending token cannot clear A2 authority"
        );
    }

    #[test]
    fn mismatched_presented_candidate_preserves_the_presented_predecessor() {
        let binding = binding(41);
        let mut store = NativeReceiverStore::default();
        store.presented.insert(
            SURFACE,
            vec![PresentedReceiverSet {
                binding,
                viewport: egui::ViewportId::ROOT,
                hit_authority: empty_hit_authority(),
                artifact: presentation_artifact(),
                receivers: Vec::new(),
                scroll_receivers: Vec::new(),
            }],
        );

        store.finish_renderer_presented(
            Some(empty_pending_receiver(
                binding,
                egui::ViewportId::from_hash_of("mismatched-candidate"),
            )),
            egui::ViewportId::ROOT,
            binding,
        );

        assert!(store.references_binding(binding));
    }

    #[test]
    fn exact_registered_hit_clears_hover_lane_while_uncertain_hits_fail_closed() {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .expect("hover receiver layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("hover receiver session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("managed native host enrolls");
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .expect("managed native capabilities configure");
        session
            .register_native_root(SURFACE, HostWindowToken::new(102))
            .expect("native root registration queues");
        let mut registration = session
            .begin_host_frame()
            .expect("native root registration frame begins");
        registration
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native root registration frame settles");
        let registration = registration
            .commit()
            .expect("native root registration commits");
        let binding = registration
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .expect("native root registration emits one binding");

        let bounds =
            PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("native window bounds validate");
        let scale = ScaleFactor::new(1.0).expect("native scale validates");
        let facts = NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None)
            .with_close(NativeCloseState::Clear, None);
        session
            .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
            .expect("exact native root facts record");
        let mut snapshot = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("native snapshot frame begins");
        snapshot
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native snapshot frame settles");
        snapshot.commit().expect("native snapshot commits");

        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("surface minimum validates"),
            96.0,
        )
        .expect("surface metrics validate");
        let mut measured = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("native measurement frame begins");
        measured
            .measure_surface(SURFACE, metrics)
            .expect("native surface measures");
        measured.commit().expect("native measurement commits");

        let context = egui::Context::default();
        let mut panes = UnclaimedPanes;
        let mut painted_semantic_output = None;
        let mut tab_point = None;
        let mut pane_point = None;
        let mut receivers = Vec::new();
        let mut painted_output = None;
        let mut full_output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..RawInput::default()
            },
            |ui| {
                let mut frame = session
                    .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                    .expect("native paint frame begins");
                let plan = frame
                    .paint_plan(SURFACE)
                    .expect("native paint plan resolves")
                    .expect("native paint plan is ready");
                tab_point = plan
                    .tab_receiver(ItemId::new(1))
                    .map(|receiver| receiver.center());
                let mut pane = None;
                plan.visit_surface_program(|step| {
                    if let SurfacePaintProgramStep::Operation {
                        operation: SurfacePaintProgramOperation::Pane(record),
                        ..
                    } = step
                    {
                        assert!(pane.replace(record).is_none());
                    }
                    Ok::<(), ()>(())
                })
                .expect("native unclaimed-hover paint program resolves");
                pane_point = pane.and_then(|pane| {
                    let bounds = plan.receiver_for_pane(pane)?.bounds();
                    LogicalPoint::new(
                        bounds.x() + bounds.width() * 0.15,
                        bounds.y() + bounds.height() * 0.2,
                    )
                    .ok()
                });
                let mut register_scroll_candidate = |ui: &Ui, rect: Rect, id: Id| {
                    let identity = ui.register_scroll_hit_candidate(rect, id);
                    (identity.id(), identity.layer_id())
                };
                let mut painted = native_support::paint_surface(
                    &mut frame,
                    Id::unique("native-unclaimed-hover-receiver"),
                    SURFACE,
                    ui,
                    &mut panes,
                    &DockStyle::default(),
                    &mut register_scroll_candidate,
                )
                .expect("native surface paints");
                painted_semantic_output = painted.semantic_output();
                receivers = painted.receivers().collect();
                assert!(
                    painted
                        .confirm_into(&mut frame)
                        .expect("native surface paint confirms"),
                    "the production renderer completes the native surface program"
                );
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .expect("native paint frame settles");
                let mut report = frame.commit().expect("native paint frame commits");
                painted_output = report.take_painted_outputs().pop();
            },
        );
        full_output.textures_delta.clear();
        let presented_semantic_output =
            painted_semantic_output.expect("ready paint has one semantic output");
        let output = painted_output.expect("confirmed native paint emits one output");
        assert!(output.matches_semantic_output(presented_semantic_output));
        let presented_artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("native presentation records");
        let mut presented = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("native presentation frame begins");
        presented
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native presentation frame settles");
        presented.commit().expect("native presentation commits");

        let previous_artifact = presented_artifact;
        let mut repeated = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the unchanged surface begins another emission");
        let repeated_plan = repeated
            .paint_plan(SURFACE)
            .expect("the repeated plan resolves")
            .expect("the unchanged surface remains ready");
        assert_eq!(repeated_plan.semantic_output(), presented_semantic_output);
        let receipt = repeated_plan
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("the unchanged owned surface program is accepted again");
        repeated
            .confirm_surface_painted(receipt)
            .expect("the repeated emission confirms");
        let repeated_output = repeated
            .commit()
            .expect("the repeated emission commits")
            .take_painted_outputs()
            .pop()
            .expect("the repeated emission produces one output");
        assert!(repeated_output.matches_semantic_output(presented_semantic_output));
        let presented_artifact = repeated_output.presentation_artifact();
        assert_ne!(previous_artifact, presented_artifact);
        session
            .report_surface_presentation(repeated_output, SurfacePresentationResult::Presented)
            .expect("the newer identical semantic output presents");
        let mut repeated_observation = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the repeated presentation observation begins");
        repeated_observation
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the repeated observation settles");
        repeated_observation
            .commit()
            .expect("the newer presentation authority commits");

        let hit_authority = context
            .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
            .expect("the presented pass publishes immutable hit authority");
        let pane_point = pane_point.expect("the ready surface paints one pane receiver");
        let pane_identity = hit_authority
            .hit_test(logical_pos(pane_point).expect("pane point projects into egui space"))
            .and_then(|hit| hit.contains_pointer())
            .expect("the pane body owns the exact pointer identity");
        let matching = receivers
            .iter()
            .copied()
            .filter(|receiver| {
                receiver.viewport_id() == egui::ViewportId::ROOT
                    && receiver.cumulative_pass_nr() == hit_authority.cumulative_pass_nr()
                    && receiver.widget_id() == pane_identity.id()
                    && receiver.layer_id() == pane_identity.layer_id()
            })
            .collect::<Vec<_>>();
        let [pane_receiver] = matching.as_slice() else {
            panic!("the pane hit must map to one exact receiver, got {matching:?}")
        };
        assert_eq!(pane_receiver.role(), DockspaceReceiverRole::PaneBody);
        let tab_receiver = receivers
            .iter()
            .copied()
            .find(|receiver| receiver.role() == DockspaceReceiverRole::TabBody)
            .expect("the ready surface paints one tab-body receiver");

        let presented_set = |receivers: Vec<NativePaintReceiver>, artifact| {
            let mut store = NativeReceiverStore::default();
            store.presented.insert(
                SURFACE,
                vec![PresentedReceiverSet {
                    binding,
                    viewport: egui::ViewportId::ROOT,
                    hit_authority: hit_authority.clone(),
                    artifact,
                    receivers,
                    scroll_receivers: Vec::new(),
                }],
            );
            store
        };
        let exact_store = presented_set(receivers.clone(), presented_artifact);
        let blocked_store = presented_set(
            receivers
                .iter()
                .copied()
                .filter(|receiver| {
                    receiver.widget_id() != pane_receiver.widget_id()
                        || receiver.layer_id() != pane_receiver.layer_id()
                })
                .collect(),
            presented_artifact,
        );
        let mut duplicate_receivers = receivers;
        duplicate_receivers.push(*pane_receiver);
        let duplicate_store = presented_set(duplicate_receivers, presented_artifact);
        let mismatched_artifact = presentation_artifact();
        assert_ne!(mismatched_artifact, presented_artifact);
        let mismatched_store = presented_set(vec![*pane_receiver], mismatched_artifact);
        let obsolete_store = presented_set(vec![*pane_receiver], previous_artifact);

        let tab_point = tab_point.expect("the ready surface paints one tab receiver");
        session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(
                        PhysicalPoint::new(tab_point.x(), tab_point.y())
                            .expect("native tab point validates"),
                    ),
                    NativePointerHover::Dock(binding),
                    None,
                ),
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            ))
            .expect("native press edge records");
        let mut press = session
            .begin_native_host_frame(|query| {
                assert!(tab_receiver.clear_hover_lane_for_query(query).is_none());
                exact_store.resolve(query)
            })
            .expect("native press frame begins");
        press
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native press frame settles");
        press.commit().expect("native press frame commits");

        session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::Moved,
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(
                        PhysicalPoint::new(pane_point.x(), pane_point.y())
                            .expect("native pane point validates"),
                    ),
                    NativePointerHover::Dock(binding),
                    None,
                ),
                NativePointerOwner::Native(binding),
                NativePointerOwner::Native(binding),
            ))
            .expect("native move edge records");
        let mut hover_resolved = false;
        let mut drag = session
            .begin_native_host_frame(|query| {
                let answer = exact_store.resolve(query);
                if matches!(query.purpose(), NativeReceiverPurpose::HoverHit)
                    && query.point() == Some(pane_point)
                {
                    let presented = query.presented_surface();
                    assert!(query.matches_semantic_output(presented_semantic_output));
                    assert!(query.matches_presentation_artifact(presented_artifact));
                    assert!(!query.matches_presentation_artifact(previous_artifact));
                    assert_eq!(answer, NativeReceiverAnswer::ClearHoverLane(presented));
                    assert_eq!(
                        resolve_matching_receiver(query, tab_receiver),
                        NativeReceiverAnswer::Unknown,
                        "an out-of-bounds tab-body descriptor cannot clear the pane hover lane"
                    );
                    assert_eq!(
                        blocked_store.resolve(query),
                        NativeReceiverAnswer::Blocked(presented)
                    );
                    assert_eq!(
                        duplicate_store.resolve(query),
                        NativeReceiverAnswer::Unknown
                    );
                    assert_eq!(
                        mismatched_store.resolve(query),
                        NativeReceiverAnswer::Unknown
                    );
                    assert_eq!(
                        obsolete_store.resolve(query),
                        NativeReceiverAnswer::Unknown,
                        "an earlier emission cannot reuse authority merely because its scene is unchanged"
                    );
                    hover_resolved = true;
                }
                answer
            })
            .expect("native drag frame begins");
        drag.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native drag frame settles");
        drag.commit().expect("native drag frame commits");
        assert!(
            hover_resolved,
            "the native drag asks one exact hover question at the pane ingress point"
        );
    }

    #[test]
    fn presented_scroll_candidate_answers_the_native_delivery_query() {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs((1..=24).map(ItemId::new)),
            ),
        )])
        .expect("scroll receiver layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("scroll receiver session initializes");
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("managed native host enrolls");
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .expect("managed native capabilities configure");
        session
            .register_native_root(SURFACE, HostWindowToken::new(101))
            .expect("native root registration queues");
        let mut registration = session
            .begin_host_frame()
            .expect("native root registration frame begins");
        registration
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native root registration frame settles");
        let registration = registration
            .commit()
            .expect("native root registration commits");
        let binding = registration
            .inputs()
            .iter()
            .find_map(|outcome| match outcome {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .expect("native root registration emits one binding");

        let bounds =
            PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("native window bounds validate");
        let scale = ScaleFactor::new(1.0).expect("native scale validates");
        let facts = NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None)
            .with_close(NativeCloseState::Clear, None);
        session
            .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
            .expect("exact native root facts record");
        let mut snapshot = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("native snapshot frame begins");
        snapshot
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native snapshot frame settles");
        snapshot.commit().expect("native snapshot commits");

        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds validate"),
            LogicalSize::new(32.0, 24.0).expect("surface minimum validates"),
            96.0,
        )
        .expect("surface metrics validate");
        let mut measured = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("native measurement frame begins");
        measured
            .measure_surface(SURFACE, metrics)
            .expect("native surface measures");
        measured.commit().expect("native measurement commits");

        let context = egui::Context::default();
        let mut panes = TestPanes;
        let mut semantic_output = None;
        let mut receiver_point = None;
        let mut scroll_receivers = Vec::new();
        let mut scroll_identities = Vec::new();
        let mut painted_output = None;
        let mut full_output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..RawInput::default()
            },
            |ui| {
                let mut frame = session
                    .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                    .expect("native paint frame begins");
                let plan = frame
                    .paint_plan(SURFACE)
                    .expect("native paint plan resolves")
                    .expect("native paint plan is ready");
                let mut bar = None;
                plan.visit_surface_program(|step| {
                    if let SurfacePaintProgramStep::Operation {
                        operation: SurfacePaintProgramOperation::TabBar(record),
                        ..
                    } = step
                    {
                        assert!(bar.replace(record).is_none());
                    }
                    Ok::<(), ()>(())
                })
                .expect("native scroll paint program resolves");
                receiver_point = bar.and_then(|bar| {
                    plan.receiver_for_tab_strip_scroll(bar)
                        .map(|receiver| receiver.center())
                });
                let mut register_scroll_candidate = |ui: &Ui, rect: Rect, id: Id| {
                    let identity = ui.register_scroll_hit_candidate(rect, id);
                    scroll_identities.push(identity);
                    (identity.id(), identity.layer_id())
                };
                let mut painted = native_support::paint_surface(
                    &mut frame,
                    Id::unique("native-scroll-receiver"),
                    SURFACE,
                    ui,
                    &mut panes,
                    &DockStyle::default(),
                    &mut register_scroll_candidate,
                )
                .expect("native surface paints");
                semantic_output = painted.semantic_output();
                scroll_receivers = painted.scroll_receivers().collect();
                assert!(
                    painted
                        .confirm_into(&mut frame)
                        .expect("native surface paint confirms"),
                    "the production renderer completes the native scroll program"
                );
                frame
                    .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                    .expect("native paint frame settles");
                let mut report = frame.commit().expect("native paint frame commits");
                painted_output = report.take_painted_outputs().pop();
            },
        );
        full_output.textures_delta.clear();
        let semantic_output = semantic_output.expect("ready paint has one semantic output");
        let output = painted_output.expect("confirmed native paint emits one output");
        assert!(output.matches_semantic_output(semantic_output));
        let presentation_artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("native presentation records");
        let mut presented = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("native presentation frame begins");
        presented
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native presentation frame settles");
        presented.commit().expect("native presentation commits");

        assert_eq!(scroll_receivers.len(), scroll_identities.len());
        let scroll_receivers = scroll_receivers
            .into_iter()
            .zip(scroll_identities)
            .map(|(receiver, identity)| PresentedScrollReceiver { receiver, identity })
            .collect::<Vec<_>>();
        let expected = scroll_receivers
            .iter()
            .copied()
            .find(|candidate| {
                candidate.receiver.role()
                    == dockspace::runtime::DockspaceReceiverRole::TabStripScroll
            })
            .expect("the overflowing tab strip paints one scroll receiver");
        let hit_authority = context
            .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
            .expect("the presented pass publishes immutable hit authority");
        assert_eq!(
            hit_authority.cumulative_pass_nr(),
            expected.receiver.cumulative_pass_nr()
        );
        let presented_pass_nr = hit_authority.cumulative_pass_nr();
        let expected_identity = expected.identity;
        let mut store = NativeReceiverStore::default();
        store.presented.insert(
            SURFACE,
            vec![PresentedReceiverSet {
                binding,
                viewport: egui::ViewportId::ROOT,
                hit_authority,
                artifact: presentation_artifact,
                receivers: Vec::new(),
                scroll_receivers,
            }],
        );

        let point = receiver_point.expect("the overflowing tab strip has receiver geometry");
        let mut later_output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    Vec2::new(640.0, 480.0),
                )),
                ..RawInput::default()
            },
            |ui| {
                ui.interact(
                    ui.max_rect(),
                    Id::unique("later-unpresented-native-blocker"),
                    Sense::click_and_drag(),
                );
            },
        );
        later_output.textures_delta.clear();
        let later_authority = context
            .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
            .expect("the later unpresented pass publishes separate hit authority");
        assert!(later_authority.cumulative_pass_nr() > presented_pass_nr);
        let projected_delta = egui::WidgetScrollDelta::new(0.0, -1.0)
            .expect("the test scroll delta is finite and non-zero");
        assert_ne!(
            later_authority
                .scroll_hit_test(
                    logical_pos(point).expect("receiver point projects into egui space"),
                    &[expected_identity],
                    egui::WidgetScrollHitChallenge::Spatial {
                        projected_delta: Some(projected_delta),
                    },
                )
                .expect("the later pass answers the finite scroll query")
                .hit(),
            egui::WidgetScrollHit::Candidate(expected_identity),
            "an unpresented later pass must not alias the presented scroll receiver"
        );
        session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(1),
                NativePointerEvent::Scrolled(NativeScrollEvent::new(
                    NativeScrollDeviceId::new(1),
                    NativeScrollPhase::Discrete {
                        delta: NativeScrollDelta::Lines { x: 0.0, y: -1.0 },
                    },
                    NativeScrollMomentum::Unknown,
                    NativeScrollModifiers::Exact {
                        shift: false,
                        control: false,
                        alt: false,
                        command: false,
                    },
                )),
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(
                        PhysicalPoint::new(point.x(), point.y())
                            .expect("receiver desktop point validates"),
                    ),
                    NativePointerHover::Dock(binding),
                    None,
                ),
                NativePointerOwner::Native(binding),
                NativePointerOwner::None,
            ))
            .expect("native scroll edge records");
        let mut resolved = false;
        let mut scroll = session
            .begin_native_host_frame(|query| {
                let answer = store.resolve(query);
                if matches!(query.purpose(), NativeReceiverPurpose::ScrollDelivery(_)) {
                    let expected = expected
                        .receiver
                        .bind_for_query(query)
                        .expect("the exact scroll descriptor binds to its core query");
                    assert_eq!(answer, NativeReceiverAnswer::Dock(expected));
                    resolved = true;
                }
                answer
            })
            .expect("native scroll frame begins");
        scroll
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("native scroll frame settles");
        scroll.commit().expect("native scroll frame commits");
        assert!(
            resolved,
            "the native edge asks one scroll delivery question"
        );
    }
}
