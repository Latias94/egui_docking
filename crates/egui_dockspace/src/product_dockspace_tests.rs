use std::cell::Cell;

use dockspace::geometry::LogicalPoint;
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::runtime::{DockspaceDropDirection, DockspaceReceiverRole};
use dockspace_host_conformance::scenario::{
    CadenceProfile, ItemAlias, JourneyId, MAGNETIC_DOCKING_BASINS, MagneticDockBasin,
    MagneticDockCase, ScenarioAdmission, ScenarioCapability, ScenarioId,
};
use egui::accesskit::Role;
use egui::{Context, Event, PointerButton, Pos2, RawInput, Rect, Ui, vec2};

use super::Dockspace;
use crate::pane::PaneView;
use crate::product_render::RegisteredDropTargetObservation;

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const FIRST: ItemId = ItemId::new(1);
const SECOND: ItemId = ItemId::new(2);
const THIRD: ItemId = ItemId::new(3);
const FOURTH: ItemId = ItemId::new(4);
const FIFTH: ItemId = ItemId::new(5);
const SIXTH: ItemId = ItemId::new(6);

struct MagneticPanes {
    target_rect: Cell<Option<Rect>>,
}

impl MagneticPanes {
    fn new() -> Self {
        Self {
            target_rect: Cell::new(None),
        }
    }
}

impl PaneView for MagneticPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            FIRST => Some("First".into()),
            SECOND => Some("Second".into()),
            THIRD => Some("Third".into()),
            FOURTH => Some("Fourth".into()),
            FIFTH => Some("Fifth".into()),
            SIXTH => Some("Sixth".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if item == FIRST {
            self.target_rect.set(Some(ui.available_rect_before_wrap()));
        }
        ui.label(format!("Pane {}", item.get()));
    }
}

struct FrameOutput {
    output: egui::FullOutput,
    drop_targets: Vec<RegisteredDropTargetObservation>,
}

fn magnetic_layout() -> DockspaceLayout {
    let middle = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [
            DockspaceNode::tabs([SECOND, THIRD]),
            DockspaceNode::tabs([FIRST]),
            DockspaceNode::tabs([FIFTH]),
        ],
    )
    .expect("the magnetic semantic row is valid");
    let root = DockspaceNode::equal_split(
        DockspaceAxis::Vertical,
        [
            DockspaceNode::tabs([FOURTH]),
            middle,
            DockspaceNode::tabs([SIXTH]),
        ],
    )
    .expect("the magnetic semantic column is valid");
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, root),
    )])
    .expect("the magnetic semantic product layout is valid")
}

fn input(events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, vec2(800.0, 600.0))),
        events,
        ..RawInput::default()
    }
}

fn pointer_button(position: Pos2, pressed: bool) -> Event {
    Event::PointerButton {
        pos: position,
        button: PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    }
}

fn run_frame(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    events: Vec<Event>,
) -> FrameOutput {
    let mut drop_targets = Vec::new();
    let output = crate::run_ui(context, input(events), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, panes)
            .expect("the product frame advances");
        let registered = dockspace.registered_drop_target_observations();
        if !registered.is_empty() {
            // Preserve the actual receivers used by the release pass even if
            // a later pass already paints the committed, guide-free workspace.
            drop_targets = registered.to_vec();
        }
    });
    let (mut output, receipt) = output.into_parts();
    receipt.report(crate::SurfacePresentationResult::Presented);
    output.textures_delta.clear();
    FrameOutput {
        output,
        drop_targets,
    }
}

