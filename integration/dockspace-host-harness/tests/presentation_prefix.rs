//! Presentation prefix retirement preserves the last actually presented output.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverDescriptor, DockspaceSession, HostFrameReport, PaintedSurfaceOutput,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};
use dockspace_host_harness::RecordingRenderer;

const SURFACE: SurfaceId = SurfaceId::new(1);
const FIRST: ItemId = ItemId::new(1);

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([FIRST, ItemId::new(2)])),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                96.0,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    session
}

fn paint(session: &mut DockspaceSession) -> (DockspaceReceiverDescriptor, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let receiver = plan.tab_receiver(FIRST).unwrap();
    let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    let mut outputs = frame.commit().unwrap().take_painted_outputs();
    assert_eq!(outputs.len(), 1);
    (receiver, outputs.pop().unwrap())
}

fn emit_after_measurement(
    session: &mut DockspaceSession,
    tab_intrinsic: f64,
) -> (DockspaceReceiverDescriptor, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                LogicalSize::new(64.0, 48.0).unwrap(),
                tab_intrinsic,
            )
            .unwrap(),
        )
        .unwrap();
    frame.commit().unwrap();
    paint(session)
}

fn settle(session: &mut DockspaceSession) -> HostFrameReport {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap()
}

#[test]
fn later_dropped_output_preserves_the_last_actually_presented_output() {
    let mut session = session();
    let (receiver, presented) = paint(&mut session);
    let (same_receiver, dropped) = paint(&mut session);
    assert_eq!(receiver, same_receiver);
    let artifact = presented.presentation_artifact();
    let dropped_artifact = dropped.presentation_artifact();
    session
        .report_surface_presentation(presented, SurfacePresentationResult::Presented)
        .unwrap();
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    let report = settle(&mut session);
    let [commit] = report.surface_presentation_commits() else {
        panic!("the complete Presented/Dropped prefix must settle once");
    };
    assert!(
        commit.presents(artifact),
        "a later dropped output must preserve the last actual presentation"
    );
    assert!(commit.retires(dropped_artifact));
    assert!(
        session.bind_presented_receiver(&receiver).is_some(),
        "the visible unchanged scene must retain input authority"
    );
}

#[test]
fn separately_settled_dropped_output_preserves_the_last_actually_presented_output() {
    for abandon_settlement in [false, true] {
        let mut session = session();
        let (receiver, presented) = paint(&mut session);
        let (same_receiver, dropped) = paint(&mut session);
        assert_eq!(receiver, same_receiver);
        let artifact = presented.presentation_artifact();
        let dropped_artifact = dropped.presentation_artifact();
        session
            .report_surface_presentation(presented, SurfacePresentationResult::Presented)
            .unwrap();
        let first = settle(&mut session);
        let [first_commit] = first.surface_presentation_commits() else {
            panic!("A must settle before B's result arrives");
        };
        assert!(first_commit.presents(artifact));
        assert!(!first_commit.settles(dropped_artifact));
        let surface = session
            .presented_surface(SURFACE)
            .expect("settling Presented(A) must grant A's surface authority");
        let bound = session
            .bind_presented_receiver(&receiver)
            .expect("settling Presented(A) must grant A's receiver authority");

        session
            .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
            .unwrap();
        if abandon_settlement {
            let mut abandoned = session.begin_host_frame().unwrap();
            abandoned
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(abandoned);
            assert_eq!(session.presented_surface(SURFACE), Some(surface));
            assert_eq!(session.bind_presented_receiver(&receiver), Some(bound));
        }
        let later = settle(&mut session);
        let [later_commit] = later.surface_presentation_commits() else {
            panic!("B's dropped result must remain settleable after an abandoned candidate");
        };
        assert!(
            later_commit.presents(artifact),
            "settling Dropped(B) in a later frame must retain A's artifact; abandoned: {abandon_settlement}"
        );
        assert!(later_commit.retires(dropped_artifact));
        assert_eq!(
            session.presented_surface(SURFACE),
            Some(surface),
            "retiring B must preserve A's exact surface authority"
        );
        assert_eq!(
            session.bind_presented_receiver(&receiver),
            Some(bound),
            "retiring B must preserve A's exact receiver authority"
        );
    }
}

