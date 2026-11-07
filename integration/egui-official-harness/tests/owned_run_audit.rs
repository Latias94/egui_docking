//! Public regressions for owned egui runs, input filtering, and shared output proof.

use std::any::TypeId;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Color32, Context, Event, FullOutput, Pos2, RawInput, Rect, Ui};
use egui_dockspace::{
    Dockspace, DockspaceCapability, DockspaceErrorKind, DockspaceLayout, DockspaceNode,
    DockspaceRootLayout, DockspaceSurfaceLayout, DockspaceUnavailableReason, ItemId, PaneView,
    RootId, SurfaceId, SurfacePresentationResult, run_ui,
};

const SURFACE: SurfaceId = SurfaceId::new(1);

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1), ItemId::new(2)]),
        ),
    )])
    .unwrap()
}

fn input(time: f64, events: Vec<Event>) -> RawInput {
    RawInput {
        screen_rect: Some(Rect::from_min_size(Pos2::ZERO, egui::vec2(800.0, 600.0))),
        time: Some(time),
        events,
        ..RawInput::default()
    }
}

#[test]
fn a_foreign_root_cannot_enroll_a_fresh_instance_in_an_active_callback() {
    for normal_first in [false, true] {
        let context = Context::default();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let mut normal = Dockspace::builder("audit-normal-root", layout())
            .build()
            .unwrap();
        let mut foreign = Dockspace::builder("audit-foreign-root", layout())
            .build()
            .unwrap();
        let mut panes = Panes::default();
        let output = run_ui(&context, input(0.0, vec![]), |ui| {
            if normal_first {
                normal.show_single_surface(SURFACE, ui, &mut panes).unwrap();
            }
            let mut foreign_ui = Ui::new(
                context.clone(),
                egui::Id::unique("audit-untagged-root"),
                egui::UiBuilder::new().max_rect(context.viewport_rect()),
            );
            let error = foreign
                .show_single_surface(SURFACE, &mut foreign_ui, &mut panes)
                .err()
                .expect("an unrelated root cannot acquire the callback's run scope");
            assert_eq!(error.kind(), DockspaceErrorKind::OperationConflict);
        });
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

#[test]
fn a_caught_raw_context_recursion_revokes_the_entire_wrapper_run() {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    let mut docking = Dockspace::builder("audit-raw-context-recursion", layout())
        .build()
        .unwrap();
    let mut panes = Panes::default();
    let mut passes = 0;
    let output = run_ui(&context, input(0.0, vec![]), |ui| {
        passes += 1;
        if passes == 1 {
            let nested = catch_unwind(AssertUnwindSafe(|| {
                context
                    .run_ui(input(0.0, vec![]), |_| {})
                    .drop_without_applying_deltas();
            }));
            assert!(
                nested.is_err(),
                "egui rejects borrowing the active root twice"
            );
            context.request_discard("poison must persist across the outer pass retry");
        }
        let error = docking
            .show_single_surface(SURFACE, ui, &mut panes)
            .err()
            .expect("caught raw recursion cannot authorize this or later passes");
        assert_eq!(error.kind(), DockspaceErrorKind::OperationConflict);
    });
    assert_eq!(passes, 2);
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
}

#[derive(Default)]
struct Panes {
    allocate_texture: bool,
    texture: Option<egui::TextureHandle>,
}

impl PaneView for Panes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        Some(format!("Pane {}", item.get()).into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) -> egui_dockspace::PaneResponse {
        if self.allocate_texture && self.texture.is_none() {
            self.texture = Some(ui.ctx().load_texture(
                "owned-run-audit-texture",
                egui::ColorImage::filled([2, 2], Color32::GREEN),
                egui::TextureOptions::NEAREST,
            ));
        }
        if let Some(texture) = &self.texture {
            ui.image((texture.id(), egui::vec2(20.0, 20.0)));
        } else {
            ui.label("Audit pane");
        }
        egui_dockspace::PaneResponse::default()
    }
}

struct RejectOneOutput(bool);

impl egui::plugin::Plugin for RejectOneOutput {
    fn debug_name(&self) -> &'static str {
        "owned_run_audit::reject_one_output"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if std::mem::take(&mut self.0) {
            output.shapes.clear();
        }
    }
}

