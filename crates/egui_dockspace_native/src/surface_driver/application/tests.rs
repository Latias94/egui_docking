//! Real coordinator reductions for the application facade's single command slot.

use crate::NativeCommand;
use dockspace::close::{CloseDecision, DeferredCloseDecision};
use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceActionOutcome, DockspaceActionRejection, DockspaceAxis,
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, FloatingPresentationId, ItemId, NativeTearOffSource,
    NativeWindowPlacement, RootId, SurfaceId,
};
use dockspace::policy::{CloseCapability, DockPolicy};
use dockspace::runtime::{
    DockspaceCloseInertReason, DockspaceCloseResolution, DockspacePresentationTransition,
    DockspacePresentationTransitionResult, DockspaceSession, HostFrameReport, HostInputOutcome,
    HostWorkAreaToken, NativeCloseState, NativeCreateReadiness, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeReceiverAnswer, NativeSurfaceBinding, NativeWindowFacts,
    NativeWindowInputState, NativeWindowPresentationState, NativeWorkAreaFacts,
    NativeWorkAreaRoster, SurfacePresentationResult, SurfaceUnavailableReason,
    UniformSurfaceMetrics,
};
use eframe::egui;
use egui_dockspace::{DockStyle, DockspaceActionStatus, PaneView};

use crate::{NativeActionRequestError, NativeActionStatus, NativeWindowClosePolicy};

use super::NativeRuntimeState;

mod policy;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const CLOSED: ItemId = ItemId::new(4);

struct Panes(String);

impl PaneView for Panes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Application pane".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut egui::Ui) -> egui_dockspace::PaneResponse {
        egui_dockspace::PaneResponse::default()
    }
}

fn state() -> NativeRuntimeState<Panes> {
    state_with_content(DockspaceNode::central_tabs([FIRST, SECOND, THIRD]), false)
}

fn state_with_content(content: DockspaceNode, companion: bool) -> NativeRuntimeState<Panes> {
    let mut surface = DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, content));
    if companion {
        surface = surface.with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(5)])),
            dockspace::geometry::LogicalRect::new(80.0, 80.0, 320.0, 240.0).unwrap(),
        ));
    }
    let layout = DockspaceLayout::new([surface]).unwrap();
    state_with_layout(layout)
}

fn state_with_layout(layout: DockspaceLayout) -> NativeRuntimeState<Panes> {
    let mut policy = DockPolicy::default();
    policy.set_close_capability(CloseCapability::DeferredAllowed);
    state_with_layout_and_policy(layout, policy)
}

fn state_with_layout_and_policy(
    layout: DockspaceLayout,
    policy: DockPolicy,
) -> NativeRuntimeState<Panes> {
    let session = DockspaceSession::from_layout(layout, policy).unwrap();
    let mut state = NativeRuntimeState::new(
        egui::Id::unique("native-layout-commands"),
        session,
        SURFACE,
        Panes("application-owned content".to_owned()),
        DockStyle::default(),
        NativeWindowClosePolicy::Cancel,
    )
    .unwrap();
    let mut initial = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    initial
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = initial.commit().unwrap();
    for input in report.inputs() {
        if let HostInputOutcome::NativeSurfaceRegistered { binding } = input {
            state
                .coordinator
                .bind_viewport(
                    egui::ViewportId::ROOT,
                    winit::window::WindowId::from(1),
                    *binding,
                )
                .unwrap();
        }
    }
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
}

#[test]
fn moving_the_last_noncentral_host_item_to_another_surface_is_refused_before_admission() {
    let target_surface = SurfaceId::new(2);
    let target_item = ItemId::new(9);
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([FIRST])),
        ),
        DockspaceSurfaceLayout::new(
            target_surface,
            DockspaceRootLayout::new(RootId::new(9), DockspaceNode::central_tabs([target_item])),
        ),
    ])
    .unwrap();
    let mut state = state_with_layout(layout);
    let before = state.coordinator.session().version();
    let group = state.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    for command in [
        NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(target_item)),
        },
        NativeCommand::FloatItem {
            item: FIRST,
            surface: target_surface,
        },
        NativeCommand::DockRoot {
            root: ROOT,
            placement: DockPlacement::Center(DockAnchor::Item(target_item)),
        },
        NativeCommand::DockTabs {
            group: group.clone(),
            placement: DockPlacement::Center(DockAnchor::Item(target_item)),
        },
        NativeCommand::FloatTabs {
            group,
            surface: target_surface,
        },
    ] {
        let error = state.request_command(command.clone()).unwrap_err();
        assert_eq!(
            error.reason(),
            NativeActionRequestError::HostSurfaceRequired { surface: SURFACE }
        );
        assert_eq!(error.into_command(), command);
        assert!(!state.application_actions.is_occupied());
        assert_eq!(state.coordinator.session().version(), before);
        assert!(
            state
                .coordinator
                .session()
                .view()
                .surface(SURFACE)
                .is_some()
        );
    }
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: true })
        .unwrap();
    commit_queued(&mut state);
    take(&mut state);
    state
        .request_command(NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(target_item)),
        })
        .unwrap();
    commit_queued(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
            DockspaceActionRejection::PolicyDenied
        ))
    );
}

#[test]
fn host_protection_allows_surviving_presentations_and_keeps_invalid_targets_core_owned() {
    let target_item = ItemId::new(9);
    let target_surface = SurfaceId::new(2);
    for content in [
        DockspaceNode::central_tabs([FIRST]),
        DockspaceNode::tabs([FIRST, SECOND]),
    ] {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, content)),
            DockspaceSurfaceLayout::new(
                target_surface,
                DockspaceRootLayout::new(RootId::new(9), DockspaceNode::tabs([target_item])),
            ),
        ])
        .unwrap();
        let mut state = state_with_layout(layout);
        state
            .request_command(NativeCommand::DockItem {
                item: FIRST,
                placement: DockPlacement::Center(DockAnchor::Item(target_item)),
            })
            .unwrap();
        commit_queued(&mut state);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                DockspaceActionOutcome::Docked { item: FIRST, .. }
            ))
        ));
        assert!(
            state
                .coordinator
                .session()
                .view()
                .surface(SURFACE)
                .is_some()
        );
    }
    let mut state = state_with_content(DockspaceNode::tabs([FIRST]), true);
    state
        .request_command(NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(ItemId::new(5))),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(_))
    ));
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .is_some()
    );

    let mut state = state_with_content(DockspaceNode::tabs([FIRST]), false);
    for (command, rejection) in [
        (
            NativeCommand::DockItem {
                item: FIRST,
                placement: DockPlacement::Center(DockAnchor::Item(CLOSED)),
            },
            DockspaceActionRejection::AnchorUnavailable {
                anchor: DockAnchor::Item(CLOSED),
            },
        ),
        (
            NativeCommand::FloatItem {
                item: FIRST,
                surface: target_surface,
            },
            DockspaceActionRejection::SurfaceUnavailable {
                surface: target_surface,
            },
        ),
    ] {
        state.request_command(command).unwrap();
        commit_queued(&mut state);
        assert_eq!(
            take(&mut state),
            NativeActionStatus::Dock(DockspaceActionStatus::Rejected(rejection))
        );
    }
    present_geometry(
        &mut state,
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
    );
    state
        .request_command(NativeCommand::FloatItem {
            item: FIRST,
            surface: SURFACE,
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Floated { root: ROOT, .. }
        ))
    ));
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .is_some()
    );
}

#[test]
fn a_queued_item_move_keeps_its_admission_revision() {
    let mut state = state();
    let before = state.coordinator.session().version();
    state
        .request_command(NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(SECOND)),
        })
        .unwrap();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.select_item_current(SECOND).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    let accepted = state.coordinator.session().version();
    assert_ne!(before, accepted);
    commit_queued(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Stale {
            expected: before,
            accepted
        })
    );
    assert!(
        state
            .coordinator
            .session()
            .view()
            .item(SECOND)
            .unwrap()
            .is_selected()
    );
    assert!(state.error().is_none());
}

