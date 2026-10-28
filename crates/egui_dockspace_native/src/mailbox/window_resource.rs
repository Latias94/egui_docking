//! One exact binding owns deferred creation, staging and command callbacks.

use std::collections::BTreeMap;

use dockspace::runtime::{NativeStagingPaintRequest, NativeSurfaceBinding};
use eframe::{
    NativePhysicalRect, NativeViewportFocusCommandToken,
    NativeViewportPointerPassthroughCommandToken, NativeViewportPropertyCommandRetirement,
    NativeViewportVisibilityCommandToken, egui::ViewportId,
};
use winit::window::WindowId;

use super::command_callback::{CommandCallbackAdmission, NativeCommandCallbacks};
use super::journal::ReservedHostRecord;
use super::{HostRecord, NativeCreateReservation};
use crate::focus_control::NativeViewportFocusRecord;
use crate::input_control::NativePointerPassthroughRecord;
use crate::viewport_callback::NativeViewportVisibilityRecord;

#[derive(Debug)]
struct NativeWindowResource {
    binding: NativeSurfaceBinding,
    retired: bool,
    create_rect: Option<NativePhysicalRect>,
    hidden_render: bool,
    staging: Option<NativeStagingPaintRequest>,
    callbacks: NativeCommandCallbacks,
}

impl NativeWindowResource {
    fn is_empty(&self) -> bool {
        self.create_rect.is_none()
            && !self.hidden_render
            && self.staging.is_none()
            && if self.retired {
                !self.callbacks.has_pending_work()
            } else {
                self.callbacks.is_empty()
            }
    }

    fn has_pending_work(&self) -> bool {
        self.create_rect.is_some()
            || self.hidden_render
            || self.staging.is_some()
            || self.callbacks.has_pending_work()
    }
}

#[derive(Debug, Default)]
pub(super) struct NativeWindowResources {
    entries: BTreeMap<(ViewportId, NativeSurfaceBinding), NativeWindowResource>,
}

impl NativeWindowResources {
    pub(super) fn has_pending_work(&self) -> bool {
        self.entries
            .values()
            .any(NativeWindowResource::has_pending_work)
    }

    pub(super) fn references_binding(&self, binding: NativeSurfaceBinding) -> bool {
        self.entries
            .values()
            .any(|resource| resource.binding == binding)
    }

    pub(super) fn references_viewport_binding(
        &self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
    ) -> bool {
        self.entries
            .get(&(viewport, binding))
            .is_some_and(|resource| !resource.retired)
    }

    fn active_resource(&self, viewport: ViewportId) -> Option<&NativeWindowResource> {
        self.entries.iter().find_map(|((candidate, _), resource)| {
            (*candidate == viewport && !resource.retired).then_some(resource)
        })
    }

