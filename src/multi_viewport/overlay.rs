use egui::{Pos2, Rect, Vec2};
use egui_tiles::{ContainerKind, InsertionPoint, Tile, TileId, Tree};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum OverlayTarget {
    Center,
    Left,
    Right,
    Top,
    Bottom,
}

#[derive(Clone, Copy, Debug)]
struct OverlayTargets {
    center: Rect,
    left: Option<Rect>,
    right: Option<Rect>,
    top: Option<Rect>,
    bottom: Option<Rect>,
}

impl OverlayTargets {
    fn iter(self) -> impl Iterator<Item = (OverlayTarget, Rect)> {
        [
            Some((OverlayTarget::Center, self.center)),
            self.left.map(|r| (OverlayTarget::Left, r)),
            self.right.map(|r| (OverlayTarget::Right, r)),
            self.top.map(|r| (OverlayTarget::Top, r)),
            self.bottom.map(|r| (OverlayTarget::Bottom, r)),
        ]
        .into_iter()
        .flatten()
    }

    fn hit_test(self, pointer: Pos2, parent_center: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.center.width() * 0.5;
        if hs_w > 0.0 {
            let delta = pointer - parent_center;
            let len2 = delta.x * delta.x + delta.y * delta.y;

            let r_threshold_center = hs_w * 1.4;
            let r_threshold_sides = hs_w * (1.4 + 1.2);

            if len2 < r_threshold_center * r_threshold_center {
                return Some((OverlayTarget::Center, self.center));
            }

            if len2 < r_threshold_sides * r_threshold_sides {
                let prefer_horizontal = delta.x.abs() >= delta.y.abs();
                if prefer_horizontal {
                    if delta.x < 0.0 {
                        if let Some(r) = self.left {
                            return Some((OverlayTarget::Left, r));
                        }
                    } else if let Some(r) = self.right {
                        return Some((OverlayTarget::Right, r));
                    }
                } else if delta.y < 0.0 {
                    if let Some(r) = self.top {
                        return Some((OverlayTarget::Top, r));
                    }
                } else if let Some(r) = self.bottom {
                    return Some((OverlayTarget::Bottom, r));
                }
            }

            let expand = (hs_w * 0.30).round();
            if let Some(hit) = self
                .iter()
                .find(|(_t, rect)| rect.expand(expand).contains(pointer))
            {
                return Some(hit);
            }
        }

        self.iter().find(|(_t, rect)| rect.contains(pointer))
    }

