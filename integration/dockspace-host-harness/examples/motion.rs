//! Renderer-free cost probe through the public optional motion contract.

use std::{
    hint::black_box,
    time::{Duration, Instant},
};

use dockspace::{
    geometry::{LogicalRect, LogicalSize},
    model::{
        DockPlacement, DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, ItemId, RootId, SurfaceId,
    },
    motion::{Curve, MotionFrame, MotionMode, MotionPreference, MotionSettings, Transition},
    policy::DockPolicy,
    runtime::{
        DockspaceSession, SurfacePresentationResult, SurfaceUnavailableReason,
        UniformSurfaceMetrics,
    },
};

const SURFACE: SurfaceId = SurfaceId::new(1);
const SAMPLES: u32 = 500;

fn tree(first: u64, count: u32, axis: DockspaceAxis) -> DockspaceNode {
    if count == 1 {
        return DockspaceNode::tabs([ItemId::new(first)]);
    }
    let next = match axis {
        DockspaceAxis::Horizontal => DockspaceAxis::Vertical,
        DockspaceAxis::Vertical => DockspaceAxis::Horizontal,
    };
    DockspaceNode::equal_split(
        axis,
        [
            tree(first, count / 2, next),
            tree(first + u64::from(count / 2), count - count / 2, next),
        ],
    )
    .unwrap()
}

fn render(
    session: &mut DockspaceSession,
    now: Duration,
    settings: MotionSettings,
) -> (Duration, Duration) {
    let mut frame = session.begin_host_frame().unwrap();
    frame
        .set_motion(MotionFrame {
            now,
            settings,
            preference: MotionPreference::Full,
        })
        .unwrap();
    let start = Instant::now();
    frame
        .measure_surface(
            SURFACE,
            UniformSurfaceMetrics::new(
                LogicalRect::new(0.0, 0.0, 3840.0, 2160.0).unwrap(),
                LogicalSize::new(10.0, 10.0).unwrap(),
                80.0,
            )
            .unwrap(),
        )
        .unwrap();
    let preparation = start.elapsed();
    let plan = frame.paint_plan(SURFACE).unwrap().unwrap();
    let start = Instant::now();
    let receipt = plan
        .execute_surface_program(|step| {
            black_box(step);
            Ok::<(), ()>(())
        })
        .unwrap();
    let walk = start.elapsed();
    frame.confirm_surface_painted(receipt).unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let mut report = frame.commit().unwrap();
    session
        .report_surface_presentation(
            report.take_painted_outputs().pop().unwrap(),
            SurfacePresentationResult::Presented,
        )
        .unwrap();
    (preparation, walk)
}

fn main() {
    println!("panes,mode,target_prepare_us,mean_tick_prepare_us,mean_program_walk_us");
    for count in [16, 64] {
        for mode in [MotionMode::Off, MotionMode::System] {
            let settings = MotionSettings {
                mode,
                // Keep the cost probe active for every sample; this is not UI tuning.
                layout: Transition::Tween {
                    duration: Duration::from_secs(10),
                    curve: Curve::Linear,
                },
                ..Default::default()
            };
            let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
                SURFACE,
                DockspaceRootLayout::new(RootId::new(1), tree(1, count, DockspaceAxis::Horizontal)),
            )])
            .unwrap();
            let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
            render(&mut session, Duration::ZERO, settings);
            let mut frame = session.begin_host_frame().unwrap();
            frame
                .dock_item_current(
                    ItemId::new(u64::from(count)),
                    DockPlacement::After(ItemId::new(1)),
                )
                .unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
            let (target, _) = render(&mut session, Duration::from_secs(1), settings);
            let mut tick = Duration::ZERO;
            let mut walk = Duration::ZERO;
            for sample in 1..=SAMPLES {
                let (sample_tick, sample_walk) = render(
                    &mut session,
                    Duration::from_secs(1) + Duration::from_micros(u64::from(sample)),
                    settings,
                );
                tick += sample_tick;
                walk += sample_walk;
            }
            println!(
                "{count},{mode:?},{:.3},{:.3},{:.3}",
                target.as_secs_f64() * 1e6,
                tick.as_secs_f64() * 1e6 / f64::from(SAMPLES),
                walk.as_secs_f64() * 1e6 / f64::from(SAMPLES)
            );
        }
    }
    println!(
        "Preparation includes public measurement admission and scene records. Program walk excludes application content, text layout, GPU rendering, and OS presentation. Run workbench --exercise for editor and renderer costs."
    );
}
