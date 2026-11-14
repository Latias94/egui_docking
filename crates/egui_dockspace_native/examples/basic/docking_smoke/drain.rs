//! Drain the real contained root, then resize both surviving splitters through input.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dockspace::model::{DockspaceAxis, ItemId};
use eframe::egui;
use egui_dockspace::DockStyle;

use super::super::{
    ASSETS, CONSOLE, EDITOR, HISTORY, INSPECTOR, INSPECTOR_FLOATING, INSPECTOR_ROOT, MAIN_ROOT,
    NativeExampleApp, OUTLINE, PREVIEW, PROFILER, SURFACE,
};
use super::{EditorObservations, MergeSmoke};

pub(super) struct DrainSmoke {
    started: Instant,
    order: [ItemId; 2],
    phase: Phase,
    original_text: Option<BTreeMap<ItemId, String>>,
}

enum Phase {
    First(MergeSmoke),
    Second(MergeSmoke),
    LocateSplitter(DockspaceAxis),
    AwaitResize {
        axis: DockspaceAxis,
        before: [egui::Rect; 2],
    },
    Complete,
}

impl DrainSmoke {
    pub(super) fn new(order: [ItemId; 2]) -> Self {
        Self {
            started: Instant::now(),
            order,
            phase: Phase::First(MergeSmoke::new(order[0], OUTLINE)),
            original_text: None,
        }
    }

    pub(super) fn advance(
        &mut self,
        app: &NativeExampleApp,
        context: &egui::Context,
        tabs: &Arc<Mutex<EditorObservations>>,
    ) -> bool {
        assert!(
            self.started.elapsed() < Duration::from_secs(35),
            "drain/resize journey timed out; pending={:?}; outputs={:?}",
            app.dockspace.pending_work(),
            app.dockspace.output_backlog(),
        );
        assert!(app.dockspace.error_kind().is_none());
        self.original_text.get_or_insert_with(|| text(app));
        match &mut self.phase {
            Phase::First(smoke) => {
                if smoke.advance(app, context, tabs) {
                    assert!(
                        tabs.lock().unwrap().pressed_reentries > smoke.reentries_before,
                        "the first drag must leave and re-enter the window while primary is held"
                    );
                    app.dockspace.with_view(|view| {
                        let remaining = view.item(self.order[1]).unwrap();
                        assert_eq!(remaining.root(), INSPECTOR_ROOT);
                        assert_eq!(remaining.contained(), Some(INSPECTOR_FLOATING));
                        assert_eq!(remaining.tabs().items(), [self.order[1]]);
                    });
                    self.phase = Phase::Second(MergeSmoke::new(self.order[1], OUTLINE));
                }
            }
            Phase::Second(smoke) => {
                if smoke.advance(app, context, tabs) {
                    assert_drained(app, self.order);
                    self.phase = Phase::LocateSplitter(DockspaceAxis::Horizontal);
                }
            }
            Phase::LocateSplitter(axis) => {
                if ready(app, tabs)
                    && let Some(before) = pane_bounds(app, *axis)
                {
                    let hit = splitter_bounds(&tabs.lock().unwrap(), *axis);
                    let scale = context.pixels_per_point();
                    let (source, target) = resize_points(hit, *axis, scale);
                    let token = eframe::current_native_output_token().unwrap();
                    eprintln!(
                        "editor-docking-resize-ready: {} {} {} {} {}",
                        u64::from(token.window_id()),
                        source.x,
                        source.y,
                        target.x,
                        target.y,
                    );
                    self.phase = Phase::AwaitResize {
                        axis: *axis,
                        before,
                    };
                }
            }
            Phase::AwaitResize { axis, before } => {
                if ready(app, tabs)
                    && let Some(after) = pane_bounds(app, *axis)
                    && resize_completed(*axis, *before, after)
                {
                    assert_drained(app, self.order);
                    eprintln!(
                        "editor-docking-resized: {axis:?}; before={before:?}; after={after:?}"
                    );
                    self.phase = match axis {
                        DockspaceAxis::Horizontal => Phase::LocateSplitter(DockspaceAxis::Vertical),
                        DockspaceAxis::Vertical => Phase::Complete,
                    };
                }
            }
            Phase::Complete if ready(app, tabs) => {
                let current = text(app);
                assert_eq!(Some(&current), self.original_text.as_ref());
                assert_eq!(current.len(), 8);
                return true;
            }
            Phase::Complete => {}
        }
        false
    }
}

