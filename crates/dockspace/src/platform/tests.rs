use super::*;
use crate::viewport_focus::{FocusObservationGeneration, unknown_focus_observation};

fn unknown_focus(generation: u64) -> FocusObservationEnvelope {
    unknown_focus_observation(
        FocusObservationGeneration::new(generation),
        AuthorityUnavailableReason::NotReported,
    )
}

fn unknown_work_areas(generation: u64) -> WorkAreaRosterObservation {
    WorkAreaRosterObservation::new(
        WorkAreaObservationGeneration::new(generation),
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    )
    .expect("work-area tombstone must be valid")
}

fn snapshot_with_unknown_inventory(
    provider_generation: u64,
    capabilities: PlatformCapabilities,
    windows: Vec<ObservedWindow>,
    close_observations: Vec<WindowCloseObservation>,
    work_area_observation: WorkAreaRosterObservation,
) -> Result<PlatformSnapshot, PlatformSnapshotError> {
    PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(provider_generation),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(provider_generation),
            Authority::Known(capabilities),
        ),
        unknown_focus(provider_generation),
        WindowInventoryObservation::unknown(
            InventoryObservationGeneration::new(provider_generation),
            AuthorityUnavailableReason::NotReported,
        ),
        windows,
        close_observations,
        work_area_observation,
    )
}

fn binding(incarnation: u64) -> ViewportBinding {
    ViewportBinding::new(
        crate::ids::EngineAuthorityDomainId::new_for_test(1),
        crate::ids::WorkspaceEpoch::new(1),
        crate::ids::SurfaceId::new(2),
        WindowToken::new(3),
        crate::viewport::WindowIncarnation::new(incarnation),
    )
}

fn capabilities(
    generation: u64,
    native_window_lifecycle: PlatformCapability,
) -> CapabilityRosterObservation {
    let mut roster = PlatformCapabilities::default();
    roster.set_native_window_lifecycle(native_window_lifecycle);
    CapabilityRosterObservation::new(
        CapabilityObservationGeneration::new(generation),
        Authority::Known(roster),
    )
}

#[test]
fn renderer_preparation_continuity_survives_compatible_facts_but_not_revocation() {
    let binding = binding(1);
    let mut stream = WindowCreatePreparationObservationStream::default();
    let observation = |generation, readiness| {
        WindowCreatePreparationObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            readiness,
            None,
        )
    };
    let first = observation(1, Authority::Known(NativeCreateReadiness::Prepared));
    stream.observe(binding, Some(first));
    stream.observe(
        binding,
        Some(observation(
            2,
            Authority::Known(NativeCreateReadiness::Prepared),
        )),
    );
    assert_eq!(
        stream.prepared_since(),
        Some(CoordinateObservationGeneration::new(1))
    );
    stream.observe(
        binding,
        Some(observation(
            3,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )),
    );
    assert_eq!(stream.prepared_since(), None);
    stream.observe(binding, Some(first));
    assert_eq!(
        stream.prepared_since(),
        None,
        "a late prepared fact cannot revive revoked readiness"
    );
    stream.observe(
        binding,
        Some(observation(
            4,
            Authority::Known(NativeCreateReadiness::Prepared),
        )),
    );
    assert_eq!(
        stream.prepared_since(),
        Some(CoordinateObservationGeneration::new(4))
    );
    stream.observe(
        binding,
        Some(observation(
            5,
            Authority::Known(NativeCreateReadiness::Created),
        )),
    );
    assert_eq!(
        stream.prepared_since(),
        None,
        "creation alone does not retain renderer readiness"
    );
}

#[test]
fn create_preparation_requires_its_exact_binding_and_coordinate_envelope() {
    let current = binding(1);
    let reason = AuthorityUnavailableReason::NotReported;
    let window =
        ObservedWindow::new(current).with_coordinate_observation(WindowCoordinateObservation::new(
            current,
            CoordinateObservationGeneration::new(1),
            Authority::Unknown(reason),
            Authority::Unknown(reason),
            Authority::Unknown(reason),
            Authority::Unknown(reason),
        ));
    for (binding, generation, expected) in [
        (
            binding(2),
            1,
            PlatformSnapshotError::CreatePreparationBindingMismatch { binding: current },
        ),
        (
            current,
            2,
            PlatformSnapshotError::CreatePreparationCoordinateMismatch { binding: current },
        ),
    ] {
        let malformed =
            window
                .clone()
                .with_create_preparation(WindowCreatePreparationObservation::new(
                    binding,
                    CoordinateObservationGeneration::new(generation),
                    Authority::Known(NativeCreateReadiness::Prepared),
                    None,
                ));
        assert_eq!(
            snapshot_with_unknown_inventory(
                1,
                PlatformCapabilities::default(),
                vec![malformed],
                Vec::new(),
                unknown_work_areas(1)
            ),
            Err(expected)
        );
    }
}