#[test]
fn ordered_pointer_transport_preserves_explicit_batch_clock_and_unknown_samples() {
    use dockspace::runtime::{
        DockspaceRuntimeErrorKind, HostPointerInput, HostPointerRoute, PointerDeviceKind,
        PointerEventMetadata, SurfacePointerCapture, SurfacePointerEvent, SurfacePointerId,
        SurfacePointerReceiverFacts,
    };

    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let mut dockspace = Dockspace::builder("ordered-batch-clock", magnetic_layout())
        .build()
        .unwrap();
    let mut panes = MagneticPanes::new();
    for _ in 0..2 {
        run_frame(&context, &mut dockspace, &mut panes, Vec::new());
    }
    let version = dockspace.version();
    for time in [Some(10.0), None, Some(10.1)] {
        let mut raw = input(vec![Event::PointerMoved(egui::pos2(400.0, 300.0))]);
        raw.time = time;
        let mut result = None;
        let output = crate::run_ui(&context, raw, |ui| {
            result = Some(
                dockspace
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .map(|_| ())
                    .map_err(|error| error.kind()),
            );
        });
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(crate::SurfacePresentationResult::Presented);
        assert_eq!(result.unwrap(), Ok(()), "raw time: {time:?}");
        assert_eq!(dockspace.version(), version);
    }

    // egui rejects a backwards frame clock before calling application code.
    // Submit the backwards probe through core instead: it must see the exact
    // clock retained by the preceding ordinary product runs.
    let mut frame = dockspace.session.begin_host_frame().unwrap();
    let error = frame
        .submit_host_pointer(
            HostPointerInput::new(
                SurfacePointerId::new(1),
                SurfacePointerEvent::Moved,
                HostPointerRoute::Unknown,
                HostPointerRoute::Unknown,
                SurfacePointerCapture::None,
                SurfacePointerReceiverFacts::unknown(),
            )
            .with_metadata(PointerEventMetadata::new(
                PointerDeviceKind::Unknown,
                std::time::Duration::from_millis(10_050),
            )),
        )
        .unwrap_err();
    assert_eq!(error.kind(), DockspaceRuntimeErrorKind::HostProtocol);
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "the fixture constrains AccessKit bounds to an 800x600 egui viewport"
)]
fn try_tab_rect(output: &egui::FullOutput, label: &str) -> Option<Rect> {
    let tree = output.platform_output.accesskit_update.as_ref()?;
    tree.nodes.iter().find_map(|(_, node)| {
        (node.role() == Role::Tab && node.label() == Some(label)).then(|| {
            let bounds = node.bounds().expect("the tab exposes exact bounds");
            Rect::from_min_max(
                Pos2::new(bounds.x0 as f32, bounds.y0 as f32),
                Pos2::new(bounds.x1 as f32, bounds.y1 as f32),
            )
        })
    })
}

#[allow(
    clippy::cast_possible_truncation,
    reason = "normalized samples are finite fractions in the open interval zero to one"
)]
fn normalized_pane_point(rect: Rect, case: MagneticDockCase) -> Pos2 {
    let sample = case.sample();
    Pos2::new(
        egui::lerp(rect.x_range(), sample.x() as f32),
        egui::lerp(rect.y_range(), sample.y() as f32),
    )
}

fn tangent_pane_point(point: Pos2, basin: MagneticDockBasin, delta: f32) -> Pos2 {
    match basin {
        MagneticDockBasin::Center | MagneticDockBasin::North | MagneticDockBasin::South => {
            point + vec2(delta, 0.0)
        }
        MagneticDockBasin::East | MagneticDockBasin::West => point + vec2(0.0, delta),
    }
}

