//! Shared magnetic scenarios executed through real egui controls and owned output.

use super::*;
use dockspace_host_conformance::scenario::MagneticDockCase;
use egui_dockspace::DockspaceNodeView;

struct MagneticPanes {
    target_rect: Cell<Option<Rect>>,
}

impl MagneticPanes {
    fn new() -> Self {
        Self {
            target_rect: Cell::new(None),
        }
    }

    fn target_rect(&self) -> Option<Rect> {
        self.target_rect.get()
    }
}

impl PaneView for MagneticPanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Panes.title(item)
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) -> PaneResponse {
        if item == FIRST {
            self.target_rect.set(Some(ui.available_rect_before_wrap()));
        }
        Panes.ui(item, ui)
    }
}

fn magnetic_semantic_layout() -> DockspaceLayout {
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

pub(super) fn magnetic_preview_rect(
    output: &egui::FullOutput,
    fill: egui::Color32,
    target: Rect,
) -> Option<Rect> {
    rects_with_fill(output, &[fill])
        .into_iter()
        .filter(|rect| rect.area() >= target.area() * 0.2)
        .max_by(|left, right| left.area().total_cmp(&right.area()))
}

fn magnetic_basin_from_preview(preview: Rect, target: Rect) -> MagneticDockBasin {
    let width_ratio = preview.width() / target.width();
    let height_ratio = preview.height() / target.height();
    if width_ratio >= 0.8 && height_ratio >= 0.8 {
        return MagneticDockBasin::Center;
    }
    if width_ratio < height_ratio {
        if preview.center().x < target.center().x {
            MagneticDockBasin::West
        } else {
            MagneticDockBasin::East
        }
    } else if preview.center().y < target.center().y {
        MagneticDockBasin::North
    } else {
        MagneticDockBasin::South
    }
}

pub(super) fn normalized_pane_point(rect: Rect, case: MagneticDockCase) -> Pos2 {
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

pub(super) fn preview_readiness(
    frame: &FrameOutput,
    target_rect: Rect,
    target_point: Pos2,
    expected: MagneticDockBasin,
    drop_fill: egui::Color32,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    let preview = magnetic_preview_rect(&frame.output, drop_fill, target_rect);
    if preview.is_none() {
        missing.push("magnetic Dock preview");
    }
    if let Some(preview) = preview {
        assert_eq!(
            magnetic_basin_from_preview(preview, target_rect),
            expected,
            "the painted magnetic preview must match the shared basin: preview={preview:?} target={target_rect:?} point={target_point:?}",
        );
    }
    missing
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProductMagneticSnapshot {
    source_tabs: Vec<ItemId>,
    target_tabs: Vec<ItemId>,
    split_relation: Option<(DockspaceAxis, bool)>,
    source_selected: bool,
    source_surface: SurfaceId,
    source_root: RootId,
    source_contained: Option<FloatingPresentationId>,
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

fn expected_magnetic_split(basin: MagneticDockBasin) -> Option<(DockspaceAxis, bool)> {
    match basin {
        MagneticDockBasin::Center => None,
        MagneticDockBasin::North => Some((DockspaceAxis::Vertical, true)),
        MagneticDockBasin::South => Some((DockspaceAxis::Vertical, false)),
        MagneticDockBasin::East => Some((DockspaceAxis::Horizontal, false)),
        MagneticDockBasin::West => Some((DockspaceAxis::Horizontal, true)),
    }
}

fn product_magnetic_snapshot(dockspace: &Dockspace) -> Option<ProductMagneticSnapshot> {
    let source = dockspace.view().item(SECOND)?;
    let target = dockspace.view().item(FIRST)?;
    let split_relation = dockspace
        .view()
        .root(ROOT)?
        .content()
        .and_then(|content| item_split_relation(content, SECOND, FIRST));
    Some(ProductMagneticSnapshot {
        source_tabs: source.tabs().items().to_vec(),
        target_tabs: target.tabs().items().to_vec(),
        split_relation,
        source_selected: source.is_selected(),
        source_surface: source.surface(),
        source_root: source.root(),
        source_contained: source.contained(),
    })
}

fn magnetic_outcome_missing(
    snapshot: Option<&ProductMagneticSnapshot>,
    basin: MagneticDockBasin,
    dragging: bool,
    previous_ready: Option<&ProductMagneticSnapshot>,
) -> Vec<&'static str> {
    let mut missing = Vec::new();
    let Some(snapshot) = snapshot else {
        return vec![
            "source item ownership",
            "terminal drag state",
            "stable quiescence",
        ];
    };
    if snapshot.source_surface != SURFACE
        || snapshot.source_root != ROOT
        || snapshot.source_contained.is_some()
        || !snapshot.source_selected
    {
        missing.push("committed main-root ownership");
    }
    match basin {
        MagneticDockBasin::Center => {
            if snapshot.source_tabs != [FIRST, SECOND]
                || snapshot.target_tabs != [FIRST, SECOND]
                || snapshot.split_relation.is_some()
            {
                missing.push("center-merge topology");
            }
        }
        MagneticDockBasin::North
        | MagneticDockBasin::South
        | MagneticDockBasin::East
        | MagneticDockBasin::West => {
            if snapshot.source_tabs != [SECOND]
                || snapshot.target_tabs != [FIRST]
                || snapshot.split_relation != expected_magnetic_split(basin)
            {
                missing.push("edge-split topology");
            }
        }
    }
    if dragging {
        missing.push("terminal drag state");
    }
    if !missing.is_empty() || previous_ready != Some(snapshot) {
        missing.push("stable quiescence");
    }
    missing
}

fn settle_product_magnetic_preview(
    mut advance: impl FnMut() -> FrameOutput,
    mut initial: Option<FrameOutput>,
    motion_point: Pos2,
    target_rect: Rect,
    case: MagneticDockCase,
    style: &DockStyle,
) -> MagneticDockBasin {
    let drop_fill = style
        .visuals
        .drop_fill
        .expect("the fixture fixes its preview fill");
    let frame = settle_until(8, || {
        let frame = initial.take().unwrap_or_else(&mut advance);
        let missing = preview_readiness(&frame, target_rect, motion_point, case.basin(), drop_fill);
        (frame, missing)
    })
    .unwrap_or_else(|timeout| panic!("shared L03 preview failed for {case:?}: {timeout}"));
    let guide_fills = [
        style
            .visuals
            .drop_guide_fill
            .expect("the fixture fixes its passive guide fill"),
        style
            .visuals
            .drop_guide_active_fill
            .expect("the fixture fixes its active guide fill"),
    ];
    assert!(
        rects_with_fill(&frame.output, &guide_fills)
            .iter()
            .all(|guide| !guide.contains(motion_point)),
        "the measured content point must not aim at an explicit guide: {case:?} {motion_point:?}",
    );
    let preview = magnetic_preview_rect(&frame.output, drop_fill, target_rect)
        .expect("causal preview readiness requires one Dock preview");
    magnetic_basin_from_preview(preview, target_rect)
}

fn settle_product_magnetic_outcome(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    case: MagneticDockCase,
) -> usize {
    let mut previous_ready = None;
    let mut mutations = 0;
    settle_until(8, || {
        let (_, changed, _) = run_pane_frame_with_mutation(context, dockspace, panes, Vec::new());
        mutations += usize::from(changed);
        let snapshot = product_magnetic_snapshot(dockspace);
        let missing = magnetic_outcome_missing(
            snapshot.as_ref(),
            case.basin(),
            context.dragged_id().is_some(),
            previous_ready.as_ref(),
        );
        if missing == ["stable quiescence"] {
            previous_ready = snapshot.clone();
        } else if !missing.is_empty() {
            previous_ready = None;
        }
        (snapshot, missing)
    })
    .unwrap_or_else(|timeout| panic!("shared L03 outcome failed for {case:?}: {timeout}"))
    .expect("causal L03 settlement retains the moved source item");
    mutations
}

fn run_product_magnetic_case(case: MagneticDockCase) -> [SemanticObservation; 2] {
    assert_eq!(case.source(), ItemAlias::Secondary);
    assert_eq!(case.target(), ItemAlias::Primary);
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder(
        ("product-shared-magnetic", case.basin(), case.cadence()),
        magnetic_semantic_layout(),
    )
    .style(instrumented_style())
    .build()
    .expect("the shared magnetic product facade initializes");
    let mut panes = MagneticPanes::new();

    let (source, target_rect) = settle_until(8, || {
        let frame = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = try_node_rect(&frame.output, Role::Tab, "Second").map(|rect| rect.center());
        let target_rect = panes.target_rect();
        let mut missing = Vec::new();
        if source.is_none() {
            missing.push("source tab semantics");
        }
        if target_rect.is_none_or(|rect| !rect.is_positive()) {
            missing.push("target pane bounds");
        }
        ((source, target_rect), missing)
    })
    .unwrap_or_else(|timeout| {
        panic!("shared L03 fixture readiness failed for {case:?}: {timeout}")
    });
    let source = source.expect("causal fixture readiness publishes the source tab");
    let target_rect = target_rect.expect("causal fixture readiness publishes target pane bounds");
    let target = normalized_pane_point(target_rect, case);
    assert!(target_rect.contains(target));
    run_product_magnetic_drag(
        &context,
        &mut dockspace,
        &mut panes,
        case,
        source,
        target_rect,
        target,
    )
}

fn run_product_magnetic_drag(
    context: &Context,
    dockspace: &mut Dockspace,
    panes: &mut dyn PaneView,
    case: MagneticDockCase,
    source: Pos2,
    target_rect: Rect,
    target: Pos2,
) -> [SemanticObservation; 2] {
    let style = dockspace.style().clone();
    let before = product_magnetic_snapshot(dockspace);
    let mut mutations = 0;
    let mut released = false;
    let mut advance = |events: Vec<Event>| {
        released |= events
            .iter()
            .any(|event| matches!(event, Event::PointerButton { pressed: false, .. }));
        let (frame, changed, _) = run_pane_frame_with_mutation(context, dockspace, panes, events);
        mutations += usize::from(changed);
        if !released {
            assert!(
                !changed,
                "hover must not commit the docking workspace: {case:?}"
            );
            assert_eq!(product_magnetic_snapshot(dockspace), before);
        }
        frame
    };
    let threshold = source + vec2(12.0, 8.0);
    let observed_basin = match case.cadence() {
        CadenceProfile::Burst => {
            let _ = advance(vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
                Event::PointerMoved(threshold),
                Event::PointerMoved(target),
                pointer_button(target, false),
            ]);
            None
        }
        CadenceProfile::Stepwise => {
            let _ = advance(vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
            ]);
            let _ = advance(vec![Event::PointerMoved(threshold)]);
            let initial = advance(vec![Event::PointerMoved(target)]);
            let observed = settle_product_magnetic_preview(
                || advance(vec![Event::PointerMoved(target)]),
                Some(initial),
                target,
                target_rect,
                case,
                &style,
            );
            let _ = advance(vec![pointer_button(target, false)]);
            Some(observed)
        }
        CadenceProfile::Hesitant => {
            let _ = advance(vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
            ]);
            let _ = advance(vec![Event::PointerMoved(threshold)]);
            for point in [
                target,
                tangent_pane_point(target, case.basin(), 3.0),
                tangent_pane_point(target, case.basin(), -3.0),
            ] {
                let _ = advance(vec![Event::PointerMoved(point)]);
            }
            let initial = advance(vec![Event::PointerMoved(target)]);
            let observed = settle_product_magnetic_preview(
                || advance(vec![Event::PointerMoved(target)]),
                Some(initial),
                target,
                target_rect,
                case,
                &style,
            );
            let _ = advance(vec![pointer_button(target, false)]);
            Some(observed)
        }
        CadenceProfile::ReleaseWithFinalMove => {
            let _ = advance(vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
            ]);
            let _ = advance(vec![Event::PointerMoved(threshold)]);
            let acquired = tangent_pane_point(target, case.basin(), -4.0);
            let initial = advance(vec![Event::PointerMoved(acquired)]);
            let observed = settle_product_magnetic_preview(
                || advance(vec![Event::PointerMoved(acquired)]),
                Some(initial),
                acquired,
                target_rect,
                case,
                &style,
            );
            let _ = advance(vec![
                Event::PointerMoved(target),
                pointer_button(target, false),
            ]);
            Some(observed)
        }
        cadence => panic!("unsupported shared L03 cadence: {cadence:?}"),
    };
    assert_eq!(
        mutations, 1,
        "the original release update must publish exactly one mutation: {case:?}"
    );
    let snapshot = product_magnetic_snapshot(dockspace).expect("the release retains both items");
    assert_eq!(
        magnetic_outcome_missing(
            Some(&snapshot),
            case.basin(),
            context.dragged_id().is_some(),
            Some(&snapshot)
        ),
        Vec::<&str>::new(),
        "the release update commits the exact basin with terminal ownership: {case:?} {snapshot:?}",
    );
    let committed_basin = match snapshot.split_relation {
        None => MagneticDockBasin::Center,
        Some((DockspaceAxis::Horizontal, true)) => MagneticDockBasin::West,
        Some((DockspaceAxis::Horizontal, false)) => MagneticDockBasin::East,
        Some((DockspaceAxis::Vertical, true)) => MagneticDockBasin::North,
        Some((DockspaceAxis::Vertical, false)) => MagneticDockBasin::South,
    };
    assert_eq!(committed_basin, case.basin());
    if let Some(observed_basin) = observed_basin {
        assert_eq!(
            observed_basin, committed_basin,
            "the held preview and final mutation must agree: {case:?}"
        );
    }
    assert_eq!(
        settle_product_magnetic_outcome(context, dockspace, panes, case),
        0,
        "presentation and idle updates must not replay the release: {case:?}",
    );

    [
        SemanticObservation::ItemDocked {
            item: ItemAlias::Secondary,
            basin: committed_basin,
        },
        SemanticObservation::Quiescent,
    ]
}

