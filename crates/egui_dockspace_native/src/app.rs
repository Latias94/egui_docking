//! Thin eframe application over one shared native runtime state.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex, Weak};

use dockspace::model::{
    DockAnchor, DockspaceActionRejection, DockspaceLayout, DockspaceView, PreparedTabsGroup,
    SurfaceId,
};
use dockspace::runtime::DockspaceClosePlan;
use dockspace::runtime::{DockspaceSession, NativeSurfacePlacement};
use eframe::egui::{self, Id, ViewportClass, ViewportId};
use eframe::{
    NativeHostHandler, NativeHostWake, NativeOutputPassFinalization, NativeOutputPassFinalizer,
    NativeOutputToken,
};
use egui_dockspace::{DockInputConfig, DockStyle, PaneView};

use crate::close_control::NativeWindowClosePolicy;
use crate::deferred_viewport::{
    DeferredViewportSpec, declare_deferred_viewports, paint_placeholder,
};
use crate::error::{NativeHostProtocolError, NativeRuntimeError, NativeRuntimeErrorKind};
use crate::lifecycle_progress::{NativeCloseProgress, NativeOutputBacklog};
use crate::mailbox::DeferredViewportPaint;
use crate::notification::{lock_state, with_state};
use crate::surface_driver::{NativeErrorPresentation, NativeRuntimeState, NativeSurfaceUpdate};
use crate::window_options::DeclaredWindowIcons;
use crate::{
    NativeActionFailure, NativeActionRequestError, NativeActionStatus, NativeChildWindowOptions,
    NativeLayoutRequestError,
};

#[cfg(all(test, target_os = "linux"))]
mod native_boundary_x11;

/// Fork-backed eframe application whose [`DockspaceSession`] is the sole graph authority.
///
/// Root and child callbacks borrow the same locked runtime state. The lock is
/// released before eframe declares deferred viewports, so embedded fallback
/// callbacks cannot re-enter a held state lock. Native product panes must be
/// [`Send`] because eframe may invoke deferred viewport callbacks independently
/// of the root callback.
///
/// Both concrete providers and `Box<dyn PaneView + Send>` are supported. Keep
/// background work in the application, then apply completed pane data through
/// [`Self::with_panes_mut`] and wake its egui viewports. Registry callbacks run
/// under the runtime lock; submit docking commands only after they return.
/// Deferred saves use [`crate::NativeCommand::DecideClose`] with
/// [`dockspace::close::CloseDecision::Deferred`], followed by
/// [`crate::NativeCommand::ContinueClose`] with the returned core-issued token.
/// Retain completion while the command slot is Busy and consume
/// [`Self::take_action_status`] before submitting the next command. Admission
/// queues a command; its later committed result determines whether it applied.
pub struct NativeDockspaceApp<P> {
    state: Arc<Mutex<NativeRuntimeState<P>>>,
    native_host: Arc<dyn NativeHostHandler>,
    child_window_options: NativeChildWindowOptions,
    child_window_overrides: BTreeMap<SurfaceId, NativeChildWindowOptions>,
    declared_child_icons: DeclaredWindowIcons,
}

