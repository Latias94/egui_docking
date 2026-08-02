//! Presentation-bound, device-independent semantic input facts.

use crate::presentation_hit::PresentationHitRegionKind;
use crate::presentation_observation::{HostFrameKey, SurfacePresentationOutputTicket};
use crate::viewport::ViewportBinding;

/// Exact presentation endpoint which delivered a semantic action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticDelivery {
    /// A headless or framework-local presentation delivered the action.
    Headless,
    /// An exact native viewport incarnation delivered the action.
    Native(ViewportBinding),
}

/// Keyboard keys with renderer-neutral docking semantics.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticKey {
    /// Move or adjust toward the left.
    ArrowLeft,
    /// Move or adjust toward the right.
    ArrowRight,
    /// Move or adjust upward.
    ArrowUp,
    /// Move or adjust downward.
    ArrowDown,
    /// Move to the first ordered target.
    Home,
    /// Move to the last ordered target.
    End,
    /// Activate the focused target.
    Enter,
    /// Activate the focused target.
    Space,
}

/// Accessibility actions understood by the docking semantic reducer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticAccessibilityAction {
    /// Activate the exact target.
    Click,
    /// Move semantic focus to the exact target.
    Focus,
    /// Increase the exact adjustable target.
    Increment,
    /// Decrease the exact adjustable target.
    Decrement,
}

/// One device-independent action delivered to a retained semantic receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SemanticReceiverAction {
    /// A physical keyboard press.
    Key(SemanticKey),
    /// An accessibility action request.
    Accessibility(SemanticAccessibilityAction),
}

/// Exact presented receiver fact supplied by a UI adapter.
///
/// The adapter can name only core-minted output and emission identities. The
/// reducer rechecks both identities and the receiver roster before applying the
/// action, so a retained or delayed framework target fails closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SemanticReceiverEvent {
    output: SurfacePresentationOutputTicket,
    emission: HostFrameKey,
    delivery: SemanticDelivery,
    target: PresentationHitRegionKind,
    action: SemanticReceiverAction,
}

impl SemanticReceiverEvent {
    /// Creates one presentation-bound semantic receiver event.
    #[must_use]
    pub const fn new(
        output: SurfacePresentationOutputTicket,
        emission: HostFrameKey,
        delivery: SemanticDelivery,
        target: PresentationHitRegionKind,
        action: SemanticReceiverAction,
    ) -> Self {
        Self {
            output,
            emission,
            delivery,
            target,
            action,
        }
    }

    /// Returns the exact semantic output targeted by the event.
    #[must_use]
    pub const fn output(self) -> SurfacePresentationOutputTicket {
        self.output
    }

    /// Returns the exact presented emission targeted by the event.
    #[must_use]
    pub const fn emission(self) -> HostFrameKey {
        self.emission
    }

    /// Returns the exact endpoint which delivered this action.
    #[must_use]
    pub const fn delivery(self) -> SemanticDelivery {
        self.delivery
    }

    /// Returns the stable receiver identity delivered by the host.
    #[must_use]
    pub const fn target(self) -> PresentationHitRegionKind {
        self.target
    }

    /// Returns the device-independent action delivered to the receiver.
    #[must_use]
    pub const fn action(self) -> SemanticReceiverAction {
        self.action
    }
}
