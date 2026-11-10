use dockspace::model::DockspaceLayout;
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
};

use super::*;

fn session() -> DockspaceSession {
    let mut session =
        DockspaceSession::from_layout(DockspaceLayout::empty(), DockPolicy::default())
            .expect("empty test session initializes");
    session
        .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
        .expect("managed native host enrolls");
    session
        .configure_managed_native_capabilities(
            NativeHostCapabilities::none_supported()
                .with(NativeHostCapability::AuthoritativeInventory)
                .with(NativeHostCapability::WorkArea),
        )
        .expect("work-area capabilities configure");
    session
}

fn apply(session: &mut DockspaceSession, roster: NativeWorkAreaRoster) {
    session
        .report_managed_native_snapshot(
            std::iter::empty::<(
                dockspace::runtime::NativeWindowBinding,
                dockspace::runtime::NativeWindowFacts,
            )>(),
            roster,
        )
        .expect("the complete work-area snapshot records");
    session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .expect("the work-area frame begins")
        .commit()
        .expect("the empty-layout frame commits");
}

fn roster(first: u32, slots: usize) -> FrozenWorkAreaRoster {
    let scale = ScaleFactor::new(1.0).expect("scale validates");
    FrozenWorkAreaRoster::for_test_exact((0..slots).map(|index| {
        let offset = u32::try_from(index).expect("the fixture has few displays");
        let x = f64::from(offset) * 1920.0;
        (
            u64::from(first + offset),
            PhysicalRect::new(x, 0.0, 1920.0, 1080.0).expect("display bounds validate"),
            PhysicalRect::new(x, 0.0, 1920.0, 1040.0).expect("work-area bounds validate"),
            scale,
        )
    }))
}

fn core_capacity_bytes(roster: &NativeWorkAreaRoster) -> usize {
    size_of::<NativeWorkAreaRoster>()
        + match roster {
            NativeWorkAreaRoster::Exact(facts) => {
                facts.capacity() * size_of::<NativeWorkAreaFacts>()
            }
            NativeWorkAreaRoster::Unknown => 0,
        }
}

#[test]
fn construction_reservation_covers_old_and_new_rosters_through_commit() {
    for slots in [1, 2, 3, 7, 64] {
        for finite in [false, true] {
            let mut state = if finite {
                NativeWorkAreaState::with_capacity_limit(slots)
                    .expect("finite work-area storage reserves")
            } else {
                NativeWorkAreaState::default()
            };
            assert!(state.can_configure_capacity_limit());
            assert_eq!(state.slot_limit, finite.then_some(slots));
            assert_eq!(state.tokens.capacity(), if finite { slots } else { 0 });
            assert_eq!(state.routes.capacity(), if finite { slots } else { 0 });
            let budget = NativeWorkAreaState::construction_reservation_bytes(slots)
                .expect("the construction peak fits usize");
            let mut session = session();

            for first in [0, 1] {
                let old_token_pointer = state.tokens.as_ptr();
                let old_route_pointer = state.routes.as_ptr();
                let old_bytes = state.owned_data_capacity_bytes();
                let old_next_token = state.next_token;
                let (core, prepared) = state
                    .prepare(&roster(first, slots))
                    .expect("a complete exact roster fits its planned slots");
                assert_eq!(state.tokens.as_ptr(), old_token_pointer);
                assert_eq!(state.routes.as_ptr(), old_route_pointer);
                assert_eq!(state.owned_data_capacity_bytes(), old_bytes);
                assert_eq!(state.next_token, old_next_token);

                let PreparedRoutes::Exact(routes) = &prepared.routes else {
                    panic!("an exact roster prepares exact routes");
                };
                assert_eq!(prepared.tokens.len(), slots);
                assert_eq!(prepared.tokens.capacity(), slots);
                assert_eq!(routes.len(), slots);
                assert_eq!(routes.capacity(), slots);
                assert!(prepared.committed_routes.is_empty());
                assert_eq!(prepared.committed_routes.capacity(), slots);
                let NativeWorkAreaRoster::Exact(facts) = &core else {
                    panic!("an exact preparation produces core facts");
                };
                assert_eq!(facts.len(), slots);
                assert_eq!(facts.capacity(), slots);
                for (display, token) in &prepared.tokens {
                    if let Some((_, previous)) =
                        state.tokens.iter().find(|(known, _)| known == display)
                    {
                        assert_eq!(token, previous);
                    }
                }
                if first == 1 {
                    assert_eq!(
                        prepared.tokens.last().expect("the new display exists").1,
                        HostWorkAreaToken::new(old_next_token)
                    );
                    assert_eq!(prepared.next_token, old_next_token + 1);
                }

                let peak =
                    old_bytes + prepared.owned_data_capacity_bytes() + core_capacity_bytes(&core);
                assert!(peak <= budget, "slots={slots}, finite={finite}");
                if finite || first == 1 {
                    assert_eq!(peak, budget, "all planned old and new backing overlaps");
                }

                let token_pointer = prepared.tokens.as_ptr();
                let route_pointer = prepared.committed_routes.as_ptr();
                let token_capacity = prepared.tokens.capacity();
                let route_capacity = prepared.committed_routes.capacity();
                apply(&mut session, core);
                state.commit(prepared, &session);
                assert_eq!(state.tokens.as_ptr(), token_pointer);
                assert_eq!(state.routes.as_ptr(), route_pointer);
                assert_eq!(state.tokens.capacity(), token_capacity);
                assert_eq!(state.routes.capacity(), route_capacity);
                assert_eq!(state.routes.len(), slots);
                assert!(!state.can_configure_capacity_limit());
                for (index, (_, token)) in state.tokens.iter().enumerate() {
                    let x = f64::from(u32::try_from(index).unwrap()) * 1920.0 + 960.0;
                    let point = PhysicalPoint::new(x, 540.0).expect("display center validates");
                    assert_eq!(state.binding_at(point), session.native_work_area(*token));
                    assert!(state.binding_at(point).is_some());
                }
            }
        }
    }
}

