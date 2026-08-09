//! Compilation of explicit host facts into one complete core platform snapshot.

use std::collections::BTreeMap;

use super::{
    CompiledNativeWindow, NativeCloseEffectAcknowledgement, NativeCloseFact, NativeCloseState,
    NativeInputFact, NativePlatformError, NativePresentationFact, NativeWindowFacts,
    NativeWindowLifecycleFact,
};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, InputEffectAcknowledgement,
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformCapabilityReason,
    PlatformRequirement, PlatformSnapshot, PresentationEffectAcknowledgement,
    WindowCloseObservation, WindowCloseState, WindowCoordinateObservation, WindowInputObservation,
    WindowInventoryObservation, WindowPresentationObservation, WorkAreaRosterObservation,
};
use crate::platform_provider::PlatformObservationLease;
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryObservationGeneration, PlatformSnapshotGeneration,
    PresentationObservationGeneration, ViewportBinding, WorkAreaObservationGeneration,
};
use crate::viewport_focus::{FocusObservationEnvelope, FocusObservationGeneration};

pub(super) fn compile_unknown_inventory_snapshot(
    generation: u64,
) -> Result<PlatformSnapshot, NativePlatformError> {
    let reason = AuthorityUnavailableReason::NotReported;
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(observed_root_capabilities()),
        ),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Unknown(reason),
            Authority::Unknown(reason),
        ),
        WindowInventoryObservation::unknown(
            InventoryObservationGeneration::new(generation),
            reason,
        ),
        Vec::new(),
        Vec::new(),
        WorkAreaRosterObservation::unknown(WorkAreaObservationGeneration::new(generation), reason),
    )
    .map_err(|_| NativePlatformError::ProtocolInvariant)
}

pub(super) fn compile_platform_snapshot(
    provider: PlatformObservationLease,
    generation: u64,
    supplied: &BTreeMap<ViewportBinding, NativeWindowFacts>,
) -> Result<PlatformSnapshot, NativePlatformError> {
    let reason = AuthorityUnavailableReason::NotReported;
    let mut live_bindings = Vec::new();
    let mut windows = Vec::new();
    let mut close_observations = Vec::new();
    for (&binding, &facts) in supplied {
        let compiled = compile_window_fact(provider, binding, facts, generation)?;
        if compiled.is_live {
            live_bindings.push(binding);
        }
        if let Some(window) = compiled.window {
            windows.push(window);
        }
        close_observations.push(compiled.close);
    }

    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(observed_root_capabilities()),
        ),
        FocusObservationEnvelope::new(
            FocusObservationGeneration::new(generation),
            Authority::Unknown(reason),
            Authority::Unknown(reason),
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(live_bindings),
        )
        .map_err(|_| NativePlatformError::ProtocolInvariant)?,
        windows,
        close_observations,
        WorkAreaRosterObservation::unknown(WorkAreaObservationGeneration::new(generation), reason),
    )
    .map_err(|_| NativePlatformError::ProtocolInvariant)
}

fn observed_root_capabilities() -> PlatformCapabilities {
    let unsupported = |requirement| {
        PlatformCapability::unsupported(requirement, PlatformCapabilityReason::BackendUnsupported)
    };
    let mut capabilities = PlatformCapabilities::default();
    capabilities
        .set_native_window_lifecycle(unsupported(PlatformRequirement::NativeWindowLifecycle));
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(unsupported(PlatformRequirement::HoveredWindow));
    capabilities
        .set_desktop_pointer_position(unsupported(PlatformRequirement::DesktopPointerPosition));
    capabilities
        .set_authoritative_button_state(unsupported(PlatformRequirement::AuthoritativeButtonState));
    capabilities
        .set_global_window_placement(unsupported(PlatformRequirement::GlobalWindowPlacement));
    capabilities.set_work_area(unsupported(PlatformRequirement::WorkArea));
    capabilities.set_pointer_hit_test_observation(unsupported(
        PlatformRequirement::PointerHitTestObservation,
    ));
    capabilities
        .set_pointer_hit_test_control(unsupported(PlatformRequirement::PointerHitTestControl));
    capabilities
        .set_global_focus_observation(unsupported(PlatformRequirement::GlobalFocusObservation));
    capabilities
        .set_window_activation_control(unsupported(PlatformRequirement::WindowActivationControl));
    capabilities.set_close_cancellation(unsupported(PlatformRequirement::CloseCancellation));
    capabilities
}

