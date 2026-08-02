//! Exact binding between egui widget receivers and core presentation regions.

use std::collections::HashMap;

use dockspace::presentation_hit::PresentationHitRegionKind;
use dockspace::presentation_observation::{
    HostFrameKey, HostPresentationOutput, PresentedSurfaceAuthority,
    SurfacePresentationOutputTicket,
};
use dockspace::retention::PresentationRetentionManifest;
use egui::{Context, Id, LayerId, PointerButton, Pos2, Rect, Response, Sense, ViewportId};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
struct ReceiverWidgetKey {
    id: Id,
    layer: LayerId,
}

/// Immutable receiver facts retained from one exact egui paint pass.
///
/// This snapshot intentionally contains only fields available on upstream
/// [`Response`]. Native integrations may translate richer backend observations
/// into this type at their bridge boundary without leaking those types into the
/// retained presentation store.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PaintReceiverFingerprint {
    id: Id,
    layer: LayerId,
    interact_rect: Rect,
    sense: Sense,
    enabled: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct ReceiverRegistration {
    receiver: PaintReceiverFingerprint,
    region: PresentationHitRegionKind,
    activity: PaintReceiverActivity,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PaintReceiverActivity {
    primary_down: bool,
    primary_clicked: bool,
    primary_drag_stopped: bool,
    contains_pointer: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct FrameworkHoverObservation {
    point_bits: [u32; 2],
    top_layer: Option<LayerId>,
}

/// Exact egui receiver evidence for one pointer lane in the current pass.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PaintReceiverEvidence {
    /// Exactly one docking receiver owns the requested lane.
    Exact(PaintReceiverFingerprint),
    /// No docking receiver reported the requested lane.
    Absent,
    /// More than one docking receiver reported the requested lane.
    Ambiguous,
}

/// Result of resolving one observed paint receiver against an exact retained generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaintReceiverLookup {
    /// The exact output, viewport, and widget-pass generation is not retained.
    GenerationUnavailable,
    /// The receiver belongs to one docking presentation region.
    Dock(PresentationHitRegionKind),
    /// The receiver was painted by another egui component in the retained generation.
    Foreign,
    /// The widget identity exists, but its immutable paint facts do not match.
    FingerprintMismatch,
}

impl ReceiverWidgetKey {
    const fn new(id: Id, layer: LayerId) -> Self {
        Self { id, layer }
    }
}

impl PaintReceiverFingerprint {
    /// Creates an immutable snapshot from exact facts observed during one paint pass.
    #[must_use]
    pub const fn new(
        id: Id,
        layer: LayerId,
        interact_rect: Rect,
        sense: Sense,
        enabled: bool,
    ) -> Self {
        Self {
            id,
            layer,
            interact_rect,
            sense,
            enabled,
        }
    }

    /// Captures the receiver facts exposed by an egui response.
    #[must_use]
    pub fn from_response(response: &Response) -> Self {
        Self::new(
            response.id,
            response.layer_id,
            response.interact_rect,
            response.sense,
            response.enabled(),
        )
    }

    /// Returns the egui widget identity.
    #[must_use]
    pub const fn id(self) -> Id {
        self.id
    }

    /// Returns the exact layer that received the widget interaction.
    #[must_use]
    pub const fn layer_id(self) -> LayerId {
        self.layer
    }

    /// Returns the interaction rectangle observed during the paint pass.
    #[must_use]
    pub const fn interact_rect(self) -> Rect {
        self.interact_rect
    }

    /// Returns the interaction sense observed during the paint pass.
    #[must_use]
    pub const fn sense(self) -> Sense {
        self.sense
    }

    /// Returns whether the receiver was enabled during the paint pass.
    #[must_use]
    pub const fn enabled(self) -> bool {
        self.enabled
    }

    const fn widget_key(self) -> ReceiverWidgetKey {
        ReceiverWidgetKey::new(self.id, self.layer)
    }
}

/// Receiver identities registered while painting one exact egui widget pass.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PaintReceiverRegistrations {
    viewport: ViewportId,
    widget_pass_nr: u64,
    by_widget: HashMap<ReceiverWidgetKey, ReceiverRegistration>,
    framework_hover: Option<FrameworkHoverObservation>,
    conflicted: bool,
}

