//! Lifecycle tests using real core emissions and the ordinary product renderer.

use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostFrameReport, SurfacePresentationArtifact, SurfacePresentationResult,
    SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use egui::{Color32, Context, FullOutput, RawInput, Rect, RepaintCause, Shape, Ui, ViewportId};

use super::{
    DockspaceOutputReceipt, PresentationSettlementFailure, ProductPassEnrollment,
    ProductPassSettlement, ProductPassSettlementBatch, ReadyProductPassSettlementBatch, run_ui,
};
use crate::output_proof::SurfaceOutputCapture;
use crate::pane::PaneView;
use crate::product_render::{self, PointerActionAuthority};
use crate::style::DockStyle;

const SURFACE: SurfaceId = SurfaceId::new(1);
const PANE_COLOR: Color32 = Color32::from_rgb(11, 97, 173);

struct SettlementPane;

impl PaneView for SettlementPane {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Settlement lifecycle pane".into())
    }

    fn ui(&mut self, _item: ItemId, ui: &mut Ui) -> crate::PaneResponse {
        ui.painter()
            .rect_filled(ui.available_rect_before_wrap().shrink(8.0), 0.0, PANE_COLOR);
        crate::PaneResponse::default()
    }
}

fn test_context() -> Context {
    let context = Context::default();
    context.options_mut(|options| {
        options.max_passes = 1.try_into().expect("one pass is nonzero");
    });
    context
}

fn input(viewport: ViewportId) -> RawInput {
    let mut input = RawInput {
        viewport_id: viewport,
        screen_rect: Some(Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(640.0, 480.0),
        )),
        ..RawInput::default()
    };
    input.viewports.entry(viewport).or_default();
    input
}

fn measured_session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .expect("the lifecycle layout validates");
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
        .expect("the lifecycle session initializes");
    let metrics = UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("the surface bounds validate"),
        LogicalSize::new(32.0, 24.0).expect("the pane minimum validates"),
        180.0,
    )
    .expect("the measurement profile validates");
    let mut frame = session.begin_host_frame().expect("measurement begins");
    frame
        .measure_surface(SURFACE, metrics)
        .expect("the surface measures");
    frame.commit().expect("measurement commits");
    session
}

fn stage_surface(
    settlement: &ProductPassSettlement,
    session: &mut DockspaceSession,
    ui: &mut Ui,
) -> SurfacePresentationArtifact {
    let ProductPassEnrollment::Acquired(claim) = settlement.enroll(ui) else {
        panic!("an owned surface must acquire its exact pass");
    };
    let capture = SurfaceOutputCapture::begin(ui.painter());
    let mut frame = session.begin_host_frame().expect("surface painting begins");
    let plan = frame
        .paint_plan(SURFACE)
        .expect("the surface plan resolves")
        .expect("the measured surface is ready");
    let instance_id = ui.next_auto_id().with("pass-settlement-lifecycle-surface");
    let painted = product_render::paint_surface(
        ui,
        instance_id,
        plan,
        &mut SettlementPane,
        &DockStyle::default(),
        product_render::PaintResources::default(),
        PointerActionAuthority::LocalResponses {
            local_actions: true,
            presentation_settlement: true,
            cancel_active_gesture: false,
        },
        #[cfg(feature = "native-render-support")]
        None,
    )
    .expect("the ordinary product renderer executes the surface");
    assert!(painted.local_actions.is_empty());
    frame
        .confirm_surface_painted(
            painted
                .execution_receipt
                .expect("complete painting authorizes an execution receipt"),
        )
        .expect("core accepts the complete program");
    let proof = capture
        .finish(ui.painter())
        .expect("the ordinary surface span is captured on its supplied layer");
    let mut report = frame.commit().expect("surface painting commits");
    let mut outputs = report.take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    let output = outputs.pop().expect("core emits the painted surface");
    let artifact = output.presentation_artifact();
    assert!(claim.stage(ProductPassSettlementBatch {
        output: Some(output),
        proof: Some(proof),
        needs_repaint: true,
        ..ProductPassSettlementBatch::default()
    }));
    artifact
}