impl<P: PaneView + Send + 'static> NativeDockspaceApp<P> {
    /// Creates one native application around an existing product session.
    ///
    /// The managed pointer provider starts with Unknown global button/capture
    /// authority. Exact event-time edges can still advance their own streams,
    /// while unavailable facts remain fail-closed rather than being inferred
    /// from application startup.
    ///
    /// The default close policy restores managed child presentations through
    /// their core-owned recovery anchors. Closing the external application
    /// root is accepted and retains the logical layout so the application can
    /// complete its normal shutdown.
    ///
    /// The session owns presentation geometry and interaction timing. Every
    /// setting represented by `style` must match it exactly; construction rejects
    /// mismatches instead of replacing either configuration. For custom style
    /// geometry, create the session with [`DockStyle::presentation_config`].
    /// Core-only settings may differ from that conversion's defaults. Visual
    /// overrides and the drag-ghost offset remain adapter-local.
    ///
    /// # Errors
    ///
    /// Returns an error when the requested root surface is absent, the style is
    /// invalid or differs from the session's represented settings, native
    /// enrollment fails, or the root registration cannot be queued.
    pub fn new(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_with_close_policy(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            NativeWindowClosePolicy::default(),
        )
    }

    /// Creates one native application with an explicit window-close policy.
    ///
    /// The original OS request remains held while ordered input or its close
    /// decision yields to the next native dispatch budget.
    ///
    /// # Errors
    ///
    /// Returns the same initialization failures as [`Self::new`].
    pub fn new_with_close_policy(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_configured(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            None,
        )
    }

    /// Creates a native application with complete finite input/lifecycle storage.
    ///
    /// Ordinary ingress freezes before it can exceed the selected byte, record
    /// or age allowance. Previously accepted input and terminal obligations
    /// retain their original ordered recovery path.
    ///
    /// # Errors
    ///
    /// Returns configuration or enrollment failure before returning a host
    /// handler, including invalid core subprofile storage allowances or a profile
    /// without a resource and close-request slot for the application root. Style
    /// validation and configuration matching follow [`Self::new`].
    pub fn new_with_storage_limit(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
        storage: crate::NativeStorageLimit,
    ) -> Result<Self, NativeRuntimeError> {
        Self::new_configured(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            Some(storage),
        )
    }

    fn new_configured(
        instance_id: Id,
        session: DockspaceSession,
        root_surface: SurfaceId,
        panes: P,
        style: DockStyle,
        close_policy: NativeWindowClosePolicy,
        storage: Option<crate::NativeStorageLimit>,
    ) -> Result<Self, NativeRuntimeError> {
        let state = NativeRuntimeState::new_with_storage_limit(
            instance_id,
            session,
            root_surface,
            panes,
            style,
            close_policy,
            storage,
        )?;
        let native_host = state.native_host_handler();
        let state = Arc::new(Mutex::new(state));
        let finalizer: Arc<dyn NativeOutputPassFinalizer> = Arc::new(NativePassFinalizer {
            state: Arc::downgrade(&state),
        });
        assert!(
            native_host.install_output_pass_finalizer(finalizer),
            "the Dockspace native host accepts exactly one output-pass finalizer"
        );
        Ok(Self {
            state,
            native_host,
            child_window_options: NativeChildWindowOptions::default(),
            child_window_overrides: BTreeMap::new(),
            declared_child_icons: DeclaredWindowIcons::new(),
        })
    }

    /// Configures the appearance of managed child OS windows.
    ///
    /// New windows and replacements use these defaults. Per-surface overrides
    /// inherit fields they leave unset. Configure the external Root separately
    /// through `eframe::NativeOptions::viewport`.
    #[must_use]
    pub fn with_child_window_options(mut self, options: NativeChildWindowOptions) -> Self {
        self.set_child_window_options(options);
        self
    }

    /// Updates child-window appearance on the next Root UI update.
    ///
    /// Request a Root repaint when calling this outside an egui update. Fields
    /// left unset use the deterministic defaults in [`NativeChildWindowOptions`].
    pub fn set_child_window_options(&mut self, options: NativeChildWindowOptions) {
        self.child_window_options = options;
    }

    /// Returns the application's current child-window appearance options.
    #[must_use]
    pub fn child_window_options(&self) -> &NativeChildWindowOptions {
        &self.child_window_options
    }

    /// Configures appearance overrides for one stable managed surface.
    ///
    /// Unset fields inherit the global child-window options. Configuration may
    /// precede window creation and survives replacement or restoration of the
    /// same surface. It remains stored until explicitly cleared. The external
    /// Root continues to use its application's `eframe::NativeOptions`.
    #[must_use]
    pub fn with_child_window_options_for(
        mut self,
        surface: SurfaceId,
        options: NativeChildWindowOptions,
    ) -> Self {
        let _ = self.set_child_window_options_for(surface, options);
        self
    }

    /// Replaces one surface's overrides, returning its previous overrides.
    ///
    /// Changes apply on the next Root UI update. Request a Root repaint when
    /// calling this outside an egui update. Unset fields inherit the current
    /// global options, then the defaults in [`NativeChildWindowOptions`].
    pub fn set_child_window_options_for(
        &mut self,
        surface: SurfaceId,
        options: NativeChildWindowOptions,
    ) -> Option<NativeChildWindowOptions> {
        self.child_window_overrides.insert(surface, options)
    }

    /// Returns the resolved global defaults and overrides for one surface.
    ///
    /// Remaining `None` fields use the defaults documented on
    /// [`NativeChildWindowOptions`] when declaring the viewport; they do not
    /// preserve historical window properties.
    /// This does not query the OS or imply that the surface has a live window.
    #[must_use]
    pub fn child_window_options_for(&self, surface: SurfaceId) -> NativeChildWindowOptions {
        self.child_window_overrides.get(&surface).map_or_else(
            || self.child_window_options.clone(),
            |overrides| self.child_window_options.with_overrides(overrides),
        )
    }

    /// Removes a surface's overrides, returning them if present.
    ///
    /// The next Root UI update restores the current global options, including
    /// Dockspace's defaults for global fields still set to `None`.
    /// Request a Root repaint when calling this outside an egui update.
    pub fn clear_child_window_options_for(
        &mut self,
        surface: SurfaceId,
    ) -> Option<NativeChildWindowOptions> {
        self.child_window_overrides.remove(&surface)
    }

    /// Handles future committed layout changes and detailed lifecycle events.
    ///
    /// The callback runs once for each nonempty core publication, including
    /// final UI passes, native service work and shutdown. Repeated encoding
    /// passes and pending native transfers do not emit completion events.
    /// Installing or replacing a handler never replays earlier publications.
    ///
    /// Notifications arrive in commit order after internal settlement releases
    /// the runtime lock. The callback may query shared runtime state and queue
    /// commands for a later update. It can run outside an egui UI pass; keep it
    /// short and move file I/O or background work to application-owned workers.
    /// Split resizing is included in the layout-change summary, even when no
    /// detailed event is emitted. The adapter retains no notification history.
    /// The second argument provides scoped view, persistence and command access
    /// without capturing the application itself. Queries see the current state,
    /// which can include later commits from the same native service operation.
    ///
    /// ```
    /// use egui_dockspace::PaneView;
    /// use egui_dockspace_native::NativeDockspaceApp;
    ///
    /// fn observe<P: PaneView + Send + 'static>(app: NativeDockspaceApp<P>) -> NativeDockspaceApp<P> {
    ///     app.with_notification_handler(|notification, runtime| {
    ///         if notification.layout_change().is_some() {
    ///             let surfaces = runtime.with_view(|view| view.surfaces().count());
    ///             println!("Layout changed across {surfaces} surfaces");
    ///         }
    ///     })
    /// }
    /// ```
    #[must_use]
    pub fn with_notification_handler(
        self,
        mut handler: impl FnMut(&crate::DockspaceNotification, crate::NativeNotificationContext<'_, P>)
        + Send
        + 'static,
    ) -> Self {
        let shared = Arc::downgrade(&self.state);
        lock_state(&self.state)
            .notifications()
            .set_handler(move |notification| {
                if let Some(state) = shared.upgrade() {
                    handler(
                        notification,
                        crate::NativeNotificationContext { state: &state },
                    );
                }
            });
        self
    }

    /// Configures input bindings before installing the native host handler.
    ///
    /// Use [`Self::request_input`] after the host begins receiving input.
    ///
    /// # Errors
    ///
    /// Returns [`NativeActionRequestError::Busy`] after input or an application
    /// command has been captured, so queued input cannot be reinterpreted.
    #[must_use]
    pub fn with_input(self, input: DockInputConfig) -> Result<Self, NativeActionRequestError> {
        lock_state(&self.state).set_initial_input(input)?;
        Ok(self)
    }

    /// Returns the currently committed docking input bindings.
    #[must_use]
    pub fn input(&self) -> DockInputConfig {
        lock_state(&self.state).input()
    }

    /// Queues input bindings in the existing application command slot.
    ///
    /// Earlier captured input keeps its original bindings. A changed configuration
    /// cancels the previous docking gesture through the existing semantic action;
    /// new bindings apply to subsequent runs after the frame commits. Consume
    /// [`NativeActionStatus::InputChanged`] before submitting another command.
    ///
    /// # Errors
    ///
    /// Returns [`NativeActionRequestError::Busy`] while the command slot is occupied,
    /// or [`NativeActionRequestError::Stopped`] after terminal shutdown.
    pub fn request_input(&self, input: DockInputConfig) -> Result<(), NativeActionRequestError> {
        lock_state(&self.state).request_input(input)
    }

    /// Returns the one-shot host callback to install in [`eframe::NativeOptions::native_host`].
    ///
    /// The returned `Arc` may be cloned for ownership plumbing, but the native
    /// runtime accepts exactly one eframe context attachment. Construct a new
    /// application and session instead of reusing this handler for another
    /// event loop.
    #[must_use]
    pub fn native_host_handler(&self) -> Arc<dyn NativeHostHandler> {
        Arc::clone(&self.native_host)
    }

    /// Inspects the current item/surface-centric product view while holding the runtime lock.
    ///
    /// A fatal native-cycle error freezes further host ingress but preserves
    /// the last committed read-only view while owned host state is quarantined.
    /// The callback always runs, including before native attachment and after
    /// shutdown. It must not call this application's other runtime methods:
    /// the same lock remains held until the callback returns.
    pub fn with_view<R>(&self, inspect: impl FnOnce(DockspaceView<'_>) -> R) -> R {
        let state = lock_state(&self.state);
        inspect(state.view())
    }

    /// Captures one complete tabs group from the published session under the runtime lock.
    ///
    /// Store the opaque result in [`crate::NativeCommand::MoveToNewWindow`] or
    /// [`crate::NativeCommand::MoveToNewWindowDefault`]. Membership, order,
    /// selection, session and version remain bound to this capture. Later
    /// admission never looks up `anchor` again or refreshes a stale group.
    /// This read-only query does not occupy the application command slot.
    ///
    /// # Errors
    ///
    /// Returns the core rejection when the anchor is absent or its group is empty.
    pub fn capture_tabs_group(
        &self,
        anchor: DockAnchor,
    ) -> Result<PreparedTabsGroup, DockspaceActionRejection> {
        lock_state(&self.state).capture_tabs_group(anchor)
    }

    /// Inspects the application pane registry while holding the runtime lock.
    ///
    /// Keep the callback short and do not call this application's other runtime
    /// methods: the same lock remains held until it returns. Run background I/O
    /// outside the callback and transfer only completed data into the registry.
    pub fn with_panes<R>(&self, inspect: impl FnOnce(&P) -> R) -> R {
        inspect(lock_state(&self.state).panes())
    }

    /// Mutates the application pane registry while holding the runtime lock.
    ///
    /// Keep the callback short, perform I/O outside it, and do not re-enter this
    /// application's runtime methods. Collect any docking command, return from
    /// the callback, then submit it through [`Self::request_command`]. Updating
    /// pane data alone does not request a repaint; wake the affected egui
    /// viewport after applying background results.
    pub fn with_panes_mut<R>(&self, mutate: impl FnOnce(&mut P) -> R) -> R {
        mutate(lock_state(&self.state).panes_mut())
    }

    /// Returns the stable category of the first fatal native-cycle error.
    #[must_use]
    pub fn error_kind(&self) -> Option<NativeRuntimeErrorKind> {
        lock_state(&self.state)
            .error()
            .map(NativeRuntimeError::kind)
    }

    /// Inspects the first fatal error and subsequent cleanup errors under the runtime lock.
    ///
    /// Borrows existing error storage without cloning or allocating. Before a
    /// fatal error, the callback receives `None` and an empty slice. Cleanup
    /// errors retain their observation order. The callback must not re-enter
    /// this application's runtime methods because the lock remains held.
    pub fn with_errors<R>(
        &self,
        inspect: impl FnOnce(Option<&NativeRuntimeError>, &[NativeRuntimeError]) -> R,
    ) -> R {
        lock_state(&self.state).with_errors(inspect)
    }

    /// Returns whether one logical surface has an exact currently presented output.
    ///
    /// This is a product readiness query, not a renderer token or historical
    /// acknowledgement. It becomes false again when the current presentation
    /// authority is retired or invalidated.
    #[must_use]
    pub fn is_surface_presented(&self, surface: SurfaceId) -> bool {
        lock_state(&self.state).is_surface_presented(surface)
    }

    /// Returns whether native lifecycle work is settled at the current committed boundary.
    ///
    /// Stable live viewports and their ordinary repaint outputs do not make the
    /// runtime busy. Pending application actions, create/show barriers, close or
    /// focus effects, callback records, and binding retirement do.
    #[must_use]
    pub fn is_quiescent(&self) -> bool {
        lock_state(&self.state).is_quiescent()
    }

    /// Returns all work currently preventing ordinary runtime quiescence.
    ///
    /// The allocation-free snapshot explains [`Self::is_quiescent`] without
    /// consuming input or retaining history. Its debug output lists active
    /// reasons. A queued application command remains pending until its result
    /// is consumed. This is diagnostic state, not authority to skip a barrier.
    #[must_use]
    pub fn pending_work(&self) -> crate::NativePendingWork {
        lock_state(&self.state).pending_work()
    }

    /// Returns whether fatal shutdown has drained application and lifecycle work.
    ///
    /// Requires an existing fatal error, no pending application or retained
    /// local actions, and a quiescent coordinator. This does not require a
    /// presented root surface and does not change [`Self::is_quiescent`].
    /// Diagnostic outputs may still be in flight, including the current pass;
    /// verify [`Self::output_backlog`] after their terminal callbacks when
    /// complete output-owner retirement is required. Inspect [`Self::with_errors`]
    /// separately for the primary failure and any cleanup failures.
    #[must_use]
    pub fn is_shutdown_drained(&self) -> bool {
        lock_state(&self.state).is_shutdown_drained()
    }

    /// Returns bounded retained-request and committed-cancellation close counters.
    #[must_use]
    pub fn close_progress(&self) -> NativeCloseProgress {
        lock_state(&self.state).close_progress()
    }

    /// Returns current output-owner occupancy at one locked host boundary.
    ///
    /// Control owners include root diagnostics, child diagnostics and protected
    /// child staging. The snapshot carries no output identity or admission authority.
    #[must_use]
    pub fn output_backlog(&self) -> NativeOutputBacklog {
        lock_state(&self.state).output_backlog()
    }

    /// Returns whether the current pass has staged retention of the previous framebuffer.
    ///
    /// This read-only status is available after the app UI runs and before its
    /// finalizer. Multipass or queued actions may still change the final result;
    /// only the real terminal callback settles output ownership. Outside a
    /// native output callback this returns `false`.
    #[must_use]
    pub fn current_output_is_retained(&self) -> bool {
        lock_state(&self.state).current_output_is_retained()
    }

    /// Returns the current owned data capacity of native input and lifecycle transport.
    ///
    /// Includes preallocated raw/roster backing, recorder/replay transport,
    /// pending report queues, exact window/output/close/retirement owners,
    /// callback controls, pointer translation state and snapshot/work-area
    /// sidecars. Shared roster and abandoned-result data are counted once
    /// through their existing owners. Spare vector capacity remains charged.
    ///
    /// Excludes allocator/BTree node metadata, Arc control blocks, opaque host
    /// callbacks/clock closures, core workspace/authority, renderer/receiver
    /// data, application action/transition/notification queues, diagnostic errors and
    /// caller-owned buffers. This
    /// measures current data at a locked host boundary; it does not reserve
    /// future terminal data or certify the complete 4 MiB admission limit.
    ///
    /// # Errors
    ///
    /// Returns an invariant error if an internal producer bypasses the finite
    /// native payload contract of the recorder.
    pub fn input_storage_capacity_bytes(&self) -> Result<usize, NativeRuntimeError> {
        lock_state(&self.state).owned_input_capacity_bytes()
    }

    /// Returns current retained data plus prepaid native construction promises.
    ///
    /// Returns `None` for the unlimited constructor. This query uses the same
    /// combined allowance as ingress admission; it is distinct from resident
    /// capacity and does not certify physical-input timing or renderer memory.
    #[must_use]
    pub fn input_storage_reserved_bytes(&self) -> Option<usize> {
        lock_state(&self.state).input_storage_reserved_bytes()
    }

    /// Queues one application command for the next eligible final root pass.
    ///
    /// Preparation freezes the current published revision under the runtime lock.
    /// Earlier ordered input may make that preparation stale before execution.
    /// Acceptance means queued, not applied: inspect [`Self::take_action_status`]
    /// for the exact committed result. The single slot remains Busy through any
    /// required presentation transition and until its terminal result is consumed.
    ///
    /// Ordinary semantic completion does not prove a fresh framebuffer. Use
    /// [`Self::is_surface_presented`] when the next operation requires presented
    /// geometry, and consume the result before waiting for [`Self::is_quiescent`].
    /// Do not re-enter this method from a `with_view` or pane-registry callback:
    /// those callbacks already hold the same runtime lock.
    ///
    /// # Errors
    ///
    /// Returns [`NativeActionRequestError::Busy`] while the previous command or
    /// its result owns the slot, `Stopped` after fatal shutdown, or
    /// `HostSurfaceRequired` when the operation would remove the fixed host.
    /// Policy and target rejections are committed action results. Close decisions
    /// remain explicit application choices, including for chrome-created plans.
    pub fn request_command(
        &self,
        command: crate::NativeCommand,
    ) -> Result<(), NativeActionRequestError> {
        lock_state(&self.state).request_command(command)
    }

    /// Queues renderer styling for the next eligible final root pass.
    ///
    /// Use [`DockStyle::from_egui`] after changing egui fonts or density, then
    /// reapply application overrides. Core-only geometry and interaction settings
    /// remain unchanged. Font resources themselves belong to egui's context.
    ///
    /// The existing application command slot retains this request until its
    /// [`NativeActionStatus::StyleChanged`] result is consumed. Acceptance does
    /// not change the style immediately: core geometry and adapter style publish
    /// together after the final pass commits. Geometry changes cancel gestures
    /// through core; visual-only changes preserve them. All live windows repaint.
    /// Consume the result before waiting for fresh presentation or quiescence.
    ///
    /// # Errors
    ///
    /// Returns a queue error while busy or stopped, or the original configuration
    /// error for invalid style. Rejected requests leave both authorities unchanged.
    pub fn request_style(&self, style: DockStyle) -> Result<(), crate::NativeStyleRequestError> {
        lock_state(&self.state).request_style(style)
    }

    /// Returns current unresolved content-close plans from the core owner.
    ///
    /// Includes tab, group and contained chrome requests and programmatic
    /// requests. OS surface-close plans retain their native close-policy owner.
    /// This query retains no second plan history and grants no new authority.
    #[must_use]
    pub fn pending_content_close_plans(&self) -> Vec<DockspaceClosePlan> {
        lock_state(&self.state).pending_content_close_plans()
    }

    /// Returns the currently committed arrangement lock state.
    #[must_use]
    pub fn arrangement_locked(&self) -> bool {
        lock_state(&self.state).arrangement_locked()
    }

    /// Queues an application-owned default or preset layout in the existing slot.
    ///
    /// The layout must preserve this application's representative host surface.
    /// Other surfaces already placed in that same Root window may also survive.
    /// Native input and final-pass local actions precede the terminal replacement.
    /// Root bindings rejoin their existing physical windows with fresh authority;
    /// retired child windows finish their existing cleanup. New logical surfaces
    /// do not implicitly create OS windows. Consume `LayoutReset`, then wait for
    /// fresh presentation with [`Self::is_surface_presented`] before interacting.
    ///
    /// # Errors
    ///
    /// Returns the original queue or core preparation error, a missing host
    /// error, or `Unsupported` for surfaces outside the current Root roster. A layout made
    /// stale before execution reports `Stale` as its queued terminal result
    /// without stopping the native runtime.
    pub fn request_layout_reset(
        &self,
        layout: DockspaceLayout,
    ) -> Result<(), NativeLayoutRequestError> {
        lock_state(&self.state).request_layout_reset(layout)
    }

    /// Queues a layout reset that continues the external Root on an incoming surface.
    ///
    /// The layout must contain `incoming_root` and may preserve the current
    /// shared siblings. Its old representative surface may disappear; the same
    /// OS window continues under fresh bindings. The logical host field and
    /// complete native roster change with the core commit. Wait for fresh
    /// presentation before interacting with the replacement layout.
    ///
    /// # Errors
    ///
    /// Returns queue/preparation errors, a missing incoming surface, an
    /// unavailable external Root or a conflicting window target.
    pub fn request_layout_reset_rebinding_root(
        &self,
        layout: DockspaceLayout,
        incoming_root: SurfaceId,
    ) -> Result<(), NativeLayoutRequestError> {
        lock_state(&self.state).request_layout_reset_rebinding_root(layout, incoming_root)
    }

    /// Returns the representative logical surface of the application's Root window.
    ///
    /// A shared Root may host additional placements. A committed explicit
    /// continuation updates this representative together with the native route.
    #[must_use]
    pub fn root_surface(&self) -> SurfaceId {
        lock_state(&self.state).root_surface()
    }

    /// Composes independent logical surfaces inside the real Root viewport.
    ///
    /// Call once from the Root application's UI callback instead of invoking
    /// this application's `eframe::App::ui`. The complete borrowed roster owns
    /// this pass's layout; each surface's logical size is independent of its
    /// physical clip. Native DPI is already represented by each placement's
    /// physical-pixels-per-logical-unit scale and is not multiplied again.
    /// The roster must be nonempty, contain [`Self::root_surface`], and name
    /// existing logical surfaces. Runtime-owned Child windows keep their
    /// ordinary whole-client placement.
    ///
    /// Every contribution uses the current output token, one candidate and one
    /// final publication. Deferred native children keep using that same host.
    /// Dockspace-owned menus retain their source placement scale and use the
    /// viewport's full bounds. A pane's independent framework popup keeps egui's
    /// ordinary viewport-point overlay semantics.
    /// Repeated passes must supply the same roster. Errors enter the existing
    /// native shutdown path and preserve the last committed workspace.
    ///
    /// When this run already owns raw input, it paints the complete roster
    /// admitted before that input was taken. A different roster announced here
    /// is eligible on the next run, whose complete output allowance is checked
    /// before taking more input. Text and IME events are never replayed.
    pub fn show_root_surfaces(&mut self, ui: &mut egui::Ui, placements: &[NativeSurfacePlacement]) {
        self.update_root_with_placements(ui, Some(placements));
    }

    /// Captures JSON for the current committed document without taking pane data.
    ///
    /// The application owns storage and preset names. Accepted but unpublished
    /// native input is not part of this snapshot. Capture is refused while an
    /// application command or unconsumed result owns the single slot, so saving
    /// cannot invalidate a queued document restore's sidecar preparation.
    ///
    /// # Errors
    ///
    /// Returns a queue/shutdown or original persistence capture error. The session
    /// must have been constructed with a document bootstrap or validated document.
    #[cfg(feature = "serde")]
    pub fn save_document_json(&self) -> Result<Vec<u8>, NativeLayoutRequestError> {
        lock_state(&self.state).save_document_json()
    }

    /// Queues one complete document restore after its ordered native prefix.
    ///
    /// Identity recognition includes closed and temporarily unloaded panes. A
    /// recognized missing pane may render an application-owned placeholder. The
    /// document must preserve the representative host surface and current lineage.
    /// Shared siblings already placed in its Root window are continued in the
    /// same atomic roster. Other surfaces require explicit Child hosting declarations
    /// and saved placement. They resume through budgeted creation after predecessor
    /// quiescence; undeclared surfaces do not implicitly create OS windows.
    /// Keys and declared identities remain core-owned; current presentation and
    /// focus require fresh native binding and output observations after restore.
    ///
    /// # Errors
    ///
    /// Returns the original queue, decoding, identity or preparation failure, or
    /// a missing host or invalid native hosting declaration before accepting the
    /// command. A queued stale workspace reports `Stale` without replacing newer
    /// committed state.
    #[cfg(feature = "serde")]
    pub fn request_document_restore_json(
        &self,
        bytes: &[u8],
        recognize: impl Fn(dockspace::runtime::DockspaceDocumentId, &str) -> bool,
    ) -> Result<(), NativeLayoutRequestError> {
        lock_state(&self.state).request_document_restore_json(bytes, recognize)
    }

    /// Restores a document and continues the external Root on its declared incoming surface.
    ///
    /// Core preserves declared Root/Child identities and placement together.
    /// Shared siblings present in the document continue with the representative
    /// in one atomic roster. Explicit Child declarations resume through normal
    /// budgeted creation after predecessor quiescence. The physical Root window
    /// survives the logical replacement.
    ///
    /// # Errors
    ///
    /// Returns ordinary restore errors, invalid native hosting declarations or
    /// an unavailable exact external Root window.
    #[cfg(feature = "serde")]
    pub fn request_document_restore_json_rebinding_root(
        &self,
        bytes: &[u8],
        incoming_root: SurfaceId,
        recognize: impl Fn(dockspace::runtime::DockspaceDocumentId, &str) -> bool,
    ) -> Result<(), NativeLayoutRequestError> {
        lock_state(&self.state).request_document_restore_json_rebinding_root(
            bytes,
            incoming_root,
            recognize,
        )
    }

    /// Takes the exact terminal status of the last queued application action.
    ///
    /// A new action remains busy until this result is consumed, so the native
    /// facade never overwrites or grows an unbounded action-result history.
    pub fn take_action_status(&self) -> Option<Result<NativeActionStatus, NativeActionFailure>> {
        lock_state(&self.state).take_action_status()
    }

    fn update_root(&mut self, ui: &mut egui::Ui) {
        self.update_root_with_placements(ui, None);
    }

    fn update_root_with_placements(
        &mut self,
        ui: &mut egui::Ui,
        placements: Option<&[NativeSurfacePlacement]>,
    ) {
        let context = ui.ctx().clone();
        let token = eframe::current_native_output_token();
        let (specs, request_repaint) = with_state(&self.state, |state| {
            let mut request_repaint = if state.error().is_some() {
                let (progress, cleanup_errors) = state.advance_shutdown(ui);
                for error in cleanup_errors {
                    state.record_cleanup_error(error);
                }
                render_native_error(state, token, ui);
                progress
            } else {
                let root_surface = state.root_surface();
                let update = match placements {
                    Some(placements) => state.update_root_surfaces(ui, placements),
                    None => state.update_surface(ui, root_surface),
                };
                match update {
                    Ok(NativeSurfaceUpdate::Semantic) => false,
                    Ok(NativeSurfaceUpdate::RetainPrevious) => {
                        paint_root_waiting(ui);
                        false
                    }
                    Err(error) => {
                        state.stop_current_output(error);
                        render_native_error(state, token, ui);
                        true
                    }
                }
            };
            let specs = match state.prepare_deferred_viewport_specs() {
                Ok(specs) => specs,
                Err(error) => {
                    state.stop_current_output(error);
                    render_native_error(state, token, ui);
                    request_repaint = true;
                    Vec::new()
                }
            };
            (specs, request_repaint)
        });
        if request_repaint {
            context.request_repaint_once_of(ViewportId::ROOT);
        }

        let state = Arc::clone(&self.state);
        let defaults = &self.child_window_options;
        let overrides = &self.child_window_overrides;
        declare_deferred_viewports(
            &context,
            &specs,
            &mut self.declared_child_icons,
            |surface| {
                overrides.get(&surface).map_or_else(
                    || defaults.clone(),
                    |options| defaults.with_overrides(options),
                )
            },
            |spec| {
                let state = lock_state(&self.state);
                crate::window_title::window_title(
                    state.view(),
                    state.panes(),
                    spec.binding().surface(),
                )
            },
            move |spec, ui, class| render_deferred_viewport(&state, spec, ui, class),
        );
    }
}

impl<P: PaneView + Send + 'static> eframe::App for NativeDockspaceApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.update_root(ui);
    }
}

