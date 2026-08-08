use std::collections::{BTreeMap, BTreeSet};
use std::ops::{Deref, DerefMut};

use dockspace::backend::engine::{CoreHostFrame, CoreHostPresentationFrame, HostFrameView};
use dockspace::backend::presentation_observation::{
    HostPresentationEndpoint, HostPresentationOutput, NativeStagingPresentation,
    SurfacePresentationOutputTicket,
};
use dockspace::backend::scene::SurfaceScene;
use dockspace::ids::{SourceSequence, SurfaceId};
use dockspace::transition::WorkspaceVersion;
#[cfg(egui_backend_event_envelope)]
use egui::UserData;
use egui::{Context, FullOutput, ViewportId};

use crate::error::DockspaceError;
use crate::pointer_input::PreparedPointerInput;
use crate::render::{EguiSurfaceDraft, PreparedStyleReplacement};

use super::native_binding::{
    ExactNativeViewport, NativeBindingCandidate, NativeBindingError, NativeCoreRoute,
};
use super::{
    AutomaticPresentationFrame, EguiFrameScheduleKey, EguiHostFrameMode, EguiInputAuthority,
    EguiOutputBoundary, OuterPresentationFrame, PaneFocusAdapterState,
};

pub(super) enum EguiCoreFramePhase {
    Input(CoreHostFrame),
    Presentation(CoreHostPresentationFrame),
}

impl EguiCoreFramePhase {
    fn view(&self) -> HostFrameView<'_> {
        match self {
            Self::Input(frame) => frame.view(),
            Self::Presentation(frame) => frame.view(),
        }
    }

    pub(super) fn input(&self) -> Option<&CoreHostFrame> {
        match self {
            Self::Input(frame) => Some(frame),
            Self::Presentation(_) => None,
        }
    }

    pub(super) fn input_mut(&mut self) -> Option<&mut CoreHostFrame> {
        match self {
            Self::Input(frame) => Some(frame),
            Self::Presentation(_) => None,
        }
    }

    #[cfg(test)]
    pub(super) fn into_input(self) -> Option<CoreHostFrame> {
        match self {
            Self::Input(frame) => Some(frame),
            Self::Presentation(_) => None,
        }
    }
}

pub(super) struct EguiSurfacePass {
    context: Context,
    viewport: ViewportId,
    cumulative_pass: u64,
    #[cfg(egui_backend_event_envelope)]
    output_proof: Option<UserData>,
}

#[cfg(egui_backend_event_envelope)]
struct EguiSurfaceOutputProof;

struct EguiNativeStagingPass {
    presentation: NativeStagingPresentation,
    pass: EguiSurfacePass,
}

impl EguiSurfacePass {
    pub(super) fn from_context(context: &Context) -> Self {
        let viewport = context.viewport_id();
        Self {
            context: context.clone(),
            viewport,
            cumulative_pass: context.cumulative_pass_nr_for(viewport),
            #[cfg(egui_backend_event_envelope)]
            output_proof: None,
        }
    }

    #[cfg(egui_backend_event_envelope)]
    fn with_output_proof(mut self) -> Self {
        let proof = UserData::new(EguiSurfaceOutputProof);
        self.context.request_output_provenance(proof.clone());
        self.output_proof = Some(proof);
        self
    }

    #[cfg(not(egui_backend_event_envelope))]
    const fn with_output_proof(self) -> Self {
        self
    }

    #[cfg(egui_backend_event_envelope)]
    fn consume_output_proof(
        &self,
        surface: SurfaceId,
        output: &mut FullOutput,
    ) -> Result<(), DockspaceError> {
        let expected = self
            .output_proof
            .as_ref()
            .ok_or(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { surface })?;
        if !output.consume_output_provenance(expected) {
            return Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { surface });
        }
        Ok(())
    }

    #[cfg(not(egui_backend_event_envelope))]
    fn consume_output_proof(
        &self,
        _surface: SurfaceId,
        _output: &mut FullOutput,
    ) -> Result<(), DockspaceError> {
        Ok(())
    }
}

struct HostFrameScratch {
    pane_focus: PaneFocusAdapterState,
    semantic_source_sequence: SourceSequence,
    terminal_configuration_pending: bool,
    style_replacement: Option<StagedStyleReplacement>,
}

