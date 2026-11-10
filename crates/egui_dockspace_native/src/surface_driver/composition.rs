//! Surface-local egui layout inside one real native viewport.

use dockspace::runtime::NativeSurfacePlacement;
use eframe::egui::{self, emath::TSTransform};

use crate::error::NativeHostProtocolError;

/// Pass-local egui mapping; output proof freezes its actual layer transform.
#[derive(Clone, Copy, Debug)]
pub(crate) struct NativePaintMapping {
    pub(crate) transform: TSTransform,
    pub(crate) local_bounds: egui::Rect,
    pub(crate) local_clip: egui::Rect,
}

impl NativePaintMapping {
    pub(crate) fn new(
        placement: NativeSurfacePlacement,
        pixels_per_point: f32,
        parent_clip: egui::Rect,
    ) -> Result<Self, NativeHostProtocolError> {
        let invalid = NativeHostProtocolError::InvalidWindowSnapshot;
        let points = f64::from(pixels_per_point);
        if !points.is_finite() || points <= 0.0 {
            return Err(invalid);
        }
        let scaling = finite_f32(placement.scale_factor().get() / points).ok_or(invalid)?;
        if scaling <= 0.0 {
            return Err(invalid);
        }
        let origin = placement.origin();
        let transform = TSTransform::new(
            egui::vec2(
                finite_f32(origin.x() / points).ok_or(invalid)?,
                finite_f32(origin.y() / points).ok_or(invalid)?,
            ),
            scaling,
        );
        let size = placement.size();
        let local_bounds = egui::Rect::from_min_max(
            egui::Pos2::ZERO,
            egui::pos2(
                finite_f32(size.width()).ok_or(invalid)?,
                finite_f32(size.height()).ok_or(invalid)?,
            ),
        );
        let clip = placement.clip();
        let viewport_clip = nonnegative_clip(
            egui::Rect::from_min_max(
                egui::pos2(
                    finite_f32(clip.x() / points).ok_or(invalid)?,
                    finite_f32(clip.y() / points).ok_or(invalid)?,
                ),
                egui::pos2(
                    finite_f32(clip.max().x() / points).ok_or(invalid)?,
                    finite_f32(clip.max().y() / points).ok_or(invalid)?,
                ),
            )
            .intersect(parent_clip),
        );
        let local_clip =
            nonnegative_clip((transform.inverse() * viewport_clip).intersect(local_bounds));
        if !finite_rect(local_bounds)
            || !local_bounds.is_positive()
            || !finite_rect(transform * local_bounds)
            || !finite_rect(viewport_clip)
            || !finite_rect(local_clip)
        {
            return Err(invalid);
        }
        Ok(Self {
            transform,
            local_bounds,
            local_clip,
        })
    }

    pub(crate) fn child_ui(
        self,
        parent: &mut egui::Ui,
        instance: egui::Id,
        surface: dockspace::model::SurfaceId,
    ) -> egui::Ui {
        let layer =
            egui::LayerId::new(parent.layer_id().order, instance.with(("surface", surface)));
        parent.ctx().set_sublayer(parent.layer_id(), layer);
        parent.ctx().set_transform_layer(layer, self.transform);
        // The host already knows this area's complete geometry. Registering it
        // directly avoids Area's invisible first sizing pass and keeps the first
        // semantic output's pixels and widget hit authority in the same pass.
        let interactable = parent.is_visible() && parent.is_enabled();
        parent.memory_mut(|memory| {
            memory.areas_mut().set_state(
                layer,
                egui::AreaState {
                    pivot_pos: Some(self.local_clip.min),
                    size: Some(self.local_clip.size()),
                    interactable,
                    ..Default::default()
                },
            );
        });
        let mut child = parent.new_child(
            egui::UiBuilder::new()
                .id_salt((instance, "native-surface", surface))
                .layer_id(layer)
                .max_rect(self.local_bounds)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        child.set_clip_rect(self.local_clip);
        let parent_transform = parent
            .ctx()
            .layer_transform_to_global(parent.layer_id())
            .unwrap_or(TSTransform::IDENTITY);
        let relative = parent_transform.inverse() * self.transform;
        // AccessKit bounds are local to their ancestor transforms, including
        // ordinary pane widgets. The real viewport root already contributes DPI.
        child
            .ctx()
            .accesskit_node_builder(child.unique_id(), |node| {
                node.set_transform(egui::accesskit::Affine::new([
                    f64::from(relative.scaling),
                    0.0,
                    0.0,
                    f64::from(relative.scaling),
                    f64::from(relative.translation.x),
                    f64::from(relative.translation.y),
                ]));
            });
        child
    }
}

fn finite_rect(rect: egui::Rect) -> bool {
    rect.is_finite() && rect.size().is_finite()
}

fn nonnegative_clip(rect: egui::Rect) -> egui::Rect {
    egui::Rect::from_min_max(rect.min, rect.max.max(rect.min))
}

fn finite_f32(value: f64) -> Option<f32> {
    if !value.is_finite() || value.abs() > f64::from(f32::MAX) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some(value as f32)
}

#[cfg(test)]
mod tests {
    use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
    use dockspace::model::SurfaceId;

