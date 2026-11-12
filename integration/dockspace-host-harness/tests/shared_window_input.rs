//! One physical input stream resolves independent exact placements in a shared window.

use std::time::Duration;

use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, PhysicalSize, ScaleFactor,
};
use dockspace::model::{
    DockAnchor, DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, NativeTearOffSource,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    DockspacePresentationCommand, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceSession, HostFrameReport, HostWindowToken, HostWorkAreaToken, NativeCreateReadiness,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeDragSourceInput,
    NativeEffectAcknowledgement, NativeEffectOperation, NativeHostCapabilities,
    NativeHostCapability, NativeLocalGeometry, NativeLocalPointerFact, NativeLocalPointerLocation,
    NativeLocalToDesktopHandoff, NativePointerButton, NativePointerEvent, NativePointerHover,
    NativePointerId, NativePointerInput, NativePointerOwner, NativePointerRoster,
    NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery, NativeScrollDelta,
    NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers, NativeScrollMomentum,
    NativeScrollPhase, NativeScrollSequenceId, NativeSurfaceBinding, NativeSurfacePlacement,
    NativeSurfaceReceiverQuery, NativeWindowBinding, NativeWindowFacts, NativeWindowHit,
    NativeWindowHitQuery, NativeWindowInputState, NativeWindowPresentationState,
    NativeWorkAreaFacts, NativeWorkAreaRoster, PointerDeviceKind, PointerEventMetadata,
    SurfacePaintProgramOperation, SurfacePaintProgramStep, SurfacePresentationArtifact,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const OTHER: ItemId = ItemId::new(3);
const RETAINED: ItemId = ItemId::new(4);
const POINTER: NativePointerId = NativePointerId::new(7);

fn point(x: f64, y: f64) -> LogicalPoint {
    LogicalPoint::new(x, y).unwrap()
}

fn physical(x: f64, y: f64) -> PhysicalPoint {
    PhysicalPoint::new(x, y).unwrap()
}

fn scale(value: f64) -> ScaleFactor {
    ScaleFactor::new(value).unwrap()
}

fn geometry() -> NativeLocalGeometry {
    NativeLocalGeometry::new(PhysicalSize::new(1800.0, 1000.0).unwrap(), scale(2.0))
}

fn placements() -> [NativeSurfacePlacement; 2] {
    [
        NativeSurfacePlacement::new(
            A,
            LogicalSize::new(500.0, 400.0).unwrap(),
            physical(20.0, 30.0),
            scale(1.5),
            PhysicalRect::new(20.0, 30.0, 750.0, 600.0).unwrap(),
        ),
        NativeSurfacePlacement::new(
            B,
            LogicalSize::new(400.0, 300.0).unwrap(),
            physical(920.0, 50.0),
            scale(2.0),
            PhysicalRect::new(920.0, 50.0, 800.0, 600.0).unwrap(),
        ),
    ]
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
    binding: NativeSurfaceBinding,
    placement: NativeSurfacePlacement,
    artifact: SurfacePresentationArtifact,
    tabs: Vec<(ItemId, DockspaceReceiverDescriptor, LogicalRect)>,
    scroll: Option<(DockspaceReceiverDescriptor, f64, f64)>,
}

impl Scene {
    fn tab(&self, item: ItemId) -> DockspaceReceiverDescriptor {
        self.tabs.iter().find(|entry| entry.0 == item).unwrap().1
    }

    fn insertion(&self, item: ItemId) -> LogicalPoint {
        let (_, receiver, visible) = self.tabs.iter().find(|entry| entry.0 == item).unwrap();
        let point = point(
            visible.x() + visible.width() * 0.625,
            visible.y() + visible.height() * 0.5,
        );
        assert!(receiver.bounds().contains(point));
        point
    }

    fn client(&self, point: LogicalPoint) -> PhysicalPoint {
        physical(
            self.placement.origin().x() + point.x() * self.placement.scale_factor().get(),
            self.placement.origin().y() + point.y() * self.placement.scale_factor().get(),
        )
    }

    fn hit(&self, query: NativeWindowHitQuery) -> Option<NativeWindowHit> {
        if query.window() != self.binding.window() || !self.placement.clip().contains(query.point())
        {
            return None;
        }
        Some(NativeWindowHit::Surface(self.artifact))
    }

    fn answer(&self, query: NativeSurfaceReceiverQuery) -> NativeReceiverAnswer {
        assert!(query.matches_native_binding(self.binding));
        let Some(point) = query.point() else {
            return NativeReceiverAnswer::Unknown;
        };
        if matches!(query.purpose(), NativeReceiverPurpose::ScrollDelivery(_)) {
            let Some((receiver, _, _)) = self.scroll else {
                return NativeReceiverAnswer::Unknown;
            };
            assert!(receiver.bounds().contains(point));
            return query
                .bind_receiver(&receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock);
        }
        let Some((_, receiver, _)) = self
            .tabs
            .iter()
            .find(|entry| entry.1.bounds().contains(point))
        else {
            return NativeReceiverAnswer::Unknown;
        };
        assert_eq!(query.client_position(), Some(self.client(point)));
        if matches!(query.purpose(), NativeReceiverPurpose::HoverHit) {
            query.clear_hover_lane(receiver).map_or(
                NativeReceiverAnswer::Unknown,
                NativeReceiverAnswer::ClearHoverLane,
            )
        } else {
            query
                .bind_receiver(receiver)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        }
    }
}

fn resolve(scenes: &[Scene], query: NativeReceiverQuery) -> NativeReceiverAnswer {
    match query {
        NativeReceiverQuery::WindowHit(query) => NativeReceiverAnswer::WindowHit(
            scenes
                .iter()
                .find_map(|scene| scene.hit(query))
                .unwrap_or(NativeWindowHit::WindowBackground),
        ),
        NativeReceiverQuery::Surface(query) => scenes
            .iter()
            .find(|scene| scene.binding.surface() == query.surface())
            .map_or(NativeReceiverAnswer::Unknown, |scene| scene.answer(query)),
    }
}

fn paint_all(
    session: &mut DockspaceSession,
    window: NativeWindowBinding,
    placements: &[NativeSurfacePlacement],
) -> Vec<Scene> {
    paint_all_with_report(session, window, placements).0
}

fn paint_all_with_report(
    session: &mut DockspaceSession,
    window: NativeWindowBinding,
    placements: &[NativeSurfacePlacement],
) -> (Vec<Scene>, HostFrameReport) {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let bindings = frame
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();
    let mut records = Vec::new();
    for placement in placements {
        let surface = placement.surface();
        frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::from_min_size(point(0.0, 0.0), placement.size()).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
        let plan = frame.paint_plan(surface).unwrap().unwrap();
        let mut tabs = Vec::new();
        let mut scroll = None;
        plan.visit_surface_program(|step| {
            match step {
                SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::Tab(tab),
                    ..
                } => {
                    if let Some(receiver) = plan.receiver_for_tab_body(tab) {
                        assert_eq!(receiver.role(), DockspaceReceiverRole::TabBody);
                        tabs.push((tab.item(), receiver, tab.visible_bounds()));
                    }
                }
                SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::TabBar(bar),
                    ..
                } => {
                    if let Some(receiver) = plan.receiver_for_tab_strip_scroll(bar) {
                        scroll = Some((receiver, bar.scroll_offset(), bar.maximum_scroll_offset()));
                    }
                }
                _ => {}
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        let (mut recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        for (_, receiver, _) in &tabs {
            assert!(
                recording
                    .records
                    .contains(&OwnedRecord::Receiver(*receiver))
            );
        }
        if let Some((receiver, _, _)) = scroll {
            recording
                .records
                .insert(recording.records.len() - 1, OwnedRecord::Receiver(receiver));
        }
        frame.confirm_surface_painted(receipt).unwrap();
        records.push((surface, tabs, scroll));
    }
    let mut report = finish(frame);
    let outputs = report.take_painted_outputs();
    let scenes = outputs
        .iter()
        .map(|output| {
            let surface = output.surface();
            let binding = *bindings
                .iter()
                .find(|binding| binding.surface() == surface)
                .unwrap();
            assert!(output.matches_native_binding(binding));
            let index = records.iter().position(|entry| entry.0 == surface).unwrap();
            Scene {
                binding,
                placement: *placements
                    .iter()
                    .find(|entry| entry.surface() == surface)
                    .unwrap(),
                artifact: output.presentation_artifact(),
                tabs: std::mem::take(&mut records[index].1),
                scroll: records[index].2,
            }
        })
        .collect();
    session
        .report_surface_presentations(outputs, SurfacePresentationResult::Presented)
        .unwrap();
    let report = idle(session);
    (scenes, report)
}

fn fixture() -> (DockspaceSession, NativeWindowBinding, Vec<Scene>) {
    fixture_with_overflow(false)
}

fn fixture_with_overflow(overflow: bool) -> (DockspaceSession, NativeWindowBinding, Vec<Scene>) {
    let capabilities = NativeHostCapabilities::none_supported()
        .with(NativeHostCapability::AuthoritativeInventory)
        .with(NativeHostCapability::AuthoritativeButtonState)
        .with(NativeHostCapability::PointerHitTestObservation);
    fixture_with_capabilities(overflow, capabilities)
}

fn fixture_with_capabilities(
    overflow: bool,
    capabilities: NativeHostCapabilities,
) -> (DockspaceSession, NativeWindowBinding, Vec<Scene>) {
    fixture_with_retained_source(overflow, capabilities, false)
}

fn fixture_with_retained_source(
    overflow: bool,
    capabilities: NativeHostCapabilities,
    retain_source: bool,
) -> (DockspaceSession, NativeWindowBinding, Vec<Scene>) {
    let a_items = [FIRST, SECOND]
        .into_iter()
        .chain((10..if overflow { 50 } else { 10 }).map(ItemId::new));
    let b_items = [OTHER]
        .into_iter()
        .chain((100..if overflow { 140 } else { 100 }).map(ItemId::new));
    let source = DockspaceSurfaceLayout::new(
        A,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs(a_items)),
    );
    let source = if retain_source {
        source.with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([RETAINED])),
            LogicalRect::new(100.0, 200.0, 280.0, 160.0).unwrap(),
        ))
    } else {
        source
    };
    let layout = DockspaceLayout::new([
        source,
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs(b_items)),
        ),
    ])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .unwrap();
    session
        .configure_managed_native_capabilities(capabilities)
        .unwrap();
    session
        .register_native_root(A, HostWindowToken::new(1))
        .unwrap();
    let registration = idle(&mut session);
    let window = registration.native_bindings()[0].window();
    session
        .report_managed_native_snapshot(
            [(
                window,
                NativeWindowFacts::live()
                    .with_content_size(geometry().content_size())
                    .with_native_scale_factor(geometry().native_scale_factor())
                    .with_presentation_scale_factor(scale(1.0))
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut session);
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .set_native_surface_placements(window, &placements())
        .unwrap();
    finish(frame);
    let scenes = paint_all(&mut session, window, &placements());
    assert_eq!(
        session.native_window_bindings().collect::<Vec<_>>(),
        [window]
    );
    assert_eq!(
        session.native_surface_bindings_for_window(window).count(),
        2
    );
    (session, window, scenes)
}

fn input(
    session: &DockspaceSession,
    window: NativeWindowBinding,
    event: NativePointerEvent,
    client: PhysicalPoint,
) -> NativePointerInput {
    let cap = session.native_local_coordinates(window).unwrap();
    let geometry = cap.geometry();
    let sample = cap
        .capture(
            window,
            NativeLocalPointerFact::new(
                client,
                geometry.content_size(),
                geometry.native_scale_factor(),
            ),
            geometry,
        )
        .unwrap();
    assert_eq!(sample.client_position(), client);
    NativePointerInput::new_local(
        POINTER,
        event,
        NativeLocalPointerLocation::Exact(sample),
        NativePointerOwner::Native(window),
        if matches!(event, NativePointerEvent::ButtonReleased(_)) {
            NativePointerOwner::None
        } else {
            NativePointerOwner::Native(window)
        },
    )
}

fn drag_inputs(
    session: &DockspaceSession,
    window: NativeWindowBinding,
    scenes: &[Scene],
) -> [NativePointerInput; 3] {
    let source = scenes
        .iter()
        .find(|scene| scene.binding.surface() == A)
        .unwrap();
    let target = scenes
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    let start = source.client(source.tab(FIRST).center());
    let end = target.client(target.insertion(OTHER));
    assert!(!source.placement.clip().contains(end));
    [
        input(
            session,
            window,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            start,
        ),
        input(session, window, NativePointerEvent::Moved, end),
        input(
            session,
            window,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            end,
        ),
    ]
}

#[test]
fn one_window_stream_moves_between_scaled_placements_once_after_candidate_retry() {
    let (mut session, window, scenes) = fixture();
    let before = session.version();
    let events = drag_inputs(&session, window, &scenes);
    session.record_native_pointer_batch(events).unwrap();
    {
        let candidate = session
            .begin_native_host_frame(|query| resolve(&scenes, query))
            .unwrap();
        assert_eq!(candidate.view().item(FIRST).unwrap().surface(), B);
    }
    assert_eq!(session.version(), before);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert!(session.has_pending_native_input());
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    let report = finish(frame);
    assert!(report.workspace_changed());
    let [feedback] = report.manipulation_feedback() else {
        panic!("one drag must terminate exactly once");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    assert_eq!(session.view().item(OTHER).unwrap().surface(), B);
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_native_input());
    let committed = session.version();
    assert!(idle(&mut session).manipulation_feedback().is_empty());
    assert_eq!(session.version(), committed);
}

#[test]
fn captured_source_outside_both_clips_can_return_to_another_placement() {
    let (mut session, window, scenes) = fixture();
    let [press, _, release] = drag_inputs(&session, window, &scenes);
    let gap = physical(850.0, 90.0);
    assert!(
        scenes
            .iter()
            .all(|scene| !scene.placement.clip().contains(gap))
    );
    session
        .record_native_pointer_batch([
            press,
            input(&session, window, NativePointerEvent::Moved, gap),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    let report = finish(frame);
    assert!(!report.workspace_changed());
    assert!(session.has_active_gesture());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    let target = scenes
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    session
        .record_native_pointer_batch([
            input(
                &session,
                window,
                NativePointerEvent::Moved,
                target.client(target.insertion(OTHER)),
            ),
            release,
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    assert!(finish(frame).workspace_changed());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert!(!session.has_active_gesture());
}

#[test]
fn changing_target_placement_rejects_its_old_presented_artifact() {
    let (mut session, window, scenes) = fixture();
    let old_b = scenes
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap()
        .binding;
    let mut next = placements();
    next[1] = NativeSurfacePlacement::new(
        B,
        next[1].size(),
        physical(940.0, 50.0),
        next[1].scale_factor(),
        next[1].clip(),
    );
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.set_native_surface_placements(window, &next).unwrap();
    finish(frame);
    assert!(session.native_surface_placement(old_b).is_none());
    let before = session.version();
    session
        .record_native_pointer_batch(drag_inputs(&session, window, &scenes))
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    let report = finish(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), before);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert!(!session.has_active_gesture());
    let current = paint_all(&mut session, window, &next);
    session
        .record_native_pointer_batch(drag_inputs(&session, window, &current))
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&current, query))
        .unwrap();
    assert!(finish(frame).workspace_changed());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
}

#[test]
fn blocked_unknown_and_window_background_never_authorize_desktop_tear_off() {
    for hit in [
        NativeWindowHit::Blocked,
        NativeWindowHit::Unknown,
        NativeWindowHit::WindowBackground,
    ] {
        let (mut session, window, scenes) = fixture();
        let [press, moved, release] = drag_inputs(&session, window, &scenes);
        session.record_native_pointer(press).unwrap();
        let frame = session
            .begin_native_host_frame(|query| resolve(&scenes, query))
            .unwrap();
        finish(frame);
        let before = session.version();
        session
            .record_native_pointer_batch([moved, release])
            .unwrap();
        let frame = session
            .begin_native_host_frame(|query| match query {
                NativeReceiverQuery::WindowHit(_) => NativeReceiverAnswer::WindowHit(hit),
                NativeReceiverQuery::Surface(_) => {
                    panic!("an unresolved window hit cannot select a placement")
                }
            })
            .unwrap();
        let mut report = finish(frame);
        assert!(!report.workspace_changed());
        assert!(report.take_native_effects().is_empty());
        assert!(!session.has_pending_native_create());
        assert_eq!(session.version(), before);
        assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
        assert!(!session.has_active_gesture());
    }
}

#[test]
fn smooth_scroll_stays_on_its_original_placement_when_the_pointer_crosses_to_another() {
    let (mut session, window, scenes) = fixture_with_overflow(true);
    let source = scenes
        .iter()
        .find(|scene| scene.binding.surface() == A)
        .unwrap();
    let target = scenes
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    let (receiver, offset, maximum) = source.scroll.unwrap();
    assert_eq!(offset, 0.0);
    assert!(maximum > 100.0);
    assert_eq!(target.scroll.unwrap().1, 0.0);
    let sequence = NativeScrollSequenceId::new(1);
    let phases = [
        NativeScrollPhase::Begin {
            sequence,
            delta: None,
        },
        NativeScrollPhase::Update {
            sequence,
            delta: NativeScrollDelta::PhysicalPixels { x: -90.0, y: 0.0 },
        },
        NativeScrollPhase::End {
            sequence,
            delta: Some(NativeScrollDelta::PhysicalPixels { x: -60.0, y: 0.0 }),
        },
    ];
    let events = phases
        .into_iter()
        .enumerate()
        .map(|(index, phase)| {
            let client = if index == 0 {
                source.client(receiver.center())
            } else {
                target.client(target.tab(OTHER).center())
            };
            input(
                &session,
                window,
                NativePointerEvent::Scrolled(NativeScrollEvent::new(
                    NativeScrollDeviceId::new(1),
                    phase,
                    NativeScrollMomentum::Direct,
                    NativeScrollModifiers::Exact {
                        shift: false,
                        control: false,
                        alt: false,
                        command: false,
                    },
                )),
                client,
            )
        })
        .collect::<Vec<_>>();
    session.record_native_pointer_batch(events).unwrap();
    let mut window_queries = 0;
    let mut surface_queries = 0;
    let frame = session
        .begin_native_host_frame(|query| {
            match query {
                NativeReceiverQuery::WindowHit(_) => window_queries += 1,
                NativeReceiverQuery::Surface(query) => {
                    surface_queries += 1;
                    assert_eq!(
                        query.surface(),
                        A,
                        "a smooth tail probes the original scroll owner"
                    );
                }
            }
            resolve(&scenes, query)
        })
        .unwrap();
    let report = finish(frame);
    assert!(!report.workspace_changed());
    assert_eq!(window_queries, 1, "only Begin selects a spatial placement");
    assert_eq!(surface_queries, 3);
    assert!(!session.has_pending_native_input());
    let current = paint_all(&mut session, window, &placements());
    let a = current
        .iter()
        .find(|scene| scene.binding.surface() == A)
        .unwrap();
    let b = current
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    assert_eq!(
        a.scroll.unwrap().1,
        100.0,
        "physical deltas use A's scale exactly once"
    );
    assert_eq!(b.scroll.unwrap().1, 0.0);
}

fn native_tear_off_capabilities() -> NativeHostCapabilities {
    [
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::AuthoritativeButtonState,
        NativeHostCapability::PointerHitTestObservation,
        NativeHostCapability::DesktopPointerPosition,
        NativeHostCapability::HoveredWindow,
        NativeHostCapability::NativeWindowLifecycle,
        NativeHostCapability::GlobalWindowPlacement,
        NativeHostCapability::WorkArea,
    ]
    .into_iter()
    .fold(NativeHostCapabilities::none_supported(), |caps, cap| {
        caps.with(cap)
    })
}

#[test]
fn native_group_availability_rejects_removing_its_only_recovery_surface() {
    let (mut session, _, _) = fixture_with_capabilities(false, native_tear_off_capabilities());
    let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    let source = NativeTearOffSource::Tabs(group);
    let placement =
        NativeWindowPlacement::new(PhysicalRect::new(2200.0, 150.0, 600.0, 400.0).unwrap());
    let availability = session
        .native_window_command(source.clone(), placement)
        .unwrap();
    assert_eq!(
        availability.unavailable_reason(),
        Some(dockspace::model::DockspaceActionRejection::NativeUnavailable),
        "a complete group cannot promise recovery into its pruned source surface"
    );
    let action = session.prepare_move_to_new_window(source, placement);
    let before = session.version();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let submitted = frame.submit_prepared_action(action).unwrap();
    let mut report = finish(frame);
    assert!(matches!(
        report.submitted_action_outcome(submitted),
        Some(dockspace::runtime::HostInputOutcome::ProductActionRejected(
            dockspace::model::DockspaceActionRejection::NativeUnavailable
        ))
    ));
    assert!(report.take_native_effects().is_empty());
    assert_eq!(session.version(), before);
    assert!(!session.has_pending_native_create());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the public journey crosses exact native creation, first-live ownership and a real dual-location return"
)]
fn first_live_child_returns_to_a_scaled_shared_placement_through_one_handoff_event() {
    let (mut session, window, _) =
        fixture_with_retained_source(false, native_tear_off_capabilities(), true);
    let root_bounds = PhysicalRect::new(100.0, 80.0, 1800.0, 1000.0).unwrap();
    let root_facts = NativeWindowFacts::live()
        .with_content_bounds(root_bounds)
        .with_outer_bounds(root_bounds)
        .with_native_scale_factor(geometry().native_scale_factor())
        .with_presentation_scale_factor(scale(1.0))
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible);
    let work_areas = || {
        NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
            HostWorkAreaToken::new(1),
            PhysicalRect::new(0.0, 0.0, 4000.0, 1800.0).unwrap(),
            scale(1.0),
        )])
    };
    session
        .report_managed_native_snapshot([(window, root_facts)], work_areas())
        .unwrap();
    idle(&mut session);
    paint_all(&mut session, window, &placements());
    let root_roster = session
        .native_surface_bindings_for_window(window)
        .collect::<Vec<_>>();

    let group = session.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    let child_bounds = PhysicalRect::new(2200.0, 150.0, 600.0, 400.0).unwrap();
    let availability = session
        .native_window_command(
            NativeTearOffSource::Tabs(group),
            NativeWindowPlacement::new(child_bounds),
        )
        .unwrap();
    let DockspacePresentationCommand::Ready(create) = availability else {
        panic!("the exact shared placement must allow group tear-off: {availability:?}");
    };
    let version = session.version();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let submitted = frame.submit_prepared_action(create).unwrap();
    let mut requested = finish(frame);
    assert!(
        matches!(
            requested.submitted_action_outcome(submitted),
            Some(dockspace::runtime::HostInputOutcome::ProductActionApplied(
                dockspace::model::DockspaceActionOutcome::NativeRootTearOffRequested { .. }
            ))
        ),
        "the exact group must start native creation: {:?}",
        requested.inputs()
    );
    let [effect] = requested.take_native_effects().try_into().unwrap();
    let child = match effect.operation() {
        NativeEffectOperation::CreateWindow {
            binding, placement, ..
        } => {
            assert_eq!(*placement, child_bounds);
            *binding
        }
        operation => panic!("the captured group must request native creation, got {operation:?}"),
    };
    let Some(NativeEffectAcknowledgement::Create(created)) = effect.accepted() else {
        panic!("the child must receive an exact create acknowledgement");
    };
    let child_facts = NativeWindowFacts::live()
        .with_content_bounds(child_bounds)
        .with_outer_bounds(child_bounds)
        .with_native_scale_factor(scale(1.0))
        .with_presentation_scale_factor(scale(1.0))
        .with_input(NativeWindowInputState::ReceivesInput, None);
    session
        .report_managed_native_snapshot(
            [
                (window, root_facts),
                (
                    child.window(),
                    child_facts
                        .with_presentation(NativeWindowPresentationState::Hidden)
                        .with_create_readiness(NativeCreateReadiness::Created, Some(created)),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    assert!(idle(&mut session).native_admissions().is_empty());
    assert_eq!(session.version(), version);
    assert_eq!(session.view().item(FIRST).unwrap().surface(), A);
    session
        .report_managed_native_snapshot(
            [
                (window, root_facts),
                (
                    child.window(),
                    child_facts
                        .with_presentation(NativeWindowPresentationState::Visible)
                        .with_create_readiness(NativeCreateReadiness::Prepared, None),
                ),
            ],
            work_areas(),
        )
        .unwrap();
    assert!(idle(&mut session).native_admissions().is_empty());
    assert_eq!(session.view().item(SECOND).unwrap().surface(), A);
    let child_placement = session.native_surface_placement(child).unwrap();
    let (_, mut admitted) = paint_all_with_report(&mut session, child.window(), &[child_placement]);
    assert_eq!(admitted.native_admissions(), [child]);
    for effect in admitted.take_native_effects() {
        assert!(
            matches!(effect.operation(), NativeEffectOperation::RetainChild { binding } if *binding == child)
        );
        assert!(effect.accepted().is_none());
    }
    assert!(!session.has_pending_native_create());
    assert_eq!(
        session.view().item(FIRST).unwrap().surface(),
        child.surface()
    );
    assert_eq!(
        session.view().item(SECOND).unwrap().surface(),
        child.surface()
    );
    assert_eq!(session.view().item(RETAINED).unwrap().surface(), A);
    let shared = paint_all(&mut session, window, &placements());
    let mut child_scenes = paint_all(&mut session, child.window(), &[child_placement]);
    let child_scene = child_scenes.pop().unwrap();
    let target = shared
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    let metadata =
        |millis| PointerEventMetadata::new(PointerDeviceKind::Mouse, Duration::from_millis(millis));
    session
        .record_native_pointer_batch([
            input(
                &session,
                child.window(),
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                child_scene.client(child_scene.tab(FIRST).center()),
            )
            .with_metadata(metadata(10)),
            input(
                &session,
                child.window(),
                NativePointerEvent::Moved,
                child_scene.client(child_scene.insertion(SECOND)),
            )
            .with_metadata(metadata(20)),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(std::slice::from_ref(&child_scene), query))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert!(session.has_active_gesture());

    let target_point = target.insertion(OTHER);
    let target_client = target.client(target_point);
    let desktop = physical(
        root_bounds.x() + target_client.x(),
        root_bounds.y() + target_client.y(),
    );
    let child_client = physical(
        desktop.x() - child_bounds.x(),
        desktop.y() - child_bounds.y(),
    );
    assert!(
        child_client.x() < 0.0,
        "capture must preserve the point outside its source client"
    );
    let coordinates = session.native_local_coordinates(child.window()).unwrap();
    let local = coordinates
        .capture(
            child.window(),
            NativeLocalPointerFact::new(
                child_client,
                coordinates.geometry().content_size(),
                coordinates.geometry().native_scale_factor(),
            ),
            coordinates.geometry(),
        )
        .unwrap();
    let location = NativeDesktopPointerLocation::new(
        NativeDesktopPosition::Exact(desktop),
        NativePointerHover::Dock(window),
        None,
    );
    session
        .record_native_pointer(
            NativePointerInput::new_handoff(
                POINTER,
                NativePointerEvent::Moved,
                NativeLocalToDesktopHandoff::new(local, location),
                NativePointerOwner::Native(child.window()),
                NativePointerOwner::Native(child.window()),
            )
            .with_metadata(metadata(30)),
        )
        .unwrap();
    let mut queries = 0;
    let frame = session
        .begin_native_host_frame(|query| {
            match query {
                NativeReceiverQuery::WindowHit(query) => {
                    queries += 1;
                    assert_eq!(query.window(), window);
                    assert_eq!(query.point(), target_client);
                }
                NativeReceiverQuery::Surface(query) => {
                    assert_eq!(query.surface(), B);
                    assert_eq!(query.point(), Some(target_point));
                    assert_eq!(query.client_position(), Some(target_client));
                    assert!(matches!(query.purpose(), NativeReceiverPurpose::HoverHit));
                }
            }
            resolve(&shared, query)
        })
        .unwrap();
    let mut handed_off = finish(frame);
    assert!(!handed_off.workspace_changed());
    assert!(handed_off.manipulation_feedback().is_empty());
    assert!(handed_off.take_native_effects().is_empty());
    assert_eq!(
        queries, 1,
        "the dual-location event independently selects B in the shared window"
    );
    let capture = session
        .native_pointer_capture_request()
        .expect("the same gesture has upgraded to its desktop lane");
    assert_eq!(capture.pointer(), POINTER);
    assert_eq!(capture.source_binding(), child);
    assert_eq!(capture.source_input(), NativeDragSourceInput::Preserve);
    assert_eq!(
        session.view().item(FIRST).unwrap().surface(),
        child.surface()
    );

    session
        .record_native_pointer(
            NativePointerInput::new(
                POINTER,
                NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
                location,
                NativePointerOwner::Native(child.window()),
                NativePointerOwner::None,
            )
            .with_metadata(metadata(40)),
        )
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&shared, query))
        .unwrap();
    let mut released = finish(frame);
    assert!(released.workspace_changed());
    let [feedback] = released.manipulation_feedback() else {
        panic!("the shared-window return must deliver exactly once");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert!(released.take_native_effects().is_empty());
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert_eq!(
        session.view().item(SECOND).unwrap().surface(),
        child.surface()
    );
    assert_eq!(session.view().item(RETAINED).unwrap().surface(), A);
    assert_eq!(
        session
            .native_surface_bindings_for_window(window)
            .collect::<Vec<_>>(),
        root_roster
    );
    assert!(!session.has_active_gesture());
    assert!(session.native_pointer_capture_request().is_none());
    assert!(!session.has_pending_native_input());
    assert!(idle(&mut session).manipulation_feedback().is_empty());
}

#[test]
fn rebasing_a_scroll_owner_drains_its_tail_without_selecting_a_sibling_placement() {
    let (mut session, window, scenes) = fixture_with_overflow(true);
    let source = scenes
        .iter()
        .find(|scene| scene.binding.surface() == A)
        .unwrap();
    let sequence = NativeScrollSequenceId::new(11);
    let event = |phase| {
        NativePointerEvent::Scrolled(NativeScrollEvent::new(
            NativeScrollDeviceId::new(1),
            phase,
            NativeScrollMomentum::Direct,
            NativeScrollModifiers::Exact {
                shift: false,
                control: false,
                alt: false,
                command: false,
            },
        ))
    };
    session
        .record_native_pointer(input(
            &session,
            window,
            event(NativeScrollPhase::Begin {
                sequence,
                delta: None,
            }),
            source.client(source.scroll.unwrap().0.center()),
        ))
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    assert!(!finish(frame).workspace_changed());

    let mut next = placements();
    next[0] = NativeSurfacePlacement::new(
        A,
        next[0].size(),
        physical(30.0, 30.0),
        next[0].scale_factor(),
        next[0].clip(),
    );
    let old_source = source.binding;
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.set_native_surface_placements(window, &next).unwrap();
    finish(frame);
    assert!(session.native_surface_placement(old_source).is_none());
    let current = paint_all(&mut session, window, &next);
    let target = current
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    let target_client = target.client(target.scroll.unwrap().0.center());
    session
        .record_native_pointer_batch([
            input(
                &session,
                window,
                event(NativeScrollPhase::Update {
                    sequence,
                    delta: NativeScrollDelta::PhysicalPixels { x: 0.0, y: -200.0 },
                }),
                target_client,
            ),
            input(
                &session,
                window,
                event(NativeScrollPhase::End {
                    sequence,
                    delta: None,
                }),
                target_client,
            ),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|_| {
            panic!("a revoked scroll sequence cannot select B from its late tail")
        })
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert!(!session.has_pending_native_input());
    let current = paint_all(&mut session, window, &next);
    assert!(current.iter().all(|scene| scene.scroll.unwrap().1 == 0.0));

    let target = current
        .iter()
        .find(|scene| scene.binding.surface() == B)
        .unwrap();
    let sequence = NativeScrollSequenceId::new(12);
    let client = target.client(target.scroll.unwrap().0.center());
    session
        .record_native_pointer_batch([
            input(
                &session,
                window,
                event(NativeScrollPhase::Begin {
                    sequence,
                    delta: Some(NativeScrollDelta::PhysicalPixels { x: 0.0, y: -200.0 }),
                }),
                client,
            ),
            input(
                &session,
                window,
                event(NativeScrollPhase::End {
                    sequence,
                    delta: None,
                }),
                client,
            ),
        ])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&current, query))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    let painted = paint_all(&mut session, window, &next);
    assert_eq!(
        painted
            .iter()
            .find(|scene| scene.binding.surface() == A)
            .unwrap()
            .scroll
            .unwrap()
            .1,
        0.0
    );
    assert_eq!(
        painted
            .iter()
            .find(|scene| scene.binding.surface() == B)
            .unwrap()
            .scroll
            .unwrap()
            .1,
        100.0
    );
    assert!(!session.has_pending_native_input());
}

#[test]
fn shared_window_input_retirement_is_retryable_after_placement_generation_changes() {
    let (mut session, window, scenes) = fixture();
    let [press, motion, _] = drag_inputs(&session, window, &scenes);
    session
        .record_native_pointer_batch([press, motion])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert!(session.has_active_gesture());
    let version = session.version();

    session.report_native_input_binding_retired(window).unwrap();
    drop(
        session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap(),
    );
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    let frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let report = finish(frame);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        dockspace::runtime::HostInputOutcome::NativeInputBindingRetired { binding }
            if binding.window() == window
    )));
    assert!(!session.has_active_gesture());
    assert_eq!(session.version(), version);
    assert!(!session.has_pending_native_input());

    session
        .report_managed_native_snapshot(
            [(window, NativeWindowFacts::destroyed())],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut session);
    assert!(session.native_local_coordinates(window).is_none());
    assert!(
        scenes
            .iter()
            .all(|scene| { session.presented_surface(scene.binding.surface()).is_none() })
    );
}

