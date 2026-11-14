//! Exact native command-slot and deferred-close boundaries across surfaces.

use super::*;
use dockspace::close::ClosePlanTarget;

const OTHER_SURFACE: SurfaceId = SurfaceId::new(2);
const OTHER_ROOT: RootId = RootId::new(9);

fn assert_busy(state: &mut NativeRuntimeState<Panes>) {
    let version = state.coordinator.session().version();
    let command = NativeCommand::CloseItem { item: SECOND };
    let error = state.request_command(command.clone()).unwrap_err();
    assert_eq!(error.reason(), NativeActionRequestError::Busy);
    assert_eq!(error.into_command(), command);
    assert!(state.application_actions.is_occupied());
    assert_eq!(state.coordinator.session().version(), version);
}

fn commit_exclusive(state: &mut NativeRuntimeState<Panes>) -> NativeActionStatus {
    assert_busy(state);
    commit_queued(state);
    assert_busy(state);
    let status = take(state);
    assert!(state.take_action_status().is_none());
    assert!(!state.application_actions.is_occupied());
    assert!(state.error().is_none());
    status
}

fn assert_tabs(state: &NativeRuntimeState<Panes>, first: &[ItemId], second: &[ItemId]) {
    let view = state.view();
    assert_eq!(view.surfaces().count(), 2);
    for (surface, root, items) in [(SURFACE, ROOT, first), (OTHER_SURFACE, OTHER_ROOT, second)] {
        let surface_view = view.surface(surface).unwrap();
        let main = surface_view.main_root().unwrap();
        assert_eq!(main.id(), root);
        assert!(surface_view.contained().next().is_none());
        assert_eq!(main.content().unwrap().tabs().unwrap().items(), items);
        assert_eq!(main.central().unwrap().tabs().unwrap().items(), items);
        for item in items {
            let location = view.item(*item).unwrap();
            assert_eq!(location.surface(), surface);
            assert_eq!(location.root(), root);
            assert!(location.contained().is_none());
        }
    }
}

