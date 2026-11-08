//! Bounded opt-in continuation of the existing physical Root window.

use super::*;
use egui_dockspace_native::NativeCommand;

const INCOMING_ROOT: SurfaceId = SurfaceId::new(1_000);
#[cfg(feature = "serde")]
const REBOUND_PRESET: &str = "Rebound Root";

#[derive(Clone, Copy)]
enum Replacement {
    Layout,
    #[cfg(feature = "serde")]
    Document,
    Default,
    #[cfg(feature = "serde")]
    Preset,
}

impl Replacement {
    fn operation(self) -> &'static str {
        match self {
            Self::Layout => "layout",
            #[cfg(feature = "serde")]
            Self::Document => "document",
            Self::Default => "default",
            #[cfg(feature = "serde")]
            Self::Preset => "preset",
        }
    }

    fn marker(self) -> &'static str {
        match self {
            Self::Layout => "ROOT_REBIND_LAYOUT_EDIT",
            #[cfg(feature = "serde")]
            Self::Document => "ROOT_REBIND_DOCUMENT_EDIT",
            Self::Default => "ROOT_REBIND_DEFAULT_EDIT",
            #[cfg(feature = "serde")]
            Self::Preset => "ROOT_REBIND_PRESET_EDIT",
        }
    }

    fn accepts(self, status: &NativeActionStatus) -> bool {
        match (self, status) {
            (
                Self::Layout | Self::Default,
                NativeActionStatus::LayoutReset { previous, current },
            ) => previous != current,
            #[cfg(feature = "serde")]
            (
                Self::Document | Self::Preset,
                NativeActionStatus::DocumentRestored { previous, current },
            ) => previous != current,
            _ => false,
        }
    }
}

enum Step {
    FocusRequested,
    Focused,
    Committed,
    Presented,
    Input,
    Complete,
}

pub(super) struct RootRebinding {
    step: Step,
    old_surface: SurfaceId,
    incoming_surface: SurfaceId,
    current_surface: SurfaceId,
    window_id: u64,
    tabs: Vec<SavedTabs>,
    replacement: Replacement,
    #[cfg(feature = "serde")]
    document: Option<(Vec<u8>, SurfaceId, Vec<SavedTabs>)>,
    #[cfg(feature = "serde")]
    preset: Option<(SurfaceId, Vec<SavedTabs>)>,
}

impl RootRebinding {
    pub(super) fn start(
        smoke: &mut EditorSmoke,
        app: &mut NativeExampleApp,
    ) -> Result<Self, String> {
        smoke.check_text(app)?;
        let old_surface = app.dockspace.root_surface();
        let window_id = Self::root_window(app).ok_or("the current Editor has no Root output")?;
        let layout = editor_layout_on_surface(false, INCOMING_ROOT);
        let tabs = Self::layout_tabs(smoke, &layout);
        // Replacement inherits actual focus. Establish that antecedent after
        // the preceding child retirements instead of assuming a central pane
        // owns focus merely because it is selected in the incoming layout.
        let request = app
            .dockspace
            .request_command(NativeCommand::SelectItem { item: EDITOR });
        smoke.submit(
            app,
            request,
            "Smoke: focus Document before Root rebinding",
            Stage::RootRebinding,
        )?;
        Ok(Self {
            step: Step::FocusRequested,
            old_surface,
            incoming_surface: INCOMING_ROOT,
            current_surface: old_surface,
            window_id,
            tabs,
            replacement: Replacement::Layout,
            #[cfg(feature = "serde")]
            document: None,
            #[cfg(feature = "serde")]
            preset: None,
        })
    }