pub(super) struct StagedStyleReplacement {
    source_sequence: SourceSequence,
    prepared: PreparedStyleReplacement,
}

impl StagedStyleReplacement {
    pub(super) const fn source_sequence(&self) -> SourceSequence {
        self.source_sequence
    }

    pub(super) fn into_prepared(self) -> PreparedStyleReplacement {
        self.prepared
    }

    pub(super) const fn prepared(&self) -> &PreparedStyleReplacement {
        &self.prepared
    }
}

/// Adapter-owned state for one core host-frame capability.
///
/// This type deliberately has no `Dockspace` reference. A backend runtime can
/// eventually retain it across callbacks without extending a mutable facade
/// borrow; the current public facade remains responsible for engine and
/// renderer effects.
pub(super) struct HostFrameState {
    core_frame: Option<EguiCoreFramePhase>,
    key: EguiFrameScheduleKey,
    sealed_workspace: WorkspaceVersion,
    expected_surfaces: BTreeSet<SurfaceId>,
    drafts: BTreeMap<SurfaceId, EguiSurfaceDraft>,
    surface_passes: BTreeMap<SurfaceId, EguiSurfacePass>,
    confirmed_full_outputs: BTreeMap<SurfaceId, FullOutput>,
    native_bindings: Option<NativeBindingCandidate>,
    native_surface_passes: BTreeMap<SurfaceId, NativeCoreRoute>,
    native_staging_passes: BTreeMap<SurfaceId, EguiNativeStagingPass>,
    scratch: HostFrameScratch,
    automatic_presentation: Option<AutomaticPresentationFrame>,
    outer_presentation: Option<OuterPresentationFrame>,
    automatic_pointer: Option<PreparedPointerInput>,
    mode: EguiHostFrameMode,
    input_authority: EguiInputAuthority,
    output_boundary: EguiOutputBoundary,
    poisoned: bool,
    finished: bool,
}

/// Movable storage for a host-frame capability shared by borrowed and owned drivers.
///
/// `DockspaceHostFrame` has a `Drop` implementation, so Rust correctly prevents
/// moving a field directly out of it. This slot lets an owned native session lend
/// its state to the existing driver and recover it without unsafe code. An empty
/// slot means ownership has already moved elsewhere and therefore needs no drop
/// rollback from that temporary driver.
pub(super) struct HostFrameStateSlot(Option<HostFrameState>);

impl HostFrameStateSlot {
    pub(super) const fn new(state: HostFrameState) -> Self {
        Self(Some(state))
    }

    pub(super) fn take(&mut self) -> Option<HostFrameState> {
        self.0.take()
    }

    pub(super) fn as_mut(&mut self) -> Option<&mut HostFrameState> {
        self.0.as_mut()
    }

    pub(super) const fn key(&self) -> EguiFrameScheduleKey {
        match self.0.as_ref() {
            Some(state) => state.key(),
            None => panic!("a live host-frame driver retains its state capability"),
        }
    }
}

impl Deref for HostFrameStateSlot {
    type Target = HostFrameState;

    fn deref(&self) -> &Self::Target {
        self.0
            .as_ref()
            .expect("a live host-frame driver retains its state capability")
    }
}

impl DerefMut for HostFrameStateSlot {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0
            .as_mut()
            .expect("a live host-frame driver retains its state capability")
    }
}

#[cfg(all(test, egui_backend_event_envelope))]
mod output_proof_tests {
    use super::*;

    fn proven_output_with(
        context: &Context,
        mut run_ui: impl FnMut(&mut egui::Ui),
    ) -> (EguiSurfacePass, FullOutput) {
        let mut pass = None;
        let output = context.run_ui(egui::RawInput::default(), |ui| {
            pass = Some(EguiSurfacePass::from_context(ui.ctx()).with_output_proof());
            run_ui(ui);
        });
        (
            pass.expect("the egui callback mints one output proof"),
            output,
        )
    }

    fn proven_output(context: &Context) -> (EguiSurfacePass, FullOutput) {
        proven_output_with(context, |_| {})
    }

