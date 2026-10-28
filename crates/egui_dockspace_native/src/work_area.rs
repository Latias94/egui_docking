//! Atomic work-area routing sidecar for the native coordinator.

use std::collections::BTreeMap;

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
    pub(crate) fn capture(
        roster: EframeWorkAreaRoster<'_>,
    ) -> Result<Self, NativeHostProtocolError> {
        match roster {
            EframeWorkAreaRoster::Exact(records) => {
                let records = records
                    .iter()
                    .copied()
                    .map(compile_work_area)
                    .collect::<Result<Vec<_>, _>>()?;
                validate_exact_roster(records)
            }
            EframeWorkAreaRoster::Unknown => Ok(Self::Unknown),
        }
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

#[derive(Debug)]
pub(crate) struct PreparedWorkAreaRoster {
    next_token: u64,
    tokens: BTreeMap<DisplayIdentity, HostWorkAreaToken>,
    routes: PreparedRoutes,
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
    tokens: BTreeMap<DisplayIdentity, HostWorkAreaToken>,
    routes: Vec<CommittedRoute>,
}

impl Default for NativeWorkAreaState {
    fn default() -> Self {
        Self {
            next_token: 1,
            tokens: BTreeMap::new(),
            routes: Vec::new(),
        }
    }
}

impl NativeWorkAreaState {
    pub(crate) fn prepare(
        &self,
        roster: &FrozenWorkAreaRoster,
    ) -> Result<(NativeWorkAreaRoster, PreparedWorkAreaRoster), NativeHostProtocolError> {
        let mut next_token = self.next_token;
        let mut tokens = BTreeMap::new();
        let (core, routes) = match roster {
            FrozenWorkAreaRoster::Unknown => {
                (NativeWorkAreaRoster::Unknown, PreparedRoutes::Unknown)
            }
            FrozenWorkAreaRoster::Exact(records) => {
                let mut facts = Vec::with_capacity(records.len());
                let mut routes = Vec::with_capacity(records.len());
                for record in records {
                    let token = match self.tokens.get(&record.display).copied() {
                        Some(token) => token,
                        None => {
                            let value = next_token;
                            next_token = next_token
                                .checked_add(1)
                                .ok_or(NativeHostProtocolError::WorkAreaIdentityExhausted)?;
                            HostWorkAreaToken::new(value)
                        }
                    };
                    tokens.insert(record.display, token);
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
        };
        Ok((core, prepared))
    }

    pub(crate) fn commit(&mut self, prepared: PreparedWorkAreaRoster, session: &DockspaceSession) {
        let PreparedRoutes::Exact(prepared_routes) = prepared.routes else {
            // Unknown revokes routing without replacing display identity history.
            self.routes = Vec::new();
            return;
        };
        let routes = prepared_routes
            .iter()
            .filter_map(|route| {
                session
                    .native_work_area(route.token)
                    .map(|binding| CommittedRoute {
                        display_bounds: route.display_bounds,
                        binding,
                    })
            })
            .collect::<Vec<_>>();
        debug_assert_eq!(
            routes.len(),
            prepared_routes.len(),
            "an applied core work-area roster must mint every adapter binding"
        );
        self.next_token = prepared.next_token;
        self.tokens = prepared.tokens;
        self.routes = if routes.len() == prepared_routes.len() {
            routes
        } else {
            Vec::new()
        };
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

fn validate_exact_roster(
    mut records: Vec<FrozenWorkArea>,
) -> Result<FrozenWorkAreaRoster, NativeHostProtocolError> {
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
    Ok(FrozenWorkAreaRoster::Exact(records))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_prepare_retains_the_existing_display_token() {
        let display = DisplayIdentity::Test(7);
        let token = HostWorkAreaToken::new(19);
        let state = NativeWorkAreaState {
            next_token: 20,
            tokens: BTreeMap::from([(display, token)]),
            routes: Vec::new(),
        };
        let roster = FrozenWorkAreaRoster::for_test_exact([(
            7,
            PhysicalRect::new(0.0, 0.0, 1920.0, 1080.0).expect("display bounds validate"),
            PhysicalRect::new(0.0, 0.0, 1920.0, 1040.0).expect("work-area bounds validate"),
            ScaleFactor::new(1.0).expect("scale validates"),
        )]);

        let (core, prepared) = state.prepare(&roster).expect("exact roster prepares");

        assert!(matches!(core, NativeWorkAreaRoster::Exact(records) if records.len() == 1));
        assert_eq!(prepared.tokens, BTreeMap::from([(display, token)]));
        assert_eq!(prepared.next_token, 20);
    }

    #[test]
    fn unknown_prepare_does_not_copy_display_identity_history() {
        let display = DisplayIdentity::Test(7);
        let token = HostWorkAreaToken::new(19);
        let state = NativeWorkAreaState {
            next_token: 20,
            tokens: BTreeMap::from([(display, token)]),
            routes: Vec::new(),
        };

        let (core, prepared) = state
            .prepare(&FrozenWorkAreaRoster::Unknown)
            .expect("unknown roster prepares without copying token history");

        assert_eq!(core, NativeWorkAreaRoster::Unknown);
        assert!(prepared.tokens.is_empty());
        assert!(matches!(prepared.routes, PreparedRoutes::Unknown));
        assert_eq!(state.tokens, BTreeMap::from([(display, token)]));
        assert_eq!(state.next_token, 20);
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
