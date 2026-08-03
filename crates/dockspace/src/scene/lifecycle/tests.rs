//! Surface presentation lifecycle regression tests.

use std::collections::{BTreeMap, BTreeSet};

use super::*;
use crate::ids::{EngineAuthorityDomainId, WorkspaceEpoch, WorkspaceRevision};
use crate::policy::TabBarPolicy;
use crate::presentation_config::PresentationConfigRevision;
use crate::presentation_observation::{
    PresentationAuthorityRejection, PresentationOutputSerial, PresentedSurfaceAuthority,
};
use crate::scene_manifest::{
    PolicyRevision, RequirementRevision, SceneRequirementDraft, SurfaceRequirementRevision,
    SurfaceRequirements, TabBarRequirement, TabStripKey,
};
use crate::transition::WorkspaceVersion;
use crate::viewport::CoordinateGeneration;

fn manifest(
    surface: SurfaceId,
    surface_revision: u64,
    manifest_revision: u64,
) -> SceneRequirementManifest {
    manifest_for_roster(
        [(surface, surface_revision)],
        manifest_revision,
        PopupPlaneRequirement::default(),
        None,
    )
}

fn manifest_for_roster(
    surfaces: impl IntoIterator<Item = (SurfaceId, u64)>,
    manifest_revision: u64,
    popup: PopupPlaneRequirement,
    popup_owner: Option<TabStripStateKey>,
) -> SceneRequirementManifest {
    let authority = EngineAuthorityDomainId::new_for_test(73);
    let epoch = WorkspaceEpoch::new(5);
    let config = PresentationConfigRevision::default();
    let policy = PolicyRevision::default();
    let surfaces = surfaces
        .into_iter()
        .map(|(surface, surface_revision)| {
            let ticket = SurfaceMeasurementTicket::new(
                authority,
                epoch,
                config,
                policy,
                SurfaceRequirementRevision::new(surface_revision),
                surface,
            );
            let (tab_strips, tab_bars) = popup_owner
                .filter(|owner| owner.surface() == surface)
                .map_or_else(
                    || (BTreeSet::new(), BTreeMap::new()),
                    |owner| {
                        let bar = owner.bar();
                        (
                            BTreeSet::from([TabStripKey::new(surface, bar)]),
                            BTreeMap::from([(
                                bar,
                                TabBarRequirement::new(
                                    bar,
                                    TabBarPolicy::default(),
                                    BTreeMap::new(),
                                ),
                            )]),
                        )
                    },
                );
            let requirements = SurfaceRequirements::new(
                ticket,
                BTreeSet::new(),
                BTreeSet::new(),
                tab_strips,
                tab_bars,
            )
            .expect("test requirements are coherent");
            (surface, requirements)
        })
        .collect();
    SceneRequirementDraft::new_for_test(
        authority,
        WorkspaceVersion::new(epoch, WorkspaceRevision::default()),
        config,
        policy,
        RequirementRevision::new(manifest_revision),
        surfaces,
    )
    .and_then(|draft| draft.finalize(popup))
    .expect("test manifest is coherent")
}

fn candidate_with_popup(
    ticket: SurfaceMeasurementTicket,
    popup: PopupPlaneRequirement,
) -> PresentationPlan {
    let bounds = LogicalRect::new(0.0, 0.0, 640.0, 480.0).expect("positive bounds");
    PresentationPlan::from_measurements(
        ticket,
        popup,
        bounds,
        matches!(popup, PopupPlaneRequirement::Active { .. }).then_some(bounds),
    )
}

fn headless_capture() -> SurfaceCoordinateCapture {
    SurfaceCoordinateCapture::Headless {
        authority_generation: CoordinateGeneration::default(),
    }
}

fn install(
    scenes: &mut SurfaceSceneSet,
    ticket: SurfaceMeasurementTicket,
    serial: u64,
) -> (SurfaceSceneStamp, SurfacePresentationOutputTicket) {
    install_with_popup(scenes, ticket, PopupPlaneRequirement::default(), serial)
}

fn install_with_popup(
    scenes: &mut SurfaceSceneSet,
    ticket: SurfaceMeasurementTicket,
    popup: PopupPlaneRequirement,
    serial: u64,
) -> (SurfaceSceneStamp, SurfacePresentationOutputTicket) {
    scenes
        .install_ready(
            candidate_with_popup(ticket, popup),
            headless_capture(),
            ticket.authority_domain(),
            PresentationOutputSerial::new_for_test(serial),
        )
        .expect("candidate installs")
}

