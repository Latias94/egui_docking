//! Thin eframe storage bridge for the atomic dockspace document.

use dockspace::document::DockspaceDocumentId;
use dockspace::ids::ItemId;
use eframe::Storage;
use egui_dockspace::{Dockspace, DockspaceDocumentLoad};

use crate::NativeRuntimeError;

/// Stable eframe storage key for the complete dockspace document.
pub const NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY: &str = "egui_dockspace_native.document.v1";

/// Restores one complete document before constructing the native runtime.
///
/// The application should build its pane registry and native viewport roster
/// from the restored `Dockspace` only after this function succeeds. Invalid
/// storage is returned as an error and is never replaced with fallback state.
///
/// # Errors
///
/// Returns the underlying strict document decode, association, or publication
/// failure. The live dockspace and storage remain unchanged on failure.
pub fn restore_document_from_storage(
    dockspace: &mut Dockspace,
    storage: Option<&dyn Storage>,
    prove_external_item_association: impl Fn(DockspaceDocumentId, ItemId, &str) -> bool,
) -> Result<Option<DockspaceDocumentLoad>, NativeRuntimeError> {
    let Some(json) =
        storage.and_then(|storage| storage.get_string(NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY))
    else {
        return Ok(None);
    };
    dockspace
        .load_document_json(&json, prove_external_item_association)
        .map(Some)
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use dockspace::document::{DockspaceDocumentBootstrap, DockspaceDocumentId};
    use dockspace::graph::{Node, RootRecord, SurfacePresentation, Workspace};
    use dockspace::ids::{ItemId, RootId, SurfaceId};
    use dockspace::viewport::WindowToken;
    use egui_dockspace::PaneView;

    use super::*;
    use crate::{NativeDockspaceApp, NativeSurfaceSpec, NativeViewportRoster};

    const DOCUMENT: DockspaceDocumentId = DockspaceDocumentId::from_bytes(*b"native-persist!!");
    const ROOT: RootId = RootId::new(1);
    const CHILD_ROOT: RootId = RootId::new(2);
    const SURFACE: SurfaceId = SurfaceId::new(1);
    const CHILD_SURFACE: SurfaceId = SurfaceId::new(2);

    #[derive(Default)]
    struct MemoryStorage {
        values: BTreeMap<String, String>,
    }

    impl Storage for MemoryStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.values.get(key).cloned()
        }

        fn set_string(&mut self, key: &str, value: String) {
            self.values.insert(key.to_owned(), value);
        }

        fn remove_string(&mut self, key: &str) {
            self.values.remove(key);
        }

        fn flush(&mut self) {}
    }

    struct EmptyPane;

    impl PaneView for EmptyPane {
        fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
            Some("Pane".into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut egui::Ui) {}
    }

    fn workspace(item: ItemId) -> Workspace {
        let mut builder = Workspace::builder();
        let tabs = builder.insert_node(Node::tabs([item]));
        builder.set_root(ROOT, RootRecord::new(tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        builder
            .build()
            .expect("native persistence workspace is valid")
    }

    fn bootstrap(next_generation: u64) -> (DockspaceDocumentBootstrap, ItemId) {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT, next_generation);
        let item = bootstrap
            .ensure_external_item_key("pane/one")
            .expect("native persistence identity must fit");
        (bootstrap, item)
    }

    fn bound_dockspace(next_generation: u64) -> Dockspace {
        let (bootstrap, item) = bootstrap(next_generation);
        let mut dockspace = Dockspace::builder("native-persistence", workspace(item))
            .build()
            .expect("native persistence dockspace must build");
        dockspace
            .bind_document_persistence(bootstrap)
            .expect("native persistence bootstrap must bind");
        dockspace
    }

    fn bound_two_surface_dockspace(next_generation: u64) -> Dockspace {
        let mut bootstrap = DockspaceDocumentBootstrap::new(DOCUMENT, next_generation);
        let first = bootstrap
            .ensure_external_item_key("pane/one")
            .expect("first native persistence identity must fit");
        let second = bootstrap
            .ensure_external_item_key("pane/two")
            .expect("second native persistence identity must fit");
        let mut builder = Workspace::builder();
        let root_tabs = builder.insert_node(Node::tabs([first]));
        let child_tabs = builder.insert_node(Node::tabs([second]));
        builder.set_root(ROOT, RootRecord::new(root_tabs));
        builder.set_root(CHILD_ROOT, RootRecord::new(child_tabs));
        builder.set_surface(SURFACE, SurfacePresentation::with_main(ROOT));
        builder.set_surface(CHILD_SURFACE, SurfacePresentation::with_main(CHILD_ROOT));
        let workspace = builder
            .build()
            .expect("two-surface persistence workspace is valid");
        let mut dockspace = Dockspace::builder("native-persistence-two", workspace)
            .build()
            .expect("two-surface native persistence dockspace must build");
        dockspace
            .bind_document_persistence(bootstrap)
            .expect("two-surface persistence bootstrap must bind");
        dockspace
    }

    fn native_app(next_generation: u64) -> NativeDockspaceApp<EmptyPane> {
        let roster =
            NativeViewportRoster::new(NativeSurfaceSpec::root(SURFACE, WindowToken::new(1)))
                .expect("native persistence root roster must build");
        NativeDockspaceApp::new(bound_dockspace(next_generation), EmptyPane, roster)
            .expect("native persistence app must build")
    }

    #[test]
    fn storage_restore_adopts_one_complete_document_before_native_startup() {
        let mut source = bound_dockspace(7);
        let json = source
            .save_document_json()
            .expect("source document must capture");
        let mut storage = MemoryStorage::default();
        storage.set_string(NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY, json);

        let (_, fallback_item) = bootstrap(0);
        let mut target = Dockspace::builder("native-restore", workspace(fallback_item))
            .build()
            .expect("unbound restore target must build");
        let restored =
            restore_document_from_storage(&mut target, Some(&storage), |id, item, key| {
                id == DOCUMENT && item == fallback_item && key == "pane/one"
            })
            .expect("valid stored document must restore")
            .expect("stored document must be present");

        assert_eq!(restored.document_id(), DOCUMENT);
        assert_eq!(restored.generation(), 7);
        assert_eq!(
            target.item_id_for_external_key("pane/one"),
            Some(fallback_item)
        );
    }

    #[test]
    fn failed_background_capture_keeps_the_last_valid_storage_value() {
        let mut app = native_app(u64::MAX);
        let mut storage = MemoryStorage::default();
        assert!(
            matches!(app.save_document_to_storage(&mut storage), Ok(true)),
            "the final representable generation must still persist",
        );
        let valid = storage
            .get_string(NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY)
            .expect("successful capture must publish storage");

        eframe::App::save(&mut app, &mut storage);

        assert_eq!(
            storage.get_string(NATIVE_DOCKSPACE_DOCUMENT_STORAGE_KEY),
            Some(valid),
            "an infallible eframe callback must not replace valid storage after capture failure",
        );
        assert!(app.last_persistence_error().is_some());
    }

    #[test]
    fn live_restore_rejects_a_document_outside_the_configured_native_roster() {
        let mut source = bound_two_surface_dockspace(7);
        let json = source
            .save_document_json()
            .expect("two-surface source document must capture");
        let mut app = native_app(1);

        let result = app.queue_document_json(&json, |document, item, key| {
            document == DOCUMENT
                && ((item == ItemId::new(1) && key == "pane/one")
                    || (item == ItemId::new(2) && key == "pane/two"))
        });

        assert!(matches!(
            result,
            Err(NativeRuntimeError::WorkspaceRosterMismatch)
        ));
        assert!(!app.dockspace().has_pending_document_restore());
    }
}