fn render_surface(
    context: &Context,
    viewport: ViewportId,
    settlement: &ProductPassSettlement,
    session: &mut DockspaceSession,
) -> (SurfacePresentationArtifact, DockspaceOutputReceipt) {
    let mut artifact = None;
    let (output, receipt) = run_ui(context, input(viewport), |ui| {
        artifact = Some(stage_surface(settlement, session, ui));
    })
    .into_parts();
    output.drop_without_applying_deltas();
    (artifact.expect("the surface callback ran"), receipt)
}

fn take_ready(
    settlement: &ProductPassSettlement,
    context: &Context,
    viewport: ViewportId,
) -> ReadyProductPassSettlementBatch {
    let mut ready = None;
    context
        .run_ui(input(viewport), |ui| {
            ready = Some(settlement.take_ready(ui));
        })
        .drop_without_applying_deltas();
    ready.expect("the ready queue was inspected from a real UI")
}

fn flush_presentations(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().expect("observation begins");
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .expect("observation retains the measured surface");
    frame.commit().expect("observation commits")
}

fn apply_ready(
    session: &mut DockspaceSession,
    ready: ReadyProductPassSettlementBatch,
) -> HostFrameReport {
    assert_eq!(ready.presentation_failure, None);
    for presentation in ready.presentations {
        session
            .report_surface_presentation(presentation.output, presentation.result)
            .expect("the exact renderer report belongs to this core session");
    }
    flush_presentations(session)
}

#[derive(Default)]
struct LateDiscard {
    remaining: usize,
}

impl egui::plugin::Plugin for LateDiscard {
    fn debug_name(&self) -> &'static str {
        "pass_settlement_tests::late_discard"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        if self.remaining > 0 {
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(RepaintCause::new_reason("discard after surface staging"));
        }
    }
}

struct RemovePaneGeometry {
    removed: Arc<AtomicUsize>,
}

impl egui::plugin::Plugin for RemovePaneGeometry {
    fn debug_name(&self) -> &'static str {
        "pass_settlement_tests::remove_pane_geometry"
    }

    fn output_hook(&mut self, _context: &Context, output: &mut FullOutput) {
        output.shapes.retain(|clipped| {
            let remove = matches!(&clipped.shape, Shape::Rect(rect) if rect.fill == PANE_COLOR);
            if remove {
                self.removed.fetch_add(1, Ordering::Relaxed);
            }
            !remove
        });
    }
}

struct PanicAfterPaint {
    armed: bool,
}

impl egui::plugin::Plugin for PanicAfterPaint {
    fn debug_name(&self) -> &'static str {
        "pass_settlement_tests::panic_after_paint"
    }

    fn output_hook(&mut self, _context: &Context, _output: &mut FullOutput) {
        if std::mem::take(&mut self.armed) {
            panic!("the output hook unwound after staging a complete surface");
        }
    }
}

#[test]
fn returning_output_without_a_host_report_does_not_make_it_ready() {
    let context = test_context();
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let (artifact, receipt) = render_surface(&context, ViewportId::ROOT, &settlement, &mut session);

    for _ in 0..2 {
        let ready = take_ready(&settlement, &context, ViewportId::ROOT);
        assert!(ready.presentations.is_empty());
        assert_eq!(ready.presentation_failure, None);
        assert!(
            flush_presentations(&mut session)
                .surface_presentation_commits()
                .is_empty(),
            "an unreported output cannot grant core presentation authority"
        );
    }

    receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(
        ready.presentations[0].output.presentation_artifact(),
        artifact
    );
    assert_eq!(
        ready.presentations[0].result,
        SurfacePresentationResult::Presented
    );
    let report = apply_ready(&mut session, ready);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(
        take_ready(&settlement, &context, ViewportId::ROOT)
            .presentations
            .is_empty(),
        "consuming a receipt cannot enqueue its output twice through Drop"
    );
}

#[test]
fn an_explicit_dropped_report_retires_the_exact_output() {
    let context = test_context();
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let (artifact, receipt) = render_surface(&context, ViewportId::ROOT, &settlement, &mut session);

    receipt.report(SurfacePresentationResult::Dropped);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(
        ready.presentations[0].output.presentation_artifact(),
        artifact
    );
    assert_eq!(
        ready.presentations[0].result,
        SurfacePresentationResult::Dropped
    );
    let report = apply_ready(&mut session, ready);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
}

