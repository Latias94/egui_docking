use egui::emath::GuiRounding as _;
use egui::{Context, Order, Rect, ResizeDirection, ViewportClass, ViewportCommand, ViewportId};
use egui_tiles::Behavior;

use super::DockingMultiViewport;
use super::geometry::outer_position_for_window_move;
use super::title::title_for_detached_tree;
use super::types::DockPayload;
use super::types::{GhostDrag, GhostDragMode};

fn detached_root_tabs_redock_requested_id(
    bridge_id: egui::Id,
    viewport_id: ViewportId,
) -> egui::Id {
    egui::Id::new((
        bridge_id,
        viewport_id,
        "egui_docking_detached_root_tabs_redock_requested",
    ))
}

struct DetachedRootTabsCsdBehavior<'a, Pane> {
    inner: &'a mut dyn Behavior<Pane>,
    root_tabs: egui_tiles::TileId,
    bridge_id: egui::Id,
    viewport_id: ViewportId,
    enabled: bool,
}

impl<'a, Pane> DetachedRootTabsCsdBehavior<'a, Pane> {
    fn new(
        inner: &'a mut dyn Behavior<Pane>,
        root_tabs: egui_tiles::TileId,
        bridge_id: egui::Id,
        viewport_id: ViewportId,
        enabled: bool,
    ) -> Self {
        Self {
            inner,
            root_tabs,
            bridge_id,
            viewport_id,
            enabled,
        }
    }

    fn window_controls_ui(&mut self, ui: &mut egui::Ui) {
        let minimized = ui.ctx().input(|i| i.viewport().minimized.unwrap_or(false));
        let maximized = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));

        let button_size = ui.spacing().icon_width.max(12.0);
        let button_size = egui::Vec2::splat(button_size);
        let gap = 4.0;

        let button = |ui: &mut egui::Ui,
                      label: &'static str,
                      id_suffix: &'static str,
                      icon: CsdButtonIcon|
         -> egui::Response {
            let (id, rect) = ui.allocate_space(button_size);
            let id = id.with(id_suffix);
            let resp = ui.interact(rect, id, egui::Sense::click());
            resp.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
            });
            DockingMultiViewport::<Pane>::paint_csd_button_icon(ui, rect, &resp, icon);
            resp
        };

        // `top_bar_right_ui` is called under a right-to-left layout in egui_tiles, so the first
        // widget we add becomes the right-most one (Windows-like ordering).
        let close = button(ui, "Close window", "csd_close", CsdButtonIcon::Close);
        if close.clicked() {
            let id = detached_root_tabs_redock_requested_id(self.bridge_id, self.viewport_id);
            ui.ctx().data_mut(|d| d.insert_temp(id, true));
        }

        ui.add_space(gap);

        let max_icon = if maximized {
            CsdButtonIcon::Restore
        } else {
            CsdButtonIcon::Maximize
        };
        let max_label = if maximized {
            "Restore window"
        } else {
            "Maximize window"
        };
        let max = button(ui, max_label, "csd_maximize", max_icon);
        if max.clicked() {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }

        ui.add_space(gap);

        let min_label = if minimized {
            "Restore from minimized"
        } else {
            "Minimize window"
        };
        let min = button(ui, min_label, "csd_minimize", CsdButtonIcon::Minimize);
        if min.clicked() {
            ui.ctx()
                .send_viewport_cmd(ViewportCommand::Minimized(!minimized));
        }
    }
}

