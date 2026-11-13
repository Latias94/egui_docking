//! Seeded pointer journeys through the workbench's public host boundary.

use std::collections::{BTreeSet, VecDeque};
use std::panic::{AssertUnwindSafe, catch_unwind};

use egui::accesskit::Role;
use egui_dockspace::PaneView;

use super::*;

struct Replay {
    context: egui::Context,
    dock: Dockspace,
    panes: DemoPanes,
    output: egui::FullOutput,
    random: u64,
    step: usize,
    recent: VecDeque<String>,
    splits: usize,
    merges: usize,
    floats: usize,
}

impl Replay {
    fn new(seed: u64) -> Self {
        let context = editor::new_context();
        context.enable_accesskit();
        let mut dock = Dockspace::builder(("workbench-pointer-replay", seed), demo_layout())
            .style(editor::dock_style())
            .build()
            .unwrap();
        let mut panes = DemoPanes::new();
        let _ = workbench_frame(&context, &mut dock, &mut panes, Vec::new());
        let output = workbench_frame(&context, &mut dock, &mut panes, Vec::new());
        Self {
            context,
            dock,
            panes,
            output,
            random: seed,
            step: 0,
            recent: VecDeque::new(),
            splits: 0,
            merges: 0,
            floats: 0,
        }
    }

    fn frame(&mut self, events: Vec<Event>) {
        self.output = workbench_frame(&self.context, &mut self.dock, &mut self.panes, events);
    }

    fn note(&mut self, operation: String) {
        if self.recent.len() == 8 {
            self.recent.pop_front();
        }
        self.recent.push_back(format!("{}: {operation}", self.step));
    }

    fn pick<T: Copy>(&mut self, choices: &[T]) -> Option<T> {
        self.random = self
            .random
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let index = (self.random % choices.len().max(1) as u64) as usize;
        choices.get(index).copied()
    }

    fn semantic_rects(&self, role: Role, label: &str) -> Vec<Rect> {
        self.output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes
            .iter()
            .filter_map(|(_, node)| {
                if node.role() != role || node.label() != Some(label) || node.is_disabled() {
                    return None;
                }
                let bounds = node.bounds()?;
                let rect = Rect::from_min_max(
                    egui::pos2(bounds.x0 as f32, bounds.y0 as f32),
                    egui::pos2(bounds.x1 as f32, bounds.y1 as f32),
                );
                (rect.width() > 8.0 && rect.height() > 8.0).then_some(rect)
            })
            .collect()
    }

    fn controls(&self, role: Role, label: &str) -> Vec<(egui::Id, Rect)> {
        let nodes = &self
            .output
            .platform_output
            .accesskit_update
            .as_ref()
            .unwrap()
            .nodes;
        self.context.viewport(|viewport| {
            nodes
                .iter()
                .filter(|(_, node)| {
                    node.role() == role && node.label() == Some(label) && !node.is_disabled()
                })
                .filter_map(|(id, _)| {
                    viewport
                        .prev_pass
                        .widgets
                        .layers()
                        .flat_map(|(_, widgets)| widgets)
                        .find(|widget| {
                            widget.id.accesskit_id() == *id
                                && widget.enabled
                                && widget.visible
                                && widget.interact_rect.is_positive()
                        })
                        .map(|widget| (widget.id, widget.interact_rect))
                })
                .collect()
        })
    }

    fn source(&self, item: egui_dockspace::ItemId) -> Option<(egui::Id, Pos2)> {
        let role = if self.dock.view().item(item)?.contained().is_some() {
            Role::TitleBar
        } else {
            Role::Tab
        };
        let title = self.panes.title(item)?;
        self.controls(role, title.text())
            .into_iter()
            .find(|(_, bounds)| bounds.width() > 8.0 && bounds.height() > 8.0)
            .map(|(widget, bounds)| (widget, bounds.center()))
    }

    fn main_items(&self) -> Vec<egui_dockspace::ItemId> {
        self.panes
            .panes
            .keys()
            .copied()
            .filter(|item| {
                self.dock
                    .view()
                    .item(*item)
                    .is_some_and(|view| view.contained().is_none())
                    && self.source(*item).is_some()
            })
            .collect()
    }