pub(super) fn compile_window_fact(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    facts: NativeWindowFacts,
    generation: u64,
) -> Result<CompiledNativeWindow, NativePlatformError> {
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let input_generation = InputObservationGeneration::new(generation);
    let presentation_generation = PresentationObservationGeneration::new(generation);
    let close_generation = CloseObservationGeneration::new(generation);
    let reason = AuthorityUnavailableReason::NotReported;

    match facts.lifecycle {
        NativeWindowLifecycleFact::Live => Ok(CompiledNativeWindow {
            is_live: true,
            window: Some(
                ObservedWindow::new(binding)
                    .with_coordinate_observation(WindowCoordinateObservation::new(
                        binding,
                        coordinate_generation,
                        fact_authority(facts.content_bounds, reason),
                        fact_authority(facts.outer_bounds, reason),
                        fact_authority(facts.native_scale_factor, reason),
                        fact_authority(facts.presentation_scale_factor, reason),
                    ))
                    .with_input_observation(WindowInputObservation::new(
                        binding,
                        input_generation,
                        facts.input.map_or(Authority::Unknown(reason), |input| {
                            Authority::Known(input.state.into())
                        }),
                        input_acknowledgement(provider, binding, facts.input, reason)?,
                    ))
                    .with_presentation_observation(WindowPresentationObservation::new(
                        binding,
                        presentation_generation,
                        facts
                            .presentation
                            .map_or(Authority::Unknown(reason), |presentation| {
                                Authority::Known(presentation.state.into())
                            }),
                        presentation_acknowledgement(
                            provider,
                            binding,
                            facts.presentation,
                            reason,
                        )?,
                    )),
            ),
            close: WindowCloseObservation::new(
                binding,
                close_generation,
                facts.close.map_or(Authority::Unknown(reason), |close| {
                    Authority::Known(match close.state {
                        NativeCloseState::Clear => WindowCloseState::LiveClear,
                        NativeCloseState::Requested => WindowCloseState::LiveRequested,
                    })
                }),
                close_acknowledgement(provider, binding, facts.close, reason)?,
            ),
        }),
        NativeWindowLifecycleFact::Destroyed { acknowledgement } => {
            if facts.content_bounds.is_some()
                || facts.outer_bounds.is_some()
                || facts.native_scale_factor.is_some()
                || facts.presentation_scale_factor.is_some()
                || facts.input.is_some()
                || facts.presentation.is_some()
                || facts.close.is_some()
            {
                return Err(NativePlatformError::DestroyedSurfaceHasLiveFacts {
                    surface: binding.surface(),
                });
            }
            Ok(CompiledNativeWindow {
                is_live: false,
                window: None,
                close: WindowCloseObservation::new(
                    binding,
                    close_generation,
                    Authority::Known(WindowCloseState::Destroyed),
                    known_close_acknowledgement(provider, binding, acknowledgement)?,
                ),
            })
        }
    }
}

fn fact_authority<T: Copy>(value: Option<T>, reason: AuthorityUnavailableReason) -> Authority<T> {
    value.map_or(Authority::Unknown(reason), Authority::Known)
}

fn input_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    fact: Option<NativeInputFact>,
    reason: AuthorityUnavailableReason,
) -> Result<InputEffectAcknowledgement, NativePlatformError> {
    let Some(fact) = fact else {
        return Ok(InputEffectAcknowledgement::unknown(reason));
    };
    match fact.acknowledgement {
        Some(acknowledgement) if acknowledgement.provider != provider => {
            Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) if acknowledgement.binding != binding => {
            Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) => Ok(InputEffectAcknowledgement::known(Some(
            acknowledgement.effect,
        ))),
        None => Ok(InputEffectAcknowledgement::known(None)),
    }
}

fn presentation_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    fact: Option<NativePresentationFact>,
    reason: AuthorityUnavailableReason,
) -> Result<PresentationEffectAcknowledgement, NativePlatformError> {
    let Some(fact) = fact else {
        return Ok(PresentationEffectAcknowledgement::unknown(reason));
    };
    match fact.acknowledgement {
        Some(acknowledgement) if acknowledgement.provider != provider => {
            Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) if acknowledgement.binding != binding => {
            Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) => Ok(PresentationEffectAcknowledgement::known(Some(
            acknowledgement.effect,
        ))),
        None => Ok(PresentationEffectAcknowledgement::known(None)),
    }
}

fn close_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    fact: Option<NativeCloseFact>,
    reason: AuthorityUnavailableReason,
) -> Result<CloseEffectAcknowledgement, NativePlatformError> {
    let Some(fact) = fact else {
        return Ok(CloseEffectAcknowledgement::unknown(reason));
    };
    known_close_acknowledgement(provider, binding, fact.acknowledgement)
}

fn known_close_acknowledgement(
    provider: PlatformObservationLease,
    binding: ViewportBinding,
    acknowledgement: Option<NativeCloseEffectAcknowledgement>,
) -> Result<CloseEffectAcknowledgement, NativePlatformError> {
    match acknowledgement {
        Some(acknowledgement) if acknowledgement.provider != provider => {
            Err(NativePlatformError::EffectAcknowledgementProviderMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) if acknowledgement.binding != binding => {
            Err(NativePlatformError::EffectAcknowledgementBindingMismatch {
                surface: binding.surface(),
            })
        }
        Some(acknowledgement) => Ok(CloseEffectAcknowledgement::known(Some(
            acknowledgement.effect,
        ))),
        None => Ok(CloseEffectAcknowledgement::known(None)),
    }
}
