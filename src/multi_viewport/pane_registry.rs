use serde::de::DeserializeOwned;

/// A reusable mapping layer between `Pane` ⇄ `PaneId` for layout persistence.
///
/// Design goals:
/// - Layout snapshots store only `PaneId`, never the `Pane` value itself (keep the RON small and portable).
/// - The app decides how to restore a `Pane` from a `PaneId` (lazy loading, placeholder panes, migrations, etc).
pub trait PaneRegistry<Pane> {
    type PaneId: Clone + serde::Serialize + DeserializeOwned;

    fn pane_id(&mut self, pane: &Pane) -> Self::PaneId;
    fn pane_from_id(&mut self, id: Self::PaneId) -> Pane;

    /// Optional restoration path: return `None` to drop panes that no longer exist.
    ///
    /// This is useful when loading older snapshots after a refactor where some panes were removed
    /// or merged. The default implementation always succeeds by delegating to [`Self::pane_from_id`].
    fn try_pane_from_id(&mut self, id: Self::PaneId) -> Option<Pane> {
        Some(self.pane_from_id(id))
    }
}

/// Convenience helper: build a [`PaneRegistry`] from two closures.
pub struct SimplePaneRegistry<PaneId, ToId, FromId> {
    pub to_id: ToId,
    pub from_id: FromId,
    _marker: std::marker::PhantomData<PaneId>,
}

impl<PaneId, ToId, FromId> SimplePaneRegistry<PaneId, ToId, FromId> {
    pub fn new(to_id: ToId, from_id: FromId) -> Self {
        Self {
            to_id,
            from_id,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<Pane, PaneId, ToId, FromId> PaneRegistry<Pane> for SimplePaneRegistry<PaneId, ToId, FromId>
where
    PaneId: Clone + serde::Serialize + DeserializeOwned,
    ToId: FnMut(&Pane) -> PaneId,
    FromId: FnMut(PaneId) -> Pane,
{
    type PaneId = PaneId;

    fn pane_id(&mut self, pane: &Pane) -> Self::PaneId {
        (self.to_id)(pane)
    }

    fn pane_from_id(&mut self, id: Self::PaneId) -> Pane {
        (self.from_id)(id)
    }

    fn try_pane_from_id(&mut self, id: Self::PaneId) -> Option<Pane> {
        Some((self.from_id)(id))
    }
}
