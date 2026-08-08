use std::{collections::BTreeSet, fmt};

use thiserror::Error;

use crate::geometry::LogicalRect;
use crate::graph::{
    Axis, ContainedFloating, Node, RootRecord, SplitWeight, SurfacePresentation, Workspace,
    WorkspaceBuilder,
};
use crate::ids::{FloatingPresentationId, ItemId, NodeId, RootId, SurfaceId};

use super::DockspaceView;

/// Product-facing direction of a docking split.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DockspaceAxis {
    /// Children are arranged from left to right.
    Horizontal,
    /// Children are arranged from top to bottom.
    Vertical,
}

impl From<DockspaceAxis> for Axis {
    fn from(axis: DockspaceAxis) -> Self {
        match axis {
            DockspaceAxis::Horizontal => Self::Horizontal,
            DockspaceAxis::Vertical => Self::Vertical,
        }
    }
}

impl From<Axis> for DockspaceAxis {
    fn from(axis: Axis) -> Self {
        match axis {
            Axis::Horizontal => Self::Horizontal,
            Axis::Vertical => Self::Vertical,
        }
    }
}

/// Recursive product declaration of one tabs or split node.
///
/// Runtime node identities are allocated only when the complete [`DockspaceLayout`] is built.
#[derive(Debug, Clone)]
pub struct DockspaceNode {
    kind: DockspaceNodeKind,
    central: bool,
}

#[derive(Debug, Clone)]
enum DockspaceNodeKind {
    Tabs {
        items: Vec<ItemId>,
        selected: Option<ItemId>,
    },
    Split {
        axis: DockspaceAxis,
        children: Vec<DockspaceNode>,
        weights: Vec<SplitWeight>,
    },
}

impl DockspaceNode {
    /// Creates a tabs leaf and selects its first item, if present.
    #[must_use]
    pub fn tabs(items: impl IntoIterator<Item = ItemId>) -> Self {
        Self::tabs_inner(items, false)
    }

    /// Creates the tabs leaf that receives the root's remaining central space.
    ///
    /// Empty tabs are permitted only when marked as the central region.
    #[must_use]
    pub fn central_tabs(items: impl IntoIterator<Item = ItemId>) -> Self {
        Self::tabs_inner(items, true)
    }

    fn tabs_inner(items: impl IntoIterator<Item = ItemId>, central: bool) -> Self {
        let items: Vec<ItemId> = items.into_iter().collect();
        let selected = items.first().copied();
        Self {
            kind: DockspaceNodeKind::Tabs { items, selected },
            central,
        }
    }

    /// Creates a tabs leaf with an explicit selected item.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceLayoutError::InvalidSelection`] when a populated leaf has no selection,
    /// an empty leaf has a selection, or the selected item is absent from the leaf.
    pub fn tabs_with_selection(
        items: impl IntoIterator<Item = ItemId>,
        selected: Option<ItemId>,
    ) -> Result<Self, DockspaceLayoutError> {
        Self::tabs_with_selection_inner(items, selected, false)
    }

    /// Creates a central tabs leaf with an explicit selected item.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceLayoutError::InvalidSelection`] when a populated leaf has no selection,
    /// an empty leaf has a selection, or the selected item is absent from the leaf.
    pub fn central_tabs_with_selection(
        items: impl IntoIterator<Item = ItemId>,
        selected: Option<ItemId>,
    ) -> Result<Self, DockspaceLayoutError> {
        Self::tabs_with_selection_inner(items, selected, true)
    }

    fn tabs_with_selection_inner(
        items: impl IntoIterator<Item = ItemId>,
        selected: Option<ItemId>,
        central: bool,
    ) -> Result<Self, DockspaceLayoutError> {
        let items: Vec<ItemId> = items.into_iter().collect();
        let selection_is_valid = match (items.is_empty(), selected) {
            (true, None) => true,
            (false, Some(selected)) => items.contains(&selected),
            _ => false,
        };
        if !selection_is_valid {
            return Err(DockspaceLayoutError::InvalidSelection { selected });
        }
        Ok(Self {
            kind: DockspaceNodeKind::Tabs { items, selected },
            central,
        })
    }

    /// Creates a weighted N-ary split.
    ///
    /// Each tuple contains a child and its positive relative weight. Weights are normalized before
    /// being stored.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceLayoutError::SplitTooFewChildren`] for fewer than two children and
    /// [`DockspaceLayoutError::InvalidSplitWeights`] when the weights cannot be normalized.
    pub fn split(
        axis: DockspaceAxis,
        children: impl IntoIterator<Item = (Self, f32)>,
    ) -> Result<Self, DockspaceLayoutError> {
        let (children, weights): (Vec<Self>, Vec<f32>) = children.into_iter().unzip();
        if children.len() < 2 {
            return Err(DockspaceLayoutError::SplitTooFewChildren {
                children: children.len(),
            });
        }
        let weights = SplitWeight::normalize(weights)
            .map_err(|_| DockspaceLayoutError::InvalidSplitWeights)?;
        Ok(Self {
            kind: DockspaceNodeKind::Split {
                axis,
                children,
                weights,
            },
            central: false,
        })
    }

