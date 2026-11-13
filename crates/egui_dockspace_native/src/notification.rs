//! Transient committed notifications, delivered outside the runtime lock.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use dockspace::model::DockspaceView;
use dockspace::runtime::DockspaceNotification;
use egui_dockspace::PaneView;

use crate::surface_driver::NativeRuntimeState;

/// Scoped runtime access from a committed notification callback.
///
/// Each method acquires and releases the runtime lock independently. Inspectors
/// passed to `with_view` must return before calling another runtime method.
/// File I/O and background scheduling remain application responsibilities.
pub struct NativeNotificationContext<'a, P> {
    pub(crate) state: &'a Arc<Mutex<NativeRuntimeState<P>>>,
}

impl<P: PaneView> NativeNotificationContext<'_, P> {
    /// Inspects the current committed layout under the runtime lock.
    ///
    /// A later publication can already be visible; the notification's versions
    /// identify its own commit, rather than pinning a historical snapshot.
    pub fn with_view<R>(&self, inspect: impl FnOnce(DockspaceView<'_>) -> R) -> R {
        inspect(lock_state(self.state).view())
    }

    /// Queues a command for a later eligible final root pass.
    ///
    /// # Errors
    ///
    /// Returns the same admission errors as [`crate::NativeDockspaceApp::request_command`],
    /// retaining the unchanged command for storage or retry.
    pub fn request_command(
        &self,
        command: crate::NativeCommand,
    ) -> Result<(), crate::NativeCommandRequestError> {
        lock_state(self.state).request_command(command)
    }

    /// Captures the current committed document for application-owned storage.
    ///
    /// # Errors
    ///
    /// Returns the same busy, shutdown or persistence errors as
    /// [`crate::NativeDockspaceApp::save_document_json`].
    #[cfg(feature = "serde")]
    pub fn save_document_json(&self) -> Result<Vec<u8>, crate::NativeLayoutRequestError> {
        lock_state(self.state).save_document_json()
    }
}

type NotificationHandler = Box<dyn FnMut(&DockspaceNotification) + Send>;

#[derive(Default)]
pub(crate) struct NativeNotifications {
    handler: Option<NotificationHandler>,
    pending: Vec<DockspaceNotification>,
    delivering: bool,
}

pub(crate) struct NativeNotificationDelivery {
    handler: NotificationHandler,
    pending: Vec<DockspaceNotification>,
}

impl NativeNotifications {
    pub(crate) fn set_handler(
        &mut self,
        handler: impl FnMut(&DockspaceNotification) + Send + 'static,
    ) {
        self.handler = Some(Box::new(handler));
        self.pending.clear();
    }

    pub(crate) fn collect(&mut self, notification: &DockspaceNotification) {
        if (self.handler.is_some() || self.delivering) && !notification.is_empty() {
            self.pending.push(notification.clone());
        }
    }

    pub(crate) fn take_delivery(&mut self) -> Option<NativeNotificationDelivery> {
        if self.pending.is_empty() || self.delivering {
            return None;
        }
        let handler = self.handler.take()?;
        self.delivering = true;
        Some(NativeNotificationDelivery {
            handler,
            pending: std::mem::take(&mut self.pending),
        })
    }

    pub(crate) fn finish_delivery(&mut self, delivery: NativeNotificationDelivery) {
        self.delivering = false;
        // A replacement installed during a callback takes effect for future work.
        if self.handler.is_none() {
            self.handler = Some(delivery.handler);
        }
    }
}

impl NativeNotificationDelivery {
    pub(crate) fn deliver(&mut self) {
        for notification in &self.pending {
            (self.handler)(notification);
        }
    }
}

pub(crate) fn lock_state<P>(state: &Arc<Mutex<P>>) -> MutexGuard<'_, P> {
    state.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Every publishing entry point finishes settlement before application callbacks.
pub(crate) fn with_state<P: PaneView, R>(
    shared: &Arc<Mutex<NativeRuntimeState<P>>>,
    action: impl FnOnce(&mut NativeRuntimeState<P>) -> R,
) -> R {
    let (result, mut delivery) = {
        let mut state = lock_state(shared);
        let result = action(&mut state);
        (result, state.notifications().take_delivery())
    };
    while let Some(current) = delivery {
        let mut guard = NotificationDeliveryGuard {
            shared,
            delivery: Some(current),
        };
        if let Some(current) = &mut guard.delivery {
            current.deliver();
        }
        let mut state = lock_state(shared);
        if let Some(current) = guard.delivery.take() {
            state.notifications().finish_delivery(current);
        }
        delivery = state.notifications().take_delivery();
    }
    result
}

struct NotificationDeliveryGuard<'a, P: PaneView> {
    shared: &'a Arc<Mutex<NativeRuntimeState<P>>>,
    delivery: Option<NativeNotificationDelivery>,
}

impl<P: PaneView> Drop for NotificationDeliveryGuard<'_, P> {
    fn drop(&mut self) {
        // Restore callback ownership if application code unwinds. The runtime
        // lock was not held by that callback, so it remains usable.
        if let Some(delivery) = self.delivery.take() {
            lock_state(self.shared)
                .notifications()
                .finish_delivery(delivery);
        }
    }
}
