//! The single unpublished native update and its exact egui run boundary.

use dockspace::model::SurfaceId;
use eframe::{NativeOutputToken, egui};

use crate::application_action::NativeSubmittedApplicationAction;
use crate::coordinator::{PendingNoFrameInvalidation, PreparedNativeRetirements};
use crate::error::{NativeHostProtocolError, NativeRuntimeError};
use crate::host_frame::NativeHostUpdate;
use crate::pass_actions::NativeUiActionAcknowledgement;

pub(super) struct NativeOwnedUpdate {
    pub(super) host: NativeHostUpdate,
    pub(super) run: NativeUpdateRun,
    pub(super) publication: NativeUpdatePublication,
}

pub(super) struct NativeUpdatePublication {
    pub(super) surfaces: Vec<SurfaceId>,
    pub(super) no_frame_invalidations: Vec<PendingNoFrameInvalidation>,
    pub(super) retirements: Option<PreparedNativeRetirements>,
    pub(super) application_action: Option<NativeSubmittedApplicationAction>,
    pub(super) ui_actions: Vec<NativeUiActionAcknowledgement>,
    pub(super) native_input_pending: bool,
    pub(super) retain_previous: bool,
    pub(super) follow_up: bool,
}

/// A native token owns the original input; pass numbers only identify its encodings.
pub(super) struct NativeUpdateRun {
    pub(super) token: NativeOutputToken,
    context: egui::Context,
    frame: u64,
    pass: u64,
    awaiting_finalizer: bool,
}

impl NativeUpdateRun {
    pub(super) fn begin(context: &egui::Context, token: NativeOutputToken) -> Self {
        Self {
            token,
            context: context.clone(),
            frame: context.cumulative_frame_nr_for(token.viewport_id()),
            pass: context.cumulative_pass_nr_for(token.viewport_id()),
            awaiting_finalizer: true,
        }
    }

    fn matches(&self, context: &egui::Context, token: NativeOutputToken) -> bool {
        self.token == token
            && self.context == *context
            && self.frame == context.cumulative_frame_nr_for(token.viewport_id())
    }

    pub(super) fn begin_repeated_pass(
        &mut self,
        context: &egui::Context,
        token: NativeOutputToken,
    ) -> Result<(), NativeRuntimeError> {
        if !self.matches(context, token)
            || self.awaiting_finalizer
            || self.pass != context.cumulative_pass_nr_for(token.viewport_id())
        {
            return Err(NativeHostProtocolError::MultipassOutputChanged.into());
        }
        self.awaiting_finalizer = true;
        Ok(())
    }

    pub(super) fn finish_pass(
        &mut self,
        context: &egui::Context,
        token: NativeOutputToken,
    ) -> Result<(), NativeRuntimeError> {
        let completed = self.pass.checked_add(1);
        if !self.matches(context, token)
            || !self.awaiting_finalizer
            || completed != Some(context.cumulative_pass_nr_for(token.viewport_id()))
        {
            return Err(NativeHostProtocolError::MultipassOutputChanged.into());
        }
        self.pass = completed.expect("the completed pass was checked");
        self.awaiting_finalizer = false;
        Ok(())
    }
}
