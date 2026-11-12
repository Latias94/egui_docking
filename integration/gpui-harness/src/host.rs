//! Retained application-facing host and its private runtime state.

use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
    time::{Duration, Instant},
};

use anyhow::{Result, ensure};
use dockspace::{
    geometry::{LogicalPoint, LogicalRect, LogicalSize},
    model::{DockspaceNodeView, DockspaceView, ItemId, SurfaceId},
    policy::{CloseCapability, DockItemRule},
    runtime::{DockspaceSession, SurfacePresentationArtifact},
};
use open_gpui::{
    AnyView, App, Bounds, FocusHandle, GlobalElementId, IntoElement, Pixels, SharedString, Window,
    WindowId,
};

use crate::{GpuiDockspaceConfig, continuation, element, input, presentation};

pub(crate) type SharedHost = Rc<RefCell<HostState>>;
pub(crate) type BoundsProbe = Rc<Cell<Option<Bounds<Pixels>>>>;

/// An application-owned view with a stable docking identity.
#[derive(Clone)]
pub struct DockPane {
    pub(crate) id: ItemId,
    pub(crate) title: SharedString,
    pub(crate) view: AnyView,
    pub(crate) minimum_size: LogicalSize,
}

impl DockPane {
    /// Associates a retained GPUI view with an item in the session.
    pub fn new(id: ItemId, title: impl Into<SharedString>, view: impl Into<AnyView>) -> Self {
        Self {
            id,
            title: title.into(),
            view: view.into(),
            minimum_size: LogicalSize::new(120.0, 90.0).expect("the default pane minimum is valid"),
        }
    }

    /// Sets the minimum content size reported to the docking layout.
    #[must_use]
    pub fn with_minimum_size(mut self, minimum_size: LogicalSize) -> Self {
        self.minimum_size = minimum_size;
        self
    }
}

/// A composable retained host for one logical surface and its contained windows.
///
/// Clones share the same session and application views. Render this host at one
/// stable element location in one GPUI window. The first window and global
/// element identity remain its owners for the host's lifetime; removing and
/// reinserting it at that location preserves its pane entities.
///
/// This adapter does not create native child windows or handle content-close
/// approval. Construct the session with native surfaces and pane closing
/// disabled. Native multiview and native placement are not emulated.
#[derive(Clone)]
pub struct GpuiDockspace {
    pub(crate) host: SharedHost,
}

impl GpuiDockspace {
    /// Creates a retained host without starting an application or opening a window.
    ///
    /// # Errors
    ///
    /// Rejects multiple surfaces, mismatched identities, missing or duplicate
    /// panes, unsupported native/close policies, and an occupied input producer.
    pub fn new(
        session: DockspaceSession,
        surface: SurfaceId,
        panes: impl IntoIterator<Item = DockPane>,
        config: GpuiDockspaceConfig,
        cx: &mut App,
    ) -> Result<Self> {
        Ok(Self {
            host: Rc::new(RefCell::new(HostState::from_session(
                session, surface, panes, config, cx,
            )?)),
        })
    }

    /// Builds an owned element that fills the space allocated by its parent.
    pub fn element(&self) -> impl IntoElement + use<> {
        element::DockspaceElement::new(self.host.clone())
    }