#[test]
fn dropping_the_host_receipt_enqueues_one_dropped_result() {
    let context = test_context();
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let (artifact, receipt) = render_surface(&context, ViewportId::ROOT, &settlement, &mut session);

    drop(receipt);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(
        ready.presentations[0].output.presentation_artifact(),
        artifact
    );
    assert_eq!(
        ready.presentations[0].result,
        SurfacePresentationResult::Dropped
    );
    let report = apply_ready(&mut session, ready);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
    assert!(
        take_ready(&settlement, &context, ViewportId::ROOT)
            .presentations
            .is_empty()
    );
}

#[test]
fn child_receipt_wakes_its_ended_viewport_when_reported_outside_the_run() {
    for result in [
        Some(SurfacePresentationResult::Presented),
        Some(SurfacePresentationResult::Dropped),
        None,
    ] {
        let context = test_context();
        let child = ViewportId::from_hash_of("settlement-repaint-child");
        let settlement = ProductPassSettlement::default();
        let mut session = measured_session();
        let (_, receipt) = render_surface(&context, child, &settlement, &mut session);

        // Consume egui's bootstrap repaints so callback coalescing cannot hide this request.
        let mut idle = false;
        for _ in 0..4 {
            let output = context.run_ui(input(child), |_| {});
            idle = output
                .viewport_output
                .get(&child)
                .is_some_and(|viewport| viewport.repaint_delay == std::time::Duration::MAX);
            output.drop_without_applying_deltas();
            if idle {
                break;
            }
        }
        assert!(
            idle,
            "the child must be idle before its delayed host report"
        );
        assert_eq!(context.viewport_id(), ViewportId::ROOT);

        let repainted = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&repainted);
        context.set_request_repaint_callback(move |info| {
            observed.lock().unwrap().push(info.viewport_id);
        });
        match result {
            Some(result) => receipt.report(result),
            None => drop(receipt),
        }
        assert_eq!(
            repainted.lock().unwrap().as_slice(),
            &[child],
            "reporting or dropping the child receipt must wake its exact viewport",
        );
    }
}

#[test]
fn delayed_receipts_preserve_emission_order_when_reported_in_reverse() {
    let context = test_context();
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let (first, first_receipt) =
        render_surface(&context, ViewportId::ROOT, &settlement, &mut session);
    let (second, second_receipt) =
        render_surface(&context, ViewportId::ROOT, &settlement, &mut session);
    assert_ne!(first, second);

    second_receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(
        ready.presentations[0].output.presentation_artifact(),
        second
    );
    assert!(
        apply_ready(&mut session, ready)
            .surface_presentation_commits()
            .is_empty(),
        "a later renderer report cannot skip an unresolved core emission"
    );

    first_receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(ready.presentations[0].output.presentation_artifact(), first);
    let report = apply_ready(&mut session, ready);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(second) && commit.retires(first)),
        "the delayed first receipt cannot replace the newer exact output"
    );
}

#[test]
fn an_output_hook_discard_delivers_only_the_terminal_pass() {
    let context = test_context();
    context.options_mut(|options| {
        options.max_passes = 2.try_into().expect("two passes are nonzero");
    });
    context.add_plugin(LateDiscard { remaining: 1 });
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let mut artifacts = Vec::new();
    let (output, receipt) = run_ui(&context, input(ViewportId::ROOT), |ui| {
        artifacts.push(stage_surface(&settlement, &mut session, ui));
    })
    .into_parts();
    assert_eq!(output.platform_output.num_completed_passes, 2);
    output.drop_without_applying_deltas();
    let [discarded, terminal] = artifacts.as_slice() else {
        panic!("the late discard must execute exactly two surface passes");
    };
    assert_ne!(discarded, terminal);

    receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(
        ready.presentations[0].output.presentation_artifact(),
        *terminal
    );
    let report = apply_ready(&mut session, ready);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(*terminal) && commit.retires(*discarded))
    );
}

