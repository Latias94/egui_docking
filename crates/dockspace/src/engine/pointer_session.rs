//! Pointer ownership gates and interaction-session retirement.

mod source;

use super::*;

impl DockEngine {
    fn native_capture_binding_is_current(
        &self,
        binding: crate::platform::viewport::WindowBinding,
    ) -> bool {
        self.viewport
            .registry()
            .window(binding)
            .is_some_and(|window| {
                window.ever_observed()
                    && !matches!(
                        window.lifecycle(),
                        crate::platform::registry::ViewportLifecycle::Destroyed
                            | crate::platform::registry::ViewportLifecycle::Missing
                            | crate::platform::registry::ViewportLifecycle::AwaitingDestroyed
                    )
            })
    }

    fn active_journal_required_capture_owner(
        &self,
        owner: GestureOwner,
    ) -> Result<PointerCaptureOwner, EngineError> {
        let stream = owner.stream();
        if self.interaction.active_owner() != Some(owner) {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal capture authority was requested for an inactive stream",
            });
        }
        match stream.lease().scope() {
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_) => {
                Ok(PointerCaptureOwner::ProviderEndpoint)
            }
            PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_) => self
                .interaction
                .active_presentation_authority()
                .and_then(|presentation| presentation.presented.binding())
                .map(|binding| PointerCaptureOwner::Native(binding.window()))
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "native journal gesture has no press-bound native capture owner",
                }),
        }
    }

    fn active_journal_required_delivery_owner(
        &self,
        owner: GestureOwner,
    ) -> Result<PointerEventDeliveryOwner, EngineError> {
        let stream = owner.stream();
        if self.interaction.active_owner() != Some(owner) {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal delivery authority was requested for an inactive stream",
            });
        }
        match stream.lease().scope() {
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_) => {
                Ok(PointerEventDeliveryOwner::ProviderEndpoint)
            }
            PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_) => self
                .interaction
                .active_presentation_authority()
                .and_then(|presentation| presentation.presented.binding())
                .map(|binding| PointerEventDeliveryOwner::Native(binding.window()))
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "native journal gesture has no press-bound native delivery owner",
                }),
        }
    }

    fn journal_delivery_action_gate(
        &self,
        owner: GestureOwner,
        observed: Authority<PointerEventDeliveryOwner>,
    ) -> Result<JournalCaptureActionGate, EngineError> {
        let required = self.active_journal_required_delivery_owner(owner)?;
        let Authority::Known(observed) = observed else {
            return Ok(JournalCaptureActionGate::Unavailable);
        };
        if observed != required {
            return Ok(JournalCaptureActionGate::Lost);
        }
        if let PointerEventDeliveryOwner::Native(binding) = observed
            && !self.native_capture_binding_is_current(binding)
        {
            return Ok(JournalCaptureActionGate::Lost);
        }
        Ok(JournalCaptureActionGate::Authorized)
    }

    pub(super) fn gate_journal_delivery_action(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        observed: Authority<PointerEventDeliveryOwner>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(bool, Option<InteractionOutcome>), EngineError> {
        match self.journal_delivery_action_gate(owner, observed)? {
            JournalCaptureActionGate::Authorized => Ok((true, None)),
            JournalCaptureActionGate::Unavailable => Ok((
                false,
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::DeliveryAuthorityUnavailable,
                    interaction_events,
                )?,
            )),
            JournalCaptureActionGate::Lost => Ok((
                false,
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::DeliveryOwnerLost,
                    interaction_events,
                )?,
            )),
        }
    }

    pub(super) fn journal_capture_action_gate(
        &self,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        release_edge: bool,
    ) -> Result<JournalCaptureActionGate, EngineError> {
        let stream = owner.stream();
        if matches!(
            stream.lease().scope(),
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_)
        ) {
            // The immutable local lease already confines delivery to one
            // endpoint. `None` means that endpoint delivered without platform
            // capture; only `Foreign` or unavailable authority blocks it.
            return Ok(match observed {
                Authority::Unknown(_) => JournalCaptureActionGate::Unavailable,
                Authority::Known(
                    PointerCaptureOwner::ProviderEndpoint | PointerCaptureOwner::None,
                ) => JournalCaptureActionGate::Authorized,
                Authority::Known(PointerCaptureOwner::Foreign | PointerCaptureOwner::Native(_)) => {
                    JournalCaptureActionGate::Lost
                }
            });
        }
        let required = self.active_journal_required_capture_owner(owner)?;
        let Authority::Known(observed) = observed else {
            // A native provider may lack persistent capture authority while
            // the native event source still proves exact edge delivery. The
            // caller validates that independent delivery fact first.
            return Ok(JournalCaptureActionGate::Authorized);
        };
        if release_edge && observed == PointerCaptureOwner::None {
            // Release carries post-edge capture authority. Relinquishing an
            // exact pre-edge owner is valid. A joined native backend can also
            // deliver the matching release to that same current source after an
            // unobservable capture sample; delivery was checked independently.
            let current = self.interaction.active_journal_capture_authority().ok_or(
                EngineError::ReductionCauseInvariant {
                    detail: "journal release has no pre-edge capture observation",
                },
            )?;
            return Ok(match current {
                Authority::Known(current)
                    if current == required
                        && !matches!(current, PointerCaptureOwner::Native(binding)
                            if !self.native_capture_binding_is_current(binding)) =>
                {
                    JournalCaptureActionGate::Authorized
                }
                Authority::Unknown(_)
                    if matches!(
                        stream.lease().scope(),
                        PointerProviderScope::NativeJoined(_)
                    ) && matches!(required, PointerCaptureOwner::Native(binding)
                            if self.native_capture_binding_is_current(binding)) =>
                {
                    JournalCaptureActionGate::Authorized
                }
                Authority::Unknown(_) => JournalCaptureActionGate::Unavailable,
                Authority::Known(_) => JournalCaptureActionGate::Lost,
            });
        }
        if observed != required {
            return Ok(JournalCaptureActionGate::Lost);
        }
        if let PointerCaptureOwner::Native(binding) = observed
            && !self.native_capture_binding_is_current(binding)
        {
            return Ok(JournalCaptureActionGate::Lost);
        }
        Ok(JournalCaptureActionGate::Authorized)
    }

    pub(super) fn gate_journal_capture_action(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        release_edge: bool,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(bool, Option<InteractionOutcome>), EngineError> {
        let gate = self.journal_capture_action_gate(owner, observed, release_edge)?;
        if !release_edge
            || !matches!(
                observed,
                Authority::Known(PointerCaptureOwner::None) | Authority::Unknown(_)
            )
        {
            let stream = owner.stream();
            if !matches!(gate, JournalCaptureActionGate::Lost)
                && !self
                    .interaction
                    .update_journal_capture_authority(stream, observed)
            {
                return Err(EngineError::ReductionCauseInvariant {
                    detail: "journal capture action lost its active gesture",
                });
            }
        }
        match gate {
            JournalCaptureActionGate::Authorized => Ok((true, None)),
            JournalCaptureActionGate::Unavailable => Ok((false, None)),
            JournalCaptureActionGate::Lost => Ok((
                false,
                self.cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::CaptureLost,
                    interaction_events,
                )?,
            )),
        }
    }

    pub(super) fn reduce_journal_capture_change(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Vec<InteractionOutcome>, EngineError> {
        let stream = owner.stream();
        if self.interaction.active_owner() != Some(owner) {
            return Ok(Vec::new());
        }
        let required = self.active_journal_required_capture_owner(owner)?;
        let capture_lost = match observed {
            Authority::Unknown(_) => false,
            Authority::Known(actual) => {
                actual != required
                    || matches!(actual, PointerCaptureOwner::Native(binding)
                        if !self.native_capture_binding_is_current(binding))
            }
        };
        if capture_lost {
            return Ok(self
                .cancel_gesture_owner(
                    cause,
                    owner,
                    InteractionCancelReason::CaptureLost,
                    interaction_events,
                )?
                .into_iter()
                .collect());
        }
        if matches!(owner.lease().scope(), PointerProviderScope::NativeJoined(_))
            && matches!(observed, Authority::Unknown(_))
        {
            self.clear_pointer_continuation(owner);
        }
        if !self
            .interaction
            .update_journal_capture_authority(stream, observed)
        {
            return Err(EngineError::ReductionCauseInvariant {
                detail: "journal capture observation lost its active gesture",
            });
        }
        Ok(Vec::new())
    }

    pub(super) fn journal_press_delivery_rejection(
        &self,
        observed: Authority<PointerEventDeliveryOwner>,
        expected: PointerEventDeliveryOwner,
    ) -> Result<Option<InteractionRejection>, EngineError> {
        let Authority::Known(actual) = observed else {
            return Ok(Some(InteractionRejection::DeliveryAuthorityUnavailable));
        };
        let compatible = match expected {
            PointerEventDeliveryOwner::Native(binding) => {
                actual == expected && self.native_capture_binding_is_current(binding)
            }
            PointerEventDeliveryOwner::ProviderEndpoint
            | PointerEventDeliveryOwner::Foreign
            | PointerEventDeliveryOwner::None => actual == expected,
        };
        Ok((!compatible)
            .then_some(InteractionRejection::DeliveryOwnerMismatch { expected, actual }))
    }

    pub(super) fn journal_press_capture_rejection(
        &self,
        owner: GestureOwner,
        observed: Authority<PointerCaptureOwner>,
        presentation: &JournalSurfacePresentation,
    ) -> Result<Option<InteractionRejection>, EngineError> {
        let stream = owner.stream();
        let Authority::Known(actual) = observed else {
            return Ok(None);
        };

        let (expected, compatible) = match stream.lease().scope() {
            PointerProviderScope::HostLocal(_) | PointerProviderScope::SurfaceLocal(_) => (
                PointerCaptureOwner::ProviderEndpoint,
                matches!(
                    actual,
                    PointerCaptureOwner::ProviderEndpoint | PointerCaptureOwner::None
                ),
            ),
            PointerProviderScope::DesktopGlobal | PointerProviderScope::NativeJoined(_) => {
                let Some(binding) = presentation.authority().binding() else {
                    return Ok(Some(InteractionRejection::TargetAuthorityInvalid));
                };
                let expected = PointerCaptureOwner::Native(binding.window());
                (
                    expected,
                    actual == expected && self.native_capture_binding_is_current(binding.window()),
                )
            }
        };
        Ok(
            (!compatible)
                .then_some(InteractionRejection::CaptureOwnerMismatch { expected, actual }),
        )
    }

    /// Cancels an exact journal or local-response owner through one terminal path.
    pub(super) fn cancel_gesture_owner(
        &mut self,
        cause: ReductionCause,
        owner: GestureOwner,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<Option<InteractionOutcome>, EngineError> {
        if self.interaction.active_owner() != Some(owner) {
            return Ok(None);
        }
        if owner.is_desktop() {
            self.viewport
                .end_drag_routing(owner.pointer())
                .map_err(|source| EngineError::Viewport {
                    input: self.last_input,
                    source,
                })?;
        }
        let status =
            self.interaction
                .cancel_owner(owner)
                .ok_or(EngineError::ReductionCauseInvariant {
                    detail: "active gesture could not cancel its exact owner",
                })?;
        interaction_events.push(InteractionEvent::new_caused(
            cause,
            self.version,
            InteractionEventKind::Cancelled { status, reason },
        ));
        Ok(Some(InteractionOutcome::Cancelled { status, reason }))
    }

    pub(super) fn cancel_retired_pointer_owner(
        &mut self,
        cause: ReductionCause,
        provider: PointerInputLease,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<(), EngineError> {
        for (session, receiver) in self.scroll_interaction.retire_provider(provider) {
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::ScrollTerminated {
                    session,
                    receiver,
                    reason: ScrollTerminationReason::ProviderRetired,
                },
            ));
        }
        self.cancel_pending_release_for_provider(cause, provider, reason, interaction_events);
        let Some(owner) = self
            .interaction
            .active_owner()
            .filter(|owner| owner.lease() == provider)
        else {
            return Ok(());
        };
        if owner.is_desktop() {
            self.viewport
                .end_all_drag_routing()
                .map_err(|source| EngineError::Viewport {
                    input: self.last_input,
                    source,
                })?;
        }
        if let Some(status) = self.interaction.cancel_owner(owner) {
            interaction_events.push(InteractionEvent::new_caused(
                cause,
                self.version,
                InteractionEventKind::Cancelled { status, reason },
            ));
        }
        Ok(())
    }
}

