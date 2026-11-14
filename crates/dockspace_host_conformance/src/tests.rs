use dockspace::close::CloseDecision;
use dockspace::geometry::{
    LogicalPoint, LogicalRect, LogicalSize, PhysicalPoint, PhysicalRect, ScaleFactor,
};
use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome, DockspaceAxis,
    DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceNodeView,
    DockspaceRootLayout, DockspaceRootView, DockspaceSurfaceLayout, DockspaceTabsView,
    DockspaceView, FloatingPresentationId, ItemId, RootId, SurfaceId, WorkspaceVersion,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    ContainedPaintRecord, ContainedResizeDirection, DockspaceCloseOutcome, DockspaceClosePlan,
    DockspaceCloseResolution, DockspaceDragDecoration, DockspaceDragPreview,
    DockspaceDropDirection, DockspaceDropEligibility, DockspaceHostFrame,
    DockspaceInteractionError, DockspaceManipulationDelivery, DockspaceManipulationStatus,
    DockspacePreviewVisual, DockspaceReceiverDescriptor, DockspaceReceiverRole,
    DockspaceRuntimeError, DockspaceSession, DockspaceVisualKind, DropAffordanceTargetPaintRecord,
    DropGuidePaintRecord, HostCloseRequestOrigin, HostFrameReport, HostInputOutcome,
    HostSurfaceCommitStatus, HostWindowToken, HostWorkAreaToken, NativeCloseState,
    NativeDesktopPointerLocation, NativeDesktopPosition, NativeEffectAcknowledgement,
    NativeEffectOperation, NativeEffectRequest, NativeGlobalFocus, NativeHostCapabilities,
    NativeHostCapability, NativeHostErrorKind, NativePointerButton, NativePointerEvent,
    NativePointerHover, NativePointerId, NativePointerInput, NativePointerOwner,
    NativePointerRoster, NativeReceiverAnswer, NativeReceiverPurpose, NativeReceiverQuery,
    NativeScrollDelta, NativeScrollDeviceId, NativeScrollEvent, NativeScrollModifiers,
    NativeScrollMomentum, NativeScrollPhase, NativeScrollReceiverChallenge, NativeScrollSequenceId,
    NativeSurfaceBinding, NativeWindowBinding, NativeWindowFacts, NativeWindowHit,
    NativeWindowInputState, NativeWindowPlacement, NativeWindowPresentationState,
    NativeWorkAreaFacts, NativeWorkAreaRoster, PanePaintRecord, PresentedDockReceiver,
    PresentedDockspaceSurface, SplitterJunctionPaintRecord, SplitterPaintRecord,
    SurfaceContainedResizeAdjustment, SurfaceMeasurementAnswer, SurfaceMeasurementRequest,
    SurfacePaintPlan, SurfacePaintProgramOperation, SurfacePaintProgramStep, SurfacePointerButton,
    SurfacePointerCancelReason, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
    SurfacePointerInput, SurfacePointerPosition, SurfacePointerReceiverFacts,
    SurfacePresentationArtifact, SurfacePresentationResult, SurfaceScrollDelta,
    SurfaceScrollDeviceId, SurfaceScrollEvent, SurfaceScrollModifiers, SurfaceScrollMomentum,
    SurfaceScrollPhase, SurfaceScrollSequenceId, SurfaceSplitterAdjustment,
    SurfaceUnavailableReason, TabBarPaintRecord, TabPaintRecord, TabStripMetrics,
    UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const SECOND_SURFACE: SurfaceId = SurfaceId::new(2);
const ROOT: RootId = RootId::new(1);
const SOURCE_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);
const X: ItemId = ItemId::new(4);
const Y: ItemId = ItemId::new(5);
const Z: ItemId = ItemId::new(6);
const WINDOW: HostWindowToken = HostWindowToken::new(41);
const SECOND_WINDOW: HostWindowToken = HostWindowToken::new(42);

trait ConfirmSurfaceProgram {
    fn confirm_surface_program(&mut self, surface: SurfaceId) -> Result<(), DockspaceRuntimeError>;
}

impl ConfirmSurfaceProgram for DockspaceHostFrame<'_> {
    fn confirm_surface_program(&mut self, surface: SurfaceId) -> Result<(), DockspaceRuntimeError> {
        let receipt = self
            .paint_plan(surface)
            .expect("conformance paint plan lookup succeeds")
            .expect("conformance surface candidate is paintable")
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .expect("conformance renderer executes the complete surface program");
        self.confirm_surface_painted(receipt)
    }
}

fn program_drag_preview<'frame>(
    plan: SurfacePaintPlan<'frame>,
) -> Option<DockspaceDragPreview<'frame>> {
    let mut preview = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::DragPreview(current) = step {
            assert!(preview.replace(current).is_none());
        }
        Ok::<(), ()>(())
    })
    .expect("the paint plan must expose one valid surface program");
    preview
}

fn program_drag_decoration<'frame>(
    plan: SurfacePaintPlan<'frame>,
) -> Option<DockspaceDragDecoration<'frame>> {
    let mut decoration = None;
    plan.visit_surface_program(|step| {
        if let SurfacePaintProgramStep::DragDecoration(current) = step {
            assert!(decoration.replace(current).is_none());
        }
        Ok::<(), ()>(())
    })
    .expect("the paint plan must expose one valid surface program");
    decoration
}

#[derive(Default)]
struct ProgramRecords<'frame> {
    panes: Vec<PanePaintRecord<'frame>>,
    tabs: Vec<TabPaintRecord<'frame>>,
    tab_bars: Vec<TabBarPaintRecord<'frame>>,
    splitters: Vec<SplitterPaintRecord<'frame>>,
    splitter_junctions: Vec<SplitterJunctionPaintRecord<'frame>>,
    contained: Vec<ContainedPaintRecord<'frame>>,
    guides: Vec<DropGuidePaintRecord<'frame>>,
    affordance_targets: Vec<DropAffordanceTargetPaintRecord<'frame>>,
}

fn program_records<'frame>(plan: SurfacePaintPlan<'frame>) -> ProgramRecords<'frame> {
    let mut records = ProgramRecords::default();
    plan.visit_surface_program(|step| {
        match step {
            SurfacePaintProgramStep::RootBegin { .. } => {}
            SurfacePaintProgramStep::Operation { operation, .. } => match operation {
                SurfacePaintProgramOperation::Pane(pane) => records.panes.push(pane),
                SurfacePaintProgramOperation::Tab(tab) => records.tabs.push(tab),
                SurfacePaintProgramOperation::TabBar(bar) => records.tab_bars.push(bar),
                SurfacePaintProgramOperation::Splitter(splitter) => {
                    records.splitters.push(splitter)
                }
                SurfacePaintProgramOperation::SplitterJunction(junction) => {
                    records.splitter_junctions.push(junction)
                }
                SurfacePaintProgramOperation::ContainedControls(contained) => {
                    records.contained.push(contained)
                }
                SurfacePaintProgramOperation::TabStripControl(_)
                | SurfacePaintProgramOperation::PresentationMenuAnchor(_) => {}
            },
            SurfacePaintProgramStep::DropGuide {
                structural,
                affordance,
            } => {
                records.guides.push(structural);
                if let Some(affordance) = affordance {
                    records.affordance_targets.extend(affordance.targets());
                }
            }
            SurfacePaintProgramStep::RootEnd(_)
            | SurfacePaintProgramStep::PopupBackdrop(_)
            | SurfacePaintProgramStep::PopupMenu { .. }
            | SurfacePaintProgramStep::DragPreview(_)
            | SurfacePaintProgramStep::DropTarget(_)
            | SurfacePaintProgramStep::ContainedTransformPreview(_)
            | SurfacePaintProgramStep::DragDecoration(_)
            | SurfacePaintProgramStep::InteractionFeedback
            | SurfacePaintProgramStep::SurfaceEnd { .. } => {}
        }
        Ok::<(), ()>(())
    })
    .expect("the paint plan must expose one valid surface program");
    records
}

fn assert_dock_delivered_once(report: &HostFrameReport) {
    assert!(report.workspace_changed());
    assert_ne!(report.after(), report.before());
    let [feedback] = report.manipulation_feedback() else {
        panic!("one ordinary release must publish exactly one terminal manipulation result");
    };
    assert_eq!(
        feedback.status(),
        DockspaceManipulationStatus::Delivered(DockspaceManipulationDelivery::Dock),
    );
}

fn full_managed_capabilities() -> NativeHostCapabilities {
    [
        NativeHostCapability::NativeWindowLifecycle,
        NativeHostCapability::AuthoritativeInventory,
        NativeHostCapability::HoveredWindow,
        NativeHostCapability::DesktopPointerPosition,
        NativeHostCapability::AuthoritativeButtonState,
        NativeHostCapability::GlobalWindowPlacement,
        NativeHostCapability::WorkArea,
        NativeHostCapability::PointerHitTestObservation,
        NativeHostCapability::PointerHitTestControl,
        NativeHostCapability::GlobalFocusObservation,
        NativeHostCapability::WindowActivationControl,
        NativeHostCapability::CloseCancellation,
    ]
    .into_iter()
    .fold(
        NativeHostCapabilities::none_supported(),
        |capabilities, capability| capabilities.with(capability),
    )
}

fn ready_window_facts(physical: PhysicalRect, scale: ScaleFactor) -> NativeWindowFacts {
    NativeWindowFacts::live()
        .with_content_bounds(physical)
        .with_outer_bounds(physical)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(NativeWindowPresentationState::Visible)
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

    fn run_native(
        &mut self,
        resolve: impl FnMut(dockspace::runtime::NativeReceiverQuery) -> NativeReceiverAnswer,
        mutate: impl FnOnce(&mut DockspaceHostFrame<'_>),
    ) -> HostFrameReport {
        let mut frame = self
            .session
            .begin_native_host_frame(resolve)
            .expect("the managed native host frame must begin");
        mutate(&mut frame);
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the driver explicitly settles every unpainted surface");
        frame
            .commit()
            .expect("the managed native frame must commit atomically")
    }

    fn observe_painted_outputs(
        &mut self,
        mut report: HostFrameReport,
    ) -> SurfacePresentationArtifact {
        let outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 1, "the fixture paints one logical surface");
        let artifact = outputs[0].presentation_artifact();
        for output in outputs {
            self.session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .expect("the host confirms the exact output it presented");
        }
        self.run(|_| {});
        artifact
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

    fn report_managed_native_snapshot(
        &mut self,
        observations: impl IntoIterator<Item = (NativeSurfaceBinding, NativeWindowFacts)>,
        work_areas: NativeWorkAreaRoster,
    ) -> HostFrameReport {
        self.session
            .report_managed_native_snapshot(observations, work_areas)
            .expect("the managed native snapshot joins the backend ingress order");
        self.run_native(|_| NativeReceiverAnswer::Unknown, |_| {})
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

/// Private complete-roster fixture for transactional multi-surface boundaries.
///
/// It deliberately stays test-local: each boundary runs every declared
/// logical surface, and dropping the affine host frame is the only abort path.
struct TransactionalMultiSurfaceFixture {
    host: DeterministicHost,
    metrics: UniformSurfaceMetrics,
    source_tab: Option<DockspaceReceiverDescriptor>,
}

impl TransactionalMultiSurfaceFixture {
    fn new() -> Self {
        let layout = DockspaceLayout::new([
            DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([A, B])),
            ),
            DockspaceSurfaceLayout::new(
                SECOND_SURFACE,
                DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([C])),
            ),
        ])
        .expect("the transactional multi-surface layout is valid");
        let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0)
            .expect("the transactional surface bounds are valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("the minimum is valid");
        let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0)
            .expect("the transactional measurements are valid");
        Self {
            host: DeterministicHost::new(layout),
            metrics,
            source_tab: None,
        }
    }

    fn measure_complete_roster(frame: &mut DockspaceHostFrame<'_>, metrics: UniformSurfaceMetrics) {
        for surface in [SURFACE, SECOND_SURFACE] {
            frame
                .measure_surface(surface, metrics)
                .expect("every logical surface contributes to the same host boundary");
        }
    }

    fn publish_complete_roster(&mut self) {
        let metrics = self.metrics;
        let mut source_tab = None;
        let mut report = self.host.run(|frame| {
            Self::measure_complete_roster(frame, metrics);
            for surface in [SURFACE, SECOND_SURFACE] {
                let plan = frame
                    .paint_plan(surface)
                    .expect("complete-roster paint lookup succeeds")
                    .expect("every measured surface is paintable");
                if surface == SURFACE {
                    source_tab = plan.tab_receiver(A);
                }
                frame
                    .confirm_surface_program(surface)
                    .expect("every logical surface records its exact output");
            }
        });
        self.source_tab = source_tab;
        let outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 2);
        assert_eq!(
            outputs
                .iter()
                .map(|output| output.surface())
                .collect::<Vec<_>>(),
            [SURFACE, SECOND_SURFACE],
            "the fixture settles outputs in declared production order",
        );
        for output in outputs {
            self.host
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .expect("the fixture reports each exact output terminal");
        }
        self.host.run(|_| {});
    }

    fn current_source_tab(&self) -> PresentedDockReceiver {
        self.host
            .session
            .bind_presented_receiver(
                self.source_tab
                    .as_ref()
                    .expect("the complete-roster paint captures the source tab"),
            )
            .expect("the source tab belongs to the current presented output")
    }

    fn commit_selection(&mut self, item: ItemId) -> HostFrameReport {
        let metrics = self.metrics;
        self.host.run(|frame| {
            assert_eq!(
                frame.surfaces(),
                [SURFACE, SECOND_SURFACE],
                "the transaction freezes the complete declared roster",
            );
            frame
                .select_item_current(item)
                .expect("the selection joins the complete-roster boundary");
            Self::measure_complete_roster(frame, metrics);
        })
    }

    fn abort_selection(&mut self, item: ItemId) {
        let version = self.host.version();
        let selected = tabs_containing(self.host.view(), B)
            .expect("the primary tabs exist before abort")
            .selected();
        {
            let mut frame = self
                .host
                .session
                .begin_host_frame()
                .expect("the abort candidate begins");
            assert_eq!(
                frame.surfaces(),
                [SURFACE, SECOND_SURFACE],
                "the abort candidate freezes the complete declared roster",
            );
            frame
                .select_item_current(item)
                .expect("the abort candidate accepts its staged selection");
            Self::measure_complete_roster(&mut frame, self.metrics);
            assert_eq!(
                tabs_containing(frame.view(), item)
                    .expect("the staged selection remains visible inside the candidate")
                    .selected(),
                Some(item),
            );
            assert_eq!(
                tabs_containing(frame.view(), C)
                    .expect("the second surface remains inside the abort candidate")
                    .selected(),
                Some(C),
            );
        }
        assert_eq!(self.host.version(), version);
        assert_eq!(
            tabs_containing(self.host.view(), B)
                .expect("the primary tabs survive abort")
                .selected(),
            selected,
        );
    }
}

