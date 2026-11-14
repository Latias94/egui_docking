//! Stable product notifications projected from committed workspace events.

use crate::engine::event::{WorkspaceEvent, WorkspaceEventKind};
use crate::engine::transition::{EngineTransition, InputOutcome, WorkspaceVersion};
use crate::graph::command::{CloseCommitOutcome, CommandOutcome};
use crate::ids::{
    EngineAuthorityDomainId, FloatingPresentationId, ItemId, ReducerTickId, RootId, SurfaceId,
};
use crate::model::DockspaceActionOutcome;

use super::DockspaceCloseOutcome;

/// Opaque identity of one committed product event.
///
/// Independent sessions cannot alias. Filtering internal events preserves the
/// original event index, so multiple changes caused by one input stay distinct.
/// Reading or cloning a report preserves these identities.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct DockspaceEventId {
    domain: EngineAuthorityDomainId,
    tick: ReducerTickId,
    event_index: usize,
}

impl std::fmt::Debug for DockspaceEventId {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("DockspaceEventId(..)")
    }
}

/// One read-only product change from a successful atomic publication.
///
/// Events describe committed changes, not requests or renderer callbacks.
/// Rejected, stale and unchanged actions produce no change event. A pending
/// native transfer emits placement changes only when ownership really moves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DockspaceEvent {
    id: DockspaceEventId,
    version: WorkspaceVersion,
    kind: DockspaceEventKind,
}

impl DockspaceEvent {
    /// Returns the identity for application-owned announcement deduplication.
    #[must_use]
    pub const fn id(&self) -> DockspaceEventId {
        self.id
    }

    /// Returns the workspace version immediately after this event's mutation.
    ///
    /// Later events in the same atomic publication may have a newer version.
    #[must_use]
    pub const fn version(&self) -> WorkspaceVersion {
        self.version
    }

    /// Returns the committed product change.
    #[must_use]
    pub const fn kind(&self) -> &DockspaceEventKind {
        &self.kind
    }
}

/// Product changes expressed only through stable item and presentation identities.
#[derive(Clone, Debug, PartialEq, Eq)]
#[non_exhaustive]
pub enum DockspaceEventKind {
    /// Durable selection or its MRU ordering changed for this item.
    ///
    /// This does not mean a widget became active, nor that pane or native-window
    /// focus was observed. Selecting an already selected item can update MRU.
    SelectionCommitted { item: ItemId },
    /// One tab changed its index within its existing group.
    TabReordered {
        item: ItemId,
        from: usize,
        to: usize,
    },
    /// One previously closed item was opened.
    ItemOpened { item: ItemId, root: RootId },
    /// Existing items changed docking placement or tab order.
    ///
    /// The roots can be equal when the payload moves inside one root.
    ItemsMoved {
        items: Vec<ItemId>,
        source_root: RootId,
        target_root: RootId,
    },
    /// Content was installed as a surface's main or contained root.
    ///
    /// `floating` is `None` for a main root. Native placement is reported only
    /// after the exact first-live presentation transfers content ownership.
    RootInstalled {
        root: RootId,
        surface: SurfaceId,
        floating: Option<FloatingPresentationId>,
        items: Vec<ItemId>,
    },
    /// An existing root changed its presentation owner.
    ///
    /// Its topology identity is preserved. `floating` is `None` for a main root.
    RootRehomed {
        root: RootId,
        surface: SurfaceId,
        floating: Option<FloatingPresentationId>,
    },
    /// One contained presentation changed its bounds or stacking order.
    ContainedPresentationChanged {
        floating: FloatingPresentationId,
        bounds_changed: bool,
        /// Previous and resulting indices in the contained back-to-front roster.
        order: Option<(usize, usize)>,
    },
    /// An approved close removed content from the workspace.
    ///
    /// Requests, reused plans, deferred decisions and vetoes do not emit this.
    Closed(DockspaceCloseOutcome),
    /// An application layout replaced the workspace in a new epoch.
    ///
    /// Earlier changes superseded inside the same commit are omitted.
    LayoutReset,
    /// A validated persistent document replaced the workspace in a new epoch.
    ///
    /// Item membership is represented by the replacement, not individual
    /// opened/closed events for every pane.
    DocumentRestored,
}

pub(super) fn committed_events(transition: &EngineTransition) -> Vec<DockspaceEvent> {
    let events = transition.events();
    let first = events
        .iter()
        .rposition(|event| matches!(event.kind(), WorkspaceEventKind::WorkspaceReplaced))
        .unwrap_or(0);
    events
        .iter()
        .enumerate()
        .skip(first)
        .filter_map(|(event_index, event)| {
            let kind = product_kind(transition, event)?;
            Some(DockspaceEvent {
                id: DockspaceEventId {
                    domain: transition.authority_domain(),
                    tick: transition.tick(),
                    event_index,
                },
                version: event.version(),
                kind,
            })
        })
        .collect()
}

