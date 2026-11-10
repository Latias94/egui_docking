//! Product menus retain their source coordinates inside a viewport-wide overlay.

use egui::containers::menu::{MenuConfig, menu_style};
use egui::emath::TSTransform;
use egui::{
    Area, AreaState, FocusDirection, Frame, Id, InnerResponse, Key, LayerId, Layout, Order, Popup,
    RectAlign, Response, ScrollArea, Sense, Ui, UiKind, UiStackInfo, ViewportId,
};

#[derive(Clone, Copy)]
struct MenuRun {
    popup: Id,
    toggled_in: Option<(ViewportId, u64)>,
    visible_pass: u64,
    press_started_while_open: bool,
}

fn run_id() -> Id {
    // egui owns one open popup per context; this slot retains no popup history.
    Id::unique("egui-dockspace-presentation-menu-run")
}

fn layer(instance: Id, surface: dockspace::model::SurfaceId) -> LayerId {
    LayerId::new(
        Order::Foreground,
        instance.with(("presentation-menu-layer", surface)),
    )
}

pub(crate) fn prepare_layer(ui: &Ui, instance: Id, surface: dockspace::model::SurfaceId) {
    let transform = ui
        .ctx()
        .layer_transform_to_global(ui.layer_id())
        .unwrap_or(TSTransform::IDENTITY);
    ui.ctx()
        .set_transform_layer(layer(instance, surface), transform);
}

pub(super) fn show(
    source: &Ui,
    instance: Id,
    surface: dockspace::model::SurfaceId,
    popup: Id,
    anchor: &Response,
    touch_requested: bool,
    content: impl FnOnce(&mut Ui) -> Vec<Id>,
) -> Option<InnerResponse<()>> {
    let context = source.ctx();
    let frame = (
        context.viewport_id(),
        context.cumulative_frame_nr_for(context.viewport_id()),
    );
    let pass = context.cumulative_pass_nr();
    let mut run = context
        .data(|data| data.get_temp::<MenuRun>(run_id()))
        .filter(|run| run.popup == popup)
        .unwrap_or(MenuRun {
            popup,
            toggled_in: None,
            visible_pass: 0,
            press_started_while_open: false,
        });
    if !touch_requested && anchor.clicked() && run.toggled_in != Some(frame) {
        Popup::toggle_id(context, popup);
        run.toggled_in = Some(frame);
    }
    if !Popup::is_id_open(context, popup) {
        context.data_mut(|data| data.insert_temp(run_id(), run));
        return None;
    }
    Popup::open_id(context, popup);

    let transform = context
        .layer_transform_to_global(source.layer_id())
        .unwrap_or(TSTransform::IDENTITY);
    let viewport = transform.inverse() * context.content_rect();
    if !viewport.is_finite() || !viewport.size().is_finite() || !viewport.is_positive() {
        Popup::close_id(context, popup);
        return None;
    }
    let menu_layer = layer(instance, surface);
    prepare_layer(source, instance, surface);
    let was_open = run.visible_pass.checked_add(1) == Some(pass);
    let (pressed, clicked) =
        source.input(|input| (input.pointer.any_pressed(), input.pointer.any_click()));
    if !was_open {
        run.press_started_while_open = false;
    } else if pressed {
        run.press_started_while_open = true;
    }
    let expected = AreaState::load(context, menu_layer.id)
        .and_then(|state| state.size)
        .unwrap_or_else(|| egui::vec2(source.spacing().menu_width, 0.0))
        .min(viewport.size());
    let align = RectAlign::find_best_align(
        std::iter::once(RectAlign::BOTTOM_START)
            .chain(RectAlign::BOTTOM_START.symmetries())
            .chain(RectAlign::MENU_ALIGNS.iter().copied()),
        viewport,
        anchor.interact_rect,
        0.0,
        expected,
    )
    .unwrap_or(RectAlign::BOTTOM_START);
    let (pivot, position) = align.pivot_pos(&anchor.interact_rect, 0.0);
    let mut output = Area::new(menu_layer.id)
        .order(menu_layer.order)
        .fixed_pos(position)
        .pivot(pivot)
        .default_width(source.spacing().menu_width)
        .constrain_to(viewport)
        .enabled(source.is_enabled())
        .movable(false)
        .fade_in(false)
        .sizing_pass(!was_open)
        .sense(Sense::click())
        .layout(Layout::top_down_justified(egui::Align::Min))
        .accessibility_parent(anchor.id)
        .info(
            UiStackInfo::new(UiKind::Menu)
                .with_tag_value(MenuConfig::MENU_CONFIG_TAG, MenuConfig::new()),
        )
        .show(context, |ui| {
            ui.set_style(std::sync::Arc::clone(source.style()));
            menu_style(ui.style_mut());
            let frame = Frame::menu(ui.style());
            let margins = frame.total_margin();
            let available = (viewport.size() - margins.sum()).max(egui::Vec2::ZERO);
            ui.set_max_width(ui.max_rect().width().min(available.x));
            let entries = frame
                .show(ui, |ui| {
                    ScrollArea::vertical()
                        .id_salt("presentation-menu-scroll")
                        .max_height(available.y)
                        .show(ui, content)
                        .inner
                })
                .inner;
            navigate(ui, anchor.id, &entries);
        });
    let escape = source.input(|input| input.key_pressed(Key::Escape));
    if output.response.should_close() || escape || (run.press_started_while_open && clicked) {
        Popup::close_id(context, popup);
        output.response.set_close();
        run.press_started_while_open = false;
    }
    run.visible_pass = pass;
    context.data_mut(|data| data.insert_temp(run_id(), run));
    Some(output)
}

fn navigate(ui: &Ui, anchor: Id, entries: &[Id]) {
    if ui.is_sizing_pass() {
        return;
    }
    let (focused, allowed) =
        ui.memory(|memory| (memory.focused(), memory.allows_interaction(ui.layer_id())));
    let index = entries.iter().position(|entry| Some(*entry) == focused);
    if !allowed || (index.is_none() && focused != Some(anchor) && focused.is_some()) {
        return;
    }
    let key = ui.input_mut(|input| {
        [Key::ArrowDown, Key::ArrowUp, Key::ArrowLeft]
            .into_iter()
            .find(|&key| input.consume_key(egui::Modifiers::NONE, key))
    });
    let next = match key {
        Some(Key::ArrowDown) => index
            .and_then(|index| entries.get(index + 1))
            .or_else(|| entries.first())
            .copied(),
        Some(Key::ArrowUp) => index
            .and_then(|index| index.checked_sub(1))
            .and_then(|index| entries.get(index))
            .or_else(|| entries.last())
            .copied(),
        Some(Key::ArrowLeft) => Some(anchor),
        // Framework Tab traversal remains free to leave this menu.
        _ => return,
    };
    ui.memory_mut(|memory| {
        memory.move_focus(FocusDirection::None);
        if let Some(next) = next {
            memory.request_focus(next);
        }
    });
    ui.request_repaint();
}