fn magnetic_modifier_fixture() -> (Context, Dockspace, MagneticPanes, Pos2, Rect) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace =
        Dockspace::builder("product-magnetic-modifiers", magnetic_semantic_layout())
            .style(instrumented_style())
            .build()
            .expect("the modifier regression facade initializes");
    let mut panes = MagneticPanes::new();
    let (source, target_rect) = settle_until(8, || {
        let frame = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let source = try_node_rect(&frame.output, Role::Tab, "Second").map(|rect| rect.center());
        let target_rect = panes.target_rect().filter(|rect| rect.is_positive());
        let mut missing = Vec::new();
        if source.is_none() {
            missing.push("source tab semantics");
        }
        if target_rect.is_none() {
            missing.push("target pane bounds");
        }
        ((source, target_rect), missing)
    })
    .unwrap_or_else(|timeout| panic!("modifier fixture readiness failed: {timeout}"));
    // Present one unchanged output so all measured receiver geometry is current.
    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    (
        context,
        dockspace,
        panes,
        source.expect("the source tab is presented"),
        target_rect.expect("the target pane is presented"),
    )
}

#[test]
fn shift_only_frames_clear_magnetic_preview_and_reenable_waits_for_real_motion() {
    for key_modifiers in [false, true] {
        let (context, mut dockspace, mut panes, source, target_rect) = magnetic_modifier_fixture();
        let case = MAGNETIC_DOCKING_BASINS
            .cases()
            .find(|case| {
                case.basin() == MagneticDockBasin::Center
                    && case.cadence() == CadenceProfile::Stepwise
            })
            .expect("the shared matrix includes a center sample away from explicit guides");
        let target = normalized_pane_point(target_rect, case);
        let style = dockspace.style().clone();
        let drop_fill = style.visuals.drop_fill.unwrap();
        let before = product_magnetic_snapshot(&dockspace);
        let _ = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(source), pointer_button(source, true)],
        );
        let _ = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
        );
        settle_product_magnetic_preview(
            || {
                run_pane_frame(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    vec![Event::PointerMoved(target)],
                )
            },
            None,
            target,
            target_rect,
            case,
            &style,
        );
        let modifier_event = |modifiers| {
            if key_modifiers {
                Event::Key {
                    key: Key::A,
                    physical_key: Some(Key::A),
                    pressed: false,
                    repeat: false,
                    modifiers,
                }
            } else {
                Event::ModifiersChanged(modifiers)
            }
        };
        for events in [
            vec![modifier_event(Modifiers::SHIFT)],
            Vec::new(),
            vec![modifier_event(Modifiers::NONE)],
            Vec::new(),
        ] {
            let (frame, changed, _) =
                run_pane_frame_with_mutation(&context, &mut dockspace, &mut panes, events);
            assert_eq!(
                rect_fill_count(&frame.output, drop_fill),
                0,
                "modifier-only and idle frames must not retain or revive a magnetic preview",
            );
            assert!(
                !changed,
                "a modifier change must preserve gesture ownership"
            );
            assert_eq!(product_magnetic_snapshot(&dockspace), before);
        }
        let reacquired = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        assert!(
            preview_readiness(
                &reacquired,
                target_rect,
                target,
                MagneticDockBasin::Center,
                drop_fill,
            )
            .is_empty(),
            "the next real move must acquire a fresh magnetic target",
        );
        let _ = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![pointer_button(target, false)],
        );
        settle_product_magnetic_outcome(&context, &mut dockspace, &mut panes, case);
    }
}