#[test]
fn ordered_input_keeps_raw_batch_time_across_reruns_without_using_predicted_time() {
    for (raw_time, expected) in [
        (None, None),
        (Some(1.25), Some(std::time::Duration::from_millis(1250))),
    ] {
        let context = test_context();
        context.options_mut(|options| {
            options.max_passes = 2.try_into().unwrap();
        });
        context.add_plugin(LateDiscard { remaining: 1 });
        let mut input = input(ViewportId::ROOT);
        input.time = raw_time;
        input.events = vec![egui::Event::PointerMoved(egui::pos2(20.0, 30.0))];
        let mut packets = Vec::new();
        let output = run_ui(&context, input, |ui| {
            // Framework state can change during a pass; the original packet
            // retains the explicit host observation or its original absence.
            ui.ctx().input_mut(|state| state.time = 999.0);
            let packet = super::pointer_packet(ui).unwrap();
            assert_eq!(packet.observed_at(), expected);
            assert_eq!(packet.events().len(), 1);
            packets.push(packet);
        });
        let (output, receipt) = output.into_parts();
        assert_eq!(output.platform_output.num_completed_passes, 2);
        assert_eq!(packets.len(), 2);
        assert!(packets[0].same_run(&packets[1]));
        output.drop_without_applying_deltas();
        drop(receipt);
    }
}

#[test]
fn removing_ordinary_pane_geometry_in_an_output_hook_denies_delivery() {
    let context = test_context();
    let removed = Arc::new(AtomicUsize::new(0));
    context.add_plugin(RemovePaneGeometry {
        removed: Arc::clone(&removed),
    });
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let (artifact, receipt) = render_surface(&context, ViewportId::ROOT, &settlement, &mut session);
    assert_eq!(removed.load(Ordering::Relaxed), 1);

    receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert!(ready.presentations.is_empty());
    assert_eq!(
        ready.presentation_failure,
        Some(PresentationSettlementFailure::OutputMismatch)
    );
    let report = flush_presentations(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact)),
        "preserving private markers cannot authorize a damaged complete surface"
    );
}

#[test]
fn an_output_hook_unwind_abandons_the_open_pass_and_allows_a_fresh_run() {
    let context = test_context();
    // Consume initial font texture updates before intentionally unwinding output.
    context
        .run_ui(input(ViewportId::ROOT), |ui| {
            ui.label("Settlement lifecycle pane");
        })
        .drop_without_applying_deltas();
    context.add_plugin(PanicAfterPaint { armed: true });
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let mut artifact = None;
    let interrupted = catch_unwind(AssertUnwindSafe(|| {
        let _ = run_ui(&context, input(ViewportId::ROOT), |ui| {
            artifact = Some(stage_surface(&settlement, &mut session, ui));
        });
    }));
    assert!(interrupted.is_err());
    let artifact = artifact.expect("the output hook runs after surface staging");
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert!(ready.presentations.is_empty());
    assert_eq!(
        ready.presentation_failure,
        Some(PresentationSettlementFailure::Abandoned)
    );
    assert!(
        flush_presentations(&mut session)
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );

    let (recovered, receipt) =
        render_surface(&context, ViewportId::ROOT, &settlement, &mut session);
    receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert!(
        apply_ready(&mut session, ready)
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(recovered))
    );
}

#[test]
fn a_caught_nested_run_poisons_the_outer_run_including_its_next_pass() {
    let context = test_context();
    context.options_mut(|options| {
        options.max_passes = 2.try_into().expect("two passes are nonzero");
    });
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let mut artifact = None;
    let mut passes = 0;
    let (output, receipt) = run_ui(&context, input(ViewportId::ROOT), |ui| {
        passes += 1;
        if passes == 1 {
            artifact = Some(stage_surface(&settlement, &mut session, ui));
            let nested = catch_unwind(AssertUnwindSafe(|| {
                let _ = run_ui(&context, input(ViewportId::ROOT), |_| {});
            }));
            assert!(nested.is_err());
            ui.ctx()
                .request_discard("verify poison survives another egui pass");
        } else {
            assert!(matches!(
                settlement.enroll(ui),
                ProductPassEnrollment::Conflict
            ));
        }
    })
    .into_parts();
    assert_eq!(passes, 2);
    output.drop_without_applying_deltas();

    receipt.report(SurfacePresentationResult::Presented);
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert!(ready.presentations.is_empty());
    assert_eq!(
        ready.presentation_failure,
        Some(PresentationSettlementFailure::Abandoned)
    );
    let artifact = artifact.expect("the first outer pass painted a surface");
    assert!(
        flush_presentations(&mut session)
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.retires(artifact))
    );
}

