use dockspace::geometry::{LogicalPoint, LogicalRect, LogicalSize};
use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceAxis, DockspaceContainedLayout,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    FloatingPresentationId, ItemId, RootId, SurfaceId,
};
use dockspace::policy::{DockPolicy, DockTargetRule, DockTargetRuleKey};
use dockspace::runtime::{
    ContainedResizeDirection as Direction, DockspaceDragSourceKind as DragSourceKind,
    DockspaceDropDirection as DropDirection, DockspaceDropEligibility as Eligibility,
    DockspaceGuideScope as GuideScope, DockspaceManipulationDelivery as Delivery,
    DockspaceManipulationStatus as Status, DockspacePreviewVisual, DockspaceReceiverDescriptor,
    DockspaceReceiverRole as Role, DockspaceRuntimeError, DockspaceRuntimeErrorKind,
    DockspaceSession, DockspaceVisualId, DockspaceVisualKind as Kind, HostFrameReport,
    HostPointerInput, HostPointerRoute, PaintedSurfaceOutput, PointerEventMetadata,
    SurfacePaintDisposition as Paint, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePointerButton as Button, SurfacePointerCapture,
    SurfacePointerEvent as Event, SurfacePointerId, SurfacePointerReceiverFacts,
    SurfacePresentationResult as Disposition, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const MAIN_ROOT: RootId = RootId::new(2);
const FLOATING: FloatingPresentationId = FloatingPresentationId::new(1);
const LEFT: ItemId = ItemId::new(1);
const RIGHT: ItemId = ItemId::new(2);
const BACKGROUND: ItemId = ItemId::new(3);

fn rect(x: f64, y: f64, width: f64, height: f64) -> LogicalRect {
    LogicalRect::new(x, y, width, height).unwrap()
}

fn initial_rect() -> LogicalRect {
    rect(200.0, 140.0, 400.0, 300.0)
}

fn offset(point: LogicalPoint, dx: f64, dy: f64) -> LogicalPoint {
    LogicalPoint::new(point.x() + dx, point.y() + dy).unwrap()
}

fn measure(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                rect(0.0, 0.0, 960.0, 720.0),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

struct RecordedGuide {
    root: RootId,
    scope: GuideScope,
    direction: DropDirection,
    visual: DockspaceVisualId,
    hit: LogicalRect,
    preview: LogicalRect,
    active: bool,
}

struct Recorded {
    renderer: RecordingRenderer,
    title: DockspaceReceiverDescriptor,
    close: DockspaceReceiverDescriptor,
    resize: Vec<(Direction, DockspaceReceiverDescriptor)>,
    panes: Vec<DockspaceReceiverDescriptor>,
    group_grips: Vec<DockspaceReceiverDescriptor>,
    guides: Vec<RecordedGuide>,
    projected_source: Option<(DragSourceKind, LogicalRect)>,
}

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        rect(0.0, 0.0, 960.0, 720.0),
        LogicalSize::new(64.0, 48.0).unwrap(),
        96.0,
    )
    .unwrap()
}

fn paint(session: &mut DockspaceSession) -> (Recorded, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    if frame.paint_plan(SURFACE).unwrap().is_none() {
        frame.measure_surface(SURFACE, metrics()).unwrap();
    }
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut title = None;
    let mut close = None;
    let mut resize = Vec::new();
    let mut panes = Vec::new();
    let mut group_grips = Vec::new();
    let mut guides = Vec::new();
    let mut projected_source = None;
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, .. } if root.root() == ROOT => {
                assert_eq!(root.title_item(), Some(LEFT));
                let contained = root.contained().unwrap();
                assert!(contained.is_frontmost());
                assert_eq!(contained.floating(), FLOATING);
                assert!(
                    title
                        .replace(plan.receiver_for_contained_title(contained).unwrap())
                        .is_none()
                );
                close = plan.receiver_for_contained_close(contained);
                for handle in contained.resize() {
                    resize.push((
                        handle.direction(),
                        plan.receiver_for_contained_resize(contained, handle)
                            .unwrap(),
                    ));
                }
            }
            Step::Operation {
                operation: Operation::Pane(pane),
                ..
            } if pane.root() == ROOT => {
                panes.push(plan.receiver_for_pane(pane).unwrap());
            }
            Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } if bar.root() == ROOT => {
                if let Some(grip) = plan.receiver_for_tab_group_grip(bar) {
                    group_grips.push(grip);
                }
            }
            Step::DropGuide {
                structural,
                affordance,
            } => {
                let active = affordance.and_then(|cluster| {
                    cluster
                        .targets()
                        .find(|target| target.is_active())
                        .map(|target| target.visual_id())
                });
                for target in structural.targets() {
                    guides.push(RecordedGuide {
                        root: structural.root(),
                        scope: structural.scope(),
                        direction: target.direction(),
                        visual: target.visual_id(),
                        hit: target.hit_bounds(),
                        preview: target.preview_bounds(),
                        active: active == Some(target.visual_id()),
                    });
                }
            }
            Step::DragDecoration(source) if source.source_is_projected() => {
                assert_eq!(source.source_surface(), SURFACE);
                assert!(
                    projected_source
                        .replace((source.kind(), source.projected_root_bounds().unwrap()))
                        .is_none()
                );
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (renderer, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().unwrap();
    assert_eq!(output.surface(), SURFACE);
    assert!(
        matches!(renderer.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
        if output.matches_semantic_output(*semantic))
    );
    (
        Recorded {
            renderer,
            title: title.unwrap(),
            close: close.unwrap(),
            resize,
            panes,
            group_grips,
            guides,
            projected_source,
        },
        output,
    )
}

fn present(
    session: &mut DockspaceSession,
    output: PaintedSurfaceOutput,
    disposition: Disposition,
) -> HostFrameReport {
    let artifact = output.presentation_artifact();
    let version = session.version();
    let durable = session.view().contained(FLOATING).unwrap().rect();
    session
        .report_surface_presentation(output, disposition)
        .unwrap();
    assert_eq!(
        session.version(),
        version,
        "reporting alone cannot mutate placement"
    );
    assert_durable(session, durable);
    let report = settle(session);
    let matching = report
        .surface_presentation_commits()
        .iter()
        .filter(|commit| commit.settles(artifact))
        .collect::<Vec<_>>();
    let [commit] = matching.as_slice() else {
        panic!("one exact recorded output must settle at the core commit: {artifact:?}: {report:?}")
    };
    assert!(commit.settles(artifact));
    if disposition == Disposition::Dropped {
        assert!(commit.retires(artifact));
    }
    report
}

fn fixture() -> (DockspaceSession, Recorded) {
    let mut session = session_with_contained_rect(initial_rect());
    let (recorded, output) = paint(&mut session);
    assert_projected(&session, &recorded, &recorded, initial_rect());
    assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
    assert_current_receivers(&session, &recorded);
    (session, recorded)
}

fn session_with_contained_rect(bounds: LogicalRect) -> DockspaceSession {
    let split = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [DockspaceNode::tabs([LEFT]), DockspaceNode::tabs([RIGHT])],
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::tabs([BACKGROUND])),
    )
    .with_contained(DockspaceContainedLayout::new(
        FLOATING,
        DockspaceRootLayout::new(ROOT, split),
        bounds,
    ))])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    session.enable_host_pointer().unwrap();
    measure(&mut session);
    session
}