#[test]
fn snapshot_payload_capacity_counts_spare_vector_storage() {
    let mut registry = crate::viewport_registry::ViewportRegistry::new(
        crate::ids::EngineAuthorityDomainId::new_for_test(1),
    );
    let binding = registry
        .register_existing(
            crate::ids::WorkspaceEpoch::new(1),
            crate::ids::SurfaceId::new(2),
            WindowToken::new(3),
            crate::viewport::ViewportRole::Root,
        )
        .expect("the registry mints the observed binding");
    let mut inventory = Vec::with_capacity(7);
    inventory.push(binding);
    let mut windows = Vec::with_capacity(11);
    windows.push(ObservedWindow::new(binding));
    let mut closes = Vec::with_capacity(13);
    closes.push(WindowCloseObservation::new(
        binding,
        CloseObservationGeneration::new(1),
        Authority::Known(WindowCloseState::LiveClear),
        CloseEffectAcknowledgement::known(None),
    ));
    let mut work_areas = Vec::with_capacity(17);
    work_areas.push(ObservedWorkArea::new(
        WorkAreaToken::new(1),
        PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("the work-area bounds validate"),
        ScaleFactor::new(1.0).expect("the scale validates"),
    ));
    assert!(inventory.capacity() > inventory.len());
    assert!(windows.capacity() > windows.len());
    assert!(closes.capacity() > closes.len());
    assert!(work_areas.capacity() > work_areas.len());
    let expected = inventory.capacity() * size_of::<ViewportBinding>()
        + windows.capacity() * size_of::<ObservedWindow>()
        + closes.capacity() * size_of::<WindowCloseObservation>()
        + work_areas.capacity() * size_of::<ObservedWorkArea>();
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    let snapshot = PlatformSnapshot::new(
        PlatformSnapshotGeneration::new(1),
        CapabilityRosterObservation::new(
            CapabilityObservationGeneration::new(1),
            Authority::Known(capabilities),
        ),
        unknown_focus(1),
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(1),
            Authority::Known(inventory),
        )
        .expect("the inventory validates"),
        windows,
        closes,
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(1),
            Authority::Known(work_areas),
        )
        .expect("the work-area roster validates"),
    )
    .expect("the complete snapshot validates");

    assert_eq!(snapshot.owned_payload_capacity_bytes(), expected);
}

#[test]
fn unavailable_snapshot_rosters_have_no_owned_payload_capacity() {
    let snapshot = snapshot_with_unknown_inventory(
        1,
        PlatformCapabilities::default(),
        Vec::new(),
        Vec::new(),
        unknown_work_areas(1),
    )
    .expect("the unavailable snapshot validates");

    assert_eq!(snapshot.owned_payload_capacity_bytes(), 0);
}

#[test]
fn capability_observation_stream_requires_causal_tombstones() {
    let supported = PlatformCapability::Supported;
    let unsupported = PlatformCapability::unsupported(
        PlatformRequirement::NativeWindowLifecycle,
        PlatformCapabilityReason::BackendUnsupported,
    );
    let known = |generation, capability| capabilities(generation, capability);
    let tombstone = |generation| {
        CapabilityRosterObservation::unknown(
            CapabilityObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        )
    };
    let mut stream = CapabilityRosterObservationStream::default();

    let initial = known(4, supported);
    stream.observe(Some(initial.clone()));
    assert_eq!(stream.current(), Some(&initial));

    stream.observe(Some(known(3, unsupported)));
    assert_eq!(stream.current(), Some(&initial), "older rosters are inert");
    stream.observe(Some(initial.clone()));
    assert_eq!(stream.current(), Some(&initial), "duplicates are inert");

    stream.observe(Some(known(4, unsupported)));
    assert_eq!(
        stream.current(),
        None,
        "same-generation conflict revokes authority"
    );
    stream.observe(Some(known(5, supported)));
    assert_eq!(
        stream.current(),
        None,
        "known rosters cannot bridge a conflict"
    );
    stream.observe(Some(tombstone(6)));
    assert_eq!(stream.current(), None);

    let restored = known(7, unsupported);
    stream.observe(Some(restored.clone()));
    assert_eq!(stream.current(), Some(&restored));
    assert_eq!(
        stream.generation_watermark(),
        Some(CapabilityObservationGeneration::new(7))
    );

    stream.observe(Some(known(9, supported)));
    assert_eq!(
        stream.current(),
        None,
        "a skipped generation revokes authority"
    );
    stream.observe(Some(known(10, unsupported)));
    assert_eq!(
        stream.current(),
        None,
        "known rosters cannot bridge a generation gap"
    );
    stream.observe(Some(tombstone(11)));
    let post_gap = known(12, supported);
    stream.observe(Some(post_gap.clone()));
    assert_eq!(stream.current(), Some(&post_gap));
}

#[test]
fn inventory_observation_canonicalizes_exact_bindings_and_accepts_empty() {
    let first = binding(1);
    let second = binding(2);
    let observation = WindowInventoryObservation::new(
        InventoryObservationGeneration::new(1),
        Authority::Known(vec![second, first]),
    )
    .expect("distinct exact bindings must form a valid inventory");

    assert_eq!(observation.known_roster(), Some([first, second].as_slice()));

    let empty = WindowInventoryObservation::new(
        InventoryObservationGeneration::new(2),
        Authority::Known(Vec::new()),
    )
    .expect("a known empty inventory is authoritative");
    assert_eq!(empty.known_roster(), Some([].as_slice()));

    assert_eq!(
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(3),
            Authority::Known(vec![first, first]),
        ),
        Err(PlatformSnapshotError::DuplicateWindowBinding { binding: first })
    );
}

