//! Default native placement through the public presentation-command and effect boundary.

use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockspaceActionRejection, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceHostFrame, DockspacePresentationCommandKind, DockspacePresentationCommandUnavailable,
    DockspaceSession, HostFrameReport, HostWindowToken, HostWorkAreaToken, NativeEffectOperation,
    NativeHostCapabilities, NativeHostCapability, NativeHostErrorKind, NativePointerRoster,
    NativeReceiverAnswer, NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, NativeWorkAreaFacts, NativeWorkAreaRoster,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const HOST: SurfaceId = SurfaceId::new(1);
const MAIN_ROOT: RootId = RootId::new(1);
const CHILD_ROOT: RootId = RootId::new(2);
const MAIN_ITEM: ItemId = ItemId::new(1);
const CHILD_ITEM: ItemId = ItemId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);

struct NativeHost {
    session: DockspaceSession,
    root: NativeSurfaceBinding,
    facts: NativeWindowFacts,
}

impl NativeHost {
    fn new(roster: NativeWorkAreaRoster) -> Self {
        Self::with_native_scale(roster, 1.0)
    }

    fn with_native_scale(roster: NativeWorkAreaRoster, native_scale: f64) -> Self {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            HOST,
            DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([MAIN_ITEM])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(CHILD_ROOT, DockspaceNode::tabs([CHILD_ITEM])),
            LogicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap(),
        ))])
        .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .unwrap();
        let capabilities = [
            NativeHostCapability::NativeWindowLifecycle,
            NativeHostCapability::AuthoritativeInventory,
            NativeHostCapability::GlobalWindowPlacement,
            NativeHostCapability::WorkArea,
            NativeHostCapability::PointerHitTestObservation,
            NativeHostCapability::CloseCancellation,
        ]
        .into_iter()
        .fold(NativeHostCapabilities::none_supported(), |caps, cap| {
            caps.with(cap)
        });
        session
            .configure_managed_native_capabilities(capabilities)
            .unwrap();
        session
            .register_native_root(HOST, HostWindowToken::new(1))
            .unwrap();
        let registration = idle(&mut session);
        let [root] = registration.native_bindings() else {
            panic!("the host must publish exactly one registered root");
        };
        let root = *root;
        let facts = local_facts(native_scale).with_outer_bounds(rect(92.0, 50.0, 656.0, 518.0));
        let mut host = Self {
            session,
            root,
            facts,
        };
        host.observe(roster);
        host.present();
        host
    }

    fn observe(&mut self, roster: NativeWorkAreaRoster) {
        self.session
            .report_managed_native_snapshot([(self.root, self.facts)], roster)
            .unwrap();
        idle(&mut self.session);
    }

    fn present(&mut self) {
        let mut measure = native_frame(&mut self.session);
        measure
            .measure_surface(
                HOST,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        assert!(finish(measure).take_native_effects().is_empty());
        let mut frame = native_frame(&mut self.session);
        let plan = frame.paint_plan(HOST).unwrap().unwrap();
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let mut report = finish(frame);
        assert!(report.take_native_effects().is_empty());
        let [output] = report.take_painted_outputs().try_into().unwrap();
        self.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        idle(&mut self.session);
    }

    fn request(&mut self, root: RootId) -> PhysicalRect {
        let version = self.session.version();
        let topology = format!("{:?}", self.session.view());
        let mut frame = native_frame(&mut self.session);
        let plan = frame.paint_plan(HOST).unwrap().unwrap();
        let command = plan
            .presentation_command(root, DockspacePresentationCommandKind::MoveToNewWindow)
            .unwrap()
            .unwrap();
        assert!(command.is_ready(), "{:?}", command.unavailable_reason());
        let action = plan.prepare_presentation_command(command).unwrap();
        frame.submit_surface_action(action).unwrap();
        let mut report = finish(frame);
        assert!(!report.workspace_changed());
        assert!(report.native_admissions().is_empty());
        let [create] = report.take_native_effects().try_into().unwrap();
        let (child, placement) = match create.operation() {
            NativeEffectOperation::CreateWindow {
                binding, placement, ..
            } => (*binding, *placement),
            operation => panic!("expected the default native create request, got {operation:?}"),
        };
        let _acknowledgement = create.accepted();
        // Dispatch acceptance and requested bounds do not establish observed geometry.
        idle(&mut self.session);
        assert_eq!(self.session.version(), version);
        assert_eq!(format!("{:?}", self.session.view()), topology);
        assert!(self.session.has_pending_native_create());
        assert!(self.session.native_local_coordinates(child).is_none());
        assert!(self.session.presented_surface(child.surface()).is_none());
        assert!(self.session.view().surface(child.surface()).is_none());
        placement
    }

    fn assert_unavailable(&mut self, reason: DockspaceActionRejection) {
        let version = self.session.version();
        let mut frame = native_frame(&mut self.session);
        let plan = frame.paint_plan(HOST).unwrap().unwrap();
        let command = plan
            .presentation_command(
                CHILD_ROOT,
                DockspacePresentationCommandKind::MoveToNewWindow,
            )
            .unwrap()
            .unwrap();
        assert!(!command.is_ready());
        assert_eq!(
            command.unavailable_reason(),
            Some(DockspacePresentationCommandUnavailable::Action(reason)),
        );
        assert!(plan.prepare_presentation_command(command).is_none());
        let mut report = finish(frame);
        assert!(report.take_native_effects().is_empty());
        assert!(!report.workspace_changed());
        assert_eq!(self.session.version(), version);
        assert!(!self.session.has_pending_native_create());
    }
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> PhysicalRect {
    PhysicalRect::new(x, y, width, height).unwrap()
}

fn local_facts(native_scale: f64) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(rect(100.0, 80.0, 640.0, 480.0))
        .with_native_scale_factor(ScaleFactor::new(native_scale).unwrap())
        .with_presentation_scale_factor(ScaleFactor::new(1.0).unwrap())
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
}