fn edge_with_routes(
    session: &mut DockspaceSession,
    event: Event,
    manipulation: LogicalPoint,
    hover: LogicalPoint,
) {
    let delivery_hit = session.host_pointer_hit(SURFACE, manipulation).unwrap();
    let hover_hit = session.host_pointer_hit(SURFACE, hover).unwrap();
    let source = delivery_hit.surface();
    let target = hover_hit.surface();
    let click = delivery_hit.delivery();
    let drag = delivery_hit.drag_delivery();
    let facts = click.as_ref().map_or_else(
        || SurfacePointerReceiverFacts::unknown().with_no_delivery(&source),
        SurfacePointerReceiverFacts::delivery,
    );
    let facts = drag.as_ref().map_or_else(
        || facts.with_no_drag_delivery(&source),
        |receiver| facts.with_drag_delivery(receiver),
    );
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .submit_host_pointer(
            HostPointerInput::new(
                SurfacePointerId::new(1),
                event,
                HostPointerRoute::new(source, manipulation),
                HostPointerRoute::new(target, hover),
                SurfacePointerCapture::ProviderEndpoint,
                facts.with_clear_hover_lane(&target),
            )
            .with_metadata(PointerEventMetadata::unknown().with_magnetic_targeting(false)),
        )
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

#[test]
fn review_contained_move_uses_manipulation_for_both_projection_and_commit_preview() {
    let (mut session, before) = fixture();
    let press = before.title.center();
    edge(&mut session, Event::ButtonPressed(Button::Primary), press).unwrap();
    edge_with_routes(
        &mut session,
        Event::Moved,
        offset(press, 56.0, 32.0),
        offset(press, 176.0, 32.0),
    );
    let (recorded, _output) = paint(&mut session);
    let projection = recorded
        .projected_source
        .expect("the contained drag must project its complete content")
        .1;
    let preview = recorded
        .renderer
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::Preview(DockspacePreviewVisual::Contained {
                surface: SURFACE,
                rect,
                ..
            }) => Some(*rect),
            _ => None,
        })
        .expect("the non-docking drag must expose a contained commit preview");
    assert_eq!(projection, rect(256.0, 172.0, 400.0, 300.0));
    assert_eq!(
        preview, projection,
        "hover coordinates must not create a second contained position"
    );
}

struct ContainedMotion {
    title: Option<DockspaceReceiverDescriptor>,
    projection: Option<LogicalRect>,
    preview: Option<LogicalRect>,
}

