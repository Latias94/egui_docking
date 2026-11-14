mod focus_idle;
mod output_backpressure;
mod owned_update;
mod retirement_backpressure;
use std::error::Error as _;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use dockspace::model::{
    DockAnchor, DockPlacement, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::runtime::{
    DockPresentationConfig, SurfacePresentationResult, SurfaceUnavailableReason,
};
use eframe::egui::{Ui, WidgetText};

use super::*;

const SURFACE: SurfaceId = SurfaceId::new(1);

struct TestPanes;

impl PaneView for TestPanes {
    fn title(&self, _item: ItemId) -> Option<WidgetText> {
        Some("Pane".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut Ui) -> egui_dockspace::PaneResponse {
        egui_dockspace::PaneResponse::default()
    }
}

fn test_session() -> dockspace::runtime::DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .expect("the native runtime test layout validates");
    let session = dockspace::runtime::DockspaceSession::from_layout(
        layout,
        dockspace::policy::DockPolicy::default(),
    )
    .expect("the native runtime test session initializes");
    session
}

fn test_state() -> NativeRuntimeState<TestPanes> {
    NativeRuntimeState::new(
        Id::unique("native-runtime-stop-test"),
        test_session(),
        SURFACE,
        TestPanes,
        DockStyle::default(),
        NativeWindowClosePolicy::Cancel,
    )
    .expect("the native runtime test state initializes")
}

#[test]
fn native_construction_preserves_matching_style_and_core_only_settings() {
    let delay = std::time::Duration::from_millis(600);
    let config = DockPresentationConfig::builder()
        .tab_bar_height(36.0)
        .touch_long_press_delay(delay)
        .tab_strip_scroll_line_extent(22.0)
        .pointer_drag_start_distance(9.0)
        .magnetic_center_fraction(0.6)
        .magnetic_outer_band_fraction(0.18)
        .magnetic_outer_band_range(30.0, 80.0)
        .target_release_hysteresis(7.0)
        .build()
        .unwrap();
    let mut session = test_session();
    {
        let mut frame = session.begin_host_frame().unwrap();
        frame.replace_presentation_config(config.clone()).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap();
    }
    let before = session.version();
    let mut style = DockStyle {
        tab_bar_height: 36.0,
        touch_long_press_delay: delay,
        ghost_offset: egui::vec2(7.0, 9.0),
        ..DockStyle::default()
    };
    style.visuals.tab_active_fill = Some(egui::Color32::RED);
    let state = NativeRuntimeState::new(
        Id::unique("native-matching-presentation-config"),
        session,
        SURFACE,
        TestPanes,
        style.clone(),
        NativeWindowClosePolicy::Cancel,
    )
    .expect("core-only configuration and adapter visuals must remain independently owned");
    assert_eq!(state.coordinator.session().presentation_config(), &config);
    assert_eq!(state.coordinator.session().version(), before);
    assert_eq!(state.style, style);
}

#[test]
fn native_construction_rejects_style_geometry_and_timing_mismatches() {
    for (field, style) in [
        (
            "tab_bar_height",
            DockStyle {
                tab_bar_height: 36.0,
                ..DockStyle::default()
            },
        ),
        (
            "touch_long_press_delay",
            DockStyle {
                touch_long_press_delay: std::time::Duration::from_millis(600),
                ..DockStyle::default()
            },
        ),
    ] {
        let error = NativeRuntimeState::new(
            Id::unique(("native-style-mismatch", field)),
            test_session(),
            SURFACE,
            TestPanes,
            style,
            NativeWindowClosePolicy::Cancel,
        )
        .err()
        .expect("a valid style must not silently disagree with its session");
        assert_eq!(error.kind(), NativeRuntimeErrorKind::InvalidConfiguration);
        assert_eq!(
            error
                .source()
                .and_then(|source| source.downcast_ref::<NativeHostProtocolError>())
                .copied(),
            Some(NativeHostProtocolError::StylePresentationMismatch { field })
        );
        assert!(error.source().unwrap().to_string().contains(field));
    }
}

#[test]
fn native_construction_keeps_invalid_style_source() {
    let error = NativeRuntimeState::new(
        Id::unique("native-invalid-renderer-style"),
        test_session(),
        SURFACE,
        TestPanes,
        DockStyle {
            ghost_offset: egui::vec2(f32::NAN, 12.0),
            ..DockStyle::default()
        },
        NativeWindowClosePolicy::Cancel,
    )
    .err()
    .expect("adapter-only style fields still require validation");
    assert_eq!(error.kind(), NativeRuntimeErrorKind::InvalidConfiguration);
    let mut source = error.source();
    let mut style_error = None;
    while let Some(current) = source {
        if let Some(original) = current.downcast_ref::<egui_dockspace::DockStyleError>() {
            style_error = Some(original);
            break;
        }
        source = current.source();
    }
    assert_eq!(
        style_error,
        Some(&egui_dockspace::DockStyleError::NonFinite {
            field: "ghost_offset.x",
        })
    );
}

#[test]
fn standalone_service_commits_overload_cancellation_without_ui_or_an_output_scope() {
    let mut state = test_state();
    let context = egui::Context::default();
    let bridge = state.coordinator.dispatch_bridge();
    bridge.freeze_ordinary_for_test();
    let pass_before = context.cumulative_pass_nr_for(ViewportId::ROOT);
    assert!(eframe::current_native_output_token().is_none());

    // Budget yielding has separate clock-driven tests. This semantic drain
    // must not depend on whether the OS deschedules one call for two milliseconds.
    let clock = dockspace::runtime::NativeMonotonicClock::new(|| std::time::Duration::ZERO);
    let mut budget = NativeInputBudget::new(&clock);
    assert!(
        state
            .service_pending_prefixes(&context, None, SURFACE, &mut budget)
            .unwrap()
    );
    assert_eq!(bridge.admission_dispatch(), NativeDispatch::Idle);
    assert_eq!(
        context.cumulative_pass_nr_for(ViewportId::ROOT),
        pass_before
    );
    assert!(eframe::current_native_output_token().is_none());
    assert_eq!(state.output_backlog().ordinary(), 0);
    assert!(state.error().is_none());

    let mut budget = NativeInputBudget::new(&clock);
    assert!(
        state
            .service_pending_prefixes(&context, None, SURFACE, &mut budget)
            .unwrap()
    );
    assert_eq!(bridge.admission_dispatch(), NativeDispatch::Idle);
}

#[test]
fn control_frames_retain_without_minting_another_output() {
    assert!(should_retain_previous_output(true, false));
    assert!(should_retain_previous_output(false, true));
    assert!(!should_retain_previous_output(false, false));
}

#[test]
fn retained_control_frame_rearms_pending_semantic_work() {
    assert!(retained_pass_requires_semantic_retry(true, false, false));
    assert!(retained_pass_requires_semantic_retry(true, true, true));
    assert!(!retained_pass_requires_semantic_retry(true, true, false));
    assert!(!retained_pass_requires_semantic_retry(false, false, true));
}

#[test]
fn no_frame_invalidation_requests_its_child_until_that_frame_accepts_it() {
    let context = egui::Context::default();
    let child_surface = SurfaceId::new(2);
    let child_viewport = ViewportId::from_hash_of("no-frame-invalidation-child");
    let repaint_viewports = Arc::new(std::sync::Mutex::new(Vec::new()));
    context.set_request_repaint_callback({
        let repaint_viewports = Arc::clone(&repaint_viewports);
        move |info| {
            repaint_viewports
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .push(info.viewport_id);
        }
    });

    let accepted = select_no_frame_invalidations_for_frame(
        &context,
        &[SURFACE],
        [
            (child_surface, child_viewport),
            (child_surface, child_viewport),
        ],
        |(surface, _)| *surface,
        |(_, viewport)| *viewport,
    );

    assert!(accepted.is_empty());
    assert_eq!(
        *repaint_viewports
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()),
        [child_viewport],
        "an out-of-roster child obligation requests exactly its owning callback"
    );

    let accepted = select_no_frame_invalidations_for_frame(
        &context,
        &[child_surface],
        [(child_surface, child_viewport)],
        |(surface, _)| *surface,
        |(_, viewport)| *viewport,
    );
    assert_eq!(accepted, [(child_surface, child_viewport)]);
}