#[test]
fn inventory_observation_stream_requires_causal_tombstones() {
    let first = binding(1);
    let second = binding(2);
    let known = |generation, bindings| {
        WindowInventoryObservation::new(
            InventoryObservationGeneration::new(generation),
            Authority::Known(bindings),
        )
        .expect("test inventory must be valid")
    };
    let tombstone = |generation| {
        WindowInventoryObservation::unknown(
            InventoryObservationGeneration::new(generation),
            AuthorityUnavailableReason::NotReported,
        )
    };
    let mut stream = WindowInventoryObservationStream::default();

    stream.observe(Some(tombstone(1)));
    stream.observe(None);
    let first_authoritative = known(2, vec![first]);
    stream.observe(Some(first_authoritative.clone()));
    assert_eq!(
        stream.current(),
        Some(&first_authoritative),
        "missing capability must not quarantine an inventory that never held authority"
    );

    stream.reset_for_provider_replacement();

    let initial = known(11, vec![second, first]);
    stream.observe(Some(initial.clone()));
    assert_eq!(stream.current(), Some(&initial));

    stream.observe(Some(known(10, vec![first])));
    assert_eq!(stream.current(), Some(&initial), "older rosters are inert");
    stream.observe(Some(known(11, vec![first, second])));
    assert_eq!(stream.current(), Some(&initial), "duplicates are inert");

    stream.observe(Some(known(11, vec![first])));
    assert_eq!(
        stream.current(),
        None,
        "same-generation conflict revokes authority"
    );
    stream.observe(Some(known(12, vec![second])));
    assert_eq!(
        stream.current(),
        None,
        "known rosters cannot bridge a conflict"
    );
    stream.observe(Some(tombstone(13)));

    let restored = known(14, Vec::new());
    stream.observe(Some(restored.clone()));
    assert_eq!(stream.current(), Some(&restored));
    assert_eq!(
        stream.generation_watermark(),
        Some(InventoryObservationGeneration::new(14))
    );

    stream.observe(Some(known(16, vec![second])));
    assert_eq!(
        stream.current(),
        None,
        "a skipped generation revokes authority"
    );
    stream.observe(Some(known(17, vec![first])));
    assert_eq!(
        stream.current(),
        None,
        "known rosters cannot bridge a generation gap"
    );
    stream.observe(Some(tombstone(18)));
    let post_gap = known(19, vec![first]);
    stream.observe(Some(post_gap.clone()));
    assert_eq!(stream.current(), Some(&post_gap));
}

#[test]
fn coordinate_content_size_is_derived_or_explicitly_supplied() {
    let content = PhysicalRect::new(-1200.0, 20.0, 800.0, 600.0).unwrap();
    let scale = ScaleFactor::new(2.0).unwrap();
    let positioned = WindowCoordinateObservation::new(
        binding(1),
        CoordinateObservationGeneration::new(1),
        Authority::Known(content),
        unavailable(),
        Authority::Known(scale),
        Authority::Known(scale),
    );
    assert_eq!(positioned.content_size(), &Authority::Known(content.size()));

    let reason = AuthorityUnavailableReason::PermissionDenied;
    let unpositioned = WindowCoordinateObservation::new(
        binding(1),
        CoordinateObservationGeneration::new(2),
        Authority::Unknown(reason),
        unavailable(),
        Authority::Known(scale),
        Authority::Known(scale),
    );
    assert_eq!(unpositioned.content_size(), &Authority::Unknown(reason));
    assert!(!unpositioned.has_authority());
    let local = unpositioned.with_content_size(Authority::Known(content.size()));
    assert!(local.has_authority());
    assert_eq!(local.content_bounds(), &Authority::Unknown(reason));
    assert_ne!(local, unpositioned);
}

#[test]
fn coordinate_stream_accepts_size_only_facts_and_checks_size_conflicts() {
    let current_binding = binding(1);
    let size = PhysicalSize::new(800.0, 600.0).unwrap();
    let scale = ScaleFactor::new(2.0).unwrap();
    let local = |generation| {
        WindowCoordinateObservation::new(
            current_binding,
            CoordinateObservationGeneration::new(generation),
            unavailable(),
            unavailable(),
            Authority::Known(scale),
            Authority::Known(scale),
        )
        .with_content_size(Authority::Known(size))
    };
    let unknown = |generation| {
        WindowCoordinateObservation::new(
            current_binding,
            CoordinateObservationGeneration::new(generation),
            unavailable(),
            unavailable(),
            unavailable(),
            unavailable(),
        )
    };
    let mut stream = WindowCoordinateObservationStream::default();
    stream.observe(current_binding, Some(local(1)));
    assert_eq!(stream.current(), Some(local(1)));
    stream.observe(current_binding, Some(local(1)));
    assert_eq!(stream.current(), Some(local(1)));
    stream.observe(
        current_binding,
        Some(
            local(1).with_content_size(Authority::Known(PhysicalSize::new(900.0, 600.0).unwrap())),
        ),
    );
    assert_eq!(stream.current(), None);

    let partial = unknown(2).with_content_size(Authority::Known(size));
    assert!(!partial.has_authority());
    stream.observe(current_binding, Some(partial));
    stream.observe(current_binding, Some(local(3)));
    assert_eq!(
        stream.current(),
        Some(local(3)),
        "a causally newer incomplete local tuple is a coordinate tombstone"
    );

    assert!(!unknown(4).has_authority());
    stream.observe(current_binding, Some(unknown(4)));
    stream.observe(current_binding, Some(local(5)));
    assert_eq!(stream.current(), Some(local(5)));
}