    fn exposed_source(&mut self, item: egui_dockspace::ItemId) -> Option<Pos2> {
        let mut exposed = None;
        for _ in 0..16 {
            let Some((widget, source)) = self.source(item) else {
                self.frame(Vec::new());
                continue;
            };
            self.frame(vec![Event::PointerMoved(source)]);
            if self
                .context
                .viewport(|viewport| viewport.hits.drag.is_some_and(|hit| hit.id == widget))
            {
                exposed = self.source(item).map(|(_, point)| point);
                break;
            }
        }
        exposed
    }

    fn begin_drag(&mut self, item: egui_dockspace::ItemId, modifiers: Modifiers) -> bool {
        let Some(source) = self.exposed_source(item) else {
            self.note(format!("begin {item}: no exposed source after reflow"));
            return false;
        };
        self.frame(vec![
            Event::ModifiersChanged(modifiers),
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers,
            },
        ]);
        self.frame(vec![Event::PointerMoved(source + egui::vec2(20.0, 8.0))]);
        self.note(format!(
            "begin {item} at {source:?}: active={}, selected={}, group={:?}",
            self.dock.has_active_gesture(),
            self.dock.view().item(item).unwrap().is_selected(),
            self.dock.view().item(item).unwrap().tabs().items(),
        ));
        let active = self.dock.has_active_gesture();
        if !active {
            self.frame(vec![button(source, false)]);
        }
        active
    }

    fn escape(&mut self) {
        self.frame(vec![Event::Key {
            key: egui::Key::Escape,
            physical_key: Some(egui::Key::Escape),
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        }]);
        self.frame(vec![
            button(egui::pos2(640.0, 410.0), false),
            Event::Key {
                key: egui::Key::Escape,
                physical_key: Some(egui::Key::Escape),
                pressed: false,
                repeat: false,
                modifiers: Modifiers::NONE,
            },
        ]);
        assert!(
            !self.dock.has_active_gesture(),
            "Escape followed by release must leave the docking gesture idle",
        );
    }

    fn dock_at(
        &mut self,
        item: egui_dockspace::ItemId,
        target: egui_dockspace::ItemId,
        label: &str,
    ) -> bool {
        if !self.begin_drag(item, Modifiers::NONE) {
            return false;
        }
        let selected = self
            .dock
            .view()
            .item(target)
            .unwrap()
            .tabs()
            .selected()
            .unwrap();
        let mut destination = self.panes.panes[&selected].content.unwrap().center();
        let mut guide = None;
        for _ in 0..16 {
            destination = self.panes.panes[&selected].content.unwrap().center();
            self.frame(vec![Event::PointerMoved(destination)]);
            guide = self
                .semantic_rects(Role::Label, label)
                .into_iter()
                .min_by(|a, b| {
                    a.center()
                        .distance_sq(destination)
                        .total_cmp(&b.center().distance_sq(destination))
                });
            if guide.is_some() {
                break;
            }
        }
        let Some(guide) = guide else {
            self.note(format!(
                "no {label} for {item}->{target} at {destination:?}: active={}, selected target={selected}",
                self.dock.has_active_gesture(),
            ));
            self.escape();
            return false;
        };
        let point = guide.center();
        self.frame(vec![Event::PointerMoved(point), button(point, false)]);
        true
    }

    fn split(&mut self) -> bool {
        let items = self.main_items();
        let mut choices = Vec::new();
        for &item in &items {
            let source = self.dock.view().item(item).unwrap();
            for &target in &items {
                let destination = self.dock.view().item(target).unwrap();
                if source.tabs().items().len() > 1
                    && destination.is_selected()
                    && !source.tabs().items().contains(&target)
                {
                    choices.push((item, target));
                }
            }
        }
        let Some((item, target)) = self.pick(&choices) else {
            return false;
        };
        let direction = self.pick(&["left", "right", "top", "bottom"]).unwrap();
        self.note(format!("split {item} from {target} at {direction}"));
        let label = format!("Dockspace {direction} drop guide");
        let groups = self.group_count();
        let operated = self.dock_at(item, target, &label);
        let separated = self.group_count() > groups;
        self.splits += usize::from(operated && separated);
        operated && separated
    }

    fn merge(&mut self) -> bool {
        let items = self.main_items();
        let mut choices = Vec::new();
        for &item in &items {
            for &target in &items {
                let source = self.dock.view().item(item).unwrap();
                let destination = self.dock.view().item(target).unwrap();
                if destination.is_selected() && !source.tabs().items().contains(&target) {
                    choices.push((item, target));
                }
            }
        }
        let Some((item, target)) = self.pick(&choices) else {
            return false;
        };
        self.note(format!("merge {item} into {target}"));
        let operated = self.dock_at(item, target, "Dockspace center drop guide");
        let merged = self
            .dock
            .view()
            .item(item)
            .unwrap()
            .tabs()
            .items()
            .contains(&target);
        self.merges += usize::from(operated && merged);
        operated && merged
    }

    fn float(&mut self) -> bool {
        let items = self.main_items();
        let Some(item) = self.pick(&items) else {
            return false;
        };
        self.note(format!("Shift-drag {item} to float"));
        if !self.begin_drag(item, Modifiers::SHIFT) {
            return false;
        }
        let body = self.panes.panes[&item].content.unwrap();
        let point = body.min + body.size() * 0.65;
        self.frame(vec![Event::PointerMoved(point)]);
        self.frame(vec![Event::PointerButton {
            pos: point,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::SHIFT,
        }]);
        self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        let floated = self.dock.view().item(item).unwrap().contained().is_some();
        self.floats += usize::from(floated);
        floated
    }

    fn redock(&mut self) -> bool {
        let floating: Vec<_> = self
            .panes
            .panes
            .keys()
            .copied()
            .filter(|item| self.dock.view().item(*item).unwrap().contained().is_some())
            .collect();
        let Some(item) = self.pick(&floating) else {
            return false;
        };
        let destinations: Vec<_> = self
            .main_items()
            .into_iter()
            .filter(|target| self.dock.view().item(*target).unwrap().is_selected())
            .collect();
        let Some(target) = self.pick(&destinations) else {
            return false;
        };
        self.note(format!("redock floating {item} into {target}"));
        self.dock_at(item, target, "Dockspace center drop guide")
            && self.dock.view().item(item).unwrap().contained().is_none()
    }

    fn cancel_and_recover(&mut self, outside: bool) {
        let items = self.main_items();
        let item = self.pick(&items).expect("there is an available main tab");
        self.note(format!(
            "drag {item}; {}",
            if outside { "release outside" } else { "Escape" }
        ));
        assert!(self.begin_drag(item, Modifiers::NONE));
        if outside {
            let point = egui::pos2(-80.0, -80.0);
            self.frame(vec![Event::PointerMoved(point), Event::PointerGone]);
            self.frame(vec![button(point, false), Event::PointerGone]);
            self.frame(vec![Event::PointerMoved(egui::pos2(640.0, 410.0))]);
        } else {
            self.escape();
        }
        self.assert_items();
        assert!(
            self.redock() || self.merge() || self.split(),
            "a completed/cancelled outside gesture must allow another actual docking operation",
        );
    }

    fn resize(&mut self, junction: bool) {
        let (role, label) = if junction {
            (Role::Group, "Resize pane grid")
        } else {
            (Role::Splitter, "Resize panes")
        };
        let mut control = None;
        for _ in 0..16 {
            let choices = self.controls(role, label);
            if let Some((widget, bounds)) = self.pick(&choices) {
                let point = bounds.center();
                self.frame(vec![Event::PointerMoved(point)]);
                if self
                    .context
                    .viewport(|viewport| viewport.hits.drag.is_some_and(|hit| hit.id == widget))
                {
                    control = Some(bounds);
                    break;
                }
            } else {
                self.frame(Vec::new());
            }
        }
        let Some(bounds) = control else {
            self.note(format!("no exposed {label}"));
            return;
        };
        let point = bounds.center();
        let distance = self.pick(&[-36.0, -20.0, 20.0, 36.0]).unwrap();
        let delta = if junction {
            egui::vec2(distance, -distance * 0.75)
        } else if bounds.width() < bounds.height() {
            egui::vec2(distance, 0.0)
        } else {
            egui::vec2(0.0, distance)
        };
        self.note(format!("drag {label} at {point:?} by {delta:?}"));
        self.frame(vec![Event::PointerMoved(point), button(point, true)]);
        assert!(
            self.dock.has_active_gesture(),
            "the exposed resize control must acquire its gesture"
        );
        self.frame(vec![Event::PointerMoved(point + delta)]);
        self.frame(vec![button(point + delta, false)]);
        assert!(
            !self.dock.has_active_gesture(),
            "releasing a divider must finish direct manipulation"
        );
    }

    fn move_floating(&mut self) {
        if !self
            .panes
            .panes
            .keys()
            .any(|item| self.dock.view().item(*item).unwrap().contained().is_some())
        {
            assert!(
                self.float(),
                "create a floating group before its direct move"
            );
        }
        let items: Vec<_> = self
            .panes
            .panes
            .keys()
            .copied()
            .filter(|item| {
                self.dock.view().item(*item).unwrap().contained().is_some()
                    && self.source(*item).is_some()
            })
            .collect();
        let item = self.pick(&items).unwrap();
        let id = self.dock.view().item(item).unwrap().contained().unwrap();
        let before = self.dock.view().contained(id).unwrap().rect();
        let source = self
            .exposed_source(item)
            .expect("the floating title must be the actual exposed drag receiver");
        let center = self.context.content_rect().center();
        let distance = self.pick(&[24.0, 36.0, 48.0]).unwrap();
        // Random floating placement can end against either viewport edge.
        // Drive toward the interior so this checks movement rather than a
        // legitimate clamp of an outward request at the current boundary.
        let delta = egui::vec2(
            if before.x() + before.width() * 0.5 > f64::from(center.x) {
                -distance
            } else {
                distance
            },
            if before.y() + before.height() * 0.5 > f64::from(center.y) {
                -distance
            } else {
                distance
            },
        );
        let point = source + delta;
        self.note(format!("move floating {item} by {delta:?}"));
        self.frame(vec![
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::SHIFT,
            },
        ]);
        assert!(
            self.dock.has_active_gesture(),
            "the exposed floating title must acquire its own move gesture",
        );
        self.frame(vec![Event::PointerMoved(point)]);
        self.frame(vec![Event::PointerButton {
            pos: point,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: Modifiers::SHIFT,
        }]);
        self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        let after = self.dock.view().contained(id).unwrap().rect();
        assert_ne!(
            after, before,
            "the floating title gesture must change its durable bounds"
        );
        assert!(
            (after.width() - before.width()).abs() <= 1e-9
                && (after.height() - before.height()).abs() <= 1e-9,
            "moving a floating title must preserve its extent: {before:?} -> {after:?}",
        );
        assert!(!self.dock.has_active_gesture());
    }

    fn toggle_docking_and_cancel(&mut self) {
        let items = self.main_items();
        let item = self.pick(&items).unwrap();
        assert!(self.begin_drag(item, Modifiers::NONE));
        let point = self
            .context
            .input(|input| input.pointer.interact_pos())
            .unwrap();
        self.note(format!("toggle Shift during {item}'s active drag"));
        self.frame(vec![
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(point + egui::vec2(24.0, 28.0)),
        ]);
        assert!(self.dock.has_active_gesture());
        self.frame(vec![
            Event::ModifiersChanged(Modifiers::NONE),
            Event::PointerMoved(point + egui::vec2(32.0, 36.0)),
        ]);
        assert!(self.dock.has_active_gesture());
        self.escape();
        assert!(
            self.merge() || self.redock() || self.split(),
            "restoring docking intent must leave the next docking gesture usable"
        );
    }

    fn focus_loss_and_late_release(&mut self) {
        let items = self.main_items();
        let item = self.pick(&items).unwrap();
        assert!(self.begin_drag(item, Modifiers::NONE));
        let version = self.dock.version();
        self.note(format!(
            "focus lost during {item}; reenter with a late release"
        ));
        self.frame(vec![Event::WindowFocused(false), Event::PointerGone]);
        assert!(!self.dock.has_active_gesture());
        self.frame(vec![
            Event::WindowFocused(true),
            button(egui::pos2(-80.0, -80.0), false),
        ]);
        assert_eq!(self.dock.version(), version);
        assert!(
            self.merge() || self.redock() || self.split(),
            "focus recovery must permit another actual docking operation"
        );
    }

    fn press_move_release_one_packet(&mut self) {
        let choices = self.main_items();
        let item = self.pick(&choices).unwrap();
        let source = self
            .exposed_source(item)
            .expect("the selected packet source becomes exposed after reflow");
        let target = egui::pos2(640.0 + self.pick(&[-60.0, 60.0]).unwrap(), 360.0);
        self.note(format!(
            "same-packet press/move/release {item} to {target:?}"
        ));
        self.frame(vec![
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(source),
            Event::PointerButton {
                pos: source,
                button: PointerButton::Primary,
                pressed: true,
                modifiers: Modifiers::SHIFT,
            },
            Event::PointerMoved(target),
            Event::PointerButton {
                pos: target,
                button: PointerButton::Primary,
                pressed: false,
                modifiers: Modifiers::SHIFT,
            },
        ]);
        self.frame(vec![Event::ModifiersChanged(Modifiers::NONE)]);
        assert!(!self.dock.has_active_gesture());
        assert!(
            self.dock.view().item(item).unwrap().contained().is_some(),
            "one actual packet must preserve its down, movement and terminal"
        );
    }

    fn restore(&mut self) {
        self.note("click Restore layout".into());
        let point = self.semantic_rects(Role::Button, "Restore layout")[0].center();
        self.frame(vec![Event::PointerMoved(point), button(point, true)]);
        self.frame(vec![Event::PointerMoved(point), button(point, false)]);
        self.frame(Vec::new());
        self.frame(Vec::new());
        assert!(
            self.redock(),
            "the restored floating Inspector must dock normally"
        );
    }

    fn assert_items(&self) {
        let mut actual = BTreeSet::new();
        for surface in self.dock.view().surfaces() {
            let mut roots = surface.main_root().into_iter().collect::<Vec<_>>();
            roots.extend(surface.contained().filter_map(|contained| contained.root()));
            for root in roots {
                let mut pending = vec![root.content().unwrap()];
                while let Some(node) = pending.pop() {
                    if let Some(tabs) = node.tabs() {
                        assert!(
                            !tabs.items().is_empty(),
                            "the workbench must not retain an unlabelled empty group"
                        );
                        for &item in tabs.items() {
                            assert!(actual.insert(item), "one item cannot occupy two groups");
                        }
                    } else if let Some(split) = node.split() {
                        pending.extend(split.children());
                    }
                }
            }
        }
        assert_eq!(actual, self.panes.panes.keys().copied().collect());
    }

    fn group_count(&self) -> usize {
        let mut count = 0;
        for surface in self.dock.view().surfaces() {
            let mut roots = surface.main_root().into_iter().collect::<Vec<_>>();
            roots.extend(surface.contained().filter_map(|contained| contained.root()));
            for root in roots {
                let mut pending = vec![root.content().unwrap()];
                while let Some(node) = pending.pop() {
                    if node.tabs().is_some() {
                        count += 1;
                    } else if let Some(split) = node.split() {
                        pending.extend(split.children());
                    }
                }
            }
        }
        count
    }

    fn run(&mut self) {
        for step in 0..30 {
            self.step = step;
            match step % 10 {
                0 => self.restore(),
                1 | 5 => {
                    self.split();
                }
                2 | 9 => {
                    self.merge();
                }
                3 => {
                    self.float();
                }
                4 => {
                    self.redock();
                }
                6 => self.cancel_and_recover(false),
                7 => self.cancel_and_recover(true),
                _ => {
                    if self.pick(&[false, true]).unwrap() {
                        self.split();
                    } else {
                        self.merge();
                    }
                }
            }
            self.assert_items();
        }
        for action in [0, 1, 2, 3, 4, 5] {
            self.step = 30 + action;
            match action {
                0 => self.resize(false),
                1 => self.resize(true),
                2 => self.move_floating(),
                3 => self.toggle_docking_and_cancel(),
                4 => self.focus_loss_and_late_release(),
                _ => self.press_move_release_one_packet(),
            }
            self.assert_items();
        }
        assert!(self.splits > 0, "the replay must commit an actual split");
        assert!(self.merges > 0, "the replay must commit an actual merge");
        assert!(self.floats > 0, "the replay must commit an actual float");
    }
}