    fn hit_test_boxes(self, pointer: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.center.width() * 0.5;
        let expand = if hs_w > 0.0 {
            (hs_w * 0.30).round()
        } else {
            0.0
        };
        self.iter()
            .find(|(_t, rect)| rect.expand(expand).contains(pointer))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct DockingOverlay {
    tile_rect: Rect,
    targets: OverlayTargets,
    hovered: Option<(OverlayTarget, Rect)>,
}

impl DockingOverlay {
    pub(super) fn hovered_target(self) -> Option<OverlayTarget> {
        self.hovered.map(|(t, _)| t)
    }
}

#[derive(Clone, Copy, Debug)]
struct OuterOverlayTargets {
    left: Rect,
    right: Rect,
    top: Rect,
    bottom: Rect,
}

impl OuterOverlayTargets {
    fn iter(self) -> impl Iterator<Item = (OverlayTarget, Rect)> {
        [
            (OverlayTarget::Left, self.left),
            (OverlayTarget::Right, self.right),
            (OverlayTarget::Top, self.top),
            (OverlayTarget::Bottom, self.bottom),
        ]
        .into_iter()
    }

    fn hit_test(self, pointer: Pos2, parent_center: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.left.width() * 0.5;
        if hs_w > 0.0 {
            let delta = pointer - parent_center;
            let len2 = delta.x * delta.x + delta.y * delta.y;

            let r_threshold_sides = hs_w * (1.4 + 1.2);
            if len2 < r_threshold_sides * r_threshold_sides {
                let prefer_horizontal = delta.x.abs() >= delta.y.abs();
                if prefer_horizontal {
                    if delta.x < 0.0 {
                        return Some((OverlayTarget::Left, self.left));
                    }
                    return Some((OverlayTarget::Right, self.right));
                }

                if delta.y < 0.0 {
                    return Some((OverlayTarget::Top, self.top));
                }
                return Some((OverlayTarget::Bottom, self.bottom));
            }

            let expand = (hs_w * 0.30).round();
            if let Some(hit) = self
                .iter()
                .find(|(_t, rect)| rect.expand(expand).contains(pointer))
            {
                return Some(hit);
            }
        }

        self.iter().find(|(_t, rect)| rect.contains(pointer))
    }

    fn hit_test_boxes(self, pointer: Pos2) -> Option<(OverlayTarget, Rect)> {
        let hs_w = self.left.width() * 0.5;
        let expand = if hs_w > 0.0 {
            (hs_w * 0.30).round()
        } else {
            0.0
        };
        self.iter()
            .find(|(_t, rect)| rect.expand(expand).contains(pointer))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct OuterDockingOverlay {
    dock_rect: Rect,
    targets: OuterOverlayTargets,
    hovered: Option<(OverlayTarget, Rect)>,
}

impl OuterDockingOverlay {
    pub(super) fn hovered_target(self) -> Option<OverlayTarget> {
        self.hovered.map(|(t, _)| t)
    }
}

pub(super) fn pointer_in_outer_band(dock_rect: Rect, pointer: Pos2) -> bool {
    if !dock_rect.contains(pointer) {
        return false;
    }

    let min_dim = dock_rect.width().min(dock_rect.height());
    if min_dim <= 0.0 {
        return false;
    }

    let band = (min_dim * 0.22).clamp(32.0, 80.0);
    let dx = (pointer.x - dock_rect.left()).min(dock_rect.right() - pointer.x);
    let dy = (pointer.y - dock_rect.top()).min(dock_rect.bottom() - pointer.y);
    dx.min(dy) <= band
}

fn outer_overlay_targets_in_rect(dock_rect: Rect) -> Option<OuterOverlayTargets> {
    let min_dim = dock_rect.width().min(dock_rect.height());
    if min_dim <= 0.0 {
        return None;
    }

    let size = (min_dim * 0.12).clamp(22.0, 56.0);
    let hs = size * 0.5;
    let margin = (size * 0.35).clamp(6.0, 18.0);

    let center = dock_rect.center();
    let left_center = Pos2::new(dock_rect.left() + margin + hs, center.y);
    let right_center = Pos2::new(dock_rect.right() - margin - hs, center.y);
    let top_center = Pos2::new(center.x, dock_rect.top() + margin + hs);
    let bottom_center = Pos2::new(center.x, dock_rect.bottom() - margin - hs);

    if left_center.x + hs >= right_center.x - hs || top_center.y + hs >= bottom_center.y - hs {
        return None;
    }

    let left = Rect::from_center_size(left_center, Vec2::splat(size)).intersect(dock_rect);
    let right = Rect::from_center_size(right_center, Vec2::splat(size)).intersect(dock_rect);
    let top = Rect::from_center_size(top_center, Vec2::splat(size)).intersect(dock_rect);
    let bottom = Rect::from_center_size(bottom_center, Vec2::splat(size)).intersect(dock_rect);

    (left.is_positive() && right.is_positive() && top.is_positive() && bottom.is_positive())
        .then_some(OuterOverlayTargets {
            left,
            right,
            top,
            bottom,
        })
}

pub(super) fn outer_overlay_for_dock_rect(
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<OuterDockingOverlay> {
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }

    let targets = outer_overlay_targets_in_rect(dock_rect)?;
    let hovered = targets.hit_test(pointer, dock_rect.center());
    Some(OuterDockingOverlay {
        dock_rect,
        targets,
        hovered,
    })
}

pub(super) fn outer_overlay_for_dock_rect_explicit(
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<OuterDockingOverlay> {
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }

    let targets = outer_overlay_targets_in_rect(dock_rect)?;
    let hovered = targets.hit_test_boxes(pointer);
    Some(OuterDockingOverlay {
        dock_rect,
        targets,
        hovered,
    })
}

fn outer_insertion_for_tree<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    let root = tree.root?;
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }
    let overlay = outer_overlay_for_dock_rect(dock_rect, pointer)?;
    let (target, _rect) = overlay.hovered?;

    Some(match target {
        OverlayTarget::Left => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Horizontal(0))
        }
        OverlayTarget::Right => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Horizontal(usize::MAX))
        }
        OverlayTarget::Top => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Vertical(0))
        }
        OverlayTarget::Bottom => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Vertical(usize::MAX))
        }
        OverlayTarget::Center => return None,
    })
}

