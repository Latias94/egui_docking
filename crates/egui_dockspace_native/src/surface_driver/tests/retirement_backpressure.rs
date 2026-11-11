//! Retirement backpressure commits accepted native input before retrying lanes.

use std::num::NonZeroUsize;

use dockspace::geometry::{LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, NativeCloseState, NativeInputStorageLimit,
    NativeMonotonicClock, NativePointerRoster, NativeProducerStorageLimit, NativeReceiverAnswer,
    NativeReportStorageLimit, NativeSurfacePlacement, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaRoster, SurfaceUnavailableReason,
};
use eframe::egui::{self, ViewportId};
use egui_dockspace::DockStyle;
use winit::window::WindowId;

use super::super::{NativeCoordinator, NativeRuntimeState, ROOT_WINDOW_TOKEN};
use super::{SURFACE, TestPanes};
use crate::{NativeStorageLimit, NativeWindowClosePolicy};

const SURFACES: [SurfaceId; 5] = [
    SURFACE,
    SurfaceId::new(2),
    SurfaceId::new(3),
    SurfaceId::new(4),
    SurfaceId::new(5),
];

fn layout() -> DockspaceLayout {
    DockspaceLayout::new(SURFACES.map(|surface| {
        DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(surface.get()),
                DockspaceNode::central_tabs([ItemId::new(surface.get())]),
            ),
        )
    }))
    .unwrap()
}

fn state() -> NativeRuntimeState<TestPanes> {
    let input = NativeInputStorageLimit::new(NonZeroUsize::new(4).unwrap(), 1 << 20)
        .with_producer_storage_limit(NativeProducerStorageLimit::new(
            NonZeroUsize::new(16).unwrap(),
            NonZeroUsize::MIN,
            1 << 20,
        ))
        .with_report_storage_limit(NativeReportStorageLimit::new(
            NonZeroUsize::new(16).unwrap(),
            NonZeroUsize::new(8).unwrap(),
            1 << 20,
        ));
    let storage = NativeStorageLimit::new(input, 16 << 20, 2, 1).unwrap();
    let session =
        DockspaceSession::from_layout(layout(), dockspace::policy::DockPolicy::default()).unwrap();
    let mut coordinator = NativeCoordinator::new_with_combined_storage_limit_and_clock(
        session,
        NativePointerRoster::Exact(Vec::new()),
        storage.combined,
        NativeMonotonicClock::new(|| std::time::Duration::ZERO),
    )
    .unwrap();
    coordinator
        .register_native_root(SURFACE, ROOT_WINDOW_TOKEN)
        .unwrap();
    NativeRuntimeState {
        coordinator,
        root_surface: SURFACE,
        instance_id: egui::Id::unique("retirement-service-input-backpressure"),
        style: DockStyle::default(),
        panes: TestPanes,
        application_actions: Default::default(),
        pass_actions: Default::default(),
        owned_update: None,
        root_placements: Default::default(),
        pending_service: false,
        close_policy: NativeWindowClosePolicy::Cancel,
        root_context: None,
        shutdown: None,
    }
}

fn commit_prefix(state: &mut NativeRuntimeState<TestPanes>) -> HostFrameReport {
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame
        .commit_publishing_root(&mut state.root_surface)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
        .root_placements
        .commit(&state.coordinator, state.root_surface, &report);
    report
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "the service regression retains five exact Root lanes across a real reset and two bounded input commits"
)]
fn service_native_events_commits_retirement_prefix_before_retrying_the_remaining_root_lane() {
    let mut state = state();
    let context = egui::Context::default();
    let original = commit_prefix(&mut state).native_bindings()[0];
    state
        .coordinator
        .bind_viewport(ViewportId::ROOT, WindowId::from(61), original)
        .unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    state
        .coordinator
        .report_snapshot(
            [(
                original,
                NativeWindowFacts::live()
                    .with_content_bounds(PhysicalRect::new(0.0, 0.0, 800.0, 480.0).unwrap())
                    .with_outer_bounds(PhysicalRect::new(0.0, 0.0, 800.0, 480.0).unwrap())
                    .with_native_scale_factor(scale)
                    .with_presentation_scale_factor(scale)
                    .with_input(NativeWindowInputState::ReceivesInput, None)
                    .with_presentation(NativeWindowPresentationState::Visible)
                    .with_close(NativeCloseState::Clear, None),
            )],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::Windows),
        )
        .unwrap();
    commit_prefix(&mut state);
    let placements = SURFACES
        .into_iter()
        .zip([0.0, 160.0, 320.0, 480.0, 640.0])
        .map(|(surface, x)| {
            NativeSurfacePlacement::new(
                surface,
                LogicalSize::new(160.0, 480.0).unwrap(),
                PhysicalPoint::new(x, 0.0).unwrap(),
                scale,
                PhysicalRect::new(x, 0.0, 160.0, 480.0).unwrap(),
            )
        })
        .collect::<Vec<_>>();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .set_native_surface_placements(original.window(), &placements)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame
        .commit_publishing_root(&mut state.root_surface)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
        .root_placements
        .commit(&state.coordinator, state.root_surface, &report);
    state.service_native_events(&context).unwrap();
    let predecessors = state
        .coordinator
        .session()
        .native_surface_bindings_for_window(original.window())
        .collect::<Vec<_>>();
    assert_eq!(predecessors.len(), 5);
    let reset = state
        .coordinator
        .session()
        .prepare_layout_reset(layout())
        .unwrap();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.submit_layout_reset(&reset).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame
        .commit_publishing_root(&mut state.root_surface)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
        .root_placements
        .commit(&state.coordinator, state.root_surface, &report);
    let successors = report.native_bindings().to_vec();
    let bridge = state.coordinator.dispatch_bridge();
    let candidates = bridge.retirement_quiescence_candidates();
    assert_eq!(candidates.len(), predecessors.len());
    assert!(
        candidates
            .iter()
            .all(|binding| predecessors.contains(binding))
    );
    assert!(!state.coordinator.session().has_pending_native_input());
    let recorded = state.coordinator.session().native_input_recorded_count();

    // The fifth append must wait for the first four to commit inside service.
    state.service_native_events(&context).unwrap();
    assert_eq!(
        state.coordinator.session().native_input_recorded_count(),
        recorded + 4
    );
    assert!(!state.coordinator.session().has_pending_native_input());
    assert_eq!(bridge.retirement_quiescence_candidates().len(), 1);
    assert_eq!(
        predecessors
            .iter()
            .filter(|&&binding| state
                .coordinator
                .session()
                .recognizes_native_binding(binding))
            .count(),
        1
    );

    state.service_native_events(&context).unwrap();
    assert_eq!(
        state.coordinator.session().native_input_recorded_count(),
        recorded + 5
    );
    assert!(!state.coordinator.session().has_pending_native_input());
    assert!(!bridge.retirement_has_pending_work());
    assert!(predecessors.iter().all(|&binding| {
        !state
            .coordinator
            .session()
            .recognizes_native_binding(binding)
    }));
    assert!(successors.iter().all(|&binding| {
        state
            .coordinator
            .session()
            .is_current_native_binding(binding)
    }));
    assert!(
        state
            .coordinator
            .viewport_binding(ViewportId::ROOT)
            .is_some()
    );
    assert!(state.error().is_none());
}
