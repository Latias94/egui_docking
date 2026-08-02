mod support;

use dockspace::engine::DockEngine;
use dockspace::geometry::{LogicalPoint, LogicalRect};
use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
use dockspace::ids::{ItemId, RootId, SurfaceId};
use dockspace::pointer_receiver::{
    PointerReceiverDelivery, PointerReceiverDeliveryDisposition, PointerReceiverHoverHit,
    PointerReceiverHoverHitDisposition, PointerReceiverObservation,
    PointerReceiverObservationError, PointerReceiverProbe, PointerReceiverProbeReceipt,
    PointerReceiverUnknownReason, PresentedPointerReceiverObservation,
};
use dockspace::policy::DockPolicy;
use dockspace::presentation_hit::{PresentationHitRegionKind, PresentationPointerLane};
use support::{TestPresentationHost, publish_surface};

const SURFACE: SurfaceId = SurfaceId::new(1);
const ROOT: RootId = RootId::new(10);

fn workspace() -> Workspace {
    let mut builder = Workspace::builder();
    let tabs = builder.insert_node(Node::tabs([ItemId::new(1), ItemId::new(2)]));
    builder.set_root(ROOT, RootRecord::new(tabs).with_central(tabs));
    builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
    builder.build().expect("workspace is valid")
}

fn bounds() -> LogicalRect {
    LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("surface bounds are valid")
}

#[test]
fn delivery_and_hover_are_independent_canonical_output_bound_probes() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let interaction = engine
        .interaction_projection(SURFACE)
        .expect("published surface is interactive");
    let tab = interaction
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabBody(_)))
        .expect("tab body is present")
        .id();
    let point = LogicalPoint::new(96.0, 24.0).expect("finite test point");

    let delivery =
        PointerReceiverDelivery::new(interaction, PointerReceiverDeliveryDisposition::Dock(tab))
            .expect("tab has core-owned click or drag capability");
    let hover = PointerReceiverHoverHit::new(
        interaction,
        point,
        PointerReceiverHoverHitDisposition::Blocked,
    )
    .expect("known blocker hover result remains output-bound");
    let presented = PresentedPointerReceiverObservation::new([
        PointerReceiverProbeReceipt::HoverHit(hover),
        PointerReceiverProbeReceipt::Delivery(delivery),
    ])
    .expect("one receipt exists for each independent probe");

    assert_eq!(delivery.output(), Some(interaction.output_ticket()));
    assert_eq!(delivery.authority(), Some(interaction.authority()));
    assert_eq!(hover.output(), Some(interaction.output_ticket()));
    assert_eq!(hover.authority(), Some(interaction.authority()));
    assert_eq!(hover.point(), Some(point));
    assert_eq!(
        presented
            .probes()
            .iter()
            .map(PointerReceiverProbeReceipt::probe)
            .collect::<Vec<_>>(),
        vec![
            PointerReceiverProbe::Delivery,
            PointerReceiverProbe::HoverHit
        ]
    );
    assert!(matches!(
        PointerReceiverObservation::Presented(presented),
        PointerReceiverObservation::Presented(_)
    ));
}

#[test]
fn delivery_dock_claims_are_validated_against_their_exact_lane() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let interaction = engine
        .interaction_projection(SURFACE)
        .expect("published surface is interactive");
    let close = interaction
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabClose(_)))
        .expect("tab close is present")
        .id();

    let result = PointerReceiverDelivery::from_lanes(
        interaction,
        PointerReceiverDeliveryDisposition::NoReceiver,
        PointerReceiverDeliveryDisposition::Dock(close),
    );

    assert_eq!(
        result,
        Err(
            PointerReceiverObservationError::DockRegionDoesNotSupportLane {
                region: close,
                lane: PresentationPointerLane::Drag,
            }
        )
    );
}

#[test]
fn hover_dock_claim_must_use_a_non_passive_hover_drop_manifest_region() {
    let mut engine = DockEngine::new(workspace(), DockPolicy::default()).expect("valid engine");
    let mut host = TestPresentationHost::new(&mut engine);
    publish_surface(&mut engine, &mut host, SURFACE, bounds());
    let interaction = engine
        .interaction_projection(SURFACE)
        .expect("published surface is interactive");
    let tab = interaction
        .hit_manifest()
        .regions()
        .iter()
        .find(|region| matches!(region.id().kind(), PresentationHitRegionKind::TabBody(_)))
        .expect("tab body is present")
        .id();

    let result = PointerReceiverHoverHit::new(
        interaction,
        LogicalPoint::new(96.0, 24.0).expect("finite test point"),
        PointerReceiverHoverHitDisposition::Dock(tab),
    );
    assert_eq!(
        result,
        Err(
            PointerReceiverObservationError::DockRegionDoesNotSupportLane {
                region: tab,
                lane: PresentationPointerLane::HoverDrop,
            }
        )
    );
}

#[test]
fn unknown_probe_facts_are_explicit_and_do_not_encode_desktop_foreign_or_outside() {
    let delivery = PointerReceiverDelivery::unknown(
        PointerReceiverUnknownReason::FrameworkDeliveryUnavailable,
    );
    let hover = PointerReceiverHoverHit::unknown(PointerReceiverUnknownReason::NotReported);

    assert_eq!(
        delivery.click(),
        PointerReceiverDeliveryDisposition::Unknown(
            PointerReceiverUnknownReason::FrameworkDeliveryUnavailable
        )
    );
    assert_eq!(delivery.drag(), delivery.click());
    assert_eq!(delivery.output(), None);
    assert_eq!(delivery.authority(), None);
    assert_eq!(
        hover.disposition(),
        PointerReceiverHoverHitDisposition::Unknown(PointerReceiverUnknownReason::NotReported)
    );
    assert_eq!(hover.point(), None);
    assert_eq!(hover.output(), None);
    assert_eq!(hover.authority(), None);
}

#[test]
fn presented_observation_rejects_duplicate_probe_answers() {
    let duplicate = PresentedPointerReceiverObservation::new([
        PointerReceiverProbeReceipt::Delivery(PointerReceiverDelivery::unknown(
            PointerReceiverUnknownReason::NotReported,
        )),
        PointerReceiverProbeReceipt::Delivery(PointerReceiverDelivery::unknown(
            PointerReceiverUnknownReason::FrameworkDeliveryUnavailable,
        )),
    ]);

    assert_eq!(
        duplicate,
        Err(PointerReceiverObservationError::DuplicateProbe {
            probe: PointerReceiverProbe::Delivery,
        })
    );
}