#[test]
fn every_three_output_terminal_order_preserves_the_exact_contiguous_prefix() {
    let orders = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    for order in orders {
        for presented_mask in 0_u8..8 {
            for drop_handle in [false, true] {
                let trace = format!(
                    "order={order:?}, presented={presented_mask:#05b}, drop_handle={drop_handle}"
                );
                let mut session = session();
                let (original_receiver, original) = paint(&mut session);
                let original_artifact = original.presentation_artifact();
                session
                    .report_surface_presentation(original, SurfacePresentationResult::Presented)
                    .unwrap();
                settle(&mut session);
                let version = session.version();
                let mut receivers = vec![original_receiver];
                let mut artifacts = vec![original_artifact];
                let mut outputs = Vec::new();
                for width in [128.0, 192.0, 256.0] {
                    let (receiver, output) = emit_after_measurement(&mut session, width);
                    assert!(!receivers.contains(&receiver), "{trace}");
                    receivers.push(receiver);
                    artifacts.push(output.presentation_artifact());
                    outputs.push(Some(output));
                }
                let mut received = [false; 3];
                let mut settled = 0;
                let mut visible = 0;
                for index in order {
                    let output = outputs[index].take().unwrap();
                    let presented = presented_mask & (1 << index) != 0;
                    if !presented && drop_handle {
                        drop(output);
                    } else {
                        session
                            .report_surface_presentation(
                                output,
                                if presented {
                                    SurfacePresentationResult::Presented
                                } else {
                                    SurfacePresentationResult::Dropped
                                },
                            )
                            .unwrap();
                    }
                    received[index] = true;
                    let previous = session.presented_surface(SURFACE);
                    let abandoned = session.begin_host_frame().unwrap();
                    drop(abandoned);
                    assert_eq!(session.presented_surface(SURFACE), previous, "{trace}");
                    assert_eq!(session.version(), version, "{trace}");

                    let before = settled;
                    while settled < received.len() && received[settled] {
                        if presented_mask & (1 << settled) != 0 {
                            visible = settled + 1;
                        }
                        settled += 1;
                    }
                    let report = settle(&mut session);
                    assert_eq!(
                        report.surface_presentation_commits().len(),
                        usize::from(before != settled),
                        "{trace}, received={received:?}"
                    );
                    if let Some(commit) = report.surface_presentation_commits().first() {
                        assert!(commit.presents(artifacts[visible]), "{trace}");
                        for (ordinal, artifact) in artifacts.iter().enumerate().skip(1) {
                            assert_eq!(commit.settles(*artifact), ordinal <= settled, "{trace}");
                        }
                    }
                    // A visibly presented suffix revokes old hit geometry even
                    // while its missing predecessor blocks prefix admission.
                    let blocked = received.iter().enumerate().any(|(index, received)| {
                        index >= settled && *received && presented_mask & (1 << index) != 0
                    });
                    assert_eq!(
                        session.presented_surface(SURFACE).is_none(),
                        blocked,
                        "{trace}"
                    );
                    if !blocked {
                        assert!(
                            session
                                .presented_surface(SURFACE)
                                .unwrap()
                                .matches_presentation_artifact(artifacts[visible]),
                            "{trace}"
                        );
                    }
                    for (ordinal, receiver) in receivers.iter().enumerate() {
                        assert_eq!(
                            session.bind_presented_receiver(receiver).is_some(),
                            !blocked && ordinal == visible,
                            "{trace}, received={received:?}, receiver={ordinal}"
                        );
                    }
                    assert_eq!(session.version(), version, "{trace}");
                    assert!(report.manipulation_feedback().is_empty(), "{trace}");
                    assert!(!report.workspace_changed(), "{trace}");
                }
                assert_eq!(settled, 3, "{trace}");
                assert!(
                    settle(&mut session)
                        .surface_presentation_commits()
                        .is_empty(),
                    "{trace}"
                );
            }
        }
    }
}

