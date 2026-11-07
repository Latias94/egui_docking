//! Real coordinator reductions for the application facade's single command slot.

use dockspace::close::{CloseDecision, DeferredCloseDecision};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceActionRejection,
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseInertReason, DockspaceCloseResolution, DockspaceSession, HostInputOutcome,
    NativeReceiverAnswer, SurfaceUnavailableReason,
};
use eframe::egui;
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};

use crate::{NativeActionRequestError, NativeActionStatus, NativeWindowClosePolicy};

use super::NativeRuntimeState;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const CLOSED: ItemId = ItemId::new(4);

struct Panes(String);

impl PaneView for Panes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Application pane".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        egui_dockspace::PaneResponse::default()
    }
}

fn state() -> NativeRuntimeState<Panes> {
    state_with_content(DockspaceNode::central_tabs([FIRST, SECOND, THIRD]), false)
}

fn state_with_content(content: DockspaceNode, companion: bool) -> NativeRuntimeState<Panes> {
    let mut surface = DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, content));
    if companion {
        surface = surface.with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(5)])),
            dockspace::geometry::LogicalRect::new(80.0, 80.0, 320.0, 240.0).unwrap(),
        ));
    }
    let layout = DockspaceLayout::new([surface]).unwrap();
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    let session = DockspaceSession::from_layout(layout, policy).unwrap();
    NativeRuntimeState::new(
        egui::Id::unique("native-layout-commands"),
        session,
        SURFACE,
        Panes("application-owned content".to_owned()),
        DockStyle::default(),
        NativeWindowClosePolicy::Cancel,
    )
    .unwrap()
}

fn commit_queued(state: &mut NativeRuntimeState<Panes>) {
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(!frame.native_input_pending());
    let submission = action.submit(&mut frame.frame, published).unwrap();
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state
        .application_actions
        .settle_submission(Some(submitted), &report)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
}

fn take(state: &mut NativeRuntimeState<Panes>) -> NativeActionStatus {
    state.take_action_status().unwrap().unwrap()
}

fn request_plan(
    state: &mut NativeRuntimeState<Panes>,
    item: ItemId,
) -> dockspace::runtime::DockspaceClosePlan {
    state.request_close_item(item).unwrap();
    commit_queued(state);
    let NativeActionStatus::CloseRequested {
        plan,
        reused: false,
    } = take(state)
    else {
        panic!("the exact close request must expose its decision plan");
    };
    plan
}

#[test]
fn deferred_veto_and_close_reopen_keep_pane_data_and_the_single_result_slot() {
    let mut state = state();
    let plan = request_plan(&mut state, FIRST);
    assert_eq!(state.pending_content_close_plans(), [plan.clone()]);
    state
        .request_close_decision(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Deferred,
        )
        .unwrap();
    assert_eq!(
        state.request_open_item(CLOSED, DockPlacement::Center(DockAnchor::Item(SECOND))),
        Err(NativeActionRequestError::Busy)
    );
    commit_queued(&mut state);
    assert_eq!(
        state.request_set_arrangement_locked(true),
        Err(NativeActionRequestError::Busy)
    );
    let NativeActionStatus::CloseDecisionProcessed {
        resolution: DockspaceCloseResolution::Deferred { continuation, .. },
        changed: false,
        ..
    } = take(&mut state)
    else {
        panic!("deferral must retain an exact continuation without closing content");
    };
    assert_eq!(state.pending_content_close_plans().len(), 1);
    state
        .request_deferred_close_decision(plan.request(), continuation, DeferredCloseDecision::Veto)
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Vetoed { .. },
            changed: false,
            ..
        }
    ));
    assert!(state.coordinator.session().view().item(FIRST).is_some());
    assert!(state.pending_content_close_plans().is_empty());

    let plan = request_plan(&mut state, FIRST);
    state
        .request_close_decision(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            application: Some(Ok(dockspace::runtime::DockspaceCloseOutcome::ItemClosed {
                item: FIRST,
                ..
            })),
            changed: true,
            ..
        }
    ));
    assert!(state.coordinator.session().view().item(FIRST).is_none());
    assert!(state.pending_content_close_plans().is_empty());
    assert_eq!(state.panes.0, "application-owned content");

    state
        .request_open_item(FIRST, DockPlacement::Center(DockAnchor::Item(SECOND)))
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Opened { item: FIRST, .. }
        ))
    ));
    let location = state.coordinator.session().view().item(FIRST).unwrap();
    assert_eq!(location.root(), ROOT);
    assert_eq!(location.tabs().selected(), Some(FIRST));
    assert_eq!(state.panes.0, "application-owned content");
    state
        .request_open_item(FIRST, DockPlacement::Center(DockAnchor::Item(THIRD)))
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Existing { item: FIRST }
        ))
    ));
}