    /// Creates an equal-share N-ary split.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceLayoutError::SplitTooFewChildren`] when fewer than two children are
    /// supplied.
    pub fn equal_split(
        axis: DockspaceAxis,
        children: impl IntoIterator<Item = Self>,
    ) -> Result<Self, DockspaceLayoutError> {
        Self::split(axis, children.into_iter().map(|child| (child, 1.0)))
    }
}

/// Declarative content and stable identity of one docking root.
#[derive(Debug, Clone)]
pub struct DockspaceRootLayout {
    id: RootId,
    content: DockspaceNode,
}

impl DockspaceRootLayout {
    /// Creates a root declaration.
    #[must_use]
    pub const fn new(id: RootId, content: DockspaceNode) -> Self {
        Self { id, content }
    }
}

/// Declarative contained-floating root and its durable surface-local bounds.
#[derive(Debug, Clone)]
pub struct DockspaceContainedLayout {
    id: FloatingPresentationId,
    root: DockspaceRootLayout,
    rect: LogicalRect,
}

impl DockspaceContainedLayout {
    /// Creates a contained-floating declaration.
    #[must_use]
    pub const fn new(
        id: FloatingPresentationId,
        root: DockspaceRootLayout,
        rect: LogicalRect,
    ) -> Self {
        Self { id, root, rect }
    }
}

/// Declarative main and contained roots presented by one logical surface.
#[derive(Debug, Clone)]
pub struct DockspaceSurfaceLayout {
    id: SurfaceId,
    main: Option<DockspaceRootLayout>,
    contained: Vec<DockspaceContainedLayout>,
}

impl DockspaceSurfaceLayout {
    /// Creates a surface with a main docking root.
    #[must_use]
    pub const fn new(id: SurfaceId, main: DockspaceRootLayout) -> Self {
        Self {
            id,
            main: Some(main),
            contained: Vec::new(),
        }
    }

    /// Creates a surface whose content consists only of contained-floating roots.
    ///
    /// The complete layout rejects this declaration if no contained root is attached.
    #[must_use]
    pub const fn rootless(id: SurfaceId) -> Self {
        Self {
            id,
            main: None,
            contained: Vec::new(),
        }
    }

    /// Appends a contained-floating root in back-to-front presentation order.
    #[must_use]
    pub fn with_contained(mut self, contained: DockspaceContainedLayout) -> Self {
        self.contained.push(contained);
        self
    }
}

/// A validated product layout backed by the core's canonical N-ary workspace.
#[derive(Clone)]
pub struct DockspaceLayout {
    workspace: Workspace,
}

impl fmt::Debug for DockspaceLayout {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_tuple("DockspaceLayout")
            .field(&self.view())
            .finish()
    }
}

impl DockspaceLayout {
    /// Compiles stable product declarations into one validated docking layout.
    ///
    /// # Errors
    ///
    /// Returns [`DockspaceLayoutError`] for duplicate stable identities, duplicate items, invalid
    /// central-region declarations, unusable contained bounds, or a topology rejected by core
    /// validation.
    pub fn new(
        surfaces: impl IntoIterator<Item = DockspaceSurfaceLayout>,
    ) -> Result<Self, DockspaceLayoutError> {
        let mut compiler = LayoutCompiler::default();
        for surface in surfaces {
            compiler.compile_surface(surface)?;
        }
        let workspace = compiler
            .builder
            .build()
            .map_err(|_| DockspaceLayoutError::InvalidTopology)?;
        Ok(Self { workspace })
    }

    /// Creates the valid empty product layout.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            workspace: Workspace::new(),
        }
    }

    /// Returns a read-only product view without exposing runtime graph identities.
    #[must_use]
    pub fn view(&self) -> DockspaceView<'_> {
        DockspaceView::new(&self.workspace)
    }

    #[allow(
        dead_code,
        reason = "consumed by the product session in the next U2 slice"
    )]
    pub(crate) fn into_workspace(self) -> Workspace {
        self.workspace
    }
}

