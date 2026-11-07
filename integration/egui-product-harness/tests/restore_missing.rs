#![cfg(feature = "serde")]

use dockspace::policy::DockPolicy;
use egui::{Context, FullOutput, Pos2, RawInput, Rect, Ui, vec2};
use egui_dockspace::{
    DockStyle, Dockspace, DockspaceCapability, DockspaceDocumentBootstrap, DockspaceDocumentId,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    DockspaceUnavailableReason, ItemId, PaneView, RootId, SurfaceId, SurfacePresentationResult,
};

const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes([0x45; 16]);
const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

struct Registry {
    item: ItemId,
    text: Option<String>,
    callbacks: usize,
    target: egui::Id,
}

impl PaneView for Registry {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        (item == self.item && self.text.is_some()).then(|| "Recovered document".into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> egui_dockspace::PaneResponse {
        assert_eq!(item, self.item);
        self.callbacks += 1;
        let text = self
            .text
            .as_mut()
            .expect("unavailable content must use the built-in placeholder");
        ui.add(egui::TextEdit::multiline(text).id(self.target));
        egui_dockspace::PaneResponse::default()
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        (item == self.item && self.text.is_some()).then_some(self.target)
    }
}

fn frame(
    context: &Context,
    dockspace: &mut Dockspace,
    registry: &mut Registry,
) -> (FullOutput, Vec<ItemId>, DockspaceCapability) {
    let surface = dockspace.view().surfaces().next().unwrap().id();
    let (output, response) = dockspace
        .run_single_surface(
            context,
            RawInput {
                screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
                focused: true,
                ..Default::default()
            },
            surface,
            registry,
        )
        .unwrap();
    let (mut output, receipt) = output.into_parts();
    receipt.report(SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    (
        output,
        response.missing_panes().to_vec(),
        response.pane_focus_capability(),
    )
}

#[test]
fn recognized_missing_content_restores_as_a_stable_placeholder_then_recovers_in_place() {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT);
    let item = bootstrap.ensure_item("plugin/document").unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([item])),
    )])
    .unwrap();
    let mut source = Dockspace::builder("missing-source", layout)
        .persistence(bootstrap)
        .build()
        .unwrap();
    let snapshot = source.save_document_json().unwrap();
    // Identity is recognized even while the plugin which supplies its content is unavailable.
    let mut dockspace = Dockspace::from_document_json(
        "missing-restored",
        &snapshot,
        DockPolicy::default(),
        DockStyle::default(),
        |document, key| document == DOCUMENT && key == "plugin/document",
    )
    .unwrap();
    assert_eq!(
        dockspace.item_id_for_external_key("plugin/document"),
        Some(item)
    );
    let before = dockspace.version();
    let original_root = dockspace.view().item(item).unwrap().root();
    let context = Context::default();
    context.enable_accesskit();
    let mut registry = Registry {
        item,
        text: None,
        callbacks: 0,
        target: egui::Id::unique("restored-document-focus"),
    };
    frame(&context, &mut dockspace, &mut registry);
    frame(&context, &mut dockspace, &mut registry);
    dockspace.select_item_current(item).unwrap();
    let (output, missing, capability) = frame(&context, &mut dockspace, &mut registry);
    assert_eq!(missing, [item]);
    assert_eq!(registry.callbacks, 0);
    assert_eq!(dockspace.version(), before);
    assert_eq!(dockspace.view().item(item).unwrap().root(), original_root);
    assert!(matches!(
        capability,
        DockspaceCapability::Unavailable(DockspaceUnavailableReason::PaneFocusBindingUnavailable)
    ));
    assert!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .any(|(_, node)| {
                node.label() == Some(format!("Missing pane {}", item.get()).as_str())
            })
    );
    assert!(!context.memory(|memory| memory.has_focus(registry.target)));
    // Commit the unavailable answer before making the application binding available.
    frame(&context, &mut dockspace, &mut registry);
    registry.text = Some("Application data loaded after layout restore".to_owned());
    let (_, missing, _) = frame(&context, &mut dockspace, &mut registry);
    assert!(missing.is_empty());
    assert!(registry.callbacks > 0);
    assert!(
        !context.memory(|memory| memory.has_focus(registry.target)),
        "completed unavailable requests must not replay when content reappears"
    );
    dockspace.select_item_current(item).unwrap();
    frame(&context, &mut dockspace, &mut registry);
    assert!(context.memory(|memory| memory.has_focus(registry.target)));
    let (_, missing, _) = frame(&context, &mut dockspace, &mut registry);
    assert!(missing.is_empty());
    assert_eq!(
        dockspace.item_id_for_external_key("plugin/document"),
        Some(item)
    );
    assert_eq!(dockspace.view().item(item).unwrap().root(), original_root);
    assert_eq!(
        registry.text.as_deref(),
        Some("Application data loaded after layout restore")
    );
}
