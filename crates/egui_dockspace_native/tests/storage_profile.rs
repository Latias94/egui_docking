use std::num::NonZeroUsize;

use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, NativeInputStorageLimit, NativeProducerStorageLimit, NativeReportStorageLimit,
};
use eframe::egui;
use egui_dockspace::{DockStyle, PaneView};
use egui_dockspace_native::{
    NativeDockspaceApp, NativeRuntimeErrorKind, NativeStorageLimit, NativeWindowClosePolicy,
};

const SURFACE: SurfaceId = SurfaceId::new(1);

struct Panes;

impl PaneView for Panes {
    fn title(&self, _item: ItemId) -> Option<egui::WidgetText> {
        Some("Storage profile".into())
    }

    fn ui(&mut self, _item: ItemId, _ui: &mut egui::Ui) {}
}

fn session() -> DockspaceSession {
    let layout = DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap();
    DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap()
}

#[test]
fn public_storage_profile_rejects_incomplete_or_short_limits_and_selects_bounded_app_storage() {
    let slots = |count| NonZeroUsize::new(count).unwrap();
    let total = 4 * 1024 * 1024;
    let transport = NativeInputStorageLimit::new(slots(512), 768 * 1024);
    let producer = NativeProducerStorageLimit::new(slots(16), slots(8), 128 * 1024);
    let reports = NativeReportStorageLimit::new(slots(64), slots(16), 256 * 1024);
    for incomplete in [
        transport,
        transport.with_producer_storage_limit(producer),
        transport.with_report_storage_limit(reports),
    ] {
        assert_eq!(
            NativeStorageLimit::new(incomplete, total, 48, 32, 16)
                .unwrap_err()
                .kind(),
            NativeRuntimeErrorKind::InvalidConfiguration,
        );
    }
    let input = transport
        .with_producer_storage_limit(producer)
        .with_report_storage_limit(reports);
    let profile = NativeStorageLimit::new(input, total, 48, 32, 16).unwrap();
    let required = profile.initial_reserved_bytes();
    assert!(required > 0 && required <= profile.byte_limit());
    // The public profile rejects insufficient storage before taking a session.
    assert_eq!(
        NativeStorageLimit::new(input, required - 1, 48, 32, 16)
            .unwrap_err()
            .kind(),
        NativeRuntimeErrorKind::OperationConflict,
    );

    let exact = NativeStorageLimit::new(input, required, 48, 32, 16).unwrap();
    let bounded = NativeDockspaceApp::new_with_storage_limit(
        egui::Id::unique("public-bounded-storage"),
        session(),
        SURFACE,
        Panes,
        DockStyle::default(),
        NativeWindowClosePolicy::default(),
        exact,
    )
    .unwrap();
    let reserved = bounded
        .input_storage_reserved_bytes()
        .expect("the public bounded constructor installs its reservation");
    let resident = bounded.input_storage_capacity_bytes().unwrap();
    assert!(resident <= reserved && reserved <= exact.byte_limit());

    let default = NativeDockspaceApp::new(
        egui::Id::unique("public-default-storage"),
        session(),
        SURFACE,
        Panes,
        DockStyle::default(),
    )
    .unwrap();
    assert_eq!(default.input_storage_reserved_bytes(), None);
}