#[test]
fn a_deferred_close_stays_inert_after_native_migration_and_return_until_a_fresh_request() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                ROOT,
                DockspaceNode::central_tabs_with_selection([SECOND, FIRST], Some(FIRST)).unwrap(),
            ),
        ),
        DockspaceSurfaceLayout::new(
            OTHER_SURFACE,
            DockspaceRootLayout::new(OTHER_ROOT, DockspaceNode::central_tabs([THIRD])),
        ),
    ])
    .unwrap();
    let mut state = state_with_layout(layout);
    assert_tabs(&state, &[SECOND, FIRST], &[THIRD]);
    state
        .request_command(NativeCommand::CloseItem { item: FIRST })
        .unwrap();
    let NativeActionStatus::CloseRequested {
        plan,
        reused: false,
    } = commit_exclusive(&mut state)
    else {
        panic!("the first close request must return a new FIRST plan");
    };
    assert_eq!(plan.target(), ClosePlanTarget::Item { item: FIRST });
    assert_eq!(plan.items().len(), 1);
    assert_eq!(plan.items()[0].item(), FIRST);
    assert_eq!(state.pending_content_close_plans(), [plan.clone()]);
    assert!(state.take_action_status().is_none());

    state
        .request_command(NativeCommand::CloseItem { item: FIRST })
        .unwrap();
    assert_eq!(
        commit_exclusive(&mut state),
        NativeActionStatus::CloseRequested {
            plan: plan.clone(),
            reused: true,
        }
    );
    assert_eq!(state.pending_content_close_plans(), [plan.clone()]);

    state
        .request_command(NativeCommand::DecideClose {
            request: plan.request(),
            token: plan.items()[0].token(),
            decision: CloseDecision::Deferred,
        })
        .unwrap();
    let NativeActionStatus::CloseDecisionProcessed {
        resolution:
            DockspaceCloseResolution::Deferred {
                request,
                item: FIRST,
                continuation,
            },
        plan: Some(deferred_plan),
        application: None,
        changed: false,
    } = commit_exclusive(&mut state)
    else {
        panic!("the exact FIRST decision must yield its deferred continuation");
    };
    assert_eq!(request, plan.request());
    assert_eq!(deferred_plan.request(), plan.request());
    assert_eq!(deferred_plan.target(), plan.target());
    assert_eq!(deferred_plan.items().len(), 1);
    assert_eq!(deferred_plan.items()[0].item(), FIRST);
    assert_eq!(
        deferred_plan.items()[0].deferred_token(),
        Some(continuation)
    );
    assert_eq!(state.pending_content_close_plans(), [deferred_plan]);
    assert_tabs(&state, &[SECOND, FIRST], &[THIRD]);

    for (target, first_tabs, other_tabs) in [
        (THIRD, &[SECOND][..], &[THIRD, FIRST][..]),
        (SECOND, &[SECOND, FIRST][..], &[THIRD][..]),
    ] {
        let before_move = state.coordinator.session().version();
        state
            .request_command(NativeCommand::DockItem {
                item: FIRST,
                placement: DockPlacement::Center(DockAnchor::Item(target)),
            })
            .unwrap();
        assert!(matches!(
            commit_exclusive(&mut state),
            NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                DockspaceActionOutcome::Docked { item: FIRST, .. }
            ))
        ));
        assert_ne!(state.coordinator.session().version(), before_move);
        assert_tabs(&state, first_tabs, other_tabs);
        assert!(state.pending_content_close_plans().is_empty());

        let before_continue = state.coordinator.session().version();
        state
            .request_command(NativeCommand::ContinueClose {
                request: plan.request(),
                token: continuation,
                decision: DeferredCloseDecision::Allow,
            })
            .unwrap();
        assert!(matches!(
            commit_exclusive(&mut state),
            NativeActionStatus::CloseDecisionProcessed {
                resolution: DockspaceCloseResolution::Inert(_),
                application: None,
                changed: false,
                ..
            }
        ));
        assert_eq!(state.coordinator.session().version(), before_continue);
        assert!(state.pending_content_close_plans().is_empty());
        assert_tabs(&state, first_tabs, other_tabs);
    }
    assert_eq!(
        state.view().item(FIRST).unwrap().tabs().selected(),
        Some(FIRST)
    );

    let fresh = request_plan(&mut state, FIRST);
    assert_eq!(fresh.target(), plan.target());
    assert_ne!(fresh.request(), plan.request());
    assert_eq!(fresh.items().len(), 1);
    assert_eq!(fresh.items()[0].item(), FIRST);
    assert_ne!(fresh.items()[0].token(), plan.items()[0].token());
    assert_eq!(state.pending_content_close_plans(), [fresh.clone()]);
    assert!(state.take_action_status().is_none());
    state
        .request_command(NativeCommand::ContinueClose {
            request: plan.request(),
            token: continuation,
            decision: DeferredCloseDecision::Allow,
        })
        .unwrap();
    assert!(matches!(
        commit_exclusive(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            application: None,
            changed: false,
            ..
        }
    ));
    assert_eq!(state.pending_content_close_plans(), [fresh.clone()]);
    assert_tabs(&state, &[SECOND, FIRST], &[THIRD]);
    state
        .request_command(NativeCommand::DecideClose {
            request: fresh.request(),
            token: fresh.items()[0].token(),
            decision: CloseDecision::Allow,
        })
        .unwrap();
    assert!(matches!(
        commit_exclusive(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Approved { request },
            application: Some(Ok(dockspace::runtime::DockspaceCloseOutcome::ItemClosed {
                item: FIRST,
                ..
            })),
            changed: true,
            ..
        } if request == fresh.request()
    ));
    assert!(state.view().item(FIRST).is_none());
    assert!(state.pending_content_close_plans().is_empty());
    assert_tabs(&state, &[SECOND], &[THIRD]);

    state
        .request_command(NativeCommand::OpenItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(SECOND)),
        })
        .unwrap();
    assert_eq!(
        commit_exclusive(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Opened {
                item: FIRST,
                root: ROOT,
            }
        ))
    );
    assert_tabs(&state, &[SECOND, FIRST], &[THIRD]);
    assert!(state.pending_content_close_plans().is_empty());
    assert_eq!(state.panes.0, "application-owned content");
}