#[test]
#[allow(clippy::too_many_lines)]
fn coordinate_observation_stream_requires_causal_tombstones() {
    let current_binding = binding(1);
    let other_binding = binding(2);
    let content = PhysicalRect::new(10.0, 20.0, 800.0, 600.0).expect("test bounds must be valid");
    let outer = PhysicalRect::new(2.0, -10.0, 816.0, 638.0).expect("test bounds must be valid");
    let scale = ScaleFactor::new(2.0).expect("test scale must be valid");
    let known = |binding, generation, outer_bounds| {
        WindowCoordinateObservation::new(
            binding,
            CoordinateObservationGeneration::new(generation),
            Authority::Known(content),
            outer_bounds,
            Authority::Known(scale),
            Authority::Known(scale),
        )
    };
    let tombstone = |generation| {
        WindowCoordinateObservation::new(
            current_binding,
            CoordinateObservationGeneration::new(generation),
            unavailable(),
            unavailable(),
            unavailable(),
            unavailable(),
        )
    };
    let mut stream = WindowCoordinateObservationStream::default();

    let initial = known(current_binding, 4, Authority::Known(outer));
    stream.observe(current_binding, Some(initial));
    assert_eq!(stream.current(), Some(initial));

    stream.observe(
        current_binding,
        Some(known(
            current_binding,
            3,
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )),
    );
    assert_eq!(stream.current(), Some(initial), "older facts are inert");
    stream.observe(current_binding, Some(initial));
    assert_eq!(stream.current(), Some(initial), "duplicates are inert");

    let outer_unknown = known(
        current_binding,
        5,
        Authority::Unknown(AuthorityUnavailableReason::NotReported),
    );
    stream.observe(current_binding, Some(outer_unknown));
    assert_eq!(
        stream.current(),
        Some(outer_unknown),
        "outer bounds are optional inside an authoritative tuple"
    );

    stream.observe(
        current_binding,
        Some(known(current_binding, 5, Authority::Known(outer))),
    );
    assert_eq!(
        stream.current(),
        None,
        "same-generation conflict revokes authority"
    );
    stream.observe(
        current_binding,
        Some(known(current_binding, 6, Authority::Known(outer))),
    );
    assert_eq!(
        stream.current(),
        None,
        "known facts cannot bridge a conflict"
    );
    stream.observe(current_binding, Some(tombstone(7)));
    assert_eq!(stream.current(), None);
    let restored = known(current_binding, 8, Authority::Known(outer));
    stream.observe(current_binding, Some(restored));
    assert_eq!(stream.current(), Some(restored));

    stream.observe(
        current_binding,
        Some(known(other_binding, 9, Authority::Known(outer))),
    );
    assert_eq!(stream.current(), None, "wrong binding revokes authority");
    stream.observe(
        current_binding,
        Some(known(current_binding, 10, Authority::Known(outer))),
    );
    assert_eq!(
        stream.current(),
        None,
        "wrong-binding gaps require a tombstone"
    );
    stream.observe(current_binding, Some(tombstone(11)));
    stream.observe(
        current_binding,
        Some(known(current_binding, 12, Authority::Known(outer))),
    );
    assert!(stream.current().is_some());

    stream.observe(
        current_binding,
        Some(known(current_binding, 14, Authority::Known(outer))),
    );
    assert_eq!(
        stream.current(),
        None,
        "a skipped generation revokes authority"
    );
    stream.observe(
        current_binding,
        Some(known(current_binding, 15, Authority::Known(outer))),
    );
    assert_eq!(
        stream.current(),
        None,
        "known facts cannot bridge a generation gap"
    );
    stream.observe(current_binding, Some(tombstone(16)));
    let post_gap = known(current_binding, 17, Authority::Known(outer));
    stream.observe(current_binding, Some(post_gap));
    assert_eq!(stream.current(), Some(post_gap));
}

#[test]
fn work_area_observation_stream_requires_causal_tombstones() {
    let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("test work area must be valid");
    let scale = ScaleFactor::new(1.0).expect("test scale must be valid");
    let area = |token| ObservedWorkArea::new(WorkAreaToken::new(token), bounds, scale);
    let known = |generation, roster| {
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Known(roster),
        )
        .expect("test work-area roster must be valid")
    };
    let tombstone = |generation| {
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
        .expect("work-area tombstone must be valid")
    };
    let mut stream = WorkAreaRosterObservationStream::default();

    stream.observe(Some(tombstone(1)));
    stream.observe(None);
    let first_known = known(2, vec![area(1)]);
    stream.observe(Some(first_known.clone()));
    assert_eq!(
        stream.current(),
        Some(&first_known),
        "an initial unavailable capability does not quarantine the first exact roster"
    );

    stream.reset_for_provider_replacement();

    let initial = known(4, vec![area(1), area(2)]);
    stream.observe(Some(initial.clone()));
    assert_eq!(stream.current(), Some(&initial));

    stream.observe(Some(known(3, vec![area(1)])));
    assert_eq!(stream.current(), Some(&initial), "older rosters are inert");
    stream.observe(Some(initial.clone()));
    assert_eq!(stream.current(), Some(&initial), "duplicates are inert");

    stream.observe(Some(known(4, vec![area(1)])));
    assert_eq!(
        stream.current(),
        None,
        "same-generation conflict revokes authority"
    );
    stream.observe(Some(known(5, vec![area(1)])));
    assert_eq!(
        stream.current(),
        None,
        "known rosters cannot bridge a conflict"
    );
    stream.observe(Some(tombstone(6)));
    assert_eq!(stream.current(), None);

    let restored = known(7, vec![area(1)]);
    stream.observe(Some(restored.clone()));
    assert_eq!(stream.current(), Some(&restored));
    assert_eq!(
        stream.generation_watermark(),
        Some(WorkAreaObservationGeneration::new(7))
    );

    stream.observe(None);
    stream.observe(Some(known(8, vec![area(1), area(2)])));
    assert_eq!(
        stream.current(),
        None,
        "an unversioned gap quarantines known facts"
    );
    stream.observe(Some(tombstone(9)));
    let final_roster = known(10, vec![area(1)]);
    stream.observe(Some(final_roster.clone()));
    assert_eq!(stream.current(), Some(&final_roster));

    stream.observe(Some(known(12, vec![area(1), area(2)])));
    assert_eq!(
        stream.current(),
        None,
        "a skipped generation revokes authority"
    );
    stream.observe(Some(known(13, vec![area(1)])));
    assert_eq!(
        stream.current(),
        None,
        "known rosters cannot bridge a generation gap"
    );
    stream.observe(Some(tombstone(14)));
    let post_gap = known(15, vec![area(1)]);
    stream.observe(Some(post_gap.clone()));
    assert_eq!(stream.current(), Some(&post_gap));
}

