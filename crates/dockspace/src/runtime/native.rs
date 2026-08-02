//! Opaque native-window lifecycle capabilities for renderer-neutral hosts.

use std::collections::BTreeMap;

use thiserror::Error;

use super::{DockspaceHostFrame, DockspaceRuntimeError, DockspaceSession};
use crate::PlatformObservationLease;
use crate::engine::EngineInput;
use crate::geometry::{PhysicalRect, ScaleFactor};
use crate::ids::{SurfaceId, WorkspaceEpoch};
use crate::intent::{Authority, AuthorityUnavailableReason};
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, InputEffectAcknowledgement,
    ObservedWindow, PlatformCapabilities, PlatformCapability, PlatformSnapshot,
    PresentationEffectAcknowledgement, WindowCloseObservation, WindowCloseState,
    WindowCoordinateObservation, WindowInputObservation, WindowInputState,
    WindowInventoryObservation, WindowPresentationObservation, WindowPresentationState,
    WorkAreaRosterObservation,
};
use crate::transition::{EngineTransition, InputOutcome};
use crate::viewport::{
    CapabilityObservationGeneration, CloseObservationGeneration, CoordinateObservationGeneration,
    InputObservationGeneration, InventoryObservationGeneration, PlatformSnapshotGeneration,
    PresentationObservationGeneration, ViewportBinding, ViewportRole, WindowToken,
    WorkAreaObservationGeneration,
};
use crate::viewport_focus::{FocusObservationEnvelope, FocusObservationGeneration};

/// Adapter-owned opaque native-window token.
///
/// A token may be reused after exact destruction. It is not sufficient input
/// authority by itself; every observation also carries a core-minted
/// [`NativeSurfaceLease`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct HostWindowToken(u64);

impl HostWindowToken {
    /// Creates a stable adapter token without exposing an operating-system handle.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    const fn into_core(self) -> WindowToken {
        WindowToken::new(self.0)
    }
}

/// Opaque exact-incarnation capability for one native docking surface.
///
/// The value is copyable so asynchronous callbacks may retain it. No accessor
/// exposes the core incarnation, workspace epoch, or authority domain.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeSurfaceLease {
    binding: ViewportBinding,
}

impl NativeSurfaceLease {
    pub(super) const fn from_binding(binding: ViewportBinding) -> Self {
        Self { binding }
    }

    /// Returns the stable logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.binding.surface()
    }

    /// Returns the reusable adapter-owned window token.
    #[must_use]
    pub const fn window_token(self) -> HostWindowToken {
        HostWindowToken(self.binding.token().get())
    }
}

/// Native close fact accepted by the narrow live-binding lane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeCloseState {
    /// The exact binding has no pending native close request.
    Clear,
    /// The exact binding has a pending native close request.
    Requested,
}

/// Complete facts supplied for one binding in a native platform snapshot.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeWindowFacts {
    state: NativeWindowFactState,
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum NativeWindowFactState {
    Ready {
        bounds: PhysicalRect,
        scale_factor: ScaleFactor,
    },
    Unavailable,
    Destroyed,
}

impl NativeWindowFacts {
    /// Reports a visible, input-receiving window with exact physical geometry.
    #[must_use]
    pub const fn ready(bounds: PhysicalRect, scale_factor: ScaleFactor) -> Self {
        Self {
            state: NativeWindowFactState::Ready {
                bounds,
                scale_factor,
            },
        }
    }

    /// Reports versioned Unknown facts while retaining the binding in inventory.
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            state: NativeWindowFactState::Unavailable,
        }
    }

    /// Reports exact destruction and removes the binding from the live inventory.
    #[must_use]
    pub const fn destroyed() -> Self {
        Self {
            state: NativeWindowFactState::Destroyed,
        }
    }
}

/// Retryable complete native snapshot minted by one session sidecar.
#[derive(Debug, Clone, PartialEq)]
pub struct NativePlatformSnapshot {
    provider: PlatformObservationLease,
    expected_epoch: WorkspaceEpoch,
    generation: u64,
    bindings: Vec<ViewportBinding>,
    snapshot: PlatformSnapshot,
}

