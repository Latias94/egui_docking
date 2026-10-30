//! Atomic work-area routing sidecar for the native coordinator.

use std::mem::size_of;

use dockspace::geometry::{PhysicalPoint, PhysicalRect, ScaleFactor};
use dockspace::runtime::{
    DockspaceSession, HostWorkAreaToken, NativeWorkAreaBinding, NativeWorkAreaFacts,
    NativeWorkAreaRoster,
};
use eframe::{
    NativeDisplayId, NativePhysicalRect, NativeWorkAreaRecord,
    NativeWorkAreaRoster as EframeWorkAreaRoster,
};

use crate::error::NativeHostProtocolError;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum DisplayIdentity {
    Native(NativeDisplayId),
    #[cfg(test)]
    Test(u64),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FrozenWorkArea {
    display: DisplayIdentity,
    display_bounds: PhysicalRect,
    work_area_bounds: PhysicalRect,
    scale_factor: ScaleFactor,
}

/// Complete work-area facts frozen with one root viewport roster.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum FrozenWorkAreaRoster {
    Exact(Vec<FrozenWorkArea>),
    Unknown,
}

impl FrozenWorkAreaRoster {
    /// Reuses caller-owned storage, including when facts become unavailable.
    /// The spare retains its allocation on Unknown, invalid or oversized input.
    pub(crate) fn capture_reusing(
        roster: EframeWorkAreaRoster<'_>,
        spare: &mut Vec<FrozenWorkArea>,
    ) -> (Result<Self, NativeHostProtocolError>, bool) {
        match roster {
            EframeWorkAreaRoster::Unknown => {
                spare.clear();
                (Ok(Self::Unknown), false)
            }
            EframeWorkAreaRoster::Exact(records) => {
                fill_exact_reusing(records.iter().copied().map(compile_work_area), spare)
            }
        }
    }

    /// Compares borrowed backend facts without allocating a normalized roster.
    pub(crate) fn matches_borrowed(&self, roster: EframeWorkAreaRoster<'_>) -> bool {
        match (self, roster) {
            (Self::Unknown, EframeWorkAreaRoster::Unknown) => true,
            (Self::Exact(expected), EframeWorkAreaRoster::Exact(records)) => {
                expected.len() == records.len()
                    && Self::validate_borrowed(roster).is_ok()
                    && records.iter().copied().all(|record| {
                        compile_work_area(record)
                            .is_ok_and(|record| expected.iter().any(|expected| *expected == record))
                    })
            }
            _ => false,
        }
    }

    pub(crate) fn validate_borrowed(
        roster: EframeWorkAreaRoster<'_>,
    ) -> Result<(), NativeHostProtocolError> {
        let EframeWorkAreaRoster::Exact(records) = roster else {
            return Ok(());
        };
        if records.is_empty() {
            return Err(NativeHostProtocolError::InvalidWorkAreaRoster);
        }
        for (index, record) in records.iter().copied().enumerate() {
            compile_work_area(record)?;
            if records[..index]
                .iter()
                .any(|previous| previous.display_id() == record.display_id())
            {
                return Err(NativeHostProtocolError::InvalidWorkAreaRoster);
            }
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn for_test_exact(
        records: impl IntoIterator<Item = (u64, PhysicalRect, PhysicalRect, ScaleFactor)>,
    ) -> Self {
        Self::Exact(
            records
                .into_iter()
                .map(
                    |(display, display_bounds, work_area_bounds, scale_factor)| FrozenWorkArea {
                        display: DisplayIdentity::Test(display),
                        display_bounds,
                        work_area_bounds,
                        scale_factor,
                    },
                )
                .collect(),
        )
    }
}

fn fill_exact_reusing(
    records: impl ExactSizeIterator<Item = Result<FrozenWorkArea, NativeHostProtocolError>>,
    spare: &mut Vec<FrozenWorkArea>,
) -> (Result<FrozenWorkAreaRoster, NativeHostProtocolError>, bool) {
    spare.clear();
    if records.len() > spare.capacity() {
        return (Ok(FrozenWorkAreaRoster::Unknown), true);
    }
    for record in records {
        match record {
            Ok(record) => spare.push(record),
            Err(error) => {
                spare.clear();
                return (Err(error), false);
            }
        }
    }
    if let Err(error) = validate_exact_records(spare) {
        spare.clear();
        return (Err(error), false);
    }
    (
        Ok(FrozenWorkAreaRoster::Exact(std::mem::take(spare))),
        false,
    )
}

#[derive(Debug)]
pub(crate) struct PreparedWorkAreaRoster {
    next_token: u64,
    tokens: Vec<(DisplayIdentity, HostWorkAreaToken)>,
    routes: PreparedRoutes,
    committed_routes: Vec<CommittedRoute>,
}

impl PreparedWorkAreaRoster {
    /// Counts inline storage and actual capacity of every owned vector.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        let route_capacity = match &self.routes {
            PreparedRoutes::Exact(routes) => routes.capacity(),
            PreparedRoutes::Unknown => 0,
        };
        size_of::<Self>()
            .saturating_add(
                self.tokens
                    .capacity()
                    .saturating_mul(size_of::<(DisplayIdentity, HostWorkAreaToken)>()),
            )
            .saturating_add(route_capacity.saturating_mul(size_of::<PreparedRoute>()))
            .saturating_add(
                self.committed_routes
                    .capacity()
                    .saturating_mul(size_of::<CommittedRoute>()),
            )
    }
}

