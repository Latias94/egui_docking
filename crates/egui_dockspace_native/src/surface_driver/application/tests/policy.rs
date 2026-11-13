//! Complete policies use the existing configuration boundary and command slot.

use dockspace::close::ClosePlanTarget;
use dockspace::policy::DockItemRule;
use dockspace::runtime::DockspaceCloseRequestRejection;

use super::*;

#[test]
fn policy_requests_share_the_slot_and_publish_only_after_commit() {
    let mut state = state();
    let original = state.coordinator.session().policy().clone();
    let before = state.coordinator.session().version();
    let mut updated = original.clone();
    updated.set_allow_edge_split(false);
    updated.set_allow_native_surfaces(true);
    let policy = Box::new(updated.clone());
    let allocation = std::ptr::from_ref(policy.as_ref());
    let command = NativeCommand::SetPolicy { policy };
    state.request_command(command.clone()).unwrap();
    assert_eq!(state.coordinator.session().policy(), &original);
    assert_eq!(state.coordinator.session().version(), before);
    let error = state.request_command(command).unwrap_err();
    assert_eq!(error.reason(), NativeActionRequestError::Busy);
    let NativeCommand::SetPolicy { policy } = error.command() else {
        panic!("rejected admission must return the original policy command");
    };
    assert!(std::ptr::eq(policy.as_ref(), allocation));
    assert!(matches!(
        state.request_style(state.style.clone()),
        Err(crate::NativeStyleRequestError::Queue(
            NativeActionRequestError::Busy
        ))
    ));
    assert!(state.take_action_status().is_none());

    let report = commit_queued_report(&mut state);
    assert_eq!(state.coordinator.session().policy(), &updated);
    assert_ne!(report.after(), before);
    let error = state.request_command(error.into_command()).unwrap_err();
    assert_eq!(error.reason(), NativeActionRequestError::Busy);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::PolicyChanged {
            changed: true,
            version: report.after(),
        }
    );
    state.request_command(error.into_command()).unwrap();
    let unchanged = commit_queued_report(&mut state);
    assert_eq!(unchanged.after(), report.after());
    assert_eq!(
        take(&mut state),
        NativeActionStatus::PolicyChanged {
            changed: false,
            version: report.after(),
        }
    );
}

#[test]
fn per_pane_policy_replacement_preserves_other_panes_and_lock_updates() {
    let mut state = state();
    let mut updated = state.coordinator.session().policy().clone();
    let mut pinned = DockItemRule::new();
    pinned.set_source_enabled(false);
    pinned.set_close_capability(Some(CloseCapability::Disabled));
    updated.set_item_rule(FIRST, pinned);
    state
        .request_command(NativeCommand::SetPolicy {
            policy: Box::new(updated.clone()),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::PolicyChanged { changed: true, .. }
    ));
    for locked in [true, false] {
        state
            .request_command(NativeCommand::SetArrangementLocked { locked })
            .unwrap();
        commit_queued(&mut state);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::ArrangementLockChanged {
                locked: actual,
                changed: true,
                ..
            } if actual == locked
        ));
        updated.set_arrangement_locked(locked);
        assert_eq!(state.coordinator.session().policy(), &updated);
    }

    for item in [FIRST, SECOND] {
        state
            .request_command(NativeCommand::DockItem {
                item,
                placement: DockPlacement::After(THIRD),
            })
            .unwrap();
        commit_queued(&mut state);
        let status = take(&mut state);
        if item == FIRST {
            assert_eq!(
                status,
                NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
                    DockspaceActionRejection::PolicyDenied
                ))
            );
        } else {
            assert!(matches!(
                status,
                NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::Docked { item: SECOND, .. }
                ))
            ));
        }
    }
    state
        .request_command(NativeCommand::CloseItem { item: FIRST })
        .unwrap();
    commit_queued(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::CloseRejected(DockspaceCloseRequestRejection::ItemCloseDisabled {
            target: ClosePlanTarget::Item { item: FIRST },
            item: FIRST,
        })
    );
    let plan = request_plan(&mut state, SECOND);
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), SECOND);
    assert_eq!(
        plan.items()[0].capability(),
        CloseCapability::DeferredAllowed
    );
    assert_eq!(state.panes.0, "application-owned content");
    assert!(state.error().is_none());
}

#[test]
fn dropped_policy_frames_publish_nothing_and_retained_commands_retry_after_restart() {
    for finish_configuration in [false, true] {
        let mut state = state();
        let original = state.coordinator.session().policy().clone();
        let before = state.coordinator.session().version();
        let mut updated = original.clone();
        updated.set_allow_edge_split(false);
        let command = NativeCommand::SetPolicy {
            policy: Box::new(updated.clone()),
        };
        state.request_command(command.clone()).unwrap();
        let action = state.application_actions.take().unwrap();
        {
            let mut frame = state
                .coordinator
                .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            let submission = action.submit(&mut frame.frame, before).unwrap();
            if finish_configuration {
                let submitted = submission.finish(&mut frame.frame).unwrap();
                drop(submitted);
            }
        }
        assert_eq!(state.coordinator.session().policy(), &original);
        assert_eq!(state.coordinator.session().version(), before);
        assert!(state.take_action_status().is_none());
        let error = state.request_command(command).unwrap_err();
        assert_eq!(error.reason(), NativeActionRequestError::Busy);
        state.stop_current_output(
            crate::error::NativeHostProtocolError::OutputRouteAttachmentFailed.into(),
        );
        let failure = state.take_action_status().unwrap().unwrap_err();
        assert_eq!(failure.kind(), crate::NativeRuntimeErrorKind::HostProtocol);
        assert_eq!(
            failure.interruption(),
            crate::NativeActionInterruption::NotCommitted
        );
        let error = state.request_command(error.into_command()).unwrap_err();
        assert_eq!(error.reason(), NativeActionRequestError::Stopped);

        let mut restarted = super::state();
        restarted.request_command(error.into_command()).unwrap();
        let report = commit_queued_report(&mut restarted);
        assert_eq!(restarted.coordinator.session().policy(), &updated);
        assert_eq!(
            take(&mut restarted),
            NativeActionStatus::PolicyChanged {
                changed: true,
                version: report.after(),
            }
        );
    }
}
