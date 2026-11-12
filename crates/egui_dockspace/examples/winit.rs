//! Small complete-output editor using the optional winit/WGPU runner.

use dockspace::policy::{CloseCapability, DockPolicy};
use egui_dockspace::{
    Dockspace, DockspaceLayout, DockspaceNode, DockspaceOutput, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, PaneResponse, PaneView, RootId, SurfaceId,
    winit::{AppResult, NativeApp, NativeOptions, run_native},
};

const SURFACE: SurfaceId = SurfaceId::new(1);

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1), ItemId::new(2)]),
        ),
    )])?;
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::Disabled);
    let mut editor = Editor {
        dockspace: Dockspace::builder("editor", layout)
            .policy(policy)
            .build()?,
        documents: Documents([
            "Edit this document. Drag its tab to split or move it.".into(),
            "Hold Shift while dragging to float this tab inside the window.".into(),
        ]),
    };
    run_native(
        NativeOptions::default(),
        egui::Context::default(),
        &mut editor,
    )?;
    Ok(())
}

struct Editor {
    dockspace: Dockspace,
    documents: Documents,
}

impl NativeApp for Editor {
    fn update(
        &mut self,
        context: &egui::Context,
        input: egui::RawInput,
    ) -> AppResult<DockspaceOutput> {
        let (output, _) =
            self.dockspace
                .run_single_surface(context, input, SURFACE, &mut self.documents)?;
        Ok(output)
    }
}

struct Documents([String; 2]);

impl PaneView for Documents {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item.get() {
            1 => Some("Document".into()),
            2 => Some("Notes".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let index = usize::from(item.get() == 2);
        PaneResponse::from(
            &ui.add_sized(
                ui.available_size(),
                egui::TextEdit::multiline(&mut self.0[index])
                    .id(document_focus(item))
                    .frame(egui::Frame::NONE),
            ),
        )
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        Some(document_focus(item))
    }
}

fn document_focus(item: ItemId) -> egui::Id {
    egui::Id::unique(("editor-document", item))
}
