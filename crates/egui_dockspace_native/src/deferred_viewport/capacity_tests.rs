use super::*;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostWindowToken, NativePointerRoster, NativeReceiverAnswer,
    SurfaceUnavailableReason,
};

const SURFACE: SurfaceId = SurfaceId::new(1);

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap()
}

fn session() -> (DockspaceSession, NativeSurfaceBinding) {
    let mut session = DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(41))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let binding = frame.commit().unwrap().native_bindings()[0];
    (session, binding)
}

fn bounds() -> PhysicalRect {
    PhysicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap()
}

#[test]
fn child_window_appearance_preserves_hidden_preparation_and_visible_redeclaration() {
    use std::sync::Arc;

    let (_session, binding) = session();
    let viewport = viewport_id_for(binding);
    let mut driver = DeferredViewportDriver::default();
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    let context = egui::Context::default();
    context.set_embed_viewports(false);
    let icon = Arc::new(egui::IconData {
        rgba: vec![255; 4 * 4 * 4],
        width: 4,
        height: 4,
    });
    let options = NativeChildWindowOptions {
        decorations: Some(false),
        taskbar: Some(false),
        window_level: Some(egui::WindowLevel::AlwaysOnTop),
        icon: Some(Arc::clone(&icon)),
    };
    for configured in [false, true] {
        let options = if configured {
            options.clone()
        } else {
            NativeChildWindowOptions::default()
        };
        for visible in [false, true] {
            if visible {
                assert_eq!(driver.set_visible(binding), Some(viewport));
            } else {
                driver.entries[0].set_visible(false);
            }
            let output = context.run_ui(egui::RawInput::default(), |ui| {
                declare_deferred_viewports(
                    ui.ctx(),
                    driver.specs(),
                    &options,
                    |_| "Inspector".into(),
                    |_, _, _| panic!("a deferred declaration must not render in the Root pass"),
                );
            });
            let child = &output.viewport_output[&viewport];
            assert!(child.class == ViewportClass::Deferred);
            assert!(child.viewport_ui_cb.is_some());
            let expected = if configured {
                ViewportBuilder::default()
                    .with_decorations(false)
                    .with_taskbar(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop)
                    .with_icon(Arc::clone(&icon))
            } else {
                ViewportBuilder::default()
            }
            .with_visible(visible)
            .with_title("Inspector");
            assert_eq!(
                child.builder, expected,
                "appearance must not override native placement, focus, visibility or pointer passthrough",
            );
            output.drop_without_applying_deltas();
        }
    }
}

#[test]
fn full_driver_preserves_exact_declarations_and_rejects_foreign_keys_before_mutation() {
    let (_session, binding) = session();
    let (_foreign_session, foreign) = session();
    let viewport = ViewportId::from_hash_of("finite deferred");
    let other = ViewportId::from_hash_of("foreign deferred");
    let mut driver = DeferredViewportDriver::with_capacity_limit(1).unwrap();
    let baseline = driver.owned_data_capacity_bytes();
    let backing = driver.entries.as_ptr();
    assert!(driver.can_insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.can_insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    let existing = driver.specs().collect::<Vec<_>>();
    let changed_bounds = PhysicalRect::new(41.0, 50.0, 360.0, 280.0).unwrap();
    for (key, exact, placement, role) in [
        (viewport, foreign, bounds(), NativeSurfaceRole::Child),
        (other, binding, bounds(), NativeSurfaceRole::Child),
        (other, foreign, bounds(), NativeSurfaceRole::Child),
        (viewport, binding, changed_bounds, NativeSurfaceRole::Child),
        (viewport, binding, bounds(), NativeSurfaceRole::Root),
    ] {
        assert!(!driver.can_insert(key, exact, placement, role));
        assert!(!driver.insert(key, exact, placement, role));
        assert_eq!(driver.specs().collect::<Vec<_>>(), existing);
        assert_eq!(driver.entries.as_ptr(), backing);
        assert_eq!(driver.owned_data_capacity_bytes(), baseline);
    }
    assert_eq!(driver.remove(foreign), None);
    assert_eq!(driver.set_visible(foreign), None);
    assert_eq!(driver.viewport_for(foreign), None);
    assert!(driver.has_transitional_viewport());
    assert_eq!(driver.set_visible(binding), Some(viewport));
    assert!(!driver.has_transitional_viewport());
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.specs().next().unwrap().visible);
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.owned_data_capacity_bytes(), baseline);
}