#[test]
fn same_packet_shift_release_cannot_commit_an_earlier_magnetic_target() {
    for shift_release in [false, true] {
        let (context, mut dockspace, mut panes, source, target_rect) = magnetic_modifier_fixture();
        let target = Pos2::new(
            egui::lerp(target_rect.x_range(), 0.28),
            egui::lerp(target_rect.y_range(), 0.28),
        );
        let before = product_magnetic_snapshot(&dockspace);
        let release = Event::PointerButton {
            pos: target,
            button: PointerButton::Primary,
            pressed: false,
            modifiers: if shift_release {
                Modifiers::SHIFT
            } else {
                Modifiers::NONE
            },
        };
        let (_frame, changed, _) = run_pane_frame_with_mutation(
            &context,
            &mut dockspace,
            &mut panes,
            vec![
                Event::PointerMoved(source),
                pointer_button(source, true),
                Event::PointerMoved(source + vec2(12.0, 8.0)),
                Event::PointerMoved(target),
                release,
                // This must neither rewrite release metadata nor revive its old latch.
                Event::ModifiersChanged(Modifiers::NONE),
            ],
        );
        if shift_release {
            // Contained placement uses the same preview fill. The terminal model
            // below distinguishes a valid float from docking the old magnetic target.
            for _ in 0..8 {
                let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
                let source_item = dockspace.view().item(SECOND).unwrap();
                assert!(
                    source_item.contained().is_some()
                        || product_magnetic_snapshot(&dockspace) == before,
                    "a suppressed release may float or preserve its source but must not dock",
                );
                assert_eq!(
                    dockspace.view().item(FIRST).unwrap().tabs().items(),
                    &[FIRST]
                );
            }
            assert_eq!(context.dragged_id(), None);
        } else {
            let case = MAGNETIC_DOCKING_BASINS
                .cases()
                .find(|case| {
                    case.basin() == MagneticDockBasin::Center
                        && case.cadence() == CadenceProfile::Burst
                })
                .unwrap();
            assert!(
                changed,
                "the unmodified control packet must commit in its release update"
            );
            let snapshot = product_magnetic_snapshot(&dockspace).unwrap();
            assert!(
                magnetic_outcome_missing(
                    Some(&snapshot),
                    MagneticDockBasin::Center,
                    context.dragged_id().is_some(),
                    Some(&snapshot)
                )
                .is_empty(),
                "{snapshot:?}"
            );
            assert_eq!(
                settle_product_magnetic_outcome(&context, &mut dockspace, &mut panes, case),
                0
            );
        }
    }
}

