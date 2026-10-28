//! Native host-frame adapter over the renderer-neutral session frame.

use std::collections::BTreeMap;
use std::sync::Arc;

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSubmittedAction, HostFrameReport, HostInputOutcome,
    NativeStagingPaintRequest, PreparedDockAction, PreparedPaneFocusObservation,
    PreparedSurfaceAction, SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};

use crate::error::NativeHostProtocolError;
use crate::error::NativeRuntimeError;
use crate::mailbox::NativeHostBridge;
use crate::pointer_event::NativePointerTranslator;
use crate::receiver::NativeReceiverStore;

/// Final-pass paint metadata plus fork-owned scroll identities from that same pass.
pub(crate) struct NativeSurfacePaint {
    inner: egui_dockspace::native_support::NativeSurfacePaint,
    scroll_identities: Vec<egui::WidgetHitIdentity>,
}

impl std::ops::Deref for NativeSurfacePaint {
    type Target = egui_dockspace::native_support::NativeSurfacePaint;

    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl std::ops::DerefMut for NativeSurfacePaint {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl NativeSurfacePaint {
    pub(crate) fn scroll_identities(&self) -> &[egui::WidgetHitIdentity] {
        &self.scroll_identities
    }
}

/// Affine native host frame which commits the output barrier and only then
/// releases later callback records.
pub(crate) struct NativeHostFrame<'session> {
    pub(crate) frame: DockspaceHostFrame<'session>,
    bridge: Arc<NativeHostBridge>,
    receivers: &'session mut NativeReceiverStore,
    repaint_context: Option<egui::Context>,
    overload: Option<OverloadCancellation<'session>>,
}

struct OverloadCancellation<'session> {
    boundary: u64,
    submitted: DockspaceSubmittedAction,
    translator: &'session mut NativePointerTranslator,
}

impl std::fmt::Debug for NativeHostFrame<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeHostFrame")
            .finish_non_exhaustive()
    }
}

impl<'session> NativeHostFrame<'session> {
    pub(crate) fn native_input_pending(&self) -> bool {
        self.overload.is_some()
            || !self.bridge.permits_unscheduled_input()
            || self.frame.native_input_pending()
            || self.bridge.has_unsubmitted_output_prefix()
    }

    pub(crate) fn new(
        frame: DockspaceHostFrame<'session>,
        bridge: Arc<NativeHostBridge>,
        receivers: &'session mut NativeReceiverStore,
        repaint_context: Option<egui::Context>,
    ) -> Self {
        NativeHostFrame {
            frame,
            bridge,
            receivers,
            repaint_context,
            overload: None,
        }
    }

    pub(crate) fn cancel_for_overload(
        &mut self,
        boundary: u64,
        translator: &'session mut NativePointerTranslator,
    ) -> Result<(), NativeRuntimeError> {
        let submitted = self.frame.cancel_native_input_for_overload()?;
        self.overload = Some(OverloadCancellation {
            boundary,
            submitted,
            translator,
        });
        Ok(())
    }

    pub(crate) fn complete_unpainted_surfaces(
        &mut self,
        reason: SurfaceUnavailableReason,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .complete_unpainted_surfaces(reason)
            .map_err(Into::into)
    }

    pub(crate) fn defer_surface(
        &mut self,
        surface: SurfaceId,
        reason: SurfaceUnavailableReason,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .defer_surface(surface, reason)
            .map_err(Into::into)
    }

    pub(crate) fn paint_surface(
        &mut self,
        instance_id: egui::Id,
        surface: SurfaceId,
        ui: &mut egui::Ui,
        panes: &mut dyn PaneView,
        style: &DockStyle,
    ) -> Result<NativeSurfacePaint, NativeRuntimeError> {
        let mut scroll_identities = Vec::new();
        let mut register_scroll_candidate = |ui: &egui::Ui, rect: egui::Rect, id: egui::Id| {
            let identity = ui.register_scroll_hit_candidate(rect, id);
            scroll_identities.push(identity);
            (identity.id(), identity.layer_id())
        };
        let paint = egui_dockspace::native_support::paint_surface(
            &mut self.frame,
            instance_id,
            surface,
            ui,
            panes,
            style,
            &mut register_scroll_candidate,
        )
        .map_err(NativeRuntimeError::from)?;
        let receiver_count = paint.scroll_receivers().len();
        if receiver_count != scroll_identities.len()
            || !paint
                .scroll_receivers()
                .zip(scroll_identities.iter().copied())
                .all(|(receiver, identity)| {
                    receiver.widget_id() == identity.id()
                        && receiver.layer_id() == identity.layer_id()
                })
        {
            return Err(NativeHostProtocolError::ScrollReceiverIdentityMismatch.into());
        }
        Ok(NativeSurfacePaint {
            inner: paint,
            scroll_identities,
        })
    }

    pub(crate) fn measure_surface(
        &mut self,
        surface: SurfaceId,
        ui: &egui::Ui,
        dock_rect: egui::Rect,
        popup_rect: egui::Rect,
        panes: &dyn PaneView,
        style: &DockStyle,
    ) -> Result<(), NativeRuntimeError> {
        egui_dockspace::native_support::measure_surface(
            &mut self.frame,
            surface,
            ui,
            dock_rect,
            popup_rect,
            panes,
            style,
        )
        .map(|_| ())
        .map_err(Into::into)
    }

