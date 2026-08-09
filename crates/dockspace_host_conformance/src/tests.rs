use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome, DockspaceAxis,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, DockspaceTabsView,
    DockspaceView, ItemId, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceCloseOutcome, DockspaceHostFrame, DockspaceInteractionError,
    DockspaceReceiverDescriptor, DockspaceReceiverRole, DockspaceSession, DockspaceVisualKind,
    HostCloseRequestOrigin, HostFrameReport, HostInputOutcome, HostWindowToken, NativeCloseState,
    NativePlatformError, NativeSurfaceBinding, NativeWindowFacts, NativeWindowInputState,
    NativeWindowPresentationState, PresentedDockReceiver, PresentedDockspaceSurface,
    SurfacePointerButton, SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent,
    SurfacePointerId, SurfacePointerInput, SurfacePointerPosition, SurfacePointerReceiverFacts,
    SurfacePresentationResult, SurfaceScrollDelta, SurfaceScrollDeviceId, SurfaceScrollEvent,
    SurfaceScrollModifiers, SurfaceScrollMomentum, SurfaceScrollPhase, SurfaceScrollSequenceId,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace::{CloseDecision, CloseResolutionOutcome};

const SURFACE: SurfaceId = SurfaceId::new(1);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const SOURCE_ROOT: RootId = RootId::new(2);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);
const X: ItemId = ItemId::new(4);
const WINDOW: HostWindowToken = HostWindowToken::new(41);
const SECOND_WINDOW: HostWindowToken = HostWindowToken::new(42);

fn ready_window_facts(physical: PhysicalRect, scale: ScaleFactor) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(physical)
        .with_outer_bounds(physical)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible, None)
        .with_close(NativeCloseState::Clear, None)
}

struct DeterministicHost {
    session: DockspaceSession,
}

impl DeterministicHost {
    fn new(layout: DockspaceLayout) -> Self {
        Self {
            session: DockspaceSession::from_layout(layout, DockPolicy::default())
                .expect("the conformance workspace must initialize"),
        }
    }

    fn view(&self) -> DockspaceView<'_> {
        self.session.view()
    }

    fn version(&self) -> WorkspaceVersion {
        self.session.version()
    }

    fn run(&mut self, mutate: impl FnOnce(&mut DockspaceHostFrame<'_>)) -> HostFrameReport {
        let mut frame = self
            .session
            .begin_host_frame()
            .expect("the deterministic host frame must begin");
        mutate(&mut frame);
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the driver explicitly settles every unpainted surface");
        frame
            .commit()
            .expect("the host frame must commit atomically")
    }

    fn observe_painted_outputs(&mut self, mut report: HostFrameReport) {
        let outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 1, "the fixture paints one logical surface");
        for output in outputs {
            self.session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .expect("the host confirms the exact output it presented");
        }
        self.run(|_| {});
    }

    fn register_native_root(
        &mut self,
        surface: SurfaceId,
        token: HostWindowToken,
    ) -> HostFrameReport {
        self.session
            .register_native_root(surface, token)
            .expect("the native registration joins the backend ingress order");
        self.run(|_| {})
    }

    fn report_native_snapshot(
        &mut self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
    ) -> HostFrameReport {
        self.session
            .report_native_snapshot(observations)
            .expect("the native snapshot joins the backend ingress order");
        self.run(|_| {})
    }

    fn publish_native_close(
        &mut self,
        binding: NativeSurfaceBinding,
        state: NativeCloseState,
    ) -> HostFrameReport {
        self.session
            .publish_native_close(binding, state, None)
            .expect("the native close fact joins the backend ingress order");
        self.run(|_| {})
    }
}

fn tabs_layout(items: impl IntoIterator<Item = ItemId>) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs(items)),
    )])
    .expect("the tabs layout is valid")
}

fn tabs_containing(view: DockspaceView<'_>, item: ItemId) -> Option<DockspaceTabsView<'_>> {
    view.item(item).map(|item| item.tabs())
}

fn assert_product_dock_applied(report: &HostFrameReport, item: ItemId) {
    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Docked {
                item: moved,
                changed: true,
                ..
            }
        )] if *moved == item
    ));
}

fn pointer_close_host() -> (DeterministicHost, DockspaceReceiverDescriptor) {
    let mut host = DeterministicHost::new(tabs_layout([A, B]));
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the minimum is valid");
    let metrics =
        UniformSurfaceMetrics::new(bounds, minimum, 72.0).expect("the measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the close fixture measures its surface");
    });

    let mut close = None;
    let paint = host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the pointer phase closes before paint")
            .expect("the measured surface is ready");
        let close_bounds = plan
            .tabs()
            .find(|tab| tab.item() == A)
            .and_then(|tab| tab.close_bounds())
            .expect("item A exposes a close control");
        close = plan.receivers().find(|receiver| {
            receiver.role() == DockspaceReceiverRole::TabClose && receiver.bounds() == close_bounds
        });
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the exact close output was painted");
    });
    host.observe_painted_outputs(paint);
    host.session
        .enable_surface_pointer(SURFACE)
        .expect("the presented surface admits pointer input");
    (
        host,
        close.expect("the close receiver is part of the public paint plan"),
    )
}

fn press_pointer_close(
    host: &mut DeterministicHost,
    descriptor: &DockspaceReceiverDescriptor,
) -> (PresentedDockReceiver, PresentedDockspaceSurface) {
    let receiver = host
        .session
        .bind_presented_receiver(descriptor)
        .expect("the close receiver belongs to the current presented output");
    let surface = host
        .session
        .presented_surface(SURFACE)
        .expect("the close surface remains presented");
    let point = receiver.center();
    let pressed = host.run(|frame| {
        frame
            .submit_surface_pointer(SurfacePointerInput::new(
                SurfacePointerId::new(77),
                SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                SurfacePointerPosition::Known(point),
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::delivery(&receiver),
            ))
            .expect("the exact close receiver accepts the press");
    });
    assert!(pressed.inputs().is_empty());
    (receiver, surface)
}