fn acknowledge(
    scenes: &mut SurfaceSceneSet,
    ticket: SurfacePresentationOutputTicket,
    serial: u64,
) -> Result<bool, PresentationAuthorityRejection> {
    scenes
        .accept_observed_authority(
            PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
                ticket,
                CoordinateGeneration::default(),
                serial,
            ),
        )
        .map(|changed| !changed.is_empty())
}

fn observed(output: SurfacePresentationOutputTicket, emission: u64) -> PresentedSurfaceAuthority {
    PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
        output,
        CoordinateGeneration::default(),
        emission,
    )
}

fn raw_interaction_authority(
    scenes: &SurfaceSceneSet,
    surface: SurfaceId,
) -> Option<PresentedSurfaceAuthority> {
    scenes
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .and_then(ReadySurfaceScene::interaction_authority)
}

#[test]
fn unpainted_projection_cannot_hit_or_survive_requirement_invalidation() {
    let surface = SurfaceId::new(11);
    let initial = manifest(surface, 0, 0);
    let ticket = initial.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
    let _ = install(&mut scenes, ticket, 1);

    assert!(scenes.ready_surface(surface).is_none());

    scenes
        .reconcile_manifest(&manifest(surface, 1, 1))
        .expect("requirements reconcile");
    assert!(matches!(
        scenes.surface(surface),
        Some(SurfaceScene::Bootstrap(_))
    ));
    assert!(
        scenes
            .surface(surface)
            .and_then(SurfaceScene::paint_projection)
            .is_none()
    );
}

#[test]
fn stale_fallback_survives_an_unpainted_ready_candidate_without_regaining_hit_authority() {
    let surface = SurfaceId::new(12);
    let initial = manifest(surface, 0, 0);
    let initial_ticket = initial.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
    let (painted, painted_output) = install(&mut scenes, initial_ticket, 1);
    assert_eq!(acknowledge(&mut scenes, painted_output, 1), Ok(true));

    let changed = manifest(surface, 1, 1);
    scenes
        .reconcile_manifest(&changed)
        .expect("requirements reconcile");
    assert!(scenes.ready_surface(surface).is_none());

    let changed_ticket = changed.surface(surface).expect("surface exists").ticket();
    let (pending, _) = install(&mut scenes, changed_ticket, 2);
    assert_ne!(painted, pending);
    assert!(scenes.ready_surface(surface).is_none());
    let projection = scenes
        .surface(surface)
        .and_then(SurfaceScene::paint_projection)
        .expect("next candidate projects");
    assert_eq!(projection.plan_stamp(), pending);
    assert!(scenes.interaction_projection(surface).is_none());

    scenes
        .demote_to_stale(surface, StaleSurfaceSceneReason::CoordinateAuthorityChanged)
        .expect("painted fallback permits stale transition");
    assert!(scenes.ready_surface(surface).is_none());
    let stale = scenes
        .surface(surface)
        .and_then(SurfaceScene::paint_projection)
        .expect("painted fallback remains visible");
    assert_eq!(stale.plan_stamp(), painted);
    assert!(scenes.interaction_projection(surface).is_none());
}

#[test]
fn exact_observation_grants_fallback_and_interaction_authority() {
    let surface = SurfaceId::new(13);
    let manifest = manifest(surface, 0, 0);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

    let (painted, output) = install(&mut scenes, ticket, 1);
    assert!(scenes.ready_surface(surface).is_none());
    assert_eq!(acknowledge(&mut scenes, output, 1), Ok(true));
    let scene = scenes.surface(surface).expect("surface remains rostered");
    let ready = scene.ready().expect("painted candidate is ready");

    assert_eq!(ready.candidate().stamp(), painted);
    assert_eq!(
        ready.paint_fallback().map(SurfacePlanScene::stamp),
        Some(painted)
    );
    assert_eq!(
        ready
            .interaction_authority()
            .map(|authority| authority.ticket()),
        Some(output)
    );
    assert_eq!(
        scenes.ready_surface(surface).map(SurfacePlanScene::stamp),
        Some(painted)
    );
}

