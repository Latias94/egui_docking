use std::num::NonZeroUsize;

use dockspace::{
    geometry::{LogicalSize, PhysicalPoint},
    model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    },
    policy::DockPolicy,
    runtime::{
        DockspaceRuntimeError, HostWindowToken, NativeHostCapabilities, NativeHostCapability,
        NativeInputStorageLimit, NativePointerRoster, NativeReceiverAnswer, NativeSurfacePlacement,
    },
};

use super::*;

fn destroyed_shared_window() -> (DockspaceSession, Host, Vec<NativeSurfaceBinding>) {
    let layout = |ids: &[u64]| {
        DockspaceLayout::new(ids.iter().map(|&id| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(id),
                DockspaceRootLayout::new(RootId::new(id), DockspaceNode::tabs([ItemId::new(id)])),
            )
        }))
        .unwrap()
    };
    let mut session =
        DockspaceSession::from_layout(layout(&[1, 2]), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host_with_input_storage_limit(
            NativePointerRoster::Unknown,
            NativeInputStorageLimit::new(NonZeroUsize::MIN, 1 << 20),
        )
        .unwrap();
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory),
        )
        .unwrap();
    workspace::idle(&mut session).unwrap();
    session
        .register_native_root(SurfaceId::new(1), HostWindowToken::new(1))
        .unwrap();
    let report = workspace::idle(&mut session).unwrap();
    let binding = report.native_bindings()[0].window();
    session
        .report_managed_native_snapshot(
            [(
                binding,
                NativeWindowFacts::live()
                    .with_content_bounds(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap())
                    .with_native_scale_factor(ScaleFactor::new(1.0).unwrap())
                    .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap()),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    workspace::idle(&mut session).unwrap();
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
        .set_native_surface_placements(binding, &placements)
        .unwrap();
    let retired: Vec<_> = frame.native_surface_bindings_for_window(binding).collect();
    frame.commit().unwrap();
    assert_eq!(retired.len(), 2);
    let reset = session.prepare_layout_reset(layout(&[3])).unwrap();
    session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap()
        .commit()
        .unwrap();
    session
        .report_managed_native_snapshot(
            [(binding, NativeWindowFacts::destroyed())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    workspace::idle(&mut session).unwrap();
    let mut host = Host::new();
    host.retiring.insert(
        WindowId::from(41),
        Retirement {
            window: binding,
            placements: retired.clone(),
            close: None,
            destroyed: true,
        },
    );
    host.last_snapshot = vec![(binding, NativeWindowFacts::destroyed())];
    (session, host, retired)
}

#[test]
fn destroyed_retirement_commits_and_retries_all_placements_without_a_new_snapshot() {
    let (mut session, mut host, retired) = destroyed_shared_window();
    assert!(!host.observe(&mut session).unwrap());

    host.settle_observations(&mut session).unwrap();

    assert!(host.idle());
    assert!(host.retiring.is_empty());
    assert!(host.last_snapshot.is_empty());
    assert!(!session.has_pending_native_input());
    assert!(
        retired
            .iter()
            .all(|&binding| !session.recognizes_native_binding(binding))
    );
}

#[test]
fn destroyed_retirement_returns_capacity_failure_without_recording_progress() {
    let (mut session, mut host, retired) = destroyed_shared_window();
    session
        .report_managed_native_snapshot(
            host.last_snapshot.iter().copied(),
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();

    let error = host.drain_destroyed(&mut session).unwrap_err();

    assert_eq!(
        error
            .downcast_ref::<DockspaceRuntimeError>()
            .unwrap()
            .native_kind(),
        Some(NativeHostErrorKind::InputCapacity)
    );
    assert!(!host.pending_records);
    assert_eq!(host.retiring[&WindowId::from(41)].placements, retired);
    assert_eq!(host.last_snapshot.len(), 1);
}