fn submit_pointer_close_release(
    frame: &mut DockspaceHostFrame<'_>,
    receiver: PresentedDockReceiver,
    surface: PresentedDockspaceSurface,
) {
    let point = receiver.center();
    frame
        .submit_surface_pointer(SurfacePointerInput::new(
            SurfacePointerId::new(77),
            SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
            SurfacePointerPosition::Known(point),
            SurfacePointerCapture::None,
            SurfacePointerReceiverFacts::delivery(&receiver).with_no_hover(&surface),
        ))
        .expect("one release supplies delivery and hover facts together");
}

fn submit_pointer_close_contact_end(
    frame: &mut DockspaceHostFrame<'_>,
    receiver: PresentedDockReceiver,
    surface: PresentedDockspaceSurface,
) {
    let point = receiver.center();
    frame
        .submit_surface_pointer(SurfacePointerInput::new(
            SurfacePointerId::new(77),
            SurfacePointerEvent::ContactEnded(SurfacePointerButton::Primary),
            SurfacePointerPosition::Known(point),
            SurfacePointerCapture::None,
            SurfacePointerReceiverFacts::delivery(&receiver).with_no_hover(&surface),
        ))
        .expect("one contact end supplies delivery and hover facts together");
}

fn request_pointer_close(
    host: &mut DeterministicHost,
    descriptor: &DockspaceReceiverDescriptor,
) -> dockspace::ClosePlan {
    let (receiver, surface) = press_pointer_close(host, descriptor);
    let released = host.run(|frame| {
        submit_pointer_close_release(frame, receiver, surface);
    });
    match released.inputs() {
        [
            HostInputOutcome::CloseRequested {
                plan,
                reused: false,
                origin: HostCloseRequestOrigin::Interaction,
            },
        ] => plan.clone(),
        outcomes => panic!("expected one pointer close request, got {outcomes:?}"),
    }
}

fn request_pointer_close_with_contact_end(
    host: &mut DeterministicHost,
    descriptor: &DockspaceReceiverDescriptor,
) -> dockspace::ClosePlan {
    let (receiver, surface) = press_pointer_close(host, descriptor);
    let released = host.run(|frame| {
        submit_pointer_close_contact_end(frame, receiver, surface);
    });
    match released.inputs() {
        [
            HostInputOutcome::CloseRequested {
                plan,
                reused: false,
                origin: HostCloseRequestOrigin::Interaction,
            },
        ] => plan.clone(),
        outcomes => panic!("expected one contact-end close request, got {outcomes:?}"),
    }
}

#[test]
fn pointer_close_report_exposes_one_plan_for_veto_and_allow() {
    let (mut host, close) = pointer_close_host();
    let before = host.version();

    let veto_plan = request_pointer_close(&mut host, &close);
    let veto_item = veto_plan
        .items()
        .iter()
        .find(|item| item.item() == A)
        .expect("the pointer close plan names item A");
    let veto = host.run(|frame| {
        frame
            .resolve_close(veto_plan.request(), veto_item.token(), CloseDecision::Veto)
            .expect("the host can veto the pointer-created plan");
    });
    assert!(matches!(
        veto.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Vetoed { request, item: A },
            changed: false,
            ..
        }] if *request == veto_plan.request()
    ));
    assert_eq!(host.version(), before);

    let allow_plan = request_pointer_close(&mut host, &close);
    let allow_item = allow_plan
        .items()
        .iter()
        .find(|item| item.item() == A)
        .expect("the replacement close plan names item A");
    let allow = host.run(|frame| {
        frame
            .resolve_close(
                allow_plan.request(),
                allow_item.token(),
                CloseDecision::Allow,
            )
            .expect("the host can allow the pointer-created plan");
    });
    assert!(matches!(
        allow.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Approved { request },
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: A, .. })),
            changed: true,
            ..
        }] if *request == allow_plan.request()
    ));
    assert!(tabs_containing(host.view(), A).is_none());
}

#[test]
fn runtime_contact_end_allows_the_same_pointer_identity_to_start_a_new_gesture() {
    let (mut host, close) = pointer_close_host();
    let before = host.version();

    let first_plan = request_pointer_close_with_contact_end(&mut host, &close);
    let first_item = first_plan
        .items()
        .iter()
        .find(|item| item.item() == A)
        .expect("the first contact owns item A");
    host.run(|frame| {
        frame
            .resolve_close(
                first_plan.request(),
                first_item.token(),
                CloseDecision::Veto,
            )
            .expect("the first contact close plan can be retired");
    });

    let second_plan = request_pointer_close_with_contact_end(&mut host, &close);
    assert_ne!(
        second_plan.request(),
        first_plan.request(),
        "reusing the provider-local pointer ID must create a fresh gesture and close plan",
    );
    let second_item = second_plan
        .items()
        .iter()
        .find(|item| item.item() == A)
        .expect("the successor contact still owns item A");
    host.run(|frame| {
        frame
            .resolve_close(
                second_plan.request(),
                second_item.token(),
                CloseDecision::Veto,
            )
            .expect("the successor contact close plan can be retired");
    });

    assert_eq!(host.version(), before);
}

