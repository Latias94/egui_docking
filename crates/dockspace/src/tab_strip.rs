//! Core-owned transient tab-strip presentation state.

use std::collections::{BTreeMap, BTreeSet};

use thiserror::Error;

use crate::ids::{ItemId, SurfaceId};
use crate::scene::TabBarSceneId;

/// Stable identity of one transient tab-strip state slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabStripStateKey {
    surface: SurfaceId,
    bar: TabBarSceneId,
}

/// Monotonic generation of one core-owned tab-list menu instance.
///
/// The numeric representation is diagnostic only. Constructors remain private
/// so UI adapters cannot mint popup authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct TabListMenuGeneration(u64);

impl TabListMenuGeneration {
    /// Returns the engine-local diagnostic representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }
}

/// Monotonic identity of one workspace-global popup presentation plane.
///
/// Only the core state store advances this value. Adapters may retain it for
/// exact projection comparison but cannot mint successor authority.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct PopupRoutingRevision(u64);

impl PopupRoutingRevision {
    /// Returns the engine-local diagnostic representation.
    #[must_use]
    pub const fn get(self) -> u64 {
        self.0
    }

    const fn checked_next(self) -> Option<Self> {
        match self.0.checked_add(1) {
            Some(value) => Some(Self(value)),
            None => None,
        }
    }

    #[cfg(test)]
    pub(crate) const fn new_for_test(value: u64) -> Self {
        Self(value)
    }
}

/// Complete semantic requirement for the workspace-global popup plane.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PopupPlaneRequirement {
    /// No popup owns the global receiver plane at this revision.
    Inactive {
        /// Exact core-owned plane revision.
        revision: PopupRoutingRevision,
    },
    /// One exact tab-list menu owns the global receiver plane.
    Active {
        /// Exact core-owned plane revision.
        revision: PopupRoutingRevision,
        /// Sole popup session visible across the workspace surface roster.
        session: TabListMenuSessionId,
        /// Structural strip which owns the menu frame and rows.
        owner: TabStripStateKey,
    },
}

impl PopupPlaneRequirement {
    /// Returns the exact core-owned popup-plane revision.
    #[must_use]
    pub const fn revision(self) -> PopupRoutingRevision {
        match self {
            Self::Inactive { revision } | Self::Active { revision, .. } => revision,
        }
    }

    /// Returns the active popup session, when one exists.
    #[must_use]
    pub const fn session(self) -> Option<TabListMenuSessionId> {
        match self {
            Self::Inactive { .. } => None,
            Self::Active { session, .. } => Some(session),
        }
    }

    /// Returns the strip which owns the active menu frame and rows.
    #[must_use]
    pub const fn owner(self) -> Option<TabStripStateKey> {
        match self {
            Self::Inactive { .. } => None,
            Self::Active { owner, .. } => Some(owner),
        }
    }
}

impl Default for PopupPlaneRequirement {
    fn default() -> Self {
        Self::Inactive {
            revision: PopupRoutingRevision::default(),
        }
    }
}

/// Exact identity of one core-opened tab-list menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TabListMenuSessionId {
    generation: TabListMenuGeneration,
    key: TabStripStateKey,
}

impl TabListMenuSessionId {
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    const fn new(key: TabStripStateKey, generation: TabListMenuGeneration) -> Self {
        Self { generation, key }
    }

    #[cfg(test)]
    pub(crate) const fn new_for_test(key: TabStripStateKey, generation: u64) -> Self {
        Self::new(key, TabListMenuGeneration(generation))
    }

    /// Returns the sole structural tab strip owned by this menu instance.
    #[must_use]
    pub const fn key(self) -> TabStripStateKey {
        self.key
    }

    /// Returns the monotonic menu generation.
    #[must_use]
    pub const fn generation(self) -> TabListMenuGeneration {
        self.generation
    }
}

impl TabStripStateKey {
    /// Creates an exact state key from core-owned surface and topology identities.
    #[must_use]
    pub const fn new(surface: SurfaceId, bar: TabBarSceneId) -> Self {
        Self { surface, bar }
    }

    /// Returns the owning logical surface.
    #[must_use]
    pub const fn surface(self) -> SurfaceId {
        self.surface
    }

    /// Returns the owning structural tab bar.
    #[must_use]
    pub const fn bar(self) -> TabBarSceneId {
        self.bar
    }
}

