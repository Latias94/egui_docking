//! Bootstrap root and optional child viewport templates.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::SurfaceCloseRequest;
use dockspace::SurfaceRecoveryBootstrap;
use dockspace::graph::Workspace;
use dockspace::ids::SurfaceId;
use dockspace::viewport::{ViewportBinding, ViewportRole, WindowToken};
use dockspace::viewport_persistence::{ViewportPlacementPreference, WindowPresentationPreference};
use egui::{ViewportBuilder, ViewportId};

use crate::NativeRuntimeError;

/// One bootstrap root or dormant child viewport template.
#[derive(Clone, Debug)]
pub struct NativeSurfaceSpec {
    viewport: ViewportId,
    surface: SurfaceId,
    bootstrap_token: Option<WindowToken>,
    role: ViewportRole,
    recovery_bootstrap: Option<SurfaceRecoveryBootstrap>,
    builder: ViewportBuilder,
    close_request: SurfaceCloseRequest,
}

impl NativeSurfaceSpec {
    /// Configures the process root viewport.
    #[must_use]
    pub fn root(surface: SurfaceId, token: WindowToken) -> Self {
        Self {
            viewport: ViewportId::ROOT,
            surface,
            bootstrap_token: Some(token),
            role: ViewportRole::Root,
            recovery_bootstrap: None,
            builder: ViewportBuilder::default(),
            close_request: SurfaceCloseRequest::CloseContent,
        }
    }

    /// Configures one docking-owned deferred child viewport.
    #[must_use]
    pub fn child(viewport: ViewportId, surface: SurfaceId, builder: ViewportBuilder) -> Self {
        Self {
            viewport,
            surface,
            bootstrap_token: None,
            role: ViewportRole::Child,
            recovery_bootstrap: None,
            builder,
            close_request: SurfaceCloseRequest::CloseContent,
        }
    }

    /// Configures one child viewport that already exists in the restored workspace.
    ///
    /// Unlike [`Self::child`], this entry is enrolled at startup with a fresh
    /// process-local token minted for the restored logical surface. Documents do
    /// not persist native window tokens or incarnations. The native runtime submits
    /// the entry through its correlated create lane and materializes it hidden;
    /// ordinary viewport declaration begins only after the exact native and core
    /// lifetimes are both bound. A later close therefore cannot resurrect it from
    /// the bootstrap catalog.
    #[must_use]
    pub fn restored_child(
        viewport: ViewportId,
        surface: SurfaceId,
        token: WindowToken,
        recovery: SurfaceRecoveryBootstrap,
        builder: ViewportBuilder,
    ) -> Self {
        Self {
            viewport,
            surface,
            bootstrap_token: Some(token),
            role: ViewportRole::Child,
            recovery_bootstrap: Some(recovery),
            builder,
            close_request: SurfaceCloseRequest::CloseContent,
        }
    }

    /// Replaces the explicit semantic disposition for a native child close.
    #[must_use]
    pub fn with_close_request(mut self, request: SurfaceCloseRequest) -> Self {
        self.close_request = request;
        self
    }

    /// Applies one durable placement preference to this startup viewport.
    ///
    /// The preference's outer position and inner size are converted through the
    /// exact scale that confirmed them. Missing geometry facts deliberately leave
    /// the caller's builder unchanged; they do not authorize a guessed restore.
    ///
    /// # Errors
    ///
    /// Returns an error when the preference belongs to another surface or cannot
    /// be represented by egui's logical `f32` viewport builder.
    pub fn try_with_restored_placement(
        mut self,
        preference: ViewportPlacementPreference,
    ) -> Result<Self, NativeRuntimeError> {
        if preference.surface() != self.surface {
            return Err(NativeRuntimeError::RestoredPlacementSurfaceMismatch {
                expected: self.surface,
                found: preference.surface(),
            });
        }
        let (Some(inner_size), Some(scale_factor)) =
            (preference.inner_size(), preference.scale_factor())
        else {
            return Ok(self);
        };
        let scale = scale_factor.get();
        let outer_rect = preference.outer_rect();
        let position = logical_pair(self.surface, outer_rect.x() / scale, outer_rect.y() / scale)?;
        let size = logical_pair(
            self.surface,
            inner_size.width() / scale,
            inner_size.height() / scale,
        )?;
        self.builder = self.builder.with_position(position).with_inner_size(size);
        self.builder = match preference.presentation() {
            Some(WindowPresentationPreference::Maximized) => self.builder.with_maximized(true),
            Some(WindowPresentationPreference::Fullscreen) => self.builder.with_fullscreen(true),
            Some(WindowPresentationPreference::Normal) | None => self.builder,
        };
        Ok(self)
    }

