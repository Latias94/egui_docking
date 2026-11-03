use std::panic::{AssertUnwindSafe, catch_unwind};

use dockspace::policy::DockPolicy;
use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Context, FullOutput, RawInput, Rect, Ui, UiBuilder, ViewportId, vec2};
use egui_dockspace::{
    Dockspace, DockspaceActionOutcome, DockspaceActionStatus, DockspaceAxis, DockspaceCapability,
    DockspaceErrorKind, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceUnavailableReason, ItemId, PaneView, RootId, SurfaceId,
    SurfacePresentationResult, run_ui,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const SECOND_ITEM: ItemId = ItemId::new(2);

#[derive(Default)]
struct SmokePanes {
    paint_count: usize,
    focus_namespace: Option<egui::Id>,
}

impl SmokePanes {
    fn focus_id(&self, item: ItemId) -> egui::Id {
        self.focus_namespace
            .map_or_else(|| pane_focus_target(item), |namespace| namespace.with(item))
    }
}

impl PaneView for SmokePanes {
    fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
        match item {
            ITEM => Some("Official egui pane".into()),
            SECOND_ITEM => Some("Second official pane".into()),
            _ => None,
        }
    }

    fn ui(&mut self, item: ItemId, ui: &mut Ui) {
        if matches!(item, ITEM | SECOND_ITEM) {
            self.paint_count += 1;
            let target = self.focus_id(item);
            let rect = Rect::from_min_size(ui.available_rect_before_wrap().min, vec2(160.0, 24.0));
            let _ = ui.interact(rect, target, egui::Sense::click_and_drag());
            ui.label("Rendered through the public egui_dockspace facade");
        }
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        matches!(item, ITEM | SECOND_ITEM).then(|| self.focus_id(item))
    }
}

fn pane_focus_target(item: ItemId) -> egui::Id {
    egui::Id::unique(("official-egui-pane-focus", item.get()))
}

fn product_layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, DockspaceNode::central_tabs([ITEM, SECOND_ITEM])),
    )])
    .expect("the smoke layout is valid")
}

fn raw_input() -> RawInput {
    raw_input_for(ViewportId::ROOT)
}

fn raw_input_for(viewport: ViewportId) -> RawInput {
    let mut input = RawInput {
        viewport_id: viewport,
        screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(800.0, 600.0))),
        ..RawInput::default()
    };
    input.viewports.entry(viewport).or_default();
    input
}

fn run_headless(
    context: &Context,
    input: RawInput,
    add_contents: impl FnMut(&mut Ui),
) -> FullOutput {
    let (output, receipt) = run_ui(context, input, add_contents).into_parts();
    // This headless owner accepts the terminal frame; callers inspect its output and clear textures.
    receipt.report(SurfacePresentationResult::Presented);
    output
}

fn accesskit_activate(target_node: egui::accesskit::NodeId) -> egui::Event {
    egui::Event::AccessKitActionRequest(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    })
}

fn selected_second_pane() -> (Context, Dockspace, SmokePanes) {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let mut dockspace = Dockspace::builder("official-egui-receipt-focus", product_layout())
        .build()
        .expect("the receipt fixture initializes");
    let mut panes = SmokePanes::default();
    let mut second_tab = None;
    for _ in 0..3 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the receipt fixture bootstraps");
        });
        if let Some(update) = output.platform_output.accesskit_update.as_ref() {
            second_tab = update
                .nodes
                .iter()
                .find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
                .or(second_tab);
        }
        output.textures_delta.clear();
    }
    let mut input = raw_input();
    input.events.push(accesskit_activate(
        second_tab.expect("the second tab is visible"),
    ));
    let mut output = run_headless(&context, input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("selection creates a pending pane-focus request");
    });
    output.textures_delta.clear();
    assert!(
        !dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
        "encoding queues the selection without publishing it",
    );
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(panes.focus_id(SECOND_ITEM)),
        "an unpublished selection cannot acquire pane focus",
    );
    // Encoding captures the original UI event; the next update applies it.
    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the queued selection publishes before encoding");
    });
    output.textures_delta.clear();
    assert!(
        dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected())
    );
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(panes.focus_id(SECOND_ITEM)),
        "the selection update publishes its intent before requesting focus",
    );
    (context, dockspace, panes)
}

