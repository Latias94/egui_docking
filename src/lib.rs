//! # [egui](https://github.com/emilk/egui) hierarchial tile manager
//! Tiles that can be arranges in horizontal, vertical, and grid-layouts, or put in tabs.
//! The tiles can be resized and re-arranged by drag-and-drop.
//!
//! ## Overview
//! The fundamental unit is the [`Tile`] which is either a [`Container`] or a `Pane` (a leaf).
//! The [`Tile`]s are put into a [`Tree`].
//! Everything is generic over the type of panes, leaving up to the user what to store in the tree.
//!
//! Each [`Tile`] is identified by a (random) [`TileId`].
//! The tiles are stored in [`Tiles`].
//!
//! The entire state is stored in a single [`Tree`] struct which consists of a [`Tiles`] and a root [`TileId`].
//!
//! The behavior and the look of the [`Tree`] is controlled by the [`Behavior`] `trait`.
//! The user needs to implement this in order to specify the `ui` of each `Pane` and
//! the tab name of panes (if there are tab tiles).
//!
//! ## Example
//! See [`Tree`] for how to construct a tree.
//!
//! ```
//! // This specifies how you want to represent your panes in memory.
//! // Implementing serde is optional, but will make the entire tree serializable.
//! #[derive(serde::Serialize, serde::Deserialize)]
//! enum Pane {
//!     Settings,
//!     Text(String),
//! }
//!
//! fn tree_ui(ui: &mut egui::Ui, tree: &mut egui_tiles::Tree<Pane>, settings: &mut Settings) {
//!     let mut behavior = MyBehavior { settings };
//!     tree.ui(&mut behavior, ui);
//! }
//!
//! struct MyBehavior<'a> {
//!     settings: &'a mut Settings
//! }
//!
//! impl<'a> egui_tiles::Behavior<Pane> for MyBehavior<'a> {
//!     fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
//!         match pane {
//!             Pane::Settings => "Settings".into(),
//!             Pane::Text(text) => text.clone().into(),
//!         }
//!     }
//!
//!     fn pane_ui(
//!         &mut self,
//!         ui: &mut egui::Ui,
//!         _tile_id: egui_tiles::TileId,
//!         pane: &mut Pane,
//!     ) -> egui_tiles::UiResponse {
//!         match pane {
//!             Pane::Settings => self.settings.ui(ui),
//!             Pane::Text(text) => {
//!                 ui.text_edit_singleline(text);
//!             },
//!         }
//!
//!         Default::default()
//!     }
//!
//!     // you can override more methods to customize the behavior further
//! }
//!
//! struct Settings {
//!     checked: bool,
//! }
//!
//! impl Settings {
//!     fn ui(&mut self, ui: &mut egui::Ui) {
//!         ui.checkbox(&mut self.checked, "Checked");
//!     }
//! }
//! ```
//!
//! ## Invisible tiles
//! Tiles can be made invisible with [`Tree::set_visible`] and [`Tiles::set_visible`].
//! Invisible tiles still retain their ordering in the container their in until
//! they are made visible again.
//!
//! ## Shares
//! The relative sizes of linear layout (horizontal or vertical) and grid columns and rows are specified by _shares_.
//! If the shares are `1,2,3` it means the first element gets `1/6` of the space, the second `2/6`, and the third `3/6`.
//! The default share size is `1`, and when resizing the shares are restributed so that
//! the total shares are always approximately the same as the number of rows/columns.
//! This makes it easy to add new rows/columns.
//!
//! ## Shortcomings
//! The implementation is recursive, so if your trees get too deep you will get a stack overflow.
//!
//! ## Future improvements
//! * Easy per-tab close-buttons
//! * Scrolling of tab-bar
//! * Vertical tab bar

// ## Implementation notes
// In many places we want to recursively visit all tiles, while also mutating them.
// In order to not get into trouble with the borrow checker a trick is used:
// each [`Tile`] is removed, mutated, recursed, and then re-added.
// You'll see this pattern many times reading the following code.
//
// Each frame consists of two passes: layout, and ui.
// The layout pass figures out where each tile should be placed.
// The ui pass does all the painting.
// These two passes could be combined into one pass if we wanted to,
// but having them split up makes the code slightly simpler, and
// leaves the door open for more complex layout (e.g. min/max sizes per tile).
//
// Everything is quite dynamic, so we have a bunch of defensive coding that call `warn!` on failure.
// These situations should not happen in normal use, but could happen if the user messes with
// the internals of the tree, putting it in an invalid state.

#![forbid(unsafe_code)]

use egui::{Pos2, Rect, pos2};

mod behavior;
mod blueprint;
mod container;
mod floating;
mod tile;
mod tiles;
mod tree;

