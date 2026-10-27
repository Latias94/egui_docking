use std::cell::Cell;

use dockspace::policy::DockPolicy;
use egui::accesskit::{Action, ActionRequest, Role, TreeId};
use egui::{Context, RawInput, Rect, Ui, UiBuilder, ViewportId, vec2};
use egui_dockspace::{
    Dockspace, DockspaceActionOutcome, DockspaceActionStatus, DockspaceAxis, DockspaceCapability,
    DockspaceErrorKind, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, DockspaceUnavailableReason, ItemId, PaneView, RootId, SurfaceId,
    run_ui,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const ITEM: ItemId = ItemId::new(1);
const SECOND_ITEM: ItemId = ItemId::new(2);

#[derive(Default)]
struct SmokePanes {
    paint_count: usize,
    focus_target_requests: Cell<usize>,
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
            let target = pane_focus_target(item);
            let rect = Rect::from_min_size(ui.available_rect_before_wrap().min, vec2(160.0, 24.0));
            let _ = ui.interact(rect, target, egui::Sense::click_and_drag());
            ui.label("Rendered through the public egui_dockspace facade");
        }
    }

    fn focus_target(&self, item: ItemId) -> Option<egui::Id> {
        matches!(item, ITEM | SECOND_ITEM).then(|| {
            self.focus_target_requests
                .set(self.focus_target_requests.get() + 1);
            pane_focus_target(item)
        })
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

fn accesskit_activate(target_node: egui::accesskit::NodeId) -> egui::Event {
    egui::Event::AccessKitActionRequest(ActionRequest {
        action: Action::Click,
        target_tree: TreeId::ROOT,
        target_node,
        data: None,
    })
}

#[test]
fn official_raw_egui_host_owns_terminal_settlement() {
    let _native_options = eframe::NativeOptions::default();
    let context = Context::default();
    context.enable_accesskit();
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
        let mut output = run_ui(&context, input, |ui| {
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
    let mut output = run_ui(&context, selection_input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the supported host selects the second tab");
    });
    output.textures_delta.clear();
    let mut settled_request_count = None;
    for step in 0..5 {
        let mut output = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the supported host settles pane focus");
        });
        output.textures_delta.clear();
        if step == 3 {
            settled_request_count = Some(panes.focus_target_requests.get());
        }
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
    assert_eq!(
        Some(panes.focus_target_requests.get()),
        settled_request_count,
        "terminal focus evidence is consumed once instead of replaying",
    );
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
    let focus_requests_before = app.panes.focus_target_requests.get();
    let mut output = context.run_ui(raw_input(), |ui| {
        eframe::App::ui(&mut app, ui, &mut frame);
    });
    output.textures_delta.clear();
    assert_eq!(
        app.panes.focus_target_requests.get(),
        focus_requests_before,
        "a capability-negative eframe callback must not repeatedly query a focus provider it cannot settle",
    );
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
        let mut output = run_ui(&context, raw_input(), |ui| {
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
        dockspace
            .view()
            .item(SECOND_ITEM)
            .is_some_and(|item| item.is_selected()),
        "the manual pass still commits its discrete AccessKit action",
    );

    let mut capability = None;
    let mut output = run_ui(&context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("a later owned pass advances normally");
        capability = Some(response.presentation_settlement_capability());
    });
    output.textures_delta.clear();
    assert_eq!(capability, Some(DockspaceCapability::Supported));
}

#[test]
fn caller_owned_run_fans_out_terminal_settlement_to_multiple_dockspaces() {
    let context = Context::default();
    context.enable_accesskit();
    let mut left = Dockspace::builder("official-egui-left", product_layout())
        .build()
        .expect("the left Dockspace initializes");
    let mut right = Dockspace::builder("official-egui-right", product_layout())
        .build()
        .expect("the right Dockspace initializes");
    let mut left_panes = SmokePanes::default();
    let mut right_panes = SmokePanes::default();
    let mut capabilities = None;
    let mut second_tabs = Vec::new();

    for _ in 0..4 {
        let mut output = run_ui(&context, raw_input(), |ui| {
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
    let mut selection_input = raw_input();
    selection_input
        .events
        .extend(second_tabs.into_iter().map(accesskit_activate));
    let mut output = run_ui(&context, selection_input, |ui| {
        ui.columns(2, |columns| {
            left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                .expect("the left Dockspace selects its second tab");
            right
                .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                .expect("the right Dockspace selects its second tab");
        });
    });
    output.textures_delta.clear();
    let mut settled_requests = None;
    for step in 0..5 {
        let mut output = run_ui(&context, raw_input(), |ui| {
            ui.columns(2, |columns| {
                left.show_single_surface(SURFACE, &mut columns[0], &mut left_panes)
                    .expect("the left Dockspace settles terminal focus evidence");
                right
                    .show_single_surface(SURFACE, &mut columns[1], &mut right_panes)
                    .expect("the right Dockspace settles terminal focus evidence");
            });
        });
        output.textures_delta.clear();
        if step == 3 {
            settled_requests = Some((
                left_panes.focus_target_requests.get(),
                right_panes.focus_target_requests.get(),
            ));
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
    assert_eq!(
        Some((
            left_panes.focus_target_requests.get(),
            right_panes.focus_target_requests.get(),
        )),
        settled_requests,
        "both participants consume terminal focus results without replay",
    );
}

#[test]
fn repeated_show_in_one_pass_survives_context_rebinding_and_then_recovers() {
    let context = Context::default();
    let mut dockspace = Dockspace::builder("official-egui-duplicate", product_layout())
        .build()
        .expect("the duplicate-show Dockspace initializes");
    let mut panes = SmokePanes::default();

    let mut output = run_ui(&context, raw_input(), |ui| {
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
    let mut output = run_ui(&rebound_context, raw_input(), |ui| {
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
    let mut output = run_ui(&rebound_context, raw_input(), |ui| {
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
        let mut output = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the valid fixture bootstraps");
        });
        output.textures_delta.clear();
    }

    let mut invalid = InvalidMinimumPanes(SmokePanes::default());
    let mut output = run_ui(&context, raw_input(), |ui| {
        let error = dockspace
            .show_single_surface(SURFACE, ui, &mut invalid)
            .expect_err("invalid measurement fails after the settlement claim");
        assert_eq!(error.kind(), DockspaceErrorKind::InvalidConfiguration);
    });
    output.textures_delta.clear();

    let mut capability = None;
    let mut output = run_ui(&context, raw_input(), |ui| {
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

    let mut output = run_ui(&context, raw_input(), |ui| {
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
        let mut output = run_ui(&context, raw_input(), |ui| {
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
    let mut output = run_ui(&context, selection_input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the max-pass fixture selects the second tab");
    });
    output.textures_delta.clear();

    let mut output = run_ui(&context, raw_input(), |ui| {
        let response = dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the max-pass frame advances");
        capability = Some(response.presentation_settlement_capability());
        ui.ctx()
            .request_discard("the configured max-pass boundary denies this request");
    });
    assert_eq!(output.platform_output.num_completed_passes, 1);
    output.textures_delta.clear();
    assert_eq!(capability, Some(DockspaceCapability::Supported));

    let mut settled_request_count = None;
    for step in 0..4 {
        let mut output = run_ui(&context, raw_input(), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the denied discard's terminal evidence remains consumable");
        });
        output.textures_delta.clear();
        if step == 2 {
            settled_request_count = Some(panes.focus_target_requests.get());
        }
    }
    assert_eq!(
        Some(panes.focus_target_requests.get()),
        settled_request_count,
        "the max-pass terminal output settles focus exactly once",
    );
}

#[test]
fn non_root_caller_owned_run_settles_focus_from_its_final_accesskit_output() {
    let context = Context::default();
    context.enable_accesskit();
    let child = ViewportId::from_hash_of("official-egui-child");
    let mut dockspace = Dockspace::builder("official-egui-child-dockspace", product_layout())
        .build()
        .expect("the child Dockspace initializes");
    let mut panes = SmokePanes::default();

    let mut second_tab = None;
    for _ in 0..4 {
        let mut output = run_ui(&context, raw_input_for(child), |ui| {
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
    let mut output = run_ui(&context, selection_input, |ui| {
        dockspace
            .show_single_surface(SURFACE, ui, &mut panes)
            .expect("the child tab selection advances");
    });
    output.textures_delta.clear();

    let mut saw_target_focus = false;
    let mut requests_after_settlement = None;
    let mut observed_accesskit = Vec::new();
    for step in 0..5 {
        let mut output = run_ui(&context, raw_input_for(child), |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, &mut panes)
                .expect("the child viewport frame advances");
        });
        if let Some(update) = output.platform_output.accesskit_update.as_ref() {
            observed_accesskit.push((
                update.focus,
                update
                    .nodes
                    .iter()
                    .map(|(id, node)| (*id, node.role(), node.label().map(str::to_owned)))
                    .collect::<Vec<_>>(),
            ));
            saw_target_focus |= update.focus == pane_focus_target(SECOND_ITEM).accesskit_id();
        }
        output.textures_delta.clear();
        if step == 3 {
            requests_after_settlement = Some(panes.focus_target_requests.get());
        }
    }

    assert!(
        saw_target_focus,
        "the child final output owns the pane focus node; observed={observed_accesskit:?}; requests={}",
        panes.focus_target_requests.get(),
    );
    assert_eq!(
        Some(panes.focus_target_requests.get()),
        requests_after_settlement,
        "the exact child observation completes the focus request instead of replaying it",
    );
}
