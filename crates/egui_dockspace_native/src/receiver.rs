//! Final-presentation-qualified egui receiver bindings.

use std::collections::BTreeMap;

use dockspace::geometry::LogicalPoint;
use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceSemanticOutput, NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeSurfaceBinding, PaintedSurfaceOutput,
};
use eframe::{NativeOutputToken, egui};
use egui_dockspace::native_support::{NativePaintReceiver, NativeSurfacePaint};

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
        Ok(Self {
            surface: paint.surface(),
            binding,
            viewport,
            cumulative_pass_nr,
            semantic_output,
            receivers,
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

    pub(crate) fn presented(
        &mut self,
        token: NativeOutputToken,
        binding: NativeSurfaceBinding,
    ) {
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
        if matches!(query.purpose(), NativeReceiverPurpose::ScrollDelivery(_)) {
            return NativeReceiverAnswer::Unknown;
        }
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
            NativeReceiverPurpose::ScrollDelivery(_) => unreachable!("scroll returned above"),
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
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativePointerRoster,
        SurfaceUnavailableReason, UniformSurfaceMetrics,
    };

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
}