/// Stable identity of one core-compiled tab-strip control.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TabStripControlId {
    /// Scroll toward the beginning of the ordered tab roster.
    ScrollBackward(TabBarSceneId),
    /// Scroll toward the end of the ordered tab roster.
    ScrollForward(TabBarSceneId),
    /// Open or close the complete tab-list menu.
    TabListMenu(TabBarSceneId),
}

impl TabStripControlId {
    /// Returns all controls in deterministic leading-to-trailing order.
    #[must_use]
    pub const fn for_bar(bar: TabBarSceneId) -> [Self; 3] {
        [
            Self::ScrollBackward(bar),
            Self::ScrollForward(bar),
            Self::TabListMenu(bar),
        ]
    }

    /// Returns the structural tab bar which owns this control.
    #[must_use]
    pub const fn bar(self) -> TabBarSceneId {
        match self {
            Self::ScrollBackward(bar) | Self::ScrollForward(bar) | Self::TabListMenu(bar) => bar,
        }
    }
}

/// One core-owned, non-persistent tab-strip presentation state.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TabStripState {
    scroll_offset: Option<f64>,
    resolved_selection: Option<ItemId>,
}

impl TabStripState {
    /// Returns an explicitly requested strip offset, if one exists.
    #[must_use]
    pub(crate) const fn scroll_offset(&self) -> Option<f64> {
        self.scroll_offset
    }

    pub(crate) const fn resolved_selection(&self) -> Option<ItemId> {
        self.resolved_selection
    }

    /// Replaces the explicit strip offset.
    ///
    /// # Errors
    ///
    /// Returns [`TabStripStateError`] for non-finite or negative values.
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    pub(crate) fn set_scroll_offset(&mut self, offset: f64) -> Result<(), TabStripStateError> {
        validate_offset("tab-strip scroll offset", offset)?;
        self.scroll_offset = Some(offset);
        Ok(())
    }
}

/// The sole workspace-global open tab-list menu.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ActiveTabListMenu {
    session: TabListMenuSessionId,
    focus: ItemId,
    scroll_offset: f64,
    items: Vec<ItemId>,
}

impl ActiveTabListMenu {
    pub(crate) const fn key(&self) -> TabStripStateKey {
        self.session.key()
    }

    pub(crate) const fn session(&self) -> TabListMenuSessionId {
        self.session
    }

    pub(crate) const fn focus(&self) -> ItemId {
        self.focus
    }

    pub(crate) const fn scroll_offset(&self) -> f64 {
        self.scroll_offset
    }

    pub(crate) fn items(&self) -> &[ItemId] {
        &self.items
    }
}

/// Core-owned inventory of live transient tab-strip states.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct TabStripStateStore {
    states: BTreeMap<TabStripStateKey, TabStripState>,
    live_rosters: BTreeMap<TabStripStateKey, Vec<ItemId>>,
    active_menu: Option<ActiveTabListMenu>,
    last_menu_generation: TabListMenuGeneration,
    popup_routing_revision: PopupRoutingRevision,
}

/// Explicit result of one transient tab-strip state transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TabStripStateDelta {
    state_changed: bool,
    influence: TabStripInfluenceDomain,
}

/// Exact presentation scope influenced by one transient tab-strip transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum TabStripInfluenceDomain {
    /// Only these logical surfaces need their transient scene input invalidated.
    Local(BTreeSet<SurfaceId>),
    /// The workspace-global popup receiver plane changed for the exact surface roster.
    PopupRoster,
}

impl TabStripStateDelta {
    fn new(state_changed: bool, influence: TabStripInfluenceDomain) -> Self {
        Self {
            state_changed,
            influence,
        }
    }

    fn unchanged() -> Self {
        Self::new(false, TabStripInfluenceDomain::Local(BTreeSet::new()))
    }

    pub(crate) const fn state_changed(&self) -> bool {
        self.state_changed
    }

    pub(crate) const fn influence(&self) -> &TabStripInfluenceDomain {
        &self.influence
    }

    pub(crate) fn merge(self, next: Self) -> Self {
        Self::new(
            self.state_changed || next.state_changed,
            self.influence.merge(next.influence),
        )
    }
}

impl TabStripInfluenceDomain {
    fn local(surface: SurfaceId) -> Self {
        Self::Local(BTreeSet::from([surface]))
    }

