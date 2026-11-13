use super::*;

fn shared_placements() -> [NativeSurfacePlacement; 2] {
    [
        NativeSurfacePlacement::new(
            SurfaceId::new(1),
            LogicalSize::new(40.0, 60.0).unwrap(),
            PhysicalPoint::new(0.0, 0.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
            PhysicalRect::new(0.0, 0.0, 40.0, 60.0).unwrap(),
        ),
        NativeSurfacePlacement::new(
            SurfaceId::new(2),
            LogicalSize::new(25.0, 30.0).unwrap(),
            PhysicalPoint::new(45.0, 10.0).unwrap(),
            ScaleFactor::new(2.0).unwrap(),
            PhysicalRect::new(50.0, 15.0, 40.0, 50.0).unwrap(),
        ),
    ]
}

fn shared_registry() -> (ViewportRegistry, ViewportBinding) {
    let mut registry = ViewportRegistry::default();
    let observed = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(10),
            ViewportRole::Root,
        )
        .unwrap();
    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window_with_presentation(
                observed,
                1,
                WindowPresentationState::Visible,
            )],
        ))
        .unwrap();
    registry
        .set_surface_placements(observed.window(), &shared_placements(), |_| false)
        .unwrap();
    (registry, observed)
}

#[test]
fn shared_window_reduces_one_physical_observation_and_projects_independent_mappings() {
    let (mut registry, observed) = shared_registry();
    registry
        .apply_snapshot_for_test(&snapshot(
            2,
            vec![ready_window_with_presentation(
                observed,
                2,
                WindowPresentationState::Visible,
            )],
        ))
        .unwrap();
    let a = registry.record(SurfaceId::new(1)).unwrap();
    let b = registry.record(SurfaceId::new(2)).unwrap();
    assert!(std::ptr::eq(a.window, b.window));
    assert_eq!(registry.windows.len(), 1);
    assert_eq!(a.binding().window(), b.binding().window());
    assert_ne!(
        a.binding().placement_generation(),
        b.binding().placement_generation()
    );
    assert!(a.is_routeable() && b.is_routeable());
    let physical = PhysicalPoint::new(65.0, 30.0).unwrap();
    assert_eq!(
        b.coordinates()
            .unwrap()
            .client_to_surface(physical)
            .unwrap(),
        crate::geometry::LogicalPoint::new(10.0, 10.0).unwrap()
    );
    assert!(
        !a.coordinates()
            .unwrap()
            .surface_client_geometry()
            .clip()
            .contains(physical)
    );
    assert_eq!(
        a.coordinates().unwrap().window_coordinates(),
        b.coordinates().unwrap().window_coordinates()
    );
}

#[test]
fn moving_one_shared_placement_preserves_sibling_and_physical_coordinate_generations() {
    let (mut registry, observed) = shared_registry();
    let a = registry.record(SurfaceId::new(1)).unwrap().binding();
    let old_b = registry.record(SurfaceId::new(2)).unwrap().binding();
    let physical = registry.window_coordinates(observed.window()).unwrap();
    let mut placements = shared_placements();
    placements[1] = NativeSurfacePlacement::new(
        placements[1].surface(),
        placements[1].size(),
        PhysicalPoint::new(48.0, 12.0).unwrap(),
        placements[1].scale_factor(),
        placements[1].clip(),
    );
    registry
        .set_surface_placements(observed.window(), &placements, |_| false)
        .unwrap();
    assert_eq!(registry.record(a.surface()).unwrap().binding(), a);
    assert_ne!(registry.record(old_b.surface()).unwrap().binding(), old_b);
    assert!(registry.placement_record(old_b).is_none());
    assert_eq!(
        registry.window_coordinates(observed.window()),
        Some(physical)
    );
    assert_eq!(
        registry
            .surface_placement(registry.record(old_b.surface()).unwrap().binding())
            .unwrap()
            .size(),
        shared_placements()[1].size()
    );
}

#[test]
fn shared_placement_refusal_preserves_the_complete_roster_and_generation_frontier() {
    let (mut registry, observed) = shared_registry();
    let before = registry.clone();
    let mut invalid = shared_placements();
    invalid[1] = invalid[0];
    assert!(
        registry
            .set_surface_placements(observed.window(), &invalid, |_| false)
            .is_err()
    );
    assert_eq!(registry, before);
    invalid[1] = NativeSurfacePlacement::new(
        SurfaceId::new(2),
        LogicalSize::new(f64::MAX, 2.0).unwrap(),
        PhysicalPoint::new(45.0, 10.0).unwrap(),
        ScaleFactor::new(2.0).unwrap(),
        invalid[0].clip(),
    );
    assert!(
        registry
            .set_surface_placements(observed.window(), &invalid, |_| false)
            .is_err()
    );
    assert_eq!(registry, before);
}

#[test]
fn detaching_one_shared_placement_cannot_retire_its_populated_sibling_window() {
    let (mut registry, observed) = shared_registry();
    let a = registry.record(SurfaceId::new(1)).unwrap().binding();
    let b = registry.record(SurfaceId::new(2)).unwrap().binding();
    assert!(!registry.detach(a).unwrap().window_retired());
    assert!(registry.window(observed.window()).is_some());
    assert!(registry.placement_record(b).unwrap().is_routeable());
    assert!(registry.detach(b).unwrap().window_retired());
    assert!(registry.window(observed.window()).is_none());
    assert!(!registry.token_to_window.contains_key(&observed.token()));
}

#[test]
fn shared_root_continuation_mints_one_window_and_resets_all_old_live_authority() {
    let (mut registry, observed) = shared_registry();
    let bindings: Vec<_> = registry.bindings_for_window(observed.window()).collect();
    let rebound = registry
        .reconcile_restore(
            WorkspaceEpoch::new(1),
            &BTreeMap::from([
                (bindings[0], bindings[1].surface()),
                (bindings[1], bindings[0].surface()),
            ]),
        )
        .unwrap();
    assert!(rebound.retired().is_empty());
    assert_eq!(rebound.retained().len(), 2);
    assert_eq!(registry.windows.len(), 1);
    let after = rebound.retained()[0].1.window();
    assert_ne!(after, observed.window());
    assert_eq!(after, rebound.retained()[1].1.window());
    for (_, record) in registry.records() {
        assert_live_provider_streams_are_revoked(record);
    }
    assert_eq!(registry.token_to_window[&observed.token()], after);
}

#[test]
fn one_shared_close_edge_and_retirement_apply_to_the_complete_window() {
    let (mut registry, observed) = shared_registry();
    let transition = registry
        .apply_close_observation(close_observation(
            observed,
            1,
            WindowCloseState::LiveRequested,
        ))
        .unwrap();
    assert_eq!(
        transition
            .events()
            .iter()
            .filter(|event| matches!(event, RegistryEvent::CloseRequested { .. }))
            .count(),
        1
    );
    let a = registry.record(SurfaceId::new(1)).unwrap().binding();
    registry.mark_awaiting_destroyed(a).unwrap();
    assert!(
        registry
            .placements_in_window(a.window())
            .all(|record| record.admission() == ViewportAdmission::Retiring
                && record.lifecycle() == ViewportLifecycle::AwaitingDestroyed)
    );
}