/// Failure to compile a product layout declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DockspaceLayoutError {
    /// A logical surface identity was declared more than once.
    #[error("surface {surface} is declared more than once")]
    DuplicateSurface {
        /// Repeated stable surface identity.
        surface: SurfaceId,
    },
    /// A root identity was declared more than once.
    #[error("root {root} is declared more than once")]
    DuplicateRoot {
        /// Repeated stable root identity.
        root: RootId,
    },
    /// A contained-floating identity was declared more than once.
    #[error("contained floating {floating} is declared more than once")]
    DuplicateContained {
        /// Repeated stable contained-floating identity.
        floating: FloatingPresentationId,
    },
    /// One item appears in more than one tabs leaf.
    #[error("item {item} is declared more than once")]
    DuplicateItem {
        /// Repeated stable item identity.
        item: ItemId,
    },
    /// A tabs leaf has no valid selected item.
    #[error("tabs selection {selected:?} is not valid for its item roster")]
    InvalidSelection {
        /// Rejected selected item.
        selected: Option<ItemId>,
    },
    /// An N-ary split has fewer than two children.
    #[error("a split requires at least two children, got {children}")]
    SplitTooFewChildren {
        /// Supplied child count.
        children: usize,
    },
    /// Split weights are non-finite, non-positive, or cannot be normalized.
    #[error("split weights must be finite, positive, and normalizable")]
    InvalidSplitWeights,
    /// More than one tabs leaf was marked as the root's central region.
    #[error("root {root} declares more than one central tabs leaf")]
    MultipleCentralRegions {
        /// Root containing the conflicting declarations.
        root: RootId,
    },
    /// A rootless surface has no contained-floating content.
    #[error("rootless surface {surface} must contain at least one floating root")]
    EmptyRootlessSurface {
        /// Empty stable surface identity.
        surface: SurfaceId,
    },
    /// A contained-floating rectangle has no usable area.
    #[error("contained floating {floating} must have positive width and height")]
    NonPositiveContainedRect {
        /// Contained-floating identity with unusable bounds.
        floating: FloatingPresentationId,
    },
    /// Core canonicalization or validation rejected the assembled topology.
    #[error("the declared docking topology is invalid")]
    InvalidTopology,
}

#[derive(Default)]
struct LayoutCompiler {
    builder: WorkspaceBuilder,
    surfaces: BTreeSet<SurfaceId>,
    roots: BTreeSet<RootId>,
    contained: BTreeSet<FloatingPresentationId>,
    items: BTreeSet<ItemId>,
}

impl LayoutCompiler {
    fn compile_surface(
        &mut self,
        surface: DockspaceSurfaceLayout,
    ) -> Result<(), DockspaceLayoutError> {
        if !self.surfaces.insert(surface.id) {
            return Err(DockspaceLayoutError::DuplicateSurface {
                surface: surface.id,
            });
        }
        if surface.main.is_none() && surface.contained.is_empty() {
            return Err(DockspaceLayoutError::EmptyRootlessSurface {
                surface: surface.id,
            });
        }

        let main_root = surface
            .main
            .map(|root| self.compile_root(root))
            .transpose()?;
        let mut contained_ids = Vec::with_capacity(surface.contained.len());
        for contained in surface.contained {
            if !self.contained.insert(contained.id) {
                return Err(DockspaceLayoutError::DuplicateContained {
                    floating: contained.id,
                });
            }
            if contained.rect.width() <= 0.0 || contained.rect.height() <= 0.0 {
                return Err(DockspaceLayoutError::NonPositiveContainedRect {
                    floating: contained.id,
                });
            }
            let root = self.compile_root(contained.root)?;
            self.builder
                .set_contained_floating(contained.id, ContainedFloating::new(root, contained.rect));
            contained_ids.push(contained.id);
        }

        self.builder.set_surface(
            surface.id,
            SurfacePresentation {
                main_root,
                contained: contained_ids,
            },
        );
        Ok(())
    }

    fn compile_root(&mut self, root: DockspaceRootLayout) -> Result<RootId, DockspaceLayoutError> {
        if !self.roots.insert(root.id) {
            return Err(DockspaceLayoutError::DuplicateRoot { root: root.id });
        }
        let mut central = None;
        let node = self.compile_node(root.content, root.id, &mut central)?;
        let record = match central {
            Some(central) => RootRecord::new(node).with_central(central),
            None => RootRecord::new(node),
        };
        self.builder.set_root(root.id, record);
        Ok(root.id)
    }

    fn compile_node(
        &mut self,
        node: DockspaceNode,
        root: RootId,
        central: &mut Option<NodeId>,
    ) -> Result<NodeId, DockspaceLayoutError> {
        let compiled = match node.kind {
            DockspaceNodeKind::Tabs { items, selected } => {
                for item in &items {
                    if !self.items.insert(*item) {
                        return Err(DockspaceLayoutError::DuplicateItem { item: *item });
                    }
                }
                self.builder
                    .insert_node(Node::tabs_with_selection(items, selected))
            }
            DockspaceNodeKind::Split {
                axis,
                children,
                weights,
            } => {
                let children = children
                    .into_iter()
                    .map(|child| self.compile_node(child, root, central))
                    .collect::<Result<Vec<_>, _>>()?;
                self.builder.insert_node(Node::Split {
                    axis: axis.into(),
                    children,
                    weights,
                })
            }
        };

        if node.central {
            if central.replace(compiled).is_some() {
                return Err(DockspaceLayoutError::MultipleCentralRegions { root });
            }
        }
        Ok(compiled)
    }
}