#[test]
fn fatal_overlay_uses_only_a_bottom_status_strip() {
    let available = egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(800.0, 600.0));

    assert_eq!(
        native_error_paint_rect(available, NativeErrorPresentation::NoFrame, 30.0),
        available
    );
    assert_eq!(
        native_error_paint_rect(available, NativeErrorPresentation::RetainedOverlay, 30.0,),
        egui::Rect::from_min_max(egui::pos2(10.0, 590.0), egui::pos2(810.0, 620.0))
    );
}

#[test]
fn retained_overlay_keeps_an_inert_accessible_status_label() {
    let context = egui::Context::default();
    context.enable_accesskit();
    let message = "Native dockspace stopped — test failure";
    let mut probe_enabled = true;
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            paint_native_error_status(ui, NativeErrorPresentation::RetainedOverlay, message);
            probe_enabled = ui.button("mutation probe").enabled();
        },
    );

    let (status_actions, observed_nodes) = {
        let tree = output
            .platform_output
            .accesskit_update
            .as_ref()
            .expect("the status pass publishes accessibility");
        let status_actions = tree.nodes.iter().find_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::Label && node.value() == Some(message)).then(
                || {
                    (
                        node.supports_action(egui::accesskit::Action::Click),
                        node.supports_action(egui::accesskit::Action::Focus),
                        node.is_disabled(),
                    )
                },
            )
        });
        let observed_nodes = tree
            .nodes
            .iter()
            .map(|(_, node)| (node.role(), node.label(), node.value()))
            .collect::<Vec<_>>();
        (status_actions, observed_nodes)
    };
    output.textures_delta.clear();
    let (supports_click, supports_focus, disabled) = status_actions.unwrap_or_else(|| {
        panic!("the visible native status strip owns one label; nodes={observed_nodes:?}")
    });
    assert!(!supports_click);
    assert!(!supports_focus);
    assert!(
        !probe_enabled,
        "fatal/quarantined UI must reject later semantic input"
    );
    assert!(
        disabled,
        "fatal/quarantined status must be exposed as non-interactive"
    );
}