#[test]
fn unsupported_is_stronger_than_unknown_for_a_required_operation() {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::unsupported(
        PlatformRequirement::AuthoritativeInventory,
        PlatformCapabilityReason::BackendUnsupported,
    ));

    assert!(matches!(
        capabilities.physical_native_drag(crate::platform::NativeDragSourceInput::PassThrough),
        PlatformCapability::Unsupported(issue)
            if issue.requirement() == PlatformRequirement::AuthoritativeInventory
    ));
}

#[test]
fn placement_and_partial_routes_do_not_qualify_a_physical_drag() {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);

    assert!(capabilities.native_exact_placement_create().is_supported());
    assert!(capabilities.cross_surface_routing().is_supported());
    assert!(matches!(
        capabilities.physical_cross_surface_drag(crate::platform::NativeDragSourceInput::PassThrough),
        PlatformCapability::Unknown(issue)
            if issue.requirement() == PlatformRequirement::AuthoritativeButtonState
    ));
    assert!(matches!(
        capabilities.physical_native_drag(crate::platform::NativeDragSourceInput::PassThrough),
        PlatformCapability::Unknown(issue)
            if issue.requirement() == PlatformRequirement::AuthoritativeButtonState
    ));
}

#[test]
fn physical_cross_surface_drag_does_not_require_window_placement() {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_control(PlatformCapability::Supported);

    assert!(
        capabilities
            .physical_cross_surface_drag(crate::platform::NativeDragSourceInput::PassThrough)
            .is_supported()
    );
    assert!(!capabilities.native_exact_placement_create().is_supported());
    assert!(
        !capabilities
            .physical_native_drag(crate::platform::NativeDragSourceInput::PassThrough)
            .is_supported()
    );
}

#[test]
fn content_drag_requires_observed_routes_but_not_whole_window_input_control() {
    let mut capabilities = PlatformCapabilities::default();
    capabilities.set_native_window_lifecycle(PlatformCapability::Supported);
    capabilities.set_authoritative_inventory(PlatformCapability::Supported);
    capabilities.set_hovered_window(PlatformCapability::Supported);
    capabilities.set_desktop_pointer_position(PlatformCapability::Supported);
    capabilities.set_authoritative_button_state(PlatformCapability::Supported);
    capabilities.set_global_window_placement(PlatformCapability::Supported);
    capabilities.set_work_area(PlatformCapability::Supported);
    capabilities.set_pointer_hit_test_observation(PlatformCapability::Supported);

    for control in [
        PlatformCapability::unknown(
            PlatformRequirement::PointerHitTestControl,
            PlatformCapabilityReason::EnvironmentUnavailable,
        ),
        PlatformCapability::unsupported(
            PlatformRequirement::PointerHitTestControl,
            PlatformCapabilityReason::BackendUnsupported,
        ),
    ] {
        capabilities.set_pointer_hit_test_control(control);
        assert!(
            capabilities
                .physical_cross_surface_drag(NativeDragSourceInput::Preserve)
                .is_supported()
        );
        assert!(
            capabilities
                .physical_native_drag(NativeDragSourceInput::Preserve)
                .is_supported()
        );
        assert_eq!(
            capabilities.physical_cross_surface_drag(NativeDragSourceInput::PassThrough),
            control
        );
        assert_eq!(
            capabilities.physical_native_drag(NativeDragSourceInput::PassThrough),
            control
        );
    }

    capabilities.set_pointer_hit_test_observation(PlatformCapability::unsupported(
        PlatformRequirement::PointerHitTestObservation,
        PlatformCapabilityReason::BackendUnsupported,
    ));
    for source_input in [
        NativeDragSourceInput::Preserve,
        NativeDragSourceInput::PassThrough,
    ] {
        assert!(
            matches!(capabilities.physical_cross_surface_drag(source_input),
            PlatformCapability::Unsupported(issue)
                if issue.requirement() == PlatformRequirement::PointerHitTestObservation)
        );
        assert!(matches!(capabilities.physical_native_drag(source_input),
            PlatformCapability::Unsupported(issue)
                if issue.requirement() == PlatformRequirement::PointerHitTestObservation));
    }
}

#[test]
fn snapshot_rejects_duplicate_window_identities() {
    let window = ObservedWindow::new(binding(7));
    assert!(matches!(
        snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            vec![window.clone(), window],
            Vec::new(),
            unknown_work_areas(1),
        ),
        Err(PlatformSnapshotError::DuplicateWindowBinding { .. })
    ));
}

#[test]
fn sorted_snapshot_validation_rejects_token_reuse_across_different_incarnations() {
    let earlier = binding(1);
    let later = binding(3);
    assert_eq!(earlier.token(), later.token());
    assert_ne!(earlier, later);
    assert!(matches!(
        snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            vec![ObservedWindow::new(later), ObservedWindow::new(earlier)],
            Vec::new(),
            unknown_work_areas(1),
        ),
        Err(PlatformSnapshotError::DuplicateWindowToken { token }) if token == earlier.token()
    ));
}

