//! Framework focus sampling and exact pending-observation ownership.

mod pending_observations;
mod tab_intent;

pub(crate) use pending_observations::PendingFocusObservations;
pub(crate) use tab_intent::DeferredTabFocus;

#[cfg(test)]
#[path = "product_focus_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "product_focus_input_tests.rs"]
mod input_tests;

use dockspace::model::{DockspaceSurfaceView, ItemId};
use dockspace::runtime::{
    DockspaceHostFocus, DockspacePaneFocusObservation, DockspacePaneFocusRequest,
};
use egui::{Context, Id, Rect};

use crate::pane::{PaneFocusState, PaneView};

#[derive(Clone, Copy)]
struct FrameworkFocusRequest {
    request: DockspacePaneFocusRequest,
    applied: bool,
    observation: Option<DockspacePaneFocusObservation>,
    wake_frame: Option<u64>,
}

fn with_framework_focus<T>(
    context: &Context,
    instance: Id,
    viewport: egui::ViewportId,
    request: DockspacePaneFocusRequest,
    update: impl FnOnce(&mut FrameworkFocusRequest) -> T,
) -> T {
    let key = Id::unique((instance, "applied-pane-focus", request.surface(), viewport));
    context.data_mut(|data| {
        let state = data.get_temp_mut_or_insert_with(key, || FrameworkFocusRequest {
            request,
            applied: false,
            observation: None,
            wake_frame: None,
        });
        if state.request != request {
            *state = FrameworkFocusRequest {
                request,
                applied: false,
                observation: None,
                wake_frame: None,
            };
        }
        update(state)
    })
}

/// A framework focus side effect is applied once, independently of renderer acknowledgement.
/// The real focus observation still accompanies each exact output until core settles it.
pub(crate) fn apply_pane_focus_request(
    context: &Context,
    instance: Id,
    request: DockspacePaneFocusRequest,
    target: Id,
) {
    let applied =
        with_framework_focus(context, instance, context.viewport_id(), request, |state| {
            std::mem::replace(&mut state.applied, true)
        });
    if !applied {
        context.memory_mut(|memory| {
            if !memory.has_focus(target) {
                memory.request_focus(target);
            }
        });
    }
}

/// Associates an already handled tab press with its newly published core intent.
/// This records no renderer presentation or focus-observation acknowledgement.
pub(crate) fn mark_pane_focus_applied(
    context: &Context,
    instance: Id,
    viewport: egui::ViewportId,
    request: DockspacePaneFocusRequest,
) {
    with_framework_focus(context, instance, viewport, request, |state| {
        state.applied = true
    });
}

/// Repeated observations remain true facts without creating an idle repaint loop.
pub(crate) fn focus_observation_needs_repaint(
    context: &Context,
    instance: Id,
    request: DockspacePaneFocusRequest,
    observation: DockspacePaneFocusObservation,
) -> bool {
    let frame = context.cumulative_frame_nr();
    with_framework_focus(context, instance, context.viewport_id(), request, |state| {
        if state.observation != Some(observation) {
            state.observation = Some(observation);
            state.wake_frame = Some(frame);
        }
        state.wake_frame == Some(frame)
    })
}

#[derive(Default)]
struct PaneFocusEvidence {
    focused: Option<ItemId>,
    ambiguous: bool,
}

/// Samples actual focus in the current viewport without consulting focus history.
///
/// Call once in the matching viewport's input phase, using the current surface
/// before close/reset actions or encoding. The caller must validate the native
/// route and presentation scope; widget IDs alone do not establish that scope.
/// Submit pending exact acknowledgements, report this sampled fact, then submit
/// UI/application actions. This function never requests or consumes focus.
///
/// Exact-target claims require an enabled, visible, focusable widget in this
/// viewport's previous-pass registry. Composite `PaneView::focus_state` overrides
/// remain authoritative when they claim focus without that exact target.
/// Unknown pane states do not invent a claim or invalidate another pane's
/// uniquely proven claim. Multiple or unprovable positive claims stay Unknown.
/// `retained_focus` may contain only registrations owned by the exact output
/// currently presented at this viewport's input endpoint. It grants no focus
/// without the live memory focus and current pane ownership checks.
///
/// `workspace_id` must match the ID used by the surface renderer. `dock_bounds`
/// must be expressed in viewport-global coordinates and enclose the workspace.
/// Only a registered focused widget fully outside these bounds proves External;
/// unknown widgets within the workspace stay Unknown. Layer transforms are
/// applied before comparing widget bounds.
#[doc(hidden)]
pub fn sample_host_focus(
    context: &Context,
    surface: DockspaceSurfaceView<'_>,
    panes: &dyn PaneView,
    workspace_id: Id,
    dock_bounds: Rect,
    retained_focus: &[(ItemId, Id)],
) -> DockspaceHostFocus {
    if !dock_bounds.is_finite()
        || !dock_bounds.is_positive()
        || !context.input(|input| input.focused)
    {
        return DockspaceHostFocus::Unknown;
    }

    let focused_id = context.memory(|memory| memory.focused());
    let mut evidence = PaneFocusEvidence::default();
    for item in selected_panes(surface) {
        collect_pane_focus(
            item,
            context,
            panes,
            focused_id,
            retained_focus,
            &mut evidence,
        );
    }

    if evidence.ambiguous {
        return DockspaceHostFocus::Unknown;
    }

    if focused_id == Some(workspace_id) {
        if evidence.focused.is_some() {
            return DockspaceHostFocus::Unknown;
        }
        return registered_widget_bounds(context, workspace_id)
            .map_or(DockspaceHostFocus::Unknown, |_| {
                DockspaceHostFocus::Workspace
            });
    }

    if let Some(item) = evidence.focused {
        return DockspaceHostFocus::Pane(item);
    }

    if let Some(id) = focused_id {
        let Some(bounds) = registered_widget_bounds(context, id) else {
            return DockspaceHostFocus::Unknown;
        };
        return if !bounds.intersects(dock_bounds) {
            DockspaceHostFocus::External
        } else {
            DockspaceHostFocus::Unknown
        };
    }

    DockspaceHostFocus::None
}