fn tabs_layout(items: impl IntoIterator<Item = ItemId>) -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::tabs(items)),
    )])
    .expect("the tabs layout is valid")
}

fn shared_semantic_layout() -> DockspaceLayout {
    let split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([A, B]), DockspaceNode::tabs([C])],
    )
    .expect("the shared semantic split is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, split),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([X])),
        LogicalRect::new(360.0, 120.0, 220.0, 180.0)
            .expect("the shared contained rectangle is valid"),
    ))])
    .expect("the shared semantic layout is valid")
}

fn magnetic_semantic_layout() -> DockspaceLayout {
    let middle = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([B, C]),
            DockspaceNode::tabs([A]),
            DockspaceNode::tabs([Y]),
        ],
    )
    .expect("the magnetic semantic row is valid");
    let root = DockspaceNode::equal_split(
        DockspaceAxis::Vertical,
        [DockspaceNode::tabs([X]), middle, DockspaceNode::tabs([Z])],
    )
    .expect("the magnetic semantic column is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .expect("the magnetic semantic layout is valid")
}

fn tabs_containing(view: DockspaceView<'_>, item: ItemId) -> Option<DockspaceTabsView<'_>> {
    view.item(item).map(|item| item.tabs())
}

fn node_contains_item(node: DockspaceNodeView<'_>, item: ItemId) -> bool {
    if let Some(tabs) = node.tabs() {
        return tabs.items().contains(&item);
    }
    node.split().is_some_and(|split| {
        split
            .children()
            .any(|child| node_contains_item(child, item))
    })
}

fn item_split_relation(
    node: DockspaceNodeView<'_>,
    source: ItemId,
    target: ItemId,
) -> Option<(DockspaceAxis, bool)> {
    let split = node.split()?;
    let children = split.children().collect::<Vec<_>>();
    let source_index = children
        .iter()
        .position(|child| node_contains_item(*child, source))?;
    let target_index = children
        .iter()
        .position(|child| node_contains_item(*child, target))?;
    if source_index != target_index {
        return Some((split.axis(), source_index < target_index));
    }
    item_split_relation(children[source_index], source, target)
}

fn expected_magnetic_split(
    basin: crate::scenario::MagneticDockBasin,
) -> Option<(DockspaceAxis, bool)> {
    use crate::scenario::MagneticDockBasin;

    match basin {
        MagneticDockBasin::Center => None,
        MagneticDockBasin::North => Some((DockspaceAxis::Vertical, true)),
        MagneticDockBasin::South => Some((DockspaceAxis::Vertical, false)),
        MagneticDockBasin::East => Some((DockspaceAxis::Horizontal, false)),
        MagneticDockBasin::West => Some((DockspaceAxis::Horizontal, true)),
    }
}

fn item_snapshot(
    view: DockspaceView<'_>,
    item: ItemId,
) -> (
    SurfaceId,
    RootId,
    Option<FloatingPresentationId>,
    Vec<ItemId>,
    Option<ItemId>,
) {
    let item_view = view.item(item).expect("the fixture item remains open");
    let tabs = item_view.tabs();
    (
        item_view.surface(),
        item_view.root(),
        item_view.contained(),
        tabs.items().to_vec(),
        tabs.selected(),
    )
}

fn magnetic_basin_from_direction(
    direction: DockspaceDropDirection,
) -> crate::scenario::MagneticDockBasin {
    use crate::scenario::MagneticDockBasin;

    match direction {
        DockspaceDropDirection::Center => MagneticDockBasin::Center,
        DockspaceDropDirection::Left => MagneticDockBasin::West,
        DockspaceDropDirection::Right => MagneticDockBasin::East,
        DockspaceDropDirection::Top => MagneticDockBasin::North,
        DockspaceDropDirection::Bottom => MagneticDockBasin::South,
    }
}

fn magnetic_basin_from_preview_rect(
    preview: LogicalRect,
    target: LogicalRect,
) -> crate::scenario::MagneticDockBasin {
    use crate::scenario::MagneticDockBasin;

    let width_ratio = preview.width() / target.width();
    let height_ratio = preview.height() / target.height();
    if width_ratio >= 0.8 && height_ratio >= 0.8 {
        return MagneticDockBasin::Center;
    }
    if width_ratio < height_ratio {
        if preview.x() + preview.width() * 0.5 < target.x() + target.width() * 0.5 {
            MagneticDockBasin::West
        } else {
            MagneticDockBasin::East
        }
    } else if preview.y() + preview.height() * 0.5 < target.y() + target.height() * 0.5 {
        MagneticDockBasin::North
    } else {
        MagneticDockBasin::South
    }
}

struct MagneticJourneyFixture {
    host: DeterministicHost,
    metrics: UniformSurfaceMetrics,
    source: DockspaceReceiverDescriptor,
    target_bounds: LogicalRect,
}

impl MagneticJourneyFixture {
    fn new() -> Self {
        let mut host = DeterministicHost::new(magnetic_semantic_layout());
        let bounds = LogicalRect::new(0.0, 0.0, 800.0, 600.0).expect("the L03 bounds are valid");
        let minimum = LogicalSize::new(0.0, 0.0).expect("the L03 minimum is valid");
        let metrics =
            UniformSurfaceMetrics::new(bounds, minimum, 72.0).expect("the L03 metrics are valid");
        let mut source = None;
        let mut target_bounds = None;
        let paint = host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the L03 fixture measures and compiles in its paint update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the initial L03 plan lookup succeeds")
                .expect("the initial L03 plan is ready");
            source = plan.tab_receiver(B);
            plan.visit_surface_program(|step| {
                if let SurfacePaintProgramStep::Operation {
                    operation: SurfacePaintProgramOperation::Pane(pane),
                    ..
                } = step
                    && pane.selected() == Some(A)
                {
                    target_bounds = Some(pane.content_bounds());
                }
                Ok::<(), ()>(())
            })
            .expect("the initial L03 surface program is valid");
            frame
                .confirm_surface_program(SURFACE)
                .expect("the initial L03 output was painted");
        });
        host.observe_painted_outputs(paint);
        host.session
            .enable_surface_pointer(SURFACE)
            .expect("the L03 surface admits one local pointer provider");

        Self {
            host,
            metrics,
            source: source.expect("the L03 source tab exposes one receiver"),
            target_bounds: target_bounds.expect("the L03 target pane exposes content bounds"),
        }
    }

    fn source(&self) -> PresentedDockReceiver {
        self.host
            .session
            .bind_presented_receiver(&self.source)
            .expect("the L03 source receiver belongs to the presented output")
    }

    fn surface(&self) -> PresentedDockspaceSurface {
        self.host
            .session
            .presented_surface(SURFACE)
            .expect("the L03 surface remains presented")
    }

    fn point(&self, sample: crate::scenario::NormalizedInsetSample) -> LogicalPoint {
        LogicalPoint::new(
            self.target_bounds.x() + self.target_bounds.width() * sample.x(),
            self.target_bounds.y() + self.target_bounds.height() * sample.y(),
        )
        .expect("the normalized L03 point is finite")
    }

    fn tangent_point(
        &self,
        point: LogicalPoint,
        basin: crate::scenario::MagneticDockBasin,
        delta: f64,
    ) -> LogicalPoint {
        use crate::scenario::MagneticDockBasin;

        let (x, y) = match basin {
            MagneticDockBasin::Center | MagneticDockBasin::North | MagneticDockBasin::South => {
                (point.x() + delta, point.y())
            }
            MagneticDockBasin::East | MagneticDockBasin::West => (point.x(), point.y() + delta),
        };
        let point = LogicalPoint::new(x, y).expect("the L03 tangent point is finite");
        assert!(self.target_bounds.contains(point));
        point
    }

    fn submit_press(&mut self) {
        let source = self.source();
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
                .expect("the shared L03 source receives one press");
        });
    }

    fn submit_threshold_move(&mut self) {
        let source = self.source();
        let surface = self.surface();
        let press = source.center();
        let moved = LogicalPoint::new(press.x() + 24.0, press.y() + 24.0)
            .expect("the shared L03 threshold point is finite");
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(SurfacePointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    SurfacePointerPosition::Known(moved),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                ))
                .expect("the shared L03 threshold move starts one drag");
        });
    }

    fn submit_target_move(&mut self, point: LogicalPoint) {
        let surface = self.surface();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(SurfacePointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::Moved,
                    SurfacePointerPosition::Known(point),
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                ))
                .expect("the ordinary L03 pane point reports a known-empty hover result");
        });
    }

    fn submit_release(&mut self, point: LogicalPoint) -> HostFrameReport {
        let surface = self.surface();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer(SurfacePointerInput::new(
                    SurfacePointerId::new(1),
                    SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                    SurfacePointerPosition::Known(point),
                    SurfacePointerCapture::None,
                    SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                ))
                .expect("the shared L03 release uses the current pane observation");
        })
    }

    fn submit_burst(&mut self, point: LogicalPoint) -> HostFrameReport {
        let source = self.source();
        let surface = self.surface();
        let press = source.center();
        let moved = LogicalPoint::new(press.x() + 24.0, press.y() + 24.0)
            .expect("the shared L03 threshold point is finite");
        self.host.run(|frame| {
            frame
                .submit_surface_pointer_batch([
                    SurfacePointerInput::new(
                        SurfacePointerId::new(1),
                        SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                        SurfacePointerPosition::Known(press),
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::delivery(&source),
                    ),
                    SurfacePointerInput::new(
                        SurfacePointerId::new(1),
                        SurfacePointerEvent::Moved,
                        SurfacePointerPosition::Known(moved),
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                    ),
                    SurfacePointerInput::new(
                        SurfacePointerId::new(1),
                        SurfacePointerEvent::Moved,
                        SurfacePointerPosition::Known(point),
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                    ),
                    SurfacePointerInput::new(
                        SurfacePointerId::new(1),
                        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                        SurfacePointerPosition::Known(point),
                        SurfacePointerCapture::None,
                        SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                    ),
                ])
                .expect("the shared L03 burst preserves press, motion, and release order");
        })
    }

    fn submit_final_move_and_release(&mut self, point: LogicalPoint) -> HostFrameReport {
        let surface = self.surface();
        self.host.run(|frame| {
            frame
                .submit_surface_pointer_batch([
                    SurfacePointerInput::new(
                        SurfacePointerId::new(1),
                        SurfacePointerEvent::Moved,
                        SurfacePointerPosition::Known(point),
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                    ),
                    SurfacePointerInput::new(
                        SurfacePointerId::new(1),
                        SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                        SurfacePointerPosition::Known(point),
                        SurfacePointerCapture::None,
                        SurfacePointerReceiverFacts::empty_hover_lane(&surface),
                    ),
                ])
                .expect("the final L03 move and release share one public input batch");
        })
    }

    fn present_preview(&mut self, point: LogicalPoint) -> crate::scenario::MagneticDockBasin {
        let metrics = self.metrics;
        let mut observed = None;
        let preview_report = self.host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the active L03 drag measures and compiles in its preview update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the L03 preview plan lookup succeeds")
                .expect("the active L03 drag retains one paint plan");
            let mut point_intersects_guide = false;
            let mut active_guide = None;
            plan.visit_surface_program(|step| {
                let SurfacePaintProgramStep::DropGuide {
                    affordance: Some(cluster),
                    ..
                } = step
                else {
                    return Ok::<(), ()>(());
                };
                for target in cluster.targets() {
                    if plan
                        .receiver_for_drop_target(target)
                        .is_some_and(|receiver| receiver.bounds().contains(point))
                    {
                        point_intersects_guide = true;
                    }
                    if target.is_active() {
                        active_guide = Some((target.direction(), target.eligibility()));
                    }
                }
                Ok::<(), ()>(())
            })
            .expect("the preview-bearing L03 surface program is valid");
            assert!(
                !point_intersects_guide,
                "the normalized L03 sample must remain outside every guide target: {point:?}",
            );
            let preview = program_drag_preview(plan)
                .expect("the ordinary pane point publishes a docking preview");
            let DockspacePreviewVisual::Dock {
                surface: SURFACE,
                rect,
            } = preview.visual()
            else {
                panic!("the ordinary L03 point must publish a local Dock preview");
            };
            let basin = magnetic_basin_from_preview_rect(rect, self.target_bounds);
            if let Some((direction, eligibility)) = active_guide {
                assert_eq!(eligibility, DockspaceDropEligibility::Eligible);
                assert_eq!(magnetic_basin_from_direction(direction), basin);
            }
            observed = Some(basin);
            frame
                .confirm_surface_program(SURFACE)
                .expect("the preview-bearing L03 output was painted");
        });
        self.host.observe_painted_outputs(preview_report);
        observed.expect("the L03 preview reports one active direction")
    }

    fn assert_committed_and_quiescent(&mut self, basin: crate::scenario::MagneticDockBasin) {
        use crate::scenario::MagneticDockBasin;

        let moved = self
            .host
            .view()
            .item(B)
            .expect("the L03 source item remains open after docking");
        assert_eq!(moved.surface(), SURFACE);
        assert_eq!(moved.root(), ROOT);
        assert_eq!(moved.contained(), None);
        assert!(moved.is_selected());
        let content = self
            .host
            .view()
            .root(ROOT)
            .and_then(DockspaceRootView::content)
            .expect("the committed L03 main root retains content");
        let split_relation = item_split_relation(content, B, A);
        assert_eq!(split_relation, expected_magnetic_split(basin));
        match basin {
            MagneticDockBasin::Center => {
                assert_eq!(moved.tabs().items(), &[A, B]);
            }
            MagneticDockBasin::North
            | MagneticDockBasin::South
            | MagneticDockBasin::East
            | MagneticDockBasin::West => {
                assert_eq!(moved.tabs().items(), &[B]);
                assert_eq!(tabs_containing(self.host.view(), A).unwrap().items(), &[A]);
            }
        }
        assert_eq!(tabs_containing(self.host.view(), C).unwrap().items(), &[C]);

        let settled_version = self.host.version();
        let metrics = self.metrics;
        let paint = self.host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the committed L03 layout measures and compiles in its paint update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the settled L03 plan lookup succeeds")
                .expect("the settled L03 plan is ready");
            assert!(program_drag_preview(plan).is_none());
            assert!(program_drag_decoration(plan).is_none());
            assert!(program_records(plan).affordance_targets.is_empty());
            frame
                .confirm_surface_program(SURFACE)
                .expect("the settled L03 output was painted");
        });
        self.host.observe_painted_outputs(paint);
        assert_eq!(self.host.version(), settled_version);
    }

    fn execute(
        mut self,
        case: crate::scenario::MagneticDockCase,
    ) -> [crate::scenario::SemanticObservation; 2] {
        use crate::scenario::{CadenceProfile, ItemAlias};

        assert_eq!(case.source(), ItemAlias::Secondary);
        assert_eq!(case.target(), ItemAlias::Primary);
        let target = self.point(case.sample());
        let (preview_basin, mut released) = match case.cadence() {
            CadenceProfile::Burst => (None, self.submit_burst(target)),
            CadenceProfile::Stepwise => {
                self.submit_press();
                self.submit_threshold_move();
                self.submit_target_move(target);
                let observed = self.present_preview(target);
                (Some(observed), self.submit_release(target))
            }
            CadenceProfile::Hesitant => {
                self.submit_press();
                self.submit_threshold_move();
                self.submit_target_move(target);
                self.submit_target_move(self.tangent_point(target, case.basin(), 3.0));
                self.submit_target_move(self.tangent_point(target, case.basin(), -3.0));
                self.submit_target_move(target);
                let observed = self.present_preview(target);
                (Some(observed), self.submit_release(target))
            }
            CadenceProfile::ReleaseWithFinalMove => {
                self.submit_press();
                self.submit_threshold_move();
                let acquired = self.tangent_point(target, case.basin(), -4.0);
                self.submit_target_move(acquired);
                let observed = self.present_preview(acquired);
                (Some(observed), self.submit_final_move_and_release(target))
            }
            cadence => panic!("unsupported L03 cadence: {cadence:?}"),
        };
        if let Some(observed) = preview_basin {
            assert_eq!(observed, case.basin());
        }
        assert_dock_delivered_once(&released);
        assert!(released.take_painted_outputs().is_empty());
        assert!(!self.host.session.has_active_gesture());
        assert!(!self.host.session.has_pending_presentation());
        self.assert_committed_and_quiescent(case.basin());
        case.expected_observations()
    }
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