fn product_kind(
    transition: &EngineTransition,
    event: &WorkspaceEvent,
) -> Option<DockspaceEventKind> {
    use DockspaceEventKind as Event;

    match event.kind() {
        WorkspaceEventKind::WorkspaceReplaced => {
            let restored = transition.reduced_inputs().iter().any(|input| {
                input.cause() == event.cause()
                    && matches!(
                        input.outcome(),
                        InputOutcome::WorkspaceReplaced {
                            restored_identity_frontier: Some(_),
                            ..
                        }
                    )
            });
            Some(if restored {
                Event::DocumentRestored
            } else {
                Event::LayoutReset
            })
        }
        WorkspaceEventKind::CloseCommitted(outcome) => Some(Event::Closed(match outcome {
            CloseCommitOutcome::ItemClosed { item, root } => DockspaceCloseOutcome::ItemClosed {
                item: *item,
                root: *root,
            },
            CloseCommitOutcome::RootClosed { root, items } => DockspaceCloseOutcome::RootClosed {
                root: *root,
                items: items.clone(),
            },
            CloseCommitOutcome::SurfaceClosed { surface, items } => {
                DockspaceCloseOutcome::SurfaceClosed {
                    surface: *surface,
                    items: items.clone(),
                }
            }
        })),
        WorkspaceEventKind::CommandCommitted(command) if command.changes_workspace() => {
            command_kind(transition, event, command)
        }
        // Applied rehomes already emitted their checked command above. A failed
        // presentation terminal remains available in presentation_transitions.
        WorkspaceEventKind::PresentationRehomeSettled { .. }
        | WorkspaceEventKind::CommandCommitted(_)
        | WorkspaceEventKind::PolicyReplaced => None,
    }
}

fn command_kind(
    transition: &EngineTransition,
    event: &WorkspaceEvent,
    command: &CommandOutcome,
) -> Option<DockspaceEventKind> {
    use DockspaceEventKind as Event;

    Some(match command {
        CommandOutcome::Selected { item, .. } => Event::SelectionCommitted { item: *item },
        CommandOutcome::Reordered { item, from, to, .. } => Event::TabReordered {
            item: *item,
            from: *from,
            to: *to,
        },
        CommandOutcome::Opened { item, root } => Event::ItemOpened {
            item: *item,
            root: *root,
        },
        CommandOutcome::Moved {
            items,
            source_root,
            target_root,
            ..
        } => Event::ItemsMoved {
            items: items.clone(),
            source_root: *source_root,
            target_root: *target_root,
        },
        CommandOutcome::SurfaceRootCreated {
            surface,
            root,
            items,
        }
        | CommandOutcome::MainRootInstalled {
            surface,
            root,
            items,
        } => {
            // Opening into a rootless surface uses MainRootInstalled internally.
            // Preserve the public open lifecycle instead of describing it as a
            // move. The causal lookup is needed only for root installation.
            let opened = transition.reduced_inputs().iter().find_map(|input| {
                if input.cause() != event.cause() {
                    return None;
                }
                match input.outcome() {
                    InputOutcome::ProductActionProcessed {
                        outcome: DockspaceActionOutcome::Opened { item, root: opened },
                        ..
                    } if opened == root => Some(*item),
                    _ => None,
                }
            });
            if let Some(item) = opened {
                Event::ItemOpened { item, root: *root }
            } else {
                Event::RootInstalled {
                    root: *root,
                    surface: *surface,
                    floating: None,
                    items: items.clone(),
                }
            }
        }
        CommandOutcome::ContainedRootCreated {
            surface,
            root,
            floating,
            items,
        } => Event::RootInstalled {
            root: *root,
            surface: *surface,
            floating: Some(*floating),
            items: items.clone(),
        },
        CommandOutcome::RootRehomed {
            root,
            surface,
            floating,
            ..
        } => Event::RootRehomed {
            root: *root,
            surface: *surface,
            floating: *floating,
        },
        CommandOutcome::ContainedPromoted { root, surface, .. } => Event::RootRehomed {
            root: *root,
            surface: *surface,
            floating: None,
        },
        CommandOutcome::ContainedRectUpdated { floating, .. } => {
            Event::ContainedPresentationChanged {
                floating: *floating,
                bounds_changed: true,
                order: None,
            }
        }
        CommandOutcome::ContainedPresentationUpdated {
            floating,
            from,
            to,
            rect_changed,
            order_changed,
        } => Event::ContainedPresentationChanged {
            floating: *floating,
            bounds_changed: *rect_changed,
            order: order_changed.then_some((*from, *to)),
        },
        CommandOutcome::ContainedRaised {
            floating, from, to, ..
        } => Event::ContainedPresentationChanged {
            floating: *floating,
            bounds_changed: false,
            order: Some((*from, *to)),
        },
        CommandOutcome::SplitsResized { .. }
        | CommandOutcome::EmptySurfaceAttached { .. }
        | CommandOutcome::EmptySurfaceRetired { .. }
        | CommandOutcome::EmptyRootRemoved { .. } => return None,
    })
}

#[cfg(test)]
mod tests;
