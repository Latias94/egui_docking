//! Native settlement delivers only actual core publications, without a backlog.

use std::sync::mpsc;

use dockspace::close::CloseDecision;
use dockspace::runtime::{DockspaceCloseOutcome, DockspaceEvent, DockspaceEventKind};

use super::*;

pub(super) fn deliver_notifications(native: &mut NativeCoordinator) {
    while let Some(mut delivery) = native.notifications.take_delivery() {
        delivery.deliver();
        native.notifications.finish_delivery(delivery);
    }
}

fn publish(
    native: &mut NativeCoordinator,
    action: impl FnOnce(&mut dockspace::runtime::DockspaceHostFrame<'_>),
    internal: bool,
) -> Vec<DockspaceEvent> {
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    action(&mut frame.frame);
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let events = report.events().to_vec();
    if internal {
        native
            .finish_internal_report(&egui::Context::default(), report)
            .unwrap();
    } else {
        native.settle_host_frame_report(&report).unwrap();
    }
    deliver_notifications(native);
    events
}

#[test]
fn native_handlers_observe_committed_selection_and_internal_close_once() {
    let mut native = coordinator();
    register_roots(&mut native);
    let first = ItemId::new(1);
    let added = ItemId::new(3);
    let (sender, receiver) = mpsc::channel();
    let mut batch = 0;
    native.notifications.set_handler(move |notification| {
        batch += 1;
        assert!(notification.layout_change().is_some());
        sender
            .send((batch, notification.events().to_vec()))
            .unwrap();
    });

    {
        let mut abandoned = native
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        abandoned
            .frame
            .open_item_current(added, DockPlacement::After(first))
            .unwrap();
    }
    assert!(native.session.view().item(added).is_none());
    assert!(receiver.try_recv().is_err());

    let opened = publish(
        &mut native,
        |frame| {
            frame
                .open_item_current(added, DockPlacement::After(first))
                .unwrap();
        },
        false,
    );
    assert!(opened.iter().any(|event| {
        matches!(event.kind(), DockspaceEventKind::ItemOpened { item, .. } if *item == added)
    }));
    assert_eq!(receiver.try_recv().unwrap(), (1, opened));
    assert!(receiver.try_recv().is_err());

    assert!(
        publish(
            &mut native,
            |frame| {
                frame
                    .open_item_current(added, DockPlacement::After(first))
                    .unwrap();
                frame.select_item_current(ItemId::new(99)).unwrap();
            },
            false,
        )
        .is_empty()
    );
    assert!(receiver.try_recv().is_err());

    let selected = publish(
        &mut native,
        |frame| {
            frame.select_item_current(first).unwrap();
        },
        false,
    );
    assert!(matches!(
        selected.as_slice(),
        [event] if event.kind() == &DockspaceEventKind::SelectionCommitted { item: first }
    ));
    assert_eq!(receiver.try_recv().unwrap(), (2, selected));

    assert!(
        publish(
            &mut native,
            |frame| {
                frame.request_close_item_current(added).unwrap();
            },
            true,
        )
        .is_empty()
    );
    assert!(native.session.view().item(added).is_some());
    assert!(receiver.try_recv().is_err());
    let request = native.session.pending_content_close_plans().next().unwrap();

    let closed = publish(
        &mut native,
        |frame| {
            frame
                .resolve_close(
                    request.request(),
                    request.items()[0].token(),
                    CloseDecision::Allow,
                )
                .unwrap();
        },
        true,
    );
    assert!(closed.iter().any(|event| {
        matches!(
            event.kind(),
            DockspaceEventKind::Closed(DockspaceCloseOutcome::ItemClosed { item, .. }) if *item == added
        )
    }));
    assert!(native.session.view().item(added).is_none());
    assert_eq!(receiver.try_recv().unwrap(), (3, closed));

    assert!(publish(&mut native, |_| {}, true).is_empty());
    assert!(receiver.try_recv().is_err());
}

#[test]
fn installing_a_native_handler_does_not_replay_earlier_publications() {
    let mut native = coordinator();
    register_roots(&mut native);
    assert!(native.notifications.take_delivery().is_none());
    let first = ItemId::new(1);
    let added = ItemId::new(3);
    assert!(
        !publish(
            &mut native,
            |frame| {
                frame
                    .open_item_current(added, DockPlacement::After(first))
                    .unwrap();
            },
            true,
        )
        .is_empty()
    );

    let (sender, receiver) = mpsc::channel();
    native
        .notifications
        .set_handler(move |notification| sender.send(notification.events().to_vec()).unwrap());
    publish(&mut native, |_| {}, true);
    assert!(receiver.try_recv().is_err());

    let events = publish(
        &mut native,
        |frame| {
            frame.select_item_current(first).unwrap();
        },
        false,
    );
    assert!(!events.is_empty());
    assert_eq!(receiver.try_recv().unwrap(), events);
    assert!(receiver.try_recv().is_err());
}