fn present_geometry(state: &mut NativeRuntimeState<Panes>, bounds: LogicalRect) {
    let binding = state
        .coordinator
        .viewport_binding(egui::ViewportId::ROOT)
        .unwrap();
    let scale = ScaleFactor::new(1.0).unwrap();
    let content = PhysicalRect::new(0.0, 0.0, bounds.width(), bounds.height()).unwrap();
    let facts = NativeWindowFacts::live()
        .with_content_bounds(content)
        .with_outer_bounds(content)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
        .with_close(NativeCloseState::Clear, None);
    state
        .coordinator
        .report_snapshot(
            [(binding, facts)],
            NativeWorkAreaRoster::Unknown,
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::Windows),
        )
        .unwrap();
    let mut initial = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    initial
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = initial.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    let surfaces: Vec<_> = state
        .coordinator
        .session()
        .view()
        .surfaces()
        .map(|surface| surface.id())
        .collect();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.begin_encoding_pass().unwrap();
    for &surface in &surfaces {
        frame
            .frame
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(bounds, LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
                    .unwrap(),
            )
            .unwrap();
        let plan = match frame.frame.surface_work(surface).unwrap() {
            dockspace::runtime::SurfaceWork::Ready(plan) => plan,
            dockspace::runtime::SurfaceWork::Deferred(reason) => {
                panic!("measured surface {surface:?} deferred: {reason:?}")
            }
        };
        let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
        frame.frame.confirm_surface_painted(receipt).unwrap();
    }
    let mut report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    for output in report.take_painted_outputs() {
        state
            .coordinator
            .report_model_presentation_for_test(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    assert!(
        state
            .coordinator
            .session()
            .native_local_coordinates(binding)
            .is_some()
    );
}

#[test]
fn item_float_and_dock_use_the_existing_result_slot_and_preserve_application_data() {
    let mut state = state();
    let bounds = LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
    present_geometry(&mut state, bounds);
    state
        .request_command(NativeCommand::FloatItem {
            item: FIRST,
            surface: SURFACE,
        })
        .unwrap();
    commit_queued(&mut state);
    assert_eq!(
        state
            .request_command(NativeCommand::SelectItem { item: SECOND })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    let NativeActionStatus::Dock(DockspaceActionStatus::Applied(DockspaceActionOutcome::Floated {
        item: FIRST,
        root,
        surface: SURFACE,
        changed: true,
        ..
    })) = take(&mut state)
    else {
        panic!("floating must commit a typed item outcome");
    };
    assert_ne!(root, ROOT);
    assert!(
        state
            .coordinator
            .session()
            .view()
            .item(FIRST)
            .unwrap()
            .contained()
            .is_some()
    );
    assert_eq!(
        state
            .coordinator
            .session()
            .view()
            .item(SECOND)
            .unwrap()
            .root(),
        ROOT
    );

    state
        .request_command(NativeCommand::DockItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(SECOND)),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Docked {
                item: FIRST,
                target_root: ROOT,
                changed: true,
                ..
            }
        ))
    ));
    assert_eq!(
        state
            .coordinator
            .session()
            .view()
            .item(FIRST)
            .unwrap()
            .root(),
        ROOT
    );
    assert_eq!(state.panes.0, "application-owned content");
}

#[test]
fn contained_recovery_preserves_stacking_and_raising_does_not_select_another_tab() {
    let rear = ItemId::new(5);
    let front = ItemId::new(6);
    let rear_id = FloatingPresentationId::new(1);
    let front_id = FloatingPresentationId::new(2);
    let clipped = LogicalRect::new(1400.0, 1000.0, 320.0, 240.0).unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST, SECOND])),
    )
    .with_contained(DockspaceContainedLayout::new(
        rear_id,
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([rear, THIRD])),
        clipped,
    ))
    .with_contained(DockspaceContainedLayout::new(
        front_id,
        DockspaceRootLayout::new(RootId::new(3), DockspaceNode::tabs([front])),
        LogicalRect::new(50.0, 40.0, 320.0, 240.0).unwrap(),
    ))])
    .unwrap();
    let mut state = state_with_layout(layout);
    let bounds = LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap();
    present_geometry(&mut state, bounds);
    assert_eq!(
        state
            .coordinator
            .session()
            .view()
            .contained(rear_id)
            .unwrap()
            .rect(),
        clipped
    );
    state
        .request_command(NativeCommand::BringContainedIntoView { item: THIRD })
        .unwrap();
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::Dock(DockspaceActionStatus::Applied(DockspaceActionOutcome::ContainedBoundsUpdated { floating, changed: true, .. })) if floating == rear_id)
    );
    let view = state.coordinator.session().view();
    let recovered = view.contained(rear_id).unwrap().rect();
    assert!(bounds.contains(recovered.min()));
    assert!(recovered.max().x() <= bounds.max().x() && recovered.max().y() <= bounds.max().y());
    assert_eq!(
        view.surface(SURFACE)
            .unwrap()
            .contained()
            .map(|window| window.id())
            .collect::<Vec<_>>(),
        [rear_id, front_id]
    );
    assert!(view.item(rear).unwrap().is_selected());
    assert!(!view.item(THIRD).unwrap().is_selected());

    present_geometry(&mut state, bounds);
    state
        .request_command(NativeCommand::BringContainedIntoView { item: rear })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::ContainedBoundsUpdated { changed: false, .. }
        ))
    ));
    for changed in [true, false] {
        state
            .request_command(NativeCommand::RaiseContained { item: THIRD })
            .unwrap();
        commit_queued(&mut state);
        assert!(
            matches!(take(&mut state), NativeActionStatus::Dock(DockspaceActionStatus::Applied(DockspaceActionOutcome::ContainedRaised { changed: actual, .. })) if actual == changed)
        );
    }
    let view = state.coordinator.session().view();
    assert_eq!(
        view.surface(SURFACE)
            .unwrap()
            .contained()
            .map(|window| window.id())
            .collect::<Vec<_>>(),
        [front_id, rear_id]
    );
    assert!(view.item(rear).unwrap().is_selected());
    assert_eq!(view.contained(rear_id).unwrap().rect(), recovered);
}

#[test]
fn new_commands_preserve_core_rejections_without_stopping_the_runtime() {
    let mut state = state();
    for (command, reason) in [
        (
            NativeCommand::DockItem {
                item: CLOSED,
                placement: DockPlacement::Center(DockAnchor::Item(FIRST)),
            },
            DockspaceActionRejection::ItemUnavailable { item: CLOSED },
        ),
        (
            NativeCommand::FloatItem {
                item: FIRST,
                surface: SurfaceId::new(99),
            },
            DockspaceActionRejection::SurfaceUnavailable {
                surface: SurfaceId::new(99),
            },
        ),
        (
            NativeCommand::FloatItem {
                item: FIRST,
                surface: SURFACE,
            },
            DockspaceActionRejection::PresentationUnavailable { surface: SURFACE },
        ),
        (
            NativeCommand::RaiseContained { item: FIRST },
            DockspaceActionRejection::ItemNotContained { item: FIRST },
        ),
        (
            NativeCommand::BringContainedIntoView { item: CLOSED },
            DockspaceActionRejection::ItemUnavailable { item: CLOSED },
        ),
        (
            NativeCommand::MoveToNewWindow {
                source: NativeTearOffSource::Item(FIRST),
                placement: NativeWindowPlacement::new(
                    PhysicalRect::new(900.0, 100.0, 480.0, 320.0).unwrap(),
                ),
            },
            DockspaceActionRejection::PolicyDenied,
        ),
        (
            NativeCommand::MoveToNewWindowDefault {
                source: NativeTearOffSource::Item(FIRST),
            },
            DockspaceActionRejection::NativeUnavailable,
        ),
    ] {
        state.request_command(command).unwrap();
        commit_queued(&mut state);
        assert_eq!(
            take(&mut state),
            NativeActionStatus::Dock(DockspaceActionStatus::Rejected(reason))
        );
        assert!(state.error().is_none());
    }
    present_geometry(
        &mut state,
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
    );
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: true })
        .unwrap();
    commit_queued(&mut state);
    take(&mut state);
    present_geometry(
        &mut state,
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
    );
    state
        .request_command(NativeCommand::FloatItem {
            item: FIRST,
            surface: SURFACE,
        })
        .unwrap();
    commit_queued(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
            DockspaceActionRejection::PolicyDenied
        ))
    );
    assert!(state.error().is_none());
}