#[test]
fn shift_held_from_press_stays_suppressed_through_moves_without_modifiers() {
    let (context, mut dockspace, mut panes, source, target_rect) = magnetic_modifier_fixture();
    let target = Pos2::new(
        egui::lerp(target_rect.x_range(), 0.28),
        egui::lerp(target_rect.y_range(), 0.28),
    );
    let style = dockspace.style().clone();
    let before = product_magnetic_snapshot(&dockspace);
    let button = |pos, pressed| Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Modifiers::SHIFT,
    };
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::ModifiersChanged(Modifiers::SHIFT)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), button(source, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
    );
    for events in [
        vec![Event::PointerMoved(target)],
        Vec::new(),
        vec![Event::PointerMoved(target)],
        Vec::new(),
    ] {
        let _ = run_pane_frame(&context, &mut dockspace, &mut panes, events);
        // A contained float preview remains valid while magnetic targets are paused.
        assert_eq!(product_magnetic_snapshot(&dockspace), before);
    }
    let dragging = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert!(
        guide_button_count(
            &dragging.output,
            style.visuals.drop_guide_fill.unwrap(),
            style.visuals.drop_guide_active_fill.unwrap(),
        ) > 0,
        "the suspended gesture must stay active and keep explicit guides available",
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![button(target, false)],
    );
    for _ in 0..8 {
        let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        assert!(
            dockspace.view().item(SECOND).unwrap().contained().is_some()
                || product_magnetic_snapshot(&dockspace) == before,
            "holding Shift for the entire gesture must not dock on release",
        );
    }
    assert_eq!(context.dragged_id(), None);
}

