//! Records the core paint program in its original order, independently of GPUI layout.

use std::{collections::BTreeMap, convert::Infallible};

use anyhow::{Result, anyhow};
use dockspace::{
    geometry::LogicalRect,
    model::ItemId,
    runtime::{
        DockspaceDropEligibility, DockspacePreviewVisual, DockspaceReceiverDescriptor,
        SurfacePaintPlan, SurfacePaintProgramOperation as Operation,
        SurfacePaintProgramStep as Step,
    },
};

use crate::{DockPane, DockspaceColors};

use super::{Draw, Packet, chrome::GuideState};

pub(super) fn record(
    plan: SurfacePaintPlan<'_>,
    panels: &BTreeMap<ItemId, DockPane>,
    pointer: Option<dockspace::geometry::LogicalPoint>,
    colors: &DockspaceColors,
) -> Result<(Packet, dockspace::runtime::SurfaceProgramExecutionReceipt)> {
    let mut draws = Vec::new();
    let label = |item: Option<ItemId>| {
        item.and_then(|item| panels.get(&item))
            .map_or_else(|| "Workspace".to_owned(), |pane| pane.title.to_string())
    };
    let receiver = |draws: &mut Vec<Draw>, value: Option<DockspaceReceiverDescriptor>| {
        if let Some(value) = value {
            draws.push(Draw::Receiver(value));
        }
    };
    let receipt = plan
        .execute_surface_program(|step| {
            match step {
                Step::RootBegin { root, paint } => {
                    if let Some(contained) = root.contained() {
                        receiver(&mut draws, plan.receiver_for_contained_frame(contained));
                        if !paint.is_source_gap() {
                            draws.push(Draw::ContainedFrame(contained.outer_bounds()));
                        }
                    }
                }
                Step::Operation {
                    operation,
                    paint,
                    root,
                } => match operation {
                    Operation::Pane(pane) => {
                        receiver(&mut draws, plan.receiver_for_pane(pane));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(pane.content_bounds(), colors.pane));
                            if let Some(item) = pane.selected() {
                                draws.push(Draw::Pane(pane.content_bounds(), item));
                            }
                        }
                    }
                    Operation::TabBar(bar) => {
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(bar.bounds(), colors.tab_bar));
                        }
                        for region in bar.group_drag_regions() {
                            receiver(&mut draws, plan.receiver_for_tab_group_drag_region(region));
                        }
                    }
                    Operation::Tab(tab) => {
                        receiver(&mut draws, plan.receiver_for_tab_body(tab));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Tab(
                                tab.visible_bounds(),
                                tab.item(),
                                if tab.selected() {
                                    colors.selected_tab
                                } else {
                                    colors.tab
                                },
                                tab.selected(),
                            ));
                            draws.push(Draw::TabText(
                                tab.text_layout_bounds(),
                                tab.text_bounds(),
                                tab.item(),
                                label(Some(tab.item())),
                                if tab.selected() {
                                    colors.text
                                } else {
                                    colors.muted_text
                                },
                            ));
                        }
                    }
                    Operation::Splitter(splitter) => {
                        receiver(&mut draws, plan.receiver_for_splitter(splitter));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(splitter.draw_bounds(), colors.splitter));
                        }
                    }
                    Operation::SplitterJunction(junction) => {
                        receiver(&mut draws, plan.receiver_for_splitter_junction(junction))
                    }
                    Operation::ContainedControls(contained) => {
                        receiver(&mut draws, plan.receiver_for_contained_title(contained));
                        for resize in contained.resize() {
                            receiver(
                                &mut draws,
                                plan.receiver_for_contained_resize(contained, resize),
                            );
                        }
                        if !paint.is_source_gap() {
                            draws.push(Draw::ContainedTitle(
                                contained.title_bounds(),
                                colors.contained_title,
                            ));
                            draws.push(Draw::Text(
                                contained.title_drag_bounds(),
                                label(root.title_item()),
                                colors.text,
                            ));
                        }
                    }
                    Operation::TabStripControl(control) => {
                        receiver(&mut draws, plan.receiver_for_tab_strip_control(control));
                        if !paint.is_source_gap() {
                            draws.push(Draw::Fill(control.bounds(), colors.tab));
                        }
                    }
                    Operation::PresentationMenuAnchor(_) => {}
                },
                Step::DropGuide {
                    affordance: Some(cluster),
                    ..
                } => {
                    for target in cluster.targets() {
                        receiver(&mut draws, plan.receiver_for_drop_target(target));
                        draws.push(Draw::Guide(
                            target.draw_bounds(),
                            GuideState {
                                direction: target.direction(),
                                active: target.is_active(),
                                eligible: target.eligibility()
                                    == DockspaceDropEligibility::Eligible,
                            },
                        ));
                    }
                }
                Step::DragPreview(preview) => {
                    if let Some(target) = preview.target() {
                        draws.push(Draw::Outline(target.bounds(), colors.accent));
                    }
                    if !preview.is_layout_projected() {
                        match preview.visual() {
                            DockspacePreviewVisual::Dock { rect, .. }
                            | DockspacePreviewVisual::Contained { rect, .. } => {
                                draws.push(Draw::Preview(rect))
                            }
                            DockspacePreviewVisual::Native { .. } => {}
                        }
                    }
                }
                Step::ContainedTransformPreview(preview) => {
                    if !preview.is_layout_projected() {
                        draws.push(Draw::Preview(preview.rect()));
                    }
                }
                Step::DragDecoration(decoration) => {
                    if !decoration.source_is_projected() {
                        if let Some(pointer) = pointer {
                            if let Ok(ghost) = LogicalRect::new(
                                pointer.x() + 12.0,
                                pointer.y() + 14.0,
                                160.0,
                                28.0,
                            ) {
                                draws.push(Draw::Fill(ghost, colors.drag_ghost));
                                draws.push(Draw::Outline(ghost, colors.accent));
                                draws.push(Draw::Text(
                                    ghost,
                                    label(decoration.ghost_item()),
                                    colors.text,
                                ));
                            }
                        }
                    }
                }
                Step::PopupBackdrop(backdrop) => receiver(
                    &mut draws,
                    plan.receiver_for_tab_list_menu_backdrop(backdrop),
                ),
                Step::PopupMenu { menu, .. } => {
                    draws.push(Draw::Fill(menu.bounds(), colors.selected_tab));
                    receiver(&mut draws, plan.receiver_for_tab_list_menu_frame(menu));
                    for row in menu.rows() {
                        receiver(&mut draws, plan.receiver_for_tab_list_menu_row(row));
                        draws.push(Draw::Text(
                            row.bounds(),
                            label(Some(row.item())),
                            colors.text,
                        ));
                    }
                }
                Step::DropTarget(_)
                | Step::DropGuide {
                    affordance: None, ..
                }
                | Step::InteractionFeedback
                | Step::RootEnd(_)
                | Step::SurfaceEnd { .. } => {}
            }
            Ok::<(), Infallible>(())
        })
        .map_err(|error| anyhow!("core surface program rejected: {error:?}"))?;
    Ok((Packet { draws }, receipt))
}
