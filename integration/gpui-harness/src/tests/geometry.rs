//! Geometry changes retained while an exact renderer submission is pending.

use super::*;

fn next_requested_frame(fixture: &mut Fixture) -> u64 {
    assert!(fixture.visual.simulate_frame(RequestFrameOptions {
        require_presentation: true,
        force_render: false,
        ..Default::default()
    }));
    fixture
        .visual
        .update(|window, _| window.rendered_frame_revision())
}

#[open_gpui::test]
fn a_pending_submission_preserves_the_latest_resize_and_then_stops_refreshing(
    cx: &mut TestAppContext,
) {
    let mut fixture = Fixture::new(cx);
    let before = fixture.geometry();
    let editor = ItemId::new(1);
    let inspector = ItemId::new(3);

    fixture.outcome(PlatformWindowPresentOutcome::Deferred);
    fixture.visual.simulate_resize(size(px(1000.0), px(760.0)));
    next_requested_frame(&mut fixture);
    let (deferred_serial, deferred_packet, deferred_bounds) = {
        let host = fixture.host.borrow();
        let pending = host.pending.as_ref().expect("the first resize is deferred");
        (
            pending.serial,
            pending.packet.clone(),
            host.bounds.expect("the first resize has measured bounds"),
        )
    };

    fixture.visual.simulate_resize(size(px(900.0), px(760.0)));
    next_requested_frame(&mut fixture);
    {
        let host = fixture.host.borrow();
        let pending = host.pending.as_ref().expect("submission is still deferred");
        assert_eq!(pending.serial, deferred_serial);
        assert!(Rc::ptr_eq(&pending.packet, &deferred_packet));
        assert_eq!(
            host.bounds,
            Some(deferred_bounds),
            "the exact pending packet must retain its measured coordinates"
        );
    }

    fixture.outcome(PlatformWindowPresentOutcome::Submitted);
    let latest_bounds = LogicalRect::new(0.0, 0.0, 900.0, 760.0).unwrap();
    for _ in 0..8 {
        next_requested_frame(&mut fixture);
        let host = fixture.host.borrow();
        assert!(host.error.is_none(), "adapter error: {:?}", host.error);
        if host.bounds == Some(latest_bounds)
            && host.pending.is_none()
            && host.inputs.is_empty()
            && !host.dirty
            && input::is_presented(&host, host.serial)
        {
            break;
        }
    }
    {
        let host = fixture.host.borrow();
        assert_eq!(
            host.bounds,
            Some(latest_bounds),
            "terminal submission must request a frame for the latest resize"
        );
        assert!(host.pending.is_none());
        assert!(host.inputs.is_empty());
        assert!(!host.dirty);
        assert!(input::is_presented(&host, host.serial));
        assert!(host.serial > deferred_serial);
    }

    let after = fixture.geometry();
    let inspector_right = |geometry: &Geometry| {
        let pane = geometry.panes[&inspector];
        pane.x() + pane.width()
    };
    assert!(after.panes[&editor].width() < before.panes[&editor].width() - 100.0);
    assert!(
        (inspector_right(&after) - (inspector_right(&before) - 200.0)).abs() < 0.01,
        "the rightmost pane must follow the full 1100-to-900 resize"
    );
    assert_eq!(
        fixture
            .host
            .borrow()
            .packet
            .as_ref()
            .unwrap()
            .pane_bounds(inspector),
        Some(after.panes[&inspector]),
        "the submitted packet must contain the latest pane geometry"
    );

    let idle_revision = next_requested_frame(&mut fixture);
    let idle_serial = fixture.host.borrow().serial;
    for _ in 0..3 {
        assert_eq!(next_requested_frame(&mut fixture), idle_revision);
        assert_eq!(fixture.host.borrow().serial, idle_serial);
    }
}
