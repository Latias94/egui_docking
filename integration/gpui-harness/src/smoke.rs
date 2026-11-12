//! Bounded framework input on real native windows and real renderer submissions.

use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
    time::{Duration, Instant},
};

use dockspace::{geometry::LogicalRect, runtime::DockspaceReceiverRole};
use open_gpui::{
    App, Bounds, Entity, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    PlatformInput, PlatformWindowPresentOutcome, Point, Window, point, px,
};

use crate::{CounterPanel, HostState, ItemId, SharedHost, input};

const TIMEOUT: Duration = Duration::from_secs(20);
const EDITOR: ItemId = ItemId::new(1);
const PREVIEW: ItemId = ItemId::new(2);
const TOOLS: ItemId = ItemId::new(4);
const VISIBLE_PANES: [ItemId; 3] = [EDITOR, ItemId::new(3), ItemId::new(4)];

pub(super) type ButtonProbe = crate::host::BoundsProbe;

#[derive(Clone, Default)]
pub(super) struct Smoke {
    complete: Rc<Cell<bool>>,
    state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct State {
    step: Step,
    submitted_before_input: u64,
    editor: Option<PanelProbe>,
    preview: Option<PanelProbe>,
    surface: ButtonProbe,
    split: Option<SplitProbe>,
    floating: Option<FloatProbe>,
}

#[derive(Clone, Copy, Debug, Default)]
enum Step {
    #[default]
    FirstFrame,
    EditorClicked,
    PreviewSelected,
    PreviewClicked,
    EditorRestored,
    SplitterPressed,
    SplitterMoved,
    SplitterReleased,
    FloatPressed,
    FloatMoving,
    FloatMoved,
    FloatReleased,
}

struct PanelProbe {
    entity: Entity<CounterPanel>,
    button: ButtonProbe,
}

#[derive(Clone, Copy)]
struct SplitProbe {
    end: Point<Pixels>,
    width_before: f64,
    distance: f64,
}

struct FloatProbe {
    start: Point<Pixels>,
    pane_before: LogicalRect,
    moves: usize,
    pending_moves: usize,
    last_input: Instant,
}

impl FloatProbe {
    fn position(&self, index: usize) -> Point<Pixels> {
        self.start + point(px(-120.0), px(40.0)) * (index as f32 / 24.0)
    }
}

impl Smoke {
    pub(super) fn install(&self, host: &SharedHost, window: &Window, cx: &mut App) {
        if cx.is_test() {
            self.fail("--smoke requires the real platform, not GPUI's CPU test platform");
        }
        {
            let mut state = self.state.borrow_mut();
            state.editor = Some(self.install_panel(host, EDITOR, cx));
            state.preview = Some(self.install_panel(host, PREVIEW, cx));
            host.borrow_mut().surface_probe = Some(state.surface.clone());
        }
        let on_quit = self.clone();
        cx.on_app_quit(move |_| {
            on_quit.require_complete("application quit before the interaction journey completed");
            std::future::ready(())
        })
        .detach();

        // AppKit may wait inside a nested termination loop. Its process deadline must
        // survive a stalled foreground executor, including after interaction succeeds.
        std::thread::spawn(|| {
            std::thread::sleep(TIMEOUT + Duration::from_secs(5));
            eprintln!(
                "SMOKE FAILED: the native process did not finish interaction and quit within 25 seconds"
            );
            std::process::exit(1);
        });
        let watchdog = self.clone();
        let weak = Rc::downgrade(host);
        cx.spawn(async move |cx| {
            cx.background_executor().timer(TIMEOUT).await;
            if !watchdog.complete.get() {
                let detail = weak.upgrade().map(|host| {
                    let host = host.borrow();
                    format!(
                        "step={:?}, serial={}, pending={}, core_presented={}, adapter_error={:?}",
                        watchdog.state.borrow().step,
                        host.serial,
                        host.pending.is_some(),
                        input::is_presented(&host, host.serial),
                        host.error,
                    )
                });
                watchdog.fail(format!(
                    "interaction journey timed out after 20 seconds ({detail:?})"
                ));
            }
        })
        .detach();
        self.observe(Rc::downgrade(host), window);
    }

