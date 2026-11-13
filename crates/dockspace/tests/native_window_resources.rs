use std::rc::Rc;

use dockspace::runtime::{NativeWindowRegistryError, NativeWindows};
use dockspace::{
    model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    },
    policy::DockPolicy,
    runtime::{
        DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken,
        NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
        NativeRootContinuation, NativeSurfaceBinding, SurfaceUnavailableReason,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct WindowId(u64);

impl From<u64> for WindowId {
    fn from(value: u64) -> Self {
        Self(value)
    }
}

fn layout() -> DockspaceLayout {
    DockspaceLayout::new((1..=2).map(|id| {
        DockspaceSurfaceLayout::new(
            SurfaceId::new(id),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
        )
    }))
    .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn enrolled() -> (DockspaceSession, [NativeSurfaceBinding; 2]) {
    let mut session = DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory),
        )
        .unwrap();
    for id in 1..=2 {
        session
            .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
            .unwrap();
    }
    let report = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    (session, report.native_bindings().try_into().unwrap())
}

fn continued(session: &mut DockspaceSession) -> NativeRootContinuation {
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let report = finish(
        session
            .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    report.native_root_continuations()[0]
}

#[test]
fn rejected_insert_preserves_resources_and_both_route_indices() {
    let (_, [first, second]) = enrolled();
    let mut windows = NativeWindows::new();
    let id = WindowId::from(41);
    windows.insert(first, id, Rc::new("original")).unwrap();
    let resource = Rc::new("unaccepted");
    let rejected = windows
        .insert(second, id, Rc::clone(&resource))
        .unwrap_err();
    assert_eq!(
        rejected.reason(),
        NativeWindowRegistryError::WindowAlreadyRegistered { window: id }
    );
    assert!(Rc::ptr_eq(&rejected.into_value(), &resource));
    let rejected = windows
        .insert(first, WindowId::from(42), Rc::clone(&resource))
        .unwrap_err();
    assert_eq!(
        rejected.reason(),
        NativeWindowRegistryError::BindingAlreadyRegistered {
            binding: first.window()
        }
    );
    assert!(Rc::ptr_eq(&rejected.into_value(), &resource));
    assert_eq!(windows.len(), 1);
    assert_eq!(windows.binding(id), Some(first.window()));
    assert_eq!(windows.binding(WindowId::from(42)), None);
    assert!(windows.get(second).is_none());
    assert_eq!(**windows.get(first).unwrap(), "original");
}

#[test]
fn delayed_binding_cannot_address_a_reused_os_window_id() {
    let (_, [first, second]) = enrolled();
    let mut windows = NativeWindows::new();
    let id = WindowId::from(41);
    windows.insert(first, id, "first").unwrap();
    let delayed = windows.binding(id).unwrap();
    assert_eq!(windows.remove(first), Some("first"));
    windows.insert(second, id, "replacement").unwrap();
    assert!(windows.get(delayed).is_none());
    assert!(windows.get_mut(delayed).is_none());
    assert!(windows.remove(delayed).is_none());
    assert_eq!(windows.binding(id), Some(second.window()));
    assert_eq!(windows.get(second), Some(&"replacement"));
}

#[test]
fn committed_root_continuation_preserves_resources_and_retires_old_lookup() {
    let (mut session, [first, _]) = enrolled();
    let mut windows = NativeWindows::new();
    let id = WindowId::from(41);
    let resource = Rc::new("renderer");
    windows.insert(first, id, Rc::clone(&resource)).unwrap();
    let continuation = continued(&mut session);
    assert_eq!(continuation.predecessor(), first);
    assert_ne!(continuation.successor().window(), first.window());
    assert!(windows.continue_root(continuation).unwrap());
    assert!(!windows.continue_root(continuation).unwrap());
    assert!(windows.get(first).is_none());
    assert!(Rc::ptr_eq(
        windows.get(continuation.successor()).unwrap(),
        &resource
    ));
    assert_eq!(windows.binding(id), Some(continuation.successor().window()));
    assert_eq!(windows.len(), 1);
}

#[test]
fn conflicting_continuation_is_atomic_and_missing_predecessor_is_reported() {
    let (mut session, [first, _]) = enrolled();
    let continuation = continued(&mut session);
    let mut windows = NativeWindows::new();
    assert_eq!(
        windows.continue_root(continuation),
        Err(NativeWindowRegistryError::BindingUnavailable {
            binding: first.window()
        })
    );
    let before = WindowId::from(41);
    let after = WindowId::from(42);
    windows.insert(first, before, "predecessor").unwrap();
    windows
        .insert(continuation.successor(), after, "conflict")
        .unwrap();
    assert_eq!(
        windows.continue_root(continuation),
        Err(NativeWindowRegistryError::BindingAlreadyRegistered {
            binding: continuation.successor().window()
        })
    );
    assert_eq!(windows.binding(before), Some(first.window()));
    assert_eq!(
        windows.binding(after),
        Some(continuation.successor().window())
    );
    assert_eq!(windows.get(first), Some(&"predecessor"));
    assert_eq!(windows.get(continuation.successor()), Some(&"conflict"));
}
