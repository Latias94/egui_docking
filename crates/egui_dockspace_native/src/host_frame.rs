//! Native host-frame adapter over the renderer-neutral session frame.

use std::sync::Arc;

use dockspace::model::SurfaceId;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSubmittedAction, DockspaceUpdate, HostFrameReport,
    HostInputOutcome, PreparedPaneFocusObservation, SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};

use crate::error::NativeHostProtocolError;
use crate::error::NativeRuntimeError;
use crate::mailbox::NativeHostBridge;
use crate::pointer_event::NativePointerTranslator;
use crate::receiver::NativeReceiverStore;

pub(crate) fn retained_focus_binding(
    context: &egui::Context,
    token: eframe::NativeOutputToken,
    surface: SurfaceId,
    bridge: &NativeHostBridge,
) -> Option<dockspace::runtime::NativeSurfaceBinding> {
    let binding = bridge
        .output_reservation(token)
        .and_then(|reservation| reservation.binding())?;
    (token.viewport_id() == context.viewport_id()
        && binding.surface() == surface
        && bridge.with_viewports(|viewports| {
            viewports.binding_for_output(
                token.viewport_id(),
                token.window_id(),
                token.create_attempt(),
            )
        }) == Some(binding))
    .then_some(binding)
}

/// Final-pass paint metadata plus fork-owned scroll identities from that same pass.
pub(crate) struct NativeSurfacePaint {
    inner: egui_dockspace::native_support::NativeSurfacePaint,
    scroll_identities: Vec<egui::WidgetHitIdentity>,
    binding: Option<dockspace::runtime::NativeSurfaceBinding>,
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

    pub(crate) fn native_binding(&self) -> Option<dockspace::runtime::NativeSurfaceBinding> {
        self.binding
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

/// Owned continuation of the same candidate, without native store borrows.
pub(crate) struct NativeHostUpdate {
    update: DockspaceUpdate,
    bridge: Arc<NativeHostBridge>,
    repaint_context: Option<egui::Context>,
    overload: Option<(u64, DockspaceSubmittedAction)>,
}

impl NativeHostUpdate {
    pub(crate) fn resume<'session>(
        self,
        session: &'session mut dockspace::runtime::DockspaceSession,
        receivers: &'session mut NativeReceiverStore,
        translator: &'session mut NativePointerTranslator,
    ) -> Result<NativeHostFrame<'session>, NativeRuntimeError> {
        Ok(NativeHostFrame {
            frame: session.resume_update(self.update)?,
            bridge: self.bridge,
            receivers,
            repaint_context: self.repaint_context,
            overload: self
                .overload
                .map(|(boundary, submitted)| OverloadCancellation {
                    boundary,
                    submitted,
                    translator,
                }),
        })
    }
}

impl std::fmt::Debug for NativeHostFrame<'_> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NativeHostFrame")
            .finish_non_exhaustive()
    }
}

impl<'session> NativeHostFrame<'session> {
    pub(crate) fn retain_pane_focus(
        &self,
        context: &egui::Context,
        surface: SurfaceId,
        panes: &dyn PaneView,
    ) {
        // A replacement may reuse item/widget IDs in a new workspace epoch.
        if self.frame.proposed_native_bindings().next().is_some() {
            return;
        }
        let window = self.bridge.with_viewports(|routes| {
            routes
                .binding(context.viewport_id())
                .map(|binding| binding.window())
        });
        if !window.is_some_and(|window| {
            self.frame
                .native_surface_bindings_for_window(window)
                .any(|binding| binding.surface() == surface)
        }) {
            return;
        }
        if let Some(surface) = self.frame.view().surface(surface) {
            egui_dockspace::native_support::retain_skipped_pane_focus(context, surface, panes);
        }
    }

    pub(crate) fn presented_focus_bindings(
        &self,
        surface: SurfaceId,
        viewport: egui::ViewportId,
    ) -> &[(dockspace::model::ItemId, egui::Id)] {
        self.receivers.presented_focus_bindings(
            surface,
            viewport,
            self.frame.presented_surface(surface),
        )
    }

    pub(crate) fn suspend(self) -> NativeHostUpdate {
        NativeHostUpdate {
            update: self.frame.suspend(),
            bridge: self.bridge,
            repaint_context: self.repaint_context,
            overload: self
                .overload
                .map(|cancellation| (cancellation.boundary, cancellation.submitted)),
        }
    }

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

    pub(crate) fn paint_surface(
        &mut self,
        instance_id: egui::Id,
        surface: SurfaceId,
        ui: &mut egui::Ui,
        panes: &mut dyn PaneView,
        style: &DockStyle,
    ) -> Result<NativeSurfacePaint, NativeRuntimeError> {
        let window = self
            .frame
            .proposed_native_bindings()
            .find(|binding| binding.surface() == surface)
            .map(|binding| binding.window())
            .or_else(|| {
                self.bridge.with_viewports(|routes| {
                    routes
                        .binding(ui.ctx().viewport_id())
                        .map(|binding| binding.window())
                })
            });
        let binding = window.and_then(|window| {
            self.frame
                .native_surface_bindings_for_window(window)
                .find(|binding| binding.surface() == surface)
        });
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
            binding,
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

    pub(crate) fn submit_pane_focus_observation(
        &mut self,
        observation: PreparedPaneFocusObservation,
    ) -> Result<(), NativeRuntimeError> {
        self.frame
            .submit_pane_focus_observation(observation)
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
        self.commit_inner(None)
    }

    /// Publishes the external Root surface under the same lock as its exact route.
    pub(crate) fn commit_publishing_root(
        self,
        root_surface: &mut SurfaceId,
    ) -> Result<HostFrameReport, NativeRuntimeError> {
        self.commit_inner(Some(root_surface))
    }

    fn commit_inner(
        self,
        root_surface: Option<&mut SurfaceId>,
    ) -> Result<HostFrameReport, NativeRuntimeError> {
        let Self {
            frame,
            bridge,
            receivers,
            repaint_context,
            overload,
        } = self;
        let (report, staging_viewports) =
            bridge.commit_host_frame_with_staging(frame, overload.is_none(), root_surface)?;
        if let Some(cancellation) = overload {
            if !matches!(
                report.submitted_action_outcome(cancellation.submitted),
                Some(&HostInputOutcome::NativeInputCancelledForOverload)
            ) || !bridge.finish_overload_cancellation(cancellation.boundary)
            {
                return Err(NativeHostProtocolError::NativeOverloadCancellationChanged.into());
            }
            cancellation.translator.finish_overload_cancellation();
            bridge.release_pointer_translation_capacity(cancellation.translator);
        }
        receivers.commit_presentations(report.surface_presentation_commits());
        for &binding in report.native_placement_detachments() {
            receivers.retire_binding(binding);
        }
        bridge.commit_frame_boundary(report.native_input_pending());
        if let Some(context) = repaint_context {
            request_native_viewport_repaints(&context, &staging_viewports, &[]);
            while let Some(viewport) = bridge.claim_ready_semantic_viewport() {
                if !staging_viewports.contains(&viewport) {
                    context.request_repaint_of(viewport);
                }
            }
        }
        Ok(report)
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