impl<'a, Pane> Behavior<Pane> for DetachedRootTabsCsdBehavior<'a, Pane> {
    fn pane_ui(
        &mut self,
        ui: &mut egui::Ui,
        tile_id: egui_tiles::TileId,
        pane: &mut Pane,
    ) -> egui_tiles::UiResponse {
        self.inner.pane_ui(ui, tile_id, pane)
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> egui::WidgetText {
        self.inner.tab_title_for_pane(pane)
    }

    fn tab_hover_cursor_icon(&self) -> egui::CursorIcon {
        self.inner.tab_hover_cursor_icon()
    }

    fn is_tab_closable(
        &self,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
    ) -> bool {
        self.inner.is_tab_closable(tiles, tile_id)
    }

    fn on_tab_close(
        &mut self,
        tiles: &mut egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
    ) -> bool {
        self.inner.on_tab_close(tiles, tile_id)
    }

    fn show_tab_close_button(&self, state: &egui_tiles::TabState, tab_hovered: bool) -> bool {
        self.inner.show_tab_close_button(state, tab_hovered)
    }

    fn close_tab_on_middle_click(&self) -> bool {
        self.inner.close_tab_on_middle_click()
    }

    fn tab_switch_on_drag_hover_delay(&self) -> f32 {
        self.inner.tab_switch_on_drag_hover_delay()
    }

    fn close_button_outer_size(&self) -> f32 {
        self.inner.close_button_outer_size()
    }

    fn close_button_inner_margin(&self) -> f32 {
        self.inner.close_button_inner_margin()
    }

    fn tab_title_for_tile(
        &mut self,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
    ) -> egui::WidgetText {
        self.inner.tab_title_for_tile(tiles, tile_id)
    }

    fn tab_ui(
        &mut self,
        tiles: &mut egui_tiles::Tiles<Pane>,
        ui: &mut egui::Ui,
        id: egui::Id,
        tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Response {
        self.inner.tab_ui(tiles, ui, id, tile_id, state)
    }

    fn drag_ui(
        &mut self,
        tiles: &egui_tiles::Tiles<Pane>,
        ui: &mut egui::Ui,
        tile_id: egui_tiles::TileId,
    ) {
        self.inner.drag_ui(tiles, ui, tile_id)
    }

    fn on_tab_button(
        &mut self,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
        button_response: egui::Response,
    ) -> egui::Response {
        self.inner.on_tab_button(tiles, tile_id, button_response)
    }

    fn retain_pane(&mut self, pane: &Pane) -> bool {
        self.inner.retain_pane(pane)
    }

    fn top_bar_right_ui(
        &mut self,
        tiles: &egui_tiles::Tiles<Pane>,
        ui: &mut egui::Ui,
        tile_id: egui_tiles::TileId,
        tabs: &egui_tiles::Tabs,
        scroll_offset: &mut f32,
    ) {
        if self.enabled && tile_id == self.root_tabs {
            self.window_controls_ui(ui);
            ui.add_space(6.0);
        }
        self.inner
            .top_bar_right_ui(tiles, ui, tile_id, tabs, scroll_offset);
    }

    fn tab_bar_height(&self, style: &egui::Style) -> f32 {
        self.inner.tab_bar_height(style)
    }

    fn gap_width(&self, style: &egui::Style) -> f32 {
        self.inner.gap_width(style)
    }

    fn min_size(&self) -> f32 {
        self.inner.min_size()
    }

    fn preview_dragged_panes(&self) -> bool {
        self.inner.preview_dragged_panes()
    }

    fn dragged_overlay_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
        self.inner.dragged_overlay_color(visuals)
    }

    fn simplification_options(&self) -> egui_tiles::SimplificationOptions {
        self.inner.simplification_options()
    }

    fn auto_hide_tab_bar_when_single_tab(&self) -> bool {
        self.inner.auto_hide_tab_bar_when_single_tab()
    }

    fn paint_on_top_of_tile(
        &self,
        painter: &egui::Painter,
        style: &egui::Style,
        tile_id: egui_tiles::TileId,
        rect: Rect,
    ) {
        self.inner
            .paint_on_top_of_tile(painter, style, tile_id, rect)
    }

    fn resize_stroke(
        &self,
        style: &egui::Style,
        resize_state: egui_tiles::ResizeState,
    ) -> egui::Stroke {
        self.inner.resize_stroke(style, resize_state)
    }

    fn tab_title_spacing(&self, visuals: &egui::Visuals) -> f32 {
        self.inner.tab_title_spacing(visuals)
    }

    fn tab_bar_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
        self.inner.tab_bar_color(visuals)
    }

