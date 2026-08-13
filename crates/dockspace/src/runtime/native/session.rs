//! Session orchestration for native facts and effects.

use super::*;
use crate::intent::Authority;
use crate::pointer_journal::PointerEdgeSequence;

impl DockspaceSession {
    fn native_state_mut(&mut self) -> Result<&mut RuntimeNativeState, NativePlatformError> {
        self.native
            .as_mut()
            .ok_or(NativePlatformError::ProviderUnavailable)
    }

    /// Returns whether the managed host currently owns this exact binding.
    ///
    /// This is an identity check only; it does not prove visibility, geometry,
    /// input authority, or final presentation.
    #[must_use]
    pub fn is_current_native_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| native.contains_binding(binding))
    }

    /// Returns whether the managed host still owns this exact live or retired binding.
    ///
    /// Retired bindings remain recognized until the host reports exact
    /// destruction and then declares every callback, route, and sidecar for
    /// that incarnation permanently quiescent.
    #[must_use]
    pub fn recognizes_native_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| native.recognizes_binding(binding))
    }

    /// Enrolls exact observation of externally owned native root windows.
    ///
    /// The method enrolls exactly once and never reissues existing surface
    /// bindings. Provider replacement remains an internal core concern until a
    /// real native coordinator defines a product-level restart operation.
    ///
    /// # Errors
    ///
    /// Returns an error when a provider is already enrolled or the core cannot
    /// mint platform authority.
    /// The returned bindings are the complete current native roster and must
    /// be retained by the host for future asynchronous facts.
    pub fn enable_observed_native_roots(
        &mut self,
    ) -> Result<Vec<NativeSurfaceBinding>, DockspaceRuntimeError> {
        self.enroll_native_host(NativeHostProfile::ObservedRoots, None)
    }

    /// Enrolls the sole desktop-global coordinator for managed native windows.
    ///
    /// The fixed capability profile requires complete window inventory,
    /// event-time desktop pointer facts, explicit work-area authority, and
    /// typed effect settlement. The initial pointer roster is enrolled in the
    /// same operation; no platform or pointer record can precede it. Platforms
    /// which cannot provide those facts should use surface-local input instead
    /// of enabling this coordinator.
    ///
    /// # Errors
    ///
    /// Returns an error when the initial roster is invalid, another native or
    /// surface-local pointer provider is active, or the core cannot mint joined
    /// platform/input authority.
    pub fn enable_managed_native_host(
        &mut self,
        initial_pointer_roster: NativePointerRoster,
    ) -> Result<Vec<NativeSurfaceBinding>, DockspaceRuntimeError> {
        let checkpoint = pointer::compile_initial_pointer_authority(initial_pointer_roster)?;
        self.enroll_native_host(NativeHostProfile::ManagedDesktop, Some(checkpoint))
    }

    fn enroll_native_host(
        &mut self,
        profile: NativeHostProfile,
        pointer_checkpoint: Option<crate::pointer_journal::PointerAuthorityCheckpoint>,
    ) -> Result<Vec<NativeSurfaceBinding>, DockspaceRuntimeError> {
        if self.pointer.is_some() {
            return Err(NativePlatformError::PointerProviderAlreadyEnabled.into());
        }
        if self.native.is_some() {
            return Err(NativePlatformError::ProviderAlreadyEnabled.into());
        }
        let recorder = self
            .engine
            .create_backend_ingress_provider(self.presentation_host, PointerEdgeSequence::new(0))?;
        let mut native = RuntimeNativeState::new(recorder, profile);
        native.commit(&self.engine);
        if let Some(checkpoint) = pointer_checkpoint {
            native.record_initial_pointer_authority(checkpoint);
        }
        let bindings = native.bindings.values().copied().collect();
        self.native = Some(native);
        Ok(bindings)
    }

    /// Records one exact-set native platform snapshot in backend order.
    ///
    /// Every currently registered native surface must appear exactly once.
    /// Validation and recording are one atomic facade operation. A rejected
    /// roster cannot be published later against a different workspace or
    /// binding incarnation.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable provider, stale or duplicate bindings,
    /// an incomplete roster, or generation exhaustion.
    pub fn report_native_snapshot(
        &mut self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.profile != NativeHostProfile::ObservedRoots {
            return Err(NativePlatformError::HostProfileMismatch.into());
        }
        native.record_snapshot_facts(
            expected_epoch,
            observations,
            NativeWorkAreaRoster::Unknown,
        )?;
        Ok(())
    }

    /// Records one complete managed native window and work-area snapshot.
    ///
    /// Both rosters are explicit facts at the same platform generation. An
    /// unavailable work-area roster revokes prior placement authority; it does
    /// not preserve the previous exact roster implicitly.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile, stale or incomplete window
    /// bindings, or an invalid exact work-area roster.
    pub fn report_managed_native_snapshot(
        &mut self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
        work_areas: NativeWorkAreaRoster,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch.into());
        }
        native.record_snapshot_facts(expected_epoch, observations, work_areas)?;
        Ok(())
    }

    /// Returns the latest committed exact binding for one platform work area.
    ///
    /// A snapshot recorded but not yet committed cannot mint route authority.
    #[must_use]
    pub fn native_work_area(&self, token: HostWorkAreaToken) -> Option<NativeWorkAreaBinding> {
        self.native
            .as_ref()
            .filter(|native| native.profile == NativeHostProfile::ManagedDesktop)
            .and_then(|native| native.work_areas.get(&token.into_core()).copied())
    }

    /// Records one desktop-global native pointer edge in host event order.
    ///
    /// The runtime assigns the provider sequence and retains the edge until a
    /// host frame resolves its receiver query and commits. A failed frame does
    /// not consume the edge.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile, stale window or work-area
    /// bindings, contradictory route facts, or sequence exhaustion.
    pub fn record_native_pointer(
        &mut self,
        input: NativePointerInput,
    ) -> Result<(), DockspaceRuntimeError> {
        self.native_state_mut()?.record_pointer(input)?;
        Ok(())
    }

    /// Records an explicit Unknown native-window inventory tombstone.
    ///
    /// This revokes retained inventory authority without guessing that any
    /// surface was destroyed. Recording is durable within the facade-owned
    /// producer and replays until a host-frame commit advances the core watermark.
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the provider
    /// generation cannot advance.
    pub fn report_native_inventory_unknown(&mut self) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        self.native_state_mut()?
            .record_unknown_inventory(expected_epoch)?;
        Ok(())
    }

    /// Records one exact live-binding native close observation in backend order.
    ///
    /// Stale callback capabilities are rejected before they can be redirected
    /// to a replacement binding which reused the same host token.
    ///
    /// # Errors
    ///
    /// Returns a typed stale-surface, acknowledgement, or generation error.
    pub fn publish_native_close(
        &mut self,
        binding: NativeSurfaceBinding,
        state: NativeCloseState,
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.bindings.get(&binding.surface()) != Some(&binding) {
            return Err(NativePlatformError::StaleSurface {
                surface: binding.surface(),
            }
            .into());
        }
        let generation = native
            .close_generations
            .get(&binding.binding)
            .copied()
            .unwrap_or(0)
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)?;
        let state = match state {
            NativeCloseState::Clear => WindowCloseState::LiveClear,
            NativeCloseState::Requested => WindowCloseState::LiveRequested,
        };
        let acknowledgement = match acknowledgement {
            Some(acknowledgement) if acknowledgement.provider != native.provider() => {
                return Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                    surface: binding.surface(),
                }
                .into());
            }
            Some(acknowledgement) if acknowledgement.binding != binding.binding => {
                return Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                    surface: binding.surface(),
                }
                .into());
            }
            Some(acknowledgement) => {
                CloseEffectAcknowledgement::known(Some(acknowledgement.effect))
            }
            None => CloseEffectAcknowledgement::known(None),
        };
        native.record_close(
            expected_epoch,
            WindowCloseObservation::new(
                binding.binding,
                CloseObservationGeneration::new(generation),
                Authority::Known(state),
                acknowledgement,
            ),
        )?;
        native.close_generations.insert(binding.binding, generation);
        Ok(())
    }

    /// Records one negative or indeterminate result for an emitted native effect.
    ///
    /// # Errors
    ///
    /// Returns a recoverable error containing the original affine result when
    /// its provider or binding was superseded, or the producer rejects it.
    pub fn report_native_effect_result(
        &mut self,
        result: NativeEffectResult,
    ) -> Result<(), NativeEffectSubmissionError> {
        let current_epoch = self.version().epoch();
        let Some(native) = self.native.as_mut() else {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::ProviderUnavailable,
                result,
            ));
        };
        if native.provider() != result.provider {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::ProviderSuperseded,
                result,
            ));
        }
        let surface = result.binding.surface();
        if !native.recognizes_binding(result.binding()) {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::StaleSurface { surface },
                result,
            ));
        }
        if !native.binding_is_live(result.binding()) && result.receipt_epoch() != current_epoch {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::StaleSurface { surface },
                result,
            ));
        }
        native
            .record_effect_result(result.result)
            .map_err(|error| NativeEffectSubmissionError::new(error, result))
    }

    /// Records permanent host quiescence for one retired exact native binding.
    ///
    /// Call this only after every route, callback, renderer sidecar, and queued
    /// platform fact capable of naming the binding has been retired. The record
    /// lets core compact binding-scoped effect, presentation, and tombstone
    /// history without inferring quiescence from callback absence.
    ///
    /// # Errors
    ///
    /// Returns an error when the binding belongs to another provider, remains
    /// live, was not retired by this provider, or the joined recorder rejects
    /// the permanent boundary.
    pub fn report_native_binding_quiescence(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> Result<(), DockspaceRuntimeError> {
        self.native_state_mut()?
            .record_binding_quiescence(binding)
            .map_err(Into::into)
    }

    /// Records registration of one externally owned native root window.
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the backend
    /// producer rejects the semantic record.
    pub fn register_native_root(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        self.native_state_mut()?
            .record_root_registration(expected, surface, token.into_core())?;
        Ok(())
    }

    /// Records the first registration of an existing dockspace-owned child window.
    ///
    /// The adapter names only stable product identities. Core resolves the
    /// current recovery anchor and mints every recovery-side presentation
    /// identity while reducing the registration.
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the backend
    /// producer rejects the bootstrap record. Semantic rejection is reported
    /// by the next [`HostFrameReport`](crate::runtime::HostFrameReport).
    pub fn register_owned_native_child(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
        recovery_host: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        self.native_state_mut()?.record_child_bootstrap(
            expected,
            surface,
            token.into_core(),
            crate::surface_recovery::SurfaceRecoveryBootstrap::new(recovery_host),
        )?;
        Ok(())
    }

    /// Records one explicit core-owned close-plan request for an exact native edge.
    ///
    /// # Errors
    ///
    /// Returns an error when the edge belongs to a superseded provider or the
    /// backend producer rejects the semantic record.
    pub fn request_native_surface_close(
        &mut self,
        close: NativeSurfaceCloseRequest,
        request: crate::SurfaceCloseRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let native = self.native_state_mut()?;
        validate_close_request(native, close)?;
        native
            .recorder_mut()
            .record_semantic_input(EngineInput::RequestSurfaceClose {
                expected,
                edge: close.edge,
                request,
            })
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }

    /// Records exact platform cancellation for one unresolved native close edge.
    ///
    /// # Errors
    ///
    /// Returns an error when the edge belongs to a superseded provider or the
    /// backend producer rejects the semantic record.
    pub fn cancel_native_surface_close(
        &mut self,
        close: NativeSurfaceCloseRequest,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let native = self.native_state_mut()?;
        validate_close_request(native, close)?;
        native
            .recorder_mut()
            .record_semantic_input(EngineInput::CancelSurfaceClose {
                expected,
                edge: close.edge,
            })
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
        Ok(())
    }
}

fn validate_close_request(
    native: &RuntimeNativeState,
    close: NativeSurfaceCloseRequest,
) -> Result<(), NativePlatformError> {
    if native.provider() != close.provider {
        return Err(NativePlatformError::ProviderSuperseded);
    }
    let binding = close.edge.binding();
    if native
        .bindings
        .get(&binding.surface())
        .is_none_or(|current| current.binding != binding)
    {
        return Err(NativePlatformError::StaleSurface {
            surface: binding.surface(),
        });
    }
    Ok(())
}