    pub(crate) fn merge(self, next: Self) -> Self {
        match (self, next) {
            (Self::PopupRoster, _) | (_, Self::PopupRoster) => Self::PopupRoster,
            (Self::Local(mut before), Self::Local(after)) => {
                before.extend(after);
                Self::Local(before)
            }
        }
    }

    pub(crate) fn is_empty(&self) -> bool {
        matches!(self, Self::Local(surfaces) if surfaces.is_empty())
    }
}

impl TabStripStateStore {
    pub(crate) const fn popup_requirement(&self) -> PopupPlaneRequirement {
        match self.active_menu.as_ref() {
            Some(menu) => PopupPlaneRequirement::Active {
                revision: self.popup_routing_revision,
                session: menu.session(),
                owner: menu.key(),
            },
            None => PopupPlaneRequirement::Inactive {
                revision: self.popup_routing_revision,
            },
        }
    }

    pub(crate) fn state(&self, key: TabStripStateKey) -> Option<&TabStripState> {
        self.states.get(&key)
    }

    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    pub(crate) fn state_mut(&mut self, key: TabStripStateKey) -> &mut TabStripState {
        self.states.entry(key).or_default()
    }

    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    pub(crate) const fn active_menu(&self) -> Option<&ActiveTabListMenu> {
        self.active_menu.as_ref()
    }

    pub(crate) fn active_menu_for(&self, key: TabStripStateKey) -> Option<&ActiveTabListMenu> {
        self.active_menu.as_ref().filter(|menu| menu.key() == key)
    }

    /// Opens the sole workspace tab-list menu for one exact live strip.
    ///
    /// A new generation is minted even when this replaces a menu for the same
    /// strip. The preferred item is focused when it remains in the live roster;
    /// otherwise the first ordered item receives focus.
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    pub(crate) fn open_tab_list_menu(
        &mut self,
        key: TabStripStateKey,
        preferred_focus: Option<ItemId>,
    ) -> Result<(TabListMenuSessionId, TabStripStateDelta), TabStripStateError> {
        let items = self
            .live_rosters
            .get(&key)
            .ok_or(TabStripStateError::UnknownTabStrip { key })?;
        let Some(first) = items.first().copied() else {
            return Err(TabStripStateError::EmptyTabListMenu { key });
        };
        let unique = items.iter().copied().collect::<BTreeSet<_>>();
        if unique.len() != items.len() {
            return Err(TabStripStateError::DuplicateTabListMenuItem { key });
        }
        let generation = self
            .last_menu_generation
            .checked_next()
            .ok_or(TabStripStateError::MenuGenerationExhausted)?;
        let routing_revision = self
            .popup_routing_revision
            .checked_next()
            .ok_or(TabStripStateError::PopupRoutingRevisionExhausted)?;
        let session = TabListMenuSessionId::new(key, generation);
        let focus = preferred_focus
            .filter(|item| unique.contains(item))
            .unwrap_or(first);
        let active = ActiveTabListMenu {
            session,
            focus,
            scroll_offset: 0.0,
            items: items.clone(),
        };

        self.last_menu_generation = generation;
        self.popup_routing_revision = routing_revision;
        self.active_menu = Some(active);
        Ok((
            session,
            TabStripStateDelta::new(true, TabStripInfluenceDomain::PopupRoster),
        ))
    }

    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    pub(crate) fn close_tab_list_menu(
        &mut self,
        session: TabListMenuSessionId,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        let active = self
            .active_menu
            .as_ref()
            .ok_or(TabStripStateError::NoActiveTabListMenu)?;
        if active.session != session {
            return Err(TabStripStateError::MenuSessionMismatch {
                expected: active.session,
                actual: session,
            });
        }
        let routing_revision = self
            .popup_routing_revision
            .checked_next()
            .ok_or(TabStripStateError::PopupRoutingRevisionExhausted)?;
        self.active_menu = None;
        self.popup_routing_revision = routing_revision;
        Ok(TabStripStateDelta::new(
            true,
            TabStripInfluenceDomain::PopupRoster,
        ))
    }

    pub(crate) fn set_tab_strip_scroll_offset(
        &mut self,
        key: TabStripStateKey,
        offset: f64,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        validate_offset("tab-strip scroll offset", offset)?;
        let state = self
            .states
            .get_mut(&key)
            .ok_or(TabStripStateError::UnknownTabStrip { key })?;
        if state.scroll_offset == Some(offset) {
            return Ok(TabStripStateDelta::unchanged());
        }
        state.scroll_offset = Some(offset);
        Ok(TabStripStateDelta::new(
            true,
            TabStripInfluenceDomain::local(key.surface()),
        ))
    }