#[test]
fn root_close_requires_all_votes_before_one_atomic_removal() {
    let mut state = state_with_content(DockspaceNode::central_tabs([FIRST, SECOND, THIRD]), true);
    state.request_close_root(ROOT).unwrap();
    commit_queued(&mut state);
    let NativeActionStatus::CloseRequested { plan, .. } = take(&mut state) else {
        panic!("root close must return a plan")
    };
    assert_eq!(plan.items().len(), 3);
    let before = state.coordinator.session().version();
    for (index, item) in plan.items().iter().enumerate() {
        state
            .request_close_decision(plan.request(), item.token(), CloseDecision::Allow)
            .unwrap();
        commit_queued(&mut state);
        let status = take(&mut state);
        if index < 2 {
            assert!(matches!(
                status,
                NativeActionStatus::CloseDecisionProcessed {
                    changed: false,
                    application: None,
                    ..
                }
            ));
            assert_eq!(state.coordinator.session().version(), before);
            assert!(
                [FIRST, SECOND, THIRD].into_iter().all(|item| state
                    .coordinator
                    .session()
                    .view()
                    .item(item)
                    .is_some())
            );
        } else {
            assert!(matches!(
                status,
                NativeActionStatus::CloseDecisionProcessed {
                    changed: true,
                    application: Some(Ok(dockspace::runtime::DockspaceCloseOutcome::RootClosed {
                        root: ROOT,
                        ..
                    })),
                    ..
                }
            ));
            assert_ne!(state.coordinator.session().version(), before);
        }
    }
    assert!(
        [FIRST, SECOND, THIRD].into_iter().all(|item| state
            .coordinator
            .session()
            .view()
            .item(item)
            .is_none())
    );
    assert!(state.pending_content_close_plans().is_empty());
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .unwrap()
            .is_rootless()
    );
    state
        .request_open_item(FIRST, DockPlacement::Main(SURFACE))
        .unwrap();
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::Dock(DockspaceActionStatus::Applied(DockspaceActionOutcome::Opened { item: FIRST, root })) if root != ROOT)
    );
    assert!(state.coordinator.session().view().item(FIRST).is_some());
}

#[test]
fn the_fixed_host_rejects_its_last_group_and_reopens_after_all_central_items_close() {
    let mut state = state();
    let before = state.coordinator.session().version();
    assert_eq!(
        state.request_close_root(ROOT),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    assert!(!state.application_actions.is_occupied());
    assert!(state.pending_content_close_plans().is_empty());
    assert_eq!(state.coordinator.session().version(), before);
    for item in [FIRST, SECOND, THIRD] {
        let plan = request_plan(&mut state, item);
        state
            .request_close_decision(
                plan.request(),
                plan.items()[0].token(),
                CloseDecision::Allow,
            )
            .unwrap();
        commit_queued(&mut state);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::CloseDecisionProcessed {
                changed: true,
                application: Some(Ok(_)),
                ..
            }
        ));
    }
    let surface = state.coordinator.session().view().surface(SURFACE).unwrap();
    assert_eq!(surface.main_root().unwrap().id(), ROOT);
    assert!(
        surface
            .main_root()
            .unwrap()
            .central()
            .unwrap()
            .tabs()
            .unwrap()
            .items()
            .is_empty()
    );
    state.request_set_arrangement_locked(true).unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged { locked: true, .. }
    ));
    state.request_set_arrangement_locked(false).unwrap();
    commit_queued(&mut state);
    take(&mut state);
    state
        .request_open_item(FIRST, DockPlacement::Center(DockAnchor::Central(ROOT)))
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Opened {
                item: FIRST,
                root: ROOT
            }
        ))
    ));
    assert!(state.coordinator.session().view().item(FIRST).is_some());
    assert_eq!(state.panes.0, "application-owned content");
}

#[test]
fn the_last_noncentral_item_cannot_remove_the_fixed_host_surface() {
    let mut state = state_with_content(DockspaceNode::tabs([FIRST]), false);
    assert_eq!(
        state.request_close_item(FIRST),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    assert!(!state.application_actions.is_occupied());
    state.request_select_item(FIRST).unwrap();
    assert_eq!(
        state.request_close_item(FIRST),
        Err(NativeActionRequestError::Busy)
    );
    commit_queued(&mut state);
    take(&mut state);
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .is_some()
    );
    assert!(state.coordinator.session().view().item(FIRST).is_some());
}