/// Keeps an already focused, exactly presented target alive during a skipped UI.
///
/// This registers no hit receiver and makes no focus request. Call only when
/// retaining that output without invoking its pane callbacks in this pass.
#[doc(hidden)]
pub fn retain_presented_pane_focus(context: &Context, retained_focus: &[(ItemId, Id)]) {
    let Some(id) = context.memory(|memory| memory.focused()) else {
        return;
    };
    if retained_focus.iter().any(|(_, target)| *target == id) {
        // egui's focus dead-man switch uses state IDs, independently of widget
        // hit registration. The original output still owns this state identity.
        context.check_for_id_clash(id, Rect::ZERO, "retained dockspace pane focus");
    }
}

/// Keeps a selected pane's existing focus identity alive while its UI is skipped.
///
/// The caller must validate the current surface's viewport and native lifetime.
/// This neither requests focus nor supplies presentation evidence to focus sampling.
#[doc(hidden)]
#[cfg(any(test, feature = "native-render-support"))]
pub fn retain_skipped_pane_focus(
    context: &Context,
    surface: DockspaceSurfaceView<'_>,
    panes: &dyn PaneView,
) {
    let Some(id) = context.memory(|memory| memory.focused()) else {
        return;
    };
    if selected_panes(surface).any(|item| panes.focus_target(item) == Some(id)) {
        context.check_for_id_clash(id, Rect::ZERO, "skipped dockspace pane focus");
    }
}

fn selected_panes(surface: DockspaceSurfaceView<'_>) -> impl Iterator<Item = ItemId> + '_ {
    let mut pending = surface
        .main_root()
        .into_iter()
        .chain(surface.contained().filter_map(|contained| contained.root()))
        .filter_map(|root| root.content())
        .collect::<Vec<_>>();
    std::iter::from_fn(move || {
        while let Some(node) = pending.pop() {
            if let Some(tabs) = node.tabs() {
                if let Some(item) = tabs.selected() {
                    return Some(item);
                }
            } else if let Some(split) = node.split() {
                pending.extend(split.children());
            }
        }
        None
    })
}

fn collect_pane_focus(
    item: ItemId,
    context: &Context,
    panes: &dyn PaneView,
    focused_id: Option<Id>,
    retained_focus: &[(ItemId, Id)],
    evidence: &mut PaneFocusEvidence,
) {
    if panes.focus_state(item, context) == PaneFocusState::Focused {
        if panes.focus_target(item).is_some_and(|target| {
            focused_id == Some(target)
                && registered_widget_bounds(context, target).is_none()
                && !retained_focus.contains(&(item, target))
        }) {
            evidence.ambiguous = true;
        } else if evidence.focused.replace(item).is_some() {
            evidence.ambiguous = true;
        }
    }
}

fn registered_widget_bounds(context: &Context, id: Id) -> Option<Rect> {
    let widget = context.viewport(|viewport| viewport.prev_pass.widgets.get(id).copied())?;
    if !widget.enabled
        || !widget.visible
        || !widget.sense.is_focusable()
        || !widget.rect.is_finite()
        || !widget.rect.is_positive()
    {
        return None;
    }

    let bounds = context
        .layer_transform_to_global(widget.layer_id)
        .map_or(widget.rect, |transform| transform * widget.rect);
    (bounds.is_finite() && bounds.is_positive()).then_some(bounds)
}
