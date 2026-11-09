//! Focus sampling and retention against real egui pass state.

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::{DockspaceHostFocus, DockspaceSession};
use egui::{Context, Id, Pos2, RawInput, Rect, TextEdit, Ui, ViewportId, WidgetText, vec2};

use super::{retain_presented_pane_focus, retain_skipped_pane_focus, sample_host_focus};
use crate::pane::PaneView;

const SURFACE: SurfaceId = SurfaceId::new(1);
const EDITOR: ItemId = ItemId::new(1);
const OTHER: ItemId = ItemId::new(2);

struct EditorPane {
    target: Id,
    text: String,
}

impl EditorPane {
    fn new() -> Self {
        Self {
            target: Id::unique("product-focus-test-editor"),
            text: "Editor".to_owned(),
        }
    }
}

impl PaneView for EditorPane {
    fn title(&self, _item: ItemId) -> Option<WidgetText> {
        Some("Editor".into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        if item == EDITOR {
            ui.add_sized(
                vec2(200.0, 32.0),
                TextEdit::singleline(&mut self.text).id(self.target),
            );
        }
        crate::PaneResponse::default()
    }

    fn focus_target(&self, item: ItemId) -> Option<Id> {
        (item == EDITOR).then_some(self.target)
    }
}

fn test_context() -> Context {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one pass is nonzero");
    });
    context
}

fn session(selected: ItemId) -> DockspaceSession {
    let node = DockspaceNode::tabs_with_selection([EDITOR, OTHER], Some(selected)).unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), node),
    )])
    .unwrap();
    DockspaceSession::from_layout(layout, Default::default()).unwrap()
}

fn input(viewport: ViewportId) -> RawInput {
    let mut input = RawInput {
        viewport_id: viewport,
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(640.0, 480.0))),
        focused: true,
        ..RawInput::default()
    };
    input.viewports.entry(viewport).or_default().focused = Some(true);
    input
}

fn run(context: &Context, input: RawInput, contents: impl FnMut(&mut Ui)) {
    context
        .run_ui(input, contents)
        .drop_without_applying_deltas();
}

fn focus_editor(context: &Context, pane: &mut EditorPane) {
    run(context, input(ViewportId::ROOT), |ui| {
        assert!(pane.ui(EDITOR, ui).content_widgets().next().is_none());
        ui.memory_mut(|memory| memory.request_focus(pane.target));
    });
    assert!(context.memory(|memory| memory.has_focus(pane.target)));
}

fn sample(
    context: &Context,
    session: &DockspaceSession,
    pane: &EditorPane,
    retained: &[(ItemId, Id)],
) -> DockspaceHostFocus {
    sample_host_focus(
        context,
        session.view().surface(SURFACE).unwrap(),
        pane,
        Id::unique("product-focus-test-workspace"),
        Rect::from_min_size(Pos2::ZERO, vec2(400.0, 480.0)),
        retained,
    )
}

#[test]
fn registered_editor_is_sampled_without_declaring_pointer_content() {
    let context = test_context();
    let session = session(EDITOR);
    let mut pane = EditorPane::new();
    focus_editor(&context, &mut pane);

    run(&context, input(ViewportId::ROOT), |ui| {
        assert!(context.viewport(|viewport| {
            viewport
                .prev_pass
                .widgets
                .get(pane.target)
                .is_some_and(|widget| {
                    widget.enabled && widget.visible && widget.sense.is_focusable()
                })
        }));
        assert_eq!(
            sample(&context, &session, &pane, &[]),
            DockspaceHostFocus::Pane(EDITOR)
        );
        assert!(context.memory(|memory| memory.has_focus(pane.target)));
        pane.ui(EDITOR, ui);
    });
}

#[test]
fn retained_editor_survives_two_skipped_passes_only_with_its_exact_witness() {
    let context = test_context();
    let session = session(EDITOR);
    let other_selected = self::session(OTHER);
    let mut pane = EditorPane::new();
    focus_editor(&context, &mut pane);
    let retained = [(EDITOR, pane.target)];

    run(&context, input(ViewportId::ROOT), |_| {
        assert_eq!(
            sample(&context, &session, &pane, &[]),
            DockspaceHostFocus::Pane(EDITOR)
        );
        retain_presented_pane_focus(&context, &retained);
        assert!(context.viewport(|viewport| viewport.this_pass.widgets.get(pane.target).is_none()));
    });
    assert!(context.memory(|memory| memory.has_focus(pane.target)));

    run(&context, input(ViewportId::ROOT), |_| {
        assert!(context.viewport(|viewport| viewport.prev_pass.widgets.get(pane.target).is_none()));
        assert_eq!(
            sample(&context, &session, &pane, &[]),
            DockspaceHostFocus::Unknown
        );
        assert_eq!(
            sample(&context, &session, &pane, &[(OTHER, pane.target)]),
            DockspaceHostFocus::Unknown
        );
        assert_eq!(
            sample(
                &context,
                &session,
                &pane,
                &[(EDITOR, pane.target.with("other-widget"))]
            ),
            DockspaceHostFocus::Unknown
        );
        assert_eq!(
            sample(&context, &other_selected, &pane, &retained),
            DockspaceHostFocus::Unknown
        );
        assert_eq!(
            sample(&context, &session, &pane, &retained),
            DockspaceHostFocus::Pane(EDITOR)
        );
        retain_presented_pane_focus(&context, &retained);
        assert!(context.viewport(|viewport| viewport.this_pass.widgets.get(pane.target).is_none()));
    });
    assert!(context.memory(|memory| memory.has_focus(pane.target)));

    run(&context, input(ViewportId::ROOT), |_| {
        retain_presented_pane_focus(&context, &[]);
        assert!(context.memory(|memory| memory.has_focus(pane.target)));
        assert_eq!(
            sample(&context, &session, &pane, &[]),
            DockspaceHostFocus::Unknown
        );
    });
    assert_eq!(context.memory(|memory| memory.focused()), None);
}