#[test]
fn shared_local_interaction_scenario_maps_to_core_host_semantics() {
    use crate::scenario::{
        ItemAlias, LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED, ScenarioAdmission, ScenarioAxis,
        ScenarioCapability, ScenarioEdge, ScenarioId, SemanticAction, SemanticSelector,
    };

    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.id(),
        ScenarioId::LocalTabCloseSplitterAndContained,
    );
    let supported = [
        ScenarioCapability::LocalTabSelection,
        ScenarioCapability::LocalItemClose,
        ScenarioCapability::LocalSplitterAdjustment,
        ScenarioCapability::LocalContainedResize,
    ];
    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.admission(&supported),
        ScenarioAdmission::Ready,
        "the core-host executor admits the shared scenario only from declared capabilities",
    );
    let mut host = DeterministicHost::new(shared_semantic_layout());
    let bounds = LogicalRect::new(0.0, 0.0, 800.0, 600.0)
        .expect("the shared semantic surface bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the shared minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0).expect("the metrics are valid");
    host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the shared semantic fixture measures its complete surface");
    });
    let mut observations = Vec::new();

    for step in LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.steps() {
        let item = match step.selector() {
            SemanticSelector::Tab(ItemAlias::Primary)
            | SemanticSelector::TabClose(ItemAlias::Primary) => A,
            SemanticSelector::Tab(ItemAlias::Secondary)
            | SemanticSelector::TabClose(ItemAlias::Secondary) => B,
            SemanticSelector::Tab(ItemAlias::Contained)
            | SemanticSelector::TabClose(ItemAlias::Contained) => X,
            SemanticSelector::Splitter(_) | SemanticSelector::ContainedResize { .. } => X,
        };
        match (step.selector(), step.action()) {
            (SemanticSelector::Tab(alias), SemanticAction::Activate) => {
                let report = host.run(|frame| {
                    frame
                        .select_item_current(item)
                        .expect("the core-host executor maps semantic tab activation");
                    frame
                        .measure_surface(SURFACE, metrics)
                        .expect("selection republishes the complete surface measurement");
                });
                assert!(matches!(
                    report.inputs(),
                    [HostInputOutcome::ProductActionApplied(
                        DockspaceActionOutcome::Selected {
                            item: selected,
                            changed: true,
                        }
                    )] if *selected == item
                ));
                assert_eq!(alias, ItemAlias::Secondary);
                observations.push(step.expected());
            }
            (SemanticSelector::TabClose(alias), SemanticAction::Activate) => {
                let report = host.run(|frame| {
                    frame
                        .request_close_item_current(item)
                        .expect("the core-host executor maps semantic close activation");
                    frame
                        .measure_surface(SURFACE, metrics)
                        .expect("the close request retains a current surface measurement");
                });
                let requested_item = match report.inputs() {
                    [
                        HostInputOutcome::CloseRequested {
                            plan,
                            reused: false,
                            origin: HostCloseRequestOrigin::Application,
                        },
                    ] => plan.items()[0].item(),
                    outcomes => panic!("expected one semantic close request, got {outcomes:?}"),
                };
                assert_eq!(requested_item, item);
                assert_eq!(alias, ItemAlias::Secondary);
                observations.push(step.expected());
            }
            (SemanticSelector::Splitter(ScenarioAxis::Horizontal), SemanticAction::Increment) => {
                let before = host
                    .view()
                    .root(ROOT)
                    .and_then(|root| root.content())
                    .and_then(|content| content.split())
                    .expect("the shared root remains split")
                    .weights()
                    .collect::<Vec<_>>();
                host.run(|frame| {
                    let plan = frame
                        .paint_plan(SURFACE)
                        .expect("the shared splitter plan lookup succeeds")
                        .expect("the shared splitter plan is ready");
                    let splitter = program_records(plan)
                        .splitters
                        .into_iter()
                        .find(|splitter| splitter.axis() == DockspaceAxis::Horizontal)
                        .expect("the shared scenario exposes one horizontal splitter");
                    let action = plan
                        .prepare_splitter_adjustment(
                            splitter.visual_id(),
                            SurfaceSplitterAdjustment::Increment,
                        )
                        .expect("the horizontal splitter accepts a semantic increment");
                    frame
                        .submit_surface_action(action)
                        .expect("the splitter adjustment joins the exact host frame");
                    frame
                        .measure_surface(SURFACE, metrics)
                        .expect("the adjusted surface is remeasured");
                });
                let after = host
                    .view()
                    .root(ROOT)
                    .and_then(|root| root.content())
                    .and_then(|content| content.split())
                    .expect("the adjusted root remains split")
                    .weights()
                    .collect::<Vec<_>>();
                assert!(after[0] > before[0]);
                observations.push(step.expected());
            }
            (
                SemanticSelector::ContainedResize {
                    item: ItemAlias::Contained,
                    edge: ScenarioEdge::East,
                },
                SemanticAction::Increment,
            ) => {
                let before = host
                    .view()
                    .contained(FLOATING)
                    .expect("the shared contained presentation exists")
                    .rect();
                host.run(|frame| {
                    let plan = frame
                        .paint_plan(SURFACE)
                        .expect("the contained plan lookup succeeds")
                        .expect("the contained plan is ready");
                    let contained = program_records(plan)
                        .contained
                        .into_iter()
                        .find(|contained| contained.floating() == FLOATING)
                        .expect("the shared contained presentation is painted");
                    let action = plan
                        .prepare_contained_resize_adjustment(
                            contained.floating(),
                            ContainedResizeDirection::East,
                            SurfaceContainedResizeAdjustment::Increment,
                        )
                        .expect("the contained east edge accepts a semantic increment");
                    frame
                        .submit_surface_action(action)
                        .expect("the contained adjustment joins the exact host frame");
                    frame
                        .measure_surface(SURFACE, metrics)
                        .expect("the resized surface is remeasured");
                });
                let after = host
                    .view()
                    .contained(FLOATING)
                    .expect("the resized contained presentation exists")
                    .rect();
                assert_eq!(after.min(), before.min());
                assert!(after.width() > before.width());
                assert_eq!(after.height(), before.height());
                observations.push(step.expected());
            }
            (selector, action) => {
                panic!("unsupported shared semantic step: {selector:?} {action:?}")
            }
        }
    }

    assert_eq!(
        observations,
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.expected_observations(),
    );
}

#[test]
fn private_multi_surface_fixture_commits_and_aborts_complete_rosters_atomically() {
    let mut fixture = TransactionalMultiSurfaceFixture::new();
    fixture.publish_complete_roster();
    assert!(fixture.host.session.presented_surface(SURFACE).is_some());
    assert!(
        fixture
            .host
            .session
            .presented_surface(SECOND_SURFACE)
            .is_some(),
    );

    let before = fixture.host.version();
    let committed = fixture.commit_selection(B);
    assert!(matches!(
        committed.inputs(),
        [HostInputOutcome::ProductActionApplied(
            DockspaceActionOutcome::Selected {
                item: B,
                changed: true,
            }
        )]
    ));
    assert_eq!(
        committed
            .surface_commits()
            .iter()
            .map(|commit| (commit.surface(), commit.status()))
            .collect::<Vec<_>>(),
        [
            (SURFACE, HostSurfaceCommitStatus::Ready),
            (SECOND_SURFACE, HostSurfaceCommitStatus::Ready),
        ],
        "the commit boundary must contain one exact contribution for every frozen surface",
    );
    assert_ne!(fixture.host.version(), before);
    assert_eq!(
        tabs_containing(fixture.host.view(), B)
            .expect("the committed primary tabs remain")
            .selected(),
        Some(B),
    );
    assert_eq!(
        tabs_containing(fixture.host.view(), C)
            .expect("the untouched target surface remains in the same transaction")
            .selected(),
        Some(C),
    );

    fixture.abort_selection(A);
}