fn outer_insertion_for_tree_explicit<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    let root = tree.root?;
    if !pointer_in_outer_band(dock_rect, pointer) {
        return None;
    }
    let targets = outer_overlay_targets_in_rect(dock_rect)?;
    let (target, _rect) = targets.hit_test_boxes(pointer)?;
    Some(match target {
        OverlayTarget::Left => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Horizontal(0))
        }
        OverlayTarget::Right => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Horizontal(usize::MAX))
        }
        OverlayTarget::Top => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Vertical(0))
        }
        OverlayTarget::Bottom => {
            InsertionPoint::new(root, egui_tiles::ContainerInsertion::Vertical(usize::MAX))
        }
        OverlayTarget::Center => return None,
    })
}

pub(super) fn overlay_insertion_for_tree_with_outer<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
) -> Option<InsertionPoint> {
    if pointer_in_outer_band(dock_rect, pointer) {
        outer_insertion_for_tree(tree, dock_rect, pointer)
    } else {
        overlay_insertion_for_tree(tree, pointer)
    }
}

fn best_tile_under_pointer_considering_dragged<Pane>(
    tree: &Tree<Pane>,
    pointer: Pos2,
    dragged_tile: Option<TileId>,
) -> Option<(TileId, Rect)> {
    let (tile_id, tile_rect) = best_tile_under_pointer(tree, pointer)?;
    if dragged_tile != Some(tile_id) {
        return Some((tile_id, tile_rect));
    }

    if let Some(parent) = tree.tiles.parent_of(tile_id) {
        if let Some(parent_rect) = tree.tiles.rect(parent) {
            return Some((parent, parent_rect));
        }
    }

    if let Some(root) = tree.root {
        if let Some(root_rect) = tree.tiles.rect(root) {
            return Some((root, root_rect));
        }
    }

    Some((tile_id, tile_rect))
}

fn overlay_insertion_for_tree_explicit_considering_dragged<Pane>(
    tree: &Tree<Pane>,
    pointer: Pos2,
    dragged_tile: Option<TileId>,
) -> Option<InsertionPoint> {
    let (tile_id, tile_rect) =
        best_tile_under_pointer_considering_dragged(tree, pointer, dragged_tile)?;

    let kind = tree.tiles.get(tile_id).and_then(|t| t.kind());
    let allow_lr = kind != Some(ContainerKind::Horizontal);
    let allow_tb = kind != Some(ContainerKind::Vertical);

    let targets = overlay_targets_in_rect(tile_rect, allow_lr, allow_tb);
    let (target, _rect) = targets.hit_test_boxes(pointer)?;

    Some(match target {
        OverlayTarget::Center => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Tabs(usize::MAX))
        }
        OverlayTarget::Left => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Horizontal(0))
        }
        OverlayTarget::Right => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ),
        OverlayTarget::Top => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Vertical(0))
        }
        OverlayTarget::Bottom => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Vertical(usize::MAX),
        ),
    })
}

pub(super) fn overlay_insertion_for_tree_explicit_with_outer_considering_dragged<Pane>(
    tree: &Tree<Pane>,
    dock_rect: Rect,
    pointer: Pos2,
    dragged_tile: Option<TileId>,
) -> Option<InsertionPoint> {
    if pointer_in_outer_band(dock_rect, pointer) {
        outer_insertion_for_tree_explicit(tree, dock_rect, pointer)
    } else {
        overlay_insertion_for_tree_explicit_considering_dragged(tree, pointer, dragged_tile)
    }
}