fn area(token: u64, bounds: PhysicalRect) -> NativeWorkAreaFacts {
    NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(token),
        bounds,
        ScaleFactor::new(1.0).unwrap(),
    )
}

fn desktop() -> NativeWorkAreaRoster {
    NativeWorkAreaRoster::Exact(vec![area(1, rect(0.0, 0.0, 1920.0, 1080.0))])
}

fn native_frame(session: &mut DockspaceSession) -> DockspaceHostFrame<'_> {
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap()
}

fn finish(mut frame: DockspaceHostFrame<'_>) -> HostFrameReport {
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

fn idle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut report = finish(native_frame(session));
    assert!(report.take_native_effects().is_empty());
    report
}

#[test]
fn ordinary_main_and_contained_defaults_preserve_outer_insets_and_cascade() {
    for (root, expected) in [
        (MAIN_ROOT, rect(116.0, 74.0, 656.0, 518.0)),
        (CHILD_ROOT, rect(156.0, 124.0, 376.0, 318.0)),
    ] {
        let mut host = NativeHost::new(desktop());
        assert_eq!(host.request(root), expected);
    }
}

#[test]
fn gap_at_requested_center_uses_largest_intersection_before_cascade() {
    // Requested outer bounds are (132, 100, 376, 318), centered in the gap at x=320.
    // The original rectangle overlaps the left area more; cascading first would pick right.
    let left = area(9, rect(-800.0, 0.0, 1100.0, 1000.0));
    let right = area(1, rect(360.0, 0.0, 800.0, 1000.0));
    let mut host = NativeHost::new(NativeWorkAreaRoster::Exact(vec![right, left]));
    assert_eq!(host.request(CHILD_ROOT), rect(-76.0, 124.0, 376.0, 318.0));
}

#[test]
fn overlapping_areas_use_the_largest_intersection_without_early_exit() {
    // Both areas contain the center and exceed half the requested area.
    // The second token has a larger intersection and must still be considered.
    let left = area(1, rect(-500.0, 0.0, 900.0, 1000.0));
    let right = area(9, rect(200.0, 0.0, 1000.0, 1000.0));
    let mut host = NativeHost::new(NativeWorkAreaRoster::Exact(vec![left, right]));
    assert_eq!(host.request(CHILD_ROOT), rect(200.0, 124.0, 376.0, 318.0));
}

#[test]
fn equal_intersections_choose_the_lowest_token_in_either_roster_order() {
    let left = area(9, rect(-500.0, 0.0, 900.0, 1000.0));
    let right = area(2, rect(240.0, 0.0, 1000.0, 1000.0));
    for roster in [vec![left, right], vec![right, left]] {
        let mut host = NativeHost::new(NativeWorkAreaRoster::Exact(roster));
        assert_eq!(host.request(CHILD_ROOT), rect(240.0, 124.0, 376.0, 318.0));
    }
}

#[test]
fn no_intersection_uses_rectangle_distance_instead_of_monitor_centers() {
    let left = area(9, rect(-900.0, 0.0, 1000.0, 1000.0));
    let right = area(1, rect(560.0, 0.0, 600.0, 1000.0));
    let mut host = NativeHost::new(NativeWorkAreaRoster::Exact(vec![right, left]));
    // The gaps are 32 and 52 pixels; the right area's center is closer.
    assert_eq!(host.request(CHILD_ROOT), rect(-276.0, 124.0, 376.0, 318.0));
}

