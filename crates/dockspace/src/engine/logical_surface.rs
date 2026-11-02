//! Incremental logical-surface membership in the existing workspace transaction.

use super::*;
use crate::model::LogicalSurfaceRejection;

impl DockEngine {
    pub(super) fn logical_surface_lifecycle_rejection(
        &self,
        command: &WorkspaceCommand,
    ) -> Option<LogicalSurfaceRejection> {
        let (surface, new_root) = match command {
            WorkspaceCommand::AttachEmptySurface { surface, root } => (*surface, Some(*root)),
            WorkspaceCommand::RetireEmptySurface { surface } => (*surface, None),
            _ => return None,
        };
        if self.viewport.viewport(surface).is_some() {
            return Some(LogicalSurfaceRejection::NativeBindingPresent);
        }
        let reservation_conflicts = |drag: &crate::interaction::ActiveDrag| {
            drag.journal_presentation_reservation
                .is_some_and(|reserved| {
                    reserved.surface == surface || new_root == Some(reserved.root)
                })
        };
        let active_reservation = self
            .interaction
            .active_drag_view()
            .and_then(|view| self.interaction.active_drag(view.session()).ok())
            .is_some_and(reservation_conflicts);
        let pending_reservation = self
            .pending_mutation
            .as_ref()
            .and_then(PendingMutation::drag_release)
            .is_some_and(|pending| reservation_conflicts(&pending.drag));
        let pending_mutation = self.pending_mutation.as_ref().is_some_and(|pending| {
            let source = match pending {
                PendingMutation::DragRelease(pending) => pending.drag.source_surface,
                PendingMutation::ContainedRelease(pending) => pending.transform.surface,
                PendingMutation::Rehome(pending) => pending.source_surface,
            };
            source == surface || pending.surface() == surface
        });
        let reserved_root = new_root.is_some_and(|root| {
            self.native_create_reserves_root(root)
                || self
                    .pending_mutation
                    .as_ref()
                    .and_then(PendingMutation::rehome)
                    .is_some_and(|pending| pending.root == root)
        });
        if active_reservation
            || pending_reservation
            || pending_mutation
            || reserved_root
            || self.surface_recovery.pending(surface).is_some()
            || self.viewport.has_surface_establishment(surface)
            || self
                .viewport
                .binding_retirements()
                .any(|(binding, _)| binding.surface() == surface)
            || self.bound_surface_recoveries.iter().any(|(source, bound)| {
                *source == surface || bound.obligation.target().host_surface() == surface
            })
            || self.viewport.native_create_sagas().any(|(_, saga)| {
                let prepared = saga.prepared();
                saga.binding().surface() == surface
                    || prepared.source_surface() == surface
                    || prepared.recovery_obligation().target().host_surface() == surface
            })
            || self.viewport.waiting_native_surfaces().any(|initial| {
                initial.request.surface == surface || initial.request.recovery_host == surface
            })
        {
            return Some(LogicalSurfaceRejection::RecoveryConflict);
        }
        None
    }