#[test]
fn finite_rejection_preserves_token_history_routes_and_backing() {
    for slots in [0, 1, 3, 64] {
        let mut state = NativeWorkAreaState::with_capacity_limit(slots)
            .expect("finite work-area storage reserves");
        let mut session = session();
        if slots > 0 {
            let (core, prepared) = state.prepare(&roster(0, slots)).unwrap();
            apply(&mut session, core);
            state.commit(prepared, &session);
        }
        let tokens = state.tokens.clone();
        let routes: Vec<_> = state
            .routes
            .iter()
            .map(|route| (route.display_bounds, route.binding))
            .collect();
        let token_pointer = state.tokens.as_ptr();
        let route_pointer = state.routes.as_ptr();
        let bytes = state.owned_data_capacity_bytes();
        let next_token = state.next_token;
        let can_configure = state.can_configure_capacity_limit();

        assert_eq!(
            state.prepare(&roster(1, slots + 1)).unwrap_err(),
            NativeHostProtocolError::NativeReportStorageUnavailable
        );
        assert_eq!(state.tokens, tokens);
        assert_eq!(state.tokens.as_ptr(), token_pointer);
        assert_eq!(state.routes.as_ptr(), route_pointer);
        assert_eq!(state.owned_data_capacity_bytes(), bytes);
        assert_eq!(state.next_token, next_token);
        assert_eq!(state.can_configure_capacity_limit(), can_configure);
        assert_eq!(state.slot_limit, Some(slots));
        assert_eq!(
            state
                .routes
                .iter()
                .map(|route| (route.display_bounds, route.binding))
                .collect::<Vec<_>>(),
            routes
        );
    }
}

#[test]
fn unknown_revokes_routes_and_reuses_retained_tokens_on_reappearance() {
    for limit in [None, Some(1), Some(64)] {
        let mut state = limit.map_or_else(NativeWorkAreaState::default, |slots| {
            NativeWorkAreaState::with_capacity_limit(slots).unwrap()
        });
        let mut session = session();
        let (core, prepared) = state.prepare(&roster(7, 1)).unwrap();
        let token = prepared.tokens[0].1;
        apply(&mut session, core);
        state.commit(prepared, &session);
        let token_pointer = state.tokens.as_ptr();
        let next_token = state.next_token;

        let (core, prepared) = state.prepare(&FrozenWorkAreaRoster::Unknown).unwrap();
        assert_eq!(prepared.tokens.capacity(), 0);
        assert_eq!(prepared.committed_routes.capacity(), 0);
        assert_eq!(
            prepared.owned_data_capacity_bytes(),
            size_of::<PreparedWorkAreaRoster>()
        );
        apply(&mut session, core);
        state.commit(prepared, &session);
        assert!(state.routes.is_empty());
        assert_eq!(state.routes.capacity(), 0);
        assert_eq!(state.tokens, vec![(DisplayIdentity::Test(7), token)]);
        assert_eq!(state.tokens.as_ptr(), token_pointer);
        assert_eq!(state.next_token, next_token);
        assert!(!state.can_configure_capacity_limit());
        assert_eq!(session.native_work_area(token), None);

        let (core, prepared) = state.prepare(&roster(7, 1)).unwrap();
        assert_eq!(prepared.tokens, state.tokens);
        assert_eq!(prepared.next_token, next_token);
        let route_pointer = prepared.committed_routes.as_ptr();
        apply(&mut session, core);
        state.commit(prepared, &session);
        assert_eq!(state.routes.as_ptr(), route_pointer);
        let point = PhysicalPoint::new(960.0, 540.0).unwrap();
        assert_eq!(state.binding_at(point), session.native_work_area(token));
        assert!(state.binding_at(point).is_some());
        assert_eq!(state.slot_limit, limit);
    }
}