#[test]
fn snapshot_requires_the_focused_dock_binding_to_match_an_exact_window_fact() {
    let delayed = binding(1);
    let current = binding(2);
    let focus = FocusObservationEnvelope::new(
        FocusObservationGeneration::new(1),
        Authority::Known(crate::viewport_focus::GlobalFocusedWindow::Dock(
            delayed.window(),
        )),
        Authority::Known(None),
    );

    assert_eq!(
        PlatformSnapshot::new(
            PlatformSnapshotGeneration::new(1),
            CapabilityRosterObservation::new(
                CapabilityObservationGeneration::new(1),
                Authority::Known(PlatformCapabilities::default()),
            ),
            focus,
            WindowInventoryObservation::unknown(
                InventoryObservationGeneration::new(1),
                AuthorityUnavailableReason::NotReported,
            ),
            vec![ObservedWindow::new(current)],
            Vec::new(),
            unknown_work_areas(1),
        ),
        Err(PlatformSnapshotError::FocusWindowWithoutObservation {
            binding: delayed.window()
        })
    );
}

#[test]
fn snapshot_rejects_a_same_token_observation_from_a_prior_incarnation() {
    let current = binding(2);
    let delayed = binding(1);
    let window = ObservedWindow::new(current).with_input_observation(WindowInputObservation::new(
        delayed,
        InputObservationGeneration::new(1),
        Authority::Known(WindowInputState::ReceivesInput),
        InputEffectAcknowledgement::known(None),
    ));

    assert_eq!(
        snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            vec![window],
            Vec::new(),
            unknown_work_areas(1),
        ),
        Err(PlatformSnapshotError::InputObservationBindingMismatch {
            window: current,
            observation: delayed,
        })
    );
}

#[test]
fn snapshot_rejects_coordinates_from_a_prior_binding_incarnation() {
    let current = binding(2);
    let delayed = binding(1);
    let window =
        ObservedWindow::new(current).with_coordinate_observation(WindowCoordinateObservation::new(
            delayed,
            CoordinateObservationGeneration::new(7),
            Authority::Known(
                PhysicalRect::new(0.0, 0.0, 640.0, 480.0).expect("test bounds must be valid"),
            ),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor must be valid")),
            Authority::Known(ScaleFactor::new(1.0).expect("test scale factor must be valid")),
        ));

    assert_eq!(
        snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            vec![window],
            Vec::new(),
            unknown_work_areas(1),
        ),
        Err(
            PlatformSnapshotError::CoordinateObservationBindingMismatch {
                window: current,
                observation: delayed,
            }
        )
    );
}

#[test]
fn snapshot_checks_content_size_against_known_bounds() {
    let current = binding(1);
    let content = PhysicalRect::new(-1200.0, 20.0, 800.0, 600.0).unwrap();
    let observation = WindowCoordinateObservation::new(
        current,
        CoordinateObservationGeneration::new(1),
        Authority::Known(content),
        unavailable(),
        Authority::Known(ScaleFactor::new(2.0).unwrap()),
        Authority::Known(ScaleFactor::new(2.5).unwrap()),
    );
    for size in [
        content.size(),
        PhysicalSize::new(900.0, 600.0).unwrap(),
        PhysicalSize::new(800.0, 700.0).unwrap(),
    ] {
        let window = ObservedWindow::new(current)
            .with_coordinate_observation(observation.with_content_size(Authority::Known(size)));
        let result = snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            vec![window],
            Vec::new(),
            unknown_work_areas(1),
        );
        if size == content.size() {
            assert!(result.is_ok());
        } else {
            assert_eq!(
                result,
                Err(PlatformSnapshotError::CoordinateObservationSizeMismatch { binding: current })
            );
        }
    }
}

#[test]
fn snapshot_accepts_content_size_without_desktop_bounds() {
    let current = binding(1);
    let observation = WindowCoordinateObservation::new(
        current,
        CoordinateObservationGeneration::new(1),
        Authority::Unknown(AuthorityUnavailableReason::PermissionDenied),
        unavailable(),
        Authority::Known(ScaleFactor::new(2.0).unwrap()),
        Authority::Known(ScaleFactor::new(2.5).unwrap()),
    )
    .with_content_size(Authority::Known(PhysicalSize::new(800.0, 600.0).unwrap()));
    let window = ObservedWindow::new(current).with_coordinate_observation(observation);
    let snapshot = snapshot_with_unknown_inventory(
        1,
        PlatformCapabilities::default(),
        vec![window],
        Vec::new(),
        unknown_work_areas(1),
    )
    .expect("local content size and scales do not require desktop bounds");

    assert_eq!(
        snapshot.window_observations()[0].coordinate_observation(),
        Some(observation)
    );
}

#[test]
fn snapshot_requires_a_complete_unambiguous_work_area_roster() {
    let bounds =
        PhysicalRect::new(-1920.0, -200.0, 1920.0, 1080.0).expect("test work area must be valid");
    let scale = ScaleFactor::new(1.0).expect("test scale must be valid");
    let work_area = ObservedWorkArea::new(WorkAreaToken::new(3), bounds, scale);
    let mut supported = PlatformCapabilities::default();
    supported.set_work_area(PlatformCapability::Supported);

    assert_eq!(
        snapshot_with_unknown_inventory(
            1,
            supported.clone(),
            Vec::new(),
            Vec::new(),
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(1),
                Authority::Known(Vec::new()),
            )
            .expect("empty roster is structurally unambiguous"),
        ),
        Err(PlatformSnapshotError::MissingWorkAreaRoster)
    );
    assert_eq!(
        snapshot_with_unknown_inventory(
            2,
            PlatformCapabilities::default(),
            Vec::new(),
            Vec::new(),
            WorkAreaRosterObservation::new(
                WorkAreaObservationGeneration::new(2),
                Authority::Known(vec![work_area]),
            )
            .expect("test roster must be structurally valid"),
        ),
        Err(PlatformSnapshotError::WorkAreaRosterWithoutCapability)
    );
    assert!(matches!(
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(3),
            Authority::Known(vec![work_area, work_area]),
        ),
        Err(PlatformSnapshotError::DuplicateWorkArea { .. })
    ));
    let empty = PhysicalRect::new(0.0, 0.0, 0.0, 1080.0)
        .expect("zero-width geometry remains representable outside work-area rosters");
    assert_eq!(
        WorkAreaRosterObservation::new(
            WorkAreaObservationGeneration::new(4),
            Authority::Known(vec![ObservedWorkArea::new(
                WorkAreaToken::new(4),
                empty,
                scale,
            )]),
        ),
        Err(PlatformSnapshotError::EmptyWorkArea {
            token: WorkAreaToken::new(4),
        })
    );
}