#[test]
fn rejected_owned_output_retains_texture_updates_for_the_next_renderer_output() {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let mut docking = Dockspace::builder("owned-run-audit-texture", layout())
        .build()
        .unwrap();
    let mut panes = Panes::default();
    for step in 0..4 {
        let (output, _) = docking
            .run_single_surface(
                &context,
                input(f64::from(step), vec![]),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }

    panes.allocate_texture = true;
    context.add_plugin(RejectOneOutput(true));
    let rejected = catch_unwind(AssertUnwindSafe(|| {
        docking.run_single_surface(&context, input(4.0, vec![]), SURFACE, &mut panes)
    }));
    match rejected {
        Ok(Err(_)) => {}
        Ok(Ok((output, _))) => {
            output.into_parts().0.drop_without_applying_deltas();
            panic!("the output hook should invalidate the docking proof");
        }
        Err(_) => panic!("rejecting docking authority must not drop renderer texture updates"),
    }

    let texture = panes
        .texture
        .as_ref()
        .expect("the rejected callback allocated a texture")
        .id();
    let (output, _) = docking
        .run_single_surface(&context, input(5.0, vec![]), SURFACE, &mut panes)
        .unwrap();
    let (output, receipt) = output.into_parts();
    let recovered = output.textures_delta.set.contains_key(&texture);
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(
        recovered,
        "the renderer must receive the texture upload from the rejected docking run"
    );
}

#[test]
fn dropping_context_abandons_pending_texture_deltas_without_panicking() {
    let result = catch_unwind(AssertUnwindSafe(|| {
        let context = Context::default();
        context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
        let mut docking = Dockspace::builder("owned-run-audit-drop-texture", layout())
            .build()
            .unwrap();
        let mut panes = Panes::default();
        for step in 0..4 {
            let (output, _) = docking
                .run_single_surface(
                    &context,
                    input(f64::from(step), vec![]),
                    SURFACE,
                    &mut panes,
                )
                .unwrap();
            let (output, receipt) = output.into_parts();
            output.drop_without_applying_deltas();
            receipt.report(SurfacePresentationResult::Presented);
        }

        panes.allocate_texture = true;
        context.add_plugin(RejectOneOutput(true));
        assert!(
            docking
                .run_single_surface(&context, input(4.0, vec![]), SURFACE, &mut panes)
                .is_err()
        );
        drop(panes);
        drop(docking);
        drop(context);
    }));
    assert!(
        result.is_ok(),
        "dropping Context must intentionally clear any retained renderer deltas"
    );
}

#[test]
fn shared_run_with_a_popup_keeps_both_dockspace_proofs_valid() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let mut left = Dockspace::builder("owned-run-audit-left", layout())
        .build()
        .unwrap();
    let mut right = Dockspace::builder("owned-run-audit-right", layout())
        .build()
        .unwrap();
    let mut left_panes = Panes::default();
    let mut right_panes = Panes::default();
    let mut left_anchor = None;

    let mut advance = |step: i32, events: Vec<Event>| {
        let mut capabilities = Vec::new();
        let output = run_ui(&context, input(f64::from(step), events), |ui| {
            ui.columns(2, |columns| {
                capabilities.push(
                    left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                        .unwrap()
                        .presentation_settlement_capability(),
                );
                capabilities.push(
                    right
                        .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                        .unwrap()
                        .presentation_settlement_capability(),
                );
            });
        });
        let (output, receipt) = output.into_parts();
        let mut menu_visible = false;
        if let Some(update) = &output.platform_output.accesskit_update {
            for (id, node) in &update.nodes {
                if node.label() == Some("Presentation commands")
                    && node.bounds().is_some_and(|bounds| bounds.x0 < 400.0)
                    && node.role() == Role::Button
                {
                    left_anchor = Some(*id);
                }
                menu_visible |= node.role() == Role::Menu
                    || (node.role() == Role::MenuItem && node.label() == Some("Float"));
            }
        }
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        (capabilities, menu_visible, left_anchor)
    };

    let mut anchor = None;
    for step in 0..5 {
        anchor = advance(step, vec![]).2;
    }
    let target = anchor.expect("the left command anchor is present in AccessKit output");
    advance(
        5,
        vec![Event::AccessKitActionRequest(ActionRequest {
            action: Action::Click,
            target_tree: TreeId::ROOT,
            target_node: target,
            data: None,
        })],
    );

    let mut saw_menu = false;
    let mut unsupported = Vec::new();
    for step in 6..11 {
        let (capabilities, menu, _) = advance(step, vec![]);
        saw_menu |= menu;
        for (participant, capability) in capabilities.into_iter().enumerate() {
            if capability != DockspaceCapability::Supported {
                unsupported.push((step, participant, capability));
            }
        }
    }
    assert!(saw_menu, "the reproduction must paint a real adapter popup");
    assert!(
        unsupported.is_empty(),
        "both shared-run participants must report Supported: {unsupported:?}"
    );
}

