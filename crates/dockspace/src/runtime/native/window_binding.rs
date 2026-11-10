//! Opaque provider-bound identity of one real native-window lifetime.

use crate::platform_provider::PlatformObservationLease;
use crate::viewport::WindowBinding;

use super::HostWindowToken;

/// Exact provider-bound identity of one native window, independent of its surfaces.
///
/// Hosts may retain this copyable value for window-level resource correlation.
/// It grants no coordinate, presentation, input or close authority by itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct NativeWindowBinding {
    pub(super) provider: PlatformObservationLease,
    pub(super) window: WindowBinding,
}

impl NativeWindowBinding {
    pub(in crate::runtime) const fn core(self) -> WindowBinding {
        self.window
    }

    pub(in crate::runtime) const fn provider(self) -> PlatformObservationLease {
        self.provider
    }
    pub(super) const fn from_binding(
        provider: PlatformObservationLease,
        window: WindowBinding,
    ) -> Self {
        Self { provider, window }
    }

    /// Returns the opaque resource key in its original host or managed namespace.
    #[must_use]
    pub const fn window_token(self) -> HostWindowToken {
        HostWindowToken(self.window.token())
    }

    /// Reports whether two handles name the same real-window lifetime.
    ///
    /// Provider handoff changes the full binding's equality while retaining
    /// its exact window lifetime. This correlation query ignores the provider;
    /// it does not grant authority to submit facts through a superseded lease.
    #[must_use]
    pub fn same_window_lifetime(self, other: Self) -> bool {
        self.window == other.window
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ids::{EngineAuthorityDomainId, SurfaceId, WorkspaceEpoch};
    use crate::platform_provider::PlatformObservationAuthority;
    use crate::runtime::native::NativeSurfaceBinding;
    use crate::viewport::{ViewportBinding, WindowIncarnation, WindowToken};

    fn surface(
        provider: PlatformObservationLease,
        window: WindowBinding,
        surface: u64,
    ) -> NativeSurfaceBinding {
        NativeSurfaceBinding::from_binding(
            provider,
            ViewportBinding::from_window(window, SurfaceId::new(surface)),
        )
    }

    #[test]
    fn different_surfaces_share_the_exact_provider_bound_window() {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let provider = PlatformObservationAuthority::new(domain).create().unwrap();
        let window = WindowBinding::new(
            domain,
            WorkspaceEpoch::new(2),
            WindowToken::new(3),
            WindowIncarnation::new(4),
        );
        let first = surface(provider, window, 5);
        let second = surface(provider, window, 6);
        assert_ne!(first, second);
        assert_eq!(first.window(), second.window());
        assert_eq!(first.window().window_token(), HostWindowToken::new(3));
        assert!(first.same_window_lifetime(second));
        assert!(first.window().same_window_lifetime(second.window()));
        assert!(!first.matches_viewport_binding(second.binding));
    }

    #[test]
    fn provider_handoff_changes_authority_without_changing_window_lifetime() {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let mut authority = PlatformObservationAuthority::new(domain);
        let predecessor = authority.create().unwrap();
        let ticket = authority.begin_replacement(predecessor).unwrap();
        let successor = authority.finish_replacement(ticket).unwrap();
        let window = WindowBinding::new(
            domain,
            WorkspaceEpoch::new(2),
            WindowToken::new(3),
            WindowIncarnation::new(4),
        );
        let before = surface(predecessor, window, 5);
        let after = surface(successor, window, 5);
        assert_ne!(before, after);
        assert_ne!(before.window(), after.window());
        assert!(before.same_window_lifetime(after));
        assert!(before.window().same_window_lifetime(after.window()));
        assert!(before.same_window_lifetime(surface(successor, window, 6)));
    }

    #[test]
    fn foreign_engine_epoch_token_and_incarnation_never_share_a_window_lifetime() {
        let domain = EngineAuthorityDomainId::new_for_test(1);
        let foreign_domain = EngineAuthorityDomainId::new_for_test(9);
        let provider = PlatformObservationAuthority::new(domain).create().unwrap();
        let foreign_provider = PlatformObservationAuthority::new(foreign_domain)
            .create()
            .unwrap();
        let epoch = WorkspaceEpoch::new(2);
        let token = WindowToken::new(3);
        let incarnation = WindowIncarnation::new(4);
        let original = surface(
            provider,
            WindowBinding::new(domain, epoch, token, incarnation),
            5,
        );
        for (provider, window) in [
            (
                foreign_provider,
                WindowBinding::new(foreign_domain, epoch, token, incarnation),
            ),
            (
                provider,
                WindowBinding::new(domain, WorkspaceEpoch::new(7), token, incarnation),
            ),
            (
                provider,
                WindowBinding::new(domain, epoch, WindowToken::new(7), incarnation),
            ),
            (
                provider,
                WindowBinding::new(domain, epoch, WindowToken::managed(3), incarnation),
            ),
            (
                provider,
                WindowBinding::new(domain, epoch, token, WindowIncarnation::new(7)),
            ),
        ] {
            let other = surface(provider, window, 5);
            assert_ne!(original.window(), other.window());
            assert!(!original.same_window_lifetime(other));
            assert!(!original.window().same_window_lifetime(other.window()));
        }
    }
}