#[test]
fn window_owners_follow_continuations_and_retirement_without_surface_keys() {
    let mut registry = ViewportRegistry::default();
    let a = SurfaceId::new(1);
    let b = SurfaceId::new(2);
    let first = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            a,
            WindowToken::new(10),
            ViewportRole::Root,
        )
        .unwrap();
    let second = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            b,
            WindowToken::new(20),
            ViewportRole::Root,
        )
        .unwrap();
    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![
                ready_window_with_presentation(first, 1, WindowPresentationState::Visible),
                ready_window_with_presentation(second, 1, WindowPresentationState::Visible),
            ],
        ))
        .unwrap();
    let view = registry.record(a).unwrap();
    let copied_view = view;
    assert!(std::ptr::eq(view.window, copied_view.window));
    assert!(std::ptr::eq(
        view.window,
        &registry.windows[&first.window()]
    ));
    assert!(view.has_coordinate_authority());

    let unchanged = registry.clone();
    assert_eq!(
        registry.register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(3),
            first.token(),
            ViewportRole::Root,
        ),
        Err(ViewportRegistryError::TokenAlreadyRegistered {
            token: first.token()
        })
    );
    assert_eq!(registry, unchanged);

    let continuation = registry
        .reconcile_restore(
            WorkspaceEpoch::new(1),
            &BTreeMap::from([(first, b), (second, a)]),
        )
        .unwrap();
    assert!(continuation.retired().is_empty());
    assert_eq!(registry.windows.len(), 2);
    assert!(!registry.windows.contains_key(&first.window()));
    assert!(!registry.windows.contains_key(&second.window()));
    let successor_a = registry.record(a).unwrap().binding();
    let successor_b = registry.record(b).unwrap().binding();
    for binding in [successor_a, successor_b] {
        assert_eq!(registry.token_to_window[&binding.token()], binding.window());
        assert_live_provider_streams_are_revoked(registry.record(binding.surface()).unwrap());
    }
    assert_eq!(successor_a.token(), second.token());
    assert_eq!(successor_b.token(), first.token());

    let surviving = registry.windows[&successor_b.window()].clone();
    let retired = registry.detach(successor_a).unwrap();
    assert_eq!(retired.binding(), successor_a);
    assert_eq!(registry.windows.len(), 1);
    assert!(!registry.windows.contains_key(&successor_a.window()));
    assert!(!registry.token_to_window.contains_key(&successor_a.token()));
    assert_eq!(registry.windows[&successor_b.window()], surviving);
    registry.discard_unobserved(successor_b).unwrap();
    assert!(registry.records.is_empty());
    assert!(registry.windows.is_empty());
    assert!(registry.token_to_window.is_empty());
}

#[test]
fn external_resource_values_do_not_control_or_alias_managed_window_allocation() {
    let mut registry = ViewportRegistry::default();
    let epoch = WorkspaceEpoch::new(0);
    let external = registry
        .register_existing(
            epoch,
            SurfaceId::new(1),
            WindowToken::new(1),
            ViewportRole::Root,
        )
        .unwrap();
    registry
        .register_owned_child(epoch, SurfaceId::new(2), WindowToken::new(u64::MAX))
        .unwrap();
    let managed = registry
        .reserve(epoch, SurfaceId::new(3), ViewportRole::Child)
        .unwrap();
    assert_ne!(external.token(), managed.token());
    assert_eq!(managed.token(), WindowToken::managed(1));
    let before = registry.clone();
    assert!(matches!(
        registry.register_existing(
            epoch,
            SurfaceId::new(4),
            managed.token(),
            ViewportRole::Root
        ),
        Err(ViewportRegistryError::ManagedTokenCannotRegister { .. })
    ));
    assert!(matches!(
        registry.register_owned_child(epoch, SurfaceId::new(4), managed.token()),
        Err(ViewportRegistryError::ManagedTokenCannotRegister { .. })
    ));
    assert_eq!(registry, before);
    assert!(matches!(
        registry.register_owned_child(epoch, SurfaceId::new(4), WindowToken::new(1)),
        Err(ViewportRegistryError::TokenAlreadyRegistered { .. })
    ));
    assert_eq!(
        registry.record(external.surface()).unwrap().binding(),
        external
    );
    assert_eq!(
        registry.record(managed.surface()).unwrap().binding(),
        managed
    );
}
use crate::geometry::{LogicalRect, PhysicalRect, ScaleFactor};
use crate::intent::AuthorityUnavailableReason;
use crate::platform::{
    CapabilityRosterObservation, CloseEffectAcknowledgement, ObservedWorkArea,
    PlatformCapabilities, PlatformCapability, WindowCloseObservation, WindowCloseState,
    WindowCoordinateObservation, WindowInputState, WindowInventoryObservation,
    WindowPresentationObservation, WindowPresentationState, WorkAreaRosterObservation,
};
use crate::viewport::{
    CapabilityObservationGeneration, CoordinateObservationGeneration,
    InventoryObservationGeneration, WorkAreaGeneration, WorkAreaObservationGeneration,
    WorkAreaToken,
};
use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

fn ready_window(binding: ViewportBinding, generation: u64) -> ObservedWindow {
    ObservedWindow::new(binding)
        .with_coordinate_observation(WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test bounds must be valid"),
            ),
            Authority::Known(
                PhysicalRect::new(-5.0, -25.0, 110.0, 130.0).expect("test bounds must be valid"),
            ),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput))
}

fn close_observation(
    binding: ViewportBinding,
    generation: u64,
    state: WindowCloseState,
) -> WindowCloseObservation {
    WindowCloseObservation::new(
        binding,
        CloseObservationGeneration::new(generation),
        Authority::Known(state),
        CloseEffectAcknowledgement::known(None),
    )
}

fn ready_window_with_presentation(
    binding: ViewportBinding,
    generation: u64,
    state: WindowPresentationState,
) -> ObservedWindow {
    ready_window(binding, generation).with_presentation_observation(presentation_observation(
        binding,
        generation,
        Authority::Known(state),
    ))
}

fn presentation_observation(
    binding: ViewportBinding,
    generation: u64,
    state: Authority<WindowPresentationState>,
) -> WindowPresentationObservation {
    WindowPresentationObservation::new(
        binding,
        crate::viewport::PresentationObservationGeneration::new(generation),
        state,
    )
}

fn snapshot(provider_generation: u64, windows: Vec<ObservedWindow>) -> PlatformSnapshot {
    snapshot_with_close(provider_generation, windows, Vec::new())
}

fn snapshot_with_close(
    provider_generation: u64,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
) -> PlatformSnapshot {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    let inventory = windows.iter().map(ObservedWindow::binding).collect();
    PlatformSnapshot::new(
        crate::viewport::PlatformSnapshotGeneration::new(provider_generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(provider_generation),
            Authority::Known(capabilities),
        ),
        unknown_focus_observation(
            FocusObservationGeneration::new(provider_generation),
            AuthorityUnavailableReason::NotReported,
        ),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(provider_generation),
            Authority::Known(inventory),
        )
        .expect("test inventory must be canonical"),
        windows,
        close_observations,
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(provider_generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .expect("test work-area tombstone must be canonical"),
    )
    .expect("test snapshot must be valid")
}

fn apply_snapshot_with_retired_bindings_for_test(
    registry: &mut ViewportRegistry,
    snapshot: &PlatformSnapshot,
    retired_bindings: &BTreeSet<ViewportBinding>,
    terminal_binding_guards: &BTreeSet<ViewportBinding>,
) -> Result<RegistryTransition, ViewportRegistryError> {
    registry.apply_snapshot_with_retired_bindings(
        snapshot,
        snapshot.inventory_observation().known_roster(),
        retired_bindings,
        terminal_binding_guards,
    )
}

fn assert_live_provider_streams_are_revoked(record: ViewportRecord<'_>) {
    assert!(record.placement.coordinates.is_none());
    assert_eq!(
        record.window.coordinate_observations,
        WindowCoordinateObservationStream::default()
    );
    assert_eq!(
        record.window.input_observations,
        WindowInputObservationStream::default()
    );
    assert_eq!(
        record.window.presentation_observations,
        WindowPresentationObservationStream::default()
    );
    assert_eq!(
        record.window.close_observations,
        WindowCloseObservationStream::default()
    );
    assert!(!record.window.ever_observed);
    assert!(record.window.pending_close_request.is_none());
}

