//! Layout motion follows the retained host clock and exact submission cadence.

use dockspace::{
    model::DockPlacement,
    motion::{MotionMode, MotionPreference, MotionSettings},
    runtime::SurfaceUnavailableReason,
};

use super::*;
use crate::GpuiDockspace;

fn requested_frame(fixture: &mut Fixture) -> u64 {
    assert!(fixture.visual.simulate_frame(RequestFrameOptions {
        require_presentation: true,
        force_render: false,
        ..Default::default()
    }));
    fixture
        .visual
        .update(|window, _| window.rendered_frame_revision())
}

fn reflow(fixture: &mut Fixture) {
    let mut host = fixture.host.borrow_mut();
    let mut frame = host.session.begin_host_frame().unwrap();
    frame
        .dock_item_current(ItemId::new(3), DockPlacement::After(ItemId::new(1)))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    host.dirty = true;
    drop(host);
    fixture.visual.update(|window, _| window.refresh());
    requested_frame(fixture);
}

fn assert_idle(fixture: &mut Fixture) {
    let revision = requested_frame(fixture);
    let serial = fixture.host.borrow().serial;
    for _ in 0..3 {
        fixture.advance(Duration::from_millis(40));
        assert_eq!(requested_frame(fixture), revision);
        let host = fixture.host.borrow();
        assert_eq!(host.serial, serial);
        assert!(host.pending.is_none());
        assert!(!host.dirty);
        assert!(!host.session.surface_motion_is_active(SURFACE));
    }
}

#[open_gpui::test]
fn default_layout_motion_requests_its_own_frames_and_stops_at_the_target(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let before = fixture.geometry().panes[&ItemId::new(3)];
    let entity = fixture.host.borrow().panels[&ItemId::new(3)]
        .view
        .entity_id();
    reflow(&mut fixture);
    assert!(
        fixture
            .host
            .borrow()
            .session
            .surface_motion_is_active(SURFACE)
    );
    assert_eq!(fixture.geometry().panes[&ItemId::new(3)], before);

    fixture.advance(Duration::from_millis(100));
    requested_frame(&mut fixture);
    let middle = fixture.geometry().panes[&ItemId::new(3)];
    assert!(middle.width() > before.width());
    assert!(
        fixture
            .host
            .borrow()
            .session
            .surface_motion_is_active(SURFACE)
    );

    fixture.advance(Duration::from_millis(100));
    requested_frame(&mut fixture);
    let after = fixture.geometry().panes[&ItemId::new(3)];
    assert!(after.width() > middle.width());
    assert_eq!(
        fixture.host.borrow().panels[&ItemId::new(3)]
            .view
            .entity_id(),
        entity
    );
    assert_idle(&mut fixture);
}

fn stop_active_motion(cx: &mut TestAppContext, reduced: bool) {
    let mut fixture = Fixture::new(cx);
    let dock = GpuiDockspace {
        host: fixture.host.clone(),
    };
    reflow(&mut fixture);
    fixture.advance(Duration::from_millis(50));
    requested_frame(&mut fixture);
    let moving = fixture.geometry().panes[&ItemId::new(3)];
    assert!(
        fixture
            .host
            .borrow()
            .session
            .surface_motion_is_active(SURFACE)
    );

    fixture.visual.update(|window, _| {
        if reduced {
            dock.set_motion_preference(MotionPreference::Reduced, window);
        } else {
            dock.set_motion_settings(
                MotionSettings {
                    mode: MotionMode::Off,
                    ..dock.motion_settings()
                },
                window,
            );
        }
    });
    requested_frame(&mut fixture);
    assert!(fixture.geometry().panes[&ItemId::new(3)].width() > moving.width());
    assert_idle(&mut fixture);

    fixture.visual.update(|window, _| {
        dock.set_motion_preference(MotionPreference::Full, window);
        dock.set_motion_settings(MotionSettings::default(), window);
    });
    requested_frame(&mut fixture);
    assert_idle(&mut fixture);
}

#[open_gpui::test]
fn disabling_active_layout_motion_settles_without_replay(cx: &mut TestAppContext) {
    stop_active_motion(cx, false);
}

#[open_gpui::test]
fn reduced_motion_settles_active_layout_without_replay(cx: &mut TestAppContext) {
    stop_active_motion(cx, true);
}

#[open_gpui::test]
fn deferred_layout_motion_reuses_the_exact_packet_until_terminal_submission(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx);
    fixture.outcome(PlatformWindowPresentOutcome::Deferred);
    reflow(&mut fixture);
    let (serial, packet) = {
        let host = fixture.host.borrow();
        let pending = host.pending.as_ref().unwrap();
        (pending.serial, pending.packet.clone())
    };
    for _ in 0..4 {
        fixture.advance(Duration::from_millis(100));
        requested_frame(&mut fixture);
        let host = fixture.host.borrow();
        let pending = host.pending.as_ref().unwrap();
        assert_eq!(pending.serial, serial);
        assert!(Rc::ptr_eq(&pending.packet, &packet));
        assert_eq!(host.serial, serial);
        assert!(!input::is_presented(&host, serial));
    }

    fixture.outcome(PlatformWindowPresentOutcome::Submitted);
    for _ in 0..4 {
        requested_frame(&mut fixture);
        let host = fixture.host.borrow();
        if host.pending.is_none() && !host.dirty {
            break;
        }
    }
    {
        let host = fixture.host.borrow();
        assert!(host.error.is_none(), "adapter error: {:?}", host.error);
        assert!(host.serial > serial);
        assert!(input::is_presented(&host, host.serial));
    }
    assert_idle(&mut fixture);
}

#[open_gpui::test]
fn unmounting_active_motion_stops_refreshing_and_remount_uses_current_time(
    cx: &mut TestAppContext,
) {
    let (mut fixture, mode) = Fixture::with_boundary(cx);
    let entity = fixture.host.borrow().panels[&ItemId::new(3)]
        .view
        .entity_id();
    reflow(&mut fixture);
    assert!(
        fixture
            .host
            .borrow()
            .session
            .surface_motion_is_active(SURFACE)
    );

    mode.set(BoundaryMode::Absent);
    fixture.visual.update(|window, _| window.refresh());
    let absent_revision = requested_frame(&mut fixture);
    let absent_serial = fixture.host.borrow().serial;
    for _ in 0..3 {
        fixture.advance(Duration::from_millis(100));
        assert_eq!(requested_frame(&mut fixture), absent_revision);
        let host = fixture.host.borrow();
        assert_eq!(host.serial, absent_serial);
        assert!(!host.attached);
        assert!(host.pending.is_none());
        assert!(host.wake.is_none());
    }

    mode.set(BoundaryMode::Live);
    fixture.visual.update(|window, _| window.refresh());
    requested_frame(&mut fixture);
    assert_eq!(
        fixture.host.borrow().panels[&ItemId::new(3)]
            .view
            .entity_id(),
        entity
    );
    assert_idle(&mut fixture);
}