    use super::*;

    #[test]
    fn placement_mapping_uses_client_pixels_once_and_preserves_unclipped_layout_size() {
        let placement = NativeSurfacePlacement::new(
            SurfaceId::new(1),
            LogicalSize::new(200.0, 100.0).unwrap(),
            PhysicalPoint::new(120.0, 60.0).unwrap(),
            ScaleFactor::new(3.0).unwrap(),
            PhysicalRect::new(150.0, 90.0, 300.0, 180.0).unwrap(),
        );
        let mapping = NativePaintMapping::new(placement, 2.0, egui::Rect::EVERYTHING).unwrap();
        assert_eq!(mapping.local_bounds.size(), egui::vec2(200.0, 100.0));
        assert_eq!(
            mapping.transform * egui::pos2(10.0, 20.0),
            egui::pos2(75.0, 60.0)
        );
        assert_eq!(
            mapping.local_clip,
            egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(110.0, 70.0))
        );
        assert_eq!(
            mapping.transform * mapping.local_clip,
            egui::Rect::from_min_max(egui::pos2(75.0, 45.0), egui::pos2(225.0, 135.0))
        );
    }

    #[test]
    fn fully_clipped_placement_keeps_layout_and_an_empty_finite_ui_clip() {
        let placement = NativeSurfacePlacement::new(
            SurfaceId::new(1),
            LogicalSize::new(200.0, 100.0).unwrap(),
            PhysicalPoint::new(900.0, 900.0).unwrap(),
            ScaleFactor::new(2.0).unwrap(),
            PhysicalRect::new(900.0, 900.0, 400.0, 200.0).unwrap(),
        );
        let mapping = NativePaintMapping::new(
            placement,
            1.0,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 400.0)),
        )
        .unwrap();
        assert_eq!(mapping.local_bounds.size(), egui::vec2(200.0, 100.0));
        assert_eq!(mapping.local_clip.size(), egui::Vec2::ZERO);
        assert!(finite_rect(mapping.local_clip));
    }

    #[test]
    fn placed_widgets_use_the_same_transformed_hit_and_clip_as_their_pixels() {
        let context = egui::Context::default();
        let mut identities = Vec::new();
        let mut clips = Vec::new();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                for (index, scale) in [(1, 1.0), (2, 2.0)] {
                    let placement = NativeSurfacePlacement::new(
                        SurfaceId::new(index),
                        LogicalSize::new(180.0, 120.0).unwrap(),
                        PhysicalPoint::new(if index == 1 { 30.0 } else { 300.0 }, 40.0).unwrap(),
                        ScaleFactor::new(scale).unwrap(),
                        PhysicalRect::new(
                            if index == 1 { 30.0 } else { 300.0 },
                            40.0,
                            160.0,
                            110.0,
                        )
                        .unwrap(),
                    );
                    let mapping = NativePaintMapping::new(placement, 1.0, ui.clip_rect()).unwrap();
                    let child = mapping.child_ui(
                        ui,
                        egui::Id::unique("placed-widget"),
                        placement.surface(),
                    );
                    let rect =
                        egui::Rect::from_min_max(egui::pos2(10.0, 10.0), egui::pos2(160.0, 100.0));
                    let response = child.interact(
                        rect,
                        child.make_persistent_id("receiver"),
                        egui::Sense::click_and_drag(),
                    );
                    child.painter().rect_filled(rect, 0.0, egui::Color32::RED);
                    identities.push((
                        response.id,
                        response.layer_id,
                        mapping.transform * egui::pos2(15.0, 15.0),
                        mapping,
                    ));
                    clips.push(mapping.transform * mapping.local_clip);
                }
            },
        );
        let authority = context
            .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
            .unwrap();
        output.textures_delta.clear();
        for (id, layer, point, mapping) in identities {
            let hit = authority.hit_test(point).unwrap();
            assert_eq!(hit.click().unwrap().id(), id);
            assert_eq!(hit.click().unwrap().layer_id(), layer);
            let outside = mapping.transform * egui::pos2(170.0, 115.0);
            assert!(!(mapping.transform * mapping.local_clip).contains(outside));
            assert!(
                authority
                    .hit_test(outside)
                    .and_then(|hit| hit.click())
                    .is_none_or(|hit| hit.id() != id)
            );
        }
        for shape in &output.shapes {
            if let egui::Shape::Rect(rect) = &shape.shape
                && rect.fill == egui::Color32::RED
            {
                assert!(clips.contains(&shape.clip_rect));
            }
        }
    }

    #[test]
    fn product_menu_preserves_placement_anchor_scale_hit_accessibility_and_output_proof() {
        assert_product_menu_geometry(egui::pos2(80.0, 40.0), false);
    }

    #[test]
    fn product_menu_uses_the_full_viewport_constraint_for_a_clipped_edge_surface() {
        assert_product_menu_geometry(egui::pos2(420.0, 330.0), true);
    }

    fn assert_product_menu_geometry(origin: egui::Pos2, at_edge: bool) {
        use dockspace::model::{
            DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
            RootId,
        };
        use dockspace::policy::DockPolicy;
        use dockspace::runtime::{DockspaceSession, SurfacePresentationResult};
        use egui_dockspace::{DockStyle, PaneView, native_support};

        struct Panes;
        impl PaneView for Panes {
            fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
                Some("Placed product pane".into())
            }
            fn ui(&mut self, _: ItemId, _: &mut egui::Ui) -> egui_dockspace::PaneResponse {
                egui_dockspace::PaneResponse::default()
            }
        }
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let context = egui::Context::default();
        context.enable_accesskit();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(700.0, 500.0));
        let placement = NativeSurfacePlacement::new(
            surface,
            LogicalSize::new(180.0, 160.0).unwrap(),
            PhysicalPoint::new(f64::from(origin.x), f64::from(origin.y)).unwrap(),
            ScaleFactor::new(1.5).unwrap(),
            PhysicalRect::new(
                f64::from(origin.x),
                f64::from(origin.y),
                270.0,
                if at_edge { 170.0 } else { 240.0 },
            )
            .unwrap(),
        );
        let instance = egui::Id::unique("placed-product-menu");
        let mut anchor = None;
        let mut final_geometry = None;
        let mut click = None;
        for phase in 0..7 {
            let mut frame = session.begin_host_frame().unwrap();
            let mut paint = None;
            let mut output = context.run_ui(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    events: match phase {
                        1 => vec![egui::Event::AccessKitActionRequest(
                            egui::accesskit::ActionRequest {
                                action: egui::accesskit::Action::Click,
                                target_tree: egui::accesskit::TreeId::ROOT,
                                target_node: anchor.unwrap(),
                                data: None,
                            },
                        )],
                        4 | 5 => vec![
                            egui::Event::PointerMoved(click.unwrap()),
                            egui::Event::PointerButton {
                                pos: click.unwrap(),
                                button: egui::PointerButton::Primary,
                                pressed: phase == 4,
                                modifiers: egui::Modifiers::NONE,
                            },
                        ],
                        _ => Vec::new(),
                    },
                    ..Default::default()
                },
                |ui| {
                    let mapping = NativePaintMapping::new(placement, 1.0, ui.clip_rect()).unwrap();
                    let mut child = mapping.child_ui(ui, instance, surface);
                    native_support::measure_surface(
                        &mut frame,
                        surface,
                        &child,
                        child.max_rect(),
                        child.clip_rect(),
                        &Panes,
                        &DockStyle::default(),
                    )
                    .unwrap();
                    let mut scroll = |ui: &egui::Ui, rect, id| {
                        let identity = ui.register_scroll_hit_candidate(rect, id);
                        (identity.id(), identity.layer_id())
                    };
                    paint = Some(
                        native_support::paint_surface(
                            &mut frame,
                            instance,
                            surface,
                            &mut child,
                            &mut Panes,
                            &DockStyle::default(),
                            &mut scroll,
                        )
                        .unwrap(),
                    );
                },
            );
            output.textures_delta.clear();
            let mut paint = paint.unwrap();
            assert!(
                paint.validate_final_output(&mut output),
                "the complete surface and product menu must retain exact final spans: phase {phase}"
            );
            let nodes = &output
                .platform_output
                .accesskit_update
                .as_ref()
                .unwrap()
                .nodes;
            let (anchor_id, anchor_node) = nodes
                .iter()
                .find(|(_, node)| {
                    node.role() == egui::accesskit::Role::Button
                        && node.label() == Some("Presentation commands")
                })
                .unwrap();
            anchor = Some(*anchor_id);
            if (2..=4).contains(&phase) {
                let (item_id, item) = nodes
                    .iter()
                    .find(|(_, node)| {
                        node.role() == egui::accesskit::Role::MenuItem
                            && node.label() == Some("Float")
                    })
                    .expect("the actual product Float command must be visible");
                let accessible = accessibility_transform(nodes, *item_id)
                    .transform_rect_bbox(item.bounds().unwrap());
                let authority = context
                    .capture_completed_pass_hit_authority(egui::ViewportId::ROOT)
                    .unwrap();
                let point = egui::pos2(
                    finite_f32((accessible.x0 + accessible.x1) * 0.5).unwrap(),
                    finite_f32((accessible.y0 + accessible.y1) * 0.5).unwrap(),
                );
                let hit = authority.hit_test(point).unwrap().click().unwrap();
                assert_eq!(hit.id().value(), item_id.0);
                assert_eq!(context.viewport_id(), egui::ViewportId::ROOT);
                let transform = context
                    .layer_transform_to_global(hit.layer_id())
                    .unwrap_or(TSTransform::IDENTITY);
                assert_eq!(transform.scaling, 1.5);
                // read_response may prefer the old this-pass backing after
                // end_pass swaps it. These rectangles and the hit authority
                // both belong to the actual completed output being checked.
                let physical = context
                    .interactive_rects_last_pass()
                    .into_iter()
                    .find(|rect| {
                        rect.contains(point)
                            && authority
                                .hit_test(rect.center())
                                .and_then(|snapshot| snapshot.click())
                                == Some(hit)
                    })
                    .expect("the completed Float widget owns an actual interactive rectangle");
                click = Some(physical.center());
                assert_eq!(
                    accessible,
                    egui::accesskit::Rect::new(
                        f64::from(physical.left()),
                        f64::from(physical.top()),
                        f64::from(physical.right()),
                        f64::from(physical.bottom()),
                    ),
                    "the product command's AccessKit bounds must match its actual pixels"
                );
                assert!(viewport.contains_rect(physical));
                assert_eq!(
                    authority
                        .hit_test(physical.center())
                        .unwrap()
                        .click()
                        .unwrap()
                        .id(),
                    hit.id()
                );
                let anchor = accessibility_transform(nodes, *anchor_id)
                    .transform_rect_bbox(anchor_node.bounds().unwrap());
                final_geometry = Some((physical, anchor));
            }
            assert_eq!(
                paint.take_local_actions().len(),
                usize::from(phase == 5),
                "a real click on the transformed Float command emits one exact action"
            );
            if phase == 6 {
                assert!(!egui::Popup::is_any_open(&context));
                assert!(
                    !nodes.iter().any(|(_, node)| {
                        node.role() == egui::accesskit::Role::MenuItem
                            && node.label() == Some("Float")
                    }),
                    "the committed click closes the product menu"
                );
            }
            assert!(paint.confirm_into(&mut frame).unwrap());
            let outputs = frame.commit().unwrap().take_painted_outputs();
            session
                .report_surface_presentations(outputs, SurfacePresentationResult::Presented)
                .unwrap();
        }
        let (bounds, anchor) = final_geometry.unwrap();
        if at_edge {
            assert!(f64::from(bounds.bottom()) < anchor.y1);
            assert!(bounds.left() < origin.x + 180.0 * 1.5);
        } else {
            assert!(f64::from(bounds.left()) >= anchor.x0);
            assert!(f64::from(bounds.left()) < anchor.x1 + 30.0);
            assert!(f64::from(bounds.top()) >= anchor.y1);
            assert!(f64::from(bounds.top()) < anchor.y1 + 30.0);
        }
    }

    fn accessibility_transform(
        nodes: &[(egui::accesskit::NodeId, egui::accesskit::Node)],
        id: egui::accesskit::NodeId,
    ) -> egui::accesskit::Affine {
        let (_, node) = nodes
            .iter()
            .find(|(candidate, _)| *candidate == id)
            .unwrap();
        let mut transform = node.transform().copied().unwrap_or_default();
        let mut current = id;
        while let Some((id, parent)) = nodes
            .iter()
            .find(|(_, node)| node.children().contains(&current))
        {
            transform = parent.transform().copied().unwrap_or_default() * transform;
            current = *id;
        }
        transform
    }

    #[test]
    fn real_surface_programs_share_one_transformed_final_output_and_preserve_pane_clips() {
        use dockspace::model::{
            DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
            RootId,
        };
        use dockspace::policy::DockPolicy;
        use dockspace::runtime::{DockspaceSession, SurfaceUnavailableReason};
        use egui_dockspace::{DockStyle, PaneView, native_support};

        struct Panes;
        impl PaneView for Panes {
            fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
                Some(format!("Pane {}", item.get()).into())
            }
            fn ui(&mut self, item: ItemId, ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
                let bounds = ui.max_rect();
                assert!(
                    ui.clip_rect().right() <= 100.0,
                    "pane clipping must retain the host's surface-local clip"
                );
                ui.painter().rect_filled(bounds, 0.0, egui::Color32::RED);
                let response = ui.button(format!("Content {}", item.get()));
                egui_dockspace::PaneResponse::default().with_content(&response)
            }
        }
        let layout = DockspaceLayout::new((1..=2).map(|index| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(index),
                DockspaceRootLayout::new(
                    RootId::new(index),
                    DockspaceNode::central_tabs([ItemId::new(index)]),
                ),
            )
        }))
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let context = egui::Context::default();
        context.enable_accesskit();
        let style = DockStyle::default();
        let mut frame = session.begin_host_frame().unwrap();
        frame.begin_encoding_pass().unwrap();
        let mut paints = Vec::new();
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(700.0, 400.0),
                )),
                ..Default::default()
            },
            |ui| {
                for (index, scale) in [(1, 1.0), (2, 2.0)] {
                    let placement = NativeSurfacePlacement::new(
                        SurfaceId::new(index),
                        LogicalSize::new(180.0, 140.0).unwrap(),
                        PhysicalPoint::new(if index == 1 { 30.0 } else { 300.0 }, 40.0).unwrap(),
                        ScaleFactor::new(scale).unwrap(),
                        PhysicalRect::new(
                            if index == 1 { 30.0 } else { 300.0 },
                            40.0,
                            100.0 * scale,
                            120.0 * scale,
                        )
                        .unwrap(),
                    );
                    let mapping = NativePaintMapping::new(placement, 1.0, ui.clip_rect()).unwrap();
                    let mut child = mapping.child_ui(
                        ui,
                        egui::Id::unique("real-composition"),
                        placement.surface(),
                    );
                    native_support::measure_surface(
                        &mut frame,
                        placement.surface(),
                        &child,
                        child.max_rect(),
                        child.clip_rect(),
                        &Panes,
                        &style,
                    )
                    .unwrap();
                    let mut scroll = |ui: &egui::Ui, rect, id| {
                        let identity = ui.register_scroll_hit_candidate(rect, id);
                        (identity.id(), identity.layer_id())
                    };
                    paints.push(
                        native_support::paint_surface(
                            &mut frame,
                            egui::Id::unique(("real-composition", index)),
                            placement.surface(),
                            &mut child,
                            &mut Panes,
                            &style,
                            &mut scroll,
                        )
                        .unwrap(),
                    );
                }
            },
        );
        let valid = native_support::validate_final_outputs(&paints, &mut output);
        output.textures_delta.clear();
        assert!(valid);
        let nodes = &output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        for (tab, content, left, scale) in [
            ("Pane 1", "Content 1", 30.0, 1.0),
            ("Pane 2", "Content 2", 300.0, 2.0),
        ] {
            for (label, role) in [
                (tab, egui::accesskit::Role::Tab),
                (content, egui::accesskit::Role::Button),
            ] {
                let (id, node) = nodes
                    .iter()
                    .find(|(_, node)| node.role() == role && node.label() == Some(label))
                    .unwrap();
                let bounds = node.bounds().unwrap();
                let transform = accessibility_transform(nodes, *id);
                let expected = egui::accesskit::Affine::new([scale, 0.0, 0.0, scale, left, 40.0])
                    .transform_rect_bbox(bounds);
                assert_eq!(
                    transform.transform_rect_bbox(bounds),
                    expected,
                    "custom chrome and ordinary pane controls must share the physical placement: {label}"
                );
            }
        }
        for paint in &mut paints {
            assert!(paint.confirm_into(&mut frame).unwrap());
        }
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let outputs = frame.commit().unwrap().take_painted_outputs();
        assert_eq!(outputs.len(), 2);
        session
            .report_surface_presentations(
                outputs,
                dockspace::runtime::SurfacePresentationResult::Presented,
            )
            .unwrap();
    }
}
