//! Retained interaction tests use GPUI's explicit CPU test-renderer boundary.

use std::{
    cell::Cell,
    collections::BTreeMap,
    convert::Infallible,
    panic::{AssertUnwindSafe, catch_unwind},
    rc::Rc,
};

use dockspace::{
    geometry::LogicalRect,
    model::{FloatingPresentationId, ItemId},
    runtime::{SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step},
};
use open_gpui::{
    Context, InputEvent, IntoElement, Modifiers, MouseButton, MouseDownEvent, MouseUpEvent, Pixels,
    PlatformWindowPresentOutcome, Point, PointerCancelEvent, PointerCancelReason, Render,
    RequestFrameOptions, TestAppContext, VisualTestContext, Window, canvas, div, fill, point,
    prelude::*, px, rgb, size,
};

use crate::{CounterPanel, SURFACE, SharedHost, Workbench, element::DockspaceElement, input};

struct Fixture {
    host: SharedHost,
    visual: VisualTestContext,
}

#[derive(Default)]
struct Geometry {
    tabs: BTreeMap<ItemId, LogicalRect>,
    panes: BTreeMap<ItemId, LogicalRect>,
    splitters: Vec<LogicalRect>,
    titles: BTreeMap<FloatingPresentationId, LogicalRect>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BoundaryMode {
    Live,
    Rollback,
    Absent,
    RejectCandidate,
}

struct BoundaryRoot {
    workbench: Workbench,
    mode: Rc<Cell<BoundaryMode>>,
}

impl Render for BoundaryRoot {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        let shared = self.workbench.host.clone();
        let mode = self.mode.clone();
        canvas(
            move |bounds, window, cx| {
                if mode.get() == BoundaryMode::Absent {
                    return None;
                }
                let mut child = div()
                    .size_full()
                    .font_family("Arial")
                    .child(DockspaceElement::new(shared.clone()))
                    .into_any_element();
                let available = size(
                    open_gpui::AvailableSpace::Definite(bounds.size.width),
                    open_gpui::AvailableSpace::Definite(bounds.size.height),
                );
                if mode.get() == BoundaryMode::Rollback {
                    let rolled_back: Result<(), ()> = window.transact(|window| {
                        child.prepaint_as_root(bounds.origin, available, window, cx);
                        Err(())
                    });
                    assert!(rolled_back.is_err());
                    return None;
                }
                child.prepaint_as_root(bounds.origin, available, window, cx);
                if mode.get() == BoundaryMode::RejectCandidate {
                    // GPUI's real draw-unwind path rejects this candidate before frame swap.
                    panic!("injected outer candidate rejection");
                }
                Some(child)
            },
            |bounds, child, window, cx| {
                window.paint_quad(fill(bounds, rgb(0xe6eaf0)));
                if let Some(mut child) = child {
                    child.paint(window, cx);
                }
            },
        )
        .size_full()
    }
}

