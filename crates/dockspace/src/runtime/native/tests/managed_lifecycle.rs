use super::*;

use crate::geometry::PhysicalPoint;
use crate::runtime::{
    DockspaceReceiverDescriptor, HostFrameReport, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeStagingPresentationPhase,
};

const SECOND_ITEM: ItemId = ItemId::new(2);
const ROOT_ORIGIN_X: f64 = 100.0;
const ROOT_ORIGIN_Y: f64 = 80.0;

#[test]
fn managed_native_tear_off_reaches_first_live_through_the_public_runtime() {
    let (mut session, root_binding) = managed_tear_off_session();
    let work_area_binding = session
        .native_work_area(HostWorkAreaToken::new(1))
        .expect("the committed work-area roster mints route authority");
    let source_receiver = presented_tab_receiver(&mut session, ITEM);
    let source_point = source_receiver.center();
    let source_desktop = PhysicalPoint::new(
        ROOT_ORIGIN_X + source_point.x(),
        ROOT_ORIGIN_Y + source_point.y(),
    )
    .expect("the source desktop point is finite");
    let outside = PhysicalPoint::new(1_000.0, 700.0).expect("the outside point is finite");
    let pointer = NativePointerId::new(1);

    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(source_desktop),
                NativePointerHover::Dock(root_binding),
                None,
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::Native(root_binding),
        ))
        .expect("the exact source press records");
    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::Moved,
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(outside),
                NativePointerHover::OutsideAll,
                Some(work_area_binding),
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::Native(root_binding),
        ))
        .expect("the exact outside-all move records");

    let mut preview = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the native drag frame begins");
    assert!(
        preview
            .paint_plan(SURFACE)
            .expect("the source plan resolves")
            .expect("the source remains paintable")
            .drag_preview()
            .is_some(),
        "the outside-all move must expose a core-owned native preview"
    );
    preview
        .confirm_surface_painted(SURFACE)
        .expect("the exact preview is painted");
    let mut preview_report = preview.commit().expect("the preview frame commits");
    assert!(preview_report.take_native_effects().is_empty());
    present_surface_outputs(&mut session, &mut preview_report);
    commit_managed_frame(&mut session);

    session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(outside),
                NativePointerHover::OutsideAll,
                Some(work_area_binding),
            ),
            NativePointerOwner::Native(root_binding),
            NativePointerOwner::None,
        ))
        .expect("the exact outside-all release records");
    let mut release = session
        .begin_native_host_frame(|query| receiver_answer(query, source_receiver))
        .expect("the native release frame begins");
    release
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the release frame retains current surface authority");
    let mut release_report = release.commit().expect("the native release commits");
    let create = take_only_native_effect(&mut release_report);
    let (child_binding, placement) = match create.operation() {
        NativeEffectOperation::CreateWindow {
            binding, placement, ..
        } => (*binding, *placement),
        operation => panic!("outside-all release emitted {operation:?} instead of CreateWindow"),
    };
    let create_ack = match create.accepted() {
        Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("create returned the wrong acknowledgement: {acknowledgement:?}"),
    };
    assert_ne!(child_binding.surface(), SURFACE);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Hidden, Some(create_ack))
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact hidden child roster records");
    commit_managed_frame(&mut session);

    let pre_show = paint_staging(
        &mut session,
        child_binding,
        NativeStagingPresentationPhase::PreShow,
    );
    session
        .report_native_staging_presentation(pre_show, SurfacePresentationResult::Presented)
        .expect("the pre-show staging result records");
    let mut pre_show_report = commit_managed_frame_report(&mut session);
    let show = take_only_native_effect(&mut pre_show_report);
    assert!(matches!(
        show.operation(),
        NativeEffectOperation::ShowWindow { binding } if *binding == child_binding
    ));
    let show_ack = match show.accepted() {
        Some(NativeEffectAcknowledgement::Presentation(acknowledgement)) => acknowledgement,
        acknowledgement => panic!("show returned the wrong acknowledgement: {acknowledgement:?}"),
    };

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Hidden, Some(show_ack))
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact show acknowledgement records while the child remains hidden");
    commit_managed_frame(&mut session);

    session
        .report_managed_native_snapshot(
            [
                (root_binding, root_window_facts()),
                (
                    child_binding,
                    NativeWindowFacts::live()
                        .with_content_bounds(placement)
                        .with_outer_bounds(placement)
                        .with_native_scale_factor(native_scale())
                        .with_presentation_scale_factor(native_scale())
                        .with_input(NativeWindowInputState::ReceivesInput, None)
                        .with_presentation(NativeWindowPresentationState::Visible, None)
                        .with_close(NativeCloseState::Clear, None),
                ),
            ],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the later exact visible child roster records");
    commit_managed_frame(&mut session);

    let post_show = paint_staging(
        &mut session,
        child_binding,
        NativeStagingPresentationPhase::PostShow,
    );
    session
        .report_native_staging_presentation(post_show, SurfacePresentationResult::Presented)
        .expect("the post-show staging result records");
    commit_managed_frame(&mut session);

    measure_all_surfaces(&mut session);
    let mut first_live = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the first-live paint frame begins");
    assert!(
        first_live
            .paint_plan(child_binding.surface())
            .expect("the child plan resolves")
            .is_some(),
        "ownership transfer must publish a semantic child plan"
    );
    first_live
        .confirm_surface_painted(child_binding.surface())
        .expect("the first-live child output is painted");
    first_live
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the root surface retains its exact plan");
    let mut first_live_report = first_live.commit().expect("the first-live paint commits");
    assert!(first_live_report.native_admissions().is_empty());
    present_surface_outputs(&mut session, &mut first_live_report);
    let admitted = commit_managed_frame_report(&mut session);

    assert_eq!(admitted.native_admissions(), &[child_binding]);
    assert_eq!(
        session.view().item(ITEM).map(|item| item.surface()),
        Some(child_binding.surface())
    );
    assert_eq!(
        session.view().item(SECOND_ITEM).map(|item| item.surface()),
        Some(SURFACE)
    );
}