#[test]
fn host_report_preserves_application_and_pointer_close_order() {
    let (mut host, close) = pointer_close_host();

    let (receiver, surface) = press_pointer_close(&mut host, &close);
    let application_first = host.run(|frame| {
        frame
            .select_item_current(A)
            .expect("the no-op selection joins the host frame");
        submit_pointer_close_release(frame, receiver, surface);
    });
    let first_plan = match application_first.inputs() {
        [
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item: A,
                changed: false,
            }),
            HostInputOutcome::CloseRequested {
                plan,
                reused: false,
                origin: HostCloseRequestOrigin::Interaction,
            },
        ] => plan.clone(),
        outcomes => panic!("application then pointer order changed: {outcomes:?}"),
    };
    let first_item = first_plan
        .items()
        .iter()
        .find(|item| item.item() == A)
        .expect("the first close plan names item A");
    host.run(|frame| {
        frame
            .resolve_close(
                first_plan.request(),
                first_item.token(),
                CloseDecision::Veto,
            )
            .expect("the first close plan is retired before the next click");
    });

    let (receiver, surface) = press_pointer_close(&mut host, &close);
    let pointer_first = host.run(|frame| {
        submit_pointer_close_release(frame, receiver, surface);
        frame
            .select_item_current(A)
            .expect("the later no-op selection joins the host frame");
    });
    let second_plan = match pointer_first.inputs() {
        [
            HostInputOutcome::CloseRequested {
                plan,
                reused: false,
                origin: HostCloseRequestOrigin::Interaction,
            },
            HostInputOutcome::ProductActionApplied(DockspaceActionOutcome::Selected {
                item: A,
                changed: false,
            }),
        ] => plan.clone(),
        outcomes => panic!("pointer then application order changed: {outcomes:?}"),
    };
    let second_item = second_plan
        .items()
        .iter()
        .find(|item| item.item() == A)
        .expect("the second close plan names item A");
    host.run(|frame| {
        frame
            .resolve_close(
                second_plan.request(),
                second_item.token(),
                CloseDecision::Veto,
            )
            .expect("the second close plan can also be retired");
    });
}

#[test]
fn runtime_paint_plan_exposes_complete_stable_renderer_geometry() {
    let split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([A]), DockspaceNode::tabs([B])],
    )
    .expect("the renderer fixture split is valid");
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, split),
    )])
    .expect("the renderer fixture layout is canonical");
    let mut host = DeterministicHost::new(layout);
    let bounds = LogicalRect::new(0.0, 0.0, 800.0, 480.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(80.0, 60.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 96.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the renderer supplies the complete measurement manifest");
    });

    let mut first_visuals = Vec::new();
    let first_output = host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the paint phase is available")
            .expect("the measured surface has one complete plan");
        assert_eq!(plan.surface(), SURFACE);
        assert_eq!(plan.bounds(), bounds);

        let panes = plan.panes().collect::<Vec<_>>();
        assert_eq!(panes.len(), 2);
        assert!(panes.iter().all(|pane| pane.root() == ROOT));
        assert_eq!(
            panes
                .iter()
                .filter_map(|pane| pane.selected())
                .collect::<Vec<_>>(),
            [A, B]
        );
        first_visuals.extend(panes.iter().map(|pane| pane.visual_id()));

        let tabs = plan.tabs().collect::<Vec<_>>();
        assert_eq!(
            tabs.iter().map(|tab| tab.item()).collect::<Vec<_>>(),
            [A, B]
        );
        assert!(tabs.iter().all(|tab| tab.selected()));
        first_visuals.extend(tabs.iter().map(|tab| tab.visual_id()));

        let tab_bars = plan.tab_bars().collect::<Vec<_>>();
        assert_eq!(tab_bars.len(), 2);
        assert!(tab_bars.iter().all(|bar| bar.members().len() == 1));
        first_visuals.extend(tab_bars.iter().map(|bar| bar.visual_id()));

        let splitters = plan.splitters().collect::<Vec<_>>();
        assert_eq!(splitters.len(), 1);
        assert_eq!(splitters[0].axis(), DockspaceAxis::Horizontal);
        assert!(splitters[0].operable());
        assert!(splitters[0].hit_bounds().width() > 0.0);
        first_visuals.push(splitters[0].visual_id());

        assert_eq!(plan.splitter_junctions().len(), 0);
        assert_eq!(plan.contained().len(), 0);
        let guides = plan.drop_guides().collect::<Vec<_>>();
        assert!(!guides.is_empty());
        assert!(guides.iter().all(|guide| guide.targets().count() >= 4));
        first_visuals.extend(guides.iter().map(|guide| guide.visual_id()));
        let roles = plan
            .receivers()
            .map(|receiver| receiver.role())
            .collect::<Vec<_>>();
        assert!(roles.contains(&DockspaceReceiverRole::PaneBody));
        assert!(roles.contains(&DockspaceReceiverRole::TabBody));
        assert!(roles.contains(&DockspaceReceiverRole::Splitter));
        assert!(first_visuals.iter().all(|id| {
            matches!(
                id.kind(),
                DockspaceVisualKind::Pane
                    | DockspaceVisualKind::Tab
                    | DockspaceVisualKind::TabBar
                    | DockspaceVisualKind::Splitter
                    | DockspaceVisualKind::DropGuide
            )
        }));

        frame
            .confirm_surface_painted(SURFACE)
            .expect("the host confirms the complete plan it painted");
    });
    host.observe_painted_outputs(first_output);

    let second_output = host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the retained paint phase is available")
            .expect("the retained surface remains paintable");
        let mut current = plan
            .panes()
            .map(|record| record.visual_id())
            .chain(plan.tabs().map(|record| record.visual_id()))
            .chain(plan.tab_bars().map(|record| record.visual_id()))
            .chain(plan.splitters().map(|record| record.visual_id()))
            .chain(plan.drop_guides().map(|record| record.visual_id()))
            .collect::<Vec<_>>();
        first_visuals.sort_unstable();
        current.sort_unstable();
        assert_eq!(current, first_visuals, "visual identity is output-stable");
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the retained complete plan was painted");
    });
    host.observe_painted_outputs(second_output);
}

