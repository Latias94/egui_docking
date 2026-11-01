//! Read-only stationary requests for the current joined native gesture.

use std::time::Duration;

use super::{
    DockspaceSession, NativeDragSourceInput, NativeHostProfile, NativeLocalCoordinates,
    NativePointerEvent, NativePointerId, NativeSurfaceBinding,
};
use crate::geometry::LogicalPoint;
use crate::ids::SurfaceId;
use crate::interaction::{DragSessionId, GestureLane, GestureOwner, PointerContinuationRequest};
use crate::pointer_journal::PointerProviderScope;
use crate::runtime::PointerContinuationToken;

/// Read-only capture requirement for the current native desktop mouse drag.
///
/// Equality identifies the exact existing gesture and its frozen source-window
/// treatment, independently of hover targets and finite stationary requests.
/// This value supplies no native capture fact or receiver authority. Report a
/// freshly observed acquisition through the existing finite continuation's
/// [`NativePointerContinuation::capture_acquired_event`] when one is available.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePointerCaptureRequest {
    owner: GestureOwner,
    session: DragSessionId,
    source_binding: NativeSurfaceBinding,
    source_input: NativeDragSourceInput,
}

impl NativePointerCaptureRequest {
    /// Returns the native pointer whose exact existing gesture requires capture.
    #[must_use]
    pub const fn pointer(&self) -> NativePointerId {
        NativePointerId::new(self.owner.pointer().get())
    }

    /// Returns the exact original source binding required for native capture.
    #[must_use]
    pub const fn source_binding(&self) -> NativeSurfaceBinding {
        self.source_binding
    }

    /// Returns core's frozen source-window treatment for this drag.
    #[must_use]
    pub const fn source_input(&self) -> NativeDragSourceInput {
        self.source_input
    }
}

/// One finite stationary action projected from the committed native gesture.
///
/// This value shares the existing gesture's token and pending action. Reading
/// or scheduling it grants no observed position, capture, or receiver authority.
/// Return its event through the normal native input journal with fresh native
/// facts and an observation time on the same clock as the original input.
/// Missing delivery or capture facts revoke the pending timed action without
/// proving that the held gesture lost its owner. Explicit owner loss still
/// cancels the gesture; a later real motion may rearm after missing facts.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativePointerContinuation {
    request: PointerContinuationRequest,
    source_binding: NativeSurfaceBinding,
    local_coordinates: Option<NativeLocalCoordinates>,
    source_input: Option<NativeDragSourceInput>,
    capture_acquired: bool,
}

impl NativePointerContinuation {
    /// Returns the native pointer whose exact existing stream owns this action.
    #[must_use]
    pub const fn pointer(&self) -> NativePointerId {
        NativePointerId::new(self.request.token.owner.pointer().get())
    }

    /// Returns the earliest observation time which can execute this action.
    #[must_use]
    pub const fn deadline(&self) -> Duration {
        self.request.token.deadline
    }

    /// Returns the logical surface whose current output must answer the query.
    ///
    /// A desktop gesture may hover a surface other than its original source.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.request.surface
    }

    /// Returns the logical point required for the fresh receiver query.
    ///
    /// This scheduling point must not substitute for a fresh native observation.
    #[must_use]
    pub const fn position(&self) -> LogicalPoint {
        self.request.point
    }

    /// Returns the existing opaque token for this pending action and frame retry.
    #[must_use]
    pub const fn token(&self) -> PointerContinuationToken {
        self.request.token
    }

    /// Returns the non-motion event for the normal ordered native pointer input.
    #[must_use]
    pub const fn event(&self) -> NativePointerEvent {
        NativePointerEvent::Continuation(self.request.token)
    }

    /// Returns the exact original source binding required for delivery and capture.
    ///
    /// This handle identifies the owner; it does not report current native facts.
    #[must_use]
    pub const fn source_binding(&self) -> NativeSurfaceBinding {
        self.source_binding
    }

    /// Returns core's frozen source-window treatment for a native desktop drag.
    ///
    /// This is a read-only requirement, not permission to change window input or
    /// infer capture. Local and pressed-touch continuations return `None`.
    #[must_use]
    pub const fn source_input(&self) -> Option<NativeDragSourceInput> {
        self.source_input
    }

    /// Returns the non-motion fact for the first fresh capture acquisition.
    ///
    /// Submit only after the host independently proves that the original drag
    /// acquired capture. Supply its actual observation time and exact source
    /// delivery/capture facts. Core starts a fresh interval without executing
    /// selection or scrolling. A result from an older target in the same drag
    /// may postpone its current challenge; another gesture cannot consume it.
    /// Missing facts revoke the current timed action without inferring owner
    /// loss. Repeated accepted acquisition cannot restart the interval again.
    /// Hosts with independently continuous capture need no acquisition event.
    #[must_use]
    pub fn capture_acquired_event(&self) -> Option<NativePointerEvent> {
        (self.source_input.is_some() && !self.capture_acquired)
            .then_some(NativePointerEvent::CaptureAcquired(self.request.token))
    }

    /// Returns the original local lane's current exact coordinate capability.
    ///
    /// `Some` requires a fresh sample captured under this binding and generation.
    /// `None` identifies a desktop lane, which needs independently observed
    /// desktop position and hover facts. No coordinates are sampled by this read.
    #[must_use]
    pub const fn local_coordinates(&self) -> Option<NativeLocalCoordinates> {
        self.local_coordinates
    }
}

