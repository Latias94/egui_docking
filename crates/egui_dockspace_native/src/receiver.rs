//! Final-presentation-qualified egui receiver bindings.

use std::collections::BTreeMap;

use dockspace::geometry::LogicalPoint;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceSemanticOutput, NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeScrollReceiverChallenge, NativeSurfaceBinding, PaintedSurfaceOutput,
};
use eframe::{NativeOutputToken, egui};
use egui_dockspace::native_support::{NativePaintReceiver, NativeScrollPaintReceiver};

use crate::host_frame::NativeSurfacePaint;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativeReceiverStageError {
    ViewportMismatch,
    BindingMismatch,
    SemanticOutputMismatch,
    PassRegressed,
}

#[derive(Debug)]
struct PendingReceiverSet {
    surface: SurfaceId,
    binding: NativeSurfaceBinding,
    viewport: egui::ViewportId,
    cumulative_pass_nr: u64,
    semantic_output: DockspaceSemanticOutput,
    receivers: Vec<NativePaintReceiver>,
    scroll_receivers: Vec<PresentedScrollReceiver>,
}

impl PendingReceiverSet {
    fn from_paint(
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
            cumulative_pass_nr,
            semantic_output,
            receivers,
            scroll_receivers,
        })
    }
}

#[derive(Debug)]
struct PresentedReceiverSet {
    binding: NativeSurfaceBinding,
    viewport: egui::ViewportId,
    cumulative_pass_nr: u64,
    semantic_output: DockspaceSemanticOutput,
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
    presented: BTreeMap<SurfaceId, PresentedReceiverSet>,
}

impl NativeReceiverStore {
    pub(crate) fn stage(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
        output: &PaintedSurfaceOutput,
        paint: &NativeSurfacePaint,
    ) -> Result<(), NativeReceiverStageError> {
        let next = PendingReceiverSet::from_paint(token, binding, output, paint)?;
        if let Some(current) = self.pending.get(&token)
            && (current.surface != next.surface
                || current.binding != next.binding
                || current.viewport != next.viewport
                || current.semantic_output != next.semantic_output
                || next.cumulative_pass_nr < current.cumulative_pass_nr)
        {
            return Err(NativeReceiverStageError::PassRegressed);
        }
        self.pending.insert(token, next);
        Ok(())
    }

    pub(crate) fn presented(&mut self, token: NativeOutputToken, binding: NativeSurfaceBinding) {
        let pending = self.pending.remove(&token);
        self.finish_presented(pending, token.viewport_id(), binding);
    }

    fn finish_presented(
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
            self.discard_presented_if_not_newer(&pending);
            return;
        }
        if self.presented.get(&pending.surface).is_some_and(|current| {
            current.binding == binding
                && current.viewport == pending.viewport
                && current.cumulative_pass_nr > pending.cumulative_pass_nr
        }) {
            return;
        }
        self.presented.insert(
            pending.surface,
            PresentedReceiverSet {
                binding,
                viewport: pending.viewport,
                cumulative_pass_nr: pending.cumulative_pass_nr,
                semantic_output: pending.semantic_output,
                receivers: pending.receivers,
                scroll_receivers: pending.scroll_receivers,
            },
        );
    }

    pub(crate) fn dropped(&mut self, token: NativeOutputToken) {
        let pending = self.pending.remove(&token);
        self.finish_dropped(pending);
    }

    fn finish_dropped(&mut self, pending: Option<PendingReceiverSet>) {
        if let Some(pending) = pending {
            self.discard_presented_if_not_newer(&pending);
        }
    }

    pub(crate) fn abandon(&mut self, token: NativeOutputToken) {
        if let Some(pending) = self.pending.remove(&token) {
            self.discard_presented_if_not_newer(&pending);
        }
    }

    pub(crate) fn retire_binding(&mut self, binding: NativeSurfaceBinding) {
        let surface = binding.surface();
        self.pending.retain(|_, pending| pending.binding != binding);
        if self
            .presented
            .get(&surface)
            .is_some_and(|presented| presented.binding == binding)
        {
            self.presented.remove(&surface);
        }
    }

    pub(crate) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.pending
            .values()
            .any(|pending| pending.binding == binding)
            || self
                .presented
                .get(&binding.surface())
                .is_some_and(|presented| presented.binding == binding)
    }

    pub(crate) fn resolve(
        &self,
        context: &egui::Context,
        query: NativeReceiverQuery,
    ) -> NativeReceiverAnswer {
        let Some(presented) = self.presented.get(&query.surface()) else {
            return NativeReceiverAnswer::Unknown;
        };
        if !query.matches_native_binding(presented.binding) {
            return NativeReceiverAnswer::Unknown;
        }
        if !query.matches_semantic_output(presented.semantic_output) {
            return NativeReceiverAnswer::Unknown;
        }
        let Some(point) = query.point().and_then(logical_pos) else {
            return NativeReceiverAnswer::Unknown;
        };
        if let NativeReceiverPurpose::ScrollDelivery(challenge) = query.purpose() {
            return resolve_scroll(context, query, presented, point, *challenge);
        }
        let Some(hit) = context.hit_test_last_pass(presented.viewport, point) else {
            return NativeReceiverAnswer::Unknown;
        };
        if hit.cumulative_pass_nr() != presented.cumulative_pass_nr {
            return NativeReceiverAnswer::Unknown;
        }
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
                && receiver.cumulative_pass_nr() == presented.cumulative_pass_nr
                && receiver.widget_id() == identity.id()
                && receiver.layer_id() == identity.layer_id()
        });
        let Some(receiver) = matches.next() else {
            return NativeReceiverAnswer::Blocked(query.presented_surface());
        };
        if matches.next().is_some() {
            return NativeReceiverAnswer::Unknown;
        }
        receiver
            .bind_for_query(query)
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
    }

    fn discard_presented_if_not_newer(&mut self, pending: &PendingReceiverSet) {
        let should_remove = self.presented.get(&pending.surface).is_some_and(|current| {
            current.binding == pending.binding
                && (current.viewport != pending.viewport
                    || current.cumulative_pass_nr <= pending.cumulative_pass_nr)
        });
        if should_remove {
            self.presented.remove(&pending.surface);
        }
    }
}