#[test]
fn a_fractional_floating_title_move_keeps_its_canonical_target() {
    use egui_dockspace::{
        DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, FloatingPresentationId, LogicalRect, RootId,
    };

    let context = editor::new_context();
    context.enable_accesskit();
    let floating = FloatingPresentationId::new(1);
    let source = LogicalRect::new(
        71.890_571_594_238_28,
        296.400_008_916_854_86,
        246.163_301_463_831_52 - 71.890_571_594_238_28,
        820.0 - 296.400_008_916_854_86,
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([EDITOR])),
    )
    .with_contained(DockspaceContainedLayout::new(
        floating,
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([INSPECTOR])),
        source,
    ))])
    .unwrap();
    let mut dock = Dockspace::builder("fractional-floating-title", layout)
        .style(editor::dock_style())
        .build()
        .unwrap();
    let mut panes = DemoPanes::new();
    let _ = workbench_frame(&context, &mut dock, &mut panes, Vec::new());
    let output = workbench_frame(&context, &mut dock, &mut panes, Vec::new());
    let title = semantic_rect(&output, Role::TitleBar, "Inspector").center();
    let pressed = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::SHIFT,
    };
    let _ = workbench_frame(
        &context,
        &mut dock,
        &mut panes,
        vec![
            Event::ModifiersChanged(Modifiers::SHIFT),
            Event::PointerMoved(title),
            pressed(title, true),
        ],
    );
    let target = title + egui::vec2(24.0, -24.0);
    let _ = workbench_frame(
        &context,
        &mut dock,
        &mut panes,
        vec![Event::PointerMoved(target)],
    );
    let _ = workbench_frame(
        &context,
        &mut dock,
        &mut panes,
        vec![pressed(target, false)],
    );
    let expected = LogicalRect::new(
        source.x() + 24.0,
        source.y() - 24.0,
        source.width(),
        source.height(),
    )
    .unwrap();
    assert_eq!(
        dock.view().contained(floating).unwrap().rect(),
        expected,
        "translation must accept the same canonical rectangle even when subtracting its shifted corners rounds the extent",
    );
    assert!(!dock.has_active_gesture());
}

#[test]
fn seeded_pointer_journeys_preserve_items_and_recover_after_cancel_and_outside_release() {
    let seeds = std::env::var("DOCKSPACE_TEST_SEED").map_or_else(
        |_| {
            vec![
                1,
                7,
                42,
                0x1a17_bfa,
                0x5eed,
                0xd0c5,
                0x1234_5678,
                0xabcd_ef01,
            ]
        },
        |seed| {
            let value = if let Some(hex) = seed.strip_prefix("0x") {
                u64::from_str_radix(hex, 16)
            } else {
                seed.parse()
            }
            .expect("DOCKSPACE_TEST_SEED must be a decimal or 0x-prefixed u64");
            vec![value]
        },
    );
    for seed in seeds {
        let mut replay = Replay::new(seed);
        let result = catch_unwind(AssertUnwindSafe(|| replay.run()));
        if let Err(cause) = result {
            let message = cause
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| cause.downcast_ref::<&str>().copied())
                .unwrap_or("non-string panic");
            panic!(
                "pointer replay failed: seed={seed:#x}, step={}, recent={:?}: {message}",
                replay.step, replay.recent
            );
        }
    }
}
