//! One exact binding owns creation, staging, callbacks and pending observations.

#[cfg(test)]
use std::collections::BTreeMap;
use std::mem::size_of;

use dockspace::runtime::{
    NativeCreateEffectAcknowledgement, NativePresentationEffectAcknowledgement,
    NativeStagingPaintRequest, NativeSurfaceBinding,
};
use eframe::{
    NativePhysicalRect, NativeViewportFocusCommandToken,
    NativeViewportPointerPassthroughCommandToken, NativeViewportPropertyCommandRetirement,
    NativeViewportVisibilityCommandToken, egui::ViewportId,
};
use winit::window::WindowId;

use super::command_callback::{CommandCallbackAdmission, NativeCommandCallbacks};
use super::journal::ReservedHostRecord;
use super::{HostRecord, NativeCreateReservation, NativeStagingPaintRecord};
use crate::coordinator::PendingNoFrameInvalidation;
use crate::error::NativeHostProtocolError;
use crate::focus_control::NativeViewportFocusRecord;
use crate::input_control::NativePointerPassthroughRecord;
use crate::pointer_observation::{NativePointerObservationKey, NativePointerObservationRecord};
use crate::viewport_callback::NativeViewportVisibilityRecord;

#[derive(Debug)]
struct StagingReceipt {
    request: NativeStagingPaintRequest,
    ingress: Option<ReservedHostRecord>,
    transferred: bool,
}

impl StagingReceipt {
    const fn new(request: NativeStagingPaintRequest) -> Self {
        Self {
            request,
            ingress: None,
            transferred: false,
        }
    }

    fn is_awaiting(&self) -> bool {
        self.ingress.is_none() && !self.transferred
    }

    fn receive(
        &mut self,
        expected: NativeStagingPaintRequest,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> bool {
        if self.request != expected || !self.is_awaiting() {
            return false;
        }
        self.ingress = Some(reserve());
        true
    }

    fn take_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        if self.transferred || self.ingress.as_ref()?.ordinal() != ordinal {
            return None;
        }
        let ingress = self.ingress.take()?;
        self.transferred = true;
        Some(ingress)
    }
}

#[derive(Debug)]
struct NativeWindowResource {
    viewport: ViewportId,
    binding: NativeSurfaceBinding,
    retired: bool,
    retained_by_host: bool,
    workspace_replacement: WorkspaceReplacementResource,
    create_rect: Option<NativePhysicalRect>,
    hidden_render: bool,
    staging: Option<StagingReceipt>,
    callbacks: NativeCommandCallbacks,
    created: bool,
    create_ack: Option<NativeCreateEffectAcknowledgement>,
    presentation_ack: Option<NativePresentationEffectAcknowledgement>,
    no_frame: Option<PendingNoFrameInvalidation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkspaceReplacementResource {
    None,
    Prepared,
    Predecessor,
}

impl NativeWindowResource {
    fn is_empty(&self) -> bool {
        !self.retained_by_host
            && self.workspace_replacement == WorkspaceReplacementResource::None
            && (!self.created || self.retired)
            && self.create_ack.is_none()
            && self.presentation_ack.is_none()
            && self.no_frame.is_none()
            && self.create_rect.is_none()
            && !self.hidden_render
            && self.staging.is_none()
            && if self.retired {
                !self.callbacks.has_pending_work()
            } else {
                self.callbacks.is_empty()
            }
    }

    fn has_pending_work(&self) -> bool {
        self.workspace_replacement != WorkspaceReplacementResource::None
            || self.create_ack.is_some()
            || self.presentation_ack.is_some()
            || self.no_frame.is_some()
            || self.create_rect.is_some()
            || self.hidden_render
            || self.staging.is_some()
            || self.callbacks.has_pending_work()
    }
}

#[derive(Debug, Default)]
pub(super) struct NativeWindowResources {
    entries: Vec<NativeWindowResource>,
    slot_limit: Option<usize>,
}

/// Prepared request data and wake backing, without callback or lifetime authority.
/// The caller must serialize owner activation and retirement between preparation
/// and publication on the same resource store.
#[derive(Debug)]
#[must_use = "prepared staging must be published after core commit or dropped"]
pub(crate) struct PreparedWindowStaging {
    requests: Vec<(ViewportId, NativeStagingPaintRequest)>,
    wake: Vec<ViewportId>,
}

impl PreparedWindowStaging {
    pub(crate) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>()
            .saturating_add(
                self.requests
                    .capacity()
                    .saturating_mul(size_of::<(ViewportId, NativeStagingPaintRequest)>()),
            )
            .saturating_add(self.wake.capacity().saturating_mul(size_of::<ViewportId>()))
    }
}

impl NativeWindowResources {
    /// Replacement is permitted only after every exact owner has been released.
    pub(super) fn can_configure_capacity_limit(&self) -> bool {
        self.entries.is_empty()
    }

    pub(super) const fn resource_slot_limit(&self) -> Option<usize> {
        self.slot_limit
    }

    /// Reserves independent exact-key slots, including old pending owners.
    pub(super) fn with_capacity_limit(slots: usize) -> Result<Self, NativeHostProtocolError> {
        Self::reservation_bytes(slots)
            .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
        let mut entries = Vec::new();
        entries
            .try_reserve_exact(slots)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        if entries.capacity() > slots {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        Ok(Self {
            entries,
            slot_limit: Some(slots),
        })
    }

    pub(super) fn reservation_bytes(slots: usize) -> Option<usize> {
        size_of::<Self>().checked_add(slots.checked_mul(size_of::<NativeWindowResource>())?)
    }

    /// Finite staging reserves both vectors once, so construction has no
    /// overlapping old request or wake allocation beyond these capacities.
    pub(super) fn staging_reservation_bytes(slots: usize) -> Option<usize> {
        let per_slot = size_of::<(ViewportId, NativeStagingPaintRequest)>()
            .checked_add(size_of::<ViewportId>())?;
        size_of::<PreparedWindowStaging>().checked_add(slots.checked_mul(per_slot)?)
    }

    /// Counts inline storage and actual retained entry backing capacity.
    /// Callback and staging receipts include their supported ingress payloads inline.
    pub(super) fn owned_data_capacity_bytes(&self) -> usize {
        size_of::<Self>().saturating_add(
            self.entries
                .capacity()
                .saturating_mul(size_of::<NativeWindowResource>()),
        )
    }

    pub(super) fn bindings(&self) -> impl Iterator<Item = NativeSurfaceBinding> + '_ {
        self.entries.iter().map(|resource| resource.binding)
    }

    pub(super) fn has_pending_work(&self) -> bool {
        self.entries
            .iter()
            .any(NativeWindowResource::has_pending_work)
    }

