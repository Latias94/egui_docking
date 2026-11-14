//! Session orchestration for native facts and effects.

use super::*;
use crate::input::intent::Authority;
use crate::input::journal::PointerEdgeSequence;

impl DockspaceSession {
    /// Returns each committed real native window exactly once.
    pub fn native_window_bindings(&self) -> impl Iterator<Item = NativeWindowBinding> + '_ {
        let native = self.native.as_ref();
        native.into_iter().flat_map(|native| {
            native
                .producer
                .windows
                .iter()
                .filter(|state| state.current)
                .map(move |state| {
                    NativeWindowBinding::from_binding(native.provider(), state.binding)
                })
        })
    }
    /// Returns the exact committed surface placements hosted by a real window.
    pub fn native_surface_bindings_for_window(
        &self,
        window: NativeWindowBinding,
    ) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        let provider = self.native.as_ref().map(RuntimeNativeState::provider);
        self.engine
            .viewport()
            .registry()
            .bindings_for_window(window.window)
            .filter_map(move |binding| {
                (provider == Some(window.provider))
                    .then(|| NativeSurfaceBinding::from_binding(window.provider, binding))
            })
    }

    /// Queries only the exact committed placement; stale generations return none.
    #[must_use]
    pub fn native_surface_placement(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativeSurfacePlacement> {
        let native = self.native.as_ref()?;
        if binding.provider != native.provider() {
            return None;
        }
        self.engine
            .viewport()
            .registry()
            .surface_placement(binding.binding)
    }

    /// Returns whether native creation still owns a preparation or first-live obligation.
    ///
    /// The answer is derived from the existing lifecycle coordinator. Source
    /// ownership stays unchanged until exact first-live presentation admits the
    /// child. A drag-out transfers its content at that boundary; an existing
    /// logical surface retains its complete topology. Cancellation settles the
    /// same owner.
    #[must_use]
    pub fn has_pending_native_create(&self) -> bool {
        self.engine
            .viewport()
            .waiting_native_surfaces()
            .next()
            .is_some()
            || self
                .engine
                .viewport()
                .native_create_sagas()
                .next()
                .is_some()
            || self
                .engine
                .viewport()
                .pending_recoveries()
                .any(|(_, pending)| {
                    matches!(
                        pending.origin(),
                        crate::platform::viewport::NativeSurfaceEstablishmentOrigin::Initial { .. }
                    ) && pending.replacement_binding().is_some()
                })
    }

    /// Returns the cumulative number of records accepted by this native producer.
    ///
    /// Hosts may compare this diagnostic counter around callback translation to
    /// account for host-only work without charging sequenced records twice.
    /// It is not an input capability, commit acknowledgement, or replay cursor.
    #[must_use]
    pub fn native_input_recorded_count(&self) -> u64 {
        self.native
            .as_ref()
            .map_or(0, |native| native.recorder.recorded_through().get())
    }

    /// Returns whether recorded native facts still await a successful frame commit.
    ///
    /// This reports recorder state, without consuming input or granting a commit
    /// capability. Pending renderer/effect reports which have not been recorded
    /// are separate; host frames retain their ordinary pending-input barrier.
    #[must_use]
    pub fn has_pending_native_input(&self) -> bool {
        self.native.as_ref().is_some_and(|native| {
            native.recorder.recorded_through() > self.engine.backend_ingress_committed_through()
        })
    }

    /// Reclaims committed transport and reports how many pointer edges can be prepared.
    ///
    /// Returns `None` for unlimited recording. A finite zero means preceding
    /// accepted records need to commit before more pointer preparation can fit.
    /// This maintenance step must precede caller iteration; the recording APIs
    /// still validate every actual append. A suspended replay retains its data
    /// and receives no retirement credit.
    ///
    /// # Errors
    ///
    /// Returns an error without a managed host, on failed prefix retirement, or
    /// when the empty configured transport cannot accommodate even one edge.
    pub fn prepare_native_pointer_capacity(
        &mut self,
    ) -> Result<Option<usize>, DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        let native = self.native_state_mut()?;
        let capacity = native
            .recorder
            .native_pointer_preparation_limit(None)
            .map_err(NativePlatformError::from_ingress)?;
        if capacity == Some(0) && native.recorder.retained_record_count() == 0 {
            return Err(NativePlatformError::InputCapacity.into());
        }
        Ok(capacity)
    }

    fn native_state_mut(&mut self) -> Result<&mut RuntimeNativeState, DockspaceRuntimeError> {
        let native = self
            .native
            .as_mut()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        if native.pending_replay.is_none() {
            // A suspended candidate retains its original retention authority.
            let retirement = native.reclaim_committed_prefix(&mut self.engine);
            self.presentation.reclaim_compacted_streams(&self.engine);
            retirement?;
        }
        Ok(native)
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

    /// Returns whether this exact binding may currently report a global focus fact.
    ///
    /// Visible pending children may report focus before first-live admission.
    /// A callback captured before hiding, closing or retirement must revalidate
    /// this query before recording its formerly known docking focus.
    #[must_use]
    pub fn native_binding_can_report_focus(&self, binding: impl Into<NativeWindowBinding>) -> bool {
        let binding = binding.into();
        self.native
            .as_ref()
            .is_some_and(|native| native.recognizes_window_binding(binding))
            && self
                .engine
                .viewport()
                .registry()
                .window(binding.window)
                .is_some_and(|record| record.can_report_focus_fact())
    }

    /// Returns native client-coordinate authority from the committed engine state.
    ///
    /// The exact native binding must be admitted and routable through this joined
    /// backend's active presentation host, with positive local size and complete
    /// scales. Desktop origin and work-area authority are not required. Recorded
    /// but uncommitted snapshots cannot grant or update this capability.
    #[must_use]
    pub fn native_local_coordinates(
        &self,
        binding: impl Into<NativeWindowBinding>,
    ) -> Option<NativeLocalCoordinates> {
        let binding = binding.into();
        let native = self.native.as_ref()?;
        let backend = native.recorder.lease();
        if native.profile != NativeHostProfile::ManagedDesktop
            || self.engine.backend_ingress_provider() != Some(backend)
            || !native.contains_window_binding(binding)
        {
            return None;
        }
        let snapshot = self
            .engine
            .native_window_coordinate_snapshot(backend.presentation_host(), binding.window)?;
        Some(NativeLocalCoordinates::new(backend, binding, snapshot))
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

    /// Returns every current or retired exact placement retained for this window.
    ///
    /// The physical callback's original correlation tag may no longer belong to
    /// the placement roster. After its real destruction and all host references
    /// drain, the host must report each retained binding's quiescence before
    /// releasing the physical lifetime. This query grants no input authority.
    pub fn retained_native_surface_bindings_for_window(
        &self,
        window: super::NativeWindowBinding,
    ) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.native
            .iter()
            .filter(move |native| native.provider() == window.provider())
            .flat_map(|native| {
                native.producer.bindings.iter().map(move |state| {
                    NativeSurfaceBinding::from_binding(native.provider(), state.binding)
                })
            })
            .filter(move |binding| binding.window() == window)
    }

    /// Returns whether the host retains this exact live or retired real window.
    ///
    /// Placement changes do not revoke physical callbacks. A retired window
    /// remains recognized until its exact lifecycle obligations are quiescent.
    #[must_use]
    pub fn recognizes_native_window_binding(&self, binding: NativeWindowBinding) -> bool {
        self.native
            .as_ref()
            .is_some_and(|native| native.recognizes_window_binding(binding))
    }

    /// Returns the exact destroyed source of a pending replacement request.
    ///
    /// The recovery owner, rather than a surface label on a platform route,
    /// correlates the replacement with its original physical window lifetime.
    #[must_use]
    pub fn native_replacement_predecessor(
        &self,
        replacement: NativeSurfaceBinding,
    ) -> Option<NativeSurfaceBinding> {
        let native = self.native.as_ref()?;
        if replacement.provider != native.provider() {
            return None;
        }
        self.engine
            .viewport()
            .pending_recoveries()
            .find_map(|(_, pending)| {
                if pending.replacement_binding() != Some(replacement.binding) {
                    return None;
                }
                match pending.origin() {
                    crate::platform::viewport::NativeSurfaceEstablishmentOrigin::Destroyed {
                        binding,
                    } => Some(NativeSurfaceBinding::from_binding(
                        native.provider(),
                        binding,
                    )),
                    crate::platform::viewport::NativeSurfaceEstablishmentOrigin::Initial {
                        ..
                    } => None,
                }
            })
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
        self.enroll_native_host(NativeHostProfile::ObservedRoots, None, None)
    }

    /// Enrolls the sole joined coordinator for managed native windows.
    ///
    /// Window inventory, pointer facts, and typed effect settlement share one
    /// causal input order. Desktop pointer input requires independent desktop
    /// facts; native-local input instead uses committed client geometry from
    /// [`Self::native_local_coordinates`]. The initial pointer roster is enrolled
    /// in the same operation; no platform or pointer record can precede it.
    ///
    /// The host owns its native preparation process and reports exact, revocable
    /// [`NativeCreateReadiness::Prepared`] facts. Source ownership remains published
    /// until the exact semantic first-live output is observed as presented.
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
        self.enroll_native_host(NativeHostProfile::ManagedDesktop, Some(checkpoint), None)
    }

    /// Enrolls a managed coordinator with bounded recorded-input storage.
    ///
    /// Uses the same capability and initial-roster contract as
    /// [`Self::enable_managed_native_host`]. The selected allowance reserves
    /// recorder and replay transport for the complete admitted record set.
    /// Full storage rejects new records with [`NativeHostErrorKind::InputCapacity`];
    /// the host must retain their original owners, drain accepted input, and retry.
    ///
    /// # Errors
    ///
    /// Returns the enrollment errors of [`Self::enable_managed_native_host`], or
    /// an input-capacity error when the allowance cannot hold the transport and
    /// initial pointer checkpoint. A failed enrollment publishes no provider.
    pub fn enable_managed_native_host_with_input_storage_limit(
        &mut self,
        initial_pointer_roster: NativePointerRoster,
        limit: NativeInputStorageLimit,
    ) -> Result<Vec<NativeSurfaceBinding>, DockspaceRuntimeError> {
        if let Some(reports) = limit.report_storage_limit() {
            reports.validate()?;
        }
        if let Some(producer) = limit.producer_storage_limit() {
            producer.validate()?;
        }
        crate::input::ingress::BackendIngressRecorder::preflight_native_transport_configuration(
            limit.record_slots(),
            limit.byte_limit(),
            1,
            pointer::pointer_authority_descendant_bytes(&initial_pointer_roster)?,
        )
        .map_err(NativePlatformError::from_ingress)?;
        let checkpoint = pointer::compile_initial_pointer_authority(initial_pointer_roster)?;
        self.enroll_native_host(
            NativeHostProfile::ManagedDesktop,
            Some(checkpoint),
            Some(limit),
        )
    }

    /// Reports a freshly observed complete pointer roster after a managed input reset.
    ///
    /// Overload cancellation and uncorrelated device removal both open this
    /// recovery boundary without claiming a complete physical roster.
    ///
    /// Earlier native records must be committed before reporting. The runtime
    /// binds the observation to its exact current provider and pointer watermark;
    /// authority changes only when a subsequent host frame commits. An unknown
    /// roster cannot prove that held buttons were released and keeps recovery
    /// available. Recovery remains available across later pointer edges until
    /// one complete roster commits. Native capture must name a current binding.
    ///
    /// # Errors
    ///
    /// Returns an error for a nonmanaged or superseded provider, pending native
    /// input, invalid facts, or an unavailable resynchronization boundary.
    pub fn report_native_pointer_roster(
        &mut self,
        roster: NativePointerRoster,
    ) -> Result<(), DockspaceRuntimeError> {
        let native = self
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        if native.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch.into());
        }
        if self.engine.backend_ingress_provider() != Some(native.recorder.lease()) {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        if self.engine.backend_ingress_committed_through() != native.recorder.recorded_through() {
            return Err(NativePlatformError::InputPending.into());
        }
        let provider = native.recorder.lease();
        let checkpoint = self.native_state_mut()?.compile_pointer_roster(roster)?;
        self.engine
            .validate_native_pointer_roster_resync(provider, &checkpoint)?;
        self.native_state_mut()?
            .record_pointer_authority(checkpoint)?;
        Ok(())
    }

    /// Configures the immutable capabilities of the attached managed backend.
    ///
    /// The native coordinator should call this after the real windowing backend
    /// is known and before publishing its first complete managed snapshot.
    /// Repeating the same roster is idempotent; changing it during one provider
    /// lifetime is rejected.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile or a conflicting roster.
    pub fn configure_managed_native_capabilities(
        &mut self,
        capabilities: NativeHostCapabilities,
    ) -> Result<(), DockspaceRuntimeError> {
        self.native_state_mut()?
            .configure_managed_capabilities(capabilities)?;
        Ok(())
    }

    fn enroll_native_host(
        &mut self,
        profile: NativeHostProfile,

        pointer_checkpoint: Option<crate::input::journal::PointerAuthorityCheckpoint>,
        storage_limit: Option<NativeInputStorageLimit>,
    ) -> Result<Vec<NativeSurfaceBinding>, DockspaceRuntimeError> {
        if self.pointer.is_some() {
            return Err(NativePlatformError::PointerProviderAlreadyEnabled.into());
        }
        if self.native.is_some() {
            return Err(NativePlatformError::ProviderAlreadyEnabled.into());
        }
        let producer_storage_limit =
            storage_limit.and_then(NativeInputStorageLimit::producer_storage_limit);
        // Admit the complete existing registry before the provider initializer
        // can publish either platform or pointer authority.
        let producer = NativeProducerState::prepare(&self.engine, None, producer_storage_limit)?;
        let mut initial_bindings = storage::producer_buffer(producer.bindings.len())?;
        let report_storage = storage_limit
            .and_then(NativeInputStorageLimit::report_storage_limit)
            .map(|limit| {
                limit.validate()?;
                if !self.presentation.is_quiescent()
                    || !self.abandoned_native_effects.is_quiescent()
                {
                    return Err(NativePlatformError::InputPending);
                }
                limit.validate_replacement(
                    self.presentation
                        .owned_data_capacity_bytes()
                        .saturating_add(self.abandoned_native_effects.owned_data_capacity_bytes()),
                )?;
                Ok((
                    limit,
                    super::super::presentation::RuntimePresentationState::with_capacity_limit(
                        limit.presentation_slots().get(),
                    )?,
                    super::super::native_effect::NativeEffectDropQueue::with_capacity_limit(
                        limit.effect_slots().get(),
                    )?,
                ))
            })
            .transpose()?;
        let watermark = PointerEdgeSequence::new(0);
        let checkpoint_journal = pointer_checkpoint
            .map(|checkpoint| {
                crate::input::journal::PointerEdgeJournal::new(watermark, watermark, Vec::new())
                    .and_then(|journal| journal.with_authority_checkpoint(checkpoint))
                    .map_err(|_| NativePlatformError::InvalidPointerFacts)
            })
            .transpose()?;
        let recorder = self
            .engine
            .create_backend_ingress_provider_initialized(
                self.presentation_host,
                watermark,
                |recorder| {
                    if let Some(limit) = storage_limit {
                        recorder.configure_native_transport_capacity(
                            limit.record_slots(),
                            limit.byte_limit(),
                        )?;
                    }
                    if let Some(journal) = checkpoint_journal {
                        recorder.record_pointer_segment(journal)?;
                    }
                    Ok(())
                },
            )
            .map_err(|error| match error {
                crate::engine::EngineError::BackendIngress { source } => {
                    DockspaceRuntimeError::from(NativePlatformError::from_ingress(source))
                }
                error => error.into(),
            })?;
        let native = RuntimeNativeState::new(recorder, profile, producer, producer_storage_limit);
        initial_bindings.extend(native.current_bindings());
        initial_bindings.sort_unstable_by_key(|binding| binding.surface());
        self.native = Some(native);
        if let Some((limit, presentation, effects)) = report_storage {
            self.presentation = presentation;
            self.abandoned_native_effects = effects;
            self.native_report_storage_limit = Some(limit);
        }
        Ok(initial_bindings)
    }

    /// Records one exact-set native platform snapshot in backend order.
    ///
    /// Every currently registered real native window must appear exactly once.
    /// Validation and recording are one atomic facade operation. A rejected
    /// roster cannot be published later against a different workspace or
    /// binding incarnation.
    ///
    /// # Errors
    ///
    /// Returns an error for an unavailable provider, stale or duplicate bindings,
    /// an incomplete roster, or generation exhaustion.
    pub fn report_native_snapshot<B: Into<NativeWindowBinding>>(
        &mut self,
        observations: impl IntoIterator<Item = (B, NativeWindowFacts)>,
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
    /// Returns an error when capabilities have not been configured, for the
    /// wrong host profile, stale or incomplete window bindings, or an invalid
    /// exact work-area roster.
    pub fn report_managed_native_snapshot<B: Into<NativeWindowBinding>>(
        &mut self,
        observations: impl IntoIterator<Item = (B, NativeWindowFacts)>,
        work_areas: NativeWorkAreaRoster,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if native.profile != NativeHostProfile::ManagedDesktop {
            return Err(NativePlatformError::HostProfileMismatch.into());
        }
        if native.managed_capabilities.is_none() {
            return Err(NativePlatformError::CapabilityRosterUnavailable.into());
        }
        native.record_snapshot_facts(expected_epoch, observations, work_areas)?;
        Ok(())
    }

    /// Records one globally consistent native-focus fact in backend order.
    ///
    /// `Unknown` explicitly revokes prior focus authority. Losing focus from
    /// one window does not by itself prove `Foreign` or `None`; hosts should
    /// report those variants only when their platform integration can prove
    /// the global state.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile, a stale docking binding,
    /// a superseded provider, or generation exhaustion.
    pub fn report_native_global_focus(
        &mut self,
        focus: NativeGlobalFocus,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected_epoch = self.version().epoch();
        self.native_state_mut()?
            .record_global_focus(expected_epoch, focus)?;
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
            .and_then(|native| native.work_area_binding(token.into_core()))
    }

    /// Records one desktop or client-local native pointer edge in host event order.
    ///
    /// The runtime assigns the provider sequence and retains the edge until a
    /// host frame resolves its receiver query and commits. A failed frame does
    /// not consume the edge.
    ///
    /// Historical facts may name an exact recognized window incarnation after
    /// its admission ends and before quiescence. The reducer checks current
    /// routing and action authority at the edge's position in the input order.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile, foreign or forgotten window
    /// bindings, stale work-area bindings, contradictory facts, or sequence exhaustion.
    pub fn record_native_pointer(
        &mut self,
        input: NativePointerInput,
    ) -> Result<(), DockspaceRuntimeError> {
        self.native_state_mut()?.record_pointer(input)?;
        Ok(())
    }

    /// Records one causally indivisible batch of native pointer edges.
    ///
    /// The runtime assigns consecutive provider sequences in iterator order.
    /// If any input is rejected, every edge appended by this call is rolled
    /// back so the adapter can retry from the exact preceding boundary.
    /// Admission and host-frame commit are both atomic: no commit boundary
    /// occurs between these edges. Use [`Self::record_native_pointer_sequence`]
    /// for independent records that only require all-or-none admission.
    /// A bounded host rejects at the first item which cannot fit without reading
    /// the remaining iterator. Caller iteration and its destructor finish before
    /// any committed data is reclaimed or any record is appended.
    ///
    /// Window lifetime acceptance follows [`Self::record_native_pointer`];
    /// historical facts do not grant current routing or action authority.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile, foreign or forgotten window
    /// bindings, stale work-area bindings, contradictory facts, or sequence exhaustion.
    pub fn record_native_pointer_batch(
        &mut self,
        inputs: impl IntoIterator<Item = NativePointerInput>,
    ) -> Result<(), DockspaceRuntimeError> {
        self.record_native_pointer_journals(inputs, true)
    }

    /// Records native pointer edges with all-or-none admission in iterator order.
    ///
    /// The runtime prepares the iterator before appending any edge and assigns
    /// consecutive provider sequences. Invalid facts or a full bounded allowance
    /// reject the entire call without accepting an edge. Capacity rejection stops
    /// at the first unaccepted item, without reading the remaining iterator.
    /// Unlike [`Self::record_native_pointer_batch`], each admitted record retains
    /// its own commit boundary, so host frames may commit a prefix of the
    /// sequence and replay its remaining suffix.
    ///
    /// Window lifetime acceptance follows [`Self::record_native_pointer`];
    /// historical facts do not grant current routing or action authority.
    ///
    /// # Errors
    ///
    /// Returns an error for the wrong host profile, foreign or forgotten window
    /// bindings, stale work-area bindings, contradictory facts, or sequence exhaustion.
    pub fn record_native_pointer_sequence(
        &mut self,
        inputs: impl IntoIterator<Item = NativePointerInput>,
    ) -> Result<(), DockspaceRuntimeError> {
        self.record_native_pointer_journals(inputs, false)
    }

    fn record_native_pointer_journals(
        &mut self,
        inputs: impl IntoIterator<Item = NativePointerInput>,
        atomic_commit: bool,
    ) -> Result<(), DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        let native = self
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        let reclaimable = if native.pending_replay.is_none() {
            self.engine.backend_ingress_commit_watermark()
        } else {
            None
        };
        // Caller iteration and its destructor finish before maintenance or append.
        let journals = {
            let mut inputs = inputs.into_iter();
            native.prepare_pointer_journals(&mut inputs, reclaimable)?
        };
        if journals.is_empty() {
            return Ok(());
        }
        self.native_state_mut()?
            .append_prepared_pointer_journals(journals, atomic_commit)?;
        Ok(())
    }

    /// Records an uncorrelated native input-device removal in backend order.
    ///
    /// A backend which cannot correlate its raw device identity with delivered
    /// pointer/scroll identities must conservatively end every live stream in
    /// the current managed provider. This captures one fixed-size fact, without
    /// synthesizing a pointer edge for each stream. The pointer edge watermark,
    /// provider incarnation and accepted release/output obligations survive.
    /// Later backend records may follow in the same host frame.
    ///
    /// Button/capture facts become Unknown; this does not report an empty
    /// physical roster. Fresh complete facts may be reported through
    /// [`Self::report_native_pointer_roster`]. A rejected or dropped frame leaves
    /// the recorded removal retryable until its exact backend prefix commits.
    ///
    /// # Errors
    ///
    /// Returns an error without a managed native host, when committed transport
    /// cannot be reclaimed, or when the producer cannot record the observation.
    pub fn report_native_input_device_removal(&mut self) -> Result<(), DockspaceRuntimeError> {
        self.ensure_managed_native_host_available()?;
        self.native_state_mut()?
            .recorder
            .record_native_input_device_removal()
            .map_err(NativePlatformError::from_ingress)?;
        Ok(())
    }

    /// Records the input terminal boundary of one exact real native window.
    ///
    /// A destroyed window can no longer own held buttons, capture or a smooth
    /// scroll sequence. One fixed-size backend fact ends only input which
    /// depends on this window, including its retained provider scroll tails.
    /// Unknown capture reports preserve their preceding binding dependency.
    /// Other windows and already accepted release/output obligations survive.
    ///
    /// This does not publish window destruction or declare callback quiescence.
    /// Those lifecycle facts retain their separate exact commit barriers. The
    /// pointer watermark and provider incarnation do not change. Rejected or
    /// dropped frames replay the recorded fact at its original backend position.
    ///
    /// # Errors
    ///
    /// Returns an error without a managed host, for a foreign or unrecognized
    /// binding, or when the selected transport cannot admit the fact. Rejection
    /// leaves the caller responsible for its original platform callback.
    pub fn report_native_input_binding_retired(
        &mut self,
        binding: impl Into<NativeWindowBinding>,
    ) -> Result<(), DockspaceRuntimeError> {
        let binding = binding.into();
        self.ensure_managed_native_host_available()?;
        let native = self
            .native
            .as_ref()
            .ok_or(NativePlatformError::ProviderUnavailable)?;
        if binding.provider != native.provider() {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        // The retained window owns this transport carrier independently of
        // whether its original logical placement is still current.
        let observation_binding = native
            .window_observation_binding(binding)
            .ok_or(NativePlatformError::StaleWindow { binding })?;
        self.native_state_mut()?
            .recorder
            .record_native_input_binding_retired(observation_binding)
            .map_err(NativePlatformError::from_ingress)?;
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
        binding: impl Into<NativeWindowBinding>,
        state: NativeCloseState,
        acknowledgement: Option<NativeCloseEffectAcknowledgement>,
    ) -> Result<(), DockspaceRuntimeError> {
        let window = binding.into();
        let expected_epoch = self.version().epoch();
        let native = self.native_state_mut()?;
        if !native.contains_window_binding(window) {
            return Err(NativePlatformError::StaleWindow { binding: window }.into());
        }
        let binding = native
            .producer
            .window(window.window)
            .ok_or(NativePlatformError::ProtocolInvariant)?
            .observation_binding;
        let generation = native
            .producer
            .window(window.window)
            .ok_or(NativePlatformError::ProtocolInvariant)?
            .close_generation
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
            Some(acknowledgement) if acknowledgement.binding.window() != window.window => {
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
                binding,
                CloseObservationGeneration::new(generation),
                Authority::Known(state),
                acknowledgement,
            ),
        )?;
        if let Some(state) = native.producer.window_mut(window.window) {
            state.close_generation = generation;
        }
        Ok(())
    }

    /// Records one negative or indeterminate result for an emitted native effect.
    ///
    /// A definitive failure of the original create may finish its exact unobserved
    /// appearance wait after a layout reset. The delivered provider and immutable
    /// request epoch remain checked; observed windows and later cleanup lineages
    /// cannot use this exception.
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
        let Some(native) = self.native.as_ref() else {
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
        if !native.recognizes_window_binding(result.binding().window()) {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::StaleSurface { surface },
                result,
            ));
        }
        if !native.contains_window_binding(result.binding().window())
            && result.receipt_epoch() != current_epoch
            && !self
                .engine
                .viewport()
                .accepts_retired_create_failure(result.result)
        {
            return Err(NativeEffectSubmissionError::new(
                NativePlatformError::StaleSurface { surface },
                result,
            ));
        }
        let native = match self.native_state_mut() {
            Ok(native) => native,
            Err(error) => {
                let reason = if error.native_kind() == Some(NativeHostErrorKind::InputCapacity) {
                    NativePlatformError::InputCapacity
                } else {
                    NativePlatformError::ProtocolInvariant
                };
                return Err(NativeEffectSubmissionError::new(reason, result));
            }
        };
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
    /// producer rejects the semantic record. A finite producer rejects a new
    /// lifetime before appending when current, retired and queued registrations
    /// exhaust its slots.
    pub fn register_native_root(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let committed = self.engine.backend_ingress_committed_through();
        self.native_state_mut()?.record_root_registration(
            expected,
            committed,
            surface,
            token.into_core()?,
        )?;
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
    /// producer rejects the bootstrap record. A finite producer checks lifetime
    /// capacity before appending, together with queued root and child
    /// registrations. Semantic rejection is reported
    /// by the next [`HostFrameReport`](crate::runtime::HostFrameReport).
    pub fn register_owned_native_child(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
        recovery_host: SurfaceId,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let committed = self.engine.backend_ingress_committed_through();
        self.native_state_mut()?.record_child_bootstrap(
            expected,
            committed,
            surface,
            token.into_core()?,
            crate::engine::surface_recovery::SurfaceRecoveryBootstrap::new(recovery_host),
        )?;
        Ok(())
    }

    /// Records a window's targeting directive in the native input order.
    ///
    /// This changes no pointer position, button or capture fact. The reducer
    /// applies it only to an active gesture from the same provider when this
    /// exact current window is its source or the known keyboard-focus window.
    /// Inactive or retired windows are inert. Enabling allows fresh acquisition
    /// on a later pointer edge and cannot revive an old preview. Already frozen
    /// releases retain their original decision.
    ///
    /// # Errors
    ///
    /// Returns an error without a managed host, for a foreign provider, or when
    /// the existing recorded-input allowance cannot retain this semantic record.
    pub fn report_native_docking_enabled(
        &mut self,
        binding: impl Into<NativeWindowBinding>,
        enabled: bool,
    ) -> Result<(), DockspaceRuntimeError> {
        let binding = binding.into();
        self.ensure_managed_native_host_available()?;
        let native = self.native_state_mut()?;
        if binding.provider != native.provider() {
            return Err(NativePlatformError::ProviderSuperseded.into());
        }
        let observation_binding = native
            .window_observation_binding(binding)
            .ok_or(NativePlatformError::StaleWindow { binding })?;
        native
            .recorder
            .preflight_native_transport_append(1, size_of::<EngineInput>())
            .map_err(NativePlatformError::from_ingress)?;
        let provider = native.recorder.lease();
        native
            .recorder_mut()
            .record_semantic_input(EngineInput::UpdateNativeDockingEnabled {
                provider,
                binding: observation_binding,
                enabled,
            })
            .map_err(NativePlatformError::from_ingress)?;
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
        action: NativeSurfaceCloseAction,
    ) -> Result<(), DockspaceRuntimeError> {
        let expected = self.version();
        let native = self.native_state_mut()?;
        validate_close_request(native, close)?;
        native
            .recorder_mut()
            .record_semantic_input(EngineInput::RequestSurfaceClose {
                expected,
                edge: close.edge,
                request: action.into_request(),
            })
            .map_err(NativePlatformError::from_ingress)?;
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
            .map_err(NativePlatformError::from_ingress)?;
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
    if !native.contains_window_binding(NativeWindowBinding::from_binding(
        close.provider,
        binding.window(),
    )) {
        return Err(NativePlatformError::StaleSurface {
            surface: binding.surface(),
        });
    }
    Ok(())
}
