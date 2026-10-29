use super::callback_prefix::{commit_boundary, pointer_record};
use super::*;
use dockspace::runtime::NativeInputStorageLimit;
use std::num::NonZeroUsize;

fn bounded_coordinator(slots: usize) -> (NativeCoordinator, NativeSurfaceBinding) {
    let layout = DockspaceLayout::new([
        DockspaceSurfaceLayout::new(
            FIRST_SURFACE,
            DockspaceRootLayout::new(
                RootId::new(1),
                DockspaceNode::central_tabs([ItemId::new(1)]),
            ),
        ),
        DockspaceSurfaceLayout::new(
            SECOND_SURFACE,
            DockspaceRootLayout::new(RootId::new(2), DockspaceNode::tabs([ItemId::new(2)])),
        ),
    ])
    .unwrap();
    let session = DockspaceSession::from_layout(layout, DockPolicy::default()).unwrap();
    let mut native = NativeCoordinator::new_with_input_storage_limit(
        session,
        NativePointerRoster::Exact(Vec::new()),
        Some(NativeInputStorageLimit::new(
            NonZeroUsize::new(slots).unwrap(),
            1024 * 1024,
        )),
    )
    .unwrap();
    let (binding, _) = register_roots(&mut native);
    native
        .bind_viewport(ViewportId::ROOT, WindowId::from(11), binding)
        .unwrap();
    assert!(!native.session.has_pending_native_input());
    (native, binding)
}

#[test]
fn bounded_cursor_prefix_clips_yields_when_full_and_resumes_the_same_stream() {
    let (mut native, binding) = bounded_coordinator(4);
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );

    // Activate the real translator before testing capacity, so idle coalescing
    // cannot hide an incorrectly consumed cursor suffix.
    for (ordinal, pressed) in [(1, true), (2, false)] {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, pressed,
            )));
        assert_eq!(
            native
                .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
                .unwrap()
                .unwrap()
                .get(),
            1
        );
        commit_boundary(&mut native);
    }
    assert!(!native.pointer_translator.can_coalesce_idle_cursor_moves());
    let movement_record = pointer_record(&native, binding, 2, false);
    let NativePointerTranslation::Input(movement) = native
        .pointer_translator
        .clone()
        .translate(&movement_record, |_| None)
    else {
        panic!("the pressed pointer must retain its movement stream");
    };

    native
        .session
        .record_native_pointer_sequence([movement; 2])
        .unwrap();
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(2)
    );
    for ordinal in 3..=6 {
        native
            .bridge
            .push_record(HostRecord::WindowEvent(pointer_record(
                &native, binding, ordinal, false,
            )));
    }
    let recorded = native.session.native_input_recorded_count();
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        2
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 5);
    assert!(native.bridge.callback_boundary_pending());
    assert!(native.session.has_pending_native_input());
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(0)
    );
    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );

    // Fill core independently while the exact raw suffix still belongs to the bridge.
    native
        .session
        .record_native_pointer_sequence([movement; 4])
        .unwrap();
    assert!(!native.bridge.callback_boundary_pending());
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    let translator = format!("{:?}", native.pointer_translator);
    let mut budget = test_budget();
    let allowance = budget.remaining_records();
    assert!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut budget)
            .unwrap()
            .is_none()
    );
    assert!(native.bridge.callback_boundary_pending());
    assert!(native.session.has_pending_native_input());
    assert_eq!(native.session.native_input_recorded_count(), recorded);
    assert_eq!(native.session.version(), version);
    assert_eq!(budget.remaining_records(), allowance);
    assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 5);
    assert_eq!(format!("{:?}", native.pointer_translator), translator);

    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert!(!native.bridge.callback_boundary_pending());
    assert_eq!(
        native.session.prepare_native_pointer_capacity().unwrap(),
        Some(4)
    );
    assert_eq!(
        native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .unwrap()
            .unwrap()
            .get(),
        2
    );
    assert_eq!(native.session.native_input_recorded_count(), recorded + 2);
    assert!(native.next_window_event().unwrap().is_none());
    assert_eq!(
        native
            .pointer_translator
            .clone()
            .translate(&movement_record, |_| None),
        NativePointerTranslation::Input(movement),
        "reclaiming transport must preserve the original pressed pointer identity"
    );
    commit_boundary(&mut native);
    assert!(!native.session.has_pending_native_input());
    assert!(!native.bridge.callback_boundary_pending());
}

#[test]
fn an_oversized_terminal_group_without_preceding_input_returns_capacity_instead_of_spinning() {
    let (mut native, binding) = bounded_coordinator(4);
    for device in 1..=5 {
        let press = pointer_record(&native, binding, device, true);
        let NativePointerTranslation::Input(input) = native
            .pointer_translator
            .translate_for_test_device(&press, device, |_| None)
        else {
            panic!("each independent pressed device must produce a live stream");
        };
        native.session.record_native_pointer(input).unwrap();
        commit_boundary(&mut native);
    }
    let destroyed = native.bridge.with_viewports_mut(|routes| {
        NativeWindowEventRecord::for_test_ingress(
            100,
            WindowId::from(11),
            Some(ViewportId::ROOT),
            WindowEvent::Destroyed,
            routes,
        )
    });
    native
        .bridge
        .push_record(HostRecord::WindowEvent(destroyed));
    let recorded = native.session.native_input_recorded_count();
    let version = native.session.version();
    let translator = format!("{:?}", native.pointer_translator);
    assert!(!native.session.has_pending_native_input());
    for _ in 0..2 {
        let error = native
            .reduce_callback_prefix(&egui::Context::default(), &mut test_budget())
            .expect_err("four slots cannot admit the exact five-edge terminal group");
        assert!(error.is_input_capacity());
        assert!(
            !native.bridge.callback_boundary_pending(),
            "an empty core cannot promise progress through another empty frame"
        );
        assert_eq!(native.next_window_event().unwrap().unwrap().ordinal(), 100);
        assert_eq!(native.session.native_input_recorded_count(), recorded);
        assert_eq!(native.session.version(), version);
        assert_eq!(format!("{:?}", native.pointer_translator), translator);
        assert!(!native.session.has_pending_native_input());
    }
}
