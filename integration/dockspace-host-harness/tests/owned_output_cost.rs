//! Structural bounds of a public host's owned output, not timing or OS certification.
//!
//! Core compiler/resolver work counters provide complementary algorithmic evidence.
//! These checks measure public requests, emitted structure and the recording buffer;
//! they do not measure core allocations, renderer resources or physical presentation.

use std::mem::{needs_drop, size_of, size_of_val};

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
    ItemId, RootId, SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverRole, DockspaceSession, DockspaceVisualKind, SurfaceMeasurementAnswer,
    SurfaceMeasurementRequest, SurfacePaintProgramOperation as Operation,
    SurfacePaintProgramStep as Step, SurfacePresentationResult, SurfaceUnavailableReason,
    TabStripMetrics,
};
use dockspace_host_harness::{OwnedRecord, RecordingRenderer};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn balanced_panes(first: u64, count: usize, axis: DockspaceAxis) -> DockspaceNode {
    if count == 1 {
        return DockspaceNode::tabs([ItemId::new(first)]);
    }
    let half = count / 2;
    let next_axis = match axis {
        DockspaceAxis::Horizontal => DockspaceAxis::Vertical,
        DockspaceAxis::Vertical => DockspaceAxis::Horizontal,
    };
    DockspaceNode::equal_split(
        axis,
        [
            balanced_panes(first, half, next_axis),
            balanced_panes(first + u64::try_from(half).unwrap(), half, next_axis),
        ],
    )
    .unwrap()
}

#[test]
fn public_owned_output_structure_and_retained_buffer_are_linear_in_panes() {
    // Counting inline records is sufficient only while they own no nested allocations.
    assert!(!needs_drop::<OwnedRecord>());
    for pane_count in [16_usize, 128, 1_024] {
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            SURFACE,
            DockspaceRootLayout::new(
                ROOT,
                balanced_panes(1, pane_count, DockspaceAxis::Horizontal),
            ),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
        let mut measurement_requests = 0;
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .measure_surface_with(SURFACE, |request| {
                measurement_requests += 1;
                match request {
                    SurfaceMeasurementRequest::DockBounds { .. }
                    | SurfaceMeasurementRequest::PopupPlaneBounds { .. } => {
                        // Even the largest fixture leaves ample room for every singleton tab.
                        SurfaceMeasurementAnswer::Bounds(
                            LogicalRect::new(0.0, 0.0, 8_192.0, 8_192.0).unwrap(),
                        )
                    }
                    SurfaceMeasurementRequest::PaneMinimum { .. } => {
                        SurfaceMeasurementAnswer::PaneMinimum(LogicalSize::new(64.0, 48.0).unwrap())
                    }
                    SurfaceMeasurementRequest::TabIntrinsic { .. } => {
                        SurfaceMeasurementAnswer::TabIntrinsic(56.0)
                    }
                    SurfaceMeasurementRequest::TabStrip { .. } => {
                        SurfaceMeasurementAnswer::TabStrip(TabStripMetrics::new(0.0, 0.0).unwrap())
                    }
                }
            })
            .unwrap();
        frame.commit().unwrap();
        // One pane minimum, intrinsic and strip per leaf, plus fixed surface bounds.
        assert!((3 * pane_count + 1..=3 * pane_count + 2).contains(&measurement_requests));

        let mut steps = 0;
        let mut panes = 0;
        let mut bars = 0;
        let mut tabs = 0;
        let mut splitters = 0;
        let (recording, output) = {
            let mut frame = session.begin_host_frame().unwrap();
            let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
            // Inspect the public program; inspection itself grants no execution receipt.
            plan.visit_surface_program(|step| {
                steps += 1;
                if let Step::Operation { operation, .. } = step {
                    match operation {
                        Operation::Pane(_) => panes += 1,
                        Operation::TabBar(_) => bars += 1,
                        Operation::Tab(_) => tabs += 1,
                        Operation::Splitter(_) => splitters += 1,
                        _ => {}
                    }
                }
                Ok::<(), ()>(())
            })
            .unwrap();
            let (recording, receipt) = RecordingRenderer::execute(plan).unwrap();
            frame.confirm_surface_painted(receipt).unwrap();
            let mut outputs = frame.commit().unwrap().take_painted_outputs();
            assert_eq!(outputs.len(), 1);
            (recording, outputs.pop().unwrap())
        };

        // Every assertion on owned records runs after the frame borrow ended.
        assert_eq!(
            (panes, bars, tabs, splitters),
            (pane_count, pane_count, pane_count, pane_count - 1)
        );
        assert_eq!(
            recording.records.first(),
            Some(&OwnedRecord::RootBegin(ROOT))
        );
        assert!(
            matches!(recording.records.last(), Some(OwnedRecord::SurfaceEnd(semantic))
            if output.matches_semantic_output(*semantic))
        );
        assert_eq!(
            recording
                .records
                .iter()
                .filter(|record| matches!(record, OwnedRecord::PaneContent { item: Some(_), .. }))
                .count(),
            pane_count,
        );
        assert_eq!(
            recording.records.iter().filter(|record| matches!(record,
                OwnedRecord::Geometry { visual, .. } if visual.kind() == DockspaceVisualKind::Pane
            )).count(),
            pane_count,
        );
        assert_eq!(
            recording.records.iter().filter(|record| matches!(record,
                OwnedRecord::Receiver(receiver) if receiver.role() == DockspaceReceiverRole::TabBody
            )).count(),
            pane_count,
        );

        // Each leaf has bounded chrome, targets and guides; a binary tree adds N-1 splitters.
        // Allow spare Vec capacity without shrinking it or depending on an exact growth factor.
        let step_budget = 16 * pane_count + 16;
        let record_budget = 32 * pane_count + 32;
        let retained_bytes =
            size_of_val(&recording) + recording.records.capacity() * size_of::<OwnedRecord>();
        let retained_budget =
            size_of::<RecordingRenderer>() + 2 * record_budget * size_of::<OwnedRecord>();
        assert!(
            steps <= step_budget,
            "{pane_count} panes emitted {steps} steps"
        );
        assert!(recording.records.len() >= steps);
        assert!(
            recording.records.len() <= record_budget,
            "{pane_count} panes retained {} records",
            recording.records.len()
        );
        assert!(
            retained_bytes <= retained_budget,
            "{pane_count} panes retained {retained_bytes} recording bytes"
        );
        eprintln!(
            "panes={pane_count} measurements={measurement_requests} steps={steps} records={} capacity={} retained_recording_bytes={retained_bytes}",
            recording.records.len(),
            recording.records.capacity(),
        );

        // Retire the exact output without claiming that recorded work reached a renderer.
        let artifact = output.presentation_artifact();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Dropped)
            .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let settled = frame.commit().unwrap();
        let [commit] = settled.surface_presentation_commits() else {
            panic!("one exact output must settle")
        };
        assert!(commit.retires(artifact));
        assert!(!commit.presents(artifact));
        assert!(session.presented_surface(SURFACE).is_none());
    }
}