    pub(super) fn advance(
        &mut self,
        smoke: &mut EditorSmoke,
        app: &mut NativeExampleApp,
    ) -> Result<bool, String> {
        let operation = self.replacement.operation();
        match self.step {
            Step::FocusRequested => {
                let Some(status) = smoke.take_status(app)? else {
                    return Ok(false);
                };
                if !matches!(
                    status,
                    NativeActionStatus::Dock(DockspaceActionStatus::Applied(
                        DockspaceActionOutcome::Selected { item: EDITOR, .. }
                    ))
                ) {
                    return Err(format!(
                        "Document focus before Root rebinding failed: {status:?}"
                    ));
                }
                self.step = Step::Focused;
            }
            Step::Focused if smoke.settled(app) => {
                let focused = app.dockspace.with_panes(|panes| {
                    panes.panes[&EDITOR]
                        .input
                        .is_some_and(|(viewport, _, focused, _)| {
                            viewport == egui::ViewportId::ROOT && focused
                        })
                });
                if !focused {
                    return Ok(false);
                }
                if Self::root_window(app) != Some(self.window_id) {
                    return Err("Document focus changed the original Root window".to_owned());
                }
                #[cfg(feature = "serde")]
                {
                    let bytes = app
                        .dockspace
                        .save_document_json()
                        .map_err(|error| error.to_string())?;
                    self.document = Some((bytes, self.old_surface, smoke.capture_tabs(app)?));
                    eprintln!(
                        "editor-smoke: programmatic=root-rebind-capture incoming_surface={} window_id={} registry=8 text=preserved tabs=preserved selected=preserved",
                        self.old_surface.get(),
                        self.window_id
                    );
                }
                let request = app.dockspace.request_layout_reset_rebinding_root(
                    editor_layout_on_surface(false, INCOMING_ROOT),
                    INCOMING_ROOT,
                );
                smoke.submit(
                    app,
                    request,
                    "Smoke: rebind Root with layout reset",
                    Stage::RootRebinding,
                )?;
                eprintln!(
                    "editor-smoke: programmatic=root-rebind-layout-requested old_surface={} incoming_surface={} window_id={}",
                    self.old_surface.get(),
                    self.incoming_surface.get(),
                    self.window_id
                );
                self.step = Step::Committed;
            }
            Step::Committed => {
                let Some(status) = smoke.take_status(app)? else {
                    return Ok(false);
                };
                if !self.replacement.accepts(&status) {
                    return Err(format!(
                        "Root {operation} rebinding did not commit: {status:?}"
                    ));
                }
                // Read the published host after the core commit; incoming IDs may rebase.
                self.current_surface = app.dockspace.root_surface();
                self.check_layout(smoke, app)?;
                smoke.check_text(app)?;
                app.dockspace.with_panes_mut(|panes| {
                    for pane in panes.panes.values_mut() {
                        pane.input = None;
                        pane.window_id = None;
                    }
                });
                eprintln!(
                    "editor-smoke: programmatic=root-rebind-{operation}-committed old_surface={} incoming_surface={} new_surface={} window_id={} logically-committed=true old-absent=true registry=8 tabs=preserved selected=preserved fresh-presented=required",
                    self.old_surface.get(),
                    self.incoming_surface.get(),
                    self.current_surface.get(),
                    self.window_id
                );
                self.step = Step::Presented;
            }
            Step::Presented if smoke.settled(app) => {
                self.check_layout(smoke, app)?;
                smoke.check_text(app)?;
                let Some(window_id) = Self::root_window(app) else {
                    return Ok(false);
                };
                if window_id != self.window_id {
                    return Err("Root rebinding replaced the original OS window".to_owned());
                }
                eprintln!(
                    "editor-smoke: programmatic=root-rebind-{operation}-presented old_surface={} incoming_surface={} new_surface={} window_id={window_id} old-absent=true registry=8 text=preserved tabs=preserved selected=preserved fresh-presented=true quiescent=true",
                    self.old_surface.get(),
                    self.incoming_surface.get(),
                    self.current_surface.get()
                );
                self.step = Step::Input;
                smoke.set_stage(Stage::RootRebinding);
            }
            Step::Input if smoke.settled(app) => {
                self.check_layout(smoke, app)?;
                if Self::root_window(app) != Some(self.window_id) {
                    return Err("the fresh Editor output lost the original Root window".to_owned());
                }
                let marker = self.replacement.marker();
                if !smoke.input(app, EDITOR, marker, false)? {
                    return Ok(false);
                }
                smoke.check_text(app)?;
                #[cfg(feature = "serde")]
                if matches!(self.replacement, Replacement::Layout) {
                    app.preset_name = REBOUND_PRESET.to_owned();
                    app.save_named_preset()?;
                    self.preset = Some((self.current_surface, smoke.capture_tabs(app)?));
                }
                #[cfg(feature = "serde")]
                if matches!(self.replacement, Replacement::Preset)
                    && let Some((bytes, incoming_surface, tabs)) = self.document.take()
                {
                    let keys = app.dockspace.with_panes(|panes| {
                        panes
                            .panes
                            .values()
                            .map(|pane| pane.key)
                            .collect::<Vec<_>>()
                    });
                    let request = app.dockspace.request_document_restore_json_rebinding_root(
                        &bytes,
                        incoming_surface,
                        |document, key| document == DOCUMENT_ID && keys.contains(&key),
                    );
                    smoke.submit(
                        app,
                        request,
                        "Smoke: rebind Root from saved JSON",
                        Stage::RootRebinding,
                    )?;
                    self.old_surface = self.current_surface;
                    self.incoming_surface = incoming_surface;
                    self.tabs = tabs;
                    self.replacement = Replacement::Document;
                    self.step = Step::Committed;
                    eprintln!(
                        "editor-smoke: programmatic=root-rebind-document-requested old_surface={} incoming_surface={} window_id={}",
                        self.old_surface.get(),
                        self.incoming_surface.get(),
                        self.window_id
                    );
                    return Ok(false);
                }
                if matches!(self.replacement, Replacement::Layout) {
                    let request = app.request_default_layout();
                    smoke.submit(
                        app,
                        request,
                        "Smoke: Default after Root rebinding",
                        Stage::RootRebinding,
                    )?;
                    self.old_surface = self.current_surface;
                    self.incoming_surface = SURFACE;
                    self.tabs = Self::layout_tabs(smoke, &app.default_layout);
                    self.replacement = Replacement::Default;
                    self.step = Step::Committed;
                    eprintln!(
                        "editor-smoke: programmatic=root-rebind-default-requested old_surface={} incoming_surface={} window_id={}",
                        self.old_surface.get(),
                        self.incoming_surface.get(),
                        self.window_id
                    );
                    return Ok(false);
                }
                #[cfg(feature = "serde")]
                if matches!(self.replacement, Replacement::Default)
                    && let Some((incoming_surface, tabs)) = self.preset.take()
                {
                    app.preset_name = REBOUND_PRESET.to_owned();
                    let request = app.request_named_preset();
                    smoke.submit(
                        app,
                        request,
                        "Smoke: named preset after Root rebinding",
                        Stage::RootRebinding,
                    )?;
                    self.old_surface = self.current_surface;
                    self.incoming_surface = incoming_surface;
                    self.tabs = tabs;
                    self.replacement = Replacement::Preset;
                    self.step = Step::Committed;
                    eprintln!(
                        "editor-smoke: programmatic=root-rebind-preset-requested old_surface={} incoming_surface={} window_id={}",
                        self.old_surface.get(),
                        self.incoming_surface.get(),
                        self.window_id
                    );
                    return Ok(false);
                }
                self.step = Step::Complete;
                return Ok(true);
            }
            Step::Complete => return Ok(true),
            _ => {}
        }
        Ok(false)
    }