#[test]
fn dropped_output_retires_without_granting_interaction_authority() {
    let mut host = DeterministicHost::new(tabs_layout([A]));
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the surface measurements are complete");
    });
    let mut paint = host.run(|frame| {
        assert!(
            frame
                .paint_plan(SURFACE)
                .expect("the paint phase is available")
                .is_some()
        );
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the host records the exact output it painted");
    });
    let mut outputs = paint.take_painted_outputs();
    assert_eq!(outputs.len(), 1, "the fixture paints one exact output");
    let output = outputs.pop().expect("the checked output exists");
    host.session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .expect("the renderer may authoritatively drop an output");
    host.run(|_| {});

    let error = host
        .session
        .enable_surface_pointer(SURFACE)
        .expect_err("a dropped output grants no interaction authority");
    assert!(matches!(
        error.interaction_error(),
        Some(
            dockspace::runtime::DockspaceInteractionError::PresentationAuthorityUnavailable {
                surface: SURFACE
            }
        )
    ));
}

#[test]
fn surface_pointer_waits_for_the_current_endpoint_to_be_presented() {
    let mut host = DeterministicHost::new(tabs_layout([A]));
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the logical surface measurements are complete");
    });
    let logical = host.run(|frame| {
        assert!(
            frame
                .paint_plan(SURFACE)
                .expect("the logical paint phase is available")
                .is_some()
        );
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the logical output was painted");
    });
    let error = host
        .session
        .enable_surface_pointer(SURFACE)
        .expect_err("an unsettled output grants no interaction authority");
    assert!(matches!(
        error.interaction_error(),
        Some(DockspaceInteractionError::PresentationAuthorityUnavailable { surface: SURFACE })
    ));
    host.observe_painted_outputs(logical);
    host.session
        .enable_surface_pointer(SURFACE)
        .expect("the finally presented logical endpoint admits pointer input");
}

#[test]
fn presentation_reports_queue_behind_a_dropped_joined_host_frame() {
    let mut host = DeterministicHost::new(tabs_layout([A]));
    host.session
        .enable_observed_native_roots()
        .expect("the joined presentation lane is active");
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the surface measurements are complete");
    });

    let mut first_report = host.run(|frame| {
        assert!(
            frame
                .paint_plan(SURFACE)
                .expect("the first paint phase is available")
                .is_some()
        );
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the first output is painted");
    });
    let mut first_outputs = first_report.take_painted_outputs();
    let first = first_outputs
        .pop()
        .expect("the first paint emits one capability");

    let mut second_report = host.run(|frame| {
        assert!(
            frame
                .paint_plan(SURFACE)
                .expect("the second paint phase is available")
                .is_some()
        );
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the second output is painted");
    });
    let mut second_outputs = second_report.take_painted_outputs();
    let second = second_outputs
        .pop()
        .expect("the second paint emits one capability");

    host.session
        .report_surface_presentation(first, SurfacePresentationResult::Presented)
        .expect("the first presented output is ready for the joined stream");
    drop(
        host.session
            .begin_host_frame()
            .expect("the joined frame freezes the first report before it is abandoned"),
    );
    host.session
        .report_surface_presentation(second, SurfacePresentationResult::Dropped)
        .expect("a later report queues behind the frozen replay record");
    host.run(|_| {});
    assert!(
        host.session.presented_surface(SURFACE).is_some(),
        "the replayed first report becomes authoritative before the queued tail"
    );
    host.run(|_| {});
    assert!(
        host.session.presented_surface(SURFACE).is_none(),
        "the queued dropped output retires the newer presentation obligation"
    );
}

#[test]
fn latest_terminal_result_is_independent_of_prelude_batching() {
    let mut host = DeterministicHost::new(tabs_layout([A]));
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the surface measurements are complete");
    });

    let mut first_report = host.run(|frame| {
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the first output is painted");
    });
    let first = first_report
        .take_painted_outputs()
        .pop()
        .expect("the first paint emits one capability");
    let mut second_report = host.run(|frame| {
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the second output is painted");
    });
    let second = second_report
        .take_painted_outputs()
        .pop()
        .expect("the second paint emits one capability");

    host.session
        .report_surface_presentation(first, SurfacePresentationResult::Presented)
        .expect("the earlier output reached final presentation");
    host.session
        .report_surface_presentation(second, SurfacePresentationResult::Dropped)
        .expect("the later output was authoritatively dropped");
    host.run(|_| {});

    assert!(
        host.session.presented_surface(SURFACE).is_none(),
        "the latest terminal output must revoke older presentation authority"
    );
}

#[test]
fn abandoned_output_does_not_block_newer_out_of_order_result() {
    let mut host = DeterministicHost::new(tabs_layout([A]));
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the surface measurements are complete");
    });

    let mut first_report = host.run(|frame| {
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the first output is painted");
    });
    let first = first_report
        .take_painted_outputs()
        .pop()
        .expect("the first paint emits one capability");
    let mut second_report = host.run(|frame| {
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the second output is painted");
    });
    let second = second_report
        .take_painted_outputs()
        .pop()
        .expect("the second paint emits one capability");

    host.session
        .report_surface_presentation(second, SurfacePresentationResult::Presented)
        .expect("the newer result may arrive before the older terminal fact");
    drop(first);
    host.run(|_| {});

    assert!(
        host.session.presented_surface(SURFACE).is_some(),
        "dropping the older capability must retire it and unblock the newer result"
    );
}