#[test]
fn private_multi_surface_fixture_rejects_a_stale_target_without_partial_commit() {
    let mut fixture = TransactionalMultiSurfaceFixture::new();
    fixture.publish_complete_roster();
    let prepared = fixture
        .host
        .session
        .prepare_dock_item(A, DockPlacement::Center(DockAnchor::Item(C)));
    fixture.commit_selection(B);
    let before = fixture.host.version();
    let metrics = fixture.metrics;

    let report = fixture.host.run(|frame| {
        frame
            .submit_prepared_action(prepared)
            .expect("the stale action remains structurally valid");
        TransactionalMultiSurfaceFixture::measure_complete_roster(frame, metrics);
    });

    assert!(matches!(
        report.inputs(),
        [HostInputOutcome::StaleRejected { .. }]
    ));
    assert_eq!(fixture.host.version(), before);
    assert_eq!(
        fixture.host.view().item(A).map(|item| item.surface()),
        Some(SURFACE)
    );
    assert_eq!(
        fixture.host.view().item(C).map(|item| item.surface()),
        Some(SECOND_SURFACE),
    );
}

#[test]
fn private_multi_surface_fixture_keeps_the_newer_output_when_an_older_one_is_abandoned() {
    let mut fixture = TransactionalMultiSurfaceFixture::new();
    fixture.publish_complete_roster();
    let metrics = fixture.metrics;
    let mut first = fixture.host.run(|frame| {
        frame
            .measure_surface(SECOND_SURFACE, metrics)
            .expect("the first target output measures in its paint update");
        frame
            .confirm_surface_program(SECOND_SURFACE)
            .expect("the first target output is painted");
    });
    let first = first
        .take_painted_outputs()
        .pop()
        .expect("the first target output capability exists");
    let mut second = fixture.host.run(|frame| {
        frame
            .measure_surface(SECOND_SURFACE, metrics)
            .expect("the successor target output measures in its paint update");
        frame
            .confirm_surface_program(SECOND_SURFACE)
            .expect("the successor target output is painted");
    });
    let second = second
        .take_painted_outputs()
        .pop()
        .expect("the successor target output capability exists");

    fixture
        .host
        .session
        .report_surface_presentation(second, SurfacePresentationResult::Presented)
        .expect("the successor target output terminal records first");
    drop(first);
    fixture.host.run(|_| {});

    assert!(
        fixture
            .host
            .session
            .presented_surface(SECOND_SURFACE)
            .is_some(),
        "abandoning a superseded output must not revoke its presented successor",
    );
    assert!(fixture.host.session.presented_surface(SURFACE).is_some());
}

#[test]
fn native_lifecycle_scenario_names_capability_and_terminal_observations() {
    use crate::scenario::{
        NATIVE_CREATE_REDOCK_RETIREMENT, ScenarioActionTerminality, ScenarioAdmission,
        ScenarioCapability, ScenarioId, ScenarioProductAction, SemanticObservation, SurfaceAlias,
    };

    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.id(),
        ScenarioId::NativeCreateRedockRetirement,
    );
    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.admission(&[]),
        ScenarioAdmission::Unsupported {
            missing: ScenarioCapability::NativeWindowLifecycle,
        },
    );
    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.expected_observations(),
        &[
            SemanticObservation::ActionTerminal {
                action: ScenarioProductAction::MoveToNewWindow,
                terminality: ScenarioActionTerminality::Requested,
            },
            SemanticObservation::PresentationApplied(SurfaceAlias::Child),
            SemanticObservation::FirstLive(SurfaceAlias::Child),
            SemanticObservation::ActionTerminal {
                action: ScenarioProductAction::DockBack,
                terminality: ScenarioActionTerminality::Requested,
            },
            SemanticObservation::PresentationApplied(SurfaceAlias::Source),
            SemanticObservation::Retired(SurfaceAlias::Child),
            SemanticObservation::Quiescent,
        ],
    );
}

#[test]
fn native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence() {
    use crate::scenario::{
        NATIVE_CREATE_REDOCK_RETIREMENT, ScenarioActionTerminality, ScenarioCapability, ScenarioId,
        ScenarioProductAction, SemanticObservation, SurfaceAlias,
    };

    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.id(),
        ScenarioId::NativeCreateRedockRetirement,
    );
    assert_eq!(
        NATIVE_CREATE_REDOCK_RETIREMENT.required_capabilities(),
        &[
            ScenarioCapability::NativeWindowLifecycle,
            ScenarioCapability::NativeFirstLive,
            ScenarioCapability::NativeRetirement,
            ScenarioCapability::NativeQuiescence,
        ],
    );

    let mut fixture = ManagedLifecycleScenarioFixture::new();
    let mut observations = Vec::new();
    let (child, create_acknowledgement) = fixture.request_child();
    observations.push(SemanticObservation::ActionTerminal {
        action: ScenarioProductAction::MoveToNewWindow,
        terminality: ScenarioActionTerminality::Requested,
    });

    let before_first_live = fixture.host.version();
    fixture.present_hidden_child(child, create_acknowledgement);
    fixture.publish_prepared_child(child);
    let mut prepared = fixture.commit();
    assert!(prepared.native_admissions().is_empty());
    assert!(prepared.take_native_effects().is_empty());
    assert_eq!(
        fixture.host.view().item(X).map(|item| item.surface()),
        Some(SURFACE),
        "renderer preparation retains source ownership before exact first-live",
    );
    assert_eq!(fixture.host.version(), before_first_live);
    assert!(fixture.host.view().surface(child.surface()).is_none());
    assert!(
        fixture
            .host
            .session
            .presented_surface(child.surface())
            .is_none()
    );

    fixture.present_first_live(child);
    assert_eq!(
        fixture.host.view().item(X).map(|item| item.surface()),
        Some(child.surface()),
        "the exact first-live output publishes ownership and admission together",
    );
    assert_ne!(fixture.host.version(), before_first_live);
    observations.push(SemanticObservation::PresentationApplied(
        SurfaceAlias::Child,
    ));
    observations.push(SemanticObservation::FirstLive(SurfaceAlias::Child));

    let release = fixture.request_redock(child);
    observations.push(SemanticObservation::ActionTerminal {
        action: ScenarioProductAction::DockBack,
        terminality: ScenarioActionTerminality::Requested,
    });
    observations.push(SemanticObservation::PresentationApplied(
        SurfaceAlias::Source,
    ));
    fixture.retire_child(child, release);
    observations.push(SemanticObservation::Retired(SurfaceAlias::Child));
    fixture.publish_quiescence(child);
    observations.push(SemanticObservation::Quiescent);

    assert_eq!(
        observations,
        NATIVE_CREATE_REDOCK_RETIREMENT.expected_observations(),
    );
}

#[test]
fn u1_journey_catalog_has_all_ids_and_cadence_profiles() {
    use crate::scenario::{CadenceProfile, JOURNEY_CATALOG, JourneyId};

    let ids = JOURNEY_CATALOG
        .iter()
        .map(|journey| journey.id())
        .collect::<Vec<_>>();
    assert_eq!(
        ids,
        vec![
            JourneyId::L01,
            JourneyId::L02,
            JourneyId::L03,
            JourneyId::L04,
            JourneyId::L05,
            JourneyId::L06,
            JourneyId::L07,
            JourneyId::L08,
            JourneyId::N01,
            JourneyId::P01,
        ],
        "the U1 catalog must expose exactly L01-L08, N01, and P01",
    );

    let cadences = JOURNEY_CATALOG
        .iter()
        .flat_map(|journey| journey.cadences())
        .collect::<Vec<_>>();
    for cadence in [
        CadenceProfile::Burst,
        CadenceProfile::Stepwise,
        CadenceProfile::Hesitant,
        CadenceProfile::ReleaseWithFinalMove,
        CadenceProfile::DiscardBeforeRelease,
        CadenceProfile::TargetDisappears,
        CadenceProfile::TouchLongPress,
    ] {
        assert!(
            cadences.contains(&&cadence),
            "cadence {cadence:?} must be assigned to at least one journey",
        );
    }
    assert!(
        JOURNEY_CATALOG
            .iter()
            .all(|journey| !journey.title().is_empty()
                && !journey.required_capabilities().is_empty())
    );
}

#[test]
fn shared_l03_catalog_covers_five_basins_three_insets_and_four_cadences() {
    use crate::scenario::{CadenceProfile, MAGNETIC_DOCKING_BASINS, MagneticDockBasin};

    let cases = MAGNETIC_DOCKING_BASINS.cases().collect::<Vec<_>>();
    assert_eq!(cases.len(), 5 * 3 * 4);
    for basin in [
        MagneticDockBasin::Center,
        MagneticDockBasin::North,
        MagneticDockBasin::South,
        MagneticDockBasin::East,
        MagneticDockBasin::West,
    ] {
        let basin_cases = cases
            .iter()
            .filter(|case| case.basin() == basin)
            .collect::<Vec<_>>();
        assert_eq!(basin_cases.len(), 3 * 4);
        for cadence in [
            CadenceProfile::Burst,
            CadenceProfile::Stepwise,
            CadenceProfile::Hesitant,
            CadenceProfile::ReleaseWithFinalMove,
        ] {
            assert_eq!(
                basin_cases
                    .iter()
                    .filter(|case| case.cadence() == cadence)
                    .count(),
                3,
            );
        }
    }
}

#[test]
fn shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics() {
    use crate::scenario::{
        JourneyId, MAGNETIC_DOCKING_BASINS, ScenarioAdmission, ScenarioCapability, ScenarioId,
    };

    assert_eq!(
        MAGNETIC_DOCKING_BASINS.id(),
        ScenarioId::MagneticDockingBasins,
    );
    assert_eq!(MAGNETIC_DOCKING_BASINS.journey(), JourneyId::L03);
    let supported = [
        ScenarioCapability::LocalDirectManipulation,
        ScenarioCapability::SemanticDockBasins,
        ScenarioCapability::PresentedCandidate,
    ];
    assert_eq!(
        MAGNETIC_DOCKING_BASINS.admission(&supported),
        ScenarioAdmission::Ready,
    );

    let mut observations = Vec::new();
    for case in MAGNETIC_DOCKING_BASINS.cases() {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            MagneticJourneyFixture::new().execute(case)
        }));
        match result {
            Ok(case_observations) => observations.extend(case_observations),
            Err(payload) => {
                let detail = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(String::as_str))
                    .unwrap_or("non-string panic payload");
                panic!("shared core-host L03 case failed: {case:?}: {detail}");
            }
        }
    }
    assert_eq!(
        observations,
        MAGNETIC_DOCKING_BASINS.expected_observations(),
        "the core-host executor must map every shared L03 case to normalized docking and quiescence observations",
    );
}

#[test]
fn u1_journey_admission_reports_missing_facts_without_guessing() {
    use crate::scenario::{JOURNEY_CATALOG, JourneyId, ScenarioAdmission, ScenarioCapability};

    let find = |id| {
        JOURNEY_CATALOG
            .iter()
            .copied()
            .find(|journey| journey.id() == id)
            .expect("the catalog contains every U1 journey")
    };
    assert_eq!(
        find(JourneyId::L03).admission(&[ScenarioCapability::LocalDirectManipulation]),
        ScenarioAdmission::Unsupported {
            missing: ScenarioCapability::SemanticDockBasins,
        },
    );
    assert_eq!(
        find(JourneyId::N01).admission(&[]),
        ScenarioAdmission::Unsupported {
            missing: ScenarioCapability::NativeWindowLifecycle,
        },
    );
    assert_eq!(
        find(JourneyId::P01).admission(&[ScenarioCapability::NativeWindowLifecycle]),
        ScenarioAdmission::Unsupported {
            missing: ScenarioCapability::NativePhysicalAuthority,
        },
    );
}

#[test]
fn u1_shared_executors_are_linked_to_local_and_native_journeys() {
    use crate::scenario::{
        JourneyId, LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED, NATIVE_CREATE_REDOCK_RETIREMENT,
    };

    assert_eq!(
        LOCAL_TAB_CLOSE_SPLITTER_AND_CONTAINED.journey(),
        JourneyId::L08,
    );
    assert_eq!(NATIVE_CREATE_REDOCK_RETIREMENT.journey(), JourneyId::N01,);
}

struct ManagedLifecycleScenarioFixture {
    host: DeterministicHost,
    root: NativeSurfaceBinding,
    child_placement: PhysicalRect,
    metrics: UniformSurfaceMetrics,
    work_areas: NativeWorkAreaRoster,
}