#[test]
fn fatal_status_identifies_the_underlying_failure() {
    let mut state = test_state();
    let cause = NativeHostProtocolError::PaintedOutputCountMismatch {
        expected: 1,
        actual: 0,
    };
    let expected = cause.to_string();
    state.stop_current_output(cause.into());
    let context = egui::Context::default();
    context.enable_accesskit();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        state.render_error(ui, NativeErrorPresentation::RetainedOverlay);
    });
    output.textures_delta.clear();
    let tree = output.platform_output.accesskit_update.as_ref().unwrap();
    assert!(
        tree.nodes.iter().any(|(_, node)| {
            node.role() == egui::accesskit::Role::Label
                && node.value().is_some_and(|message| {
                    message.starts_with("Native dockspace stopped") && message.contains(&expected)
                })
        }),
        "the visible status must preserve the exact cause instead of only the error category"
    );
    output.drop_without_applying_deltas();
}

#[test]
fn no_frame_placeholder_disables_semantic_input_and_accessibility() {
    let context = egui::Context::default();
    context.enable_accesskit();
    let mut probe_enabled = true;
    let mut output = context.run_ui(
        egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            ..Default::default()
        },
        |ui| {
            crate::deferred_viewport::paint_placeholder(
                ui,
                egui::ViewportClass::Deferred,
                Some(crate::mailbox::DeferredViewportPaint::Created),
            );
            probe_enabled = ui.button("mutation probe").enabled();
        },
    );
    output.textures_delta.clear();

    let tree = output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("the no-frame status publishes accessibility");
    let status = tree
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::Label
                && node.value() == Some("Creating dockspace window…"))
            .then_some(node)
        })
        .expect("the no-frame status owns one semantic node");
    assert!(!probe_enabled, "NoFrame must reject semantic input");
    assert!(
        status.is_disabled(),
        "NoFrame status must be disabled in AccessKit"
    );
    assert!(
        context.has_requested_repaint_for(&ViewportId::ROOT),
        "creating a no-frame viewport must request the next lifecycle boundary"
    );
}

