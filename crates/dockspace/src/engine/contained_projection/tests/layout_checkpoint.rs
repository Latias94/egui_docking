//! Owned logical checkpoints exclude transient projections and host authority.

use super::*;
use crate::model::DockPlacement;

#[test]
fn a_layout_checkpoint_preserves_committed_content_during_a_live_transform() {
    for resizing in [false, true] {
        let (mut session, before) = fixture(16);
        let start = press(
            &mut session,
            if resizing {
                before.resize
            } else {
                before.title
            },
        );
        let durable = session.workspace().clone();
        let version = session.version();
        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, offset(start, 48.0, 32.0));
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                Event::Moved,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown(),
            )])
            .unwrap();
        let (projected, output) = paint_frame(frame, 16);
        assert_ne!(projected.outer, initial_rect());
        assert!(session.has_active_gesture());

        let snapshot = session.view().to_layout();
        assert_eq!(
            snapshot.view().contained(FLOATING).unwrap().rect(),
            initial_rect()
        );
        let rebuilt = DockspaceSession::from_layout_with_presentation_config(
            snapshot,
            session.policy().clone(),
            session.presentation_config().clone(),
        )
        .unwrap();
        assert_eq!(rebuilt.workspace(), &durable);
        assert!(!rebuilt.has_active_gesture());
        assert!(!rebuilt.has_pending_presentation());
        assert!(!rebuilt.has_pending_native_input());
        assert!(rebuilt.presented_surface(SURFACE).is_none());
        assert!(rebuilt.bind_presented_receiver(&before.title).is_none());
        assert_eq!(session.version(), version);
        assert_eq!(session.workspace(), &durable);
        assert!(session.has_active_gesture());

        session
            .report_surface_presentation(output, SurfacePresentationResult::Dropped)
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
    }
}

#[test]
fn a_candidate_layout_checkpoint_is_owned_without_publishing_the_abandoned_frame() {
    let (mut session, _) = fixture(16);
    let before = session.workspace().clone();
    let version = session.version();
    let checkpoint = {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .dock_item_current(RIGHT, DockPlacement::After(LEFT))
            .unwrap();
        let candidate = frame.view().to_layout();
        assert_eq!(
            candidate.view().item(LEFT).unwrap().tabs().items(),
            [LEFT, RIGHT]
        );
        candidate
    };
    assert_eq!(session.version(), version);
    assert_eq!(session.workspace(), &before);
    assert_eq!(session.view().item(LEFT).unwrap().tabs().items(), [LEFT]);
    let mut rebuilt = DockspaceSession::from_layout(checkpoint, session.policy().clone()).unwrap();
    assert_eq!(
        rebuilt.view().item(LEFT).unwrap().tabs().items(),
        [LEFT, RIGHT]
    );
    assert_eq!(rebuilt.view().item(RIGHT).unwrap().root(), CONTAINED_ROOT);
    let mut frame = rebuilt.begin_host_frame().unwrap();
    frame.select_item_current(LEFT).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    assert!(rebuilt.view().item(LEFT).unwrap().is_selected());
    assert_eq!(session.workspace(), &before);
}
