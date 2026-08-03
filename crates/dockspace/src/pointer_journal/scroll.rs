use super::*;

impl ScrollDeviceId {
    /// Creates a device identity from its provider protocol representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the provider protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl ScrollSequenceToken {
    /// Creates a sequence token from its provider protocol representation.
    #[must_use]
    pub const fn new(value: u64) -> Self {
        Self(value)
    }

    /// Returns the provider protocol representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }
}
impl FiniteScrollVector {
    /// Creates a finite content-movement vector.
    ///
    /// # Errors
    ///
    /// Returns [`ScrollEdgeError::NonFiniteDelta`] when either component is NaN
    /// or infinite.
    pub fn new(x: f64, y: f64) -> Result<Self, ScrollEdgeError> {
        if !x.is_finite() {
            return Err(ScrollEdgeError::NonFiniteDelta { axis: "x" });
        }
        if !y.is_finite() {
            return Err(ScrollEdgeError::NonFiniteDelta { axis: "y" });
        }
        Ok(Self {
            x_bits: canonical_scroll_component(x).to_bits(),
            y_bits: canonical_scroll_component(y).to_bits(),
        })
    }

    /// Returns horizontal content movement.
    #[must_use]
    pub const fn x(self) -> f64 {
        f64::from_bits(self.x_bits)
    }

    /// Returns vertical content movement.
    #[must_use]
    pub const fn y(self) -> f64 {
        f64::from_bits(self.y_bits)
    }
}

const fn canonical_scroll_component(value: f64) -> f64 {
    if value == 0.0 { 0.0 } else { value }
}
impl ScrollDelta {
    /// Returns the raw two-axis content movement.
    #[must_use]
    pub const fn vector(self) -> FiniteScrollVector {
        match self {
            Self::PhysicalPixels { delta, .. }
            | Self::LogicalPoints(delta)
            | Self::Lines(delta)
            | Self::Pages(delta) => delta,
        }
    }
}
impl PhysicalScrollCoordinates {
    /// Creates exact conversion authority for one physical-pixel sample.
    #[must_use]
    pub const fn new(
        binding: ViewportBinding,
        coordinate_generation: CoordinateGeneration,
    ) -> Self {
        Self {
            binding,
            coordinate_generation,
        }
    }

    /// Returns the native viewport incarnation whose scale converts the sample.
    #[must_use]
    pub const fn binding(self) -> ViewportBinding {
        self.binding
    }

    /// Returns the exact coordinate generation observed with the sample.
    #[must_use]
    pub const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }
}
impl ScrollModifiers {
    /// Creates an exact modifier snapshot.
    #[must_use]
    pub const fn new(shift: bool, control: bool, alt: bool, command: bool) -> Self {
        Self {
            shift,
            control,
            alt,
            command,
        }
    }

    /// Returns whether Shift was pressed.
    #[must_use]
    pub const fn shift(self) -> bool {
        self.shift
    }

    /// Returns whether Control was pressed.
    #[must_use]
    pub const fn control(self) -> bool {
        self.control
    }

    /// Returns whether Alt was pressed.
    #[must_use]
    pub const fn alt(self) -> bool {
        self.alt
    }

    /// Returns whether the platform command modifier was pressed.
    #[must_use]
    pub const fn command(self) -> bool {
        self.command
    }
}
impl ScrollDeliveryEndpoint {
    /// Creates one endpoint-bound delivery fact.
    ///
    /// # Errors
    ///
    /// Returns [`ScrollEdgeError`] when a native binding names another surface
    /// or engine authority domain.
    pub fn new(
        host: PresentationHostLease,
        surface: SurfaceId,
        binding: Option<ViewportBinding>,
        coordinate_generation: CoordinateGeneration,
    ) -> Result<Self, ScrollEdgeError> {
        if let Some(binding) = binding {
            if binding.surface() != surface {
                return Err(ScrollEdgeError::DeliverySurfaceMismatch {
                    surface,
                    binding_surface: binding.surface(),
                });
            }
            if binding.authority_domain() != host.authority_domain() {
                return Err(ScrollEdgeError::DeliveryAuthorityDomainMismatch);
            }
        }
        Ok(Self {
            host,
            surface,
            binding,
            coordinate_generation,
        })
    }