/// Native lifecycle failure at the stable facade boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum NativePlatformError {
    /// No native provider was enrolled for this session.
    #[error("no facade-owned native platform provider is active")]
    ProviderUnavailable,
    /// The submitted snapshot belongs to a superseded provider.
    #[error("native snapshot belongs to a superseded platform provider")]
    ProviderSuperseded,
    /// A callback named an older binding incarnation for this logical surface.
    #[error("native surface {surface} lease is stale")]
    StaleSurface {
        /// Stable logical surface named by the stale capability.
        surface: SurfaceId,
    },
    /// A complete snapshot repeated one binding capability.
    #[error("native snapshot repeats surface {surface}")]
    DuplicateSurface {
        /// Duplicated logical surface.
        surface: SurfaceId,
    },
    /// A complete snapshot did not exactly cover the current binding roster.
    #[error("native snapshot roster is incomplete or contains a foreign surface")]
    IncompleteRoster,
    /// A snapshot was captured against an older workspace epoch.
    #[error("native snapshot belongs to an older workspace epoch")]
    StaleWorkspace,
    /// Provider-owned observation generations cannot advance without wrapping.
    #[error("native observation generation is exhausted")]
    GenerationExhausted,
    /// More than one complete native snapshot was supplied in a host frame.
    #[error("native platform snapshot was already submitted for this host frame")]
    SnapshotAlreadySubmitted,
    /// Snapshot generation is no longer the next provider-owned generation.
    #[error("native platform snapshot generation is stale")]
    SnapshotGenerationStale,
    /// The live binding roster changed after this snapshot was captured.
    #[error("native platform snapshot was captured for an older binding roster")]
    SnapshotRosterStale,
    /// An earlier close edge in this frame conflicts with the frozen snapshot generations.
    #[error("native close input must not precede a snapshot captured from the same boundary")]
    SnapshotOrderConflict,
    /// Facade-owned typed facts violated an internal platform invariant.
    #[error("facade-owned native platform data violated an internal invariant")]
    ProtocolInvariant,
}

#[derive(Debug)]
pub(super) struct RuntimeNativeState {
    provider: PlatformObservationLease,
    snapshot_generation: u64,
    bindings: BTreeMap<SurfaceId, NativeSurfaceLease>,
    close_generations: BTreeMap<ViewportBinding, u64>,
}

#[derive(Debug, Clone)]
pub(super) struct NativeSnapshotCommit {
    generation: u64,
}

#[derive(Debug)]
struct CompiledNativeWindow {
    is_live: bool,
    window: Option<ObservedWindow>,
    close: WindowCloseObservation,
}

impl RuntimeNativeState {
    fn new(provider: PlatformObservationLease) -> Self {
        Self {
            provider,
            snapshot_generation: 0,
            bindings: BTreeMap::new(),
            close_generations: BTreeMap::new(),
        }
    }

    pub(super) fn close_generations(&self) -> BTreeMap<ViewportBinding, u64> {
        self.close_generations.clone()
    }

    fn next_snapshot_generation(&self) -> Result<u64, NativePlatformError> {
        self.close_generations
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .max(self.snapshot_generation)
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)
    }

    fn validate_snapshot_roster(
        &self,
        observations: impl IntoIterator<Item = (NativeSurfaceLease, NativeWindowFacts)>,
    ) -> Result<BTreeMap<ViewportBinding, NativeWindowFacts>, NativePlatformError> {
        let mut supplied = BTreeMap::new();
        for (lease, facts) in observations {
            if self.bindings.get(&lease.surface()) != Some(&lease) {
                return Err(NativePlatformError::StaleSurface {
                    surface: lease.surface(),
                });
            }
            if supplied.insert(lease.binding, facts).is_some() {
                return Err(NativePlatformError::DuplicateSurface {
                    surface: lease.surface(),
                });
            }
        }
        if supplied.len() != self.bindings.len() {
            return Err(NativePlatformError::IncompleteRoster);
        }
        Ok(supplied)
    }

    fn capture_snapshot(
        &self,
        expected_epoch: WorkspaceEpoch,
        observations: impl IntoIterator<Item = (NativeSurfaceLease, NativeWindowFacts)>,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        let generation = self.next_snapshot_generation()?;
        let supplied = self.validate_snapshot_roster(observations)?;
        let snapshot = compile_platform_snapshot(generation, &supplied)?;
        Ok(NativePlatformSnapshot {
            provider: self.provider,
            expected_epoch,
            generation,
            bindings: supplied.into_keys().collect(),
            snapshot,
        })
    }

    pub(super) fn commit(
        &mut self,
        engine: &crate::engine::DockEngine,
        transition: &EngineTransition,
        snapshot: Option<NativeSnapshotCommit>,
        close_generations: BTreeMap<ViewportBinding, u64>,
    ) {
        if let Some(snapshot) = snapshot {
            self.snapshot_generation = snapshot.generation;
        }
        self.close_generations = close_generations;
        for reduced in transition.reduced_inputs() {
            if let InputOutcome::ViewportRegistered { binding } = reduced.outcome() {
                self.bindings.insert(
                    binding.surface(),
                    NativeSurfaceLease::from_binding(*binding),
                );
            }
        }
        self.bindings.retain(|surface, lease| {
            engine
                .viewport()
                .viewport(*surface)
                .is_some_and(|record| record.binding() == lease.binding)
        });
        self.close_generations.retain(|binding, _| {
            self.bindings
                .get(&binding.surface())
                .is_some_and(|lease| lease.binding == *binding)
        });
    }
}