    /// Adopts offsets already solved by one successfully installed Ready plan.
    ///
    /// This synchronizes transient state with the plan which is now
    /// authoritative. It deliberately produces no invalidation delta: the
    /// installed plan already contains these exact values.
    pub(crate) fn adopt_resolved_scroll_offsets(
        &mut self,
        surface: SurfaceId,
        offsets: impl IntoIterator<Item = (TabBarSceneId, f64, Option<ItemId>)>,
    ) -> Result<(), TabStripStateError> {
        let offsets = offsets
            .into_iter()
            .map(|(bar, offset, selected)| (TabStripStateKey::new(surface, bar), offset, selected))
            .collect::<Vec<_>>();
        for (key, offset, _) in &offsets {
            validate_offset("tab-strip scroll offset", *offset)?;
            if !self.states.contains_key(key) {
                return Err(TabStripStateError::UnknownTabStrip { key: *key });
            }
        }
        for (key, offset, selected) in offsets {
            let state = self
                .states
                .get_mut(&key)
                .expect("validated tab-strip state remains present");
            state.scroll_offset = Some(offset);
            state.resolved_selection = selected;
        }
        Ok(())
    }

    pub(crate) fn set_active_menu_focus(
        &mut self,
        session: TabListMenuSessionId,
        focus: ItemId,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        let active = self.active_menu_mut(session)?;
        if !active.items.contains(&focus) {
            return Err(TabStripStateError::MenuFocusUnavailable { session, focus });
        }
        if active.focus == focus {
            return Ok(TabStripStateDelta::unchanged());
        }
        let active = self.active_menu_mut(session)?;
        active.focus = focus;
        Ok(TabStripStateDelta::new(
            true,
            TabStripInfluenceDomain::local(session.key().surface()),
        ))
    }

    pub(crate) fn set_active_menu_scroll_offset(
        &mut self,
        session: TabListMenuSessionId,
        offset: f64,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        validate_offset("tab-list menu scroll offset", offset)?;
        if self.active_menu_mut(session)?.scroll_offset == offset {
            return Ok(TabStripStateDelta::unchanged());
        }
        self.active_menu_mut(session)?.scroll_offset = offset;
        Ok(TabStripStateDelta::new(
            true,
            TabStripInfluenceDomain::PopupRoster,
        ))
    }

    fn active_menu_mut(
        &mut self,
        session: TabListMenuSessionId,
    ) -> Result<&mut ActiveTabListMenu, TabStripStateError> {
        let active = self
            .active_menu
            .as_mut()
            .ok_or(TabStripStateError::NoActiveTabListMenu)?;
        if active.session != session {
            return Err(TabStripStateError::MenuSessionMismatch {
                expected: active.session,
                actual: session,
            });
        }
        Ok(active)
    }

    pub(crate) fn reconcile_exact_for_surface_roster<'a>(
        &mut self,
        live: impl IntoIterator<Item = (TabStripStateKey, &'a [ItemId])>,
        surface_roster_changed: bool,
    ) -> Result<TabStripStateDelta, TabStripStateError> {
        let mut candidate = self.clone();
        let live = live
            .into_iter()
            .map(|(key, items)| (key, items.to_vec()))
            .collect::<BTreeMap<_, _>>();
        candidate.live_rosters = live;
        let live_rosters = &candidate.live_rosters;
        candidate
            .states
            .retain(|key, _| live_rosters.contains_key(key));
        for key in live_rosters.keys().copied() {
            if !candidate.states.contains_key(&key) {
                candidate.states.insert(key, TabStripState::default());
            }
        }
        for (key, state) in &mut candidate.states {
            let items = &live_rosters[key];
            if items.is_empty() {
                let reset = TabStripState::default();
                if *state != reset {
                    *state = reset;
                }
            }
        }
        let active_before = candidate.active_menu.clone();
        candidate.reconcile_active_menu();
        let routing_changed = surface_roster_changed || candidate.active_menu != active_before;
        if routing_changed {
            candidate.popup_routing_revision = candidate
                .popup_routing_revision
                .checked_next()
                .ok_or(TabStripStateError::PopupRoutingRevisionExhausted)?;
        }
        let state_changed = candidate != *self;
        let influence = if routing_changed {
            TabStripInfluenceDomain::PopupRoster
        } else {
            TabStripInfluenceDomain::Local(changed_local_surfaces(self, &candidate))
        };
        *self = candidate;
        Ok(TabStripStateDelta::new(state_changed, influence))
    }

