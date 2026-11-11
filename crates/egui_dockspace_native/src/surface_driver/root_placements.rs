//! Pending declarations, committed cache and exact token-owned Root input geometry.

use std::sync::Arc;

use dockspace::model::SurfaceId;
use dockspace::runtime::{HostFrameReport, HostInputOutcome, NativeSurfacePlacement};
use eframe::{NativeOutputToken, egui::ViewportId};

use crate::coordinator::NativeCoordinator;
use crate::error::{NativeHostProtocolError, NativeRuntimeError};
use crate::mailbox::NativeHostBridge;

#[derive(Default)]
pub(super) struct RootPlacements {
    // A declaration is pending work; only core grants current roster authority.
    pub(super) request: Option<Arc<[NativeSurfacePlacement]>>,
    pub(super) cache: Option<Arc<[NativeSurfacePlacement]>>,
    input: Option<RootInputPlacements>,
    pub(super) explicit: bool,
    invalid_request: bool,
}

pub(super) struct RootInputPlacements {
    token: NativeOutputToken,
    placements: Option<Arc<[NativeSurfacePlacement]>>,
    declare_explicit: bool,
}

impl RootInputPlacements {
    fn implicit(token: NativeOutputToken) -> Self {
        Self {
            token,
            placements: None,
            declare_explicit: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum NativeRootPlacementUpdate {
    Unchanged,
    Applied,
    Deferred,
}

pub(super) fn apply_root_placements(
    frame: &mut dockspace::runtime::DockspaceHostFrame<'_>,
    bridge: &NativeHostBridge,
    window: dockspace::runtime::NativeWindowBinding,
    placements: &[NativeSurfacePlacement],
    declare_explicit: bool,
) -> Result<NativeRootPlacementUpdate, NativeRuntimeError> {
    let unchanged = root_placements_match_frame(frame, window, placements);
    if unchanged && !declare_explicit {
        return Ok(NativeRootPlacementUpdate::Unchanged);
    }
    let close_pending = bridge.with_close_control(|control| {
        control
            .unresolved_requests()
            .any(|request| request.binding().window() == window)
            || frame
                .native_surface_bindings_for_window(window)
                .any(|binding| control.request_for_binding(binding).is_some())
    });
    // A held close freezes the whole window. Responsive geometry waits for its
    // decision instead of making an ordinary resize a fatal configuration error.
    if !unchanged && close_pending {
        return Ok(NativeRootPlacementUpdate::Deferred);
    }
    frame.set_native_surface_placements(window, placements)?;
    Ok(if unchanged {
        NativeRootPlacementUpdate::Unchanged
    } else {
        NativeRootPlacementUpdate::Applied
    })
}

pub(super) fn root_placements_match_frame(
    frame: &dockspace::runtime::DockspaceHostFrame<'_>,
    window: dockspace::runtime::NativeWindowBinding,
    placements: &[NativeSurfacePlacement],
) -> bool {
    frame.native_surface_bindings_for_window(window).count() == placements.len()
        && placements.iter().all(|placement| {
            frame
                .native_surface_bindings_for_window(window)
                .find(|binding| binding.surface() == placement.surface())
                .and_then(|binding| frame.native_surface_placement(binding))
                == Some(*placement)
        })
}

fn snapshot_root_placements(
    bridge: &NativeHostBridge,
    retained_bytes: usize,
    count: usize,
    source: impl Iterator<Item = Option<NativeSurfacePlacement>>,
) -> Result<Option<Arc<[NativeSurfacePlacement]>>, NativeRuntimeError> {
    let bytes = count
        .checked_mul(size_of::<NativeSurfacePlacement>())
        // Vec-to-Arc conversion briefly owns both complete backings.
        .and_then(|bytes| bytes.checked_mul(2))
        .and_then(|bytes| bytes.checked_add(retained_bytes))
        .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
    if !bridge.prepare_root_placement_storage(bytes) {
        return Ok(None);
    }
    let mut placements = Vec::new();
    if placements.try_reserve_exact(count).is_err() || placements.capacity() > count {
        return Ok(None);
    }
    for placement in source {
        placements.push(placement.ok_or(NativeHostProtocolError::InvalidWindowSnapshot)?);
    }
    Ok(Some(Arc::from(placements)))
}

impl RootPlacements {
    pub(super) fn retained_bytes(&self) -> usize {
        let rosters = [
            self.request.as_ref(),
            self.cache.as_ref(),
            self.input
                .as_ref()
                .and_then(|input| input.placements.as_ref()),
        ];
        rosters
            .iter()
            .enumerate()
            .fold(0_usize, |bytes, (index, roster)| {
                let Some(roster) = roster else {
                    return bytes;
                };
                if rosters[..index]
                    .iter()
                    .flatten()
                    .any(|previous| Arc::ptr_eq(previous, roster))
                {
                    bytes
                } else {
                    bytes.saturating_add(size_of_val(&***roster))
                }
            })
    }

    pub(super) fn reserve(
        &self,
        bridge: &NativeHostBridge,
        additional: usize,
    ) -> Result<(), NativeRuntimeError> {
        let bytes = self
            .retained_bytes()
            .checked_add(additional)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if !bridge.prepare_root_placement_storage(bytes) {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
        }
        Ok(())
    }

    pub(super) fn refresh_storage(&self, bridge: &NativeHostBridge) {
        let paid = bridge.prepare_root_placement_storage(self.retained_bytes());
        debug_assert!(
            paid,
            "releasing already prepaid placement backing cannot fail"
        );
    }

    pub(super) fn matches_committed(
        coordinator: &NativeCoordinator,
        placements: &[NativeSurfacePlacement],
    ) -> bool {
        let Some(route) = coordinator.viewport_binding(ViewportId::ROOT) else {
            return false;
        };
        let session = coordinator.session();
        session
            .native_surface_bindings_for_window(route.window())
            .count()
            == placements.len()
            && placements.iter().all(|placement| {
                session
                    .native_surface_bindings_for_window(route.window())
                    .find(|binding| binding.surface() == placement.surface())
                    .and_then(|binding| session.native_surface_placement(binding))
                    == Some(*placement)
            })
    }

    fn declared(&self, coordinator: &NativeCoordinator) -> Option<&[NativeSurfacePlacement]> {
        self.request.as_deref().or_else(|| {
            self.cache
                .as_deref()
                .filter(|cache| Self::matches_committed(coordinator, cache))
        })
    }

    fn request_is_current(
        coordinator: &NativeCoordinator,
        root: SurfaceId,
        placements: &[NativeSurfacePlacement],
    ) -> bool {
        let session = coordinator.session();
        let window = coordinator
            .viewport_binding(ViewportId::ROOT)
            .map(|binding| binding.window());
        placements
            .iter()
            .any(|placement| placement.surface() == root)
            && placements.iter().all(|placement| {
                session.view().surface(placement.surface()).is_some()
                    && session.native_window_bindings().all(|owner| {
                        Some(owner) == window
                            || session
                                .native_surface_bindings_for_window(owner)
                                .all(|binding| binding.surface() != placement.surface())
                    })
            })
    }

    pub(super) fn next(
        &mut self,
        coordinator: &NativeCoordinator,
        replacement_queued: bool,
    ) -> Result<Option<Arc<[NativeSurfacePlacement]>>, NativeRuntimeError> {
        if !replacement_queued && let Some(request) = &self.request {
            return Ok(Some(Arc::clone(request)));
        }
        let Some(route) = coordinator.viewport_binding(ViewportId::ROOT) else {
            return Ok(None);
        };
        let count = coordinator
            .session()
            .native_surface_bindings_for_window(route.window())
            .count();
        if !self.explicit && count <= 1 {
            return Ok(None);
        }
        self.explicit = true;
        if let Some(cache) = &self.cache
            && Self::matches_committed(coordinator, cache)
        {
            return Ok(Some(Arc::clone(cache)));
        }
        let bridge = coordinator.dispatch_bridge();
        self.cache = None;
        self.refresh_storage(&bridge);
        let session = coordinator.session();
        let placements = snapshot_root_placements(
            &bridge,
            self.retained_bytes(),
            count,
            session
                .native_surface_bindings_for_window(route.window())
                .map(|binding| session.native_surface_placement(binding)),
        )?
        .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        self.cache = Some(Arc::clone(&placements));
        self.refresh_storage(&bridge);
        Ok(Some(placements))
    }

    pub(super) fn for_run(
        &mut self,
        coordinator: &NativeCoordinator,
        replacement_queued: bool,
        token: NativeOutputToken,
    ) -> Result<Option<Arc<[NativeSurfacePlacement]>>, NativeRuntimeError> {
        if let Some(input) = &self.input
            && input.token == token
        {
            return Ok(input.placements.clone());
        }
        let bridge = coordinator.dispatch_bridge();
        self.input = None;
        self.refresh_storage(&bridge);
        let placements = self.next(coordinator, replacement_queued)?;
        let declare_explicit = !replacement_queued
            && placements.as_ref().is_some_and(|placements| {
                self.request
                    .as_ref()
                    .is_some_and(|request| Arc::ptr_eq(request, placements))
            });
        self.admit(
            RootInputPlacements {
                token,
                placements: placements.clone(),
                declare_explicit,
            },
            &bridge,
        );
        Ok(placements)
    }

    pub(super) fn finish(&mut self, token: NativeOutputToken, bridge: &NativeHostBridge) {
        if self
            .input
            .as_ref()
            .is_some_and(|input| input.token == token)
        {
            self.input = None;
            self.refresh_storage(bridge);
        }
    }

    pub(super) fn discard_other_run(
        &mut self,
        token: NativeOutputToken,
        bridge: &NativeHostBridge,
    ) {
        if self
            .input
            .as_ref()
            .is_some_and(|input| input.token != token)
        {
            self.input = None;
            self.refresh_storage(bridge);
        }
    }

    pub(super) fn invalid_request(&self) -> bool {
        self.invalid_request
    }

    pub(super) fn declares_for(&self, token: NativeOutputToken) -> bool {
        self.input
            .as_ref()
            .is_some_and(|input| input.token == token && input.declare_explicit)
    }

    pub(super) fn commit(
        &mut self,
        coordinator: &NativeCoordinator,
        root: SurfaceId,
        report: &HostFrameReport,
    ) {
        let replaced = report.inputs().iter().any(|input| match input {
            HostInputOutcome::LayoutReset { .. } => true,
            #[cfg(feature = "serde")]
            HostInputOutcome::DocumentRestored { .. } => true,
            _ => false,
        });
        let request_expired = self
            .request
            .as_ref()
            .is_some_and(|request| !Self::request_is_current(coordinator, root, request));
        if replaced || request_expired {
            // A declaration from the preceding workspace cannot revive it.
            self.request = None;
        } else if self.request.as_ref().is_some_and(|request| {
            self.input
                .as_ref()
                .and_then(|input| input.placements.as_deref())
                == Some(&**request)
                && Self::matches_committed(coordinator, request)
        }) {
            self.request = None;
        }
        if let Some(placements) = self
            .input
            .as_ref()
            .and_then(|input| input.placements.as_ref())
            .filter(|placements| Self::matches_committed(coordinator, placements))
        {
            self.cache = Some(Arc::clone(placements));
        } else if self
            .cache
            .as_ref()
            .is_some_and(|cache| !Self::matches_committed(coordinator, cache))
        {
            self.cache = None;
        }
        // Cache publication shares already prepaid backing and cannot allocate.
        self.refresh_storage(&coordinator.dispatch_bridge());
    }

    pub(super) fn clear(&mut self, bridge: &NativeHostBridge) {
        self.request = None;
        self.cache = None;
        self.input = None;
        self.refresh_storage(bridge);
    }

    pub(super) fn declare(
        &mut self,
        coordinator: &NativeCoordinator,
        root: SurfaceId,
        token: NativeOutputToken,
        placements: &[NativeSurfacePlacement],
        repeated_run: bool,
    ) -> Result<bool, NativeRuntimeError> {
        let admitted = self
            .input
            .as_ref()
            .is_some_and(|input| input.token == token);
        let view = coordinator.session().view();
        let invalid = placements.is_empty()
            || !placements
                .iter()
                .any(|placement| placement.surface() == root)
            || placements.len() > view.surfaces().count()
            || placements
                .iter()
                .any(|placement| view.surface(placement.surface()).is_none())
            || placements.iter().enumerate().any(|(index, placement)| {
                placements[..index]
                    .iter()
                    .any(|previous| previous.surface() == placement.surface())
            });
        if invalid && !admitted {
            return Err(NativeHostProtocolError::InvalidWindowSnapshot.into());
        }
        if repeated_run && !invalid && self.declared(coordinator) != Some(placements) {
            return Err(NativeHostProtocolError::MultipassOutputChanged.into());
        }
        let bridge = coordinator.dispatch_bridge();
        if invalid {
            self.invalid_request = true;
        } else if self.declared(coordinator) != Some(placements) {
            if let Err(error) = self.reserve(&bridge, size_of_val(placements)) {
                if !admitted {
                    return Err(error);
                }
                self.invalid_request = true;
            } else {
                self.request = Some(Arc::from(placements));
                self.refresh_storage(&bridge);
            }
        }
        self.explicit = true;
        // Raw input retains its admitted roster until the exact run terminates.
        Ok(admitted
            && self
                .input
                .as_ref()
                .and_then(|input| input.placements.as_deref())
                != Some(placements))
    }

    pub(super) fn prepare_input(
        &mut self,
        bridge: &NativeHostBridge,
        root: SurfaceId,
        token: NativeOutputToken,
        replacement_queued: bool,
        frame: &mut dockspace::runtime::DockspaceHostFrame<'_>,
        native_input_pending: bool,
    ) -> Result<Option<RootInputPlacements>, NativeRuntimeError> {
        let mut input = RootInputPlacements::implicit(token);
        let Some(window) = bridge
            .output_reservation(token)
            .and_then(|reservation| reservation.binding())
            .map(|binding| binding.window())
        else {
            return Ok(Some(input));
        };
        if let Some(placements) = self.request.as_ref()
            && !replacement_queued
            && !native_input_pending
            && placements
                .iter()
                .any(|placement| placement.surface() == root)
            && placements
                .iter()
                .all(|placement| frame.view().surface(placement.surface()).is_some())
            && apply_root_placements(frame, bridge, window, placements, true)?
                != NativeRootPlacementUpdate::Deferred
        {
            input.placements = Some(Arc::clone(placements));
            input.declare_explicit = true;
        }
        let count = frame.native_surface_bindings_for_window(window).count();
        self.explicit |= count > 1;
        if input.placements.is_none() && self.explicit {
            input.placements = self
                .cache
                .as_ref()
                .filter(|cache| root_placements_match_frame(frame, window, cache))
                .cloned();
            if input.placements.is_none() {
                input.placements = snapshot_root_placements(
                    bridge,
                    self.retained_bytes(),
                    count,
                    frame
                        .native_surface_bindings_for_window(window)
                        .map(|binding| frame.native_surface_placement(binding)),
                )?;
                if input.placements.is_none() {
                    return Ok(None);
                }
            }
        }
        Ok(Some(input))
    }

    pub(super) fn admit(&mut self, input: RootInputPlacements, bridge: &NativeHostBridge) {
        self.input = Some(input);
        self.refresh_storage(bridge);
    }
}
