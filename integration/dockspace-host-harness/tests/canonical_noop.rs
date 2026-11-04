//! Valid split weights remain exact across no-op public docking commands.

use dockspace::model::{
    DockAnchor, DockEdge, DockFraction, DockPlacement, DockspaceActionOutcome, DockspaceAxis,
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{DockspaceSession, HostInputOutcome};

#[test]
fn self_docking_does_not_round_unrelated_weights_or_advance_the_version() {
    let root = RootId::new(1);
    let item = ItemId::new(1);
    let weights = [0.8, 0.1, 0.033_333_335, 0.066_666_6];
    let content = DockspaceNode::split(
        DockspaceAxis::Vertical,
        weights.into_iter().enumerate().map(|(index, weight)| {
            (
                DockspaceNode::tabs([ItemId::new(u64::try_from(index).unwrap() + 1)]),
                weight,
            )
        }),
    )
    .unwrap();
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SurfaceId::new(1),
        DockspaceRootLayout::new(root, content),
    )])
    .unwrap();
    let mut session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let before = session
        .view()
        .root(root)
        .unwrap()
        .content()
        .unwrap()
        .split()
        .unwrap()
        .weights()
        .collect::<Vec<_>>();
    let version = session.version();
    for edge in [
        DockEdge::Left,
        DockEdge::Right,
        DockEdge::Top,
        DockEdge::Bottom,
    ] {
        let mut frame = session.begin_host_frame().unwrap();
        frame
            .dock_item_current(
                item,
                DockPlacement::InnerEdge {
                    anchor: DockAnchor::Item(item),
                    edge,
                    fraction: DockFraction::new(0.75).unwrap(),
                },
            )
            .unwrap();
        let report = frame.commit().unwrap();
        assert!(matches!(
            report.inputs(),
            [HostInputOutcome::ProductActionApplied(
                DockspaceActionOutcome::Docked { changed: false, .. }
            )]
        ));
        assert_eq!(session.version(), version);
        let after = session
            .view()
            .root(root)
            .unwrap()
            .content()
            .unwrap()
            .split()
            .unwrap()
            .weights()
            .collect::<Vec<_>>();
        assert_eq!(after, before, "{edge:?} must preserve exact split shares");
    }
}
