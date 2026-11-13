//! Item-local egui state survives docking and host UI reparenting in one Context.

use std::collections::BTreeMap;

use egui::text::{CCursor, CCursorRange};
use egui::{Context, Event, Id, Pos2, RawInput, Rect, ScrollArea, TextEdit, Ui, UiBuilder, Vec2};
use egui_dockspace::{
    DockPlacement, Dockspace, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    SurfacePresentationResult, Tab, TabBarId, TabVisuals,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const EDITOR: ItemId = ItemId::new(1);
const SIBLING: ItemId = ItemId::new(2);
const DESTINATION: ItemId = ItemId::new(3);

#[derive(Clone, Copy, Debug)]
struct ContentSnapshot {
    scope: Id,
    editor: Id,
    selection: Option<CCursorRange>,
    scroll: Id,
    scroll_offset: Vec2,
}

struct Documents {
    text: BTreeMap<ItemId, String>,
    snapshots: BTreeMap<ItemId, ContentSnapshot>,
    groups: BTreeMap<ItemId, TabBarId>,
    explicit_focus: Option<Id>,
    scroll_to: Option<(ItemId, f32)>,
}

impl PaneView for Documents {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.text
            .contains_key(&item)
            .then(|| format!("Document {item}").into())
    }

    fn tab_ui(&mut self, ui: &mut Ui, tab: &Tab<'_>) -> TabVisuals {
        self.groups.insert(tab.item, tab.bar);
        tab.paint(ui)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        let mut editor = TextEdit::multiline(self.text.get_mut(&item).unwrap())
            .desired_rows(4)
            .desired_width(f32::INFINITY);
        if item == EDITOR
            && let Some(target) = self.explicit_focus
        {
            editor = editor.id(target);
        }
        let editor = editor.show(ui);
        let mut scroll = ScrollArea::vertical().max_height(120.0).animated(false);
        if let Some((target, offset)) = self.scroll_to
            && target == item
        {
            self.scroll_to = None;
            scroll = scroll.vertical_scroll_offset(offset);
        }
        let scroll = scroll.show(ui, |ui| {
            for line in 0..64 {
                ui.label(format!("Document {item}, history line {line}"));
            }
        });
        self.snapshots.insert(
            item,
            ContentSnapshot {
                scope: ui.scope_id(),
                editor: editor.response.id,
                selection: editor.state.cursor.char_range(),
                scroll: scroll.id,
                scroll_offset: scroll.state.clamped_offset(),
            },
        );
        PaneResponse::from(&*editor.response)
    }

    fn focus_target(&self, item: ItemId) -> Option<Id> {
        (item == EDITOR).then_some(self.explicit_focus).flatten()
    }
}

struct Harness {
    context: Context,
    dock: Dockspace,
    documents: Documents,
    owned: bool,
    parent_salt: u64,
    time: f64,
}

impl Harness {
    fn new(owned: bool, explicit_focus: Option<Id>) -> Self {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let content = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::tabs_with_selection([EDITOR, SIBLING], Some(EDITOR)).unwrap(),
                DockspaceNode::central_tabs([DESTINATION]),
            ],
        )
        .unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(RootId::new(1), content),
        )])
        .unwrap();
        let mut harness = Self {
            context,
            dock: Dockspace::builder(("content-identity", owned), layout)
                .build()
                .unwrap(),
            documents: Documents {
                text: [EDITOR, SIBLING, DESTINATION]
                    .map(|item| (item, "alpha bravo charlie delta".to_owned()))
                    .into_iter()
                    .collect(),
                snapshots: BTreeMap::new(),
                groups: BTreeMap::new(),
                explicit_focus,
                scroll_to: None,
            },
            owned,
            parent_salt: 0,
            time: 0.0,
        };
        #[cfg(feature = "motion")]
        harness
            .dock
            .set_motion_preference(egui_dockspace::motion::MotionPreference::Full);
        for _ in 0..4 {
            harness.frame(Vec::new());
        }
        harness
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.time += 0.1;
        self.documents.snapshots.clear();
        self.documents.groups.clear();
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
            focused: true,
            time: Some(self.time),
            events,
            ..RawInput::default()
        };
        let output = if self.owned {
            self.dock
                .run_single_surface(&self.context, input, SURFACE, &mut self.documents)
                .unwrap()
                .0
        } else {
            egui_dockspace::run_ui(&self.context, input, |ui| {
                ui.scope_builder(
                    UiBuilder::new().id_salt(("host-container", self.parent_salt)),
                    |ui| {
                        self.dock
                            .show_single_surface(SURFACE, ui, &mut self.documents)
                            .unwrap();
                    },
                );
            })
        };
        let (mut output, receipt) = output.into_parts();
        output.textures_delta.clear();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }

    fn snapshot(&self, item: ItemId) -> ContentSnapshot {
        *self
            .documents
            .snapshots
            .get(&item)
            .expect("the current frame renders this selected document")
    }

    fn focus_and_select_editor(&mut self) -> ContentSnapshot {
        let editor = self.snapshot(EDITOR).editor;
        let mut state = TextEdit::load_state(&self.context, editor).unwrap();
        state.cursor.set_char_range(Some(CCursorRange::two(
            CCursor::new(6_usize),
            CCursor::new(11_usize),
        )));
        state.store(&self.context, editor);
        self.context
            .memory_mut(|memory| memory.request_focus(editor));
        self.documents.scroll_to = Some((EDITOR, 120.0));
        self.frame(Vec::new());
        self.frame(Vec::new());
        let snapshot = self.snapshot(EDITOR);
        assert!(self.context.memory(|memory| memory.has_focus(editor)));
        assert_eq!(
            snapshot.selection,
            Some(CCursorRange::two(
                CCursor::new(6_usize),
                CCursor::new(11_usize)
            ))
        );
        assert!((snapshot.scroll_offset.y - 120.0).abs() < 0.01);
        snapshot
    }

    fn assert_editor_state(&self, expected: ContentSnapshot) {
        let actual = self.snapshot(EDITOR);
        assert_eq!(actual.scope, expected.scope);
        assert_eq!(actual.editor, expected.editor);
        assert_eq!(actual.selection, expected.selection);
        assert_eq!(actual.scroll, expected.scroll);
        assert!((actual.scroll_offset - expected.scroll_offset).length() < 0.01);
        assert!(
            self.context
                .memory(|memory| memory.has_focus(actual.editor))
        );
    }
}