#[derive(Debug)]
enum PreparedRoutes {
    Exact(Vec<PreparedRoute>),
    Unknown,
}

#[derive(Debug, Clone, Copy)]
struct PreparedRoute {
    token: HostWorkAreaToken,
    display_bounds: PhysicalRect,
}

#[derive(Debug, Clone, Copy)]
struct CommittedRoute {
    display_bounds: PhysicalRect,
    binding: NativeWorkAreaBinding,
}

/// Adapter-owned identities plus exact routes from the last committed snapshot.
#[derive(Debug)]
pub(crate) struct NativeWorkAreaState {
    next_token: u64,
    tokens: Vec<(DisplayIdentity, HostWorkAreaToken)>,
    routes: Vec<CommittedRoute>,
    slot_limit: Option<usize>,
}

impl Default for NativeWorkAreaState {
    fn default() -> Self {
        Self {
            next_token: 1,
            tokens: Vec::new(),
            routes: Vec::new(),
            slot_limit: None,
        }
    }
}

impl NativeWorkAreaState {
    /// Allows replacing only a store that has never committed identity history.
    pub(crate) fn can_configure_capacity_limit(&self) -> bool {
        self.next_token == 1 && self.tokens.is_empty() && self.routes.is_empty()
    }

    /// Reserves bounded token history and committed route storage up front.
    pub(crate) fn with_capacity_limit(slots: usize) -> Result<Self, NativeHostProtocolError> {
        Self::construction_reservation_bytes(slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut tokens = Vec::new();
        let mut routes = Vec::new();
        reserve_exact_slots(&mut tokens, slots)?;
        reserve_exact_slots(&mut routes, slots)?;
        Ok(Self {
            next_token: 1,
            tokens,
            routes,
            slot_limit: Some(slots),
        })
    }

    /// Bounds old state plus one exact preparation and its core facts.
    /// Includes old tokens/routes, new tokens/prepared routes, future committed
    /// routes, `NativeWorkAreaFacts`, and every owning header. The bridge already
    /// accounts for the original `FrozenWorkAreaRoster` source allocation.
    pub(crate) fn construction_reservation_bytes(slots: usize) -> Option<usize> {
        let per_slot = size_of::<(DisplayIdentity, HostWorkAreaToken)>()
            .checked_mul(2)?
            .checked_add(size_of::<PreparedRoute>())?
            .checked_add(size_of::<CommittedRoute>().checked_mul(2)?)?
            .checked_add(size_of::<NativeWorkAreaFacts>())?;
        size_of::<Self>()
            .checked_add(size_of::<PreparedWorkAreaRoster>())?
            .checked_add(size_of::<NativeWorkAreaRoster>())?
            .checked_add(slots.checked_mul(per_slot)?)
    }

    /// Counts inline storage, retained token history, and actual route capacity.
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.tokens
                    .capacity()
                    .saturating_mul(size_of::<(DisplayIdentity, HostWorkAreaToken)>()),
            )
            .saturating_add(
                self.routes
                    .capacity()
                    .saturating_mul(size_of::<CommittedRoute>()),
            )
    }

    pub(crate) fn prepare(
        &self,
        roster: &FrozenWorkAreaRoster,
    ) -> Result<(NativeWorkAreaRoster, PreparedWorkAreaRoster), NativeHostProtocolError> {
        let mut next_token = self.next_token;
        let mut tokens = Vec::new();
        let mut committed_routes = Vec::new();
        let (core, routes) = match roster {
            FrozenWorkAreaRoster::Unknown => {
                (NativeWorkAreaRoster::Unknown, PreparedRoutes::Unknown)
            }
            FrozenWorkAreaRoster::Exact(records) => {
                let slots = records.len();
                if self.slot_limit.is_some_and(|limit| slots > limit) {
                    return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
                }
                let mut facts = Vec::new();
                let mut routes = Vec::new();
                reserve_exact_slots(&mut tokens, slots)?;
                reserve_exact_slots(&mut routes, slots)?;
                reserve_exact_slots(&mut committed_routes, slots)?;
                reserve_exact_slots(&mut facts, slots)?;
                for record in records {
                    let token = match self
                        .tokens
                        .iter()
                        .find(|(display, _)| *display == record.display)
                        .map(|(_, token)| *token)
                    {
                        Some(token) => token,
                        None => {
                            let value = next_token;
                            next_token = next_token
                                .checked_add(1)
                                .ok_or(NativeHostProtocolError::WorkAreaIdentityExhausted)?;
                            HostWorkAreaToken::new(value)
                        }
                    };
                    tokens.push((record.display, token));
                    facts.push(NativeWorkAreaFacts::new(
                        token,
                        record.work_area_bounds,
                        record.scale_factor,
                    ));
                    routes.push(PreparedRoute {
                        token,
                        display_bounds: record.display_bounds,
                    });
                }
                (
                    NativeWorkAreaRoster::Exact(facts),
                    PreparedRoutes::Exact(routes),
                )
            }
        };
        let prepared = PreparedWorkAreaRoster {
            next_token,
            tokens,
            routes,
            committed_routes,
        };
        Ok((core, prepared))
    }

    pub(crate) fn commit(&mut self, prepared: PreparedWorkAreaRoster, session: &DockspaceSession) {
        let PreparedRoutes::Exact(prepared_routes) = prepared.routes else {
            // Unknown revokes routing without replacing display identity history.
            self.routes = Vec::new();
            return;
        };
        let mut routes = prepared.committed_routes;
        for route in &prepared_routes {
            if let Some(binding) = session.native_work_area(route.token) {
                routes.push(CommittedRoute {
                    display_bounds: route.display_bounds,
                    binding,
                });
            }
        }
        debug_assert_eq!(
            routes.len(),
            prepared_routes.len(),
            "an applied core work-area roster must mint every adapter binding"
        );
        self.next_token = prepared.next_token;
        self.tokens = prepared.tokens;
        if routes.len() != prepared_routes.len() {
            routes.clear();
        }
        self.routes = routes;
    }

    pub(crate) fn binding_at(&self, point: PhysicalPoint) -> Option<NativeWorkAreaBinding> {
        let mut matches = self
            .routes
            .iter()
            .filter(|route| route.display_bounds.contains(point));
        let binding = matches.next()?.binding;
        matches.next().is_none().then_some(binding)
    }
}