fn paint_contained_motion(
    session: &mut DockspaceSession,
) -> (ContainedMotion, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut title = None;
    let mut projection = None;
    let mut preview = None;
    // Oversized windows can clip resize/close receivers; only the visible title is needed.
    plan.visit_surface_program(|step| {
        match step {
            Step::RootBegin { root, .. } if root.root() == ROOT => {
                title = plan.receiver_for_contained_title(root.contained().unwrap());
            }
            Step::DragDecoration(source) if source.source_is_projected() => {
                projection = source.projected_root_bounds();
            }
            Step::DragPreview(candidate) => {
                if let DockspacePreviewVisual::Contained { rect, .. } = candidate.visual() {
                    preview = Some(rect);
                }
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    (
        ContainedMotion {
            title,
            projection,
            preview,
        },
        outputs.pop().unwrap(),
    )
}

#[test]
fn review_contained_move_of_oversized_source_preserves_one_width() {
    let mut session = session_with_contained_rect(rect(0.0, 140.0, 1000.0, 300.0));
    let (before, output) = paint_contained_motion(&mut session);
    present(&mut session, output, Disposition::Presented);
    let press = before
        .title
        .expect("the oversized contained source must retain a visible title")
        .center();
    edge(&mut session, Event::ButtonPressed(Button::Primary), press).unwrap();
    let moved = offset(press, 30.0, 20.0);
    edge_with_routes(&mut session, Event::Moved, moved, moved);
    let (recorded, _output) = paint_contained_motion(&mut session);
    let projection = recorded
        .projection
        .expect("the oversized contained drag must project its content");
    let preview = recorded
        .preview
        .expect("the oversized contained drag must expose a commit preview");
    assert_eq!(projection.width(), 1000.0);
    assert_eq!(
        preview.width(),
        projection.width(),
        "moving an existing contained source must preserve its width in the commit preview"
    );
}

fn edge(
    session: &mut DockspaceSession,
    event: Event,
    position: LogicalPoint,
) -> Result<HostFrameReport, DockspaceRuntimeError> {
    // Resolve each lane at this edge, after every inspecting frame has ended.
    let hit = session.host_pointer_hit(SURFACE, position).unwrap();
    let surface = hit.surface();
    let click = hit.delivery();
    let drag = hit.drag_delivery();
    let hover = hit.hover();
    let facts = match click.as_ref() {
        Some(receiver) => SurfacePointerReceiverFacts::delivery(receiver),
        None => SurfacePointerReceiverFacts::unknown().with_no_delivery(&surface),
    };
    let facts = match drag.as_ref() {
        Some(receiver) => facts.with_drag_delivery(receiver),
        None => facts.with_no_drag_delivery(&surface),
    };
    let facts = match hover.as_ref() {
        Some(receiver) => facts.with_hover(receiver),
        None => facts.with_no_hover(&surface),
    };
    let route = HostPointerRoute::new(surface, position);
    let mut frame = session.begin_host_frame().unwrap();
    frame.submit_host_pointer_batch([HostPointerInput::new(
        SurfacePointerId::new(1),
        event,
        route,
        route,
        SurfacePointerCapture::ProviderEndpoint,
        facts,
    )])?;
    frame.complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)?;
    frame.commit()
}

fn root_records(recorded: &Recorded) -> impl Iterator<Item = &OwnedRecord> {
    recorded
        .renderer
        .records
        .iter()
        .skip_while(|record| !matches!(record, OwnedRecord::RootBegin(ROOT)))
        .take_while(|record| !matches!(record, OwnedRecord::RootEnd(ROOT)))
}

fn geometry(recorded: &Recorded, kind: Kind) -> Vec<(DockspaceVisualId, LogicalRect, Paint)> {
    root_records(recorded)
        .filter_map(|record| match record {
            OwnedRecord::Geometry {
                visual,
                bounds,
                paint,
            } if visual.kind() == kind => Some((*visual, *bounds, *paint)),
            _ => None,
        })
        .collect()
}

fn tab_receivers(recorded: &Recorded) -> Vec<DockspaceReceiverDescriptor> {
    root_records(recorded)
        .filter_map(|record| match record {
            OwnedRecord::Receiver(receiver) if receiver.role() == Role::TabBody => Some(*receiver),
            _ => None,
        })
        .collect()
}

fn assert_durable(session: &DockspaceSession, expected: LogicalRect) {
    assert_eq!(session.view().contained(FLOATING).unwrap().rect(), expected);
    assert_eq!(
        session.view().surface(SURFACE).unwrap().contained_count(),
        1
    );
    assert_eq!(session.view().item(BACKGROUND).unwrap().root(), MAIN_ROOT);
    let split = session
        .view()
        .root(ROOT)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap();
    assert_eq!(split.axis(), DockspaceAxis::Horizontal);
    assert_eq!(split.weights().collect::<Vec<_>>(), [0.5, 0.5]);
    assert_eq!(split.children().count(), 2);
    for item in [LEFT, RIGHT] {
        let view = session.view().item(item).unwrap();
        assert_eq!(view.root(), ROOT);
        assert_eq!(view.surface(), SURFACE);
        assert_eq!(view.contained(), Some(FLOATING));
        assert_eq!(view.tabs().items(), &[item]);
        assert!(view.is_selected());
    }
}

fn assert_projected(
    session: &DockspaceSession,
    before: &Recorded,
    recorded: &Recorded,
    projected: LogicalRect,
) {
    let config = session.presentation_config();
    let border = config.floating_border_width();
    let title = config.floating_title_height();
    let tab = config.tab_bar_height();
    let gap = config.splitter_thickness();
    let x = projected.x() + border;
    let y = projected.y() + border + title;
    let height = projected.height() - 2.0 * border - title;
    let width = (projected.width() - 2.0 * border - gap) * 0.5;
    let expected_panes = [
        rect(x, y, width, height),
        rect(x + width + gap, y, width, height),
    ];
    let contents: Vec<_> = root_records(recorded)
        .filter_map(|record| match record {
            OwnedRecord::PaneContent {
                visual,
                item,
                bounds,
                paint,
            } => Some((*visual, *item, *bounds, *paint)),
            _ => None,
        })
        .collect();
    let original_panes = geometry(before, Kind::Pane);
    assert_eq!(
        contents.len(),
        2,
        "the complete split subtree must be emitted exactly once"
    );
    assert_eq!(recorded.panes.len(), 2);
    for (index, (item, pane)) in [LEFT, RIGHT].into_iter().zip(expected_panes).enumerate() {
        let content = rect(pane.x(), pane.y() + tab, pane.width(), pane.height() - tab);
        assert_eq!(contents[index].1, Some(item));
        assert_eq!(
            contents[index].2, content,
            "{item:?}: pane content must be compiled from {projected:?}, not left under a preview shell"
        );
        assert_eq!(contents[index].0, original_panes[index].0);
        assert_eq!(
            contents[index].3,
            Paint::Visible,
            "live content cannot remain a source gap"
        );
        assert_eq!(recorded.panes[index].bounds(), content);
        assert_eq!(
            recorded.panes[index].visual_id(),
            before.panes[index].visual_id()
        );
    }
    for (kind, expected) in [
        (Kind::Pane, expected_panes.to_vec()),
        (
            Kind::TabBar,
            expected_panes
                .map(|pane| rect(pane.x(), pane.y(), pane.width(), tab))
                .to_vec(),
        ),
        (Kind::Splitter, vec![rect(x + width, y, gap, height)]),
        (
            Kind::Contained,
            vec![
                projected,
                rect(
                    x,
                    projected.y() + border,
                    projected.width() - 2.0 * border,
                    title,
                ),
            ],
        ),
    ] {
        let original = geometry(before, kind);
        let actual = geometry(recorded, kind);
        assert_eq!(
            actual.len(),
            expected.len(),
            "complete {kind:?} geometry roster"
        );
        for (index, bounds) in expected.into_iter().enumerate() {
            assert_eq!(
                actual[index],
                (original[index].0, bounds, Paint::Visible),
                "{kind:?}"
            );
        }
    }
    let dx = projected.x() - initial_rect().x();
    let dy = projected.y() - initial_rect().y();
    let dw = projected.width() - initial_rect().width();
    let dh = projected.height() - initial_rect().height();
    let old_title = before.title.bounds();
    assert_eq!(recorded.title.visual_id(), before.title.visual_id());
    assert_eq!(
        recorded.title.bounds(),
        rect(
            old_title.x() + dx,
            old_title.y() + dy,
            old_title.width() + dw,
            old_title.height()
        )
    );
    let close = before.close.bounds();
    assert_eq!(recorded.close.visual_id(), before.close.visual_id());
    assert_eq!(
        recorded.close.bounds(),
        rect(
            close.x() + dx + dw,
            close.y() + dy,
            close.width(),
            close.height()
        )
    );
    let old_anchor = geometry(before, Kind::PresentationMenuAnchor);
    let anchors = geometry(recorded, Kind::PresentationMenuAnchor);
    assert_eq!(anchors.len(), 1);
    let (visual, anchor, _) = old_anchor[0];
    assert_eq!(
        anchors[0],
        (
            visual,
            rect(
                anchor.x() + dx + dw,
                anchor.y() + dy,
                anchor.width(),
                anchor.height()
            ),
            Paint::Visible
        )
    );
    assert_eq!(recorded.resize.len(), 8);
    for ((direction, actual), (old_direction, old)) in recorded.resize.iter().zip(&before.resize) {
        assert_eq!(direction, old_direction);
        assert_eq!(actual.visual_id(), old.visual_id());
        let (sx, sy, sw, sh) = match direction {
            Direction::NorthWest => (0.0, 0.0, 0.0, 0.0),
            Direction::North => (0.0, 0.0, dw, 0.0),
            Direction::NorthEast => (dw, 0.0, 0.0, 0.0),
            Direction::East => (dw, 0.0, 0.0, dh),
            Direction::SouthEast => (dw, dh, 0.0, 0.0),
            Direction::South => (0.0, dh, dw, 0.0),
            Direction::SouthWest => (0.0, dh, 0.0, 0.0),
            Direction::West => (0.0, 0.0, 0.0, dh),
        };
        let bounds = old.bounds();
        assert_eq!(
            actual.bounds(),
            rect(
                bounds.x() + dx + sx,
                bounds.y() + dy + sy,
                bounds.width() + sw,
                bounds.height() + sh
            ),
            "{direction:?} receiver"
        );
    }
    let old_tabs = geometry(before, Kind::Tab);
    let tabs = geometry(recorded, Kind::Tab);
    let old_tab_receivers = tab_receivers(before);
    let tab_receivers = tab_receivers(recorded);
    assert_eq!(tabs.len(), 2);
    assert_eq!(tab_receivers.len(), 2);
    for index in 0..2 {
        let shift_x = dx + if index == 0 { 0.0 } else { dw * 0.5 };
        let (_, old, _) = old_tabs[index];
        assert_eq!(
            tabs[index],
            (
                old_tabs[index].0,
                rect(old.x() + shift_x, old.y() + dy, old.width(), old.height()),
                Paint::Visible
            )
        );
        let old = old_tab_receivers[index];
        let bounds = old.bounds();
        assert_eq!(tab_receivers[index].visual_id(), old.visual_id());
        assert_eq!(
            tab_receivers[index].bounds(),
            rect(
                bounds.x() + shift_x,
                bounds.y() + dy,
                bounds.width(),
                bounds.height()
            )
        );
    }
}

fn assert_current_receivers(session: &DockspaceSession, recorded: &Recorded) {
    for descriptor in recorded
        .panes
        .iter()
        .copied()
        .chain([recorded.title, recorded.close])
        .chain(recorded.resize.iter().map(|(_, receiver)| *receiver))
        .chain(tab_receivers(recorded))
    {
        let bound = session.bind_presented_receiver(&descriptor).unwrap();
        let hit = session.host_pointer_hit(SURFACE, bound.center()).unwrap();
        if descriptor.role() != Role::ContainedClose {
            assert_eq!(
                hit.drag_delivery(),
                Some(bound),
                "projected drag receiver must win at its painted bounds"
            );
        }
        if matches!(
            descriptor.role(),
            Role::PaneBody | Role::TabBody | Role::ContainedClose
        ) {
            assert_eq!(hit.delivery(), Some(bound));
        }
    }
}

fn assert_preview(recorded: &Recorded, projected: LogicalRect, resize: bool) {
    assert!(
        recorded.renderer.records.iter().any(|record| match record {
            OwnedRecord::TransformPreview(floating, bounds) if resize =>
                *floating == FLOATING && *bounds == projected,
            OwnedRecord::Preview(DockspacePreviewVisual::Contained { surface, rect, .. })
                if !resize =>
                *surface == SURFACE && *rect == projected,
            _ => false,
        }),
        "the real pointer journey must have reached the requested projection"
    );
}

#[test]
fn host_local_title_move_compiles_the_complete_split_before_terminal_commit() {
    let (mut session, before) = fixture();
    let version = session.version();
    let press = before.title.center();
    assert!(
        !edge(&mut session, Event::ButtonPressed(Button::Primary), press)
            .unwrap()
            .workspace_changed()
    );
    for (dx, dy) in [(56.0, 32.0), (80.0, 48.0)] {
        let moved = offset(press, dx, dy);
        let projected = rect(200.0 + dx, 140.0 + dy, 400.0, 300.0);
        assert!(
            !edge(&mut session, Event::Moved, moved)
                .unwrap()
                .workspace_changed()
        );
        assert!(session.has_active_gesture());
        assert_durable(&session, initial_rect());
        let (recorded, output) = paint(&mut session);
        assert_preview(&recorded, projected, false);
        assert_projected(&session, &before, &recorded, projected);
        assert_eq!(session.version(), version);
        assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
        assert_current_receivers(&session, &recorded);
        assert_durable(&session, initial_rect());
    }
    let released = edge(
        &mut session,
        Event::ButtonReleased(Button::Primary),
        offset(press, 80.0, 48.0),
    )
    .unwrap();
    assert!(released.workspace_changed());
    assert_durable(&session, rect(280.0, 188.0, 400.0, 300.0));
    assert!(!session.has_active_gesture());
}

#[test]
fn host_local_all_eight_resize_directions_recompile_both_panes_and_receivers() {
    for (direction, dx, dy, projected) in [
        (
            Direction::North,
            0.0,
            -24.0,
            rect(200.0, 116.0, 400.0, 324.0),
        ),
        (
            Direction::NorthEast,
            40.0,
            -24.0,
            rect(200.0, 116.0, 440.0, 324.0),
        ),
        (Direction::East, 40.0, 0.0, rect(200.0, 140.0, 440.0, 300.0)),
        (
            Direction::SouthEast,
            40.0,
            24.0,
            rect(200.0, 140.0, 440.0, 324.0),
        ),
        (
            Direction::South,
            0.0,
            24.0,
            rect(200.0, 140.0, 400.0, 324.0),
        ),
        (
            Direction::SouthWest,
            -40.0,
            24.0,
            rect(160.0, 140.0, 440.0, 324.0),
        ),
        (
            Direction::West,
            -40.0,
            0.0,
            rect(160.0, 140.0, 440.0, 300.0),
        ),
        (
            Direction::NorthWest,
            -40.0,
            -24.0,
            rect(160.0, 116.0, 440.0, 324.0),
        ),
    ] {
        let (mut session, before) = fixture();
        let version = session.version();
        let press = before
            .resize
            .iter()
            .find(|(actual, _)| *actual == direction)
            .unwrap()
            .1
            .center();
        assert!(
            !edge(&mut session, Event::ButtonPressed(Button::Primary), press)
                .unwrap()
                .workspace_changed()
        );
        let moved = offset(press, dx, dy);
        assert!(
            !edge(&mut session, Event::Moved, moved)
                .unwrap()
                .workspace_changed()
        );
        assert!(session.has_active_gesture(), "{direction:?}");
        let (recorded, output) = paint(&mut session);
        assert_preview(&recorded, projected, true);
        assert_projected(&session, &before, &recorded, projected);
        assert_eq!(session.version(), version);
        assert_durable(&session, initial_rect());
        assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
        assert_current_receivers(&session, &recorded);
        assert_durable(&session, initial_rect());
        assert!(
            edge(&mut session, Event::ButtonReleased(Button::Primary), moved)
                .unwrap()
                .workspace_changed()
        );
        assert_durable(&session, projected);
        assert!(!session.has_active_gesture());
    }
}

#[test]
fn release_before_paint_commits_once_and_presented_or_dropped_outputs_preserve_it() {
    for resize in [false, true] {
        for disposition in [Disposition::Presented, Disposition::Dropped] {
            let (mut session, before) = fixture();
            let version = session.version();
            let (press, dx, dy, projected) = if resize {
                let press = before
                    .resize
                    .iter()
                    .find(|(direction, _)| *direction == Direction::SouthEast)
                    .unwrap()
                    .1
                    .center();
                (press, 40.0, 24.0, rect(200.0, 140.0, 440.0, 324.0))
            } else {
                (
                    before.title.center(),
                    56.0,
                    32.0,
                    rect(256.0, 172.0, 400.0, 300.0),
                )
            };
            let moved = offset(press, dx, dy);
            let held = session.bind_presented_receiver(&before.title).unwrap();
            let old_surface = session.presented_surface(SURFACE).unwrap();
            for (event, point) in [
                (Event::ButtonPressed(Button::Primary), press),
                (Event::Moved, moved),
            ] {
                let report = edge(&mut session, event, point).unwrap();
                assert!(!report.workspace_changed());
                assert!(report.manipulation_feedback().is_empty());
                assert_durable(&session, initial_rect());
            }
            let released =
                edge(&mut session, Event::ButtonReleased(Button::Primary), moved).unwrap();
            assert!(released.workspace_changed());
            assert_eq!(released.before(), version);
            assert_eq!(released.after(), session.version());
            let [terminal] = released.manipulation_feedback() else {
                panic!("one ordinary release terminal: {released:?}")
            };
            assert_eq!(
                terminal.status(),
                Status::Delivered(if resize {
                    Delivery::ContainedTransform
                } else {
                    Delivery::Contained
                })
            );
            assert_durable(&session, projected);
            assert_ne!(session.version(), version);
            assert!(!session.has_active_gesture());
            assert!(!session.has_pending_presentation());
            assert!(session.bind_presented_receiver(&before.title).is_none());
            // Even an already held receiver retains its old geometry and must be rejected for a fresh press.
            let route = HostPointerRoute::new(old_surface, held.center());
            let mut stale = session.begin_host_frame().unwrap();
            stale
                .submit_host_pointer(HostPointerInput::new(
                    SurfacePointerId::new(2),
                    Event::ButtonPressed(Button::Primary),
                    route,
                    route,
                    SurfacePointerCapture::ProviderEndpoint,
                    SurfacePointerReceiverFacts::unknown().with_drag_delivery(&held),
                ))
                .unwrap();
            stale
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            let rejected = stale.commit().unwrap();
            assert!(!rejected.workspace_changed());
            assert!(
                rejected
                    .manipulation_feedback()
                    .iter()
                    .all(|feedback| !matches!(
                        feedback.status(),
                        Status::Delivered(_) | Status::Pending(_)
                    ))
            );
            assert!(!session.has_active_gesture());
            let terminal_version = session.version();
            let (recorded, output) = paint(&mut session);
            assert_projected(&session, &before, &recorded, projected);
            assert!(recorded.renderer.records.iter().all(|record| !matches!(
                record,
                OwnedRecord::Preview(_) | OwnedRecord::TransformPreview(..)
            )));
            let settled = present(&mut session, output, disposition);
            assert!(!settled.workspace_changed());
            assert!(settled.manipulation_feedback().is_empty());
            assert_durable(&session, projected);
            assert_eq!(session.version(), terminal_version);
            for _ in 0..2 {
                let (recorded, output) = paint(&mut session);
                assert_projected(&session, &before, &recorded, projected);
                let report = present(&mut session, output, Disposition::Presented);
                assert!(!report.workspace_changed());
                assert!(report.manipulation_feedback().is_empty());
                assert_current_receivers(&session, &recorded);
                assert_durable(&session, projected);
            }
            match edge(&mut session, Event::ButtonReleased(Button::Primary), moved) {
                Ok(duplicate) => {
                    assert!(!duplicate.workspace_changed());
                    assert!(duplicate.manipulation_feedback().is_empty());
                }
                Err(error) => assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol),
            }
            assert!(!settle(&mut session).workspace_changed());
            assert_eq!(session.version(), terminal_version);
            assert_durable(&session, projected);
            assert!(!session.has_active_gesture());
        }
    }
}

#[test]
fn dropped_resize_successor_preserves_the_presented_projection_and_release_authority() {
    let (mut session, before) = fixture();
    let version = session.version();
    let press = before
        .resize
        .iter()
        .find(|(direction, _)| *direction == Direction::SouthEast)
        .unwrap()
        .1
        .center();
    let moved = offset(press, 40.0, 24.0);
    let projected = rect(200.0, 140.0, 440.0, 324.0);
    for (event, point) in [
        (Event::ButtonPressed(Button::Primary), press),
        (Event::Moved, moved),
    ] {
        assert!(
            !edge(&mut session, event, point)
                .unwrap()
                .workspace_changed()
        );
    }
    let (live, output) = paint(&mut session);
    assert_preview(&live, projected, true);
    assert_projected(&session, &before, &live, projected);
    assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
    assert_current_receivers(&session, &live);
    assert!(session.has_active_gesture());
    assert_durable(&session, initial_rect());

    // Dropping a successor does not erase the exact source still on screen.
    let authority = session.presented_surface(SURFACE).unwrap();
    let (later, output) = paint(&mut session);
    assert_projected(&session, &before, &later, projected);
    assert!(!present(&mut session, output, Disposition::Dropped).workspace_changed());
    assert_eq!(session.presented_surface(SURFACE), Some(authority));
    assert_current_receivers(&session, &live);
    assert!(session.has_active_gesture());
    assert_eq!(session.version(), version);
    assert_durable(&session, initial_rect());

    let released = edge(&mut session, Event::ButtonReleased(Button::Primary), moved).unwrap();
    assert!(released.workspace_changed());
    assert!(!session.has_active_gesture());
    assert!(!session.has_pending_presentation());
    assert_durable(&session, projected);
    let committed = session.version();
    assert!(!settle(&mut session).workspace_changed());
    assert_eq!(session.version(), committed);
}

#[test]
fn changed_external_measurements_revoke_active_resize_and_its_old_output_proof() {
    for (bounds, minimum) in [
        (
            rect(0.0, 0.0, 800.0, 600.0),
            LogicalSize::new(64.0, 48.0).unwrap(),
        ),
        (
            rect(0.0, 0.0, 960.0, 720.0),
            LogicalSize::new(160.0, 96.0).unwrap(),
        ),
    ] {
        let (mut session, before) = fixture();
        let version = session.version();
        let press = before
            .resize
            .iter()
            .find(|(direction, _)| *direction == Direction::SouthEast)
            .unwrap()
            .1
            .center();
        let moved = offset(press, -40.0, -24.0);
        let projected = rect(200.0, 140.0, 360.0, 276.0);
        for (event, point) in [
            (Event::ButtonPressed(Button::Primary), press),
            (Event::Moved, moved),
        ] {
            assert!(
                !edge(&mut session, event, point)
                    .unwrap()
                    .workspace_changed()
            );
        }
        let (live, output) = paint(&mut session);
        assert_preview(&live, projected, true);
        assert_projected(&session, &before, &live, projected);
        assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
        assert_current_receivers(&session, &live);
        assert!(session.has_active_gesture());
        let source = session.presented_surface(SURFACE).unwrap();
        let held = session.bind_presented_receiver(&live.title).unwrap();
        let (_, old_output) = paint(&mut session);
        let old_artifact = old_output.presentation_artifact();

        // Only a source-owned recompile may continue; changed host facts may not.
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(bounds, minimum, 96.0).unwrap(),
            )
            .unwrap();
        assert!(!frame.commit().unwrap().workspace_changed());
        assert!(
            !session.has_active_gesture(),
            "external measurement changes must revoke the original resize continuation: {bounds:?}, {minimum:?}"
        );
        // Measurement changes replace the candidate, not the last displayed
        // output. They revoke the gesture without inventing a presentation.
        assert_eq!(session.presented_surface(SURFACE), Some(source));
        assert_eq!(session.version(), version);
        assert_durable(&session, initial_rect());
        for descriptor in live
            .panes
            .iter()
            .copied()
            .chain([live.title, live.close])
            .chain(live.resize.iter().map(|(_, receiver)| *receiver))
            .chain(tab_receivers(&live))
        {
            assert!(
                session.bind_presented_receiver(&descriptor).is_none(),
                "cancelling the active resize must revoke its old projected receiver: {descriptor:?}"
            );
        }
        // Holding the exact receiver before cancellation cannot reauthorize its old projection.
        let press_route = HostPointerRoute::new(source, held.center());
        let mut stale = session.begin_host_frame().unwrap();
        stale
            .submit_host_pointer(HostPointerInput::new(
                SurfacePointerId::new(2),
                Event::ButtonPressed(Button::Primary),
                press_route,
                press_route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_drag_delivery(&held),
            ))
            .unwrap();
        stale
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let rejected = stale.commit().unwrap();
        assert!(!rejected.workspace_changed());
        assert!(
            rejected
                .manipulation_feedback()
                .iter()
                .all(|feedback| !matches!(
                    feedback.status(),
                    Status::Delivered(_) | Status::Pending(_)
                ))
        );
        assert!(!session.has_active_gesture());
        assert_durable(&session, initial_rect());

        let (restored, output) = paint(&mut session);
        assert_projected(&session, &before, &restored, initial_rect());
        let restored_artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, Disposition::Presented)
            .unwrap();
        let blocked = settle(&mut session);
        assert!(!blocked.workspace_changed());
        assert!(
            blocked.surface_presentation_commits().is_empty(),
            "the older unresolved output still blocks this stream prefix"
        );
        assert!(
            session.presented_surface(SURFACE).is_none(),
            "newer presented pixels revoke old input while their prefix is blocked"
        );

        session
            .report_surface_presentation(old_output, Disposition::Presented)
            .unwrap();
        let retired = settle(&mut session);
        assert!(!retired.workspace_changed());
        let [commit] = retired.surface_presentation_commits() else {
            panic!("the now-complete stream prefix must settle once")
        };
        assert!(commit.retires(old_artifact));
        assert!(commit.presents(restored_artifact));
        assert!(!session.has_active_gesture());
        assert_durable(&session, initial_rect());

        assert_current_receivers(&session, &restored);
        assert_ne!(session.presented_surface(SURFACE), Some(source));

        // Fresh presentation cannot launder the saved pre-measurement route.
        let route = HostPointerRoute::new(source, offset(moved, -100.0, -48.0));
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([Event::Moved, Event::ButtonReleased(Button::Primary)].map(
                |event| {
                    HostPointerInput::new(
                        SurfacePointerId::new(1),
                        event,
                        route,
                        route,
                        SurfacePointerCapture::ProviderEndpoint,
                        SurfacePointerReceiverFacts::unknown(),
                    )
                },
            ))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(!frame.commit().unwrap().workspace_changed());
        let (idle, output) = paint(&mut session);
        assert_projected(&session, &before, &idle, initial_rect());
        assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
        assert!(!session.has_active_gesture());
        assert_eq!(session.version(), version);
        assert_durable(&session, initial_rect());
    }
}

