use super::*;
use crate::mailbox::window_resource::tests::bindings;
use eframe::egui::ViewportId;

fn facts(x: i64, visible: Option<bool>) -> WindowFacts {
    WindowFacts {
        geometry: Geometry {
            size: [640, 480],
            inner: Some([x, 0, 640, 480]),
            outer: Some([x - 8, -30, 656, 518]),
            native_scale: 1.0_f64.to_bits(),
            presentation_scale: 1.0_f64.to_bits(),
        },
        visible,
    }
}

fn paint(
    preparation: &mut WindowPreparation,
    binding: NativeSurfaceBinding,
    presented: bool,
) -> NativePreparationPaint {
    let request = preparation.paint(binding).unwrap();
    assert!(preparation.begin_paint(request));
    assert!(preparation.paint(binding).is_none());
    assert!(preparation.finish_paint(request, presented));
    request
}

fn visible_ready() -> (
    WindowPreparation,
    NativeSurfaceBinding,
    NativePreparationPaint,
) {
    let binding = bindings().0;
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(17);
    let mut preparation = WindowPreparation::new(window);
    preparation.observe_facts(Some(facts(0, Some(false))), 1);
    let pre = paint(&mut preparation, binding, true);
    assert_eq!(pre.phase(), NativePreparationPaintPhase::PreShow);
    assert_eq!(preparation.show_window(), Some(window));
    let token = eframe::prepare_native_viewport_visibility(viewport, true).token();
    assert!(preparation.dispatch_show(token));
    assert!(preparation.visibility(
        NativeViewportVisibilityRecord::for_test(
            token,
            viewport,
            window,
            binding,
            true,
            NativeViewportVisibilityStatus::Dispatched,
        ),
        2
    ));
    preparation.observe_facts(Some(facts(0, Some(true))), 3);
    let post = paint(&mut preparation, binding, true);
    assert_eq!(post.phase(), NativePreparationPaintPhase::PostShow);
    assert!(preparation.is_ready());
    assert_eq!(
        preparation.observation().readiness,
        NativeCreateReadiness::Prepared
    );
    (preparation, binding, post)
}

#[test]
fn exact_show_callback_requires_a_later_true_visible_observation() {
    let binding = bindings().0;
    let viewport = ViewportId::ROOT;
    let window = WindowId::from(17);
    let mut preparation = WindowPreparation::new(window);
    preparation.observe_facts(Some(facts(0, Some(false))), 1);
    paint(&mut preparation, binding, true);
    let token = eframe::prepare_native_viewport_visibility(viewport, true).token();
    let foreign = eframe::prepare_native_viewport_visibility(viewport, true).token();
    assert!(preparation.dispatch_show(token));
    preparation.observe_facts(Some(facts(0, Some(true))), 2);
    assert!(preparation.paint(binding).is_none());
    let callback = |token, window| {
        NativeViewportVisibilityRecord::for_test(
            token,
            viewport,
            window,
            binding,
            true,
            NativeViewportVisibilityStatus::Dispatched,
        )
    };
    assert!(!preparation.visibility(callback(foreign, window), 3));
    assert!(!preparation.visibility(callback(token, WindowId::from(18)), 3));
    assert!(preparation.visibility(callback(token, window), 3));
    preparation.observe_facts(Some(facts(0, Some(true))), 2);
    assert!(preparation.paint(binding).is_none());
    preparation.observe_facts(Some(facts(0, None)), 4);
    assert!(preparation.paint(binding).is_none());
    preparation.observe_facts(Some(facts(0, Some(true))), 5);
    assert_eq!(
        preparation.paint(binding).unwrap().phase(),
        NativePreparationPaintPhase::PostShow
    );
    assert!(!preparation.is_ready());
}

#[test]
fn ready_revocation_must_commit_created_before_the_same_geometry_can_be_prepared_again() {
    let (mut preparation, binding, old_output) = visible_ready();
    let old_prepared = preparation.observation();
    preparation.renderer_lost();
    let revoked = preparation.observation();
    assert_eq!(revoked.readiness, NativeCreateReadiness::Created);
    assert!(!preparation.finish_paint(old_output, true));
    paint(&mut preparation, binding, true);
    assert!(!preparation.is_ready());
    assert_eq!(
        preparation.observation().readiness,
        NativeCreateReadiness::Created
    );
    assert!(!preparation.settle_observation(old_prepared));
    assert!(preparation.settle_observation(revoked));
    assert!(preparation.is_ready());
    assert_eq!(
        preparation.observation().readiness,
        NativeCreateReadiness::Prepared
    );
}