/// Rejects backing larger than the plan, even if the allocator rounds upward.
fn reserve_exact_slots<T>(
    values: &mut Vec<T>,
    slots: usize,
) -> Result<(), NativeHostProtocolError> {
    values
        .try_reserve_exact(slots.saturating_sub(values.len()))
        .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
    if values.capacity() > slots {
        return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
    }
    Ok(())
}

fn compile_work_area(
    record: NativeWorkAreaRecord,
) -> Result<FrozenWorkArea, NativeHostProtocolError> {
    let display_bounds = physical_rect(record.display_bounds())?;
    let work_area_bounds = physical_rect(record.work_area_bounds())?;
    if work_area_bounds.x() < display_bounds.x()
        || work_area_bounds.y() < display_bounds.y()
        || work_area_bounds.max().x() > display_bounds.max().x()
        || work_area_bounds.max().y() > display_bounds.max().y()
    {
        return Err(NativeHostProtocolError::InvalidWorkAreaRoster);
    }
    let scale_factor = ScaleFactor::new(record.scale_factor())
        .map_err(|_| NativeHostProtocolError::InvalidWorkAreaRoster)?;
    Ok(FrozenWorkArea {
        display: DisplayIdentity::Native(record.display_id()),
        display_bounds,
        work_area_bounds,
        scale_factor,
    })
}

