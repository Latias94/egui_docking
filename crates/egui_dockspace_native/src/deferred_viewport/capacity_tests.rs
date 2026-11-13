use super::*;
use dockspace::model::{
    DockspaceLayout, DockspaceNode, DockspaceRootLayout, DockspaceSurfaceLayout, ItemId, RootId,
    SurfaceId,
};
use dockspace::policy::DockPolicy;
use dockspace::runtime::{
    DockspaceSession, HostWindowToken, NativePointerRoster, NativeReceiverAnswer,
    SurfaceUnavailableReason,
};

const SURFACE: SurfaceId = SurfaceId::new(1);

fn layout() -> DockspaceLayout {
    DockspaceLayout::new([DockspaceSurfaceLayout::new(
        SURFACE,
        DockspaceRootLayout::new(
            RootId::new(1),
            DockspaceNode::central_tabs([ItemId::new(1)]),
        ),
    )])
    .unwrap()
}

fn session() -> (DockspaceSession, NativeSurfaceBinding) {
    let mut session = DockspaceSession::from_layout(layout(), DockPolicy::default()).unwrap();
    session
        .enable_managed_native_host(NativePointerRoster::Unknown)
        .unwrap();
    session
        .register_native_root(SURFACE, HostWindowToken::new(41))
        .unwrap();
    let mut frame = session
        .begin_native_host_frame(|_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let binding = frame.commit().unwrap().native_bindings()[0];
    (session, binding)
}

fn bounds() -> PhysicalRect {
    PhysicalRect::new(40.0, 50.0, 360.0, 280.0).unwrap()
}

#[test]
fn child_window_appearance_preserves_hidden_preparation_and_visible_redeclaration() {
    use std::sync::Arc;

    let (_session, binding) = session();
    let viewport = viewport_id_for(binding);
    let mut driver = DeferredViewportDriver::default();
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    let context = egui::Context::default();
    context.set_embed_viewports(false);
    let icon = Arc::new(egui::IconData {
        rgba: vec![255; 4 * 4 * 4],
        width: 4,
        height: 4,
    });
    let options = NativeChildWindowOptions {
        decorations: Some(false),
        taskbar: Some(false),
        window_level: Some(egui::WindowLevel::AlwaysOnTop),
        icon: Some(Arc::clone(&icon)),
    };
    let mut declared_icons = DeclaredWindowIcons::new();
    for configured in [false, true] {
        let options = if configured {
            options.clone()
        } else {
            NativeChildWindowOptions::default()
        };
        for visible in [false, true] {
            if visible {
                assert_eq!(driver.set_visible(binding), Some(viewport));
            } else {
                driver.entries[0].set_visible(false);
            }
            let specs = driver.retained_specs();
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                declare_deferred_viewports(
                    ui.ctx(),
                    &specs,
                    &mut declared_icons,
                    |_| options.clone(),
                    |_| "Inspector".into(),
                    |_, _, _| panic!("a deferred declaration must not render in the Root pass"),
                );
            });
            output.textures_delta.clear();
            let child = &output.viewport_output[&viewport];
            assert!(child.class == ViewportClass::Deferred);
            assert!(child.viewport_ui_cb.is_some());
            let expected = if configured {
                ViewportBuilder::default()
                    .with_decorations(false)
                    .with_taskbar(false)
                    .with_window_level(egui::WindowLevel::AlwaysOnTop)
                    .with_icon(Arc::clone(&icon))
            } else {
                ViewportBuilder::default()
                    .with_decorations(true)
                    .with_taskbar(true)
                    .with_window_level(egui::WindowLevel::Normal)
            }
            .with_visible(visible)
            .with_title("Inspector");
            assert_eq!(
                child.builder, expected,
                "appearance must not override native placement, focus, visibility or pointer passthrough",
            );
            output.drop_without_applying_deltas();
        }
    }
}

