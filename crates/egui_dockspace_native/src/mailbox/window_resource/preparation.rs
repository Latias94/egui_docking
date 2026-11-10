//! Eframe's renderer preparation belongs to the exact physical window owner.

use dockspace::runtime::{NativeCreateReadiness, NativeSurfaceBinding};
use eframe::{
    NativeViewportVisibilityCommandToken, NativeViewportVisibilityStatus, NativeWindowSnapshot,
};
use winit::window::WindowId;

use crate::viewport_callback::NativeViewportVisibilityRecord;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum NativePreparationPaintPhase {
    PreShow,
    PostShow,
}

/// A private renderer request. Its original output slot owns the terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePreparationPaint {
    binding: NativeSurfaceBinding,
    window: WindowId,
    generation: u64,
    phase: NativePreparationPaintPhase,
}

impl NativePreparationPaint {
    pub(crate) const fn binding(self) -> NativeSurfaceBinding {
        self.binding
    }
    pub(crate) const fn phase(self) -> NativePreparationPaintPhase {
        self.phase
    }
}

/// Settled with the existing complete snapshot, never in a separate queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct NativePreparationObservation {
    pub(crate) generation: u64,
    pub(crate) readiness: NativeCreateReadiness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Geometry {
    size: [u32; 2],
    inner: Option<[i64; 4]>,
    outer: Option<[i64; 4]>,
    native_scale: u64,
    presentation_scale: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct WindowFacts {
    geometry: Geometry,
    visible: Option<bool>,
}

impl WindowFacts {
    fn capture(snapshot: NativeWindowSnapshot) -> Option<Self> {
        let size = snapshot.content_size();
        let native_scale = snapshot.native_scale_factor();
        let presentation_scale = snapshot.presentation_scale_factor();
        if size.contains(&0)
            || !native_scale.is_finite()
            || native_scale <= 0.0
            || !presentation_scale.is_finite()
            || presentation_scale <= 0.0
            || snapshot
                .inner_rect()
                .is_some_and(|rect| [rect.width(), rect.height()] != size)
        {
            return None;
        }
        let rect = |rect: eframe::NativePhysicalRect| {
            [
                i64::from(rect.x()),
                i64::from(rect.y()),
                i64::from(rect.width()),
                i64::from(rect.height()),
            ]
        };
        Some(Self {
            geometry: Geometry {
                size,
                inner: snapshot.inner_rect().map(rect),
                outer: snapshot.outer_rect().map(rect),
                native_scale: native_scale.to_bits(),
                presentation_scale: presentation_scale.to_bits(),
            },
            visible: match (snapshot.visible(), snapshot.minimized()) {
                (Some(visible), Some(false) | None) => Some(visible),
                _ => None,
            },
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitingObservation,
    AwaitingPaint(NativePreparationPaintPhase),
    Painting(NativePreparationPaintPhase),
    ShowQueued,
    AwaitingShow(NativeViewportVisibilityCommandToken),
    AwaitingVisible { after: u64 },
    Ready,
    Failed,
    Cancelled,
}

#[derive(Debug)]
pub(super) struct WindowPreparation {
    window: WindowId,
    generation: u64,
    phase: Phase,
    facts: Option<WindowFacts>,
    last_observation: u64,
    shown: bool,
    published_ready: bool,
    revocation_pending: bool,
}

impl WindowPreparation {
    pub(super) const fn new(window: WindowId) -> Self {
        Self {
            window,
            generation: 1,
            phase: Phase::AwaitingObservation,
            facts: None,
            last_observation: 0,
            shown: false,
            published_ready: false,
            revocation_pending: false,
        }
    }

    fn invalidate(&mut self) -> bool {
        self.revocation_pending |= self.published_ready;
        self.published_ready = false;
        let Some(generation) = self.generation.checked_add(1) else {
            // Exact callback and output reservations remain in their original
            // owners; exhaustion only stops this owner from granting readiness.
            self.phase = Phase::Failed;
            return false;
        };
        self.generation = generation;
        true
    }

    fn restart(&mut self) {
        if matches!(
            self.phase,
            Phase::Cancelled
                | Phase::Failed
                | Phase::AwaitingShow(_)
                | Phase::AwaitingVisible { .. }
        ) {
            return;
        }
        self.phase = match self.facts.map(|facts| facts.visible) {
            Some(Some(false)) => Phase::AwaitingPaint(NativePreparationPaintPhase::PreShow),
            Some(Some(true)) if self.shown => {
                Phase::AwaitingPaint(NativePreparationPaintPhase::PostShow)
            }
            _ => Phase::AwaitingObservation,
        };
    }

    pub(super) fn observe(&mut self, snapshot: Option<NativeWindowSnapshot>, ordinal: u64) {
        self.observe_facts(snapshot.and_then(WindowFacts::capture), ordinal);
    }

    fn observe_facts(&mut self, facts: Option<WindowFacts>, ordinal: u64) {
        if ordinal <= self.last_observation {
            return;
        }
        self.last_observation = ordinal;
        if self.facts != facts {
            if !self.invalidate() {
                return;
            }
            self.facts = facts;
            if !matches!(
                self.phase,
                Phase::AwaitingShow(_)
                    | Phase::AwaitingVisible { .. }
                    | Phase::Cancelled
                    | Phase::Failed
            ) {
                self.phase = Phase::AwaitingObservation;
            }
        }
        if let Phase::AwaitingVisible { after } = self.phase {
            if ordinal > after && facts.is_some_and(|facts| facts.visible == Some(true)) {
                self.shown = true;
                self.phase = Phase::AwaitingPaint(NativePreparationPaintPhase::PostShow);
            }
        } else if self.phase == Phase::AwaitingObservation {
            self.restart();
        }
    }

    /// A newer output may revoke an old roster basis, but cannot grant readiness.
    pub(super) fn validate_output(&mut self, snapshot: NativeWindowSnapshot) -> bool {
        if self.facts == WindowFacts::capture(snapshot) && self.facts.is_some() {
            return true;
        }
        if !matches!(self.phase, Phase::Cancelled | Phase::Failed) {
            if !self.invalidate() {
                return false;
            }
            self.facts = None;
            if !matches!(
                self.phase,
                Phase::AwaitingShow(_) | Phase::AwaitingVisible { .. }
            ) {
                self.phase = Phase::AwaitingObservation;
            }
        }
        false
    }

    pub(super) fn paint(&self, binding: NativeSurfaceBinding) -> Option<NativePreparationPaint> {
        let Phase::AwaitingPaint(phase) = self.phase else {
            return None;
        };
        Some(NativePreparationPaint {
            binding,
            window: self.window,
            generation: self.generation,
            phase,
        })
    }

    pub(super) fn begin_paint(&mut self, request: NativePreparationPaint) -> bool {
        if self.paint(request.binding) != Some(request) {
            return false;
        }
        self.phase = Phase::Painting(request.phase);
        true
    }

    pub(super) fn finish_paint(
        &mut self,
        request: NativePreparationPaint,
        presented: bool,
    ) -> bool {
        if request.window != self.window
            || request.generation != self.generation
            || self.phase != Phase::Painting(request.phase)
        {
            return false;
        }
        if !presented {
            self.renderer_lost();
        } else {
            self.phase = match request.phase {
                NativePreparationPaintPhase::PreShow => Phase::ShowQueued,
                NativePreparationPaintPhase::PostShow => {
                    self.published_ready = true;
                    Phase::Ready
                }
            };
        }
        true
    }

    pub(super) fn renderer_lost(&mut self) {
        if matches!(self.phase, Phase::Cancelled | Phase::Failed) {
            return;
        }
        if !self.invalidate() {
            return;
        }
        if !matches!(
            self.phase,
            Phase::AwaitingShow(_) | Phase::AwaitingVisible { .. }
        ) {
            self.phase = Phase::AwaitingObservation;
        }
        self.restart();
    }

    pub(super) fn inventory_unknown(&mut self) {
        if matches!(self.phase, Phase::Cancelled | Phase::Failed) || self.facts.is_none() {
            return;
        }
        if !self.invalidate() {
            return;
        }
        self.facts = None;
        if !matches!(
            self.phase,
            Phase::AwaitingShow(_) | Phase::AwaitingVisible { .. }
        ) {
            self.phase = Phase::AwaitingObservation;
        }
    }

    pub(super) fn show_window(&self) -> Option<WindowId> {
        (self.phase == Phase::ShowQueued).then_some(self.window)
    }

    pub(super) fn dispatch_show(&mut self, token: NativeViewportVisibilityCommandToken) -> bool {
        if self.show_window().is_none() {
            return false;
        }
        self.phase = Phase::AwaitingShow(token);
        true
    }

    pub(super) fn visibility(
        &mut self,
        record: NativeViewportVisibilityRecord,
        ordinal: u64,
    ) -> bool {
        if self.phase != Phase::AwaitingShow(record.token())
            || record.window() != self.window
            || !record.visible()
        {
            return false;
        }
        self.phase = match record.status() {
            NativeViewportVisibilityStatus::Dispatched => Phase::AwaitingVisible { after: ordinal },
            NativeViewportVisibilityStatus::Unsupported
            | NativeViewportVisibilityStatus::Retired => Phase::Failed,
        };
        true
    }

    pub(super) fn observation(&self) -> NativePreparationObservation {
        let readiness = match self.phase {
            Phase::Ready if !self.revocation_pending => NativeCreateReadiness::Prepared,
            Phase::Failed => NativeCreateReadiness::Failed,
            _ => NativeCreateReadiness::Created,
        };
        NativePreparationObservation {
            generation: self.generation,
            readiness,
        }
    }

    pub(super) fn settle_observation(&mut self, observation: NativePreparationObservation) -> bool {
        if observation.generation != self.generation
            || !self.revocation_pending
            || observation.readiness == NativeCreateReadiness::Prepared
        {
            return false;
        }
        self.revocation_pending = false;
        true
    }

    pub(super) fn is_ready(&self) -> bool {
        self.phase == Phase::Ready && !self.revocation_pending
    }
    pub(super) fn is_pending(&self) -> bool {
        !matches!(self.phase, Phase::Ready | Phase::Failed | Phase::Cancelled)
            || self.revocation_pending
    }

    pub(super) fn cancel(&mut self) -> bool {
        if matches!(self.phase, Phase::Cancelled | Phase::Failed) {
            return false;
        }
        if self.invalidate() {
            self.phase = Phase::Cancelled;
        }
        true
    }
}

#[cfg(test)]
impl WindowPreparation {
    pub(super) fn ready_for_test(
        viewport: eframe::egui::ViewportId,
        window: WindowId,
        binding: NativeSurfaceBinding,
    ) -> Self {
        let facts = |visible| WindowFacts {
            geometry: Geometry {
                size: [640, 480],
                inner: Some([1400, 0, 640, 480]),
                outer: Some([1392, -30, 656, 518]),
                native_scale: 1.0_f64.to_bits(),
                presentation_scale: 1.0_f64.to_bits(),
            },
            visible: Some(visible),
        };
        let mut preparation = Self::new(window);
        preparation.observe_facts(Some(facts(false)), 1);
        assert_eq!(
            preparation.present_for_test(binding).phase(),
            NativePreparationPaintPhase::PreShow
        );
        let token = eframe::prepare_native_viewport_visibility(viewport, true).token();
        assert!(preparation.dispatch_show(token));
        assert!(preparation.visibility(
            NativeViewportVisibilityRecord::for_test(
                token,
                viewport,
                window,
                binding,
                true,
                NativeViewportVisibilityStatus::Dispatched,
            ),
            2,
        ));
        preparation.observe_facts(Some(facts(true)), 3);
        assert_eq!(
            preparation.present_for_test(binding).phase(),
            NativePreparationPaintPhase::PostShow
        );
        assert!(preparation.is_ready());
        preparation
    }

    pub(super) fn present_for_test(
        &mut self,
        binding: NativeSurfaceBinding,
    ) -> NativePreparationPaint {
        let request = self.paint(binding).expect("a preparation paint is pending");
        assert!(self.begin_paint(request));
        assert!(self.finish_paint(request, true));
        request
    }
}

#[cfg(test)]
mod tests;