fn assert_registered_non_guide_point(frame: &FrameOutput, point: Pos2, case: MagneticDockCase) {
    assert_eq!(
        frame.drop_targets.len(),
        9,
        "the nested L03 target must register its inner five-way cluster and root outer four-way cluster: case={case:?} receivers={:?}",
        frame.drop_targets,
    );
    assert!(
        frame
            .drop_targets
            .iter()
            .all(|observation| observation.receiver.role() == DockspaceReceiverRole::DropTarget),
        "the test receipt must contain drop targets only: case={case:?} receivers={:?}",
        frame.drop_targets,
    );
    assert!(
        frame.drop_targets.iter().enumerate().all(|(index, left)| {
            frame.drop_targets[index + 1..]
                .iter()
                .all(|right| left.widget_id != right.widget_id && left.visual_id != right.visual_id)
        }),
        "every registered guide receiver must retain distinct renderer identities: case={case:?} receivers={:?}",
        frame.drop_targets,
    );
    assert!(
        frame
            .drop_targets
            .windows(2)
            .all(|pair| pair[0].layer_id == pair[1].layer_id),
        "the L03 guide clusters must remain on one egui interaction layer: case={case:?} receivers={:?}",
        frame.drop_targets,
    );
    for (direction, expected) in [
        (DockspaceDropDirection::Center, 1),
        (DockspaceDropDirection::Left, 2),
        (DockspaceDropDirection::Right, 2),
        (DockspaceDropDirection::Top, 2),
        (DockspaceDropDirection::Bottom, 2),
    ] {
        assert_eq!(
            frame
                .drop_targets
                .iter()
                .filter(|observation| observation.direction == direction)
                .count(),
            expected,
            "the registered guide direction roster must match the core program: case={case:?} direction={direction:?} receivers={:?}",
            frame.drop_targets,
        );
    }
    let logical = LogicalPoint::new(f64::from(point.x), f64::from(point.y))
        .expect("the normalized product point is finite");
    assert!(
        frame
            .drop_targets
            .iter()
            .all(|observation| !observation.receiver.bounds().contains(logical)),
        "the shared ordinary L03 point must remain outside every renderer-registered guide receiver: case={case:?} point={point:?} receivers={:?}",
        frame.drop_targets,
    );
}

fn settle_until<T>(budget: usize, mut observe: impl FnMut() -> Option<T>) -> T {
    for _ in 0..budget {
        if let Some(value) = observe() {
            return value;
        }
    }
    panic!("causal settlement exceeded {budget} observations");
}

fn settle_drop_target_receipt(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    mut initial: Option<FrameOutput>,
    point: Pos2,
) -> FrameOutput {
    settle_until(8, || {
        let frame = initial.take().unwrap_or_else(|| {
            run_frame(context, dockspace, panes, vec![Event::PointerMoved(point)])
        });
        (!frame.drop_targets.is_empty()).then_some(frame)
    })
}

fn observe_supported_cadence(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut MagneticPanes,
    target: Pos2,
    case: MagneticDockCase,
) {
    match case.cadence() {
        CadenceProfile::Stepwise => {
            let initial = run_frame(context, dockspace, panes, vec![Event::PointerMoved(target)]);
            let frame =
                settle_drop_target_receipt(context, dockspace, panes, Some(initial), target);
            assert_registered_non_guide_point(&frame, target, case);
        }
        CadenceProfile::Hesitant => {
            for point in [
                target,
                tangent_pane_point(target, case.basin(), 3.0),
                tangent_pane_point(target, case.basin(), -3.0),
            ] {
                let initial =
                    run_frame(context, dockspace, panes, vec![Event::PointerMoved(point)]);
                let frame =
                    settle_drop_target_receipt(context, dockspace, panes, Some(initial), point);
                assert_registered_non_guide_point(&frame, point, case);
            }
        }
        CadenceProfile::ReleaseWithFinalMove => {
            let acquired = tangent_pane_point(target, case.basin(), -4.0);
            let initial = run_frame(
                context,
                dockspace,
                panes,
                vec![Event::PointerMoved(acquired)],
            );
            let acquired_frame =
                settle_drop_target_receipt(context, dockspace, panes, Some(initial), acquired);
            assert_registered_non_guide_point(&acquired_frame, acquired, case);
            let before_release = dockspace.version();
            let terminal = run_frame(
                context,
                dockspace,
                panes,
                vec![Event::PointerMoved(target), pointer_button(target, false)],
            );
            if terminal.drop_targets.is_empty() {
                assert_ne!(
                    dockspace.version(),
                    before_release,
                    "guide-free release output requires an already committed exact preview"
                );
                let committed = dockspace.version();
                let settled = run_frame(context, dockspace, panes, Vec::new());
                assert_eq!(
                    dockspace.version(),
                    committed,
                    "an immediately committed release must not commit again during settlement"
                );
                assert!(settled.drop_targets.is_empty());
            } else {
                assert_registered_non_guide_point(&terminal, target, case);
            }
        }
        cadence => panic!("unsupported shared L03 cadence: {cadence:?}"),
    }
}