fn managed_tear_off_session() -> (DockspaceSession, NativeSurfaceBinding) {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ITEM, SECOND_ITEM]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    let mut policy = DockPolicy::default();
    policy.set_allow_native_surfaces(true);
    let mut session = DockspaceSession::from_backend_workspace(
        builder.build().expect("the tear-off workspace validates"),
        policy,
    )
    .expect("the tear-off session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed native provider enrolls");
    session
        .register_native_root(SURFACE, WINDOW)
        .expect("the root registration records");
    let mut registration = session.begin_host_frame().expect("the root frame begins");
    registration
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the root registration frame settles every surface");
    let report = registration
        .commit()
        .expect("the root registration commits");
    let binding = match report.inputs() {
        [super::super::super::HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one root registration, got {outcomes:?}"),
    };
    session
        .report_managed_native_snapshot(
            [(binding, root_window_facts())],
            NativeWorkAreaRoster::Exact(vec![work_area(1)]),
        )
        .expect("the exact root snapshot records");
    commit_managed_frame(&mut session);
    measure_all_surfaces(&mut session);
    let (output, _) = paint_native_output(&mut session);
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .expect("the root output presentation records");
    commit_managed_frame(&mut session);
    (session, binding)
}

fn root_window_facts() -> NativeWindowFacts {
    let content = PhysicalRect::new(ROOT_ORIGIN_X, ROOT_ORIGIN_Y, 640.0, 480.0)
        .expect("the root content bounds validate");
    let outer = PhysicalRect::new(ROOT_ORIGIN_X - 8.0, ROOT_ORIGIN_Y - 30.0, 656.0, 518.0)
        .expect("the root outer bounds validate");
    NativeWindowFacts::live()
        .with_content_bounds(content)
        .with_outer_bounds(outer)
        .with_native_scale_factor(native_scale())
        .with_presentation_scale_factor(native_scale())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None)
}

fn native_scale() -> ScaleFactor {
    ScaleFactor::new(1.0).expect("the native scale validates")
}

fn presented_tab_receiver(
    session: &mut DockspaceSession,
    item: ItemId,
) -> DockspaceReceiverDescriptor {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the receiver inspection frame begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("the source plan resolves")
        .expect("the source plan is ready");
    let tab = plan
        .tabs()
        .find(|tab| tab.item() == item)
        .expect("the source tab is present");
    plan.receiver_for_tab_body(tab)
        .expect("the source tab exposes its exact drag receiver")
}

fn receiver_answer(
    query: NativeReceiverQuery,
    descriptor: DockspaceReceiverDescriptor,
) -> NativeReceiverAnswer {
    query
        .bind_receiver(&descriptor)
        .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
}

fn measure_all_surfaces(session: &mut DockspaceSession) {
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("test minimum validates"),
        80.0,
    )
    .expect("test metrics validate");
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the measurement frame begins");
    for surface in frame.surfaces() {
        frame
            .measure_surface(surface, metrics)
            .expect("the complete surface measurement stages");
    }
    frame.commit().expect("the complete measurements commit");
}

fn present_surface_outputs(session: &mut DockspaceSession, report: &mut HostFrameReport) {
    let outputs = report.take_painted_outputs();
    assert!(!outputs.is_empty(), "the frame must emit a painted output");
    for output in outputs {
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the exact painted output presentation records");
    }
}

fn paint_staging(
    session: &mut DockspaceSession,
    binding: NativeSurfaceBinding,
    phase: NativeStagingPresentationPhase,
) -> crate::runtime::PaintedNativeStagingOutput {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the staging frame begins");
    let request = frame
        .native_staging_paints()
        .into_iter()
        .find(|request| request.binding() == binding && request.phase() == phase)
        .unwrap_or_else(|| panic!("the {phase:?} staging request must exist"));
    frame
        .confirm_native_staging_painted(request)
        .expect("the exact staging placeholder is painted");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the staging frame retains semantic surfaces");
    let mut report = frame.commit().expect("the staging frame commits");
    let outputs = report.take_painted_native_staging_outputs();
    let [output] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
        panic!("expected one {phase:?} staging output, got {outputs:?}")
    });
    output
}

fn commit_managed_frame_report(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the managed frame begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the managed frame settles every surface");
    frame.commit().expect("the managed frame commits")
}

fn take_only_native_effect(report: &mut HostFrameReport) -> crate::runtime::NativeEffectRequest {
    let effects = report.take_native_effects();
    let [effect] = effects
        .try_into()
        .unwrap_or_else(|effects: Vec<_>| panic!("expected one native effect, got {effects:?}"));
    effect
}
