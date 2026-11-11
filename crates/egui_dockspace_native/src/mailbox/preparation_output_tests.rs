//! Real output identities cannot acquire another exact window's preparation.

use std::num::NonZeroUsize;

use dockspace::runtime::NativeCreateReadiness;
use winit::platform::x11::EventLoopBuilderExtX11;

use super::*;

struct Fixture {
    bridge: NativeHostBridge,
    session: DockspaceSession,
    predecessor: NativeSurfaceBinding,
    successor: NativeSurfaceBinding,
    old_token: Option<NativeOutputToken>,
    successor_token: Option<NativeOutputToken>,
    revoked: Option<NativePreparationObservation>,
    request: Option<NativePreparationPaint>,
    successor_presented: bool,
    done: bool,
}

impl Fixture {
    fn preparation(&self) -> NativePreparationObservation {
        self.bridge
            .lock()
            .window_resources
            .preparation_observation(self.successor)
            .unwrap()
    }
}

struct Host(Arc<Mutex<Fixture>>);

impl NativeHostHandler for Host {
    fn try_attach(&self, attachment: NativeHostAttachment) -> bool {
        self.0.lock().unwrap().bridge.try_attach(attachment)
    }

    fn detach(&self, attachment: NativeHostAttachment) {
        self.0.lock().unwrap().bridge.detach(attachment);
    }

    fn on_output_begin(
        &self,
        token: NativeOutputToken,
        window: NativeWindowSnapshot,
        _roster: Option<NativeViewportRoster<'_>>,
    ) -> NativeOutputAdmission {
        let fixture = self.0.lock().unwrap();
        if fixture.old_token.is_none() {
            fixture
                .bridge
                .bind_viewport_route(token.viewport_id(), token.window_id(), fixture.predecessor)
                .unwrap();
            return fixture.bridge.on_output_begin(token, window, None);
        }
        // The private renderer basis is a deterministic owner fixture. Only
        // token identity, pass lifetime and terminals come from the real backend.
        // Reserve the existing output owner without replacing that model basis
        // with this unrelated harness window's geometry.
        assert!(fixture.bridge.lock().reserve_output(
            token,
            Some(fixture.successor),
            NativeInputBudget::new(&NativeMonotonicClock::new(|| std::time::Duration::ZERO)),
        ));
        NativeOutputAdmission::Proceed
    }

    fn on_output(&self, result: NativeOutputResult) -> NativeHostWake {
        let mut fixture = self.0.lock().unwrap();
        let wake = fixture.bridge.on_output(result);
        if fixture.old_token == Some(result.token()) {
            assert!(fixture.bridge.output_reservation(result.token()).is_none());
            assert!(!fixture.bridge.references_binding(fixture.predecessor));
            assert_eq!(fixture.preparation(), fixture.revoked.unwrap());
        } else if fixture.successor_token == Some(result.token()) {
            assert!(!fixture.successor_presented);
            assert_eq!(result.status(), eframe::NativeOutputStatus::Presented);
            let Some(PendingNativeOutput::Staging(request)) =
                fixture.bridge.take_pending_output(result.token())
            else {
                panic!("the exact successor token owns its preparation output");
            };
            assert_eq!(request, fixture.request.unwrap());
            fixture
                .bridge
                .finish_preparation_output(result.token(), request, true);
            assert!(fixture.bridge.mark_output_submitted(result.token()));
            fixture.bridge.commit_frame_boundary(false);
            assert!(fixture.bridge.output_reservation(result.token()).is_none());
            assert_eq!(fixture.preparation(), fixture.revoked.unwrap());
            fixture
                .bridge
                .settle_window_preparation(fixture.successor, fixture.revoked.unwrap());
            assert_eq!(
                fixture.preparation(),
                NativePreparationObservation {
                    readiness: NativeCreateReadiness::Prepared,
                    ..fixture.revoked.unwrap()
                },
                "the original Created receipt still releases the exact revocation barrier",
            );
            fixture.successor_presented = true;
        }
        wake
    }
}

struct App(Arc<Mutex<Fixture>>);