    fn install_panel(&self, shared: &SharedHost, item: ItemId, cx: &mut App) -> PanelProbe {
        let view = shared
            .borrow()
            .panels
            .get(&item)
            .map(|panel| panel.view.clone());
        let Some(view) = view else {
            self.fail(format!("missing smoke pane {item:?}"));
        };
        let Ok(entity) = view.downcast::<CounterPanel>() else {
            self.fail(format!("smoke pane {item:?} is not a counter view"));
        };
        let button = ButtonProbe::default();
        entity.update(cx, |panel, cx| {
            panel.button_probe = Some(button.clone());
            cx.notify();
        });
        PanelProbe { entity, button }
    }

    fn observe(&self, weak: Weak<RefCell<HostState>>, window: &Window) {
        let smoke = self.clone();
        window.on_next_frame(move |window, cx| {
            if smoke.complete.get() {
                return;
            }
            let Some(shared) = weak.upgrade() else {
                smoke.fail("workbench was released before the interaction journey completed");
            };
            if smoke.advance(&shared, window, cx) {
                smoke.complete.set(true);
                println!(
                    "SMOKE OK: native window visible; exact Submitted/core artifacts accepted; retained buttons and tabs verified; splitter and continuous floating drag completed; requesting native quit"
                );
                cx.quit();
            } else {
                smoke.observe(weak, window);
            }
        });
    }

