//! Complete-roster storage is promised before a new native lifetime is admitted.

use super::{NativeHostState, NativeSurfaceBinding};

impl NativeHostState {
    /// Existing route, resource and retirement roots share one lifetime promise.
    /// A route-to-retirement move cannot return capacity before quiescence.
    pub(super) fn can_admit_binding(&self, incoming: NativeSurfaceBinding) -> bool {
        let bindings = || {
            self.viewports
                .bindings()
                .chain(self.retirements.bindings())
                .chain(self.window_resources.bindings())
        };
        if bindings().any(|binding| binding.same_window_lifetime(incoming)) {
            return true;
        }
        let retained = bindings()
            .enumerate()
            .filter(|(index, binding)| {
                !bindings()
                    .take(*index)
                    .any(|previous| previous.same_window_lifetime(*binding))
            })
            .count();
        retained < self.terminal_roster.window_capacity()
    }
}
