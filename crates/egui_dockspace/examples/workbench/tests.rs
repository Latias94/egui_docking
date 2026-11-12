//! Headless editor journeys through the complete public host path.

use super::super::editor::{self, apply_editor_commands, demo_layout, run_editor};
use super::*;

#[test]
fn complete_egui_input_journey_preserves_content_and_commits_layout() {
    let context = editor::new_context();
    context.enable_accesskit();
    let mut dock = Dockspace::builder("official-exercise-test", demo_layout())
        .style(editor::dock_style())
        .build()
        .unwrap();
    let mut panes = DemoPanes::new();
    panes.custom_tabs = true;
    let path = std::env::temp_dir().join(format!("dockspace-exercise-{}.csv", std::process::id()));
    let mut exercise = Exercise::new(path.clone());
    for frame in 0..640 {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        exercise.prepare(&mut input, &panes).unwrap();
        let (output, response) =
            run_editor(&mut dock, &context, input, &mut panes, &mut false).unwrap();
        let (mut output, receipt) = output.into_parts();
        exercise.observe(output.platform_output.accesskit_update.as_ref());
        let primitives =
            context.tessellate(std::mem::take(&mut output.shapes), output.pixels_per_point);
        // This is a CPU test owner, not GPU/OS presentation evidence.
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        apply_editor_commands(&mut dock, response.as_ref(), false).unwrap();
        if exercise
            .finish_frame(
                &panes,
                &dock,
                SurfacePresentationResult::Presented,
                &primitives,
                Timing {
                    update_ns: 0,
                    tessellate_ns: 0,
                    paint_present_ns: 0,
                    ui_to_present_ns: 0,
                },
            )
            .unwrap()
        {
            break;
        }
    }
    exercise.require_complete().unwrap();
    assert_eq!(exercise.samples.len(), 6 * SAMPLES);
    std::fs::remove_file(path).unwrap();
}

#[test]
fn editor_close_and_restore_keep_application_document_edits() {
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};

    let context = editor::new_context();
    context.enable_accesskit();
    let mut dock = Dockspace::builder("official-editor-close-test", demo_layout())
        .style(editor::dock_style())
        .build()
        .unwrap();
    let mut panes = DemoPanes::new();
    panes
        .panes
        .get_mut(&EDITOR)
        .unwrap()
        .text
        .push_str("\n// unsaved edit");
    let edited = panes.panes[&EDITOR].text.clone();
    let mut close = None;
    let mut saw_request = false;
    for frame in 0..8 {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        if frame == 2 {
            input
                .events
                .push(Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node: close.expect("the current editor exposes Close Editor"),
                    data: None,
                }));
        }
        let (output, response) =
            run_editor(&mut dock, &context, input, &mut panes, &mut false).unwrap();
        let (output, receipt) = output.into_parts();
        if frame == 1 {
            close = output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes
                .iter()
                .find_map(|(id, node)| {
                    (node.role() == Role::Button
                        && node.label() == Some("Close Editor")
                        && node.supports_action(Action::Click))
                    .then_some(*id)
                });
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        saw_request |= response
            .as_ref()
            .is_some_and(|response| !response.close_request_events().is_empty());
        apply_editor_commands(&mut dock, response.as_ref(), false).unwrap();
        if frame >= 3 {
            assert!(dock.view().item(EDITOR).is_none());
            assert!(dock.view().item(PREVIEW).is_some());
        }
        if frame == 7 {
            assert!(apply_editor_commands(&mut dock, response.as_ref(), true).unwrap());
        }
    }
    assert!(
        saw_request,
        "the real close control must produce an application decision"
    );
    assert_eq!(panes.panes[&EDITOR].text, edited);
    assert_eq!(panes.panes.len(), 8);
    assert!(
        panes
            .panes
            .keys()
            .all(|item| dock.view().item(*item).is_some())
    );
}
#[test]
fn closing_all_groups_keeps_the_empty_editor_and_restore_button_usable() {
    use egui::accesskit::{Action, ActionRequest, Role, TreeId};

    let context = editor::new_context();
    context.enable_accesskit();
    let mut dock = Dockspace::builder("official-empty-editor-test", demo_layout())
        .style(editor::dock_style())
        .build()
        .unwrap();
    let mut panes = DemoPanes::new();
    panes
        .panes
        .get_mut(&EDITOR)
        .unwrap()
        .text
        .push_str("\n// retained after closing all panes");
    let edited = panes.panes[&EDITOR].text.clone();
    let mut pending_click = None;
    let mut saw_empty = false;
    let mut restored = false;
    for frame in 0..80 {
        let mut input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(1280.0, 820.0))),
            time: Some(f64::from(frame) / 60.0),
            ..Default::default()
        };
        if let Some(target_node) = pending_click.take() {
            input
                .events
                .push(Event::AccessKitActionRequest(ActionRequest {
                    action: Action::Click,
                    target_tree: TreeId::ROOT,
                    target_node,
                    data: None,
                }));
        }
        let mut restore = false;
        let (output, response) =
            run_editor(&mut dock, &context, input, &mut panes, &mut restore).unwrap();
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        if response.is_none() {
            saw_empty = true;
            assert!(dock.view().surfaces().next().is_none());
            assert!(output.shapes.iter().any(|shape| matches!(
                &shape.shape,
                egui::Shape::Text(text) if text.galley.text().starts_with("All panes are closed.")
            )));
            pending_click = nodes.iter().find_map(|(id, node)| {
                (node.role() == Role::Button
                    && node.label() == Some("Restore layout")
                    && node.supports_action(Action::Click))
                .then_some(*id)
            });
            assert!(
                pending_click.is_some(),
                "the empty editor must expose an actionable recovery"
            );
        } else if frame % 3 == 1 {
            let label = if dock.view().item(INSPECTOR).is_some() {
                "Close floating Inspector"
            } else {
                "Close Group"
            };
            pending_click = nodes
                .iter()
                .find_map(|(id, node)| {
                    (matches!(node.role(), Role::Button | Role::MenuItem)
                        && node.label() == Some(label)
                        && node.supports_action(Action::Click))
                    .then_some(*id)
                })
                .or_else(|| {
                    nodes.iter().find_map(|(id, node)| {
                        (node.role() == Role::Button
                            && node.label() == Some("Presentation commands")
                            && node.supports_action(Action::Click))
                        .then_some(*id)
                    })
                });
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        apply_editor_commands(&mut dock, response.as_ref(), restore).unwrap();
        if restore {
            assert!(saw_empty);
            restored = true;
            break;
        }
    }
    assert!(
        saw_empty && restored,
        "close the floating group and main root, then activate the empty editor's Restore button"
    );
    assert_eq!(panes.panes[&EDITOR].text, edited);
    assert_eq!(panes.panes.len(), 8);
    assert!(
        panes
            .panes
            .keys()
            .all(|item| dock.view().item(*item).is_some())
    );
}