#[test]
fn token_reuse_requires_a_new_incarnation() {
    let mut registry = ViewportRegistry::default();
    let token = WindowToken::new(8);
    let first = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            ViewportRole::Root,
        )
        .expect("first binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(first, 1)]))
        .expect("ready snapshot must apply");
    registry
        .apply_snapshot_for_test(&snapshot(2, Vec::new()))
        .expect("missing snapshot must apply");
    assert_eq!(
        registry.remove_destroyed(first),
        Err(ViewportRegistryError::BindingNotDestroyed { binding: first })
    );
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            3,
            Vec::new(),
            vec![close_observation(first, 1, WindowCloseState::Destroyed)],
        ))
        .expect("typed destruction must apply");
    registry
        .remove_destroyed(first)
        .expect("destroyed binding can be removed");
    let second = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            ViewportRole::Child,
        )
        .expect("reused token must register after destruction");

    assert_ne!(first.incarnation(), second.incarnation());
}

#[test]
fn foreign_authority_domain_observation_is_rejected_atomically() {
    let first_domain = EngineAuthorityDomainId::new_for_test(41);
    let second_domain = EngineAuthorityDomainId::new_for_test(42);
    let mut first = ViewportRegistry::new(first_domain);
    let mut second = ViewportRegistry::new(second_domain);
    let epoch = WorkspaceEpoch::new(3);
    let surface = SurfaceId::new(5);
    let token = WindowToken::new(7);
    let first_binding = first
        .register_existing(epoch, surface, token, ViewportRole::Root)
        .expect("first binding must register");
    let second_binding = second
        .register_existing(epoch, surface, token, ViewportRole::Root)
        .expect("second binding must register");

    assert_eq!(first_binding.epoch(), second_binding.epoch());
    assert_eq!(first_binding.surface(), second_binding.surface());
    assert_eq!(first_binding.token(), second_binding.token());
    assert_eq!(first_binding.incarnation(), second_binding.incarnation());
    assert_ne!(first_binding, second_binding);

    let before = second.clone();
    assert_eq!(
        second.apply_snapshot_for_test(&snapshot(1, vec![ready_window(first_binding, 1)])),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: first_binding,
            current: second_binding,
        })
    );
    assert_eq!(second, before);
}

#[test]
fn delayed_geometry_from_a_reused_token_cannot_mutate_the_new_binding() {
    let mut registry = ViewportRegistry::default();
    let token = WindowToken::new(8);
    let first = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            ViewportRole::Root,
        )
        .expect("first binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(first, 1)]))
        .expect("first binding must become ready");
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            2,
            Vec::new(),
            vec![close_observation(first, 1, WindowCloseState::Destroyed)],
        ))
        .expect("first binding must be destroyed");
    registry
        .remove_destroyed(first)
        .expect("destroyed binding must be removable");
    let second = registry
        .register_existing(
            WorkspaceEpoch::new(1),
            SurfaceId::new(1),
            token,
            ViewportRole::Child,
        )
        .expect("reused token must allocate a new binding");
    let before = registry.clone();

    assert_eq!(
        registry.apply_snapshot_for_test(&snapshot(3, vec![ready_window(first, 3)])),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: first,
            current: second,
        })
    );
    assert_eq!(registry, before);
    assert!(
        registry
            .record(second.surface())
            .expect("new binding must remain registered")
            .coordinates()
            .is_none(),
        "late geometry must not become coordinate authority for the new binding"
    );

    registry
        .apply_snapshot_for_test(&snapshot(4, vec![ready_window(second, 4)]))
        .expect("the exact new binding must remain observable");
}

#[test]
fn terminal_tombstone_is_the_only_old_binding_fact_allowed_after_token_aba() {
    let mut registry = ViewportRegistry::default();
    let token = WindowToken::new(8);
    let first = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            ViewportRole::Root,
        )
        .expect("first binding must register");
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            1,
            Vec::new(),
            vec![close_observation(first, 1, WindowCloseState::Destroyed)],
        ))
        .expect("first binding must be destroyed");
    registry
        .remove_destroyed(first)
        .expect("destroyed binding must be removable");
    let second = registry
        .register_existing(
            WorkspaceEpoch::new(1),
            SurfaceId::new(1),
            token,
            ViewportRole::Child,
        )
        .expect("the exact destroyed token may receive a new incarnation");
    let terminal_tombstones = BTreeSet::from([first]);

    let repeated_tombstone = snapshot_with_close(
        2,
        vec![ready_window(second, 2)],
        vec![close_observation(first, 2, WindowCloseState::Destroyed)],
    );
    apply_snapshot_with_retired_bindings_for_test(
        &mut registry,
        &repeated_tombstone,
        &BTreeSet::new(),
        &terminal_tombstones,
    )
    .expect("a repeated exact terminal tombstone must be inert");
    assert_eq!(
        registry
            .record(second.surface())
            .map(ViewportRecord::binding),
        Some(second)
    );

    let unknown_old_close = WindowCloseObservation::new(
        first,
        CloseObservationGeneration::new(3),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        CloseEffectAcknowledgement::known(None),
    );
    let before_unknown_close = registry.clone();
    assert_eq!(
        apply_snapshot_with_retired_bindings_for_test(
            &mut registry,
            &snapshot_with_close(3, vec![ready_window(second, 3)], vec![unknown_old_close]),
            &BTreeSet::new(),
            &terminal_tombstones,
        ),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: first,
            current: second,
        })
    );
    assert_eq!(registry, before_unknown_close);

    let before_old_window = registry.clone();
    assert_eq!(
        apply_snapshot_with_retired_bindings_for_test(
            &mut registry,
            &snapshot(4, vec![ready_window(first, 4)]),
            &BTreeSet::new(),
            &terminal_tombstones,
        ),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: first,
            current: second,
        })
    );
    assert_eq!(registry, before_old_window);
}

#[test]
fn restore_rebind_resets_old_presentation_stream_authority() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(9);
    let token = WindowToken::new(12);
    let before = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Root)
        .expect("initial binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window_with_presentation(
                before,
                7,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("old presentation must become authoritative");

    let reconciliation = registry
        .reconcile_restore(
            WorkspaceEpoch::new(1),
            &BTreeMap::from([(before, before.surface())]),
        )
        .expect("retained binding must rebind");
    let [(_, after)] = reconciliation.retained() else {
        panic!("exactly one retained binding must be returned");
    };

    // A delayed envelope from the old incarnation is rejected, but it must
    // not force a tombstone for the newly bound incarnation.
    let before_delayed_old = registry.clone();
    assert_eq!(
        registry.apply_snapshot_for_test(&snapshot(
            2,
            vec![ready_window_with_presentation(
                before,
                8,
                WindowPresentationState::Hidden,
            )],
        )),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: before,
            current: *after,
        })
    );
    assert_eq!(registry, before_delayed_old);
    assert_eq!(
        registry
            .record(surface)
            .expect("rebound record must remain")
            .presentation_observation(),
        None
    );

    let first_new = registry
        .apply_snapshot_for_test(&snapshot(
            3,
            vec![ready_window_with_presentation(
                *after,
                1,
                WindowPresentationState::Hidden,
            )],
        ))
        .expect("first observation for the new incarnation must recover authority");
    assert!(first_new.events().iter().any(|event| {
        matches!(event, RegistryEvent::PresentationChanged { binding } if *binding == *after)
    }));
    assert_eq!(
        registry
            .record(surface)
            .expect("rebound record must remain")
            .presentation_observation()
            .and_then(WindowPresentationObservation::known_state),
        Some(WindowPresentationState::Hidden)
    );
}

