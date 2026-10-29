use super::*;

#[test]
fn constructor_prepays_both_empty_backings_and_the_inline_root() {
    let resources = NativeOutputResources::try_new().unwrap();
    assert!(resources.ordinary.is_empty());
    assert!(resources.root_control.is_none());
    assert!(resources.child_controls.is_empty());
    assert_eq!(resources.ordinary.capacity(), ORDINARY_OUTPUT_CAPACITY);
    assert_eq!(
        resources.child_controls.capacity(),
        super::super::terminal_roster::DEFAULT_WINDOW_CAPACITY,
    );
    assert!(resources.can_reserve_ordinary());
    assert_eq!(resources.iter().count(), 0);
    assert_eq!(
        resources.maximum_token_count(),
        NativeOutputResources::default_maximum_token_count(),
    );
    assert_eq!(
        NativeOutputResources::reservation_bytes(),
        Some(size_of::<NativeOutputResources>() + resources.owned_descendant_capacity_bytes()),
    );
    assert_eq!(
        resources.owned_descendant_capacity_bytes(),
        (ORDINARY_OUTPUT_CAPACITY + super::super::terminal_roster::DEFAULT_WINDOW_CAPACITY)
            * size_of::<(NativeOutputToken, NativeOutputResource)>(),
    );
}

#[test]
fn accounting_charges_actual_spare_backing_and_clear_preserves_it() {
    let mut resources = NativeOutputResources::try_new().unwrap();
    let baseline = resources.owned_descendant_capacity_bytes();
    let ordinary_capacity = resources.ordinary.capacity();
    let child_capacity = resources.child_controls.capacity();
    // Exercise metering with extra spare backing independently of constructor policy.
    resources
        .ordinary
        .try_reserve_exact(ordinary_capacity + 1)
        .unwrap();
    resources
        .child_controls
        .try_reserve_exact(child_capacity + 1)
        .unwrap();
    let added_slots = resources.ordinary.capacity() - ordinary_capacity
        + resources.child_controls.capacity()
        - child_capacity;
    assert!(added_slots >= 2);
    let retained = resources.owned_descendant_capacity_bytes();
    assert_eq!(
        retained - baseline,
        added_slots * size_of::<(NativeOutputToken, NativeOutputResource)>(),
    );
    resources.ordinary.clear();
    resources.child_controls.clear();
    assert_eq!(resources.owned_descendant_capacity_bytes(), retained);
}
