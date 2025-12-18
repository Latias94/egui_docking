use egui::ViewportBuilder;
use egui_tiles::Tree;

/// A scripted "workspace preset": one root dock tree plus zero or more detached native viewport trees.
///
/// This is the closest `egui_docking` concept to Dear ImGui's:
/// - `DockSpaceOverViewport` (root dockspace), plus
/// - multiple platform windows hosting additional dock trees.
///
/// It is designed for game-engine/editor use cases where you want to define a deterministic default
/// layout in code (Unity-style) and then allow the user to customize it at runtime.
#[derive(Debug)]
pub struct WorkspaceLayout<Pane> {
    pub root: Tree<Pane>,
    pub detached: Vec<DetachedViewportLayout<Pane>>,
}

impl<Pane> WorkspaceLayout<Pane> {
    pub fn new(root: Tree<Pane>) -> Self {
        Self {
            root,
            detached: Vec::new(),
        }
    }
}

/// A detached native viewport (OS window) hosting a dock tree.
#[derive(Debug)]
pub struct DetachedViewportLayout<Pane> {
    pub builder: ViewportBuilder,
    pub tree: Tree<Pane>,
}

impl<Pane> DetachedViewportLayout<Pane> {
    pub fn new(builder: ViewportBuilder, tree: Tree<Pane>) -> Self {
        Self { builder, tree }
    }
}