#[test]
fn item_local_state_and_focus_survive_moving_between_tab_groups() {
    for owned in [true, false] {
        let mut harness = Harness::new(owned, None);
        let before = harness.focus_and_select_editor();
        let source_group = harness.documents.groups[&EDITOR];
        let destination_group = harness.documents.groups[&DESTINATION];
        assert_ne!(source_group, destination_group);

        harness
            .dock
            .dock_item_current(EDITOR, DockPlacement::After(DESTINATION))
            .unwrap();
        harness.frame(Vec::new());

        assert_eq!(harness.documents.groups[&EDITOR], destination_group);
        assert_eq!(harness.documents.groups[&SIBLING], source_group);
        assert!(harness.dock.view().item(EDITOR).unwrap().is_selected());
        harness.assert_editor_state(before);
        let sibling = harness.snapshot(SIBLING);
        assert_ne!(sibling.editor, before.editor);
        assert_ne!(sibling.scroll, before.scroll);
        assert!(sibling.scroll_offset.length() < 0.01);

        harness.frame(vec![Event::Text("revised".to_owned())]);
        assert_eq!(
            harness.documents.text[&EDITOR],
            "alpha revised charlie delta"
        );
        assert_eq!(
            harness.documents.text[&SIBLING],
            "alpha bravo charlie delta"
        );
    }
}

#[test]
fn content_state_and_focus_survive_changing_the_host_parent_scope() {
    let mut harness = Harness::new(false, None);
    let before = harness.focus_and_select_editor();
    harness.parent_salt = 1;
    harness.frame(Vec::new());
    harness.assert_editor_state(before);
}

#[test]
fn declared_global_focus_target_stays_exact_after_docking() {
    for owned in [true, false] {
        let target = Id::unique(("application-editor-focus", owned));
        let mut harness = Harness::new(owned, Some(target));
        let before = harness.focus_and_select_editor();
        assert_eq!(before.editor, target);
        harness
            .dock
            .dock_item_current(EDITOR, DockPlacement::After(DESTINATION))
            .unwrap();
        harness.frame(Vec::new());
        harness.assert_editor_state(before);
        assert_eq!(harness.documents.focus_target(EDITOR), Some(target));
        harness.frame(vec![Event::Text("revised".to_owned())]);
        assert_eq!(
            harness.documents.text[&EDITOR],
            "alpha revised charlie delta"
        );
    }
}

#[test]
fn replacing_workspace_epoch_does_not_reuse_retired_content_state() {
    let mut harness = Harness::new(true, None);
    let before = harness.focus_and_select_editor();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([EDITOR])),
    )])
    .unwrap();
    harness.dock.reset_layout(layout).unwrap();
    harness.frame(Vec::new());
    let after = harness.snapshot(EDITOR);
    assert_ne!(before.scope, after.scope);
    assert_ne!(before.editor, after.editor);
    assert_ne!(before.scroll, after.scroll);
}
