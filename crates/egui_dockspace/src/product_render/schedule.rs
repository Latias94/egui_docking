//! Private root-oriented paint schedule built once per semantic output.

use std::collections::{BTreeMap, BTreeSet};

use dockspace::model::RootId;
use dockspace::runtime::{
    ContainedPaintRecord, PanePaintRecord, PresentationMenuAnchorPaintRecord,
    SplitterJunctionPaintRecord, SplitterPaintRecord, SurfacePaintPlan, TabBarPaintRecord,
    TabPaintRecord, TabStripControlPaintRecord,
};

#[derive(Default)]
pub(super) struct RootPaintSchedule<'plan> {
    panes: Vec<PanePaintRecord<'plan>>,
    tab_bars: Vec<TabBarPaintRecord<'plan>>,
    tabs: Vec<TabPaintRecord<'plan>>,
    splitters: Vec<SplitterPaintRecord<'plan>>,
    splitter_junctions: Vec<SplitterJunctionPaintRecord<'plan>>,
    tab_strip_controls: Vec<TabStripControlPaintRecord>,
    presentation_menu_anchor: Option<PresentationMenuAnchorPaintRecord<'plan>>,
}

impl<'plan> RootPaintSchedule<'plan> {
    pub(super) fn panes(&self) -> impl Iterator<Item = PanePaintRecord<'plan>> + '_ {
        self.panes.iter().copied()
    }

    pub(super) fn tab_bars(&self) -> impl Iterator<Item = TabBarPaintRecord<'plan>> + '_ {
        self.tab_bars.iter().copied()
    }

    pub(super) fn tabs(&self) -> impl Iterator<Item = TabPaintRecord<'plan>> + '_ {
        self.tabs.iter().copied()
    }

    pub(super) fn splitters(&self) -> impl Iterator<Item = SplitterPaintRecord<'plan>> + '_ {
        self.splitters.iter().copied()
    }

    pub(super) fn splitter_junctions(
        &self,
    ) -> impl Iterator<Item = SplitterJunctionPaintRecord<'plan>> + '_ {
        self.splitter_junctions.iter().copied()
    }

    pub(super) fn tab_strip_controls(
        &self,
    ) -> impl Iterator<Item = TabStripControlPaintRecord> + '_ {
        self.tab_strip_controls.iter().copied()
    }

    pub(super) const fn presentation_menu_anchor(
        &self,
    ) -> Option<PresentationMenuAnchorPaintRecord<'plan>> {
        self.presentation_menu_anchor
    }
}

pub(super) struct ContainedRootPaintSchedule<'plan> {
    contained: ContainedPaintRecord<'plan>,
    records: RootPaintSchedule<'plan>,
}

impl<'plan> ContainedRootPaintSchedule<'plan> {
    pub(super) const fn contained(&self) -> ContainedPaintRecord<'plan> {
        self.contained
    }

    pub(super) const fn records(&self) -> &RootPaintSchedule<'plan> {
        &self.records
    }
}

pub(super) struct SurfacePaintSchedule<'plan> {
    main_roots: Vec<RootPaintSchedule<'plan>>,
    contained_roots: Vec<ContainedRootPaintSchedule<'plan>>,
}