#[test]
fn geometry_visibility_and_unknown_inventory_revoke_old_outputs_and_snapshot_receipts() {
    for next in [Some(facts(1, Some(true))), Some(facts(0, None)), None] {
        let (mut preparation, binding, old_output) = visible_ready();
        preparation.observe_facts(next, 4);
        let first_revocation = preparation.observation();
        assert_eq!(first_revocation.readiness, NativeCreateReadiness::Created);
        assert!(!preparation.finish_paint(old_output, true));
        preparation.observe_facts(Some(facts(2, Some(true))), 5);
        assert!(!preparation.settle_observation(first_revocation));
        let current = preparation.observation();
        paint(&mut preparation, binding, true);
        assert!(!preparation.is_ready());
        assert!(preparation.settle_observation(current));
        assert!(preparation.is_ready());
    }
}

#[test]
fn dropped_outputs_retry_only_their_phase_and_cancellation_cannot_be_reopened_by_late_results() {
    let binding = bindings().0;
    let mut preparation = WindowPreparation::new(WindowId::from(17));
    preparation.observe_facts(Some(facts(0, Some(false))), 1);
    let dropped = paint(&mut preparation, binding, false);
    assert_eq!(
        preparation.paint(binding).unwrap().phase(),
        NativePreparationPaintPhase::PreShow
    );
    assert!(!preparation.finish_paint(dropped, true));
    let outstanding = preparation.paint(binding).unwrap();
    assert!(preparation.begin_paint(outstanding));
    preparation.cancel();
    assert!(!preparation.finish_paint(outstanding, true));
    preparation.observe_facts(Some(facts(0, Some(true))), 2);
    assert!(preparation.paint(binding).is_none());
    assert!(preparation.show_window().is_none());
    assert_ne!(
        preparation.observation().readiness,
        NativeCreateReadiness::Prepared
    );
}

#[test]
fn repeated_cancellation_preserves_the_exact_readiness_revocation_receipt() {
    let (mut preparation, _, old_output) = visible_ready();
    let old_prepared = preparation.observation();
    assert!(preparation.cancel());
    let revoked = preparation.observation();
    assert_eq!(revoked.readiness, NativeCreateReadiness::Created);
    assert!(preparation.is_pending());
    assert!(!preparation.finish_paint(old_output, true));
    assert!(!preparation.cancel());
    assert_eq!(preparation.observation(), revoked);
    assert!(!preparation.settle_observation(old_prepared));
    assert!(preparation.settle_observation(revoked));
    assert!(!preparation.is_pending());
    assert!(!preparation.is_ready());
    assert!(!preparation.cancel());
    assert_eq!(preparation.observation(), revoked);
}

#[test]
fn replacement_owns_an_independent_window_and_rejects_the_predecessors_renderer_result() {
    let (mut predecessor, _, old_output) = visible_ready();
    predecessor.cancel();
    let successor_binding = bindings().1;
    let mut successor = WindowPreparation::new(WindowId::from(18));
    successor.observe_facts(Some(facts(0, Some(false))), 1);
    let successor_output = successor.paint(successor_binding).unwrap();
    assert!(successor.begin_paint(successor_output));
    assert!(!successor.finish_paint(old_output, true));
    assert!(successor.finish_paint(successor_output, true));
    assert_eq!(successor.show_window(), Some(WindowId::from(18)));
    assert!(!predecessor.is_ready());
}

#[test]
fn generation_exhaustion_revokes_readiness_and_fails_closed_without_reusing_an_old_request() {
    let (mut preparation, binding, old_output) = visible_ready();
    preparation.generation = u64::MAX;
    preparation.renderer_lost();
    assert_eq!(
        preparation.observation().readiness,
        NativeCreateReadiness::Failed
    );
    assert!(!preparation.is_ready());
    assert!(preparation.paint(binding).is_none());
    assert!(!preparation.finish_paint(old_output, true));
    preparation.observe_facts(Some(facts(0, Some(true))), 4);
    assert_eq!(
        preparation.observation().readiness,
        NativeCreateReadiness::Failed
    );
    assert!(preparation.show_window().is_none());
}
