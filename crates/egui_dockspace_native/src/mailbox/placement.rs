//! Placement preparation and core publication under the existing host lock.

use super::*;

impl NativeHostState {
    /// The speculative rows carry backing promises, never callback authority.
    /// Holding this lock through publication prevents another owner spending them.
    pub(super) fn prepare_native_placement_changes(
        &mut self,
        frame: &PreparedDockspaceHostFrameCommit<'_>,
    ) -> Result<(), NativeRuntimeError> {
        self.viewports
            .validate_workspace_root_continuation_rosters(
                || frame.native_root_continuations(),
                frame.native_placement_detachments(),
                |window| frame.committed_native_surface_bindings_for_window(window),
                |window| frame.native_surface_bindings_for_window(window),
            )?;
        self.refresh_window_resource_references();
        let mut detached_placements = 0_usize;
        for continuation in frame.native_root_continuations() {
            let predecessor = continuation.predecessor();
            let successor = continuation.successor();
            let Some(viewport) = self
                .viewports
                .workspace_root_continuation_viewport(continuation)?
            else {
                continue;
            };
            let window = self
                .viewports
                .window(viewport)
                .expect("validated same-window replacement retains its physical window");
            if !self.retirements.can_detach_binding(predecessor)
                || self.retirements.has_quiescence_owner(successor)
                || self.retirements.owns_window(window, None)
            {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    successor.surface(),
                )
                .into());
            }
            detached_placements = detached_placements
                .checked_add(1)
                .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        }
        for &binding in frame.native_placement_detachments() {
            if !self.retirements.can_detach_binding(binding) {
                return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(
                    binding.surface(),
                )
                .into());
            }
            detached_placements = detached_placements
                .checked_add(usize::from(
                    self.retirements.detachment_needs_entry(binding),
                ))
                .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        }
        // One physical continuation can retire several exact placements. Pay
        // the complete batch before publication, independently of window slots.
        if detached_placements > 0 {
            let required = self
                .retirements
                .bindings()
                .count()
                .checked_add(detached_placements)
                .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
            let growth = NativeRetirementState::reservation_bytes(required)
                .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?
                .saturating_sub(self.retirements.owned_data_capacity_bytes());
            if !self.can_reserve_input_data(growth) {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
            self.retirements
                .prepare_capacity_for_retained_lifetimes(required)?;
        }
        for (index, continuation) in frame.native_root_continuations().enumerate() {
            let predecessor = continuation.predecessor();
            let successor = continuation.successor();
            let Some(viewport) = self
                .viewports
                .workspace_root_continuation_viewport(continuation)?
            else {
                continue;
            };
            if frame
                .native_root_continuations()
                .take(index)
                .any(|previous| previous.predecessor().window() == predecessor.window())
            {
                continue;
            }
            if !self.can_admit_binding(successor) {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable.into());
            }
            self.window_resources
                .prepare_workspace_replacement(viewport, successor)?;
        }
        Ok(())
    }

    /// Every fallible capacity and receipt check ran under this same lock.
    pub(super) fn commit_native_placement_changes(&mut self, report: &HostFrameReport) {
        if report.native_root_continuations().is_empty() {
            self.window_resources.abort_workspace_replacement();
        }
        self.viewports
            .commit_workspace_root_continuations(report.native_root_continuations());
        for (index, &continuation) in report.native_root_continuations().iter().enumerate() {
            let predecessor = continuation.predecessor();
            let successor = continuation.successor();
            let Some(viewport) = self.viewports.viewport_for_window(successor.window()) else {
                continue;
            };
            assert_eq!(
                self.viewports.window_binding(viewport),
                Some(successor.window()),
                "the prepaid exact successor window retains its viewport"
            );
            if !report.native_root_continuations()[..index]
                .iter()
                .any(|previous| previous.successor().window() == successor.window())
            {
                self.window_resources.retain_workspace_predecessor(
                    viewport,
                    predecessor,
                    successor,
                );
                self.window_resources.retire(viewport, predecessor);
                self.window_resources
                    .commit_host_reference(viewport, successor);
            }
            // The physical receipt moves once; every exact predecessor keeps
            // its own independently drainable core terminal guard.
            self.retirements.commit_detachment(predecessor);
        }
        for &binding in report.native_placement_detachments() {
            self.retirements.commit_detachment(binding);
            for (_, resource) in self.output_resources.iter_mut() {
                if resource
                    .reservation
                    .binding()
                    .is_some_and(|owner| owner.window() == binding.window())
                {
                    resource.retains_detached_placements = true;
                }
            }
        }
        self.refresh_window_resource_references();
        if !report.native_root_continuations().is_empty()
            || !report.native_placement_detachments().is_empty()
        {
            self.invalidate_viewport_roster();
        }
    }

    pub(super) fn refresh_window_resource_references(&mut self) {
        let Self {
            viewports,
            output_resources,
            window_resources,
            ..
        } = self;
        window_resources.refresh_host_references(|viewport, window| {
            viewports.window_binding(viewport) == Some(window)
                || output_resources.iter().any(|(token, resource)| {
                    token.viewport_id() == viewport
                        && resource
                            .reservation
                            .binding()
                            .is_some_and(|binding| binding.window() == window)
                })
        });
    }

    pub(super) fn prepare_window_resource_reference(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeViewportBindingError> {
        self.refresh_window_resource_references();
        self.window_resources
            .prepare_host_reference(viewport, binding)
            .map_err(|_| NativeViewportBindingError::CapacityExceeded {
                limit: self
                    .window_resources
                    .resource_slot_limit()
                    .unwrap_or(usize::MAX),
            })
    }
}