    #[test]
    fn exact_pass_output_consumes_its_private_proof() {
        let context = Context::default();
        let (pass, output) = proven_output(&context);

        let mut output = output;
        pass.consume_output_proof(SurfaceId::new(1), &mut output)
            .expect("the exact pass output carries its private proof");

        assert!(output.platform_output.presentation_token.is_none());
    }

    #[test]
    fn output_proof_preserves_the_application_presentation_token() {
        let context = Context::default();
        let application_token = UserData::new("application-owned");
        let expected = application_token.clone();
        let (pass, mut output) = proven_output_with(&context, |ui| {
            ui.ctx()
                .set_presentation_token(Some(application_token.clone()));
        });

        pass.consume_output_proof(SurfaceId::new(1), &mut output)
            .expect("the proof uses an independent backend-only lane");

        assert_eq!(output.platform_output.presentation_token, Some(expected));
    }

    #[test]
    fn changed_paint_content_invalidates_the_output_proof() {
        let context = Context::default();
        let (pass, mut output) = proven_output_with(&context, |ui| {
            ui.label("content-bound output proof");
        });
        assert!(!output.shapes.is_empty());
        output.shapes.clear();

        assert!(matches!(
            pass.consume_output_proof(SurfaceId::new(1), &mut output),
            Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { .. })
        ));
    }

    #[test]
    fn changed_texture_content_invalidates_the_output_proof() {
        let context = Context::default();
        let (pass, mut output) = proven_output(&context);
        output
            .textures_delta
            .free
            .push(egui::TextureId::Managed(91));

        assert!(matches!(
            pass.consume_output_proof(SurfaceId::new(1), &mut output),
            Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { .. })
        ));
    }

    #[test]
    fn changed_hit_graph_invalidates_the_output_proof() {
        let context = Context::default();
        let (pass, mut output) = proven_output(&context);
        assert!(output.pointer_hit_graph_candidate.is_some());
        output.pointer_hit_graph_candidate = None;

        assert!(matches!(
            pass.consume_output_proof(SurfaceId::new(1), &mut output),
            Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { .. })
        ));
    }

    #[test]
    fn cloned_output_shares_one_affine_proof() {
        let context = Context::default();
        let (pass, mut first) = proven_output(&context);
        let mut duplicate = first.clone();

        pass.consume_output_proof(SurfaceId::new(1), &mut first)
            .expect("the first exact output consumes the proof");
        assert!(matches!(
            pass.consume_output_proof(SurfaceId::new(1), &mut duplicate),
            Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { .. })
        ));
    }

    #[test]
    fn later_output_from_the_same_context_cannot_replace_an_older_pass() {
        let context = Context::default();
        let (older, _) = proven_output(&context);
        let (_, newer_output) = proven_output(&context);

        assert!(matches!(
            older.consume_output_proof(SurfaceId::new(1), &mut newer_output.clone()),
            Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { .. })
        ));
    }

    #[test]
    fn another_context_cannot_supply_the_output() {
        let first = Context::default();
        let second = Context::default();
        let (pass, _) = proven_output(&first);
        let (_, foreign_output) = proven_output(&second);

        assert!(matches!(
            pass.consume_output_proof(SurfaceId::new(1), &mut foreign_output.clone()),
            Err(DockspaceError::OuterHostSurfaceOutputAuthorityMismatch { .. })
        ));
    }
}

