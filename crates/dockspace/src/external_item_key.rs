//! Stable application keys for runtime [`ItemId`] allocation.
//!
//! [`ExternalItemKeyMap`] is a one-time bootstrap/import value. A persistent
//! dockspace consumes it into the serde-enabled `DockspaceDocumentSession`,
//! which becomes the sole mutable owner and prevents the complete map from being
//! paired with another engine at capture time. Keys remain opaque and byte-exact:
//! this module deliberately performs no trimming, case-folding, or Unicode
//! normalization.

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU64;

use thiserror::Error;

use crate::ids::ItemId;

/// A bidirectional, monotonic mapping from external string keys to dockspace items.
///
/// Assignments are append-only: removing an item from a live workspace does not
/// remove its identity mapping, so reopening the same external key resolves to
/// the same [`ItemId`]. New keys in one [`Self::ensure_all`] call use the
/// caller's explicit first-occurrence order; the map never derives identity
/// from string sorting or current workspace traversal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExternalItemKeyMap {
    by_external_key: BTreeMap<String, ItemId>,
    by_item: BTreeMap<ItemId, String>,
    next_item_id: Option<NonZeroU64>,
}

impl ExternalItemKeyMap {
    /// Creates an empty mapping whose first allocation is item `1`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            by_external_key: BTreeMap::new(),
            by_item: BTreeMap::new(),
            next_item_id: NonZeroU64::new(1),
        }
    }

    /// Returns the number of live mappings.
    #[must_use]
    pub fn len(&self) -> usize {
        self.by_external_key.len()
    }

    /// Returns whether the mapping is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.by_external_key.is_empty()
    }

    /// Resolves an exact external key to its stable item identity.
    #[must_use]
    pub fn item_id(&self, external_key: &str) -> Option<ItemId> {
        self.by_external_key.get(external_key).copied()
    }

    /// Resolves a stable item identity to its exact external key.
    #[must_use]
    pub fn external_key(&self, item: ItemId) -> Option<&str> {
        self.by_item.get(&item).map(String::as_str)
    }

    /// Iterates live mappings in canonical external-key order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (&str, ItemId)> {
        self.by_external_key
            .iter()
            .map(|(key, item)| (key.as_str(), *item))
    }

    /// Returns the existing item for `external_key`, or assigns a fresh one.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalItemKeyMapError::EmptyExternalKey`] for an empty key,
    /// or [`ExternalItemKeyMapError::ItemIdSpaceExhausted`] after the complete
    /// non-zero `u64` item identity space has been consumed.
    pub fn ensure(
        &mut self,
        external_key: impl Into<String>,
    ) -> Result<ItemId, ExternalItemKeyMapError> {
        let external_key = external_key.into();
        validate_external_key(&external_key)?;
        if let Some(item) = self.item_id(&external_key) {
            return Ok(item);
        }

        let Some(next_item_id) = self.next_item_id else {
            return Err(ExternalItemKeyMapError::ItemIdSpaceExhausted {
                requested_new_keys: 1,
            });
        };
        let item = ItemId::new(next_item_id.get());
        self.next_item_id = next_item_id.get().checked_add(1).and_then(NonZeroU64::new);
        let previous_key = self.by_item.insert(item, external_key.clone());
        let previous_item = self.by_external_key.insert(external_key, item);
        debug_assert!(previous_key.is_none() && previous_item.is_none());
        Ok(item)
    }

    /// Ensures a complete key batch as one atomic allocation transaction.
    ///
    /// Existing mappings are retained. Unseen keys are deduplicated while
    /// preserving their explicit first-occurrence order. Invalid input or
    /// insufficient remaining identity space leaves both maps and the
    /// allocation frontier unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalItemKeyMapError::EmptyExternalKey`] if any key is
    /// empty, or [`ExternalItemKeyMapError::ItemIdSpaceExhausted`] when every
    /// unseen key cannot be allocated atomically.
    pub fn ensure_all<I, K>(&mut self, external_keys: I) -> Result<(), ExternalItemKeyMapError>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        let external_keys = external_keys
            .into_iter()
            .map(Into::into)
            .collect::<Vec<String>>();
        for external_key in &external_keys {
            validate_external_key(external_key)?;
        }

        let mut seen = BTreeSet::new();
        let unseen = external_keys
            .into_iter()
            .filter(|external_key| {
                !self.by_external_key.contains_key(external_key)
                    && seen.insert(external_key.clone())
            })
            .collect::<Vec<_>>();
        let (allocations, next_item_id) = self.plan_allocations(unseen)?;

        for (external_key, item) in allocations {
            let previous_key = self.by_item.insert(item, external_key.clone());
            let previous_item = self.by_external_key.insert(external_key, item);
            debug_assert!(previous_key.is_none() && previous_item.is_none());
        }
        self.next_item_id = next_item_id;
        Ok(())
    }

    /// Reconciles two append-only identity histories into one monotonic candidate.
    ///
    /// Every assignment from both maps is retained, including keys absent from
    /// the current workspace. The allocation frontier never moves backward, and
    /// exhaustion in either history remains exhausted in the result. Both input
    /// maps remain unchanged.
    ///
    /// # Errors
    ///
    /// Returns [`ExternalItemKeyReconcileError::ExternalKeyConflict`] when the
    /// same key names different items, or
    /// [`ExternalItemKeyReconcileError::ItemIdConflict`] when the same item names
    /// different keys. The two opaque-history variants reject an explicit
    /// assignment below the other map's already-consumed frontier. Conflicts are
    /// validated before constructing the result.
    pub fn reconciled_with(&self, incoming: &Self) -> Result<Self, ExternalItemKeyReconcileError> {
        for (external_key, incoming_item) in &incoming.by_external_key {
            if let Some(active_item) = self.by_external_key.get(external_key)
                && active_item != incoming_item
            {
                return Err(ExternalItemKeyReconcileError::ExternalKeyConflict {
                    external_key: external_key.clone(),
                    active_item: *active_item,
                    incoming_item: *incoming_item,
                });
            }
            if let Some(active_external_key) = self.by_item.get(incoming_item)
                && active_external_key != external_key
            {
                return Err(ExternalItemKeyReconcileError::ItemIdConflict {
                    item: *incoming_item,
                    active_external_key: active_external_key.clone(),
                    incoming_external_key: external_key.clone(),
                });
            }
        }
        for (incoming_external_key, item) in &incoming.by_external_key {
            if !self.by_item.contains_key(item) && self.has_consumed(*item) {
                return Err(ExternalItemKeyReconcileError::ActiveHistoryOpaqueConflict {
                    item: *item,
                    incoming_external_key: incoming_external_key.clone(),
                });
            }
        }
        for (active_external_key, item) in &self.by_external_key {
            if !incoming.by_item.contains_key(item) && incoming.has_consumed(*item) {
                return Err(
                    ExternalItemKeyReconcileError::IncomingHistoryOpaqueConflict {
                        item: *item,
                        active_external_key: active_external_key.clone(),
                    },
                );
            }
        }

        let mut reconciled = self.clone();
        for (external_key, item) in &incoming.by_external_key {
            reconciled
                .by_external_key
                .entry(external_key.clone())
                .or_insert(*item);
            reconciled
                .by_item
                .entry(*item)
                .or_insert_with(|| external_key.clone());
        }
        reconciled.next_item_id = match (self.next_item_id, incoming.next_item_id) {
            (Some(active), Some(incoming)) => Some(active.max(incoming)),
            (None, _) | (_, None) => None,
        };
        Ok(reconciled)
    }

    fn has_consumed(&self, item: ItemId) -> bool {
        self.next_item_id
            .is_none_or(|frontier| item.get() < frontier.get())
    }

    fn plan_allocations(
        &self,
        unseen: Vec<String>,
    ) -> Result<(Vec<(String, ItemId)>, Option<NonZeroU64>), ExternalItemKeyMapError> {
        if unseen.is_empty() {
            return Ok((Vec::new(), self.next_item_id));
        }

        let Some(first) = self.next_item_id else {
            return Err(ExternalItemKeyMapError::ItemIdSpaceExhausted {
                requested_new_keys: unseen.len(),
            });
        };
        let last_offset = u64::try_from(unseen.len() - 1).map_err(|_| {
            ExternalItemKeyMapError::ItemIdSpaceExhausted {
                requested_new_keys: unseen.len(),
            }
        })?;
        let Some(last) = first.get().checked_add(last_offset) else {
            return Err(ExternalItemKeyMapError::ItemIdSpaceExhausted {
                requested_new_keys: unseen.len(),
            });
        };

        let mut raw_item_id = first.get();
        let mut allocations = Vec::with_capacity(unseen.len());
        for external_key in unseen {
            allocations.push((external_key, ItemId::new(raw_item_id)));
            if raw_item_id != u64::MAX {
                raw_item_id += 1;
            }
        }
        let next_item_id = last.checked_add(1).and_then(NonZeroU64::new);
        Ok((allocations, next_item_id))
    }
}