#[test]
fn an_unregistered_child_target_cannot_borrow_the_roots_identical_widget_id() {
    let context = test_context();
    let session = session(EDITOR);
    let mut pane = EditorPane::new();
    focus_editor(&context, &mut pane);
    let child = ViewportId::from_hash_of("product-focus-test-child");

    run(&context, input(child), |ui| {
        assert_eq!(context.viewport_id(), child);
        assert!(context.viewport_for(ViewportId::ROOT, |viewport| {
            viewport.prev_pass.widgets.get(pane.target).is_some()
        }));
        assert!(context.viewport(|viewport| viewport.prev_pass.widgets.get(pane.target).is_none()));
        ui.memory_mut(|memory| memory.request_focus(pane.target));
        assert!(context.memory(|memory| memory.has_focus(pane.target)));
        assert_eq!(
            sample(&context, &session, &pane, &[]),
            DockspaceHostFocus::Unknown
        );
    });
}

#[test]
fn a_retained_record_cannot_override_window_blur_or_resurrect_surrendered_focus() {
    let context = test_context();
    let session = session(EDITOR);
    let mut pane = EditorPane::new();
    focus_editor(&context, &mut pane);
    let retained = [(EDITOR, pane.target)];
    let mut unfocused = input(ViewportId::ROOT);
    unfocused.focused = false;
    unfocused
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .focused = Some(false);

    run(&context, unfocused, |_| {
        assert!(context.memory(|memory| memory.has_focus(pane.target)));
        assert_eq!(
            sample(&context, &session, &pane, &retained),
            DockspaceHostFocus::Unknown
        );
        retain_presented_pane_focus(&context, &retained);
    });
    run(&context, input(ViewportId::ROOT), |ui| {
        ui.memory_mut(|memory| memory.surrender_focus(pane.target));
        assert_eq!(context.memory(|memory| memory.focused()), None);
        retain_presented_pane_focus(&context, &retained);
        assert_eq!(
            sample(&context, &session, &pane, &retained),
            DockspaceHostFocus::None
        );
        assert_eq!(context.memory(|memory| memory.focused()), None);
    });
    assert_eq!(context.memory(|memory| memory.focused()), None);
}

#[test]
fn skipped_pane_focus_requires_the_current_selected_target_and_never_requests_focus() {
    let context = test_context();
    let selected = session(EDITOR);
    let unselected = session(OTHER);
    let mut pane = EditorPane::new();
    focus_editor(&context, &mut pane);
    let mut blurred = input(ViewportId::ROOT);
    blurred.focused = false;
    blurred
        .viewports
        .get_mut(&ViewportId::ROOT)
        .unwrap()
        .focused = Some(false);
    for _ in 0..2 {
        run(&context, blurred.clone(), |_| {
            retain_skipped_pane_focus(&context, selected.view().surface(SURFACE).unwrap(), &pane);
        });
        assert!(context.memory(|memory| memory.has_focus(pane.target)));
    }
    run(&context, input(ViewportId::ROOT), |_| {
        retain_skipped_pane_focus(&context, unselected.view().surface(SURFACE).unwrap(), &pane);
    });
    assert_eq!(context.memory(|memory| memory.focused()), None);
    run(&context, input(ViewportId::ROOT), |_| {
        retain_skipped_pane_focus(&context, selected.view().surface(SURFACE).unwrap(), &pane);
    });
    assert_eq!(context.memory(|memory| memory.focused()), None);

    focus_editor(&context, &mut pane);
    let old_target = pane.target;
    pane.target = pane.target.with("replacement-editor");
    run(&context, input(ViewportId::ROOT), |_| {
        retain_skipped_pane_focus(&context, selected.view().surface(SURFACE).unwrap(), &pane);
    });
    assert!(!context.memory(|memory| memory.has_focus(old_target)));
    assert_eq!(context.memory(|memory| memory.focused()), None);
}