fn sized_audit_areas(context: &Context, namespace: &'static str) -> [egui::Area; 2] {
    let size = egui::vec2(390.0, 560.0);
    let areas = [
        egui::Area::new(egui::Id::unique((namespace, "foreground")))
            .order(egui::Order::Foreground)
            .fixed_pos(Pos2::ZERO)
            .default_size(size)
            .fade_in(false),
        egui::Area::new(egui::Id::unique((namespace, "middle")))
            .order(egui::Order::Middle)
            .fixed_pos(egui::pos2(410.0, 0.0))
            .default_size(size)
            .fade_in(false),
    ];
    // A new Area first measures with an invisible painter, which replaces
    // private markers with Noop. Finish that real sizing pass before asking
    // Dockspace to emit output that can authorize presentation.
    context
        .run_ui(input(0.0, vec![]), |ui| {
            for area in &areas {
                area.clone().show(ui, |area_ui| {
                    assert!(area_ui.is_sizing_pass());
                    assert!(!area_ui.is_visible());
                    area_ui.set_min_size(size);
                });
            }
        })
        .drop_without_applying_deltas();
    areas
}

#[test]
fn shared_run_accepts_callback_order_different_from_layer_paint_order() {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let mut foreground = Dockspace::builder("owned-run-audit-foreground", layout())
        .build()
        .unwrap();
    let mut middle = Dockspace::builder("owned-run-audit-middle", layout())
        .build()
        .unwrap();
    let mut foreground_panes = Panes::default();
    let mut middle_panes = Panes::default();
    let [foreground_area, middle_area] = sized_audit_areas(&context, "audit-reverse-order");
    let foreground_layer = foreground_area.layer();
    let middle_layer = middle_area.layer();

    for step in 0..6 {
        let mut capabilities = [None, None];
        let mut callback_order = Vec::new();

        let output = run_ui(&context, input(f64::from(step + 1), vec![]), |ui| {
            foreground_area.clone().show(ui, |area_ui| {
                assert!(!area_ui.is_sizing_pass());
                assert!(area_ui.is_visible());
                assert_eq!(area_ui.painter().opacity(), 1.0);
                callback_order.push(area_ui.layer_id());
                capabilities[0] = Some(
                    foreground
                        .show_single_surface(SURFACE, area_ui, &mut foreground_panes)
                        .unwrap()
                        .presentation_settlement_capability(),
                );
            });
            middle_area.clone().show(ui, |area_ui| {
                assert!(!area_ui.is_sizing_pass());
                assert!(area_ui.is_visible());
                assert_eq!(area_ui.painter().opacity(), 1.0);
                callback_order.push(area_ui.layer_id());
                capabilities[1] = Some(
                    middle
                        .show_single_surface(SURFACE, area_ui, &mut middle_panes)
                        .unwrap()
                        .presentation_settlement_capability(),
                );
            });
        });

        assert_eq!(callback_order, [foreground_layer, middle_layer]);
        let paint_order = context.memory(|memory| memory.layer_ids().collect::<Vec<_>>());
        let middle_rank = paint_order
            .iter()
            .position(|layer| *layer == middle_layer)
            .expect("the Middle area is registered for this pass");
        let foreground_rank = paint_order
            .iter()
            .position(|layer| *layer == foreground_layer)
            .expect("the Foreground area is registered for this pass");
        assert!(
            middle_rank < foreground_rank,
            "egui paints Middle before Foreground"
        );
        let capabilities = capabilities.map(|capability| capability.unwrap());
        assert_eq!(
            capabilities,
            [DockspaceCapability::Supported; 2],
            "visible step {step} must settle even when callbacks ran in the reverse layer order"
        );

        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }
}

struct SwapLayerSpans {
    armed: Arc<AtomicBool>,
    did_swap: Arc<AtomicBool>,
}