impl ManagedLifecycleScenarioFixture {
    fn new() -> Self {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([A])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([X])),
            LogicalRect::new(320.0, 80.0, 240.0, 200.0)
                .expect("the lifecycle contained rectangle is valid"),
        ))])
        .expect("the lifecycle layout is valid");
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut host = DeterministicHost {
            session: DockspaceSession::from_layout(layout, policy)
                .expect("the lifecycle session initializes"),
        };
        host.session
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
            .expect("the managed lifecycle provider enrolls");
        host.session
            .configure_managed_native_capabilities(full_managed_capabilities())
            .expect("the managed lifecycle capabilities configure");
        let registration = host.register_native_root(SURFACE, WINDOW);
        let root = match registration.inputs() {
            [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
            outcomes => panic!("expected one root registration, got {outcomes:?}"),
        };
        let scale = ScaleFactor::new(1.0).expect("the lifecycle scale is valid");
        let work_areas = NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
            HostWorkAreaToken::new(1),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the lifecycle work area is valid"),
            scale,
        )]);
        host.report_managed_native_snapshot(
            [(root, scenario_window_facts(100.0, 80.0, 640.0, 480.0))],
            work_areas.clone(),
        );
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 640.0, 480.0)
                .expect("the lifecycle logical bounds are valid"),
            LogicalSize::new(32.0, 24.0).expect("the lifecycle minimum is valid"),
            80.0,
        )
        .expect("the lifecycle metrics are valid");
        let child_placement = PhysicalRect::new(900.0, 120.0, 420.0, 320.0)
            .expect("the lifecycle child placement is valid");
        let mut fixture = Self {
            host,
            root,
            child_placement,
            metrics,
            work_areas,
        };
        fixture.paint_and_present_all();
        fixture
    }

    fn request_child(
        &mut self,
    ) -> (
        NativeSurfaceBinding,
        dockspace::runtime::NativeCreateEffectAcknowledgement,
    ) {
        let mut frame = self
            .host
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the lifecycle tear-off frame begins");
        frame
            .move_root_to_new_window_current(
                SOURCE_ROOT,
                NativeWindowPlacement::new(self.child_placement),
            )
            .expect("the lifecycle root tear-off stages");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the source presentation remains retained");
        let mut report = frame.commit().expect("the lifecycle tear-off commits");
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::NativeRootTearOffRequested {
                    root: SOURCE_ROOT,
                    source_surface: SURFACE,
                    ..
                }
            )]
        ));
        let effect = take_only_effect(&mut report);
        let child = match effect.operation() {
            NativeEffectOperation::CreateWindow {
                binding, placement, ..
            } if *placement == self.child_placement => *binding,
            operation => panic!("expected one create-window effect, got {operation:?}"),
        };
        let acknowledgement = match effect.accepted() {
            Some(NativeEffectAcknowledgement::Create(acknowledgement)) => acknowledgement,
            acknowledgement => panic!("create returned {acknowledgement:?}"),
        };
        (child, acknowledgement)
    }

    fn present_hidden_child(
        &mut self,
        child: NativeSurfaceBinding,
        acknowledgement: dockspace::runtime::NativeCreateEffectAcknowledgement,
    ) {
        self.host.report_managed_native_snapshot(
            [
                (self.root, scenario_window_facts(100.0, 80.0, 640.0, 480.0)),
                (
                    child,
                    scenario_child_facts(
                        self.child_placement,
                        NativeWindowPresentationState::Hidden,
                    )
                    .with_create_readiness(
                        dockspace::runtime::NativeCreateReadiness::Created,
                        Some(acknowledgement),
                    ),
                ),
            ],
            self.work_areas.clone(),
        );
        let mut frame = self
            .host
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the unprepared lifecycle frame begins");
        Self::measure_all(&mut frame, self.metrics);
        assert!(
            frame
                .paint_plan(child.surface())
                .expect("the unprepared child plan resolves")
                .is_none(),
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the unprepared child retains source presentation");
        let mut report = frame.commit().expect("the Created facts commit");
        assert!(report.native_admissions().is_empty());
        assert!(report.take_native_effects().is_empty());
        assert!(report.take_painted_outputs().is_empty());
    }

    fn publish_prepared_child(&mut self, child: NativeSurfaceBinding) {
        self.host.report_managed_native_snapshot(
            [
                (self.root, scenario_window_facts(100.0, 80.0, 640.0, 480.0)),
                (
                    child,
                    scenario_child_facts(
                        self.child_placement,
                        NativeWindowPresentationState::Visible,
                    ),
                ),
            ],
            self.work_areas.clone(),
        );
    }

    fn present_first_live(&mut self, child: NativeSurfaceBinding) {
        let mut frame = self
            .host
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the first-live frame begins");
        Self::measure_all(&mut frame, self.metrics);
        assert!(
            frame
                .paint_plan(child.surface())
                .expect("the child plan resolves")
                .is_some(),
        );
        frame
            .confirm_surface_program(child.surface())
            .expect("the exact child first-live output is painted");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the root retains its current presentation");
        let mut report = frame.commit().expect("the first-live output commits");
        self.present_outputs(&mut report);
        let mut admitted = self.commit();
        assert_eq!(admitted.native_admissions(), &[child]);
        let focus = take_only_effect(&mut admitted);
        assert!(matches!(
            focus.operation(),
            NativeEffectOperation::RequestFocus { binding } if *binding == child
        ));
        assert!(focus.accepted().is_none());
        self.host
            .session
            .report_native_global_focus(NativeGlobalFocus::Dock(child.window()))
            .expect("the admitted child reports the requested focus");
        self.commit();
    }

    fn request_redock(&mut self, child: NativeSurfaceBinding) -> NativeEffectRequest {
        self.paint_and_present_all();
        let mut frame = self
            .host
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the lifecycle redock frame begins");
        frame
            .dock_root_current(SOURCE_ROOT, DockPlacement::Center(DockAnchor::Item(A)))
            .expect("the child root redocks into its recovery surface");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the redock frame settles every surface");
        let report = frame.commit().expect("the redock request commits");
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::ProductPresentationActionRequested {
                outcome: DockspaceActionOutcome::RootDockRequested { .. },
                ..
            }]
        ));
        let mut settled = self.paint_and_present_all();
        assert!(settled.presentation_transitions().iter().any(|transition| {
            transition.root() == SOURCE_ROOT
                && transition.source_surface() == child.surface()
                && transition.target_surface() == SURFACE
                && transition.result()
                    == dockspace::runtime::DockspacePresentationTransitionResult::Applied
        }));
        assert_eq!(
            self.host.view().item(X).map(|item| item.surface()),
            Some(SURFACE),
        );
        let mut release = None;
        let mut requested_root_focus = false;
        for effect in settled.take_native_effects() {
            match effect.operation() {
                NativeEffectOperation::ReleaseChild { binding } if *binding == child => {
                    assert!(release.replace(effect).is_none());
                }
                NativeEffectOperation::RequestFocus { binding } if *binding == self.root => {
                    assert!(!requested_root_focus);
                    requested_root_focus = true;
                    assert!(effect.accepted().is_none());
                }
                operation => panic!("unexpected redock effect: {operation:?}"),
            }
        }
        assert!(requested_root_focus);
        self.host
            .session
            .report_native_global_focus(NativeGlobalFocus::Dock(self.root.window()))
            .expect("the redock host reports the requested focus");
        release.expect("the redocked child retains its exact release effect")
    }

    fn retire_child(&mut self, child: NativeSurfaceBinding, release: NativeEffectRequest) {
        let acknowledgement = match release.accepted() {
            Some(NativeEffectAcknowledgement::Close(acknowledgement)) => acknowledgement,
            acknowledgement => panic!("release returned {acknowledgement:?}"),
        };
        self.host.report_managed_native_snapshot(
            [
                (self.root, scenario_window_facts(100.0, 80.0, 640.0, 480.0)),
                (child, NativeWindowFacts::destroyed_after(acknowledgement)),
            ],
            self.work_areas.clone(),
        );
        assert!(!self.host.session.is_current_native_binding(child));
        assert!(self.host.session.recognizes_native_binding(child));
    }

    fn publish_quiescence(&mut self, child: NativeSurfaceBinding) {
        self.host
            .session
            .report_native_binding_quiescence(child)
            .expect("the retired child binding becomes externally quiescent");
        self.commit();
        self.commit();
        assert!(!self.host.session.recognizes_native_binding(child));
    }

    fn measure_all(frame: &mut DockspaceHostFrame<'_>, metrics: UniformSurfaceMetrics) {
        for surface in frame.surfaces() {
            frame
                .measure_surface(surface, metrics)
                .expect("the complete lifecycle roster measures in the paint update");
        }
    }

    fn paint_and_present_all(&mut self) -> HostFrameReport {
        let mut frame = self
            .host
            .session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .expect("the lifecycle paint frame begins");
        Self::measure_all(&mut frame, self.metrics);
        for surface in frame.surfaces() {
            assert!(
                frame
                    .paint_plan(surface)
                    .expect("the lifecycle paint plan resolves")
                    .is_some(),
            );
            frame
                .confirm_surface_program(surface)
                .expect("the lifecycle surface output is painted");
        }
        let mut report = frame.commit().expect("the lifecycle paint frame commits");
        self.present_outputs(&mut report);
        self.commit()
    }

    fn present_outputs(&mut self, report: &mut HostFrameReport) {
        let outputs = report.take_painted_outputs();
        assert!(!outputs.is_empty());
        for output in outputs {
            self.host
                .session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .expect("the exact lifecycle surface terminal records");
        }
    }

    fn commit(&mut self) -> HostFrameReport {
        self.host
            .run_native(|_| NativeReceiverAnswer::Unknown, |_| {})
    }
}

fn scenario_window_facts(x: f64, y: f64, width: f64, height: f64) -> NativeWindowFacts {
    ready_window_facts(
        PhysicalRect::new(x, y, width, height).expect("the scenario window bounds are valid"),
        ScaleFactor::new(1.0).expect("the scenario scale is valid"),
    )
}

fn scenario_child_facts(
    bounds: PhysicalRect,
    presentation: NativeWindowPresentationState,
) -> NativeWindowFacts {
    let scale = ScaleFactor::new(1.0).expect("the child scale is valid");
    NativeWindowFacts::live()
        .with_create_readiness(dockspace::runtime::NativeCreateReadiness::Prepared, None)
        .with_content_bounds(bounds)
        .with_outer_bounds(bounds)
        .with_native_scale_factor(scale)
        .with_presentation_scale_factor(scale)
        .with_input(NativeWindowInputState::ReceivesInput, None)
        .with_presentation(presentation)
        .with_close(NativeCloseState::Clear, None)
}

fn take_only_effect(report: &mut HostFrameReport) -> NativeEffectRequest {
    let effects = report.take_native_effects();
    let [effect] = effects
        .try_into()
        .unwrap_or_else(|effects: Vec<_>| panic!("expected one native effect, got {effects:?}"));
    effect
}

#[test]
fn private_multi_surface_fixture_unknown_owner_cannot_start_a_drag() {
    let mut fixture = TransactionalMultiSurfaceFixture::new();
    fixture.publish_complete_roster();
    fixture
        .host
        .session
        .enable_surface_pointer(SURFACE)
        .expect("the presented source admits one surface-local pointer provider");
    let source = fixture.current_source_tab();
    let surface = fixture
        .host
        .session
        .presented_surface(SURFACE)
        .expect("the source surface remains presented");
    let before = fixture.host.version();
    fixture.host.run(|frame| {
        frame
            .submit_surface_pointer_batch([
                SurfacePointerInput::new(
                    SurfacePointerId::new(42),
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                    SurfacePointerPosition::Known(source.center()),
                    SurfacePointerCapture::Unknown,
                    SurfacePointerReceiverFacts::delivery(&source),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(42),
                    SurfacePointerEvent::Moved,
                    SurfacePointerPosition::Known(source.center()),
                    SurfacePointerCapture::Unknown,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
                ),
                SurfacePointerInput::new(
                    SurfacePointerId::new(42),
                    SurfacePointerEvent::ButtonReleased(SurfacePointerButton::Primary),
                    SurfacePointerPosition::Known(source.center()),
                    SurfacePointerCapture::Unknown,
                    SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
                ),
            ])
            .expect("unknown capture is recorded as fail-closed owner evidence");
    });
    assert!(!fixture.host.session.has_active_gesture());
    let metrics = fixture.metrics;
    fixture.host.run(|frame| {
        TransactionalMultiSurfaceFixture::measure_complete_roster(frame, metrics);
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the unknown-owner plan resolves")
            .expect("the source surface remains paintable");
        assert!(program_drag_preview(plan).is_none());
    });
    assert_eq!(fixture.host.version(), before);
    assert_eq!(
        tabs_containing(fixture.host.view(), A)
            .expect("the source tabs remain")
            .items(),
        &[A, B],
    );
    assert_eq!(
        fixture.host.view().item(C).map(|item| item.surface()),
        Some(SECOND_SURFACE),
        "the complete target-surface roster must remain unchanged",
    );
}