fn overlay_insertion_for_tree<Pane>(tree: &Tree<Pane>, pointer: Pos2) -> Option<InsertionPoint> {
    let overlay = overlay_for_tree_at_pointer(tree, pointer)?;
    let (target, _rect) = overlay.hovered?;

    let tile_id = best_tile_under_pointer(tree, pointer)?.0;

    Some(match target {
        OverlayTarget::Center => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Tabs(usize::MAX))
        }
        OverlayTarget::Left => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Horizontal(0))
        }
        OverlayTarget::Right => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Horizontal(usize::MAX),
        ),
        OverlayTarget::Top => {
            InsertionPoint::new(tile_id, egui_tiles::ContainerInsertion::Vertical(0))
        }
        OverlayTarget::Bottom => InsertionPoint::new(
            tile_id,
            egui_tiles::ContainerInsertion::Vertical(usize::MAX),
        ),
    })
}

pub(super) fn overlay_for_tree_at_pointer<Pane>(
    tree: &Tree<Pane>,
    pointer: Pos2,
) -> Option<DockingOverlay> {
    let (tile_id, tile_rect) = best_tile_under_pointer(tree, pointer)?;

    let kind = tree.tiles.get(tile_id).and_then(|t| t.kind());
    let allow_lr = kind != Some(ContainerKind::Horizontal);
    let allow_tb = kind != Some(ContainerKind::Vertical);

    let targets = overlay_targets_in_rect(tile_rect, allow_lr, allow_tb);
    let hovered = targets.hit_test(pointer, tile_rect.center());

    Some(DockingOverlay {
        tile_rect,
        targets,
        hovered,
    })
}

pub(super) fn overlay_for_tree_at_pointer_explicit<Pane>(
    tree: &Tree<Pane>,
    pointer: Pos2,
) -> Option<DockingOverlay> {
    let (tile_id, tile_rect) = best_tile_under_pointer(tree, pointer)?;

    let kind = tree.tiles.get(tile_id).and_then(|t| t.kind());
    let allow_lr = kind != Some(ContainerKind::Horizontal);
    let allow_tb = kind != Some(ContainerKind::Vertical);

    let targets = overlay_targets_in_rect(tile_rect, allow_lr, allow_tb);
    let hovered = targets.hit_test_boxes(pointer);

    Some(DockingOverlay {
        tile_rect,
        targets,
        hovered,
    })
}

pub(super) fn overlay_for_tree_at_pointer_considering_dragged<Pane>(
    tree: &Tree<Pane>,
    pointer: Pos2,
    dragged_tile: Option<TileId>,
) -> Option<DockingOverlay> {
    let (tile_id, tile_rect) =
        best_tile_under_pointer_considering_dragged(tree, pointer, dragged_tile)?;

    let kind = tree.tiles.get(tile_id).and_then(|t| t.kind());
    let allow_lr = kind != Some(ContainerKind::Horizontal);
    let allow_tb = kind != Some(ContainerKind::Vertical);

    let targets = overlay_targets_in_rect(tile_rect, allow_lr, allow_tb);
    let hovered = targets.hit_test(pointer, tile_rect.center());

    Some(DockingOverlay {
        tile_rect,
        targets,
        hovered,
    })
}

fn overlay_targets_in_rect(tile_rect: Rect, allow_lr: bool, allow_tb: bool) -> OverlayTargets {
    let min_dim = tile_rect.width().min(tile_rect.height());
    let size = (min_dim * 0.16).clamp(24.0, 56.0);
    let gap = (size * 0.25).clamp(6.0, 18.0);

    let center = Rect::from_center_size(tile_rect.center(), Vec2::splat(size)).intersect(tile_rect);
    let left = allow_lr
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() - Vec2::new(size + gap, 0.0),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());
    let right = allow_lr
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() + Vec2::new(size + gap, 0.0),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());
    let top = allow_tb
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() - Vec2::new(0.0, size + gap),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());
    let bottom = allow_tb
        .then(|| {
            Rect::from_center_size(
                tile_rect.center() + Vec2::new(0.0, size + gap),
                Vec2::splat(size),
            )
        })
        .map(|r| r.intersect(tile_rect))
        .filter(|r| r.is_positive());

    OverlayTargets {
        center,
        left,
        right,
        top,
        bottom,
    }
}