#[test]
fn inactive_surface_authority_does_not_wait_for_a_bootstrap_sibling() {
    let first = SurfaceId::new(131);
    let second = SurfaceId::new(132);
    let manifest = manifest_for_roster(
        [(first, 0), (second, 0)],
        0,
        PopupPlaneRequirement::default(),
        None,
    );
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
    let (_, first_output) = install(
        &mut scenes,
        manifest.surface(first).expect("first exists").ticket(),
        1,
    );
    assert_eq!(
        scenes.accept_observed_authority(observed(first_output, 1)),
        Ok(BTreeSet::from([first]))
    );
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_none());
    assert_eq!(
        raw_interaction_authority(&scenes, first).map(PresentedSurfaceAuthority::ticket),
        Some(first_output)
    );
    assert!(raw_interaction_authority(&scenes, second).is_none());
    assert!(matches!(
        scenes.surface(second),
        Some(SurfaceScene::Bootstrap(_))
    ));

    let (_, second_output) = install(
        &mut scenes,
        manifest.surface(second).expect("second exists").ticket(),
        2,
    );

    assert_eq!(
        scenes.accept_observed_authority(observed(second_output, 2)),
        Ok(BTreeSet::from([second]))
    );
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_some());
    assert_eq!(
        raw_interaction_authority(&scenes, first).map(PresentedSurfaceAuthority::ticket),
        Some(first_output)
    );
    assert_eq!(
        raw_interaction_authority(&scenes, second).map(PresentedSurfaceAuthority::ticket),
        Some(second_output)
    );
}

#[test]
fn popup_gate_presentation_is_independent_of_acknowledgement_order() {
    fn present(order: [SurfaceId; 2]) -> BTreeMap<SurfaceId, SurfacePresentationOutputTicket> {
        let first = SurfaceId::new(133);
        let second = SurfaceId::new(134);
        let manifest = manifest_for_roster(
            [(first, 0), (second, 0)],
            0,
            PopupPlaneRequirement::default(),
            None,
        );
        let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
        let outputs = [first, second]
            .into_iter()
            .enumerate()
            .map(|(index, surface)| {
                let (_, output) = install(
                    &mut scenes,
                    manifest.surface(surface).expect("surface exists").ticket(),
                    index as u64 + 1,
                );
                (surface, output)
            })
            .collect::<BTreeMap<_, _>>();

        for (index, surface) in order.into_iter().enumerate() {
            scenes
                .accept_observed_authority(observed(outputs[&surface], index as u64 + 1))
                .expect("proof is accepted");
        }
        assert!(scenes.ready_surface(first).is_some());
        assert!(scenes.ready_surface(second).is_some());
        outputs
    }

    let first = SurfaceId::new(133);
    let second = SurfaceId::new(134);
    assert_eq!(present([first, second]), present([second, first]));
}

#[test]
fn inactive_revocation_removes_every_surface_proven_by_the_exact_stream() {
    let first = SurfaceId::new(135);
    let second = SurfaceId::new(136);
    let manifest = manifest_for_roster(
        [(first, 0), (second, 0)],
        0,
        PopupPlaneRequirement::default(),
        None,
    );
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
    let (_, first_output) = install(
        &mut scenes,
        manifest.surface(first).expect("first exists").ticket(),
        1,
    );
    let (_, second_output) = install(
        &mut scenes,
        manifest.surface(second).expect("second exists").ticket(),
        2,
    );
    scenes
        .accept_observed_authority(observed(first_output, 1))
        .expect("first proof is accepted");
    scenes
        .accept_observed_authority(observed(second_output, 2))
        .expect("second proof is accepted");
    let invalidated_stream = raw_interaction_authority(&scenes, first)
        .expect("first surface is interactive")
        .stream();

    assert_eq!(
        scenes.invalidate_interaction_authority_for_streams(&BTreeSet::from([invalidated_stream,])),
        BTreeSet::from([first, second])
    );
    assert!(raw_interaction_authority(&scenes, first).is_none());
    assert!(raw_interaction_authority(&scenes, second).is_none());
    assert!(scenes.ready_surface(first).is_none());
    assert!(scenes.ready_surface(second).is_none());
    assert!(
        scenes
            .surface(first)
            .and_then(SurfaceScene::paint_projection)
            .is_some()
    );
    assert!(
        scenes
            .surface(second)
            .and_then(SurfaceScene::paint_projection)
            .is_some()
    );
}

#[test]
fn inactive_coordinate_invalidation_preserves_sibling_interaction_authority() {
    let first = SurfaceId::new(137);
    let second = SurfaceId::new(138);
    let manifest = manifest_for_roster(
        [(first, 0), (second, 0)],
        0,
        PopupPlaneRequirement::default(),
        None,
    );
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
    for (serial, surface) in [first, second].into_iter().enumerate() {
        let (_, output) = install(
            &mut scenes,
            manifest.surface(surface).expect("surface exists").ticket(),
            serial as u64 + 1,
        );
        scenes
            .accept_observed_authority(observed(output, serial as u64 + 1))
            .expect("proof is accepted");
    }
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_some());

    scenes
        .invalidate_presentation_input(first)
        .expect("surface invalidates");

    assert!(scenes.ready_surface(first).is_none());
    assert!(scenes.ready_surface(second).is_some());
    assert!(raw_interaction_authority(&scenes, second).is_some());
    assert!(
        scenes
            .surface(second)
            .and_then(SurfaceScene::paint_projection)
            .is_some()
    );
}

