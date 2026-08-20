//! Semantic and geometry assertions for the physical X11 smoke.

use eframe::egui::accesskit::{Role, TreeUpdate};

pub(super) const POINTER_DIAGNOSTIC_LABEL: &str = "Physical smoke pointer input";

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

pub(super) fn choose_outside_point(
    work_area: PhysicalBounds,
    occupied: impl IntoIterator<Item = PhysicalBounds>,
) -> Option<PhysicalPoint> {
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
    FRACTIONS
        .into_iter()
        .flat_map(|x| FRACTIONS.into_iter().map(move |y| (x, y)))
        .map(|(x, y)| PhysicalPoint {
            x: work_area.min_x + work_area.width() * x / 10,
            y: work_area.min_y + work_area.height() * y / 10,
        })
        .filter(|point| {
            work_area.contains(*point) && expanded.iter().all(|bounds| !bounds.contains(*point))
        })
        .max_by_key(|point| {
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
        })
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
    use eframe::egui::accesskit::{Node, NodeId, Rect, Tree, TreeId};

    use super::*;

    fn tree(nodes: impl IntoIterator<Item = (NodeId, Node)>) -> TreeUpdate {
        let root = NodeId::from(1_u64);
        TreeUpdate {
            nodes: nodes.into_iter().collect(),
            tree: Some(Tree::new(root)),
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
    fn outside_point_is_derived_from_the_work_area_and_live_windows() {
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

        let point = choose_outside_point(work_area, [root]).expect("the desktop has free space");

        assert!(work_area.contains(point));
        assert!(!root.expanded(50).contains(point));
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