#[test]
fn exact_commit_replaces_history_and_discarded_preparation_keeps_it() {
    let mut state = NativeWorkAreaState::with_capacity_limit(1).unwrap();
    let mut session = session();
    let (core, prepared) = state.prepare(&roster(7, 1)).unwrap();
    let token = prepared.tokens[0].1;
    apply(&mut session, core);
    state.commit(prepared, &session);
    let next_token = state.next_token;

    let (_, discarded) = state.prepare(&roster(8, 1)).unwrap();
    assert_eq!(discarded.tokens[0].1, HostWorkAreaToken::new(next_token));
    drop(discarded);
    assert_eq!(state.tokens, vec![(DisplayIdentity::Test(7), token)]);
    assert_eq!(state.next_token, next_token);

    let (core, prepared) = state.prepare(&roster(8, 1)).unwrap();
    apply(&mut session, core);
    state.commit(prepared, &session);
    assert_eq!(
        state.tokens,
        vec![(DisplayIdentity::Test(8), HostWorkAreaToken::new(next_token))]
    );
    let (_, prepared) = state.prepare(&roster(7, 1)).unwrap();
    assert_eq!(prepared.tokens[0].1, HostWorkAreaToken::new(next_token + 1));
    assert_ne!(prepared.tokens[0].1, token);
}

#[test]
fn prepared_capacity_counts_all_retained_vector_backing() {
    let state = NativeWorkAreaState::default();
    let (_, mut prepared) = state.prepare(&roster(0, 3)).unwrap();
    let PreparedRoutes::Exact(routes) = &mut prepared.routes else {
        panic!("an exact roster prepares routes");
    };
    prepared.tokens.clear();
    routes.clear();
    let route_capacity = routes.capacity();
    prepared.committed_routes.reserve_exact(5);
    assert_eq!(
        prepared.owned_data_capacity_bytes(),
        size_of::<PreparedWorkAreaRoster>()
            + prepared.tokens.capacity() * size_of::<(DisplayIdentity, HostWorkAreaToken)>()
            + route_capacity * size_of::<PreparedRoute>()
            + prepared.committed_routes.capacity() * size_of::<CommittedRoute>()
    );
    assert!(prepared.owned_data_capacity_bytes() > size_of::<PreparedWorkAreaRoster>());
}

#[test]
fn exact_reservation_rejects_backing_larger_than_planned_slots() {
    let mut routes = Vec::<CommittedRoute>::new();
    reserve_exact_slots(&mut routes, 4).expect("the exact planned backing reserves");
    assert_eq!(routes.capacity(), 4);
    let pointer = routes.as_ptr();
    assert_eq!(
        reserve_exact_slots(&mut routes, 3),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable)
    );
    assert_eq!(routes.as_ptr(), pointer);
    assert_eq!(routes.capacity(), 4);

    let mut facts = Vec::<NativeWorkAreaFacts>::new();
    assert_eq!(
        reserve_exact_slots(&mut facts, usize::MAX),
        Err(NativeHostProtocolError::NativeReportStorageUnavailable)
    );
    assert_eq!(facts.capacity(), 0);
}

#[test]
fn zero_slot_initialization_and_peak_overflow_are_explicit() {
    let mut state = NativeWorkAreaState::with_capacity_limit(0).unwrap();
    assert!(state.can_configure_capacity_limit());
    assert_eq!(state.tokens.capacity(), 0);
    assert_eq!(state.routes.capacity(), 0);
    assert_eq!(
        NativeWorkAreaState::construction_reservation_bytes(0),
        Some(
            size_of::<NativeWorkAreaState>()
                + size_of::<PreparedWorkAreaRoster>()
                + size_of::<NativeWorkAreaRoster>()
        )
    );
    assert_eq!(
        NativeWorkAreaState::construction_reservation_bytes(usize::MAX),
        None
    );
    assert_eq!(
        NativeWorkAreaState::with_capacity_limit(usize::MAX).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable
    );
    let (_, prepared) = state.prepare(&FrozenWorkAreaRoster::Unknown).unwrap();
    state.commit(prepared, &session());
    assert!(state.can_configure_capacity_limit());
    state.next_token = 2;
    assert!(!state.can_configure_capacity_limit());
}