#[test]
fn close_events_preserve_provider_and_ingress_generations_without_reemitting() {
    let mut registry = ViewportRegistry::default();
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            ViewportRole::Child,
        )
        .expect("binding must register");
    let requested = close_observation(binding, 2, WindowCloseState::LiveRequested);
    let first = registry
        .apply_snapshot_for_test(&snapshot_with_close(
            1,
            vec![ready_window(binding, 1)],
            vec![requested],
        ))
        .expect("close snapshot must apply");
    assert_eq!(
        first.events(),
        &[RegistryEvent::CloseRequested {
            observation: requested.with_inventory_generation(InventoryGeneration::new(1)),
        }]
    );

    let repeated = registry
        .apply_snapshot_for_test(&snapshot_with_close(
            2,
            vec![ready_window(binding, 2)],
            vec![requested],
        ))
        .expect("repeated snapshot must apply");
    assert!(repeated.events().is_empty());

    let stale_clear = close_observation(binding, 1, WindowCloseState::LiveClear);
    let stale = registry
        .apply_snapshot_for_test(&snapshot_with_close(
            3,
            vec![ready_window(binding, 3)],
            vec![stale_clear],
        ))
        .expect("stale observation must be ignored");
    assert!(stale.events().is_empty());

    let clear = close_observation(binding, 3, WindowCloseState::LiveClear);
    let cleared = registry
        .apply_snapshot_for_test(&snapshot_with_close(
            4,
            vec![ready_window(binding, 4)],
            vec![clear],
        ))
        .expect("new clear observation must apply");
    assert!(cleared.events().iter().any(|event| matches!(
        event,
        RegistryEvent::CloseRequestCleared {
            requested: actual_requested,
            observation,
        }
            if *actual_requested
                == requested.with_inventory_generation(InventoryGeneration::new(1))
                && *observation == clear.with_inventory_generation(InventoryGeneration::new(4))
    )));

    let duplicate_clear = registry
        .apply_snapshot_for_test(&snapshot_with_close(
            5,
            vec![ready_window(binding, 5)],
            vec![clear],
        ))
        .expect("duplicate clear must apply without another edge");
    assert!(duplicate_clear.events().is_empty());
}

#[test]
fn narrow_close_observations_preserve_exact_edge_and_clear_order() {
    let mut registry = ViewportRegistry::default();
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
        .expect("baseline snapshot must apply");

    let requested = close_observation(binding, 1, WindowCloseState::LiveRequested);
    let request = registry
        .apply_close_observation(requested)
        .expect("exact close request must apply");
    assert_eq!(request.generation(), InventoryGeneration::new(2));
    assert_eq!(
        request.events(),
        &[RegistryEvent::CloseRequested {
            observation: requested.with_inventory_generation(InventoryGeneration::new(2)),
        }]
    );

    let clear = close_observation(binding, 2, WindowCloseState::LiveClear);
    let cleared = registry
        .apply_close_observation(clear)
        .expect("later exact clear must apply");
    assert_eq!(cleared.generation(), InventoryGeneration::new(3));
    assert_eq!(
        cleared.events(),
        &[RegistryEvent::CloseRequestCleared {
            requested: requested.with_inventory_generation(InventoryGeneration::new(2)),
            observation: clear.with_inventory_generation(InventoryGeneration::new(3)),
        }]
    );
}

#[test]
fn narrow_close_observation_rejects_stale_incarnation_atomically() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let before = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            surface,
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    let reconciliation = registry
        .reconcile_restore(
            WorkspaceEpoch::new(1),
            &BTreeMap::from([(before, before.surface())]),
        )
        .expect("retained binding must rebind");
    let [(_, after)] = reconciliation.retained() else {
        panic!("one retained binding must be rebound");
    };
    let unchanged = registry.clone();
    assert_eq!(
        registry.apply_close_observation(close_observation(
            before,
            1,
            WindowCloseState::LiveRequested,
        )),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: before,
            current: *after,
        })
    );
    assert_eq!(registry, unchanged);
}

#[test]
fn narrow_close_observation_cannot_bypass_retirement_for_destruction() {
    let mut registry = ViewportRegistry::default();
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    let unchanged = registry.clone();
    assert_eq!(
        registry.apply_close_observation(close_observation(
            binding,
            1,
            WindowCloseState::Destroyed,
        )),
        Err(ViewportRegistryError::NarrowDestroyedObservationForbidden { binding })
    );
    assert_eq!(registry, unchanged);
}

#[test]
fn pending_close_edge_requires_exact_provider_and_ingress_generations() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
        .expect("ready snapshot must apply");

    let requested = close_observation(binding, 5, WindowCloseState::LiveRequested);
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            2,
            vec![ready_window(binding, 2)],
            vec![requested],
        ))
        .expect("close request must apply");
    let record = registry
        .record(surface)
        .expect("record must remain present");
    let domain = EngineAuthorityDomainId::new_for_test(7);
    let exact = NativeCloseEdge::from_authoritative_requested(
        domain,
        binding,
        CloseObservationGeneration::new(5),
        InventoryGeneration::new(2),
    );
    let different_provider_generation = NativeCloseEdge::from_authoritative_requested(
        domain,
        binding,
        CloseObservationGeneration::new(6),
        InventoryGeneration::new(2),
    );
    let different_ingress_generation = NativeCloseEdge::from_authoritative_requested(
        domain,
        binding,
        CloseObservationGeneration::new(5),
        InventoryGeneration::new(3),
    );
    assert!(record.matches_pending_close_edge(exact));
    assert!(!record.matches_pending_close_edge(different_provider_generation));
    assert!(!record.matches_pending_close_edge(different_ingress_generation));

    let cleared = close_observation(binding, 6, WindowCloseState::LiveClear);
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            3,
            vec![ready_window(binding, 3)],
            vec![cleared],
        ))
        .expect("close clear must apply");
    let reissued = close_observation(binding, 7, WindowCloseState::LiveRequested);
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            4,
            vec![ready_window(binding, 4)],
            vec![reissued],
        ))
        .expect("reissued close request must apply");
    let record = registry
        .record(surface)
        .expect("record must remain present");
    let current = NativeCloseEdge::from_authoritative_requested(
        domain,
        binding,
        CloseObservationGeneration::new(7),
        InventoryGeneration::new(4),
    );
    assert!(!record.matches_pending_close_edge(exact));
    assert!(record.matches_pending_close_edge(current));
    assert_eq!(
        registry.native_close_edge_disposition(exact),
        NativeCloseEdgeDisposition::DifferentPending {
            observation: reissued.with_inventory_generation(InventoryGeneration::new(4)),
        }
    );

    let tombstone = close_observation(binding, 8, WindowCloseState::Destroyed);
    registry
        .apply_snapshot_for_test(&snapshot_with_close(5, Vec::new(), vec![tombstone]))
        .expect("typed destruction must apply");
    assert_eq!(
        registry.native_close_edge_disposition(exact),
        NativeCloseEdgeDisposition::Destroyed {
            observation: tombstone.with_inventory_generation(InventoryGeneration::new(5)),
        }
    );
}

#[test]
fn authoritative_absence_is_missing_and_never_synthesizes_destruction() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
        .expect("live window must apply");

    let missing = registry
        .apply_snapshot_for_test(&snapshot(2, Vec::new()))
        .expect("authoritative absence must apply");
    assert_eq!(
        missing.events(),
        &[RegistryEvent::BindingMissing { binding }]
    );
    assert_eq!(
        registry
            .record(surface)
            .expect("missing binding remains registered")
            .lifecycle(),
        ViewportLifecycle::Missing
    );
    assert!(
        !missing
            .events()
            .iter()
            .any(|event| matches!(event, RegistryEvent::Destroyed { .. }))
    );

    let repeated = registry
        .apply_snapshot_for_test(&snapshot(3, Vec::new()))
        .expect("repeated absence must apply");
    assert!(repeated.events().is_empty());
}