    /// Returns the egui viewport slot.
    #[must_use]
    pub const fn viewport(&self) -> ViewportId {
        self.viewport
    }

    /// Returns the logical dockspace surface.
    #[must_use]
    pub const fn surface(&self) -> SurfaceId {
        self.surface
    }

    /// Returns the adapter-owned root bootstrap token.
    ///
    /// Child templates return `None`: their stable token is minted by the core
    /// and arrives only in a native create effect.
    #[must_use]
    pub const fn bootstrap_token(&self) -> Option<WindowToken> {
        self.bootstrap_token
    }

    /// Returns the core native-window role.
    #[must_use]
    pub const fn role(&self) -> ViewportRole {
        self.role
    }

    /// Returns core-consumable recovery facts for a restored child registration.
    #[must_use]
    pub const fn recovery_bootstrap(&self) -> Option<SurfaceRecoveryBootstrap> {
        self.recovery_bootstrap
    }

    /// Returns the explicit semantic disposition for a native child close.
    #[must_use]
    pub const fn close_request(&self) -> &SurfaceCloseRequest {
        &self.close_request
    }
}

fn logical_pair(surface: SurfaceId, x: f64, y: f64) -> Result<[f32; 2], NativeRuntimeError> {
    let lower = f64::from(f32::MIN);
    let upper = f64::from(f32::MAX);
    if !x.is_finite()
        || !y.is_finite()
        || !(lower..=upper).contains(&x)
        || !(lower..=upper).contains(&y)
    {
        return Err(NativeRuntimeError::RestoredPlacementUnrepresentable { surface });
    }
    Ok([x as f32, y as f32])
}

/// Bootstrap catalog owned by one native runtime.
///
/// The root entry is live at startup. Child entries are dormant templates and
/// become eligible only after the core emits a create effect for their surface.
#[derive(Clone, Debug)]
pub struct NativeViewportRoster {
    specs: BTreeMap<ViewportId, NativeSurfaceSpec>,
}

impl NativeViewportRoster {
    /// Creates a roster containing its required physical root.
    pub fn new(root: NativeSurfaceSpec) -> Result<Self, NativeRuntimeError> {
        if root.viewport != ViewportId::ROOT || root.role != ViewportRole::Root {
            return Err(NativeRuntimeError::MissingRootViewport);
        }
        let mut roster = Self {
            specs: BTreeMap::new(),
        };
        roster.insert_checked(root)?;
        Ok(roster)
    }

    /// Inserts one dormant child viewport template.
    ///
    /// # Errors
    ///
    /// Returns an error when the viewport, surface, or stable window token is
    /// already assigned.
    pub fn insert(&mut self, spec: NativeSurfaceSpec) -> Result<(), NativeRuntimeError> {
        if spec.viewport == ViewportId::ROOT || spec.role != ViewportRole::Child {
            return Err(NativeRuntimeError::MissingRootViewport);
        }
        self.insert_checked(spec)
    }

    fn insert_checked(&mut self, spec: NativeSurfaceSpec) -> Result<(), NativeRuntimeError> {
        if self.specs.contains_key(&spec.viewport) {
            return Err(NativeRuntimeError::DuplicateViewport {
                viewport: spec.viewport,
            });
        }
        if self
            .specs
            .values()
            .any(|existing| existing.surface == spec.surface)
        {
            return Err(NativeRuntimeError::DuplicateSurface {
                surface: spec.surface,
            });
        }
        if let Some(token) = spec.bootstrap_token
            && self
                .specs
                .values()
                .any(|existing| existing.bootstrap_token == Some(token))
        {
            return Err(NativeRuntimeError::DuplicateWindowToken { token });
        }
        self.specs.insert(spec.viewport, spec);
        Ok(())
    }

