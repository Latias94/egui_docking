use super::*;
use crate::engine::DockEngine;
use crate::geometry::LogicalSize;
use crate::ids::ItemId;
use crate::model::{
    DockspaceAxis, DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout,
};
use crate::policy::DockPolicy;
use crate::presentation::manifest::{Measurement, TabIntrinsic, TabStripMetrics};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(1);

fn fixture(children: u16) -> (DockEngine, SurfaceMeasurements) {
    let node = DockspaceNode::equal_split(
        DockspaceAxis::Horizontal,
        (1..=children).map(|item| DockspaceNode::tabs([ItemId::new(u64::from(item))])),
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(ROOT, node),
    )])
    .unwrap();
    let engine = DockEngine::from_layout_with_presentation_config(
        layout,
        DockPolicy::default(),
        DockPresentationConfig::default(),
    )
    .unwrap();
    let requirements = engine.presentation_requirements().surface(SURFACE).unwrap();
    let mut measurements = SurfaceMeasurements::new(requirements.ticket());
    measurements
        .set_bounds(
            requirements.bounds(),
            Measurement::Measured(
                LogicalRect::new(0.0, 0.0, f64::from(children) * 128.0, 600.0).unwrap(),
            ),
        )
        .unwrap();
    for key in requirements.pane_minimums() {
        measurements
            .insert_pane_minimum(
                key,
                Measurement::Measured(LogicalSize::new(0.0, 0.0).unwrap()),
            )
            .unwrap();
    }
    for key in requirements.tab_intrinsics() {
        measurements
            .insert_tab_intrinsic(key, Measurement::Measured(TabIntrinsic::new(24.0).unwrap()))
            .unwrap();
    }
    for key in requirements.tab_strips() {
        measurements
            .insert_tab_strip(
                key,
                Measurement::Measured(TabStripMetrics::new(0.0, 0.0).unwrap()),
            )
            .unwrap();
    }
    (engine, measurements)
}

fn compile(
    engine: &DockEngine,
    measurements: &SurfaceMeasurements,
    overrides: &[SplitWeightOverride<'_>],
) -> PresentationPlan {
    compile_surface_measurements_without_guides(
        engine.workspace(),
        engine.version(),
        engine.policy_snapshot(),
        engine.presentation_config(),
        engine.presentation_requirements(),
        measurements,
        &TabStripStateStore::default(),
        overrides,
        None,
    )
    .unwrap()
}

#[test]
fn wide_split_retains_one_resize_snapshot_per_projection() {
    for children in [4, 64] {
        let (engine, measurements) = fixture(children);
        let original = compile(&engine, &measurements, &[]);
        let in_flight = original.clone();
        let splitters = original.splitter_records();
        assert_eq!(splitters.len(), usize::from(children) - 1);
        let first = &splitters[0];
        let durable_weights = first.weights().to_vec();
        assert_eq!(first.child_extents().len(), usize::from(children));
        assert_eq!(first.weights().len(), usize::from(children));
        assert!(first.child_extents().iter().all(|extent| *extent > 0.0));
        for splitter in splitters {
            assert!(std::ptr::eq(
                splitter.child_extents(),
                first.child_extents()
            ));
            assert!(std::ptr::eq(splitter.weights(), first.weights()));
            assert_eq!(
                splitter.before_bounds().width().to_bits(),
                first.child_extents()[splitter.id().index].to_bits()
            );
            assert_eq!(
                splitter.after_bounds().width().to_bits(),
                first.child_extents()[splitter.id().index + 1].to_bits()
            );
        }
        assert!(std::ptr::eq(
            first.child_extents(),
            in_flight.splitter_records()[0].child_extents()
        ));

        let source = engine
            .workspace()
            .capture_node_source(ROOT, first.id().split)
            .unwrap();
        let weights = SplitWeight::normalize((1..=children).map(f32::from)).unwrap();
        let projected = compile(
            &engine,
            &measurements,
            &[SplitWeightOverride::new(&source, &weights)],
        );
        assert_ne!(
            projected.splitter_records()[0].child_extents(),
            first.child_extents()
        );
        for splitter in projected.splitter_records() {
            assert_eq!(splitter.weights(), weights.as_slice());
            assert!(std::ptr::eq(
                splitter.weights(),
                projected.splitter_records()[0].weights()
            ));
        }
        assert_eq!(first.weights(), durable_weights.as_slice());
        assert_eq!(
            in_flight.splitter_records()[0].weights(),
            durable_weights.as_slice()
        );
    }
}
