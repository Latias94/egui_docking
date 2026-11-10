//! Native stationary scrolling and device-specific hit proofs through public input.

use dockspace::runtime::{NativeWindowBinding, NativeWindowHit, SurfacePresentationArtifact};
use std::cell::Cell;

use std::time::Duration;

use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockspaceHostFrame, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceSession, DockspaceTouchEvent, HostFrameReport, HostInputOutcome, HostWindowToken,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeHostCapabilities,
    NativeHostCapability, NativeLocalGeometry, NativeLocalPointerFact, NativeLocalPointerLocation,
    NativePointerButton as Button, NativePointerEvent as Event, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery, NativeSurfaceBinding,
    NativeWindowFacts, NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaRoster,
    PaintedSurfaceOutput, PointerDeviceKind, PointerEventMetadata,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationResult as Disposition, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const POINTER: NativePointerId = NativePointerId::new(1);

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    )
}

struct Scene {
    presentation: Cell<Option<(NativeWindowBinding, SurfacePresentationArtifact)>>,
    recording: RecordingRenderer,
    first_tab: Option<DockspaceReceiverDescriptor>,
    bar: Option<(LogicalRect, f64, f64)>,
    splitter: Option<DockspaceReceiverDescriptor>,
}

impl Scene {
    fn receiver(&self, item: ItemId) -> DockspaceReceiverDescriptor {
        assert_eq!(item, ItemId::new(1));
        self.first_tab.unwrap()
    }

    fn answer(&self, query: impl Into<NativeReceiverQuery>) -> NativeReceiverAnswer {
        let query = match query.into() {
            NativeReceiverQuery::WindowHit(query) => {
                let (window, artifact) = self
                    .presentation
                    .get()
                    .expect("a receiver scene must retain its exact painted output");
                return NativeReceiverAnswer::WindowHit(if query.window() == window {
                    NativeWindowHit::Surface(artifact)
                } else {
                    NativeWindowHit::Unknown
                });
            }
            NativeReceiverQuery::Surface(query) => query,
        };
        assert_eq!(query.surface(), SURFACE);
        let Some(point) = query.point() else {
            return NativeReceiverAnswer::Unknown;
        };
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            return self
                .recording
                .records
                .iter()
                .find_map(|record| match record {
                    OwnedRecord::Receiver(receiver)
                        if matches!(
                            receiver.role(),
                            DockspaceReceiverRole::TabBody | DockspaceReceiverRole::TabStripScroll
                        ) && receiver.bounds().contains(point) =>
                    {
                        query.clear_hover_lane(receiver)
                    }
                    _ => None,
                })
                .map_or(
                    NativeReceiverAnswer::Unknown,
                    NativeReceiverAnswer::ClearHoverLane,
                );
        }
        // The touch-only splitter fixture uses the actual expanded hit query;
        // its paint descriptor intentionally retains only the base bounds.
        if query.metadata().device() == PointerDeviceKind::Touch
            && let Some(splitter) = self.splitter
        {
            return query
                .bind_receiver(&splitter)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock);
        }
        self.recording
            .records
            .iter()
            .find_map(|record| match record {
                OwnedRecord::Receiver(receiver)
                    if receiver.role() == DockspaceReceiverRole::TabBody
                        && receiver.bounds().contains(point) =>
                {
                    query.bind_receiver(receiver)
                }
                _ => None,
            })
            .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
    }
}