    /// Iterates the complete configured roster in canonical viewport order.
    pub fn iter(&self) -> impl ExactSizeIterator<Item = &NativeSurfaceSpec> {
        self.specs.values()
    }

    /// Returns one configured viewport.
    #[must_use]
    pub fn get(&self, viewport: ViewportId) -> Option<&NativeSurfaceSpec> {
        self.specs.get(&viewport)
    }

    /// Returns the number of bootstrap entries and dormant templates.
    #[must_use]
    pub fn len(&self) -> usize {
        self.specs.len()
    }

    /// Returns whether no physical viewport is configured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.specs.is_empty()
    }

    pub(crate) fn validate_workspace(
        &self,
        workspace: &Workspace,
    ) -> Result<(), NativeRuntimeError> {
        if !self.specs.contains_key(&ViewportId::ROOT) {
            return Err(NativeRuntimeError::MissingRootViewport);
        }
        let actual = workspace
            .surfaces()
            .map(|(surface, _)| surface)
            .collect::<BTreeSet<_>>();
        let root = self
            .specs
            .get(&ViewportId::ROOT)
            .ok_or(NativeRuntimeError::MissingRootViewport)?;
        let invalid_recovery_host = self.specs.values().any(|spec| {
            spec.role == ViewportRole::Child
                && spec.bootstrap_token.is_some()
                && spec
                    .recovery_bootstrap
                    .is_none_or(|recovery| recovery.host_surface() != root.surface)
        });
        let restored = self
            .specs
            .values()
            .filter(|spec| {
                spec.viewport == ViewportId::ROOT
                    || (spec.role == ViewportRole::Child && spec.bootstrap_token.is_some())
            })
            .map(|spec| spec.surface)
            .collect::<BTreeSet<_>>();
        if root.role != ViewportRole::Root || invalid_recovery_host || actual != restored {
            return Err(NativeRuntimeError::WorkspaceRosterMismatch);
        }
        Ok(())
    }

    pub(crate) fn root(&self) -> &NativeSurfaceSpec {
        self.specs
            .get(&ViewportId::ROOT)
            .expect("construction proves the bootstrap root")
    }

    pub(crate) fn restored_children(&self) -> impl Iterator<Item = &NativeSurfaceSpec> + Clone {
        self.specs
            .values()
            .filter(|spec| spec.role == ViewportRole::Child && spec.bootstrap_token.is_some())
    }

    pub(crate) fn builder(&self, viewport: ViewportId) -> Option<ViewportBuilder> {
        self.specs.get(&viewport).map(|spec| spec.builder.clone())
    }

    pub(crate) fn restored_staging_builder(&self, viewport: ViewportId) -> Option<ViewportBuilder> {
        self.specs
            .get(&viewport)
            .filter(|spec| spec.role == ViewportRole::Child && spec.bootstrap_token.is_some())
            .map(|spec| spec.builder.clone().with_visible(false).with_active(false))
    }

    pub(crate) fn child_for_binding(&self, binding: ViewportBinding) -> RuntimeViewportSpec {
        self.specs
            .values()
            .find(|spec| spec.role == ViewportRole::Child && spec.surface == binding.surface())
            .map_or_else(
                || RuntimeViewportSpec {
                    viewport: ViewportId::from_hash_of((
                        "dockspace-native",
                        binding.authority_domain().get(),
                        binding.epoch().get(),
                        binding.surface().get(),
                        binding.token().get(),
                        binding.incarnation().get(),
                    )),
                    builder: ViewportBuilder::default()
                        .with_visible(false)
                        .with_active(false),
                },
                |spec| RuntimeViewportSpec {
                    viewport: spec.viewport,
                    builder: spec.builder.clone().with_visible(false).with_active(false),
                },
            )
    }

    pub(crate) fn retained_builder(&self, surface: SurfaceId) -> ViewportBuilder {
        self.specs
            .values()
            .find(|spec| spec.role == ViewportRole::Child && spec.surface == surface)
            .map_or_else(ViewportBuilder::default, |spec| spec.builder.clone())
    }

    pub(crate) fn close_request(&self, surface: SurfaceId) -> Option<&SurfaceCloseRequest> {
        self.specs
            .values()
            .find(|spec| spec.role == ViewportRole::Child && spec.surface == surface)
            .map(|spec| &spec.close_request)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct RuntimeViewportSpec {
    viewport: ViewportId,
    builder: ViewportBuilder,
}

impl RuntimeViewportSpec {
    pub(crate) const fn viewport(&self) -> ViewportId {
        self.viewport
    }

    pub(crate) fn builder(&self) -> ViewportBuilder {
        self.builder.clone()
    }
}

#[cfg(test)]
mod tests {
    use dockspace::geometry::{PhysicalRect, PhysicalSize, ScaleFactor};
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId};
    use dockspace::viewport_persistence::{
        ViewportPlacementPreference, WindowPresentationPreference,
    };

    use super::*;

    fn recovery(host_surface: SurfaceId) -> SurfaceRecoveryBootstrap {
        SurfaceRecoveryBootstrap::new(host_surface)
    }

    fn workspace(surfaces: &[SurfaceId]) -> Workspace {
        let mut builder = Workspace::builder();
        for (index, surface) in surfaces.iter().copied().enumerate() {
            let id = u64::try_from(index + 1).unwrap();
            let node = builder.insert_node(Node::tabs([ItemId::new(id)]));
            let root = RootId::new(id);
            builder.set_root(root, RootRecord::new(node));
            builder.set_surface(surface, SurfacePresentation::with_main(root));
        }
        builder.build().unwrap()
    }

    #[test]
    fn restored_placement_uses_outer_position_and_inner_size_at_the_confirmed_scale() {
        let surface = SurfaceId::new(2);
        let preference = ViewportPlacementPreference::new(
            surface,
            PhysicalRect::new(200.0, 100.0, 1600.0, 1200.0)
                .expect("fixture outer rect must be valid"),
        )
        .expect("fixture placement must be non-empty")
        .try_with_inner_size(
            PhysicalSize::new(1560.0, 1160.0).expect("fixture inner size must be valid"),
        )
        .expect("fixture inner size must be non-empty")
        .with_scale_factor(ScaleFactor::new(2.0).expect("fixture scale must be valid"))
        .with_presentation(WindowPresentationPreference::Maximized);
        let spec = NativeSurfaceSpec::restored_child(
            ViewportId::from_hash_of("restored-placement"),
            surface,
            WindowToken::new(2),
            recovery(SurfaceId::new(1)),
            ViewportBuilder::default(),
        )
        .try_with_restored_placement(preference)
        .expect("matching placement must configure the viewport");

        assert_eq!(spec.builder.position, Some(egui::pos2(100.0, 50.0)));
        assert_eq!(spec.builder.inner_size, Some(egui::vec2(780.0, 580.0)));
        assert_eq!(spec.builder.maximized, Some(true));
    }

    #[test]
    fn restored_placement_rejects_another_surface_identity() {
        let expected = SurfaceId::new(2);
        let found = SurfaceId::new(3);
        let preference = ViewportPlacementPreference::new(
            found,
            PhysicalRect::new(0.0, 0.0, 100.0, 100.0).expect("fixture outer rect must be valid"),
        )
        .expect("fixture placement must be non-empty");
        assert!(matches!(
            NativeSurfaceSpec::restored_child(
                ViewportId::from_hash_of("mismatched-placement"),
                expected,
                WindowToken::new(2),
                recovery(SurfaceId::new(1)),
                ViewportBuilder::default(),
            )
            .try_with_restored_placement(preference),
            Err(NativeRuntimeError::RestoredPlacementSurfaceMismatch {
                expected: error_expected,
                found: error_found,
            }) if error_expected == expected && error_found == found
        ));
    }

    #[test]
    fn restored_children_must_name_existing_workspace_surfaces() {
        let root_surface = SurfaceId::new(1);
        let missing_surface = SurfaceId::new(2);
        let mut roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(root_surface, WindowToken::new(1)))
                .unwrap();
        roster
            .insert(NativeSurfaceSpec::restored_child(
                ViewportId::from_hash_of("restored"),
                missing_surface,
                WindowToken::new(2),
                recovery(root_surface),
                ViewportBuilder::default(),
            ))
            .unwrap();

        assert!(matches!(
            roster.validate_workspace(&workspace(&[root_surface])),
            Err(NativeRuntimeError::WorkspaceRosterMismatch)
        ));
        roster
            .validate_workspace(&workspace(&[root_surface, missing_surface]))
            .unwrap();
    }

    #[test]
    fn dormant_and_restored_children_remain_distinct() {
        let root_surface = SurfaceId::new(1);
        let restored_surface = SurfaceId::new(2);
        let dormant_surface = SurfaceId::new(3);
        let restored_viewport = ViewportId::from_hash_of("restored");
        let mut roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(root_surface, WindowToken::new(1)))
                .unwrap();
        roster
            .insert(NativeSurfaceSpec::restored_child(
                restored_viewport,
                restored_surface,
                WindowToken::new(2),
                recovery(root_surface),
                ViewportBuilder::default(),
            ))
            .unwrap();
        roster
            .insert(NativeSurfaceSpec::child(
                ViewportId::from_hash_of("dormant"),
                dormant_surface,
                ViewportBuilder::default(),
            ))
            .unwrap();

        assert_eq!(
            roster
                .restored_children()
                .map(NativeSurfaceSpec::viewport)
                .collect::<Vec<_>>(),
            vec![restored_viewport]
        );
        roster
            .validate_workspace(&workspace(&[root_surface, restored_surface]))
            .expect("dormant templates do not participate in restored coverage");
        assert!(matches!(
            roster.validate_workspace(&workspace(&[
                root_surface,
                restored_surface,
                dormant_surface,
            ])),
            Err(NativeRuntimeError::WorkspaceRosterMismatch)
        ));
    }

    #[test]
    fn restored_child_materializes_hidden_without_changing_its_live_builder() {
        let root_surface = SurfaceId::new(1);
        let child_surface = SurfaceId::new(2);
        let viewport = ViewportId::from_hash_of("restored staging");
        let mut roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(root_surface, WindowToken::new(1)))
                .unwrap();
        roster
            .insert(NativeSurfaceSpec::restored_child(
                viewport,
                child_surface,
                WindowToken::new(2),
                recovery(root_surface),
                ViewportBuilder::default()
                    .with_visible(true)
                    .with_active(true),
            ))
            .unwrap();

        let staging = roster
            .restored_staging_builder(viewport)
            .expect("restored child must have a staging builder");
        assert_eq!(staging.visible, Some(false));
        assert_eq!(staging.active, Some(false));

        let live = roster
            .builder(viewport)
            .expect("restored child must retain its live builder");
        assert_eq!(live.visible, Some(true));
        assert_eq!(live.active, Some(true));
    }

    #[test]
    fn every_existing_non_root_surface_requires_one_restored_child() {
        let root_surface = SurfaceId::new(1);
        let restored_surface = SurfaceId::new(2);
        let mut roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(root_surface, WindowToken::new(1)))
                .unwrap();

        assert!(matches!(
            roster.validate_workspace(&workspace(&[root_surface, restored_surface])),
            Err(NativeRuntimeError::WorkspaceRosterMismatch)
        ));

        roster
            .insert(NativeSurfaceSpec::restored_child(
                ViewportId::from_hash_of("restored exact coverage"),
                restored_surface,
                WindowToken::new(2),
                recovery(root_surface),
                ViewportBuilder::default(),
            ))
            .unwrap();
        roster
            .validate_workspace(&workspace(&[root_surface, restored_surface]))
            .expect("root and restored children exactly cover the workspace");
    }

    #[test]
    fn restored_child_recovery_must_target_the_configured_root() {
        let root_surface = SurfaceId::new(1);
        let child_surface = SurfaceId::new(2);
        let mut roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(root_surface, WindowToken::new(1)))
                .unwrap();
        roster
            .insert(NativeSurfaceSpec::restored_child(
                ViewportId::from_hash_of("invalid recovery host"),
                child_surface,
                WindowToken::new(2),
                recovery(child_surface),
                ViewportBuilder::default(),
            ))
            .unwrap();

        assert!(matches!(
            roster.validate_workspace(&workspace(&[root_surface, child_surface])),
            Err(NativeRuntimeError::WorkspaceRosterMismatch)
        ));
    }
}