#[test]
fn chrome_group_decisions_keep_the_host_and_forward_inert_tokens_to_the_core() {
    let mut state = state();
    // Chrome requests reach this same core owner without using the application slot.
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.request_close_root_current(ROOT).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let plan = state.pending_content_close_plans().pop().unwrap();
    assert_eq!(
        state.request_close_decision(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow
        ),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    state
        .request_close_decision(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Deferred,
        )
        .unwrap();
    commit_queued(&mut state);
    let NativeActionStatus::CloseDecisionProcessed {
        resolution: DockspaceCloseResolution::Deferred { continuation, .. },
        ..
    } = take(&mut state)
    else {
        panic!("the core must issue the exact deferred continuation")
    };
    assert_eq!(
        state.request_deferred_close_decision(
            plan.request(),
            continuation,
            DeferredCloseDecision::Allow
        ),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    state
        .request_deferred_close_decision(plan.request(), continuation, DeferredCloseDecision::Veto)
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Vetoed { .. },
            changed: false,
            ..
        }
    ));
    state
        .request_deferred_close_decision(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            changed: false,
            ..
        }
    ));
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .is_some()
    );
    assert!(
        [FIRST, SECOND, THIRD].into_iter().all(|item| state
            .coordinator
            .session()
            .view()
            .item(item)
            .is_some())
    );
}

#[test]
fn configuration_follows_local_selection_and_lock_keeps_selection_available() {
    let mut state = state();
    state.request_set_arrangement_locked(true).unwrap();
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let submission = action.submit(&mut frame.frame, published).unwrap();
    frame.frame.select_item_current(SECOND).unwrap();
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state
        .application_actions
        .settle_submission(Some(submitted), &report)
        .unwrap();
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged {
            locked: true,
            changed: true,
            ..
        }
    ));
    assert!(state.arrangement_locked());
    assert_eq!(
        state
            .coordinator
            .session()
            .view()
            .item(SECOND)
            .unwrap()
            .tabs()
            .selected(),
        Some(SECOND)
    );

    state.request_select_item(FIRST).unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Selected { item: FIRST, .. }
        ))
    ));
    state
        .request_open_item(CLOSED, DockPlacement::Center(DockAnchor::Item(FIRST)))
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
            DockspaceActionRejection::PolicyDenied
        ))
    ));
    assert!(state.coordinator.session().view().item(CLOSED).is_none());

    let before = state.coordinator.session().version();
    state.request_set_arrangement_locked(true).unwrap();
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::ArrangementLockChanged { locked: true, changed: false, version } if version == before)
    );
    state.request_set_arrangement_locked(false).unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged {
            locked: false,
            changed: true,
            ..
        }
    ));
    assert!(!state.arrangement_locked());
}

#[test]
fn exact_close_submission_ignores_other_close_requests_in_the_same_frame() {
    let mut state = state();
    state.request_close_item(FIRST).unwrap();
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.request_close_item_current(SECOND).unwrap();
    let submission = action.submit(&mut frame.frame, published).unwrap();
    frame.frame.request_close_item_current(THIRD).unwrap();
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        report
            .inputs()
            .iter()
            .filter(|input| matches!(input, HostInputOutcome::CloseRequested { .. }))
            .count(),
        3
    );
    state
        .application_actions
        .settle_submission(Some(submitted), &report)
        .unwrap();
    let NativeActionStatus::CloseRequested { plan, .. } = take(&mut state) else {
        panic!("the submitted close result must be correlated")
    };
    assert_eq!(plan.items()[0].item(), FIRST);
    assert_eq!(state.pending_content_close_plans().len(), 3);
}

#[test]
fn a_queued_close_keeps_its_revision_after_an_earlier_selection_commits() {
    let mut state = state();
    state.request_close_item(FIRST).unwrap();
    let before = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.select_item_current(SECOND).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let accepted = state.coordinator.session().version();
    assert_ne!(before, accepted);
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::Stale { expected, accepted: current } if expected == before && current == accepted)
    );
    assert!(state.coordinator.session().view().item(FIRST).is_some());
    assert!(state.pending_content_close_plans().is_empty());
}

#[test]
fn a_foreign_close_token_cannot_be_approved_through_the_native_queue() {
    let mut source = state();
    let plan = request_plan(&mut source, FIRST);
    let mut other = state();
    other
        .request_close_decision(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Allow,
        )
        .unwrap();
    commit_queued(&mut other);
    assert!(matches!(
        take(&mut other),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(
                DockspaceCloseInertReason::AuthorityMismatch
            ),
            changed: false,
            ..
        }
    ));
    assert!(other.coordinator.session().view().item(FIRST).is_some());
    assert!(other.pending_content_close_plans().is_empty());
    assert_eq!(source.pending_content_close_plans(), [plan]);
}