pub use behavior::{Behavior, DockIndicatorStyle, EditAction, TabState};
pub use blueprint::{
    FullBlueprint, Node as DockBlueprintNode, export_tree, export_tree_full, to_blueprint,
};
pub use container::{
    Container, ContainerFlags, ContainerKind, Grid, GridLayout, Linear, LinearDir, Shares, Tabs,
};
pub use floating::FloatingWindow;
pub use tile::{Tile, TileId};
pub use tiles::Tiles;
pub use tree::Tree;

/// Five-way docking side used for visuals and builder APIs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DockSide {
    Left,
    Right,
    Top,
    Bottom,
    Center,
}

// Actions requested from UI context menus, queued in egui memory and applied in `Tree::tile_ui` after reinserting tiles.
#[derive(Clone, Copy, Debug)]
pub(crate) enum MenuAction {
    /// Close a specific tile (pane or container), removing it (and its subtree) from the tree.
    CloseTile(TileId),
    /// Float a specific tile into a floating window.
    FloatTile(TileId),
    /// Close the current container tile_id.
    CloseContainer,
    /// Float the current container tile_id.
    FloatContainer,
    /// Convert the current container to a different kind.
    ConvertKind(ContainerKind),
    /// If container has a single child, unwrap container, replacing it with the child.
    UnwrapContainer,
    /// Split a specific tile out of its current parent to the given side.
    SplitTileLeft(TileId),
    SplitTileRight(TileId),
    SplitTileTop(TileId),
    SplitTileBottom(TileId),
    /// Reorder a tab within its Tabs container
    MoveTabLeft(TileId),
    MoveTabRight(TileId),
    /// Move tab out of its Tabs into parent linear as a sibling next to this container
    MoveToLeftNeighbor(TileId),
    MoveToRightNeighbor(TileId),
    MoveToTopNeighbor(TileId),
    MoveToBottomNeighbor(TileId),
    /// Merge tab into neighbor tile (wrap neighbor into Tabs if needed), next to current container
    MergeIntoLeftNeighborTabs(TileId),
    MergeIntoRightNeighborTabs(TileId),
    MergeIntoTopNeighborTabs(TileId),
    MergeIntoBottomNeighborTabs(TileId),
}

mod builder;
pub use builder::DockBuilder;

// ----------------------------------------------------------------------------

/// The response from [`Behavior::pane_ui`] for a pane.
#[must_use]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UiResponse {
    #[default]
    None,

    /// The viewer is being dragged via some element in the Pane
    DragStarted,
}

/// What are the rules for simplifying the tree?
///
/// Drag-dropping tiles can often leave containers empty, or with only a single child.
/// The [`SimplificationOptions`] specifies what simplifications are allowed.
///
/// The [`Tree`] will run a simplification pass each frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SimplificationOptions {
    /// Remove empty [`Tabs`] containers?
    pub prune_empty_tabs: bool,

    /// Remove empty containers (that aren't [`Tabs`])?
    pub prune_empty_containers: bool,

    /// Remove [`Tabs`] containers with only a single child?
    ///
    /// Even if `true`, [`Self::all_panes_must_have_tabs`] will be respected.
    pub prune_single_child_tabs: bool,

    /// Prune containers (that aren't [`Tabs`]) with only a single child?
    pub prune_single_child_containers: bool,

    /// If true, each pane will have a [`Tabs`] container as a parent.
    ///
    /// This will win out over [`Self::prune_single_child_tabs`].
    pub all_panes_must_have_tabs: bool,

    /// If a horizontal container contain another horizontal container, join them?
    /// Same for vertical containers. Does NOT apply to grid container or tab containers.
    pub join_nested_linear_containers: bool,
}

impl SimplificationOptions {
    /// [`SimplificationOptions`] with all simplifications turned off.
    ///
    /// This makes it easy to run a single simplification type on a tree:
    /// ```
    /// # use egui_tiles::*;
    /// # let mut tree: Tree<()> = Tree::empty("tree");
    /// tree.simplify(&SimplificationOptions {
    ///     prune_empty_tabs: true,
    ///     ..SimplificationOptions::OFF
    /// });
    ///
    pub const OFF: Self = Self {
        prune_empty_tabs: false,
        prune_empty_containers: false,
        prune_single_child_tabs: false,
        prune_single_child_containers: false,
        all_panes_must_have_tabs: false,
        join_nested_linear_containers: false,
    };
}

impl Default for SimplificationOptions {
    fn default() -> Self {
        Self {
            prune_empty_tabs: true,
            prune_single_child_tabs: true,
            prune_empty_containers: true,
            prune_single_child_containers: true,
            all_panes_must_have_tabs: false,
            join_nested_linear_containers: true,
        }
    }
}