impl eframe::App for App {
    fn ui(&mut self, ui: &mut eframe::egui::Ui, _frame: &mut eframe::Frame) {
        let token = eframe::current_native_output_token().unwrap();
        let mut fixture = self.0.lock().unwrap();
        if fixture.old_token.is_none() {
            fixture.old_token = Some(token);
            let predecessor = fixture.predecessor;
            let successor = {
                let Fixture {
                    bridge, session, ..
                } = &mut *fixture;
                let reset = session
                    .prepare_layout_reset(super::tests::workspace_root_layout(
                        predecessor.surface(),
                    ))
                    .unwrap();
                let mut frame = session.begin_host_frame().unwrap();
                frame.submit_layout_reset(&reset).unwrap();
                frame
                    .complete_unpainted_surfaces(
                        dockspace::runtime::SurfaceUnavailableReason::Deferred,
                    )
                    .unwrap();
                let (report, _) = bridge
                    .commit_host_frame_with_staging(frame, false, None)
                    .unwrap();
                bridge.finish_workspace_replacement(token.viewport_id(), predecessor);
                report.native_bindings()[0]
            };
            assert_ne!(predecessor, successor);
            fixture.successor = successor;
            fixture.bridge.ready_window_preparation_for_test(
                token.viewport_id(),
                fixture.successor,
                token.window_id(),
            );
            assert_eq!(
                fixture.preparation().readiness,
                NativeCreateReadiness::Prepared
            );
            fixture
                .bridge
                .lose_preparation_renderer_for_test(token.viewport_id(), fixture.successor);
            let revoked = fixture.preparation();
            let request = fixture
                .bridge
                .lock()
                .window_resources
                .staging(token.viewport_id())
                .unwrap();
            assert_eq!(revoked.readiness, NativeCreateReadiness::Created);
            assert_eq!(request.phase(), NativePreparationPaintPhase::PostShow);
            fixture.revoked = Some(revoked);
            fixture.request = Some(request);

            for _ in 0..2 {
                assert_eq!(
                    fixture.bridge.record_deferred_viewport_paint(token),
                    DeferredViewportPaint::Waiting,
                );
                assert_eq!(
                    fixture.preparation(),
                    revoked,
                    "rejecting the predecessor output must not revoke its successor",
                );
                assert_eq!(
                    fixture
                        .bridge
                        .lock()
                        .window_resources
                        .staging(token.viewport_id()),
                    Some(request),
                    "the successor keeps the same awaiting paint phase and generation",
                );
            }
            let reservation = fixture.bridge.output_reservation(token).unwrap();
            assert_eq!(reservation.binding(), Some(fixture.predecessor));
            assert!(reservation.is_abandoned());
            assert!(!reservation.terminal_recorded);
            assert!(fixture.bridge.references_binding(fixture.predecessor));
        } else if fixture.successor_token.is_none() {
            assert!(
                fixture
                    .bridge
                    .output_reservation(fixture.old_token.unwrap())
                    .is_none()
            );
            fixture.successor_token = Some(token);
            assert_eq!(
                fixture.bridge.output_reservation(token).unwrap().binding(),
                Some(fixture.successor)
            );
            for _ in 0..2 {
                assert_eq!(
                    fixture.bridge.record_deferred_viewport_paint(token),
                    DeferredViewportPaint::Staging(fixture.request.unwrap()),
                    "repeated passes reuse the same exact successor output",
                );
                assert_eq!(fixture.preparation(), fixture.revoked.unwrap());
            }
        } else {
            assert!(fixture.successor_presented);
            assert!(fixture.bridge.abandon_output(token));
            fixture.done = true;
            ui.ctx()
                .send_viewport_cmd(eframe::egui::ViewportCommand::Close);
        }
        ui.label("Exact preparation output regression");
        ui.ctx().request_repaint();
    }
}

#[test]
#[ignore = "requires a real X11 display and renderer; output identities are minted by eframe"]
fn real_old_output_cannot_revoke_successor_preparation_and_its_exact_created_receipt() {
    let (session, _, predecessor) = super::tests::workspace_root_fixture();
    let bridge = NativeHostBridge::new(NativeViewportMap::default());
    bridge.lock().window_resources = NativeWindowResources::with_capacity_limit(2).unwrap();
    let fixture = Arc::new(Mutex::new(Fixture {
        bridge,
        session,
        predecessor,
        successor: predecessor,
        old_token: None,
        successor_token: None,
        revoked: None,
        request: None,
        successor_presented: false,
        done: false,
    }));
    let app = Arc::clone(&fixture);
    eframe::run_native(
        "Exact preparation output regression",
        eframe::NativeOptions {
            viewport: eframe::egui::ViewportBuilder::default().with_inner_size([640.0, 480.0]),
            native_host: Some(Arc::new(Host(Arc::clone(&fixture)))),
            event_loop_builder: Some(Box::new(|builder| {
                builder.with_any_thread(true).with_x11();
            })),
            ..Default::default()
        },
        Box::new(move |creation| {
            creation
                .egui_ctx
                .options_mut(|options| options.max_passes = NonZeroUsize::MIN);
            Ok(Box::new(App(app)))
        }),
    )
    .unwrap();
    let fixture = fixture.lock().unwrap();
    assert!(fixture.done);
    assert!(fixture.successor_presented);
    assert!(!fixture.bridge.references_binding(predecessor));
    assert_eq!(fixture.bridge.pending_output_count(), 0);
}