fn commit_queued(state: &mut NativeRuntimeState<Panes>) {
    commit_queued_report(state);
}

fn commit_queued_report(state: &mut NativeRuntimeState<Panes>) -> HostFrameReport {
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    assert!(!frame.native_input_pending());
    let submission = action.submit(&mut frame.frame, published).unwrap();
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let mut submitted = Some(submitted);
    state
        .publish_application_configuration(&egui::Context::default(), &mut submitted, &report)
        .unwrap();
    state
        .application_actions
        .settle_submission(submitted, &report)
        .unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    report
}

fn take(state: &mut NativeRuntimeState<Panes>) -> NativeActionStatus {
    state.take_action_status().unwrap().unwrap()
}

fn native_command_work_areas() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap(),
        ScaleFactor::new(1.0).unwrap(),
    )])
}

fn native_command_facts(bounds: PhysicalRect) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).unwrap();
    NativeWindowFacts::live()
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn commit_native_command_observations(state: &mut NativeRuntimeState<Panes>) -> HostFrameReport {
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    report
}

fn state_for_native_commands() -> NativeRuntimeState<Panes> {
    let content = DockspaceNode::split(
        DockspaceAxis::Horizontal,
        [
            (DockspaceNode::central_tabs([FIRST]), 1.0),
            (DockspaceNode::tabs([SECOND, THIRD]), 1.0),
        ],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )
    .with_contained(DockspaceContainedLayout::new(
        FloatingPresentationId::new(1),
        DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(5)])),
        LogicalRect::new(600.0, 400.0, 160.0, 120.0).unwrap(),
    ))])
    .unwrap();
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut state = state_with_layout_and_policy(layout, policy);
    present_geometry(
        &mut state,
        LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
    );
    let binding = state
        .coordinator
        .viewport_binding(egui::ViewportId::ROOT)
        .unwrap();
    state
        .coordinator
        .report_snapshot(
            [(
                binding,
                native_command_facts(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap()),
            )],
            native_command_work_areas(),
            crate::capabilities::capabilities_for_backend(eframe::NativeWindowingBackend::Windows),
        )
        .unwrap();
    commit_native_command_observations(&mut state);
    state
}

#[test]
fn native_payload_commands_share_the_result_slot_and_exact_first_live_transfer() {
    let placement =
        NativeWindowPlacement::new(PhysicalRect::new(900.0, 100.0, 480.0, 320.0).unwrap());
    let mut prior_terminals = [None, None];
    for kind in [0, 1, 2, 2, 3, 3, 4] {
        let mut state = state_for_native_commands();
        let before = state.coordinator.session().version();
        let group = state.capture_tabs_group(DockAnchor::Item(SECOND)).unwrap();
        let (source, expected_items) = match kind {
            0 | 1 => (NativeTearOffSource::Item(SECOND), vec![SECOND]),
            2 | 3 => (NativeTearOffSource::Tabs(group), vec![SECOND, THIRD]),
            _ => (NativeTearOffSource::Root(ROOT), vec![FIRST, SECOND, THIRD]),
        };
        let command = match kind {
            0 | 2 => NativeCommand::MoveToNewWindow { source, placement },
            1 | 3 => NativeCommand::MoveToNewWindowDefault { source },
            _ => NativeCommand::MoveRootToNewWindow {
                root: ROOT,
                placement,
            },
        };
        state.request_command(command).unwrap();
        assert_eq!(
            state
                .request_command(NativeCommand::SelectItem { item: FIRST })
                .map_err(|error| error.reason()),
            Err(NativeActionRequestError::Busy)
        );
        let mut report = commit_queued_report(&mut state);
        assert_eq!(state.coordinator.session().version(), before);
        assert_eq!(
            state
                .request_command(NativeCommand::SelectItem { item: FIRST })
                .map_err(|error| error.reason()),
            Err(NativeActionRequestError::Busy)
        );
        let NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::NativeRootTearOffRequested {
                root,
                source_surface: SURFACE,
                target_surface,
                items,
            },
        )) = take(&mut state)
        else {
            panic!("native payload admission must retain its typed request outcome");
        };
        assert_eq!(items, expected_items);
        assert_eq!(root == ROOT, kind == 4);
        assert!(state.coordinator.session().has_pending_native_create());
        assert!(!state.application_actions.is_occupied());
        for item in [FIRST, SECOND, THIRD] {
            assert_eq!(state.view().item(item).unwrap().surface(), SURFACE);
        }
        let [create] = report.take_native_effects().try_into().unwrap();
        let (binding, bounds) = match create.operation() {
            NativeEffectOperation::CreateWindow {
                binding, placement, ..
            } => (*binding, *placement),
            operation => panic!("native command must emit one Create, got {operation:?}"),
        };
        assert_eq!(binding.surface(), target_surface);
        let Some(NativeEffectAcknowledgement::Create(acknowledgement)) = create.accepted() else {
            panic!("the exact Create must supply its acknowledgement");
        };
        let host = state
            .coordinator
            .viewport_binding(egui::ViewportId::ROOT)
            .unwrap();
        state
            .coordinator
            .report_snapshot(
                [
                    (
                        host,
                        native_command_facts(PhysicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap()),
                    ),
                    (
                        binding,
                        native_command_facts(bounds).with_create_readiness(
                            NativeCreateReadiness::Prepared,
                            Some(acknowledgement),
                        ),
                    ),
                ],
                native_command_work_areas(),
                crate::capabilities::capabilities_for_backend(
                    eframe::NativeWindowingBackend::Windows,
                ),
            )
            .unwrap();
        assert!(
            commit_native_command_observations(&mut state)
                .native_admissions()
                .is_empty()
        );
        assert_eq!(state.coordinator.session().version(), before);
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .frame
            .measure_surface(
                binding.surface(),
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, bounds.width(), bounds.height()).unwrap(),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
        let plan = frame.frame.paint_plan(binding.surface()).unwrap().unwrap();
        let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
        frame.frame.confirm_surface_painted(receipt).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        state.coordinator.settle_host_frame_report(&report).unwrap();
        let [output] = report.take_painted_outputs().try_into().unwrap();
        assert!(output.matches_native_binding(binding));
        assert_eq!(state.coordinator.session().version(), before);
        state
            .coordinator
            .report_model_presentation_for_test(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = commit_native_command_observations(&mut state);
        assert_eq!(report.native_admissions(), &[binding]);
        for item in expected_items {
            assert_eq!(
                state.view().item(item).unwrap().surface(),
                binding.surface()
            );
        }
        assert!(!state.coordinator.session().has_pending_native_create());
        assert_eq!(state.panes.0, "application-owned content");
        assert!(state.error().is_none());
        if kind == 2 || kind == 3 {
            let prior = &mut prior_terminals[kind - 2];
            *prior = Some(return_complete_group_through_the_result_slot(
                &mut state,
                binding,
                bounds,
                kind == 3,
                prior.as_ref(),
            ));
        }
    }
}