    fn advance(&self, shared: &SharedHost, window: &mut Window, cx: &mut App) -> bool {
        if matches!(self.state.borrow().step, Step::FloatMoving) {
            let mut state = self.state.borrow_mut();
            let floating = state.floating.as_mut().expect("floating drag journey");
            floating.moves += 1;
            floating.pending_moves += usize::from(shared.borrow().pending.is_some());
            move_floating(window, cx, floating.position(floating.moves));
            floating.last_input = Instant::now();
            if floating.moves == 24 {
                state.step = Step::FloatMoved;
            }
            // Continuous motion deliberately does not wait for the preceding candidate's
            // Submitted result. This exercises input while the native renderer is pending.
            window.refresh();
            return false;
        }
        let host = shared.borrow();
        if let Some(error) = &host.error {
            self.fail(format!("adapter failed: {error}"));
        }
        let facts = window.presentation_facts();
        let Some(generation) = facts.present_submitted_generation else {
            return false;
        };
        let native_submitted = facts.native_created
            && facts.native_visible
            && window.is_window_active()
            && facts.non_empty_presented_generation == Some(generation)
            && facts.frame_accepted_generation == Some(generation)
            && facts.latest_present_attempt.is_some_and(|attempt| {
                attempt.generation == generation
                    && attempt.outcome == PlatformWindowPresentOutcome::Submitted
            });
        if !native_submitted
            || host.pending.is_some()
            || !host.inputs.is_empty()
            || host.dirty
            || !input::is_presented(&host, host.serial)
        {
            return false;
        }
        let Some(packet) = host.packet.clone() else {
            return false;
        };
        let mut state = self.state.borrow_mut();
        if generation <= state.submitted_before_input {
            return false;
        }
        let Some(surface) = state.surface.get() else {
            return false;
        };
        let editor = state.editor.as_ref().expect("installed Editor probe");
        let preview = state.preview.as_ref().expect("installed Preview probe");
        let editor_clicks = editor.entity.read(cx).clicks;
        let preview_clicks = preview.entity.read(cx).clicks;
        let editor_selected = host
            .session
            .view()
            .item(EDITOR)
            .is_some_and(|item| item.is_selected());
        let preview_selected = host
            .session
            .view()
            .item(PREVIEW)
            .is_some_and(|item| item.is_selected());
        let active = host.session.has_active_gesture();
        let retained = host.panels[&EDITOR].view.entity_id() == editor.entity.entity_id()
            && host.panels[&PREVIEW].view.entity_id() == preview.entity.entity_id();
        self.require(retained, "switching tabs replaced a retained GPUI entity");
        let step = state.step;
        drop(host);

        match step {
            Step::FirstFrame => {
                self.require(
                    VISIBLE_PANES.iter().all(|item| packet.contains_pane(*item)),
                    "initial submitted packet omitted a visible AnyView",
                );
                self.require(
                    editor_selected && editor_clicks == 0 && preview_clicks == 0,
                    "initial selection or counter state was unexpected",
                );
                let Some(button) = editor.button.get() else {
                    return false;
                };
                click(window, cx, button.center());
                state.step = Step::EditorClicked;
            }
            Step::EditorClicked => {
                self.require(
                    editor_clicks == 1 && editor_selected && !active,
                    "Editor button did not receive exactly one click independently of docking",
                );
                let Some(tab) = packet.tab_bounds(PREVIEW) else {
                    self.fail("Preview tab is absent from the submitted packet");
                };
                click(window, cx, center(tab, surface));
                state.step = Step::PreviewSelected;
            }
            Step::PreviewSelected => {
                self.require(
                    preview_selected && packet.contains_pane(PREVIEW) && !active,
                    "clicking Preview did not select and compose its GPUI view",
                );
                let Some(button) = preview.button.get() else {
                    return false;
                };
                click(window, cx, button.center());
                state.step = Step::PreviewClicked;
            }
            Step::PreviewClicked => {
                self.require(
                    preview_clicks == 1 && editor_clicks == 1 && !active,
                    "Preview button did not receive its own click",
                );
                let Some(tab) = packet.tab_bounds(EDITOR) else {
                    self.fail("Editor tab is absent from the submitted packet");
                };
                click(window, cx, center(tab, surface));
                state.step = Step::EditorRestored;
            }
            Step::EditorRestored => {
                self.require(
                    editor_selected
                        && editor_clicks == 1
                        && preview_clicks == 1
                        && packet.contains_pane(EDITOR)
                        && !active,
                    "returning to Editor did not preserve both counters",
                );
                let Some(pane) = packet.pane_bounds(EDITOR) else {
                    self.fail("Editor pane bounds are missing");
                };
                let Some(splitter) = packet.receiver_bounds(DockspaceReceiverRole::Splitter) else {
                    self.fail("the initial split has no operable divider");
                };
                // The upper section is exposed above the contained Tools window.
                let start = point(
                    surface.origin.x + px((splitter.x() + splitter.width() / 2.0) as f32),
                    surface.origin.y + px((splitter.y() + splitter.height() * 0.2) as f32),
                );
                let distance = pane.width() * 0.1;
                let end = start - point(px(distance as f32), px(0.0));
                move_pointer(window, cx, start, false);
                press(window, cx, start);
                state.split = Some(SplitProbe {
                    end,
                    width_before: pane.width(),
                    distance,
                });
                state.step = Step::SplitterPressed;
            }
            Step::SplitterPressed => {
                self.require(
                    active,
                    "divider press did not begin a captured docking gesture",
                );
                let split = state.split.expect("splitter journey");
                move_pointer(window, cx, split.end, true);
                state.step = Step::SplitterMoved;
            }
            Step::SplitterMoved => {
                self.require(active, "divider motion lost its captured gesture");
                let split = state.split.expect("splitter journey");
                release(window, cx, split.end);
                state.step = Step::SplitterReleased;
            }
            Step::SplitterReleased => {
                let split = state.split.expect("splitter journey");
                let Some(pane) = packet.pane_bounds(EDITOR) else {
                    self.fail("resized Editor pane bounds are missing");
                };
                self.require(
                    !active
                        && editor_clicks == 1
                        && preview_clicks == 1
                        && pane.width() < split.width_before - split.distance * 0.5,
                    "divider drag did not publish the expected resized pane",
                );
                let Some(title) = packet.receiver_bounds(DockspaceReceiverRole::ContainedTitle)
                else {
                    self.fail("the contained Tools window has no title receiver");
                };
                let Some(pane_before) = packet.pane_bounds(TOOLS) else {
                    self.fail("Tools pane bounds are missing");
                };
                let start = center(title, surface);
                move_pointer(window, cx, start, false);
                press(window, cx, start);
                state.floating = Some(FloatProbe {
                    start,
                    pane_before,
                    moves: 0,
                    pending_moves: 0,
                    last_input: Instant::now(),
                });
                state.step = Step::FloatPressed;
            }
            Step::FloatPressed => {
                self.require(
                    active,
                    "floating title press did not capture a docking gesture",
                );
                state.step = Step::FloatMoving;
            }
            Step::FloatMoving => unreachable!("continuous motion is handled before frame gating"),
            Step::FloatMoved => {
                let floating = state.floating.as_ref().expect("floating drag journey");
                let Some(pane) = packet.pane_bounds(TOOLS) else {
                    self.fail("continuous floating motion lost the Tools view");
                };
                self.require(
                    active
                        && (pane.x() - (floating.pane_before.x() - 120.0)).abs() < 1.0
                        && (pane.y() - (floating.pane_before.y() + 40.0)).abs() < 1.0,
                    "the submitted floating preview did not follow the latest continuous input",
                );
                println!(
                    "SMOKE floating drag: {} moves without per-edge presentation waits ({} while pending); final input to observed Submitted artifact: {:.1} ms",
                    floating.moves,
                    floating.pending_moves,
                    floating.last_input.elapsed().as_secs_f64() * 1000.0,
                );
                let mut event = MouseUpEvent {
                    position: floating.position(24),
                    button: MouseButton::Left,
                    ..Default::default()
                };
                event.modifiers.shift = true;
                window.dispatch_event(PlatformInput::MouseUp(event), cx);
                state.step = Step::FloatReleased;
            }
            Step::FloatReleased => {
                let floating = state.floating.as_ref().expect("floating drag journey");
                let Some(pane) = packet.pane_bounds(TOOLS) else {
                    self.fail("releasing the floating title lost the Tools view");
                };
                self.require(
                    !active
                        && retained
                        && (pane.x() - (floating.pane_before.x() - 120.0)).abs() < 1.0
                        && (pane.y() - (floating.pane_before.y() + 40.0)).abs() < 1.0,
                    "continuous floating drag did not retain its final position and view",
                );
                return true;
            }
        }
        state.submitted_before_input = generation;
        // on_next_frame runs before draw. The next step must observe a newer
        // renderer submission and the matching core artifact before using its geometry.
        window.refresh();
        false
    }