impl egui::plugin::Plugin for SwapLayerSpans {
    fn debug_name(&self) -> &'static str {
        "owned_run_audit::swap_layer_spans"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if !self.armed.swap(false, Ordering::Relaxed) {
            return;
        }
        let mut callback_groups: Vec<(TypeId, Vec<usize>)> = Vec::new();
        for (index, shape) in output.shapes.iter().enumerate() {
            let egui::Shape::Callback(callback) = &shape.shape else {
                continue;
            };
            let callback_type = callback.callback.as_ref().type_id();
            if let Some((_, indices)) = callback_groups
                .iter_mut()
                .find(|(known_type, _)| *known_type == callback_type)
            {
                indices.push(index);
            } else {
                callback_groups.push((callback_type, vec![index]));
            }
        }
        let Some((_, markers)) = callback_groups
            .into_iter()
            .find(|(_, indices)| indices.len() == 4)
        else {
            return;
        };

        // Keep every shape and marker of each primary span intact. Only their
        // placement changes; egui's completed layer order remains the witness.
        let first_start = markers[0];
        let first_end = markers[1] + 1;
        let second_start = markers[2];
        let second_end = markers[3] + 1;
        let first = output.shapes[first_start..first_end].to_vec();
        let gap = output.shapes[first_end..second_start].to_vec();
        let second = output.shapes[second_start..second_end].to_vec();
        let mut swapped = Vec::with_capacity(second_end - first_start);
        swapped.extend_from_slice(&second);
        swapped.extend_from_slice(&gap);
        swapped.extend_from_slice(&first);
        drop(output.shapes.splice(first_start..second_end, swapped));
        assert_eq!(
            output.shapes[first_start..first_start + second.len()],
            second,
            "the second complete span survives at the first span's position"
        );
        assert_eq!(
            output.shapes[second_end - first.len()..second_end],
            first,
            "the first complete span survives at the second span's position"
        );
        self.did_swap.store(true, Ordering::Relaxed);
    }
}

#[test]
fn swapping_two_instances_complete_layer_spans_rejects_both_participants() {
    let context = Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let mut foreground = Dockspace::builder("owned-run-audit-swap-foreground", layout())
        .build()
        .unwrap();
    let mut middle = Dockspace::builder("owned-run-audit-swap-middle", layout())
        .build()
        .unwrap();
    let mut foreground_panes = Panes::default();
    let mut middle_panes = Panes::default();
    let [foreground_area, middle_area] = sized_audit_areas(&context, "audit-swap-order");
    let armed = Arc::new(AtomicBool::new(false));
    let did_swap = Arc::new(AtomicBool::new(false));

    context.add_plugin(SwapLayerSpans {
        armed: Arc::clone(&armed),
        did_swap: Arc::clone(&did_swap),
    });

    for step in 0..8 {
        armed.store(step == 5, Ordering::Relaxed);
        let mut capabilities = [None, None];
        let output = run_ui(&context, input(f64::from(step + 1), vec![]), |ui| {
            foreground_area.clone().show(ui, |area_ui| {
                assert!(!area_ui.is_sizing_pass());
                assert!(area_ui.is_visible());
                assert_eq!(area_ui.painter().opacity(), 1.0);
                capabilities[0] = Some(
                    foreground
                        .show_single_surface(SURFACE, area_ui, &mut foreground_panes)
                        .unwrap()
                        .presentation_settlement_capability(),
                );
            });
            middle_area.clone().show(ui, |area_ui| {
                assert!(!area_ui.is_sizing_pass());
                assert!(area_ui.is_visible());
                assert_eq!(area_ui.painter().opacity(), 1.0);
                capabilities[1] = Some(
                    middle
                        .show_single_surface(SURFACE, area_ui, &mut middle_panes)
                        .unwrap()
                        .presentation_settlement_capability(),
                );
            });
        });
        let (output, receipt) = output.into_parts();
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
        let expected = if step == 6 {
            DockspaceCapability::Unavailable(DockspaceUnavailableReason::PresentationOutputMismatch)
        } else {
            DockspaceCapability::Supported
        };
        assert_eq!(
            capabilities.map(Option::unwrap),
            [expected; 2],
            "visible step {step} must reject both participants only after the complete span swap"
        );
        if step < 5 {
            assert!(!did_swap.load(Ordering::Relaxed));
        } else {
            assert!(
                did_swap.load(Ordering::Relaxed),
                "both complete spans were swapped"
            );
        }
    }
}

struct RemovePointerInput;

impl egui::plugin::Plugin for RemovePointerInput {
    fn debug_name(&self) -> &'static str {
        "owned_run_audit::remove_pointer_input"
    }

    fn input_hook(&mut self, _context: &Context, input: &mut RawInput) {
        input.events.retain(|event| {
            !matches!(
                event,
                Event::PointerMoved(_) | Event::PointerButton { .. } | Event::PointerGone
            )
        });
    }
}