#[test]
fn typed_tombstone_after_missing_destroys_once() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
        .expect("live window must apply");
    registry
        .apply_snapshot_for_test(&snapshot(2, Vec::new()))
        .expect("missing inventory must apply");

    let tombstone = close_observation(binding, 7, WindowCloseState::Destroyed);
    let destroyed = registry
        .apply_snapshot_for_test(&snapshot_with_close(3, Vec::new(), vec![tombstone]))
        .expect("typed tombstone must apply after absence");
    assert_eq!(
        destroyed.events(),
        &[RegistryEvent::Destroyed {
            observation: tombstone.with_inventory_generation(InventoryGeneration::new(3)),
        }]
    );
    assert_eq!(
        registry
            .record(surface)
            .expect("destroyed binding remains queryable")
            .lifecycle(),
        ViewportLifecycle::Destroyed
    );

    let duplicate = registry
        .apply_snapshot_for_test(&snapshot_with_close(4, Vec::new(), vec![tombstone]))
        .expect("duplicate tombstone must be idempotent");
    assert!(duplicate.events().is_empty());
}

#[test]
fn destroyed_binding_cannot_reappear_and_rejection_is_atomic() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            1,
            Vec::new(),
            vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
        ))
        .expect("typed tombstone must apply");
    let before = registry.clone();

    assert_eq!(
        registry.apply_snapshot_for_test(&snapshot(2, vec![ready_window(binding, 2)])),
        Err(ViewportRegistryError::DestroyedBindingReappeared { binding })
    );
    assert_eq!(registry, before);
}

#[test]
fn inventory_generation_exhaustion_is_atomic() {
    let mut registry = ViewportRegistry::default();
    registry.exhaust_inventory_generation();
    let before = registry.clone();
    assert_eq!(
        registry.apply_snapshot_for_test(&snapshot(1, Vec::new())),
        Err(ViewportRegistryError::InventoryGenerationExhausted)
    );
    assert_eq!(registry, before);
}

#[test]
fn coordinate_generation_exhaustion_is_atomic() {
    let mut registry = ViewportRegistry::default();
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            token,
            ViewportRole::Child,
        )
        .expect("binding must register");
    registry.exhaust_coordinate_generation();
    let before = registry.clone();

    assert_eq!(
        registry.apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)])),
        Err(ViewportRegistryError::CoordinateGenerationExhausted)
    );
    assert_eq!(registry, before);
}

#[test]
fn provider_revocation_clears_live_streams_and_preserves_structural_state() {
    let mut registry = ViewportRegistry::default();
    let live_surface = SurfaceId::new(1);
    let retiring_surface = SurfaceId::new(2);
    let live = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            live_surface,
            WindowToken::new(3),
            ViewportRole::Root,
        )
        .expect("live binding must register");
    let retiring = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            retiring_surface,
            WindowToken::new(4),
            ViewportRole::Child,
        )
        .expect("retiring binding must register");
    let requested = close_observation(live, 1, WindowCloseState::LiveRequested);
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            1,
            vec![
                ready_window_with_presentation(live, 1, WindowPresentationState::Visible),
                ready_window_with_presentation(retiring, 1, WindowPresentationState::Visible),
            ],
            vec![requested],
        ))
        .expect("live provider facts must apply");
    registry
        .mark_awaiting_destroyed(retiring)
        .expect("retirement obligation must be established");

    let before = registry.clone();
    let live_before = before.record(live_surface).expect("live record must exist");
    let retiring_before = before
        .record(retiring_surface)
        .expect("retiring record must exist");
    let inventory_before = registry.inventory_generation();

    let transition = registry
        .revoke_live_provider_authority()
        .expect("provider authority must revoke atomically");
    assert!(transition.generation() > inventory_before);
    assert_eq!(
        transition.events(),
        &[
            RegistryEvent::FactsUnavailable { binding: live },
            RegistryEvent::FactsUnavailable { binding: retiring },
        ]
    );

    let live_after = registry
        .record(live_surface)
        .expect("live record must remain registered");
    assert_eq!(live_after.binding(), live_before.binding());
    assert_eq!(live_after.role(), live_before.role());
    assert_eq!(live_after.ownership(), live_before.ownership());
    assert_eq!(live_after.admission(), live_before.admission());
    assert_eq!(
        live_after.lifecycle(),
        ViewportLifecycle::AwaitingObservation
    );
    assert_eq!(
        live_after.recovery_coordinates(),
        live_before.recovery_coordinates()
    );
    assert!(live_after.coordinate_generation() > live_before.coordinate_generation());
    assert_live_provider_streams_are_revoked(live_after);

    let retiring_after = registry
        .record(retiring_surface)
        .expect("retiring record must remain registered");
    assert_eq!(retiring_after.binding(), retiring_before.binding());
    assert_eq!(retiring_after.ownership(), retiring_before.ownership());
    assert_eq!(retiring_after.admission(), ViewportAdmission::Retiring);
    assert_eq!(
        retiring_after.lifecycle(),
        ViewportLifecycle::AwaitingDestroyed
    );
    assert_eq!(
        retiring_after.recovery_coordinates(),
        retiring_before.recovery_coordinates()
    );
    assert_eq!(
        retiring_after.coordinate_generation(),
        retiring_before.coordinate_generation(),
        "retirement already revoked live coordinate authority"
    );
    assert_live_provider_streams_are_revoked(retiring_after);

    let after_first = registry.clone();
    let repeated = registry
        .revoke_live_provider_authority()
        .expect("repeated provider revocation must be inert");
    assert_eq!(repeated.generation(), transition.generation());
    assert!(repeated.events().is_empty());
    assert_eq!(registry, after_first);
}

#[test]
fn provider_revocation_does_not_roll_back_destroyed_records() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            surface,
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            1,
            Vec::new(),
            vec![close_observation(binding, 1, WindowCloseState::Destroyed)],
        ))
        .expect("terminal destruction must apply");
    let before = registry.clone();

    let transition = registry
        .revoke_live_provider_authority()
        .expect("destroyed records must be inert during provider revocation");

    assert!(transition.events().is_empty());
    assert_eq!(transition.generation(), before.inventory_generation());
    assert_eq!(registry, before);
}

#[test]
fn provider_revocation_resets_only_stale_watermarks_without_advancing_inventory() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            surface,
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    let coordinate_tombstone = WindowCoordinateObservation::new(
        binding,
        CoordinateObservationGeneration::new(1),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    );
    let window = registry
        .windows
        .get_mut(&binding.window())
        .expect("window must remain registered");
    window
        .coordinate_observations
        .observe(binding, Some(coordinate_tombstone));
    assert!(window.coordinate_observations.current().is_none());
    assert_ne!(
        window.coordinate_observations,
        WindowCoordinateObservationStream::default()
    );
    let inventory_before = registry.inventory_generation();
    let coordinate_before = registry
        .record(surface)
        .expect("record must exist")
        .coordinate_generation();

    let transition = registry
        .revoke_live_provider_authority()
        .expect("stale provider watermarks must reset");

    assert!(transition.events().is_empty());
    assert_eq!(transition.generation(), inventory_before);
    let record = registry
        .record(surface)
        .expect("record must remain registered");
    assert_eq!(
        record.window.coordinate_observations,
        WindowCoordinateObservationStream::default()
    );
    assert_eq!(record.coordinate_generation(), coordinate_before);
}

#[test]
fn provider_revocation_coordinate_generation_exhaustion_is_atomic() {
    let mut registry = ViewportRegistry::default();
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window_with_presentation(
                binding,
                1,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("coordinate authority must apply");
    registry.exhaust_coordinate_generation();
    let before = registry.clone();

    assert_eq!(
        registry.revoke_live_provider_authority(),
        Err(ViewportRegistryError::CoordinateGenerationExhausted)
    );
    assert_eq!(registry, before);
}

#[test]
fn provider_revocation_inventory_generation_exhaustion_is_atomic() {
    let mut registry = ViewportRegistry::default();
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ObservedWindow::new(binding)]))
        .expect("an observed live binding must apply");
    registry.exhaust_inventory_generation();
    let before = registry.clone();

    assert_eq!(
        registry.revoke_live_provider_authority(),
        Err(ViewportRegistryError::InventoryGenerationExhausted)
    );
    assert_eq!(registry, before);
}

