//! Logical recovery checkpoints remain available through the public stopped view.

use super::tests::{Pane, app};
use super::*;
use dockspace::model::ItemId;

#[test]
fn layout_checkpoints_remain_read_only_while_busy_and_after_fatal_shutdown() {
    let app = app();
    let surface = SurfaceId::new(1);
    app.request_command(crate::NativeCommand::SelectItem {
        item: ItemId::new(1),
    })
    .unwrap();
    let pending = app.pending_work();
    let backlog = app.output_backlog();
    let before = app.with_view(|view| view.to_layout());
    assert_eq!(app.pending_work(), pending);
    assert_eq!(app.output_backlog(), backlog);
    assert_eq!(app.error_kind(), None);
    assert!(app.take_action_status().is_none());
    {
        let mut state = lock_state(&app.state);
        state.stop_current_output(
            NativeHostProtocolError::PaintedOutputCountMismatch {
                expected: 1,
                actual: 0,
            }
            .into(),
        );
    }
    let pending = app.pending_work();
    let backlog = app.output_backlog();
    let errors = app.with_errors(|error, cleanup| (error.unwrap().to_string(), cleanup.len()));
    let checkpoint = app.with_view(|view| view.to_layout());
    assert_eq!(app.pending_work(), pending);
    assert_eq!(app.output_backlog(), backlog);
    assert_eq!(
        app.with_errors(|error, cleanup| (error.unwrap().to_string(), cleanup.len())),
        errors
    );
    assert_eq!(
        checkpoint
            .view()
            .item(ItemId::new(1))
            .unwrap()
            .tabs()
            .items(),
        before.view().item(ItemId::new(1)).unwrap().tabs().items()
    );
    assert_eq!(
        checkpoint.view().item(ItemId::new(1)).unwrap().surface(),
        surface
    );
    assert!(
        app.request_command(crate::NativeCommand::SelectItem {
            item: ItemId::new(1)
        })
        .is_err()
    );

    let session = DockspaceSession::from_layout(checkpoint, Default::default()).unwrap();
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    assert!(!session.has_pending_presentation());
    assert!(session.presented_surface(surface).is_none());
    assert_eq!(session.native_window_bindings().count(), 0);
    let rebuilt = NativeDockspaceApp::new(
        Id::unique("recovered-native-layout"),
        session,
        surface,
        Pane,
        DockStyle::default(),
    )
    .unwrap();
    assert_eq!(rebuilt.error_kind(), None);
    assert!(rebuilt.take_action_status().is_none());
    rebuilt.with_view(|view| assert!(view.item(ItemId::new(1)).unwrap().is_selected()));
    assert!(app.error_kind().is_some());
}