#[test]
fn shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics() {
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

    let cases = MAGNETIC_DOCKING_BASINS
        .cases()
        .filter(|case| {
            matches!(
                case.cadence(),
                CadenceProfile::Stepwise
                    | CadenceProfile::Hesitant
                    | CadenceProfile::ReleaseWithFinalMove
            )
        })
        .collect::<Vec<_>>();
    let observations = cases
        .iter()
        .copied()
        .flat_map(run_product_magnetic_case)
        .collect::<Vec<_>>();
    let expected = cases
        .iter()
        .copied()
        .flat_map(MagneticDockCase::expected_observations)
        .collect::<Vec<_>>();
    assert_eq!(
        observations, expected,
        "the egui product executor must map every currently supported shared L03 cadence to normalized docking and quiescence observations",
    );
}

#[test]
fn shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics() {
    let observations = MAGNETIC_DOCKING_BASINS
        .cases()
        .flat_map(run_product_magnetic_case)
        .collect::<Vec<_>>();
    assert_eq!(
        observations,
        MAGNETIC_DOCKING_BASINS.expected_observations(),
        "the complete egui product L03 matrix must eventually match core-host normalized observations",
    );
}

#[test]
fn ordinary_content_widgets_accept_the_complete_magnetic_docking_matrix() {
    for widget in PaneContentWidget::ALL {
        for case in MAGNETIC_DOCKING_BASINS.cases() {
            let context = Context::default();
            context.enable_accesskit();
            context.options_mut(|options| {
                options.max_passes = 4.try_into().expect("four is non-zero");
            });
            let mut dockspace = Dockspace::builder(
                (
                    "product-widget-magnetic",
                    widget,
                    case.basin(),
                    case.cadence(),
                ),
                magnetic_semantic_layout(),
            )
            .style(instrumented_style())
            .build()
            .expect("the content-widget facade initializes");
            let mut panes = TextPanes::magnetic(widget);
            let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
            let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
            let source = tab_center(&stable.output, "Preview");
            let pane_rect = panes.pane_rect(FIRST);
            let response = panes.widget_response(FIRST);
            let widget_rect = response.interact_rect;
            assert!(
                response.enabled() && response.sense.interactive(),
                "{widget:?}"
            );
            assert!(
                widget_rect.is_finite() && widget_rect.is_positive(),
                "{widget:?}"
            );
            assert!(
                pane_rect.contains_rect(widget_rect),
                "{widget:?}: pane={pane_rect:?}, widget={widget_rect:?}"
            );
            assert!(panes.content_widgets[&FIRST].contains(&response.id));
            let target = normalized_pane_point(widget_rect, case);
            for delta in [-4.0, -3.0, 0.0, 3.0] {
                assert!(
                    widget_rect.contains(tangent_pane_point(target, case.basin(), delta)),
                    "every input sample must hit the actual {widget:?} response: {case:?}",
                );
            }

            assert_eq!(
                run_product_magnetic_drag(
                    &context,
                    &mut dockspace,
                    &mut panes,
                    case,
                    source,
                    pane_rect,
                    target,
                ),
                case.expected_observations(),
                "ordinary {widget:?} must pass docking hover without guide aiming: {case:?}",
            );
            assert_eq!(
                panes.clicks.get(&FIRST),
                None,
                "a docking release must not click {widget:?}"
            );
            assert!(
                !panes.dragged.contains(&FIRST),
                "a tab drag must not drag {widget:?}"
            );
            assert_eq!(panes.texts[&FIRST], "Target content");
        }
    }
}