#[test]
fn ogc_01_repeated_same_axis_docks_flatten_and_stale_targets_are_inert() {
    let mut host = DeterministicHost::new(tabs_layout([A, B, C]));
    let fraction = DockFraction::new(0.5).expect("the fixture fraction is valid");
    let stale_action = host.session.prepare_dock_item(
        A,
        DockPlacement::InnerEdge {
            anchor: DockAnchor::Item(B),
            edge: DockEdge::Right,
            fraction,
        },
    );
    let first = host.run(|frame| {
        frame
            .dock_item_current(
                B,
                DockPlacement::InnerEdge {
                    anchor: DockAnchor::Item(A),
                    edge: DockEdge::Right,
                    fraction,
                },
            )
            .expect("the first product move must append");
    });
    assert_product_dock_applied(&first, B);

    let second = host.run(|frame| {
        frame
            .dock_item_current(
                C,
                DockPlacement::InnerEdge {
                    anchor: DockAnchor::Item(B),
                    edge: DockEdge::Right,
                    fraction,
                },
            )
            .expect("the second product move must append");
    });
    assert_product_dock_applied(&second, C);

    let split = host
        .view()
        .root(ROOT)
        .and_then(|root| root.content())
        .and_then(|node| node.split())
        .expect("the root must be a same-axis split");
    assert_eq!(split.axis(), DockspaceAxis::Horizontal);
    assert_eq!(split.child_count(), 3, "same-axis wrappers must flatten");

    let before_stale = host.version();
    let stale = host.run(|frame| {
        frame
            .submit_prepared_action(stale_action)
            .expect("a stale product action is still structurally accepted");
    });
    assert!(matches!(
        stale.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert_eq!(host.version(), before_stale);
}

#[test]
fn ogc_02_merge_and_close_restore_the_previous_target_selection_atomically() {
    let target = DockspaceNode::tabs_with_selection([A, B, C], Some(C))
        .expect("the target selection is valid");
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, target)),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([X])),
        ),
    ])
    .expect("the merge layout is valid");
    let mut host = DeterministicHost::new(layout);

    let select = host.run(|frame| {
        frame
            .select_item_current(B)
            .expect("selection must append through the product contract");
    });
    assert!(matches!(
        select.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: B,
                changed: true,
            }
        )]
    ));
    assert_eq!(
        tabs_containing(host.view(), B).and_then(DockspaceTabsView::selected),
        Some(B)
    );

    let merge = host.run(|frame| {
        frame
            .dock_root_current(SOURCE_ROOT, DockPlacement::Center(DockAnchor::Item(B)))
            .expect("the merge must append through the product contract");
    });
    assert!(matches!(
        merge.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::RootDocked {
                root: SOURCE_ROOT,
                target_root: ROOT,
                items,
                changed: true,
            }
        )] if items == &[X]
    ));
    assert_eq!(
        tabs_containing(host.view(), X).and_then(DockspaceTabsView::selected),
        Some(X),
        "merging a selected source makes the moved item current"
    );

    let request = host.run(|frame| {
        frame
            .request_close_item(X)
            .expect("the close request must append");
    });
    let plan = match request.inputs() {
        [
            HostInputOutcome::CloseRequested {
                plan,
                reused: false,
                origin: HostCloseRequestOrigin::Application,
            },
        ] => plan.clone(),
        outcomes => panic!("expected one new close plan, got {outcomes:?}"),
    };
    let close_item = plan
        .items()
        .iter()
        .copied()
        .find(|item| item.item() == X)
        .expect("the close plan contains item X");
    let close = host.run(|frame| {
        frame
            .resolve_close(plan.request(), close_item.token(), CloseDecision::Allow)
            .expect("the close decision must append");
    });
    assert!(matches!(
        close.inputs(),
        [HostInputOutcome::CloseDecisionProcessed {
            resolution: CloseResolutionOutcome::Approved { request },
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: X, .. })),
            changed: true,
            ..
        }] if *request == plan.request()
    ));

    let tabs = tabs_containing(host.view(), B).expect("the target tabs remain");
    assert_eq!(tabs.items(), &[A, B, C]);
    assert_eq!(tabs.selected(), Some(B));
    assert!(tabs_containing(host.view(), X).is_none());
}

struct InteractionFixture {
    host: DeterministicHost,
    source: DockspaceReceiverDescriptor,
    target: DockspaceReceiverDescriptor,
}

