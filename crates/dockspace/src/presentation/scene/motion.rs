//! Sampled pane geometry retains the sealed target's layout and semantic facts.

use super::*;

impl PresentationPlan {
    /// Returns whether this artifact contains the target root's valid split partition.
    #[must_use]
    pub fn partition_available(&self, root: RootId) -> bool {
        #[cfg(feature = "motion")]
        {
            !self.facts.unavailable_partitions.contains(&root)
        }
        #[cfg(not(feature = "motion"))]
        {
            let _ = root;
            true
        }
    }

    /// Returns the shared pane precedence used by drawing and input.
    #[must_use]
    #[cfg(any(feature = "motion", test))]
    pub fn pane_rank(&self, root: RootId, tabs: NodeId) -> usize {
        #[cfg(feature = "motion")]
        {
            self.facts
                .pane_ranks
                .get(&PaneSceneId { root, tabs })
                .copied()
                .unwrap_or(0)
        }
        #[cfg(not(feature = "motion"))]
        {
            let _ = (root, tabs);
            0
        }
    }

    #[cfg(feature = "motion")]
    pub(crate) fn root_control_pane_rank(&self, root: RootId) -> (bool, usize) {
        (
            !self.partition_available(root)
                || self
                    .facts
                    .pane_ranks
                    .iter()
                    .any(|(id, rank)| id.root == root && *rank != 0),
            0,
        )
    }

    #[cfg(feature = "motion")]
    pub(crate) fn begin_presentation_sample(
        &self,
        moving_roots: BTreeSet<RootId>,
        ranks: &BTreeMap<PaneSceneId, usize>,
    ) -> Self {
        let mut sampled = self.clone();
        let facts = sampled.facts_mut();
        facts.unavailable_partitions = moving_roots;
        facts.pane_ranks.clone_from(ranks);
        facts.pane_records.clear();
        facts.tab_records.clear();
        facts.tab_bar_records.clear();
        facts.tab_strip_control_records.clear();
        facts.presentation_menu_anchor_records.retain(|anchor| {
            matches!(anchor.host(), PresentationMenuAnchorHost::ContainedTitle(_))
        });
        let unavailable_partitions = &facts.unavailable_partitions;
        facts.drop_targets.retain(|target| {
            target
                .id()
                .root()
                .is_none_or(|root| !unavailable_partitions.contains(&root))
        });
        facts
            .drop_guide_clusters
            .retain(|cluster| !unavailable_partitions.contains(&cluster.id().root));
        facts
            .magnetic_target_field
            .retain_roots(|root| !unavailable_partitions.contains(&root));
        facts.surface_program = SurfaceProgram::default();
        if let Some(background) = &facts.surface_background {
            facts
                .surface_program
                .defer_surface_background(background.id());
        }
        for target in &facts.drop_targets {
            facts.surface_program.defer_drop_target(target.id());
        }
        for cluster in &facts.drop_guide_clusters {
            facts.surface_program.defer_guide(cluster);
        }
        for backdrop in &facts.tab_list_menu_backdrop_records {
            facts
                .surface_program
                .defer_popup_backdrop(backdrop.session());
        }
        for menu in &facts.tab_list_menu_records {
            facts.surface_program.defer_popup_menu(menu.session());
        }
        sampled
    }

    /// Links fresh leaf rows without repeating target topology or solver validation.
    #[cfg(feature = "motion")]
    pub(crate) fn finish_presentation_sample(&mut self) -> Result<(), SceneBuildError> {
        let surface = self.surface;
        let bounds = self.bounds();
        let facts = self.facts_mut();
        for pane in &facts.pane_records {
            if !contains(bounds, pane.bounds()) || !contains(pane.bounds(), pane.content_bounds()) {
                return Err(SceneBuildError::InvalidPaneRecord {
                    surface,
                    id: pane.id(),
                });
            }
        }
        facts.pane_records.sort_unstable_by_key(PaneRecord::id);
        facts
            .tab_records
            .sort_unstable_by_key(|record| *record.id());
        facts
            .tab_bar_records
            .sort_unstable_by_key(|record| *record.id());
        facts
            .tab_strip_control_records
            .sort_unstable_by_key(|record| record.id());
        facts
            .presentation_menu_anchor_records
            .sort_unstable_by_key(|record| record.root());
        self.link_surface_program()
    }
}

#[cfg(feature = "motion")]
fn contains(outer: LogicalRect, inner: LogicalRect) -> bool {
    inner.x() >= outer.x()
        && inner.y() >= outer.y()
        && inner.max().x() <= outer.max().x()
        && inner.max().y() <= outer.max().y()
}

#[cfg(all(test, not(feature = "motion")))]
mod tests {
    use super::*;

    #[test]
    fn static_plan_keeps_partition_available_with_zero_pane_rank() {
        let plan = PresentationPlan::new(
            SurfaceId::new(1),
            LogicalRect::new(0.0, 0.0, 100.0, 100.0).unwrap(),
        );
        assert!(plan.partition_available(RootId::new(1)));
        assert_eq!(plan.pane_rank(RootId::new(1), NodeId::default()), 0);
    }
}
