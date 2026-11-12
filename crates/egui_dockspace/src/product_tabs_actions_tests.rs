use super::*;
use crate::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceActionStatus, PreparedTabsGroup,
};

fn tabs_facade() -> Dockspace {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs_with_selection([FIRST, SECOND], Some(SECOND)).unwrap(),
            DockspaceNode::central_tabs([THIRD]),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .unwrap();
    Dockspace::builder("programmatic-tabs-groups", layout)
        .build()
        .unwrap()
}

#[test]
fn egui_facade_docks_a_complete_group_with_one_selected_result() {
    for current in [false, true] {
        let mut dockspace = tabs_facade();
        let group: PreparedTabsGroup = dockspace
            .capture_tabs_group(DockAnchor::Item(FIRST))
            .unwrap();
        let result = if current {
            dockspace
                .dock_tabs_current(group, DockPlacement::After(THIRD))
                .unwrap()
        } else {
            let action = dockspace.prepare_dock_tabs(group, DockPlacement::After(THIRD));
            dockspace.submit_prepared_action(action).unwrap()
        };
        assert_eq!(
            result.status(),
            &DockspaceActionStatus::Applied(DockspaceActionOutcome::TabsDocked {
                source_root: ROOT,
                target_root: ROOT,
                items: vec![FIRST, SECOND],
                changed: true,
            })
        );
        let selected = dockspace.view().item(SECOND).unwrap();
        assert_eq!(selected.tabs().items(), &[THIRD, FIRST, SECOND]);
        assert!(selected.is_selected());
    }
}

#[test]
fn egui_facade_floats_only_the_captured_group_after_real_surface_measurement() {
    for current in [false, true] {
        let context = Context::default();
        let mut panes = MagneticPanes::new();
        let mut dockspace = tabs_facade();
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let group = dockspace
            .capture_tabs_group(DockAnchor::Item(FIRST))
            .unwrap();
        let result = if current {
            dockspace.float_tabs_current(group, SURFACE).unwrap()
        } else {
            let action = dockspace.prepare_float_tabs(group, SURFACE);
            dockspace.submit_prepared_action(action).unwrap()
        };
        let (root, floating) = match result.status() {
            DockspaceActionStatus::Applied(DockspaceActionOutcome::TabsFloated {
                source_root: ROOT,
                root,
                surface: SURFACE,
                floating,
                items,
                changed: true,
            }) if items == &[FIRST, SECOND] => (*root, *floating),
            status => panic!("the facade must expose one complete-group float result: {status:?}"),
        };
        assert_ne!(root, ROOT);
        let selected = dockspace.view().item(SECOND).unwrap();
        assert_eq!(selected.tabs().items(), &[FIRST, SECOND]);
        assert!(selected.is_selected());
        assert_eq!(selected.contained(), Some(floating));
        assert_eq!(dockspace.view().item(THIRD).unwrap().root(), ROOT);
        assert_eq!(
            dockspace.view().item(THIRD).unwrap().tabs().items(),
            &[THIRD]
        );
    }
}

#[test]
fn egui_facade_does_not_refresh_a_stored_group_after_its_anchor_moves() {
    for float in [false, true] {
        let mut dockspace = tabs_facade();
        let group = dockspace
            .capture_tabs_group(DockAnchor::Item(FIRST))
            .unwrap();
        let captured = group.expected_version();
        dockspace
            .dock_item_current(FIRST, DockPlacement::After(THIRD))
            .unwrap();
        let accepted = dockspace.version();
        let result = if float {
            dockspace.float_tabs_current(group, SURFACE).unwrap()
        } else {
            dockspace
                .dock_tabs_current(group, DockPlacement::After(THIRD))
                .unwrap()
        };
        assert_eq!(
            result.status(),
            &DockspaceActionStatus::Stale {
                expected: captured,
                accepted
            }
        );
        assert_eq!(
            dockspace.view().item(FIRST).unwrap().tabs().items(),
            &[THIRD, FIRST]
        );
        assert_eq!(
            dockspace.view().item(SECOND).unwrap().tabs().items(),
            &[SECOND]
        );
    }
}