impl InteractionFixture {
    fn new() -> Self {
        let split = DockspaceNode::equal_split(
            DockspaceAxis::Horizontal,
            [DockspaceNode::tabs([A, B]), DockspaceNode::tabs([C])],
        )
        .expect("the fixture split is valid");
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, split),
        )])
        .expect("the interaction layout is canonical");
        let mut host = DeterministicHost::new(layout);
        let bounds =
            LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture surface bounds are valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
        let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
            .expect("the fixture measurements are valid");
        host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the first pass measures the complete surface");
        });

        let mut source = None;
        let mut target = None;
        let paint = host.run(|frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the pointer-input phase must close before paint")
                .expect("the measured surface exposes one paint plan");
            source = Some(
                plan.tab_receiver(A)
                    .expect("the source tab exposes its exact receiver"),
            );
            target = Some(
                plan.center_drop_receiver_for_item(C)
                    .expect("the target stack exposes its exact center receiver"),
            );
            assert!(plan.drag_preview().is_none());
            frame
                .confirm_surface_painted(SURFACE)
                .expect("the host records the exact plan it painted");
        });
        host.observe_painted_outputs(paint);
        host.session
            .enable_surface_pointer(SURFACE)
            .expect("the observed surface admits one local pointer provider");

        Self {
            host,
            source: source.expect("source descriptor was captured while painting"),
            target: target.expect("target descriptor was captured while painting"),
        }
    }

    fn current_source(&self) -> PresentedDockReceiver {
        self.host
            .session
            .bind_presented_receiver(&self.source)
            .expect("the source receiver belongs to the current presented output")
    }

    fn current_target(&self) -> PresentedDockReceiver {
        self.host
            .session
            .bind_presented_receiver(&self.target)
            .expect("the target receiver belongs to the current presented output")
    }

    fn begin_drag_without_target(&mut self) {
        let source = self.current_source();
        let press = source.center();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(SurfacePointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    SurfacePointerPosition::Known(press),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&source),
                ))
                .expect("the source tab receives the press");
        });

        let surface = self
            .host
            .session
            .presented_surface(SURFACE)
            .expect("the surface remains presented");
        let moved = LogicalPoint::new(press.x() + 24.0, press.y() + 24.0)
            .expect("the threshold-crossing point is valid");
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(SurfacePointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    SurfacePointerPosition::Known(moved),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::no_hover(&surface),
                ))
                .expect("the threshold move has an exact known-empty hover result");
        });
    }

    fn preview_target(&mut self) {
        let target = self.current_target();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(SurfacePointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    SurfacePointerPosition::Known(target.center()),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::hover(&target),
                ))
                .expect("the target receives one exact hover edge");
        });
    }

    fn paint_preview(&mut self) {
        let paint = self.host.run(|frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the pointer-input phase must close before paint")
                .expect("the active drag retains one paint plan");
            assert!(
                plan.drag_preview().is_some(),
                "the renderer must actually see the preview it confirms"
            );
            let guides = plan.drop_guides().collect::<Vec<_>>();
            assert!(
                !guides.is_empty(),
                "an active docking preview exposes its complete guide cluster"
            );
            assert!(
                guides.iter().any(|guide| guide.targets().count() >= 5),
                "one guide cluster exposes center and four directional targets"
            );
            frame
                .confirm_surface_painted(SURFACE)
                .expect("the preview-bearing output was painted");
        });
        self.host.observe_painted_outputs(paint);
    }

    fn attach_native_surface(&mut self) -> NativeSurfaceBinding {
        self.host
            .session
            .enable_observed_native_roots()
            .expect("the deterministic host enrolls one native platform provider");
        let registration = self.host.register_native_root(SURFACE, WINDOW);
        let binding = match registration.inputs() {
            [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
            outcomes => panic!("expected one native registration, got {outcomes:?}"),
        };

        let physical = PhysicalRect::new(0.0, 0.0, 640.0, 360.0)
            .expect("the fixture physical bounds are valid");
        let scale = ScaleFactor::new(1.0).expect("the fixture scale is valid");
        self.host
            .report_native_snapshot([(binding, ready_window_facts(physical, scale))]);

        let bounds =
            LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the fixture bounds are valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
        let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
            .expect("the fixture measurements are valid");
        self.host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("native coordinate authority admits a fresh measurement");
        });
        let mut source = None;
        let mut target = None;
        let paint = self.host.run(|frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the fresh native plan is inspectable")
                .expect("the fresh native plan is ready");
            source = plan.tab_receiver(A);
            target = plan.center_drop_receiver_for_item(C);
            frame
                .confirm_surface_painted(SURFACE)
                .expect("the native surface plan was painted");
        });
        self.host.observe_painted_outputs(paint);
        self.source = source.expect("the native source receiver is present");
        self.target = target.expect("the native target receiver is present");
        binding
    }
}

#[test]
fn runtime_pointer_batch_preserves_order_and_explicit_cancellation() {
    let mut fixture = InteractionFixture::new();
    let source = fixture.current_source();
    let surface = fixture
        .host
        .session
        .presented_surface(SURFACE)
        .expect("the surface remains presented");
    let press = source.center();
    let moved = LogicalPoint::new(press.x() + 24.0, press.y() + 24.0)
        .expect("the threshold-crossing point is valid");
    let before = fixture.host.version();

    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer_batch([
                SurfacePointerInput::new(
                    SurfacePointerId::new(7),
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    SurfacePointerPosition::Known(press),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&source),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(7),
                    SurfacePointerEvent::Moved,
                    SurfacePointerPosition::Known(moved),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::no_hover(&surface),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(7),
                    SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    ),
                    SurfacePointerPosition::Known(moved),
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::unknown(),
                ),
            ])
            .expect("one batch preserves press, move, then cancellation order");
    });

    assert_eq!(fixture.host.version(), before);
    fixture.host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the cancelled surface remains paintable")
            .expect("the retained surface has one paint plan");
        assert!(
            plan.drag_preview().is_none(),
            "the explicit terminal edge must not leave a drag session behind"
        );
    });
}

#[test]
fn runtime_pointer_batch_prevalidates_before_reducing_any_prefix() {
    let mut fixture = InteractionFixture::new();
    let source = fixture.current_source();
    let press = source.center();
    let before = fixture.host.version();

    fixture.host.run(|frame| {
        let error = frame
            .submit_surface_pointer_batch([
                SurfacePointerInput::new(
                    SurfacePointerId::new(9),
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    SurfacePointerPosition::Known(press),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::delivery(&source),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(9),
                    SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                        SurfaceScrollDeviceId::new(4),
                        SurfaceScrollPhase::Discrete {
                            delta: SurfaceScrollDelta::Lines {
                                x: f64::NAN,
                                y: 0.0,
                            },
                        },
                        SurfaceScrollMomentum::Direct,
                        SurfaceScrollModifiers::default(),
                    )),
                    SurfacePointerPosition::Known(press),
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::unknown(),
                ),
            ])
            .expect_err("the invalid second edge rejects the complete public batch");
        assert!(matches!(
            error.interaction_error(),
            Some(DockspaceInteractionError::InvalidScrollSample)
        ));
    });

    assert_eq!(fixture.host.version(), before);
    fixture.host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the rejected batch leaves the surface paintable")
            .expect("the retained surface has one paint plan");
        assert!(
            plan.drag_preview().is_none(),
            "the valid prefix was never reduced"
        );
    });
}

