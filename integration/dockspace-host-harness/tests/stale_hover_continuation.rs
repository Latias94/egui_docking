use std::time::Duration;

use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockEdge, DockFraction, DockPlacement, DockspaceContainedLayout, DockspaceLayout,
    DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId,
    RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspaceSession, HostFrameReport, HostPointerInput, HostPointerRoute,
    PointerDeviceKind, PointerEventMetadata, SurfacePointerButton, SurfacePointerCapture,
    SurfacePointerEvent, SurfacePointerId, SurfacePointerReceiverFacts, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const A: SurfaceId = SurfaceId::new(1);
const B: SurfaceId = SurfaceId::new(2);
const C: SurfaceId = SurfaceId::new(3);
const A_ROOT: RootId = RootId::new(1);
const B_ROOT: RootId = RootId::new(2);
const SOURCE_ROOT: RootId = RootId::new(3);
const CONTAINED_ROOT: RootId = RootId::new(4);
const CONTAINED: FloatingPresentationId = FloatingPresentationId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const FIFTH: ItemId = ItemId::new(5);
const SIXTH: ItemId = ItemId::new(6);
const POINTER: SurfacePointerId = SurfacePointerId::new(1);

fn commit(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("unpainted surfaces are deferred");
    frame.commit().expect("host frame commits")
}

fn measure(frame: &mut DockspaceHostFrame<'_>) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("valid bounds"),
        LogicalSize::new(64.0, 48.0).expect("valid minimum"),
        96.0,
    )
    .expect("valid metrics");
    for surface in [A, B, C] {
        frame
            .measure_surface(surface, metrics)
            .expect("surface measures");
    }
}

#[test]
fn moved_contained_root_cannot_poison_its_stale_hover_continuation() {
    stale_hover_after_migration(false);
}

#[test]
fn rehomed_contained_root_cannot_authorize_hover_from_its_previous_surface() {
    stale_hover_after_migration(true);
}

