#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::collections::BTreeMap;
use std::error::Error;
use std::io;

use dockspace::backend::graph::{Axis, Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::backend::ids::{ItemId, RootId, SurfaceId};
use dockspace::backend::surface_recovery::SurfaceRecoveryBootstrap;
use dockspace::document::{DockspaceDocumentBootstrap, DockspaceDocumentId};
use dockspace::policy::{ContainedFallback, DockPolicy};
use dockspace::viewport::WindowToken;
use eframe::egui;
use egui_dockspace::{Dockspace, PaneView};
use egui_dockspace_native::{
    AllowNativeClose, NativeDockspaceApp, NativeSurfaceSpec, NativeViewportRoster,
    restore_document_from_storage,
};

const ROOT_SURFACE: SurfaceId = SurfaceId::new(1);
const INSPECTOR_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const INSPECTOR_ROOT: RootId = RootId::new(2);
const DOCUMENT_ID: DockspaceDocumentId = DockspaceDocumentId::from_bytes(*b"native-multiview");
const OUTLINE_KEY: &str = "example/outline";
const EDITOR_KEY: &str = "example/editor";
const INSPECTOR_KEY: &str = "example/inspector";

type ExampleError = Box<dyn Error + Send + Sync>;

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("egui_dockspace native multiview")
            .with_inner_size([1180.0, 760.0]),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "egui_dockspace native multiview",
        options,
        Box::new(|creation_context| Ok(Box::new(build_app(creation_context.storage)?))),
    )
}

fn build_app(
    storage: Option<&dyn eframe::Storage>,
) -> Result<NativeDockspaceApp<ExamplePanes>, ExampleError> {
    let (bootstrap, fallback_ids) = example_bootstrap()?;
    let mut policy = DockPolicy::new();
    policy.set_allow_native_surfaces(true);
    policy.set_contained_fallback(ContainedFallback::Enabled);

    let mut dockspace =
        Dockspace::backend_builder("native-multiview", example_workspace(fallback_ids))
            .policy(policy)
            .build()?;
    let restored =
        restore_document_from_storage(&mut dockspace, storage, |document, item, key| {
            document == DOCUMENT_ID && fallback_ids.item_for_key(key) == Some(item)
        })?;
    if restored.is_none() {
        dockspace.bind_document_persistence(bootstrap)?;
    }

    let pane_ids = PaneIds::from_dockspace(&dockspace)?;
    let roster = restored_roster(&dockspace)?;
    NativeDockspaceApp::new(dockspace, ExamplePanes::new(pane_ids), roster)
        .map(|app| app.with_close_handler(AllowNativeClose))
        .map_err(Into::into)
}

fn example_bootstrap() -> Result<(DockspaceDocumentBootstrap, PaneIds), ExampleError> {
    let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT_ID, 0);
    let ids = PaneIds {
        outline: bootstrap.ensure_external_item_key(OUTLINE_KEY)?,
        editor: bootstrap.ensure_external_item_key(EDITOR_KEY)?,
        inspector: bootstrap.ensure_external_item_key(INSPECTOR_KEY)?,
    };
    Ok((bootstrap, ids))
}

fn restored_roster(dockspace: &Dockspace) -> Result<NativeViewportRoster, ExampleError> {
    let workspace = dockspace.workspace();
    if workspace.surface(ROOT_SURFACE).is_none() {
        return Err(io::Error::other("restored document omitted the example root surface").into());
    }

    let mut roster =
        NativeViewportRoster::new(NativeSurfaceSpec::root(ROOT_SURFACE, WindowToken::new(1)))?;
    for (index, (surface, _)) in workspace
        .surfaces()
        .filter(|(surface, _)| *surface != ROOT_SURFACE)
        .enumerate()
    {
        let token = u64::try_from(index)
            .ok()
            .and_then(|index| index.checked_add(2))
            .ok_or(egui_dockspace_native::NativeRuntimeError::IdentityExhausted)?;
        let mut spec = NativeSurfaceSpec::restored_child(
            egui::ViewportId::from_hash_of(("native-multiview-child", surface.get())),
            surface,
            WindowToken::new(token),
            SurfaceRecoveryBootstrap::new(ROOT_SURFACE),
            child_builder(surface),
        );
        if let Some(preference) = dockspace.viewport_placement_preference(surface).copied() {
            spec = spec.try_with_restored_placement(preference)?;
        }
        roster.insert(spec)?;
    }
    Ok(roster)
}