#[test]
#[allow(clippy::too_many_lines)]
fn input_observation_stream_rejects_stale_duplicate_and_wrong_binding_facts() {
    let current_binding = binding(1);
    let mut stream = WindowInputObservationStream::default();
    let observation = |binding, generation, state| {
        WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(generation),
            Authority::Known(state),
            InputEffectAcknowledgement::known(None),
        )
    };
    let tombstone = |binding, generation| {
        WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            InputEffectAcknowledgement::unknown(AuthorityUnavailableReason::NotReported),
        )
    };

    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            4,
            WindowInputState::ReceivesInput,
        )),
    );
    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            3,
            WindowInputState::PassThrough,
        )),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowInputObservation::known_state),
        Some(WindowInputState::ReceivesInput)
    );

    stream.observe(current_binding, None);
    assert_eq!(stream.current(), None);
    assert_eq!(
        stream.generation_watermark(),
        Some(InputObservationGeneration::new(4))
    );
    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            4,
            WindowInputState::ReceivesInput,
        )),
    );
    assert_eq!(stream.current(), None);
    stream.observe(
        current_binding,
        Some(observation(binding(2), 5, WindowInputState::PassThrough)),
    );
    assert_eq!(stream.current(), None);
    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            6,
            WindowInputState::PassThrough,
        )),
    );
    assert_eq!(stream.current(), None);
    assert_eq!(
        stream.generation_watermark(),
        Some(InputObservationGeneration::new(6))
    );
    stream.observe(current_binding, Some(tombstone(current_binding, 5)));
    assert_eq!(stream.current(), None);
    assert_eq!(
        stream.generation_watermark(),
        Some(InputObservationGeneration::new(6))
    );
    stream.observe(current_binding, Some(tombstone(current_binding, 7)));
    assert_eq!(stream.current(), None);
    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            8,
            WindowInputState::PassThrough,
        )),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowInputObservation::known_state),
        Some(WindowInputState::PassThrough)
    );

    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            8,
            WindowInputState::ReceivesInput,
        )),
    );
    assert_eq!(stream.current(), None);
    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            9,
            WindowInputState::PassThrough,
        )),
    );
    assert_eq!(stream.current(), None);
    stream.observe(current_binding, Some(tombstone(current_binding, 10)));
    stream.observe(
        current_binding,
        Some(observation(
            current_binding,
            11,
            WindowInputState::ReceivesInput,
        )),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowInputObservation::known_state),
        Some(WindowInputState::ReceivesInput)
    );
}

#[test]
fn snapshot_rejects_conflicting_causal_input_facts() {
    let binding = binding(1);
    let window = ObservedWindow::new(binding)
        .with_input_observation(WindowInputObservation::new(
            binding,
            InputObservationGeneration::new(1),
            Authority::Known(WindowInputState::PassThrough),
            InputEffectAcknowledgement::known(None),
        ))
        .with_input_state(Authority::Known(WindowInputState::ReceivesInput));
    assert_eq!(
        snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            vec![window],
            Vec::new(),
            unknown_work_areas(1),
        ),
        Err(PlatformSnapshotError::InputObservationStateMismatch { binding })
    );
}

#[test]
fn presentation_observation_stream_requires_an_explicit_versioned_tombstone() {
    let current_binding = binding(1);
    let mut stream = WindowPresentationObservationStream::default();
    let known = |generation, state| {
        WindowPresentationObservation::new(
            current_binding,
            PresentationObservationGeneration::new(generation),
            Authority::Known(state),
        )
    };
    let unknown = |generation| {
        WindowPresentationObservation::new(
            current_binding,
            PresentationObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
        )
    };

    stream.observe(
        current_binding,
        Some(known(1, WindowPresentationState::Visible)),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowPresentationObservation::known_state),
        Some(WindowPresentationState::Visible)
    );

    // An unversioned gap revokes authority. A later known state cannot silently
    // bridge that gap; the provider must publish an explicit versioned tombstone.
    stream.observe(current_binding, None);
    stream.observe(
        current_binding,
        Some(known(2, WindowPresentationState::Hidden)),
    );
    assert_eq!(stream.current(), None);
    stream.observe(current_binding, Some(unknown(3)));
    stream.observe(
        current_binding,
        Some(known(4, WindowPresentationState::Visible)),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowPresentationObservation::known_state),
        Some(WindowPresentationState::Visible)
    );

    // A same-generation conflict also requires a tombstone before recovery.
    stream.observe(
        current_binding,
        Some(known(4, WindowPresentationState::Hidden)),
    );
    stream.observe(
        current_binding,
        Some(known(5, WindowPresentationState::Hidden)),
    );
    assert_eq!(stream.current(), None);
    stream.observe(current_binding, Some(unknown(6)));
    stream.observe(
        current_binding,
        Some(known(7, WindowPresentationState::Hidden)),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowPresentationObservation::known_state),
        Some(WindowPresentationState::Hidden)
    );
}

