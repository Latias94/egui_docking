//! Finite W16/O256 fixture identities, outside the runtime storage allowance.

use super::*;
use egui_dockspace_native::NativeCommand;

const SATELLITES: usize = 14;

#[derive(Clone, Copy)]
struct Satellite {
    item: ItemId,
    root: RootId,
    token: Option<eframe::NativeOutputToken>,
    surface: Option<SurfaceId>,
    destroyed: bool,
    redock_applied: bool,
}

pub(super) struct CombinedPressure {
    satellites: [Satellite; SATELLITES],
    next_create: usize,
    create_queued: bool,
    next_redock: usize,
    redock_queued: bool,
    child: Option<eframe::NativeOutputToken>,
    pub(super) child_destroyed: bool,
    full_roster_checked: bool,
    pub(super) final_roster_checked: bool,
}

pub(super) fn with_satellites(
    mut surface: DockspaceSurfaceLayout,
) -> Result<DockspaceSurfaceLayout, dockspace::geometry::GeometryError> {
    for id in 3..=16 {
        surface = surface.with_contained(DockspaceContainedLayout::new(
            FloatingPresentationId::new(id - 1),
            DockspaceRootLayout::new(
                RootId::new(id),
                DockspaceNode::central_tabs([ItemId::new(id)]),
            ),
            LogicalRect::new(80.0, 80.0, 240.0, 180.0)?,
        ));
    }
    Ok(surface)
}

impl CombinedPressure {
    pub(super) fn new() -> Self {
        Self {
            satellites: std::array::from_fn(|index| {
                let id = u64::try_from(index).unwrap() + 3;
                Satellite {
                    item: ItemId::new(id),
                    root: RootId::new(id),
                    token: None,
                    surface: None,
                    destroyed: false,
                    redock_applied: false,
                }
            }),
            next_create: 0,
            create_queued: false,
            next_redock: 0,
            redock_queued: false,
            child: None,
            child_destroyed: false,
            full_roster_checked: false,
            final_roster_checked: false,
        }
    }

    pub(super) fn observe_pane(&mut self, item: ItemId, token: eframe::NativeOutputToken) {
        if let Some(satellite) = self.satellites.iter_mut().find(|entry| entry.item == item) {
            if let Some(previous) = satellite.token {
                assert_eq!(previous.viewport_id(), token.viewport_id());
                assert_eq!(previous.window_id(), token.window_id());
                assert!(previous.same_context(token));
            } else {
                satellite.token = Some(token);
            }
        }
    }

    pub(super) fn is_satellite(&self, viewport: ViewportId) -> bool {
        self.satellites.iter().any(|entry| {
            entry
                .token
                .is_some_and(|token| token.viewport_id() == viewport)
        })
    }

    pub(super) fn observe_child(&mut self, token: eframe::NativeOutputToken) {
        assert!(self.child.replace(token).is_none());
    }

    pub(super) fn observe_destroyed(&mut self, event: eframe::NativeWindowEvent<'_>) {
        if !matches!(event.event(), winit::event::WindowEvent::Destroyed) {
            return;
        }
        if let Some(satellite) = self.satellites.iter_mut().find(|entry| {
            entry.token.is_some_and(|token| {
                token.window_id() == event.window_id()
                    && event
                        .viewport_id()
                        .is_none_or(|viewport| viewport == token.viewport_id())
            })
        }) {
            satellite.destroyed = true;
        }
        if self.child.is_some_and(|token| {
            token.window_id() == event.window_id()
                && event
                    .viewport_id()
                    .is_none_or(|viewport| viewport == token.viewport_id())
        }) {
            self.child_destroyed = true;
        }
    }

    pub(super) fn on_action(&mut self, outcome: &DockspaceActionOutcome) -> bool {
        match *outcome {
            DockspaceActionOutcome::NativeRootTearOffRequested {
                root,
                target_surface,
                ..
            } => {
                let Some(entry) = self.satellites.get_mut(self.next_create) else {
                    return false;
                };
                if root != entry.root {
                    return false;
                }
                assert!(self.create_queued);
                assert!(entry.surface.replace(target_surface).is_none());
                true
            }
            DockspaceActionOutcome::RootDocked {
                root,
                changed: true,
                ..
            } => {
                let Some(entry) = self.satellites.get_mut(self.next_redock) else {
                    return false;
                };
                if root != entry.root {
                    return false;
                }
                assert!(self.redock_queued);
                assert!(!entry.redock_applied);
                entry.redock_applied = true;
                true
            }
            _ => false,
        }
    }