#[test]
fn only_a_skipped_retain_omits_the_accessibility_update() {
    assert!(RetainedOutputKind::Skip.omits_accessibility_when_retained());
    assert!(RetainedOutputKind::Rejected.omits_accessibility_when_retained());
    assert!(!RetainedOutputKind::Overlay.omits_accessibility_when_retained());
}

#[test]
fn rejected_output_discards_hook_payload_and_delivers_nonvisual_deltas_once() {
    let context = egui::Context::default();
    context.enable_accesskit();
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        let _ = ui.button("unconfirmed interactive payload");
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title("accepted title".into()));
    });
    output.shapes.push(egui::epaint::ClippedShape {
        clip_rect: egui::Rect::EVERYTHING,
        transform_after_tessellation: Default::default(),
        shape: egui::Shape::Callback(egui::epaint::PaintCallback {
            rect: egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1.0, 1.0)),
            callback: Arc::new(()),
        }),
    });
    let texture = egui::TextureId::Managed(u64::MAX);
    let delta = egui::epaint::ImageDelta::full(
        egui::ColorImage::filled([1, 1], egui::Color32::RED),
        egui::TextureOptions::LINEAR,
    );
    output.textures_delta.push(texture, delta.clone());
    output.textures_delta.free(texture);
    let command = egui::OutputCommand::CopyText("accepted clipboard intent".into());
    output.platform_output.commands.push(command.clone());
    let viewport_commands = output.viewport_output[&ViewportId::ROOT].commands.clone();
    assert!(output.platform_output.accesskit_update.is_some());

    discard_unproven_output(&mut output);

    assert!(
        output.shapes.is_empty(),
        "markers and hook replacements cannot reach the renderer"
    );
    assert!(output.platform_output.accesskit_update.is_none());
    assert!(output.platform_output.events.is_empty());
    assert!(output.textures_delta.set[&texture].as_slice() == [delta]);
    assert!(output.textures_delta.free.contains(&texture));
    assert_eq!(output.platform_output.commands, vec![command]);
    assert_eq!(
        output.viewport_output[&ViewportId::ROOT].commands,
        viewport_commands
    );
    let delivered = output.platform_output.take();
    assert_eq!(delivered.commands.len(), 1);
    assert!(output.platform_output.take().commands.is_empty());
    output.textures_delta.clear();
}

#[test]
fn presented_pending_keeps_the_previous_accessibility_tree() {
    let context = egui::Context::default();
    context.enable_accesskit();
    let mut pending_probe_enabled = false;
    let mut output = context.run_ui(egui::RawInput::default(), |ui| {
        pending_probe_enabled = ui.button("pending mutation").enabled();
    });
    assert!(pending_probe_enabled);
    assert!(
        !output.shapes.is_empty(),
        "the pending pass may produce paint, but the retained renderer path must skip it"
    );
    assert!(output.platform_output.accesskit_update.is_some());

    finalize_retained_accessibility(&mut output, RetainedOutputKind::Overlay, true);
    assert!(
        output.platform_output.accesskit_update.is_some(),
        "a visible fatal overlay must publish its inert status semantics"
    );
    finalize_retained_accessibility(&mut output, RetainedOutputKind::Skip, false);
    assert!(
        output.platform_output.accesskit_update.is_some(),
        "a failed retain falls back to the current NoFrame semantics"
    );
    finalize_retained_accessibility(&mut output, RetainedOutputKind::Skip, true);

    assert!(
        output.platform_output.accesskit_update.is_none(),
        "PresentedPending must keep the prior semantic tree instead of publishing pending UI"
    );
    output.textures_delta.clear();
}