impl<'plan> SurfacePaintSchedule<'plan> {
    pub(super) fn from_plan(plan: SurfacePaintPlan<'plan>) -> Self {
        let mut roots = BTreeMap::<RootId, RootPaintSchedule<'plan>>::new();
        let mut main_roots = BTreeSet::new();

        for pane in plan.panes() {
            #[cfg(test)]
            structural_work::record_pane();
            main_roots.insert(pane.root());
            roots.entry(pane.root()).or_default().panes.push(pane);
        }
        for bar in plan.tab_bars() {
            #[cfg(test)]
            structural_work::record_tab_bar();
            roots.entry(bar.root()).or_default().tab_bars.push(bar);
        }
        for tab in plan.tabs() {
            #[cfg(test)]
            structural_work::record_tab();
            roots.entry(tab.root()).or_default().tabs.push(tab);
        }
        for splitter in plan.splitters() {
            #[cfg(test)]
            structural_work::record_splitter();
            roots
                .entry(splitter.root())
                .or_default()
                .splitters
                .push(splitter);
        }
        for junction in plan.splitter_junctions() {
            roots
                .entry(junction.root())
                .or_default()
                .splitter_junctions
                .push(junction);
        }
        for control in plan.tab_strip_controls() {
            roots
                .entry(control.root())
                .or_default()
                .tab_strip_controls
                .push(control);
        }
        for anchor in plan.presentation_menu_anchors() {
            roots
                .entry(anchor.root())
                .or_default()
                .presentation_menu_anchor = Some(anchor);
        }

        let mut contained = plan
            .contained()
            .inspect(|_| {
                #[cfg(test)]
                structural_work::record_contained();
            })
            .collect::<Vec<_>>();
        contained.sort_by_key(|record| record.ordinal());
        for record in &contained {
            main_roots.remove(&record.root());
        }

        let main_roots = main_roots
            .into_iter()
            .map(|root| roots.remove(&root).unwrap_or_default())
            .collect();
        let contained_roots = contained
            .into_iter()
            .map(|contained| ContainedRootPaintSchedule {
                records: roots.remove(&contained.root()).unwrap_or_default(),
                contained,
            })
            .collect();

        Self {
            main_roots,
            contained_roots,
        }
    }

    pub(super) fn main_roots(&self) -> impl Iterator<Item = &RootPaintSchedule<'plan>> {
        self.main_roots.iter()
    }

    pub(super) fn contained_roots(
        &self,
    ) -> impl Iterator<Item = &ContainedRootPaintSchedule<'plan>> {
        self.contained_roots.iter()
    }
}

#[cfg(test)]
mod structural_work {
    use std::cell::Cell;

    #[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
    pub(super) struct PaintRecordVisits {
        pub(super) panes: usize,
        pub(super) tab_bars: usize,
        pub(super) tabs: usize,
        pub(super) splitters: usize,
        pub(super) contained: usize,
    }

    #[derive(Default)]
    struct PaintRecordVisitCounters {
        panes: Cell<usize>,
        tab_bars: Cell<usize>,
        tabs: Cell<usize>,
        splitters: Cell<usize>,
        contained: Cell<usize>,
    }

    impl PaintRecordVisitCounters {
        fn reset(&self) {
            self.panes.set(0);
            self.tab_bars.set(0);
            self.tabs.set(0);
            self.splitters.set(0);
            self.contained.set(0);
        }

        fn snapshot(&self) -> PaintRecordVisits {
            PaintRecordVisits {
                panes: self.panes.get(),
                tab_bars: self.tab_bars.get(),
                tabs: self.tabs.get(),
                splitters: self.splitters.get(),
                contained: self.contained.get(),
            }
        }
    }

    thread_local! {
        static PAINT_RECORD_VISITS: PaintRecordVisitCounters =
            PaintRecordVisitCounters::default();
    }

    pub(super) fn reset() {
        PAINT_RECORD_VISITS.with(PaintRecordVisitCounters::reset);
    }

    pub(super) fn snapshot() -> PaintRecordVisits {
        PAINT_RECORD_VISITS.with(PaintRecordVisitCounters::snapshot)
    }

    pub(super) fn record_pane() {
        PAINT_RECORD_VISITS.with(|visits| visits.panes.set(visits.panes.get() + 1));
    }

    pub(super) fn record_tab_bar() {
        PAINT_RECORD_VISITS.with(|visits| visits.tab_bars.set(visits.tab_bars.get() + 1));
    }

    pub(super) fn record_tab() {
        PAINT_RECORD_VISITS.with(|visits| visits.tabs.set(visits.tabs.get() + 1));
    }