#[test]
fn coordinate_provider_stream_separates_watermark_from_semantic_authority() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let binding = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            surface,
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");

    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window_with_presentation(
                binding,
                1,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("initial coordinates must apply");
    let initial = registry.record(surface).expect("record must exist");
    let initial_core_generation = initial.coordinate_generation();
    assert_eq!(
        initial.coordinate_observation_generation(),
        Some(CoordinateObservationGeneration::new(1))
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            2,
            vec![ready_window_with_presentation(
                binding,
                2,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("same coordinate facts with a newer provider generation must apply");
    let repeated = registry.record(surface).expect("record must exist");
    assert_eq!(repeated.coordinate_generation(), initial_core_generation);
    assert_eq!(
        repeated.coordinate_observation_generation_watermark(),
        Some(CoordinateObservationGeneration::new(2))
    );
    assert_eq!(
        repeated.coordinate_observation_generation(),
        Some(CoordinateObservationGeneration::new(2))
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            3,
            vec![ObservedWindow::new(binding).with_presentation_observation(
                presentation_observation(
                    binding,
                    3,
                    Authority::Known(WindowPresentationState::Visible),
                ),
            )],
        ))
        .expect("an unversioned coordinate gap must apply fail-closed");
    let gap = registry.record(surface).expect("record must exist");
    assert!(gap.coordinates().is_none());
    assert_eq!(
        gap.recovery_coordinates()
            .map(|coordinates| coordinates.content().observation_generation()),
        Some(CoordinateObservationGeneration::new(2)),
        "a temporary gap must not erase the last trusted recovery anchor"
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            4,
            vec![ready_window_with_presentation(
                binding,
                3,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("known coordinates after a gap must remain quarantined");
    let quarantined = registry.record(surface).expect("record must exist");
    assert!(quarantined.coordinates().is_none());
    assert_eq!(
        quarantined.coordinate_observation_generation_watermark(),
        Some(CoordinateObservationGeneration::new(3))
    );
    assert_eq!(
        quarantined
            .recovery_coordinates()
            .map(|coordinates| coordinates.content().observation_generation()),
        Some(CoordinateObservationGeneration::new(2))
    );

    let tombstone = WindowCoordinateObservation::new(
        binding,
        CoordinateObservationGeneration::new(4),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    );
    registry
        .apply_snapshot_for_test(&snapshot(
            5,
            vec![
                ObservedWindow::new(binding)
                    .with_coordinate_observation(tombstone)
                    .with_presentation_observation(presentation_observation(
                        binding,
                        4,
                        Authority::Known(WindowPresentationState::Visible),
                    )),
            ],
        ))
        .expect("an explicit tombstone must clear quarantine");
    registry
        .apply_snapshot_for_test(&snapshot(
            6,
            vec![ready_window_with_presentation(
                binding,
                5,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("known coordinates after a tombstone must restore authority");
    let restored = registry.record(surface).expect("record must exist");
    assert_eq!(
        restored.coordinate_observation_generation(),
        Some(CoordinateObservationGeneration::new(5))
    );
    let exact_outer_anchor = restored
        .recovery_coordinates()
        .and_then(RecoveryCoordinateSnapshot::outer_bounds)
        .expect("ready coordinates must retain one exact outer anchor");

    let outer_unknown = WindowCoordinateObservation::new(
        binding,
        CoordinateObservationGeneration::new(6),
        Authority::Known(
            PhysicalRect::new(0.0, 0.0, 100.0, 100.0).expect("test bounds must be valid"),
        ),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
        Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
        Authority::Known(ScaleFactor::new(1.0).expect("test scale must be valid")),
    );
    let before_outer_change = restored.coordinate_generation();
    registry
        .apply_snapshot_for_test(&snapshot(
            7,
            vec![
                ObservedWindow::new(binding)
                    .with_coordinate_observation(outer_unknown)
                    .with_presentation_observation(presentation_observation(
                        binding,
                        6,
                        Authority::Known(WindowPresentationState::Visible),
                    )),
            ],
        ))
        .expect("optional outer bounds may become unavailable authoritatively");
    let outer_changed = registry.record(surface).expect("record must exist");
    assert!(outer_changed.has_coordinate_authority());
    assert!(
        outer_changed
            .coordinates()
            .is_some_and(|coordinates| coordinates.outer_bounds().is_none())
    );
    assert!(outer_changed.coordinate_generation() > before_outer_change);
    assert_eq!(
        outer_changed
            .recovery_coordinates()
            .and_then(RecoveryCoordinateSnapshot::outer_bounds),
        Some(exact_outer_anchor),
        "an unavailable outer fact must not erase or replace the last exact anchor"
    );
    assert_eq!(
        outer_changed
            .recovery_coordinates()
            .map(RecoveryCoordinateSnapshot::content),
        outer_changed.coordinates(),
        "recovery retains the newest exact content projection independently"
    );
}

#[test]
fn hidden_and_minimized_presentations_revoke_placement_authority() {
    for presentation in [
        WindowPresentationState::Hidden,
        WindowPresentationState::Minimized,
    ] {
        let mut registry = ViewportRegistry::default();
        let surface = SurfaceId::new(1);
        let binding = registry
            .register_existing(
                WorkspaceEpoch::new(0),
                surface,
                WindowToken::new(3),
                ViewportRole::Root,
            )
            .expect("binding must register");
        registry
            .apply_snapshot_for_test(&snapshot(
                1,
                vec![ready_window_with_presentation(
                    binding,
                    1,
                    WindowPresentationState::Visible,
                )],
            ))
            .expect("visible coordinates must become authoritative");

        let work_area = ObservedWorkArea::new(
            WorkAreaToken::new(1),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid"),
            ScaleFactor::new(1.0).expect("test scale must be valid"),
        );
        let logical_rect =
            LogicalRect::new(10.0, 10.0, 100.0, 80.0).expect("test rect must be valid");
        let proof = registry
            .placement(surface, logical_rect, work_area, WorkAreaGeneration::new(1))
            .expect("visible binding must authorize placement");
        let visible = registry.record(surface).expect("visible record must exist");
        let visible_generation = visible.coordinate_generation();
        let recovery_anchor = visible.recovery_coordinates();

        registry
            .apply_snapshot_for_test(&snapshot(
                2,
                vec![ready_window_with_presentation(binding, 2, presentation)],
            ))
            .expect("non-visible presentation must apply");

        let unavailable = registry
            .record(surface)
            .expect("non-visible record must remain registered");
        assert!(!unavailable.has_coordinate_authority());
        assert!(unavailable.coordinate_generation() > visible_generation);
        assert_eq!(unavailable.recovery_coordinates(), recovery_anchor);
        assert!(!registry.proof_is_current(&proof, WorkAreaGeneration::new(1)));
        assert!(matches!(
            registry.placement(surface, logical_rect, work_area, WorkAreaGeneration::new(1),),
            Err(CoordinateUnavailable::FactUnavailable {
                reason: AuthorityUnavailableReason::SurfaceUnavailable,
                ..
            })
        ));
    }
}

#[test]
fn coordinate_provider_generation_restarts_after_exact_rebind() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let before = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            surface,
            WindowToken::new(3),
            ViewportRole::Child,
        )
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(before, 99)]))
        .expect("high provider generation must apply to the original incarnation");

    let retained = BTreeMap::from([(before, before.surface())]);
    let reconciliation = registry
        .reconcile_restore(WorkspaceEpoch::new(1), &retained)
        .expect("retained binding must rebind");
    let after = reconciliation.retained()[0].1;
    assert_ne!(before, after);
    assert_eq!(
        registry
            .record(surface)
            .expect("rebound record must exist")
            .coordinate_observation_generation_watermark(),
        None
    );

    registry
        .apply_snapshot_for_test(&snapshot(2, vec![ready_window(after, 1)]))
        .expect("a new exact incarnation may restart its provider generation");
    assert_eq!(
        registry
            .record(surface)
            .expect("rebound record must exist")
            .coordinate_observation_generation(),
        Some(CoordinateObservationGeneration::new(1))
    );
}

#[test]
fn only_a_never_observed_reservation_can_be_discarded() {
    let mut registry = ViewportRegistry::default();
    let unobserved = registry
        .reserve(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            ViewportRole::Child,
        )
        .expect("future binding must reserve");
    registry
        .discard_unobserved(unobserved)
        .expect("unobserved reservation must be discardable");
    assert!(registry.record(unobserved.surface()).is_none());

    let observed = registry
        .reserve(
            WorkspaceEpoch::new(0),
            SurfaceId::new(2),
            ViewportRole::Child,
        )
        .expect("observed binding must reserve");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(observed, 1)]))
        .expect("window must become observed");
    assert_eq!(
        registry.discard_unobserved(observed),
        Err(ViewportRegistryError::BindingAlreadyObserved { binding: observed })
    );
}

#[test]
fn restore_continuation_validates_exact_sources_and_unique_unoccupied_targets_atomically() {
    let mut registry = ViewportRegistry::default();
    let first = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(1),
            WindowToken::new(10),
            ViewportRole::Root,
        )
        .expect("first root must register");
    let peer = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            SurfaceId::new(2),
            WindowToken::new(20),
            ViewportRole::Root,
        )
        .expect("peer root must register");
    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window(first, 9), ready_window(peer, 9)],
        ))
        .expect("both roots must become observable");
    let target = SurfaceId::new(3);
    let stale = ViewportBinding::new(
        first.authority_domain(),
        first.epoch(),
        first.surface(),
        first.token(),
        first
            .incarnation()
            .checked_next()
            .expect("test incarnation must advance"),
    );
    let foreign = ViewportBinding::new(
        EngineAuthorityDomainId::new_for_test(999),
        first.epoch(),
        first.surface(),
        first.token(),
        first.incarnation(),
    );
    let unchanged = registry.clone();
    for (retained, expected) in [
        (
            BTreeMap::from([(first, target), (peer, target)]),
            ViewportRegistryError::SurfaceAlreadyRegistered { surface: target },
        ),
        (
            BTreeMap::from([(first, peer.surface())]),
            ViewportRegistryError::SurfaceAlreadyRegistered {
                surface: peer.surface(),
            },
        ),
        (
            BTreeMap::from([(stale, target)]),
            ViewportRegistryError::StaleBinding { binding: stale },
        ),
        (
            BTreeMap::from([(foreign, target)]),
            ViewportRegistryError::StaleBinding { binding: foreign },
        ),
    ] {
        assert_eq!(
            registry.reconcile_restore(WorkspaceEpoch::new(1), &retained),
            Err(expected)
        );
        assert_eq!(registry, unchanged);
    }

    let reconciliation = registry
        .reconcile_restore(
            WorkspaceEpoch::new(1),
            &BTreeMap::from([(first, target), (peer, peer.surface())]),
        )
        .expect("unique unoccupied targets must retain exact windows");
    assert!(reconciliation.retired().is_empty());
    assert!(registry.record(first.surface()).is_none());
    assert_eq!(
        registry.token_to_window.get(&first.token()),
        Some(&registry.record(target).unwrap().binding().window())
    );
    assert_eq!(
        registry.token_to_window.get(&peer.token()),
        Some(&registry.record(peer.surface()).unwrap().binding().window())
    );
    let continued = registry
        .record(target)
        .expect("continued root must move to its target");
    assert_eq!(reconciliation.retained()[0], (first, continued.binding()));
    assert_live_provider_streams_are_revoked(continued);
    assert!(continued.recovery_coordinates().is_none());
    assert_eq!(
        continued.window.create_preparation_observations,
        WindowCreatePreparationObservationStream::default()
    );
}