    fn layout_tabs(smoke: &EditorSmoke, layout: &DockspaceLayout) -> Vec<SavedTabs> {
        smoke
            .expected
            .keys()
            .map(|item| {
                let pane = layout
                    .view()
                    .item(*item)
                    .expect("the complete eight-pane layout");
                SavedTabs {
                    item: *item,
                    items: pane.tabs().items().to_vec(),
                    selected: pane.tabs().selected(),
                }
            })
            .collect()
    }

    pub(super) fn check_layout(
        &self,
        smoke: &EditorSmoke,
        app: &NativeExampleApp,
    ) -> Result<(), String> {
        let current = app.dockspace.root_surface();
        let intact = current == self.current_surface
            && current != self.old_surface
            && app.dockspace.with_view(|view| {
                view.surface(self.old_surface).is_none()
                    && view.surface(current).is_some()
                    && view.surfaces().count() == 1
                    && smoke.expected.keys().all(|item| {
                        view.item(*item)
                            .is_some_and(|pane| pane.surface() == current)
                    })
            })
            && smoke.capture_tabs(app)? == self.tabs;
        intact.then_some(()).ok_or_else(|| {
            "Root rebinding retained the old surface or changed pane IDs, tabs, selection or the published host".to_owned()
        })
    }

    fn root_window(app: &NativeExampleApp) -> Option<u64> {
        app.dockspace.with_panes(|panes| {
            let pane = &panes.panes[&EDITOR];
            let (viewport, _, _, _) = pane.input?;
            (viewport == egui::ViewportId::ROOT)
                .then_some(pane.window_id)
                .flatten()
        })
    }
}