    pub(super) fn record_splitter() {
        PAINT_RECORD_VISITS.with(|visits| visits.splitters.set(visits.splitters.get() + 1));
    }

    pub(super) fn record_contained() {
        PAINT_RECORD_VISITS.with(|visits| visits.contained.set(visits.contained.get() + 1));
    }
}

#[cfg(test)]
mod tests {
    use dockspace::geometry::LogicalRect;
    use dockspace::model::{
        DockspaceAxis, DockspaceContainedLayout, DockspaceLayout, DockspaceNode,
        DockspaceRootLayout, DockspaceSurfaceLayout, FloatingPresentationId, ItemId, RootId,
        SurfaceId,
    };
    use egui::{Context, RawInput, Rect, Ui, pos2, vec2};

    use super::structural_work;
    use crate::{Dockspace, PaneView};

    const SURFACE: SurfaceId = SurfaceId::new(1);

    struct Panes;

    impl PaneView for Panes {
        fn title(&self, item: ItemId) -> Option<egui::WidgetText> {
            Some(format!("Pane {}", item.get()).into())
        }

        fn ui(&mut self, _item: ItemId, _ui: &mut Ui) {}
    }

    fn split_root(root: RootId, first: ItemId, second: ItemId) -> DockspaceRootLayout {
        DockspaceRootLayout::new(
            root,
            DockspaceNode::equal_split(
                DockspaceAxis::Horizontal,
                [DockspaceNode::tabs([first]), DockspaceNode::tabs([second])],
            )
            .expect("the structural split fixture is valid"),
        )
    }

    fn layout(contained_count: usize) -> DockspaceLayout {
        let mut next_item = 1_u64;
        let mut items = || {
            let first = ItemId::new(next_item);
            let second = ItemId::new(next_item + 1);
            next_item += 2;
            (first, second)
        };
        let (main_first, main_second) = items();
        let mut surface = DockspaceSurfaceLayout::new(
            SURFACE,
            split_root(RootId::new(1), main_first, main_second),
        );
        let rect = LogicalRect::new(40.0, 50.0, 240.0, 180.0)
            .expect("the contained structural fixture rectangle is valid");
        for index in 0..contained_count {
            let (first, second) = items();
            surface = surface.with_contained(DockspaceContainedLayout::new(
                FloatingPresentationId::new(u64::try_from(index + 1).expect("id fits u64")),
                split_root(
                    RootId::new(u64::try_from(index + 2).expect("id fits u64")),
                    first,
                    second,
                ),
                rect,
            ));
        }
        DockspaceLayout::new([surface]).expect("the structural paint layout is valid")
    }

    fn run_frame(context: &Context, dockspace: &mut Dockspace, panes: &mut Panes) {
        let input = RawInput {
            screen_rect: Some(Rect::from_min_size(pos2(0.0, 0.0), vec2(800.0, 600.0))),
            ..RawInput::default()
        };
        let mut output = context.run_ui(input, |ui| {
            dockspace
                .show_single_surface(SURFACE, ui, panes)
                .expect("the structural product frame advances");
        });
        output.textures_delta.clear();
    }

    #[test]
    fn paint_records_are_classified_once_before_root_rendering() {
        for contained_count in [16, 128, 1_024] {
            let context = Context::default();
            let mut dockspace = Dockspace::builder(
                ("root-paint-structural-work", contained_count),
                layout(contained_count),
            )
            .build()
            .expect("the structural product dockspace initializes");
            let mut panes = Panes;
            run_frame(&context, &mut dockspace, &mut panes);

            structural_work::reset();
            run_frame(&context, &mut dockspace, &mut panes);

            let visits = structural_work::snapshot();
            let roots = contained_count + 1;
            assert_eq!(visits.panes, roots * 2);
            assert_eq!(visits.tab_bars, roots * 2);
            assert_eq!(visits.tabs, roots * 2);
            assert_eq!(visits.splitters, roots);
            assert_eq!(visits.contained, contained_count);
        }
    }
}