impl Default for ExternalItemKeyMap {
    fn default() -> Self {
        Self::new()
    }
}

fn validate_external_key(external_key: &str) -> Result<(), ExternalItemKeyMapError> {
    if external_key.is_empty() {
        return Err(ExternalItemKeyMapError::EmptyExternalKey);
    }
    Ok(())
}

/// Failure to allocate an external item key mapping.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ExternalItemKeyMapError {
    /// An empty string cannot be a stable application identity.
    #[error("external item keys cannot be empty")]
    EmptyExternalKey,
    /// The requested atomic allocation does not fit in the remaining non-zero `u64` space.
    #[error(
        "cannot allocate {requested_new_keys} external item keys: item identity space exhausted"
    )]
    ItemIdSpaceExhausted {
        /// Number of previously unseen keys in the rejected transaction.
        requested_new_keys: usize,
    },
}

/// Failure to reconcile two append-only external item identity histories.
#[derive(Clone, Debug, PartialEq, Eq, Error)]
pub enum ExternalItemKeyReconcileError {
    /// One exact external key was assigned two different item identities.
    #[error(
        "external item key {external_key:?} maps to active item {active_item} and incoming item {incoming_item}"
    )]
    ExternalKeyConflict {
        /// Conflicting application-owned key.
        external_key: String,
        /// Identity retained by the active history.
        active_item: ItemId,
        /// Different identity supplied by the incoming history.
        incoming_item: ItemId,
    },
    /// One item identity was assigned to two different external keys.
    #[error(
        "item identity {item} maps to active key {active_external_key:?} and incoming key {incoming_external_key:?}"
    )]
    ItemIdConflict {
        /// Conflicting stable item identity.
        item: ItemId,
        /// Key retained by the active history.
        active_external_key: String,
        /// Different key supplied by the incoming history.
        incoming_external_key: String,
    },
    /// The incoming map names an item already consumed opaquely by the active history.
    #[error(
        "incoming key {incoming_external_key:?} assigns item {item}, but the active history already consumed that identity without a mapping"
    )]
    ActiveHistoryOpaqueConflict {
        /// Identity already below the active allocation frontier.
        item: ItemId,
        /// Incoming key attempting to assign the opaque identity.
        incoming_external_key: String,
    },
    /// The active map names an item already consumed opaquely by the incoming history.
    #[error(
        "active key {active_external_key:?} assigns item {item}, but the incoming history already consumed that identity without a mapping"
    )]
    IncomingHistoryOpaqueConflict {
        /// Identity already below the incoming allocation frontier.
        item: ItemId,
        /// Active key attempting to retain the opaque identity.
        active_external_key: String,
    },
}

