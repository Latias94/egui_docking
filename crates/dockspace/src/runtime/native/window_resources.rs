use std::{
    collections::{BTreeMap, HashMap},
    error::Error,
    fmt,
    hash::Hash,
};

use super::{NativeRootContinuation, NativeWindowBinding};

#[derive(Debug)]
struct Entry<K, T> {
    window: K,
    value: T,
}

/// Application-owned resources indexed by exact native-window lifetime.
///
/// The application chooses its own copyable, hashable key `K` (for example its
/// framework window ID). One entry represents one physical window, independently of its logical
/// surface roster. Store renderer resources or the application's existing
/// window state in `T`; this registry imposes no `Send`, `Clone` or runtime bound.
///
/// Resolve the framework window ID at the synchronous event-loop boundary with [`Self::binding`].
/// Carry that binding through deferred work and use [`Self::get`] afterward so
/// an old callback cannot address a replacement window that reused its OS ID.
/// Removing an entry releases host ownership; it does not report destruction or
/// quiescence to the docking session.
#[derive(Debug)]
pub struct NativeWindows<K, T> {
    entries: BTreeMap<NativeWindowBinding, Entry<K, T>>,
    routes: HashMap<K, NativeWindowBinding>,
}

impl<K, T> Default for NativeWindows<K, T> {
    fn default() -> Self {
        Self {
            entries: BTreeMap::new(),
            routes: HashMap::new(),
        }
    }
}

impl<K: Eq + Hash + Copy + fmt::Debug, T> NativeWindows<K, T> {
    /// Creates an empty window resource registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers resources without replacing an existing window or binding.
    ///
    /// # Errors
    ///
    /// Returns the unchanged `value` with the conflict reason when either key
    /// is already registered. Both indices remain unchanged on failure.
    pub fn insert(
        &mut self,
        binding: impl Into<NativeWindowBinding>,
        window: K,
        value: T,
    ) -> Result<(), NativeWindowInsertError<K, T>> {
        let binding = binding.into();
        let reason = if self.entries.contains_key(&binding) {
            Some(NativeWindowRegistryError::BindingAlreadyRegistered { binding })
        } else if self.routes.contains_key(&window) {
            Some(NativeWindowRegistryError::WindowAlreadyRegistered { window })
        } else {
            None
        };
        if let Some(reason) = reason {
            return Err(NativeWindowInsertError { reason, value });
        }
        self.entries.insert(binding, Entry { window, value });
        self.routes.insert(window, binding);
        Ok(())
    }

    /// Resolves a physical event's current native-window binding.
    #[must_use]
    pub fn binding(&self, window: K) -> Option<NativeWindowBinding> {
        self.routes.get(&window).copied()
    }

    /// Borrows resources only for this exact native-window binding.
    #[must_use]
    pub fn get(&self, binding: impl Into<NativeWindowBinding>) -> Option<&T> {
        self.entries.get(&binding.into()).map(|entry| &entry.value)
    }

    /// Mutably borrows resources only for this exact native-window binding.
    #[must_use]
    pub fn get_mut(&mut self, binding: impl Into<NativeWindowBinding>) -> Option<&mut T> {
        self.entries
            .get_mut(&binding.into())
            .map(|entry| &mut entry.value)
    }

    /// Removes the exact binding and returns its resources to the application.
    #[must_use]
    pub fn remove(&mut self, binding: impl Into<NativeWindowBinding>) -> Option<T> {
        let entry = self.entries.remove(&binding.into())?;
        self.routes.remove(&entry.window);
        Some(entry.value)
    }

    /// Continues physical resources using a committed core Root continuation.
    ///
    /// Apply only pairs supplied by a successful host-frame report. This changes
    /// resource lookup, without promoting old hit records, coordinates or outputs.
    /// Shared-surface pairs and repeated delivery are idempotent. Returns whether
    /// a registered predecessor changed to its successor.
    ///
    /// # Errors
    ///
    /// Returns an error without changing either index if the successor conflicts
    /// with another entry, or neither side is registered.
    pub fn continue_root(
        &mut self,
        continuation: NativeRootContinuation,
    ) -> Result<bool, NativeWindowRegistryError<K>> {
        let before = continuation.predecessor().window();
        let after = continuation.successor().window();
        if before == after || !self.entries.contains_key(&before) {
            return self
                .entries
                .contains_key(&after)
                .then_some(false)
                .ok_or(NativeWindowRegistryError::BindingUnavailable { binding: before });
        }
        if self.entries.contains_key(&after) {
            return Err(NativeWindowRegistryError::BindingAlreadyRegistered { binding: after });
        }
        let Some(entry) = self.entries.remove(&before) else {
            return Err(NativeWindowRegistryError::BindingUnavailable { binding: before });
        };
        self.routes.insert(entry.window, after);
        self.entries.insert(after, entry);
        Ok(true)
    }

    /// Visits each physical window once, in core binding order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = (NativeWindowBinding, K, &T)> {
        self.entries
            .iter()
            .map(|(binding, entry)| (*binding, entry.window, &entry.value))
    }

    /// Mutably visits each physical window's application resources once.
    pub fn iter_mut(&mut self) -> impl ExactSizeIterator<Item = (NativeWindowBinding, K, &mut T)> {
        self.entries
            .iter_mut()
            .map(|(binding, entry)| (*binding, entry.window, &mut entry.value))
    }

    /// Returns the number of registered physical windows.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Reports whether there are no registered physical windows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// A conflict while associating an application resource with a core window lifetime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum NativeWindowRegistryError<K: fmt::Debug> {
    /// Resources already exist for this exact binding.
    #[error("native window binding is already registered: {binding:?}")]
    BindingAlreadyRegistered {
        /// Conflicting core-issued window binding.
        binding: NativeWindowBinding,
    },
    /// The framework window ID already belongs to another registered binding.
    #[error("framework window ID is already registered: {window:?}")]
    WindowAlreadyRegistered {
        /// Conflicting framework resource identity.
        window: K,
    },
    /// The exact predecessor and successor have no registered resources.
    #[error("native window binding has no registered resources: {binding:?}")]
    BindingUnavailable {
        /// Missing core-issued window binding.
        binding: NativeWindowBinding,
    },
}

/// An unaccepted registration retaining its application-owned resources.
pub struct NativeWindowInsertError<K: fmt::Debug, T> {
    reason: NativeWindowRegistryError<K>,
    value: T,
}

impl<K: Copy + fmt::Debug, T> NativeWindowInsertError<K, T> {
    /// Returns why the registration was rejected.
    #[must_use]
    pub const fn reason(&self) -> NativeWindowRegistryError<K> {
        self.reason
    }

    /// Returns the original resources without cloning or dropping them.
    #[must_use]
    pub fn into_value(self) -> T {
        self.value
    }
}

impl<K: fmt::Debug, T> fmt::Debug for NativeWindowInsertError<K, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("NativeWindowInsertError")
            .field("reason", &self.reason)
            .finish_non_exhaustive()
    }
}

impl<K: fmt::Debug, T> fmt::Display for NativeWindowInsertError<K, T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.reason.fmt(formatter)
    }
}

impl<K: fmt::Debug + 'static, T> Error for NativeWindowInsertError<K, T> {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.reason)
    }
}
