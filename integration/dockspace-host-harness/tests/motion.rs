#![cfg(feature = "motion")]

use std::collections::BTreeMap;
use std::time::Duration;

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockPlacement, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
    DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
};
use dockspace::motion::{MotionFrame, MotionPreference, MotionSettings};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceReceiverDescriptor, DockspaceSession, PaintedSurfaceOutput,
    SurfacePaintProgramOperation as Operation, SurfacePaintProgramStep as Step,
    SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);
const A: ItemId = ItemId::new(1);
const B: ItemId = ItemId::new(2);
const C: ItemId = ItemId::new(3);

fn metrics() -> UniformSurfaceMetrics {
    UniformSurfaceMetrics::new(
        LogicalRect::new(0.0, 0.0, 960.0, 600.0).unwrap(),
        LogicalSize::new(40.0, 30.0).unwrap(),
        80.0,
    )
    .unwrap()
}

fn motion(now: u64) -> MotionFrame {
    MotionFrame {
        now: Duration::from_millis(now),
        settings: MotionSettings::default(),
        preference: MotionPreference::Full,
    }
}

struct Sample {
    panes: BTreeMap<ItemId, LogicalRect>,
    receivers: BTreeMap<ItemId, DockspaceReceiverDescriptor>,
    partition: bool,
    splitters: usize,
    tab_offsets: Vec<f64>,
}

fn paint(session: &mut DockspaceSession, motion: MotionFrame) -> (Sample, PaintedSurfaceOutput) {
    let mut frame = session.begin_host_frame().unwrap();
    frame.set_motion(motion).unwrap();
    frame.measure_surface(SURFACE, metrics()).unwrap();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let mut sample = Sample {
        panes: BTreeMap::new(),
        receivers: BTreeMap::new(),
        partition: plan.partition_available(ROOT),
        splitters: 0,
        tab_offsets: Vec::new(),
    };
    plan.visit_surface_program(|step| {
        match step {
            Step::Operation {
                operation: Operation::Pane(pane),
                ..
            } => {
                if let Some(item) = pane.selected() {
                    sample.panes.insert(item, pane.bounds());
                    sample
                        .receivers
                        .insert(item, plan.receiver_for_pane(pane).unwrap());
                }
            }
            Step::Operation {
                operation: Operation::TabBar(bar),
                ..
            } => {
                sample.tab_offsets.push(bar.scroll_offset());
            }
            Step::Operation {
                operation: Operation::Splitter(splitter),
                ..
            } => {
                assert!(plan.receiver_for_splitter(splitter).is_some());
                sample.splitters += 1;
            }
            _ => {}
        }
        Ok::<(), ()>(())
    })
    .unwrap();
    let receipt = plan.execute_surface_program(|_| Ok::<(), ()>(())).unwrap();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    (sample, report.take_painted_outputs().pop().unwrap())
}

fn settle(session: &mut DockspaceSession) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    frame.commit().unwrap();
}

fn present(session: &mut DockspaceSession, output: PaintedSurfaceOutput) {
    session
        .report_surface_presentation(output, SurfacePresentationResult::Presented)
        .unwrap();
    settle(session);
}

fn fixture() -> (DockspaceSession, Sample) {
    let content = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        [A, B, C].map(|item| DockspaceNode::tabs([item])),
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, content),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let (before, output) = paint(&mut session, motion(0));
    present(&mut session, output);
    (session, before)
}

fn dock(session: &mut DockspaceSession, item: ItemId, after: ItemId) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .dock_item_current(item, DockPlacement::After(after))
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    assert!(frame.commit().unwrap().workspace_changed());
}

#[test]
fn independent_host_uses_core_sampled_geometry_and_restores_structural_receivers() {
    let (mut session, before) = fixture();
    dock(&mut session, C, A);
    let (start, output) = paint(&mut session, motion(1000));
    assert_eq!(start.panes[&B], before.panes[&B]);
    assert!(!start.partition);
    assert_eq!(start.splitters, 0);
    present(&mut session, output);
    let (middle, output) = paint(&mut session, motion(1050));
    assert_ne!(middle.panes[&B], before.panes[&B]);
    assert!(!middle.partition);
    assert!(session.surface_motion_is_active(SURFACE));
    present(&mut session, output);
    let (end, output) = paint(&mut session, motion(6000));
    assert!(end.partition);
    assert_eq!(end.splitters, 1);
    assert!(!session.surface_motion_is_active(SURFACE));
    present(&mut session, output);
    assert!(
        session
            .bind_presented_receiver(&end.receivers[&B])
            .is_some()
    );
}