    pub(super) fn reduce_logical_surface_command(
        &mut self,
        input: InputSequence,
        expected: WorkspaceVersion,
        command: &WorkspaceCommand,
        policy: &DockPolicySnapshot,
        events: &mut Vec<WorkspaceEvent>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InputOutcome, EngineError> {
        let surface = match command {
            WorkspaceCommand::AttachEmptySurface { surface, .. }
            | WorkspaceCommand::RetireEmptySurface { surface } => *surface,
            _ => unreachable!("logical surface reduction only accepts membership commands"),
        };
        if expected != self.version {
            return Ok(InputOutcome::StaleRejected {
                expected,
                accepted_base: self.version,
            });
        }
        if let Some(reason) = self.logical_surface_lifecycle_rejection(command) {
            return Ok(InputOutcome::LogicalSurfaceRejected { surface, reason });
        }
        let outcome = self.reduce_workspace_command(
            input,
            expected,
            self.version,
            command,
            policy,
            events,
            interaction_events,
        )?;
        Ok(match outcome {
            InputOutcome::CommandProcessed {
                outcome: CommandOutcome::EmptySurfaceAttached { surface, root },
                version,
                ..
            } => InputOutcome::LogicalSurfaceAttached {
                surface,
                root,
                version,
            },
            InputOutcome::CommandProcessed {
                outcome: CommandOutcome::EmptySurfaceRetired { surface },
                version,
                ..
            } => InputOutcome::LogicalSurfaceRetired { surface, version },
            InputOutcome::CommandRejected { error, .. } => {
                let reason = match error {
                    CommandError::SurfaceIdCollision { .. } => {
                        LogicalSurfaceRejection::SurfaceAlreadyAttached
                    }
                    CommandError::RootIdCollision { root } => {
                        LogicalSurfaceRejection::RootAlreadyOwned { root }
                    }
                    CommandError::MissingSurface { .. } => {
                        LogicalSurfaceRejection::SurfaceUnavailable
                    }
                    CommandError::SurfaceNotEmpty { items, .. } => {
                        LogicalSurfaceRejection::SurfaceNotEmpty { items }
                    }
                    CommandError::SurfaceLifecycleFrozen { .. } => {
                        LogicalSurfaceRejection::RecoveryConflict
                    }
                    CommandError::Policy(_) => LogicalSurfaceRejection::PolicyDenied,
                    source => {
                        return Err(EngineError::Command {
                            input,
                            source: TransactionError::Command { index: 0, source },
                        });
                    }
                };
                InputOutcome::LogicalSurfaceRejected { surface, reason }
            }
            other => other,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::behavior_tests::support::{
        TestInputStream, TestPresentationHost, complete_host_frame_with_retained_or_unavailable,
        publish_surface, submit_input,
    };
    use crate::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    };

    #[test]
    fn surface_membership_keeps_epoch_and_unaffected_authority_and_mints_fresh_requirements() {
        let s1 = SurfaceId::new(1);
        let s2 = SurfaceId::new(2);
        let r2 = RootId::new(2);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            s1,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([])),
        )])
        .unwrap();
        let mut engine = DockEngine::new(layout.into_workspace(), DockPolicy::default()).unwrap();
        let mut host = TestPresentationHost::new(&mut engine);
        publish_surface(
            &mut engine,
            &mut host,
            s1,
            crate::geometry::LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
        );
        let epoch = engine.version.epoch();
        let source_authority = engine.interaction_authority(s1).unwrap();
        let source = StableInputSourceId::new(77);
        let expected = engine.version;
        submit_input(
            &mut engine,
            &mut host,
            source,
            EngineInput::AttachEmptySurface {
                expected,
                surface: s2,
                root: r2,
            },
        )
        .unwrap();
        assert_eq!(engine.interaction_authority(s1), Some(source_authority));
        let before = engine
            .presentation_requirements()
            .surface(s2)
            .unwrap()
            .ticket()
            .surface_requirement();
        for same_frame in [false, true] {
            if same_frame {
                let mut inputs = TestInputStream::resume(&engine, source);
                let mut frame = host.begin(&engine);
                let expected = frame.view().version();
                inputs
                    .append(
                        &mut frame,
                        EngineInput::RetireEmptySurface {
                            expected,
                            surface: s2,
                        },
                    )
                    .unwrap();
                let expected = frame.view().version();
                inputs
                    .append(
                        &mut frame,
                        EngineInput::AttachEmptySurface {
                            expected,
                            surface: s2,
                            root: r2,
                        },
                    )
                    .unwrap();
                complete_host_frame_with_retained_or_unavailable(&engine, &mut frame);
                host.finish(frame, &mut engine);
            } else {
                let expected = engine.version;
                submit_input(
                    &mut engine,
                    &mut host,
                    source,
                    EngineInput::RetireEmptySurface {
                        expected,
                        surface: s2,
                    },
                )
                .unwrap();
                assert!(
                    !engine
                        .presentation_authority
                        .surface_requirement_revisions
                        .contains_key(&s2)
                );
                let expected = engine.version;
                submit_input(
                    &mut engine,
                    &mut host,
                    source,
                    EngineInput::AttachEmptySurface {
                        expected,
                        surface: s2,
                        root: r2,
                    },
                )
                .unwrap();
            }
            let current = engine
                .presentation_requirements()
                .surface(s2)
                .unwrap()
                .ticket()
                .surface_requirement();
            assert!(current > before);
            assert_eq!(engine.version.epoch(), epoch);
            assert_eq!(engine.interaction_authority(s1), Some(source_authority));
            assert!(engine.interaction_authority(s2).is_none());
        }
    }
}