#[test]
fn official_raw_egui_host_owns_terminal_settlement() {
    let _native_options = eframe::NativeOptions::default();
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let mut policy = DockPolicy::default();
    policy.set_allow_resize_axis(DockspaceAxis::Vertical, false);
    let mut dockspace = Dockspace::builder("official-egui-consumer", product_layout())
        .policy(policy)
        .build()
        .expect("the public facade accepts a valid product layout");
    let view = dockspace.view();
    assert_eq!(
        view.surface(SURFACE)
            .and_then(|surface| surface.main_root())
            .map(|root| root.id()),
        Some(ROOT),
    );

    let prepared = dockspace.prepare_select_item(ITEM);
    assert_eq!(prepared.expected_version(), dockspace.version());
    let action = dockspace
        .submit_prepared_action(prepared)
        .expect("the product action boundary reduces one checked action");
    assert!(matches!(
        action.status(),
        DockspaceActionStatus::Applied(DockspaceActionOutcome::Selected {
            item: ITEM,
            changed: false,
        })
    ));

    let mut panes = SmokePanes::default();
    let mut local_actions_current = false;
    let mut presentation_settlement_capability = None;
    let mut second_tab = None;
    for _ in 0..4 {
        let input = raw_input();
        let mut output = run_headless(&context, input, |ui| {
            let response = dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the public facade advances an official-egui frame");
            assert!(response.missing_panes().is_empty());
            local_actions_current |= response.interaction_capabilities().local_actions_current();
            presentation_settlement_capability =
                Some(response.presentation_settlement_capability());
        });
        second_tab = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
            })
            .or(second_tab);
        output.textures_delta.clear();
    }

    let mut selection_input = raw_input();
    selection_input.events.push(accesskit_activate(
        second_tab.expect("the supported host exposes the second tab"),
    ));
    let mut output = run_headless(&context, selection_input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the supported host selects the second tab");
    });
    output.textures_delta.clear();
    let target = panes.focus_id(SECOND_ITEM);
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));
    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the queued selection publishes its pane-focus intent");
    });
    output.textures_delta.clear();
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(target),
        "the intent must publish before its target can acquire focus",
    );

    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the supported host requests the published pane focus");
    });
    assert_eq!(context.memory(|memory| memory.focused()), Some(target));
    assert_eq!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the terminal output exposes pane focus")
            .focus,
        target.accesskit_id(),
    );
    output.textures_delta.clear();

    let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the exact presented observation completes pane focus");
    })
    .into_parts();
    output.textures_delta.clear();
    assert_eq!(context.memory(|memory| memory.focused()), Some(target));
    // This acknowledgement output must not supply a second Focused receipt.
    receipt.report(SurfacePresentationResult::Dropped);
    context.memory_mut(|memory| memory.surrender_focus(target));
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));
    for _ in 0..2 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the settled request stays complete after focus is surrendered");
        });
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "terminal focus evidence is consumed once instead of reclaiming focus",
        );
    }

    assert!(panes.paint_count > 0, "the product pane must paint");
    assert!(
        local_actions_current,
        "the product facade must become interactive after bootstrap"
    );
    assert_eq!(
        presentation_settlement_capability,
        Some(DockspaceCapability::Supported),
        "the caller-owned run mints exact terminal presentation proof",
    );
    assert!(
        dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
    );
}

