use egui::{Color32, Context, FullOutput, Id, Shape};

use super::{NativeSurfacePaint, SurfaceId, validate_final_outputs};
use crate::output_proof::SurfaceOutputCapture;

fn unready_paint(surface: SurfaceId) -> NativeSurfacePaint {
    NativeSurfacePaint {
        surface,
        viewport_id: egui::ViewportId::ROOT,
        cumulative_pass_nr: 1,
        had_ready_plan: false,
        deferred_measurement: false,
        transient_visuals_complete: true,
        missing_items: Default::default(),
        receivers: Vec::new(),
        scroll_receivers: Vec::new(),
        semantic_output: None,
        execution_receipt: None,
        output_proof: None,
        local_actions: Vec::new(),
        pane_focus_observation: None,
        pane_focus_needs_repaint: false,
        pane_focus_bindings: Vec::new(),
    }
}

fn popup(context: &Context, index: usize) {
    let x = if index == 0 { 100.0 } else { 200.0 };
    let _ = egui::Area::new(Id::unique(("native-output-popup", index)))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(x, 100.0))
        .show(context, |ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(40.0, 40.0), egui::Sense::hover());
            ui.painter().circle_filled(rect.center(), 8.0, Color32::RED);
        });
}

fn fixture(with_popups: bool) -> ([NativeSurfacePaint; 2], FullOutput) {
    let context = Context::default();
    let input = egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        )),
        ..Default::default()
    };
    if with_popups {
        context
            .run_ui(input.clone(), |ui| {
                popup(ui.ctx(), 0);
                popup(ui.ctx(), 1);
            })
            .drop_without_applying_deltas();
    }
    let mut paints = None;
    let mut output = context.run_ui(input, |ui| {
        paints = Some(std::array::from_fn(|index| {
            let capture = SurfaceOutputCapture::begin_multilayer(ui.painter());
            let (x, color, surface) = if index == 0 {
                (20.0, Color32::BLUE, SurfaceId::new(1))
            } else {
                (60.0, Color32::GREEN, SurfaceId::new(2))
            };
            ui.painter().circle_filled(egui::pos2(x, 20.0), 8.0, color);
            if with_popups {
                popup(ui.ctx(), index);
            }
            NativeSurfacePaint {
                had_ready_plan: true,
                output_proof: Some(
                    capture
                        .finish(ui.painter())
                        .expect("the fixture only appends to its surface and registered popup"),
                ),
                ..unready_paint(surface)
            }
        }));
    });
    output.textures_delta.clear();
    (paints.unwrap(), output)
}

fn user_shapes(output: &FullOutput) -> Vec<egui::epaint::ClippedShape> {
    output
        .shapes
        .iter()
        .filter(|clipped| !matches!(clipped.shape, Shape::Callback(_)))
        .cloned()
        .collect()
}

#[test]
fn final_outputs_scrub_all_markers_after_every_surface_validates() {
    let (paints, mut output) = fixture(false);
    let expected = user_shapes(&output);
    assert_eq!(expected.len(), 2);
    assert!(output.shapes.len() > expected.len());

    assert!(validate_final_outputs(&paints, &mut output));
    assert_eq!(output.shapes, expected);
}

#[test]
fn invalid_later_surface_preserves_all_markers_and_allows_retry() {
    let ([first, second], mut output) = fixture(false);
    let index = output
        .shapes
        .iter()
        .position(|clipped| matches!(&clipped.shape, Shape::Circle(circle) if circle.fill == Color32::GREEN))
        .unwrap();
    let removed = output.shapes.remove(index);
    let rejected = output.shapes.clone();

    assert!(!validate_final_outputs([&first, &second], &mut output));
    assert_eq!(output.shapes, rejected);
    assert!(
        first
            .output_proof
            .as_ref()
            .unwrap()
            .present_spans(&output)
            .is_some(),
        "rejecting a later surface must not scrub the earlier proof"
    );

    output.shapes.insert(index, removed);
    let expected = user_shapes(&output);
    assert!(validate_final_outputs([&first, &second], &mut output));
    assert_eq!(output.shapes, expected);
}

#[test]
fn duplicate_or_reversed_surfaces_reject_without_scrubbing() {
    let ([first, second], output) = fixture(false);
    for paints in [[&first, &first], [&second, &first]] {
        let mut rejected = output.clone();
        assert!(!validate_final_outputs(paints, &mut rejected));
        assert_eq!(rejected.shapes, output.shapes);
    }
}