#[test]
fn surface_appearance_survives_replacement_and_clear_restores_unset_global_defaults() {
    let (mut session, mut binding) = session();
    let first = binding;
    let mut driver = DeferredViewportDriver::default();
    let context = egui::Context::default();
    context.set_embed_viewports(false);
    let defaults = NativeChildWindowOptions::default();
    let tools = NativeChildWindowOptions {
        decorations: Some(false),
        taskbar: Some(false),
        window_level: Some(egui::WindowLevel::AlwaysOnTop),
        icon: Some(std::sync::Arc::new(egui::IconData {
            rgba: vec![255; 4],
            width: 1,
            height: 1,
        })),
    };
    let mut overrides = std::collections::BTreeMap::from([(SURFACE, tools.clone())]);
    let mut previous_builder: Option<ViewportBuilder> = None;
    let mut declared_icons = DeclaredWindowIcons::new();
    for step in 0..5 {
        if matches!(step, 1 | 4) {
            let previous = binding;
            let reset = session.prepare_layout_reset(layout()).unwrap();
            let mut frame = session
                .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
                .unwrap();
            frame
                .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
                .unwrap();
            binding = frame.commit().unwrap().native_bindings()[0];
            assert_ne!(binding, first);
            assert_eq!(binding.surface(), first.surface());
            assert_ne!(viewport_id_for(binding), viewport_id_for(first));
            assert_eq!(driver.remove(previous), Some(viewport_id_for(previous)));
        } else if step == 2 {
            assert_eq!(overrides.remove(&SURFACE), Some(tools.clone()));
            assert_eq!(driver.set_visible(binding), Some(viewport_id_for(binding)));
        }
        let viewport = viewport_id_for(binding);
        assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
        let specs = driver.retained_specs();
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            declare_deferred_viewports(
                ui.ctx(),
                &specs,
                &mut declared_icons,
                |surface| {
                    assert_eq!(surface, SURFACE);
                    overrides.get(&surface).map_or_else(
                        || defaults.clone(),
                        |options| defaults.with_overrides(options),
                    )
                },
                |_| "Inspector".into(),
                |_, _, _| panic!("a deferred declaration must not render in the Root pass"),
            );
        });
        output.textures_delta.clear();
        let child = &output.viewport_output[&viewport];
        let options = if step >= 2 {
            defaults.clone()
        } else {
            defaults.with_overrides(&tools)
        };
        assert_eq!(
            child.builder,
            options
                .viewport_builder()
                .with_visible(specs[0].visible)
                .with_title("Inspector"),
            "stable surface options must survive a new binding and viewport, step={step}"
        );
        if step == 2 {
            let (commands, _) = previous_builder
                .as_mut()
                .unwrap()
                .patch(child.builder.clone());
            assert!(commands.contains(&egui::ViewportCommand::Decorations(true)));
            assert!(commands.contains(&egui::ViewportCommand::WindowLevel(
                egui::WindowLevel::Normal
            )));
            assert_eq!(previous_builder.as_ref().unwrap().taskbar, Some(true));
            assert_eq!(icon_clear_count(&child.commands), 1);
        } else if step == 3 {
            assert_eq!(icon_clear_count(&child.commands), 0);
        } else if step == 4 {
            let before = previous_builder.as_ref().unwrap();
            assert_eq!(child.builder.decorations, before.decorations);
            assert_eq!(child.builder.taskbar, before.taskbar);
            assert_eq!(child.builder.window_level, before.window_level);
            assert_eq!(child.builder.icon, before.icon);
            assert_eq!(icon_clear_count(&child.commands), 1);
        }
        assert_eq!(declared_icons.len(), 1, "retired viewport, step={step}");
        previous_builder = Some(child.builder.clone());
        output.drop_without_applying_deltas();
    }
}

fn icon_clear_count(commands: &[egui::ViewportCommand]) -> usize {
    commands
        .iter()
        .filter(|command| matches!(command, egui::ViewportCommand::Icon(None)))
        .count()
}