#[test]
fn duplicate_enrollment_poisons_every_staged_participant_in_that_pass() {
    let context = test_context();
    let first = ProductPassSettlement::default();
    let second = ProductPassSettlement::default();
    let mut first_session = measured_session();
    let mut second_session = measured_session();
    let mut artifacts = None;
    let (output, receipt) = run_ui(&context, input(ViewportId::ROOT), |ui| {
        let first_artifact = stage_surface(&first, &mut first_session, ui);
        let second_artifact = stage_surface(&second, &mut second_session, ui);
        artifacts = Some((first_artifact, second_artifact));
        assert!(matches!(first.enroll(ui), ProductPassEnrollment::Conflict));
    })
    .into_parts();
    output.drop_without_applying_deltas();
    receipt.report(SurfacePresentationResult::Presented);

    for settlement in [&first, &second] {
        let ready = take_ready(settlement, &context, ViewportId::ROOT);
        assert!(ready.presentations.is_empty());
        assert_eq!(
            ready.presentation_failure,
            Some(PresentationSettlementFailure::Abandoned)
        );
    }
    let (first_artifact, second_artifact) = artifacts.expect("both surfaces were staged");
    for (session, artifact) in [
        (&mut first_session, first_artifact),
        (&mut second_session, second_artifact),
    ] {
        assert!(
            flush_presentations(session)
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.retires(artifact))
        );
    }
}

#[test]
fn foreign_context_and_viewport_readers_cannot_drain_an_owner_delivery() {
    let context = test_context();
    let foreign_context = test_context();
    let foreign_viewport = ViewportId::from_hash_of("foreign-settlement-reader");
    let settlement = ProductPassSettlement::default();
    let mut session = measured_session();
    let (artifact, receipt) = render_surface(&context, ViewportId::ROOT, &settlement, &mut session);
    receipt.report(SurfacePresentationResult::Presented);

    for (reader_context, viewport) in [
        (&foreign_context, ViewportId::ROOT),
        (&context, foreign_viewport),
    ] {
        let ready = take_ready(&settlement, reader_context, viewport);
        assert!(ready.presentations.is_empty());
        assert_eq!(ready.presentation_failure, None);
    }
    let ready = take_ready(&settlement, &context, ViewportId::ROOT);
    assert_eq!(ready.presentations.len(), 1);
    assert_eq!(
        ready.presentations[0].output.presentation_artifact(),
        artifact
    );
    assert!(
        apply_ready(&mut session, ready)
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
}

#[test]
fn rebinding_context_or_viewport_rejects_a_retained_foreign_lane_receipt() {
    for change_context in [true, false] {
        let original_context = test_context();
        let other_context = test_context();
        let rebound_context = if change_context {
            &other_context
        } else {
            &original_context
        };
        let rebound_viewport = if change_context {
            ViewportId::ROOT
        } else {
            ViewportId::from_hash_of("rebound-settlement-lane")
        };
        let settlement = ProductPassSettlement::default();
        let mut session = measured_session();
        let (old, old_receipt) = render_surface(
            &original_context,
            ViewportId::ROOT,
            &settlement,
            &mut session,
        );
        let (current, current_receipt) =
            render_surface(rebound_context, rebound_viewport, &settlement, &mut session);

        old_receipt.report(SurfacePresentationResult::Presented);
        let ready = take_ready(&settlement, rebound_context, rebound_viewport);
        assert!(ready.presentations.is_empty());
        assert_eq!(ready.presentation_failure, None);

        current_receipt.report(SurfacePresentationResult::Presented);
        assert!(
            take_ready(&settlement, &original_context, ViewportId::ROOT)
                .presentations
                .is_empty()
        );
        let ready = take_ready(&settlement, rebound_context, rebound_viewport);
        assert_eq!(ready.presentations.len(), 1);
        assert_eq!(
            ready.presentations[0].output.presentation_artifact(),
            current
        );
        let report = apply_ready(&mut session, ready);
        assert!(
            report
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.presents(current) && commit.retires(old)),
            "a retained receipt cannot cross a context or viewport lane rebind"
        );
    }
}