#[test]
fn missing_or_dropped_host_receipt_cannot_complete_pane_focus() {
    #[derive(Clone, Copy, Debug)]
    enum HostResult {
        Unreported,
        Dropped,
        Drop,
        Unwind,
    }

    for result in [
        HostResult::Unreported,
        HostResult::Dropped,
        HostResult::Drop,
        HostResult::Unwind,
    ] {
        let (context, mut dockspace, mut panes) = selected_second_pane();
        let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the owned frame observes the pending pane focus");
        })
        .into_parts();
        output.textures_delta.clear();
        let target = panes.focus_id(SECOND_ITEM);
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "the output must acquire the pending focus before {result:?}",
        );
        let pending = match result {
            HostResult::Unreported => Some(receipt),
            HostResult::Dropped => {
                receipt.report(SurfacePresentationResult::Dropped);
                None
            }
            HostResult::Drop => {
                drop(receipt);
                None
            }
            HostResult::Unwind => {
                let interrupted = catch_unwind(AssertUnwindSafe(move || {
                    let _receipt = receipt;
                    panic!("the host renderer panics before reporting its output");
                }));
                assert!(interrupted.is_err());
                None
            }
        };

        context.memory_mut(|memory| memory.surrender_focus(target));
        assert_ne!(context.memory(|memory| memory.focused()), Some(target));
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the unpresented observation leaves pane focus pending");
        });
        output.textures_delta.clear();
        assert_eq!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "{result:?} must leave the request pending so a legal retry reacquires focus",
        );
        drop(pending);
        let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the presented retry completes the exact focus request");
        })
        .into_parts();
        output.textures_delta.clear();
        assert_eq!(context.memory(|memory| memory.focused()), Some(target));
        receipt.report(SurfacePresentationResult::Dropped);
        context.memory_mut(|memory| memory.surrender_focus(target));
        assert_ne!(context.memory(|memory| memory.focused()), Some(target));
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the acknowledged retry no longer reclaims surrendered focus");
        });
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "only the presented retry may complete the request after {result:?}",
        );
    }
}

#[test]
fn out_of_order_host_report_keeps_focus_until_its_exact_core_commit() {
    let (context, mut dockspace, mut panes) = selected_second_pane();
    let target = panes.focus_id(SECOND_ITEM);
    let (mut output_a, receipt_a) = run_ui(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("output A observes the pending focus request");
    })
    .into_parts();
    output_a.textures_delta.clear();
    assert_eq!(context.memory(|memory| memory.focused()), Some(target));
    context.memory_mut(|memory| memory.surrender_focus(target));
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));
    let (mut output_b, receipt_b) = run_ui(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("output B observes the same pending focus request");
    })
    .into_parts();
    assert_eq!(
        output_b
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("output B publishes the real focused pane")
            .focus,
        target.accesskit_id(),
    );
    output_b.textures_delta.clear();
    assert_eq!(
        context.memory(|memory| memory.focused()),
        Some(target),
        "the unreported output A cannot complete the pending request",
    );
    receipt_b.report(SurfacePresentationResult::Presented);
    context.memory_mut(|memory| memory.surrender_focus(target));
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));

    let (mut output_c, receipt_c) = run_ui(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("B's report waits for the earlier unreported output A");
    })
    .into_parts();
    output_c.textures_delta.clear();
    assert_eq!(
        context.memory(|memory| memory.focused()),
        Some(target),
        "B's report cannot settle focus before core accepts its exact prefix; C must reacquire it",
    );

    // Keep C unreported so its result cannot replace B in the next core acknowledgement.
    receipt_a.report(SurfacePresentationResult::Dropped);
    context.memory_mut(|memory| memory.surrender_focus(target));
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));
    let (mut output_d, receipt_d) = run_ui(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("retiring A lets core accept B's already reported focus observation");
    })
    .into_parts();
    output_d.textures_delta.clear();
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(target),
        "retiring A must accept B's retained exact observation without reclaiming focus",
    );
    drop((receipt_c, receipt_d));
    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("later receipt retirement preserves B's completed request");
    });
    output.textures_delta.clear();
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));
}