fn physical_rect(rect: NativePhysicalRect) -> Result<PhysicalRect, NativeHostProtocolError> {
    if rect.width() == 0 || rect.height() == 0 {
        return Err(NativeHostProtocolError::InvalidWorkAreaRoster);
    }
    PhysicalRect::new(
        f64::from(rect.x()),
        f64::from(rect.y()),
        f64::from(rect.width()),
        f64::from(rect.height()),
    )
    .map_err(|_| NativeHostProtocolError::InvalidWorkAreaRoster)
}

#[cfg(test)]
fn validate_exact_roster(
    mut records: Vec<FrozenWorkArea>,
) -> Result<FrozenWorkAreaRoster, NativeHostProtocolError> {
    validate_exact_records(&mut records)?;
    Ok(FrozenWorkAreaRoster::Exact(records))
}

fn validate_exact_records(records: &mut [FrozenWorkArea]) -> Result<(), NativeHostProtocolError> {
    if records.is_empty() {
        return Err(NativeHostProtocolError::InvalidWorkAreaRoster);
    }
    records.sort_unstable_by_key(|record| record.display);
    if records
        .windows(2)
        .any(|pair| pair[0].display == pair[1].display)
    {
        return Err(NativeHostProtocolError::InvalidWorkAreaRoster);
    }
    Ok(())
}

#[cfg(test)]
mod capacity_tests;