    fn reconcile_active_menu(&mut self) -> bool {
        let Some(active) = self.active_menu.as_mut() else {
            return false;
        };
        let Some(items) = self.live_rosters.get(&active.key()) else {
            self.active_menu = None;
            return true;
        };
        let Some(fallback_focus) = items.first().copied() else {
            self.active_menu = None;
            return true;
        };
        if active.items == *items {
            return false;
        }

        active.focus = restored_menu_focus(&active.items, active.focus, items, fallback_focus);
        active.items.clone_from(items);
        true
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.states.len()
    }
}

fn changed_local_surfaces(
    before: &TabStripStateStore,
    after: &TabStripStateStore,
) -> BTreeSet<SurfaceId> {
    before
        .states
        .keys()
        .chain(after.states.keys())
        .filter(|key| before.states.get(*key) != after.states.get(*key))
        .chain(
            before
                .live_rosters
                .keys()
                .chain(after.live_rosters.keys())
                .filter(|key| before.live_rosters.get(*key) != after.live_rosters.get(*key)),
        )
        .map(|key| key.surface())
        .collect()
}

/// Invalid transient presentation input.
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub(crate) enum TabStripStateError {
    /// A transient offset was non-finite or negative.
    #[error("{field} must be finite and non-negative, got {value}")]
    InvalidOffset {
        /// Stable field name for diagnostics.
        field: &'static str,
        /// Rejected value.
        value: f64,
    },
    /// The requested strip is absent from the exact live roster.
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    #[error("tab-list menu strip {key:?} is not live")]
    UnknownTabStrip { key: TabStripStateKey },
    /// A live strip has no item which could receive menu focus.
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    #[error("tab-list menu strip {key:?} has an empty item roster")]
    EmptyTabListMenu { key: TabStripStateKey },
    /// A malformed item roster contained a duplicate stable identity.
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    #[error("tab-list menu strip {key:?} contains duplicate items")]
    DuplicateTabListMenuItem { key: TabStripStateKey },
    /// The core-owned menu generation cannot advance without wrapping.
    #[allow(dead_code, reason = "reserved for the typed tab-strip input reducer")]
    #[error("tab-list menu generation exhausted")]
    MenuGenerationExhausted,
    /// The global popup-plane revision cannot advance without wrapping.
    #[error("popup-plane revision exhausted")]
    PopupRoutingRevisionExhausted,
    /// No workspace tab-list menu is currently active.
    #[error("no tab-list menu is active")]
    NoActiveTabListMenu,
    /// A reducer input addressed a stale menu instance.
    #[error("tab-list menu session mismatch: expected {expected:?}, got {actual:?}")]
    MenuSessionMismatch {
        expected: TabListMenuSessionId,
        actual: TabListMenuSessionId,
    },
    /// A reducer input attempted to focus an item outside the frozen roster.
    #[error("item {focus} is unavailable in tab-list menu {session:?}")]
    MenuFocusUnavailable {
        session: TabListMenuSessionId,
        focus: ItemId,
    },
}

fn restored_menu_focus(
    previous: &[ItemId],
    previous_focus: ItemId,
    current: &[ItemId],
    fallback: ItemId,
) -> ItemId {
    let current_set = current.iter().copied().collect::<BTreeSet<_>>();
    if current_set.contains(&previous_focus) {
        return previous_focus;
    }
    if let Some(index) = previous.iter().position(|item| *item == previous_focus) {
        if let Some(next) = previous[index.saturating_add(1)..]
            .iter()
            .copied()
            .find(|item| current_set.contains(item))
        {
            return next;
        }
        if let Some(previous) = previous[..index]
            .iter()
            .rev()
            .copied()
            .find(|item| current_set.contains(item))
        {
            return previous;
        }
    }
    fallback
}

