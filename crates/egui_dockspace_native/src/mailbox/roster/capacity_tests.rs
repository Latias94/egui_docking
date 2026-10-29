use dockspace::geometry::{PhysicalRect, ScaleFactor};

use super::*;

#[test]
fn constructor_refuses_byte_overflow_and_unaddressable_window_backing() {
    for (windows, areas) in [(usize::MAX, 0), (0, usize::MAX), (usize::MAX, usize::MAX)] {
        assert_eq!(
            NativeViewportRosterRecord::reservation_bytes(windows, areas),
            None
        );
        assert_eq!(
            NativeViewportRosterRecord::try_with_capacity(windows, areas).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable,
        );
    }
    let windows = usize::try_from(isize::MAX).unwrap() / size_of::<FrozenWindowObservation>() + 1;
    assert!(NativeViewportRosterRecord::reservation_bytes(windows, 0).is_some());
    assert_eq!(
        NativeViewportRosterRecord::try_with_capacity(windows, 0).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable,
    );
}

#[test]
fn empty_payload_charges_both_actual_backings_including_zero_capacity_headers() {
    for (windows, areas) in [(0, 0), (1, 2), (7, 3), (256, 64)] {
        let record = NativeViewportRosterRecord::try_with_capacity(windows, areas).unwrap();
        assert_eq!(record.window_capacity(), windows);
        assert_eq!(record.work_area_spare.capacity(), areas);
        assert!(record.observations().is_empty());
        assert!(record.work_area_spare.is_empty());
        assert_eq!(record.work_areas(), Ok(&FrozenWorkAreaRoster::Unknown));
        assert!(!record.work_area_capacity_exceeded());
        assert_eq!(
            NativeViewportRosterRecord::reservation_bytes(windows, areas),
            Some(record.owned_payload_capacity_bytes()),
        );
    }
}

#[test]
fn active_and_empty_spare_work_area_storage_keep_one_prepaid_charge_and_cutoff() {
    let mut record = NativeViewportRosterRecord::try_with_capacity(2, 1).unwrap();
    let bytes = record.owned_payload_capacity_bytes();
    let backing = record.work_area_spare.as_ptr();
    let bounds = PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap();
    let FrozenWorkAreaRoster::Exact(facts) =
        FrozenWorkAreaRoster::for_test_exact([(1, bounds, bounds, ScaleFactor::new(1.0).unwrap())])
    else {
        panic!("the existing fixture supplies one exact work area")
    };
    record.work_area_spare.extend(facts);
    record.work_areas = Ok(FrozenWorkAreaRoster::Exact(std::mem::take(
        &mut record.work_area_spare,
    )));
    assert_eq!(record.work_area_spare.capacity(), 0);
    assert_eq!(record.owned_payload_capacity_bytes(), bytes);
    assert!(record.matches_normalized_work_areas(Some(1), 1, || true));
    assert!(!record.matches_normalized_work_areas(Some(2), 1, || {
        panic!("overflow must reject before comparing facts")
    }));

    let Ok(FrozenWorkAreaRoster::Exact(mut storage)) =
        std::mem::replace(&mut record.work_areas, Ok(FrozenWorkAreaRoster::Unknown))
    else {
        panic!("the exact active backing remains owned")
    };
    storage.clear();
    record.work_area_spare = storage;
    assert!(record.work_area_spare.is_empty());
    assert_eq!(record.work_area_spare.as_ptr(), backing);
    assert_eq!(record.owned_payload_capacity_bytes(), bytes);
    assert!(!record.matches_normalized_work_areas(Some(1), 0, || {
        panic!("a smaller logical cutoff must still reject before comparison")
    }));
}