    fn require(&self, condition: bool, reason: &str) {
        if !condition {
            self.fail(reason);
        }
    }

    pub(super) fn require_complete(&self, reason: &str) {
        self.require(self.complete.get(), reason);
    }

    pub(super) fn fail(&self, reason: impl std::fmt::Display) -> ! {
        eprintln!("SMOKE FAILED: {reason}");
        // macOS Platform::quit uses NSApplication::terminate and exits successfully.
        // A failed executable probe must retain its failure status on that path too.
        std::process::exit(1);
    }
}

fn center(rect: LogicalRect, surface: Bounds<Pixels>) -> Point<Pixels> {
    point(
        surface.origin.x + px((rect.x() + rect.width() / 2.0) as f32),
        surface.origin.y + px((rect.y() + rect.height() / 2.0) as f32),
    )
}

fn click(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    move_pointer(window, cx, position, false);
    press(window, cx, position);
    release(window, cx, position);
}

fn move_pointer(window: &mut Window, cx: &mut App, position: Point<Pixels>, down: bool) {
    window.dispatch_event(
        PlatformInput::MouseMove(MouseMoveEvent {
            position,
            pressed_button: down.then_some(MouseButton::Left),
            modifiers: Default::default(),
        }),
        cx,
    );
}

fn move_floating(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    let mut event = MouseMoveEvent {
        position,
        pressed_button: Some(MouseButton::Left),
        modifiers: Default::default(),
    };
    event.modifiers.shift = true;
    window.dispatch_event(PlatformInput::MouseMove(event), cx);
}

fn press(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    window.dispatch_event(
        PlatformInput::MouseDown(MouseDownEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
}

fn release(window: &mut Window, cx: &mut App, position: Point<Pixels>) {
    window.dispatch_event(
        PlatformInput::MouseUp(MouseUpEvent {
            position,
            button: MouseButton::Left,
            click_count: 1,
            ..Default::default()
        }),
        cx,
    );
}