impl PaintReceiverRegistrations {
    pub(crate) fn new(viewport: ViewportId, widget_pass_nr: u64) -> Self {
        Self {
            viewport,
            widget_pass_nr,
            by_widget: HashMap::new(),
            framework_hover: None,
            conflicted: false,
        }
    }

    pub(crate) fn capture_framework_hover(&mut self, context: &Context) {
        self.framework_hover = context
            .input(|input| input.pointer.hover_pos())
            .map(|point| FrameworkHoverObservation {
                point_bits: [point.x.to_bits(), point.y.to_bits()],
                top_layer: context.layer_id_at(point),
            });
    }

    pub(crate) fn register(&mut self, response: &Response, region: PresentationHitRegionKind) {
        let receiver = PaintReceiverFingerprint::from_response(response);
        let activity = PaintReceiverActivity {
            primary_down: response.is_pointer_button_down_on()
                && response
                    .ctx
                    .input(|input| input.pointer.button_down(PointerButton::Primary)),
            primary_clicked: response.clicked_by(PointerButton::Primary),
            primary_drag_stopped: response.drag_stopped_by(PointerButton::Primary),
            contains_pointer: response.contains_pointer(),
        };
        let widget = receiver.widget_key();
        if self.by_widget.get(&widget).is_some_and(|registered| {
            registered.region != region
                || registered.receiver != receiver
                || registered.activity != activity
        }) {
            self.conflicted = true;
            return;
        }
        self.by_widget.insert(
            widget,
            ReceiverRegistration {
                receiver,
                region,
                activity,
            },
        );
    }

    pub(crate) const fn is_conflicted(&self) -> bool {
        self.conflicted
    }

    pub(crate) const fn viewport(&self) -> ViewportId {
        self.viewport
    }

    pub(crate) const fn widget_pass_nr(&self) -> u64 {
        self.widget_pass_nr
    }

    pub(crate) fn primary_press_receiver(&self, drag_lane: bool) -> PaintReceiverEvidence {
        self.unique_receiver(|registration| {
            registration.activity.primary_down
                && if drag_lane {
                    registration.receiver.sense.senses_drag()
                } else {
                    registration.receiver.sense.senses_click()
                }
        })
    }

    pub(crate) fn primary_press_receivers(
        &self,
        drag_lane: bool,
    ) -> impl Iterator<Item = PaintReceiverFingerprint> + '_ {
        self.by_widget
            .values()
            .filter(move |registration| {
                registration.activity.primary_down
                    && if drag_lane {
                        registration.receiver.sense.senses_drag()
                    } else {
                        registration.receiver.sense.senses_click()
                    }
            })
            .map(|registration| registration.receiver)
    }

    pub(crate) fn primary_release_receivers(
        &self,
        drag_lane: bool,
    ) -> impl Iterator<Item = PaintReceiverFingerprint> + '_ {
        self.by_widget
            .values()
            .filter(move |registration| {
                if drag_lane {
                    registration.activity.primary_drag_stopped
                } else {
                    registration.activity.primary_clicked
                }
            })
            .map(|registration| registration.receiver)
    }

    pub(crate) fn hover_receivers(&self) -> impl Iterator<Item = PaintReceiverFingerprint> + '_ {
        self.by_widget
            .values()
            .filter(|registration| registration.activity.contains_pointer)
            .map(|registration| registration.receiver)
    }

    pub(crate) fn framework_top_layer_at(&self, point: Pos2) -> Option<Option<LayerId>> {
        self.framework_hover
            .filter(|observation| observation.point_bits == [point.x.to_bits(), point.y.to_bits()])
            .map(|observation| observation.top_layer)
    }

    pub(crate) fn owns_layer(&self, layer: LayerId) -> bool {
        self.by_widget.keys().any(|widget| widget.layer == layer)
    }

    fn unique_receiver(
        &self,
        predicate: impl Fn(&ReceiverRegistration) -> bool,
    ) -> PaintReceiverEvidence {
        let mut matches = self
            .by_widget
            .values()
            .filter(|registration| predicate(registration))
            .map(|registration| registration.receiver);
        let Some(receiver) = matches.next() else {
            return PaintReceiverEvidence::Absent;
        };
        if matches.next().is_some() {
            PaintReceiverEvidence::Ambiguous
        } else {
            PaintReceiverEvidence::Exact(receiver)
        }
    }

    fn region_for(&self, receiver: PaintReceiverFingerprint) -> PaintReceiverLookup {
        match self.by_widget.get(&receiver.widget_key()) {
            Some(registration) if registration.receiver == receiver => {
                PaintReceiverLookup::Dock(registration.region)
            }
            Some(_) => PaintReceiverLookup::FingerprintMismatch,
            None => PaintReceiverLookup::Foreign,
        }
    }
}