fn return_complete_group_through_the_result_slot(
    state: &mut NativeRuntimeState<Panes>,
    source_binding: NativeSurfaceBinding,
    source_bounds: PhysicalRect,
    float: bool,
    unrelated: Option<&DockspacePresentationTransition>,
) -> DockspacePresentationTransition {
    present_group_return_endpoints(state, source_binding, source_bounds);
    let source_root = state.view().item(SECOND).unwrap().root();
    let source_surface = state.view().item(SECOND).unwrap().surface();
    assert_eq!(source_surface, source_binding.surface());
    let before = state.coordinator.session().version();
    let group = state.capture_tabs_group(DockAnchor::Item(SECOND)).unwrap();
    assert_eq!(group.expected_version(), before);
    let command = if float {
        NativeCommand::FloatTabs {
            group,
            surface: SURFACE,
        }
    } else {
        NativeCommand::DockTabs {
            group,
            placement: DockPlacement::After(FIRST),
        }
    };
    state.request_command(command).unwrap();
    let staged = commit_queued_report(state);
    assert_eq!(staged.before(), before);
    assert_eq!(staged.after(), before);
    let transition = match staged.inputs() {
        [
            HostInputOutcome::ProductPresentationActionRequested {
                outcome,
                transition,
            },
        ] => {
            let expected = if float {
                DockspaceActionOutcome::RootFloatRequested {
                    root: source_root,
                    source_surface,
                    target_surface: SURFACE,
                    items: vec![SECOND, THIRD],
                }
            } else {
                DockspaceActionOutcome::RootDockRequested {
                    root: source_root,
                    source_surface,
                    target_root: ROOT,
                    items: vec![SECOND, THIRD],
                }
            };
            assert_eq!(outcome, &expected);
            *transition
        }
        inputs => {
            panic!("the complete native group must request an exact return barrier: {inputs:?}")
        }
    };
    assert!(state.coordinator.session().has_pending_presentation());
    assert!(state.take_action_status().is_none());
    assert_eq!(state.view().item(SECOND).unwrap().surface(), source_surface);
    if let Some(unrelated) = unrelated {
        assert_ne!(unrelated.id(), transition);
        assert_eq!(unrelated.root(), source_root);
        assert_eq!(unrelated.source_surface(), source_surface);
        assert_eq!(unrelated.target_surface(), SURFACE);
        assert!(
            !state
                .application_actions
                .settle_presentation_transitions(std::slice::from_ref(unrelated))
                .unwrap(),
            "an identical transfer in another session must not settle this command"
        );
        assert!(state.take_action_status().is_none());
    }
    assert_eq!(
        state
            .request_command(NativeCommand::SelectItem { item: FIRST })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );

    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    let receipt = frame
        .frame
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .execute_surface_program(|_| Ok::<(), ()>(()))
        .unwrap();
    frame.frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut painted = frame.commit().unwrap();
    state
        .coordinator
        .settle_host_frame_report(&painted)
        .unwrap();
    assert!(
        !state
            .application_actions
            .settle_presentation_transitions(painted.presentation_transitions())
            .unwrap()
    );
    let [output] = painted.take_painted_outputs().try_into().unwrap();
    assert_eq!(state.view().item(SECOND).unwrap().surface(), source_surface);
    state
        .coordinator
        .report_model_presentation_for_test(output, SurfacePresentationResult::Presented)
        .unwrap();
    let settled = commit_native_command_observations(state);
    let [terminal] = settled.presentation_transitions() else {
        panic!("the exact group return must produce one transition terminal");
    };
    assert_eq!(terminal.id(), transition);
    assert_eq!(terminal.root(), source_root);
    assert_eq!(terminal.source_surface(), source_surface);
    assert_eq!(terminal.target_surface(), SURFACE);
    assert_eq!(
        terminal.result(),
        DockspacePresentationTransitionResult::Applied
    );
    assert!(
        state
            .application_actions
            .settle_presentation_transitions(settled.presentation_transitions())
            .unwrap()
    );
    let status = take(state);
    let NativeActionStatus::Dock(DockspaceActionStatus::Applied(outcome)) = &status else {
        panic!("the exact group return must settle with its typed outcome: {status:?}");
    };
    assert_eq!(terminal.outcome(), Some(outcome));
    match status {
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::TabsFloated {
                source_root: completed_source,
                root,
                surface: SURFACE,
                items,
                changed: true,
                ..
            },
        )) if float => {
            assert_eq!(completed_source, source_root);
            assert_eq!(root, source_root);
            assert_eq!(items, [SECOND, THIRD]);
            assert!(state.view().item(SECOND).unwrap().contained().is_some());
        }
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::TabsDocked {
                source_root: completed_source,
                target_root: ROOT,
                items,
                changed: true,
            },
        )) if !float => {
            assert_eq!(completed_source, source_root);
            assert_eq!(items, [SECOND, THIRD]);
            assert_eq!(
                state.view().item(SECOND).unwrap().tabs().items(),
                &[FIRST, SECOND, THIRD]
            );
        }
        status => panic!("the slot must settle the exact complete-group terminal: {status:?}"),
    }
    assert_eq!(state.view().item(SECOND).unwrap().surface(), SURFACE);
    assert!(state.view().item(SECOND).unwrap().is_selected());
    assert!(!state.application_actions.is_occupied());
    assert!(state.error().is_none());
    terminal.clone()
}

fn present_group_return_endpoints(
    state: &mut NativeRuntimeState<Panes>,
    source_binding: NativeSurfaceBinding,
    source_bounds: PhysicalRect,
) {
    let before = state.coordinator.session().version();
    let host = state
        .coordinator
        .viewport_binding(egui::ViewportId::ROOT)
        .unwrap();
    let endpoints = [
        (host, LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap()),
        (
            source_binding,
            LogicalRect::new(0.0, 0.0, source_bounds.width(), source_bounds.height()).unwrap(),
        ),
    ];
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.begin_encoding_pass().unwrap();
    for (binding, bounds) in endpoints {
        frame
            .frame
            .measure_surface(
                binding.surface(),
                UniformSurfaceMetrics::new(bounds, LogicalSize::new(64.0, 48.0).unwrap(), 96.0)
                    .unwrap(),
            )
            .unwrap();
        let receipt = frame
            .frame
            .paint_plan(binding.surface())
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        frame.frame.confirm_surface_painted(receipt).unwrap();
    }
    let mut painted = frame.commit().unwrap();
    assert_eq!(painted.before(), before);
    assert_eq!(painted.after(), before);
    state
        .coordinator
        .settle_host_frame_report(&painted)
        .unwrap();
    let outputs = painted.take_painted_outputs();
    assert_eq!(outputs.len(), endpoints.len());
    for (output, (binding, _)) in outputs.into_iter().zip(endpoints) {
        assert!(output.matches_native_binding(binding));
        state
            .coordinator
            .report_model_presentation_for_test(output, SurfacePresentationResult::Presented)
            .unwrap();
    }
    let settled = commit_native_command_observations(state);
    assert_eq!(settled.before(), before);
    assert_eq!(settled.after(), before);
    for (binding, _) in endpoints {
        assert!(
            state
                .coordinator
                .session()
                .is_current_native_binding(binding)
        );
        assert!(
            state
                .coordinator
                .session()
                .presented_surface(binding.surface())
                .is_some()
        );
    }
}