#[test]
fn declared_content_widgets_keep_their_click_edit_scroll_and_drag_input() {
    for widget in PaneContentWidget::ALL {
        let context = Context::default();
        context.enable_accesskit();
        context.options_mut(|options| {
            options.max_passes = 4.try_into().expect("four is non-zero");
        });
        let mut dockspace = Dockspace::builder(
            ("product-widget-ordinary-input", widget),
            magnetic_semantic_layout(),
        )
        .style(instrumented_style())
        .build()
        .expect("the ordinary widget-input facade initializes");
        let mut panes = TextPanes::magnetic(widget);
        let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
        let response = panes.widget_response(FIRST);
        let id = response.id;
        let rect = response.interact_rect;
        let point = Pos2::new(
            egui::lerp(rect.x_range(), 0.65),
            egui::lerp(rect.y_range(), 0.65),
        );
        let before = product_magnetic_snapshot(&dockspace);
        let drop_fill = dockspace
            .style()
            .visuals
            .drop_fill
            .expect("the fixture fixes its preview fill");
        let mut advance = |events| {
            let (frame, changed, _) =
                run_pane_frame_with_mutation(&context, &mut dockspace, &mut panes, events);
            assert!(
                !changed,
                "ordinary {widget:?} input must not mutate docking state"
            );
            assert_eq!(product_magnetic_snapshot(&dockspace), before);
            assert_eq!(
                rect_fill_count(&frame.output, drop_fill),
                0,
                "ordinary {widget:?} input must not arm a docking preview"
            );
        };

        match widget {
            PaneContentWidget::Button | PaneContentWidget::TextEdit => {
                advance(vec![
                    Event::PointerMoved(point),
                    pointer_button(point, true),
                ]);
                advance(vec![pointer_button(point, false)]);
                if widget == PaneContentWidget::TextEdit {
                    assert!(context.memory(|memory| memory.has_focus(id)));
                    advance(vec![Event::Text("!".to_owned())]);
                }
            }
            PaneContentWidget::ScrollArea => {
                advance(vec![
                    Event::PointerMoved(point),
                    mouse_wheel(vec2(0.0, -96.0)),
                ]);
            }
            PaneContentWidget::Canvas => {
                let moved = point + vec2(20.0, 15.0);
                assert!(rect.contains(moved));
                advance(vec![
                    Event::PointerMoved(point),
                    pointer_button(point, true),
                ]);
                advance(vec![Event::PointerMoved(moved)]);
                assert_eq!(
                    context.dragged_id(),
                    Some(id),
                    "the canvas, not docking chrome, owns this drag"
                );
                advance(vec![pointer_button(moved, false)]);
            }
        }
        for _ in 0..3 {
            advance(Vec::new());
        }
        match widget {
            PaneContentWidget::Button => assert_eq!(panes.clicks.get(&FIRST), Some(&1)),
            PaneContentWidget::TextEdit => {
                assert!(
                    panes.texts[&FIRST].contains('!'),
                    "the focused TextEdit must receive text input"
                );
                assert_eq!(panes.texts[&SECOND], "Source content");
            }
            PaneContentWidget::ScrollArea => assert!(
                panes.scroll_offsets[&FIRST] > 0.0,
                "the real ScrollArea must consume wheel input"
            ),
            PaneContentWidget::Canvas => assert!(panes.dragged.contains(&FIRST)),
        }
        assert_eq!(context.dragged_id(), None);
    }
}

