//! Semantic and geometry assertions for the physical X11 smoke.

use eframe::egui;
use eframe::egui::accesskit::{Role, TreeUpdate};

#[path = "motion_receipt.rs"]
pub(super) mod motion_receipt;

pub(super) const POINTER_DIAGNOSTIC_LABEL: &str = "Physical smoke pointer input";
pub(super) const NATIVE_DRAG_PREVIEW_DIAGNOSTIC_LABEL: &str =
    "Dockspace native drag preview painted";
pub(super) const ROOT_PANE_CONTENT_LABEL: &str = "Root pane content";
pub(super) const DOCK_PREVIEW_EMITTED_LABEL: &str = "Dockspace dock preview emitted";
pub(super) const PHYSICAL_TARGET_ENV: &str = "EGUI_DOCKSPACE_PHYSICAL_TARGET";
const ACTUAL_GUIDE_LABELS: [&str; 5] = [
    "Dockspace center drop guide",
    "Dockspace left drop guide",
    "Dockspace right drop guide",
    "Dockspace top drop guide",
    "Dockspace bottom drop guide",
];
const OVERFLOW_TAB_WIDTH: f64 = 400.0;
const OVERFLOW_TAB_SCROLL_BAND: f64 = 32.0;
const OVERFLOW_TAB_CONTROL_LABELS: [&str; 6] = [
    "Close Anchor",
    "Close Main",
    "Close Child",
    "Scroll tabs backward",
    "Scroll tabs forward",
    "Show hidden tabs",
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum NestedPaneCase {
    Center,
    West,
}

impl NestedPaneCase {
    pub(super) fn parse(value: &str) -> Option<Self> {
        match value {
            "nested-pane-center" => Some(Self::Center),
            "nested-pane-edge" => Some(Self::West),
            _ => None,
        }
    }

    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Center => "nested-pane-center",
            Self::West => "nested-pane-edge",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaneCenterCase {
    A,
    B,
    C,
}

impl PaneCenterCase {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::A => "pane-center-a",
            Self::B => "pane-center-b",
            Self::C => "pane-center-c",
        }
    }

    const fn fractions(self) -> (f64, f64) {
        match self {
            Self::A => (0.35, 0.35),
            Self::B => (0.65, 0.35),
            Self::C => (0.65, 0.65),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum PaneEdgeCase {
    North,
    South,
    East,
    West,
}

impl PaneEdgeCase {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::North => "pane-edge-north",
            Self::South => "pane-edge-south",
            Self::East => "pane-edge-east",
            Self::West => "pane-edge-west",
        }
    }

    const fn fractions(self) -> (f64, f64) {
        match self {
            Self::North => (0.30, 0.16),
            Self::South => (0.30, 0.84),
            Self::East => (0.84, 0.25),
            Self::West => (0.16, 0.25),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum TabHalfCase {
    Before,
    After,
}

impl TabHalfCase {
    pub(super) const fn name(self) -> &'static str {
        match self {
            Self::Before => "tab-half-before",
            Self::After => "tab-half-after",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TabHalfGeometry {
    pub(super) tab: LogicalBounds,
    close: LogicalBounds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct OverflowTabGeometry {
    pub(super) viewport: LogicalBounds,
    pub(super) full: LogicalBounds,
    pub(super) visible: LogicalBounds,
    pub(super) offset: f64,
    pub(super) maximum: f64,
    controls: [Option<LogicalBounds>; OVERFLOW_TAB_CONTROL_LABELS.len()],
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PaneCenterGeometry {
    pub(super) content: LogicalBounds,
    pub(super) workspace: LogicalBounds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct ActualGuideHit {
    pub(super) label: &'static str,
    pub(super) bounds: LogicalBounds,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct DockPreviewEmission {
    pub(super) bounds: LogicalBounds,
}

#[derive(Debug, PartialEq)]
pub(super) enum PaneCenterError {
    Semantic {
        label: &'static str,
        error: SemanticLookupError,
    },
    InteractiveDiagnostic {
        label: &'static str,
    },
    MissingGuide {
        label: &'static str,
    },
    InvalidPreviewValue,
    BoundsTooSmall,
    OutsideContent,
    OutsideCenterBasin,
    OutsideEdgeBasin,
    OutsideTabHalf,
    InvalidOverflowTabGeometry,
    InvalidOverflowTabValue {
        label: &'static str,
    },
    NoSafeOverflowTabPoint,
    ControlCovered,
    InsideRootEdgeBand,
    GuideCovered {
        label: &'static str,
    },
    Coordinates(CoordinateError),
    TargetMoved,
    InvalidNestedLeaf,
    PreviewBoundsMismatch,
    FinalLayoutMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LogicalBounds {
    pub(super) min_x: f64,
    pub(super) min_y: f64,
    pub(super) max_x: f64,
    pub(super) max_y: f64,
}

impl LogicalBounds {
    pub(super) fn center(self) -> LogicalPoint {
        LogicalPoint {
            x: self.min_x + (self.max_x - self.min_x) * 0.5,
            y: self.min_y + (self.max_y - self.min_y) * 0.5,
        }
    }

    pub(super) fn contains(self, point: LogicalPoint) -> bool {
        point.x >= self.min_x
            && point.x <= self.max_x
            && point.y >= self.min_y
            && point.y <= self.max_y
    }
}

pub(super) fn two_stage_ingress_points(bounds: LogicalBounds) -> (LogicalPoint, LogicalPoint) {
    let first = LogicalPoint {
        x: bounds.min_x + (bounds.max_x - bounds.min_x) * 0.4,
        y: bounds.min_y + (bounds.max_y - bounds.min_y) * 0.4,
    };
    let routed = LogicalPoint {
        x: bounds.min_x + (bounds.max_x - bounds.min_x) * 0.6,
        y: bounds.min_y + (bounds.max_y - bounds.min_y) * 0.6,
    };
    (first, routed)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct LogicalPoint {
    pub(super) x: f64,
    pub(super) y: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PhysicalPoint {
    pub(super) x: i32,
    pub(super) y: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct PhysicalBounds {
    pub(super) min_x: i32,
    pub(super) min_y: i32,
    pub(super) max_x: i32,
    pub(super) max_y: i32,
}

impl PhysicalBounds {
    pub(super) fn width(self) -> i32 {
        self.max_x - self.min_x
    }

    pub(super) fn height(self) -> i32 {
        self.max_y - self.min_y
    }

    pub(super) fn center(self) -> PhysicalPoint {
        PhysicalPoint {
            x: self.min_x + self.width() / 2,
            y: self.min_y + self.height() / 2,
        }
    }

    pub(super) fn contains(self, point: PhysicalPoint) -> bool {
        point.x >= self.min_x
            && point.x < self.max_x
            && point.y >= self.min_y
            && point.y < self.max_y
    }

    pub(super) fn expanded(self, amount: i32) -> Self {
        Self {
            min_x: self.min_x.saturating_sub(amount),
            min_y: self.min_y.saturating_sub(amount),
            max_x: self.max_x.saturating_add(amount),
            max_y: self.max_y.saturating_add(amount),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum CoordinateError {
    InvalidScale,
    NonFinite,
    OutOfRange,
}

pub(super) fn viewport_logical_to_desktop(
    client: PhysicalBounds,
    pixels_per_point: f32,
    logical: LogicalPoint,
) -> Result<PhysicalPoint, CoordinateError> {
    let scale = f64::from(pixels_per_point);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(CoordinateError::InvalidScale);
    }
    if !logical.x.is_finite() || !logical.y.is_finite() {
        return Err(CoordinateError::NonFinite);
    }
    let x = f64::from(client.min_x) + logical.x * scale;
    let y = f64::from(client.min_y) + logical.y * scale;
    if !x.is_finite()
        || !y.is_finite()
        || x < f64::from(i32::MIN)
        || x > f64::from(i32::MAX)
        || y < f64::from(i32::MIN)
        || y > f64::from(i32::MAX)
    {
        return Err(CoordinateError::OutOfRange);
    }
    // The finite range checks above make the rounded conversion exact for i32 bounds.
    #[allow(clippy::cast_possible_truncation)]
    Ok(PhysicalPoint {
        x: x.round() as i32,
        y: y.round() as i32,
    })
}

pub(super) fn desktop_to_viewport_logical(
    client: PhysicalBounds,
    pixels_per_point: f32,
    physical: PhysicalPoint,
) -> Result<LogicalPoint, CoordinateError> {
    let scale = f64::from(pixels_per_point);
    if !scale.is_finite() || scale <= 0.0 {
        return Err(CoordinateError::InvalidScale);
    }
    Ok(LogicalPoint {
        x: (f64::from(physical.x) - f64::from(client.min_x)) / scale,
        y: (f64::from(physical.y) - f64::from(client.min_y)) / scale,
    })
}

pub(super) fn choose_outside_points(
    work_area: PhysicalBounds,
    occupied: impl IntoIterator<Item = PhysicalBounds>,
) -> Option<(PhysicalPoint, PhysicalPoint)> {
    const FRACTIONS: [i32; 5] = [1, 3, 5, 7, 9];

    if work_area.width() <= 0 || work_area.height() <= 0 {
        return None;
    }
    let occupied = occupied.into_iter().collect::<Vec<_>>();
    let margin = (work_area.width().min(work_area.height()) / 20).clamp(24, 96);
    let expanded = occupied
        .iter()
        .copied()
        .map(|bounds| bounds.expanded(margin))
        .collect::<Vec<_>>();
    let candidates = FRACTIONS
        .into_iter()
        .flat_map(|x| FRACTIONS.into_iter().map(move |y| (x, y)))
        .map(|(x, y)| PhysicalPoint {
            x: work_area.min_x + work_area.width() * x / 10,
            y: work_area.min_y + work_area.height() * y / 10,
        })
        .filter(|point| {
            work_area.contains(*point) && expanded.iter().all(|bounds| !bounds.contains(*point))
        })
        .collect::<Vec<_>>();
    let distance_from_nearest_window = |point: &PhysicalPoint| {
        occupied
            .iter()
            .map(|bounds| {
                let center = bounds.center();
                let dx = i64::from(point.x) - i64::from(center.x);
                let dy = i64::from(point.y) - i64::from(center.y);
                dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
            })
            .min()
            .unwrap_or(i64::MAX)
    };
    let first = candidates
        .iter()
        .copied()
        .max_by_key(distance_from_nearest_window)?;
    let routed = candidates
        .into_iter()
        .filter(|point| *point != first)
        .min_by_key(|point| {
            let dx = i64::from(point.x) - i64::from(first.x);
            let dy = i64::from(point.y) - i64::from(first.y);
            dx.saturating_mul(dx).saturating_add(dy.saturating_mul(dy))
        })?;
    Some((first, routed))
}

pub(super) fn point_towards(
    from: PhysicalPoint,
    to: PhysicalPoint,
    distance: f64,
) -> PhysicalPoint {
    let dx = f64::from(to.x - from.x);
    let dy = f64::from(to.y - from.y);
    let length = dx.hypot(dy);
    if !distance.is_finite() || distance <= 0.0 || length == 0.0 {
        return from;
    }
    let scale = (distance / length).min(1.0);
    // Convex interpolation between two i32 endpoints remains inside the i32 range.
    #[allow(clippy::cast_possible_truncation)]
    PhysicalPoint {
        x: (f64::from(from.x) + dx * scale).round() as i32,
        y: (f64::from(from.y) + dy * scale).round() as i32,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum SemanticText<'a> {
    Label(&'a str),
    Value(&'a str),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct SemanticSelector<'a> {
    pub(super) role: Role,
    pub(super) text: SemanticText<'a>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum SemanticLookupError {
    Missing,
    Ambiguous { matches: usize },
    MissingBounds,
    InvalidBounds,
}

pub(super) fn semantic_node<'a>(
    tree: &'a TreeUpdate,
    selector: SemanticSelector<'_>,
) -> Result<&'a eframe::egui::accesskit::Node, SemanticLookupError> {
    let mut matches = tree.nodes.iter().filter(|(_, node)| {
        !node.is_hidden()
            && node.role() == selector.role
            && match selector.text {
                SemanticText::Label(expected) => node.label() == Some(expected),
                SemanticText::Value(expected) => node.value() == Some(expected),
            }
    });
    let Some((_, node)) = matches.next() else {
        return Err(SemanticLookupError::Missing);
    };
    let remaining = matches.count();
    if remaining != 0 {
        return Err(SemanticLookupError::Ambiguous {
            matches: remaining + 1,
        });
    }
    Ok(node)
}

pub(super) fn semantic_bounds(
    tree: &TreeUpdate,
    selector: SemanticSelector<'_>,
) -> Result<LogicalBounds, SemanticLookupError> {
    let node = semantic_node(tree, selector)?;
    checked_node_bounds(node)
}

fn checked_node_bounds(
    node: &eframe::egui::accesskit::Node,
) -> Result<LogicalBounds, SemanticLookupError> {
    let bounds = node.bounds().ok_or(SemanticLookupError::MissingBounds)?;
    let bounds = LogicalBounds {
        min_x: bounds.x0,
        min_y: bounds.y0,
        max_x: bounds.x1,
        max_y: bounds.y1,
    };
    if ![bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y]
        .into_iter()
        .all(f64::is_finite)
        || bounds.max_x <= bounds.min_x
        || bounds.max_y <= bounds.min_y
    {
        return Err(SemanticLookupError::InvalidBounds);
    }
    Ok(bounds)
}

pub(super) fn pane_center_geometry(
    tree: &TreeUpdate,
) -> Result<PaneCenterGeometry, PaneCenterError> {
    let selector = SemanticSelector {
        role: Role::Label,
        text: SemanticText::Value(ROOT_PANE_CONTENT_LABEL),
    };
    let node = semantic_node(tree, selector).map_err(|error| PaneCenterError::Semantic {
        label: ROOT_PANE_CONTENT_LABEL,
        error,
    })?;
    require_passive(node, ROOT_PANE_CONTENT_LABEL)?;
    let content = checked_node_bounds(node).map_err(|error| PaneCenterError::Semantic {
        label: ROOT_PANE_CONTENT_LABEL,
        error,
    })?;
    let workspace = semantic_bounds(
        tree,
        SemanticSelector {
            role: Role::Pane,
            text: SemanticText::Label("Dockspace workspace"),
        },
    )
    .map_err(|error| PaneCenterError::Semantic {
        label: "Dockspace workspace",
        error,
    })?;
    Ok(PaneCenterGeometry { content, workspace })
}

pub(super) fn tab_half_geometry(tree: &TreeUpdate) -> Result<TabHalfGeometry, PaneCenterError> {
    let read = |role, label| {
        semantic_bounds(
            tree,
            SemanticSelector {
                role,
                text: SemanticText::Label(label),
            },
        )
        .map_err(|error| PaneCenterError::Semantic { label, error })
    };
    Ok(TabHalfGeometry {
        tab: {
            let node = semantic_node(
                tree,
                SemanticSelector {
                    role: Role::Label,
                    text: SemanticText::Label("Dockspace tab bounds Main"),
                },
            )
            .map_err(|error| PaneCenterError::Semantic {
                label: "Dockspace tab bounds Main",
                error,
            })?;
            require_passive(node, "Dockspace tab bounds Main")?;
            checked_node_bounds(node).map_err(|error| PaneCenterError::Semantic {
                label: "Dockspace tab bounds Main",
                error,
            })?
        },
        close: read(Role::Button, "Close Main")?,
    })
}

pub(super) fn overflow_tab_geometry(
    tree: &TreeUpdate,
) -> Result<OverflowTabGeometry, PaneCenterError> {
    overflow_tab_geometry_for_selection(tree, true)
}

pub(super) fn stationary_overflow_geometry(
    tree: &TreeUpdate,
) -> Result<OverflowTabGeometry, PaneCenterError> {
    overflow_tab_geometry_for_selection(tree, false)
}

fn overflow_tab_geometry_for_selection(
    tree: &TreeUpdate,
    expected_selected: bool,
) -> Result<OverflowTabGeometry, PaneCenterError> {
    let read = |label: &'static str| -> Result<_, PaneCenterError> {
        let node = semantic_node(
            tree,
            SemanticSelector {
                role: Role::Label,
                text: SemanticText::Label(label),
            },
        )
        .map_err(|error| PaneCenterError::Semantic { label, error })?;
        require_passive(node, label)?;
        let bounds = checked_node_bounds(node)
            .map_err(|error| PaneCenterError::Semantic { label, error })?;
        Ok((node, bounds))
    };
    let (full_node, full) = read("Dockspace tab full bounds Main")?;
    if full_node.value() != Some("partially-visible") {
        return Err(PaneCenterError::InvalidOverflowTabValue {
            label: "Dockspace tab full bounds Main",
        });
    }
    let (viewport_node, viewport) = read("Dockspace tab strip viewport Main")?;
    let (offset, maximum) = viewport_node
        .value()
        .and_then(|value| value.strip_prefix("offset="))
        .and_then(|value| value.split_once(";maximum="))
        .and_then(|(offset, maximum)| {
            Some((offset.parse::<f64>().ok()?, maximum.parse::<f64>().ok()?))
        })
        .filter(|(offset, maximum)| {
            offset.is_finite()
                && maximum.is_finite()
                && *offset >= 0.0
                && *maximum > 0.0
                && offset <= maximum
        })
        .ok_or(PaneCenterError::InvalidOverflowTabValue {
            label: "Dockspace tab strip viewport Main",
        })?;
    let (_, visible) = read("Dockspace tab bounds Main")?;
    let main = semantic_node(
        tree,
        SemanticSelector {
            role: Role::Tab,
            text: SemanticText::Label("Main"),
        },
    )
    .map_err(|error| PaneCenterError::Semantic {
        label: "Main",
        error,
    })?;
    let intersection = LogicalBounds {
        min_x: full.min_x.max(viewport.min_x),
        min_y: full.min_y.max(viewport.min_y),
        max_x: full.max_x.min(viewport.max_x),
        max_y: full.max_y.min(viewport.max_y),
    };
    let viewport_width = viewport.max_x - viewport.min_x;
    let full_width = full.max_x - full.min_x;
    let visible_width = visible.max_x - visible.min_x;
    // The product renderer clears this optional field for an unselected Tab.
    // The stationary journey also checks the exact public model selection.
    if main.is_selected().unwrap_or(false) != expected_selected
        || viewport_width <= OVERFLOW_TAB_WIDTH
        || viewport_width >= 2.0 * OVERFLOW_TAB_WIDTH
        || (full_width - OVERFLOW_TAB_WIDTH).abs() > 1.0
        || visible_width >= full_width
        || visible != intersection
        || full.max_x <= viewport.max_x
    {
        return Err(PaneCenterError::InvalidOverflowTabGeometry);
    }
    let mut controls = [None; OVERFLOW_TAB_CONTROL_LABELS.len()];
    for (control, label) in controls.iter_mut().zip(OVERFLOW_TAB_CONTROL_LABELS) {
        *control = match semantic_bounds(
            tree,
            SemanticSelector {
                role: Role::Button,
                text: SemanticText::Label(label),
            },
        ) {
            Ok(bounds) => Some(bounds),
            Err(SemanticLookupError::Missing) => None,
            Err(error) => return Err(PaneCenterError::Semantic { label, error }),
        };
    }
    Ok(OverflowTabGeometry {
        viewport,
        full,
        visible,
        offset,
        maximum,
        controls,
    })
}

pub(super) fn nested_pane_bounds(
    geometry: PaneCenterGeometry,
    tab: TabHalfGeometry,
    pixels_per_point: f32,
) -> Result<LogicalBounds, PaneCenterError> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return Err(PaneCenterError::Coordinates(CoordinateError::InvalidScale));
    }
    // Main is the bottom-right leaf of this fixed two-axis fixture. Its painted tab supplies
    // the pane's top edge; its painted content supplies the remaining three edges.
    let content = geometry.content;
    let workspace = geometry.workspace;
    let tolerance = 1.5 / f64::from(pixels_per_point);
    if content.min_x <= workspace.min_x + tolerance
        || tab.tab.min_y <= workspace.min_y + tolerance
        || content.max_x > workspace.max_x + tolerance
        || content.max_y > workspace.max_y + tolerance
        || tab.tab.min_x < content.min_x - tolerance
        || tab.tab.max_x > content.max_x + tolerance
        || tab.tab.min_y >= content.min_y - tolerance
        || (tab.tab.max_y - content.min_y).abs() > tolerance
    {
        return Err(PaneCenterError::InvalidNestedLeaf);
    }
    Ok(LogicalBounds {
        min_y: tab.tab.min_y,
        ..content
    })
}

pub(super) fn nested_pane_point(
    case: NestedPaneCase,
    geometry: PaneCenterGeometry,
    tab: TabHalfGeometry,
    client: PhysicalBounds,
    pixels_per_point: f32,
    guides: &[ActualGuideHit],
) -> Result<(PhysicalPoint, LogicalPoint), PaneCenterError> {
    nested_pane_bounds(geometry, tab, pixels_per_point)?;
    match case {
        NestedPaneCase::Center => pane_center_point(
            PaneCenterCase::A,
            geometry,
            client,
            pixels_per_point,
            guides,
        ),
        NestedPaneCase::West => pane_edge_point(
            PaneEdgeCase::West,
            geometry,
            client,
            pixels_per_point,
            guides,
        ),
    }
}

pub(super) fn verify_nested_pane_preview(
    case: NestedPaneCase,
    geometry: PaneCenterGeometry,
    tab: TabHalfGeometry,
    preview: DockPreviewEmission,
    pixels_per_point: f32,
) -> Result<(), PaneCenterError> {
    let leaf = nested_pane_bounds(geometry, tab, pixels_per_point)?;
    match case {
        NestedPaneCase::Center => verify_center_preview(geometry, preview, pixels_per_point),
        NestedPaneCase::West => verify_preview_bounds(
            edge_preview_bounds(PaneEdgeCase::West, leaf),
            preview,
            pixels_per_point,
        ),
    }
}

pub(super) fn tab_half_point(
    case: TabHalfCase,
    geometry: TabHalfGeometry,
    client: PhysicalBounds,
    pixels_per_point: f32,
    guides: &[ActualGuideHit],
) -> Result<(PhysicalPoint, LogicalPoint), PaneCenterError> {
    let tab = geometry.tab;
    let candidate = LogicalPoint {
        x: match case {
            TabHalfCase::Before => (tab.min_x + tab.center().x) * 0.5,
            TabHalfCase::After => (tab.center().x + geometry.close.min_x.min(tab.max_x)) * 0.5,
        },
        y: tab.center().y,
    };
    let physical = viewport_logical_to_desktop(client, pixels_per_point, candidate)
        .map_err(PaneCenterError::Coordinates)?;
    let point = desktop_to_viewport_logical(client, pixels_per_point, physical)
        .map_err(PaneCenterError::Coordinates)?;
    let margin = 2.0 / f64::from(pixels_per_point);
    let inside_half = match case {
        TabHalfCase::Before => point.x < tab.center().x - margin,
        TabHalfCase::After => point.x > tab.center().x + margin,
    };
    if !client.contains(physical) || !strictly_inside(tab, point, margin) || !inside_half {
        return Err(PaneCenterError::OutsideTabHalf);
    }
    let overlaps = |bounds: LogicalBounds| {
        point.x >= bounds.min_x - margin
            && point.x <= bounds.max_x + margin
            && point.y >= bounds.min_y - margin
            && point.y <= bounds.max_y + margin
    };
    if overlaps(geometry.close) {
        return Err(PaneCenterError::ControlCovered);
    }
    for guide in guides {
        if overlaps(guide.bounds) {
            return Err(PaneCenterError::GuideCovered { label: guide.label });
        }
    }
    Ok((physical, point))
}

pub(super) fn verify_tab_half_preview(
    case: TabHalfCase,
    geometry: TabHalfGeometry,
    preview: DockPreviewEmission,
    pixels_per_point: f32,
) -> Result<(), PaneCenterError> {
    // The finite target has one fully visible Main tab and no overflow chrome.
    let width = dockspace::runtime::DockPresentationConfig::default().splitter_thickness();
    let mut expected = geometry.tab;
    expected.min_x = match case {
        TabHalfCase::Before => expected.min_x,
        TabHalfCase::After => expected.max_x - width * 0.5,
    };
    expected.max_x = expected.min_x + width;
    verify_preview_bounds(expected, preview, pixels_per_point)
}

pub(super) fn overflow_tab_point(
    geometry: OverflowTabGeometry,
    client: PhysicalBounds,
    pixels_per_point: f32,
    guides: &[ActualGuideHit],
) -> Result<(PhysicalPoint, egui::Pos2), PaneCenterError> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return Err(PaneCenterError::Coordinates(CoordinateError::InvalidScale));
    }
    if (geometry.full.max_x - geometry.full.min_x - OVERFLOW_TAB_WIDTH).abs()
        > 1.0 / f64::from(pixels_per_point)
    {
        return Err(PaneCenterError::InvalidOverflowTabGeometry);
    }
    let safe = LogicalBounds {
        min_x: geometry
            .visible
            .min_x
            .max(geometry.full.min_x)
            .max(geometry.viewport.min_x + OVERFLOW_TAB_SCROLL_BAND),
        min_y: geometry
            .visible
            .min_y
            .max(geometry.full.min_y)
            .max(geometry.viewport.min_y),
        max_x: geometry
            .visible
            .max_x
            .min(geometry.full.center().x)
            .min(geometry.viewport.max_x - OVERFLOW_TAB_SCROLL_BAND),
        max_y: geometry
            .visible
            .max_y
            .min(geometry.full.max_y)
            .min(geometry.viewport.max_y),
    };
    if safe.max_x <= safe.min_x || safe.max_y <= safe.min_y {
        return Err(PaneCenterError::NoSafeOverflowTabPoint);
    }
    let physical = viewport_logical_to_desktop(client, pixels_per_point, safe.center())
        .map_err(PaneCenterError::Coordinates)?;
    let point = desktop_to_viewport_logical(client, pixels_per_point, physical)
        .map_err(PaneCenterError::Coordinates)?;
    let margin = 2.0 / f64::from(pixels_per_point);
    if !client.contains(physical) || !strictly_inside(safe, point, margin) {
        return Err(PaneCenterError::NoSafeOverflowTabPoint);
    }
    let overlaps = |bounds: LogicalBounds| {
        point.x >= bounds.min_x - margin
            && point.x <= bounds.max_x + margin
            && point.y >= bounds.min_y - margin
            && point.y <= bounds.max_y + margin
    };
    if geometry.controls.into_iter().flatten().any(overlaps) {
        return Err(PaneCenterError::ControlCovered);
    }
    for guide in guides {
        if overlaps(guide.bounds) {
            return Err(PaneCenterError::GuideCovered { label: guide.label });
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    let logical = egui::pos2(point.x as f32, point.y as f32);
    Ok((physical, logical))
}

pub(super) fn stationary_overflow_edge_point(
    geometry: OverflowTabGeometry,
    client: PhysicalBounds,
    pixels_per_point: f32,
    band: f64,
    guides: &[ActualGuideHit],
) -> Result<(PhysicalPoint, egui::Pos2), PaneCenterError> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return Err(PaneCenterError::Coordinates(CoordinateError::InvalidScale));
    }
    if (geometry.full.max_x - geometry.full.min_x - OVERFLOW_TAB_WIDTH).abs()
        > 1.0 / f64::from(pixels_per_point)
    {
        return Err(PaneCenterError::InvalidOverflowTabGeometry);
    }
    if !band.is_finite() || band <= 0.0 {
        return Err(PaneCenterError::NoSafeOverflowTabPoint);
    }
    let mut viewport = geometry.viewport;
    // Match edge_content_viewport using the renderer's leading/trailing placements.
    // Reserved controls lie outside the viewport; Close never trims its content.
    for (label, control) in OVERFLOW_TAB_CONTROL_LABELS
        .into_iter()
        .zip(geometry.controls)
    {
        let Some(control) = control else {
            continue;
        };
        if control.max_x <= viewport.min_x
            || control.min_x >= viewport.max_x
            || control.max_y <= viewport.min_y
            || control.min_y >= viewport.max_y
        {
            continue;
        }
        match label {
            "Scroll tabs backward" => viewport.min_x = viewport.min_x.max(control.max_x),
            "Scroll tabs forward" | "Show hidden tabs" => {
                viewport.max_x = viewport.max_x.min(control.min_x);
            }
            _ => {}
        }
    }
    if viewport.max_x <= viewport.min_x {
        return Err(PaneCenterError::NoSafeOverflowTabPoint);
    }
    let band = band.min((viewport.max_x - viewport.min_x) * 0.5);
    let edge_band = LogicalBounds {
        min_x: viewport.max_x - band,
        ..viewport
    };
    let physical = viewport_logical_to_desktop(client, pixels_per_point, edge_band.center())
        .map_err(PaneCenterError::Coordinates)?;
    let point = desktop_to_viewport_logical(client, pixels_per_point, physical)
        .map_err(PaneCenterError::Coordinates)?;
    let margin = 2.0 / f64::from(pixels_per_point);
    // This single band midpoint must remain in Main's visible body after pixel rounding.
    if !client.contains(physical)
        || !strictly_inside(edge_band, point, margin)
        || !strictly_inside(geometry.visible, point, margin)
    {
        return Err(PaneCenterError::NoSafeOverflowTabPoint);
    }
    let overlaps = |bounds: LogicalBounds| {
        point.x >= bounds.min_x - margin
            && point.x <= bounds.max_x + margin
            && point.y >= bounds.min_y - margin
            && point.y <= bounds.max_y + margin
    };
    if geometry.controls.into_iter().flatten().any(overlaps) {
        return Err(PaneCenterError::ControlCovered);
    }
    for guide in guides {
        if overlaps(guide.bounds) {
            return Err(PaneCenterError::GuideCovered { label: guide.label });
        }
    }
    #[allow(clippy::cast_possible_truncation)]
    let logical = egui::pos2(point.x as f32, point.y as f32);
    Ok((physical, logical))
}

pub(super) fn verify_overflow_tab_preview(
    geometry: OverflowTabGeometry,
    preview: DockPreviewEmission,
    pixels_per_point: f32,
) -> Result<(), PaneCenterError> {
    if !pixels_per_point.is_finite() || pixels_per_point <= 0.0 {
        return Err(PaneCenterError::Coordinates(CoordinateError::InvalidScale));
    }
    // The fixed [Anchor, Child, Main] layout reveals the newly selected 400-point Child.
    let thickness = dockspace::runtime::DockPresentationConfig::default().splitter_thickness();
    let mut expected = geometry.viewport;
    expected.min_x = (expected.max_x - OVERFLOW_TAB_WIDTH - thickness * 0.5)
        .clamp(expected.min_x, expected.max_x - thickness);
    expected.max_x = expected.min_x + thickness;
    verify_preview_bounds(expected, preview, pixels_per_point)
}

fn require_passive(
    node: &eframe::egui::accesskit::Node,
    label: &'static str,
) -> Result<(), PaneCenterError> {
    if node.supports_action(eframe::egui::accesskit::Action::Click)
        || node.supports_action(eframe::egui::accesskit::Action::Focus)
    {
        return Err(PaneCenterError::InteractiveDiagnostic { label });
    }
    Ok(())
}

pub(super) fn actual_guide_hits(tree: &TreeUpdate) -> Result<Vec<ActualGuideHit>, PaneCenterError> {
    let mut hits = Vec::new();
    for (_, node) in &tree.nodes {
        if node.is_hidden() || node.role() != Role::Label {
            continue;
        }
        let Some(label) = ACTUAL_GUIDE_LABELS
            .into_iter()
            .find(|label| node.label() == Some(*label))
        else {
            continue;
        };
        require_passive(node, label)?;
        let bounds = checked_node_bounds(node)
            .map_err(|error| PaneCenterError::Semantic { label, error })?;
        hits.push(ActualGuideHit { label, bounds });
    }
    // Inner and outer clusters can expose the same direction. Keep every actual hit instead
    // of selecting one matching label and accidentally overlooking an overlapping receiver.
    for label in ACTUAL_GUIDE_LABELS {
        if !hits.iter().any(|hit| hit.label == label) {
            return Err(PaneCenterError::MissingGuide { label });
        }
    }
    hits.sort_by(|left, right| {
        left.label
            .cmp(right.label)
            .then_with(|| left.bounds.min_x.total_cmp(&right.bounds.min_x))
            .then_with(|| left.bounds.min_y.total_cmp(&right.bounds.min_y))
            .then_with(|| left.bounds.max_x.total_cmp(&right.bounds.max_x))
            .then_with(|| left.bounds.max_y.total_cmp(&right.bounds.max_y))
    });
    Ok(hits)
}

pub(super) fn dock_preview_emission(
    tree: &TreeUpdate,
) -> Result<Option<DockPreviewEmission>, PaneCenterError> {
    let selector = SemanticSelector {
        role: Role::Label,
        text: SemanticText::Label(DOCK_PREVIEW_EMITTED_LABEL),
    };
    let node = match semantic_node(tree, selector) {
        Ok(node) => node,
        Err(SemanticLookupError::Missing) => return Ok(None),
        Err(error) => {
            return Err(PaneCenterError::Semantic {
                label: DOCK_PREVIEW_EMITTED_LABEL,
                error,
            });
        }
    };
    require_passive(node, DOCK_PREVIEW_EMITTED_LABEL)?;
    if node.value() != Some("painted") {
        return Err(PaneCenterError::InvalidPreviewValue);
    }
    let bounds = checked_node_bounds(node).map_err(|error| PaneCenterError::Semantic {
        label: DOCK_PREVIEW_EMITTED_LABEL,
        error,
    })?;
    Ok(Some(DockPreviewEmission { bounds }))
}

pub(super) fn pane_center_point(
    case: PaneCenterCase,
    geometry: PaneCenterGeometry,
    client: PhysicalBounds,
    pixels_per_point: f32,
    guides: &[ActualGuideHit],
) -> Result<(PhysicalPoint, LogicalPoint), PaneCenterError> {
    let content = geometry.content;
    let config = dockspace::runtime::DockPresentationConfig::default();
    let width = content.max_x - content.min_x;
    let height = content.max_y - content.min_y;
    if width < config.minimum_pane_size().width() || height < config.minimum_pane_size().height() {
        return Err(PaneCenterError::BoundsTooSmall);
    }
    let (x, y) = case.fractions();
    let candidate = LogicalPoint {
        x: content.min_x + width * x,
        y: content.min_y + height * y,
    };
    let physical = viewport_logical_to_desktop(client, pixels_per_point, candidate)
        .map_err(PaneCenterError::Coordinates)?;
    let point = desktop_to_viewport_logical(client, pixels_per_point, physical)
        .map_err(PaneCenterError::Coordinates)?;
    let margin = 2.0 / f64::from(pixels_per_point);
    let inset = (1.0 - config.magnetic_center_fraction()) * 0.5;
    let center = LogicalBounds {
        min_x: content.min_x + width * inset,
        min_y: content.min_y + height * inset,
        max_x: content.max_x - width * inset,
        max_y: content.max_y - height * inset,
    };
    if !client.contains(physical) || !strictly_inside(content, point, margin) {
        return Err(PaneCenterError::OutsideContent);
    }
    if !strictly_inside(center, point, margin) {
        return Err(PaneCenterError::OutsideCenterBasin);
    }
    let workspace = geometry.workspace;
    let band = ((workspace.max_x - workspace.min_x).min(workspace.max_y - workspace.min_y)
        * config.magnetic_outer_band_fraction())
    .clamp(
        config.magnetic_outer_band_minimum(),
        config.magnetic_outer_band_maximum(),
    );
    if !strictly_inside(workspace, point, band + margin) {
        return Err(PaneCenterError::InsideRootEdgeBand);
    }
    for guide in guides {
        let bounds = guide.bounds;
        if point.x >= bounds.min_x - margin
            && point.x <= bounds.max_x + margin
            && point.y >= bounds.min_y - margin
            && point.y <= bounds.max_y + margin
        {
            return Err(PaneCenterError::GuideCovered { label: guide.label });
        }
    }
    Ok((physical, point))
}

pub(super) fn pane_edge_point(
    case: PaneEdgeCase,
    geometry: PaneCenterGeometry,
    client: PhysicalBounds,
    pixels_per_point: f32,
    guides: &[ActualGuideHit],
) -> Result<(PhysicalPoint, LogicalPoint), PaneCenterError> {
    let content = geometry.content;
    let config = dockspace::runtime::DockPresentationConfig::default();
    let width = content.max_x - content.min_x;
    let height = content.max_y - content.min_y;
    if width < config.minimum_pane_size().width() || height < config.minimum_pane_size().height() {
        return Err(PaneCenterError::BoundsTooSmall);
    }
    let (x, y) = case.fractions();
    let candidate = LogicalPoint {
        x: content.min_x + width * x,
        y: content.min_y + height * y,
    };
    let physical = viewport_logical_to_desktop(client, pixels_per_point, candidate)
        .map_err(PaneCenterError::Coordinates)?;
    let point = desktop_to_viewport_logical(client, pixels_per_point, physical)
        .map_err(PaneCenterError::Coordinates)?;
    let margin = 2.0 / f64::from(pixels_per_point);
    if !client.contains(physical) || !strictly_inside(content, point, margin) {
        return Err(PaneCenterError::OutsideContent);
    }
    let x = (point.x - content.min_x) / width;
    let y = (point.y - content.min_y) / height;
    let inset = (1.0 - config.magnetic_center_fraction()) * 0.5;
    let in_basin = match case {
        PaneEdgeCase::North => y < inset - margin / height && y < x.min(1.0 - x),
        PaneEdgeCase::South => 1.0 - y < inset - margin / height && 1.0 - y < x.min(1.0 - x),
        PaneEdgeCase::East => 1.0 - x < inset - margin / width && 1.0 - x < y.min(1.0 - y),
        PaneEdgeCase::West => x < inset - margin / width && x < y.min(1.0 - y),
    };
    if !in_basin {
        return Err(PaneCenterError::OutsideEdgeBasin);
    }
    let workspace = geometry.workspace;
    let band = ((workspace.max_x - workspace.min_x).min(workspace.max_y - workspace.min_y)
        * config.magnetic_outer_band_fraction())
    .clamp(
        config.magnetic_outer_band_minimum(),
        config.magnetic_outer_band_maximum(),
    );
    if !strictly_inside(workspace, point, band + margin) {
        return Err(PaneCenterError::InsideRootEdgeBand);
    }
    for guide in guides {
        let bounds = guide.bounds;
        if point.x >= bounds.min_x - margin
            && point.x <= bounds.max_x + margin
            && point.y >= bounds.min_y - margin
            && point.y <= bounds.max_y + margin
        {
            return Err(PaneCenterError::GuideCovered { label: guide.label });
        }
    }
    Ok((physical, point))
}

fn strictly_inside(bounds: LogicalBounds, point: LogicalPoint, margin: f64) -> bool {
    point.x > bounds.min_x + margin
        && point.x < bounds.max_x - margin
        && point.y > bounds.min_y + margin
        && point.y < bounds.max_y - margin
}

pub(super) fn verify_center_preview(
    geometry: PaneCenterGeometry,
    preview: DockPreviewEmission,
    pixels_per_point: f32,
) -> Result<(), PaneCenterError> {
    verify_preview_bounds(geometry.content, preview, pixels_per_point)
}

pub(super) fn verify_edge_preview(
    case: PaneEdgeCase,
    geometry: PaneCenterGeometry,
    preview: DockPreviewEmission,
    pixels_per_point: f32,
) -> Result<(), PaneCenterError> {
    // This finite fixture starts with one pane covering the original main root.
    verify_preview_bounds(
        edge_preview_bounds(case, geometry.workspace),
        preview,
        pixels_per_point,
    )
}

fn edge_preview_bounds(case: PaneEdgeCase, mut expected: LogicalBounds) -> LogicalBounds {
    let config = dockspace::runtime::DockPresentationConfig::default();
    let width =
        (expected.max_x - expected.min_x - config.splitter_thickness()) * config.dock_fraction();
    let height =
        (expected.max_y - expected.min_y - config.splitter_thickness()) * config.dock_fraction();
    match case {
        PaneEdgeCase::North => expected.max_y = expected.min_y + height,
        PaneEdgeCase::South => expected.min_y = expected.max_y - height,
        PaneEdgeCase::East => expected.min_x = expected.max_x - width,
        PaneEdgeCase::West => expected.max_x = expected.min_x + width,
    }
    expected
}

fn verify_preview_bounds(
    expected: LogicalBounds,
    preview: DockPreviewEmission,
    pixels_per_point: f32,
) -> Result<(), PaneCenterError> {
    let tolerance = 1.5 / f64::from(pixels_per_point);
    if ![
        preview.bounds.min_x - expected.min_x,
        preview.bounds.min_y - expected.min_y,
        preview.bounds.max_x - expected.max_x,
        preview.bounds.max_y - expected.max_y,
    ]
    .into_iter()
    .all(|delta| delta.abs() <= tolerance)
    {
        return Err(PaneCenterError::PreviewBoundsMismatch);
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct SemanticStabilitySample {
    pub(super) step: u64,
    pub(super) roster_changes: u64,
    pub(super) bounds: LogicalBounds,
}

pub(super) fn semantic_snapshot_is_stable(
    previous: Option<SemanticStabilitySample>,
    current: SemanticStabilitySample,
) -> bool {
    previous.is_some_and(|previous| {
        current.step > previous.step
            && current.roster_changes == previous.roster_changes
            && current.bounds == previous.bounds
    })
}

pub(super) fn semantic_baseline_after_missing(
    previous: Option<SemanticStabilitySample>,
    roster_changes: u64,
) -> Option<SemanticStabilitySample> {
    previous.filter(|previous| previous.roster_changes == roster_changes)
}

#[cfg(target_os = "linux")]
pub(super) fn semantic_tree_summary(tree: &TreeUpdate) -> String {
    const MAX_NODES: usize = 24;
    const MAX_TEXT_CHARS: usize = 64;

    fn text(value: Option<&str>) -> String {
        let Some(value) = value else {
            return "-".to_owned();
        };
        let mut chars = value.chars();
        let prefix = chars.by_ref().take(MAX_TEXT_CHARS).collect::<String>();
        if chars.next().is_some() {
            format!("{prefix}…")
        } else {
            prefix
        }
    }

    let visible = tree.nodes.iter().filter(|(_, node)| !node.is_hidden());
    let visible_count = visible.clone().count();
    let sample = visible
        .take(MAX_NODES)
        .map(|(id, node)| {
            format!(
                "id={id:?} role={:?} label={:?} value={:?} bounds={:?}",
                node.role(),
                text(node.label()),
                text(node.value()),
                node.bounds()
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    format!(
        "nodes={} visible={} sample=[{sample}]{}",
        tree.nodes.len(),
        visible_count,
        if visible_count > MAX_NODES {
            " (truncated)"
        } else {
            ""
        }
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PointerObservation {
    pub(super) position: LogicalPoint,
    pub(super) primary_down: bool,
}

pub(super) fn pointer_observation_for_barrier(
    tree: &TreeUpdate,
) -> Result<Option<PointerObservation>, String> {
    let selector = SemanticSelector {
        role: Role::Label,
        text: SemanticText::Label(POINTER_DIAGNOSTIC_LABEL),
    };
    let node = match semantic_node(tree, selector) {
        Ok(node) => node,
        Err(SemanticLookupError::Missing) => return Ok(None),
        Err(error) => {
            return Err(format!("pointer diagnostic selector is invalid: {error:?}"));
        }
    };
    let bounds = semantic_bounds(tree, selector)
        .map_err(|error| format!("pointer diagnostic bounds are invalid: {error:?}"))?;
    let primary_down = match node.value() {
        Some("primary-down") => true,
        Some("primary-up") => false,
        value => {
            return Err(format!(
                "pointer diagnostic has unexpected primary-button value {value:?}"
            ));
        }
    };
    Ok(Some(PointerObservation {
        position: bounds.center(),
        primary_down,
    }))
}

pub(super) fn native_drag_preview_observation(tree: &TreeUpdate) -> Result<bool, String> {
    let selector = SemanticSelector {
        role: Role::Label,
        text: SemanticText::Label(NATIVE_DRAG_PREVIEW_DIAGNOSTIC_LABEL),
    };
    let node = match semantic_node(tree, selector) {
        Ok(node) => node,
        Err(SemanticLookupError::Missing) => return Ok(false),
        Err(error) => {
            return Err(format!(
                "native drag-preview diagnostic selector is invalid: {error:?}"
            ));
        }
    };
    semantic_bounds(tree, selector)
        .map_err(|error| format!("native drag-preview bounds are invalid: {error:?}"))?;
    if node.value() != Some("painted") {
        return Err(format!(
            "native drag-preview diagnostic has unexpected value {:?}",
            node.value()
        ));
    }
    if node.supports_action(eframe::egui::accesskit::Action::Click)
        || node.supports_action(eframe::egui::accesskit::Action::Focus)
    {
        return Err("native drag-preview diagnostic must remain non-interactive".to_owned());
    }
    Ok(true)
}

pub(super) fn pointer_observation_matches(
    observed: PointerObservation,
    expected_position: LogicalPoint,
    expected_primary_down: bool,
    pixels_per_point: f32,
) -> bool {
    let scale = f64::from(pixels_per_point);
    if !scale.is_finite() || scale <= 0.0 {
        return false;
    }
    let tolerance = 1.5 / scale;
    (observed.position.x - expected_position.x).abs() <= tolerance
        && (observed.position.y - expected_position.y).abs() <= tolerance
        && observed.primary_down == expected_primary_down
}

pub(super) fn pointer_barrier_is_satisfied(
    step: u64,
    previous_step: u64,
    observed: PointerObservation,
    expected_position: LogicalPoint,
    expected_primary_down: bool,
    pixels_per_point: f32,
) -> bool {
    step > previous_step
        && pointer_observation_matches(
            observed,
            expected_position,
            expected_primary_down,
            pixels_per_point,
        )
}

#[cfg(test)]
fn pointer_barrier_sample(
    step: u64,
    previous_step: u64,
    tree: &TreeUpdate,
    expected_position: LogicalPoint,
    expected_primary_down: bool,
    pixels_per_point: f32,
) -> Result<Option<PointerObservation>, String> {
    let Some(observed) = pointer_observation_for_barrier(tree)? else {
        return Ok(None);
    };
    Ok(pointer_barrier_is_satisfied(
        step,
        previous_step,
        observed,
        expected_position,
        expected_primary_down,
        pixels_per_point,
    )
    .then_some(observed))
}

pub(super) fn tab_drag_point(
    tab: LogicalBounds,
    excluded_controls: impl IntoIterator<Item = LogicalBounds>,
) -> Option<LogicalPoint> {
    const X_FRACTIONS: [f64; 5] = [0.25, 0.4, 0.5, 0.65, 0.75];
    const Y_FRACTIONS: [f64; 3] = [0.5, 0.35, 0.65];

    let excluded_controls = excluded_controls.into_iter().collect::<Vec<_>>();
    let width = tab.max_x - tab.min_x;
    let height = tab.max_y - tab.min_y;
    Y_FRACTIONS
        .into_iter()
        .flat_map(|y| X_FRACTIONS.into_iter().map(move |x| (x, y)))
        .map(|(x, y)| LogicalPoint {
            x: tab.min_x + width * x,
            y: tab.min_y + height * y,
        })
        .find(|point| {
            tab.contains(*point)
                && excluded_controls
                    .iter()
                    .all(|control| !control.contains(*point))
        })
}

#[cfg(test)]
mod tests {
    use eframe::egui::accesskit::{Node, NodeId, Rect, TreeId, TreeInfo};

    use super::*;

    fn nested_geometry() -> (PaneCenterGeometry, TabHalfGeometry) {
        (
            PaneCenterGeometry {
                content: LogicalBounds {
                    min_x: 180.75,
                    min_y: 223.625,
                    max_x: 720.0,
                    max_y: 520.0,
                },
                workspace: LogicalBounds {
                    min_x: 0.0,
                    min_y: 0.0,
                    max_x: 720.0,
                    max_y: 520.0,
                },
            },
            TabHalfGeometry {
                tab: LogicalBounds {
                    min_x: 208.75,
                    min_y: 195.625,
                    max_x: 280.75,
                    max_y: 223.625,
                },
                close: LogicalBounds {
                    min_x: 262.75,
                    min_y: 201.625,
                    max_x: 278.75,
                    max_y: 217.625,
                },
            },
        )
    }

    #[test]
    fn nested_geometry_reads_the_painted_main_leaf_and_rejects_mismatched_tab_bounds() {
        let (expected, expected_tab) = nested_geometry();
        let rect = |bounds: LogicalBounds| {
            Rect::new(bounds.min_x, bounds.min_y, bounds.max_x, bounds.max_y)
        };
        let update = tree([
            (
                NodeId::from(1_u64),
                node(
                    Role::Pane,
                    Some("Dockspace workspace"),
                    None,
                    rect(expected.workspace),
                ),
            ),
            (
                NodeId::from(2_u64),
                node(
                    Role::Label,
                    None,
                    Some(ROOT_PANE_CONTENT_LABEL),
                    rect(expected.content),
                ),
            ),
            (
                NodeId::from(3_u64),
                node(
                    Role::Label,
                    Some("Dockspace tab bounds Main"),
                    None,
                    rect(expected_tab.tab),
                ),
            ),
            (
                NodeId::from(4_u64),
                node(
                    Role::Button,
                    Some("Close Main"),
                    None,
                    rect(expected_tab.close),
                ),
            ),
            (
                NodeId::from(5_u64),
                node(
                    Role::Label,
                    Some("Dockspace tab bounds Sibling"),
                    None,
                    Rect::new(208.75, 0.0, 300.75, 28.0),
                ),
            ),
        ]);
        let geometry = pane_center_geometry(&update).unwrap();
        let tab = tab_half_geometry(&update).unwrap();
        assert_eq!(geometry, expected);
        assert_eq!(tab, expected_tab);
        assert_eq!(
            nested_pane_bounds(geometry, tab, 1.5),
            Ok(LogicalBounds {
                min_y: 195.625,
                ..expected.content
            })
        );
        let mismatched_tab = TabHalfGeometry {
            tab: LogicalBounds {
                max_y: tab.tab.max_y + 4.0,
                ..tab.tab
            },
            ..tab
        };
        assert_eq!(
            nested_pane_bounds(geometry, mismatched_tab, 1.5),
            Err(PaneCenterError::InvalidNestedLeaf)
        );
        assert_eq!(
            nested_pane_bounds(
                PaneCenterGeometry {
                    content: geometry.workspace,
                    ..geometry
                },
                tab,
                1.5
            ),
            Err(PaneCenterError::InvalidNestedLeaf)
        );
    }

    #[test]
    fn nested_points_exclude_guides_and_outer_root_band_after_physical_rounding() {
        let (geometry, tab) = nested_geometry();
        for (scale, width, height) in [(1.0, 720, 520), (1.5, 1080, 780), (2.0, 1440, 1040)] {
            let client = PhysicalBounds {
                min_x: -200,
                min_y: 80,
                max_x: -200 + width,
                max_y: 80 + height,
            };
            for case in [NestedPaneCase::Center, NestedPaneCase::West] {
                let (physical, logical) =
                    nested_pane_point(case, geometry, tab, client, scale, &[]).unwrap();
                assert_eq!(
                    desktop_to_viewport_logical(client, scale, physical),
                    Ok(logical)
                );
                let guide = ActualGuideHit {
                    label: ACTUAL_GUIDE_LABELS[1],
                    bounds: LogicalBounds {
                        min_x: logical.x,
                        min_y: logical.y,
                        max_x: logical.x + 1.0,
                        max_y: logical.y + 1.0,
                    },
                };
                assert_eq!(
                    nested_pane_point(case, geometry, tab, client, scale, &[guide]),
                    Err(PaneCenterError::GuideCovered { label: guide.label })
                );
            }
        }
        let near_root_edge = PaneCenterGeometry {
            content: LogicalBounds {
                min_x: 4.0,
                min_y: 100.0,
                max_x: 164.0,
                max_y: 520.0,
            },
            ..geometry
        };
        let near_root_tab = TabHalfGeometry {
            tab: LogicalBounds {
                min_x: 32.0,
                min_y: 72.0,
                max_x: 104.0,
                max_y: 100.0,
            },
            close: LogicalBounds {
                min_x: 84.0,
                min_y: 76.0,
                max_x: 100.0,
                max_y: 96.0,
            },
        };
        let client = PhysicalBounds {
            min_x: 0,
            min_y: 0,
            max_x: 720,
            max_y: 520,
        };
        for case in [NestedPaneCase::Center, NestedPaneCase::West] {
            assert_eq!(
                nested_pane_point(case, near_root_edge, near_root_tab, client, 1.0, &[]),
                Err(PaneCenterError::InsideRootEdgeBand)
            );
        }
    }

    #[test]
    fn nested_preview_requires_leaf_content_or_inner_payload_instead_of_root_half() {
        let (geometry, tab) = nested_geometry();
        let west_payload = LogicalBounds {
            min_x: 180.75,
            min_y: 195.625,
            max_x: 449.875,
            max_y: 520.0,
        };
        for (case, expected) in [
            (NestedPaneCase::Center, geometry.content),
            (NestedPaneCase::West, west_payload),
        ] {
            assert_eq!(
                verify_nested_pane_preview(
                    case,
                    geometry,
                    tab,
                    DockPreviewEmission { bounds: expected },
                    1.5
                ),
                Ok(())
            );
            for wrong in [
                geometry.workspace,
                LogicalBounds {
                    min_y: 28.0,
                    ..geometry.workspace
                },
                LogicalBounds {
                    max_x: 359.5,
                    ..geometry.workspace
                },
            ] {
                assert_eq!(
                    verify_nested_pane_preview(
                        case,
                        geometry,
                        tab,
                        DockPreviewEmission { bounds: wrong },
                        1.5
                    ),
                    Err(PaneCenterError::PreviewBoundsMismatch)
                );
            }
        }
        for wrong in [
            LogicalBounds {
                min_y: 0.0,
                ..west_payload
            },
            LogicalBounds {
                min_y: geometry.content.min_y,
                ..west_payload
            },
            LogicalBounds {
                min_x: 450.875,
                max_x: 720.0,
                ..west_payload
            },
        ] {
            assert_eq!(
                verify_nested_pane_preview(
                    NestedPaneCase::West,
                    geometry,
                    tab,
                    DockPreviewEmission { bounds: wrong },
                    1.5
                ),
                Err(PaneCenterError::PreviewBoundsMismatch)
            );
        }
    }

    #[test]
    fn tab_half_points_use_full_tab_bounds_and_exclude_close_after_rounding() {
        let geometry = TabHalfGeometry {
            tab: LogicalBounds {
                min_x: 28.0,
                min_y: 0.0,
                max_x: 110.0,
                max_y: 28.0,
            },
            close: LogicalBounds {
                min_x: 92.0,
                min_y: 4.0,
                max_x: 108.0,
                max_y: 24.0,
            },
        };
        let client = PhysicalBounds {
            min_x: 300,
            min_y: 400,
            max_x: 1020,
            max_y: 920,
        };
        for case in [TabHalfCase::Before, TabHalfCase::After] {
            let (physical, logical) = tab_half_point(case, geometry, client, 1.5, &[]).unwrap();
            assert_eq!(
                desktop_to_viewport_logical(client, 1.5, physical).unwrap(),
                logical
            );
            assert_eq!(
                logical.x < geometry.tab.center().x,
                case == TabHalfCase::Before
            );
            assert!(!geometry.close.contains(logical));
            let guide = ActualGuideHit {
                label: "test guide",
                bounds: geometry.tab,
            };
            assert_eq!(
                tab_half_point(case, geometry, client, 1.5, &[guide]),
                Err(PaneCenterError::GuideCovered {
                    label: "test guide"
                })
            );
        }
        let blocked = TabHalfGeometry {
            close: LogicalBounds {
                min_x: 68.0,
                ..geometry.close
            },
            ..geometry
        };
        assert_eq!(
            tab_half_point(TabHalfCase::After, blocked, client, 1.5, &[]),
            Err(PaneCenterError::OutsideTabHalf)
        );
    }

    #[test]
    fn tab_insertion_preview_rejects_the_other_half_and_pane_center() {
        let geometry = TabHalfGeometry {
            tab: LogicalBounds {
                min_x: 28.0,
                min_y: 0.0,
                max_x: 110.0,
                max_y: 28.0,
            },
            close: LogicalBounds {
                min_x: 92.0,
                min_y: 4.0,
                max_x: 108.0,
                max_y: 24.0,
            },
        };
        let before = DockPreviewEmission {
            bounds: LogicalBounds {
                max_x: 29.0,
                ..geometry.tab
            },
        };
        let after = DockPreviewEmission {
            bounds: LogicalBounds {
                min_x: 109.5,
                max_x: 110.5,
                ..geometry.tab
            },
        };
        assert_eq!(
            verify_tab_half_preview(TabHalfCase::Before, geometry, before, 1.5),
            Ok(())
        );
        assert_eq!(
            verify_tab_half_preview(TabHalfCase::After, geometry, after, 1.5),
            Ok(())
        );
        for (case, wrong) in [(TabHalfCase::Before, after), (TabHalfCase::After, before)] {
            assert_eq!(
                verify_tab_half_preview(case, geometry, wrong, 1.5),
                Err(PaneCenterError::PreviewBoundsMismatch)
            );
            assert_eq!(
                verify_tab_half_preview(
                    case,
                    geometry,
                    DockPreviewEmission {
                        bounds: geometry.tab
                    },
                    1.5
                ),
                Err(PaneCenterError::PreviewBoundsMismatch)
            );
        }
    }

    fn overflow_tree() -> TreeUpdate {
        let visible = Rect::new(424.75, 12.25, 724.75, 40.25);
        let mut main = node(Role::Tab, Some("Main"), None, visible);
        main.set_selected(true);
        tree([
            (
                NodeId::from(1_u64),
                node(
                    Role::Label,
                    Some("Dockspace tab full bounds Main"),
                    Some("partially-visible"),
                    Rect::new(424.75, 12.25, 824.75, 40.25),
                ),
            ),
            (
                NodeId::from(2_u64),
                node(
                    Role::Label,
                    Some("Dockspace tab strip viewport Main"),
                    Some("offset=0;maximum=100"),
                    Rect::new(24.75, 12.25, 724.75, 40.25),
                ),
            ),
            (
                NodeId::from(3_u64),
                node(
                    Role::Label,
                    Some("Dockspace tab bounds Main"),
                    None,
                    visible,
                ),
            ),
            (NodeId::from(4_u64), main),
        ])
    }

    #[test]
    fn overflow_geometry_uses_full_left_half_and_checks_the_rounded_margin() {
        let mut update = overflow_tree();
        let geometry = overflow_tab_geometry(&update).unwrap();
        assert_eq!(geometry.full.max_x - geometry.full.min_x, 400.0);
        assert_eq!(geometry.visible.max_x - geometry.visible.min_x, 300.0);
        let client = PhysicalBounds {
            min_x: -500,
            min_y: 300,
            max_x: 1000,
            max_y: 1200,
        };
        assert_eq!(
            overflow_tab_point(geometry, client, 1.5, &[]),
            Ok((PhysicalPoint { x: 287, y: 339 }, egui::pos2(524.6667, 26.0)))
        );
        let preview = DockPreviewEmission {
            bounds: LogicalBounds {
                min_x: 324.25,
                min_y: 12.25,
                max_x: 325.25,
                max_y: 40.25,
            },
        };
        assert_eq!(verify_overflow_tab_preview(geometry, preview, 1.5), Ok(()));
        assert_eq!(
            verify_overflow_tab_preview(
                geometry,
                DockPreviewEmission {
                    bounds: LogicalBounds {
                        min_x: 424.75,
                        max_x: 425.75,
                        ..preview.bounds
                    },
                },
                1.5,
            ),
            Err(PaneCenterError::PreviewBoundsMismatch)
        );

        // This real viewport leaves a 2.6875-point safe interval. Its midpoint clears
        // two physical pixels before rounding, but the rounded desktop point does not.
        update.nodes[1]
            .1
            .set_bounds(Rect::new(24.75, 12.25, 459.4375, 40.25));
        update.nodes[1].1.set_value("offset=0;maximum=365.3125");
        let narrow_visible = Rect::new(424.75, 12.25, 459.4375, 40.25);
        update.nodes[2].1.set_bounds(narrow_visible);
        update.nodes[3].1.set_bounds(narrow_visible);
        let narrow = overflow_tab_geometry(&update).unwrap();
        assert_eq!(
            overflow_tab_point(narrow, client, 1.5, &[]),
            Err(PaneCenterError::NoSafeOverflowTabPoint)
        );
        update.nodes[2]
            .1
            .set_bounds(Rect::new(424.75, 12.25, 460.0, 40.25));
        assert_eq!(
            overflow_tab_geometry(&update),
            Err(PaneCenterError::InvalidOverflowTabGeometry)
        );
    }

    #[test]
    fn overflow_point_rejects_control_and_guide_coverage_without_searching() {
        let update = overflow_tree();
        let geometry = overflow_tab_geometry(&update).unwrap();
        let client = PhysicalBounds {
            min_x: -500,
            min_y: 300,
            max_x: 1000,
            max_y: 1200,
        };
        // The rounded x is 524.6667: a control ending at 523.375 overlaps its
        // two-pixel margin. The unrounded midpoint would miss this collision.
        for label in OVERFLOW_TAB_CONTROL_LABELS {
            let mut blocked = update.clone();
            blocked.nodes.push((
                NodeId::from(5_u64),
                node(
                    Role::Button,
                    Some(label),
                    None,
                    Rect::new(507.375, 20.0, 523.375, 36.0),
                ),
            ));
            let blocked_geometry = overflow_tab_geometry(&blocked).unwrap();
            assert_ne!(geometry, blocked_geometry);
            assert_eq!(
                overflow_tab_point(blocked_geometry, client, 1.5, &[]),
                Err(PaneCenterError::ControlCovered)
            );
        }
        let guide = ActualGuideHit {
            label: "Dockspace center drop guide",
            bounds: LogicalBounds {
                min_x: 507.375,
                min_y: 20.0,
                max_x: 523.375,
                max_y: 36.0,
            },
        };
        assert_eq!(
            overflow_tab_point(geometry, client, 1.5, &[guide]),
            Err(PaneCenterError::GuideCovered { label: guide.label })
        );
        let mut unselected = update;
        unselected.nodes[3].1.clear_selected();
        assert_eq!(
            overflow_tab_geometry(&unselected),
            Err(PaneCenterError::InvalidOverflowTabGeometry)
        );
    }

    #[test]
    fn stationary_overflow_requires_unselected_main_and_trims_only_intersecting_strip_controls() {
        let mut update = overflow_tree();
        assert_eq!(
            stationary_overflow_geometry(&update),
            Err(PaneCenterError::InvalidOverflowTabGeometry)
        );
        update.nodes[3].1.clear_selected();
        let cleared = stationary_overflow_geometry(&update).unwrap();
        update.nodes[3].1.set_selected(false);
        assert_eq!(stationary_overflow_geometry(&update), Ok(cleared));
        assert_eq!(
            overflow_tab_geometry(&update),
            Err(PaneCenterError::InvalidOverflowTabGeometry)
        );
        update.nodes.extend([
            (
                NodeId::from(5_u64),
                node(
                    Role::Button,
                    Some("Scroll tabs backward"),
                    None,
                    Rect::new(24.75, 12.25, 52.75, 40.25),
                ),
            ),
            (
                NodeId::from(6_u64),
                node(
                    Role::Button,
                    Some("Scroll tabs forward"),
                    None,
                    Rect::new(696.75, 12.25, 724.75, 40.25),
                ),
            ),
            (
                NodeId::from(7_u64),
                node(
                    Role::Button,
                    Some("Close Main"),
                    None,
                    Rect::new(600.75, 20.0, 616.75, 36.0),
                ),
            ),
            (
                NodeId::from(8_u64),
                node(
                    Role::Button,
                    Some("Show hidden tabs"),
                    None,
                    Rect::new(724.75, 12.25, 752.75, 40.25),
                ),
            ),
        ]);
        let client = PhysicalBounds {
            min_x: -500,
            min_y: 300,
            max_x: 1000,
            max_y: 1200,
        };
        // A large configured band clamps to half the effective viewport, so both
        // leading and trailing overlays affect this single midpoint. Close does not.
        for (menu_bounds, expected) in [
            (
                Rect::new(724.75, 12.25, 752.75, 40.25),
                (PhysicalPoint { x: 304, y: 339 }, egui::pos2(536.0, 26.0)),
            ),
            (
                Rect::new(678.75, 12.25, 696.75, 40.25),
                (PhysicalPoint { x: 283, y: 339 }, egui::pos2(522.0, 26.0)),
            ),
            (
                Rect::new(678.75, 40.25, 696.75, 68.25),
                (PhysicalPoint { x: 304, y: 339 }, egui::pos2(536.0, 26.0)),
            ),
        ] {
            update.nodes[7].1.set_bounds(menu_bounds);
            let geometry = stationary_overflow_geometry(&update).unwrap();
            assert_eq!(
                stationary_overflow_edge_point(geometry, client, 1.5, 1000.0, &[]),
                Ok(expected)
            );
        }
    }

    #[test]
    fn stationary_overflow_edge_rejects_rounded_band_control_and_guide_collisions() {
        let mut update = overflow_tree();
        update.nodes[3].1.set_selected(false);
        let geometry = stationary_overflow_geometry(&update).unwrap();
        let client = PhysicalBounds {
            min_x: -500,
            min_y: 300,
            max_x: 1000,
            max_y: 1200,
        };
        assert_eq!(
            stationary_overflow_edge_point(geometry, client, 1.5, 20.0, &[]),
            Ok((PhysicalPoint { x: 572, y: 339 }, egui::pos2(714.6667, 26.0)))
        );
        // The midpoint clears two physical pixels before rounding, but the rounded
        // desktop point falls inside the narrow band's safety margin.
        assert_eq!(
            stationary_overflow_edge_point(geometry, client, 1.5, 2.6875, &[]),
            Err(PaneCenterError::NoSafeOverflowTabPoint)
        );
        for band in [0.0, -1.0, f64::INFINITY, f64::NAN] {
            assert_eq!(
                stationary_overflow_edge_point(geometry, client, 1.5, band, &[]),
                Err(PaneCenterError::NoSafeOverflowTabPoint)
            );
        }
        // The unrounded x=714.75 misses this margin; the rounded x=714.6667 does not.
        for label in OVERFLOW_TAB_CONTROL_LABELS.into_iter().take(3) {
            let mut blocked = update.clone();
            blocked.nodes.push((
                NodeId::from(5_u64),
                node(
                    Role::Button,
                    Some(label),
                    None,
                    Rect::new(697.375, 20.0, 713.375, 36.0),
                ),
            ));
            assert_eq!(
                stationary_overflow_edge_point(
                    stationary_overflow_geometry(&blocked).unwrap(),
                    client,
                    1.5,
                    20.0,
                    &[],
                ),
                Err(PaneCenterError::ControlCovered)
            );
        }
        let guide = ActualGuideHit {
            label: "Dockspace center drop guide",
            bounds: LogicalBounds {
                min_x: 697.375,
                min_y: 20.0,
                max_x: 713.375,
                max_y: 36.0,
            },
        };
        assert_eq!(
            stationary_overflow_edge_point(geometry, client, 1.5, 20.0, &[guide]),
            Err(PaneCenterError::GuideCovered { label: guide.label })
        );
    }

    fn tree(nodes: impl IntoIterator<Item = (NodeId, Node)>) -> TreeUpdate {
        let root = NodeId::from(1_u64);
        TreeUpdate {
            nodes: nodes.into_iter().collect(),
            tree: Some(TreeInfo::new(root)),
            tree_id: TreeId::ROOT,
            focus: root,
        }
    }

    fn node(role: Role, label: Option<&str>, value: Option<&str>, rect: Rect) -> Node {
        let mut node = Node::new(role);
        if let Some(label) = label {
            node.set_label(label);
        }
        if let Some(value) = value {
            node.set_value(value);
        }
        node.set_bounds(rect);
        node
    }

    #[test]
    fn pane_cases_round_trip_inside_center_and_reject_actual_guide_overlap() {
        let geometry = PaneCenterGeometry {
            content: LogicalBounds {
                min_x: 0.0,
                min_y: 28.0,
                max_x: 720.0,
                max_y: 520.0,
            },
            workspace: LogicalBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 720.0,
                max_y: 520.0,
            },
        };
        let client = PhysicalBounds {
            min_x: 120,
            min_y: 80,
            max_x: 1200,
            max_y: 860,
        };
        for case in [PaneCenterCase::A, PaneCenterCase::B, PaneCenterCase::C] {
            let (physical, logical) = pane_center_point(case, geometry, client, 1.5, &[]).unwrap();
            assert_eq!(
                desktop_to_viewport_logical(client, 1.5, physical),
                Ok(logical)
            );
            let covered = ActualGuideHit {
                label: ACTUAL_GUIDE_LABELS[1],
                bounds: LogicalBounds {
                    min_x: logical.x,
                    min_y: logical.y - 1.0,
                    max_x: logical.x + 10.0,
                    max_y: logical.y + 1.0,
                },
            };
            assert_eq!(
                pane_center_point(case, geometry, client, 1.5, &[covered]),
                Err(PaneCenterError::GuideCovered {
                    label: covered.label
                })
            );
        }
    }

    #[test]
    fn pane_edge_cases_preserve_inner_basins_and_exclude_guides_after_rounding() {
        let geometry = PaneCenterGeometry {
            content: LogicalBounds {
                min_x: 0.0,
                min_y: 28.0,
                max_x: 720.0,
                max_y: 520.0,
            },
            workspace: LogicalBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 720.0,
                max_y: 520.0,
            },
        };
        for scale in [1.0_f32, 1.5, 2.0] {
            let client = PhysicalBounds {
                min_x: -360,
                min_y: 80,
                max_x: -360 + (720.0 * scale) as i32,
                max_y: 80 + (520.0 * scale) as i32,
            };
            for case in [
                PaneEdgeCase::North,
                PaneEdgeCase::South,
                PaneEdgeCase::East,
                PaneEdgeCase::West,
            ] {
                let (physical, logical) =
                    pane_edge_point(case, geometry, client, scale, &[]).unwrap();
                assert_eq!(
                    desktop_to_viewport_logical(client, scale, physical),
                    Ok(logical)
                );
                let covered = ActualGuideHit {
                    label: ACTUAL_GUIDE_LABELS[0],
                    bounds: LogicalBounds {
                        min_x: logical.x,
                        min_y: logical.y,
                        max_x: logical.x + 1.0,
                        max_y: logical.y + 1.0,
                    },
                };
                assert_eq!(
                    pane_edge_point(case, geometry, client, scale, &[covered]),
                    Err(PaneCenterError::GuideCovered {
                        label: covered.label,
                    })
                );
            }
        }
        let short = PaneCenterGeometry {
            content: LogicalBounds {
                max_y: 200.0,
                ..geometry.content
            },
            workspace: LogicalBounds {
                max_y: 200.0,
                ..geometry.workspace
            },
        };
        assert_eq!(
            pane_edge_point(
                PaneEdgeCase::South,
                short,
                PhysicalBounds {
                    min_x: 0,
                    min_y: 0,
                    max_x: 720,
                    max_y: 200,
                },
                1.0,
                &[],
            ),
            Err(PaneCenterError::InsideRootEdgeBand)
        );
    }

    #[test]
    fn pane_edge_preview_requires_exact_side_and_half_root_extent() {
        let geometry = PaneCenterGeometry {
            content: LogicalBounds {
                min_x: 0.0,
                min_y: 28.0,
                max_x: 720.0,
                max_y: 520.0,
            },
            workspace: LogicalBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 720.0,
                max_y: 520.0,
            },
        };
        for (case, bounds) in [
            (
                PaneEdgeCase::North,
                LogicalBounds {
                    max_y: 259.5,
                    ..geometry.workspace
                },
            ),
            (
                PaneEdgeCase::South,
                LogicalBounds {
                    min_y: 260.5,
                    ..geometry.workspace
                },
            ),
            (
                PaneEdgeCase::East,
                LogicalBounds {
                    min_x: 360.5,
                    ..geometry.workspace
                },
            ),
            (
                PaneEdgeCase::West,
                LogicalBounds {
                    max_x: 359.5,
                    ..geometry.workspace
                },
            ),
        ] {
            let preview = DockPreviewEmission { bounds };
            assert_eq!(verify_edge_preview(case, geometry, preview, 1.5), Ok(()));
            assert_eq!(
                verify_center_preview(geometry, preview, 1.5),
                Err(PaneCenterError::PreviewBoundsMismatch)
            );
            assert_eq!(
                verify_edge_preview(
                    case,
                    geometry,
                    DockPreviewEmission {
                        bounds: geometry.content,
                    },
                    1.5,
                ),
                Err(PaneCenterError::PreviewBoundsMismatch)
            );
            assert_eq!(
                verify_edge_preview(
                    case,
                    geometry,
                    DockPreviewEmission {
                        bounds: LogicalBounds {
                            min_x: bounds.min_x + 10.0,
                            ..bounds
                        },
                    },
                    1.5,
                ),
                Err(PaneCenterError::PreviewBoundsMismatch)
            );
        }
    }

    #[test]
    fn guide_exclusion_keeps_every_hit_when_directions_repeat() {
        let mut nodes = ACTUAL_GUIDE_LABELS
            .into_iter()
            .enumerate()
            .map(|(index, label)| {
                (
                    NodeId::from(index as u64 + 1),
                    node(
                        Role::Label,
                        Some(label),
                        None,
                        Rect::new(300.0, 200.0, 340.0, 240.0),
                    ),
                )
            })
            .collect::<Vec<_>>();
        nodes.push((
            NodeId::from(9_u64),
            node(
                Role::Label,
                Some(ACTUAL_GUIDE_LABELS[1]),
                None,
                Rect::new(10.0, 100.0, 40.0, 130.0),
            ),
        ));
        let hits = actual_guide_hits(&tree(nodes)).unwrap();
        assert_eq!(hits.len(), 6);
        assert_eq!(
            hits.iter()
                .filter(|hit| hit.label == ACTUAL_GUIDE_LABELS[1])
                .count(),
            2
        );
        assert!(matches!(
            actual_guide_hits(&tree([])),
            Err(PaneCenterError::MissingGuide { .. })
        ));
    }

    #[test]
    fn dock_emission_accepts_only_painted_and_checks_center_rectangle() {
        let update = tree([(
            NodeId::from(1_u64),
            node(
                Role::Label,
                Some(DOCK_PREVIEW_EMITTED_LABEL),
                Some("painted"),
                Rect::new(0.0, 28.0, 720.0, 520.0),
            ),
        )]);
        let preview = dock_preview_emission(&update).unwrap().unwrap();
        let geometry = PaneCenterGeometry {
            content: preview.bounds,
            workspace: LogicalBounds {
                min_x: 0.0,
                min_y: 0.0,
                max_x: 720.0,
                max_y: 520.0,
            },
        };
        assert_eq!(verify_center_preview(geometry, preview, 1.5), Ok(()));
        let half = DockPreviewEmission {
            bounds: LogicalBounds {
                max_x: 360.0,
                ..preview.bounds
            },
            ..preview
        };
        assert_eq!(
            verify_center_preview(geometry, half, 1.5),
            Err(PaneCenterError::PreviewBoundsMismatch)
        );
        let native = tree([(
            NodeId::from(1_u64),
            node(
                Role::Label,
                Some(NATIVE_DRAG_PREVIEW_DIAGNOSTIC_LABEL),
                Some("painted"),
                Rect::new(4.0, 4.0, 716.0, 516.0),
            ),
        )]);
        assert_eq!(dock_preview_emission(&native), Ok(None));
        for value in ["projected", "presented"] {
            let malformed = tree([(
                NodeId::from(1_u64),
                node(
                    Role::Label,
                    Some(DOCK_PREVIEW_EMITTED_LABEL),
                    Some(value),
                    Rect::new(0.0, 28.0, 720.0, 520.0),
                ),
            )]);
            assert_eq!(
                dock_preview_emission(&malformed),
                Err(PaneCenterError::InvalidPreviewValue)
            );
        }
    }

    #[test]
    fn semantic_selector_resolves_one_visible_tab_by_role_and_label() {
        let update = tree([
            (
                NodeId::from(1_u64),
                node(
                    Role::Window,
                    Some("Dockspace"),
                    None,
                    Rect::new(0.0, 0.0, 720.0, 520.0),
                ),
            ),
            (
                NodeId::from(2_u64),
                node(
                    Role::Tab,
                    Some("Child"),
                    None,
                    Rect::new(82.0, 12.0, 174.0, 44.0),
                ),
            ),
        ]);

        assert_eq!(
            semantic_bounds(
                &update,
                SemanticSelector {
                    role: Role::Tab,
                    text: SemanticText::Label("Child"),
                },
            ),
            Ok(LogicalBounds {
                min_x: 82.0,
                min_y: 12.0,
                max_x: 174.0,
                max_y: 44.0,
            })
        );
    }

    #[test]
    fn native_drag_preview_requires_an_exact_passive_renderer_fact() {
        let without_preview = tree([(
            NodeId::from(1_u64),
            node(
                Role::Window,
                Some("Dockspace"),
                None,
                Rect::new(0.0, 0.0, 720.0, 520.0),
            ),
        )]);
        assert_eq!(native_drag_preview_observation(&without_preview), Ok(false));

        let preview = tree([
            (
                NodeId::from(1_u64),
                node(
                    Role::Window,
                    Some("Dockspace"),
                    None,
                    Rect::new(0.0, 0.0, 720.0, 520.0),
                ),
            ),
            (
                NodeId::from(2_u64),
                node(
                    Role::Label,
                    Some(NATIVE_DRAG_PREVIEW_DIAGNOSTIC_LABEL),
                    Some("painted"),
                    Rect::new(4.0, 4.0, 716.0, 516.0),
                ),
            ),
        ]);
        assert_eq!(native_drag_preview_observation(&preview), Ok(true));

        let mut actionable = node(
            Role::Label,
            Some(NATIVE_DRAG_PREVIEW_DIAGNOSTIC_LABEL),
            Some("painted"),
            Rect::new(4.0, 4.0, 716.0, 516.0),
        );
        actionable.add_action(eframe::egui::accesskit::Action::Click);
        let actionable = tree([(NodeId::from(1_u64), actionable)]);
        assert!(native_drag_preview_observation(&actionable).is_err());
    }

    #[test]
    fn semantic_selector_rejects_ambiguous_nodes() {
        let update = tree([
            (
                NodeId::from(2_u64),
                node(
                    Role::Tab,
                    Some("Child"),
                    None,
                    Rect::new(82.0, 12.0, 174.0, 44.0),
                ),
            ),
            (
                NodeId::from(3_u64),
                node(
                    Role::Tab,
                    Some("Child"),
                    None,
                    Rect::new(182.0, 12.0, 274.0, 44.0),
                ),
            ),
        ]);

        assert_eq!(
            semantic_bounds(
                &update,
                SemanticSelector {
                    role: Role::Tab,
                    text: SemanticText::Label("Child"),
                },
            ),
            Err(SemanticLookupError::Ambiguous { matches: 2 })
        );
    }

    #[test]
    fn semantic_selector_can_use_accessible_value_for_a_pane_marker() {
        let update = tree([(
            NodeId::from(4_u64),
            node(
                Role::Label,
                None,
                Some("Root docking target"),
                Rect::new(330.0, 240.0, 390.0, 260.0),
            ),
        )]);

        assert_eq!(
            semantic_bounds(
                &update,
                SemanticSelector {
                    role: Role::Label,
                    text: SemanticText::Value("Root docking target"),
                },
            ),
            Ok(LogicalBounds {
                min_x: 330.0,
                min_y: 240.0,
                max_x: 390.0,
                max_y: 260.0,
            })
        );
    }

    #[test]
    fn semantic_snapshot_requires_same_bounds_and_roster_generation() {
        let bounds = LogicalBounds {
            min_x: 10.0,
            min_y: 20.0,
            max_x: 110.0,
            max_y: 60.0,
        };
        let sample = SemanticStabilitySample {
            step: 9,
            roster_changes: 4,
            bounds,
        };

        assert!(!semantic_snapshot_is_stable(None, sample));
        assert!(semantic_snapshot_is_stable(
            Some(sample),
            SemanticStabilitySample { step: 10, ..sample }
        ));
        assert!(!semantic_snapshot_is_stable(Some(sample), sample));
        assert!(!semantic_snapshot_is_stable(
            Some(sample),
            SemanticStabilitySample {
                roster_changes: 5,
                ..sample
            }
        ));
        assert!(!semantic_snapshot_is_stable(
            Some(sample),
            SemanticStabilitySample {
                bounds: LogicalBounds {
                    max_x: 111.0,
                    ..bounds
                },
                ..sample
            }
        ));

        assert_eq!(
            semantic_baseline_after_missing(Some(sample), 4),
            Some(sample)
        );
        assert_eq!(semantic_baseline_after_missing(Some(sample), 5), None);
    }

    #[test]
    fn pointer_diagnostic_reports_position_and_primary_button_state() {
        let update = tree([(
            NodeId::from(5_u64),
            node(
                Role::Label,
                Some(POINTER_DIAGNOSTIC_LABEL),
                Some("primary-down"),
                Rect::new(126.5, 74.5, 127.5, 75.5),
            ),
        )]);

        assert_eq!(
            pointer_observation_for_barrier(&update),
            Ok(Some(PointerObservation {
                position: LogicalPoint { x: 127.0, y: 75.0 },
                primary_down: true,
            }))
        );
    }

    #[test]
    fn pointer_barrier_match_requires_position_and_button_state() {
        let expected = LogicalPoint { x: 127.0, y: 75.0 };
        let matching = PointerObservation {
            position: LogicalPoint { x: 127.5, y: 74.5 },
            primary_down: true,
        };
        let stale_position = PointerObservation {
            position: LogicalPoint { x: 120.0, y: 75.0 },
            primary_down: true,
        };
        let stale_button = PointerObservation {
            position: expected,
            primary_down: false,
        };

        assert!(pointer_observation_matches(matching, expected, true, 1.0));
        assert!(!pointer_observation_matches(
            stale_position,
            expected,
            true,
            1.0
        ));
        assert!(!pointer_observation_matches(
            stale_button,
            expected,
            true,
            1.0
        ));
    }

    #[test]
    fn pointer_barrier_retries_only_a_missing_diagnostic() {
        let missing = tree(std::iter::empty());
        assert_eq!(pointer_observation_for_barrier(&missing), Ok(None));

        let ambiguous = tree([
            (
                NodeId::from(5_u64),
                node(
                    Role::Label,
                    Some(POINTER_DIAGNOSTIC_LABEL),
                    Some("primary-up"),
                    Rect::new(126.5, 74.5, 127.5, 75.5),
                ),
            ),
            (
                NodeId::from(6_u64),
                node(
                    Role::Label,
                    Some(POINTER_DIAGNOSTIC_LABEL),
                    Some("primary-up"),
                    Rect::new(128.5, 74.5, 129.5, 75.5),
                ),
            ),
        ]);
        let error = pointer_observation_for_barrier(&ambiguous)
            .expect_err("an ambiguous diagnostic must fail closed");
        assert!(error.contains("Ambiguous"));

        let invalid_bounds = tree([(
            NodeId::from(7_u64),
            node(
                Role::Label,
                Some(POINTER_DIAGNOSTIC_LABEL),
                Some("primary-up"),
                Rect::new(127.0, 75.0, 127.0, 75.0),
            ),
        )]);
        let error = pointer_observation_for_barrier(&invalid_bounds)
            .expect_err("invalid diagnostic bounds must fail closed");
        assert!(error.contains("InvalidBounds"));
    }

    #[test]
    fn pointer_barrier_requires_a_strictly_new_exact_sample() {
        let expected = LogicalPoint { x: 127.0, y: 75.0 };
        let observed = PointerObservation {
            position: expected,
            primary_down: true,
        };

        assert!(!pointer_barrier_is_satisfied(
            9, 9, observed, expected, true, 1.0
        ));
        assert!(pointer_barrier_is_satisfied(
            10, 9, observed, expected, true, 1.0
        ));
        assert!(!pointer_barrier_is_satisfied(
            10, 9, observed, expected, false, 1.0
        ));
    }

    #[test]
    fn pointer_barrier_accepts_a_later_exact_tree_after_missing() {
        let expected = LogicalPoint { x: 127.0, y: 75.0 };
        let missing = tree(std::iter::empty());
        assert_eq!(
            pointer_barrier_sample(10, 9, &missing, expected, true, 1.0),
            Ok(None)
        );

        let exact = tree([(
            NodeId::from(8_u64),
            node(
                Role::Label,
                Some(POINTER_DIAGNOSTIC_LABEL),
                Some("primary-down"),
                Rect::new(126.5, 74.5, 127.5, 75.5),
            ),
        )]);
        assert_eq!(
            pointer_barrier_sample(11, 9, &exact, expected, true, 1.0),
            Ok(Some(PointerObservation {
                position: expected,
                primary_down: true,
            }))
        );
    }

    #[test]
    fn tab_drag_point_avoids_the_close_control() {
        let tab = LogicalBounds {
            min_x: 80.0,
            min_y: 10.0,
            max_x: 200.0,
            max_y: 42.0,
        };
        let close = LogicalBounds {
            min_x: 172.0,
            min_y: 12.0,
            max_x: 198.0,
            max_y: 40.0,
        };

        let point = tab_drag_point(tab, [close]).expect("the tab has a safe drag point");

        assert!(tab.contains(point));
        assert!(!close.contains(point));
        assert!(point.x < tab.center().x);
    }

    #[test]
    fn logical_accessibility_point_uses_current_client_origin_and_scale() {
        let client = PhysicalBounds {
            min_x: 210,
            min_y: 120,
            max_x: 930,
            max_y: 640,
        };

        assert_eq!(
            viewport_logical_to_desktop(client, 1.5, LogicalPoint { x: 40.0, y: 20.0 }),
            Ok(PhysicalPoint { x: 270, y: 150 })
        );
        assert_eq!(
            desktop_to_viewport_logical(client, 1.5, PhysicalPoint { x: 270, y: 150 }),
            Ok(LogicalPoint { x: 40.0, y: 20.0 })
        );
    }

    #[test]
    fn two_stage_ingress_uses_distinct_points_inside_the_semantic_target() {
        let bounds = LogicalBounds {
            min_x: 72.0,
            min_y: 77.2,
            max_x: 144.0,
            max_y: 126.4,
        };

        let (first, routed) = two_stage_ingress_points(bounds);

        assert!(bounds.contains(first));
        assert!(bounds.contains(routed));
        assert_ne!(first, routed);
    }

    #[test]
    fn outside_route_uses_two_distinct_points_derived_from_live_geometry() {
        let work_area = PhysicalBounds {
            min_x: 0,
            min_y: 24,
            max_x: 1280,
            max_y: 1024,
        };
        let root = PhysicalBounds {
            min_x: 120,
            min_y: 100,
            max_x: 840,
            max_y: 620,
        };

        let (first, routed) =
            choose_outside_points(work_area, [root]).expect("the desktop has free space");

        assert!(work_area.contains(first));
        assert!(work_area.contains(routed));
        assert!(!root.expanded(50).contains(first));
        assert!(!root.expanded(50).contains(routed));
        assert_ne!(first, routed);
    }

    #[test]
    fn drag_threshold_point_is_derived_from_source_and_destination() {
        assert_eq!(
            point_towards(
                PhysicalPoint { x: 100, y: 100 },
                PhysicalPoint { x: 400, y: 500 },
                50.0,
            ),
            PhysicalPoint { x: 130, y: 140 }
        );
    }
}