fn render(frame: &mut DockspaceHostFrame<'_>) -> Scene {
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let first_tab = plan.tab_receiver(ItemId::new(1));
    let mut bar = None;
    let mut splitter = None;
    let mut registrations = Vec::new();
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::TabBar(record),
                ..
            } => {
                if bar.is_none() {
                    bar = Some((
                        record.viewport(),
                        record.scroll_offset(),
                        record.maximum_scroll_offset(),
                    ));
                }
                if let Some(receiver) = plan.receiver_for_tab_strip_scroll(record) {
                    registrations.push(receiver);
                }
            }
            Step::Operation {
                operation: Operation::Splitter(record),
                ..
            } => {
                splitter = plan.receiver_for_splitter(record);
                registrations.extend(splitter);
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (mut recording, receipt) = RecordingRenderer::execute(plan).unwrap();
    for receiver in registrations {
        recording
            .records
            .insert(recording.records.len() - 1, OwnedRecord::Receiver(receiver));
    }
    frame.confirm_surface_painted(receipt).unwrap();
    Scene {
        presentation: Cell::new(None),
        recording,
        first_tab,
        bar,
        splitter,
    }
}

fn paint(session: &mut DockspaceSession) -> (Scene, PaintedSurfaceOutput) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let scene = render(&mut frame);
    let mut report = finish(frame);
    let [output] = report.take_painted_outputs().try_into().unwrap();
    assert!(
        matches!(scene.recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic))
    );
    let [window]: [NativeWindowBinding; 1] = session
        .native_window_bindings()
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    scene
        .presentation
        .set(Some((window, output.presentation_artifact())));
    (scene, output)
}

fn present(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
    disposition: Disposition,
) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    session
        .report_surface_presentation(output, disposition)
        .unwrap();
    let report = idle(session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.settles(artifact))
    );
    report
}

fn geometry(width: f64) -> NativeLocalGeometry {
    NativeLocalGeometry::new(
        PhysicalSize::new(width, 600.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    )
}

fn fixture(desktop: bool, split: bool) -> (DockspaceSession, NativeSurfaceBinding, Scene) {
    let width = if split { 800.0 } else { 240.0 };
    let node = if split {
        DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [
                DockspaceNode::tabs([ItemId::new(1)]),
                DockspaceNode::tabs([ItemId::new(2)]),
            ],
        )
        .unwrap()
    } else {
        DockspaceNode::central_tabs((1..9).map(ItemId::new))
    };
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), node),
    )])
    .unwrap();
    let config = DockPresentationConfig::builder()
        .hover_selection_delay(None)
        .build()
        .unwrap();
    let mut session = DockspaceSession::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        config,
    )
    .unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    let mut capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    if desktop {
        capabilities = capabilities
            .with(NativeHostCapability::HoveredWindow)
            .with(NativeHostCapability::DesktopPointerPosition);
    }
    session
        .configure_managed_native_capabilities(capabilities)
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(1))
        .unwrap();
    let binding = idle(&mut session).native_bindings()[0];
    let geometry = geometry(width);
    let mut facts = NativeWindowFacts::live()
        .with_content_size(geometry.content_size())
        .with_native_scale_factor(geometry.native_scale_factor())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    if desktop {
        facts = facts.with_content_bounds(PhysicalRect::new(400.0, -200.0, width, 600.0).unwrap());
    }
    session
        .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, width, 600.0).unwrap(),
                LogicalSize::new(32.0, 24.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    finish(frame);
    let (scene, output) = paint(&mut session);
    present(&mut session, output, Disposition::Presented);
    (session, binding, scene)
}

fn input(
    session: &DockspaceSession,
    binding: NativeSurfaceBinding,
    desktop: bool,
    event: Event,
    point: LogicalPoint,
    millis: u64,
    device: PointerDeviceKind,
) -> NativePointerInput {
    let capture = if matches!(event, Event::ButtonReleased(_) | Event::ContactEnded(_)) {
        NativePointerOwner::None
    } else {
        NativePointerOwner::Native(binding.window())
    };
    let input = if desktop {
        NativePointerInput::new(
            POINTER,
            event,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(
                    PhysicalPoint::new(400.0 + point.x(), -200.0 + point.y()).unwrap(),
                ),
                NativePointerHover::Dock(binding.window()),
                None,
            ),
            NativePointerOwner::Native(binding.window()),
            capture,
        )
    } else {
        let coordinates = session.native_local_coordinates(binding).unwrap();
        let geometry = coordinates.geometry();
        let sample = coordinates
            .capture(
                binding.window(),
                NativeLocalPointerFact::new(
                    PhysicalPoint::new(point.x(), point.y()).unwrap(),
                    geometry.content_size(),
                    geometry.native_scale_factor(),
                ),
                geometry,
            )
            .unwrap();
        NativePointerInput::new_local(
            POINTER,
            event,
            NativeLocalPointerLocation::Exact(sample),
            NativePointerOwner::Native(binding.window()),
            capture,
        )
    };
    input.with_metadata(PointerEventMetadata::new(
        device,
        Duration::from_millis(millis),
    ))
}