    fn tab_bg_color(
        &self,
        visuals: &egui::Visuals,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Color32 {
        self.inner.tab_bg_color(visuals, tiles, tile_id, state)
    }

    fn tab_outline_stroke(
        &self,
        visuals: &egui::Visuals,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Stroke {
        self.inner
            .tab_outline_stroke(visuals, tiles, tile_id, state)
    }

    fn tab_bar_hline_stroke(&self, visuals: &egui::Visuals) -> egui::Stroke {
        self.inner.tab_bar_hline_stroke(visuals)
    }

    fn tab_text_color(
        &self,
        visuals: &egui::Visuals,
        tiles: &egui_tiles::Tiles<Pane>,
        tile_id: egui_tiles::TileId,
        state: &egui_tiles::TabState,
    ) -> egui::Color32 {
        self.inner.tab_text_color(visuals, tiles, tile_id, state)
    }

    fn drag_preview_stroke(&self, visuals: &egui::Visuals) -> egui::Stroke {
        self.inner.drag_preview_stroke(visuals)
    }

    fn drag_preview_color(&self, visuals: &egui::Visuals) -> egui::Color32 {
        self.inner.drag_preview_color(visuals)
    }

    fn paint_drag_preview(
        &self,
        visuals: &egui::Visuals,
        painter: &egui::Painter,
        parent_rect: Option<Rect>,
        preview_rect: Rect,
    ) {
        self.inner
            .paint_drag_preview(visuals, painter, parent_rect, preview_rect)
    }

    fn grid_auto_column_count(&self, num_visible_children: usize, rect: Rect, gap: f32) -> usize {
        self.inner
            .grid_auto_column_count(num_visible_children, rect, gap)
    }

    fn ideal_tile_aspect_ratio(&self) -> f32 {
        self.inner.ideal_tile_aspect_ratio()
    }

    fn on_edit(&mut self, edit_action: egui_tiles::EditAction) {
        self.inner.on_edit(edit_action)
    }
}

impl<Pane> DockingMultiViewport<Pane> {
    fn csd_controls_side() -> CsdControlsSide {
        if cfg!(target_os = "macos") {
            CsdControlsSide::Left
        } else {
            CsdControlsSide::Right
        }
    }

    fn detached_root_is_tabs(detached: &super::types::DetachedDock<Pane>) -> bool {
        detached
            .tree
            .root
            .and_then(|root| detached.tree.tiles.get(root))
            .is_some_and(|tile| {
                matches!(
                    tile,
                    egui_tiles::Tile::Container(container)
                        if container.kind() == egui_tiles::ContainerKind::Tabs
                )
            })
    }

    fn start_detached_window_move(&mut self, ctx: &Context, viewport_id: ViewportId) {
        let bridge_id = self.tree.id();
        let move_active_id = self.detached_window_move_active_id(viewport_id);

        let already_active = ctx
            .data(|d| d.get_temp::<bool>(move_active_id))
            .unwrap_or(false);
        if already_active {
            return;
        }

        self.observe_tiles_drag_detached();
        ctx.data_mut(|d| d.insert_temp(move_active_id, true));

        if self.options.focus_detached_on_custom_title_drag {
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        // Prefer OS/native window dragging for detached viewport moves.
        //
        // This avoids the self-referential coordinate feedback that happens when we drive
        // `OuterPosition` from window-local pointer deltas (and it works even on platforms where
        // raw device motion isn't available).
        ctx.send_viewport_cmd(ViewportCommand::StartDrag);

        egui::DragAndDrop::set_payload(
            ctx,
            DockPayload {
                bridge_id,
                source_viewport: viewport_id,
                source_floating: None,
                tile_id: None,
            },
        );

        // Transfer authority away from egui_tiles; we are now in "window move" mode.
        ctx.stop_dragging();
        ctx.request_repaint_of(ViewportId::ROOT);

        if self.options.debug_event_log {
            self.debug_log_event(format!(
                "detached_window_move START viewport={viewport_id:?}"
            ));
        }
    }

    fn paint_csd_button_icon(
        ui: &egui::Ui,
        rect: Rect,
        response: &egui::Response,
        icon: CsdButtonIcon,
    ) {
        let visuals = ui.style().interact(response);
        let stroke = visuals.fg_stroke;
        let rect = rect.shrink(2.0).expand(visuals.expansion);

        match icon {
            CsdButtonIcon::Close => {
                ui.painter()
                    .line_segment([rect.left_top(), rect.right_bottom()], stroke);
                ui.painter()
                    .line_segment([rect.right_top(), rect.left_bottom()], stroke);
            }
            CsdButtonIcon::Minimize => {
                ui.painter().hline(rect.x_range(), rect.center().y, stroke);
            }
            CsdButtonIcon::Maximize => {
                ui.painter()
                    .rect_stroke(rect.shrink(1.0), 0.0, stroke, egui::StrokeKind::Inside);
            }
            CsdButtonIcon::Restore => {
                let a = rect.shrink(2.0);
                let b = a.translate(egui::vec2(-3.0, 3.0));
                ui.painter()
                    .rect_stroke(a, 0.0, stroke, egui::StrokeKind::Inside);
                ui.painter()
                    .rect_stroke(b, 0.0, stroke, egui::StrokeKind::Inside);
            }
        }
    }

    fn csd_window_controls_ui(
        &mut self,
        ctx: &Context,
        viewport_id: ViewportId,
        bar_rect: Rect,
        should_redock_to_root: &mut bool,
    ) {
        if !self.options.detached_csd_window_controls {
            return;
        }

        let bar_rect = bar_rect.round_to_pixels(ctx.pixels_per_point()).round_ui();
        let controls_rect = self.csd_window_controls_rect(ctx, bar_rect);
        let gap = 4.0;
        let padding_x = 6.0;

        let area_id = egui::Id::new((self.tree.id(), viewport_id, "csd_controls"));
        egui::Area::new(area_id)
            .order(Order::Foreground)
            .fixed_pos(controls_rect.min)
            .interactable(true)
            .show(ctx, |ui| {
                ui.set_clip_rect(controls_rect);
                let mut ui = ui.new_child(egui::UiBuilder::new().max_rect(controls_rect));

                let side = Self::csd_controls_side();
                let controls_height = controls_rect.height();
                let button_size = ctx
                    .global_style()
                    .spacing
                    .icon_width
                    .min(controls_height)
                    .max(12.0);
                let button_size = egui::Vec2::splat(button_size);
                let layout = match side {
                    CsdControlsSide::Left => egui::Layout::left_to_right(egui::Align::Center),
                    CsdControlsSide::Right => egui::Layout::right_to_left(egui::Align::Center),
                };

                ui.with_layout(layout, |ui| {
                    ui.add_space(padding_x);

                    let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));

                    match side {
                        CsdControlsSide::Left => {
                            // close
                            let close_id = ui.id().with("close");
                            let (_, close_rect) = ui.allocate_space(button_size);
                            let close_resp =
                                ui.interact(close_rect, close_id, egui::Sense::click());
                            close_resp.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    "Close window (re-dock to root)",
                                )
                            });
                            Self::paint_csd_button_icon(
                                ui,
                                close_rect,
                                &close_resp,
                                CsdButtonIcon::Close,
                            );
                            if close_resp.clicked() {
                                *should_redock_to_root = true;
                            }
                            ui.add_space(gap);

                            // minimize
                            let min_id = ui.id().with("minimize");
                            let (_, min_rect) = ui.allocate_space(button_size);
                            let min_resp = ui.interact(min_rect, min_id, egui::Sense::click());
                            min_resp.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    if minimized {
                                        "Restore from minimized"
                                    } else {
                                        "Minimize window"
                                    },
                                )
                            });
                            Self::paint_csd_button_icon(
                                ui,
                                min_rect,
                                &min_resp,
                                CsdButtonIcon::Minimize,
                            );
                            if min_resp.clicked() {
                                ctx.send_viewport_cmd(ViewportCommand::Minimized(!minimized));
                            }
                            ui.add_space(gap);

                            // maximize
                            let max_id = ui.id().with("maximize");
                            let (_, max_rect) = ui.allocate_space(button_size);
                            let max_resp = ui.interact(max_rect, max_id, egui::Sense::click());
                            max_resp.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    if maximized {
                                        "Restore window"
                                    } else {
                                        "Maximize window"
                                    },
                                )
                            });
                            Self::paint_csd_button_icon(
                                ui,
                                max_rect,
                                &max_resp,
                                if maximized {
                                    CsdButtonIcon::Restore
                                } else {
                                    CsdButtonIcon::Maximize
                                },
                            );
                            if max_resp.clicked() {
                                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                            }
                        }
                        CsdControlsSide::Right => {
                            // close
                            let close_id = ui.id().with("close");
                            let (_, close_rect) = ui.allocate_space(button_size);
                            let close_resp =
                                ui.interact(close_rect, close_id, egui::Sense::click());
                            close_resp.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    "Close window (re-dock to root)",
                                )
                            });
                            Self::paint_csd_button_icon(
                                ui,
                                close_rect,
                                &close_resp,
                                CsdButtonIcon::Close,
                            );
                            if close_resp.clicked() {
                                *should_redock_to_root = true;
                            }
                            ui.add_space(gap);

                            // maximize
                            let max_id = ui.id().with("maximize");
                            let (_, max_rect) = ui.allocate_space(button_size);
                            let max_resp = ui.interact(max_rect, max_id, egui::Sense::click());
                            max_resp.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    if maximized {
                                        "Restore window"
                                    } else {
                                        "Maximize window"
                                    },
                                )
                            });
                            Self::paint_csd_button_icon(
                                ui,
                                max_rect,
                                &max_resp,
                                if maximized {
                                    CsdButtonIcon::Restore
                                } else {
                                    CsdButtonIcon::Maximize
                                },
                            );
                            if max_resp.clicked() {
                                ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                            }
                            ui.add_space(gap);

                            // minimize
                            let min_id = ui.id().with("minimize");
                            let (_, min_rect) = ui.allocate_space(button_size);
                            let min_resp = ui.interact(min_rect, min_id, egui::Sense::click());
                            min_resp.widget_info(|| {
                                egui::WidgetInfo::labeled(
                                    egui::WidgetType::Button,
                                    ui.is_enabled(),
                                    if minimized {
                                        "Restore from minimized"
                                    } else {
                                        "Minimize window"
                                    },
                                )
                            });
                            Self::paint_csd_button_icon(
                                ui,
                                min_rect,
                                &min_resp,
                                CsdButtonIcon::Minimize,
                            );
                            if min_resp.clicked() {
                                ctx.send_viewport_cmd(ViewportCommand::Minimized(!minimized));
                            }
                        }
                    }

                    ui.add_space(padding_x);
                });
            });
    }

    fn csd_window_controls_rect(&self, ctx: &Context, bar_rect: Rect) -> Rect {
        let controls_height = bar_rect.height();
        let button_size = ctx
            .global_style()
            .spacing
            .icon_width
            .min(controls_height)
            .max(12.0);
        let button_size = egui::Vec2::splat(button_size);
        let gap = 4.0;
        let padding_x = 6.0;

        let total_w = padding_x * 2.0 + button_size.x * 3.0 + gap * 2.0;
        match Self::csd_controls_side() {
            CsdControlsSide::Left => Rect::from_min_max(
                bar_rect.min,
                egui::pos2(
                    (bar_rect.min.x + total_w).min(bar_rect.max.x),
                    bar_rect.max.y,
                ),
            ),
            CsdControlsSide::Right => Rect::from_min_max(
                egui::pos2(
                    (bar_rect.max.x - total_w).max(bar_rect.min.x),
                    bar_rect.min.y,
                ),
                bar_rect.max,
            ),
        }
    }

    fn ui_borderless_detached_chrome(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
        viewport_id: ViewportId,
        title: &str,
        should_redock_to_root: &mut bool,
    ) {
        let style = ctx.global_style();
        let bar_height = behavior.tab_bar_height(&style).max(24.0);
        let bar_id = egui::Id::new((self.tree.id(), viewport_id, "borderless_title_bar"));

        egui::Panel::top(bar_id)
            .exact_size(bar_height)
            .frame(egui::Frame::new().fill(style.visuals.window_fill()))
            .show(ctx, |ui| {
                let rect = ui.max_rect();

                let padding_x = 8.0;

                // Choose a drag rect that never overlaps window buttons.
                let mut drag_rect = rect.shrink2(egui::vec2(padding_x, 0.0));
                if self.options.detached_csd_window_controls {
                    let controls_rect = self.csd_window_controls_rect(ctx, rect);
                    if controls_rect.center().x <= rect.center().x {
                        drag_rect.min.x = (controls_rect.max.x + padding_x).min(rect.max.x);
                    } else {
                        drag_rect.max.x = (controls_rect.min.x - padding_x).max(rect.min.x);
                    }
                } else {
                    let button_rects =
                        egui::containers::window_chrome::title_bar_button_rects(ui, rect);
                    let close_rect = button_rects.close;
                    if close_rect.center().x <= rect.center().x {
                        drag_rect.min.x = (close_rect.max.x + padding_x).min(rect.max.x);
                    } else {
                        drag_rect.max.x = (close_rect.min.x - padding_x).max(rect.min.x);
                    }
                }
                drag_rect = drag_rect.intersect(rect);

                let drag_id = ui.id().with("drag");
                let drag = ui
                    .interact(drag_rect, drag_id, egui::Sense::click_and_drag())
                    .on_hover_cursor(egui::CursorIcon::Grab);
                if drag.drag_started() {
                    self.start_detached_window_move(ctx, viewport_id);
                }
                if drag.double_clicked() {
                    let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                    ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                    if self.options.debug_event_log {
                        self.debug_log_event(format!(
                            "csd_titlebar DOUBLE_CLICK maximize_toggle viewport={viewport_id:?} -> {}",
                            !maximized
                        ));
                    }
                }

                if self.options.detached_csd_window_controls {
                    self.csd_window_controls_ui(ctx, viewport_id, rect, should_redock_to_root);
                } else {
                    let button_rects =
                        egui::containers::window_chrome::title_bar_button_rects(ui, rect);
                    let close = egui::containers::window_chrome::window_close_button(
                        ui,
                        button_rects.close,
                    );
                    if close.clicked() {
                        *should_redock_to_root = true;
                    }
                }

                ui.scope_builder(egui::UiBuilder::new().max_rect(drag_rect), |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        ui.label(egui::RichText::new(title).strong());
                    });
                })
                .response
            });
    }

    fn ui_borderless_resize_handles(&self, ctx: &Context, viewport_id: ViewportId) {
        let maximized_or_fullscreen = ctx.input(|i| {
            i.viewport().maximized.unwrap_or(false) || i.viewport().fullscreen.unwrap_or(false)
        });
        if maximized_or_fullscreen {
            return;
        }

        let viewport_rect = ctx.viewport_rect();
        if !viewport_rect.is_positive() {
            return;
        }

        let thickness = self
            .options
            .detached_csd_resize_edge_thickness
            .max(1.0)
            .min(viewport_rect.width().min(viewport_rect.height()) * 0.5);
        let corner = self
            .options
            .detached_csd_resize_corner_size
            .max(thickness)
            .min(viewport_rect.width().min(viewport_rect.height()) * 0.5);

        let active_id = egui::Id::new((self.tree.id(), viewport_id, "borderless_resize_active"));
        if !ctx.input(|i| i.pointer.primary_down()) {
            ctx.data_mut(|d| d.remove::<u8>(active_id));
        }

        fn dir_key(dir: ResizeDirection) -> u8 {
            match dir {
                ResizeDirection::North => 0,
                ResizeDirection::South => 1,
                ResizeDirection::East => 2,
                ResizeDirection::West => 3,
                ResizeDirection::NorthEast => 4,
                ResizeDirection::SouthEast => 5,
                ResizeDirection::NorthWest => 6,
                ResizeDirection::SouthWest => 7,
            }
        }

        let layer_id = egui::Id::new((self.tree.id(), viewport_id, "borderless_resize_area"));
        egui::Area::new(layer_id)
            .order(Order::Foreground)
            .fixed_pos(viewport_rect.min)
            .show(ctx, |ui| {
                ui.set_clip_rect(viewport_rect);
                let ui = ui.new_child(egui::UiBuilder::new().max_rect(viewport_rect));

                let make = |rect: Rect, dir: ResizeDirection, cursor: egui::CursorIcon| {
                    let id = egui::Id::new((
                        self.tree.id(),
                        viewport_id,
                        "borderless_resize",
                        dir_key(dir),
                    ));
                    let resp = ui
                        .interact(rect, id, egui::Sense::click_and_drag())
                        .on_hover_cursor(cursor);
                    let already_active = ctx.data(|d| d.get_temp::<u8>(active_id)).is_some();
                    if !already_active
                        && resp.is_pointer_button_down_on()
                        && ctx.input(|i| i.pointer.primary_down())
                    {
                        ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
                        ctx.data_mut(|d| d.insert_temp(active_id, dir_key(dir)));
                    }
                };

                let r = viewport_rect;
                let left = Rect::from_min_max(r.min, egui::pos2(r.min.x + thickness, r.max.y));
                let right = Rect::from_min_max(egui::pos2(r.max.x - thickness, r.min.y), r.max);
                let top = Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + thickness));
                let bottom = Rect::from_min_max(egui::pos2(r.min.x, r.max.y - thickness), r.max);

                let nw = Rect::from_min_max(r.min, r.min + egui::vec2(corner, corner));
                let ne = Rect::from_min_max(
                    egui::pos2(r.max.x - corner, r.min.y),
                    egui::pos2(r.max.x, r.min.y + corner),
                );
                let sw = Rect::from_min_max(
                    egui::pos2(r.min.x, r.max.y - corner),
                    egui::pos2(r.min.x + corner, r.max.y),
                );
                let se = Rect::from_min_max(r.max - egui::vec2(corner, corner), r.max);

                make(
                    left,
                    ResizeDirection::West,
                    egui::CursorIcon::ResizeHorizontal,
                );
                make(
                    right,
                    ResizeDirection::East,
                    egui::CursorIcon::ResizeHorizontal,
                );
                make(
                    top,
                    ResizeDirection::North,
                    egui::CursorIcon::ResizeVertical,
                );
                make(
                    bottom,
                    ResizeDirection::South,
                    egui::CursorIcon::ResizeVertical,
                );

                // Corners must win over edges.
                make(nw, ResizeDirection::NorthWest, egui::CursorIcon::ResizeNwSe);
                make(ne, ResizeDirection::NorthEast, egui::CursorIcon::ResizeNeSw);
                make(sw, ResizeDirection::SouthWest, egui::CursorIcon::ResizeNeSw);
                make(se, ResizeDirection::SouthEast, egui::CursorIcon::ResizeNwSe);
            });
    }

    pub(super) fn ui_detached_viewports(
        &mut self,
        ctx: &Context,
        behavior: &mut dyn Behavior<Pane>,
    ) {
        let viewport_ids: Vec<ViewportId> = self.detached.keys().copied().collect();
        let bridge_id = self.tree.id();

        for viewport_id in viewport_ids {
            if self
                .detached_rendered_frame
                .get(&viewport_id)
                .is_some_and(|&f| f == self.debug_frame)
            {
                continue;
            }
            self.detached_rendered_frame
                .insert(viewport_id, self.debug_frame);

            let Some(mut detached) = self.detached.remove(&viewport_id) else {
                continue;
            };

            let root_is_tabs = Self::detached_root_is_tabs(&detached);

            let builder = detached
                .builder
                .clone()
                .with_decorations(self.options.detached_viewport_decorations);
            let mut should_redock_to_root = false;

            ctx.show_viewport_immediate(viewport_id, builder, |ctx, class| {
                self.update_last_pointer_global_from_active_viewport(ctx);
                self.update_viewport_outer_from_inner_offset(ctx);
                #[cfg(feature = "persistence")]
                self.capture_viewport_runtime(ctx);
                self.observe_drag_sources_in_ctx(ctx);

                if let Some(GhostDrag {
                    mode: GhostDragMode::Native { viewport },
                    grab_offset,
                }) = self.ghost
                {
                    if viewport == viewport_id {
                        if let Some(pointer_global) = self.drag_state.last_pointer_global() {
                            let outer_from_inner =
                                self.viewport_outer_from_inner_offset(viewport_id);
                            let desired_outer = outer_position_for_window_move(
                                pointer_global,
                                grab_offset,
                                outer_from_inner,
                            )
                            .round_to_pixels(ctx.pixels_per_point())
                            .round_ui();

                            let last_id = egui::Id::new((
                                bridge_id,
                                viewport_id,
                                "egui_docking_ghost_native_last_sent_outer",
                            ));
                            let prev = ctx.data(|d| d.get_temp::<egui::Pos2>(last_id));
                            let should_send = match prev {
                                Some(prev) => desired_outer != prev,
                                None => true,
                            };
                            if should_send {
                                ctx.send_viewport_cmd(egui::ViewportCommand::OuterPosition(
                                    desired_outer,
                                ));
                                ctx.data_mut(|d| d.insert_temp(last_id, desired_outer));

                                if self.options.debug_event_log {
                                    let step_id = egui::Id::new((
                                        bridge_id,
                                        viewport_id,
                                        "egui_docking_ghost_native_move_step_count",
                                    ));
                                    let step = ctx
                                        .data(|d| d.get_temp::<u64>(step_id).unwrap_or(0))
                                        .saturating_add(1);
                                    ctx.data_mut(|d| d.insert_temp(step_id, step));

                                    let log_every_send = self.options.debug_log_window_move_every_send
                                        && self.options.debug_log_file_path.is_some();
                                    if log_every_send || step == 1 || step % 20 == 0 {
                                        self.debug_log_event(format!(
                                            "ghost_window_move step={step} viewport={:?} pointer_global=({:.1},{:.1}) grab_in_inner=({:.1},{:.1}) outer_from_inner=({:.1},{:.1}) desired_outer=({:.1},{:.1})",
                                            viewport_id,
                                            pointer_global.x,
                                            pointer_global.y,
                                            grab_offset.x,
                                            grab_offset.y,
                                            outer_from_inner.x,
                                            outer_from_inner.y,
                                            desired_outer.x,
                                            desired_outer.y
                                        ));
                                    }
                                }
                            }
                        }
                    }
                }

                let title = title_for_detached_tree(&detached.tree, behavior);

                match class {
                    ViewportClass::Immediate | ViewportClass::Deferred | ViewportClass::Root => {
                        // For a child viewport created with `show_viewport_immediate` we expect
                        // `Immediate` (or `Embedded` below).
                    }
                    ViewportClass::Embedded => {
                        egui::Window::new(title.clone())
                            .default_size(
                                detached
                                    .builder
                                    .inner_size
                                    .unwrap_or(self.options.default_detached_inner_size),
                            )
                            .show(ctx, |ui| detached.tree.ui(behavior, ui));
                        return;
                    }
                }

                if !self.options.detached_viewport_decorations {
                    // Important: render borderless resize handles early so later chrome/controls
                    // (tab-bar controls, overlays) can take pointer priority.
                    self.ui_borderless_resize_handles(ctx, viewport_id);
                }

                // For borderless detached windows:
                // - If root is a Tabs container, the tab bar is the only "chrome" (ImGui-like).
                // - Otherwise, render a small custom title bar above the dock surface.
                let use_borderless_chrome = !self.options.detached_viewport_decorations && !root_is_tabs;
                if use_borderless_chrome {
                    self.ui_borderless_detached_chrome(
                        ctx,
                        behavior,
                        viewport_id,
                        &title,
                        &mut should_redock_to_root,
                    );
                }

                egui::CentralPanel::default().show(ctx, |ui| {
                    let dock_rect = ui.available_rect_before_wrap();
                    self.last_dock_rects.insert(viewport_id, dock_rect);
                    // Hit-testing and drop preview must not depend on draw order. Rebuild the floating
                    // rect cache before any release/overlay logic runs for this viewport.
                    self.rebuild_floating_rect_cache_for_viewport(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                    );

                    // ImGui-like: dragging the tab-bar background of a detached window should move the
                    // native viewport itself (window-move docking), without adding a second custom title bar.
                    //
                    // We only enable this when the detached tree is hosted as a single Tabs root node.
                    // More complex split layouts should be redocked by dragging individual tabs/panes.
                    let move_active_id = self.detached_window_move_active_id(viewport_id);

                    let (root_tabs_tile, root_tabs_single_child, root_tabs_visible_children_all_panes) = detached
                        .tree
                        .root
                        .and_then(|root| detached.tree.tiles.get(root).map(|t| (root, t)))
                        .and_then(|(root, tile)| match tile {
                            egui_tiles::Tile::Container(container)
                                if container.kind() == egui_tiles::ContainerKind::Tabs =>
                            {
                                let children: Vec<egui_tiles::TileId> =
                                    container.children().copied().collect();
                                let single_child = (children.len() == 1).then_some(children[0]);
                                let visible_children_all_panes = children
                                    .iter()
                                    .copied()
                                    .filter(|&child| {
                                        detached.tree.tiles.get(child).is_some()
                                            && detached.tree.tiles.is_visible(child)
                                    })
                                    .all(|child| {
                                        matches!(detached.tree.tiles.get(child), Some(egui_tiles::Tile::Pane(_)))
                                    });
                                Some((Some(root), single_child, visible_children_all_panes))
                            }
                            _ => Some((None, None, false)),
                        })
                        .unwrap_or((None, None, false));
                    let root_is_tabs = root_tabs_tile.is_some();

                    if !self.options.detached_viewport_decorations
                        && root_is_tabs
                        && self.options.detached_csd_window_controls
                    {
                        // Window controls are injected into the root Tabs tab bar via a Behavior wrapper
                        // (so tab scrolling/layout accounts for the reserved width).
                    }

                    let mut window_move_active =
                        ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);

                    // If egui_tiles is already dragging the root Tabs tile (tab-bar background drag),
                    // transfer authority to the viewport host and use OS/native window dragging.
                    if !window_move_active
                        && root_tabs_tile.is_some()
                        && root_tabs_visible_children_all_panes
                        && detached.tree.dragged_id_including_root(ctx) == root_tabs_tile
                    {
                        self.start_detached_window_move(ctx, viewport_id);
                        window_move_active = true;
                    }

                    if window_move_active {
                        // Keep both the moving window and the root preview alive while dragging.
                        //
                        // We intentionally don't end the session here: the release may be queued/handled
                        // by a different viewport (cross-viewport drop), and some backends may swallow
                        // the mouse-up event during native window moves. A late cleanup pass in
                        // `DockingMultiViewport::ui` clears stale per-viewport flags.
                        ctx.request_repaint();
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }

                    let took_over_internal_drop = self.process_release_before_detached_tree_ui(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &detached.tree,
                        "internal_overlay_drop_detached",
                    );

                    self.set_tiles_disable_drop_apply_if_taken_over(
                        ctx,
                        detached.tree.id(),
                        viewport_id,
                        took_over_internal_drop,
                    );
                    self.set_tiles_disable_drop_preview_if_overlay_hovered(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &detached.tree,
                    );

                    if let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx) {
                        self.observe_tiles_drag_detached();
                        self.queue_pending_local_drop_from_dragged_tile_on_release(
                            ctx,
                            dock_rect,
                            viewport_id,
                            None,
                            dragged_tile,
                        );
                    }

                    let mut did_tear_off = false;
                    self.try_tear_off_from_detached(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &mut detached.tree,
                        &mut did_tear_off,
                    );

                    self.maybe_start_ghost_from_tree_in_viewport(
                        ctx,
                        behavior,
                        dock_rect,
                        viewport_id,
                        &mut detached.tree,
                    );

                    self.set_tiles_debug_visit_enabled(ctx, detached.tree.id(), viewport_id);
                    let enable_root_tabs_controls = !self.options.detached_viewport_decorations
                        && root_is_tabs
                        && self.options.detached_csd_window_controls
                        && detached.tree.root.is_some();
                    if enable_root_tabs_controls {
                        let root_tabs = detached.tree.root.expect("checked above");
                        let mut wrapped = DetachedRootTabsCsdBehavior::new(
                            behavior,
                            root_tabs,
                            bridge_id,
                            viewport_id,
                            self.options.detached_csd_window_controls,
                        );
                        detached.tree.ui(&mut wrapped, ui);
                    } else {
                        detached.tree.ui(behavior, ui);
                    }

                    if ctx.data(|d| {
                        d.get_temp::<bool>(detached_root_tabs_redock_requested_id(
                            bridge_id,
                            viewport_id,
                        ))
                        .unwrap_or(false)
                    }) {
                        ctx.data_mut(|d| {
                            d.insert_temp(
                                detached_root_tabs_redock_requested_id(bridge_id, viewport_id),
                                false,
                            );
                        });
                        should_redock_to_root = true;
                    }

                    // ImGui-like: double-click tab-bar background toggles maximize.
                    if !self.options.detached_viewport_decorations
                        && root_is_tabs
                        && ctx.dragged_id().is_none()
                    {
                        if let Some(root_tabs) = root_tabs_tile
                            && egui_tiles::take_tab_bar_background_double_clicked(
                                ctx,
                                detached.tree.id(),
                                root_tabs,
                            )
                        {
                            let maximized =
                                ctx.input(|i| i.viewport().maximized.unwrap_or(false));
                            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
                        }
                    }

                    // If the detached viewport is a single-tab host, treat dragging that lone tab
                    // as a window-move drag (ImGui-style), so the native viewport follows the cursor
                    // while docking back.
                    let window_move_active =
                        ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);
                    let dragged_tile_after_ui = detached.tree.dragged_id_including_root(ctx);
                    let should_upgrade_single_tab_to_window_move = !window_move_active
                        && root_tabs_single_child.is_some_and(|child| Some(child) == dragged_tile_after_ui);
                    let should_start_window_move_late = !window_move_active
                        && root_tabs_tile.is_some()
                        && root_tabs_visible_children_all_panes
                        && dragged_tile_after_ui == root_tabs_tile;
                    if should_upgrade_single_tab_to_window_move || should_start_window_move_late {
                        self.start_detached_window_move(ctx, viewport_id);
                    }

                    if self.pending_drop.is_none()
                        && self.pending_local_drop.is_none()
                        && self.ghost.is_none()
                    {
                        // If we just upgraded to window-move mode above, do not re-install a tile payload.
                        let window_move_active =
                            ctx.data(|d| d.get_temp::<bool>(move_active_id)).unwrap_or(false);
                        if !window_move_active
                            && let Some(dragged_tile) = detached.tree.dragged_id_including_root(ctx)
                        {
                            egui::DragAndDrop::set_payload(
                                ctx,
                                super::types::DockPayload {
                                    bridge_id,
                                    source_viewport: viewport_id,
                                    source_floating: None,
                                    tile_id: Some(dragged_tile),
                                },
                            );
                            ctx.request_repaint_of(ViewportId::ROOT);
                        }
                    }

                    self.paint_drop_preview_if_any_for_tree(
                        ui,
                        behavior,
                        &detached.tree,
                        dock_rect,
                        viewport_id,
                    );

                    self.ui_floating_windows_in_viewport(ui, behavior, dock_rect, viewport_id);

                    self.process_release_after_floating_ui(ctx, dock_rect, viewport_id);

                    if self.options.debug_drop_targets
                        || self.options.debug_event_log
                        || self.options.debug_integrity
                    {
                        self.ui_debug_window(ctx, viewport_id, detached.tree.id());
                    }
                    if did_tear_off {
                        ctx.request_repaint_of(ViewportId::ROOT);
                    }
                });

                if ctx.input(|i| i.viewport().close_requested()) {
                    // Safe default: closing the native window re-docks it to the root.
                    should_redock_to_root = true;
                }
            });

            if should_redock_to_root {
                self.dock_tree_into_root(detached.tree, None);
                continue;
            }

            if detached.tree.root.is_none() {
                // Empty: stop calling `show_viewport_immediate` so the native viewport closes.
                if self.options.debug_event_log {
                    self.debug_log_event(format!(
                        "detached_viewport EMPTY -> close viewport={viewport_id:?}"
                    ));
                }
                continue;
            }

            // Keep detached.
            detached.builder = detached
                .builder
                .clone()
                .with_title(title_for_detached_tree(&detached.tree, behavior))
                .with_decorations(self.options.detached_viewport_decorations);
            self.detached.insert(viewport_id, detached);
        }
    }

    pub(super) fn allocate_detached_viewport_id(&mut self) -> (ViewportId, u64) {
        let serial = self.next_viewport_serial;
        self.next_viewport_serial = self.next_viewport_serial.saturating_add(1);
        (
            ViewportId::from_hash_of(("egui_docking_detached", serial)),
            serial,
        )
    }
}

#[derive(Clone, Copy, Debug)]
enum CsdButtonIcon {
    Close,
    Minimize,
    Maximize,
    Restore,
}

#[derive(Clone, Copy, Debug)]
enum CsdControlsSide {
    Left,
    Right,
}