#[test]
fn global_and_surface_icon_changes_clear_once_and_can_reapply_the_same_arc() {
    use std::sync::Arc;

    let (_session, binding) = session();
    let viewport = viewport_id_for(binding);
    let mut driver = DeferredViewportDriver::default();
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    let specs = driver.retained_specs();
    let icon = Arc::new(egui::IconData {
        rgba: vec![255; 4],
        width: 1,
        height: 1,
    });
    let parent_icon = Arc::new(egui::IconData {
        rgba: vec![0, 0, 255, 255],
        width: 1,
        height: 1,
    });
    for per_surface in [false, true] {
        for parent_has_icon in [false, true] {
            let context = egui::Context::default();
            context.set_embed_viewports(false);
            let mut declared_icons = DeclaredWindowIcons::new();
            let mut retained = ViewportBuilder::default();
            let mut window_icon = None;
            for step in 0..8 {
                let configured = NativeChildWindowOptions {
                    icon: matches!(step, 1 | 4).then(|| Arc::clone(&icon)),
                    taskbar: (step >= 6).then_some(false),
                    ..Default::default()
                };
                let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                    declare_deferred_viewports(
                        ui.ctx(),
                        &specs,
                        &mut declared_icons,
                        |_| {
                            if per_surface {
                                NativeChildWindowOptions::default().with_overrides(&configured)
                            } else {
                                configured.clone()
                            }
                        },
                        |_| "Inspector".into(),
                        |_, _, _| panic!("deferred declaration must not invoke pane UI"),
                    );
                });
                output.textures_delta.clear();
                let child = &output.viewport_output[&viewport];
                // Use the real builder patch, including eframe's parent-icon
                // fallback and recreation, before applying explicit commands.
                let mut builder = child.builder.clone();
                if builder.icon.is_none() && parent_has_icon {
                    builder.icon = Some(Arc::clone(&parent_icon));
                }
                let (commands, recreate) = retained.patch(builder);
                if recreate {
                    window_icon = retained.icon.clone();
                }
                if step == 4 && !parent_has_icon {
                    assert!(
                        commands
                            .iter()
                            .all(|command| { !matches!(command, egui::ViewportCommand::Icon(_)) })
                    );
                }
                for command in commands.iter().chain(&child.commands) {
                    if let egui::ViewportCommand::Icon(icon) = command {
                        window_icon.clone_from(icon);
                    }
                }
                assert_eq!(
                    window_icon, configured.icon,
                    "step={step}, per_surface={per_surface}, parent_has_icon={parent_has_icon}"
                );
                assert_eq!(
                    icon_clear_count(&child.commands),
                    usize::from(matches!(step, 0 | 2 | 5 | 6))
                );
                output.drop_without_applying_deltas();
            }
            let mut output = context.run_ui(egui::RawInput::default(), |ui| {
                declare_deferred_viewports(
                    ui.ctx(),
                    &[],
                    &mut declared_icons,
                    |_| unreachable!("the managed viewport roster is empty"),
                    |_| unreachable!("the managed viewport roster is empty"),
                    |_, _, _| unreachable!("the managed viewport roster is empty"),
                );
            });
            output.textures_delta.clear();
            assert!(declared_icons.is_empty());
            output.drop_without_applying_deltas();
        }
    }
}

#[test]
fn full_driver_preserves_exact_declarations_and_rejects_foreign_keys_before_mutation() {
    let (_session, binding) = session();
    let (_foreign_session, foreign) = session();
    let viewport = ViewportId::from_hash_of("finite deferred");
    let other = ViewportId::from_hash_of("foreign deferred");
    let mut driver = DeferredViewportDriver::with_capacity_limit(1).unwrap();
    let baseline = driver.owned_data_capacity_bytes();
    let backing = driver.entries.as_ptr();
    assert!(driver.can_insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.can_insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    let existing = driver.specs().collect::<Vec<_>>();
    let changed_bounds = PhysicalRect::new(41.0, 50.0, 360.0, 280.0).unwrap();
    for (key, exact, placement, role) in [
        (viewport, foreign, bounds(), NativeSurfaceRole::Child),
        (other, binding, bounds(), NativeSurfaceRole::Child),
        (other, foreign, bounds(), NativeSurfaceRole::Child),
        (viewport, binding, changed_bounds, NativeSurfaceRole::Child),
        (viewport, binding, bounds(), NativeSurfaceRole::Root),
    ] {
        assert!(!driver.can_insert(key, exact, placement, role));
        assert!(!driver.insert(key, exact, placement, role));
        assert_eq!(driver.specs().collect::<Vec<_>>(), existing);
        assert_eq!(driver.entries.as_ptr(), backing);
        assert_eq!(driver.owned_data_capacity_bytes(), baseline);
    }
    assert_eq!(driver.remove(foreign), None);
    assert_eq!(driver.set_visible(foreign), None);
    assert_eq!(driver.viewport_for(foreign), None);
    assert!(driver.has_transitional_viewport());
    assert_eq!(driver.set_visible(binding), Some(viewport));
    assert!(!driver.has_transitional_viewport());
    assert!(driver.insert(viewport, binding, bounds(), NativeSurfaceRole::Child));
    assert!(driver.specs().next().unwrap().visible);
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.owned_data_capacity_bytes(), baseline);
}