    fn can_activate(&self, viewport: ViewportId, binding: NativeSurfaceBinding) -> bool {
        !self
            .entries
            .get(&(viewport, binding))
            .is_some_and(|resource| resource.retired)
            && self
                .active_resource(viewport)
                .is_none_or(|resource| resource.binding == binding)
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
        self.active_resource(viewport)?.staging
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
        let Some(resource) = self.entries.get_mut(&(viewport, binding)) else {
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
        let Some(resource) = self.entries.get_mut(&(viewport, binding)) else {
            return false;
        };
        if !resource.hidden_render {
            return false;
        }
        resource.hidden_render = false;
        self.prune(viewport, binding);
        true
    }

    pub(super) fn can_publish_staging(
        &self,
        requests: &BTreeMap<ViewportId, NativeStagingPaintRequest>,
    ) -> bool {
        requests
            .iter()
            .all(|(viewport, request)| self.can_activate(*viewport, request.binding()))
    }

    /// Validates every binding before mutating any slot.
    pub(super) fn publish_staging(
        &mut self,
        requests: BTreeMap<ViewportId, NativeStagingPaintRequest>,
        in_flight: &[NativeStagingPaintRequest],
    ) -> Option<Vec<ViewportId>> {
        if !self.can_publish_staging(&requests) {
            return None;
        }
        let mut wake = Vec::new();
        for (viewport, request) in requests {
            if in_flight.contains(&request) {
                continue;
            }
            let resource = self
                .command_owner(viewport, request.binding())
                .expect("all staging owners were validated before mutation");
            if resource.staging.replace(request).is_none() {
                wake.push(viewport);
            }
        }
        Some(wake)
    }

    pub(super) fn take_staging(
        &mut self,
        viewport: ViewportId,
        expected: NativeStagingPaintRequest,
    ) -> bool {
        let Some(resource) = self.entries.get_mut(&(viewport, expected.binding())) else {
            return false;
        };
        if resource.staging != Some(expected) {
            return false;
        }
        resource.staging = None;
        self.prune(viewport, expected.binding());
        true
    }

    pub(super) fn clear_staging(&mut self) {
        self.entries.retain(|_, resource| {
            resource.staging = None;
            !resource.is_empty()
        });
    }

    pub(super) fn retire(&mut self, viewport: ViewportId, binding: NativeSurfaceBinding) {
        let Some(resource) = self.entries.get_mut(&(viewport, binding)) else {
            return;
        };
        resource.retired = true;
        resource.create_rect = None;
        resource.hidden_render = false;
        resource.staging = None;
        // An already dispatched terminal keeps its original owner through full commit.
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
        let resource =
            self.entries
                .entry((viewport, binding))
                .or_insert_with(|| NativeWindowResource {
                    binding,
                    retired: false,
                    create_rect: None,
                    hidden_render: false,
                    staging: None,
                    callbacks: NativeCommandCallbacks::default(),
                });
        Some(resource)
    }

    pub(super) fn reserve_pointer_callback(
        &mut self,
        viewport: ViewportId,
        binding: NativeSurfaceBinding,
        window: WindowId,
        token: NativeViewportPointerPassthroughCommandToken,
        enabled: bool,
    ) -> bool {
        if self.entries.iter().any(|(key, owner)| {
            *key != (viewport, binding) && owner.callbacks.has_pointer_token(token)
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
        let Some((key, owner)) = self.entries.iter_mut().find(|(key, owner)| {
            key.0 == record.viewport() && owner.callbacks.matches_pointer(record)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        let binding = key.1;
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
        if self.entries.iter().any(|(key, owner)| {
            *key != (viewport, binding) && owner.callbacks.has_focus_token(token)
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
        let Some((key, owner)) = self.entries.iter_mut().find(|(key, owner)| {
            key.0 == record.viewport() && owner.callbacks.matches_focus(record)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        let binding = key.1;
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
        if self.entries.iter().any(|(key, owner)| {
            *key != (viewport, binding) && owner.callbacks.has_visibility_token(token)
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
        let Some((key, owner)) = self.entries.iter_mut().find(|(key, owner)| {
            key.0 == record.viewport()
                && owner.callbacks.matches_visibility(
                    record.token(),
                    record.window(),
                    record.visible(),
                )
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        let binding = key.1;
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
        self.entries
            .iter()
            .find_map(|((candidate, binding), owner)| {
                (*candidate == viewport
                    && owner.callbacks.matches_visibility(token, window, visible))
                .then_some(*binding)
            })
    }

    pub(super) fn retire_property_command(
        &mut self,
        retirement: NativeViewportPropertyCommandRetirement,
        make_ingress: impl FnOnce(HostRecord) -> ReservedHostRecord,
    ) -> CommandCallbackAdmission {
        let viewport = match &retirement {
            NativeViewportPropertyCommandRetirement::Focus { viewport_id, .. }
            | NativeViewportPropertyCommandRetirement::Visibility { viewport_id, .. }
            | NativeViewportPropertyCommandRetirement::PointerPassthrough { viewport_id, .. } => {
                *viewport_id
            }
        };
        let Some((key, owner)) = self.entries.iter_mut().find(|(key, owner)| {
            key.0 == viewport && owner.callbacks.matches_property_retirement(&retirement)
        }) else {
            return CommandCallbackAdmission::Unreserved;
        };
        owner
            .callbacks
            .retire_property_command(viewport, key.1, retirement, make_ingress)
    }

    pub(super) fn callback_ingress(&self) -> impl Iterator<Item = &ReservedHostRecord> {
        self.entries
            .values()
            .flat_map(|owner| owner.callbacks.callback_ingress())
    }

    pub(super) fn take_callback_ingress(&mut self, ordinal: u64) -> Option<ReservedHostRecord> {
        self.entries
            .values_mut()
            .find_map(|owner| owner.callbacks.take_callback_ingress(ordinal))
    }

    pub(super) fn commit_transferred_callbacks(&mut self) {
        for owner in self.entries.values_mut() {
            owner.callbacks.commit_transferred();
        }
        self.entries.retain(|_, resource| !resource.is_empty());
    }

    fn prune(&mut self, viewport: ViewportId, binding: NativeSurfaceBinding) {
        if self
            .entries
            .get(&(viewport, binding))
            .is_some_and(NativeWindowResource::is_empty)
        {
            self.entries.remove(&(viewport, binding));
        }
    }
}

#[cfg(test)]
mod tests {
    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use dockspace::policy::DockPolicy;
    use dockspace::runtime::{
        DockspaceSession, HostInputOutcome, HostWindowToken, NativeMonotonicClock,
        NativePointerRoster, SurfaceUnavailableReason,
    };
    use eframe::{
        NativeViewportFocusStatus, NativeViewportPointerPassthroughStatus,
        NativeViewportVisibilityStatus, egui,
    };

    use super::super::{HostRecord, journal::HostIngressJournal};
    use super::*;

    fn bindings() -> (NativeSurfaceBinding, NativeSurfaceBinding) {
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
            .enable_managed_native_host(NativePointerRoster::Exact(Vec::new()))
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
        assert!(resources.reserve_pointer_callback(viewport, first, window, first_token, true));
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
        resources.retire(viewport, first);
        assert!(resources.create(viewport).is_none());
        assert!(resources.hidden_binding(viewport).is_none());
        assert!(resources.staging(viewport).is_none());
        assert!(!resources.references_viewport_binding(viewport, first));
        assert!(resources.references_binding(first));
        assert!(!resources.reserve_create(viewport, first, rect));
        assert!(resources.reserve_create(viewport, next, rect));
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
        resources.commit_transferred_callbacks();
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
