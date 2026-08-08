//! Deferred native policy and style configuration.

use dockspace::policy::DockPolicy;
use egui_dockspace::backend::EguiNativeConfigurationSession;
use egui_dockspace::{DockStyle, Dockspace, DockspaceError};

/// Application configuration waiting for the next terminal host-frame phase.
#[derive(Clone, Default)]
pub(crate) struct NativeConfigurationQueue {
    policy: Option<DockPolicy>,
    style: Option<DockStyle>,
}

impl NativeConfigurationQueue {
    pub(crate) fn replace_policy(&mut self, policy: DockPolicy) {
        self.policy = Some(policy);
    }

    pub(crate) fn replace_style(&mut self, style: DockStyle) -> Result<(), DockspaceError> {
        style.validate()?;
        self.style = Some(style);
        Ok(())
    }

    pub(crate) fn apply(
        &self,
        session: &mut EguiNativeConfigurationSession,
        dockspace: &Dockspace,
    ) -> Result<(), DockspaceError> {
        if let Some(policy) = self.policy.as_ref() {
            session.set_policy(dockspace, policy.clone())?;
        }
        if let Some(style) = self.style.as_ref() {
            session.set_style(dockspace, style.clone())?;
        }
        Ok(())
    }

    pub(crate) fn accept_commit(&mut self) {
        self.policy = None;
        self.style = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacements_are_last_wins_and_commit_clears_the_queue() {
        let mut queue = NativeConfigurationQueue::default();
        let mut first = DockPolicy::new();
        first.set_allow_tab_merge(false);
        let mut second = DockPolicy::new();
        second.set_allow_edge_split(false);
        queue.replace_policy(first);
        queue.replace_policy(second.clone());

        let mut first_style = DockStyle::default();
        first_style.tab_bar_height += 1.0;
        let mut second_style = DockStyle::default();
        second_style.tab_bar_height += 2.0;
        queue
            .replace_style(first_style)
            .expect("the first style is valid");
        queue
            .replace_style(second_style.clone())
            .expect("the replacement style is valid");

        assert_eq!(queue.policy, Some(second));
        assert_eq!(queue.style, Some(second_style));
        queue.accept_commit();
        assert!(queue.policy.is_none());
        assert!(queue.style.is_none());
    }

    #[test]
    fn invalid_style_does_not_replace_the_last_valid_candidate() {
        let mut queue = NativeConfigurationQueue::default();
        let valid = DockStyle::default();
        queue
            .replace_style(valid.clone())
            .expect("the default style is valid");
        let mut invalid = DockStyle::default();
        invalid.tab_bar_height = f32::NAN;

        assert!(queue.replace_style(invalid).is_err());
        assert_eq!(queue.style, Some(valid));
    }
}
