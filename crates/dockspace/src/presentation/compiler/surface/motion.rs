//! Materialize sampled pane bounds from a sealed target without solving its layout again.

use super::*;
use crate::presentation::scene::{
    PaneSceneId, PresentationMenuAnchorHost, ResolvedSurfaceProgramOperation,
    ResolvedSurfaceProgramStep, SurfaceProgramVisitError,
};

/// Rebuilds leaf content and chrome at their sampled sizes, retaining target constraints.
///
/// A popup on this surface settles immediately to the target so its complete popup
/// plane and exact receiver roster stay available. Other surfaces keep their motion.
#[allow(clippy::too_many_arguments)]
pub(crate) fn materialize_presentation_sample(
    workspace: &Workspace,
    policy: &DockPolicySnapshot,
    manifest: &SceneRequirementManifest,
    tab_strip_states: &TabStripStateStore,
    target: &PresentationPlan,
    rects: &BTreeMap<PaneSceneId, LogicalRect>,
    ranks: &BTreeMap<PaneSceneId, usize>,
) -> Result<PresentationPlan, PresentationCompilationError> {
    let surface = target.surface();
    if target.popup().visual_for_surface(surface).is_some() {
        return Ok(target.clone());
    }
    let moving_roots = target
        .pane_records()
        .iter()
        .filter(|pane| {
            rects
                .get(&pane.id())
                .is_some_and(|rect| *rect != pane.bounds())
        })
        .map(|pane| pane.id().root)
        .collect::<BTreeSet<_>>();
    if moving_roots.is_empty() {
        return Ok(target.clone());
    }

    let requirements = manifest
        .surface(surface)
        .ok_or(PresentationCompilationError::RequirementsVanished { surface })?;
    if let Some(actual) = target.measurement_ticket()
        && actual != requirements.ticket()
    {
        return Err(SceneCompilationError::TicketMismatch {
            expected: requirements.ticket(),
            actual,
        }
        .into());
    }
    if requirements.ticket().policy() != policy.revision() {
        return Err(SceneCompilationError::PolicyRevisionMismatch {
            ticket: requirements.ticket().policy(),
            snapshot: policy.revision(),
        }
        .into());
    }
    if tab_strip_states.popup_requirement() != target.popup() || manifest.popup() != target.popup()
    {
        return Err(SceneCompilationError::PopupPlaneStateMismatch.into());
    }
    let layout = target
        .layout_facts()
        .ok_or(SceneCompilationError::SurfaceProgramInvariant)?;
    let fraction = dock_fraction(layout.config())?;
    let mut sampled = target.begin_presentation_sample(moving_roots, ranks);
    target
        .visit_surface_program(|step| -> Result<(), SceneCompilationError> {
            match step {
                ResolvedSurfaceProgramStep::RootBegin(root) => {
                    let presentation = root
                        .contained_record()
                        .map_or(SurfaceProgramRootPresentation::Main, |contained| {
                            SurfaceProgramRootPresentation::Contained(contained.floating())
                        });
                    sampled.begin_surface_program_root(root.root(), presentation)?;
                }
                ResolvedSurfaceProgramStep::Operation { operation, .. } => match operation {
                    ResolvedSurfaceProgramOperation::Pane(pane) => {
                        let id = pane.id();
                        let Some(Node::Tabs {
                            items, selected, ..
                        }) = workspace.node(id.tabs)
                        else {
                            return Err(SceneCompilationError::MissingNode {
                                surface,
                                root: id.root,
                                node: id.tabs,
                            });
                        };
                        let root_layout =
                            layout
                                .root(id.root)
                                .ok_or(SceneCompilationError::MissingRoot {
                                    surface,
                                    root: id.root,
                                })?;
                        let bounds = clip_rect(
                            rects.get(&id).copied().unwrap_or_else(|| pane.bounds()),
                            root_layout.bounds(),
                        )?;
                        let presentation_menu_anchor = target
                            .presentation_menu_anchor_records()
                            .iter()
                            .any(|anchor| {
                                anchor.host()
                                    == PresentationMenuAnchorHost::TabBar(TabBarSceneId {
                                        root: id.root,
                                        tabs: id.tabs,
                                    })
                            });
                        compile_tabs_leaf(
                            policy,
                            layout.config(),
                            requirements,
                            layout.measurements(),
                            surface,
                            id.root,
                            id.tabs,
                            0,
                            bounds,
                            items,
                            *selected,
                            pane.layer(),
                            fraction,
                            tab_strip_states,
                            false,
                            presentation_menu_anchor,
                            None,
                            false,
                            &mut sampled,
                        )?;
                    }
                    ResolvedSurfaceProgramOperation::Splitter(record) => {
                        if sampled.partition_available(record.id().root) {
                            sampled.push_surface_program_root_operation(
                                SurfaceProgramRootOperation::Splitter(*record.id()),
                            )?;
                        }
                    }
                    ResolvedSurfaceProgramOperation::SplitterJunction(record) => {
                        if sampled.partition_available(record.id().root()) {
                            sampled.push_surface_program_root_operation(
                                SurfaceProgramRootOperation::SplitterJunction(record.id()),
                            )?;
                        }
                    }
                    ResolvedSurfaceProgramOperation::ContainedControls(record) => {
                        sampled.push_surface_program_root_operation(
                            SurfaceProgramRootOperation::ContainedControls(record.floating()),
                        )?;
                    }
                    ResolvedSurfaceProgramOperation::PresentationMenuAnchor(record)
                        if matches!(
                            record.host(),
                            PresentationMenuAnchorHost::ContainedTitle(_)
                        ) =>
                    {
                        sampled.push_surface_program_root_operation(
                            SurfaceProgramRootOperation::PresentationMenuAnchor(record.root()),
                        )?;
                    }
                    ResolvedSurfaceProgramOperation::TabBar(_)
                    | ResolvedSurfaceProgramOperation::Tab(_)
                    | ResolvedSurfaceProgramOperation::TabStripControl(_)
                    | ResolvedSurfaceProgramOperation::PresentationMenuAnchor(_) => {}
                },
                ResolvedSurfaceProgramStep::RootEnd(root) => {
                    sampled.end_surface_program_root(root.root())?;
                }
                // The fresh program defers the retained catalog and emits the same
                // global phases. Leaf compilation supplies its new local operations.
                ResolvedSurfaceProgramStep::PopupBackdrop(_)
                | ResolvedSurfaceProgramStep::PopupMenu(_)
                | ResolvedSurfaceProgramStep::DragPreview
                | ResolvedSurfaceProgramStep::DropTarget(_)
                | ResolvedSurfaceProgramStep::DropGuide(_)
                | ResolvedSurfaceProgramStep::ContainedTransformPreview
                | ResolvedSurfaceProgramStep::DragDecoration
                | ResolvedSurfaceProgramStep::InteractionFeedback
                | ResolvedSurfaceProgramStep::SurfaceEnd => {}
            }
            Ok(())
        })
        .map_err(|error| match error {
            SurfaceProgramVisitError::InvalidProgram => {
                SceneCompilationError::Scene(SceneBuildError::InvalidSurfaceProgram { surface })
            }
            SurfaceProgramVisitError::Sink(error) => error,
        })?;
    sampled
        .finish_surface_program()
        .map_err(SceneCompilationError::from)?;
    sampled
        .finish_presentation_sample()
        .map_err(SceneCompilationError::from)?;
    Ok(sampled)
}

#[cfg(test)]
mod tests;