fn render_deferred_viewport<P: PaneView + Send + 'static>(
    shared: &Arc<Mutex<NativeRuntimeState<P>>>,
    spec: DeferredViewportSpec,
    ui: &mut egui::Ui,
    class: ViewportClass,
) {
    with_state(shared, |state| {
        render_deferred_viewport_locked(state, spec, ui, class)
    });
}

fn render_deferred_viewport_locked<P: PaneView + Send + 'static>(
    state: &mut NativeRuntimeState<P>,
    spec: DeferredViewportSpec,
    ui: &mut egui::Ui,
    class: ViewportClass,
) {
    let token = eframe::current_native_output_token();
    if state.error().is_some() {
        let (progress, cleanup_errors) = state.advance_shutdown(ui);
        for error in cleanup_errors {
            state.record_cleanup_error(error);
        }
        if progress {
            ui.ctx().request_repaint_once_of(ViewportId::ROOT);
        }
        render_native_error(state, token, ui);
        return;
    }
    if class != ViewportClass::Deferred {
        paint_placeholder(ui, class, None);
        return;
    }
    let Some(token) = token else {
        paint_placeholder(ui, class, None);
        return;
    };

    let disposition = state.deferred_viewport_paint(token);
    match disposition {
        DeferredViewportPaint::Semantic(binding) if binding == spec.binding() => {
            match state.update_surface(ui, binding.surface()) {
                Ok(NativeSurfaceUpdate::Semantic) => {
                    let title = crate::window_title::window_title(
                        state.view(),
                        state.panes(),
                        binding.surface(),
                    );
                    if !ui
                        .ctx()
                        .viewport(|viewport| viewport.builder.title.as_deref() == Some(&title))
                    {
                        // The existing declaration builder diffs captions, so
                        // unchanged titles emit no OS command. Explicit host
                        // title commands remain effective until pane titles change.
                        ui.ctx().request_repaint_once_of(ViewportId::ROOT);
                    }
                }
                Ok(NativeSurfaceUpdate::RetainPrevious) => {
                    paint_placeholder(ui, class, Some(DeferredViewportPaint::Waiting));
                }
                Err(error) => {
                    state.stop_current_output(error);
                    ui.ctx().request_repaint_once_of(ViewportId::ROOT);
                    render_native_error(state, Some(token), ui);
                }
            }
        }
        DeferredViewportPaint::Semantic(_binding) => {
            state.stop_current_output(NativeHostProtocolError::OutputRouteAttachmentFailed.into());
            ui.ctx().request_repaint_once_of(ViewportId::ROOT);
            render_native_error(state, Some(token), ui);
        }
        DeferredViewportPaint::Created | DeferredViewportPaint::Staging(_) => {
            paint_placeholder(ui, class, Some(disposition));
        }
        DeferredViewportPaint::Retain(binding) => {
            if let Err(error) = state.stage_retain_previous(ui.ctx(), token, binding.surface()) {
                state.stop_current_output(error);
                ui.ctx().request_repaint_once_of(ViewportId::ROOT);
                state.render_error(ui, NativeErrorPresentation::NoFrame);
            } else {
                paint_placeholder(ui, class, Some(DeferredViewportPaint::Waiting));
            }
        }
        DeferredViewportPaint::Waiting => {
            paint_placeholder(ui, class, Some(disposition));
        }
    }
}