fn validate_offset(field: &'static str, value: f64) -> Result<(), TabStripStateError> {
    if value.is_finite() && value >= 0.0 {
        Ok(())
    } else {
        Err(TabStripStateError::InvalidOffset { field, value })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, Workspace};
    use crate::ids::{ItemId, NodeId, RootId, SurfaceId};
    use crate::scene::TabBarSceneId;

    fn key(surface: u64, root: u64, node: NodeId) -> TabStripStateKey {
        TabStripStateKey::new(
            SurfaceId::new(surface),
            TabBarSceneId {
                root: RootId::new(root),
                tabs: node,
            },
        )
    }

    fn nodes() -> [NodeId; 2] {
        let mut builder = Workspace::builder();
        [
            builder.insert_node(Node::tabs([])),
            builder.insert_node(Node::tabs([])),
        ]
    }

    #[test]
    fn control_ids_are_stable_structural_identities() {
        let [node, _] = nodes();
        let bar = key(1, 2, node).bar();

        assert_eq!(
            TabStripControlId::for_bar(bar),
            [
                TabStripControlId::ScrollBackward(bar),
                TabStripControlId::ScrollForward(bar),
                TabStripControlId::TabListMenu(bar),
            ]
        );
    }

    #[test]
    fn exact_set_reconciliation_drops_moved_removed_and_foreign_states() {
        let [node, replacement_node] = nodes();
        let retained = key(1, 10, node);
        let moved_surface = key(2, 10, node);
        let replaced_root = key(1, 11, node);
        let removed_node = key(1, 10, replacement_node);
        let mut states = TabStripStateStore::default();

        let live_items = [ItemId::new(7)];
        assert!(
            states
                .reconcile_exact_for_surface_roster(
                    [retained, moved_surface, replaced_root, removed_node]
                        .map(|key| (key, live_items.as_slice())),
                    false,
                )
                .expect("initial roster reconciles")
                .state_changed()
        );

        for key in [retained, moved_surface, replaced_root, removed_node] {
            let state = states.state_mut(key);
            state.set_scroll_offset(42.0).expect("offset is valid");
        }

        assert!(
            states
                .reconcile_exact_for_surface_roster([(retained, live_items.as_slice())], false,)
                .expect("reduced roster reconciles")
                .state_changed()
        );
        assert_eq!(states.len(), 1);
        assert!(states.state(retained).is_some());
        assert!(states.state(moved_surface).is_none());
        assert!(states.state(replaced_root).is_none());
        assert!(states.state(removed_node).is_none());
        assert!(
            !states
                .reconcile_exact_for_surface_roster([(retained, live_items.as_slice())], false,)
                .expect("stable roster reconciles")
                .state_changed()
        );
    }

    #[test]
    fn only_one_menu_is_active_and_a_second_open_mints_a_new_generation() {
        let [first_node, second_node] = nodes();
        let first = key(1, 10, first_node);
        let second = key(2, 20, second_node);
        let mut states = TabStripStateStore::default();
        let first_items = [ItemId::new(1), ItemId::new(2)];
        let second_items = [ItemId::new(3), ItemId::new(4)];
        assert!(
            states
                .reconcile_exact_for_surface_roster(
                    [
                        (first, first_items.as_slice()),
                        (second, second_items.as_slice()),
                    ],
                    false,
                )
                .expect("menu rosters reconcile")
                .state_changed()
        );

        let routing_before_first = states.popup_requirement().revision();
        let (first_session, first_delta) = states
            .open_tab_list_menu(first, Some(first_items[1]))
            .expect("the live non-empty strip can open its menu");
        assert_ne!(states.popup_requirement().revision(), routing_before_first);
        assert_eq!(
            first_delta.influence(),
            &TabStripInfluenceDomain::PopupRoster
        );
        assert_eq!(
            states.active_menu().map(ActiveTabListMenu::key),
            Some(first)
        );
        assert_eq!(
            states.active_menu().map(ActiveTabListMenu::focus),
            Some(first_items[1])
        );

        let routing_before_second = states.popup_requirement().revision();
        let (second_session, second_delta) = states
            .open_tab_list_menu(second, None)
            .expect("opening another live menu replaces the first");
        assert_ne!(states.popup_requirement().revision(), routing_before_second);
        assert_eq!(
            second_delta.influence(),
            &TabStripInfluenceDomain::PopupRoster
        );
        assert_ne!(first_session, second_session);
        assert_eq!(
            second_session.generation().get(),
            first_session.generation().get() + 1
        );
        let active = states
            .active_menu()
            .expect("exactly one menu remains active");
        assert_eq!(active.key(), second);
        assert_eq!(active.session(), second_session);
        assert_eq!(active.focus(), second_items[0]);
        assert_eq!(active.items(), second_items);
    }

    #[test]
    fn roster_reconciliation_restores_removed_focus_to_a_stable_neighbor() {
        let [node, _] = nodes();
        let key = key(1, 10, node);
        let mut states = TabStripStateStore::default();
        let initial = [
            ItemId::new(1),
            ItemId::new(2),
            ItemId::new(3),
            ItemId::new(4),
        ];
        assert!(
            states
                .reconcile_exact_for_surface_roster([(key, initial.as_slice())], false)
                .expect("initial roster reconciles")
                .state_changed()
        );
        let (session, _) = states
            .open_tab_list_menu(key, Some(initial[1]))
            .expect("the live strip can open its menu");
        let routing_before_scroll = states.popup_requirement().revision();
        let scroll_delta = states
            .set_active_menu_scroll_offset(session, 18.0)
            .expect("the active menu accepts a valid offset");
        assert!(scroll_delta.state_changed());
        assert_eq!(
            scroll_delta.influence(),
            &TabStripInfluenceDomain::PopupRoster
        );
        assert_eq!(states.popup_requirement().revision(), routing_before_scroll);

        let focus_delta = states
            .set_active_menu_focus(session, initial[2])
            .expect("the active menu accepts a live focus item");
        assert!(focus_delta.state_changed());
        assert_eq!(
            focus_delta.influence(),
            &TabStripInfluenceDomain::Local(BTreeSet::from([key.surface()]))
        );
        assert_eq!(states.popup_requirement().revision(), routing_before_scroll);
        states
            .set_active_menu_focus(session, initial[1])
            .expect("focus can return before roster reconciliation");

        let removed_focus = [initial[0], initial[2], initial[3]];
        let routing_before_reconcile = states.popup_requirement().revision();
        let reconcile = states
            .reconcile_exact_for_surface_roster([(key, removed_focus.as_slice())], false)
            .expect("removed focus reconciles");
        assert!(reconcile.state_changed());
        assert_ne!(
            states.popup_requirement().revision(),
            routing_before_reconcile
        );
        assert_eq!(reconcile.influence(), &TabStripInfluenceDomain::PopupRoster);
        let active = states.active_menu().expect("the live menu remains active");
        assert_eq!(active.focus(), initial[2]);
        assert_eq!(active.items(), removed_focus);
        assert_eq!(active.scroll_offset(), 18.0);

        let removed_next_neighbor = [initial[0], initial[3]];
        assert!(
            states
                .reconcile_exact_for_surface_roster(
                    [(key, removed_next_neighbor.as_slice())],
                    false,
                )
                .expect("removed successor reconciles")
                .state_changed()
        );
        assert_eq!(
            states.active_menu().map(ActiveTabListMenu::focus),
            Some(initial[3])
        );

        let removed_last = [initial[0]];
        assert!(
            states
                .reconcile_exact_for_surface_roster([(key, removed_last.as_slice())], false)
                .expect("removed last neighbor reconciles")
                .state_changed()
        );
        assert_eq!(
            states.active_menu().map(ActiveTabListMenu::focus),
            Some(initial[0])
        );
    }

    #[test]
    fn reconcile_closes_a_menu_whose_key_is_removed_or_roster_becomes_empty() {
        let [node, _] = nodes();
        let key = key(1, 10, node);
        let mut states = TabStripStateStore::default();
        let items = [ItemId::new(7)];
        assert!(
            states
                .reconcile_exact_for_surface_roster([(key, items.as_slice())], false)
                .expect("initial roster reconciles")
                .state_changed()
        );
        states
            .state_mut(key)
            .set_scroll_offset(42.0)
            .expect("offset is valid");
        states
            .open_tab_list_menu(key, Some(items[0]))
            .expect("the live strip can open its menu");

        assert!(
            states
                .reconcile_exact_for_surface_roster([(key, &[][..])], false)
                .expect("empty roster reconciles")
                .state_changed()
        );
        let state = states
            .state(key)
            .expect("the live bar keeps its state slot");
        assert_eq!(state.scroll_offset(), None);
        assert!(states.active_menu().is_none());

        assert!(
            states
                .reconcile_exact_for_surface_roster([(key, items.as_slice())], false)
                .expect("repopulated roster reconciles")
                .state_changed()
        );
        states
            .open_tab_list_menu(key, None)
            .expect("the repopulated strip can reopen");
        assert!(
            states
                .reconcile_exact_for_surface_roster(std::iter::empty(), false)
                .expect("removed roster reconciles")
                .state_changed()
        );
        assert!(states.active_menu().is_none());
        assert!(states.state(key).is_none());
    }

    #[test]
    fn menu_generation_exhaustion_is_atomic() {
        let [first_node, second_node] = nodes();
        let first = key(1, 10, first_node);
        let second = key(1, 20, second_node);
        let first_items = [ItemId::new(1)];
        let second_items = [ItemId::new(2)];
        let mut states = TabStripStateStore::default();
        assert!(
            states
                .reconcile_exact_for_surface_roster(
                    [
                        (first, first_items.as_slice()),
                        (second, second_items.as_slice()),
                    ],
                    false,
                )
                .expect("menu rosters reconcile")
                .state_changed()
        );
        states
            .open_tab_list_menu(first, None)
            .expect("the first menu opens");
        states.last_menu_generation = TabListMenuGeneration(u64::MAX);
        let before = states.clone();

        assert_eq!(
            states
                .open_tab_list_menu(second, None)
                .map(|(session, _)| session),
            Err(TabStripStateError::MenuGenerationExhausted)
        );
        assert_eq!(states, before);
    }

    #[test]
    fn delta_merge_unions_local_surfaces_and_popup_roster_absorbs_local_scope() {
        let [first_node, second_node] = nodes();
        let first = key(1, 10, first_node);
        let second = key(2, 20, second_node);
        let first_items = [ItemId::new(1)];
        let second_items = [ItemId::new(2)];
        let mut states = TabStripStateStore::default();
        states
            .reconcile_exact_for_surface_roster(
                [
                    (first, first_items.as_slice()),
                    (second, second_items.as_slice()),
                ],
                false,
            )
            .expect("two live strips reconcile");

        let first_local = states
            .set_tab_strip_scroll_offset(first, 4.0)
            .expect("first local offset is valid");
        let second_local = states
            .set_tab_strip_scroll_offset(second, 8.0)
            .expect("second local offset is valid");
        let local = first_local.clone().merge(second_local);
        assert!(local.state_changed());
        assert_eq!(
            local.influence(),
            &TabStripInfluenceDomain::Local(BTreeSet::from([first.surface(), second.surface(),]))
        );

        let routing_before_popup = states.popup_requirement().revision();
        let (_, popup) = states
            .open_tab_list_menu(first, None)
            .expect("the live strip can own the popup plane");
        let absorbed = first_local.merge(popup);
        assert_ne!(states.popup_requirement().revision(), routing_before_popup);
        assert_eq!(absorbed.influence(), &TabStripInfluenceDomain::PopupRoster);
    }

    #[test]
    fn close_and_exact_surface_roster_changes_advance_global_routing() {
        let [node, _] = nodes();
        let key = key(1, 10, node);
        let items = [ItemId::new(1)];
        let mut states = TabStripStateStore::default();
        states
            .reconcile_exact_for_surface_roster([(key, items.as_slice())], false)
            .expect("the live strip reconciles");
        let revision_before_open = states.popup_requirement().revision();
        let (session, open) = states
            .open_tab_list_menu(key, None)
            .expect("the live strip can open");
        let revision_after_open = states.popup_requirement().revision();
        let close = states
            .close_tab_list_menu(session)
            .expect("the exact session can close");
        let revision_after_close = states.popup_requirement().revision();
        assert_ne!(revision_after_open, revision_before_open);
        assert_ne!(revision_after_close, revision_after_open);
        assert_eq!(open.influence(), &TabStripInfluenceDomain::PopupRoster);
        assert_eq!(close.influence(), &TabStripInfluenceDomain::PopupRoster);

        let revision_before_roster = states.popup_requirement().revision();
        let roster = states
            .reconcile_exact_for_surface_roster([(key, items.as_slice())], true)
            .expect("an exact surface-roster change reconciles");
        assert!(roster.state_changed());
        assert_eq!(roster.influence(), &TabStripInfluenceDomain::PopupRoster);
        assert_ne!(
            states.popup_requirement().revision(),
            revision_before_roster
        );
    }
}