#[test]
fn last_presented_output_is_kept_when_the_earlier_output_was_dropped() {
    let mut session = session();
    let (_, dropped) = paint(&mut session);
    let (receiver, presented) = paint(&mut session);
    let artifact = presented.presentation_artifact();
    session
        .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
        .unwrap();
    session
        .report_surface_presentation(presented, SurfacePresentationResult::Presented)
        .unwrap();
    let report = settle(&mut session);
    assert!(
        report
            .surface_presentation_commits()
            .iter()
            .any(|commit| commit.presents(artifact))
    );
    assert!(session.bind_presented_receiver(&receiver).is_some());
}

fn assert_pending_measurements_remain_presentable(tab_intrinsics: &[f64]) {
    let mut session = session();
    let outputs = tab_intrinsics
        .iter()
        .map(|&width| emit_after_measurement(&mut session, width))
        .collect::<Vec<_>>();
    for (scenes, widths) in outputs.windows(2).zip(tab_intrinsics.windows(2)) {
        assert_eq!(scenes[0].0 == scenes[1].0, widths[0] == widths[1]);
    }
    for (ordinal, (receiver, output)) in outputs.into_iter().enumerate() {
        let artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let report = settle(&mut session);
        let [commit] = report.surface_presentation_commits() else {
            panic!("each separately reported output must settle once");
        };
        assert!(commit.presents(artifact));
        assert!(
            session.bind_presented_receiver(&receiver).is_some(),
            "pending emission {ordinal} lost its measured scene: {tab_intrinsics:?}"
        );
    }
}

#[test]
fn review_three_pending_tab_measurements_remain_presentable_in_emission_order() {
    assert_pending_measurements_remain_presentable(&[96.0, 104.0, 112.0]);
}

#[test]
fn two_pending_tab_measurements_remain_presentable_in_emission_order() {
    assert_pending_measurements_remain_presentable(&[96.0, 104.0]);
}

#[test]
fn identical_tab_measurements_reuse_the_scene_across_three_pending_outputs() {
    assert_pending_measurements_remain_presentable(&[96.0, 96.0, 96.0]);
}

#[test]
fn configuration_replacement_retains_the_encoded_scene_without_reviving_its_input() {
    for replace_geometry in [false, true] {
        let mut session = session();
        let (old_receiver, old_output) = paint(&mut session);
        let old_artifact = old_output.presentation_artifact();
        let mut frame = session.begin_host_frame().unwrap();
        if replace_geometry {
            let config = dockspace::runtime::DockPresentationConfig::builder()
                .tab_bar_height(36.0)
                .build()
                .unwrap();
            frame.replace_presentation_config(config).unwrap();
        } else {
            let mut policy = DockPolicy::default();
            policy.set_arrangement_locked(true);
            frame.replace_policy(policy).unwrap();
        }
        frame
            .measure_surface(
                SURFACE,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
                    LogicalSize::new(64.0, 48.0).unwrap(),
                    96.0,
                )
                .unwrap(),
            )
            .unwrap();
        let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
        let receiver = plan.tab_receiver(FIRST).unwrap();
        let (_, receipt) = RecordingRenderer::execute(plan).unwrap();
        frame.confirm_surface_painted(receipt).unwrap();
        let output = frame
            .commit()
            .unwrap()
            .take_painted_outputs()
            .pop()
            .unwrap();
        session
            .report_surface_presentation(old_output, SurfacePresentationResult::Presented)
            .unwrap();
        let old_settlement = settle(&mut session);
        assert!(
            old_settlement
                .surface_presentation_commits()
                .iter()
                .any(|commit| commit.settles(old_artifact)),
            "the old encoded output keeps its own terminal owner"
        );
        assert!(session.bind_presented_receiver(&old_receiver).is_none());
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        settle(&mut session);
        assert!(session.bind_presented_receiver(&old_receiver).is_none());
        assert!(session.bind_presented_receiver(&receiver).is_some());
    }
}