#[test]
fn restore_reconciliation_incarnation_exhaustion_is_atomic() {
    let mut registry = ViewportRegistry::default();
    let first_surface = SurfaceId::new(1);
    let second_surface = SurfaceId::new(2);
    let first = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            first_surface,
            WindowToken::new(1),
            ViewportRole::Root,
        )
        .expect("first binding must register");
    let second = registry
        .register_existing(
            WorkspaceEpoch::new(0),
            second_surface,
            WindowToken::new(2),
            ViewportRole::Child,
        )
        .expect("second binding must register");
    registry.last_incarnation = WindowIncarnation::new(u64::MAX - 1);
    let retained_bindings =
        BTreeMap::from([(first, SurfaceId::new(3)), (second, second.surface())]);
    let before = registry.clone();

    assert_eq!(
        registry.reconcile_restore(WorkspaceEpoch::new(1), &retained_bindings),
        Err(ViewportRegistryError::WindowIncarnationExhausted)
    );
    assert_eq!(registry, before);
}

#[test]
fn restore_rebind_requires_a_new_exact_binding_close_observation() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let before = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Root)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            1,
            vec![ready_window(before, 1)],
            vec![close_observation(
                before,
                8,
                WindowCloseState::LiveRequested,
            )],
        ))
        .expect("close-request snapshot must apply");

    let reconciliation = registry
        .reconcile_restore(
            WorkspaceEpoch::new(1),
            &BTreeMap::from([(before, before.surface())]),
        )
        .expect("retained binding must reconcile");
    let [(retained_before, retained_after)] = reconciliation.retained() else {
        panic!("exactly one retained binding must be returned");
    };
    assert_eq!(*retained_before, before);
    assert_eq!(retained_after.epoch(), WorkspaceEpoch::new(1));
    assert_eq!(retained_after.surface(), before.surface());
    assert_eq!(retained_after.token(), before.token());
    assert_ne!(retained_after.incarnation(), before.incarnation());
    assert!(reconciliation.retired().is_empty());

    let record = registry
        .record(surface)
        .expect("retained record must remain current");
    assert_eq!(record.binding(), *retained_after);
    assert_eq!(
        record.window.lifecycle,
        ViewportLifecycle::AwaitingObservation
    );
    assert!(record.placement.coordinates.is_none());
    assert!(record.placement.recovery_coordinates.is_none());
    assert!(!record.window.ever_observed);

    let before_delayed_old = registry.clone();
    assert_eq!(
        registry.apply_snapshot_for_test(&snapshot_with_close(
            2,
            vec![ready_window(before, 2)],
            vec![close_observation(
                before,
                9,
                WindowCloseState::LiveRequested,
            )],
        )),
        Err(ViewportRegistryError::StaleObservedBinding {
            observed: before,
            current: *retained_after,
        })
    );
    assert_eq!(registry, before_delayed_old);

    let requested = close_observation(*retained_after, 1, WindowCloseState::LiveRequested);
    let rebound_close = registry
        .apply_snapshot_for_test(&snapshot_with_close(
            3,
            vec![ready_window(*retained_after, 3)],
            vec![requested],
        ))
        .expect("new-incarnation close observation must apply");
    assert!(rebound_close.events().iter().any(|event| matches!(
        event,
        RegistryEvent::CloseRequested { observation }
            if observation.binding() == *retained_after
                && observation.inventory_generation() == InventoryGeneration::new(2)
    )));
    assert_eq!(
        registry
            .record(surface)
            .expect("record must remain current")
            .lifecycle(),
        ViewportLifecycle::CloseRequested
    );
}

#[test]
fn restore_reconciliation_removes_and_returns_complete_retired_facts() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(7);
    let token = WindowToken::new(11);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
        .expect("ready snapshot must apply");
    registry
        .mark_awaiting_destroyed(binding)
        .expect("binding must enter its terminal close state");
    let reconciliation = registry
        .reconcile_restore(WorkspaceEpoch::new(1), &BTreeMap::new())
        .expect("unretained binding must retire");

    assert!(reconciliation.retained().is_empty());
    let [retired] = reconciliation.retired() else {
        panic!("exactly one retired record must be returned");
    };
    assert_eq!(retired.binding(), binding);
    assert_eq!(retired.role(), ViewportRole::Child);
    assert_eq!(retired.lifecycle(), ViewportLifecycle::AwaitingDestroyed);
    assert!(retired.ever_observed());
    assert!(registry.record(surface).is_none());
    assert!(registry.records().next().is_none());
}