#[test]
fn delayed_host_receipt_cannot_replace_a_newer_output() {
    for result in [
        Some(SurfacePresentationResult::Presented),
        Some(SurfacePresentationResult::Dropped),
        None,
    ] {
        let (context, mut dockspace, mut panes) = selected_second_pane();
        let (mut output, old_receipt) = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the first output remains unreported");
        })
        .into_parts();
        let first_tab = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Official egui pane"))
                        .then_some(*id)
                })
            });
        output.textures_delta.clear();

        let mut input = raw_input();
        input.events.push(accesskit_activate(
            first_tab.expect("the first tab is visible"),
        ));
        let (mut output, selection_receipt) = run_ui(&context, input, |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("a different pane supersedes the old focus request");
        })
        .into_parts();
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(panes.focus_id(ITEM)),
            "the newly queued selection cannot focus its unpublished target",
        );
        selection_receipt.report(SurfacePresentationResult::Dropped);
        let (mut output, applied_receipt) = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the queued selection publishes before issuing its focus request");
        })
        .into_parts();
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(panes.focus_id(ITEM)),
            "the replacement intent must publish before it can request pane focus",
        );
        applied_receipt.report(SurfacePresentationResult::Dropped);
        let (mut output, current_receipt) = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the current output observes focus for the newly selected pane");
        })
        .into_parts();
        output.textures_delta.clear();
        let target = panes.focus_id(ITEM);
        assert_eq!(context.memory(|memory| memory.focused()), Some(target));
        current_receipt.report(SurfacePresentationResult::Presented);
        match result {
            Some(result) => old_receipt.report(result),
            None => drop(old_receipt),
        }
        let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the current output retains its own presentation result");
        })
        .into_parts();
        output.textures_delta.clear();
        assert_eq!(context.memory(|memory| memory.focused()), Some(target));
        receipt.report(SurfacePresentationResult::Dropped);
        context.memory_mut(|memory| memory.surrender_focus(target));
        assert_ne!(context.memory(|memory| memory.focused()), Some(target));
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the current request stays complete after the delayed old receipt");
        });
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "an old {result:?} receipt cannot prevent the current request from completing",
        );
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(panes.focus_id(SECOND_ITEM)),
            "a delayed receipt cannot restore the superseded pane's focus",
        );
    }
}

#[test]
fn official_eframe_app_reports_missing_terminal_settlement() {
    struct OfficialApp {
        dockspace: Dockspace,
        panes: SmokePanes,
        local_actions_current: bool,
        presentation_settlement_capability: Option<DockspaceCapability>,
        pane_focus_capability: Option<DockspaceCapability>,
    }

    impl eframe::App for OfficialApp {
        fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
            let response = self
                .dockspace
                .show_single_surface(SURFACE, ui, &mut self.panes)
                .expect("the eframe application advances its Dockspace frame");
            self.local_actions_current |=
                response.interaction_capabilities().local_actions_current();
            self.presentation_settlement_capability =
                Some(response.presentation_settlement_capability());
            self.pane_focus_capability = Some(response.pane_focus_capability());
        }
    }

    let context = Context::default();
    context.enable_accesskit();
    let mut app = OfficialApp {
        dockspace: Dockspace::builder("official-eframe-app", product_layout())
            .build()
            .expect("the official eframe application initializes"),
        panes: SmokePanes::default(),
        local_actions_current: false,
        presentation_settlement_capability: None,
        pane_focus_capability: None,
    };
    let mut frame = eframe::Frame::_new_kittest();

    let mut stable_second_tab = None;
    for _ in 0..4 {
        let mut output = context.run_ui(raw_input(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        if let Some(node) = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
            })
        {
            stable_second_tab = Some(node);
        }
        output.textures_delta.clear();
    }
    let second_tab = stable_second_tab.expect("the stable eframe output exposes the second tab");
    let mut selection_input = raw_input();
    selection_input.events.push(accesskit_activate(second_tab));
    let mut output = context.run_ui(selection_input, |ui| {
        eframe::App::ui(&mut app, ui, &mut frame);
    });
    output.textures_delta.clear();
    for _ in 0..3 {
        let mut output = context.run_ui(raw_input(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        output.textures_delta.clear();
    }

    assert!(app.panes.paint_count > 0, "the eframe pane must paint");
    assert!(
        app.local_actions_current,
        "an ordinary eframe App keeps current-pass local actions"
    );
    assert_eq!(
        app.presentation_settlement_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
        "eframe owns the surrounding run and cannot mint a terminal receipt in App::ui",
    );
    assert_eq!(
        app.pane_focus_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        )),
        "pane-focus observation cannot settle without terminal presentation proof",
    );
    assert!(
        app.dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
        "the AccessKit action must select the second tab through App::ui",
    );
    let target = app.panes.focus_id(SECOND_ITEM);
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(target),
        "the eframe callback cannot acquire pane focus without terminal presentation authority",
    );
    context.memory_mut(|memory| memory.surrender_focus(target));
    for _ in 0..2 {
        let mut output = context.run_ui(raw_input(), |ui| {
            eframe::App::ui(&mut app, ui, &mut frame);
        });
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "a capability-negative eframe callback cannot reclaim pane focus",
        );
    }
}

