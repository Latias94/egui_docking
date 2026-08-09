//! Session orchestration for native facts and effects.

use super::*;
use crate::intent::Authority;

impl DockspaceSession {
    fn native_state(&self) -> Result<&RuntimeNativeState, NativePlatformError> {
        self.native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)
    }

    fn native_state_mut(&mut self) -> Result<&mut RuntimeNativeState, NativePlatformError> {
        self.native
            .as_mut()
            .ok_or(NativePlatformError::ProviderUnavailable)
    }

    /// Enrolls the session-owned native platform observation source.
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
    pub fn enable_native_platform(
        &mut self,
        mode: NativePlatformMode,
    ) -> Result<Vec<NativeSurfaceBinding>, DockspaceRuntimeError> {
        if let Some(native) = &self.native {
            return if native.mode == mode {
                Err(NativePlatformError::ProviderAlreadyEnabled.into())
            } else {
                Err(NativePlatformError::ProviderModeConflict.into())
            };
        }
        let recorder = self
            .engine
            .create_backend_ingress_provider(self.presentation_host, PointerEdgeSequence::new(0))?;
        let mut native = RuntimeNativeState::new(recorder, mode);
        native.commit(&self.engine);
        let bindings = native.bindings.values().copied().collect();
        self.native = Some(native);
        Ok(bindings)
    }

    /// Captures one retryable exact-set native platform snapshot.
    ///
    /// Every currently registered native surface must appear exactly once.
    /// The returned value carries no core authority until it is recorded through
    /// [`Self::publish_native_snapshot`].
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable provider, stale or duplicate bindings,
    /// an incomplete roster, or generation exhaustion.
    pub fn capture_native_snapshot(
        &self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        self.native_state()?
            .capture_snapshot(self.version().epoch(), observations)
    }

    /// Captures an explicit Unknown native-window inventory tombstone.
    ///
    /// This revokes retained inventory authority without guessing that any
    /// surface was destroyed. The capture remains bound to the current provider,
    /// workspace epoch, and logical binding roster until it is durably recorded
    /// through [`Self::publish_native_snapshot`].
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the provider
    /// generation cannot advance.
    pub fn capture_native_inventory_unknown(
        &self,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        self.native_state()?
            .capture_unknown_inventory_snapshot(self.version().epoch())
    }

    /// Records one complete native platform snapshot in the joined backend order.
    ///
    /// Recording is durable within the facade-owned producer: dropping or
    /// rejecting the next host frame does not lose the physical fact. The exact
    /// immutable record is replayed until a host-frame commit advances the core
    /// watermark.
    ///
    /// # Errors
    ///
    /// Returns an error for a superseded provider, stale workspace or binding
    /// roster, or a non-contiguous provider generation.
    pub fn publish_native_snapshot(
        &mut self,
        snapshot: NativePlatformSnapshot,
    ) -> Result<(), DockspaceRuntimeError> {
        let current_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.provider() != snapshot.provider {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        if snapshot.expected_epoch != current_epoch {
            return Err(NativePlatformError::StaleWorkspace.into());
        }
        let expected_generation = native.next_snapshot_generation()?;
        if expected_generation != snapshot.generation {
            return Err(NativePlatformError::SnapshotGenerationStale.into());
        }
        if snapshot.bindings.len() != native.bindings.len()
            || snapshot.bindings.iter().any(|binding| {
                native
                    .bindings
                    .get(&binding.surface())
                    .is_none_or(|current| current.binding != *binding)
            })
        {
            return Err(NativePlatformError::SnapshotRosterStale.into());
        }

        let generation = snapshot.generation;
        let bindings = snapshot.bindings.clone();
        native.record_snapshot(snapshot)?;
        native.snapshot_generation = generation;
        for binding in bindings {
            native.close_generations.insert(binding, generation);
        }
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
        if native
            .bindings
            .get(&surface)
            .is_none_or(|current| current.binding != result.binding)
        {
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
            .recorder_mut()
            .record_viewport_registration(
                expected,
                surface,
                token.into_core(),
                ViewportRole::Root,
                None,
            )
            .map_err(|_| NativePlatformError::ProtocolInvariant)?;
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
