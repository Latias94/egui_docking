//! Explicit touch contacts retained independently of gesture ownership.

use std::collections::BTreeMap;

use egui::{Pos2, TouchDeviceId, TouchId, TouchPhase};

use super::{
    DockspaceInteractionError, PointerDeviceKind, PointerSample, SurfacePointerButton,
    SurfacePointerCancelReason, SurfacePointerEvent, SurfacePointerId,
};

#[derive(Clone, Copy)]
struct Contact {
    pointer: SurfacePointerId,
    position: Pos2,
}

#[derive(Clone)]
pub(super) struct TouchContacts {
    active: BTreeMap<(TouchDeviceId, TouchId), Contact>,
    last_pointer: u64,
}

impl Default for TouchContacts {
    fn default() -> Self {
        Self {
            active: BTreeMap::new(),
            // Pointer one belongs to generic, unclassified egui input.
            last_pointer: 1,
        }
    }
}

impl TouchContacts {
    pub(super) fn is_empty(&self) -> bool {
        self.active.is_empty()
    }

    pub(super) fn clear(&mut self) {
        self.active.clear();
    }

    pub(super) fn observe(
        &mut self,
        device: TouchDeviceId,
        id: TouchId,
        phase: TouchPhase,
        position: Pos2,
    ) -> Result<Option<PointerSample>, DockspaceInteractionError> {
        let key = (device, id);
        let (contact, event) = match phase {
            TouchPhase::Start => {
                if self.active.contains_key(&key) {
                    return Ok(None);
                }
                let next = self
                    .last_pointer
                    .checked_add(1)
                    .ok_or(DockspaceInteractionError::PointerIdentityExhausted)?;
                let contact = Contact {
                    pointer: SurfacePointerId::new(next),
                    position,
                };
                self.last_pointer = next;
                self.active.insert(key, contact);
                (
                    contact,
                    SurfacePointerEvent::ButtonPressed(SurfacePointerButton::Primary),
                )
            }
            TouchPhase::Move => {
                let Some(contact) = self.active.get_mut(&key) else {
                    return Ok(None);
                };
                contact.position = position;
                (*contact, SurfacePointerEvent::Moved)
            }
            TouchPhase::End | TouchPhase::Cancel => {
                let Some(mut contact) = self.active.remove(&key) else {
                    return Ok(None);
                };
                contact.position = position;
                let event = if phase == TouchPhase::End {
                    SurfacePointerEvent::ContactEnded(SurfacePointerButton::Primary)
                } else {
                    SurfacePointerEvent::StreamCancelled(
                        SurfacePointerCancelReason::ExplicitPlatformCancellation,
                    )
                };
                (contact, event)
            }
        };
        Ok(Some(sample(contact, event)))
    }

    pub(super) fn cancel_all(&mut self, reason: SurfacePointerCancelReason) -> Vec<PointerSample> {
        std::mem::take(&mut self.active)
            .into_values()
            .map(|contact| sample(contact, SurfacePointerEvent::StreamCancelled(reason)))
            .collect()
    }

    pub(super) fn continuation(
        &self,
        pointer: SurfacePointerId,
        event: SurfacePointerEvent,
    ) -> Option<PointerSample> {
        self.active
            .values()
            .find(|contact| contact.pointer == pointer)
            .copied()
            .map(|contact| sample(contact, event))
    }
}

fn sample(contact: Contact, event: SurfacePointerEvent) -> PointerSample {
    PointerSample {
        pointer: contact.pointer,
        device: PointerDeviceKind::Touch,
        event,
        position: Some(contact.position),
        magnetic_targeting: None,
    }
}