#[test]
fn final_outputs_accept_interleaved_surface_and_popup_spans() {
    let ([first, second], mut output) = fixture(true);
    let first_spans = first
        .output_proof
        .as_ref()
        .unwrap()
        .present_spans(&output)
        .unwrap();
    let second_spans = second
        .output_proof
        .as_ref()
        .unwrap()
        .present_spans(&output)
        .unwrap();
    assert_eq!(first_spans.spans.len(), 2);
    assert_eq!(second_spans.spans.len(), 2);
    assert!(first_spans.spans[0].range.1 <= second_spans.spans[0].range.0);
    assert!(second_spans.spans[0].range.1 <= first_spans.spans[1].range.0);
    let expected = user_shapes(&output);

    assert!(validate_final_outputs([&first, &second], &mut output));
    assert_eq!(output.shapes, expected);
}

#[test]
fn only_ready_surfaces_require_a_complete_proof() {
    let ([first, mut second], mut output) = fixture(false);
    let unready = unready_paint(SurfaceId::new(3));
    let proof = second.output_proof.take();
    let original = output.shapes.clone();
    assert!(!validate_final_outputs(
        [&first, &unready, &second],
        &mut output
    ));
    assert_eq!(output.shapes, original);

    second.output_proof = proof;
    assert!(validate_final_outputs(
        [&first, &unready, &second],
        &mut output
    ));
}

#[test]
fn repeated_pending_focus_observations_remain_available_without_requesting_another_paint() {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::DockspaceSession;
    use egui::{RawInput, Rect, Ui};

    use crate::{DockInputConfig, DockStyle, PaneResponse, PaneView};

    struct Editors {
        pane: Id,
        search: Id,
        request_focus: Option<Id>,
        pane_text: String,
        search_text: String,
    }

    impl PaneView for Editors {
        fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
            Some("Document".into())
        }

        fn ui(&mut self, _: ItemId, ui: &mut Ui) -> PaneResponse {
            let pane = ui.add(egui::TextEdit::singleline(&mut self.pane_text).id(self.pane));
            let search = ui.add(egui::TextEdit::singleline(&mut self.search_text).id(self.search));
            if let Some(target) = self.request_focus.take() {
                ui.memory_mut(|memory| memory.request_focus(target));
            }
            PaneResponse::from(&pane).with_content(&search)
        }

        fn focus_target(&self, _: ItemId) -> Option<Id> {
            Some(self.pane)
        }
    }

    let surface = SurfaceId::new(1);
    let item = ItemId::new(1);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        surface,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([item])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(item).unwrap();
    frame.commit().unwrap();
    let request = session
        .pane_focus_request(surface)
        .expect("the committed selection creates an exact focus request");
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let pane = Id::unique("native-focus-repaint-pane");
    let search = Id::unique("native-focus-repaint-search");
    let mut editors = Editors {
        pane,
        search,
        request_focus: Some(search),
        pane_text: String::new(),
        search_text: String::new(),
    };
    for (pass, expected_repaint, expected_focus) in [
        (0, true, search),
        (1, false, search),
        (2, true, pane),
        (3, false, pane),
    ] {
        if pass == 2 {
            editors.request_focus = Some(pane);
        }
        let mut observed = false;
        let mut output = context.run_ui(
            RawInput {
                screen_rect: Some(Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                focused: true,
                time: Some(f64::from(pass) * 0.1),
                ..RawInput::default()
            },
            |ui| {
                let mut frame = session.begin_host_frame().unwrap();
                let bounds = ui.available_rect_before_wrap();
                super::measure_surface(
                    &mut frame,
                    surface,
                    ui,
                    bounds,
                    bounds,
                    &editors,
                    &DockStyle::default(),
                )
                .unwrap();
                let mut register_scroll = |ui: &Ui, _: Rect, id: Id| (id, ui.layer_id());
                let mut paint = super::paint_surface(
                    &mut frame,
                    Id::unique("native-focus-repaint"),
                    surface,
                    ui,
                    &mut editors,
                    &DockStyle::default(),
                    &DockInputConfig::default(),
                    &mut register_scroll,
                )
                .unwrap();
                assert!(paint.had_ready_plan());
                assert_eq!(paint.pane_focus_needs_repaint(), expected_repaint);
                observed = paint.take_pane_focus_observation().is_some();
                assert!(
                    paint.confirm_into(&mut frame).unwrap(),
                    "every observation still belongs to a complete surface output"
                );
                // Dropped outputs cannot acknowledge the request, so the same
                // pending request remains observable across ordinary UI frames.
                drop(frame.commit().unwrap());
            },
        );
        output.textures_delta.clear();
        assert!(observed, "unchanged observations must not be discarded");
        assert_eq!(context.memory(egui::Memory::focused), Some(expected_focus));
        assert_eq!(session.pane_focus_request(surface), Some(request));
    }
}
