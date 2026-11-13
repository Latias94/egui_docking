//! Native placement notifications share the atomic document-capture boundary.

use super::*;

fn window_facts(x: f64, width: f64, native_scale: f64) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(PhysicalRect::new(x, 30.0, width, 480.0).unwrap())
        .with_outer_bounds(PhysicalRect::new(x - 8.0, 0.0, width + 16.0, 518.0).unwrap())
        .with_native_scale_factor(ScaleFactor::new(native_scale).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
        .with_close(NativeCloseState::Clear, None)
}

fn observe(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    facts: &NativeWindowFacts,
) {
    session
        .report_managed_native_snapshot(
            [(binding, *facts)],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .unwrap();
}

fn commit(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn native_geometry_changes_notify_once_without_changing_the_docking_version() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let version = session.version();
    for facts in [
        window_facts(100.0, 640.0, 1.0),
        window_facts(200.0, 640.0, 1.0),
        window_facts(200.0, 720.0, 1.0),
        window_facts(200.0, 720.0, 2.0),
    ] {
        observe(&mut session, binding, &facts);
        let report = commit(&mut session);
        let notification = report.notification();
        assert!(notification.native_window_placement_changed());
        assert!(notification.document_changed());
        assert!(!notification.is_empty());
        assert!(notification.layout_change().is_none());
        assert!(notification.events().is_empty());
        assert_eq!(session.version(), version);

        observe(&mut session, binding, &facts);
        assert!(commit(&mut session).notification().is_empty());
        assert!(commit(&mut session).notification().is_empty());
    }
}

#[test]
fn zoom_input_and_unavailable_geometry_do_not_independently_request_a_save() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let facts = window_facts(100.0, 640.0, 1.0);
    observe(&mut session, binding, &facts);
    assert!(commit(&mut session).notification().document_changed());

    for focus in [
        NativeGlobalFocus::Dock(binding.window()),
        NativeGlobalFocus::None,
        NativeGlobalFocus::Unknown,
    ] {
        session.report_native_global_focus(focus).unwrap();
        assert!(commit(&mut session).notification().is_empty());
    }

    let zoomed = facts.with_presentation_scale_factor(ScaleFactor::new(1.5).unwrap());
    for facts in [
        zoomed,
        zoomed.with_input(NativeWindowInputState::PassThrough, None),
        zoomed.with_presentation(NativeWindowPresentationState::Hidden),
        NativeWindowFacts::live(),
    ] {
        observe(&mut session, binding, &facts);
        assert!(commit(&mut session).notification().is_empty());
    }
}

#[test]
fn placement_recovered_after_hidden_motion_is_announced_only_after_commit() {
    let (mut session, binding) =
        native_root_session_with_profile(NativeHostProfile::ManagedDesktop);
    let facts = window_facts(100.0, 640.0, 1.0);
    observe(&mut session, binding, &facts);
    assert!(commit(&mut session).notification().document_changed());
    let version = session.version();

    let moved = window_facts(300.0, 720.0, 2.0);
    observe(
        &mut session,
        binding,
        &moved.with_presentation(NativeWindowPresentationState::Hidden),
    );
    assert!(commit(&mut session).notification().is_empty());

    observe(&mut session, binding, &moved);
    {
        let mut abandoned = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        abandoned
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
    }
    assert_eq!(session.version(), version);
    assert!(commit(&mut session).notification().document_changed());
    assert_eq!(session.version(), version);
    assert!(commit(&mut session).notification().is_empty());
}

#[cfg(feature = "serde")]
#[test]
fn geometry_only_notification_can_capture_the_latest_persistent_placement() {
    use crate::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    };
    use crate::runtime::{DockspaceDocumentBootstrap, DockspaceDocumentId};

    let mut bootstrap =
        DockspaceDocumentBootstrap::new(DockspaceDocumentId::from_bytes([0x47; 16]));
    assert_eq!(bootstrap.ensure_item("pane").unwrap(), ITEM);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM])),
    )])
    .unwrap();
    let mut session =
        DockspaceSession::from_persistent_layout(layout, DockPolicy::default(), bootstrap).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .unwrap();
    session.register_native_root(SURFACE, WINDOW).unwrap();
    let report = commit(&mut session);
    let [binding] = report.native_bindings() else {
        panic!("the root registration must publish one binding");
    };
    let binding = *binding;
    observe(&mut session, binding, &window_facts(100.0, 640.0, 1.0));
    commit(&mut session);
    session.save_document_json().unwrap();
    let version = session.version();

    observe(&mut session, binding, &window_facts(300.0, 720.0, 2.0));
    let report = commit(&mut session);
    assert!(report.notification().document_changed());
    assert!(report.layout_change().is_none());
    assert_eq!(session.version(), version);
    let saved = session.save_document_json().unwrap();
    let restored = session
        .prepare_document_restore_json(&saved, |_, key| key == "pane")
        .unwrap();
    let placement = restored
        .surface_presentation_preferences()
        .find(|placement| placement.surface() == SURFACE)
        .unwrap();
    assert_eq!(
        placement.outer_rect(),
        Some(PhysicalRect::new(292.0, 0.0, 736.0, 518.0).unwrap())
    );
    assert_eq!(
        placement.inner_size_hint(),
        Some(crate::geometry::PhysicalSize::new(720.0, 480.0).unwrap())
    );
    assert_eq!(
        placement.scale_factor_hint(),
        Some(ScaleFactor::new(2.0).unwrap())
    );
}
