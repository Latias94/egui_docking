//! Real runtime publications, rather than synthetic event conversions.

use std::collections::HashSet;

use crate::close_plan::{CloseDecision, DeferredCloseDecision};
use crate::geometry::{LogicalRect, LogicalSize};
use crate::ids::{FloatingPresentationId, ItemId, RootId, SurfaceId};
use crate::model::{
    DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceContainedLayout,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
};
use crate::policy::{CloseCapability, DockPolicy};
use crate::runtime::{
    DockspaceCloseOutcome, DockspaceCloseResolution, DockspaceEventKind, DockspaceHostFrame,
    DockspaceSession, HostFrameReport, HostInputOutcome, SurfacePresentationResult,
    UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);

fn layout(items: impl IntoIterator<Item = ItemId>) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs(items)),
    )])
    .unwrap()
}

fn session() -> DockspaceSession {
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    DockspaceSession::from_layout(layout([FIRST, SECOND]), policy).unwrap()
}

fn commit(frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame.commit().unwrap()
}

fn present_surface(session: &mut DockspaceSession) {
    let mut measure = session.begin_host_frame().unwrap();
    measure
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    assert!(commit(measure).events().is_empty());

    let mut paint = session.begin_host_frame().unwrap();
    let receipt = paint
        .paint_plan(SURFACE)
        .unwrap()
        .expect("the real measurement must install a ready surface")
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    paint.confirm_surface_painted(receipt).unwrap();
    let mut report = commit(paint);
    assert!(report.events().is_empty());
    let outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    assert!(
        commit(session.begin_host_frame().unwrap())
            .events()
            .is_empty()
    );
}

#[test]
fn events_keep_intermediate_versions_order_and_session_scoped_identity() {
    fn publish(session: &mut DockspaceSession) -> HostFrameReport {
        let mut frame = session.begin_host_frame().unwrap();
        frame.select_item_current(SECOND).unwrap();
        let selected = frame.version();
        frame
            .open_item_current(THIRD, DockPlacement::After(SECOND))
            .unwrap();
        let report = commit(frame);
        assert_eq!(
            report.events()[0].kind(),
            &DockspaceEventKind::SelectionCommitted { item: SECOND }
        );
        assert_eq!(report.events()[0].version(), selected);
        let opened = report
            .events()
            .iter()
            .find(|event| {
                matches!(
                    event.kind(),
                    DockspaceEventKind::ItemOpened {
                        item: THIRD,
                        root: ROOT
                    }
                )
            })
            .unwrap();
        assert_ne!(selected, opened.version());
        assert_eq!(opened.version(), report.after());
        assert_eq!(
            report
                .events()
                .iter()
                .map(|event| event.id())
                .collect::<HashSet<_>>()
                .len(),
            report.events().len()
        );
        report
    }

    let mut first_session = session();
    let first = publish(&mut first_session);
    let second = publish(&mut session());
    assert_eq!(first.after(), second.after());
    assert_ne!(first.events()[0].id(), second.events()[0].id());
    assert!(
        commit(first_session.begin_host_frame().unwrap())
            .events()
            .is_empty()
    );
}

#[test]
fn unchanged_rejected_stale_and_abandoned_actions_do_not_replay_changes() {
    let mut session = session();
    let stale = session.prepare_select_item(FIRST);
    {
        let mut abandoned = session.begin_host_frame().unwrap();
        abandoned.select_item_current(SECOND).unwrap();
    }
    assert!(session.view().item(FIRST).unwrap().is_selected());
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    assert_eq!(commit(frame).events().len(), 1);

    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    frame
        .open_item_current(FIRST, DockPlacement::After(SECOND))
        .unwrap();
    frame.select_item_current(ItemId::new(99)).unwrap();
    frame.submit_prepared_action(stale).unwrap();
    let report = commit(frame);
    assert!(report.events().is_empty(), "{:?}", report.events());
    assert!(
        report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::ProductActionRejected(_)))
    );
    assert!(
        report
            .inputs()
            .iter()
            .any(|input| matches!(input, HostInputOutcome::StaleRejected { .. }))
    );
}