#[test]
fn native_group_commands_preserve_the_remaining_central_area() {
    for float in [false, true] {
        let mut state = state_for_native_commands();
        let group = state.capture_tabs_group(DockAnchor::Item(SECOND)).unwrap();
        let command = if float {
            NativeCommand::FloatTabs {
                group,
                surface: SURFACE,
            }
        } else {
            NativeCommand::DockTabs {
                group,
                placement: DockPlacement::After(FIRST),
            }
        };
        state.request_command(command).unwrap();
        let report = commit_queued_report(&mut state);
        assert_ne!(report.before(), report.after());
        match take(&mut state) {
            NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                DockspaceActionOutcome::TabsFloated {
                    source_root: ROOT,
                    root,
                    surface: SURFACE,
                    items,
                    changed: true,
                    ..
                },
            )) if float => {
                assert_ne!(root, ROOT);
                assert_eq!(items, [SECOND, THIRD]);
                assert_eq!(
                    state.view().item(SECOND).unwrap().tabs().items(),
                    &[SECOND, THIRD]
                );
                assert_eq!(state.view().item(FIRST).unwrap().tabs().items(), &[FIRST]);
            }
            NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                DockspaceActionOutcome::TabsDocked {
                    source_root: ROOT,
                    target_root: ROOT,
                    items,
                    changed: true,
                },
            )) if !float => {
                assert_eq!(items, [SECOND, THIRD]);
                assert_eq!(
                    state.view().item(SECOND).unwrap().tabs().items(),
                    &[FIRST, SECOND, THIRD]
                );
            }
            status => panic!("one captured group must produce one terminal result: {status:?}"),
        }
        assert_eq!(state.view().item(FIRST).unwrap().root(), ROOT);
        assert!(state.view().root(ROOT).unwrap().central().is_some());
        assert!(state.view().item(SECOND).unwrap().is_selected());
        assert!(!state.coordinator.session().has_pending_presentation());
        assert!(state.error().is_none());
    }
}

#[test]
fn native_group_dock_and_float_commands_keep_stale_and_foreign_authority() {
    for float in [false, true] {
        for foreign in [false, true] {
            let mut source = state();
            let group = source.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
            let captured = group.expected_version();
            if !foreign {
                source
                    .request_command(NativeCommand::SelectItem { item: SECOND })
                    .unwrap();
                commit_queued(&mut source);
                take(&mut source);
            }
            let mut target = if foreign { state() } else { source };
            let accepted = target.coordinator.session().version();
            let command = if float {
                NativeCommand::FloatTabs {
                    group,
                    surface: SURFACE,
                }
            } else {
                NativeCommand::DockTabs {
                    group,
                    placement: DockPlacement::After(FIRST),
                }
            };
            target.request_command(command).unwrap();
            let report = commit_queued_report(&mut target);
            assert_eq!(report.before(), report.after());
            assert_eq!(
                take(&mut target),
                NativeActionStatus::Dock(if foreign {
                    DockspaceActionStatus::Rejected(DockspaceActionRejection::Conflict)
                } else {
                    DockspaceActionStatus::Stale {
                        expected: captured,
                        accepted,
                    }
                })
            );
            assert_eq!(
                target.view().item(FIRST).unwrap().tabs().items(),
                &[FIRST, SECOND, THIRD]
            );
            target
                .request_command(NativeCommand::SelectItem { item: THIRD })
                .unwrap();
            commit_queued(&mut target);
            assert!(matches!(
                take(&mut target),
                NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::Selected { item: THIRD, .. }
                ))
            ));
            assert!(target.error().is_none());
        }
    }
}

#[test]
fn stored_native_group_command_keeps_its_capture_revision_after_admission() {
    let mut state = state();
    let group = state.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
    let captured = group.expected_version();
    let command = NativeCommand::MoveToNewWindowDefault {
        source: NativeTearOffSource::Tabs(group),
    };
    state
        .request_command(NativeCommand::SelectItem { item: SECOND })
        .unwrap();
    commit_queued(&mut state);
    take(&mut state);
    let accepted = state.coordinator.session().version();
    assert_ne!(captured, accepted);
    state.request_command(command).unwrap();
    let mut report = commit_queued_report(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Stale {
            expected: captured,
            accepted,
        })
    );
    assert!(report.take_native_effects().is_empty());
    assert_eq!(
        state.view().item(SECOND).unwrap().tabs().items(),
        &[FIRST, SECOND, THIRD]
    );
    assert!(state.view().item(SECOND).unwrap().is_selected());
    assert!(!state.coordinator.session().has_pending_native_create());
    assert!(state.error().is_none());
}

#[test]
fn foreign_native_groups_reject_without_staling_or_stopping_the_command_pipeline() {
    let placement =
        NativeWindowPlacement::new(PhysicalRect::new(900.0, 100.0, 480.0, 320.0).unwrap());
    for different_revision in [false, true] {
        for default_placement in [false, true] {
            let mut source = state();
            if different_revision {
                source
                    .request_command(NativeCommand::SelectItem { item: SECOND })
                    .unwrap();
                commit_queued(&mut source);
                take(&mut source);
            }
            let group = source.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap();
            let original = group.clone();
            let source_version = source.coordinator.session().version();
            let mut destination = state();
            let before = destination.coordinator.session().version();
            assert_eq!(group.expected_version() != before, different_revision);
            let command = if default_placement {
                NativeCommand::MoveToNewWindowDefault {
                    source: NativeTearOffSource::Tabs(group),
                }
            } else {
                NativeCommand::MoveToNewWindow {
                    source: NativeTearOffSource::Tabs(group),
                    placement,
                }
            };
            destination.request_command(command).unwrap();
            let mut report = commit_queued_report(&mut destination);
            assert_eq!(report.before(), before);
            assert_eq!(report.after(), before);
            assert!(report.inputs().is_empty());
            assert!(report.take_native_effects().is_empty());
            assert_eq!(
                destination
                    .request_command(NativeCommand::SelectItem { item: SECOND })
                    .map_err(|error| error.reason()),
                Err(NativeActionRequestError::Busy)
            );
            assert_eq!(
                take(&mut destination),
                NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
                    DockspaceActionRejection::Conflict,
                ))
            );
            assert_eq!(destination.coordinator.session().version(), before);
            assert!(destination.view().item(FIRST).unwrap().is_selected());
            assert!(
                !destination
                    .coordinator
                    .session()
                    .has_pending_native_create()
            );
            assert_eq!(source.coordinator.session().version(), source_version);
            assert_eq!(
                source.capture_tabs_group(DockAnchor::Item(FIRST)).unwrap(),
                original
            );
            destination
                .request_command(NativeCommand::SelectItem { item: SECOND })
                .unwrap();
            commit_queued(&mut destination);
            assert!(matches!(
                take(&mut destination),
                NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                    DockspaceActionOutcome::Selected { item: SECOND, .. }
                ))
            ));
            assert!(destination.view().item(SECOND).unwrap().is_selected());
            assert!(destination.error().is_none());
        }
    }
}

#[test]
fn dock_submission_preserves_phase_errors_outside_foreign_action_authority() {
    for replacement_submitted in [false, true] {
        let mut state = state();
        let before = state.coordinator.session().version();
        let reset = state
            .coordinator
            .session()
            .prepare_layout_reset(
                DockspaceLayout::new([DockspaceSurfaceLayout::new(
                    SURFACE,
                    DockspaceRootLayout::new(
                        ROOT,
                        DockspaceNode::central_tabs([FIRST, SECOND, THIRD]),
                    ),
                )])
                .unwrap(),
            )
            .unwrap();
        state
            .request_command(NativeCommand::SelectItem { item: SECOND })
            .unwrap();
        let action = state.application_actions.take().unwrap();
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        if replacement_submitted {
            frame.frame.submit_layout_reset(&reset).unwrap();
        } else {
            frame.frame.begin_encoding_pass().unwrap();
        }
        let Err(error) = action.submit(&mut frame.frame, before) else {
            panic!("phase errors must not become an ordinary Dock rejection");
        };
        assert!(!error.is_foreign_prepared_action());
        assert_eq!(
            error.kind(),
            if replacement_submitted {
                dockspace::runtime::DockspaceRuntimeErrorKind::OperationConflict
            } else {
                dockspace::runtime::DockspaceRuntimeErrorKind::HostProtocol
            }
        );
        drop(frame);
        assert_eq!(state.coordinator.session().version(), before);
        assert!(state.view().item(FIRST).unwrap().is_selected());
    }
}