#[test]
fn private_multi_surface_fixture_target_destruction_preserves_the_source_drag() {
    let source_split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([A]), DockspaceNode::tabs([B])],
    )
    .expect("the source split is valid");
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(SURFACE, DockspaceRootLayout::new(ROOT, source_split)),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(SOURCE_ROOT, DockspaceNode::tabs([C])),
        ),
    ])
    .expect("the target-destruction layout is valid");
    let mut host = DeterministicHost::new(layout);
    host.session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the deterministic managed-native provider enrolls");
    host.session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed-native capabilities configure");

    let source_binding = match host.register_native_root(SURFACE, WINDOW).inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected the source registration, got {outcomes:?}"),
    };
    let target_binding = match host
        .register_native_root(SECOND_SURFACE, SECOND_WINDOW)
        .inputs()
    {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected the target registration, got {outcomes:?}"),
    };
    let scale = ScaleFactor::new(1.0).expect("the native scale is valid");
    let source_bounds =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the source bounds are valid");
    let target_bounds =
        PhysicalRect::new(720.0, 0.0, 640.0, 360.0).expect("the target bounds are valid");
    let work_areas = NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
        HostWorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, 1_920.0, 1_080.0).expect("the work area is valid"),
        scale,
    )]);
    host.report_managed_native_snapshot(
        [
            (source_binding, ready_window_facts(source_bounds, scale)),
            (target_binding, ready_window_facts(target_bounds, scale)),
        ],
        work_areas.clone(),
    );

    let logical_bounds =
        LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the logical bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the minimum is valid");
    let metrics = UniformSurfaceMetrics::new(logical_bounds, minimum, 72.0)
        .expect("the target-destruction metrics are valid");
    let mut source_tab = None;
    let mut target_drop = None;
    let mut painted = host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            for surface in frame.surfaces() {
                frame
                    .measure_surface(surface, metrics)
                    .expect("the complete native roster measures in its paint update");
            }
            for surface in frame.surfaces() {
                let plan = frame
                    .paint_plan(surface)
                    .expect("the complete native paint lookup succeeds")
                    .expect("every native surface is paintable");
                if surface == SURFACE {
                    source_tab = plan.tab_receiver(A);
                } else if surface == SECOND_SURFACE {
                    target_drop = plan.center_drop_receiver_for_item(C);
                }
                frame
                    .confirm_surface_program(surface)
                    .expect("the complete native roster records its output");
            }
        },
    );
    let painted_outputs = painted.take_painted_outputs();
    assert_eq!(painted_outputs.len(), 2);
    let source_artifact = painted_outputs
        .iter()
        .find(|output| output.surface() == SURFACE)
        .expect("the source exposes its exact painted artifact")
        .presentation_artifact();
    let target_artifact = painted_outputs
        .iter()
        .find(|output| output.surface() == SECOND_SURFACE)
        .expect("the target exposes its exact painted artifact")
        .presentation_artifact();
    for output in painted_outputs {
        host.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the initial native output terminal records");
    }
    host.run_native(|_| NativeReceiverAnswer::Unknown, |_| {});

    let source_tab_descriptor = source_tab.expect("the source tab is painted");
    let target_drop_descriptor = target_drop.expect("the target drop is painted");
    let source_tab = host
        .session
        .bind_presented_receiver(&source_tab_descriptor)
        .expect("the source tab belongs to the current output");
    let target_drop = host
        .session
        .bind_presented_receiver(&target_drop_descriptor)
        .expect("the target drop belongs to the current output");
    let source_point = source_tab.center();
    let target_point = target_drop.center();
    let source_desktop = PhysicalPoint::new(
        source_bounds.x() + source_point.x(),
        source_bounds.y() + source_point.y(),
    )
    .expect("the source desktop point is valid");
    let target_desktop = PhysicalPoint::new(
        target_bounds.x() + target_point.x(),
        target_bounds.y() + target_point.y(),
    )
    .expect("the target desktop point is valid");
    let pointer = NativePointerId::new(77);
    host.session
        .record_native_pointer_batch([
            NativePointerInput::new(
                pointer,
                NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(source_desktop),
                    NativePointerHover::Dock(source_binding.window()),
                    None,
                ),
                NativePointerOwner::Native(source_binding.window()),
                NativePointerOwner::Native(source_binding.window()),
            ),
            NativePointerInput::new(
                pointer,
                NativePointerEvent::Moved,
                NativeDesktopPointerLocation::new(
                    NativeDesktopPosition::Exact(target_desktop),
                    NativePointerHover::Dock(target_binding.window()),
                    None,
                ),
                NativePointerOwner::Native(source_binding.window()),
                NativePointerOwner::Native(source_binding.window()),
            ),
        ])
        .expect("the exact cross-surface drag prefix records atomically");
    let mut previewed = false;
    let mut preview = host.run_native(
        |query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(query) => {
                    let artifact = if query.window() == source_binding.window() {
                        source_artifact
                    } else {
                        assert_eq!(query.window(), target_binding.window());
                        target_artifact
                    };
                    return NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(artifact));
                }
                NativeReceiverQuery::Surface(query) => query,
            };
            let descriptor = match query.surface() {
                SURFACE => &source_tab_descriptor,
                SECOND_SURFACE => &target_drop_descriptor,
                _ => return NativeReceiverAnswer::Unknown,
            };
            query
                .bind_receiver(descriptor)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        },
        |frame| {
            for surface in frame.surfaces() {
                frame
                    .measure_surface(surface, metrics)
                    .expect("the cross-surface drag measures in its preview update");
            }
            for surface in frame.surfaces() {
                if let Some(plan) = frame
                    .paint_plan(surface)
                    .expect("the exact cross-surface drag plan resolves")
                {
                    if surface == SECOND_SURFACE {
                        let target_preview = program_drag_preview(plan).expect(
                            "exact capture and delivery resolve the target preview while the External Root receives input",
                        );
                        assert!(matches!(
                            target_preview.visual(),
                            DockspacePreviewVisual::Dock {
                                surface: SECOND_SURFACE,
                                ..
                            }
                        ));
                        previewed = true;
                    }
                    frame
                        .confirm_surface_program(surface)
                        .expect("the exact cross-surface preview output is painted");
                }
            }
        },
    );
    assert!(
        previewed,
        "the target surface must immediately publish a resolved Dock preview"
    );
    assert!(
        preview.take_native_effects().is_empty(),
        "an External Root content drag must preserve native input without pass-through effects",
    );
    for output in preview.take_painted_outputs() {
        host.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the preview output terminal records");
    }
    host.run_native(|_| NativeReceiverAnswer::Unknown, |_| {});

    let mut destroyed = host.report_managed_native_snapshot(
        [
            (source_binding, ready_window_facts(source_bounds, scale)),
            (target_binding, NativeWindowFacts::destroyed()),
        ],
        work_areas,
    );
    assert!(
        destroyed.take_native_effects().is_empty(),
        "destroying only the target must not change the External Root input state",
    );
    assert!(
        !host.session.is_current_native_binding(target_binding),
        "the authoritative inventory must retire the closed target binding",
    );
    assert!(
        host.session
            .bind_presented_receiver(&target_drop_descriptor)
            .is_none(),
        "the closed target window cannot regain receiver authority",
    );
    assert!(
        host.session.has_active_gesture(),
        "destroying only the target must not cancel the source-owned gesture",
    );

    let mut fresh_target = None;
    let mut surviving = host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the surviving source measures in its paint update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the surviving source plan resolves")
                .expect("the surviving source remains paintable");
            assert!(
                program_drag_preview(plan).is_none(),
                "closing the target window must clear its preview",
            );
            let decoration = program_drag_decoration(plan)
                .expect("the source-owned drag must survive target-window close");
            assert_eq!(decoration.source_surface(), SURFACE);
            assert_eq!(decoration.item(), Some(A));
            fresh_target = plan.center_drop_receiver_for_item(B);
            frame
                .confirm_surface_program(SURFACE)
                .expect("the surviving source publishes a fresh target output");
        },
    );
    let surviving_outputs = surviving.take_painted_outputs();
    assert_eq!(surviving_outputs.len(), 1);
    assert_eq!(surviving_outputs[0].surface(), SURFACE);
    let surviving_artifact = surviving_outputs[0].presentation_artifact();
    for output in surviving_outputs {
        host.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the fresh surviving target output terminal records");
    }
    host.run_native(|_| NativeReceiverAnswer::Unknown, |_| {});
    let fresh_target = fresh_target.expect("the surviving source exposes a fresh target for B");
    let target = host
        .session
        .bind_presented_receiver(&fresh_target)
        .expect("the fresh target belongs to the surviving presented output");
    let target_point = target.center();
    let target_desktop = PhysicalPoint::new(
        source_bounds.x() + target_point.x(),
        source_bounds.y() + target_point.y(),
    )
    .expect("the surviving target desktop point is valid");
    host.session
        .record_native_pointer(NativePointerInput::new(
            pointer,
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            NativeDesktopPointerLocation::new(
                NativeDesktopPosition::Exact(target_desktop),
                NativePointerHover::Dock(source_binding.window()),
                None,
            ),
            NativePointerOwner::Native(source_binding.window()),
            NativePointerOwner::None,
        ))
        .expect("the surviving source gesture releases over the fresh target");
    let mut released = host.run_native(
        |query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(query) => {
                    assert_eq!(query.window(), source_binding.window());
                    return NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(
                        surviving_artifact,
                    ));
                }
                NativeReceiverQuery::Surface(query) => query,
            };
            query
                .bind_receiver(&fresh_target)
                .map_or(NativeReceiverAnswer::Unknown, NativeReceiverAnswer::Dock)
        },
        |frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the committed source measures in its release update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the committed source plan resolves")
                .expect("the committed source is paintable");
            assert!(program_drag_preview(plan).is_none());
            assert!(program_drag_decoration(plan).is_none());
            frame
                .confirm_surface_program(SURFACE)
                .expect("the committed source output is painted");
        },
    );
    assert_dock_delivered_once(&released);
    assert_eq!(released.after(), host.version());
    assert!(!host.session.has_active_gesture());
    assert!(!host.session.has_pending_presentation());
    assert_eq!(
        tabs_containing(host.view(), B)
            .expect("the release immediately publishes the source item in the fresh target")
            .items(),
        &[B, A],
    );
    let committed_version = host.version();
    let [focus] = released
        .take_native_effects()
        .try_into()
        .unwrap_or_else(|effects: Vec<_>| {
            panic!(
                "the root release must request focus without a pass-through restore: {effects:?}"
            )
        });
    assert!(matches!(
        focus.operation(),
        NativeEffectOperation::RequestFocus { binding } if *binding == source_binding
    ));
    assert!(focus.accepted().is_none());
    host.session
        .report_native_global_focus(NativeGlobalFocus::Dock(source_binding.window()))
        .expect("the surviving source reports the requested focus");
    let released_outputs = released.take_painted_outputs();
    assert_eq!(
        released_outputs.len(),
        1,
        "the release update must paint its exact committed source output",
    );
    for output in released_outputs {
        host.session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .expect("the committed source output terminal records");
    }
    let settled = host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the settled source layout measures in its inspection update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the settled source plan resolves")
                .expect("the settled source remains paintable");
            assert!(program_drag_preview(plan).is_none());
            assert!(program_drag_decoration(plan).is_none());
        },
    );
    assert_eq!(host.version(), committed_version);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
    assert!(
        !host.session.has_active_gesture(),
        "the fresh target release must reset the surviving source gesture",
    );
    assert!(
        !host.session.has_pending_presentation(),
        "the ordinary source release must leave no presentation obligation",
    );
    assert_eq!(
        tabs_containing(host.view(), B)
            .expect("the fresh target retains the released source item")
            .items(),
        &[B, A],
    );
}

fn pointer_close_host() -> (DeterministicHost, DockspaceReceiverDescriptor) {
    let mut host = DeterministicHost::new(tabs_layout([A, B]));
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the minimum is valid");
    let metrics =
        UniformSurfaceMetrics::new(bounds, minimum, 72.0).expect("the measurements are valid");
    let mut close = None;
    let paint = host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the close fixture measures and compiles in its paint update");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the pointer phase closes before paint")
            .expect("the measured surface is ready");
        let close_bounds = program_records(plan)
            .tabs
            .into_iter()
            .find(|tab| tab.item() == A)
            .and_then(|tab| tab.close_bounds())
            .expect("item A exposes a close control");
        let tab = program_records(plan)
            .tabs
            .into_iter()
            .find(|tab| tab.item() == A)
            .expect("item A remains in the ordered tab program");
        close = plan.receiver_for_tab_close(tab).filter(|receiver| {
            receiver.role() == DockspaceReceiverRole::TabClose && receiver.bounds() == close_bounds
        });
        frame
            .confirm_surface_program(SURFACE)
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
            SurfacePointerReceiverFacts::delivery(&receiver).with_empty_hover_lane(&surface),
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
            SurfacePointerReceiverFacts::delivery(&receiver).with_empty_hover_lane(&surface),
        ))
        .expect("one contact end supplies delivery and hover facts together");
}