#[test]
fn unknown_content_and_overlapping_undeclared_widgets_block_docking_hover() {
    for widget in PaneContentWidget::ALL {
        for (registered, overlapping) in [(false, false), (true, true)] {
            for case in MAGNETIC_DOCKING_BASINS.cases().filter(|case| {
                case.basin() == MagneticDockBasin::Center
                    && case.cadence() == CadenceProfile::Stepwise
            }) {
                let context = Context::default();
                context.enable_accesskit();
                context.options_mut(|options| {
                    options.max_passes = 4.try_into().expect("four is non-zero");
                });
                let mut dockspace = Dockspace::builder(
                    ("product-widget-unknown", widget, registered, overlapping),
                    magnetic_semantic_layout(),
                )
                .style(instrumented_style())
                .build()
                .expect("the unknown-widget facade initializes");
                let mut panes = TextPanes::magnetic(widget);
                panes.register_content = registered;
                panes.overlapping_blocker = overlapping;
                let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
                let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
                let source = tab_center(&stable.output, "Preview");
                let response = panes.widget_response(FIRST);
                let target = normalized_pane_point(response.interact_rect, case);
                assert!(response.enabled() && response.interact_rect.contains(target));
                assert_eq!(
                    panes.content_widgets[&FIRST].len(),
                    if registered { 2 } else { 0 }
                );
                if overlapping {
                    let blocker = context
                        .read_response(response.id.with("unknown-blocker"))
                        .expect("the overlapping blocker registered a real response");
                    assert!(blocker.enabled() && blocker.sense.interactive());
                    assert_eq!(blocker.interact_rect, response.interact_rect);
                    assert!(!panes.content_widgets[&FIRST].contains(&blocker.id));
                }
                let before = product_magnetic_snapshot(&dockspace);
                let drop_fill = dockspace
                    .style()
                    .visuals
                    .drop_fill
                    .expect("the fixture fixes its preview fill");
                let guide_fills = [
                    dockspace
                        .style()
                        .visuals
                        .drop_guide_fill
                        .expect("the fixture fixes its passive guide fill"),
                    dockspace
                        .style()
                        .visuals
                        .drop_guide_active_fill
                        .expect("the fixture fixes its active guide fill"),
                ];
                for events in [
                    vec![Event::PointerMoved(source), pointer_button(source, true)],
                    vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
                ] {
                    let (_, changed, _) =
                        run_pane_frame_with_mutation(&context, &mut dockspace, &mut panes, events);
                    assert!(!changed);
                    assert_eq!(product_magnetic_snapshot(&dockspace), before);
                }
                let mut advance = |events| {
                    let (frame, changed, _) =
                        run_pane_frame_with_mutation(&context, &mut dockspace, &mut panes, events);
                    assert!(
                        !changed,
                        "unknown {widget:?} must not authorize a commit; overlapping={overlapping} {case:?}"
                    );
                    assert_eq!(product_magnetic_snapshot(&dockspace), before);
                    assert_eq!(
                        rect_fill_count(&frame.output, drop_fill),
                        0,
                        "unknown {widget:?} must stop hover, not fall through or float; overlapping={overlapping} {case:?}"
                    );
                    assert!(
                        rects_with_fill(&frame.output, &guide_fills)
                            .iter()
                            .all(|guide| !guide.contains(target))
                    );
                };
                for _ in 0..8 {
                    advance(vec![Event::PointerMoved(target)]);
                }
                advance(vec![pointer_button(target, false)]);
                for _ in 0..8 {
                    advance(Vec::new());
                }
                assert_eq!(context.dragged_id(), None);
            }
        }
    }
}

#[test]
fn ordinary_pane_center_release_uses_magnetic_merge_without_guide_aiming() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-magnetic-center-basin", split_layout())
        .style(instrumented_style())
        .build()
        .expect("the magnetic docking facade initializes");
    let mut panes = FocusPanes::new("magnetic-center-basin", true);

    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let (stable, _) = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = tab_center(&stable.output, "First");
    let pane = panes.pane_rect();
    let target = Pos2::new(
        egui::lerp(pane.x_range(), 0.65),
        egui::lerp(pane.y_range(), 0.65),
    );
    let passive = dockspace
        .style()
        .visuals
        .drop_guide_fill
        .expect("the fixture fixes its passive guide color");
    let active = dockspace
        .style()
        .visuals
        .drop_guide_active_fill
        .expect("the fixture fixes its active guide color");
    let drop_fill = dockspace
        .style()
        .visuals
        .drop_fill
        .expect("the fixture fixes its preview fill");

    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(12.0, 8.0))],
    );

    let mut preview = None;
    for _ in 0..8 {
        let frame = run_focus_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        )
        .0;
        let center_merge = rect_with_fill(&frame.output, drop_fill)
            .is_some_and(|rect| rect.min.x < pane.min.x && rect.max.x >= pane.max.x - 0.01);
        if center_merge {
            preview = Some(frame);
            break;
        }
    }
    let preview = preview
        .expect("the ordinary pane point publishes a center-merge preview before the deadline");
    assert!(
        preview.output.shapes.iter().any(|shape| matches!(
            &shape.shape,
            egui::Shape::Rect(rect) if rect.fill == active
        )),
        "the magnetic center candidate highlights its explanatory guide"
    );
    assert!(
        preview.output.shapes.iter().all(|shape| !matches!(
            &shape.shape,
            egui::Shape::Rect(rect)
                if (rect.fill == passive || rect.fill == active) && rect.rect.contains(target)
        )),
        "the semantic journey must not aim at an explicit guide button"
    );

    let _ = run_focus_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );

    let mut merged = false;
    for _ in 0..8 {
        let no_contained = dockspace
            .view()
            .surface(SURFACE)
            .is_some_and(|surface| surface.contained().next().is_none());
        if root_tabs(&dockspace) == Some(vec![SECOND, FIRST]) && no_contained {
            merged = true;
            break;
        }
        let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    }

    assert!(
        merged,
        "releasing inside the pane's center basin merges without exact guide aiming"
    );
    let moved = dockspace
        .view()
        .item(FIRST)
        .expect("the moved tab remains open after magnetic docking");
    assert_eq!(moved.surface(), SURFACE);
    assert_eq!(moved.root(), ROOT);
    assert_eq!(moved.contained(), None);
    assert!(moved.is_selected());
    assert!(dockspace.view().contained(FLOATING).is_none());
    assert_eq!(context.dragged_id(), None);

    let stable_topology = root_tabs(&dockspace);
    let _ = run_focus_frame(&context, &mut dockspace, &mut panes, Vec::new());
    assert_eq!(root_tabs(&dockspace), stable_topology);
    assert_eq!(context.dragged_id(), None);
}