#[cfg(feature = "serde")]
mod persistence {
    use std::fmt;

    use serde::de::{Error as _, IgnoredAny, SeqAccess, Visitor};
    use serde::ser::SerializeTuple;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use thiserror::Error;

    use super::{ExternalItemKeyMap, ItemId, NonZeroU64, validate_external_key};

    /// The external-item-key sidecar schema emitted and accepted by this release.
    pub const EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION: u32 = 1;

    /// One untrusted persisted external-key assignment.
    #[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
    #[serde(deny_unknown_fields)]
    pub struct SnapshotExternalItemKeyEntry {
        /// Exact application-owned key.
        pub external_key: String,
        /// Stable dockspace item identity assigned to the key.
        pub item_id: u64,
    }

    /// A complete versioned external-item-key sidecar.
    ///
    /// `next_item_id` is `None` only when the non-zero `u64` identity space is
    /// exhausted. A present frontier must be non-zero and strictly greater than
    /// every persisted entry. Gaps are treated as previously consumed identities
    /// and are never reused.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ExternalItemKeySnapshot {
        /// Live assignments in canonical external-key order.
        pub entries: Vec<SnapshotExternalItemKeyEntry>,
        /// Next allocatable item identity, or exhaustion after `u64::MAX`.
        pub next_item_id: Option<u64>,
    }

    #[derive(Serialize)]
    struct ExternalItemKeySnapshotPayloadRef<'snapshot> {
        entries: &'snapshot [SnapshotExternalItemKeyEntry],
        next_item_id: Option<u64>,
    }

    impl Serialize for ExternalItemKeySnapshot {
        fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
        where
            S: Serializer,
        {
            let mut document = serializer.serialize_tuple(2)?;
            document.serialize_element(&EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION)?;
            document.serialize_element(&ExternalItemKeySnapshotPayloadRef {
                entries: &self.entries,
                next_item_id: self.next_item_id,
            })?;
            document.end()
        }
    }

    impl ExternalItemKeySnapshot {
        /// Returns the schema version emitted for this normalized snapshot.
        #[must_use]
        pub const fn version(&self) -> u32 {
            EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION
        }

        /// Captures the complete live mapping and monotonic allocation frontier.
        #[must_use]
        pub fn capture(mapping: &ExternalItemKeyMap) -> Self {
            Self {
                entries: mapping
                    .iter()
                    .map(|(external_key, item)| SnapshotExternalItemKeyEntry {
                        external_key: external_key.to_owned(),
                        item_id: item.get(),
                    })
                    .collect(),
                next_item_id: mapping.next_item_id.map(NonZeroU64::get),
            }
        }

        /// Validates and restores a complete mapping without partial publication.
        ///
        /// # Errors
        ///
        /// Returns a structured version, key, identity, conflict, or frontier
        /// error. No caller-owned mapping is mutated on failure.
        pub fn restore(self) -> Result<ExternalItemKeyMap, ExternalItemKeyRestoreError> {
            let mut by_external_key = std::collections::BTreeMap::<String, ItemId>::new();
            let mut by_item = std::collections::BTreeMap::<ItemId, String>::new();
            let mut highest_item_id = None::<u64>;
            for entry in self.entries {
                if validate_external_key(&entry.external_key).is_err() {
                    return Err(ExternalItemKeyRestoreError::EmptyExternalKey);
                }
                if entry.item_id == 0 {
                    return Err(ExternalItemKeyRestoreError::ZeroItemId {
                        external_key: entry.external_key,
                    });
                }
                let item = ItemId::new(entry.item_id);
                if by_external_key.contains_key(&entry.external_key) {
                    return Err(ExternalItemKeyRestoreError::DuplicateExternalKey {
                        external_key: entry.external_key,
                    });
                }
                if let Some(existing_external_key) = by_item.get(&item) {
                    return Err(ExternalItemKeyRestoreError::DuplicateItemId {
                        item_id: entry.item_id,
                        first_external_key: existing_external_key.clone(),
                        second_external_key: entry.external_key,
                    });
                }
                highest_item_id = Some(
                    highest_item_id.map_or(entry.item_id, |highest| highest.max(entry.item_id)),
                );
                by_item.insert(item, entry.external_key.clone());
                by_external_key.insert(entry.external_key, item);
            }

            let next_item_id = match self.next_item_id {
                Some(0) => return Err(ExternalItemKeyRestoreError::ZeroNextItemId),
                Some(next_item_id)
                    if highest_item_id.is_some_and(|highest| next_item_id <= highest) =>
                {
                    return Err(ExternalItemKeyRestoreError::FrontierNotBeyondAssigned {
                        next_item_id,
                        highest_item_id: highest_item_id.unwrap_or(0),
                    });
                }
                Some(next_item_id) => NonZeroU64::new(next_item_id),
                None if highest_item_id == Some(u64::MAX) => None,
                None => {
                    return Err(
                        ExternalItemKeyRestoreError::ExhaustedFrontierWithoutMaximumAssignment,
                    );
                }
            };

            Ok(ExternalItemKeyMap {
                by_external_key,
                by_item,
                next_item_id,
            })
        }
    }

    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ExternalItemKeySnapshotV1 {
        entries: Vec<SnapshotExternalItemKeyEntry>,
        #[serde(deserialize_with = "deserialize_required_option")]
        next_item_id: Option<u64>,
    }

    fn deserialize_required_option<'de, D, T>(deserializer: D) -> Result<Option<T>, D::Error>
    where
        D: Deserializer<'de>,
        T: Deserialize<'de>,
    {
        Option::<T>::deserialize(deserializer)
    }

    impl ExternalItemKeySnapshotV1 {
        fn into_snapshot(self) -> ExternalItemKeySnapshot {
            ExternalItemKeySnapshot {
                entries: self.entries,
                next_item_id: self.next_item_id,
            }
        }
    }

    /// Version-first decoder for an untrusted external-item-key sidecar.
    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct ExternalItemKeySnapshotEnvelope {
        version: u32,
        snapshot: Option<ExternalItemKeySnapshot>,
    }

    impl ExternalItemKeySnapshotEnvelope {
        /// Returns the schema version declared by the serialized document.
        #[must_use]
        pub const fn version(&self) -> u32 {
            self.version
        }

        /// Returns the strictly decoded snapshot for the supported schema version.
        ///
        /// # Errors
        ///
        /// Returns [`ExternalItemKeyRestoreError::UnsupportedVersion`] for every
        /// version other than [`EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION`].
        pub fn into_snapshot(self) -> Result<ExternalItemKeySnapshot, ExternalItemKeyRestoreError> {
            self.snapshot
                .ok_or(ExternalItemKeyRestoreError::UnsupportedVersion {
                    found: self.version,
                    supported: EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION,
                })
        }

        /// Decodes and validates the complete sidecar into a new mapping.
        ///
        /// # Errors
        ///
        /// Returns a structured version, key, identity, conflict, or frontier error.
        pub fn into_mapping(self) -> Result<ExternalItemKeyMap, ExternalItemKeyRestoreError> {
            self.into_snapshot()?.restore()
        }
    }

    impl<'de> Deserialize<'de> for ExternalItemKeySnapshotEnvelope {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: Deserializer<'de>,
        {
            deserializer.deserialize_tuple(2, ExternalItemKeySnapshotEnvelopeVisitor)
        }
    }

    struct ExternalItemKeySnapshotEnvelopeVisitor;

    impl<'de> Visitor<'de> for ExternalItemKeySnapshotEnvelopeVisitor {
        type Value = ExternalItemKeySnapshotEnvelope;

        fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
            formatter.write_str("a [version, payload] external item key document")
        }

        fn visit_seq<A>(self, mut sequence: A) -> Result<Self::Value, A::Error>
        where
            A: SeqAccess<'de>,
        {
            let version = sequence
                .next_element::<u32>()?
                .ok_or_else(|| A::Error::invalid_length(0, &self))?;
            let snapshot = if version == EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION {
                let payload = sequence
                    .next_element::<ExternalItemKeySnapshotV1>()?
                    .ok_or_else(|| A::Error::invalid_length(1, &self))?;
                Some(payload.into_snapshot())
            } else {
                sequence
                    .next_element::<IgnoredAny>()?
                    .ok_or_else(|| A::Error::invalid_length(1, &self))?;
                None
            };
            if sequence.next_element::<IgnoredAny>()?.is_some() {
                return Err(A::Error::invalid_length(3, &self));
            }
            Ok(ExternalItemKeySnapshotEnvelope { version, snapshot })
        }
    }

    /// Failure to restore an external-item-key sidecar.
    #[derive(Clone, Debug, PartialEq, Eq, Error)]
    pub enum ExternalItemKeyRestoreError {
        /// The snapshot version is not implemented by this crate release.
        #[error(
            "unsupported external item key snapshot version {found}; supported version is {supported}"
        )]
        UnsupportedVersion {
            /// Version read from the snapshot.
            found: u32,
            /// Version implemented by this crate release.
            supported: u32,
        },
        /// A persisted application key is empty.
        #[error("external item key snapshot contains an empty key")]
        EmptyExternalKey,
        /// A persisted assignment uses reserved item identity zero.
        #[error("external item key {external_key:?} uses reserved item identity zero")]
        ZeroItemId {
            /// Key owning the invalid identity.
            external_key: String,
        },
        /// Two records declare the same exact external key.
        #[error("duplicate external item key {external_key:?}")]
        DuplicateExternalKey {
            /// Repeated external key.
            external_key: String,
        },
        /// Two external keys declare the same item identity.
        #[error(
            "item identity {item_id} is assigned to both {first_external_key:?} and {second_external_key:?}"
        )]
        DuplicateItemId {
            /// Repeated numeric identity.
            item_id: u64,
            /// First key observed with the identity.
            first_external_key: String,
            /// Second key observed with the identity.
            second_external_key: String,
        },
        /// The available allocation frontier uses reserved identity zero.
        #[error("external item key allocation frontier cannot be zero")]
        ZeroNextItemId,
        /// The available frontier would reuse a live or previously assigned identity.
        #[error(
            "external item key allocation frontier {next_item_id} is not beyond highest assigned identity {highest_item_id}"
        )]
        FrontierNotBeyondAssigned {
            /// Invalid next allocation frontier.
            next_item_id: u64,
            /// Greatest live item identity in the snapshot.
            highest_item_id: u64,
        },
        /// An exhausted frontier was declared without consuming the final identity.
        #[error(
            "external item key allocation frontier cannot be exhausted before item identity u64::MAX is assigned"
        )]
        ExhaustedFrontierWithoutMaximumAssignment,
    }
}

#[cfg(feature = "serde")]
pub use persistence::{
    EXTERNAL_ITEM_KEY_SNAPSHOT_VERSION, ExternalItemKeyRestoreError, ExternalItemKeySnapshot,
    ExternalItemKeySnapshotEnvelope, SnapshotExternalItemKeyEntry,
};