#[test]
fn manual_pass_keeps_local_actions_but_cannot_mint_terminal_evidence() {
    let context = Context::default();
    context.enable_accesskit();
    let mut dockspace = Dockspace::builder("official-egui-manual-pass", product_layout())
        .build()
        .expect("the manual-pass Dockspace initializes");
    let mut panes = SmokePanes::default();
    let mut second_tab = None;

    for _ in 0..4 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the owned fixture bootstraps");
        });
        second_tab = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
            })
            .or(second_tab);
        output.textures_delta.clear();
    }

    let mut input = raw_input();
    input.events.push(accesskit_activate(
        second_tab.expect("the owned output exposes the second tab"),
    ));
    context.begin_pass(input);
    let response = context.root_ui(|ui| {
        ui.scope_builder(
            UiBuilder::new().id_salt("egui_dockspace::official_run_scope"),
            |ui| dockspace.show_single_surface(SURFACE, ui, &mut panes),
        )
        .inner
        .expect("the manual pass keeps current-call local actions")
    });
    assert!(response.interaction_capabilities().local_actions_current());
    assert_eq!(
        response.presentation_settlement_capability(),
        DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementRequired,
        ),
    );
    let mut output = context.end_pass();
    output.textures_delta.clear();
    assert!(
        !dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
        "the manual pass queues its discrete AccessKit action for the next update",
    );
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(panes.focus_id(SECOND_ITEM)),
        "a manual pass cannot focus an unpublished selection",
    );

    let mut capability = None;
    let mut output = run_headless(&context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("a later owned pass advances normally");
        capability = Some(response.presentation_settlement_capability());
    });
    output.textures_delta.clear();
    assert!(
        dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
        "the queued manual-pass action publishes once in the next update",
    );
    assert_eq!(capability, Some(DockspaceCapability::Supported));
    assert_ne!(
        context.memory(|memory| memory.focused()),
        Some(panes.focus_id(SECOND_ITEM)),
        "the later owned update must publish the intent before requesting focus",
    );
}