#[test]
fn input_requests_share_the_command_slot_and_publish_only_after_commit() {
    let mut state = state();
    let original = state.input();
    let configured = egui_dockspace::DockInputConfig {
        docking: egui_dockspace::DockingActivation::HoldToEnable(egui::Modifiers::ALT),
        cancel_drag: None,
    };
    state.request_input(configured).unwrap();
    assert_eq!(state.input(), original);
    assert_eq!(
        state.request_input(original),
        Err(NativeActionRequestError::Busy)
    );
    assert_eq!(
        state
            .request_command(NativeCommand::SelectItem { item: FIRST })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    commit_queued(&mut state);
    assert_eq!(state.input(), configured);
    assert_eq!(
        state.request_input(original),
        Err(NativeActionRequestError::Busy)
    );
    assert_eq!(
        take(&mut state),
        NativeActionStatus::InputChanged { changed: true }
    );
    state.request_input(configured).unwrap();
    commit_queued(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::InputChanged { changed: false }
    );
}

#[test]
fn initial_input_configuration_does_not_consume_the_application_slot_and_stops_at_first_run() {
    let mut state = state();
    let configured = egui_dockspace::DockInputConfig {
        docking: egui_dockspace::DockingActivation::Always,
        cancel_drag: None,
    };
    state.set_initial_input(configured).unwrap();
    assert_eq!(state.input(), configured);
    assert!(!state.application_actions.is_occupied());
    state.root_context = Some(egui::Context::default());
    assert_eq!(
        state.set_initial_input(Default::default()),
        Err(NativeActionRequestError::Busy)
    );
    assert_eq!(state.input(), configured);
    state.request_input(Default::default()).unwrap();
    assert_eq!(state.input(), configured);
    commit_queued(&mut state);
    assert_eq!(state.input(), egui_dockspace::DockInputConfig::default());
    assert_eq!(
        take(&mut state),
        NativeActionStatus::InputChanged { changed: true }
    );
}

#[test]
fn style_requests_share_the_command_slot_and_publish_only_after_commit() {
    let mut state = state();
    let original = state.style.clone();
    let updated = DockStyle {
        tab_bar_height: 42.0,
        ..original.clone()
    };
    state.request_style(updated.clone()).unwrap();
    assert_eq!(state.style, original);
    assert_eq!(
        state
            .coordinator
            .session()
            .presentation_config()
            .tab_bar_height(),
        28.0
    );
    assert!(matches!(
        state.request_style(original.clone()),
        Err(crate::NativeStyleRequestError::Queue(
            NativeActionRequestError::Busy
        ))
    ));
    commit_queued(&mut state);
    assert_eq!(state.style, updated);
    assert_eq!(
        state
            .coordinator
            .session()
            .presentation_config()
            .tab_bar_height(),
        42.0
    );
    assert_eq!(
        state
            .request_command(NativeCommand::SelectItem { item: FIRST })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    assert_eq!(
        take(&mut state),
        NativeActionStatus::StyleChanged {
            changed: true,
            geometry_changed: true
        }
    );
    state.request_style(original).unwrap();
    commit_queued(&mut state);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::StyleChanged {
            changed: true,
            geometry_changed: true
        }
    );
}

#[test]
fn candidate_style_and_core_geometry_encode_together_across_repeat_passes() {
    use dockspace::runtime::{
        SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    };

    let mut state = state();
    let bounds = LogicalRect::new(0.0, 0.0, 180.0, 480.0).unwrap();
    present_geometry(&mut state, bounds);
    let original = state.style.clone();
    let updated = DockStyle {
        tab_bar_height: 44.0,
        ..original.clone()
    };
    state.request_style(updated.clone()).unwrap();
    let action = state.application_actions.take().unwrap();
    let version = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let submitted = action
        .submit(&mut frame.frame, version)
        .unwrap()
        .finish(&mut frame.frame)
        .unwrap();
    assert_eq!(state.style, original);
    let style = submitted.encoding_style().unwrap();
    assert_eq!(style, &updated);
    let context = egui::Context::default();
    frame.frame.begin_encoding_pass().unwrap();
    for repeat in [false, true] {
        if repeat {
            frame.frame.restart_encoding_pass().unwrap();
        }
        context
            .run_ui(egui::RawInput::default(), |ui| {
                let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(180.0, 480.0));
                frame
                    .measure_surface(SURFACE, ui, rect, rect, &state.panes, style)
                    .unwrap();
            })
            .drop_without_applying_deltas();
        let plan = frame.frame.paint_plan(SURFACE).unwrap().unwrap();
        let mut bars = 0;
        let mut controls = 0;
        plan.visit_surface_program(|step| {
            if let Step::Operation { operation, .. } = step {
                match operation {
                    Operation::TabBar(bar) => {
                        assert_eq!(bar.bounds().height(), 44.0);
                        bars += 1;
                    }
                    Operation::TabStripControl(control) => {
                        assert_eq!(control.bounds().width(), 44.0);
                        controls += 1;
                    }
                    _ => {}
                }
            }
            Ok::<(), ()>(())
        })
        .unwrap();
        assert!(bars > 0 && controls > 0);
    }
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    let mut submitted = Some(submitted);
    state
        .publish_application_configuration(&context, &mut submitted, &report)
        .unwrap();
    state
        .application_actions
        .settle_submission(submitted, &report)
        .unwrap();
    assert_eq!(state.style, updated);
}