/// A widget registry bound to the exact semantic output that was actually painted.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PaintReceiverPresentation {
    emission: HostFrameKey,
    output: SurfacePresentationOutputTicket,
    registrations: PaintReceiverRegistrations,
}

/// Retained receiver resources for outputs that may still own interaction authority.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct PaintReceiverStore {
    presentations: Vec<PaintReceiverPresentation>,
}

impl PaintReceiverStore {
    pub(crate) fn bind(
        &mut self,
        output: HostPresentationOutput,
        registrations: PaintReceiverRegistrations,
    ) {
        let Some(scene) = output.payload().scene() else {
            return;
        };
        self.presentations.retain(|presentation| {
            presentation.emission != output.key()
                || presentation.registrations.viewport != registrations.viewport
        });
        self.presentations.push(PaintReceiverPresentation {
            emission: output.key(),
            output: scene,
            registrations,
        });
    }

    pub(crate) fn retain(&mut self, manifest: &PresentationRetentionManifest) {
        self.presentations
            .retain(|presentation| manifest.retains(presentation.emission));
    }

    pub(crate) fn region_for(
        &self,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
        viewport: ViewportId,
        widget_pass_nr: u64,
        receiver: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        if !authority.matches_output(output) {
            return PaintReceiverLookup::GenerationUnavailable;
        }
        Self::lookup_in_generation(
            self.presentations
                .iter()
                .find(|presentation| {
                    presentation.emission == authority.emission()
                        && presentation.output == output
                        && presentation.registrations.viewport == viewport
                        && presentation.registrations.widget_pass_nr == widget_pass_nr
                })
                .map(|presentation| &presentation.registrations),
            receiver,
        )
    }

    pub(crate) fn region_for_latest_presented_pass(
        &self,
        output: SurfacePresentationOutputTicket,
        authority: PresentedSurfaceAuthority,
        viewport: ViewportId,
        before_widget_pass_nr: u64,
        receiver: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        if !authority.matches_output(output) {
            return PaintReceiverLookup::GenerationUnavailable;
        }
        Self::lookup_in_generation(
            self.presentations
                .iter()
                .filter(|presentation| {
                    presentation.emission == authority.emission()
                        && presentation.output == output
                        && presentation.registrations.viewport == viewport
                        && presentation.registrations.widget_pass_nr < before_widget_pass_nr
                })
                .max_by_key(|presentation| presentation.registrations.widget_pass_nr)
                .map(|presentation| &presentation.registrations),
            receiver,
        )
    }

    pub(crate) fn region_for_emission(
        &self,
        output: SurfacePresentationOutputTicket,
        emission: HostFrameKey,
        viewport: ViewportId,
        widget_pass_nr: u64,
        receiver: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        Self::lookup_in_generation(
            self.presentations
                .iter()
                .find(|presentation| {
                    presentation.emission == emission
                        && presentation.output == output
                        && presentation.registrations.viewport == viewport
                        && presentation.registrations.widget_pass_nr == widget_pass_nr
                })
                .map(|presentation| &presentation.registrations),
            receiver,
        )
    }

    fn lookup_in_generation(
        registrations: Option<&PaintReceiverRegistrations>,
        receiver: PaintReceiverFingerprint,
    ) -> PaintReceiverLookup {
        registrations.map_or(
            PaintReceiverLookup::GenerationUnavailable,
            |registrations| registrations.region_for(receiver),
        )
    }

    #[cfg(test)]
    pub(crate) const fn presentation_count(&self) -> usize {
        self.presentations.len()
    }

    #[cfg(test)]
    pub(crate) fn first_registration_for(
        &self,
        authority: PresentedSurfaceAuthority,
    ) -> Option<(ViewportId, u64, PaintReceiverFingerprint)> {
        let presentation = self
            .presentations
            .iter()
            .find(|presentation| presentation.emission == authority.emission())?;
        let receiver = presentation
            .registrations
            .by_widget
            .values()
            .next()?
            .receiver;
        Some((
            presentation.registrations.viewport,
            presentation.registrations.widget_pass_nr,
            receiver,
        ))
    }
}