#[test]
fn runtime_pointer_batch_routes_discrete_and_smooth_scroll_to_the_core_owner() {
    let mut host = DeterministicHost::new(tabs_layout([A, B, C, X]));
    let bounds = LogicalRect::new(0.0, 0.0, 220.0, 180.0).expect("the fixture bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the fixture minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 96.0)
        .expect("the fixture measurements are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the first pass measures the complete overflow strip");
    });

    let mut scroll_receiver = None;
    let paint = host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the overflow strip is paintable")
            .expect("the measured surface has one paint plan");
        let bar = plan.tab_bars().next().expect("the tabs expose one bar");
        assert!(bar.maximum_scroll_offset() > 0.0);
        assert_eq!(bar.scroll_offset(), 0.0);
        scroll_receiver = plan
            .receivers()
            .find(|receiver| receiver.role() == DockspaceReceiverRole::TabStripScroll);
        frame
            .confirm_surface_painted(SURFACE)
            .expect("the initial overflow output was painted");
    });
    host.observe_painted_outputs(paint);
    host.session
        .enable_surface_pointer(SURFACE)
        .expect("the presented surface admits a local pointer provider");
    let receiver = host
        .session
        .bind_presented_receiver(
            &scroll_receiver.expect("the overflow strip exposes a scroll receiver"),
        )
        .expect("the scroll receiver belongs to the presented output");

    let smooth = SurfaceScrollSequenceId::new(1);
    host.run(|frame| {
        frame
            .submit_surface_pointer_batch([
                SurfacePointerInput::new(
                    SurfacePointerId::new(11),
                    SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                        SurfaceScrollDeviceId::new(3),
                        SurfaceScrollPhase::Discrete {
                            delta: SurfaceScrollDelta::Lines { x: -1.0, y: 0.0 },
                        },
                        SurfaceScrollMomentum::Direct,
                        SurfaceScrollModifiers::default(),
                    )),
                    SurfacePointerPosition::Known(receiver.center()),
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::delivery(&receiver),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(12),
                    SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                        SurfaceScrollDeviceId::new(4),
                        SurfaceScrollPhase::Begin {
                            sequence: smooth,
                            delta: Some(SurfaceScrollDelta::Lines { x: -0.5, y: 0.0 }),
                        },
                        SurfaceScrollMomentum::Direct,
                        SurfaceScrollModifiers::default(),
                    )),
                    SurfacePointerPosition::Known(receiver.center()),
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::delivery(&receiver),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(12),
                    SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                        SurfaceScrollDeviceId::new(4),
                        SurfaceScrollPhase::Update {
                            sequence: smooth,
                            delta: SurfaceScrollDelta::Lines { x: -0.5, y: 0.0 },
                        },
                        SurfaceScrollMomentum::Direct,
                        SurfaceScrollModifiers::default(),
                    )),
                    SurfacePointerPosition::Unknown,
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::delivery(&receiver),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(12),
                    SurfacePointerEvent::Scrolled(SurfaceScrollEvent::new(
                        SurfaceScrollDeviceId::new(4),
                        SurfaceScrollPhase::End {
                            sequence: smooth,
                            delta: None,
                        },
                        SurfaceScrollMomentum::Direct,
                        SurfaceScrollModifiers::default(),
                    )),
                    SurfacePointerPosition::Unknown,
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::delivery(&receiver),
                ),
            ])
            .expect("the exact scroll receiver consumes discrete and phaseful samples");
    });
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the changed scroll state recompiles from the same measurements");
    });
    host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the scrolled surface remains paintable")
            .expect("the scrolled surface has one paint plan");
        let bar = plan.tab_bars().next().expect("the tabs retain one bar");
        assert!(
            bar.scroll_offset() > 0.0,
            "the offset is owned and changed by dockspace; offset={}, maximum={}",
            bar.scroll_offset(),
            bar.maximum_scroll_offset(),
        );
    });
}

#[test]
fn ogc_03_release_on_first_target_hit_is_inert_without_a_painted_preview() {
    let mut fixture = InteractionFixture::new();
    fixture.begin_drag_without_target();
    let target = fixture.current_target();
    let before = fixture.host.version();

    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer(SurfacePointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                SurfacePointerPosition::Known(target.center()),
                SurfacePointerCapture::None,
                SurfacePointerReceiverFacts::hover(&target),
            ))
            .expect("the first target hit is reported exactly on release");
    });

    assert_eq!(fixture.host.version(), before);
}

#[test]
fn ogc_03_cached_or_stale_receiver_cannot_authorize_release() {
    let mut cached = InteractionFixture::new();
    cached.begin_drag_without_target();
    cached.preview_target();
    cached.paint_preview();
    let before_cached = cached.host.version();
    let point = cached.current_target().center();
    cached.host.run(|frame| {
        frame
            .submit_surface_pointer(SurfacePointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                SurfacePointerPosition::Known(point),
                SurfacePointerCapture::None,
                SurfacePointerReceiverFacts::unknown(),
            ))
            .expect("absence of a current hover fact is represented as unknown");
    });
    assert_eq!(cached.host.version(), before_cached);

    let mut stale = InteractionFixture::new();
    stale.begin_drag_without_target();
    stale.preview_target();
    stale.paint_preview();
    let old_target = stale.current_target();
    stale.paint_preview();
    let before_stale = stale.host.version();
    stale.host.run(|frame| {
        frame
            .submit_surface_pointer(SurfacePointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                SurfacePointerPosition::Known(old_target.center()),
                SurfacePointerCapture::None,
                SurfacePointerReceiverFacts::hover(&old_target),
            ))
            .expect("the facade converts a stale concrete receiver into fail-closed evidence");
    });
    assert_eq!(stale.host.version(), before_stale);
}

