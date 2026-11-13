use super::*;

fn semantic_point(output: &FullOutput, role: Role, label: &str) -> Pos2 {
    let bounds = output
        .platform_output
        .accesskit_update
        .as_ref()
        .unwrap()
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == role && node.label() == Some(label))
                .then(|| node.bounds())
                .flatten()
        })
        .unwrap_or_else(|| panic!("the current output exposes {role:?} {label}"));
    egui::pos2(
        (bounds.x0 + bounds.x1) as f32 * 0.5,
        (bounds.y0 + bounds.y1) as f32 * 0.5,
    )
}

fn float_and_redock(context: &Context, dockspace: &mut Dockspace) {
    let destination = Pos2::new(500.0, 330.0);
    frame(
        context,
        dockspace,
        1.25,
        vec![
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(destination),
        ],
    );
    let (floating, _) = frame(
        context,
        dockspace,
        1.3,
        vec![Event::PointerButton {
            pos: destination,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::SHIFT,
        }],
    );
    assert!(!dockspace.has_active_gesture());
    assert!(dockspace.view().item(FIRST).unwrap().contained().is_some());
    assert!(dockspace.view().item(SECOND).unwrap().contained().is_none());

    redock(context, dockspace, &floating);
}

fn redock(context: &Context, dockspace: &mut Dockspace, floating: &FullOutput) {
    let source = semantic_point(floating, Role::TitleBar, "First");
    frame(
        context,
        dockspace,
        1.35,
        vec![
            Event::ModifiersChanged(Modifiers::NONE),
            Event::PointerMoved(source),
            button(source, true),
        ],
    );
    frame(
        context,
        dockspace,
        1.4,
        vec![Event::PointerMoved(source + egui::vec2(20.0, 8.0))],
    );
    let (preview, _) = frame(
        context,
        dockspace,
        1.45,
        vec![Event::PointerMoved(Pos2::new(100.0, 300.0))],
    );
    let guide = semantic_point(&preview, Role::Label, "Dockspace center drop guide");
    frame(
        context,
        dockspace,
        1.5,
        vec![Event::PointerMoved(guide), button(guide, false)],
    );
    assert!(!dockspace.has_active_gesture());
    assert!(dockspace.view().item(FIRST).unwrap().contained().is_none());
    assert_eq!(
        dockspace.view().item(FIRST).unwrap().tabs().items(),
        &[SECOND, FIRST]
    );
}

#[test]
fn a_replacement_press_move_and_release_share_the_new_route() {
    for explicit_cancel in [false, true] {
        let (context, mut dockspace, first, _) = presented_tabs("replacement-press-one-packet");
        frame(
            &context,
            &mut dockspace,
            1.0,
            vec![Event::PointerMoved(first), button(first, true)],
        );
        frame(
            &context,
            &mut dockspace,
            1.05,
            vec![Event::PointerMoved(first + egui::vec2(12.0, 30.0))],
        );
        assert!(dockspace.has_active_gesture());
        let destination = Pos2::new(500.0, 330.0);
        let mut events = Vec::new();
        if explicit_cancel {
            events.push(Event::Key {
                key: Key::Escape,
                physical_key: Some(Key::Escape),
                pressed: true,
                repeat: false,
                modifiers: Modifiers::NONE,
            });
        }
        events.extend([
            Event::PointerMoved(first),
            button(first, true),
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(destination),
            Event::PointerButton {
                pos: destination,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::SHIFT,
            },
        ]);
        let (floating, _) = frame(&context, &mut dockspace, 1.2, events);
        assert!(!dockspace.has_active_gesture());
        assert!(
            dockspace.view().item(FIRST).unwrap().contained().is_some(),
            "the replacement down, move and release must execute after cancellation in one packet",
        );
        redock(&context, &mut dockspace, &floating);
    }
}

#[test]
fn a_new_primary_press_recovers_after_the_old_release_is_lost() {
    for pointer_gone in [true, false] {
        let (context, mut dockspace, first, _) = presented_tabs("generic-lost-release");
        frame(
            &context,
            &mut dockspace,
            1.0,
            vec![Event::PointerMoved(first), button(first, true)],
        );
        frame(
            &context,
            &mut dockspace,
            1.05,
            vec![Event::PointerMoved(first + egui::vec2(12.0, 30.0))],
        );
        assert!(dockspace.has_active_gesture());
        if pointer_gone {
            frame(&context, &mut dockspace, 1.1, vec![Event::PointerGone]);
        }
        frame(
            &context,
            &mut dockspace,
            1.15,
            vec![Event::PointerMoved(first)],
        );
        frame(&context, &mut dockspace, 1.2, vec![button(first, true)]);
        assert!(
            dockspace.has_active_gesture(),
            "the new real press starts its own gesture"
        );

        float_and_redock(&context, &mut dockspace);
    }
}

fn cancelled_press_with_late_release(focus_loss: bool) {
    let (context, mut dockspace, first, _) = presented_tabs("cancelled-press-late-release");
    let version = dockspace.version();
    frame(
        &context,
        &mut dockspace,
        1.0,
        vec![Event::PointerMoved(first), button(first, true)],
    );
    frame(
        &context,
        &mut dockspace,
        1.05,
        vec![Event::PointerMoved(first + egui::vec2(12.0, 30.0))],
    );
    assert!(dockspace.has_active_gesture());
    let cancel = if focus_loss {
        Event::WindowFocused(false)
    } else {
        Event::Key {
            key: Key::Escape,
            physical_key: Some(Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }
    };
    frame(&context, &mut dockspace, 1.1, vec![cancel]);
    assert!(!dockspace.has_active_gesture());
    frame(
        &context,
        &mut dockspace,
        1.15,
        vec![
            Event::WindowFocused(true),
            button(Pos2::new(-80.0, -80.0), false),
        ],
    );
    assert!(!dockspace.has_active_gesture());
    assert_eq!(
        dockspace.version(),
        version,
        "a late release cannot commit a cancelled drag"
    );
    frame(
        &context,
        &mut dockspace,
        1.2,
        vec![Event::PointerMoved(first), button(first, true)],
    );
    assert!(dockspace.has_active_gesture());
    float_and_redock(&context, &mut dockspace);
}

#[test]
fn focus_loss_then_late_release_does_not_revive_a_gesture() {
    cancelled_press_with_late_release(true);
}

#[test]
fn escape_then_late_release_and_a_new_press_can_dock() {
    cancelled_press_with_late_release(false);
}