#[test]
fn equal_nearest_distances_choose_the_lowest_token_in_either_roster_order() {
    let left = area(9, rect(-900.0, 0.0, 1000.0, 1000.0));
    let right = area(2, rect(540.0, 0.0, 600.0, 1000.0));
    for roster in [vec![left, right], vec![right, left]] {
        let mut host = NativeHost::new(NativeWorkAreaRoster::Exact(roster));
        assert_eq!(host.request(CHILD_ROOT), rect(540.0, 124.0, 376.0, 318.0));
    }
}

#[test]
fn selected_area_clamps_size_and_position_independently_on_each_axis() {
    for (bounds, expected) in [
        (
            rect(180.0, 160.0, 240.0, 180.0),
            rect(180.0, 160.0, 240.0, 180.0),
        ),
        (
            rect(180.0, 0.0, 240.0, 1000.0),
            rect(180.0, 124.0, 240.0, 318.0),
        ),
    ] {
        let mut host = NativeHost::new(NativeWorkAreaRoster::Exact(vec![area(1, bounds)]));
        assert_eq!(host.request(CHILD_ROOT), expected);
    }
}

#[test]
fn cascade_uses_source_native_scale_once_and_work_area_bounds_stay_physical() {
    let roster = NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        rect(0.0, 0.0, 1920.0, 1080.0),
        ScaleFactor::new(3.0).unwrap(),
    )]);
    let mut host = NativeHost::with_native_scale(roster, 2.0);
    assert_eq!(host.request(CHILD_ROOT), rect(180.0, 148.0, 376.0, 318.0));
}

#[test]
fn unknown_roster_has_no_default_even_with_exact_window_geometry() {
    let mut host = NativeHost::new(NativeWorkAreaRoster::Unknown);
    host.assert_unavailable(DockspaceActionRejection::NativeUnavailable);
}

#[test]
fn unknown_roster_revokes_old_areas_until_a_new_authoritative_roster_arrives() {
    let mut host = NativeHost::new(desktop());
    assert!(
        host.session
            .native_work_area(HostWorkAreaToken::new(1))
            .is_some()
    );
    host.observe(NativeWorkAreaRoster::Unknown);
    assert!(
        host.session
            .native_work_area(HostWorkAreaToken::new(1))
            .is_none()
    );
    host.assert_unavailable(DockspaceActionRejection::NativeUnavailable);
    host.observe(NativeWorkAreaRoster::Exact(vec![area(
        2,
        rect(400.0, 0.0, 1000.0, 1000.0),
    )]));
    assert_eq!(host.request(CHILD_ROOT), rect(400.0, 124.0, 376.0, 318.0));
}

#[test]
fn unknown_source_outer_bounds_do_not_reuse_historical_window_facts() {
    let mut host = NativeHost::new(desktop());
    host.facts = local_facts(1.0);
    host.observe(desktop());
    host.present();
    host.assert_unavailable(DockspaceActionRejection::PresentationUnavailable { surface: HOST });
}

#[test]
fn empty_and_zero_area_rosters_are_rejected_without_granting_placement_authority() {
    let mut host = NativeHost::new(NativeWorkAreaRoster::Unknown);
    for roster in [
        NativeWorkAreaRoster::Exact(Vec::new()),
        NativeWorkAreaRoster::Exact(vec![area(1, rect(0.0, 0.0, 0.0, 1080.0))]),
        NativeWorkAreaRoster::Exact(vec![area(1, rect(0.0, 0.0, 1920.0, 0.0))]),
    ] {
        let recorded = host.session.native_input_recorded_count();
        let error = host
            .session
            .report_managed_native_snapshot([(host.root, host.facts)], roster)
            .unwrap_err();
        assert_eq!(error.native_kind(), Some(NativeHostErrorKind::InvalidFacts));
        assert_eq!(host.session.native_input_recorded_count(), recorded);
        idle(&mut host.session);
        host.assert_unavailable(DockspaceActionRejection::NativeUnavailable);
    }
    host.observe(desktop());
    assert_eq!(host.request(CHILD_ROOT), rect(156.0, 124.0, 376.0, 318.0));
}

#[test]
fn foreign_snapshot_cannot_grant_work_area_authority_to_another_host() {
    let foreign = NativeHost::new(desktop());
    let mut host = NativeHost::new(NativeWorkAreaRoster::Unknown);
    let recorded = host.session.native_input_recorded_count();
    let error = host
        .session
        .report_managed_native_snapshot([(foreign.root, foreign.facts)], desktop())
        .unwrap_err();
    assert_eq!(error.native_kind(), Some(NativeHostErrorKind::StaleBinding));
    assert_eq!(host.session.native_input_recorded_count(), recorded);
    idle(&mut host.session);
    host.assert_unavailable(DockspaceActionRejection::NativeUnavailable);
    host.observe(desktop());
    assert_eq!(host.request(CHILD_ROOT), rect(156.0, 124.0, 376.0, 318.0));
}