pub(super) fn tile_contains_descendant<Pane>(
    tree: &Tree<Pane>,
    root: TileId,
    candidate: TileId,
) -> bool {
    if root == candidate {
        return true;
    }

    let mut stack = vec![root];
    while let Some(tile_id) = stack.pop() {
        let Some(tile) = tree.tiles.get(tile_id) else {
            continue;
        };
        match tile {
            Tile::Pane(_) => {}
            Tile::Container(container) => {
                for &child in container.children() {
                    if child == candidate {
                        return true;
                    }
                    stack.push(child);
                }
            }
        }
    }

    false
}

fn best_tile_under_pointer<Pane>(tree: &Tree<Pane>, pointer: Pos2) -> Option<(TileId, Rect)> {
    let mut best: Option<(TileId, Rect)> = None;
    let mut best_area = f32::INFINITY;

    for tile_id in tree.active_tiles() {
        let Some(rect) = tree.tiles.rect(tile_id) else {
            continue;
        };
        if !rect.contains(pointer) {
            continue;
        }
        let area = rect.width() * rect.height();
        if area < best_area {
            best_area = area;
            best = Some((tile_id, rect));
        }
    }

    best
}

pub(super) fn paint_overlay(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    overlay: DockingOverlay,
) {
    if let Some((target, _rect)) = overlay.hovered {
        let split_frac = 0.5;
        let preview_rect = match target {
            OverlayTarget::Center => overlay.tile_rect.shrink(1.0),
            OverlayTarget::Left => overlay
                .tile_rect
                .split_left_right_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Right => overlay
                .tile_rect
                .split_left_right_at_fraction(split_frac)
                .1
                .shrink(1.0),
            OverlayTarget::Top => overlay
                .tile_rect
                .split_top_bottom_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Bottom => overlay
                .tile_rect
                .split_top_bottom_at_fraction(split_frac)
                .1
                .shrink(1.0),
        };

        let stroke = visuals.selection.stroke;
        let base = visuals.selection.bg_fill;
        let fill = with_alpha(base, ((base.a() as f32) * 0.45) as u8);
        painter.rect(preview_rect, 1.0, fill, stroke, egui::StrokeKind::Inside);
    }

    let panel_fill = visuals.window_fill().gamma_multiply(0.75);
    let panel_stroke = visuals.widgets.inactive.bg_stroke;
    let active_fill = visuals.selection.bg_fill.gamma_multiply(0.85);
    let active_stroke = visuals.selection.stroke;
    let inactive_icon = visuals.widgets.inactive.fg_stroke.color;
    let active_icon = visuals.selection.stroke.color;

    for (t, rect) in overlay.targets.iter() {
        let hovered = overlay.hovered.is_some_and(|(ht, _)| ht == t);
        let (fill, stroke) = if hovered {
            (active_fill, active_stroke)
        } else {
            (panel_fill, panel_stroke)
        };

        painter.rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);

        let icon_color = if hovered { active_icon } else { inactive_icon };
        paint_overlay_icon(painter, rect, t, icon_color);
    }
}