#[test]
fn caller_owned_run_fans_out_terminal_settlement_to_multiple_dockspaces() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let mut left = Dockspace::builder("official-egui-left", product_layout())
        .build()
        .expect("the left Dockspace initializes");
    let mut right = Dockspace::builder("official-egui-right", product_layout())
        .build()
        .expect("the right Dockspace initializes");
    let mut left_panes = SmokePanes::default();
    let mut right_panes = SmokePanes {
        focus_namespace: Some(egui::Id::unique("official-right-pane")),
        ..SmokePanes::default()
    };
    let mut capabilities = None;
    let mut second_tabs = Vec::new();

    for _ in 0..4 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            ui.columns(2, |columns| {
                let left_response = left
                    .show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                    .expect("the left Dockspace advances");
                let right_response = right
                    .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                    .expect("the right Dockspace advances");
                capabilities = Some((
                    left_response.presentation_settlement_capability(),
                    right_response.presentation_settlement_capability(),
                ));
            });
        });
        if let Some(update) = output.platform_output.accesskit_update.as_ref() {
            second_tabs = update
                .nodes
                .iter()
                .filter_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
                .collect();
        }
        output.textures_delta.clear();
    }

    assert_eq!(
        second_tabs.len(),
        2,
        "the two Dockspaces expose distinct semantic second tabs",
    );
    assert_ne!(second_tabs[0], second_tabs[1]);
    let left_target = left_panes.focus_id(SECOND_ITEM);
    let right_target = right_panes.focus_id(SECOND_ITEM);
    assert_ne!(left_target, right_target);

    // One Context has one focus owner. Settle each participant in a shared run,
    // then surrender its focus before activating the other participant.
    for second_tab in second_tabs {
        let was_selected = (
            left.view()
                .item(SECOND_ITEM)
                .is_some_and(|item| item.is_selected()),
            right
                .view()
                .item(SECOND_ITEM)
                .is_some_and(|item| item.is_selected()),
        );
        let mut selection_input = raw_input();
        selection_input.events.push(accesskit_activate(second_tab));
        let mut output = run_headless(&context, selection_input, |ui| {
            ui.columns(2, |columns| {
                left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                    .expect("the left Dockspace queues its selection if targeted");
                right
                    .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                    .expect("the right Dockspace queues its selection if targeted");
            });
        });
        output.textures_delta.clear();
        assert_eq!(
            (
                left.view()
                    .item(SECOND_ITEM)
                    .is_some_and(|item| item.is_selected()),
                right
                    .view()
                    .item(SECOND_ITEM)
                    .is_some_and(|item| item.is_selected()),
            ),
            was_selected,
            "the shared encoding run cannot publish either queued selection",
        );
        assert_ne!(context.memory(|memory| memory.focused()), Some(left_target));
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(right_target)
        );

        let mut output = run_headless(&context, raw_input(), |ui| {
            ui.columns(2, |columns| {
                left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                    .expect("the left Dockspace publishes any queued selection");
                right
                    .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                    .expect("the right Dockspace publishes any queued selection");
            });
        });
        output.textures_delta.clear();
        let is_selected = (
            left.view()
                .item(SECOND_ITEM)
                .is_some_and(|item| item.is_selected()),
            right
                .view()
                .item(SECOND_ITEM)
                .is_some_and(|item| item.is_selected()),
        );
        let target = match (was_selected, is_selected) {
            ((false, previous), (true, current)) if previous == current => left_target,
            ((previous, false), (current, true)) if previous == current => right_target,
            state => panic!("one semantic tab must select exactly one Dockspace: {state:?}"),
        };
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "the participant must publish its intent before acquiring focus",
        );

        let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
            ui.columns(2, |columns| {
                left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                    .expect("the shared output includes the left participant");
                right
                    .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                    .expect("the shared output includes the right participant");
            });
        })
        .into_parts();
        assert_eq!(context.memory(|memory| memory.focused()), Some(target));
        assert_eq!(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("the shared output exposes its sole pane focus owner")
                .focus,
            target.accesskit_id(),
        );
        output.textures_delta.clear();
        receipt.report(SurfacePresentationResult::Presented);

        let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
            ui.columns(2, |columns| {
                left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                    .expect("the shared receipt reaches the left participant");
                right
                    .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                    .expect("the shared receipt reaches the right participant");
            });
        })
        .into_parts();
        output.textures_delta.clear();
        assert_eq!(context.memory(|memory| memory.focused()), Some(target));
        receipt.report(SurfacePresentationResult::Dropped);
        context.memory_mut(|memory| memory.surrender_focus(target));
        assert_ne!(context.memory(|memory| memory.focused()), Some(target));
        for _ in 0..2 {
            let mut output = run_headless(&context, raw_input(), |ui| {
                ui.columns(2, |columns| {
                    left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                        .expect("the left participant preserves completed focus requests");
                    right
                        .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                        .expect("the right participant preserves completed focus requests");
                });
            });
            output.textures_delta.clear();
            assert_ne!(
                context.memory(|memory| memory.focused()),
                Some(left_target),
                "the left participant must not replay a settled focus request",
            );
            assert_ne!(
                context.memory(|memory| memory.focused()),
                Some(right_target),
                "the right participant must not replay a settled focus request",
            );
        }
    }

    assert!(left_panes.paint_count > 0);
    assert!(right_panes.paint_count > 0);
    assert_eq!(
        capabilities,
        Some((
            DockspaceCapability::Supported,
            DockspaceCapability::Supported
        )),
    );
    assert!(
        left.view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected())
            && right
                .view()
                .item(SECOND_ITEM)
                .is_some_and(|item| item.is_selected()),
    );
}

#[test]
fn repeated_show_in_one_pass_survives_context_rebinding_and_then_recovers() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-duplicate", product_layout())
        .build()
        .expect("the duplicate-show Dockspace initializes");
    let mut panes = SmokePanes::default();

    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the first contribution is accepted");
        let error = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect_err("the same Dockspace cannot contribute twice to one pass");
        assert_eq!(error.kind(), DockspaceErrorKind::OperationConflict);
    });
    output.textures_delta.clear();

    let rebound_context = Context::default();
    let mut poisoned_capability = None;
    let mut output = run_headless(&rebound_context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the rebound run consumes the abandonment tombstone");
        poisoned_capability = Some(response.presentation_settlement_capability());
    });
    output.textures_delta.clear();
    assert_eq!(
        poisoned_capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementAbandoned,
        )),
    );

    let mut recovered_capability = None;
    let mut output = run_headless(&rebound_context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("a fresh exact run recovers");
        recovered_capability = Some(response.presentation_settlement_capability());
    });
    output.textures_delta.clear();
    assert_eq!(recovered_capability, Some(DockspaceCapability::Supported));
}