fn render_native_error<P: PaneView>(
    state: &mut NativeRuntimeState<P>,
    token: Option<eframe::NativeOutputToken>,
    ui: &mut egui::Ui,
) {
    let retained_overlay = token.is_some_and(|token| {
        if let Err(error) = state.stage_retained_overlay(token) {
            state.record_cleanup_error(error);
            return false;
        }
        request_retained_error_overlay()
    });
    let presentation = if retained_overlay {
        NativeErrorPresentation::RetainedOverlay
    } else {
        NativeErrorPresentation::NoFrame
    };
    state.render_error(ui, presentation);
}

fn request_retained_error_overlay() -> bool {
    eframe::retain_current_native_output_with_overlay()
}

struct NativePassFinalizer<P> {
    state: Weak<Mutex<NativeRuntimeState<P>>>,
}

impl<P: PaneView + Send + 'static> NativeOutputPassFinalizer for NativePassFinalizer<P> {
    fn on_service(&self, context: &egui::Context) -> NativeHostWake {
        let Some(state) = self.state.upgrade() else {
            return NativeHostWake::Wait;
        };
        with_state(&state, |state| match state.service_native_events(context) {
            Ok(wake) => wake,
            Err(error) => {
                state.stop_current_output(error);
                NativeHostWake::RepaintRootAfterCurrent
            }
        })
    }

    fn before_output_input(
        &self,
        context: &egui::Context,
        token: NativeOutputToken,
        has_input: bool,
    ) -> eframe::NativeOutputInputAdmission {
        let Some(state) = self.state.upgrade() else {
            return eframe::NativeOutputInputAdmission::Defer(NativeHostWake::Wait);
        };
        with_state(&state, |state| {
            match state.prepare_output_input(context, token, has_input) {
                Ok(admission) => admission,
                Err(error) => {
                    state.stop_current_output(error);
                    eframe::NativeOutputInputAdmission::Proceed
                }
            }
        })
    }

    fn finalize(&self, finalization: NativeOutputPassFinalization<'_>) -> NativeHostWake {
        let Some(state) = self.state.upgrade() else {
            return NativeHostWake::Wait;
        };
        let (context, token, ended_viewport, disposition, output) = finalization.into_parts();
        with_state(&state, |state| {
            let result = if token.viewport_id() == ended_viewport {
                state.settle_egui_pass(
                    context,
                    output,
                    token,
                    disposition == egui::OutputPassDisposition::Repeat,
                )
            } else {
                Err(NativeHostProtocolError::MultipassOutputChanged.into())
            };
            if let Err(error) = result {
                if state.error().is_some() {
                    state.record_cleanup_error(error);
                } else {
                    state.stop_current_output(error);
                    return NativeHostWake::RepaintRoot;
                }
            }
            NativeHostWake::Wait
        })
    }
}