#[test]
fn old_shared_window_input_terminals_and_modifiers_cannot_affect_the_new_epoch() {
    let (mut session, old_window, _) = fixture();
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([FIRST, SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::central_tabs([OTHER])),
        ),
    ])
    .unwrap();
    let reset = session.prepare_layout_reset(layout).unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.submit_layout_reset(&reset).unwrap();
    let report = finish(frame);
    let window = report.native_bindings()[0].window();
    assert_ne!(window, old_window);
    assert!(
        report
            .native_bindings()
            .iter()
            .all(|binding| binding.window() == window)
    );
    session
        .report_managed_native_snapshot(
            [(
                window,
                NativeWindowFacts::live()
                    .with_content_size(geometry().content_size())
                    .with_native_scale_factor(geometry().native_scale_factor())
                    .with_presentation_scale_factor(scale(1.0))
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible),
            )],
            NativeWorkAreaRoster::Unknown,
        )
        .unwrap();
    idle(&mut session);
    let scenes = paint_all(&mut session, window, &placements());
    let [press, motion, release] = drag_inputs(&session, window, &scenes);
    session
        .record_native_pointer_batch([press, motion])
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    assert!(!finish(frame).workspace_changed());
    assert!(session.has_active_gesture());

    session
        .report_native_input_binding_retired(old_window)
        .unwrap();
    session
        .report_native_docking_enabled(old_window, false)
        .unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    let report = finish(frame);
    assert!(report.inputs().iter().any(|input| matches!(
        input,
        dockspace::runtime::HostInputOutcome::DockingEnabledUpdated {
            enabled: false,
            changed: false,
        }
    )));
    assert!(session.has_active_gesture());
    assert!(!report.workspace_changed());
    session.record_native_pointer(release).unwrap();
    let frame = session
        .begin_native_host_frame(|query| resolve(&scenes, query))
        .unwrap();
    let report = finish(frame);
    assert!(report.workspace_changed());
    let [feedback] = report.manipulation_feedback() else {
        panic!("only the real successor release may deliver the gesture");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock)
    );
    assert_eq!(session.view().item(FIRST).unwrap().surface(), B);
    assert!(!session.has_active_gesture());
}