#[test]
fn terminal_layout_replacement_hides_its_superseded_input_prefix() {
    let mut session = session();
    let reset = session
        .prepare_layout_reset(layout([FIRST, THIRD]))
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    let report = commit(frame);
    assert_eq!(report.events().len(), 1);
    assert_eq!(report.events()[0].kind(), &DockspaceEventKind::LayoutReset);
    assert_eq!(report.events()[0].version(), report.after());
    assert!(session.view().item(SECOND).is_none());
    assert!(session.view().item(THIRD).is_some());
}

#[test]
fn only_the_final_allowed_close_emits_a_closed_event() {
    let mut session = session();
    let mut frame = session.begin_host_frame().unwrap();
    let requested = frame.request_close_item_current(FIRST).unwrap();
    let report = commit(frame);
    assert!(report.events().is_empty());
    let Some(HostInputOutcome::CloseRequested { plan, .. }) =
        report.submitted_action_outcome(requested)
    else {
        panic!("the real close request must return its plan");
    };
    let plan = plan.clone();
    let mut frame = session.begin_host_frame().unwrap();
    frame.request_close_item_current(FIRST).unwrap();
    let deferred = frame
        .resolve_close(
            plan.request(),
            plan.items()[0].token(),
            CloseDecision::Deferred,
        )
        .unwrap();
    let report = commit(frame);
    assert!(report.events().is_empty());
    let Some(HostInputOutcome::CloseDecisionProcessed {
        resolution: DockspaceCloseResolution::Deferred { continuation, .. },
        ..
    }) = report.submitted_action_outcome(deferred)
    else {
        panic!("the deferred decision must return its continuation");
    };
    let continuation = *continuation;
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .continue_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    frame
        .continue_deferred_close(plan.request(), continuation, DeferredCloseDecision::Allow)
        .unwrap();
    let report = commit(frame);
    let closed: Vec<_> = report
        .events()
        .iter()
        .filter(|event| matches!(event.kind(), DockspaceEventKind::Closed(_)))
        .collect();
    assert_eq!(closed.len(), 1);
    assert_eq!(
        closed[0].kind(),
        &DockspaceEventKind::Closed(DockspaceCloseOutcome::ItemClosed {
            item: FIRST,
            root: ROOT,
        })
    );
    assert!(session.view().item(FIRST).is_none());
    assert!(
        commit(session.begin_host_frame().unwrap())
            .events()
            .is_empty()
    );
}

#[test]
fn opening_into_a_rootless_surface_remains_an_open_event() {
    let floating = FloatingPresentationId::new(1);
    let mut session = DockspaceSession::from_layout(
        DockspaceLayout::new([DockspaceSurfaceLayout::rootless(SURFACE).with_contained(
            DockspaceContainedLayout::new(
                floating,
                DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST])),
                LogicalRect::new(20.0, 20.0, 240.0, 180.0).unwrap(),
            ),
        )])
        .unwrap(),
        DockPolicy::default(),
    )
    .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .open_item_current(SECOND, DockPlacement::Main(SURFACE))
        .unwrap();
    let report = commit(frame);
    let root = session.view().item(SECOND).unwrap().root();
    assert!(
        report.events().iter().any(|event| {
            event.kind() == &DockspaceEventKind::ItemOpened { item: SECOND, root }
        })
    );
    assert!(
        !report
            .events()
            .iter()
            .any(|event| matches!(event.kind(), DockspaceEventKind::ItemsMoved { .. }))
    );
}