impl HostFrameState {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        core_frame: CoreHostFrame,
        key: EguiFrameScheduleKey,
        pane_focus: PaneFocusAdapterState,
        semantic_source_sequence: SourceSequence,
        automatic_presentation: Option<AutomaticPresentationFrame>,
        outer_presentation: Option<OuterPresentationFrame>,
        automatic_pointer: Option<PreparedPointerInput>,
        mode: EguiHostFrameMode,
        input_authority: EguiInputAuthority,
        output_boundary: EguiOutputBoundary,
    ) -> Self {
        let sealed_workspace = core_frame.view().version();
        let expected_surfaces = core_frame.surfaces().collect();
        Self {
            core_frame: Some(EguiCoreFramePhase::Input(core_frame)),
            key,
            sealed_workspace,
            expected_surfaces,
            drafts: BTreeMap::new(),
            surface_passes: BTreeMap::new(),
            confirmed_full_outputs: BTreeMap::new(),
            native_bindings: None,
            native_surface_passes: BTreeMap::new(),
            native_staging_passes: BTreeMap::new(),
            scratch: HostFrameScratch {
                pane_focus,
                semantic_source_sequence,
                terminal_configuration_pending: false,
                style_replacement: None,
            },
            automatic_presentation,
            outer_presentation,
            automatic_pointer,
            mode,
            input_authority,
            output_boundary,
            poisoned: false,
            finished: false,
        }
    }

    pub(super) const fn key(&self) -> EguiFrameScheduleKey {
        self.key
    }

    pub(super) fn view(&self) -> HostFrameView<'_> {
        self.core_frame
            .as_ref()
            .expect("a live egui host frame always retains its core frame capability")
            .view()
    }

    pub(super) fn expected_surfaces(&self) -> impl ExactSizeIterator<Item = SurfaceId> + '_ {
        self.expected_surfaces.iter().copied()
    }

    pub(super) fn native_staging_presentations(
        &self,
    ) -> impl ExactSizeIterator<Item = NativeStagingPresentation> + '_ {
        self.view()
            .native_staging_presentations()
            .collect::<Vec<_>>()
            .into_iter()
    }

    pub(super) fn stage_native_bindings(&mut self, candidate: NativeBindingCandidate) {
        debug_assert!(self.native_bindings.is_none());
        self.native_bindings = Some(candidate);
    }

    pub(super) fn resolve_native_callback(
        &self,
        native: ExactNativeViewport,
    ) -> Result<NativeCoreRoute, DockspaceError> {
        let candidate = self
            .native_bindings
            .as_ref()
            .expect("a native session retains one exact binding candidate");
        let surface = candidate
            .callback_surface(native)
            .map_err(NativeBindingError::from)?;
        let current = self
            .view()
            .viewport()
            .viewport(surface)
            .map(|record| record.binding());
        candidate
            .resolve_callback(native, current)
            .map_err(NativeBindingError::from)
            .map_err(DockspaceError::from)
    }

    pub(super) fn resolve_native_input_receiver(
        &self,
        native: ExactNativeViewport,
    ) -> Result<NativeCoreRoute, DockspaceError> {
        let candidate = self
            .native_bindings
            .as_ref()
            .expect("a native session retains one exact binding candidate");
        let surface = candidate
            .input_receiver_surface(native)
            .map_err(NativeBindingError::from)?;
        let current = self
            .view()
            .viewport()
            .viewport(surface)
            .map(|record| record.binding());
        candidate
            .resolve_input_receiver(native, current)
            .map_err(NativeBindingError::from)
            .map_err(DockspaceError::from)
    }

    pub(super) fn resolve_native_presentation(
        &self,
        native: ExactNativeViewport,
    ) -> Result<NativeCoreRoute, DockspaceError> {
        let candidate = self
            .native_bindings
            .as_ref()
            .expect("a native session retains one exact binding candidate");
        let surface = candidate
            .presentation_surface(native)
            .map_err(NativeBindingError::from)?;
        let current = self
            .view()
            .viewport()
            .viewport(surface)
            .map(|record| record.binding());
        candidate
            .resolve_presentation(native, current)
            .map_err(NativeBindingError::from)
            .map_err(DockspaceError::from)
    }

    pub(super) fn resolve_native_staging_callback(
        &self,
        native: ExactNativeViewport,
    ) -> Result<(NativeCoreRoute, NativeStagingPresentation), DockspaceError> {
        let route = self.resolve_native_callback(native)?;
        let presentation = self
            .view()
            .native_staging_presentations()
            .find(|presentation| presentation.binding().surface() == route.surface())
            .ok_or(DockspaceError::NativeStagingRequestUnavailable { native })?;
        if presentation.binding() != route.core() {
            return Err(DockspaceError::NativeStagingBindingMismatch {
                native,
                presentation,
            });
        }
        Ok((route, presentation))
    }

    pub(super) fn resolve_native_staging_presentation(
        &self,
        native: ExactNativeViewport,
    ) -> Result<(NativeCoreRoute, NativeStagingPresentation), DockspaceError> {
        let route = self.resolve_native_presentation(native)?;
        let presentation = self
            .view()
            .native_staging_presentations()
            .find(|presentation| presentation.binding().surface() == route.surface())
            .ok_or(DockspaceError::NativeStagingRequestUnavailable { native })?;
        if presentation.binding() != route.core() {
            return Err(DockspaceError::NativeStagingBindingMismatch {
                native,
                presentation,
            });
        }
        Ok((route, presentation))
    }

    pub(super) fn record_native_surface_pass(
        &mut self,
        route: NativeCoreRoute,
    ) -> Result<(), DockspaceError> {
        let surface = route.surface();
        if let Some(previous) = self.native_surface_passes.insert(surface, route)
            && previous != route
        {
            self.native_surface_passes.insert(surface, previous);
            return Err(DockspaceError::NativeSurfaceBindingChanged {
                surface,
                previous: previous.native(),
                submitted: route.native(),
            });
        }
        Ok(())
    }

    pub(super) fn validate_native_surface_output(
        &self,
        surface: SurfaceId,
        native: ExactNativeViewport,
    ) -> Result<(), DockspaceError> {
        let Some(previous) = self.native_surface_passes.get(&surface).copied() else {
            return Err(DockspaceError::NativeSurfaceOutputWithoutCallback { surface });
        };
        if previous.native() != native {
            return Err(DockspaceError::NativeSurfaceBindingChanged {
                surface,
                previous: previous.native(),
                submitted: native,
            });
        }
        Ok(())
    }

    pub(super) fn native_surface_pass(&self, surface: SurfaceId) -> Option<NativeCoreRoute> {
        self.native_surface_passes.get(&surface).copied()
    }

    pub(super) fn record_native_staging_pass(
        &mut self,
        presentation: NativeStagingPresentation,
        pass: EguiSurfacePass,
    ) -> Result<(), DockspaceError> {
        if !self
            .view()
            .native_staging_presentations()
            .any(|current| current == presentation)
        {
            return Err(DockspaceError::NativeStagingRequestOutsideRoster { presentation });
        }
        let surface = presentation.binding().surface();
        if let Some(previous) = self.native_staging_passes.get(&surface) {
            if previous.presentation != presentation {
                return Err(DockspaceError::NativeStagingRequestOutsideRoster { presentation });
            }
            if !previous.pass.context.eq(&pass.context) {
                return Err(DockspaceError::OuterHostSurfaceContextMismatch { surface });
            }
            if previous.pass.viewport != pass.viewport {
                return Err(DockspaceError::HostFrameSurfaceViewportChanged {
                    surface,
                    previous: previous.pass.viewport,
                    submitted: pass.viewport,
                });
            }
            if pass.cumulative_pass <= previous.pass.cumulative_pass {
                return Err(DockspaceError::HostFrameSurfacePassNotIncreasing {
                    surface,
                    previous: previous.pass.cumulative_pass,
                    submitted: pass.cumulative_pass,
                });
            }
        }
        let pass = pass.with_output_proof();
        self.native_staging_passes
            .insert(surface, EguiNativeStagingPass { presentation, pass });
        self.confirmed_full_outputs.remove(&surface);
        Ok(())
    }

    pub(super) fn native_staging_was_painted(
        &self,
        presentation: NativeStagingPresentation,
    ) -> bool {
        self.native_staging_passes
            .get(&presentation.binding().surface())
            .is_some_and(|pass| pass.presentation == presentation)
    }

    pub(super) fn painted_native_staging_presentations(
        &self,
    ) -> impl Iterator<Item = NativeStagingPresentation> + '_ {
        self.native_staging_passes
            .values()
            .map(|pass| pass.presentation)
    }

    pub(super) fn validate_native_presentation_output(
        &self,
        output: HostPresentationOutput,
    ) -> Result<(), DockspaceError> {
        if self.native_bindings.is_none() {
            return Ok(());
        }
        let surface = output.surface();
        let Some(route) = self.native_surface_pass(surface) else {
            return Err(DockspaceError::NativeSurfaceOutputWithoutCallback { surface });
        };
        let expected = HostPresentationEndpoint::Native(route.core());
        if output.endpoint() != expected {
            return Err(DockspaceError::NativePresentationEndpointMismatch {
                native: route.native(),
                expected,
                submitted: output.endpoint(),
            });
        }
        Ok(())
    }

    pub(super) fn take_native_bindings(&mut self) -> Option<NativeBindingCandidate> {
        self.native_bindings.take()
    }

    pub(super) const fn mode(&self) -> EguiHostFrameMode {
        self.mode
    }

    pub(super) const fn input_authority(&self) -> EguiInputAuthority {
        self.input_authority
    }

    pub(super) const fn output_boundary(&self) -> EguiOutputBoundary {
        self.output_boundary
    }

    pub(super) const fn sealed_workspace(&self) -> WorkspaceVersion {
        self.sealed_workspace
    }

    pub(super) fn input_core_frame(&self) -> Option<&CoreHostFrame> {
        self.core_frame.as_ref().and_then(EguiCoreFramePhase::input)
    }

    pub(super) fn input_core_frame_mut(&mut self) -> Option<&mut CoreHostFrame> {
        self.core_frame
            .as_mut()
            .and_then(EguiCoreFramePhase::input_mut)
    }

    pub(super) fn close_input(&mut self) -> Result<(), DockspaceError> {
        let core = self
            .core_frame
            .take()
            .expect("a live backend input frame retains its core capability");
        let EguiCoreFramePhase::Input(core) = core else {
            panic!("a backend input frame cannot hold a presentation capability");
        };
        let core = core.into_presentation()?;
        self.expected_surfaces = core.surfaces().collect();
        self.sealed_workspace = core.view().version();
        self.core_frame = Some(EguiCoreFramePhase::Presentation(core));
        Ok(())
    }

    pub(super) fn begin_configuration_phase(&mut self) -> Result<(), DockspaceError> {
        self.input_core_frame_mut()
            .expect("a native input session cannot hold a presentation capability")
            .begin_configuration_phase()?;
        Ok(())
    }

    pub(super) fn take_core_frame(&mut self) -> EguiCoreFramePhase {
        self.core_frame
            .take()
            .expect("a live egui host frame always retains its core frame capability")
    }

    pub(super) fn validate_new_surface_slot(
        &self,
        surface: SurfaceId,
    ) -> Result<(), DockspaceError> {
        if !self.expected_surfaces.contains(&surface) {
            return Err(DockspaceError::HostFrameSurfaceOutsideRoster { surface });
        }
        if self.drafts.contains_key(&surface) {
            return Err(DockspaceError::HostFrameDuplicateSurface { surface });
        }
        Ok(())
    }

    pub(super) fn validate_painted_surface_slot(
        &self,
        surface: SurfaceId,
        submitted: &EguiSurfacePass,
    ) -> Result<(), DockspaceError> {
        if !self.expected_surfaces.contains(&surface) {
            return Err(DockspaceError::HostFrameSurfaceOutsideRoster { surface });
        }
        if !self.mode.defers_publication_staging() {
            return self.validate_new_surface_slot(surface);
        }
        let Some(previous) = self.surface_passes.get(&surface) else {
            if self.drafts.contains_key(&surface) {
                return Err(DockspaceError::HostFrameDuplicateSurface { surface });
            }
            return Ok(());
        };
        if !previous.context.eq(&submitted.context) {
            return Err(DockspaceError::OuterHostSurfaceContextMismatch { surface });
        }
        if previous.viewport != submitted.viewport {
            return Err(DockspaceError::HostFrameSurfaceViewportChanged {
                surface,
                previous: previous.viewport,
                submitted: submitted.viewport,
            });
        }
        if submitted.cumulative_pass <= previous.cumulative_pass {
            return Err(DockspaceError::HostFrameSurfacePassNotIncreasing {
                surface,
                previous: previous.cumulative_pass,
                submitted: submitted.cumulative_pass,
            });
        }
        Ok(())
    }

    pub(super) fn record_painted_surface(
        &mut self,
        surface: SurfaceId,
        mut draft: EguiSurfaceDraft,
        pass: EguiSurfacePass,
    ) -> Result<(), crate::render::EguiRendererError> {
        if let Some(previous) = self.drafts.get_mut(&surface) {
            draft.preserve_prior_raw_event_inputs(previous)?;
        }
        let pass = pass.with_output_proof();
        self.drafts.insert(surface, draft);
        self.surface_passes.insert(surface, pass);
        self.confirmed_full_outputs.remove(&surface);
        Ok(())
    }

    pub(super) fn record_unavailable_surface(
        &mut self,
        surface: SurfaceId,
        draft: EguiSurfaceDraft,
    ) {
        self.drafts.insert(surface, draft);
        self.confirmed_full_outputs.remove(&surface);
    }

    pub(super) fn confirm_surface_output(
        &mut self,
        surface: SurfaceId,
        context: &Context,
        viewport: ViewportId,
        mut output: FullOutput,
    ) -> Result<(), DockspaceError> {
        let pass = self.validate_surface_output(surface, context, viewport, &output)?;
        pass.consume_output_proof(surface, &mut output)?;
        self.confirmed_full_outputs.insert(surface, output);
        Ok(())
    }

    fn validate_surface_output<'output>(
        &'output self,
        surface: SurfaceId,
        context: &Context,
        viewport: ViewportId,
        output: &FullOutput,
    ) -> Result<&'output EguiSurfacePass, DockspaceError> {
        if self.mode != EguiHostFrameMode::CompleteRoster {
            return Err(DockspaceError::OuterHostFrameRequired);
        }
        let pass = self
            .surface_passes
            .get(&surface)
            .or_else(|| {
                self.native_staging_passes
                    .get(&surface)
                    .map(|pass| &pass.pass)
            })
            .ok_or(DockspaceError::OuterHostSurfaceOutputUnconfirmed { surface })?;
        if !pass.context.eq(context) {
            return Err(DockspaceError::OuterHostSurfaceContextMismatch { surface });
        }
        if pass.viewport != viewport {
            return Err(DockspaceError::OuterHostSurfaceViewportMismatch {
                surface,
                expected: pass.viewport,
                submitted: viewport,
            });
        }
        let expected_completed_pass = pass
            .cumulative_pass
            .checked_add(1)
            .ok_or(DockspaceError::OuterHostSurfaceOutputPassExhausted { surface })?;
        let submitted_pass = context.cumulative_pass_nr_for(viewport);
        if expected_completed_pass != submitted_pass {
            return Err(DockspaceError::OuterHostSurfaceOutputPassMismatch {
                surface,
                expected: expected_completed_pass,
                submitted: submitted_pass,
            });
        }
        if output.platform_output.num_completed_passes == 0
            || !output.viewport_output.contains_key(&viewport)
        {
            return Err(DockspaceError::OuterHostSurfaceFullOutputMissing { surface });
        }
        if self.confirmed_full_outputs.contains_key(&surface) {
            return Err(DockspaceError::OuterHostSurfaceOutputAlreadyConfirmed { surface });
        }
        Ok(pass)
    }

    pub(super) fn confirm_external_surface_output(
        &mut self,
        surface: SurfaceId,
        context: &Context,
        viewport: ViewportId,
        output: &mut FullOutput,
    ) -> Result<(), DockspaceError> {
        let pass = self.validate_surface_output(surface, context, viewport, output)?;
        pass.consume_output_proof(surface, output)?;
        self.confirmed_full_outputs.insert(surface, output.clone());
        Ok(())
    }

    pub(super) fn validate_finish(&self) -> Result<(), DockspaceError> {
        if self.poisoned {
            return Err(DockspaceError::HostFramePoisoned);
        }
        if self.mode == EguiHostFrameMode::CompleteRoster
            && let Some(surface) = self.drafts.iter().find_map(|(surface, draft)| {
                (draft.paint().is_some() && !self.confirmed_full_outputs.contains_key(surface))
                    .then_some(*surface)
            })
        {
            return Err(DockspaceError::OuterHostSurfaceOutputUnconfirmed { surface });
        }
        if self.mode == EguiHostFrameMode::CompleteRoster
            && let Some(surface) = self
                .native_staging_passes
                .keys()
                .find(|surface| !self.confirmed_full_outputs.contains_key(surface))
                .copied()
        {
            return Err(DockspaceError::OuterHostSurfaceOutputUnconfirmed { surface });
        }
        Ok(())
    }

    pub(super) fn missing_surfaces(&self) -> Vec<SurfaceId> {
        self.expected_surfaces
            .difference(&self.drafts.keys().copied().collect())
            .copied()
            .collect()
    }

    pub(super) fn current_ready_output_ticket(
        &self,
        surface: SurfaceId,
    ) -> Option<SurfacePresentationOutputTicket> {
        self.view()
            .scene()
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .map(|ready| ready.output_ticket())
    }

    pub(super) fn drafts(&self) -> &BTreeMap<SurfaceId, EguiSurfaceDraft> {
        &self.drafts
    }

    pub(super) fn has_surface_passes(&self) -> bool {
        !self.surface_passes.is_empty()
    }

    pub(super) fn drafts_mut(&mut self) -> &mut BTreeMap<SurfaceId, EguiSurfaceDraft> {
        &mut self.drafts
    }

    pub(super) fn take_drafts(&mut self) -> BTreeMap<SurfaceId, EguiSurfaceDraft> {
        std::mem::take(&mut self.drafts)
    }

    pub(super) fn replace_expected_surfaces(&mut self, surfaces: BTreeSet<SurfaceId>) {
        self.expected_surfaces = surfaces;
    }

    pub(super) fn request_surface_repaint(&self, reason: &'static str) {
        for pass in self.surface_passes.values() {
            pass.context.request_discard(reason);
            pass.context.request_repaint();
        }
    }

    pub(super) fn automatic_pointer(&self) -> Option<&PreparedPointerInput> {
        self.automatic_pointer.as_ref()
    }

    pub(super) fn set_automatic_pointer(&mut self, pointer: Option<PreparedPointerInput>) {
        self.automatic_pointer = pointer;
    }

    #[cfg(test)]
    pub(super) fn automatic_presentation(&self) -> Option<&AutomaticPresentationFrame> {
        self.automatic_presentation.as_ref()
    }

    pub(super) fn take_automatic_presentation(&mut self) -> Option<AutomaticPresentationFrame> {
        self.automatic_presentation.take()
    }

    pub(super) fn take_outer_presentation(&mut self) -> Option<OuterPresentationFrame> {
        self.outer_presentation.take()
    }

    pub(super) fn confirmed_full_outputs(&self) -> &BTreeMap<SurfaceId, FullOutput> {
        &self.confirmed_full_outputs
    }

    pub(super) fn take_confirmed_full_outputs(&mut self) -> BTreeMap<SurfaceId, FullOutput> {
        std::mem::take(&mut self.confirmed_full_outputs)
    }

    pub(super) const fn semantic_source_sequence(&self) -> SourceSequence {
        self.scratch.semantic_source_sequence
    }

    pub(super) fn set_semantic_source_sequence(&mut self, sequence: SourceSequence) {
        self.scratch.semantic_source_sequence = sequence;
    }

    pub(super) const fn terminal_configuration_pending(&self) -> bool {
        self.scratch.terminal_configuration_pending
    }

    pub(super) fn mark_terminal_configuration_pending(&mut self) {
        self.scratch.terminal_configuration_pending = true;
    }

    pub(super) fn stage_style_replacement(
        &mut self,
        source_sequence: SourceSequence,
        prepared: PreparedStyleReplacement,
    ) {
        self.scratch.style_replacement = Some(StagedStyleReplacement {
            source_sequence,
            prepared,
        });
    }

    pub(super) const fn staged_style_replacement(&self) -> Option<&StagedStyleReplacement> {
        self.scratch.style_replacement.as_ref()
    }

    pub(super) fn take_staged_style_replacement(&mut self) -> Option<StagedStyleReplacement> {
        self.scratch.style_replacement.take()
    }

    pub(super) fn pane_focus(&self) -> &PaneFocusAdapterState {
        &self.scratch.pane_focus
    }

    pub(super) fn pane_focus_mut(&mut self) -> &mut PaneFocusAdapterState {
        &mut self.scratch.pane_focus
    }

    pub(super) fn poison(&mut self) {
        self.poisoned = true;
    }

    pub(super) const fn is_finished(&self) -> bool {
        self.finished
    }

    pub(super) fn finish(&mut self) {
        self.finished = true;
    }
}

#[cfg(test)]
mod tests {
    use super::HostFrameState;

    #[test]
    fn host_frame_state_does_not_borrow_the_dockspace_facade() {
        fn assert_static<T: 'static>() {}

        assert_static::<HostFrameState>();
    }
}
