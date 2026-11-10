//! Presentation fractions must remain valid in the docking model's representation.

use dockspace::geometry::{LogicalRect, LogicalSize};
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockPresentationConfig, DockPresentationConfigError, DockspaceSession, UniformSurfaceMetrics,
};

#[test]
fn rejects_fractions_that_round_to_model_endpoints() {
    for fraction in [0.999_999_99, 1.0e-50] {
        assert!(matches!(
            DockPresentationConfig::builder()
                .dock_fraction(fraction)
                .build(),
            Err(DockPresentationConfigError::DockFractionOutOfRange { value })
                if value == fraction
        ));
    }
}

#[test]
fn representable_fractions_measure_a_surface() {
    for fraction in [0.25, f64::from(f32::from_bits(1.0_f32.to_bits() - 1))] {
        let surface = SurfaceId::new(1);
        let config = DockPresentationConfig::builder()
            .dock_fraction(fraction)
            .build()
            .unwrap();
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(RootId::new(1), DockspaceNode::tabs([ItemId::new(1)])),
        )])
        .unwrap();
        let mut session = DockspaceSession::from_layout_with_presentation_config(
            layout,
            DockPolicy::default(),
            config,
        )
        .unwrap();
        let mut frame = session.begin_host_frame().unwrap();
        let metrics = UniformSurfaceMetrics::new(
            LogicalRect::new(0.0, 0.0, 800.0, 600.0).unwrap(),
            LogicalSize::new(64.0, 48.0).unwrap(),
            96.0,
        )
        .unwrap();
        frame.measure_surface(surface, metrics).unwrap();
        frame.commit().unwrap();
    }
}

#[test]
fn positive_model_subnormal_fraction_is_accepted() {
    DockPresentationConfig::builder()
        .dock_fraction(f64::from(f32::from_bits(1)))
        .build()
        .unwrap();
}