#[cfg(test)]
mod tests {
    use dockspace::model::DockspaceLayout;
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        NativeHostCapabilities, NativeHostCapability, NativePointerRoster, NativeReceiverAnswer,
    };

    use super::*;

    #[test]
    fn reusable_capture_keeps_storage_through_unknown_invalid_and_display_overflow() {
        let FrozenWorkAreaRoster::Exact(records) = FrozenWorkAreaRoster::for_test_exact([(
            7,
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).unwrap(),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).unwrap(),
            ScaleFactor::new(1.0).unwrap(),
        )]) else {
            unreachable!("the fixture constructs exact work-area facts");
        };
        let record = records[0];
        let mut spare = Vec::with_capacity(1);
        let original = spare.as_ptr();
        let capacity = spare.capacity();

        let (exact, exceeded) = fill_exact_reusing([Ok(record)].into_iter(), &mut spare);
        assert!(!exceeded);
        let Ok(FrozenWorkAreaRoster::Exact(storage)) = exact else {
            panic!("one valid display fits the protected backing");
        };
        assert_eq!(storage.as_ptr(), original);
        assert_eq!(storage.capacity(), capacity);
        spare = storage;

        let (unknown, exceeded) =
            FrozenWorkAreaRoster::capture_reusing(EframeWorkAreaRoster::Unknown, &mut spare);
        assert_eq!(unknown, Ok(FrozenWorkAreaRoster::Unknown));
        assert!(!exceeded);
        assert_eq!(spare.as_ptr(), original);
        let (invalid, exceeded) = fill_exact_reusing(
            [Err(NativeHostProtocolError::InvalidWorkAreaRoster)].into_iter(),
            &mut spare,
        );
        assert_eq!(invalid, Err(NativeHostProtocolError::InvalidWorkAreaRoster));
        assert!(!exceeded);
        assert_eq!(spare.as_ptr(), original);

        let (overflow, exceeded) =
            fill_exact_reusing(vec![Ok(record); capacity + 1].into_iter(), &mut spare);
        assert_eq!(overflow, Ok(FrozenWorkAreaRoster::Unknown));
        assert!(exceeded);
        assert_eq!(spare.capacity(), capacity);
        assert_eq!(spare.as_ptr(), original);
        assert!(spare.is_empty());

        let (exact, exceeded) = fill_exact_reusing([Ok(record)].into_iter(), &mut spare);
        assert!(!exceeded);
        let Ok(FrozenWorkAreaRoster::Exact(storage)) = exact else {
            panic!("the same backing accepts a later exact display roster");
        };
        assert_eq!(storage, records);
        assert_eq!(storage.as_ptr(), original);
        assert_eq!(storage.capacity(), capacity);
    }

    #[test]
    fn exact_prepare_retains_the_existing_display_token() {
        let display = DisplayIdentity::Test(7);
        let token = HostWorkAreaToken::new(19);
        let state = NativeWorkAreaState {
            next_token: 20,
            tokens: vec![(display, token)],
            routes: Vec::new(),
            slot_limit: None,
        };
        let roster = FrozenWorkAreaRoster::for_test_exact([(
            7,
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("display bounds validate"),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).expect("work-area bounds validate"),
            ScaleFactor::new(1.0).expect("scale validates"),
        )]);

        let (core, mut prepared) = state.prepare(&roster).expect("exact roster prepares");

        assert!(matches!(core, NativeWorkAreaRoster::Exact(records) if records.len() == 1));
        assert_eq!(prepared.tokens, vec![(display, token)]);
        assert_eq!(prepared.next_token, 20);
        let initial_bytes = prepared.owned_data_capacity_bytes();
        assert_eq!(
            initial_bytes,
            size_of::<PreparedWorkAreaRoster>()
                + size_of::<(DisplayIdentity, HostWorkAreaToken)>()
                + size_of::<PreparedRoute>()
                + size_of::<CommittedRoute>()
        );

        let PreparedRoutes::Exact(routes) = &mut prepared.routes else {
            panic!("an exact roster prepares route storage");
        };
        let initial_capacity = routes.capacity();
        routes.reserve_exact(7);
        let extra_capacity = routes.capacity() - initial_capacity;
        assert!(extra_capacity > 0);
        routes.clear();
        assert_eq!(
            prepared.owned_data_capacity_bytes(),
            initial_bytes + extra_capacity * size_of::<PreparedRoute>()
        );
    }

    #[test]
    fn unknown_prepare_does_not_copy_display_identity_history() {
        let display = DisplayIdentity::Test(7);
        let token = HostWorkAreaToken::new(19);
        let state = NativeWorkAreaState {
            next_token: 20,
            tokens: vec![(display, token)],
            routes: Vec::new(),
            slot_limit: None,
        };

        let (core, prepared) = state
            .prepare(&FrozenWorkAreaRoster::Unknown)
            .expect("unknown roster prepares without copying token history");

        assert_eq!(core, NativeWorkAreaRoster::Unknown);
        assert!(prepared.tokens.is_empty());
        assert!(matches!(prepared.routes, PreparedRoutes::Unknown));
        assert_eq!(
            prepared.owned_data_capacity_bytes(),
            size_of::<PreparedWorkAreaRoster>()
        );
        assert_eq!(state.tokens, vec![(display, token)]);
        assert_eq!(state.next_token, 20);
    }

    #[test]
    fn owned_capacity_tracks_commit_spare_storage_and_unknown_revocation() {
        let mut session =
            DockspaceSession::from_layout(DockspaceLayout::empty(), DockPolicy::default())
                .expect("empty test session initializes");
        session
            .enable_managed_native_host(
                dockspace::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .expect("managed native host enrolls");
        session
            .configure_managed_native_capabilities(
                NativeHostCapabilities::none_supported()
                    .with(NativeHostCapability::AuthoritativeInventory)
                    .with(NativeHostCapability::WorkArea),
            )
            .expect("work-area capabilities configure");
        let apply_snapshot = |session: &mut DockspaceSession, roster| {
            session
                .report_managed_native_snapshot([], roster)
                .expect("the complete work-area snapshot records");
            session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .expect("the work-area frame begins")
                .commit()
                .expect("the empty-layout frame commits");
        };
        let roster = FrozenWorkAreaRoster::for_test_exact([(
            7,
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("display bounds validate"),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).expect("work-area bounds validate"),
            ScaleFactor::new(1.0).expect("scale validates"),
        )]);
        let point = PhysicalPoint::new(1600.0, 1060.0).expect("desktop point validates");
        let mut state = NativeWorkAreaState::default();
        let empty_bytes = state.owned_data_capacity_bytes();
        assert_eq!(empty_bytes, size_of::<NativeWorkAreaState>());

        let (core, prepared) = state.prepare(&roster).expect("exact roster prepares");
        let token = prepared.tokens[0].1;
        assert_eq!(state.owned_data_capacity_bytes(), empty_bytes);
        assert_eq!(state.binding_at(point), None);
        assert_eq!(session.native_work_area(token), None);
        apply_snapshot(&mut session, core);
        let binding = session
            .native_work_area(token)
            .expect("the committed core snapshot mints work-area authority");
        state.commit(prepared, &session);
        assert_eq!(state.routes.len(), 1);
        assert_eq!(state.routes.capacity(), 1);
        assert_eq!(state.binding_at(point), Some(binding));
        let committed_bytes = state.owned_data_capacity_bytes();
        assert_eq!(
            committed_bytes - empty_bytes,
            size_of::<(DisplayIdentity, HostWorkAreaToken)>() + size_of::<CommittedRoute>()
        );

        let initial_capacity = state.routes.capacity();
        state.routes.reserve_exact(7);
        let extra_capacity = state.routes.capacity() - initial_capacity;
        assert!(extra_capacity > 0);
        let retained_bytes = state.owned_data_capacity_bytes();
        assert_eq!(
            retained_bytes - committed_bytes,
            extra_capacity * size_of::<CommittedRoute>()
        );
        assert_eq!(state.binding_at(point), Some(binding));
        let route_bytes = state.routes.capacity() * size_of::<CommittedRoute>();
        let next_token = state.next_token;

        let (core, prepared) = state
            .prepare(&FrozenWorkAreaRoster::Unknown)
            .expect("unknown roster prepares");
        apply_snapshot(&mut session, core);
        state.commit(prepared, &session);
        assert!(state.routes.is_empty());
        assert_eq!(state.routes.capacity(), 0);
        assert_eq!(state.binding_at(point), None);
        assert_eq!(session.native_work_area(token), None);
        assert_eq!(state.tokens, vec![(DisplayIdentity::Test(7), token)]);
        assert_eq!(state.next_token, next_token);
        assert_eq!(
            state.owned_data_capacity_bytes(),
            retained_bytes - route_bytes
        );
        assert!(state.owned_data_capacity_bytes() > empty_bytes);
    }

    #[test]
    fn exact_validation_sorts_display_identities_and_rejects_duplicates() {
        let display_bounds =
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("display bounds validate");
        let work_area_bounds =
            PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).expect("work-area bounds validate");
        let scale = ScaleFactor::new(1.0).expect("scale validates");
        let FrozenWorkAreaRoster::Exact(records) = FrozenWorkAreaRoster::for_test_exact([
            (2, display_bounds, work_area_bounds, scale),
            (1, display_bounds, work_area_bounds, scale),
        ]) else {
            unreachable!("the fixture constructs exact records");
        };
        let FrozenWorkAreaRoster::Exact(sorted) =
            validate_exact_roster(records).expect("distinct displays validate")
        else {
            unreachable!("validation preserves exact records");
        };
        assert_eq!(sorted[0].display, DisplayIdentity::Test(1));
        assert_eq!(sorted[1].display, DisplayIdentity::Test(2));
        assert_eq!(
            validate_exact_roster(vec![sorted[0], sorted[0], sorted[1]]),
            Err(NativeHostProtocolError::InvalidWorkAreaRoster)
        );
    }
}
