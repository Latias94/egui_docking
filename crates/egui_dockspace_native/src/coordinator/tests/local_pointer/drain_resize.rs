//! The native editor's contained-central drain followed by real splitter input.

use super::*;
use dockspace::model::{DockspaceAxis, DockspaceContainedLayout, FloatingPresentationId};

mod real_hits;

const INSPECTOR: ItemId = ItemId::new(4);
const CONSOLE: ItemId = ItemId::new(5);
const HISTORY: ItemId = ItemId::new(6);
const PROFILER: ItemId = ItemId::new(7);
const ASSETS: ItemId = ItemId::new(8);
const INSPECTOR_ROOT: RootId = RootId::new(2);

struct EditorScene {
    scene: Scene,
    outline: LogicalPoint,
    splitters: Vec<(
        DockspaceAxis,
        dockspace::runtime::DockspaceVisualId,
        DockspaceReceiverDescriptor,
    )>,
    junctions: Vec<LogicalRect>,
}

fn editor_fixture(desktop: bool) -> (NativeCoordinator, NativeSurfaceBinding, Arc<AtomicU64>) {
    let center = DockspaceNode::split(
        DockspaceAxis::Vertical,
        [
            (
                DockspaceNode::central_tabs([SECOND, HISTORY, PROFILER]),
                0.74,
            ),
            (DockspaceNode::tabs([CONSOLE]), 0.26),
        ],
    )
    .unwrap();
    let root = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [(DockspaceNode::tabs([FIRST, ASSETS]), 0.20), (center, 0.80)],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        FIRST_SURFACE,
        DockspaceRootLayout::new(RootId::new(1), root),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(
            INSPECTOR_ROOT,
            DockspaceNode::central_tabs_with_selection([THIRD, INSPECTOR], Some(INSPECTOR))
                .unwrap(),
        ),
        LogicalRect::new(440.0, 130.0, 320.0, 340.0).unwrap(),
    ))])
    .unwrap();
    let now = Arc::new(AtomicU64::new(40));
    let clock = Arc::clone(&now);
    let mut native = NativeCoordinator::new_with_storage_allowance(
        DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap(),
        NativePointerRoster::Exact(Vec::new()),
        None,
        None,
        None,
        Some(NativeMonotonicClock::new(move || {
            Duration::from_millis(clock.load(Ordering::SeqCst))
        })),
    )
    .unwrap();
    native
        .register_native_root(FIRST_SURFACE, HostWindowToken::new(WINDOW))
        .unwrap();
    let binding = idle(&mut native).native_bindings()[0];
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(WINDOW), binding)
        .unwrap();
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    let mut facts = NativeWindowFacts::live()
        .with_content_size(geometry().content_size())
        .with_native_scale_factor(geometry().native_scale_factor())
        .with_presentation_scale_factor(ScaleFactor::new(1.5).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    if desktop {
        capabilities = capabilities
            .with(NativeHostCapability::DesktopPointerPosition)
            .with(NativeHostCapability::HoveredWindow);
        facts = facts.with_content_bounds(PhysicalRect::new(400.0, -300.0, 1200.0, 900.0).unwrap());
    }
    native
        .report_snapshot(
            [(binding, facts)],
            NativeWorkAreaRoster::Unknown,
            capabilities,
        )
        .unwrap();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .replace_presentation_config(
            egui_dockspace::DockStyle::default()
                .presentation_config()
                .unwrap(),
        )
        .unwrap();
    frame
        .frame
        .measure_surface(
            FIRST_SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 94.0, 800.0, 506.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let report = finish(frame);
    native.settle_host_frame_report(&report).unwrap();
    (native, binding, now)
}

fn encode(native: &mut NativeCoordinator) -> (EditorScene, HostFrameReport) {
    let outline_selected = native.session.view().item(FIRST).unwrap().tabs().selected();
    let mut frame = native
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let scene = render(&mut frame);
    let plan = frame.frame.paint_plan(FIRST_SURFACE).unwrap().unwrap();
    let mut outline = None;
    let mut splitters = Vec::new();
    let mut junctions = Vec::new();
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::Operation { operation, .. } = step {
            match operation {
                SurfacePaintProgramOperation::Pane(pane) if pane.selected() == outline_selected => {
                    let bounds = pane.content_bounds();
                    outline = Some(
                        LogicalPoint::new(
                            bounds.x() + bounds.width() * 0.5,
                            bounds.y() + bounds.height() * 0.75,
                        )
                        .unwrap(),
                    );
                }
                SurfacePaintProgramOperation::Splitter(splitter) => {
                    splitters.push((
                        splitter.axis(),
                        splitter.visual_id(),
                        plan.receiver_for_splitter(splitter).unwrap(),
                    ));
                }
                SurfacePaintProgramOperation::SplitterJunction(junction) => {
                    junctions.push(junction.hit_bounds());
                }
                _ => {}
            }
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let report = finish(frame);
    native.settle_host_frame_report(&report).unwrap();
    (
        EditorScene {
            scene,
            outline: outline.unwrap(),
            splitters,
            junctions,
        },
        report,
    )
}

fn present(native: &mut NativeCoordinator, binding: NativeSurfaceBinding) -> EditorScene {
    let (scene, mut report) = encode(native);
    let output = take_output(&mut report, &scene.scene, binding);
    native
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    assert!(idle(native).manipulation_feedback().is_empty());
    scene
}

fn apply(
    native: &mut NativeCoordinator,
    binding: NativeSurfaceBinding,
    scene: &EditorScene,
    receiver: &DockspaceReceiverDescriptor,
    ordinal: &mut u64,
    event: WindowEvent,
    retry: bool,
) -> HostFrameReport {
    *ordinal += 1;
    let record = capture(native, binding, *ordinal, event);
    native.bridge.push_record(HostRecord::WindowEvent(record));
    reduce_one(native);
    let answer = |query| match query {
        NativeReceiverQuery::WindowHit(_) => scene.scene.answer(query, CAPTURED_AT),
        NativeReceiverQuery::Surface(query)
            if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) =>
        {
            NativeReceiverAnswer::ClearHoverLane(query.presented_surface())
        }
        NativeReceiverQuery::Surface(query) => query
            .bind_receiver(receiver)
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock),
    };
    if retry {
        let version = native.session.version();
        drop(native.begin_host_frame(answer).unwrap());
        assert_eq!(native.session.version(), version);
    }
    let frame = native
        .begin_host_frame(answer)
        .unwrap_or_else(|error| panic!("drain/resize raw ordinal {ordinal}: {error:?}"));
    let mut report = finish(frame);
    assert!(report.take_native_effects().is_empty());
    native.settle_host_frame_report(&report).unwrap();
    report
}