#[test]
fn no_frame_recovery_reaches_enabled_first_live_at_the_presented_boundary() {
    let mut session = test_session();
    let context = egui::Context::default();
    context.enable_accesskit();
    let raw_input = || egui::RawInput {
        screen_rect: Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(800.0, 600.0),
        )),
        ..Default::default()
    };

    let mut no_frame_probe_enabled = true;
    let mut no_frame_output = context.run_ui(raw_input(), |ui| {
        crate::deferred_viewport::paint_placeholder(
            ui,
            egui::ViewportClass::Deferred,
            Some(crate::mailbox::DeferredViewportPaint::Created),
        );
        no_frame_probe_enabled = ui.button("no-frame mutation probe").enabled();
    });
    let no_frame_accesskit = no_frame_output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("NoFrame publishes its preparing status");
    assert!(!no_frame_probe_enabled);
    assert!(no_frame_accesskit.nodes.iter().any(|(_, node)| {
        node.role() == egui::accesskit::Role::Label
            && node.value() == Some("Creating dockspace window…")
            && node.is_disabled()
    }));
    no_frame_output.textures_delta.clear();

    let mut measurement_output = context.run_ui(raw_input(), |ui| {
        let mut frame = session
            .begin_host_frame()
            .expect("the first-live measurement frame begins");
        let dock_rect = ui.available_rect_before_wrap();
        egui_dockspace::native_support::measure_surface(
            &mut frame,
            SURFACE,
            ui,
            dock_rect,
            dock_rect,
            &TestPanes,
            &DockStyle::default(),
        )
        .expect("the first-live surface measures through the production renderer");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the measurement frame settles every surface");
        drop(
            frame
                .commit()
                .expect("the measurement contribution commits"),
        );
    });
    measurement_output.textures_delta.clear();

    let mut panes = TestPanes;
    let mut paint_report = None;
    let mut paint_had_ready_plan = false;
    let mut mutation_probe_enabled = false;
    let mut semantic_output = context.run_ui(raw_input(), |ui| {
        let mut frame = session
            .begin_host_frame()
            .expect("the first-live semantic frame begins");
        let mut register_scroll = |ui: &egui::Ui, rect: egui::Rect, id: egui::Id| {
            let identity = ui.register_scroll_hit_candidate(rect, id);
            (identity.id(), identity.layer_id())
        };
        let mut paint = egui_dockspace::native_support::paint_surface(
            &mut frame,
            Id::unique("first-live-semantic-paint"),
            SURFACE,
            ui,
            &mut panes,
            &DockStyle::default(),
            &egui_dockspace::DockInputConfig::default(),
            &mut register_scroll,
        )
        .expect("the first-live surface paints through the production renderer");
        paint_had_ready_plan = paint.had_ready_plan();
        mutation_probe_enabled = ui.button("first-live mutation probe").enabled();
        assert!(
            paint
                .confirm_into(&mut frame)
                .expect("the production paint confirms against its issuing frame"),
            "the production renderer completes the first-live program"
        );
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("the semantic frame settles every surface");
        paint_report = Some(frame.commit().expect("the first-live paint commits"));
    });

    assert!(paint_had_ready_plan);
    assert!(
        mutation_probe_enabled,
        "FirstLive must leave product input enabled"
    );
    let accesskit = semantic_output
        .platform_output
        .accesskit_update
        .as_ref()
        .expect("FirstLive publishes the current semantic tree");
    let mutation_probe = accesskit
        .nodes
        .iter()
        .find_map(|(_, node)| {
            (node.role() == egui::accesskit::Role::Button
                && node.label() == Some("first-live mutation probe"))
            .then_some(node)
        })
        .expect("the enabled FirstLive mutation probe is accessible");
    assert!(!mutation_probe.is_disabled());
    assert!(mutation_probe.supports_action(egui::accesskit::Action::Click));
    assert!(
        accesskit.nodes.iter().all(|(_, node)| {
            node.value().is_none_or(|value| {
                value != "Creating dockspace window…"
                    && value != "Waiting for dockspace authority…"
                    && !value.starts_with("Native dockspace stopped")
            })
        }),
        "FirstLive removes transitional and fatal status UI"
    );

    let mut report = paint_report.take().expect("the semantic report exists");
    let outputs = report.take_painted_outputs();
    let [painted] = outputs.try_into().unwrap_or_else(|outputs: Vec<_>| {
        panic!("expected one first-live output, got {}", outputs.len())
    });
    session
        .report_surface_presentation(painted, SurfacePresentationResult::Presented)
        .expect("the renderer presents the exact first-live output");
    let mut admission = session
        .begin_host_frame()
        .expect("the first-live admission boundary begins");
    admission
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("the admission boundary settles every surface");
    drop(
        admission
            .commit()
            .expect("the first-live admission commits"),
    );
    assert!(session.presented_surface(SURFACE).is_some());
    semantic_output.textures_delta.clear();
}

