//! Test support for actions bound to an exact presented tab control.

use super::*;

impl DockEngine {
    pub(super) fn prepare_presented_tab_strip_control(
        &self,
        projection: crate::presentation::scene::SurfaceInteractionProjection<'_>,
        control: crate::presentation::tab_strip::TabStripControlId,
    ) -> Result<PreparedTabStripControlActivation, InteractionRejection> {
        let key = TabStripStateKey::new(projection.output_ticket().surface(), control.bar());
        let plan = projection.plan();
        let record = plan
            .tab_strip_control_records()
            .iter()
            .copied()
            .find(|record| record.id() == control)
            .ok_or(InteractionRejection::TabStripControlUnavailable { control })?;
        if !record.enabled() {
            return Err(InteractionRejection::TabStripControlDisabled { control });
        }
        if self
            .presentation_authority
            .tab_strip_states
            .state(key)
            .is_none()
            || !control.has_source_in(plan)
        {
            return Err(InteractionRejection::TabStripSourceUnavailable { key });
        }
        Ok(PreparedTabStripControlActivation {
            presentation: Self::freeze_interaction_projection(projection),
            control: crate::interaction::FrozenTabStripControlClick { key, record },
        })
    }

    pub(super) fn prepare_presented_tab_list_menu_row(
        &self,
        projection: crate::presentation::scene::SurfaceInteractionProjection<'_>,
        session: crate::presentation::tab_strip::TabListMenuSessionId,
        tab: crate::presentation::scene::TabSceneId,
    ) -> Result<PreparedTabListMenuRowActivation, InteractionRejection> {
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|menu| menu.session() == session)
            .and_then(|menu| menu.rows().iter().copied().find(|row| row.tab() == tab))
            .ok_or(InteractionRejection::TabListMenuRowUnavailable { session, tab })?;
        if !self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .is_some_and(|active| active.session() == session && active.items().contains(&tab.item))
        {
            return Err(InteractionRejection::TabListMenuSessionUnavailable { session });
        }
        Ok(PreparedTabListMenuRowActivation {
            presentation: Self::freeze_interaction_projection(projection),
            row: crate::interaction::FrozenTabListMenuRowClick {
                session,
                record,
                revision,
            },
        })
    }

    pub(super) fn prepare_presented_tab_list_menu_scroll(
        &self,
        projection: crate::presentation::scene::SurfaceInteractionProjection<'_>,
        session: crate::presentation::tab_strip::TabListMenuSessionId,
        adjustment: TabScrollAdjustment,
    ) -> Result<PreparedTabListMenuScroll, InteractionRejection> {
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .cloned()
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let active = self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .filter(|active| active.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let requested = match &adjustment.0 {
            super::TabScrollAdjustmentKind::ScrollByPreserving { keep_visible, .. } => {
                keep_visible.as_slice()
            }
            super::TabScrollAdjustmentKind::RevealItem(item) => std::slice::from_ref(item),
        };
        if let Some(item) = requested.iter().copied().find(|item| {
            !active.items().contains(item)
                || !record.rows().iter().any(|row| row.tab().item == *item)
        }) {
            return Err(InteractionRejection::TabListMenuScrollItemUnavailable { session, item });
        }
        Ok(PreparedTabListMenuScroll {
            presentation: Self::freeze_interaction_projection(projection),
            session,
            revision,
            record,
            adjustment,
        })
    }

    pub(super) fn prepare_presented_tab_list_menu_navigation(
        &self,
        projection: crate::presentation::scene::SurfaceInteractionProjection<'_>,
        session: crate::presentation::tab_strip::TabListMenuSessionId,
        navigation: TabListMenuNavigation,
    ) -> Result<PreparedTabListMenuNavigation, InteractionRejection> {
        let plan = projection.plan();
        let revision = plan.popup().revision();
        self.validate_tab_list_menu_popup(plan, session, revision)?;
        let record = plan
            .tab_list_menu_records()
            .iter()
            .find(|record| record.session() == session)
            .cloned()
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let active = self
            .presentation_authority
            .tab_strip_states
            .active_menu_for(session.key())
            .filter(|active| active.session() == session)
            .ok_or(InteractionRejection::TabListMenuSessionUnavailable { session })?;
        let current = active
            .items()
            .iter()
            .position(|item| *item == active.focus())
            .ok_or(InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: active.focus(),
            })?;
        let last = active.items().len().checked_sub(1).ok_or(
            InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: active.focus(),
            },
        )?;
        let target = match navigation {
            TabListMenuNavigation::Previous => active.items()[current.saturating_sub(1)],
            TabListMenuNavigation::Next => active.items()[current.saturating_add(1).min(last)],
            TabListMenuNavigation::First => active.items()[0],
            TabListMenuNavigation::Last => active.items()[last],
            TabListMenuNavigation::Focus(item) => item,
        };
        if !active.items().contains(&target)
            || !record.rows().iter().any(|row| row.tab().item == target)
        {
            return Err(InteractionRejection::TabListMenuFocusItemUnavailable {
                session,
                item: target,
            });
        }
        Ok(PreparedTabListMenuNavigation {
            presentation: Self::freeze_interaction_projection(projection),
            session,
            revision,
            record,
            target,
        })
    }
}