    pub(super) fn bootstrap(
        &mut self,
        runtime: &NativeDockspaceApp<Panes>,
    ) -> Result<bool, String> {
        let Some(entry) = self.satellites.get(self.next_create) else {
            return Ok(true);
        };
        if !self.create_queued && runtime.is_quiescent() {
            let column = u32::try_from(self.next_create % 4).unwrap();
            let row = u32::try_from(self.next_create / 4).unwrap();
            runtime
                .request_command(NativeCommand::MoveRootToNewWindow {
                    root: entry.root,
                    placement: NativeWindowPlacement::new(
                        PhysicalRect::new(
                            710.0 + f64::from(column) * 280.0,
                            60.0 + f64::from(row) * 220.0,
                            240.0,
                            180.0,
                        )
                        .unwrap(),
                    ),
                })
                .map_err(|error| format!("satellite tear-off: {error}"))?;
            self.create_queued = true;
        }
        if let Some(surface) = entry.surface
            && entry.token.is_some()
            && runtime.is_surface_presented(surface)
            && runtime.is_surface_presented(ROOT_SURFACE)
            && runtime.is_quiescent()
            && runtime.with_view(|view| {
                view.item(entry.item).is_some_and(|item| {
                    item.surface() == surface
                        && item.root() == entry.root
                        && item.contained().is_none()
                })
            }) == Some(true)
        {
            eprintln!(
                "output-pressure: satellite {} first-live committed",
                entry.item.get()
            );
            self.next_create += 1;
            self.create_queued = false;
        }
        Ok(false)
    }

    pub(super) fn check_full_roster(
        &mut self,
        root: eframe::NativeOutputToken,
        roster: Option<eframe::NativeViewportRoster<'_>>,
    ) {
        assert_eq!(self.next_create, SATELLITES);
        let roster = roster.expect("combined pressure must keep complete root roster capture");
        assert_eq!(roster.records().len(), 16);
        let child = self
            .child
            .expect("the protected child has a real output identity");
        for token in std::iter::once(root)
            .chain(std::iter::once(child))
            .chain(self.satellites.iter().map(|entry| entry.token.unwrap()))
        {
            assert!(root.same_context(token));
            assert_eq!(
                roster
                    .records()
                    .iter()
                    .filter(|record| {
                        record.viewport_id() == token.viewport_id()
                            && record.window_id() == token.window_id()
                    })
                    .count(),
                1,
                "every exact expected window must occur once in this full root roster"
            );
        }
        assert!(!self.child_destroyed);
        assert!(self.satellites.iter().all(|entry| !entry.destroyed));
        self.full_roster_checked = true;
        eprintln!(
            "output-pressure: W16 exact fresh roster coincides with O256/control2/protected1"
        );
    }

    pub(super) fn observe_final_roster(
        &mut self,
        root: eframe::NativeOutputToken,
        roster: Option<eframe::NativeViewportRoster<'_>>,
    ) {
        if self.next_redock == SATELLITES
            && let Some(roster) = roster
            && let [record] = roster.records()
            && record.viewport_id() == ViewportId::ROOT
            && record.window_id() == root.window_id()
        {
            self.final_roster_checked = true;
        }
    }

    pub(super) fn redock(&mut self, runtime: &NativeDockspaceApp<Panes>) -> Result<bool, String> {
        let Some(entry) = self.satellites.get(self.next_redock) else {
            return Ok(true);
        };
        if !self.redock_queued && runtime.is_quiescent() {
            runtime
                .request_command(NativeCommand::DockRoot {
                    root: entry.root,
                    placement: DockPlacement::Center(DockAnchor::Item(ItemId::new(
                        entry.item.get() - 1,
                    ))),
                })
                .map_err(|error| format!("satellite redock: {error}"))?;
            self.redock_queued = true;
        }
        let surface = entry.surface.expect("only a committed child can redock");
        if entry.redock_applied
            && entry.destroyed
            && runtime.is_quiescent()
            && !runtime.is_surface_presented(surface)
            && runtime.with_view(|view| {
                view.surface(surface).is_none()
                    && view.item(entry.item).is_some_and(|item| {
                        item.surface() == ROOT_SURFACE && item.root() == MAIN_ROOT
                    })
            }) == Some(true)
        {
            eprintln!(
                "output-pressure: satellite {} destroyed and retired",
                entry.item.get()
            );
            self.next_redock += 1;
            self.redock_queued = false;
        }
        Ok(false)
    }

    pub(super) fn verify_finished(
        &self,
        runtime: &NativeDockspaceApp<Panes>,
    ) -> Result<(), String> {
        if !self.full_roster_checked
            || !self.final_roster_checked
            || !self.child_destroyed
            || self.next_redock != SATELLITES
            || self.satellites.iter().any(|entry| !entry.destroyed)
        {
            return Err("combined pressure did not retire every exact native child".into());
        }
        let expected: Vec<_> = (1..=16).map(ItemId::new).collect();
        if runtime.with_view(|view| {
            view.surfaces().count() == 1
                && view
                    .surface(ROOT_SURFACE)
                    .is_some_and(|surface| surface.contained_count() == 0)
                && view
                    .item(MAIN_ITEM)
                    .is_some_and(|item| item.root() == MAIN_ROOT && item.tabs().items() == expected)
        }) != Some(true)
        {
            return Err(
                "combined pressure did not restore the exact ordered [1..16] main tabs".into(),
            );
        }
        Ok(())
    }
}
