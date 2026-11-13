//! Committed notifications can query and reenter the real shared runtime.

use std::sync::{Arc, mpsc};

use dockspace::model::{DockPlacement, ItemId};
use dockspace::runtime::{DockspaceEventKind, NativeReceiverAnswer, SurfaceUnavailableReason};
use eframe::NativeOutputPassFinalizer;

use super::tests::{Pane, app};
use super::{NativePassFinalizer, lock_state, with_state};
use crate::surface_driver::NativeRuntimeState;

fn open(state: &mut NativeRuntimeState<Pane>, item: ItemId) {
    let native = state.boundary_coordinator();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .open_item_current(item, DockPlacement::After(ItemId::new(1)))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert!(report.layout_change().is_some());
    native.settle_host_frame_report(&report).unwrap();
}

#[test]
fn callbacks_release_the_runtime_lock_and_preserve_reentrant_commit_order() {
    let app = app();
    let weak = Arc::downgrade(&app.state);
    let (sender, receiver) = mpsc::channel();
    let app = app.with_notification_handler(move |notification, context| {
        let shared = weak.upgrade().unwrap();
        let events = notification.events();
        let item = events
            .iter()
            .find_map(|event| match event.kind() {
                DockspaceEventKind::ItemOpened { item, .. } => Some(*item),
                _ => None,
            })
            .unwrap();
        {
            let state = shared
                .try_lock()
                .expect("the callback must run outside the runtime lock");
            assert!(state.view().item(item).is_some());
        }
        context.with_view(|view| assert!(view.item(item).is_some()));
        if item == ItemId::new(2) {
            context
                .request_command(crate::NativeCommand::SelectItem {
                    item: ItemId::new(1),
                })
                .unwrap();
        }
        sender.send(item).unwrap();
        if item == ItemId::new(2) {
            with_state(&shared, |state| open(state, ItemId::new(3)));
        }
    });
    with_state(&app.state, |state| open(state, ItemId::new(2)));
    assert_eq!(
        receiver.try_iter().collect::<Vec<_>>(),
        [ItemId::new(2), ItemId::new(3)]
    );
    with_state(&app.state, |_| {});
    assert!(receiver.try_recv().is_err());
}

#[test]
fn native_service_flushes_notifications_after_internal_settlement() {
    let app = app();
    let weak = Arc::downgrade(&app.state);
    let (sender, receiver) = mpsc::channel();
    let app = app.with_notification_handler(move |notification, context| {
        let shared = weak.upgrade().unwrap();
        let state = shared
            .try_lock()
            .expect("native service must release its runtime lock");
        assert!(state.view().item(ItemId::new(2)).is_some());
        drop(state);
        context.with_view(|view| assert!(view.item(ItemId::new(2)).is_some()));
        sender.send(notification.clone()).unwrap();
    });
    open(&mut lock_state(&app.state), ItemId::new(2));
    assert!(receiver.try_recv().is_err());
    let finalizer = NativePassFinalizer {
        state: Arc::downgrade(&app.state),
    };
    finalizer.on_service(&eframe::egui::Context::default());
    assert!(receiver.try_recv().unwrap().layout_change().is_some());
    finalizer.on_service(&eframe::egui::Context::default());
    assert!(receiver.try_recv().is_err());
}

#[test]
fn an_unwinding_callback_does_not_poison_or_disable_the_runtime() {
    let app = app();
    let (sender, receiver) = mpsc::channel();
    let mut first = true;
    let app = app.with_notification_handler(move |notification, _context| {
        if std::mem::take(&mut first) {
            panic!("application callback failure");
        }
        sender.send(notification.clone()).unwrap();
    });
    let failed = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        with_state(&app.state, |state| open(state, ItemId::new(2)));
    }));
    assert!(failed.is_err());
    assert!(!app.state.is_poisoned());
    with_state(&app.state, |state| open(state, ItemId::new(3)));
    assert!(receiver.try_recv().unwrap().layout_change().is_some());
    assert!(receiver.try_recv().is_err());
}

#[cfg(feature = "serde")]
#[test]
fn notification_context_captures_committed_persistence_without_an_application_cycle() {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, RootId,
        SurfaceId,
    };
    use dockspace::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId, DockspaceSession};

    let document = DockspaceDocumentId::from_bytes([0x68; 16]);
    let mut bootstrap = DockspaceDocumentBootstrap::new(document);
    bootstrap.ensure_item("first").unwrap();
    bootstrap.ensure_item("second").unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    let session =
        DockspaceSession::from_persistent_layout(layout, Default::default(), bootstrap).unwrap();
    let (sender, receiver) = mpsc::channel();
    let app = super::NativeDockspaceApp::new(
        eframe::egui::Id::unique("notification-persistence"),
        session,
        SurfaceId::new(1),
        Pane,
        Default::default(),
    )
    .unwrap()
    .with_notification_handler(move |notification, runtime| {
        if notification.layout_change().is_some() {
            sender.send(runtime.save_document_json().unwrap()).unwrap();
        }
    });
    with_state(&app.state, |state| open(state, ItemId::new(2)));
    let bytes = receiver.try_recv().unwrap();
    let restored =
        DockspaceSession::from_document_json(&bytes, Default::default(), |actual, key| {
            actual == document && matches!(key, "first" | "second")
        })
        .unwrap();
    assert!(restored.view().item(ItemId::new(2)).is_some());
    assert!(receiver.try_recv().is_err());
    let shared = Arc::downgrade(&app.state);
    drop(app);
    assert!(
        shared.upgrade().is_none(),
        "the handler must not retain the application runtime"
    );
}