struct InjectPointerInput {
    position: Pos2,
    armed: bool,
}

impl egui::plugin::Plugin for InjectPointerInput {
    fn debug_name(&self) -> &'static str {
        "owned_run_audit::inject_pointer_input_once"
    }

    fn input_hook(&mut self, _context: &Context, input: &mut RawInput) {
        if !std::mem::take(&mut self.armed) {
            return;
        }
        input.events.extend([
            Event::PointerMoved(self.position),
            Event::PointerButton {
                pos: self.position,
                button: egui::PointerButton::Primary,
                pressed: true,
                modifiers: egui::Modifiers::NONE,
            },
        ]);
    }
}

#[test]
fn owned_run_snapshots_pointer_events_before_dockspace_appears_on_second_pass() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 2.try_into().unwrap());
    let mut docking = Dockspace::builder("owned-run-audit-second-pass-input", layout())
        .build()
        .unwrap();
    let mut panes = Panes::default();
    let mut second_center = None;

    for step in 0..5 {
        let (output, _) = docking
            .run_single_surface(
                &context,
                input(f64::from(step), vec![]),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        second_center = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(_, node)| {
                    if node.role() == Role::Tab && node.label() == Some("Pane 2") {
                        node.bounds().map(|bounds| {
                            egui::pos2(
                                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                                ((bounds.y0 + bounds.y1) * 0.5) as f32,
                            )
                        })
                    } else {
                        None
                    }
                })
            });
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }

    let position = second_center.expect("the inactive tab is present in AccessKit output");
    context.add_plugin(InjectPointerInput {
        position,
        armed: true,
    });
    let mut calls = 0;
    let mut capability = None;
    let output = run_ui(&context, input(5.0, vec![]), |ui| {
        if calls == 0 {
            ui.ctx()
                .request_discard("defer Dockspace until the next pass");
        } else {
            capability = Some(
                docking
                    .show_single_surface(SURFACE, ui, &mut panes)
                    .unwrap()
                    .presentation_settlement_capability(),
            );
        }
        calls += 1;
    });

    assert_eq!(calls, 2, "Dockspace first appears on the second egui pass");
    assert_eq!(capability, Some(DockspaceCapability::Supported));
    assert!(
        context.input(|input| input.raw.events.is_empty()),
        "the second pass has consumed RawInput even though the run snapshot retains pass one"
    );
    let (output, receipt) = output.into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(
        !docking.view().item(ItemId::new(1)).unwrap().is_selected(),
        "the first-pass post-hook click must reach Dockspace on its first appearance"
    );
    assert!(docking.view().item(ItemId::new(2)).unwrap().is_selected());
}

#[test]
fn owned_run_pointer_input_uses_the_post_hook_event_set() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let mut docking = Dockspace::builder("owned-run-audit-input-hook", layout())
        .build()
        .unwrap();
    let mut panes = Panes::default();
    let mut second_center = None;

    for step in 0..5 {
        let (output, _) = docking
            .run_single_surface(
                &context,
                input(f64::from(step), vec![]),
                SURFACE,
                &mut panes,
            )
            .unwrap();
        let (output, receipt) = output.into_parts();
        second_center = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(_, node)| {
                    if node.role() == Role::Tab && node.label() == Some("Pane 2") {
                        node.bounds().map(|bounds| {
                            egui::pos2(
                                ((bounds.x0 + bounds.x1) * 0.5) as f32,
                                ((bounds.y0 + bounds.y1) * 0.5) as f32,
                            )
                        })
                    } else {
                        None
                    }
                })
            });
        output.drop_without_applying_deltas();
        receipt.report(SurfacePresentationResult::Presented);
    }

    let position = second_center.expect("the inactive tab is present in AccessKit output");
    context.add_plugin(RemovePointerInput);
    let (output, _) = docking
        .run_single_surface(
            &context,
            input(
                5.0,
                vec![
                    Event::PointerMoved(position),
                    Event::PointerButton {
                        pos: position,
                        button: egui::PointerButton::Primary,
                        pressed: true,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            ),
            SURFACE,
            &mut panes,
        )
        .unwrap();
    let (output, receipt) = output.into_parts();
    assert!(
        context.input(|input| input.raw.events.is_empty()),
        "egui itself received the hook-filtered event set"
    );
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);
    assert!(
        docking.view().item(ItemId::new(1)).unwrap().is_selected(),
        "the filtered press cannot select the inactive tab"
    );
}