#[test]
fn interactive_text_pane_still_accepts_magnetic_tab_dock() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 4.try_into().expect("four is non-zero");
    });
    let mut dockspace = Dockspace::builder("product-text-pane-magnetic-dock", editor_layout())
        .style(instrumented_style())
        .build()
        .expect("the editor product facade initializes");
    let mut panes = TextPanes::new([
        (FIRST, "fn main() {}"),
        (SECOND, "Preview output"),
        (THIRD, "Outline"),
    ]);
    panes.fill_widget = true;

    let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let stable = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    let source = node_center(&stable.output, Role::Tab, "Preview");
    let target_pane = panes.pane_rect(THIRD);
    let widget_rect = panes.widget_response(THIRD).interact_rect;
    let probe = Pos2::new(
        egui::lerp(widget_rect.x_range(), 0.72),
        egui::lerp(widget_rect.y_range(), 0.72),
    );
    assert!(widget_rect.contains(probe) && target_pane.contains(probe));

    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source), pointer_button(source, true)],
    );
    let _ = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(source + vec2(24.0, 8.0))],
    );
    let guide_frame = run_pane_frame(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(probe)],
    );
    let guide_fills = [
        dockspace.style().visuals.drop_guide_fill.unwrap(),
        dockspace.style().visuals.drop_guide_active_fill.unwrap(),
    ];
    let guides = rects_with_fill(&guide_frame.output, &guide_fills);
    assert!(
        !guides.is_empty(),
        "the target's actual guide geometry must be available"
    );
    let target = MAGNETIC_DOCKING_BASINS
        .cases()
        .filter(|case| {
            case.basin() == MagneticDockBasin::Center && case.cadence() == CadenceProfile::Stepwise
        })
        .map(|case| normalized_pane_point(target_pane, case))
        .filter(|point| {
            widget_rect.contains(*point) && guides.iter().all(|guide| !guide.contains(*point))
        })
        .min_by(|left, right| left.distance_sq(probe).total_cmp(&right.distance_sq(probe)))
        .expect("a shared center-basin sample must hit the text widget outside the painted guides");
    assert!(widget_rect.contains(target) && target_pane.contains(target));
    let mut preview = None;
    for _ in 0..8 {
        let frame = run_pane_frame(
            &context,
            &mut dockspace,
            &mut panes,
            vec![Event::PointerMoved(target)],
        );
        if rect_with_fill(
            &frame.output,
            dockspace
                .style()
                .visuals
                .drop_fill
                .expect("the fixture fixes its preview fill"),
        )
        .is_some_and(|rect| rect.contains(target) || rect.width() > 100.0)
        {
            preview = Some(frame);
            break;
        }
    }
    assert!(
        preview.is_some(),
        "dragging a tab over an interactive pane must publish a magnetic preview"
    );
    let preview = preview.unwrap();
    assert!(
        rects_with_fill(&preview.output, &guide_fills)
            .iter()
            .all(|guide| !guide.contains(target)),
        "the actual text input point must not hit a guide"
    );
    let preview_rect = rect_with_fill(
        &preview.output,
        dockspace.style().visuals.drop_fill.unwrap(),
    )
    .unwrap();
    assert_eq!(
        magnetic_basin_from_preview(preview_rect, target_pane),
        MagneticDockBasin::Center,
        "widget={widget_rect:?}, pane={target_pane:?}, point={target:?}, preview={preview_rect:?}"
    );

    let (_released, workspace_changed, published_state_changed) = run_pane_frame_with_mutation(
        &context,
        &mut dockspace,
        &mut panes,
        vec![Event::PointerMoved(target), pointer_button(target, false)],
    );
    assert!(
        workspace_changed,
        "releasing a valid magnetic target mutates the workspace"
    );
    assert!(
        published_state_changed,
        "releasing a valid magnetic target publishes the committed state"
    );
    for _ in 0..8 {
        if root_split(&dockspace).is_some_and(|(_, children)| {
            children.iter().any(|items| items.contains(&SECOND))
                && children.iter().any(|items| items.contains(&THIRD))
                && children.iter().any(|items| items.contains(&FIRST))
        }) {
            break;
        }
        let _ = run_pane_frame(&context, &mut dockspace, &mut panes, Vec::new());
    }
    let split = root_split(&dockspace).expect("the editor split remains present");
    assert!(
        split.1[0].contains(&SECOND),
        "Preview must dock into the Outline tab stack when released over its text editor: {split:?}"
    );
}