impl NativeHostBridge {
    /// Core commit does not call eframe or the bridge. Holding the existing host
    /// lock through preparation and publication prevents concurrent callbacks
    /// from spending a prepared resource slot before its infallible install.
    pub(crate) fn commit_host_frame_with_staging(
        &self,
        frame: DockspaceHostFrame<'_>,
        include_staging: bool,
        root_surface: Option<&mut SurfaceId>,
    ) -> Result<(HostFrameReport, Vec<ViewportId>), NativeRuntimeError> {
        let frame = frame.prepare_commit()?;
        let mut records = self.lock();
        let root_continuation = frame.native_root_continuations().find(|continuation| {
            records.viewports.window_binding(ViewportId::ROOT)
                == Some(continuation.predecessor().window())
                && root_surface.as_deref().map_or_else(
                    || {
                        records
                            .viewports
                            .binding(ViewportId::ROOT)
                            .is_some_and(|binding| {
                                binding.surface() == continuation.predecessor().surface()
                            })
                    },
                    |surface| *surface == continuation.predecessor().surface(),
                )
        });
        if root_continuation.is_none()
            && let Some(root_surface) = root_surface.as_deref()
            && frame.native_root_continuations().any(|continuation| {
                records.viewports.window_binding(ViewportId::ROOT)
                    == Some(continuation.predecessor().window())
            })
        {
            return Err(NativeHostProtocolError::NativeAdmissionRouteChanged(*root_surface).into());
        }
        if let Err(error) = records.prepare_native_placement_changes(&frame) {
            records.window_resources.abort_workspace_replacement();
            return Err(error);
        }
        let committed = (|| {
            let wake = records
                .window_resources
                .prepare_repaints(include_staging && records.mode.accepts_new_work())?;
            let report = frame.commit()?;
            Ok::<_, NativeRuntimeError>((report, wake))
        })();
        match committed {
            Ok((report, wake)) => {
                records.commit_native_placement_changes(&report);
                if let Some(root_surface) = root_surface
                    && let Some(continuation) = report
                        .native_root_continuations()
                        .iter()
                        .find(|&&continuation| Some(continuation) == root_continuation)
                {
                    *root_surface = continuation.successor().surface();
                }
                Ok((report, wake))
            }
            Err(error) => {
                records.window_resources.abort_workspace_replacement();
                Err(error)
            }
        }
    }

    pub(crate) fn workspace_replacement_predecessor(
        &self,
        successor: NativeSurfaceBinding,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        self.lock()
            .window_resources
            .workspace_predecessor(successor)
    }

    pub(crate) fn finish_workspace_replacement(
        &self,
        viewport: ViewportId,
        predecessor: NativeSurfaceBinding,
    ) {
        let mut records = self.lock();
        records
            .window_resources
            .finish_workspace_predecessor(viewport, predecessor);
        records.refresh_window_resource_references();
    }
}