#[cfg(test)]
mod tests {
    use dockspace::drop_guide::DropGuideClusterId;
    use dockspace::ids::{RootId, SurfaceId};
    use egui::{Context, Order, Pos2, RawInput, Sense, vec2};

    use super::*;

    const ROOT: RootId = RootId::new(1);
    const SURFACE: SurfaceId = SurfaceId::new(2);

    fn region() -> PresentationHitRegionKind {
        PresentationHitRegionKind::DropGuideActivation(DropGuideClusterId::outer(SURFACE, ROOT))
    }

    fn registered_response() -> Response {
        let context = Context::default();
        let mut response = None;
        let _ = context.run_ui(RawInput::default(), |ui| {
            response = Some(ui.allocate_response(vec2(80.0, 24.0), Sense::click_and_drag()));
        });
        response.expect("fixture response is painted")
    }

    #[test]
    fn exact_fingerprint_resolves_to_registered_dock_region() {
        let response = registered_response();
        let mut registrations = PaintReceiverRegistrations::new(ViewportId::ROOT, 7);
        registrations.register(&response, region());

        assert_eq!(
            registrations.region_for(PaintReceiverFingerprint::from_response(&response)),
            PaintReceiverLookup::Dock(region())
        );
    }

    #[test]
    fn public_fingerprint_accessors_preserve_exact_paint_facts() {
        let response = registered_response();
        let fingerprint = PaintReceiverFingerprint::from_response(&response);

        assert_eq!(fingerprint.id(), response.id);
        assert_eq!(fingerprint.layer_id(), response.layer_id);
        assert_eq!(fingerprint.interact_rect(), response.interact_rect);
        assert_eq!(fingerprint.sense(), response.sense);
        assert_eq!(fingerprint.enabled(), response.enabled());
    }

    #[test]
    fn matching_identity_with_changed_receiver_facts_is_not_authoritative() {
        let response = registered_response();
        let mut registrations = PaintReceiverRegistrations::new(ViewportId::ROOT, 7);
        registrations.register(&response, region());
        let changed = PaintReceiverFingerprint::new(
            response.id,
            response.layer_id,
            response.interact_rect.translate(vec2(1.0, 0.0)),
            response.sense,
            response.enabled(),
        );

        assert_eq!(
            registrations.region_for(changed),
            PaintReceiverLookup::FingerprintMismatch
        );
    }

    #[test]
    fn unregistered_identity_is_foreign_even_on_an_owned_layer() {
        let response = registered_response();
        let mut registrations = PaintReceiverRegistrations::new(ViewportId::ROOT, 7);
        registrations.register(&response, region());
        let foreign = PaintReceiverFingerprint::new(
            Id::new("foreign receiver"),
            response.layer_id,
            response.interact_rect,
            response.sense,
            response.enabled(),
        );

        assert_eq!(
            registrations.region_for(foreign),
            PaintReceiverLookup::Foreign
        );
    }

    #[test]
    fn layer_ownership_is_derived_only_from_registered_snapshots() {
        let response = registered_response();
        let mut registrations = PaintReceiverRegistrations::new(ViewportId::ROOT, 7);
        registrations.register(&response, region());

        assert!(registrations.owns_layer(response.layer_id));
        assert!(!registrations.owns_layer(LayerId::new(Order::Tooltip, Id::new("foreign layer"))));
    }

    #[test]
    fn duplicate_identity_with_different_fingerprint_conflicts() {
        let response = registered_response();
        let mut registrations = PaintReceiverRegistrations::new(ViewportId::ROOT, 7);
        registrations.register(&response, region());
        let mut changed = response.clone();
        changed.interact_rect = Rect::from_min_size(Pos2::new(2.0, 3.0), vec2(40.0, 10.0));
        registrations.register(&changed, region());

        assert!(registrations.is_conflicted());
    }

    #[test]
    fn missing_retained_generation_is_not_conflated_with_a_foreign_receiver() {
        let response = registered_response();

        assert_eq!(
            PaintReceiverStore::lookup_in_generation(
                None,
                PaintReceiverFingerprint::from_response(&response),
            ),
            PaintReceiverLookup::GenerationUnavailable
        );
    }
}