#[test]
fn visual_and_unchanged_styles_do_not_submit_geometry_replacements() {
    let mut state = state();
    let mut style = state.style.clone();
    style.visuals.tab_active_fill = Some(egui::Color32::RED);
    for changed in [true, false] {
        let before = state.coordinator.session().presentation_config().clone();
        state.request_style(style.clone()).unwrap();
        let action = state.application_actions.take().unwrap();
        let published = state.coordinator.session().version();
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let mut submitted = Some(
            action
                .submit(&mut frame.frame, published)
                .unwrap()
                .finish(&mut frame.frame)
                .unwrap(),
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let report = frame.commit().unwrap();
        assert!(
            !report
                .inputs()
                .iter()
                .any(|input| matches!(input, HostInputOutcome::PresentationConfigReplaced { .. }))
        );
        state
            .publish_application_configuration(&egui::Context::default(), &mut submitted, &report)
            .unwrap();
        state
            .application_actions
            .settle_submission(submitted, &report)
            .unwrap();
        state.coordinator.settle_host_frame_report(&report).unwrap();
        assert_eq!(state.style, style);
        assert_eq!(state.coordinator.session().presentation_config(), &before);
        assert_eq!(
            take(&mut state),
            NativeActionStatus::StyleChanged {
                changed,
                geometry_changed: false
            }
        );
    }
}

#[test]
fn rejected_and_rolled_back_styles_leave_both_authorities_unchanged() {
    let mut state = state();
    let original = state.style.clone();
    let original_config = state.coordinator.session().presentation_config().clone();
    let invalid = DockStyle {
        tab_bar_height: f32::NAN,
        ..original.clone()
    };
    assert!(matches!(
        state.request_style(invalid),
        Err(crate::NativeStyleRequestError::Style(_))
    ));
    assert!(!state.application_actions.is_occupied());
    state
        .request_style(DockStyle {
            tab_bar_height: 42.0,
            ..original.clone()
        })
        .unwrap();
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    {
        let mut frame = state
            .coordinator
            .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let submitted = action
            .submit(&mut frame.frame, published)
            .unwrap()
            .finish(&mut frame.frame)
            .unwrap();
        drop(submitted);
    }
    assert_eq!(state.style, original);
    assert_eq!(
        state.coordinator.session().presentation_config(),
        &original_config
    );
    state.stop_current_output(
        crate::error::NativeHostProtocolError::OutputRouteAttachmentFailed.into(),
    );
    assert!(matches!(
        state.request_style(original),
        Err(crate::NativeStyleRequestError::Queue(
            NativeActionRequestError::Stopped
        ))
    ));
}

#[test]
fn style_updates_preserve_custom_core_only_settings() {
    let mut state = state();
    let config = state
        .coordinator
        .session()
        .presentation_config()
        .to_builder()
        .pointer_drag_start_distance(12.0)
        .tab_strip_scroll_line_extent(30.0)
        .magnetic_center_fraction(0.6)
        .target_release_hysteresis(11.0)
        .build()
        .unwrap();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .frame
        .replace_presentation_config(config.clone())
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state.coordinator.settle_host_frame_report(&report).unwrap();
    state
        .request_style(DockStyle {
            tab_bar_height: 42.0,
            ..state.style.clone()
        })
        .unwrap();
    commit_queued(&mut state);
    let expected = config.to_builder().tab_bar_height(42.0).build().unwrap();
    assert_eq!(state.coordinator.session().presentation_config(), &expected);
    assert_eq!(
        take(&mut state),
        NativeActionStatus::StyleChanged {
            changed: true,
            geometry_changed: true
        }
    );
}

fn request_plan(
    state: &mut NativeRuntimeState<Panes>,
    item: ItemId,
) -> dockspace::runtime::DockspaceClosePlan {
    state
        .request_command(NativeCommand::CloseItem { item })
        .unwrap();
    commit_queued(state);
    let NativeActionStatus::CloseRequested {
        plan,
        reused: false,
    } = take(state)
    else {
        panic!("the exact close request must expose its decision plan");
    };
    plan
}

#[test]
fn deferred_veto_and_close_reopen_keep_pane_data_and_the_single_result_slot() {
    let mut state = state();
    let plan = request_plan(&mut state, FIRST);
    assert_eq!(state.pending_content_close_plans(), [plan.clone()]);
    state
        .request_command(NativeCommand::DecideClose {
            request: plan.request(),
            token: plan.items()[0].token(),
            decision: CloseDecision::Deferred,
        })
        .unwrap();
    assert_eq!(
        state
            .request_command(NativeCommand::OpenItem {
                item: CLOSED,
                placement: DockPlacement::Center(DockAnchor::Item(SECOND))
            })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    commit_queued(&mut state);
    assert_eq!(
        state
            .request_command(NativeCommand::SetArrangementLocked { locked: true })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    let NativeActionStatus::CloseDecisionProcessed {
        resolution: DockspaceCloseResolution::Deferred { continuation, .. },
        changed: false,
        ..
    } = take(&mut state)
    else {
        panic!("deferral must retain an exact continuation without closing content");
    };
    assert_eq!(state.pending_content_close_plans().len(), 1);
    state
        .request_command(NativeCommand::ContinueClose {
            request: plan.request(),
            token: continuation,
            decision: DeferredCloseDecision::Veto,
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Vetoed { .. },
            changed: false,
            ..
        }
    ));
    assert!(state.coordinator.session().view().item(FIRST).is_some());
    assert!(state.pending_content_close_plans().is_empty());

    let plan = request_plan(&mut state, FIRST);
    state
        .request_command(NativeCommand::DecideClose {
            request: plan.request(),
            token: plan.items()[0].token(),
            decision: CloseDecision::Allow,
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            application: Some(Ok(dockspace::runtime::DockspaceCloseOutcome::ItemClosed {
                item: FIRST,
                ..
            })),
            changed: true,
            ..
        }
    ));
    assert!(state.coordinator.session().view().item(FIRST).is_none());
    assert!(state.pending_content_close_plans().is_empty());
    assert_eq!(state.panes.0, "application-owned content");

    state
        .request_command(NativeCommand::OpenItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(SECOND)),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Opened { item: FIRST, .. }
        ))
    ));
    let location = state.coordinator.session().view().item(FIRST).unwrap();
    assert_eq!(location.root(), ROOT);
    assert_eq!(location.tabs().selected(), Some(FIRST));
    assert_eq!(state.panes.0, "application-owned content");
    state
        .request_command(NativeCommand::OpenItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Item(THIRD)),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Existing { item: FIRST }
        ))
    ));
}

#[test]
fn root_close_requires_all_votes_before_one_atomic_removal() {
    let mut state = state_with_content(DockspaceNode::central_tabs([FIRST, SECOND, THIRD]), true);
    state
        .request_command(NativeCommand::CloseRoot { root: ROOT })
        .unwrap();
    commit_queued(&mut state);
    let NativeActionStatus::CloseRequested { plan, .. } = take(&mut state) else {
        panic!("root close must return a plan")
    };
    assert_eq!(plan.items().len(), 3);
    let before = state.coordinator.session().version();
    for (index, item) in plan.items().iter().enumerate() {
        state
            .request_command(NativeCommand::DecideClose {
                request: plan.request(),
                token: item.token(),
                decision: CloseDecision::Allow,
            })
            .unwrap();
        commit_queued(&mut state);
        let status = take(&mut state);
        if index < 2 {
            assert!(matches!(
                status,
                NativeActionStatus::CloseDecisionProcessed {
                    changed: false,
                    application: None,
                    ..
                }
            ));
            assert_eq!(state.coordinator.session().version(), before);
            assert!(
                [FIRST, SECOND, THIRD].into_iter().all(|item| state
                    .coordinator
                    .session()
                    .view()
                    .item(item)
                    .is_some())
            );
        } else {
            assert!(matches!(
                status,
                NativeActionStatus::CloseDecisionProcessed {
                    changed: true,
                    application: Some(Ok(dockspace::runtime::DockspaceCloseOutcome::RootClosed {
                        root: ROOT,
                        ..
                    })),
                    ..
                }
            ));
            assert_ne!(state.coordinator.session().version(), before);
        }
    }
    assert!(
        [FIRST, SECOND, THIRD].into_iter().all(|item| state
            .coordinator
            .session()
            .view()
            .item(item)
            .is_none())
    );
    assert!(state.pending_content_close_plans().is_empty());
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .unwrap()
            .is_rootless()
    );
    state
        .request_command(NativeCommand::OpenItem {
            item: FIRST,
            placement: DockPlacement::Main(SURFACE),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::Dock(DockspaceActionStatus::Applied(DockspaceActionOutcome::Opened { item: FIRST, root })) if root != ROOT)
    );
    assert!(state.coordinator.session().view().item(FIRST).is_some());
}

#[test]
fn the_fixed_host_rejects_its_last_group_and_reopens_after_all_central_items_close() {
    let mut state = state();
    let before = state.coordinator.session().version();
    assert_eq!(
        state
            .request_command(NativeCommand::CloseRoot { root: ROOT })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    assert!(!state.application_actions.is_occupied());
    assert!(state.pending_content_close_plans().is_empty());
    assert_eq!(state.coordinator.session().version(), before);
    for item in [FIRST, SECOND, THIRD] {
        let plan = request_plan(&mut state, item);
        state
            .request_command(NativeCommand::DecideClose {
                request: plan.request(),
                token: plan.items()[0].token(),
                decision: CloseDecision::Allow,
            })
            .unwrap();
        commit_queued(&mut state);
        assert!(matches!(
            take(&mut state),
            NativeActionStatus::CloseDecisionProcessed {
                changed: true,
                application: Some(Ok(_)),
                ..
            }
        ));
    }
    let surface = state.coordinator.session().view().surface(SURFACE).unwrap();
    assert_eq!(surface.main_root().unwrap().id(), ROOT);
    assert!(
        surface
            .main_root()
            .unwrap()
            .central()
            .unwrap()
            .tabs()
            .unwrap()
            .items()
            .is_empty()
    );
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: true })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged { locked: true, .. }
    ));
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: false })
        .unwrap();
    commit_queued(&mut state);
    take(&mut state);
    state
        .request_command(NativeCommand::OpenItem {
            item: FIRST,
            placement: DockPlacement::Center(DockAnchor::Central(ROOT)),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Opened {
                item: FIRST,
                root: ROOT
            }
        ))
    ));
    assert!(state.coordinator.session().view().item(FIRST).is_some());
    assert_eq!(state.panes.0, "application-owned content");
}