fn paint_root_waiting(ui: &mut egui::Ui) {
    ui.painter()
        .rect_filled(ui.max_rect(), 0.0, egui::Color32::from_gray(24));
    ui.centered_and_justified(|ui| {
        ui.label("Waiting for dockspace authority…");
    });
}

#[cfg(test)]
#[cfg(target_os = "linux")]
#[path = "retained_focus_tests.rs"]
mod retained_focus_tests;

#[cfg(test)]
#[path = "app/notification_tests.rs"]
mod notification_tests;

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex, PoisonError};

    use dockspace::model::{
        DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId,
        RootId, SurfaceId,
    };
    use eframe::egui;
    use egui_dockspace::{DockInputConfig, DockStyle, DockingActivation, PaneView};

    pub(super) struct Pane;

    impl PaneView for Pane {
        fn title(&self, _: ItemId) -> Option<egui::WidgetText> {
            Some("Document".into())
        }

        fn ui(&mut self, _: ItemId, _: &mut egui::Ui) -> egui_dockspace::PaneResponse {
            Default::default()
        }
    }

    pub(super) fn app() -> super::NativeDockspaceApp<Pane> {
        let surface = SurfaceId::new(1);
        let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
            surface,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        )])
        .unwrap();
        let session =
            dockspace::runtime::DockspaceSession::from_layout(layout, Default::default()).unwrap();
        super::NativeDockspaceApp::new(
            egui::Id::unique("initial-native-options"),
            session,
            surface,
            Pane,
            DockStyle::default(),
        )
        .unwrap()
    }

    #[test]
    fn input_can_be_configured_before_the_bootstrap_registration_commits() {
        let config = DockInputConfig {
            docking: DockingActivation::HoldToEnable(egui::Modifiers::ALT),
            cancel_drag: None,
        };
        let app = app().with_input(config).unwrap();
        assert_eq!(app.input(), config);
        assert!(app.take_action_status().is_none());
    }

    #[test]
    fn pending_work_explains_queued_commands_and_keeps_snapshots_read_only() {
        use crate::{NativeCommand, NativeWaitReason};

        let app = app();
        let before = app.pending_work();
        assert!(before.contains(NativeWaitReason::RootPresentation));
        assert!(!before.contains(NativeWaitReason::ApplicationAction));
        app.request_command(NativeCommand::SelectItem {
            item: ItemId::new(1),
        })
        .unwrap();
        let pending = app.pending_work();
        assert!(
            pending
                .iter()
                .any(|reason| reason == NativeWaitReason::ApplicationAction)
        );
        assert_eq!(pending.is_empty(), app.is_quiescent());
        assert!(!before.contains(NativeWaitReason::ApplicationAction));
        assert!(format!("{pending:?}").contains("ApplicationAction"));
        assert!(
            app.take_action_status().is_none(),
            "reading diagnostics never executes a command"
        );
    }

    #[test]
    fn surface_window_options_inherit_defaults_and_can_be_replaced_and_cleared() {
        use crate::NativeChildWindowOptions;

        let document = SurfaceId::new(2);
        let tools = SurfaceId::new(3);
        let defaults = NativeChildWindowOptions {
            decorations: Some(true),
            taskbar: Some(true),
            window_level: Some(egui::WindowLevel::Normal),
            icon: Some(Arc::new(egui::IconData {
                rgba: vec![255; 4],
                width: 1,
                height: 1,
            })),
        };
        let tool_overrides = NativeChildWindowOptions {
            taskbar: Some(false),
            window_level: Some(egui::WindowLevel::AlwaysOnTop),
            ..Default::default()
        };
        let mut app = app().with_child_window_options_for(tools, tool_overrides.clone());
        assert_eq!(
            app.child_window_options(),
            &NativeChildWindowOptions::default()
        );
        assert_eq!(app.child_window_options_for(document).decorations, None);
        assert_eq!(
            app.child_window_options_for(document)
                .viewport_builder()
                .decorations,
            Some(true)
        );
        app = app.with_child_window_options(defaults.clone());
        assert_eq!(app.child_window_options(), &defaults);
        assert_eq!(app.child_window_options_for(document), defaults);
        let resolved = app.child_window_options_for(tools);
        assert_eq!(resolved.decorations, Some(true));
        assert_eq!(resolved.taskbar, Some(false));
        assert_eq!(resolved.window_level, Some(egui::WindowLevel::AlwaysOnTop));
        assert_eq!(resolved.icon, defaults.icon);

        let new_defaults = NativeChildWindowOptions {
            decorations: Some(false),
            ..defaults
        };
        app.set_child_window_options(new_defaults.clone());
        assert_eq!(app.child_window_options_for(document), new_defaults);
        assert_eq!(app.child_window_options_for(tools).decorations, Some(false));
        assert_eq!(app.child_window_options_for(tools).taskbar, Some(false));

        let replacement = NativeChildWindowOptions {
            decorations: Some(true),
            ..Default::default()
        };
        assert_eq!(
            app.set_child_window_options_for(tools, replacement.clone()),
            Some(tool_overrides)
        );
        assert_eq!(app.child_window_options_for(tools).taskbar, Some(true));
        assert_eq!(app.clear_child_window_options_for(tools), Some(replacement));
        assert_eq!(app.child_window_options_for(tools), new_defaults);
        assert!(app.clear_child_window_options_for(tools).is_none());
    }

    #[derive(Default)]
    struct LateDiscardProbe {
        remaining: usize,
    }

    impl egui::plugin::Plugin for LateDiscardProbe {
        fn debug_name(&self) -> &'static str {
            "egui_dockspace_native::late_discard_request"
        }

        fn output_hook(&mut self, _context: &egui::Context, output: &mut egui::FullOutput) {
            if self.remaining == 0 {
                return;
            }
            self.remaining -= 1;
            output
                .platform_output
                .request_discard_reasons
                .push(egui::RepaintCause::new_reason(
                    "outer application requested a later pass",
                ));
        }
    }

    #[test]
    fn post_output_settlement_observes_a_late_discard() {
        let context = egui::Context::default();
        let observations = Arc::new(Mutex::new(Vec::new()));
        context
            .plugin_or_default::<LateDiscardProbe>()
            .lock()
            .remaining = 1;

        let finalizer_observations = Arc::clone(&observations);
        let mut output = context.run_ui_with_pass_finalizer(
            egui::RawInput::default(),
            |ui| {
                ui.label("native dockspace ui has returned");
            },
            move |_context, _ended_viewport, disposition, _output| {
                finalizer_observations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(disposition);
            },
        );
        output.textures_delta.clear();

        assert_eq!(
            *observations.lock().unwrap_or_else(PoisonError::into_inner),
            [
                egui::OutputPassDisposition::Repeat,
                egui::OutputPassDisposition::Terminal,
            ],
            "only the pass followed by another egui pass is discarded"
        );
    }

    #[test]
    fn denied_discard_is_the_terminal_pass() {
        let context = egui::Context::default();
        context.options_mut(|options| {
            options.max_passes = std::num::NonZeroUsize::MIN;
        });
        let observations = Arc::new(Mutex::new(Vec::new()));
        context
            .plugin_or_default::<LateDiscardProbe>()
            .lock()
            .remaining = 1;

        let finalizer_observations = Arc::clone(&observations);
        let mut output = context.run_ui_with_pass_finalizer(
            egui::RawInput::default(),
            |ui| {
                ui.label("the configured pass limit denies the late request");
            },
            move |_context, _ended_viewport, disposition, _output| {
                finalizer_observations
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(disposition);
            },
        );
        output.textures_delta.clear();

        assert_eq!(
            *observations.lock().unwrap_or_else(PoisonError::into_inner),
            [egui::OutputPassDisposition::Terminal],
            "a denied discard must still settle the current output"
        );
    }
}