impl DockEngine {
    pub(super) fn cancel_drag(
        &mut self,
        input: InputSequence,
        session: crate::interaction::DragSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> Result<InteractionOutcome, EngineError> {
        let pointer = self
            .interaction
            .active_drag(session)
            .ok()
            .and_then(|drag| Some(drag.owner.pointer()));
        match self.interaction.cancel_drag(session) {
            Ok(status) => {
                if let Some(pointer) = pointer {
                    let _ = self
                        .viewport
                        .end_drag_routing(pointer)
                        .map_err(|source| EngineError::Viewport { input, source })?;
                }
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                Ok(InteractionOutcome::Cancelled { status, reason })
            }
            Err(error) => Ok(InteractionOutcome::Rejected(error)),
        }
    }

    pub(super) fn cancel_click(
        &mut self,
        input: InputSequence,
        session: ClickSessionId,
        reason: InteractionCancelReason,
        interaction_events: &mut Vec<InteractionEvent>,
    ) -> InteractionOutcome {
        match self.interaction.cancel_click(session) {
            Ok(status) => {
                interaction_events.push(InteractionEvent::new(
                    input,
                    self.version,
                    InteractionEventKind::Cancelled { status, reason },
                ));
                InteractionOutcome::Cancelled { status, reason }
            }
            Err(error) => InteractionOutcome::Rejected(error),
        }
    }
}

impl DockEngine {
    pub(super) fn freeze_payload_focus(
        &self,
        payload: &MovePayload,
    ) -> Result<PaneFocusDisposition, crate::graph::error::CommandError> {
        self.validate_payload(payload)?;
        let surface =
            self.payload_surface(payload)
                .ok_or(crate::graph::error::CommandError::Invariant {
                    stage: "freezing pane focus for an unpresented payload",
                })?;
        let record = self.viewport_focus.panel_focus(surface);
        let PanelFocusRecord::Item(item) = record else {
            return Ok(PaneFocusDisposition::from_record(record));
        };
        let payload_contains_item = match payload {
            MovePayload::Item(source) => source.item() == item,
            MovePayload::Tabs(source) | MovePayload::Subtree(source) => self
                .workspace
                .collect_items_in_subtree(source.node())
                .contains(&item),
        };
        if payload_contains_item {
            Ok(PaneFocusDisposition::Set(item))
        } else {
            Ok(PaneFocusDisposition::Clear)
        }
    }
}