#[test]
fn popup_close_requires_the_new_inactive_revision_across_the_exact_roster() {
    let first = SurfaceId::new(139);
    let second = SurfaceId::new(140);
    let bar = TabBarSceneId {
        root: RootId::new(141),
        tabs: NodeId::default(),
    };
    let owner = TabStripStateKey::new(first, bar);
    let active = PopupPlaneRequirement::Active {
        revision: PopupRoutingRevision::new_for_test(1),
        session: TabListMenuSessionId::new_for_test(owner, 1),
        owner,
    };
    let active_manifest = manifest_for_roster([(first, 0), (second, 0)], 0, active, Some(owner));
    let mut scenes = SurfaceSceneSet::new(&active_manifest).expect("scene set initializes");
    let mut active_outputs = BTreeMap::new();
    for (serial, surface) in [first, second].into_iter().enumerate() {
        let (_, output) = install_with_popup(
            &mut scenes,
            active_manifest
                .surface(surface)
                .expect("surface exists")
                .ticket(),
            active,
            serial as u64 + 1,
        );
        active_outputs.insert(surface, output);
    }
    scenes
        .accept_observed_authority(observed(active_outputs[&first], 1))
        .expect("first active proof is accepted");
    assert!(scenes.ready_surface(first).is_none());
    assert!(scenes.ready_surface(second).is_none());
    scenes
        .accept_observed_authority(observed(active_outputs[&second], 2))
        .expect("second active proof is accepted");
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_some());

    let inactive = PopupPlaneRequirement::Inactive {
        revision: PopupRoutingRevision::new_for_test(2),
    };
    let inactive_manifest =
        manifest_for_roster([(first, 1), (second, 1)], 1, inactive, Some(owner));
    scenes
        .reconcile_manifest(&inactive_manifest)
        .expect("close revision reconciles");
    assert!(scenes.ready_surface(first).is_none());
    assert!(scenes.ready_surface(second).is_none());
    assert!(
        scenes
            .accept_observed_authority(observed(active_outputs[&first], 3))
            .is_err(),
        "an old active-plane proof cannot authorize the close successor"
    );

    let mut inactive_outputs = BTreeMap::new();
    for (serial, surface) in [first, second].into_iter().enumerate() {
        let (_, output) = install_with_popup(
            &mut scenes,
            inactive_manifest
                .surface(surface)
                .expect("surface exists")
                .ticket(),
            inactive,
            serial as u64 + 3,
        );
        inactive_outputs.insert(surface, output);
    }
    scenes
        .accept_observed_authority(observed(inactive_outputs[&first], 4))
        .expect("first close proof is accepted");
    assert!(scenes.ready_surface(first).is_none());
    assert!(scenes.ready_surface(second).is_none());
    scenes
        .accept_observed_authority(observed(inactive_outputs[&second], 5))
        .expect("second close proof is accepted");
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_some());
}

#[test]
fn inactive_roster_changes_preserve_retained_surface_authority() {
    let first = SurfaceId::new(143);
    let second = SurfaceId::new(144);
    let third = SurfaceId::new(145);
    let initial = manifest_for_roster(
        [(first, 0), (second, 0)],
        0,
        PopupPlaneRequirement::default(),
        None,
    );
    let mut scenes = SurfaceSceneSet::new(&initial).expect("scene set initializes");
    for (serial, surface) in [first, second].into_iter().enumerate() {
        let (_, output) = install(
            &mut scenes,
            initial.surface(surface).expect("surface exists").ticket(),
            serial as u64 + 1,
        );
        scenes
            .accept_observed_authority(observed(output, serial as u64 + 1))
            .expect("initial proof is accepted");
    }
    assert!(scenes.ready_surface(first).is_some());

    let added = manifest_for_roster(
        [(first, 0), (second, 0), (third, 0)],
        1,
        PopupPlaneRequirement::default(),
        None,
    );
    scenes
        .reconcile_manifest(&added)
        .expect("added roster reconciles");
    let (_, third_output) = install(
        &mut scenes,
        added.surface(third).expect("third exists").ticket(),
        3,
    );
    scenes
        .accept_observed_authority(observed(third_output, 3))
        .expect("third proof is accepted");
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_some());
    assert!(scenes.ready_surface(third).is_some());

    let removed = manifest_for_roster(
        [(first, 0), (second, 0)],
        2,
        PopupPlaneRequirement::default(),
        None,
    );
    scenes
        .reconcile_manifest(&removed)
        .expect("removed roster reconciles");
    assert!(scenes.surface(third).is_none());
    assert!(scenes.ready_surface(first).is_some());
    assert!(scenes.ready_surface(second).is_some());
}

