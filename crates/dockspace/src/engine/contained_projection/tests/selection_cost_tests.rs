use super::*;

use crate::interaction::targeting::resolver::structural_work;

fn assert_linear_resize_selection_cost(pane_count: usize) {
    let (mut session, before) = fixture(pane_count);
    let version = session.version();
    let start = press(&mut session, before.resize);
    let surface = session.presented_surface(SURFACE).unwrap();
    structural_work::reset();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer_batch((1..=MOVE_COUNT).map(|index| {
            let route = HostPointerRoute::new(
                surface,
                offset(start, f64::from(index), f64::from(index) * 0.5),
            );
            HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::Moved,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
            )
        }))
        .unwrap();
    let before_close = structural_work::snapshot();
    assert_eq!(
        before_close.selection_pane_visits, 0,
        "{pane_count} panes: pointer edges must defer selection refresh until input close"
    );
    assert_eq!(before_close.selection_lookups, 0);

    let (painted, output) = paint_frame(frame, pane_count);
    let work = structural_work::snapshot();
    let dx = f64::from(MOVE_COUNT);
    painted.assert_projection(&before, rect(200.0, 140.0, 400.0 + dx, 300.0 + dx * 0.5));
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    assert_eq!(
        session.view().contained(FLOATING).unwrap().rect(),
        initial_rect()
    );
    assert_eq!(
        work.selection_lookups, pane_count,
        "{pane_count} panes: input close must resolve each tab bar selection exactly once"
    );
    assert!(
        work.selection_pane_visits <= 2 * pane_count,
        "{pane_count} panes: input-close selection refresh visited {} pane candidates for {} lookups; the linear budget is {} visits",
        work.selection_pane_visits,
        work.selection_lookups,
        2 * pane_count,
    );
    present(&mut session, output);
}

#[test]
fn resize_input_close_selection_cost_is_linear_for_16_panes() {
    assert_linear_resize_selection_cost(16);
}

#[test]
fn resize_input_close_selection_cost_is_linear_for_128_panes() {
    assert_linear_resize_selection_cost(128);
}

#[test]
fn resize_input_close_selection_cost_is_linear_for_1024_panes() {
    assert_linear_resize_selection_cost(1_024);
}