#[test]
fn protocol_follow_up_requests_exactly_one_root_cycle() {
    let context = egui::Context::default();
    for _ in 0..8 {
        if !context.has_requested_repaint_for(&ViewportId::ROOT) {
            break;
        }
        context.begin_pass(egui::RawInput::default());
        let mut output = context.end_pass();
        output.textures_delta.clear();
    }
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));
    let repaint_count = Arc::new(AtomicUsize::new(0));
    context.set_request_repaint_callback({
        let repaint_count = Arc::clone(&repaint_count);
        move |_| {
            repaint_count.fetch_add(1, Ordering::Relaxed);
        }
    });

    request_follow_up_root_cycle(&context, false, false);
    assert!(!context.has_requested_repaint_for(&ViewportId::ROOT));

    request_follow_up_root_cycle(&context, true, false);
    assert!(context.has_requested_repaint_for(&ViewportId::ROOT));
    assert_eq!(repaint_count.load(Ordering::Relaxed), 1);

    let mut output = context.run_ui(egui::RawInput::default(), |_| {});
    output.textures_delta.clear();
    assert_eq!(repaint_count.load(Ordering::Relaxed), 1);
}

#[test]
fn programmatic_action_queue_is_bounded_and_stops_with_the_runtime() {
    let mut state = test_state();
    let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));

    state
        .request_command(NativeCommand::DockRoot {
            root: RootId::new(1),
            placement,
        })
        .expect("the empty application-action slot accepts one request");
    assert_eq!(
        state
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement
            })
            .expect_err("a second action cannot overtake the pending request")
            .reason(),
        crate::NativeActionRequestError::Busy
    );

    let action = state
        .application_actions
        .take()
        .expect("the final root pass takes the pending action");
    assert_eq!(
        action.expected_version(),
        state.coordinator.session().version()
    );
    state
        .application_actions
        .settle(
            Some(&HostInputOutcome::ProductActionRejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied,
            )),
            &[],
        )
        .expect("the exact product result settles the action");
    assert!(!state.application_actions.has_queued());
    assert!(state.application_actions.is_occupied());
    assert!(matches!(
        state.take_action_status(),
        Some(Ok(crate::NativeActionStatus::Dock(
            DockspaceActionStatus::Rejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied
            )
        )))
    ));

    state
        .request_command(NativeCommand::DockRoot {
            root: RootId::new(1),
            placement,
        })
        .expect("consuming the result releases the bounded slot");

    state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
    assert!(state.application_actions.is_occupied());
    assert!(matches!(
        state.take_action_status(),
        Some(Err(error))
            if error.interruption() == crate::NativeActionInterruption::NotCommitted
                && error.kind() == NativeRuntimeErrorKind::InvalidConfiguration
    ));
    assert!(!state.application_actions.is_occupied());
    assert_eq!(
        state
            .request_command(NativeCommand::DockRoot {
                root: RootId::new(1),
                placement
            })
            .expect_err("a stopped runtime rejects new application actions")
            .reason(),
        crate::NativeActionRequestError::Stopped
    );
}