fn request_pointer_close(
    host: &mut DeterministicHost,
    descriptor: &DockspaceReceiverDescriptor,
) -> DockspaceClosePlan {
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
) -> DockspaceClosePlan {
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
            resolution: DockspaceCloseResolution::Vetoed { request, item: A },
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
            resolution: DockspaceCloseResolution::Approved { request },
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
    host.run(|frame| {
        frame
            .measure_surface_with(SURFACE, |request| match request {
                SurfaceMeasurementRequest::DockBounds { .. }
                | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                    SurfaceMeasurementAnswer::Bounds(bounds)
                }
                SurfaceMeasurementRequest::PaneMinimum { visual, .. } => {
                    assert_eq!(visual.kind(), DockspaceVisualKind::Pane);
                    SurfaceMeasurementAnswer::PaneMinimum(minimum)
                }
                SurfaceMeasurementRequest::TabIntrinsic { visual, .. } => {
                    assert_eq!(visual.kind(), DockspaceVisualKind::Tab);
                    SurfaceMeasurementAnswer::TabIntrinsic(96.0)
                }
                SurfaceMeasurementRequest::TabStrip { visual, .. } => {
                    assert_eq!(visual.kind(), DockspaceVisualKind::TabBar);
                    SurfaceMeasurementAnswer::TabStrip(
                        TabStripMetrics::new(0.0, 0.0)
                            .expect("the fixture tab-strip metrics are valid"),
                    )
                }
            })
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

        let records = program_records(plan);
        let panes = records.panes;
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

        let tabs = records.tabs;
        assert_eq!(
            tabs.iter().map(|tab| tab.item()).collect::<Vec<_>>(),
            [A, B]
        );
        assert!(tabs.iter().all(|tab| tab.selected()));
        first_visuals.extend(tabs.iter().map(|tab| tab.visual_id()));

        let tab_bars = records.tab_bars;
        assert_eq!(tab_bars.len(), 2);
        assert!(tab_bars.iter().all(|bar| bar.members().len() == 1));
        first_visuals.extend(tab_bars.iter().map(|bar| bar.visual_id()));

        let splitters = records.splitters;
        assert_eq!(splitters.len(), 1);
        assert_eq!(splitters[0].axis(), DockspaceAxis::Horizontal);
        assert!(splitters[0].operable());
        assert!(splitters[0].hit_bounds().width() > 0.0);
        first_visuals.push(splitters[0].visual_id());

        assert_eq!(records.splitter_junctions.len(), 0);
        assert_eq!(records.contained.len(), 0);
        let guides = records.guides;
        assert!(!guides.is_empty());
        assert!(guides.iter().all(|guide| guide.targets().count() >= 4));
        first_visuals.extend(guides.iter().map(|guide| guide.visual_id()));
        assert!(panes.iter().any(|pane| {
            plan.receiver_for_pane(*pane)
                .is_some_and(|receiver| receiver.role() == DockspaceReceiverRole::PaneBody)
        }));
        assert!(tabs.iter().any(|tab| {
            plan.receiver_for_tab_body(*tab)
                .is_some_and(|receiver| receiver.role() == DockspaceReceiverRole::TabBody)
        }));
        assert!(splitters.iter().any(|splitter| {
            plan.receiver_for_splitter(*splitter)
                .is_some_and(|receiver| receiver.role() == DockspaceReceiverRole::Splitter)
        }));
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
            .confirm_surface_program(SURFACE)
            .expect("the host confirms the complete plan it painted");
    });
    host.observe_painted_outputs(first_output);

    let second_output = host.run(|frame| {
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the retained paint phase is available")
            .expect("the retained surface remains paintable");
        let records = program_records(plan);
        let mut current = records
            .panes
            .into_iter()
            .map(|record| record.visual_id())
            .chain(records.tabs.into_iter().map(|record| record.visual_id()))
            .chain(
                records
                    .tab_bars
                    .into_iter()
                    .map(|record| record.visual_id()),
            )
            .chain(
                records
                    .splitters
                    .into_iter()
                    .map(|record| record.visual_id()),
            )
            .chain(records.guides.into_iter().map(|record| record.visual_id()))
            .collect::<Vec<_>>();
        first_visuals.sort_unstable();
        current.sort_unstable();
        assert_eq!(current, first_visuals, "visual identity is output-stable");
        frame
            .confirm_surface_program(SURFACE)
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
            .confirm_surface_program(SURFACE)
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
            .confirm_surface_program(SURFACE)
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
            .confirm_surface_program(SURFACE)
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
            .confirm_surface_program(SURFACE)
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
        host.session.presented_surface(SURFACE).is_some(),
        "the queued dropped output preserves the earlier actual presentation"
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
            .confirm_surface_program(SURFACE)
            .expect("the first output is painted");
    });
    let first = first_report
        .take_painted_outputs()
        .pop()
        .expect("the first paint emits one capability");
    let mut second_report = host.run(|frame| {
        frame
            .confirm_surface_program(SURFACE)
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
        host.session.presented_surface(SURFACE).is_some(),
        "a dropped successor preserves the last actual presentation in the same prefix"
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
            .confirm_surface_program(SURFACE)
            .expect("the first output is painted");
    });
    let first = first_report
        .take_painted_outputs()
        .pop()
        .expect("the first paint emits one capability");
    let mut second_report = host.run(|frame| {
        frame
            .confirm_surface_program(SURFACE)
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
fn ogc_02_merge_and_close_select_the_visual_successor_atomically() {
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
            .request_close_item_current(X)
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
            resolution: DockspaceCloseResolution::Approved { request },
            application: Some(Ok(DockspaceCloseOutcome::ItemClosed { item: X, .. })),
            changed: true,
            ..
        }] if *request == plan.request()
    ));

    let tabs = tabs_containing(host.view(), B).expect("the target tabs remain");
    assert_eq!(tabs.items(), &[A, B, C]);
    assert_eq!(tabs.selected(), Some(C));
    assert!(tabs_containing(host.view(), X).is_none());
}

struct InteractionFixture {
    host: DeterministicHost,
    metrics: UniformSurfaceMetrics,
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
        let mut source = None;
        let mut target = None;
        let paint = host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the interaction fixture measures and compiles in its paint update");
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
            assert!(program_drag_preview(plan).is_none());
            frame
                .confirm_surface_program(SURFACE)
                .expect("the host records the exact plan it painted");
        });
        host.observe_painted_outputs(paint);
        host.session
            .enable_surface_pointer(SURFACE)
            .expect("the observed surface admits one local pointer provider");

        Self {
            host,
            metrics,
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
                    SurfacePointerReceiverFacts::empty_hover_lane(&surface),
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
        let metrics = self.metrics;
        let paint = self.host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the active drag measures and compiles in its preview update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the pointer-input phase must close before paint")
                .expect("the active drag retains one paint plan");
            assert!(
                program_drag_preview(plan).is_some(),
                "the renderer must actually see the preview it confirms"
            );
            let records = program_records(plan);
            let guides = records.guides;
            assert!(
                !guides.is_empty(),
                "an active docking preview exposes its complete guide cluster"
            );
            assert!(
                guides.iter().any(|guide| guide.targets().count() >= 5),
                "one guide cluster exposes center and four directional targets"
            );
            let targets = records.affordance_targets;
            assert!(
                targets.len() >= 5,
                "the dynamic affordance retains the complete guide cluster"
            );
            let active = targets
                .into_iter()
                .find(|target| target.is_active())
                .expect("the exact center receiver activates one guide target");
            assert_eq!(active.direction(), DockspaceDropDirection::Center);
            assert_eq!(active.eligibility(), DockspaceDropEligibility::Eligible);
            frame
                .confirm_surface_program(SURFACE)
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

        let metrics = self.metrics;
        let mut source = None;
        let mut target = None;
        let paint = self.host.run(|frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("native coordinate authority admits measurement in the paint update");
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the fresh native plan is inspectable")
                .expect("the fresh native plan is ready");
            source = plan.tab_receiver(A);
            target = plan.center_drop_receiver_for_item(C);
            frame
                .confirm_surface_program(SURFACE)
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
                    SurfacePointerReceiverFacts::empty_hover_lane(&surface),
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
            program_drag_preview(plan).is_none(),
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
            program_drag_preview(plan).is_none(),
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
        let bar = program_records(plan)
            .tab_bars
            .into_iter()
            .next()
            .expect("the tabs expose one bar");
        assert!(bar.maximum_scroll_offset() > 0.0);
        assert_eq!(bar.scroll_offset(), 0.0);
        scroll_receiver = plan
            .receiver_for_tab_strip_scroll(bar)
            .filter(|receiver| receiver.role() == DockspaceReceiverRole::TabStripScroll);
        frame
            .confirm_surface_program(SURFACE)
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
        let bar = program_records(plan)
            .tab_bars
            .into_iter()
            .next()
            .expect("the tabs retain one bar");
        assert!(
            bar.scroll_offset() > 0.0,
            "the offset is owned and changed by dockspace; offset={}, maximum={}",
            bar.scroll_offset(),
            bar.maximum_scroll_offset(),
        );
    });
}

