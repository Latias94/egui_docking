//! Stable semantic focus successors for shared workspace replacement.

use super::*;
use crate::platform::focus::WorkspaceReplacementFocus;

impl DockEngine {
    pub(super) fn request_workspace_replacement_focus(
        &mut self,
        previous: WorkspaceReplacementFocus,
        previously_native: &BTreeSet<SurfaceId>,
        causal: FocusCausalStamp,
    ) -> Result<(), EngineError> {
        let Some((surface, focus)) = self.workspace_replacement_focus_target(previous) else {
            return Ok(());
        };
        let requires_native_binding =
            previously_native.contains(&surface) || self.viewport.viewport(surface).is_some();
        self.viewport_focus
            .request_workspace_replacement_focus(surface, focus, requires_native_binding, causal)
            .map_err(|source| EngineError::CausedViewportFocus {
                cause: causal.cause(),
                source,
            })?;
        Ok(())
    }

    fn workspace_replacement_focus_target(
        &self,
        previous: WorkspaceReplacementFocus,
    ) -> Option<(SurfaceId, PanelFocus)> {
        let previous = match previous {
            WorkspaceReplacementFocus::OutsideDock => return None,
            WorkspaceReplacementFocus::NoHistory => None,
            WorkspaceReplacementFocus::Dock { surface, item } => Some((surface, item)),
        };
        if let Some(item) = previous.and_then(|(_, item)| item)
            && let Some((root, tabs, _)) = self.workspace.locate_item_by_id(item)
            && self.replacement_selected_item(tabs) == Some(item)
            && let Some(surface) = self.root_surface(root)
        {
            return Some((surface, PanelFocus::Item(item)));
        }
        let surface = previous
            .map(|(surface, _)| surface)
            .filter(|surface| self.workspace.surface(*surface).is_some())
            .or_else(|| self.workspace.surfaces().next().map(|(surface, _)| surface))?;
        let presentation = self.workspace.surface(surface)?;
        let item = presentation
            .main_root
            .and_then(|root| self.replacement_root_selection(root))
            .or_else(|| {
                presentation.contained.iter().rev().find_map(|floating| {
                    let root = self.workspace.contained_floating(*floating)?.root;
                    self.replacement_root_selection(root)
                })
            });
        Some((surface, item.map_or(PanelFocus::None, PanelFocus::Item)))
    }

    fn replacement_root_selection(&self, root: RootId) -> Option<ItemId> {
        let root = self.workspace.root(root)?;
        root.central
            .and_then(|tabs| self.replacement_selected_item(tabs))
            .or_else(|| {
                self.workspace
                    .collect_items_in_subtree(root.node)
                    .into_iter()
                    .find(|item| {
                        self.workspace
                            .locate_item_by_id(*item)
                            .is_some_and(|(_, tabs, _)| {
                                self.replacement_selected_item(tabs) == Some(*item)
                            })
                    })
            })
    }

    fn replacement_selected_item(&self, tabs: crate::ids::NodeId) -> Option<ItemId> {
        match self.workspace.node(tabs)? {
            Node::Tabs { selected, .. } => *selected,
            Node::Split { .. } => None,
        }
    }
}