pub(super) fn paint_outer_overlay(
    painter: &egui::Painter,
    visuals: &egui::Visuals,
    overlay: OuterDockingOverlay,
) {
    if let Some((target, _rect)) = overlay.hovered {
        let split_frac = 0.5;
        let preview_rect = match target {
            OverlayTarget::Left => overlay
                .dock_rect
                .split_left_right_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Right => overlay
                .dock_rect
                .split_left_right_at_fraction(split_frac)
                .1
                .shrink(1.0),
            OverlayTarget::Top => overlay
                .dock_rect
                .split_top_bottom_at_fraction(split_frac)
                .0
                .shrink(1.0),
            OverlayTarget::Bottom => overlay
                .dock_rect
                .split_top_bottom_at_fraction(split_frac)
                .1
                .shrink(1.0),
            OverlayTarget::Center => overlay.dock_rect.shrink(1.0),
        };

        let stroke = visuals.selection.stroke;
        let base = visuals.selection.bg_fill;
        let fill = with_alpha(base, ((base.a() as f32) * 0.45) as u8);
        painter.rect(preview_rect, 1.0, fill, stroke, egui::StrokeKind::Inside);
    }

    let panel_fill = visuals.window_fill().gamma_multiply(0.75);
    let panel_stroke = visuals.widgets.inactive.bg_stroke;
    let active_fill = visuals.selection.bg_fill.gamma_multiply(0.85);
    let active_stroke = visuals.selection.stroke;
    let inactive_icon = visuals.widgets.inactive.fg_stroke.color;
    let active_icon = visuals.selection.stroke.color;

    for (t, rect) in overlay.targets.iter() {
        let hovered = overlay.hovered.is_some_and(|(ht, _)| ht == t);
        let (fill, stroke) = if hovered {
            (active_fill, active_stroke)
        } else {
            (panel_fill, panel_stroke)
        };

        painter.rect(rect, 4.0, fill, stroke, egui::StrokeKind::Inside);

        let icon_color = if hovered { active_icon } else { inactive_icon };
        paint_overlay_icon(painter, rect, t, icon_color);
    }
}

fn with_alpha(color: egui::Color32, alpha: u8) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

fn paint_overlay_icon(
    painter: &egui::Painter,
    rect: Rect,
    target: OverlayTarget,
    color: egui::Color32,
) {
    let icon_rect = Rect::from_center_size(rect.center(), rect.size() * 0.62);
    let stroke = egui::Stroke::new(1.5, color.gamma_multiply(0.9));

    painter.rect_stroke(icon_rect, 2.0, stroke, egui::StrokeKind::Inside);

    match target {
        OverlayTarget::Center => {
            let mid = icon_rect.center();
            painter.line_segment(
                [
                    Pos2::new(icon_rect.left(), mid.y),
                    Pos2::new(icon_rect.right(), mid.y),
                ],
                stroke,
            );
            painter.line_segment(
                [
                    Pos2::new(mid.x, icon_rect.top()),
                    Pos2::new(mid.x, icon_rect.bottom()),
                ],
                stroke,
            );
        }
        OverlayTarget::Left => {
            let split_x = icon_rect.left() + icon_rect.width() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(icon_rect.min, Pos2::new(split_x, icon_rect.max.y)),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(split_x, icon_rect.top()),
                    Pos2::new(split_x, icon_rect.bottom()),
                ],
                stroke,
            );
        }
        OverlayTarget::Right => {
            let split_x = icon_rect.right() - icon_rect.width() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(split_x, icon_rect.min.y), icon_rect.max),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(split_x, icon_rect.top()),
                    Pos2::new(split_x, icon_rect.bottom()),
                ],
                stroke,
            );
        }
        OverlayTarget::Top => {
            let split_y = icon_rect.top() + icon_rect.height() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(icon_rect.min, Pos2::new(icon_rect.max.x, split_y)),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(icon_rect.left(), split_y),
                    Pos2::new(icon_rect.right(), split_y),
                ],
                stroke,
            );
        }
        OverlayTarget::Bottom => {
            let split_y = icon_rect.bottom() - icon_rect.height() * 0.38;
            let fill = color.gamma_multiply(0.25);
            painter.rect_filled(
                Rect::from_min_max(Pos2::new(icon_rect.min.x, split_y), icon_rect.max),
                0.0,
                fill,
            );
            painter.line_segment(
                [
                    Pos2::new(icon_rect.left(), split_y),
                    Pos2::new(icon_rect.right(), split_y),
                ],
                stroke,
            );
        }
    }
}