    /// Inspects the published layout without exposing mutable runtime state.
    pub fn with_view<R>(&self, inspect: impl FnOnce(DockspaceView<'_>) -> R) -> R {
        inspect(self.host.borrow().session.view())
    }

    /// Returns the most recent runtime error.
    ///
    /// A rejected additional mount paints its own diagnostic and leaves the
    /// original host operational.
    #[must_use]
    pub fn error(&self) -> Option<String> {
        self.host.borrow().error.clone()
    }

    /// Returns whether the published session owns a pointer gesture.
    #[must_use]
    pub fn has_active_gesture(&self) -> bool {
        self.host.borrow().session.has_active_gesture()
    }
}

pub(crate) struct HostState {
    pub(crate) session: DockspaceSession,
    pub(crate) surface: SurfaceId,
    pub(crate) panels: BTreeMap<ItemId, DockPane>,
    pub(crate) config: GpuiDockspaceConfig,
    pub(crate) inputs: Vec<input::QueuedPointer>,
    pub(crate) pending: Option<presentation::PendingSubmission>,
    pub(crate) packet: Option<Rc<element::Packet>>,
    pub(crate) presented: Option<(u64, SurfacePresentationArtifact)>,
    pub(crate) bounds: Option<LogicalRect>,
    pub(crate) pointer: Option<LogicalPoint>,
    pub(crate) dirty: bool,
    pub(crate) serial: u64,
    pub(crate) error: Option<String>,
    pub(crate) publication: open_gpui::PrepaintPublicationId,
    pub(crate) capture: Option<open_gpui::PointerCaptureHandle>,
    pub(crate) focus: FocusHandle,
    pub(crate) surface_probe: Option<BoundsProbe>,
    pub(crate) input_frame: Option<continuation::InputFrame>,
    pub(crate) wake: Option<continuation::Wake>,
    pub(crate) clock: open_gpui::BackgroundExecutor,
    pub(crate) clock_origin: Instant,
    owner: Option<(WindowId, Option<GlobalElementId>)>,
    pub(crate) attached: bool,
}

impl HostState {
    fn from_session(
        mut session: DockspaceSession,
        surface: SurfaceId,
        panes: impl IntoIterator<Item = DockPane>,
        config: GpuiDockspaceConfig,
        cx: &mut App,
    ) -> Result<Self> {
        ensure!(
            session.view().surfaces().count() == 1 && session.view().surface(surface).is_some(),
            "GPUI dockspace requires exactly the supplied logical surface"
        );
        ensure!(
            !session.policy().allows_native_surfaces(),
            "GPUI dockspace requires a policy with native surfaces disabled"
        );
        ensure!(
            f32::from(config.text_size).is_finite() && config.text_size > open_gpui::px(0.0),
            "GPUI dockspace text size must be positive and finite"
        );
        let mut panels = BTreeMap::new();
        for pane in panes {
            let item = pane.id;
            ensure!(
                panels.insert(item, pane).is_none(),
                "duplicate GPUI pane {item:?}"
            );
        }
        let surface_view = session
            .view()
            .surface(surface)
            .expect("surface was checked");
        let mut nodes = Vec::new();
        if let Some(root) = surface_view.main_root() {
            nodes.extend(root.content());
        }
        for contained in surface_view.contained() {
            nodes.extend(contained.root().and_then(|root| root.content()));
        }
        let mut items = Vec::new();
        while let Some(node) = nodes.pop() {
            collect_items(node, &mut nodes, &mut items);
        }
        // Policy queries are performed only during construction. The copy is
        // never installed into the caller's session.
        let policy = session.policy();
        let mut disabled = policy.clone();
        disabled.set_close_capability(CloseCapability::Disabled);
        let default_close_disabled = *policy == disabled;
        for item in items {
            ensure!(
                panels.contains_key(&item),
                "open item {item:?} has no GPUI pane"
            );
            let rule = disabled.set_item_rule(item, DockItemRule::default());
            ensure!(
                rule.and_then(|rule| rule.close_capability())
                    .map_or(default_close_disabled, |capability| {
                        capability == CloseCapability::Disabled
                    }),
                "GPUI pane {item:?} enables unsupported content closing"
            );
        }
        session.enable_host_pointer()?;
        let clock = cx.background_executor().clone();
        let clock_origin = clock.now();
        Ok(Self {
            session,
            surface,
            panels,
            config,
            inputs: Vec::new(),
            pending: None,
            packet: None,
            presented: None,
            bounds: None,
            pointer: None,
            dirty: true,
            serial: 0,
            error: None,
            publication: open_gpui::PrepaintPublicationId::new(),
            capture: None,
            focus: cx.focus_handle(),
            surface_probe: None,
            input_frame: None,
            wake: None,
            clock,
            clock_origin,
            owner: None,
            attached: true,
        })
    }

    pub(crate) fn ensure_mount(
        &mut self,
        id: Option<&GlobalElementId>,
        window: &Window,
    ) -> Result<bool> {
        let window_id = window.window_handle().window_id();
        let first_mount = self.owner.is_none();
        if let Some((owner_window, owner_element)) = &self.owner {
            ensure!(
                *owner_window == window_id && owner_element.as_ref() == id,
                "a GPUI dockspace can only be rendered at its original window and element location"
            );
        } else {
            self.owner = Some((window_id, id.cloned()));
        }
        let newly_attached = !self.attached;
        if newly_attached {
            self.session.enable_host_pointer()?;
            self.attached = true;
            self.dirty = true;
        }
        Ok(first_mount)
    }

    pub(crate) fn observed_at(&self) -> Duration {
        self.clock
            .now()
            .saturating_duration_since(self.clock_origin)
    }

    pub(crate) fn fail(&mut self, error: impl std::fmt::Display) {
        let message = error.to_string();
        eprintln!("GPUI docking: {message}");
        self.error = Some(message);
    }
}

fn collect_items<'a>(
    node: DockspaceNodeView<'a>,
    nodes: &mut Vec<DockspaceNodeView<'a>>,
    items: &mut Vec<ItemId>,
) {
    if let Some(tabs) = node.tabs() {
        items.extend_from_slice(tabs.items());
    } else if let Some(split) = node.split() {
        nodes.extend(split.children());
    }
}