#[test]
fn awaiting_destroyed_requires_a_typed_tombstone() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    registry
        .apply_snapshot_for_test(&snapshot(1, vec![ready_window(binding, 1)]))
        .expect("ready snapshot must apply");
    registry
        .mark_awaiting_destroyed(binding)
        .expect("current binding can await destruction");

    let still_observed = registry
        .apply_snapshot_for_test(&snapshot(2, vec![ready_window(binding, 2)]))
        .expect("observed snapshot must apply");
    assert_eq!(
        registry
            .record(surface)
            .expect("record must remain registered")
            .lifecycle(),
        ViewportLifecycle::AwaitingDestroyed
    );
    assert!(still_observed.events().is_empty());

    let facts_unavailable = registry
        .apply_snapshot_for_test(&snapshot(3, vec![ObservedWindow::new(binding)]))
        .expect("incomplete observed snapshot must apply");
    assert_eq!(
        registry
            .record(surface)
            .expect("record must remain registered")
            .lifecycle(),
        ViewportLifecycle::AwaitingDestroyed
    );
    assert!(facts_unavailable.events().is_empty());

    let missing = registry
        .apply_snapshot_for_test(&snapshot(4, Vec::new()))
        .expect("authoritative absence must apply");
    assert_eq!(
        registry
            .record(surface)
            .expect("missing record remains queryable")
            .lifecycle(),
        ViewportLifecycle::Missing
    );
    assert_eq!(
        missing.events(),
        &[RegistryEvent::BindingMissing { binding }]
    );

    let tombstone = close_observation(binding, 4, WindowCloseState::Destroyed);
    let destroyed = registry
        .apply_snapshot_for_test(&snapshot_with_close(5, Vec::new(), vec![tombstone]))
        .expect("typed tombstone must finish destruction");
    assert_eq!(
        destroyed.events(),
        &[RegistryEvent::Destroyed {
            observation: tombstone.with_inventory_generation(InventoryGeneration::new(5)),
        }]
    );
    assert_eq!(
        registry
            .record(surface)
            .expect("destroyed record remains queryable")
            .lifecycle(),
        ViewportLifecycle::Destroyed
    );
}

#[test]
fn focus_authority_requires_visible_presentation_but_survives_coordinate_gaps() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(1);
    let token = WindowToken::new(3);
    let binding = registry
        .register_existing(WorkspaceEpoch::new(0), surface, token, ViewportRole::Child)
        .expect("binding must register");
    assert!(
        !registry
            .record(surface)
            .expect("reservation must exist")
            .can_observe_focus(),
        "an unobserved reservation cannot own provider focus"
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window_with_presentation(
                binding,
                1,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("visible ready snapshot must apply");
    assert!(
        registry
            .record(surface)
            .expect("ready record must exist")
            .can_observe_focus()
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            2,
            vec![ready_window_with_presentation(
                binding,
                2,
                WindowPresentationState::Hidden,
            )],
        ))
        .expect("hidden snapshot must apply");
    assert!(
        !registry
            .record(surface)
            .expect("hidden record must remain registered")
            .can_observe_focus()
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            3,
            vec![ready_window_with_presentation(
                binding,
                3,
                WindowPresentationState::Minimized,
            )],
        ))
        .expect("minimized snapshot must apply");
    assert!(
        !registry
            .record(surface)
            .expect("minimized record must remain registered")
            .can_observe_focus()
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            4,
            vec![
                ready_window(binding, 4).with_presentation_observation(presentation_observation(
                    binding,
                    4,
                    Authority::Unknown(AuthorityUnavailableReason::NotReported),
                )),
            ],
        ))
        .expect("unknown presentation snapshot must apply");
    assert!(
        !registry
            .record(surface)
            .expect("unknown presentation record must remain registered")
            .can_observe_focus()
    );

    registry
        .apply_snapshot_for_test(&snapshot(
            5,
            vec![ObservedWindow::new(binding).with_presentation_observation(
                presentation_observation(
                    binding,
                    5,
                    Authority::Known(WindowPresentationState::Visible),
                ),
            )],
        ))
        .expect("visible coordinate gap must apply");
    let unavailable = registry
        .record(surface)
        .expect("observed record must remain registered");
    assert_eq!(
        unavailable.lifecycle(),
        ViewportLifecycle::AwaitingObservation
    );
    assert!(unavailable.can_observe_focus());
    assert!(unavailable.can_accept_activation());
    assert!(!unavailable.is_routeable());

    registry
        .apply_snapshot_for_test(&snapshot_with_close(
            6,
            vec![ready_window_with_presentation(
                binding,
                6,
                WindowPresentationState::Visible,
            )],
            vec![close_observation(
                binding,
                1,
                WindowCloseState::LiveRequested,
            )],
        ))
        .expect("close-requested snapshot must apply");
    let closing = registry
        .record(surface)
        .expect("close-requested record must remain registered");
    assert!(closing.can_observe_focus());
    assert!(!closing.can_accept_activation());

    registry
        .mark_awaiting_destroyed(binding)
        .expect("binding must enter terminal close state");
    assert!(
        !registry
            .record(surface)
            .expect("closing record must remain queryable")
            .can_observe_focus()
    );

    registry
        .apply_snapshot_for_test(&snapshot(7, Vec::new()))
        .expect("destroyed snapshot must apply");
    assert!(
        !registry
            .record(surface)
            .expect("missing record remains queryable")
            .can_observe_focus()
    );
}

#[test]
fn visible_staging_binding_can_report_focus_without_semantic_admission() {
    let mut registry = ViewportRegistry::default();
    let surface = SurfaceId::new(9);
    let binding = registry
        .reserve(WorkspaceEpoch::new(0), surface, ViewportRole::Child)
        .expect("native staging binding must reserve");
    registry
        .apply_snapshot_for_test(&snapshot(
            1,
            vec![ready_window_with_presentation(
                binding,
                1,
                WindowPresentationState::Visible,
            )],
        ))
        .expect("visible staging snapshot must apply");

    let staging = registry
        .record(surface)
        .expect("staging binding must remain registered");
    assert_eq!(staging.admission(), ViewportAdmission::Pending);
    assert!(staging.can_report_focus_fact());
    assert!(!staging.can_observe_focus());
    assert!(!staging.can_accept_activation());
}

#[test]
fn retired_surface_authority_history_compacts_without_resetting_live_headless_aba() {
    let mut registry = ViewportRegistry::default();
    let epoch = WorkspaceEpoch::new(1);
    let retained_surface = SurfaceId::new(1);

    for index in 1_u64..=10_001 {
        let surface = SurfaceId::new(index);
        let binding = registry
            .register_existing(epoch, surface, WindowToken::new(index), ViewportRole::Root)
            .expect("unique test binding should register");
        registry
            .detach(binding)
            .expect("registered binding should detach");
    }
    let retained_generation = registry.surface_authority_generation(retained_surface);
    assert_ne!(retained_generation, CoordinateGeneration::default());
    assert_eq!(registry.surface_authority_generation_count(), 10_001);

    let removed =
        registry.compact_surface_authority_generations(&BTreeSet::from([retained_surface]));
    assert_eq!(removed, 10_000);
    assert_eq!(registry.surface_authority_generation_count(), 1);
    assert_eq!(
        registry.surface_authority_generation(retained_surface),
        retained_generation,
        "a still-live headless surface must not regain generation zero"
    );

    assert_eq!(
        registry.compact_surface_authority_generations(&BTreeSet::new()),
        1
    );
    assert_eq!(registry.surface_authority_generation_count(), 0);
}