#[test]
fn retiring_and_rebinding_reuses_prepaid_backing_and_old_incarnations_cannot_remove_the_new_one() {
    let (mut session, old) = session();
    let mut driver = DeferredViewportDriver::with_capacity_limit(1).unwrap();
    let viewport = ViewportId::from_hash_of("rebound deferred");
    assert!(driver.insert(viewport, old, bounds(), NativeSurfaceRole::Root));
    let baseline = driver.owned_data_capacity_bytes();
    let backing = driver.entries.as_ptr();
    let reset = session.prepare_layout_reset(layout()).unwrap();
    let mut frame = session
        .begin_native_layout_reset_frame(&reset, |_| NativeReceiverAnswer::Unknown)
        .unwrap();
    frame
        .complete_unpainted_surfaces(SurfaceUnavailableReason::Deferred)
        .unwrap();
    let new = frame.commit().unwrap().native_bindings()[0];
    assert_ne!(new, old);
    assert_eq!(new.surface(), old.surface());
    assert!(!driver.can_insert(viewport, new, bounds(), NativeSurfaceRole::Root));
    assert_eq!(driver.remove(old), Some(viewport));
    assert!(driver.specs().next().is_none());
    assert!(!driver.has_transitional_viewport());
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.owned_data_capacity_bytes(), baseline);
    assert!(driver.can_insert(viewport, new, bounds(), NativeSurfaceRole::Root));
    assert!(driver.insert(viewport, new, bounds(), NativeSurfaceRole::Root));
    assert_eq!(driver.remove(old), None);
    assert_eq!(driver.set_visible(old), None);
    assert_eq!(driver.viewport_for(new), Some(viewport));
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.owned_data_capacity_bytes(), baseline);
}

#[test]
fn finite_reservations_match_actual_capacity_and_overflow_rejects_before_allocation() {
    for slots in [0, 1, 2, 7, 65] {
        let driver = DeferredViewportDriver::with_capacity_limit(slots).unwrap();
        let required = DeferredViewportDriver::reservation_bytes(slots).unwrap();
        assert_eq!(driver.entries.capacity(), slots);
        assert_eq!(driver.owned_data_capacity_bytes(), required);
    }
    assert_eq!(DeferredViewportDriver::reservation_bytes(usize::MAX), None);
    assert_eq!(
        DeferredViewportDriver::with_capacity_limit(usize::MAX).unwrap_err(),
        NativeHostProtocolError::NativeReportStorageUnavailable
    );
    let (_session, binding) = session();
    let mut zero = DeferredViewportDriver::with_capacity_limit(0).unwrap();
    assert!(!zero.can_insert(
        ViewportId::ROOT,
        binding,
        bounds(),
        NativeSurfaceRole::Child
    ));
    assert!(!zero.insert(
        ViewportId::ROOT,
        binding,
        bounds(),
        NativeSurfaceRole::Child
    ));
}

#[test]
fn unlimited_driver_preflights_backing_and_preserves_viewport_order() {
    let (_first_session, first) = session();
    let (_second_session, second) = session();
    let low = ViewportId::from_hash_of("deferred low");
    let high = ViewportId::from_hash_of("deferred high");
    let (low, high) = if low < high { (low, high) } else { (high, low) };
    let mut driver = DeferredViewportDriver::default();
    assert!(driver.can_insert(high, first, bounds(), NativeSurfaceRole::Child));
    let capacity = driver.entries.capacity();
    let backing = driver.entries.as_ptr();
    assert!(capacity > 0);
    assert!(driver.insert(high, first, bounds(), NativeSurfaceRole::Child));
    assert_eq!(driver.entries.as_ptr(), backing);
    assert_eq!(driver.entries.capacity(), capacity);
    assert!(driver.insert(low, second, bounds(), NativeSurfaceRole::Child));
    assert_eq!(
        driver
            .specs()
            .map(DeferredViewportSpec::viewport)
            .collect::<Vec<_>>(),
        [low, high]
    );
    let owned = driver.owned_data_capacity_bytes();
    driver.remove(first);
    driver.remove(second);
    assert_eq!(driver.owned_data_capacity_bytes(), owned);
    assert!(driver.specs().next().is_none());
}