    /// Returns the presentation host which received the edge.
    #[must_use]
    pub const fn host(self) -> PresentationHostLease {
        self.host
    }

    /// Returns the logical surface which received the edge.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the exact native binding, when delivery was native.
    #[must_use]
    pub const fn binding(self) -> Option<ViewportBinding> {
        self.binding
    }

    /// Returns the coordinate generation observed at delivery.
    #[must_use]
    pub const fn coordinate_generation(self) -> CoordinateGeneration {
        self.coordinate_generation
    }
}
impl ScrollEdge {
    /// Creates and structurally validates one scroll sample.
    ///
    /// # Errors
    ///
    /// Returns [`ScrollEdgeError`] when phase, sequence, delta, or physical
    /// coordinate facts do not form a legal edge.
    pub fn new(
        device: ScrollDeviceId,
        sequence: Option<ScrollSequenceToken>,
        phase: ScrollPhase,
        delta: Option<ScrollDelta>,
        momentum: Authority<ScrollMomentum>,
        modifiers: Authority<ScrollModifiers>,
        delivery: Authority<ScrollDeliveryEndpoint>,
    ) -> Result<Self, ScrollEdgeError> {
        let edge = Self {
            device,
            sequence,
            phase,
            delta,
            momentum,
            modifiers,
            delivery,
        };
        edge.validate()?;
        Ok(edge)
    }

    pub(super) fn validate(self) -> Result<(), ScrollEdgeError> {
        let legal_shape = match self.phase {
            ScrollPhase::Discrete => self.sequence.is_none() && self.delta.is_some(),
            ScrollPhase::Begin => self.sequence.is_some(),
            ScrollPhase::Update => self.sequence.is_some() && self.delta.is_some(),
            ScrollPhase::End => self.sequence.is_some(),
            ScrollPhase::Cancel(_) => self.sequence.is_some() && self.delta.is_none(),
        };
        if !legal_shape {
            return Err(ScrollEdgeError::InvalidPhaseShape { phase: self.phase });
        }
        if let (
            Some(ScrollDelta::PhysicalPixels {
                coordinates: Authority::Known(coordinates),
                ..
            }),
            Authority::Known(delivery),
        ) = (self.delta, self.delivery)
            && (delivery.binding() != Some(coordinates.binding())
                || delivery.coordinate_generation() != coordinates.coordinate_generation())
        {
            return Err(ScrollEdgeError::PhysicalDeltaEndpointMismatch);
        }
        Ok(())
    }

    /// Returns the provider-owned device identity.
    #[must_use]
    pub const fn device(self) -> ScrollDeviceId {
        self.device
    }

    /// Returns the provider sequence token for a phaseful sample.
    #[must_use]
    pub const fn sequence(self) -> Option<ScrollSequenceToken> {
        self.sequence
    }

    /// Returns the native scroll phase.
    #[must_use]
    pub const fn phase(self) -> ScrollPhase {
        self.phase
    }

    /// Returns the optional raw delta.
    #[must_use]
    pub const fn delta(self) -> Option<ScrollDelta> {
        self.delta
    }

    /// Returns direct-versus-momentum authority.
    #[must_use]
    pub const fn momentum(self) -> Authority<ScrollMomentum> {
        self.momentum
    }

    /// Returns the event-time modifier authority.
    #[must_use]
    pub const fn modifiers(self) -> Authority<ScrollModifiers> {
        self.modifiers
    }

    /// Returns the physical delivery endpoint authority.
    #[must_use]
    pub const fn delivery(self) -> Authority<ScrollDeliveryEndpoint> {
        self.delivery
    }
}