#[test]
fn ogc_03_release_on_first_target_hit_commits_without_a_painted_preview() {
    let mut fixture = InteractionFixture::new();
    fixture.begin_drag_without_target();
    let target = fixture.current_target();
    let before = fixture.host.version();

    let mut released = fixture.host.run(|frame| {
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

    assert_eq!(released.before(), before);
    assert_eq!(released.after(), fixture.host.version());
    assert_dock_delivered_once(&released);
    assert!(released.take_painted_outputs().is_empty());
    assert!(!fixture.host.session.has_active_gesture());
    assert!(!fixture.host.session.has_pending_presentation());
    assert_eq!(
        tabs_containing(fixture.host.view(), C)
            .expect("the first-hit release publishes the source item in the target tabs")
            .items(),
        &[C, A],
    );
    assert_eq!(
        tabs_containing(fixture.host.view(), B)
            .expect("the first-hit release preserves the remaining source item")
            .items(),
        &[B],
    );
    let committed = fixture.host.version();
    let settled = fixture.host.run(|_| {});
    assert_eq!(fixture.host.version(), committed);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
}

#[test]
fn ogc_03_cached_or_stale_receiver_cannot_authorize_release() {
    let mut cached = InteractionFixture::new();
    cached.begin_drag_without_target();
    cached.preview_target();
    cached.paint_preview();
    let before_cached = [A, B, C]
        .into_iter()
        .map(|item| item_snapshot(cached.host.view(), item))
        .collect::<Vec<_>>();
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
    assert_eq!(
        [A, B, C]
            .into_iter()
            .map(|item| item_snapshot(cached.host.view(), item))
            .collect::<Vec<_>>(),
        before_cached,
        "an unavailable receiver may cancel the gesture, but cannot mutate workspace ownership"
    );

    let mut stale = InteractionFixture::new();
    stale.begin_drag_without_target();
    stale.preview_target();
    stale.paint_preview();
    let old_target = stale.current_target();
    stale.paint_preview();
    let before_stale = [A, B, C]
        .into_iter()
        .map(|item| item_snapshot(stale.host.view(), item))
        .collect::<Vec<_>>();
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
    assert_eq!(
        [A, B, C]
            .into_iter()
            .map(|item| item_snapshot(stale.host.view(), item))
            .collect::<Vec<_>>(),
        before_stale,
        "a stale receiver may cancel the gesture, but cannot mutate workspace ownership"
    );
}

#[test]
fn ogc_03_current_painted_preview_commits_exactly_once() {
    let mut fixture = InteractionFixture::new();
    fixture.begin_drag_without_target();
    fixture.preview_target();
    fixture.paint_preview();
    let target = fixture.current_target();

    let released = fixture.host.run(|frame| {
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

    assert_dock_delivered_once(&released);
    assert_eq!(released.after(), fixture.host.version());
    assert!(!fixture.host.session.has_active_gesture());
    assert!(!fixture.host.session.has_pending_presentation());
    let target_tabs = tabs_containing(fixture.host.view(), C)
        .expect("the delivered target tabs remain product-visible");
    assert_eq!(target_tabs.items(), &[C, A]);

    let committed = fixture.host.version();
    let metrics = fixture.metrics;
    let mut painted = fixture.host.run(|frame| {
        frame
            .measure_surface(SURFACE, metrics)
            .expect("the committed dock measures in its paint update");
        let plan = frame
            .paint_plan(SURFACE)
            .expect("the committed dock plan resolves")
            .expect("the committed dock remains paintable");
        assert!(program_drag_preview(plan).is_none());
        assert!(program_drag_decoration(plan).is_none());
        frame
            .confirm_surface_program(SURFACE)
            .expect("the committed dock output is painted");
    });
    assert!(!painted.workspace_changed());
    assert!(painted.manipulation_feedback().is_empty());
    let [output] = painted
        .take_painted_outputs()
        .try_into()
        .unwrap_or_else(|outputs: Vec<_>| panic!("expected one dock output, got {outputs:?}"));
    fixture
        .host
        .session
        .report_surface_presentation(output, SurfacePresentationResult::Dropped)
        .expect("a later dropped output retires only that exact output");
    let settled = fixture.host.run(|_| {});
    assert_eq!(fixture.host.version(), committed);
    assert!(!settled.workspace_changed());
    assert!(settled.manipulation_feedback().is_empty());
    assert!(!fixture.host.session.has_pending_presentation());
    assert_eq!(
        tabs_containing(fixture.host.view(), C)
            .expect("dropping the output preserves the committed dock")
            .items(),
        &[C, A],
    );
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
            plan.is_none() || plan.is_some_and(|plan| program_drag_preview(plan).is_none()),
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
fn managed_native_pointer_uses_synchronous_product_receiver_resolution() {
    let mut host = DeterministicHost::new(tabs_layout([A, B]));
    host.session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed desktop coordinator enrolls");
    host.session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed backend capabilities configure");
    let registration = host.register_native_root(SURFACE, WINDOW);
    let binding = match registration.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one native registration, got {outcomes:?}"),
    };
    let physical =
        PhysicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the native window bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the native window scale is valid");
    host.report_managed_native_snapshot(
        [(binding, ready_window_facts(physical, scale))],
        NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
            HostWorkAreaToken::new(1),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work area is valid"),
            scale,
        )]),
    );

    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 360.0).expect("the bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 72.0).expect("the metrics are valid");
    host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the native surface is measured");
        },
    );
    let mut tab = None;
    let paint = host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the native paint plan is available")
                .expect("the native surface is ready");
            tab = plan.tab_receiver(B);
            frame
                .confirm_surface_program(SURFACE)
                .expect("the native output was painted");
        },
    );
    let artifact = host.observe_painted_outputs(paint);
    let tab = tab.expect("item B exposes a tab receiver");
    let receiver = host
        .session
        .bind_presented_receiver(&tab)
        .expect("the tab receiver is bound to the accepted output");
    let point = receiver.center();
    let desktop = PhysicalPoint::new(point.x(), point.y()).expect("the desktop point is valid");
    let location = NativeDesktopPointerLocation::new(
        NativeDesktopPosition::Exact(desktop),
        NativePointerHover::Dock(binding.window()),
        None,
    );
    host.session
        .record_native_pointer(NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ButtonPressed(NativePointerButton::Primary),
            location,
            NativePointerOwner::Native(binding.window()),
            NativePointerOwner::Native(binding.window()),
        ))
        .expect("the press records without exposing a sequence");
    host.session
        .record_native_pointer(NativePointerInput::new(
            NativePointerId::new(1),
            NativePointerEvent::ButtonReleased(NativePointerButton::Primary),
            location,
            NativePointerOwner::Native(binding.window()),
            NativePointerOwner::None,
        ))
        .expect("the release records after the press");

    let resolve = |query| match query {
        NativeReceiverQuery::WindowHit(query) => {
            assert_eq!(query.window(), binding.window());
            assert_eq!(query.point(), desktop);
            NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(artifact))
        }
        NativeReceiverQuery::Surface(query) => {
            assert_eq!(query.surface(), SURFACE);
            assert_eq!(query.point(), Some(point));
            NativeReceiverAnswer::Dock(receiver)
        }
    };
    let mut abandoned_queries = Vec::new();
    drop(
        host.session
            .begin_native_host_frame(|query| {
                abandoned_queries.push(query);
                resolve(query)
            })
            .expect("the first native attempt begins before being abandoned"),
    );
    let mut retry_queries = Vec::new();
    host.run_native(
        |query| {
            retry_queries.push(query);
            resolve(query)
        },
        |_| {},
    );
    assert_eq!(
        abandoned_queries, retry_queries,
        "the failed frame replays the exact lane questions"
    );
    assert_eq!(
        retry_queries
            .iter()
            .filter_map(|query| match query {
                NativeReceiverQuery::Surface(query) => Some(*query.purpose()),
                NativeReceiverQuery::WindowHit(_) => None,
            })
            .collect::<Vec<_>>(),
        vec![
            NativeReceiverPurpose::ClickDelivery,
            NativeReceiverPurpose::DragDelivery,
        ],
        "the frozen press resolves its click and drag lanes independently"
    );
    assert_eq!(
        tabs_containing(host.view(), B)
            .expect("item B remains in one tabs group")
            .selected(),
        Some(B),
        "the resolved native click selects the inactive tab"
    );
}

#[test]
fn managed_native_scroll_keeps_delivery_owner_when_hover_position_is_unknown() {
    let mut host = DeterministicHost::new(tabs_layout([A, B, C, X]));
    host.session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed desktop coordinator enrolls");
    host.session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed backend capabilities configure");
    let registration = host.register_native_root(SURFACE, WINDOW);
    let binding = match registration.inputs() {
        [HostInputOutcome::NativeSurfaceRegistered { binding }] => *binding,
        outcomes => panic!("expected one native registration, got {outcomes:?}"),
    };
    let physical = PhysicalRect::new(0.0, 0.0, 220.0, 180.0).expect("the native bounds are valid");
    let scale = ScaleFactor::new(1.0).expect("the native scale is valid");
    host.report_managed_native_snapshot(
        [(binding, ready_window_facts(physical, scale))],
        NativeWorkAreaRoster::Exact(vec![NativeWorkAreaFacts::new(
            HostWorkAreaToken::new(1),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work area is valid"),
            scale,
        )]),
    );

    let bounds = LogicalRect::new(0.0, 0.0, 220.0, 180.0).expect("the bounds are valid");
    let minimum = LogicalSize::new(0.0, 0.0).expect("the minimum is valid");
    let metrics = UniformSurfaceMetrics::new(bounds, minimum, 96.0)
        .expect("the fixture measurements are valid");
    host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the first pass measures the overflow strip");
        },
    );
    let mut scroll_receiver = None;
    let paint = host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the native strip is paintable")
                .expect("the native surface has one paint plan");
            let bar = program_records(plan)
                .tab_bars
                .into_iter()
                .next()
                .expect("the native tabs expose one bar");
            scroll_receiver = plan
                .receiver_for_tab_strip_scroll(bar)
                .filter(|receiver| receiver.role() == DockspaceReceiverRole::TabStripScroll);
            frame
                .confirm_surface_program(SURFACE)
                .expect("the native overflow output was painted");
        },
    );
    let artifact = host.observe_painted_outputs(paint);
    let receiver = host
        .session
        .bind_presented_receiver(
            &scroll_receiver.expect("the overflow strip exposes a scroll receiver"),
        )
        .expect("the scroll receiver belongs to the presented native output");
    let center = receiver.center();
    let exact = NativeDesktopPointerLocation::new(
        NativeDesktopPosition::Exact(
            PhysicalPoint::new(center.x(), center.y()).expect("the desktop point is valid"),
        ),
        NativePointerHover::Dock(binding.window()),
        None,
    );
    let continuation = NativeDesktopPointerLocation::new(
        NativeDesktopPosition::Unknown,
        NativePointerHover::Dock(binding.window()),
        None,
    );
    let modifiers = NativeScrollModifiers::Exact {
        shift: false,
        control: false,
        alt: false,
        command: false,
    };
    let smooth = NativeScrollSequenceId::new(1);
    for (location, phase) in [
        (
            exact,
            NativeScrollPhase::Discrete {
                delta: NativeScrollDelta::Lines { x: -1.0, y: 0.0 },
            },
        ),
        (
            exact,
            NativeScrollPhase::Begin {
                sequence: smooth,
                delta: Some(NativeScrollDelta::Lines { x: -0.5, y: 0.0 }),
            },
        ),
        (
            continuation,
            NativeScrollPhase::Update {
                sequence: smooth,
                delta: NativeScrollDelta::Lines { x: -0.5, y: 0.0 },
            },
        ),
        (
            continuation,
            NativeScrollPhase::End {
                sequence: smooth,
                delta: None,
            },
        ),
    ] {
        host.session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(21),
                NativePointerEvent::Scrolled(NativeScrollEvent::new(
                    NativeScrollDeviceId::new(4),
                    phase,
                    NativeScrollMomentum::Direct,
                    modifiers,
                )),
                location,
                NativePointerOwner::Native(binding.window()),
                NativePointerOwner::None,
            ))
            .expect("the ordered native scroll edge records");
    }

    let mut purposes = Vec::new();
    host.run_native(
        |query| {
            let query = match query {
                NativeReceiverQuery::WindowHit(query) => {
                    assert_eq!(query.window(), binding.window());
                    assert_eq!(
                        query.point(),
                        PhysicalPoint::new(center.x(), center.y())
                            .expect("the window-client scroll point is valid"),
                    );
                    return NativeReceiverAnswer::WindowHit(NativeWindowHit::Surface(artifact));
                }
                NativeReceiverQuery::Surface(query) => query,
            };
            purposes.push(*query.purpose());
            assert_eq!(query.surface(), SURFACE);
            assert_eq!(query.point(), Some(center));
            NativeReceiverAnswer::Dock(receiver)
        },
        |_| {},
    );
    assert_eq!(
        purposes.len(),
        4,
        "each scroll edge retains one delivery proof"
    );
    assert!(matches!(
        purposes[0],
        NativeReceiverPurpose::ScrollDelivery(NativeScrollReceiverChallenge::Spatial {
            projected_delta: Some(_),
        })
    ));
    assert!(matches!(
        purposes[1],
        NativeReceiverPurpose::ScrollDelivery(NativeScrollReceiverChallenge::Spatial {
            projected_delta: Some(_),
        })
    ));
    for purpose in &purposes[2..] {
        let NativeReceiverPurpose::ScrollDelivery(NativeScrollReceiverChallenge::Locked {
            receiver: locked,
            ..
        }) = purpose
        else {
            panic!("smooth continuation must name its frozen receiver");
        };
        assert_eq!(*locked, receiver);
    }
    assert!(matches!(
        purposes[3],
        NativeReceiverPurpose::ScrollDelivery(NativeScrollReceiverChallenge::Locked {
            projected_delta: None,
            ..
        })
    ));
    host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            frame
                .measure_surface(SURFACE, metrics)
                .expect("the changed scroll state recompiles");
        },
    );
    host.run_native(
        |_| NativeReceiverAnswer::Unknown,
        |frame| {
            let plan = frame
                .paint_plan(SURFACE)
                .expect("the native strip remains paintable")
                .expect("the native surface retains one paint plan");
            let bar = program_records(plan)
                .tab_bars
                .into_iter()
                .next()
                .expect("the tabs retain one bar");
            assert!(
                bar.scroll_offset() > 0.0,
                "native scroll changed the core-owned offset"
            );
        },
    );
}

#[test]
fn unowned_native_scroll_fails_closed_without_receiver_queries() {
    let mut host = DeterministicHost::new(tabs_layout([A, B]));
    host.session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("the managed desktop coordinator enrolls");
    host.session
        .configure_managed_native_capabilities(full_managed_capabilities())
        .expect("the managed backend capabilities configure");
    let exact = PhysicalPoint::new(16.0, 24.0).expect("the desktop point is valid");
    let modifiers = NativeScrollModifiers::Exact {
        shift: false,
        control: false,
        alt: false,
        command: false,
    };
    for (position, hover, delivery) in [
        (
            NativeDesktopPosition::Exact(exact),
            NativePointerHover::Foreign,
            NativePointerOwner::Foreign,
        ),
        (
            NativeDesktopPosition::Exact(exact),
            NativePointerHover::OutsideAll,
            NativePointerOwner::None,
        ),
        (
            NativeDesktopPosition::Unknown,
            NativePointerHover::Unknown,
            NativePointerOwner::Unknown,
        ),
    ] {
        host.session
            .record_native_pointer(NativePointerInput::new(
                NativePointerId::new(31),
                NativePointerEvent::Scrolled(NativeScrollEvent::new(
                    NativeScrollDeviceId::new(7),
                    NativeScrollPhase::Discrete {
                        delta: NativeScrollDelta::Lines { x: 0.0, y: -1.0 },
                    },
                    NativeScrollMomentum::Direct,
                    modifiers,
                )),
                NativeDesktopPointerLocation::new(position, hover, None),
                delivery,
                NativePointerOwner::None,
            ))
            .expect("a legal unowned scroll edge records");
    }

    let mut queries = 0;
    host.run_native(
        |_| {
            queries += 1;
            NativeReceiverAnswer::Unknown
        },
        |_| {},
    );
    assert_eq!(
        queries, 0,
        "foreign, outside-all, and unknown scroll facts do not query dock receivers"
    );
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
        error.native_kind(),
        Some(NativeHostErrorKind::StaleBinding)
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
        error.native_kind(),
        Some(NativeHostErrorKind::InvalidFacts)
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
        .report_native_snapshot(std::iter::empty::<(NativeWindowBinding, NativeWindowFacts)>())
        .expect_err("facts cannot be validated against the pre-registration roster");
    assert!(matches!(
        error.native_kind(),
        Some(NativeHostErrorKind::OperationConflict)
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