fn assert_inventory(native: &NativeCoordinator, contained: usize) {
    let view = native.session.view();
    let surface = view.surface(FIRST_SURFACE).unwrap();
    assert_eq!(surface.contained_count(), contained);
    let mut items = Vec::new();
    for root in surface
        .main_root()
        .into_iter()
        .chain(surface.contained().map(|window| window.root().unwrap()))
    {
        let mut pending = vec![root.content().unwrap()];
        while let Some(node) = pending.pop() {
            if let Some(tabs) = node.tabs() {
                items.extend_from_slice(tabs.items());
            } else {
                pending.extend(node.split().unwrap().children());
            }
        }
    }
    items.sort_by_key(|item| item.get());
    assert_eq!(items, (1..=8).map(ItemId::new).collect::<Vec<_>>());
    for item in items {
        assert_eq!(view.item(item).unwrap().surface(), FIRST_SURFACE);
    }
}

fn drain_and_resize(desktop: bool, late_output: bool, items: [ItemId; 2]) {
    let (mut native, binding, now) = editor_fixture(desktop);
    now.store(40, Ordering::SeqCst);
    let mut held = None;
    let mut ordinal = 0;
    for (index, item) in items.into_iter().enumerate() {
        let scene = present(&mut native, binding);
        if late_output && index == 1 {
            // Keep the last actually presented scene as the input authority.
            // Presenting a newer copy behind this unresolved output would
            // correctly block pointer admission until the prefix settles.
            let (copy, mut report) = encode(&mut native);
            held = Some(take_output(&mut report, &copy.scene, binding));
        }
        let source = scene.scene.point(item, 0.25);
        let receiver = scene.scene.tabs.iter().find(|tab| tab.0 == item).unwrap().1;
        for (point, state) in [
            (source, Some(ElementState::Pressed)),
            (scene.outline, None),
            (scene.outline, Some(ElementState::Released)),
        ] {
            let event = if desktop {
                raw_desktop_event(point, state)
            } else {
                raw_event(point, state, 2.0)
            };
            let report = apply(
                &mut native,
                binding,
                &scene,
                &receiver,
                &mut ordinal,
                event,
                true,
            );
            if state == Some(ElementState::Released) {
                assert!(
                    matches!(
                        report.manipulation_feedback(),
                        [feedback] if feedback.status() == Status::Delivered(Delivery::Dock)
                    ),
                    "desktop={desktop}, late_output={late_output}, drain_index={index}, item={item:?}, raw_ordinal={ordinal}: {report:?}"
                );
                assert!(report.workspace_changed());
            }
        }
        assert!(!native.session.has_active_gesture());
        assert_inventory(&native, 1 - index);
        assert!(
            native
                .session
                .view()
                .item(item)
                .unwrap()
                .contained()
                .is_none()
        );
    }
    assert!(native.session.view().root(INSPECTOR_ROOT).is_none());
    if let Some(output) = held {
        native
            .session
            .report_surface_presentation(output, SurfacePresentationResult::Dropped)
            .unwrap();
        let report = idle(&mut native);
        assert!(report.manipulation_feedback().is_empty());
        assert!(!report.workspace_changed());
    }
    for axis in [DockspaceAxis::Vertical, DockspaceAxis::Horizontal] {
        let scene = present(&mut native, binding);
        let (_, visual, receiver) = *scene
            .splitters
            .iter()
            .find(|splitter| splitter.0 == axis)
            .unwrap();
        let splitter_count = scene.splitters.len();
        let bounds = receiver.bounds();
        let before_center = receiver.center();
        let start = [0.25, 0.75, 0.125, 0.875]
            .into_iter()
            .map(|fraction| {
                LogicalPoint::new(
                    bounds.x()
                        + bounds.width()
                            * if axis == DockspaceAxis::Vertical {
                                fraction
                            } else {
                                0.5
                            },
                    bounds.y()
                        + bounds.height()
                            * if axis == DockspaceAxis::Horizontal {
                                fraction
                            } else {
                                0.5
                            },
                )
                .unwrap()
            })
            .find(|point| {
                !scene.junctions.iter().any(|bounds| bounds.contains(*point))
                    && !scene.splitters.iter().any(|(_, other_visual, other)| {
                        *other_visual != visual && other.bounds().contains(*point)
                    })
            })
            .expect("the handle must expose a point outside every junction and peer handle");
        let target = LogicalPoint::new(
            start.x()
                + if axis == DockspaceAxis::Horizontal {
                    18.0
                } else {
                    0.0
                },
            start.y()
                + if axis == DockspaceAxis::Vertical {
                    18.0
                } else {
                    0.0
                },
        )
        .unwrap();
        let before = native.session.version();
        for (point, state) in [
            (start, Some(ElementState::Pressed)),
            (target, None),
            (target, Some(ElementState::Released)),
        ] {
            let event = if desktop {
                raw_desktop_event(point, state)
            } else {
                raw_event(point, state, 2.0)
            };
            let report = apply(
                &mut native,
                binding,
                &scene,
                &receiver,
                &mut ordinal,
                event,
                true,
            );
            if state == Some(ElementState::Released) {
                assert!(
                    matches!(
                        report.manipulation_feedback(),
                        [feedback] if feedback.status() == Status::Delivered(Delivery::SplitterResize)
                    ),
                    "desktop={desktop}, late_output={late_output}, axis={axis:?}, raw_ordinal={ordinal}: {report:?}"
                );
            }
        }
        assert_ne!(native.session.version(), before);
        assert!(!native.session.has_active_gesture());
        assert_inventory(&native, 0);
        let after = present(&mut native, binding);
        assert_eq!(after.splitters.len(), splitter_count);
        assert_ne!(
            after
                .splitters
                .iter()
                .find(|splitter| splitter.1 == visual)
                .unwrap()
                .2
                .center(),
            before_center
        );
    }
    assert!(native.session.is_current_native_binding(binding));
    assert!(!native.session.has_pending_native_input());
    assert_eq!(native.bridge.input_backlog_for_test(), (0, None));
    assert!(native.pointer_translator.can_coalesce_idle_cursor_moves());
}

#[test]
fn draining_a_central_contained_editor_keeps_both_nested_splitters_operable() {
    for desktop in [false, true] {
        for items in [[THIRD, INSPECTOR], [INSPECTOR, THIRD]] {
            drain_and_resize(desktop, false, items);
        }
    }
}

#[test]
fn a_late_output_from_the_drained_contained_root_cannot_poison_splitter_input() {
    for desktop in [false, true] {
        for items in [[THIRD, INSPECTOR], [INSPECTOR, THIRD]] {
            drain_and_resize(desktop, true, items);
        }
    }
}