fn text(app: &NativeExampleApp) -> BTreeMap<ItemId, String> {
    app.dockspace.with_panes(|panes| {
        panes
            .panes
            .iter()
            .map(|(&item, pane)| (item, pane.text.clone()))
            .collect()
    })
}

fn ready(app: &NativeExampleApp, tabs: &Arc<Mutex<EditorObservations>>) -> bool {
    app.dockspace.is_surface_presented(SURFACE)
        && app.dockspace.is_quiescent()
        && !app.dockspace.current_output_is_retained()
        && !tabs.lock().unwrap().primary_down
}

fn splitter_bounds(observations: &EditorObservations, axis: DockspaceAxis) -> egui::Rect {
    let orientation = match axis {
        DockspaceAxis::Horizontal => egui::accesskit::Orientation::Vertical,
        DockspaceAxis::Vertical => egui::accesskit::Orientation::Horizontal,
    };
    let mut candidates = observations
        .splitters
        .iter()
        .filter(|(direction, _)| *direction == orientation);
    let (_, hit) = candidates.next().expect("one ordinary splitter per axis");
    assert!(
        candidates.next().is_none(),
        "ordinary splitter must be unique"
    );
    *hit
}

fn assert_drained(app: &NativeExampleApp, order: [ItemId; 2]) {
    app.dockspace.with_view(|view| {
        assert!(view.contained(INSPECTOR_FLOATING).is_none());
        assert!(view.root(INSPECTOR_ROOT).is_none());
        assert_eq!(view.surfaces().count(), 1);
        for item in [
            OUTLINE, EDITOR, PREVIEW, INSPECTOR, CONSOLE, HISTORY, PROFILER, ASSETS,
        ] {
            let pane = view.item(item).expect("every original item must survive");
            assert_eq!(pane.surface(), SURFACE);
            assert_eq!(pane.root(), MAIN_ROOT);
            assert!(pane.contained().is_none());
        }
        assert_eq!(
            view.item(OUTLINE).unwrap().tabs().items(),
            [OUTLINE, ASSETS, order[0], order[1]]
        );
    });
}

fn pane_bounds(app: &NativeExampleApp, axis: DockspaceAxis) -> Option<[egui::Rect; 2]> {
    let items = match axis {
        // The merged left group keeps the second dragged tab selected.
        DockspaceAxis::Horizontal => {
            let selected = app
                .dockspace
                .with_view(|view| view.item(OUTLINE).unwrap().tabs().selected())?;
            [selected, EDITOR]
        }
        DockspaceAxis::Vertical => [EDITOR, CONSOLE],
    };
    app.dockspace.with_panes(|panes| {
        Some([
            panes.panes[&items[0]].content?.0,
            panes.panes[&items[1]].content?.0,
        ])
    })
}

fn resize_points(hit: egui::Rect, axis: DockspaceAxis, scale: f32) -> (egui::Pos2, egui::Pos2) {
    let mut source = hit.min + hit.size() * 0.35;
    let mut delta = egui::vec2(0.0, 0.0);
    match axis {
        DockspaceAxis::Horizontal => {
            source.x = hit.left() + 1.0 / scale;
            delta.x = 48.0;
        }
        DockspaceAxis::Vertical => {
            source.y = hit.top() + 1.0 / scale;
            delta.y = 48.0;
        }
    }
    // Round exactly as the external physical-input driver does. The start must
    // remain inside the hit padding, outside the one-point visible divider.
    let source = (source * scale).round();
    let logical = source / scale;
    assert!(hit.contains(logical));
    let distance = match axis {
        DockspaceAxis::Horizontal => (logical.x - hit.center().x).abs(),
        DockspaceAxis::Vertical => (logical.y - hit.center().y).abs(),
    };
    assert!(distance > DockStyle::default().splitter_thickness * 0.5);
    (source, ((logical + delta) * scale).round())
}

fn resize_completed(axis: DockspaceAxis, before: [egui::Rect; 2], after: [egui::Rect; 2]) -> bool {
    match axis {
        DockspaceAxis::Horizontal => {
            after[0].width() > before[0].width() + 20.0
                && after[1].left() > before[1].left() + 20.0
                && after[1].width() < before[1].width() - 20.0
        }
        DockspaceAxis::Vertical => {
            after[0].height() > before[0].height() + 20.0
                && after[1].top() > before[1].top() + 20.0
                && after[1].height() < before[1].height() - 20.0
        }
    }
}

#[cfg(test)]
mod tests;