    pub(super) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.entries.iter().any(|resource| {
            resource.binding == binding || resource.callbacks.references_binding(binding)
        })
    }

    pub(super) fn references_viewport_binding(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.resource(viewport, binding)
            .is_some_and(|resource| !resource.retired)
    }

    /// Reserves only backing, allowing a successor while its predecessor is active.
    /// The caller must keep preparation, route mutation and publication serialized;
    /// no other insertion may consume the reserved spare before publication.
    pub(super) fn prepare_host_reference(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeHostProtocolError> {
        if self.resource(viewport, binding).is_none() {
            self.reserve_entries(1)?;
        }
        Ok(())
    }

    /// Pins prepaid backing only; the caller holds the host lock until publication.
    pub(super) fn prepare_workspace_replacement(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Result<(), NativeHostProtocolError> {
        self.prepare_host_reference(viewport, binding)?;
        if self.resource(viewport, binding).is_none() {
            self.insert_owner(viewport, binding);
        }
        self.resource_mut(viewport, binding)
            .expect("the prepaid successor owner exists")
            .workspace_replacement = WorkspaceReplacementResource::Prepared;
        Ok(())
    }

    /// Failed candidate preparation releases every speculative exact owner.
    pub(super) fn abort_workspace_replacement(&mut self) {
        self.entries.retain(|resource| {
            resource.workspace_replacement != WorkspaceReplacementResource::Prepared
        });
    }

    pub(super) fn retain_workspace_predecessor(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        self.resource_mut(viewport, binding)
            .expect("the attached predecessor retains its host resource")
            .workspace_replacement = WorkspaceReplacementResource::Predecessor;
    }

    pub(super) fn workspace_predecessor(
        &self,
        successor: NativeSurfaceBinding,
    ) -> Option<(ViewportId, NativeSurfaceBinding)> {
        self.entries.iter().find_map(|resource| {
            (resource.workspace_replacement == WorkspaceReplacementResource::Predecessor
                && resource.binding.surface() == successor.surface()
                && resource.binding.window_token() == successor.window_token())
            .then_some((resource.viewport, resource.binding))
        })
    }

    pub(super) fn finish_workspace_predecessor(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        let resource = self
            .resource_mut(viewport, binding)
            .expect("settlement retains its exact predecessor resource");
        debug_assert_eq!(
            resource.workspace_replacement,
            WorkspaceReplacementResource::Predecessor
        );
        resource.workspace_replacement = WorkspaceReplacementResource::None;
        self.prune(viewport, binding);
    }

    /// Pins a successfully published route or output using its prepared backing.
    /// The pin neither revives a retired owner nor retires another exact binding.
    pub(super) fn commit_host_reference(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) {
        if self.resource(viewport, binding).is_none() {
            assert!(
                self.can_add_entries(1) && self.entries.len() < self.entries.capacity(),
                "host references must reserve exact owner backing before publication"
            );
            self.insert_owner(viewport, binding);
        }
        let resource = self
            .resource_mut(viewport, binding)
            .expect("the exact prepared host owner is present");
        resource.retained_by_host = true;
        if resource.workspace_replacement == WorkspaceReplacementResource::Prepared {
            resource.workspace_replacement = WorkspaceReplacementResource::None;
        }
    }

    /// Recomputes backing retention from all current routes and retained outputs.
    /// Route replacement and unbinding must separately retire their old owners:
    /// an output-only pin does not identify the currently active route.
    pub(super) fn refresh_host_references(
        &mut self,
        mut retained: impl FnMut(ViewportId, NativeSurfaceBinding) -> bool,
    ) {
        self.entries.retain_mut(|resource| {
            resource.retained_by_host = retained(resource.viewport, resource.binding);
            !resource.is_empty()
        });
    }

    pub(super) fn presentation_ack(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<NativePresentationEffectAcknowledgement> {
        self.entries.iter().find_map(|resource| {
            (resource.binding == binding)
                .then_some(resource.presentation_ack)
                .flatten()
        })
    }

    /// An outer Some preserves Created even after its acknowledgement is consumed.
    pub(super) fn created_ack(
        &self,
        binding: NativeSurfaceBinding,
    ) -> Option<Option<NativeCreateEffectAcknowledgement>> {
        self.entries
            .iter()
            .find(|resource| resource.binding == binding && resource.created)
            .map(|resource| resource.create_ack)
    }

    /// The admitted create already prepaid this exact owner; no row is added here.
    pub(super) fn can_retain_create_ack(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.resource(viewport, binding).is_some() && self.created_ack(binding).is_none()
    }

    pub(super) fn retain_create_ack(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativeCreateEffectAcknowledgement,
    ) -> Result<(), NativeCreateEffectAcknowledgement> {
        if !self.can_retain_create_ack(viewport, binding) {
            return Err(acknowledgement);
        }
        let resource = self
            .resource_mut(viewport, binding)
            .expect("create ACK admission requires its exact owner");
        resource.created = true;
        resource.create_ack = Some(acknowledgement);
        Ok(())
    }

    pub(super) fn clear_create_ack(&mut self, binding: NativeSurfaceBinding) -> bool {
        let Some(resource) = self
            .entries
            .iter_mut()
            .find(|resource| resource.binding == binding && resource.create_ack.is_some())
        else {
            return false;
        };
        resource.create_ack = None;
        let viewport = resource.viewport;
        self.prune(viewport, binding);
        true
    }

    pub(super) fn create_ack_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|resource| resource.create_ack.is_some())
            .count()
    }

    /// Must succeed before taking the affine request that produces its ACK.
    /// Aliases on other viewports share the exact binding's single observation.
    pub(super) fn can_retain_presentation_ack(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.resource(viewport, binding).is_some() && self.presentation_ack(binding).is_none()
    }

    pub(super) fn retain_presentation_ack(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        acknowledgement: NativePresentationEffectAcknowledgement,
    ) -> Result<(), NativePresentationEffectAcknowledgement> {
        if !self.can_retain_presentation_ack(viewport, binding) {
            return Err(acknowledgement);
        }
        self.resource_mut(viewport, binding)
            .expect("presentation ACK admission requires its exact owner")
            .presentation_ack = Some(acknowledgement);
        Ok(())
    }

    pub(super) fn clear_presentation_ack(&mut self, binding: NativeSurfaceBinding) -> bool {
        let Some(resource) = self
            .entries
            .iter_mut()
            .find(|resource| resource.binding == binding && resource.presentation_ack.is_some())
        else {
            return false;
        };
        resource.presentation_ack = None;
        let viewport = resource.viewport;
        self.prune(viewport, binding);
        true
    }

    pub(super) fn presentation_ack_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|resource| resource.presentation_ack.is_some())
            .count()
    }

    pub(super) fn pending_no_frame(&self) -> impl Iterator<Item = &PendingNoFrameInvalidation> {
        self.entries
            .iter()
            .filter_map(|resource| resource.no_frame.as_ref())
    }

    /// Filters observations in place while the caller borrows routes and outputs
    /// under the same host lock. Surviving records retain their exact owner key.
    pub(super) fn retain_no_frame_records(
        &mut self,
        mut retain: impl FnMut(&mut PendingNoFrameInvalidation) -> bool,
    ) {
        self.entries.retain_mut(|resource| {
            if resource
                .no_frame
                .as_mut()
                .is_some_and(|record| !retain(record))
            {
                resource.no_frame = None;
            }
            !resource.is_empty()
        });
    }

    /// Retains the first record for an exact binding without allocating.
    /// Call before removing or abandoning the last output that pins its owner.
    pub(super) fn retain_no_frame(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        record: PendingNoFrameInvalidation,
    ) -> Result<(), PendingNoFrameInvalidation> {
        if record.viewport != viewport || record.binding != binding {
            return Err(record);
        }
        if self
            .entries
            .iter()
            .any(|resource| resource.binding == binding && resource.no_frame.is_some())
        {
            return Ok(());
        }
        let Some(resource) = self.resource_mut(viewport, binding) else {
            return Err(record);
        };
        resource.no_frame = Some(record);
        Ok(())
    }

    pub(super) fn clear_no_frame_if(&mut self, record: PendingNoFrameInvalidation) -> bool {
        let Some(resource) = self.entries.iter_mut().find(|resource| {
            resource.binding == record.binding && resource.no_frame == Some(record)
        }) else {
            return false;
        };
        resource.no_frame = None;
        let viewport = resource.viewport;
        self.prune(viewport, record.binding);
        true
    }

    fn resource(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<&NativeWindowResource> {
        self.entries
            .iter()
            .find(|resource| resource.viewport == viewport && resource.binding == binding)
    }

    fn resource_mut(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<&mut NativeWindowResource> {
        self.entries
            .iter_mut()
            .find(|resource| resource.viewport == viewport && resource.binding == binding)
    }

    fn active_resource(&self, viewport: ViewportId) -> Option<&NativeWindowResource> {
        self.entries
            .iter()
            .find(|resource| resource.viewport == viewport && !resource.retired)
    }

    fn can_activate(&self, viewport: ViewportId, binding: NativeSurfaceBinding) -> bool {
        !self
            .resource(viewport, binding)
            .is_some_and(|resource| resource.retired)
            && self
                .active_resource(viewport)
                .is_none_or(|resource| resource.binding == binding)
    }

    fn can_add_entries(&self, additional: usize) -> bool {
        self.entries
            .len()
            .checked_add(additional)
            .is_some_and(|len| self.slot_limit.is_none_or(|limit| len <= limit))
    }

    fn reserve_entries(&mut self, additional: usize) -> Result<(), NativeHostProtocolError> {
        if !self.can_add_entries(additional) {
            return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
        }
        self.entries
            .try_reserve_exact(additional)
            .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)
    }

    fn insert_owner(&mut self, viewport: ViewportId, binding: NativeSurfaceBinding) {
        debug_assert!(self.entries.len() < self.entries.capacity());
        self.entries.push(NativeWindowResource {
            viewport,
            binding,
            retired: false,
            retained_by_host: false,
            workspace_replacement: WorkspaceReplacementResource::None,
            create_rect: None,
            hidden_render: false,
            staging: None,
            callbacks: NativeCommandCallbacks::default(),
            created: false,
            create_ack: None,
            presentation_ack: None,
            no_frame: None,
        });
    }

    pub(super) fn create(&self, viewport: ViewportId) -> Option<NativeCreateReservation> {
        let resource = self.active_resource(viewport)?;
        Some(NativeCreateReservation {
            binding: resource.binding,
            rect: resource.create_rect?,
        })
    }

    pub(super) fn hidden_binding(&self, viewport: ViewportId) -> Option<NativeSurfaceBinding> {
        let resource = self.active_resource(viewport)?;
        resource.hidden_render.then_some(resource.binding)
    }

    pub(super) fn staging(&self, viewport: ViewportId) -> Option<NativeStagingPaintRequest> {
        let staging = self.active_resource(viewport)?.staging.as_ref()?;
        staging.is_awaiting().then_some(staging.request)
    }

    pub(super) fn awaiting_staging_count(&self) -> usize {
        self.entries
            .iter()
            .filter(|resource| {
                !resource.retired
                    && resource
                        .staging
                        .as_ref()
                        .is_some_and(StagingReceipt::is_awaiting)
            })
            .count()
    }

    pub(super) fn reserve_create(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        rect: NativePhysicalRect,
    ) -> bool {
        let Some(resource) = self.command_owner(viewport, binding) else {
            return false;
        };
        if let Some(current) = resource.create_rect {
            return current == rect && resource.hidden_render;
        }
        resource.create_rect = Some(rect);
        resource.hidden_render = true;
        true
    }

    pub(super) fn clear_create(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(resource) = self.resource_mut(viewport, binding) else {
            return false;
        };
        if resource.create_rect.take().is_none() {
            return false;
        }
        self.prune(viewport, binding);
        true
    }

    pub(super) fn clear_hidden(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        let Some(resource) = self.resource_mut(viewport, binding) else {
            return false;
        };
        if !resource.hidden_render {
            return false;
        }
        resource.hidden_render = false;
        self.prune(viewport, binding);
        true
    }

    #[cfg(test)]
    pub(super) fn can_publish_staging(
        &self,
        requests: &BTreeMap<ViewportId, NativeStagingPaintRequest>,
    ) -> bool {
        self.staging_missing_entries(requests)
            .is_some_and(|missing| self.can_add_entries(missing))
    }

    #[cfg(test)]
    fn staging_missing_entries(
        &self,
        requests: &BTreeMap<ViewportId, NativeStagingPaintRequest>,
    ) -> Option<usize> {
        let mut missing = 0;
        // BTreeMap already guarantees one request per viewport, hence unique exact keys.
        for (viewport, request) in requests {
            if !self.can_activate(*viewport, request.binding()) {
                return None;
            }
            if self.resource(*viewport, request.binding()).is_none() {
                missing += 1;
            }
        }
        Some(missing)
    }

    /// Reserves all construction backing without changing owners or staging receipts.
    /// Finite preparation pays its full slot limit before reading the iterator;
    /// duplicate viewports and excess keys reject without unbounded collection.
    /// Dropping the preparation leaves only the store's charged spare capacity.
    pub(super) fn prepare_staging(
        &mut self,
        requests: impl IntoIterator<Item = (ViewportId, NativeStagingPaintRequest)>,
    ) -> Result<PreparedWindowStaging, NativeHostProtocolError> {
        let mut prepared_requests = Vec::new();
        let mut wake = Vec::new();
        if let Some(limit) = self.slot_limit {
            Self::staging_reservation_bytes(limit)
                .ok_or(NativeHostProtocolError::NativeReportStorageUnavailable)?;
            prepared_requests
                .try_reserve_exact(limit)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            wake.try_reserve_exact(limit)
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            if prepared_requests.capacity() > limit || wake.capacity() > limit {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
        }
        let mut missing = 0;
        for (viewport, request) in requests {
            if !self.can_activate(viewport, request.binding())
                || prepared_requests
                    .iter()
                    .any(|(previous, _)| *previous == viewport)
            {
                return Err(NativeHostProtocolError::OutputRouteAttachmentFailed);
            }
            if self.resource(viewport, request.binding()).is_none() {
                missing += 1;
            }
            if !self.can_add_entries(missing) {
                return Err(NativeHostProtocolError::NativeReportStorageUnavailable);
            }
            if self.slot_limit.is_none() {
                prepared_requests
                    .try_reserve_exact(1)
                    .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
            }
            prepared_requests.push((viewport, request));
        }
        if self.slot_limit.is_none() {
            wake.try_reserve_exact(prepared_requests.len())
                .map_err(|_| NativeHostProtocolError::NativeReportStorageUnavailable)?;
        }
        self.reserve_entries(missing)?;
        Ok(PreparedWindowStaging {
            requests: prepared_requests,
            wake,
        })
    }

    /// Publishes a serialized preparation without allocating. Received and
    /// transferred staging keeps its exact basis until the full commit.
    pub(super) fn commit_staging(&mut self, prepared: PreparedWindowStaging) -> Vec<ViewportId> {
        let PreparedWindowStaging { requests, mut wake } = prepared;
        assert!(
            requests
                .iter()
                .all(|(viewport, request)| self.can_activate(*viewport, request.binding())),
            "prepared staging owners must remain compatible until publication"
        );
        let missing = requests
            .iter()
            .filter(|(viewport, request)| self.resource(*viewport, request.binding()).is_none())
            .count();
        assert!(
            self.can_add_entries(missing)
                && missing <= self.entries.capacity() - self.entries.len()
                && wake.capacity() >= requests.len(),
            "prepared staging backing must remain reserved until publication"
        );
        for (viewport, request) in requests {
            if self.resource(viewport, request.binding()).is_none() {
                self.insert_owner(viewport, request.binding());
            }
            let resource = self
                .resource_mut(viewport, request.binding())
                .expect("the exact prepared staging owner is present");
            match &mut resource.staging {
                Some(staging) if staging.request == request || !staging.is_awaiting() => {}
                Some(staging) => *staging = StagingReceipt::new(request),
                None => {
                    resource.staging = Some(StagingReceipt::new(request));
                    wake.push(viewport);
                }
            }
        }
        wake
    }

    /// Compatibility wrapper for immediate preparation and publication.
    #[cfg(test)]
    pub(super) fn publish_staging(
        &mut self,
        requests: BTreeMap<ViewportId, NativeStagingPaintRequest>,
    ) -> Option<Vec<ViewportId>> {
        let prepared = self.prepare_staging(requests).ok()?;
        Some(self.commit_staging(prepared))
    }

    /// Only the first exact paint captures ingress into the existing staging owner.
    pub(super) fn receive_staging(
        &mut self,
        record: NativeStagingPaintRecord,
        reserve: impl FnOnce() -> ReservedHostRecord,
    ) -> bool {
        let Some(resource) =
            self.resource_mut(record.token().viewport_id(), record.request().binding())
        else {
            return false;
        };
        if resource.retired {
            return false;
        }
        let Some(staging) = &mut resource.staging else {
            return false;
        };
        staging.receive(record.request(), reserve)
    }

    pub(super) fn clear_staging(&mut self) {
        self.entries.retain_mut(|resource| {
            if resource
                .staging
                .as_ref()
                .is_some_and(StagingReceipt::is_awaiting)
            {
                resource.staging = None;
            }
            !resource.is_empty()
        });
    }

    pub(super) fn retire(&mut self, viewport: ViewportId, binding: NativeSurfaceBinding) {
        let Some(resource) = self.resource_mut(viewport, binding) else {
            return;
        };
        resource.retired = true;
        resource.create_rect = None;
        resource.hidden_render = false;
        if resource
            .staging
            .as_ref()
            .is_some_and(StagingReceipt::is_awaiting)
        {
            resource.staging = None;
        }
        // Dispatched terminals and pending observations keep their original owner
        // through their respective full commits, including during quarantine.
        self.prune(viewport, binding);
    }

    fn command_owner(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> Option<&mut NativeWindowResource> {
        if !self.can_activate(viewport, binding) {
            return None;
        }
        if self.resource(viewport, binding).is_none() {
            self.reserve_entries(1).ok()?;
            self.insert_owner(viewport, binding);
        }
        self.resource_mut(viewport, binding)
    }

    pub(super) fn reserve_pointer_callback(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        token: NativeViewportPointerPassthroughCommandToken,
        enabled: bool,
    ) -> bool {
        if self.entries.iter().any(|owner| {
            (owner.viewport, owner.binding) != (viewport, binding)
                && owner.callbacks.has_pointer_token(token)
        }) {
            return false;
        }
        self.command_owner(viewport, binding)
            .is_some_and(|owner| owner.callbacks.reserve_pointer(token, window, enabled))
    }

    pub(super) fn accept_pointer_callback(
        &mut self,
        record: NativePointerPassthroughRecord,
        make_ingress: impl FnOnce(NativeSurfaceBinding) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let Some(owner) = self.entries.iter_mut().find(|owner| {
            owner.viewport == record.viewport() && owner.callbacks.matches_pointer(record)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        let binding = owner.binding;
        owner
            .callbacks
            .accept_pointer(record, || make_ingress(binding))
    }

    pub(super) fn reserve_focus_callback(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        token: NativeViewportFocusCommandToken,
    ) -> bool {
        if self.entries.iter().any(|owner| {
            (owner.viewport, owner.binding) != (viewport, binding)
                && owner.callbacks.has_focus_token(token)
        }) {
            return false;
        }
        self.command_owner(viewport, binding)
            .is_some_and(|owner| owner.callbacks.reserve_focus(token, window))
    }

    pub(super) fn accept_focus_callback(
        &mut self,
        record: NativeViewportFocusRecord,
        make_ingress: impl FnOnce(NativeSurfaceBinding) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let Some(owner) = self.entries.iter_mut().find(|owner| {
            owner.viewport == record.viewport() && owner.callbacks.matches_focus(record)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        let binding = owner.binding;
        owner
            .callbacks
            .accept_focus(record, || make_ingress(binding))
    }

    pub(super) fn reserve_visibility_callback(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        token: NativeViewportVisibilityCommandToken,
        visible: bool,
    ) -> bool {
        if self.entries.iter().any(|owner| {
            (owner.viewport, owner.binding) != (viewport, binding)
                && owner.callbacks.has_visibility_token(token)
        }) {
            return false;
        }
        self.command_owner(viewport, binding)
            .is_some_and(|owner| owner.callbacks.reserve_visibility(token, window, visible))
    }

    pub(super) fn accept_visibility_callback(
        &mut self,
        record: NativeViewportVisibilityRecord,
        make_ingress: impl FnOnce(NativeSurfaceBinding) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let Some(owner) = self.entries.iter_mut().find(|owner| {
            owner.viewport == record.viewport()
                && owner.callbacks.matches_visibility(
                    record.token(),
                    record.window(),
                    record.visible(),
                )
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        let binding = owner.binding;
        owner
            .callbacks
            .accept_visibility(record, || make_ingress(binding))
    }

    pub(super) fn visibility_callback_binding(
        &self,
        viewport: ViewportId,
        token: NativeViewportVisibilityCommandToken,
        window: WindowId,
        visible: bool,
    ) -> Option<NativeSurfaceBinding> {
        self.entries.iter().find_map(|owner| {
            (owner.viewport == viewport
                && owner.callbacks.matches_visibility(token, window, visible))
            .then_some(owner.binding)
        })
    }

    pub(super) fn retire_property_command(
        &mut self,
        retirement: NativeViewportPropertyCommandRetirement,
        make_ingress: impl FnOnce(HostRecord) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let viewport = match &retirement {
            NativeViewportPropertyCommandRetirement::PointerObservation { viewport_id, .. }
            | NativeViewportPropertyCommandRetirement::Focus { viewport_id, .. }
            | NativeViewportPropertyCommandRetirement::Visibility { viewport_id, .. }
            | NativeViewportPropertyCommandRetirement::PointerPassthrough { viewport_id, .. } => {
                *viewport_id
            }
        };
        let Some(owner) = self.entries.iter_mut().find(|owner| {
            owner.viewport == viewport && owner.callbacks.matches_property_retirement(&retirement)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        owner
            .callbacks
            .retire_property_command(viewport, owner.binding, retirement, make_ingress)
    }

    pub(super) fn reserve_observation(
        &mut self,
        key: NativePointerObservationKey,
        binding: NativeSurfaceBinding,
    ) -> bool {
        // There is one core manipulation and at most one outstanding sampler.
        // An obsolete command still occupies its original terminal receipt.
        if let Some(owner) = self
            .entries
            .iter()
            .find(|owner| owner.callbacks.has_observation_token(key.token))
        {
            return owner.viewport == key.viewport
                && owner.binding == binding
                && owner.callbacks.matches_observation(key);
        }
        if self
            .entries
            .iter()
            .any(|owner| owner.callbacks.has_pending_observation())
        {
            return false;
        }
        self.command_owner(key.viewport, binding)
            .is_some_and(|owner| owner.callbacks.reserve_observation(key))
    }

    pub(super) fn observation_pending(&self, key: NativePointerObservationKey) -> bool {
        self.entries
            .iter()
            .any(|owner| owner.callbacks.observation_pending(key))
    }

    pub(super) fn observation_record(
        &self,
        key: NativePointerObservationKey,
    ) -> Option<NativePointerObservationRecord> {
        self.entries
            .iter()
            .find_map(|owner| owner.callbacks.observation_record(key))
    }

    pub(super) fn accept_observation(
        &mut self,
        result: eframe::NativeViewportPointerObservationResult,
        viewports: &crate::viewport_map::NativeViewportMap,
        make_ingress: impl FnOnce(HostRecord) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let key = NativePointerObservationKey::from_result(result);
        let Some(owner) = self
            .entries
            .iter_mut()
            .find(|owner| owner.callbacks.matches_observation(key))
        else {
            return CommandCallbackAdmission::Unreserved;
        };
        let record = NativePointerObservationRecord::capture(result, owner.binding, viewports);
        owner
            .callbacks
            .accept_observation(record, || make_ingress(HostRecord::PointerObservation(key)))
    }

    #[cfg(test)]
    pub(super) fn receive_observation_record(
        &mut self,
        record: NativePointerObservationRecord,
        make_ingress: impl FnOnce(HostRecord) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let key = record.key();
        let Some(owner) = self.entries.iter_mut().find(|owner| {
            owner.binding == record.binding() && owner.callbacks.matches_observation(key)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        owner
            .callbacks
            .accept_observation(record, || make_ingress(HostRecord::PointerObservation(key)))
    }

    pub(super) fn detach_observation(
        &mut self,
        make_ingress: impl FnOnce(HostRecord) -> ReservedHostRecord,
    ) {
        let Some(owner) = self
            .entries
            .iter_mut()
            .find(|owner| owner.callbacks.awaiting_observation().is_some())
        else {
            return;
        };
        let key = owner
            .callbacks
            .awaiting_observation()
            .expect("the single pending observation was checked");
        owner.callbacks.accept_observation(
            NativePointerObservationRecord::retired(key, owner.binding),
            || make_ingress(HostRecord::PointerObservation(key)),
        );
    }

    pub(super) fn callback_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.entries.iter().flat_map(|owner| {
            owner.callbacks.callback_ingress().chain(
                owner
                    .staging
                    .iter()
                    .filter_map(|staging| staging.ingress.as_ref()),
            )
        })
    }

    pub(super) fn take_callback_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.entries.iter_mut().find_map(|owner| {
            owner.callbacks.take_callback_ingress(ordinal).or_else(|| {
                owner
                    .staging
                    .as_mut()
                    .and_then(|staging| staging.take_ingress(ordinal))
            })
        })
    }

    pub(super) fn commit_transferred_callbacks(&mut self) {
        for owner in &mut self.entries {
            owner.callbacks.commit_transferred();
            if owner
                .staging
                .as_ref()
                .is_some_and(|staging| staging.transferred)
            {
                owner.staging = None;
            }
        }
        self.entries.retain(|resource| !resource.is_empty());
    }

    fn prune(&mut self, viewport: ViewportId, binding: NativeSurfaceBinding) {
        if let Some(index) = self.entries.iter().position(|resource| {
            resource.viewport == viewport && resource.binding == binding && resource.is_empty()
        }) {
            self.entries.remove(index);
        }
    }
}

#[cfg(test)]
mod capacity_tests;

#[cfg(test)]
mod observation_tests;

#[cfg(test)]
mod tests {
    use dockspace::geometry::{LogicalRect, LogicalSize, PhysicalRect, ScaleFactor};
    use dockspace::model::{
        DockspaceContainedLayout, DockspaceLayout, DockspaceNode, DockspaceRootLayout,
        DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostFrameReport, HostInputOutcome, HostWindowToken, NativeCloseState,
        NativeEffectAcknowledgement, NativeEffectOperation, NativeMonotonicClock,
        NativePointerRoster, NativeReceiverAnswer, NativeWindowFacts, NativeWindowInputState,
        NativeWindowPlacement, NativeWindowPresentationState, NativeWorkAreaRoster,
        SurfacePresentationResult, SurfaceUnavailableReason, UniformSurfaceMetrics,
    };
    use eframe::{
        NativeViewportFocusStatus, NativeViewportPointerPassthroughStatus,
        NativeViewportVisibilityStatus, egui,
    };

    use super::super::{HostRecord, journal::HostIngressJournal};
    use super::*;

    fn commit_deferred(session: &mut DockspaceSession) -> HostFrameReport {
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        frame.commit().unwrap()
    }

    // Obtain two real bases from the public create lifecycle, without constructing
    // opaque staging requests or eframe output tokens in the adapter tests.
    pub(super) fn reissued_staging_requests() -> [NativeStagingPaintRequest; 2] {
        staging_observations().0
    }

    pub(super) fn staging_observations() -> (
        [NativeStagingPaintRequest; 2],
        NativeCreateEffectAcknowledgement,
        NativePresentationEffectAcknowledgement,
    ) {
        let surface = SurfaceId::new(1);
        let root = RootId::new(2);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )
        .with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(1),
            DockspaceRootLayout::new(root, DockspaceNode::central_tabs([ItemId::new(2)])),
            LogicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap(),
        ))])
        .unwrap();
        let mut policy = DockPolicy::default();
        policy.set_allow_native_surfaces(true);
        let mut session = DockspaceSession::from_layout(layout, policy).unwrap();
        session
            .enable_managed_native_host(
                dockspace::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .unwrap();
        session
            .configure_managed_native_capabilities(crate::capabilities::capabilities_for_backend(
                eframe::NativeWindowingBackend::Windows,
            ))
            .unwrap();
        session
            .register_native_root(surface, HostWindowToken::new(1))
            .unwrap();
        let registration = commit_deferred(&mut session);
        let binding = registration
            .inputs()
            .iter()
            .find_map(|input| match input {
                HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
                _ => None,
            })
            .unwrap();
        let bounds = PhysicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap();
        let scale = ScaleFactor::new(1.0).unwrap();
        let facts = NativeWindowFacts::live()
            .with_content_bounds(bounds)
            .with_outer_bounds(bounds)
            .with_native_scale_factor(scale)
            .with_presentation_scale_factor(scale)
            .with_input(NativeWindowInputState::ReceivesInput, None)
            .with_presentation(NativeWindowPresentationState::Visible, None)
            .with_close(NativeCloseState::Clear, None);
        session
            .report_managed_native_snapshot([(binding, facts)], NativeWorkAreaRoster::Unknown)
            .unwrap();
        commit_deferred(&mut session);
        let mut measurement = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        measurement
            .measure_surface(
                surface,
                UniformSurfaceMetrics::new(
                    LogicalRect::new(0.0, 0.0, 640.0, 480.0).unwrap(),
                    LogicalSize::new(32.0, 24.0).unwrap(),
                    80.0,
                )
                .unwrap(),
            )
            .unwrap();
        measurement.commit().unwrap();
        let mut paint = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let receipt = paint
            .paint_plan(surface)
            .unwrap()
            .unwrap()
            .execute_surface_program(|_| Ok::<(), ()>(()))
            .unwrap();
        paint.confirm_surface_painted(receipt).unwrap();
        let mut report = paint.commit().unwrap();
        let output = report.take_painted_outputs().pop().unwrap();
        session
            .report_surface_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        commit_deferred(&mut session);
        let placement = PhysicalRect::new(920.0, 120.0, 420.0, 320.0).unwrap();
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        frame
            .move_root_to_new_window_current(root, NativeWindowPlacement::new(placement))
            .unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        let create = report.take_native_effects().pop().unwrap();
        let child = match create.operation() {
            NativeEffectOperation::CreateWindow { binding, .. } => *binding,
            operation => panic!("unexpected native create effect: {operation:?}"),
        };
        let Some(NativeEffectAcknowledgement::Create(created)) = create.accepted() else {
            panic!("the create request must provide its create acknowledgement")
        };
        let mut acknowledgement = Some(created);
        let requests = std::array::from_fn(|_| {
            let child_facts = NativeWindowFacts::live()
                .with_content_bounds(placement)
                .with_outer_bounds(placement)
                .with_native_scale_factor(scale)
                .with_presentation_scale_factor(scale)
                .with_input(NativeWindowInputState::ReceivesInput, None)
                .with_presentation(NativeWindowPresentationState::Hidden, None)
                .with_create_readiness(
                    dockspace::runtime::NativeCreateReadiness::Created,
                    acknowledgement.take(),
                )
                .with_close(NativeCloseState::Clear, None);
            session
                .report_managed_native_snapshot(
                    [(binding, facts), (child, child_facts)],
                    NativeWorkAreaRoster::Unknown,
                )
                .unwrap();
            let mut frame = session
                .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
                .unwrap();
            let request = frame
                .native_staging_paints()
                .into_iter()
                .find(|request| request.binding() == child)
                .expect("the hidden child must request an exact staging paint");
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            frame.commit().unwrap();
            request
        });
        assert_ne!(requests[0], requests[1]);
        let mut frame = session
            .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
            .unwrap();
        let request = frame
            .native_staging_paints()
            .find(|request| request.binding() == child)
            .unwrap();
        frame.confirm_native_staging_painted(request).unwrap();
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .unwrap();
        let mut report = frame.commit().unwrap();
        let output = report.take_painted_native_staging_outputs().pop().unwrap();
        session
            .report_native_staging_presentation(output, SurfacePresentationResult::Presented)
            .unwrap();
        let mut report = commit_deferred(&mut session);
        let [show] = report.take_native_effects().try_into().unwrap();
        assert!(matches!(
            show.operation(),
            NativeEffectOperation::ShowWindow { binding } if *binding == child
        ));
        let Some(NativeEffectAcknowledgement::Presentation(shown)) = show.accepted() else {
            panic!("the show request must provide its presentation acknowledgement")
        };
        (requests, created, shown)
    }

    // The private receipt protocol only needs the journal envelope. Callback
    // target/token qualification remains in receive_staging and mailbox tests.
    fn staging_ingress(journal: &mut HostIngressJournal) -> ReservedHostRecord {
        journal.reserve(HostRecord::DeviceRemoval(
            crate::event::NativeDeviceRemovalRecord::for_test(1),
        ))
    }

    #[test]
    fn staging_receipt_only_mints_ingress_for_the_first_exact_basis() {
        let [first, newer] = reissued_staging_requests();
        let mut receipt = StagingReceipt::new(first);
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        assert!(!receipt.receive(newer, || panic!("a different basis cannot reserve ingress")));
        assert_eq!(journal.captured_through(), 0);
        assert!(receipt.receive(first, || staging_ingress(&mut journal)));
        let ordinal = receipt.ingress.as_ref().unwrap().ordinal();
        assert!(!receipt.is_awaiting());
        assert!(!receipt.receive(first, || panic!("a duplicate cannot reserve ingress")));
        assert!(!receipt.receive(newer, || panic!(
            "a different basis cannot overwrite ingress"
        )));
        assert!(receipt.take_ingress(ordinal + 1).is_none());
        let ingress = receipt.take_ingress(ordinal).unwrap();
        assert_eq!(ingress.ordinal(), ordinal);
        journal.transfer_reserved(ingress);
        assert_eq!(journal.captured_through(), ordinal);
        assert_eq!(journal.retained_record_count(), 1);
        assert!(receipt.transferred);
        assert!(!receipt.is_awaiting());
        assert!(receipt.take_ingress(ordinal).is_none());
        assert!(!receipt.receive(first, || panic!("handoff cannot reopen the receipt")));
    }

    #[test]
    fn staging_received_and_transferred_survive_clear_and_retirement_until_full_commit() {
        let [request, _] = reissued_staging_requests();
        let viewport = ViewportId::from_hash_of("staging-retirement");
        let binding = request.binding();
        for retire_before_transfer in [false, true] {
            let mut resources = NativeWindowResources::default();
            let empty = resources.owned_data_capacity_bytes();
            let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
            resources
                .publish_staging(BTreeMap::from([(viewport, request)]))
                .unwrap();
            let retained = resources.owned_data_capacity_bytes();
            assert!(retained > empty);
            resources.clear_staging();
            assert!(!resources.references_binding(binding));
            assert_eq!(resources.owned_data_capacity_bytes(), retained);
            resources
                .publish_staging(BTreeMap::from([(viewport, request)]))
                .unwrap();
            let receipt = resources
                .resource_mut(viewport, binding)
                .unwrap()
                .staging
                .as_mut()
                .unwrap();
            assert!(receipt.receive(request, || staging_ingress(&mut journal)));
            let ordinal = resources.callback_ingress().next().unwrap().ordinal();
            resources.clear_staging();
            if retire_before_transfer {
                resources.retire(viewport, binding);
            }
            resources.commit_transferred_callbacks();
            assert!(resources.references_binding(binding));
            assert!(resources.has_pending_work());
            assert!(resources.staging(viewport).is_none());
            assert_eq!(resources.callback_ingress().count(), 1);
            assert_eq!(resources.owned_data_capacity_bytes(), retained);
            let ingress = resources.take_callback_ingress(ordinal).unwrap();
            journal.transfer_reserved(ingress);
            resources.clear_staging();
            resources.retire(viewport, binding);
            assert!(resources.references_binding(binding));
            assert!(resources.has_pending_work());
            assert!(resources.callback_ingress().next().is_none());
            assert_eq!(resources.owned_data_capacity_bytes(), retained);
            assert!(resources.take_callback_ingress(ordinal).is_none());
            assert_eq!(journal.retained_record_count(), 1);
            resources.commit_transferred_callbacks();
            journal.commit_transferred();
            assert!(!resources.references_binding(binding));
            assert!(!resources.has_pending_work());
            assert_eq!(journal.retained_record_count(), 0);
            assert_eq!(resources.owned_data_capacity_bytes(), retained);
        }
    }

    #[test]
    fn staging_new_basis_waits_for_full_commit_then_reissues_without_a_second_slot() {
        let [first, newer] = reissued_staging_requests();
        let viewport = ViewportId::from_hash_of("staging-reissued-basis");
        let binding = first.binding();
        let mut resources = NativeWindowResources::default();
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        assert_eq!(
            resources.publish_staging(BTreeMap::from([(viewport, first)])),
            Some(vec![viewport]),
        );
        let receipt = resources
            .resource_mut(viewport, binding)
            .unwrap()
            .staging
            .as_mut()
            .unwrap();
        assert!(receipt.receive(first, || staging_ingress(&mut journal)));
        let ordinal = resources.callback_ingress().next().unwrap().ordinal();
        for request in [first, newer] {
            assert_eq!(
                resources.publish_staging(BTreeMap::from([(viewport, request)])),
                Some(Vec::new()),
            );
        }
        resources.commit_transferred_callbacks();
        assert_eq!(resources.callback_ingress().count(), 1);
        assert_eq!(
            resources.callback_ingress().next().unwrap().ordinal(),
            ordinal
        );
        assert!(resources.staging(viewport).is_none());
        let ingress = resources.take_callback_ingress(ordinal).unwrap();
        journal.transfer_reserved(ingress);
        assert_eq!(
            resources.publish_staging(BTreeMap::from([(viewport, newer)])),
            Some(Vec::new()),
        );
        assert!(resources.staging(viewport).is_none());
        assert_eq!(journal.captured_through(), ordinal);
        resources.commit_transferred_callbacks();
        journal.commit_transferred();
        assert_eq!(
            resources.publish_staging(BTreeMap::from([(viewport, newer)])),
            Some(vec![viewport]),
        );
        assert_eq!(resources.staging(viewport), Some(newer));
        let receipt = resources
            .resource_mut(viewport, binding)
            .unwrap()
            .staging
            .as_mut()
            .unwrap();
        assert!(!receipt.receive(first, || panic!("the old basis cannot claim its successor")));
        assert!(receipt.receive(newer, || staging_ingress(&mut journal)));
        assert_eq!(journal.captured_through(), ordinal + 1);
        assert_eq!(resources.callback_ingress().count(), 1);
        assert!(resources.staging(viewport).is_none());
    }

    pub(super) fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
        let layout = DockspaceLayout::new([1, 2].map(|id| {
            DockspaceSurfaceLayout::new(
                SurfaceId::new(id),
                DockspaceRootLayout::new(
                    RootId::new(id),
                    DockspaceNode::central_tabs([ItemId::new(id)]),
                ),
            )
        }))
        .expect("test layout validates");
        let mut session = DockspaceSession::from_layout(layout, DockPolicy::default())
            .expect("test session initializes");
        session
            .enable_managed_native_host(
                dockspace::runtime::NativeCreateStrategy::Staged,
                NativePointerRoster::Exact(Vec::new()),
            )
            .expect("native host enrolls");
        for id in [1, 2] {
            session
                .register_native_root(SurfaceId::new(id), HostWindowToken::new(id))
                .expect("native root registers");
        }
        let mut frame = session
            .begin_host_frame()
            .expect("registration frame begins");
        frame
            .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
            .expect("registration surfaces settle");
        let report = frame.commit().expect("registration frame commits");
        let mut bindings = report.inputs().iter().filter_map(|input| match input {
            HostInputOutcome::NativeSurfaceRegistered { binding } => Some(*binding),
            _ => None,
        });
        (bindings.next().unwrap(), bindings.next().unwrap())
    }

    #[test]
    fn owned_data_capacity_retains_backing_after_the_last_resource_clears() {
        let (first, second) = bindings();
        let viewport = ViewportId::ROOT;
        let rect = NativePhysicalRect::new(0, 0, 640, 480);
        let mut resources = NativeWindowResources::default();
        let empty = resources.owned_data_capacity_bytes();
        assert_eq!(empty, size_of::<NativeWindowResources>());
        assert!(resources.reserve_create(viewport, first, rect));
        let one = resources.owned_data_capacity_bytes();
        assert!(one > empty);
        let peer = ViewportId::from_hash_of("capacity-peer");
        assert!(resources.reserve_create(peer, second, rect));
        let two = resources.owned_data_capacity_bytes();
        assert!(two > one);
        assert!(resources.clear_create(viewport, first));
        assert!(resources.hidden_binding(viewport).is_some());
        assert_eq!(resources.owned_data_capacity_bytes(), two);
        assert!(resources.clear_hidden(viewport, first));
        assert_eq!(resources.owned_data_capacity_bytes(), two);
        resources.retire(peer, second);
        assert!(resources.entries.is_empty());
        assert_eq!(resources.owned_data_capacity_bytes(), two);
    }

    #[test]
    fn retired_received_callback_survives_new_active_owner_until_exact_handoff_and_commit() {
        let (first, next) = bindings();
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(11);
        let context = egui::Context::default();
        let first_token =
            eframe::queue_native_viewport_pointer_passthrough(&context, viewport, true);
        let next_token =
            eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
        let mut resources = NativeWindowResources::default();
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        let rect = NativePhysicalRect::new(0, 0, 640, 480);
        assert!(resources.reserve_create(viewport, first, rect));
        let one = resources.owned_data_capacity_bytes();
        assert!(resources.reserve_pointer_callback(viewport, first, window, first_token, true));
        assert_eq!(resources.owned_data_capacity_bytes(), one);
        let record = NativePointerPassthroughRecord::for_test(
            first_token,
            viewport,
            window,
            None,
            true,
            NativeViewportPointerPassthroughStatus::Applied,
        );
        assert_eq!(
            resources.accept_pointer_callback(record, |binding| {
                assert_eq!(binding, first);
                journal.reserve(HostRecord::ViewportPointerPassthrough(
                    record.qualified_for_binding(binding),
                ))
            }),
            CommandCallbackAdmission::First,
        );
        assert_eq!(
            resources.retire_property_command(
                NativeViewportPropertyCommandRetirement::PointerPassthrough {
                    viewport_id: viewport,
                    token: first_token,
                    enabled: true,
                },
                |_| panic!("retirement cannot replace an already received result"),
            ),
            CommandCallbackAdmission::Duplicate,
        );
        let ordinal = resources.callback_ingress().next().unwrap().ordinal();
        assert_eq!(resources.owned_data_capacity_bytes(), one);
        resources.retire(viewport, first);
        assert_eq!(resources.owned_data_capacity_bytes(), one);
        assert!(resources.create(viewport).is_none());
        assert!(resources.hidden_binding(viewport).is_none());
        assert!(resources.staging(viewport).is_none());
        assert!(!resources.references_viewport_binding(viewport, first));
        assert!(resources.references_binding(first));
        assert!(!resources.reserve_create(viewport, first, rect));
        assert!(resources.reserve_create(viewport, next, rect));
        let two = resources.owned_data_capacity_bytes();
        assert!(two > one);
        assert_eq!(resources.create(viewport).unwrap().binding, next);
        assert_eq!(resources.hidden_binding(viewport), Some(next));
        resources.retire(viewport, first);
        assert!(!resources.references_viewport_binding(viewport, first));
        assert!(resources.references_viewport_binding(viewport, next));
        assert_eq!(resources.create(viewport).unwrap().binding, next);
        assert_eq!(resources.hidden_binding(viewport), Some(next));
        assert!(!resources.reserve_pointer_callback(viewport, next, window, first_token, true));
        assert!(resources.reserve_pointer_callback(
            viewport,
            next,
            WindowId::from(12),
            next_token,
            false
        ));
        assert_eq!(
            resources
                .accept_pointer_callback(record, |_| panic!("a duplicate cannot reserve ingress")),
            CommandCallbackAdmission::Duplicate,
        );
        assert_eq!(journal.captured_through(), ordinal);
        resources.commit_transferred_callbacks();
        assert!(
            resources.references_binding(first),
            "a received callback is not committed by an unrelated boundary"
        );
        assert_eq!(resources.owned_data_capacity_bytes(), two);
        assert!(resources.take_callback_ingress(ordinal + 1).is_none());
        let ingress = resources
            .take_callback_ingress(ordinal)
            .expect("exact old-owner ingress transfers");
        assert!(
            matches!(ingress.record(), HostRecord::ViewportPointerPassthrough(record) if record.binding() == Some(first))
        );
        assert!(resources.take_callback_ingress(ordinal).is_none());
        assert!(
            resources.references_binding(first),
            "handoff alone does not release the retired owner"
        );
        assert_eq!(resources.owned_data_capacity_bytes(), two);
        resources.commit_transferred_callbacks();
        assert_eq!(resources.owned_data_capacity_bytes(), two);
        assert!(!resources.references_binding(first));
        assert!(resources.references_binding(next));
        assert_eq!(resources.create(viewport).unwrap().binding, next);
        assert_eq!(
            resources.accept_pointer_callback(record, |_| panic!(
                "a pruned owner cannot reserve ingress"
            )),
            CommandCallbackAdmission::Unreserved,
        );
    }

    #[test]
    fn retired_awaiting_callbacks_match_exact_keys_despite_missing_or_foreign_route_binding() {
        let (first, next) = bindings();
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(21);
        let context = egui::Context::default();
        let focus_token = eframe::queue_native_viewport_focus(&context, viewport);
        let visibility_token = eframe::queue_native_viewport_visibility(&context, viewport, true);
        let mut resources = NativeWindowResources::default();
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        assert!(resources.reserve_focus_callback(viewport, first, window, focus_token));
        assert!(resources.reserve_visibility_callback(
            viewport,
            first,
            window,
            visibility_token,
            true
        ));
        resources.retire(viewport, first);
        resources.commit_transferred_callbacks();
        assert!(
            resources.references_binding(first),
            "an awaiting terminal survives retirement and unrelated commits"
        );
        assert!(resources.reserve_create(viewport, next, NativePhysicalRect::new(0, 0, 640, 480)));
        assert_eq!(
            resources.visibility_callback_binding(viewport, visibility_token, window, true),
            Some(first)
        );
        assert_eq!(
            resources.visibility_callback_binding(
                viewport,
                visibility_token,
                WindowId::from(22),
                true
            ),
            None
        );
        assert_eq!(
            resources.visibility_callback_binding(viewport, visibility_token, window, false),
            None
        );
        let focus = NativeViewportFocusRecord::for_test(
            focus_token,
            viewport,
            window,
            None,
            NativeViewportFocusStatus::Requested,
        );
        assert_eq!(
            resources.accept_focus_callback(focus, |binding| {
                assert_eq!(binding, first);
                journal.reserve(HostRecord::ViewportFocus(
                    focus.qualified_for_binding(binding),
                ))
            }),
            CommandCallbackAdmission::First,
        );
        let visibility = NativeViewportVisibilityRecord::for_test(
            visibility_token,
            viewport,
            window,
            next,
            true,
            NativeViewportVisibilityStatus::Dispatched,
        );
        assert_eq!(
            resources.accept_visibility_callback(visibility, |binding| {
                assert_eq!(binding, first);
                journal.reserve(HostRecord::ViewportVisibility(
                    visibility.qualified_for_binding(binding),
                ))
            }),
            CommandCallbackAdmission::First,
        );
        assert_eq!(
            resources.visibility_callback_binding(viewport, visibility_token, window, true),
            Some(first)
        );
        assert_eq!(
            resources
                .accept_focus_callback(focus, |_| panic!("a duplicate cannot reserve ingress")),
            CommandCallbackAdmission::Duplicate,
        );
        assert_eq!(
            resources.accept_visibility_callback(visibility, |_| panic!(
                "a duplicate cannot reserve ingress"
            )),
            CommandCallbackAdmission::Duplicate,
        );
        assert_eq!(journal.captured_through(), 2);
        let ordinals = resources
            .callback_ingress()
            .map(|ingress| ingress.ordinal())
            .collect::<Vec<_>>();
        assert_eq!(ordinals.len(), 2);
        let focus_ingress = resources.take_callback_ingress(ordinals[0]).unwrap();
        assert!(
            matches!(focus_ingress.record(), HostRecord::ViewportFocus(record) if record.references_binding(first))
        );
        resources.commit_transferred_callbacks();
        assert!(
            resources.references_binding(first),
            "the other received terminal still retains its owner"
        );
        let visibility_ingress = resources.take_callback_ingress(ordinals[1]).unwrap();
        assert!(
            matches!(visibility_ingress.record(), HostRecord::ViewportVisibility(record) if record.binding() == first)
        );
        resources.commit_transferred_callbacks();
        assert!(!resources.references_binding(first));
        assert_eq!(resources.create(viewport).unwrap().binding, next);
    }

    #[test]
    fn queued_property_retirements_settle_existing_receipts_only_after_handoff_and_commit() {
        let (first, next) = bindings();
        let viewport = ViewportId::ROOT;
        let window = WindowId::from(31);
        let context = egui::Context::default();
        let focus = eframe::queue_native_viewport_focus(&context, viewport);
        let visibility = eframe::queue_native_viewport_visibility(&context, viewport, true);
        let pointer = eframe::queue_native_viewport_pointer_passthrough(&context, viewport, false);
        let mut resources = NativeWindowResources::default();
        let mut journal = HostIngressJournal::new(NativeMonotonicClock::system());
        assert!(resources.reserve_focus_callback(viewport, first, window, focus));
        assert!(resources.reserve_visibility_callback(viewport, first, window, visibility, true));
        assert!(resources.reserve_pointer_callback(viewport, first, window, pointer, false));
        resources.retire(viewport, first);
        resources.commit_transferred_callbacks();
        assert!(resources.references_binding(first));
        assert!(resources.reserve_create(viewport, next, NativePhysicalRect::new(0, 0, 640, 480)));

        for foreign in [
            NativeViewportPropertyCommandRetirement::PointerObservation {
                viewport_id: viewport,
                token: eframe::prepare_native_viewport_pointer_observation(
                    viewport,
                    window,
                    winit::event::DeviceId::dummy(),
                )
                .token(),
                expected_window_id: window,
                device_id: winit::event::DeviceId::dummy(),
            },
            NativeViewportPropertyCommandRetirement::Focus {
                viewport_id: ViewportId::from_hash_of("foreign-property-retirement"),
                token: focus,
            },
            NativeViewportPropertyCommandRetirement::Focus {
                viewport_id: viewport,
                token: eframe::queue_native_viewport_focus(&context, viewport),
            },
            NativeViewportPropertyCommandRetirement::Visibility {
                viewport_id: viewport,
                token: visibility,
                visible: false,
            },
            NativeViewportPropertyCommandRetirement::PointerPassthrough {
                viewport_id: viewport,
                token: pointer,
                enabled: true,
            },
        ] {
            assert_eq!(
                resources.retire_property_command(foreign, |_| panic!(
                    "an unmatched retirement cannot reserve ingress"
                )),
                CommandCallbackAdmission::Unreserved,
            );
        }
        assert_eq!(journal.captured_through(), 0);
        assert!(resources.callback_ingress().next().is_none());

        for retirement in [
            NativeViewportPropertyCommandRetirement::Focus {
                viewport_id: viewport,
                token: focus,
            },
            NativeViewportPropertyCommandRetirement::Visibility {
                viewport_id: viewport,
                token: visibility,
                visible: true,
            },
            NativeViewportPropertyCommandRetirement::PointerPassthrough {
                viewport_id: viewport,
                token: pointer,
                enabled: false,
            },
        ] {
            assert_eq!(
                resources.retire_property_command(retirement, |record| journal.reserve(record)),
                CommandCallbackAdmission::First,
            );
        }
        assert_eq!(journal.captured_through(), 3);
        assert_eq!(resources.callback_ingress().count(), 3);
        assert_eq!(
            resources.retire_property_command(
                NativeViewportPropertyCommandRetirement::PointerPassthrough {
                    viewport_id: viewport,
                    token: pointer,
                    enabled: false,
                },
                |_| panic!("a received retirement cannot reserve another ingress"),
            ),
            CommandCallbackAdmission::Duplicate,
        );
        resources.commit_transferred_callbacks();
        assert!(
            resources.references_binding(first),
            "received retirements still await handoff"
        );
        assert!(resources.take_callback_ingress(4).is_none());

        for (ordinal, retirement) in [
            (
                1,
                NativeViewportPropertyCommandRetirement::Focus {
                    viewport_id: viewport,
                    token: focus,
                },
            ),
            (
                2,
                NativeViewportPropertyCommandRetirement::Visibility {
                    viewport_id: viewport,
                    token: visibility,
                    visible: true,
                },
            ),
            (
                3,
                NativeViewportPropertyCommandRetirement::PointerPassthrough {
                    viewport_id: viewport,
                    token: pointer,
                    enabled: false,
                },
            ),
        ] {
            let ingress = resources
                .take_callback_ingress(ordinal)
                .expect("exact retirement ingress transfers");
            match ingress.record() {
                HostRecord::ViewportFocus(record) => assert_eq!(
                    *record,
                    NativeViewportFocusRecord::for_test(
                        focus,
                        viewport,
                        window,
                        Some(first),
                        NativeViewportFocusStatus::Retired,
                    ),
                ),
                HostRecord::ViewportVisibility(record) => assert_eq!(
                    *record,
                    NativeViewportVisibilityRecord::for_test(
                        visibility,
                        viewport,
                        window,
                        first,
                        true,
                        NativeViewportVisibilityStatus::Retired,
                    ),
                ),
                HostRecord::ViewportPointerPassthrough(record) => assert_eq!(
                    *record,
                    NativePointerPassthroughRecord::for_test(
                        pointer,
                        viewport,
                        window,
                        Some(first),
                        false,
                        NativeViewportPointerPassthroughStatus::Retired,
                    ),
                ),
                record => panic!("unexpected property retirement record: {record:?}"),
            }
            assert!(resources.take_callback_ingress(ordinal).is_none());
            assert_eq!(
                resources.retire_property_command(retirement, |_| panic!(
                    "a transferred retirement cannot reserve another ingress"
                )),
                CommandCallbackAdmission::Duplicate,
            );
        }
        assert!(
            resources.references_binding(first),
            "handoff alone is not full commit"
        );
        assert!(resources.callback_ingress().next().is_none());
        assert_eq!(journal.captured_through(), 3);
        resources.commit_transferred_callbacks();
        assert!(!resources.references_binding(first));
        assert!(resources.references_viewport_binding(viewport, next));
        assert_eq!(resources.create(viewport).unwrap().binding, next);
        assert_eq!(
            resources.retire_property_command(
                NativeViewportPropertyCommandRetirement::Focus {
                    viewport_id: viewport,
                    token: focus
                },
                |_| panic!("a pruned receipt cannot reserve ingress"),
            ),
            CommandCallbackAdmission::Unreserved,
        );
    }
}