#[test]
fn review_reverse_results_present_the_last_non_dropped_tab_measurement() {
    for newest_result in [
        SurfacePresentationResult::Presented,
        SurfacePresentationResult::Dropped,
    ] {
        let mut session = session();
        let (first_receiver, first) = emit_after_measurement(&mut session, 96.0);
        let (middle_receiver, middle) = emit_after_measurement(&mut session, 104.0);
        let (last_receiver, last) = emit_after_measurement(&mut session, 112.0);
        let first_artifact = first.presentation_artifact();
        let middle_artifact = middle.presentation_artifact();
        let last_artifact = last.presentation_artifact();
        session
            .report_surface_presentation(last, newest_result)
            .unwrap();
        assert!(
            settle(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
        session
            .report_surface_presentation(middle, SurfacePresentationResult::Presented)
            .unwrap();
        assert!(
            settle(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
        assert!(session.presented_surface(SURFACE).is_none());
        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .unwrap();
        let report = settle(&mut session);
        let [commit] = report.surface_presentation_commits() else {
            panic!("reverse reports must settle only the complete emission prefix");
        };
        let (visible_artifact, receiver) = if newest_result == SurfacePresentationResult::Presented
        {
            (last_artifact, last_receiver)
        } else {
            (middle_artifact, middle_receiver)
        };
        assert!(commit.presents(visible_artifact));
        assert!(commit.retires(first_artifact));
        assert_eq!(
            commit.retires(middle_artifact),
            middle_artifact != visible_artifact
        );
        assert_eq!(
            commit.retires(last_artifact),
            last_artifact != visible_artifact
        );
        assert!(session.bind_presented_receiver(&first_receiver).is_none());
        assert!(session.bind_presented_receiver(&receiver).is_some());
        assert!(
            settle(&mut session)
                .surface_presentation_commits()
                .is_empty()
        );
        assert!(session.bind_presented_receiver(&receiver).is_some());
    }
}

#[test]
fn review_separately_dropped_newer_measurement_preserves_the_middle_presentation() {
    for abandon_settlement in [false, true] {
        let mut session = session();
        let (_, first) = emit_after_measurement(&mut session, 96.0);
        let (receiver, middle) = emit_after_measurement(&mut session, 104.0);
        let (_, dropped) = emit_after_measurement(&mut session, 112.0);
        let artifact = middle.presentation_artifact();
        let dropped_artifact = dropped.presentation_artifact();
        session
            .report_surface_presentation(first, SurfacePresentationResult::Presented)
            .unwrap();
        session
            .report_surface_presentation(middle, SurfacePresentationResult::Presented)
            .unwrap();
        let first_report = settle(&mut session);
        let [first_commit] = first_report.surface_presentation_commits() else {
            panic!("the middle presentation must settle before the newest result arrives");
        };
        assert!(first_commit.presents(artifact));
        assert!(!first_commit.settles(dropped_artifact));
        let surface = session.presented_surface(SURFACE).unwrap();
        let bound = session.bind_presented_receiver(&receiver).unwrap();
        session
            .report_surface_presentation(dropped, SurfacePresentationResult::Dropped)
            .unwrap();
        if abandon_settlement {
            let mut abandoned = session.begin_host_frame().unwrap();
            abandoned
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            drop(abandoned);
            assert_eq!(session.presented_surface(SURFACE), Some(surface));
            assert_eq!(session.bind_presented_receiver(&receiver), Some(bound));
        }
        let report = settle(&mut session);
        let [commit] = report.surface_presentation_commits() else {
            panic!("the newest dropped result must remain settleable after an abandoned frame");
        };
        assert!(commit.presents(artifact));
        assert!(commit.retires(dropped_artifact));
        assert_eq!(session.presented_surface(SURFACE), Some(surface));
        assert_eq!(session.bind_presented_receiver(&receiver), Some(bound));
    }
}
