//! A public-contract consumer that records geometry without a UI or platform host.

use std::convert::Infallible;

use dockspace::geometry::LogicalRect;
use dockspace::model::{FloatingPresentationId, ItemId, RootId};
use dockspace::runtime::{
    DockspacePreviewVisual, DockspaceReceiverDescriptor, DockspaceSemanticOutput,
    DockspaceVisualId, SurfacePaintDisposition, SurfacePaintPlan,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePaintProgramVisitError, SurfaceProgramExecutionReceipt,
};

/// Owned observations in program order; none retain a frame borrow or grant input authority.
#[derive(Debug, PartialEq)]
pub enum OwnedRecord {
    RootBegin(RootId),
    Geometry {
        visual: DockspaceVisualId,
        bounds: LogicalRect,
        paint: SurfacePaintDisposition,
    },
    PaneContent {
        visual: DockspaceVisualId,
        item: Option<ItemId>,
        bounds: LogicalRect,
        paint: SurfacePaintDisposition,
    },
    RootEnd(RootId),
    Receiver(DockspaceReceiverDescriptor),
    Preview(DockspacePreviewVisual),
    TransformPreview(FloatingPresentationId, LogicalRect),
    InteractionFeedback,
    SurfaceEnd(DockspaceSemanticOutput),
}

/// Accepts the entire ordered program into an owned recording sink.
#[derive(Debug, Default)]
pub struct RecordingRenderer {
    pub records: Vec<OwnedRecord>,
}

impl RecordingRenderer {
    /// A receipt is returned only after every public program step has been accepted.
    pub fn execute(
        plan: SurfacePaintPlan<'_>,
    ) -> Result<(Self, SurfaceProgramExecutionReceipt), SurfacePaintProgramVisitError<Infallible>>
    {
        let mut sink = Self::default();
        let visible = SurfacePaintDisposition::Visible;
        let receipt = plan.execute_surface_program(|step| {
            match step {
                Step::RootBegin { root, paint } => {
                    sink.records.push(OwnedRecord::RootBegin(root.root()));
                    if let Some(frame) = root.contained() {
                        sink.geometry(frame.visual_id(), frame.outer_bounds(), paint);
                    }
                }
                Step::Operation {
                    operation, paint, ..
                } => {
                    let bounds = match operation {
                        Operation::Pane(pane) => {
                            sink.records.push(OwnedRecord::PaneContent {
                                visual: pane.visual_id(),
                                item: pane.selected(),
                                bounds: pane.content_bounds(),
                                paint,
                            });
                            pane.bounds()
                        }
                        Operation::TabBar(bar) => bar.bounds(),
                        Operation::Tab(tab) => {
                            if let Some(receiver) = plan.receiver_for_tab_body(tab) {
                                sink.records.push(OwnedRecord::Receiver(receiver));
                            }
                            tab.visible_bounds()
                        }
                        Operation::Splitter(splitter) => splitter.draw_bounds(),
                        Operation::SplitterJunction(junction) => junction.hit_bounds(),
                        Operation::TabStripControl(control) => control.bounds(),
                        Operation::ContainedControls(frame) => frame.title_bounds(),
                        Operation::PresentationMenuAnchor(anchor) => anchor.bounds(),
                    };
                    sink.geometry(operation.visual_id(), bounds, paint);
                }
                Step::RootEnd(root) => sink.records.push(OwnedRecord::RootEnd(root.root())),
                Step::PopupBackdrop(backdrop) => {
                    sink.geometry(backdrop.visual_id(), backdrop.bounds(), visible);
                }
                Step::PopupMenu { menu, paint } => {
                    sink.geometry(menu.visual_id(), menu.bounds(), paint);
                    for row in menu.rows() {
                        sink.geometry(row.visual_id(), row.bounds(), paint);
                    }
                }
                Step::DragPreview(preview) => {
                    sink.records.push(OwnedRecord::Preview(preview.visual()))
                }
                Step::DropTarget(receiver) => sink.records.push(OwnedRecord::Receiver(receiver)),
                Step::DropGuide {
                    structural,
                    affordance,
                } => {
                    sink.geometry(
                        structural.visual_id(),
                        structural.activation_bounds(),
                        visible,
                    );
                    for target in structural.targets() {
                        sink.geometry(target.visual_id(), target.draw_bounds(), visible);
                    }
                    if let Some(cluster) = affordance {
                        for target in cluster.targets() {
                            sink.geometry(target.visual_id(), target.draw_bounds(), visible);
                        }
                    }
                }
                Step::ContainedTransformPreview(preview) => sink.records.push(
                    OwnedRecord::TransformPreview(preview.floating(), preview.rect()),
                ),
                Step::DragDecoration(decoration) => {
                    sink.geometry(
                        decoration.source_visual(),
                        decoration.source_bounds(),
                        visible,
                    );
                    if let Some(bounds) = decoration.projected_root_bounds() {
                        sink.geometry(decoration.source_visual(), bounds, visible);
                    }
                }
                Step::InteractionFeedback => sink.records.push(OwnedRecord::InteractionFeedback),
                Step::SurfaceEnd { semantic_output } => {
                    sink.records.push(OwnedRecord::SurfaceEnd(semantic_output));
                }
            }
            Ok::<(), Infallible>(())
        })?;
        Ok((sink, receipt))
    }

    fn geometry(
        &mut self,
        visual: DockspaceVisualId,
        bounds: LogicalRect,
        paint: SurfacePaintDisposition,
    ) {
        self.records.push(OwnedRecord::Geometry {
            visual,
            bounds,
            paint,
        });
    }
}