#[test]
fn frozen_clear_hover_batch_docks_behind_complete_source_at_release_commit() {
    for (blocked_release, disposition) in [
        (false, Disposition::Presented),
        (false, Disposition::Dropped),
        (true, Disposition::Presented),
    ] {
        let (mut session, before) = fixture();
        let version = session.version();
        let surface = session.presented_surface(SURFACE).unwrap();
        let title = session.bind_presented_receiver(&before.title).unwrap();
        let press = title.center();
        let (point, target_visual, target_draw, target_hit) = {
            let mut inspect = session.begin_host_frame().unwrap();
            let plan = inspect.paint_plan(SURFACE).unwrap().unwrap();
            let mut target = None;
            plan.visit_surface_program(|step| {
                if let Step::DropGuide { structural, .. } = step
                    && structural.root() == MAIN_ROOT
                    && structural.scope() == GuideScope::Inner
                {
                    for button in structural.targets() {
                        if button.direction() == DropDirection::Left {
                            let draw = button.draw_bounds();
                            let center = LogicalPoint::new(
                                draw.x() + draw.width() * 0.5,
                                draw.y() + draw.height() * 0.5,
                            )
                            .unwrap();
                            assert!(
                                target
                                    .replace((
                                        center,
                                        button.visual_id(),
                                        draw,
                                        button.hit_bounds()
                                    ))
                                    .is_none()
                            );
                        }
                    }
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            target.expect("joint placement must expose the main pane's explicit inner-left guide")
        };
        assert!(target_draw.width() > 0.0 && target_draw.height() > 0.0);
        assert!(target_hit.contains(point));
        assert!(
            !initial_rect().contains(point),
            "joint placement must expose this guide beside the initial source"
        );
        assert!(
            (point.x() - press.x())
                .abs()
                .max((point.y() - press.y()).abs())
                > 12.0
        );
        assert!(
            before.renderer.records.contains(&OwnedRecord::Geometry {
                visual: target_visual,
                bounds: target_draw,
                paint: Paint::Visible,
            }),
            "the aim point must come from the actual visible guide drawing"
        );
        let target = session
            .host_pointer_hit(SURFACE, point)
            .unwrap()
            .hover()
            .expect("the visible inner guide must own the actual hover winner");
        assert_eq!(target.descriptor().role(), Role::DropTarget);
        assert_eq!(target.bounds(), target_hit);
        assert_eq!(
            session.bind_presented_receiver(&target.descriptor()),
            Some(target)
        );
        let clear = SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface);
        let click = session
            .host_pointer_hit(SURFACE, press)
            .unwrap()
            .delivery()
            .unwrap();
        let press_facts = SurfacePointerReceiverFacts::delivery(&click)
            .with_drag_delivery(&title)
            .with_clear_hover_lane(&surface);
        let release_facts = if blocked_release {
            clear.with_blocked_hover(&surface)
        } else {
            clear
        };
        let inputs = [
            (Event::ButtonPressed(Button::Primary), press, press_facts),
            (Event::Moved, point, clear),
            (Event::ButtonReleased(Button::Primary), point, release_facts),
        ]
        .map(|(event, position, facts)| {
            let route = HostPointerRoute::new(surface, position);
            HostPointerInput::new(
                SurfacePointerId::new(1),
                event,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                facts,
            )
        });
        // Inspect this exact frozen prefix without publishing it. The complete
        // source follows the pointer over the guide, while source suppression
        // keeps the explicit background target selected through the projection.
        {
            let mut motion = session.begin_host_frame().unwrap();
            motion
                .submit_host_pointer_batch(inputs[..2].iter().copied())
                .unwrap();
            let plan = motion.paint_plan(SURFACE).unwrap().unwrap();
            let mut projected_source = None;
            let mut active_targets = Vec::new();
            plan.visit_surface_program(|step| {
                match step {
                    Step::DragDecoration(source) if source.source_is_projected() => {
                        assert_eq!(source.kind(), DragSourceKind::Root);
                        assert!(
                            projected_source
                                .replace(source.projected_root_bounds().unwrap())
                                .is_none()
                        );
                    }
                    Step::DropGuide {
                        affordance: Some(cluster),
                        ..
                    } => {
                        active_targets.extend(
                            cluster
                                .targets()
                                .filter(|target| target.is_active())
                                .map(|target| target.visual_id()),
                        );
                    }
                    _ => {}
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            assert!(
                projected_source
                    .expect("the complete split-root must project its live content")
                    .contains(point),
                "the moved source must cover the explicit target point"
            );
            assert_eq!(
                active_targets,
                [target_visual],
                "source suppression must preserve the explicit inner-left winner"
            );
            let (recording, _) = RecordingRenderer::execute(plan).unwrap();
            for item in [LEFT, RIGHT] {
                assert_eq!(recording.records.iter().filter(|record| matches!(record,
                    OwnedRecord::PaneContent { item: Some(current), paint: Paint::Visible, .. } if *current == item
                )).count(), 1, "the projected source must retain both split panes");
            }
            assert!(
                recording.records.iter().any(|record| matches!(
                    record,
                    OwnedRecord::Preview(DockspacePreviewVisual::Dock {
                        surface: SURFACE,
                        ..
                    })
                )),
                "the explicit guide must resolve a dock preview before release"
            );
        }
        assert_eq!(session.version(), version);
        assert!(!session.has_active_gesture());
        assert_durable(&session, initial_rect());
        let mut frame = session.begin_host_frame().unwrap();
        frame.submit_host_pointer_batch(inputs).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let released = frame.commit().unwrap();
        let accepted = !blocked_release;
        assert_eq!(released.workspace_changed(), accepted, "{released:?}");
        if accepted {
            assert!(
                matches!(released.manipulation_feedback(), [terminal] if terminal.status() == Status::Delivered(Delivery::Dock)),
                "frozen clear-hover release must dock the complete source: blocked={blocked_release}, disposition={disposition:?}, press={press:?}, release={point:?}; feedback={:#?}; report={released:#?}",
                released.manipulation_feedback(),
            );
            assert!(session.view().contained(FLOATING).is_none());
            assert_eq!(
                session.view().surface(SURFACE).unwrap().contained_count(),
                0
            );
            for item in [LEFT, RIGHT, BACKGROUND] {
                let view = session.view().item(item).unwrap();
                assert_eq!(view.surface(), SURFACE);
                assert_eq!(view.root(), MAIN_ROOT);
                assert_eq!(view.contained(), None);
                assert_eq!(view.tabs().items(), &[item]);
            }
            assert!(session.bind_presented_receiver(&before.title).is_none());
        } else {
            assert!(
                released
                    .manipulation_feedback()
                    .iter()
                    .all(|feedback| !matches!(
                        feedback.status(),
                        Status::Delivered(_) | Status::Pending(_)
                    ))
            );
            assert_durable(&session, initial_rect());
        }
        assert_eq!(session.version() != version, accepted);
        assert!(!session.has_active_gesture());
        let terminal_version = session.version();
        // Recompile and encode in one update after the semantic release invalidated the scene.
        let mut frame = session.begin_host_frame().unwrap();
        if frame.paint_plan(SURFACE).unwrap().is_none() {
            frame.measure_surface(SURFACE, metrics()).unwrap();
        }
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
        assert!(recording.records.iter().all(|record| !matches!(
            record,
            OwnedRecord::Preview(_) | OwnedRecord::TransformPreview(..)
        )));
        frame.confirm_surface_painted(receipt).unwrap();
        let mut painted = frame.commit().unwrap();
        assert!(!painted.workspace_changed());
        assert!(painted.manipulation_feedback().is_empty());
        let output = painted.take_painted_outputs().pop().unwrap();
        let artifact = output.presentation_artifact();
        assert!(
            matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic)) if output.matches_semantic_output(*semantic))
        );
        session
            .report_surface_presentation(output, disposition)
            .unwrap();
        let settled = settle(&mut session);
        assert!(matches!(settled.surface_presentation_commits(), [ack] if ack.settles(artifact)));
        assert!(!settled.workspace_changed());
        assert!(settled.manipulation_feedback().is_empty());
        assert_eq!(session.version(), terminal_version);
        assert_eq!(session.view().contained(FLOATING).is_none(), accepted);
        assert!(!settle(&mut session).workspace_changed());
        assert_eq!(session.version(), terminal_version);
    }
}

#[test]
fn rejected_non_source_contained_target_blocks_a_legal_background_target_and_fallback() {
    const SOURCE: ItemId = ItemId::new(4);
    let (mut session, _) = fixture();
    let mut target_rule = DockTargetRule::new();
    target_rule.set_allowed_payloads([]);
    let mut policy = DockPolicy::default();
    policy.set_target_rule(DockTargetRuleKey::Item(RIGHT), target_rule);
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .open_item_current(
            SOURCE,
            DockPlacement::InnerEdge {
                anchor: DockAnchor::Item(BACKGROUND),
                edge: DockEdge::Left,
                fraction: DockFraction::new(0.2).unwrap(),
            },
        )
        .unwrap();
    frame.replace_policy(policy).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().unwrap().workspace_changed());
    measure(&mut session);
    let (before, output) = paint(&mut session);
    assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
    let version = session.version();

    let mut inspect = session.begin_host_frame().unwrap();
    let press = inspect
        .paint_plan(SURFACE)
        .unwrap()
        .unwrap()
        .tab_receiver(SOURCE)
        .unwrap()
        .center();
    drop(inspect);
    assert_eq!(session.view().item(SOURCE).unwrap().root(), MAIN_ROOT);
    assert_ne!(session.view().item(SOURCE).unwrap().root(), ROOT);
    assert!(!initial_rect().contains(press));
    let front = before
        .guides
        .iter()
        .filter(|guide| {
            guide.root == ROOT
                && guide.scope == GuideScope::Inner
                && guide.direction == DropDirection::Center
        })
        .max_by(|left, right| left.hit.x().total_cmp(&right.hit.x()))
        .unwrap();
    let front_content = before
        .renderer
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::PaneContent {
                item: Some(RIGHT),
                bounds,
                ..
            } => Some(*bounds),
            _ => None,
        })
        .unwrap();
    let background = before
        .renderer
        .records
        .iter()
        .find_map(|record| match record {
            OwnedRecord::PaneContent {
                item: Some(BACKGROUND),
                bounds,
                ..
            } => Some(*bounds),
            _ => None,
        })
        .unwrap();
    let fraction = session.presentation_config().magnetic_center_fraction();
    let inset = (1.0 - fraction) * 0.5;
    let background_center = rect(
        background.x() + background.width() * inset,
        background.y() + background.height() * inset,
        background.width() * fraction,
        background.height() * fraction,
    );
    let samples = [0.35, 0.45, 0.55, 0.65, 0.75, 0.85];
    let point = samples
        .into_iter()
        .flat_map(|x| {
            samples.into_iter().map(move |y| {
                LogicalPoint::new(
                    front_content.x() + front_content.width() * x,
                    front_content.y() + front_content.height() * y,
                )
                .unwrap()
            })
        })
        .find(|point| {
            initial_rect().contains(*point)
                && background_center.contains(*point)
                && before
                    .guides
                    .iter()
                    .filter(|guide| guide.root == ROOT)
                    .all(|guide| !guide.hit.contains(*point))
        })
        .expect(
            "the rejected front pane must expose a bare point over the background magnetic field",
        );
    let clear_point = samples
        .into_iter()
        .flat_map(|x| {
            samples.into_iter().map(move |y| {
                LogicalPoint::new(
                    background.x() + background.width() * x,
                    background.y() + background.height() * y,
                )
                .unwrap()
            })
        })
        .find(|point| {
            !initial_rect().contains(*point)
                && background_center.contains(*point)
                && before
                    .guides
                    .iter()
                    .all(|guide| !guide.hit.contains(*point))
        })
        .expect("the background center field must also expose an unobscured bare control point");
    let back = before
        .guides
        .iter()
        .find(|guide| {
            guide.root == MAIN_ROOT
                && guide.scope == GuideScope::Inner
                && guide.direction == DropDirection::Center
                && guide.preview.contains(clear_point)
        })
        .unwrap();

    assert!(
        !edge(&mut session, Event::ButtonPressed(Button::Primary), press)
            .unwrap()
            .workspace_changed()
    );
    for (event, position, occluded) in [
        (Event::Moved, clear_point, false),
        (Event::Moved, point, true),
        (Event::ButtonReleased(Button::Primary), point, true),
    ] {
        let releasing = matches!(event, Event::ButtonReleased(_));
        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, position);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                event,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
            )])
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        assert!(!frame.commit().unwrap().workspace_changed());
        assert_eq!(session.has_active_gesture(), !releasing);

        if occluded && !releasing {
            let mut inspect = session.begin_host_frame().unwrap();
            let plan = inspect.paint_plan(SURFACE).unwrap().unwrap();
            let mut rejected = false;
            plan.visit_surface_program(|step| {
                if let Step::DropGuide {
                    affordance: Some(cluster),
                    ..
                } = step
                {
                    for target in cluster.targets().filter(|target| target.is_active()) {
                        assert_eq!(cluster.root(), ROOT);
                        assert_eq!(target.visual_id(), front.visual);
                        assert_eq!(target.eligibility(), Eligibility::Rejected);
                        rejected = true;
                    }
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            assert!(
                rejected,
                "the front candidate is rejected, not absent or source-suppressed"
            );
        }
        let (recorded, output) = paint(&mut session);
        if occluded {
            assert!(recorded.renderer.records.iter().all(|record| !matches!(
                record,
                OwnedRecord::Preview(_) | OwnedRecord::TransformPreview(..)
            )));
        } else {
            assert_eq!(
                recorded
                    .guides
                    .iter()
                    .filter(|guide| guide.active)
                    .map(|guide| guide.visual)
                    .collect::<Vec<_>>(),
                [back.visual]
            );
            assert!(
                recorded.renderer.records.iter().any(|record| matches!(
                    record,
                    OwnedRecord::Preview(DockspacePreviewVisual::Dock { surface: SURFACE, rect })
                        if rect.contains(clear_point)
                )),
                "the exact background candidate previews at the unobscured control point"
            );
        }
        assert!(!present(&mut session, output, Disposition::Presented).workspace_changed());
        assert_eq!(session.version(), version);
        assert_durable(&session, initial_rect());
        let source_view = session.view().item(SOURCE).unwrap();
        assert_eq!(source_view.surface(), SURFACE);
        assert_eq!(source_view.root(), MAIN_ROOT);
        assert!(source_view.contained().is_none());
        assert_eq!(source_view.tabs().items(), &[SOURCE]);
        assert_eq!(
            session.view().item(BACKGROUND).unwrap().tabs().items(),
            &[BACKGROUND]
        );
        assert_eq!(session.view().surfaces().count(), 1);
    }
    assert!(!settle(&mut session).workspace_changed());
    assert_eq!(session.version(), version);
    assert!(!session.has_active_gesture());
}

#[test]
fn complete_contained_group_magnetic_hysteresis_survives_presented_source_only_projections() {
    fn clear_edge(
        session: &mut DockspaceSession,
        event: Event,
        position: LogicalPoint,
    ) -> HostFrameReport {
        // Only the trusted host clears framework hover; the core still resolves the target.
        let surface = session.presented_surface(SURFACE).unwrap();
        let route = HostPointerRoute::new(surface, position);
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .submit_host_pointer_batch([HostPointerInput::new(
                SurfacePointerId::new(1),
                event,
                route,
                route,
                SurfacePointerCapture::ProviderEndpoint,
                SurfacePointerReceiverFacts::unknown().with_clear_hover_lane(&surface),
            )])
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    }

    fn present_group(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
        let version = session.version();
        let artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, Disposition::Presented)
            .unwrap();
        assert_eq!(session.version(), version);
        let report = settle(session);
        assert!(!report.workspace_changed());
        assert_eq!(session.version(), version);
        let [ack] = report.surface_presentation_commits() else {
            panic!("one exact complete-group output must be acknowledged: {report:?}")
        };
        assert!(ack.presents(artifact));
        assert!(
            session
                .presented_surface(SURFACE)
                .unwrap()
                .matches_presentation_artifact(artifact)
        );
        assert_eq!(
            session.view().contained(FLOATING).unwrap().rect(),
            initial_rect()
        );
        assert_eq!(
            session
                .view()
                .root(ROOT)
                .unwrap()
                .content()
                .unwrap()
                .tabs()
                .unwrap()
                .items(),
            &[LEFT, RIGHT]
        );
    }

    for outside_envelope in [false, true] {
        // A whole tabs root gives the public group grip DockContent intent;
        // the existing split-root title fixture intentionally remains MoveWindow.
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(MAIN_ROOT, DockspaceNode::central_tabs([BACKGROUND])),
        )
        .with_contained(DockspaceContainedLayout::new(
            FLOATING,
            DockspaceRootLayout::new(ROOT, DockspaceNode::tabs([LEFT, RIGHT])),
            initial_rect(),
        ))])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        session.enable_host_pointer().unwrap();
        measure(&mut session);
        let (before, output) = paint(&mut session);
        present_group(&mut session, output);
        let version = session.version();
        let [grip] = before.group_grips.as_slice() else {
            panic!("the complete tabs root must expose exactly one public group grip")
        };
        assert_eq!(grip.role(), Role::TabGroupGrip);
        let bound = session.bind_presented_receiver(grip).unwrap();
        let press = bound.center();
        assert_eq!(
            session
                .host_pointer_hit(SURFACE, press)
                .unwrap()
                .drag_delivery(),
            Some(bound)
        );
        assert!(
            !edge(&mut session, Event::ButtonPressed(Button::Primary), press)
                .unwrap()
                .workspace_changed()
        );

        let content = before
            .renderer
            .records
            .iter()
            .find_map(|record| match record {
                OwnedRecord::PaneContent {
                    item: Some(BACKGROUND),
                    bounds,
                    ..
                } => Some(*bounds),
                _ => None,
            })
            .unwrap();
        let config = session.presentation_config();
        let inset = (1.0 - config.magnetic_center_fraction()) * 0.5;
        let boundary = LogicalPoint::new(
            content.x() + content.width() * inset,
            content.y() + content.height() * inset + 10.0,
        )
        .unwrap();
        // These are the center-boundary fixtures from drop_guide_interaction.rs,
        // not a second target resolver: 7.5 stays inside the 8-point release margin.
        assert_eq!(config.target_release_hysteresis(), 8.0);
        let exit = LogicalPoint::new(
            content.max().x() - 10.0,
            content.y() + content.height() * 0.5,
        )
        .unwrap();
        assert!(!initial_rect().contains(exit));
        let acquired = offset(boundary, -0.25, 0.0);
        assert!(initial_rect().contains(acquired));
        let mut moves = vec![
            (exit, GuideScope::Outer, DropDirection::Right),
            (acquired, GuideScope::Outer, DropDirection::Left),
            (
                offset(boundary, 7.5, 0.0),
                GuideScope::Outer,
                DropDirection::Left,
            ),
            (
                offset(boundary, 7.0, 0.0),
                GuideScope::Outer,
                DropDirection::Left,
            ),
        ];
        if outside_envelope {
            moves.push((
                offset(boundary, 8.5, 0.0),
                GuideScope::Inner,
                DropDirection::Center,
            ));
        }
        let release = moves.last().unwrap().0;
        let mut previous_projection = initial_rect();
        for (position, scope, direction) in moves {
            assert!(!clear_edge(&mut session, Event::Moved, position).workspace_changed());
            assert!(session.has_active_gesture());
            assert_eq!(session.version(), version);
            let (recorded, output) = paint(&mut session);
            assert!(
                recorded
                    .guides
                    .iter()
                    .all(|guide| !guide.hit.contains(position)),
                "magnetic acquisition and jitter must never hit a guide button"
            );
            let expected = before
                .guides
                .iter()
                .find(|guide| {
                    guide.root == MAIN_ROOT && guide.scope == scope && guide.direction == direction
                })
                .unwrap();
            assert_eq!(
                recorded
                    .guides
                    .iter()
                    .filter(|guide| guide.active)
                    .map(|guide| guide.visual)
                    .collect::<Vec<_>>(),
                [expected.visual],
                "outside envelope: {outside_envelope}, point: {position:?}"
            );
            let previews = recorded
                .renderer
                .records
                .iter()
                .filter_map(|record| match record {
                    OwnedRecord::Preview(visual) => Some(*visual),
                    _ => None,
                })
                .collect::<Vec<_>>();
            // The measured 960 x 720 root splits into 479.5-point branches
            // around its one-point splitter, unlike the static guide preview.
            let expected_preview = match direction {
                DropDirection::Left => rect(0.0, 0.0, 479.5, 720.0),
                DropDirection::Right => rect(480.5, 0.0, 479.5, 720.0),
                DropDirection::Center => expected.preview,
                _ => unreachable!("this journey only crosses the left-center boundary"),
            };
            assert_eq!(
                previews,
                [DockspacePreviewVisual::Dock {
                    surface: SURFACE,
                    rect: expected_preview
                }]
            );
            let (kind, projected) = recorded
                .projected_source
                .expect("the exact active drag must compile its complete source, not a ghost");
            assert_eq!(kind, DragSourceKind::TabGroup);
            assert_ne!(projected, previous_projection);
            previous_projection = projected;
            assert_eq!(recorded.panes.len(), 1);
            assert_eq!(tab_receivers(&recorded).len(), 2);
            assert_eq!(recorded.group_grips.len(), 1);
            for kind in [
                Kind::Contained,
                Kind::Pane,
                Kind::TabBar,
                Kind::Tab,
                Kind::PresentationMenuAnchor,
            ] {
                let original = geometry(&before, kind);
                let current = geometry(&recorded, kind);
                assert_eq!(current.len(), original.len());
                for ((visual, bounds, _), actual) in original.into_iter().zip(current) {
                    assert_eq!(
                        actual,
                        (
                            visual,
                            rect(
                                projected.x() + (bounds.x() - initial_rect().x()),
                                projected.y() + (bounds.y() - initial_rect().y()),
                                bounds.width(),
                                bounds.height(),
                            ),
                            Paint::Visible
                        )
                    );
                }
            }
            assert!(recorded.renderer.records.iter().any(|record| matches!(record,
                OwnedRecord::PaneContent { item: Some(BACKGROUND), bounds, paint: Paint::Visible, .. } if *bounds == content
            )), "the target layout must not change with the source-only projection");
            assert_eq!(session.version(), version);
            present_group(&mut session, output);
            assert_current_receivers(&session, &recorded);
            assert!(
                session
                    .bind_presented_receiver(&recorded.group_grips[0])
                    .is_some()
            );
        }

        let delivered = clear_edge(
            &mut session,
            Event::ButtonReleased(Button::Primary),
            release,
        );
        assert!(
            delivered.workspace_changed(),
            "a Presented target must deliver once at release: {outside_envelope}: {delivered:?}"
        );
        assert_eq!(delivered.before(), version);
        assert_ne!(delivered.after(), version);
        assert!(!session.has_active_gesture());
        assert!(session.view().contained(FLOATING).is_none());
        assert!(session.view().root(ROOT).is_none());
        assert_eq!(
            session.view().surface(SURFACE).unwrap().contained_count(),
            0
        );
        let content = session.view().root(MAIN_ROOT).unwrap().content().unwrap();
        if outside_envelope {
            assert_eq!(content.tabs().unwrap().items(), &[BACKGROUND, LEFT, RIGHT]);
        } else {
            let split = content
                .split()
                .expect("the retained left edge must split, not merge at the center jitter point");
            assert_eq!(split.axis(), DockspaceAxis::Horizontal);
            assert_eq!(
                split
                    .children()
                    .map(|child| child.tabs().unwrap().items().to_vec())
                    .collect::<Vec<_>>(),
                [vec![LEFT, RIGHT], vec![BACKGROUND]]
            );
        }
        for item in [LEFT, RIGHT, BACKGROUND] {
            let view = session.view().item(item).unwrap();
            assert_eq!(view.root(), MAIN_ROOT);
            assert_eq!(view.surface(), SURFACE);
            assert_eq!(view.contained(), None);
        }
        let terminal_version = session.version();
        for _ in 0..2 {
            assert!(!settle(&mut session).workspace_changed());
            assert_eq!(
                session.version(),
                terminal_version,
                "no duplicate release mutation"
            );
        }
    }
}