fn compile_platform_snapshot(
    generation: u64,
    supplied: &BTreeMap<ViewportBinding, NativeWindowFacts>,
) -> Result<PlatformSnapshot, NativePlatformError> {
    let reason = AuthorityUnavailableReason::NotReported;
    let mut live_bindings = Vec::new();
    let mut windows = Vec::new();
    let mut close_observations = Vec::new();
    for (&binding, &facts) in supplied {
        let compiled = compile_window_fact(binding, facts, generation);
        if compiled.is_live {
            live_bindings.push(binding);
        }
        if let Some(window) = compiled.window {
            windows.push(window);
        }
        close_observations.push(compiled.close);
    }

    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(generation),
            Authority::Known(capabilities),
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

fn compile_window_fact(
    binding: ViewportBinding,
    facts: NativeWindowFacts,
    generation: u64,
) -> CompiledNativeWindow {
    let coordinate_generation = CoordinateObservationGeneration::new(generation);
    let input_generation = InputObservationGeneration::new(generation);
    let presentation_generation = PresentationObservationGeneration::new(generation);
    let close_generation = CloseObservationGeneration::new(generation);
    let reason = AuthorityUnavailableReason::NotReported;

    match facts.state {
        NativeWindowFactState::Ready {
            bounds,
            scale_factor,
        } => CompiledNativeWindow {
            is_live: true,
            window: Some(
                ObservedWindow::new(binding)
                    .with_coordinate_observation(WindowCoordinateObservation::new(
                        binding,
                        coordinate_generation,
                        Authority::Known(bounds),
                        Authority::Known(bounds),
                        Authority::Known(scale_factor),
                        Authority::Known(scale_factor),
                    ))
                    .with_input_observation(WindowInputObservation::new(
                        binding,
                        input_generation,
                        Authority::Known(WindowInputState::ReceivesInput),
                        InputEffectAcknowledgement::known(None),
                    ))
                    .with_presentation_observation(WindowPresentationObservation::new(
                        binding,
                        presentation_generation,
                        Authority::Known(WindowPresentationState::Visible),
                        PresentationEffectAcknowledgement::known(None),
                    )),
            ),
            close: WindowCloseObservation::new(
                binding,
                close_generation,
                Authority::Known(WindowCloseState::LiveClear),
                CloseEffectAcknowledgement::known(None),
            ),
        },
        NativeWindowFactState::Unavailable => CompiledNativeWindow {
            is_live: true,
            window: Some(
                ObservedWindow::new(binding)
                    .with_coordinate_observation(WindowCoordinateObservation::new(
                        binding,
                        coordinate_generation,
                        Authority::Unknown(reason),
                        Authority::Unknown(reason),
                        Authority::Unknown(reason),
                        Authority::Unknown(reason),
                    ))
                    .with_input_observation(WindowInputObservation::new(
                        binding,
                        input_generation,
                        Authority::Unknown(reason),
                        InputEffectAcknowledgement::unknown(reason),
                    ))
                    .with_presentation_observation(WindowPresentationObservation::new(
                        binding,
                        presentation_generation,
                        Authority::Unknown(reason),
                        PresentationEffectAcknowledgement::unknown(reason),
                    )),
            ),
            close: WindowCloseObservation::new(
                binding,
                close_generation,
                Authority::Unknown(reason),
                CloseEffectAcknowledgement::unknown(reason),
            ),
        },
        NativeWindowFactState::Destroyed => CompiledNativeWindow {
            is_live: false,
            window: None,
            close: WindowCloseObservation::new(
                binding,
                close_generation,
                Authority::Known(WindowCloseState::Destroyed),
                CloseEffectAcknowledgement::known(None),
            ),
        },
    }
}

impl DockspaceSession {
    /// Enrolls the session-owned native platform observation source.
    ///
    /// Repeating this call is inert. Transport replacement remains outside this
    /// first facade slice and does not leak provider tickets through the API.
    ///
    /// # Errors
    ///
    /// Returns an error when the core cannot mint platform authority.
    pub fn enable_native_platform(&mut self) -> Result<(), DockspaceRuntimeError> {
        if self.native.is_some() {
            return Ok(());
        }
        let provider = self.engine.create_platform_provider()?;
        self.native = Some(RuntimeNativeState::new(provider));
        Ok(())
    }

    /// Returns the current exact native binding for one logical surface.
    #[must_use]
    pub fn native_surface(&self, surface: SurfaceId) -> Option<NativeSurfaceLease> {
        self.native.as_ref()?.bindings.get(&surface).copied()
    }

    /// Captures one retryable exact-set native platform snapshot.
    ///
    /// Every currently registered native surface must appear exactly once.
    /// Generation allocation remains tentative until the host frame commits.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable provider, stale or duplicate leases,
    /// an incomplete roster, or generation exhaustion.
    pub fn capture_native_snapshot(
        &self,
        observations: impl IntoIterator<Item = (NativeSurfaceLease, NativeWindowFacts)>,
    ) -> Result<NativePlatformSnapshot, NativePlatformError> {
        self.native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?
            .capture_snapshot(self.version().epoch(), observations)
    }
}

impl DockspaceHostFrame<'_> {
    /// Registers one externally owned native root window in caller input order.
    ///
    /// # Errors
    ///
    /// Returns an error when native authority is unavailable or the affine host
    /// frame rejects the lifecycle input.
    pub fn register_native_root(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
    ) -> Result<(), DockspaceRuntimeError> {
        let provider = self
            .session
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?
            .provider;
        let expected = self.frame.view().version();
        self.append(EngineInput::RegisterViewport {
            provider,
            expected,
            surface,
            token: token.into_core(),
            role: ViewportRole::Root,
            recovery_target: None,
        })
    }

    /// Publishes one complete native platform snapshot in caller input order.
    ///
    /// # Errors
    ///
    /// Returns an error for a stale provider, workspace epoch, generation, or a
    /// second snapshot in the same frame.
    pub fn publish_native_snapshot(
        &mut self,
        snapshot: NativePlatformSnapshot,
    ) -> Result<(), DockspaceRuntimeError> {
        if self.native_snapshot_commit.is_some() {
            return Err(NativePlatformError::SnapshotAlreadySubmitted.into());
        }
        let native = self
            .session
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        if native.provider != snapshot.provider {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        if snapshot.expected_epoch != self.frame.view().version().epoch() {
            return Err(NativePlatformError::StaleWorkspace.into());
        }
        let expected_generation = native
            .close_generations
            .values()
            .copied()
            .max()
            .unwrap_or(0)
            .max(native.snapshot_generation)
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)?;
        if expected_generation != snapshot.generation {
            return Err(NativePlatformError::SnapshotGenerationStale.into());
        }
        if snapshot.bindings.len() != native.bindings.len()
            || snapshot.bindings.iter().any(|binding| {
                native
                    .bindings
                    .get(&binding.surface())
                    .is_none_or(|lease| lease.binding != *binding)
            })
        {
            return Err(NativePlatformError::SnapshotRosterStale.into());
        }
        if snapshot.bindings.iter().any(|binding| {
            self.next_native_close_generations
                .get(binding)
                .is_some_and(|generation| *generation >= snapshot.generation)
        }) {
            return Err(NativePlatformError::SnapshotOrderConflict.into());
        }
        let commit = NativeSnapshotCommit {
            generation: snapshot.generation,
        };
        self.append(EngineInput::PublishPlatformSnapshot {
            provider: snapshot.provider,
            expected_epoch: snapshot.expected_epoch,
            snapshot: snapshot.snapshot,
        })?;
        for binding in &snapshot.bindings {
            self.next_native_close_generations
                .insert(*binding, snapshot.generation);
        }
        self.native_snapshot_commit = Some(commit);
        Ok(())
    }

    /// Publishes one exact live-binding native close observation.
    ///
    /// Stale callback capabilities are rejected before they can be redirected
    /// to a replacement binding which reused the same host token.
    ///
    /// # Errors
    ///
    /// Returns a typed stale-surface error or an affine host-frame failure.
    pub fn publish_native_close(
        &mut self,
        lease: NativeSurfaceLease,
        state: NativeCloseState,
    ) -> Result<(), DockspaceRuntimeError> {
        let native = self
            .session
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        if native.bindings.get(&lease.surface()) != Some(&lease) {
            return Err(NativePlatformError::StaleSurface {
                surface: lease.surface(),
            }
            .into());
        }
        let previous = self
            .next_native_close_generations
            .get(&lease.binding)
            .copied()
            .unwrap_or(0);
        let generation = previous
            .checked_add(1)
            .ok_or(NativePlatformError::GenerationExhausted)?;
        let state = match state {
            NativeCloseState::Clear => WindowCloseState::LiveClear,
            NativeCloseState::Requested => WindowCloseState::LiveRequested,
        };
        self.append(EngineInput::PublishNativeCloseObservation {
            provider: native.provider,
            expected_epoch: self.frame.view().version().epoch(),
            observation: WindowCloseObservation::new(
                lease.binding,
                CloseObservationGeneration::new(generation),
                Authority::Known(state),
                CloseEffectAcknowledgement::known(None),
            ),
        })?;
        self.next_native_close_generations
            .insert(lease.binding, generation);
        Ok(())
    }
}