#[test]
fn ogc_03_current_painted_preview_commits_exactly_once() {
    let mut fixture = InteractionFixture::new();
    fixture.begin_drag_without_target();
    fixture.preview_target();
    fixture.paint_preview();
    let target = fixture.current_target();

    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer(SurfacePointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                SurfacePointerPosition::Known(target.center()),
                SurfacePointerCapture::None,
                SurfacePointerReceiverFacts::hover(&target),
            ))
            .expect("the current receiver reports release over the painted preview");
    });

    let target_tabs = tabs_containing(fixture.host.view(), C)
        .expect("the delivered target tabs remain product-visible");
    assert_eq!(target_tabs.items(), &[C, A]);
}

#[test]
fn ogc_04_stale_window_facts_revoke_receiver_authority_and_require_repaint() {
    let mut fixture = InteractionFixture::new();
    let retirement = fixture
        .host
        .session
        .disable_surface_pointer()
        .expect("the surface-local provider retires before desktop enrollment")
        .expect("the fixture owns one surface-local provider");
    assert_eq!(retirement.surface(), SURFACE);
    let binding = fixture.attach_native_surface();
    let _presented_target = fixture.current_target();
    let before = fixture.host.version();

    let invalidated = fixture
        .host
        .report_native_snapshot([(binding, NativeWindowFacts::live())]);
    assert_eq!(invalidated.repaint_surfaces(), &[SURFACE]);
    assert!(
        fixture
            .host
            .session
            .bind_presented_receiver(&fixture.target)
            .is_none(),
        "the last receiver must lose authority with its native route facts"
    );

    fixture.host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the invalidated surface remains a valid frame member");
        assert!(
            plan.is_none() || plan.is_some_and(|plan| plan.drag_preview().is_none()),
            "the replacement paint may never carry stale routed interaction state"
        );
    });
    assert!(
        fixture
            .host
            .session
            .bind_presented_receiver(&fixture.target)
            .is_none(),
        "stale geometry cannot regain receiver authority without a new presentation"
    );
    assert_eq!(fixture.host.version(), before);
}

#[test]
fn ogc_04_late_a1_close_cannot_mutate_same_token_a2_binding() {
    let mut host = DeterministicHost::new(tabs_layout([A, B]));
    host.session
        .enable_observed_native_roots()
        .expect("the deterministic host enrolls one native platform provider");

    let registered_a1 = host.register_native_root(SURFACE, WINDOW);
    let a1 = match registered_a1.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected A1 registration, got {outcomes:?}"),
    };
    host.report_native_snapshot([(a1, NativeWindowFacts::destroyed())]);

    let registered_a2 = host.register_native_root(SURFACE, WINDOW);
    let a2 = match registered_a2.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected A2 registration, got {outcomes:?}"),
    };
    assert_ne!(a1, a2, "the core must mint a fresh binding incarnation");
    assert_eq!(a1.window_token(), a2.window_token());

    host.publish_native_close(a2, NativeCloseState::Clear);
    let physical =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the A2 physical bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the A2 scale is valid");
    host.report_native_snapshot([(a2, ready_window_facts(physical, scale))]);

    let before = host.version();
    let error = host
        .session
        .publish_native_close(a1, NativeCloseState::Requested, None)
        .expect_err("the delayed A1 close must be rejected before reduction");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::StaleSurface { surface: SURFACE })
    ));
    host.run(|_| {});

    assert_eq!(host.version(), before);
}

#[test]
fn ogc_04_incomplete_snapshot_is_rejected_before_recording() {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([A])),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([B])),
        ),
    ])
    .expect("the two-surface native layout is valid");
    let mut host = DeterministicHost::new(layout);
    host.session
        .enable_observed_native_roots()
        .expect("the deterministic host enrolls one native platform provider");

    let first_registration = host.register_native_root(SURFACE, WINDOW);
    let first = match first_registration.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected the first registration, got {outcomes:?}"),
    };
    let physical =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the physical bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the scale is valid");
    let second_registration = host.register_native_root(SECOND_SURFACE, SECOND_WINDOW);
    let second = match second_registration.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }]
            if binding.surface() == SECOND_SURFACE =>
        {
            *binding
        }
        outcomes => panic!("expected the second registration, got {outcomes:?}"),
    };

    let error = host
        .session
        .report_native_snapshot([(first, ready_window_facts(physical, scale))])
        .expect_err("an incomplete roster must be rejected before it is recorded");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::IncompleteRoster)
    ));
    host.run(|_| {});

    host.report_native_snapshot([
        (first, ready_window_facts(physical, scale)),
        (second, ready_window_facts(physical, scale)),
    ]);
}

#[test]
fn ogc_04_snapshot_waits_for_a_pending_roster_registration() {
    let mut host = DeterministicHost::new(tabs_layout([A]));
    host.session
        .enable_observed_native_roots()
        .expect("the deterministic host enrolls one native platform provider");
    host.session
        .register_native_root(SURFACE, WINDOW)
        .expect("the root registration joins the pending recorder prefix");

    let error = host
        .session
        .report_native_snapshot([])
        .expect_err("facts cannot be validated against the pre-registration roster");
    assert!(matches!(
        error.native_error(),
        Some(NativePlatformError::BindingRosterUnsettled)
    ));

    let registration = host.run(|_| {});
    let binding = match registration.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one native registration, got {outcomes:?}"),
    };
    let physical =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the physical bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the scale is valid");
    host.report_native_snapshot([(binding, ready_window_facts(physical, scale))]);
}