#[test]
fn placement_changes_are_actual_commits_rather_than_requested_actions() {
    let mut session = session();
    let mut frame = session.begin_host_frame().unwrap();
    let unavailable = frame.float_item_current(SECOND, SURFACE).unwrap();
    let report = commit(frame);
    assert_eq!(
        report.submitted_action_outcome(unavailable),
        Some(&HostInputOutcome::ProductActionRejected(
            DockspaceActionRejection::PresentationUnavailable { surface: SURFACE },
        ))
    );
    assert!(report.events().is_empty());
    assert_eq!(session.view().item(SECOND).unwrap().root(), ROOT);

    // FloatItem derives its bounds from the actual source layout. A mere
    // accepted input is not evidence that floating was available or committed.
    present_surface(&mut session);
    let mut frame = session.begin_host_frame().unwrap();
    let floated = frame.float_item_current(SECOND, SURFACE).unwrap();
    let report = commit(frame);
    let root = session.view().item(SECOND).unwrap().root();
    assert_ne!(root, ROOT);
    assert!(matches!(
        report.submitted_action_outcome(floated),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Floated {
                item: SECOND,
                root: actual,
                surface: SURFACE,
                changed: true,
                ..
            }
        )) if *actual == root
    ));
    assert!(report.events().iter().any(|event| {
        matches!(
            event.kind(),
            DockspaceEventKind::RootInstalled {
                root: actual,
                surface: SURFACE,
                floating: Some(_),
                items,
            } if *actual == root && items == &[SECOND]
        )
    }));
    assert_eq!(
        report
            .events()
            .iter()
            .filter(|event| matches!(event.kind(), DockspaceEventKind::RootInstalled { .. }))
            .count(),
        1
    );
    let mut frame = session.begin_host_frame().unwrap();
    let docked = frame
        .dock_item_current(SECOND, DockPlacement::After(FIRST))
        .unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.submitted_action_outcome(docked),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: SECOND,
                source_root,
                target_root: ROOT,
                changed: true,
            }
        )) if *source_root == root
    ));
    assert!(report.events().iter().any(|event| {
        matches!(
            event.kind(),
            DockspaceEventKind::ItemsMoved {
                items,
                source_root,
                target_root: ROOT,
            } if items == &[SECOND] && *source_root == root
        )
    }));
    assert_eq!(
        report
            .events()
            .iter()
            .filter(|event| matches!(event.kind(), DockspaceEventKind::ItemsMoved { .. }))
            .count(),
        1
    );

    // A complete root changes owner without allocating another root. Keep
    // that actual rehome distinct from the partial payload's installation.
    present_surface(&mut session);
    let prepared = session.prepare_float_root(ROOT, SURFACE);
    let mut frame = session.begin_host_frame().unwrap();
    let rehomed = frame.submit_prepared_action(prepared).unwrap();
    let report = commit(frame);
    assert!(matches!(
        report.submitted_action_outcome(rehomed),
        Some(HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootFloated {
                root: ROOT,
                surface: SURFACE,
                changed: true,
                ..
            }
        ))
    ));
    assert!(report.events().iter().any(|event| {
        matches!(
            event.kind(),
            DockspaceEventKind::RootRehomed {
                root: ROOT,
                surface: SURFACE,
                floating: Some(_),
            }
        )
    }));
    assert_eq!(
        report
            .events()
            .iter()
            .filter(|event| matches!(event.kind(), DockspaceEventKind::RootRehomed { .. }))
            .count(),
        1
    );
    assert_eq!(session.view().item(SECOND).unwrap().root(), ROOT);
}

#[cfg(feature = "serde")]
#[test]
fn restored_document_replaces_its_prefix_without_synthetic_open_or_close_events() {
    use crate::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};

    let document = DockspaceDocumentId::from_bytes([0x35; 16]);
    let mut bootstrap = DockspaceDocumentBootstrap::new(document);
    assert_eq!(bootstrap.ensure_item("first").unwrap(), FIRST);
    assert_eq!(bootstrap.ensure_item("second").unwrap(), SECOND);
    let mut session = DockspaceSession::from_persistent_layout(
        layout([FIRST, SECOND]),
        DockPolicy::default(),
        bootstrap,
    )
    .unwrap();
    let saved = session.save_document_json().unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(SECOND).unwrap();
    commit(frame);

    let restore = session
        .prepare_document_restore_json(&saved, |actual, key| {
            actual == document && matches!(key, "first" | "second")
        })
        .unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(FIRST).unwrap();
    frame.submit_document_restore(&restore).unwrap();
    let report = commit(frame);
    assert_eq!(report.events().len(), 1);
    assert_eq!(
        report.events()[0].kind(),
        &DockspaceEventKind::DocumentRestored
    );
    assert_eq!(report.events()[0].version(), report.after());
}
