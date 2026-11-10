//! Product presentation commands and current-state availability.

use super::{DockspaceRuntimeError, DockspaceSession, NativeWindowPlacement, PreparedDockAction};
use crate::ids::{RootId, SurfaceId};
use crate::model::{DockspaceActionRejection, NativeTearOffSource, ProductAction};

/// One presentation command evaluated against the current published session state.
#[derive(Debug)]
#[must_use = "a ready presentation command must be submitted or deliberately discarded"]
pub enum DockspacePresentationCommand {
    /// The command is currently executable and carries its revision-bound action.
    Ready(PreparedDockAction),
    /// The command is unavailable for one stable product-level reason.
    Unavailable(DockspaceActionRejection),
}

impl DockspacePresentationCommand {
    /// Returns whether this command was ready when queried.
    #[must_use]
    pub const fn is_ready(&self) -> bool {
        matches!(self, Self::Ready(_))
    }

    /// Returns the current unavailability reason, if any.
    #[must_use]
    pub const fn unavailable_reason(&self) -> Option<DockspaceActionRejection> {
        match self {
            Self::Ready(_) => None,
            Self::Unavailable(reason) => Some(*reason),
        }
    }

    /// Converts this availability result into the revision-bound action or its stable reason.
    pub fn into_prepared(self) -> Result<PreparedDockAction, DockspaceActionRejection> {
        match self {
            Self::Ready(action) => Ok(action),
            Self::Unavailable(reason) => Err(reason),
        }
    }
}

/// Core-derived presentation commands for one stable root.
///
/// The view is read-only and non-owning. It does not expose presentation internals; every method
/// returns either an opaque prepared action or a stable product-level unavailability reason.
pub struct DockspacePresentationCommands<'session> {
    session: &'session DockspaceSession,
    root: RootId,
}

impl<'session> DockspacePresentationCommands<'session> {
    pub(super) const fn new(session: &'session DockspaceSession, root: RootId) -> Self {
        Self { session, root }
    }

    /// Returns the stable root addressed by every command in this view.
    #[must_use]
    pub const fn root(&self) -> RootId {
        self.root
    }

    /// Preflights a default contained-floating transition on `surface`.
    ///
    /// Returns [`DockspacePresentationCommand::Unavailable`] when the root is
    /// already contained on this surface. A contained root may move to another surface.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when core command compilation violates an invariant.
    pub fn float_to(
        &self,
        surface: SurfaceId,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        self.prepare(ProductAction::FloatRoot {
            root: self.root,
            surface,
            rect: None,
        })
    }

    /// Preflights the core-derived default dock-back transition.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when core command compilation violates an invariant.
    pub fn dock_back(&self) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        self.prepare(ProductAction::DockBackRoot { root: self.root })
    }

    /// Preflights promotion into a managed native child window.
    ///
    /// An explicitly placed promotion may recover a contained group outside
    /// its source surface's visible bounds. It still requires current source
    /// presentation authority and the group's measured minimum size.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when native preflight violates a core invariant.
    pub fn move_to_new_window(
        &self,
        placement: NativeWindowPlacement,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        self.session
            .native_window_command(NativeTearOffSource::Root(self.root), placement)
    }

    /// Preflights promotion using core-derived source geometry and default placement.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when native preflight violates a core invariant.
    pub fn move_to_new_window_default(
        &self,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        self.session
            .native_window_command_default(NativeTearOffSource::Root(self.root))
    }

    fn prepare(
        &self,
        action: ProductAction,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        Ok(
            match self
                .session
                .engine
                .prepare_product_action_if_available(action)?
            {
                Ok(action) => DockspacePresentationCommand::Ready(action),
                Err(reason) => DockspacePresentationCommand::Unavailable(reason),
            },
        )
    }
}

impl DockspaceSession {
    /// Preflights one item, captured tabs group or root into an explicitly placed native window.
    ///
    /// A ready action remains revision-bound. Neither this query nor submitting
    /// its result transfers content before exact first-live presentation.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when native preflight violates a core invariant.
    pub fn native_window_command(
        &self,
        source: NativeTearOffSource,
        placement: NativeWindowPlacement,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        self.prepare_native_window_command(source, Some(placement))
    }

    /// Preflights native tear-off with core-derived placement from current platform facts.
    ///
    /// No paint encoding is needed. Missing source geometry or work-area authority
    /// makes the command unavailable; it never falls back to saved window positions.
    ///
    /// # Errors
    ///
    /// Returns an internal runtime error only when native preflight violates a core invariant.
    pub fn native_window_command_default(
        &self,
        source: NativeTearOffSource,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        self.prepare_native_window_command(source, None)
    }

    fn prepare_native_window_command(
        &self,
        source: NativeTearOffSource,
        placement: Option<NativeWindowPlacement>,
    ) -> Result<DockspacePresentationCommand, DockspaceRuntimeError> {
        if self
            .native
            .as_ref()
            .and_then(|native| native.binding_allocation_budget())
            == Some(0)
        {
            return Ok(DockspacePresentationCommand::Unavailable(
                DockspaceActionRejection::NativeUnavailable,
            ));
        }
        Ok(
            match self
                .engine
                .prepare_product_action_if_available(ProductAction::TearOffNative {
                    source,
                    placement,
                })? {
                Ok(action) => DockspacePresentationCommand::Ready(action),
                Err(reason) => DockspacePresentationCommand::Unavailable(reason),
            },
        )
    }
}