/// The current state of a resize handle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ResizeState {
    Idle,

    /// The user is hovering over the resize handle.
    Hovering,

    /// The user is dragging the resize handle.
    Dragging,
}

// ----------------------------------------------------------------------------

/// An insertion point in a specific container.
///
/// Specifies the expected container layout type, and where to insert.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ContainerInsertion {
    Tabs(usize),
    Horizontal(usize),
    Vertical(usize),
    Grid(usize),
}

impl ContainerInsertion {
    /// Where in the parent (in what order among its children).
    fn index(self) -> usize {
        match self {
            Self::Tabs(index)
            | Self::Horizontal(index)
            | Self::Vertical(index)
            | Self::Grid(index) => index,
        }
    }

    fn kind(self) -> ContainerKind {
        match self {
            Self::Tabs(_) => ContainerKind::Tabs,
            Self::Horizontal(_) => ContainerKind::Horizontal,
            Self::Vertical(_) => ContainerKind::Vertical,
            Self::Grid(_) => ContainerKind::Grid,
        }
    }
}

/// Where in the tree to insert a tile.
#[derive(Clone, Copy, Debug)]
struct InsertionPoint {
    pub parent_id: TileId,

    /// Where in the parent?
    pub insertion: ContainerInsertion,
}

impl InsertionPoint {
    pub fn new(parent_id: TileId, insertion: ContainerInsertion) -> Self {
        Self {
            parent_id,
            insertion,
        }
    }
}

#[derive(PartialEq, Eq)]
enum GcAction {
    Keep,
    Remove,
}

#[must_use]
enum SimplifyAction {
    Remove,
    Keep,
    Replace(TileId),
}

pub(crate) fn is_being_dragged(ctx: &egui::Context, tree_id: egui::Id, tile_id: TileId) -> bool {
    let dragged_id = ctx.dragged_id().or(ctx.drag_stopped_id());
    dragged_id == Some(tile_id.egui_id(tree_id))
}

/// If this tile is currently being dragged, cover it with a semi-transparent overlay ([`Behavior::dragged_overlay_color`]).
fn cover_tile_if_dragged<Pane>(
    tree: &Tree<Pane>,
    behavior: &dyn Behavior<Pane>,
    ui: &egui::Ui,
    tile_id: TileId,
) {
    if is_being_dragged(ui.ctx(), tree.id, tile_id) {
        if let Some(child_rect) = tree.tiles.rect(tile_id) {
            let overlay_color = behavior.dragged_overlay_color(ui.visuals());
            ui.painter().rect_filled(child_rect, 0.0, overlay_color);
        }
    }
}

// ----------------------------------------------------------------------------

/// Context used for drag-and-dropping of tiles.
///
/// This is passed down during the `ui` pass.
/// Each tile registers itself with this context.
struct DropContext {
    enabled: bool,
    dragged_tile_id: Option<TileId>,
    mouse_pos: Option<Pos2>,

    best_insertion: Option<InsertionPoint>,
    best_dist_sq: f32,
    preview_rect: Option<Rect>,

    /// Optional central tile id that should not accept docking.
    central_id: Option<TileId>,
    /// If true, disallow docking into the central tile.
    no_docking_in_central: bool,

    /// Current keyboard modifiers (for requiring a modifier to allow docking).
    modifiers: egui::Modifiers,

    /// Activation threshold squared (pixels^2) before docking is considered.
    activation_sq: f32,

    /// Hysteresis (pixels^2) when changing candidates.
    hysteresis_sq: f32,

    /// Snap distance in pixels used to widen edge bands for docking.
    snap_px: f32,
}