#[test]
fn quiescence_tracks_the_committed_boundary_and_real_pending_actions() {
    let mut state = test_state();
    let before = state.pending_work();
    assert!(before.contains(crate::NativeWaitReason::RootPresentation));
    assert!(!before.contains(crate::NativeWaitReason::ApplicationAction));

    state
        .request_command(NativeCommand::DockRoot {
            root: RootId::new(1),
            placement: DockPlacement::Center(DockAnchor::Item(ItemId::new(1))),
        })
        .expect("the committed boundary accepts one application action");

    let pending = state.pending_work();
    assert!(pending.contains(crate::NativeWaitReason::ApplicationAction));
    assert!(!state.is_quiescent());
    assert!(
        !before.contains(crate::NativeWaitReason::ApplicationAction),
        "snapshots never query later state"
    );
}

#[test]
fn committed_action_requires_its_exact_submission_outcome() {
    let mut state = test_state();
    let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));
    state
        .request_command(NativeCommand::DockRoot {
            root: RootId::new(1),
            placement,
        })
        .expect("the presentation action is queued");
    let _action = state
        .application_actions
        .take()
        .expect("the final root pass takes the pending action");

    assert!(state.application_actions.settle(None, &[]).is_err());
    assert!(state.application_actions.is_occupied());
    assert!(matches!(
        state.take_action_status(),
        Some(Err(error))
            if error.interruption()
                == crate::NativeActionInterruption::CommittedOutcomeUnavailable
                && error.kind() == NativeRuntimeErrorKind::Internal
    ));
}

#[test]
fn fatal_stop_terminalizes_an_uncommitted_application_frame_once() {
    let mut state = test_state();
    let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));
    state
        .request_command(NativeCommand::DockRoot {
            root: RootId::new(1),
            placement,
        })
        .expect("the application action is queued");
    let _action = state
        .application_actions
        .take()
        .expect("the final root pass takes the pending action");

    state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
    state.stop_current_output(NativeHostProtocolError::RootRegistrationRejected(SURFACE).into());

    assert!(matches!(
        state.take_action_status(),
        Some(Err(error))
            if error.interruption() == crate::NativeActionInterruption::NotCommitted
                && error.kind() == NativeRuntimeErrorKind::InvalidConfiguration
    ));
    assert!(state.take_action_status().is_none());
}

#[test]
fn fatal_stop_retains_the_coordinator_and_first_error() {
    let mut state = test_state();

    state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
    let first_error = state
        .error()
        .expect("fatal stop records its primary error")
        .to_string();
    state.stop_current_output(NativeHostProtocolError::RootRegistrationRejected(SURFACE).into());

    assert_eq!(
        state
            .error()
            .expect("later failures do not replace the primary error")
            .to_string(),
        first_error
    );
    assert!(state.view().surface(SURFACE).is_some());
}