    pub(crate) fn submit_surface_action(
        &mut self,
        action: PreparedSurfaceAction,
    ) -> Result<(), NativeRuntimeError> {
        self.frame.submit_surface_action(action).map_err(Into::into)
    }

    pub(crate) fn submit_pane_focus_observation(
        &mut self,
        observation: PreparedPaneFocusObservation,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .submit_pane_focus_observation(observation)
            .map_err(Into::into)
    }

    pub(crate) fn submit_prepared_action(
        &mut self,
        action: PreparedDockAction,
    ) -> Result<DockspaceSubmittedAction, NativeRuntimeError> {
        self.frame
            .submit_prepared_action(action)
            .map_err(Into::into)
    }

    /// Commits the core frame and releases all presentation records included in
    /// that committed causal boundary.
    ///
    /// # Errors
    ///
    /// Returns an error without releasing the callback-order barrier when the
    /// core rejects the candidate frame.
    pub fn commit(self) -> Result<HostFrameReport, NativeRuntimeError> {
        let staging_requests = self.compile_staging_requests()?;
        let Self {
            frame,
            bridge,
            receivers,
            repaint_context,
            overload,
        } = self;
        let report = frame.commit()?;
        if let Some(cancellation) = overload {
            if !matches!(
                report.submitted_action_outcome(cancellation.submitted),
                Some(&HostInputOutcome::NativeInputCancelledForOverload)
            ) || !bridge.finish_overload_cancellation(cancellation.boundary)
            {
                return Err(NativeHostProtocolError::NativeOverloadCancellationChanged.into());
            }
            cancellation.translator.finish_overload_cancellation();
        }
        receivers.commit_presentations(report.surface_presentation_commits());
        let staging_viewports = bridge.publish_staging_requests(staging_requests)?;
        bridge.commit_frame_boundary(report.native_input_pending());
        if let Some(context) = repaint_context {
            let ready_semantic_viewports = bridge.take_ready_semantic_viewports();
            request_native_viewport_repaints(
                &context,
                &staging_viewports,
                &ready_semantic_viewports,
            );
        }
        Ok(report)
    }

    fn compile_staging_requests(
        &self,
    ) -> Result<BTreeMap<egui::ViewportId, NativeStagingPaintRequest>, NativeRuntimeError> {
        if self.overload.is_some() {
            return Ok(BTreeMap::new());
        }
        self.bridge
            .compile_staging_requests(self.frame.native_staging_paints())
            .map_err(Into::into)
    }
}

fn request_native_viewport_repaints(
    context: &egui::Context,
    staging_viewports: &[egui::ViewportId],
    ready_semantic_viewports: &[egui::ViewportId],
) {
    for &viewport in staging_viewports {
        // A staging request is a new durable paint obligation. It must survive
        // an already queued creation/stabilization pass for the same hidden
        // viewport, so arm egui's one extra repaint instead of coalescing into
        // the pass which is already in flight.
        context.request_repaint_of(viewport);
    }
    for &viewport in ready_semantic_viewports {
        if !staging_viewports.contains(&viewport) {
            // Unblocking creates a new paint obligation even when the child
            // already has an in-flight placeholder or retained pass.
            context.request_repaint_of(viewport);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    #[test]
    fn staging_repaint_survives_an_already_queued_child_cycle() {
        assert_paint_obligation_survives_queued_cycle(true);
    }

    #[test]
    fn semantic_repaint_survives_an_already_queued_child_cycle() {
        assert_paint_obligation_survives_queued_cycle(false);
    }

    fn assert_paint_obligation_survives_queued_cycle(staging: bool) {
        let context = egui::Context::default();
        let child = egui::ViewportId::from_hash_of("staging-repaint-child");
        let mut input = egui::RawInput::default();
        input.viewport_id = child;
        input.viewports.insert(
            child,
            egui::ViewportInfo {
                parent: Some(egui::ViewportId::ROOT),
                ..Default::default()
            },
        );
        for _ in 0..8 {
            let mut output = context.run_ui(input.clone(), |_| {});
            output.textures_delta.clear();
            if !context.has_requested_repaint_for(&child) {
                break;
            }
        }
        assert!(
            !context.has_requested_repaint_for(&child),
            "initial viewport stabilization must not satisfy the tested obligation"
        );
        let repaint_count = Arc::new(AtomicUsize::new(0));
        context.set_request_repaint_callback({
            let repaint_count = Arc::clone(&repaint_count);
            move |info| {
                if info.viewport_id == child {
                    repaint_count.fetch_add(1, Ordering::Relaxed);
                }
            }
        });

        context.request_repaint_once_of(child);
        assert_eq!(repaint_count.load(Ordering::Relaxed), 1);

        if staging {
            request_native_viewport_repaints(&context, &[child], &[]);
        } else {
            request_native_viewport_repaints(&context, &[], &[child]);
        }
        assert_eq!(
            repaint_count.load(Ordering::Relaxed),
            1,
            "the already queued callback is not duplicated immediately"
        );

        let mut output = context.run_ui(input, |_| {});
        output.textures_delta.clear();
        assert_eq!(
            repaint_count.load(Ordering::Relaxed),
            2,
            "servicing the queued pass must reveal the retained paint obligation"
        );
    }
}