impl DropContext {
    fn on_tile<Pane>(
        &mut self,
        behavior: &dyn Behavior<Pane>,
        style: &egui::Style,
        tiles: &Tiles<Pane>,
        parent_id: TileId,
        rect: Rect,
        tile: &Tile<Pane>,
    ) {
        if !self.enabled {
            return;
        }

        let Some(mouse) = self.mouse_pos else {
            return;
        };

        // Require modifier for docking if requested by behavior
        if let Some(req) = behavior.dock_requires_modifier() {
            if self.modifiers != req {
                return;
            }
        }

        // Determine which directions are allowed for this tile (avoid splitting into same kind).
        let allow_h = tile.kind() != Some(ContainerKind::Horizontal);
        let allow_v = tile.kind() != Some(ContainerKind::Vertical);

        // Container-level flags to constrain docking behavior
        let (no_split, no_tabs, lock_layout) = match tile {
            Tile::Container(container) => {
                let f = container.flags();
                (f.no_split, f.no_tabs, f.lock_layout)
            }
            _ => (false, false, false),
        };

        // Early out: central tile should not accept docking
        if self.no_docking_in_central && Some(parent_id) == self.central_id {
            return;
        }

        // Determine candidate side by position relative to rect center, with a central catch-zone.
        let center = rect.center();
        let snap = self.snap_px;
        let v = mouse - center;
        let frac = behavior.docking_edge_fraction();
        let center_rect = rect.shrink2(egui::vec2(rect.width() * frac, rect.height() * frac));
        let mut candidate_side = if center_rect.contains(mouse) {
            crate::DockSide::Center
        } else if v.x.abs() > v.y.abs() {
            if v.x < 0.0 {
                crate::DockSide::Left
            } else {
                crate::DockSide::Right
            }
        } else {
            if v.y < 0.0 {
                crate::DockSide::Top
            } else {
                crate::DockSide::Bottom
            }
        };

        // If side not allowed, fallback preferences.
        candidate_side = match candidate_side {
            crate::DockSide::Left | crate::DockSide::Right if !allow_h => crate::DockSide::Center,
            crate::DockSide::Top | crate::DockSide::Bottom if !allow_v => crate::DockSide::Center,
            side => side,
        };

        // Container flag filtering: forbid certain actions
        let candidate_side = match candidate_side {
            crate::DockSide::Center if no_tabs || lock_layout => {
                // Center merge would create or use tabs - disabled
                return;
            }
            s @ (crate::DockSide::Left
            | crate::DockSide::Right
            | crate::DockSide::Top
            | crate::DockSide::Bottom)
                if no_split || lock_layout =>
            {
                // Splitting disabled
                return;
            }
            s => s,
        };

        // Behavior-level can_split/can_dock filtering
        let side_ok = match candidate_side {
            crate::DockSide::Center => true,
            s => behavior.can_split(tiles, parent_id, s),
        };
        if !side_ok {
            return;
        }

        // Use a fraction of rect for preview band; center uses content area below tabbar.
        let preview_rect = match candidate_side {
            crate::DockSide::Left if allow_h => {
                let bw = rect.width() * frac + snap;
                let x = (rect.left() + bw).min(rect.right());
                Rect::from_min_max(rect.min, pos2(x, rect.max.y))
            }
            crate::DockSide::Right if allow_h => {
                let bw = rect.width() * frac + snap;
                let x = (rect.right() - bw).max(rect.left());
                Rect::from_min_max(pos2(x, rect.min.y), rect.max)
            }
            crate::DockSide::Top if allow_v => {
                let bh = rect.height() * frac + snap;
                let y = (rect.top() + bh).min(rect.bottom());
                Rect::from_min_max(rect.min, pos2(rect.max.x, y))
            }
            crate::DockSide::Bottom if allow_v => {
                let bh = rect.height() * frac + snap;
                let y = (rect.bottom() - bh).max(rect.top());
                Rect::from_min_max(pos2(rect.min.x, y), rect.max)
            }
            _ => {
                rect.split_top_bottom_at_y(rect.top() + behavior.tab_bar_height(style))
                    .1
            }
        };

        let insertion = match candidate_side {
            crate::DockSide::Left if allow_h => ContainerInsertion::Horizontal(0),
            crate::DockSide::Right if allow_h => ContainerInsertion::Horizontal(usize::MAX),
            crate::DockSide::Top if allow_v => ContainerInsertion::Vertical(0),
            crate::DockSide::Bottom if allow_v => ContainerInsertion::Vertical(usize::MAX),
            _ => ContainerInsertion::Tabs(usize::MAX),
        };

        // Behavior-level can_dock check
        if let Some(src) = self.dragged_tile_id {
            let side = match insertion {
                ContainerInsertion::Horizontal(0) => crate::DockSide::Left,
                ContainerInsertion::Horizontal(_) => crate::DockSide::Right,
                ContainerInsertion::Vertical(0) => crate::DockSide::Top,
                ContainerInsertion::Vertical(_) => crate::DockSide::Bottom,
                _ => crate::DockSide::Center,
            };
            if !behavior.can_dock(tiles, src, parent_id, side) {
                return;
            }
        }

        self.suggest_rect(InsertionPoint::new(parent_id, insertion), preview_rect);
    }

    fn suggest_rect(&mut self, insertion: InsertionPoint, preview_rect: Rect) {
        if !self.enabled {
            return;
        }
        let target_point = preview_rect.center();
        if let Some(mouse_pos) = self.mouse_pos {
            let dist_sq = mouse_pos.distance_sq(target_point);
            if dist_sq + self.hysteresis_sq < self.best_dist_sq {
                self.best_dist_sq = dist_sq;
                self.best_insertion = Some(insertion);
                self.preview_rect = Some(preview_rect);
            }
        }
    }
}