#[test]
fn fatal_stop_preserves_a_committed_application_action_result() {
    let mut state = test_state();
    let placement = DockPlacement::Center(DockAnchor::Item(ItemId::new(1)));
    state
        .request_command(NativeCommand::DockRoot {
            root: RootId::new(1),
            placement,
        })
        .expect("the application action is queued");
    let _action = state
        .application_actions
        .take()
        .expect("the final root pass takes the pending action");
    state
        .application_actions
        .settle(
            Some(&HostInputOutcome::ProductActionRejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied,
            )),
            &[],
        )
        .expect("the exact product result settles the action");

    state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());

    assert!(matches!(
        state.take_action_status(),
        Some(Ok(crate::NativeActionStatus::Dock(
            DockspaceActionStatus::Rejected(
                dockspace::model::DockspaceActionRejection::PolicyDenied
            )
        )))
    ));
    assert!(!state.application_actions.is_occupied());
}

#[test]
fn shutdown_retains_cleanup_errors_in_observation_order() {
    let mut state = test_state();
    state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
    state.record_cleanup_error(NativeHostProtocolError::RootRegistrationRejected(SURFACE).into());
    state.record_cleanup_error(NativeHostProtocolError::IncompleteTransientPaint(SURFACE).into());

    let shutdown = state
        .shutdown
        .as_ref()
        .expect("fatal stop owns one shutdown record");
    assert_eq!(shutdown.cleanup_errors.len(), 2);
    assert!(
        shutdown.cleanup_errors[0]
            .source()
            .expect("cleanup error retains its source")
            .to_string()
            .contains("registration")
    );
    assert!(
        shutdown.cleanup_errors[1]
            .source()
            .expect("cleanup error retains its source")
            .to_string()
            .contains("transient")
    );
    assert!(
        shutdown
            .primary
            .source()
            .expect("primary error retains its source")
            .to_string()
            .contains("output callback")
    );
}

#[test]
fn fatal_quarantine_recovery_commits_close_before_exit_errors() {
    let mut state = test_state();
    state.stop_current_output(NativeHostProtocolError::OutputTokenUnavailable.into());
    assert!(state.error().is_some());
    assert!(!state.is_quiescent());
    assert_eq!(
        state
            .request_command(NativeCommand::DockRootBack {
                root: RootId::new(1)
            })
            .expect_err("fatal quarantine rejects semantic mutation")
            .reason(),
        crate::NativeActionRequestError::Stopped
    );

    let context = egui::Context::default();
    context.begin_pass(egui::RawInput::default());

    // Shutdown dispatches the command before returning its progress report.
    context.send_viewport_cmd_to(egui::ViewportId::ROOT, egui::ViewportCommand::Close);
    let (progress, errors) = state.apply_shutdown_advance_for_root_window(
        &context,
        NativeShutdownAdvance {
            progress: true,
            transition_source_exhausted: false,
            presentation_transitions: Vec::new(),
            registrations: vec![NativeShutdownRegistration::Rejected(SURFACE)],
            commands_dispatched: true,
            repaint_viewports: Vec::new(),
            pointer_passthrough: None,
            abandoned_outputs: Vec::new(),
            errors: vec![NativeHostProtocolError::IncompleteTransientPaint(SURFACE).into()],
        },
        Some(winit::window::WindowId::from(7)),
    );
    assert!(progress);
    let [committed_error, registration_error] = errors.as_slice() else {
        panic!("committed and adapter cleanup errors remain ordered")
    };
    assert!(
        committed_error
            .source()
            .expect("the committed cleanup error retains its source")
            .to_string()
            .contains("transient")
    );
    assert!(
        registration_error
            .source()
            .expect("the adapter cleanup error retains its source")
            .to_string()
            .contains("registration")
    );

    let mut output = context.end_pass();
    let close_was_queued = output
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .expect("the committed root command remains in egui output")
        .commands
        .contains(&egui::ViewportCommand::Close);
    output.textures_delta.clear();
    assert!(
        close_was_queued,
        "fatal recovery exits only after the committed close command survives the output"
    );
}