#[test]
fn recoverable_error_after_claim_abandons_the_exact_terminal_pass() {
    struct InvalidMinimumPanes(SmokePanes);

    impl PaneView for InvalidMinimumPanes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            self.0.title(item)
        }

        fn ui(&mut self, item: ItemId, ui: &mut Ui) {
            self.0.ui(item, ui);
        }

        fn minimum_size(&self, _item: ItemId) -> egui::Vec2 {
            egui::Vec2::new(f32::NAN, 0.0)
        }
    }

    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-unstaged-error", product_layout())
        .build()
        .expect("the recoverable-error Dockspace initializes");
    let mut panes = SmokePanes::default();
    for _ in 0..3 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the valid fixture bootstraps");
        });
        output.textures_delta.clear();
    }

    let mut invalid = InvalidMinimumPanes(SmokePanes::default());
    let mut output = run_headless(&context, raw_input(), |ui| {
        let error = dockspace
            .show_single_surface(SURFACE, ui, &mut invalid)
            .expect_err("invalid measurement fails after the settlement claim");
        assert_eq!(error.kind(), DockspaceErrorKind::InvalidConfiguration);
    });
    output.textures_delta.clear();

    let mut capability = None;
    let mut output = run_headless(&context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the next exact run remains usable");
        capability = Some(response.presentation_settlement_capability());
    });
    output.textures_delta.clear();
    assert_eq!(
        capability,
        Some(DockspaceCapability::Unavailable(
            DockspaceUnavailableReason::PresentationSettlementAbandoned,
        )),
    );
}

#[test]
fn foreign_root_ui_cannot_claim_an_active_wrapper_pass() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-foreign-ui", product_layout())
        .build()
        .expect("the foreign-ui Dockspace initializes");
    let mut panes = SmokePanes::default();

    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the tagged wrapper UI contributes once");
        let mut foreign = Ui::new(
            context.clone(),
            egui::Id::unique("official-egui-foreign-root"),
            UiBuilder::new().max_rect(context.viewport_rect()),
        );
        let error = dockspace
            .show_single_surface(SURFACE, &mut foreign, &mut panes)
            .expect_err("an untagged root UI cannot borrow the outer receipt");
        assert_eq!(error.kind(), DockspaceErrorKind::OperationConflict);
    });
    output.textures_delta.clear();
}

#[test]
fn denied_max_pass_discard_still_terminally_settles_output() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let mut dockspace = Dockspace::builder("official-egui-max-pass", product_layout())
        .build()
        .expect("the max-pass Dockspace initializes");
    let mut panes = SmokePanes::default();
    let mut capability = None;
    let mut second_tab = None;

    for _ in 0..4 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the max-pass fixture bootstraps");
        });
        second_tab = output
            .platform_output
            .accesskit_update
            .as_ref()
            .and_then(|update| {
                update.nodes.iter().find_map(|(id, node)| {
                    (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                        .then_some(*id)
                })
            })
            .or(second_tab);
        output.textures_delta.clear();
    }

    let mut selection_input = raw_input();
    selection_input.events.push(accesskit_activate(
        second_tab.expect("the max-pass fixture exposes the second tab"),
    ));
    let mut output = run_headless(&context, selection_input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the max-pass fixture selects the second tab");
    });
    output.textures_delta.clear();

    let mut output = run_headless(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the selection publishes before the max-pass focus output");
    });
    output.textures_delta.clear();
    let target = panes.focus_id(SECOND_ITEM);
    assert!(
        dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
    );
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));

    let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the max-pass frame advances");
        capability = Some(response.presentation_settlement_capability());
        ui.ctx()
            .request_discard("the configured max-pass boundary denies this request");
    })
    .into_parts();
    assert_eq!(output.platform_output.num_completed_passes, 1);
    assert_eq!(context.memory(|memory| memory.focused()), Some(target));
    assert_eq!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the denied discard keeps its terminal focused output")
            .focus,
        target.accesskit_id(),
    );
    output.textures_delta.clear();
    assert_eq!(capability, Some(DockspaceCapability::Supported));
    receipt.report(SurfacePresentationResult::Presented);

    let (mut output, receipt) = run_ui(&context, raw_input(), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the denied discard's exact terminal observation completes focus");
    })
    .into_parts();
    output.textures_delta.clear();
    assert_eq!(context.memory(|memory| memory.focused()), Some(target));
    receipt.report(SurfacePresentationResult::Dropped);
    context.memory_mut(|memory| memory.surrender_focus(target));
    assert_ne!(context.memory(|memory| memory.focused()), Some(target));
    for _ in 0..2 {
        let mut output = run_headless(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the max-pass terminal acknowledgement stays consumed");
        });
        output.textures_delta.clear();
        assert_ne!(
            context.memory(|memory| memory.focused()),
            Some(target),
            "the max-pass terminal output settles focus exactly once",
        );
    }
}