#[test]
fn native_edge_scroll_retries_and_waits_for_each_exact_output_without_motion() {
    for desktop in [false, true] {
        let (mut session, binding, scene) = fixture(desktop, false);
        let version = session.version();
        let source = scene.receiver(ItemId::new(1)).center();
        let (viewport, _, maximum) = scene.bar.unwrap();
        assert!(maximum > 80.0);
        let edge = point(viewport.max().x() - 1.0, viewport.y() + 1.0);
        session
            .record_native_pointer_batch([
                input(
                    &session,
                    binding,
                    desktop,
                    Event::ButtonPressed(Button::Primary),
                    source,
                    0,
                    PointerDeviceKind::Mouse,
                ),
                input(
                    &session,
                    binding,
                    desktop,
                    Event::Moved,
                    edge,
                    10,
                    PointerDeviceKind::Mouse,
                ),
            ])
            .unwrap();
        let mut queries = Vec::new();
        let report = finish(
            session
                .begin_native_host_frame(|query| {
                    let query = match query {
                        NativeReceiverQuery::WindowHit(_) => return scene.answer(query),
                        NativeReceiverQuery::Surface(query) => query,
                    };

                    let answer = scene.answer(query);
                    queries.push((query, answer));
                    answer
                })
                .unwrap(),
        );
        assert!(
            session.has_active_gesture(),
            "desktop={desktop}: {report:?}, {queries:?}"
        );
        let wake = session.native_pointer_continuation().unwrap_or_else(|| {
            panic!(
                "desktop={desktop}, edge={edge:?}, viewport={viewport:?}: {report:?}, {queries:?}"
            )
        });
        assert_eq!(wake.deadline(), Duration::from_millis(60));
        assert_eq!(wake.source_binding(), binding);
        assert_eq!(wake.local_coordinates().is_none(), desktop);
        assert_eq!(wake.position(), edge);
        assert!(session.pointer_continuation().is_none());
        let recorded = session.native_input_recorded_count();
        assert_eq!(session.native_pointer_continuation(), Some(wake));
        assert_eq!(session.native_input_recorded_count(), recorded);

        session
            .record_native_pointer(input(
                &session,
                binding,
                desktop,
                wake.event(),
                edge,
                60,
                PointerDeviceKind::Mouse,
            ))
            .unwrap();
        assert!(session.native_pointer_continuation().is_none());
        {
            let mut abandoned = session
                .begin_native_host_frame(|query| scene.answer(query))
                .unwrap();
            assert_eq!(render(&mut abandoned).bar.unwrap().1, 40.0);
        }
        assert_eq!(session.version(), version);
        let mut frame = session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap();
        let scrolled = render(&mut frame);
        assert_eq!(scrolled.bar.unwrap().1, 40.0);
        let mut report = finish(frame);
        assert!(!report.workspace_changed());
        assert!(session.native_pointer_continuation().is_none());
        assert!(session.native_pointer_continuation_awaits_presentation(SURFACE));
        assert!(!session.pointer_continuation_awaits_presentation(SURFACE));
        let [exact] = report.take_painted_outputs().try_into().unwrap();
        scrolled
            .presentation
            .set(Some((binding.window(), exact.presentation_artifact())));
        present(&mut session, exact, Disposition::Presented);
        let next = session.native_pointer_continuation().unwrap();
        assert_ne!(next.token(), wake.token());
        assert_eq!(next.deadline(), Duration::from_millis(110));
        session
            .record_native_pointer(input(
                &session,
                binding,
                desktop,
                next.event(),
                edge,
                110,
                PointerDeviceKind::Mouse,
            ))
            .unwrap();
        let mut frame = session
            .begin_native_host_frame(|query| scrolled.answer(query))
            .unwrap();
        assert_eq!(render(&mut frame).bar.unwrap().1, 80.0);
        let mut report = finish(frame);
        assert!(!report.workspace_changed());
        let [dropped] = report.take_painted_outputs().try_into().unwrap();
        present(&mut session, dropped, Disposition::Dropped);
        assert!(session.native_pointer_continuation().is_none());
        assert_eq!(session.version(), version);
        session
            .record_native_pointer(input(
                &session,
                binding,
                desktop,
                Event::ContactEnded(Button::Primary),
                edge,
                120,
                PointerDeviceKind::Mouse,
            ))
            .unwrap();
        let report = finish(
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap(),
        );
        assert!(!report.workspace_changed());
        assert!(!session.has_active_gesture());
        assert_eq!(session.version(), version);
        assert_eq!(
            session.view().item(ItemId::new(1)).unwrap().tabs().items(),
            (1..9).map(ItemId::new).collect::<Vec<_>>()
        );
    }
}

