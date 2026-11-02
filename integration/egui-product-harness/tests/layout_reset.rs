//! Application defaults work without serialized documents or persistence features.

use std::collections::BTreeMap;

use egui::{Context, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    CloseDecision, Dockspace, DockspaceCloseRequestStatus, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, PaneView, RootId, SurfaceId,
    SurfacePresentationResult, run_ui,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);

fn default_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )])
    .unwrap()
}

struct Panes {
    text: BTreeMap<ItemId, String>,
    rendered: Vec<ItemId>,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.text
            .get(&item)
            .map(|_| format!("Document {}", item.get()).into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        self.rendered.push(item);
        ui.add(
            egui::TextEdit::multiline(self.text.get_mut(&item).unwrap())
                .id(egui::Id::unique(("default-pane", item.get()))),
        );
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        self.text
            .contains_key(&item)
            .then(|| egui::Id::unique(("default-pane", item.get())))
    }
}

fn frame(context: &Context, dockspace: &mut Dockspace, panes: &mut Panes) {
    let surface = dockspace.view().surfaces().next().unwrap().id();
    let (mut output, receipt) = run_ui(
        context,
        RawInput {
            screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
            focused: true,
            ..Default::default()
        },
        |ui| {
            dockspace.show_single_surface(surface, ui, panes).unwrap();
        },
    )
    .into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
}

#[test]
fn application_default_reset_preserves_semantic_identities_pane_data_and_rules() {
    let context = Context::default();
    context.enable_accesskit();
    let default = default_layout();
    let mut dockspace = Dockspace::builder("serde-free-default", default.clone())
        .build()
        .unwrap();
    let mut panes = Panes {
        text: BTreeMap::from([
            (FIRST, "Unsaved first document".into()),
            (SECOND, "Unsaved second document".into()),
        ]),
        rendered: Vec::new(),
    };
    frame(&context, &mut dockspace, &mut panes);
    frame(&context, &mut dockspace, &mut panes);
    dockspace.select_item_current(SECOND).unwrap();
    frame(&context, &mut dockspace, &mut panes);
    assert!(context.memory(|memory| memory.has_focus(panes.focus_target(SECOND).unwrap())));
    let close = dockspace.request_close_root_current(ROOT).unwrap();
    let DockspaceCloseRequestStatus::Requested(request) = close.status() else {
        panic!("the application approves closure through the ordinary public workflow");
    };
    for item in request.plan().items() {
        dockspace
            .resolve_close(request.plan().request(), item.token(), CloseDecision::Allow)
            .unwrap();
    }
    assert!(dockspace.view().surfaces().next().is_none());
    let mutation = dockspace.reset_layout(default.clone()).unwrap();
    assert!(mutation.workspace_changed());
    let restored = dockspace.view().item(FIRST).unwrap();
    assert_eq!(restored.surface(), SURFACE);
    assert_eq!(restored.root(), ROOT);
    assert_eq!(restored.tabs().items(), &[FIRST, SECOND]);
    assert!(
        restored.is_selected(),
        "the declared default selection is retained"
    );
    panes.rendered.clear();
    frame(&context, &mut dockspace, &mut panes);
    frame(&context, &mut dockspace, &mut panes);
    assert!(panes.rendered.contains(&FIRST));
    assert_eq!(panes.text[&FIRST], "Unsaved first document");
    assert_eq!(panes.text[&SECOND], "Unsaved second document");
    let mut locked = dockspace.policy().clone();
    locked.set_arrangement_locked(true);
    dockspace.set_policy(locked).unwrap();
    let version = dockspace.version();
    assert!(dockspace.reset_layout(default.clone()).is_err());
    assert_eq!(dockspace.version(), version);
    assert!(dockspace.policy().arrangement_locked());
    let mut unlocked = dockspace.policy().clone();
    unlocked.set_arrangement_locked(false);
    dockspace.set_policy(unlocked).unwrap();
    dockspace.reset_layout(default).unwrap();
    frame(&context, &mut dockspace, &mut panes);
    assert!(dockspace.view().item(FIRST).unwrap().is_selected());
    assert_eq!(panes.text[&SECOND], "Unsaved second document");
}