#[test]
fn non_root_caller_owned_run_settles_focus_from_its_final_accesskit_output() {
    let context = Context::default();
    context.enable_accesskit();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one is non-zero");
    });
    let child = ViewportId::from_hash_of("official-egui-child");
    let mut dockspace = Dockspace::builder("official-egui-child-dockspace", product_layout())
        .build()
        .expect("the child Dockspace initializes");
    let mut panes = SmokePanes::default();

    let mut second_tab = None;
    for _ in 0..4 {
        let mut output = run_headless(&context, raw_input_for(child), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the child viewport bootstraps");
        });
        if let Some(update) = output.platform_output.accesskit_update.as_ref() {
            second_tab = update.nodes.iter().find_map(|(id, node)| {
                (node.role() == Role::Tab && node.label() == Some("Second official pane"))
                    .then_some(*id)
            });
        }
        output.textures_delta.clear();
    }
    let mut selection_input = raw_input_for(child);
    selection_input.events.push(accesskit_activate(
        second_tab.expect("the child output exposes its second tab"),
    ));
    let mut output = run_headless(&context, selection_input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the child tab selection advances");
        assert_ne!(
            ui.memory(|memory| memory.focused()),
            Some(panes.focus_id(SECOND_ITEM)),
            "the child selection cannot acquire focus before publication",
        );
    });
    output.textures_delta.clear();

    let target = panes.focus_id(SECOND_ITEM);
    let mut output = run_headless(&context, raw_input_for(child), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the child update publishes its pane-focus intent");
        assert_ne!(
            ui.memory(|memory| memory.focused()),
            Some(target),
            "the child intent must publish before requesting focus",
        );
    });
    output.textures_delta.clear();
    assert!(
        dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
    );

    let (mut output, receipt) = run_ui(&context, raw_input_for(child), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the published child intent acquires its pane focus");
        assert_eq!(ui.memory(|memory| memory.focused()), Some(target));
    })
    .into_parts();
    assert_eq!(
        output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the child final output exposes its focused pane node")
            .focus,
        target.accesskit_id(),
        "the child final output owns the pane focus node",
    );
    output.textures_delta.clear();
    receipt.report(SurfacePresentationResult::Presented);

    let (mut output, receipt) = run_ui(&context, raw_input_for(child), |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the exact child terminal observation completes pane focus");
        assert_eq!(ui.memory(|memory| memory.focused()), Some(target));
        ui.memory_mut(|memory| memory.surrender_focus(target));
        assert_ne!(ui.memory(|memory| memory.focused()), Some(target));
    })
    .into_parts();
    output.textures_delta.clear();
    receipt.report(SurfacePresentationResult::Dropped);
    for _ in 0..2 {
        let mut output = run_headless(&context, raw_input_for(child), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the child acknowledgement stays complete after surrendering focus");
            assert_ne!(
                ui.memory(|memory| memory.focused()),
                Some(target),
                "the exact child observation completes the request instead of reclaiming focus",
            );
        });
        assert_ne!(
            output
                .platform_output
                .accesskit_update
                .as_ref()
                .expect("the child final output reports its current focus owner")
                .focus,
            target.accesskit_id(),
            "the terminal child output must preserve surrendered focus",
        );
        output.textures_delta.clear();
    }
}
