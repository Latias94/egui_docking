use std::rc::Rc;

use dockspace::runtime::{NativeWindowRegistryError, NativeWindows};
use dockspace::{
    geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor},
    model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    },
    policy::DockPolicy,
    runtime::{
        DockspaceHostFrame, DockspaceSession, HostFrameReport, HostWindowToken,
        NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
        NativeRootContinuation, NativeSurfaceBinding, NativeSurfacePlacement, NativeWindowFacts,
        NativeWorkAreaRoster, SurfaceUnavailableReason,
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
fn shared_root_pairs_move_resources_once_and_keep_each_placement_retirement() {
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
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let report = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let original = report.native_bindings()[0].window();
    session
        .report_managed_native_snapshot(
            [(
                original,
                NativeWindowFacts::live()
                    .with_content_bounds(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
                    .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
                    .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap()),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let placements = [1_u32, 2].map(|id| {
        let x = f64::from(id - 1) * 400.0;
        NativeSurfacePlacement::new(
            SurfaceId::new(u64::from(id)),
            LogicalSize::new(400.0, 600.0).unwrap(),
            PhysicalPoint::new(x, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(x, 0.0, 400.0, 600.0).unwrap(),
        )
    });
    frame
        .set_native_surface_placements(original, &placements)
        .unwrap();
    finish(frame);
    let resource = Rc::new("shared renderer");
    let id = WindowId::from(41);
    let mut windows = NativeWindows::new();
    windows.insert(original, id, Rc::clone(&resource)).unwrap();
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let report = finish(
        session
            .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    let pairs = report.native_root_continuations();
    assert_eq!(pairs.len(), 2);
    assert_ne!(pairs[0].predecessor(), pairs[1].predecessor());
    assert_eq!(pairs[0].successor().window(), pairs[1].successor().window());
    assert!(windows.continue_root(pairs[0]).unwrap());
    assert!(!windows.continue_root(pairs[1]).unwrap());
    assert_eq!(windows.len(), 1);
    assert_eq!(windows.binding(id), Some(pairs[0].successor().window()));
    for &pair in pairs {
        assert!(windows.get(pair.predecessor()).is_none());
        assert!(Rc::ptr_eq(
            windows.get(pair.successor()).unwrap(),
            &resource
        ));
        session
            .report_native_binding_quiescence(pair.predecessor())
            .unwrap();
    }
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    for pair in pairs {
        assert!(!session.recognizes_native_binding(pair.predecessor()));
        assert!(session.is_current_native_binding(pair.successor()));
    }
    assert!(Rc::ptr_eq(
        windows.get(pairs[0].successor()).unwrap(),
        &resource
    ));
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