#[test]
fn repeated_presentation_envelope_keeps_its_first_inventory_ingress() {
    let binding = binding(1);
    let mut stream = WindowPresentationObservationStream::default();
    let observation = WindowPresentationObservation::new(
        binding,
        PresentationObservationGeneration::new(4),
        Authority::Known(WindowPresentationState::Visible),
    );

    stream.observe(
        binding,
        Some(observation.with_inventory_generation(InventoryGeneration::new(7))),
    );
    stream.observe(
        binding,
        Some(observation.with_inventory_generation(InventoryGeneration::new(8))),
    );

    let current = stream
        .current()
        .expect("an exact provider duplicate must preserve authority");
    assert_eq!(
        current.known_state(),
        Some(WindowPresentationState::Visible)
    );
    assert_eq!(
        current.inventory_generation(),
        InventoryGeneration::new(7),
        "repeating an old provider envelope cannot make it causally newer"
    );
}

#[test]
fn close_observation_stream_requires_tombstones_and_destroyed_is_terminal() {
    let current_binding = binding(1);
    let mut stream = WindowCloseObservationStream::default();
    let known = |binding, generation, state, effect| {
        WindowCloseObservation::new(
            binding,
            CloseObservationGeneration::new(generation),
            Authority::Known(state),
            CloseEffectAcknowledgement::known(effect),
        )
    };
    let unknown = |generation| {
        WindowCloseObservation::new(
            current_binding,
            CloseObservationGeneration::new(generation),
            Authority::Unknown(AuthorityUnavailableReason::NotReported),
            CloseEffectAcknowledgement::unknown(AuthorityUnavailableReason::NotReported),
        )
    };

    stream.observe(
        current_binding,
        Some(known(
            current_binding,
            4,
            WindowCloseState::LiveRequested,
            None,
        )),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowCloseObservation::known_state),
        Some(WindowCloseState::LiveRequested)
    );

    stream.observe(current_binding, None);
    stream.observe(
        current_binding,
        Some(known(
            current_binding,
            5,
            WindowCloseState::LiveClear,
            Some(EffectId::new(10)),
        )),
    );
    assert_eq!(stream.current(), None);
    stream.observe(current_binding, Some(unknown(6)));
    stream.observe(
        current_binding,
        Some(known(
            current_binding,
            7,
            WindowCloseState::LiveClear,
            Some(EffectId::new(10)),
        )),
    );
    assert_eq!(
        stream
            .current()
            .and_then(WindowCloseObservation::known_state),
        Some(WindowCloseState::LiveClear)
    );

    stream.observe(
        current_binding,
        Some(known(
            current_binding,
            7,
            WindowCloseState::LiveRequested,
            Some(EffectId::new(10)),
        )),
    );
    assert_eq!(stream.current(), None);
    stream.observe(
        current_binding,
        Some(known(
            binding(2),
            8,
            WindowCloseState::LiveClear,
            Some(EffectId::new(10)),
        )),
    );
    assert_eq!(stream.current(), None);

    stream.observe(
        current_binding,
        Some(
            known(
                current_binding,
                9,
                WindowCloseState::Destroyed,
                Some(EffectId::new(11)),
            )
            .with_inventory_generation(InventoryGeneration::new(20)),
        ),
    );
    let destroyed = stream
        .current()
        .expect("an exact destroyed observation terminates the binding stream");
    assert_eq!(destroyed.known_state(), Some(WindowCloseState::Destroyed));
    assert_eq!(
        destroyed.inventory_generation(),
        InventoryGeneration::new(20)
    );
    stream.observe(
        current_binding,
        Some(known(
            current_binding,
            10,
            WindowCloseState::LiveClear,
            Some(EffectId::new(12)),
        )),
    );
    assert_eq!(stream.current(), Some(destroyed));
}

#[test]
fn snapshot_rejects_ambiguous_close_observation_rosters() {
    let binding = binding(1);
    let live = WindowCloseObservation::new(
        binding,
        CloseObservationGeneration::new(1),
        Authority::Known(WindowCloseState::LiveClear),
        CloseEffectAcknowledgement::known(None),
    );
    let destroyed = WindowCloseObservation::new(
        binding,
        CloseObservationGeneration::new(2),
        Authority::Known(WindowCloseState::Destroyed),
        CloseEffectAcknowledgement::known(Some(EffectId::new(1))),
    );

    assert_eq!(
        snapshot_with_unknown_inventory(
            1,
            PlatformCapabilities::default(),
            Vec::new(),
            vec![live],
            unknown_work_areas(1),
        ),
        Err(PlatformSnapshotError::LiveCloseObservationWithoutWindow { binding })
    );
    assert_eq!(
        snapshot_with_unknown_inventory(
            2,
            PlatformCapabilities::default(),
            vec![ObservedWindow::new(binding)],
            vec![destroyed],
            unknown_work_areas(2),
        ),
        Err(PlatformSnapshotError::DestroyedCloseObservationWithWindow { binding })
    );
    assert_eq!(
        snapshot_with_unknown_inventory(
            3,
            PlatformCapabilities::default(),
            vec![ObservedWindow::new(binding)],
            vec![live, live],
            unknown_work_areas(3),
        ),
        Err(PlatformSnapshotError::DuplicateCloseObservation { binding })
    );
}