impl DockspaceSession {
    /// Returns the committed native desktop mouse drag's capture requirement.
    ///
    /// The gesture need not have a finite stationary action. Hover target or
    /// deadline changes preserve this request's identity; another drag session
    /// differs even when it reuses the same native pointer and source binding.
    /// Reading this projection does not record input or advance the gesture.
    ///
    /// Hosts must drain accepted raw callbacks and commit pending core input
    /// before comparing this with a retained request. Pending input temporarily
    /// returns `None`, which must not be interpreted as completed native cleanup.
    /// An ended gesture or invalid source also returns `None`; native capture
    /// release and cleanup still require their own actual host observations.
    #[must_use]
    pub fn native_pointer_capture_request(&self) -> Option<NativePointerCaptureRequest> {
        let (owner, source_binding, _) = self.native_pointer_continuation_source()?;
        let (session, capture_owner, source_input) =
            self.engine.native_pointer_capture_projection()?;
        (capture_owner == owner).then_some(NativePointerCaptureRequest {
            owner,
            session,
            source_binding,
            source_input,
        })
    }

    /// Returns the committed joined native gesture's finite stationary request.
    ///
    /// Accepted native input must commit before a request can be returned. Hosts
    /// must also drain accepted raw callbacks before scheduling, then re-query
    /// at the deadline and submit fresh native facts through
    /// [`Self::record_native_pointer`]. The normal receiver callback must prove
    /// the current output at that edge's ordered reduction position.
    ///
    /// Reading this projection does not mint a token, record input, or advance
    /// the gesture. A changed owner, binding, local generation, or pending input
    /// makes it unavailable.
    #[must_use]
    pub fn native_pointer_continuation(&self) -> Option<NativePointerContinuation> {
        let (owner, source_binding, local_coordinates) =
            self.native_pointer_continuation_source()?;
        let request = self.engine.pointer_continuation()?;
        if request.token.owner != owner.stream()
            || (local_coordinates.is_some() && request.surface != source_binding.surface())
        {
            return None;
        }
        let capture = self.engine.native_drag_capture_projection(request.token);
        Some(NativePointerContinuation {
            request,
            source_binding,
            local_coordinates,
            source_input: capture.map(|(input, _)| input),
            capture_acquired: capture.is_some_and(|(_, acquired)| acquired),
        })
    }

    /// Returns whether a native stationary action awaits this surface's exact output.
    ///
    /// An output result should wake the host to accept it in normal input order
    /// and re-query [`Self::native_pointer_continuation`]. This does not grant
    /// permission to execute a continuation or request continuous repainting.
    /// Recorded native input and an invalid original source suppress this hint.
    #[must_use]
    pub fn native_pointer_continuation_awaits_presentation(&self, surface: SurfaceId) -> bool {
        self.native_pointer_continuation_source().is_some_and(
            |(_, source_binding, local_coordinates)| {
                (local_coordinates.is_none() || surface == source_binding.surface())
                    && self
                        .engine
                        .pointer_continuation_awaits_presentation(surface)
            },
        )
    }

    fn native_pointer_continuation_source(
        &self,
    ) -> Option<(
        GestureOwner,
        NativeSurfaceBinding,
        Option<NativeLocalCoordinates>,
    )> {
        let native = self.native.as_ref()?;
        let backend = native.recorder.lease();
        if native.profile != NativeHostProfile::ManagedDesktop
            || self.engine.backend_ingress_provider() != Some(backend)
            || self.has_pending_native_input()
        {
            return None;
        }
        let interaction = self.engine.interaction();
        let owner = interaction.active_owner()?;
        if owner.lease() != backend.pointer_provider()
            || self.engine.pointer_provider() != Some(owner.lease())
            || owner.lease().scope()
                != PointerProviderScope::NativeJoined(backend.presentation_host())
        {
            return None;
        }
        let presentation = interaction.active_presentation_authority()?;
        let binding = presentation.presented.binding()?;
        let source_binding = NativeSurfaceBinding::from_binding(native.provider(), binding);
        if interaction.active_surface() != Some(binding.surface())
            || !self.is_current_native_binding(source_binding)
        {
            return None;
        }
        let local_coordinates = match owner.lane() {
            GestureLane::NativeLocal {
                binding: local_binding,
                coordinate_generation,
            } => {
                if local_binding != binding
                    || presentation.presented.coordinate_generation() != coordinate_generation
                {
                    return None;
                }
                let coordinates = self.native_local_coordinates(source_binding)?;
                if coordinates.coordinate_generation() != coordinate_generation {
                    return None;
                }
                Some(coordinates)
            }
            GestureLane::Desktop => None,
            GestureLane::HostLocal | GestureLane::SurfaceLocal => return None,
        };
        Some((owner, source_binding, local_coordinates))
    }
}