#[test]
fn native_desktop_touch_expanded_splitter_uses_original_contact_for_long_press_and_resize() {
    let (mut session, binding, scene) = fixture(true, true);
    let descriptor = scene.splitter.unwrap();
    let press = point(descriptor.center().x() + 21.0, 300.0);
    assert!(!descriptor.bounds().contains(press));
    let touch = session
        .host_pointer_hit_for_device(SURFACE, press, PointerDeviceKind::Touch)
        .unwrap();
    assert_eq!(touch.drag_delivery().unwrap().descriptor(), descriptor);
    let mouse = session
        .host_pointer_hit_for_device(SURFACE, press, PointerDeviceKind::Mouse)
        .unwrap();
    assert!(
        mouse
            .drag_delivery()
            .is_none_or(|receiver| receiver.descriptor() != descriptor)
    );
    let version = session.version();
    session
        .record_native_pointer(input(
            &session,
            binding,
            true,
            Event::ButtonPressed(Button::Primary),
            press,
            0,
            PointerDeviceKind::Touch,
        ))
        .unwrap();
    let report = finish(
        session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap(),
    );
    assert!(!report.workspace_changed());
    let wake = session.native_pointer_continuation().unwrap();
    assert_eq!(wake.deadline(), Duration::from_millis(350));
    assert_eq!(wake.position(), press);
    session
        .record_native_pointer(input(
            &session,
            binding,
            true,
            wake.event(),
            press,
            350,
            PointerDeviceKind::Touch,
        ))
        .unwrap();
    let report = finish(
        session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap(),
    );
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::Touch(DockspaceTouchEvent::Armed { .. })]
    ));
    assert_eq!(session.version(), version);
    let moved = point(press.x() + 28.0, press.y());
    session
        .record_native_pointer(input(
            &session,
            binding,
            true,
            Event::Moved,
            moved,
            360,
            PointerDeviceKind::Touch,
        ))
        .unwrap();
    let report = finish(
        session
            .begin_native_host_frame(|query| scene.answer(query))
            .unwrap(),
    );
    assert!(!report.workspace_changed());
    assert!(session.has_active_gesture());
    session
        .record_native_pointer(input(
            &session,
            binding,
            true,
            Event::ContactEnded(Button::Primary),
            moved,
            370,
            PointerDeviceKind::Touch,
        ))
        .unwrap();
    let report = finish(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(report.workspace_changed());
    assert!(!session.has_active_gesture());
    assert_ne!(session.version(), version);
    assert!(session.native_pointer_continuation().is_none());
}