#[test]
fn retiring_and_rebinding_reuses_prepaid_backing_and_old_incarnations_cannot_remove_the_new_one() {
    let (mut session, old) = session();
    let mut driver = DeferredViewportDriver::with_capacity_limit(1).unwrap();
    let viewport = ViewportId::from_hash_of("rebound deferred");
    assert!(driver.insert(viewport, old, bounds(), NativeSurfaceRole::Root));
    let baseline = driver.owned_data_capacity_bytes();
    let backing = driver.entries.as_ptr();
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut frame = session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let new = frame.commit().unwrap().native_bindings()[0];
    assert_ne!(new, old);
    assert_eq!(new.surface(), old.surface());
    assert!(!driver.can_insert(viewport, new, bounds(), NativeSurfaceRole::Root));
    assert_eq!(driver.remove(old), Some(viewport));
    assert!(driver.specs().next().is_none());
    assert!(!driver.has_transitional_viewport());
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.owned_data_capacity_bytes(), baseline);
    assert!(driver.can_insert(viewport, new, bounds(), NativeSurfaceRole::Root));
    assert!(driver.insert(viewport, new, bounds(), NativeSurfaceRole::Root));
    assert_eq!(driver.remove(old), None);
    assert_eq!(driver.set_visible(old), None);
    assert_eq!(driver.viewport_for(new), Some(viewport));
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.owned_data_capacity_bytes(), baseline);
}

#[test]
fn finite_reservations_match_actual_capacity_and_overflow_rejects_before_allocation() {
    for slots in [0, 1, 2, 7, 65] {
        let driver = DeferredViewportDriver::with_capacity_limit(slots).unwrap();
        let required = DeferredViewportDriver::reservation_bytes(slots).unwrap();
        assert_eq!(driver.entries.capacity(), slots);
        assert_eq!(driver.owned_data_capacity_bytes(), required);
    }
    assert_eq!(DeferredViewportDriver::reservation_bytes(usize::MAX), None);
    assert_eq!(
        DeferredViewportDriver::with_capacity_limit(usize::MAX).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable
    );
    let (_session, binding) = session();
    let mut zero = DeferredViewportDriver::with_capacity_limit(0).unwrap();
    assert!(!zero.can_insert(
        ViewportId::ROOT,
        binding,
        bounds(),
        NativeSurfaceRole::Child
    ));
    assert!(!zero.insert(
        ViewportId::ROOT,
        binding,
        bounds(),
        NativeSurfaceRole::Child
    ));
}

#[test]
fn unlimited_driver_preflights_backing_and_preserves_viewport_order() {
    let (_first_session, first) = session();
    let (_second_session, second) = session();
    let low = ViewportId::from_hash_of("deferred low");
    let high = ViewportId::from_hash_of("deferred high");
    let (low, high) = if low < high { (low, high) } else { (high, low) };
    let mut driver = DeferredViewportDriver::default();
    assert!(driver.can_insert(high, first, bounds(), NativeSurfaceRole::Child));
    let capacity = driver.entries.capacity();
    let backing = driver.entries.as_ptr();
    assert!(capacity > 0);
    assert!(driver.insert(high, first, bounds(), NativeSurfaceRole::Child));
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.entries.capacity(), capacity);
    assert!(driver.insert(low, second, bounds(), NativeSurfaceRole::Child));
    assert_eq!(
        driver
            .specs()
            .map(DeferredViewportSpec::viewport)
            .collect::<Vec<_>>(),
        [low, high]
    );
    let owned = driver.owned_data_capacity_bytes();
    driver.remove(first);
    driver.remove(second);
    assert_eq!(driver.owned_data_capacity_bytes(), owned);
    assert!(driver.specs().next().is_none());
}