#[test]
fn repeated_identical_authority_is_idempotent() {
    let surface = SurfaceId::new(14);
    let manifest = manifest(surface, 0, 0);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
    let (candidate, output) = install(&mut scenes, ticket, 1);

    assert_eq!(acknowledge(&mut scenes, output, 1), Ok(true));
    assert_eq!(acknowledge(&mut scenes, output, 1), Ok(false));
    assert_eq!(
        scenes.ready_surface(surface).map(SurfacePlanScene::stamp),
        Some(candidate)
    );
}

#[test]
fn refreshed_emission_advances_provenance_without_changing_interaction_semantics() {
    let surface = SurfaceId::new(141);
    let manifest = manifest(surface, 0, 0);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
    let (_, output) = install(&mut scenes, ticket, 1);

    assert_eq!(acknowledge(&mut scenes, output, 1), Ok(true));
    let first = scenes
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .and_then(ReadySurfaceScene::interaction_authority)
        .expect("first observation grants interaction authority");

    assert_eq!(acknowledge(&mut scenes, output, 2), Ok(false));
    let refreshed = scenes
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .and_then(ReadySurfaceScene::interaction_authority)
        .expect("refreshed observation retains interaction authority");
    assert_ne!(first, refreshed);
    assert!(first.emission() < refreshed.emission());
    assert!(first.same_interaction_semantics(refreshed));
}

#[test]
fn late_older_emission_cannot_regress_concrete_presentation_provenance() {
    let surface = SurfaceId::new(142);
    let manifest = manifest(surface, 0, 0);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");
    let (_, output) = install(&mut scenes, ticket, 1);

    assert_eq!(acknowledge(&mut scenes, output, 2), Ok(true));
    let current = scenes
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .and_then(ReadySurfaceScene::interaction_authority)
        .expect("newer observation grants interaction authority");
    let submitted = PresentedSurfaceAuthority::mint_observed_for_test_with_emission(
        output,
        CoordinateGeneration::default(),
        1,
    );

    assert_eq!(
        scenes.accept_observed_authority(submitted),
        Err(PresentationAuthorityRejection::EmissionRegressed {
            current: current.emission(),
            submitted: submitted.emission(),
        })
    );
    assert_eq!(
        scenes
            .surface(surface)
            .and_then(SurfaceScene::ready)
            .and_then(ReadySurfaceScene::interaction_authority),
        Some(current)
    );
}

#[test]
fn delayed_fallback_ticket_can_grant_authority_after_a_replacement_candidate() {
    let surface = SurfaceId::new(15);
    let manifest = manifest(surface, 0, 0);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

    let (_painted_stamp, delayed_output) = install(&mut scenes, ticket, 1);
    let (replacement_stamp, replacement_output) = install(&mut scenes, ticket, 2);

    assert_eq!(acknowledge(&mut scenes, delayed_output, 1), Ok(true));
    let ready = scenes
        .surface(surface)
        .and_then(SurfaceScene::ready)
        .expect("replacement scene remains ready");
    assert_eq!(ready.candidate().stamp(), replacement_stamp);
    assert_eq!(ready.candidate().output_ticket(), replacement_output);
    assert_eq!(
        ready.paint_fallback().map(SurfacePlanScene::output_ticket),
        Some(delayed_output)
    );
    assert_eq!(
        ready
            .interaction_authority()
            .map(|authority| authority.ticket()),
        Some(delayed_output)
    );
}

#[test]
fn displaced_unpainted_candidate_cannot_be_observed_after_one_more_replacement() {
    let surface = SurfaceId::new(16);
    let manifest = manifest(surface, 0, 0);
    let ticket = manifest.surface(surface).expect("surface exists").ticket();
    let mut scenes = SurfaceSceneSet::new(&manifest).expect("scene set initializes");

    let (_, delayed_output) = install(&mut scenes, ticket, 1);
    let (_, displaced_output) = install(&mut scenes, ticket, 2);
    let (_, current_output) = install(&mut scenes, ticket, 3);

    assert_eq!(
        acknowledge(&mut scenes, displaced_output, 1),
        Err(PresentationAuthorityRejection::TicketNotRetained {
            candidate: current_output,
            paint_fallback: Some(delayed_output),
        })
    );
}