fn run_supported_case(case: MagneticDockCase) {
    assert_eq!(case.source(), ItemAlias::Secondary);
    assert_eq!(case.target(), ItemAlias::Primary);
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder(
        (
            "registered-magnetic-receivers",
            case.basin(),
            case.cadence(),
        ),
        magnetic_layout(),
    )
    .build()
    .expect("the shared magnetic product facade initializes");
    let mut panes = MagneticPanes::new();

    let (source, target_rect) = settle_until(8, || {
        let frame = run_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = try_tab_rect(&frame.output, "Second").map(|rect| rect.center());
        let target_rect = panes.target_rect.get();
        source.zip(target_rect.filter(Rect::is_positive))
    });
    let target = normalized_pane_point(target_rect, case);
    let threshold = source + vec2(12.0, 8.0);
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(threshold)],
    );

    observe_supported_cadence(&context, &mut dockspace, &mut panes, target, case);
}

#[test]
fn shared_l03_supported_points_use_renderer_registered_magnetic_receivers() {
    assert_eq!(
        MAGNETIC_DOCKING_BASINS.id(),
        ScenarioId::MagneticDockingBasins,
    );
    assert_eq!(MAGNETIC_DOCKING_BASINS.journey(), JourneyId::L03);
    assert_eq!(
        MAGNETIC_DOCKING_BASINS.admission(&[
            ScenarioCapability::LocalDirectManipulation,
            ScenarioCapability::SemanticDockBasins,
            ScenarioCapability::PresentedCandidate,
        ]),
        ScenarioAdmission::Ready,
    );

    for case in MAGNETIC_DOCKING_BASINS.cases().filter(|case| {
        matches!(
            case.cadence(),
            CadenceProfile::Stepwise
                | CadenceProfile::Hesitant
                | CadenceProfile::ReleaseWithFinalMove
        )
    }) {
        run_supported_case(case);
    }
}

#[test]
fn pane_callback_cannot_authorize_deleted_chrome_or_cross_layer_pixels() {
    struct CorruptingPane {
        cross_layer: bool,
        writes: usize,
    }

    impl PaneView for CorruptingPane {
        fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
            Some("Corrupting pane".into())
        }

        fn ui(&mut self, _item: ItemId, ui: &mut Ui) {
            if self.cross_layer {
                ui.ctx()
                    .layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::unique("unregistered-pane-layer"),
                    ))
                    .circle_filled(ui.min_rect().center(), 5.0, egui::Color32::RED);
            } else {
                let layer = ui.layer_id();
                ui.ctx().graphics_mut(|graphics| {
                    let list = graphics.get_mut(layer).expect("the surface layer exists");
                    let index = list
                        .all_entries()
                        .enumerate()
                        .filter_map(|(index, shape)| {
                            matches!(shape.shape, egui::Shape::Rect(_)).then_some(index)
                        })
                        .last()
                        .expect("ordinary surface chrome was painted before the pane");
                    list.reset_shape(egui::layers::ShapeIdx(index));
                });
            }
            self.writes += 1;
            ui.label("The pane still emits its own ordinary content");
        }
    }

    for cross_layer in [false, true] {
        let context = Context::default();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([FIRST])),
        )])
        .unwrap();
        let mut dockspace = Dockspace::builder(("invalid-pane-output", cross_layer), layout)
            .build()
            .unwrap();
        let mut panes = CorruptingPane {
            cross_layer,
            writes: 0,
        };
        let mut capability = None;
        for _ in 0..3 {
            let (output, receipt) = crate::run_ui(&context, input(Vec::new()), |ui| {
                let response = dockspace
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .unwrap();
                capability = Some(response.presentation_settlement_capability());
            })
            .into_parts();
            output.drop_without_applying_deltas();
            receipt.report(crate::SurfacePresentationResult::Presented);
            assert!(dockspace.session.presented_surface(SURFACE).is_none());
        }
        assert!(panes.writes > 0);
        assert_eq!(
            capability,
            Some(crate::DockspaceCapability::Unavailable(
                crate::DockspaceUnavailableReason::PresentationOutputMismatch,
            )),
        );
    }
}