impl Fixture {
    fn new(cx: &mut TestAppContext) -> Self {
        cx.defer_next_window_frame_requests();
        let window = cx.open_window(size(px(1100.0), px(760.0)), |_, cx| {
            Workbench::new(cx).expect("valid example workspace")
        });
        let host = window
            .root(cx)
            .expect("workbench root")
            .read_with(cx, |workbench, _| workbench.host.clone());
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, _| {
            let _ = window.activate_window();
        });
        let mut fixture = Self { host, visual };
        fixture.present();
        fixture
    }

    fn with_boundary(cx: &mut TestAppContext) -> (Self, Rc<Cell<BoundaryMode>>) {
        cx.defer_next_window_frame_requests();
        let mode = Rc::new(Cell::new(BoundaryMode::Live));
        let root_mode = mode.clone();
        let window = cx.open_window(size(px(1100.0), px(760.0)), |_, cx| BoundaryRoot {
            workbench: Workbench::new(cx).expect("valid example workspace"),
            mode: root_mode,
        });
        let root = window.root(cx).expect("boundary root");
        let host = root.read_with(cx, |root, _| root.workbench.host.clone());
        let mut visual = VisualTestContext::from_window(window.into(), cx);
        visual.update(|window, _| {
            let _ = window.activate_window();
        });
        let mut fixture = Self { host, visual };
        fixture.present();
        (fixture, mode)
    }

    /// Submitted is a deterministic test-renderer outcome, not GPU or OS evidence.
    fn present(&mut self) {
        for _ in 0..8 {
            self.frame();
            let host = self.host.borrow();
            assert!(host.error.is_none(), "adapter error: {:?}", host.error);
            if host.pending.is_none() && host.inputs.is_empty() && !host.dirty {
                assert!(
                    input::is_presented(&host, host.serial),
                    "serial={} presented={:?} core={:?}",
                    host.serial,
                    host.presented,
                    host.session.presented_surface(SURFACE)
                );
                return;
            }
        }
        panic!("the retained adapter did not settle within eight submitted test frames");
    }

    fn frame(&mut self) {
        assert!(self.visual.simulate_frame(RequestFrameOptions {
            require_presentation: true,
            force_render: true,
            ..Default::default()
        }));
    }

    fn outcome(&mut self, outcome: PlatformWindowPresentOutcome) {
        let window = self.visual.update(|window, _| window.window_handle());
        self.visual.set_window_present_outcome(window, outcome);
    }

    fn geometry(&self) -> Geometry {
        let mut host = self.host.borrow_mut();
        let mut frame = host
            .session
            .begin_host_frame()
            .expect("geometry query frame");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("surface work")
            .expect("measured plan");
        let mut geometry = Geometry::default();
        plan.visit_surface_program(|step| {
            if let Step::Operation { operation, .. } = step {
                match operation {
                    Operation::Tab(tab) => {
                        geometry.tabs.insert(tab.item(), tab.visible_bounds());
                    }
                    Operation::Pane(pane) => {
                        if let Some(item) = pane.selected() {
                            geometry.panes.insert(item, pane.content_bounds());
                        }
                    }
                    Operation::Splitter(splitter) => geometry.splitters.push(splitter.hit_bounds()),
                    Operation::ContainedControls(contained) => {
                        geometry
                            .titles
                            .insert(contained.floating(), contained.title_drag_bounds());
                    }
                    _ => {}
                }
            }
            Ok::<_, Infallible>(())
        })
        .expect("inspect the ordered program without confirming emission");
        geometry
    }

    fn selected(&self, item: u64) -> bool {
        self.host
            .borrow()
            .session
            .view()
            .item(ItemId::new(item))
            .expect("open item")
            .is_selected()
    }

    fn clicks(&self, item: u64) -> usize {
        let view = self.host.borrow().panels[&ItemId::new(item)].view.clone();
        view.downcast::<CounterPanel>()
            .expect("counter panel entity")
            .read_with(&self.visual, |panel, _| panel.clicks)
    }

    fn click_tab(&mut self, item: u64) {
        let bounds = self.geometry().tabs[&ItemId::new(item)];
        self.visual
            .simulate_click(center(bounds), Modifiers::default());
        self.present();
    }

    fn drag(&mut self, start: Point<Pixels>, end: Point<Pixels>) {
        self.visual
            .simulate_mouse_down(start, MouseButton::Left, Modifiers::default());
        self.present();
        self.visual
            .simulate_mouse_move(end, MouseButton::Left, Modifiers::default());
        self.present();
        // This is a second real device edge after the first preview was submitted.
        self.visual.simulate_mouse_move(
            end + point(px(1.0), px(0.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        self.present();
        self.visual.simulate_mouse_up(
            end + point(px(1.0), px(0.0)),
            MouseButton::Left,
            Modifiers::default(),
        );
        self.present();
    }
}

fn center(bounds: LogicalRect) -> Point<Pixels> {
    point(
        px((bounds.x() + bounds.width() / 2.0) as f32),
        px((bounds.y() + bounds.height() / 2.0) as f32),
    )
}

#[open_gpui::test]
fn application_button_and_tab_selection_preserve_retained_view_state(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let button = fixture
        .visual
        .debug_bounds("counter:Editor")
        .expect("real GPUI button");
    fixture
        .visual
        .simulate_click(button.center(), Modifiers::default());
    fixture.present();
    assert_eq!(fixture.clicks(1), 1);
    assert!(fixture.selected(1));
    assert!(!fixture.host.borrow().session.has_active_gesture());

    fixture.click_tab(2);
    assert!(fixture.selected(2));
    assert!(!fixture.selected(1));
    fixture.click_tab(1);
    assert_eq!(
        fixture.clicks(1),
        1,
        "switching tabs must retain the original entity"
    );
}

#[open_gpui::test]
fn deferred_replacement_cannot_borrow_the_old_artifacts_input_authority(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let tab = center(fixture.geometry().tabs[&ItemId::new(2)]);
    let previous_serial = fixture.host.borrow().serial;
    let old_artifact = fixture
        .host
        .borrow()
        .presented
        .expect("submitted initial packet")
        .1;
    fixture.outcome(PlatformWindowPresentOutcome::Deferred);
    fixture.visual.simulate_resize(size(px(1000.0), px(760.0)));
    fixture.frame();
    {
        let host = fixture.host.borrow();
        assert!(host.error.is_none(), "adapter error: {:?}", host.error);
        assert!(host.serial > previous_serial);
        assert!(
            host.pending.is_some(),
            "the test renderer has not submitted the replacement"
        );
        assert!(!input::is_presented(&host, host.serial));
        assert!(
            host.session
                .presented_surface(SURFACE)
                .is_some_and(|surface| surface.matches_presentation_artifact(old_artifact))
        );
    }
    let facts = fixture
        .visual
        .update(|window, _| window.presentation_facts());
    assert!(facts.frame_accepted_generation > facts.present_submitted_generation);
    assert_eq!(
        facts
            .latest_present_attempt
            .expect("test renderer attempt")
            .outcome,
        PlatformWindowPresentOutcome::Deferred
    );
    // Resizing leaves these leading tab bounds intact; GPUI has accepted their new packet.
    fixture.visual.simulate_click(tab, Modifiers::default());
    assert!(fixture.selected(1));
    assert!(fixture.host.borrow().inputs.is_empty());
    assert!(!fixture.host.borrow().session.has_active_gesture());

    fixture.outcome(PlatformWindowPresentOutcome::Submitted);
    fixture.present();
    fixture.click_tab(2);
    assert!(fixture.selected(2));
}

#[open_gpui::test]
fn real_tab_drag_merges_into_another_pane(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let geometry = fixture.geometry();
    let source = center(geometry.tabs[&ItemId::new(2)]);
    let target = center(geometry.panes[&ItemId::new(3)]);
    let original_entity = fixture.host.borrow().panels[&ItemId::new(2)]
        .view
        .entity_id();
    fixture.drag(source, target);

    let host = fixture.host.borrow();
    let view = host.session.view();
    let target = view.item(ItemId::new(3)).expect("target item");
    assert!(
        target.tabs().items().contains(&ItemId::new(2)),
        "dragged tab should join the target's group"
    );
    assert!(
        !view
            .item(ItemId::new(1))
            .expect("source item")
            .tabs()
            .items()
            .contains(&ItemId::new(2))
    );
    assert_eq!(
        host.panels[&ItemId::new(2)].view.entity_id(),
        original_entity
    );
    assert!(!host.session.has_active_gesture());
}

#[open_gpui::test]
fn real_splitter_drag_changes_the_pane_geometry(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let before = fixture.geometry();
    let splitter = before.splitters[0];
    // Use the exposed part above the floating Tools window.
    let start = point(
        px((splitter.x() + splitter.width() / 2.0) as f32),
        px((splitter.y() + splitter.height() * 0.2) as f32),
    );
    fixture.drag(start, start + point(px(-90.0), px(0.0)));
    let after = fixture.geometry();
    assert!(after.panes[&ItemId::new(1)].width() < before.panes[&ItemId::new(1)].width() - 50.0);
    assert!(after.panes[&ItemId::new(3)].width() > before.panes[&ItemId::new(3)].width() + 50.0);
    assert!(!fixture.host.borrow().session.has_active_gesture());
}

#[open_gpui::test]
fn real_contained_title_drag_moves_the_existing_floating_root(cx: &mut TestAppContext) {
    let mut fixture = Fixture::new(cx);
    let floating = FloatingPresentationId::new(1);
    let before = fixture
        .host
        .borrow()
        .session
        .view()
        .contained(floating)
        .expect("initial floating root")
        .rect();
    let start = center(fixture.geometry().titles[&floating]);
    fixture.drag(start, start + point(px(-120.0), px(110.0)));
    let host = fixture.host.borrow();
    let after = host
        .session
        .view()
        .contained(floating)
        .expect("same floating root")
        .rect();
    assert!(after.x() < before.x() - 70.0);
    assert!(after.y() > before.y() + 70.0);
    assert_eq!(
        host.session
            .view()
            .item(ItemId::new(4))
            .expect("Tools item")
            .contained(),
        Some(floating)
    );
    assert!(!host.session.has_active_gesture());
}

#[open_gpui::test]
fn publication_loss_with_outer_transaction_rollback_retires_the_pending_candidate(
    cx: &mut TestAppContext,
) {
    let (mut fixture, mode) = Fixture::with_boundary(cx);
    let tab = center(fixture.geometry().tabs[&ItemId::new(2)]);
    let before = fixture.host.borrow().serial;
    fixture.visual.update(|window, app| {
        window.dispatch_event(
            MouseDownEvent {
                position: tab,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }
            .to_platform_input(),
            app,
        );
        window.dispatch_event(
            MouseUpEvent {
                position: tab,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }
            .to_platform_input(),
            app,
        );
        mode.set(BoundaryMode::Rollback);
        window.refresh();
        window.draw(app).clear(app);
        // Do not attempt a second rollback after the previous publication has
        // already been removed. The next accepted frame keeps the subtree absent.
        mode.set(BoundaryMode::Absent);
    });

    {
        let host = fixture.host.borrow();
        assert!(host.error.is_none(), "adapter error: {:?}", host.error);
        assert!(
            host.pending.is_none(),
            "the absent publication must withdraw candidate B"
        );
        assert_eq!(
            host.inputs.len(),
            2,
            "the unpublished Down/Up prefix is restored"
        );
        assert!(!host.session.has_active_gesture());
        assert!(
            host.serial > before,
            "rollback must have attempted a replacement frame"
        );
    }

    fixture.frame();

    // The old publication's discard callback must leave the session usable and
    // must not publish the rolled-back candidate's layout.
    let mut host = fixture.host.borrow_mut();
    let frame = host
        .session
        .begin_host_frame()
        .expect("rollback released update lease");
    drop(frame);
    assert!(host.pending.is_none());
    assert!(
        host.session
            .view()
            .item(ItemId::new(1))
            .unwrap()
            .is_selected()
    );
    assert_eq!(
        host.session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .tab_index(),
        1
    );
}

#[open_gpui::test]
fn platform_cancel_covers_a_down_retained_by_a_rejected_candidate(cx: &mut TestAppContext) {
    let (mut fixture, mode) = Fixture::with_boundary(cx);
    let tab = center(fixture.geometry().tabs[&ItemId::new(1)]);
    let before = fixture.host.borrow().session.version();

    // This edge belongs to the accepted A frame. Its input is retained in the
    // next pending candidate until that candidate is accepted.
    let shared = fixture.host.clone();
    fixture.visual.update(|window, app| {
        window.dispatch_event(
            MouseDownEvent {
                position: tab,
                button: MouseButton::Left,
                click_count: 1,
                ..Default::default()
            }
            .to_platform_input(),
            app,
        );
        assert_eq!(shared.borrow().inputs.len(), 1);
        assert!(!shared.borrow().session.has_active_gesture());
        mode.set(BoundaryMode::RejectCandidate);
        let previous = window.rendered_frame_revision();
        window.refresh();
        let rejected = catch_unwind(AssertUnwindSafe(|| window.draw(app).clear(app)))
            .expect_err("the actual outer candidate must be rejected");
        assert_eq!(
            rejected.downcast_ref::<&str>(),
            Some(&"injected outer candidate rejection")
        );
        assert_eq!(
            window.rendered_frame_revision(),
            previous,
            "candidate B never replaced accepted A"
        );
        let host = shared.borrow();
        assert!(
            host.pending.is_some(),
            "rejected candidate retains its input prefix"
        );
        assert!(host.pending.as_ref().unwrap().update.is_some());
        assert_eq!(host.pending.as_ref().unwrap().inputs.len(), 1);
        assert!(!host.session.has_active_gesture());
        drop(host);

        // The previous accepted frame's listener must see the retained prefix.
        window.dispatch_event(
            PointerCancelEvent {
                reason: PointerCancelReason::PlatformCaptureLost,
            }
            .to_platform_input(),
            app,
        );
        assert_eq!(shared.borrow().inputs.len(), 1);
        mode.set(BoundaryMode::Live);
    });
    fixture.present();

    let host = fixture.host.borrow();
    assert!(host.pending.is_none());
    assert!(!host.session.has_active_gesture());
    assert_eq!(
        host.session.version(),
        before,
        "cancelled retry must not change layout"
    );
    assert_eq!(
        host.session
            .view()
            .item(ItemId::new(2))
            .unwrap()
            .tab_index(),
        1
    );
}