fn stale_hover_after_migration(preserve_root: bool) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            A,
            DockspaceRootLayout::new(A_ROOT, DockspaceNode::central_tabs([SECOND])),
        ),
        DockspaceSurfaceLayout::new(
            B,
            DockspaceRootLayout::new(B_ROOT, DockspaceNode::central_tabs([THIRD])),
        )
        .with_contained(DockspaceContainedLayout::new(
            CONTAINED,
            DockspaceRootLayout::new(
                CONTAINED_ROOT,
                DockspaceNode::tabs_with_selection([FOURTH, FIFTH], Some(FIFTH))
                    .expect("contained tab selection is valid"),
            ),
            LogicalRect::new(180.0, 120.0, 400.0, 320.0).expect("valid contained bounds"),
        )),
        DockspaceSurfaceLayout::new(
            C,
            DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::central_tabs([FIRST, SIXTH])),
        ),
    ])
    .expect("layout validates");
    let mut session =
        DockspaceSession::from_layout(layout, DockPolicy::default()).expect("session initializes");
    session.enable_host_pointer().expect("host pointer enables");
    let mut frame = session
        .begin_host_frame()
        .expect("measurement frame begins");
    measure(&mut frame);
    frame.commit().expect("measurements publish");

    let mut frame = session.begin_host_frame().expect("paint frame begins");
    let source_descriptor = frame
        .paint_plan(C)
        .expect("source plan lookup")
        .expect("source surface is ready")
        .tab_receiver(FIRST)
        .expect("source tab receiver exists");
    let target_descriptor = frame
        .paint_plan(B)
        .expect("target plan lookup")
        .expect("target surface is ready")
        .tab_receiver(FOURTH)
        .expect("contained target tab receiver exists");
    for surface in [A, B, C] {
        let plan = frame
            .paint_plan(surface)
            .expect("plan lookup")
            .expect("surface is ready");
        let (_, receipt) = RecordingRenderer::execute(plan).expect("renderer accepts plan");
        frame
            .confirm_surface_painted(receipt)
            .expect("paint is confirmed");
    }
    for output in frame
        .commit()
        .expect("initial paint commits")
        .take_painted_outputs()
    {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("initial output is presented");
    }
    commit(session.begin_host_frame().expect("presentation settles"));
    let old_target_point = session
        .bind_presented_receiver(&target_descriptor)
        .expect("old contained target initially binds")
        .center();
    let old_target_surface = session.presented_surface(B).unwrap();

    let rehome = preserve_root.then(|| session.prepare_float_root(CONTAINED_ROOT, A));
    let mut frame = session
        .begin_host_frame()
        .expect("root migration frame begins");
    if let Some(rehome) = rehome {
        frame.submit_prepared_action(rehome).unwrap();
    } else {
        frame
            .dock_root_current(
                CONTAINED_ROOT,
                DockPlacement::OuterEdge {
                    root: A_ROOT,
                    edge: DockEdge::Right,
                    fraction: DockFraction::new(0.3).expect("valid split fraction"),
                },
            )
            .expect("contained root moves to another root edge");
    }
    commit(frame);
    assert_eq!(session.view().item(FOURTH).unwrap().surface(), A);
    assert_eq!(
        session.view().item(FOURTH).unwrap().root(),
        if preserve_root {
            CONTAINED_ROOT
        } else {
            A_ROOT
        }
    );
    assert_eq!(
        session.view().item(FOURTH).unwrap().tabs().items(),
        &[FOURTH, FIFTH]
    );
    assert!(
        session
            .bind_presented_receiver(&target_descriptor)
            .is_none()
    );
    let source = session
        .bind_presented_receiver(&source_descriptor)
        .expect("unchanged source receiver remains current");

    let source_surface = session
        .presented_surface(C)
        .expect("old source pixels remain visible");
    let stale_target_surface = old_target_surface;
    let press = HostPointerRoute::new(source_surface, source.center());
    let moved = LogicalPoint::new(700.0, 500.0).expect("valid pointer point");
    let mut frame = session.begin_host_frame().expect("pointer frame begins");
    frame
        .submit_host_pointer_batch([
            HostPointerInput::new(
                POINTER,
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                press,
                press,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&source),
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Mouse,
                Duration::ZERO,
            )),
            HostPointerInput::new(
                POINTER,
                SurfacePointerEvent::Moved,
                HostPointerRoute::new(source_surface, moved),
                HostPointerRoute::new(stale_target_surface, old_target_point),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&stale_target_surface),
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Mouse,
                Duration::from_millis(10),
            )),
        ])
        .expect("source press and stale visible hover are accepted");
    commit(frame);
    assert!(session.has_active_gesture());

    assert!(
        session.pointer_continuation().is_none(),
        "a receiver with changed membership or presentation owner cannot schedule selection"
    );
    let source_surface = session
        .presented_surface(C)
        .expect("source remains presented");
    let stale_target_surface = old_target_surface;
    let version_before_wake = session.version();
    let mut frame = session
        .begin_host_frame()
        .expect("continuation frame begins");
    let result = frame.submit_host_pointer(
        HostPointerInput::new(
            POINTER,
            SurfacePointerEvent::Moved,
            HostPointerRoute::new(source_surface, moved),
            HostPointerRoute::new(stale_target_surface, old_target_point),
            SurfacePointerCapture::ProviderEndpoint,
            SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&stale_target_surface),
        )
        .with_metadata(PointerEventMetadata::new(
            PointerDeviceKind::Mouse,
            Duration::from_secs(5),
        )),
    );
    assert!(
        result.is_ok(),
        "a stale root/tabs/item identity clears its timer locally: {result:?}"
    );
    let report = commit(frame);
    assert!(!report.workspace_changed());
    assert_eq!(session.version(), version_before_wake);
    assert!(session.pointer_continuation().is_none());
    assert!(session.has_active_gesture());
    assert!(!session.view().item(FOURTH).unwrap().is_selected());
}