fn child_builder(surface: SurfaceId) -> egui::ViewportBuilder {
    let builder =
        egui::ViewportBuilder::default().with_title(format!("Dockspace surface {}", surface.get()));
    if surface == INSPECTOR_SURFACE {
        builder
            .with_title("Inspector - egui_dockspace")
            .with_inner_size([440.0, 620.0])
            .with_position([1220.0, 120.0])
    } else {
        builder
    }
}

fn example_workspace(ids: PaneIds) -> Workspace {
    let mut builder = Workspace::builder();
    let outline = builder.insert_node(Node::tabs([ids.outline]));
    let editor = builder.insert_node(Node::tabs([ids.editor]));
    let main = builder.insert_node(
        Node::split(Axis::Horizontal, [outline, editor], [0.2, 0.8])
            .expect("the static root split is valid"),
    );
    let inspector = builder.insert_node(Node::tabs([ids.inspector]));

    builder.set_root(ROOT, RootRecord::new(main).with_central(editor));
    builder.set_root(INSPECTOR_ROOT, RootRecord::new(inspector));
    builder.set_surface(ROOT_SURFACE, SurfacePresentation::with_main(ROOT));
    builder.set_surface(
        INSPECTOR_SURFACE,
        SurfacePresentation::with_main(INSPECTOR_ROOT),
    );
    builder.build().expect("the static workspace is valid")
}

struct ExamplePane {
    title: &'static str,
    body: String,
}

struct ExamplePanes {
    panes: BTreeMap<ItemId, ExamplePane>,
}

impl ExamplePanes {
    fn new(ids: PaneIds) -> Self {
        Self {
            panes: BTreeMap::from([
                (
                    ids.outline,
                    ExamplePane {
                        title: "Outline",
                        body: "workspace\n  editor\n  inspector".to_owned(),
                    },
                ),
                (
                    ids.editor,
                    ExamplePane {
                        title: "Editor",
                        body: "Drag this tab to an edge, another window, or the desktop."
                            .to_owned(),
                    },
                ),
                (
                    ids.inspector,
                    ExamplePane {
                        title: "Inspector",
                        body: "Selection: Editor".to_owned(),
                    },
                ),
            ]),
        }
    }
}

#[derive(Clone, Copy)]
struct PaneIds {
    outline: ItemId,
    editor: ItemId,
    inspector: ItemId,
}

impl PaneIds {
    fn item_for_key(self, key: &str) -> Option<ItemId> {
        match key {
            OUTLINE_KEY => Some(self.outline),
            EDITOR_KEY => Some(self.editor),
            INSPECTOR_KEY => Some(self.inspector),
            _ => None,
        }
    }

    fn from_dockspace(dockspace: &Dockspace) -> Result<Self, ExampleError> {
        Ok(Self {
            outline: required_pane_id(dockspace, OUTLINE_KEY)?,
            editor: required_pane_id(dockspace, EDITOR_KEY)?,
            inspector: required_pane_id(dockspace, INSPECTOR_KEY)?,
        })
    }
}

fn required_pane_id(dockspace: &Dockspace, key: &str) -> Result<ItemId, ExampleError> {
    dockspace.item_id_for_external_key(key).ok_or_else(|| {
        io::Error::other(format!("restored document omitted required pane key {key}")).into()
    })
}

impl PaneView for ExamplePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        self.panes.get(&item).map(|pane| pane.title.into())
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) {
        let Some(pane) = self.panes.get_mut(&item) else {
            return;
        };
        ui.add(
            egui::TextEdit::multiline(&mut pane.body)
                .desired_width(f32::INFINITY)
                .desired_rows(16),
        );
    }
}
