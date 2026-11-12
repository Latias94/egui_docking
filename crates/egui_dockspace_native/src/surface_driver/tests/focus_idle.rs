//! Retained exact focus observations do not repeatedly wake an unchanged native UI.

#![cfg(target_os = "linux")]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{DockspaceSession, SurfacePresentationResult};
use eframe::{NativeOutputToken, egui};
use egui_dockspace::native_support;
use egui_dockspace::{DockInputConfig, DockStyle, PaneResponse, PaneView};
use winit::platform::x11::EventLoopBuilderExtX11 as _;

use super::super::{NativePassActions, retained_actions_need_repaint};
use super::SURFACE;

struct Panes {
    target: egui::Id,
    application: egui::Id,
    request: Option<egui::Id>,
    pane_text: String,
    application_text: String,
}

impl PaneView for Panes {
    fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
        Some("Document".into())
    }

    fn ui(&mut self, _: ItemId, ui: &mut egui::Ui) -> PaneResponse {
        let pane = ui.add(egui::TextEdit::singleline(&mut self.pane_text).id(self.target));
        let application =
            ui.add(egui::TextEdit::singleline(&mut self.application_text).id(self.application));
        if let Some(target) = self.request.take() {
            ui.memory_mut(|memory| memory.request_focus(target));
        }
        PaneResponse::from(&pane).with_content(&application)
    }

    fn focus_target(&self, _: ItemId) -> Option<egui::Id> {
        Some(self.target)
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "the journey retains real output-bound observations through unchanged focus and final acknowledgement"
)]
fn exercise_focus_observations(tokens: &[NativeOutputToken]) {
    let item = ItemId::new(1);
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::central_tabs([item])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame.select_item_current(item).unwrap();
    frame.commit().unwrap();
    let original = session.pane_focus_request(SURFACE).unwrap();
    let context = egui::Context::default();
    context.options_mut(|options| options.max_passes = 1.try_into().unwrap());
    let target = egui::Id::unique("native-idle-pane");
    let application = egui::Id::unique("native-idle-application");
    let mut panes = Panes {
        target,
        application,
        request: Some(application),
        pane_text: String::new(),
        application_text: String::new(),
    };
    let mut actions = NativePassActions::default();

    for (pass, expected_wake, expected_observation) in [
        (0_u32, true, true),
        (1, false, true),
        (2, true, true),
        (3, false, false),
    ] {
        if pass == 2 {
            panes.request = Some(target);
        }
        let token = tokens[usize::try_from(pass).unwrap() % tokens.len()];
        let mut frame = session.begin_host_frame().unwrap();
        let (_, mut acknowledged) = actions.peek_deferred_actions(token, SURFACE);
        if let Some(observation) =
            actions.peek_pane_focus(token, frame.presented_surface(SURFACE), &mut acknowledged)
        {
            frame.submit_pane_focus_observation(observation).unwrap();
        }
        frame.begin_encoding_pass().unwrap();
        let mut paint = None;
        let mut output = context.run_ui(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(640.0, 480.0),
                )),
                focused: true,
                time: Some(f64::from(pass) * 0.1),
                ..Default::default()
            },
            |ui| {
                let bounds = ui.available_rect_before_wrap();
                native_support::measure_surface(
                    &mut frame,
                    SURFACE,
                    ui,
                    bounds,
                    bounds,
                    &panes,
                    &DockStyle::default(),
                )
                .unwrap();
                let mut register = |ui: &egui::Ui, rect, id| {
                    let hit = ui.register_scroll_hit_candidate(rect, id);
                    (hit.id(), hit.layer_id())
                };
                paint = Some(
                    native_support::paint_surface(
                        &mut frame,
                        egui::Id::unique("native-focus-idle"),
                        SURFACE,
                        ui,
                        &mut panes,
                        &DockStyle::default(),
                        &DockInputConfig::default(),
                        &mut register,
                    )
                    .unwrap(),
                );
            },
        );
        let mut paint = paint.unwrap();
        assert!(paint.validate_final_output(&mut output));
        let changed_focus = paint.pane_focus_needs_repaint();
        let observation = paint.take_pane_focus_observation();
        assert_eq!(observation.is_some(), expected_observation);
        assert!(paint.confirm_into(&mut frame).unwrap());
        let mut report = frame.commit().unwrap();
        actions.acknowledge(acknowledged);
        actions.commit_presentations(report.surface_presentation_commits());
        let outputs = report.take_painted_outputs();
        assert_eq!(outputs.len(), 1);
        if let Some(observation) = observation {
            actions.retain_pane_focus(
                token,
                SURFACE,
                None,
                outputs[0].presentation_artifact(),
                observation,
            );
            assert!(
                actions.has_retained_action_work(),
                "an unchanged observation keeps its exact output ownership"
            );
        }
        assert_eq!(
            retained_actions_need_repaint(&actions, changed_focus),
            expected_wake,
            "only a new or changed focus fact can wake another UI frame, pass {pass}"
        );
        // This fixture supplies a logical renderer sink. The real native tokens
        // identify event origins; no physical presentation is inferred from UI return.
        for output in outputs {
            session
                .report_surface_presentation(output, SurfacePresentationResult::Presented)
                .unwrap();
        }
        output.textures_delta.clear();
        assert_eq!(
            session.pane_focus_request(SURFACE),
            (pass < 3).then_some(original),
            "a Focused acknowledgement clears only after its exact output is presented"
        );
    }
    assert!(
        !actions.has_retained_action_work(),
        "the focused acknowledgement is published and retired"
    );
}

struct TokenHost;

impl eframe::NativeHostHandler for TokenHost {}

struct Driver {
    tokens: Vec<NativeOutputToken>,
    completed: Arc<AtomicBool>,
    started: Instant,
}

impl eframe::App for Driver {
    fn ui(&mut self, ui: &mut egui::Ui, _: &mut eframe::Frame) {
        assert!(self.started.elapsed() < Duration::from_secs(15));
        if !self.completed.load(Ordering::SeqCst) {
            let token = eframe::current_native_output_token().unwrap();
            if self.tokens.last() != Some(&token) {
                self.tokens.push(token);
            }
            if self.tokens.len() == 3 {
                exercise_focus_observations(&self.tokens);
                self.completed.store(true, Ordering::SeqCst);
            }
        }
        if self.completed.load(Ordering::SeqCst) {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        } else {
            ui.ctx().request_repaint();
        }
    }
}

#[test]
fn stable_unfocused_observations_idle_and_later_focused_acknowledgement_still_publishes() {
    if std::env::var_os("DISPLAY").is_none() {
        eprintln!("SKIP native focus idle regression: DISPLAY unavailable; run the X11 gate");
        return;
    }
    let completed = Arc::new(AtomicBool::new(false));
    let result = Arc::clone(&completed);
    eframe::run_native(
        "Native focus idle regression",
        eframe::NativeOptions {
            native_host: Some(Arc::new(TokenHost)),
            renderer: eframe::Renderer::Glow,
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_x11().with_any_thread(true);
            })),
            ..Default::default()
        },
        Box::new(move |_| {
            Ok(Box::new(Driver {
                tokens: Vec::new(),
                completed,
                started: Instant::now(),
            }))
        }),
    )
    .unwrap();
    assert!(result.load(Ordering::SeqCst));
}