#[test]
fn the_last_noncentral_item_cannot_remove_the_fixed_host_surface() {
    let mut state = state_with_content(DockspaceNode::tabs([FIRST]), false);
    assert_eq!(
        state
            .request_command(NativeCommand::CloseItem { item: FIRST })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    assert!(!state.application_actions.is_occupied());
    state
        .request_command(NativeCommand::SelectItem { item: FIRST })
        .unwrap();
    assert_eq!(
        state
            .request_command(NativeCommand::CloseItem { item: FIRST })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::Busy)
    );
    commit_queued(&mut state);
    take(&mut state);
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .is_some()
    );
    assert!(state.coordinator.session().view().item(FIRST).is_some());
}

#[test]
fn chrome_group_decisions_keep_the_host_and_forward_inert_tokens_to_the_core() {
    let mut state = state();
    // Chrome requests reach this same core owner without using the application slot.
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.request_close_root_current(ROOT).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let plan = state.pending_content_close_plans().pop().unwrap();
    assert_eq!(
        state
            .request_command(NativeCommand::DecideClose {
                request: plan.request(),
                token: plan.items()[0].token(),
                decision: CloseDecision::Allow
            })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    state
        .request_command(NativeCommand::DecideClose {
            request: plan.request(),
            token: plan.items()[0].token(),
            decision: CloseDecision::Deferred,
        })
        .unwrap();
    commit_queued(&mut state);
    let NativeActionStatus::CloseDecisionProcessed {
        resolution: DockspaceCloseResolution::Deferred { continuation, .. },
        ..
    } = take(&mut state)
    else {
        panic!("the core must issue the exact deferred continuation")
    };
    assert_eq!(
        state
            .request_command(NativeCommand::ContinueClose {
                request: plan.request(),
                token: continuation,
                decision: DeferredCloseDecision::Allow
            })
            .map_err(|error| error.reason()),
        Err(NativeActionRequestError::HostSurfaceRequired { surface: SURFACE })
    );
    state
        .request_command(NativeCommand::ContinueClose {
            request: plan.request(),
            token: continuation,
            decision: DeferredCloseDecision::Veto,
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Vetoed { .. },
            changed: false,
            ..
        }
    ));
    state
        .request_command(NativeCommand::ContinueClose {
            request: plan.request(),
            token: continuation,
            decision: DeferredCloseDecision::Allow,
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(_),
            changed: false,
            ..
        }
    ));
    assert!(
        state
            .coordinator
            .session()
            .view()
            .surface(SURFACE)
            .is_some()
    );
    assert!(
        [FIRST, SECOND, THIRD].into_iter().all(|item| state
            .coordinator
            .session()
            .view()
            .item(item)
            .is_some())
    );
}

#[test]
fn configuration_follows_local_selection_and_lock_keeps_selection_available() {
    let mut state = state();
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: true })
        .unwrap();
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    let submission = action.submit(&mut frame.frame, published).unwrap();
    frame.frame.select_item_current(SECOND).unwrap();
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    state
        .application_actions
        .settle_submission(Some(submitted), &report)
        .unwrap();
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged {
            locked: true,
            changed: true,
            ..
        }
    ));
    assert!(state.arrangement_locked());
    assert_eq!(
        state
            .coordinator
            .session()
            .view()
            .item(SECOND)
            .unwrap()
            .tabs()
            .selected(),
        Some(SECOND)
    );

    state
        .request_command(NativeCommand::SelectItem { item: FIRST })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Applied(
            DockspaceActionOutcome::Selected { item: FIRST, .. }
        ))
    ));
    state
        .request_command(NativeCommand::OpenItem {
            item: CLOSED,
            placement: DockPlacement::Center(DockAnchor::Item(FIRST)),
        })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::Dock(DockspaceActionStatus::Rejected(
            DockspaceActionRejection::PolicyDenied
        ))
    ));
    assert!(state.coordinator.session().view().item(CLOSED).is_none());

    let before = state.coordinator.session().version();
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: true })
        .unwrap();
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::ArrangementLockChanged { locked: true, changed: false, version } if version == before)
    );
    state
        .request_command(NativeCommand::SetArrangementLocked { locked: false })
        .unwrap();
    commit_queued(&mut state);
    assert!(matches!(
        take(&mut state),
        NativeActionStatus::ArrangementLockChanged {
            locked: false,
            changed: true,
            ..
        }
    ));
    assert!(!state.arrangement_locked());
}

#[test]
fn exact_close_submission_ignores_other_close_requests_in_the_same_frame() {
    let mut state = state();
    state
        .request_command(NativeCommand::CloseItem { item: FIRST })
        .unwrap();
    let action = state.application_actions.take().unwrap();
    let published = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.request_close_item_current(SECOND).unwrap();
    let submission = action.submit(&mut frame.frame, published).unwrap();
    frame.frame.request_close_item_current(THIRD).unwrap();
    let submitted = submission.finish(&mut frame.frame).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let report = frame.commit().unwrap();
    assert_eq!(
        report
            .inputs()
            .iter()
            .filter(|input| matches!(input, HostInputOutcome::CloseRequested { .. }))
            .count(),
        3
    );
    state
        .application_actions
        .settle_submission(Some(submitted), &report)
        .unwrap();
    let NativeActionStatus::CloseRequested { plan, .. } = take(&mut state) else {
        panic!("the submitted close result must be correlated")
    };
    assert_eq!(plan.items()[0].item(), FIRST);
    assert_eq!(state.pending_content_close_plans().len(), 3);
}

#[test]
fn a_queued_close_keeps_its_revision_after_an_earlier_selection_commits() {
    let mut state = state();
    state
        .request_command(NativeCommand::CloseItem { item: FIRST })
        .unwrap();
    let before = state.coordinator.session().version();
    let mut frame = state
        .coordinator
        .begin_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame.frame.select_item_current(SECOND).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
    let accepted = state.coordinator.session().version();
    assert_ne!(before, accepted);
    commit_queued(&mut state);
    assert!(
        matches!(take(&mut state), NativeActionStatus::Stale { expected, accepted: current } if expected == before && current == accepted)
    );
    assert!(state.coordinator.session().view().item(FIRST).is_some());
    assert!(state.pending_content_close_plans().is_empty());
}

#[test]
fn a_foreign_close_token_cannot_be_approved_through_the_native_queue() {
    let mut source = state();
    let plan = request_plan(&mut source, FIRST);
    let mut other = state();
    other
        .request_command(NativeCommand::DecideClose {
            request: plan.request(),
            token: plan.items()[0].token(),
            decision: CloseDecision::Allow,
        })
        .unwrap();
    commit_queued(&mut other);
    assert!(matches!(
        take(&mut other),
        NativeActionStatus::CloseDecisionProcessed {
            resolution: DockspaceCloseResolution::Inert(
                DockspaceCloseInertReason::AuthorityMismatch
            ),
            changed: false,
            ..
        }
    ));
    assert!(other.coordinator.session().view().item(FIRST).is_some());
    assert!(other.pending_content_close_plans().is_empty());
    assert_eq!(source.pending_content_close_plans(), [plan]);
}
