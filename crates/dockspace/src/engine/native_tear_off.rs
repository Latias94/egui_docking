//! Shared native-create preparation for pointer and product entry points.

use super::*;

use crate::model::{DockspaceActionOutcome, DockspaceActionRejection, NativeWindowPlacement};

#[derive(Debug)]
pub(super) enum NativeRootCreateRejection {
    Command(CommandError),
    RootPending { source_surface: SurfaceId },
    RecoveryUnavailable { source_surface: SurfaceId },
}

impl NativeRootCreateRejection {
    pub(super) fn into_interaction_command_error(self) -> CommandError {
        match self {
            Self::Command(error) => error,
            Self::RootPending { source_surface } | Self::RecoveryUnavailable { source_surface } => {
                CommandError::SurfaceLifecycleFrozen {
                    surface: source_surface,
                }
            }
        }
    }
}

impl DockEngine {
    pub(super) fn reduce_product_native_tear_off(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        expected: WorkspaceVersion,
        root: RootId,
        placement: NativeWindowPlacement,
        policy: &DockPolicySnapshot,
    ) -> Result<InputOutcome, EngineError> {
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if policy.check_tear_off(TearOffPresentation::Native).is_err() {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::PolicyDenied,
                version: self.version,
            });
        }
        if !self
            .viewport
            .native_exact_placement_create_capability()
            .is_supported()
        {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::NativeUnavailable,
                version: self.version,
            });
        }

        let (source, payload, items) = match self.capture_product_root_payload(root) {
            Ok(payload) => payload,
            Err(reason) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason,
                    version: self.version,
                });
            }
        };
        let complete = self.capture_complete_root_payload(&payload);
        if !matches!(complete, Ok(Some(ref complete)) if complete == &source) {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::Conflict,
                version: self.version,
            });
        }

        let (source_surface, existing_floating) = match self.workspace.presentation_for_root(root) {
            Some(crate::RootPresentationOwner::Main { surface }) => (surface, None),
            Some(crate::RootPresentationOwner::Contained { surface, floating }) => {
                (surface, Some(floating))
            }
            None => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::Conflict,
                    version: self.version,
                });
            }
        };
        let Some(projection) = self.interaction_projection(source_surface) else {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::PresentationUnavailable {
                    surface: source_surface,
                },
                version: self.version,
            });
        };
        let minimum_size = match existing_floating {
            Some(floating) => projection
                .plan()
                .contained_record(floating)
                .filter(|record| record.root() == root)
                .map(crate::scene::ContainedRecord::minimum_size),
            None => projection
                .plan()
                .layout_facts()
                .and_then(|facts| facts.root(root))
                .map(|_| self.presentation_config().minimum_floating_size()),
        };
        let Some(minimum_size) = minimum_size else {
            return Ok(InputOutcome::ProductActionRejected {
                reason: DockspaceActionRejection::PresentationUnavailable {
                    surface: source_surface,
                },
                version: self.version,
            });
        };
        let source_presentation = projection.authority();
        let pane_focus = match self.freeze_payload_focus(&payload) {
            Ok(pane_focus) => pane_focus,
            Err(error) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(
                        ProductAction::TearOffRoot { root, placement },
                        &error,
                    )?,
                    version: self.version,
                });
            }
        };
        let reservation = match self.prepare_native_root_transfer_identities(existing_floating) {
            Ok(reservation) => reservation,
            Err(
                EngineError::PresentationSurfaceIdentityExhausted
                | EngineError::PresentationFloatingIdentityExhausted,
            ) => {
                return Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::IdentityExhausted,
                    version: self.version,
                });
            }
            Err(error) => return Err(error),
        };
        let command = WorkspaceCommand::RehomeRoot {
            source,
            target: RootPresentationTarget::NewSurface {
                surface: reservation.surface(),
            },
        };
        let proposal = crate::frame::NativeCreateProposal::new(
            reservation.surface(),
            placement.outer_rect(),
            ConvertedMainRecovery::new(root, reservation.floating(), minimum_size),
        );
        match self.start_native_root_create(
            cause,
            focus_causal,
            source_presentation,
            payload,
            command,
            proposal,
            pane_focus,
            policy,
        )? {
            Ok(request) => Ok(InputOutcome::ProductActionProcessed {
                outcome: DockspaceActionOutcome::NativeRootTearOffRequested {
                    root,
                    source_surface,
                    target_surface: request.binding().surface(),
                    items,
                },
                version: self.version,
            }),
            Err(NativeRootCreateRejection::RootPending { .. }) => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::Conflict,
                    version: self.version,
                })
            }
            Err(NativeRootCreateRejection::RecoveryUnavailable { .. }) => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: DockspaceActionRejection::NativeUnavailable,
                    version: self.version,
                })
            }
            Err(NativeRootCreateRejection::Command(error)) => {
                Ok(InputOutcome::ProductActionRejected {
                    reason: super::product_action::product_command_rejection(
                        ProductAction::TearOffRoot { root, placement },
                        &error,
                    )?,
                    version: self.version,
                })
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn start_native_root_create(
        &mut self,
        cause: ReductionCause,
        focus_causal: FocusCausalStamp,
        source_presentation: PresentedSurfaceAuthority,
        payload: MovePayload,
        command: WorkspaceCommand,
        proposal: crate::frame::NativeCreateProposal,
        pane_focus: PaneFocusDisposition,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<crate::frame::NativeCreateRequest, NativeRootCreateRejection>, EngineError>
    {
        let source_surface = source_presentation.surface();
        if self.native_create_reserves_root(proposal.root()) {
            return Ok(Err(NativeRootCreateRejection::RootPending {
                source_surface,
            }));
        }

        let recovery_anchor = self
            .surface_recovery_target(source_surface)
            .map(SurfaceRecoveryTarget::anchor)
            .or_else(|| self.root_recovery_anchor(source_surface));
        let Some(recovery_anchor) = recovery_anchor else {
            return Ok(Err(NativeRootCreateRejection::RecoveryUnavailable {
                source_surface,
            }));
        };
        let recovery_target =
            SurfaceRecoveryTarget::with_converted_main(recovery_anchor, proposal.converted_main());
        let future = match self.preflight_native_root_create_command(
            cause,
            source_surface,
            &command,
            policy,
        )? {
            Ok(future) => future,
            Err(error) => return Ok(Err(NativeRootCreateRejection::Command(error))),
        };
        if future.surface(recovery_target.host_surface()).is_none() {
            return Ok(Err(NativeRootCreateRejection::RecoveryUnavailable {
                source_surface,
            }));
        }
        let id = self.next_surface_recovery_obligation_id(self.last_input)?;
        let obligation = match self.authorize_surface_recovery_obligation(
            self.last_input,
            id,
            &future,
            proposal.surface(),
            recovery_target,
            policy,
        ) {
            Ok(obligation) => obligation,
            Err(error) => return Ok(Err(NativeRootCreateRejection::Command(error))),
        };
        let proposal_surface = proposal.surface();
        let proposal_root = proposal.root();
        let proposal_floating = proposal.converted_main().floating();
        let prepared = crate::frame::PreparedNativeCreate::new(
            source_presentation,
            payload,
            self.version.epoch(),
            command,
            proposal,
            obligation,
            focus_causal,
        );
        let request = self
            .viewport
            .start_native_create(prepared)
            .map_err(|source| EngineError::Viewport {
                input: self.last_input,
                source,
            })?;
        let _ = self.viewport_focus.reserve_activation_causal(
            request.saga(),
            focus_causal,
            ViewportActivationRequest::tear_off_committed(request.binding(), pane_focus),
        );
        self.presentation_identity.observe_native(
            proposal_surface,
            proposal_root,
            proposal_floating,
        );
        self.last_surface_recovery_obligation = id;
        Ok(Ok(request))
    }

    fn preflight_native_root_create_command(
        &self,
        cause: ReductionCause,
        source_surface: SurfaceId,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
    ) -> Result<Result<Workspace, CommandError>, EngineError> {
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&self.workspace, None, None)
        {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        let mut future = self.clone_workspace_candidate();
        let report = match WorkspaceTransaction::from_commands([command.clone()])
            .apply(&mut future, policy)
        {
            Ok(report) => report,
            Err(TransactionError::Command { index: 0, source })
                if source.is_expected_rejection() =>
            {
                return Ok(Err(source));
            }
            Err(source) => {
                return Err(EngineError::PointerInteractionInvariant {
                    cause,
                    detail: source.to_string(),
                });
            }
        };
        if report.outcomes().len() != 1 {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "native root create preflight produced a non-unit outcome roster",
            });
        }
        if let Some(surface) =
            self.first_workspace_publication_mismatch(&future, None, Some(source_surface))
        {
            return Ok(Err(CommandError::SurfaceLifecycleFrozen { surface }));
        }
        match self.stage_workspace_publication(future, policy) {
            Ok(publication) => Ok(Ok(publication.workspace)),
            Err(source) if source.is_expected_rejection() => Ok(Err(source)),
            Err(source) => Err(EngineError::PointerInteractionInvariant {
                cause,
                detail: source.to_string(),
            }),
        }
    }
}
