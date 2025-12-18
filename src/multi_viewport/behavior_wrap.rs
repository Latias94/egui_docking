use egui::{Rect, Response, Stroke, Ui, Visuals, WidgetText};
use egui_tiles::{Behavior, SimplificationOptions, TabState, TileId, Tiles, UiResponse};

pub(super) struct PaneBackgroundBehavior<'a, Pane> {
    inner: &'a mut dyn Behavior<Pane>,
    enabled: bool,
}

impl<'a, Pane> PaneBackgroundBehavior<'a, Pane> {
    pub(super) fn new(inner: &'a mut dyn Behavior<Pane>, enabled: bool) -> Self {
        Self { inner, enabled }
    }
}

impl<'a, Pane> Behavior<Pane> for PaneBackgroundBehavior<'a, Pane> {
    fn pane_ui(&mut self, ui: &mut Ui, tile_id: TileId, pane: &mut Pane) -> UiResponse {
        if self.enabled {
            ui.painter()
                .rect_filled(ui.max_rect(), 0.0, ui.visuals().panel_fill);
        }
        self.inner.pane_ui(ui, tile_id, pane)
    }

    fn tab_title_for_pane(&mut self, pane: &Pane) -> WidgetText {
        self.inner.tab_title_for_pane(pane)
    }

    fn tab_hover_cursor_icon(&self) -> egui::CursorIcon {
        self.inner.tab_hover_cursor_icon()
    }

    fn is_tab_closable(&self, tiles: &Tiles<Pane>, tile_id: TileId) -> bool {
        self.inner.is_tab_closable(tiles, tile_id)
    }

    fn on_tab_close(&mut self, tiles: &mut Tiles<Pane>, tile_id: TileId) -> bool {
        self.inner.on_tab_close(tiles, tile_id)
    }

    fn show_tab_close_button(&self, state: &TabState, tab_hovered: bool) -> bool {
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

    fn tab_title_for_tile(&mut self, tiles: &Tiles<Pane>, tile_id: TileId) -> WidgetText {
        self.inner.tab_title_for_tile(tiles, tile_id)
    }

    fn tab_ui(
        &mut self,
        tiles: &mut Tiles<Pane>,
        ui: &mut Ui,
        id: egui::Id,
        tile_id: TileId,
        state: &TabState,
    ) -> Response {
        self.inner.tab_ui(tiles, ui, id, tile_id, state)
    }

    fn drag_ui(&mut self, tiles: &Tiles<Pane>, ui: &mut Ui, tile_id: TileId) {
        self.inner.drag_ui(tiles, ui, tile_id)
    }

    fn on_tab_button(
        &mut self,
        tiles: &Tiles<Pane>,
        tile_id: TileId,
        button_response: Response,
    ) -> Response {
        self.inner.on_tab_button(tiles, tile_id, button_response)
    }

    fn retain_pane(&mut self, pane: &Pane) -> bool {
        self.inner.retain_pane(pane)
    }

    fn top_bar_left_ui(
        &mut self,
        tiles: &Tiles<Pane>,
        ui: &mut Ui,
        tile_id: TileId,
        tabs: &egui_tiles::Tabs,
        scroll_offset: &mut f32,
    ) {
        self.inner
            .top_bar_left_ui(tiles, ui, tile_id, tabs, scroll_offset);
    }

    fn top_bar_right_ui(
        &mut self,
        tiles: &Tiles<Pane>,
        ui: &mut Ui,
        tile_id: TileId,
        tabs: &egui_tiles::Tabs,
        scroll_offset: &mut f32,
    ) {
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

    fn dragged_overlay_color(&self, visuals: &Visuals) -> egui::Color32 {
        self.inner.dragged_overlay_color(visuals)
    }

    fn simplification_options(&self) -> SimplificationOptions {
        self.inner.simplification_options()
    }

    fn auto_hide_tab_bar_when_single_tab(&self) -> bool {
        self.inner.auto_hide_tab_bar_when_single_tab()
    }

    fn paint_on_top_of_tile(&self, painter: &egui::Painter, style: &egui::Style, tile_id: TileId, rect: Rect) {
        self.inner.paint_on_top_of_tile(painter, style, tile_id, rect)
    }

    fn resize_stroke(&self, style: &egui::Style, resize_state: egui_tiles::ResizeState) -> Stroke {
        self.inner.resize_stroke(style, resize_state)
    }

    fn tab_title_spacing(&self, visuals: &Visuals) -> f32 {
        self.inner.tab_title_spacing(visuals)
    }

    fn tab_bar_color(&self, visuals: &Visuals) -> egui::Color32 {
        self.inner.tab_bar_color(visuals)
    }

    fn tab_bg_color(&self, visuals: &Visuals, tiles: &Tiles<Pane>, tile_id: TileId, state: &TabState) -> egui::Color32 {
        self.inner.tab_bg_color(visuals, tiles, tile_id, state)
    }

    fn tab_outline_stroke(&self, visuals: &Visuals, tiles: &Tiles<Pane>, tile_id: TileId, state: &TabState) -> Stroke {
        self.inner.tab_outline_stroke(visuals, tiles, tile_id, state)
    }

    fn tab_bar_hline_stroke(&self, visuals: &Visuals) -> Stroke {
        self.inner.tab_bar_hline_stroke(visuals)
    }

    fn tab_text_color(&self, visuals: &Visuals, tiles: &Tiles<Pane>, tile_id: TileId, state: &TabState) -> egui::Color32 {
        self.inner.tab_text_color(visuals, tiles, tile_id, state)
    }

    fn drag_preview_stroke(&self, visuals: &Visuals) -> Stroke {
        self.inner.drag_preview_stroke(visuals)
    }

    fn drag_preview_color(&self, visuals: &Visuals) -> egui::Color32 {
        self.inner.drag_preview_color(visuals)
    }

    fn paint_drag_preview(&self, visuals: &Visuals, painter: &egui::Painter, parent_rect: Option<Rect>, preview_rect: Rect) {
        self.inner
            .paint_drag_preview(visuals, painter, parent_rect, preview_rect)
    }

    fn grid_auto_column_count(&self, num_visible_children: usize, rect: Rect, gap: f32) -> usize {
        self.inner.grid_auto_column_count(num_visible_children, rect, gap)
    }

    fn ideal_tile_aspect_ratio(&self) -> f32 {
        self.inner.ideal_tile_aspect_ratio()
    }

    fn on_edit(&mut self, edit_action: egui_tiles::EditAction) {
        self.inner.on_edit(edit_action)
    }
}