fn resolve_scroll(
    context: &egui::Context,
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
    let Some(hit) = context.scroll_hit_test_last_pass(
        presented.viewport,
        point,
        candidates.as_slice(),
        challenge,
    ) else {
        return NativeReceiverAnswer::Unknown;
    };
    if hit.cumulative_pass_nr() != presented.cumulative_pass_nr {
        return NativeReceiverAnswer::Unknown;
    }
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
                        && candidate.receiver.cumulative_pass_nr() == presented.cumulative_pass_nr
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
        DockspaceSession, HostInputOutcome, HostWindowToken, NativeCloseState,
        NativeDesktopPointerLocation, NativeDesktopPosition, NativePointerEvent,
        NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
        NativePointerRoster, NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent,
        NativeScrollModifiers, NativeScrollMomentum, NativeScrollPhase, NativeWindowFacts,
        NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
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

    fn semantic_output() -> DockspaceSemanticOutput {
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
        frame
            .paint_plan(SURFACE)
            .expect("semantic-output plan lookup succeeds")
            .expect("semantic-output plan is ready")
            .semantic_output()
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
                let anchor = plan
                    .presentation_menu_anchors()
                    .next()
                    .expect("main root publishes one presentation-menu anchor");
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
                    Id::new("native-presentation-menu"),
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
        let instance_id = Id::new("native-overlapping-tab-control");
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
                let control = plan
                    .tab_strip_controls()
                    .find(|control| control.root() == main_root)
                    .expect("the overflowing main strip publishes one control");
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
    fn retiring_an_old_binding_preserves_the_same_surface_successor() {
        let predecessor = binding(11);
        let successor = binding(12);
        let mut store = NativeReceiverStore::default();
        store.presented.insert(
            SURFACE,
            PresentedReceiverSet {
                binding: successor,
                viewport: egui::ViewportId::ROOT,
                cumulative_pass_nr: 7,
                semantic_output: semantic_output(),
                receivers: Vec::new(),
                scroll_receivers: Vec::new(),
            },
        );

        store.retire_binding(predecessor);

        assert_eq!(
            store.presented.get(&SURFACE).map(|entry| entry.binding),
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
            PresentedReceiverSet {
                binding: successor,
                viewport: egui::ViewportId::ROOT,
                cumulative_pass_nr: 9,
                semantic_output: semantic_output(),
                receivers: Vec::new(),
                scroll_receivers: Vec::new(),
            },
        );

        store.finish_presented(None, egui::ViewportId::ROOT, predecessor);
        store.finish_dropped(None);

        assert_eq!(
            store.presented.get(&SURFACE).map(|entry| entry.binding),
            Some(successor),
            "an A1 terminal without its retired pending token cannot clear A2 authority"
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
                receiver_point = plan.tab_bars().find_map(|bar| {
                    plan.receiver_for_tab_strip_scroll(bar)
                        .map(|receiver| receiver.center())
                });
                let mut register_scroll_candidate = |ui: &Ui, rect: Rect, id: Id| {
                    let identity = ui.register_scroll_hit_candidate(rect, id);
                    scroll_identities.push(identity);
                    (identity.id(), identity.layer_id())
                };
                let painted = native_support::paint_surface(
                    &mut frame,
                    Id::new("native-scroll-receiver"),
                    SURFACE,
                    ui,
                    &mut panes,
                    &DockStyle::default(),
                    &mut register_scroll_candidate,
                )
                .expect("native surface paints");
                semantic_output = painted.semantic_output();
                scroll_receivers = painted.scroll_receivers().collect();
                frame
                    .confirm_surface_painted(SURFACE)
                    .expect("native surface paint confirms");
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
        let mut store = NativeReceiverStore::default();
        store.presented.insert(
            SURFACE,
            PresentedReceiverSet {
                binding,
                viewport: egui::ViewportId::ROOT,
                cumulative_pass_nr: expected.receiver.cumulative_pass_nr(),
                semantic_output,
                receivers: Vec::new(),
                scroll_receivers,
            },
        );

        let point = receiver_point.expect("the overflowing tab strip has receiver geometry");
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
                let answer = store.resolve(&context, query);
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
