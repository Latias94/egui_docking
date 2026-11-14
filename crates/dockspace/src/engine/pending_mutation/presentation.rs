//! Progress of a frozen mutation; renderer terminals remain owned by the ledger.

use std::collections::BTreeSet;

use crate::presentation::observation::{HostFrameKey, HostPresentationObservationOutcome};

#[derive(Debug, Clone, PartialEq)]
pub(in crate::engine) enum PresentationProgress {
    Waiting { outputs: BTreeSet<HostFrameKey> },
    Presented { output: HostFrameKey },
    Failed,
}

impl Default for PresentationProgress {
    fn default() -> Self {
        Self::waiting([])
    }
}

impl PresentationProgress {
    pub(in crate::engine) fn waiting(outputs: impl IntoIterator<Item = HostFrameKey>) -> Self {
        Self::Waiting {
            outputs: outputs.into_iter().collect(),
        }
    }

    pub(in crate::engine) fn awaiting_presentation(&self) -> bool {
        matches!(self, Self::Waiting { .. })
    }

    pub(in crate::engine) fn needs_paint(&self) -> bool {
        matches!(self, Self::Waiting { outputs } if outputs.is_empty())
    }

    pub(in crate::engine) fn is_presented(&self) -> bool {
        matches!(self, Self::Presented { .. })
    }

    pub(in crate::engine) fn is_failed(&self) -> bool {
        matches!(self, Self::Failed)
    }

    pub(in crate::engine) fn record_output(&mut self, output: HostFrameKey) {
        if let Self::Waiting { outputs } = self {
            outputs.insert(output);
        }
    }

    pub(in crate::engine) fn observe_presented(&mut self, output: HostFrameKey) {
        if !self.is_failed() {
            *self = Self::Presented { output };
        }
    }

    pub(in crate::engine) fn fail(&mut self) {
        // Presentation can still be revoked before the frozen command commits.
        // A failed command cannot be revived by a late renderer observation.
        *self = Self::Failed;
    }

    pub(in crate::engine) fn retire_prefixes(
        &mut self,
        outcomes: &[HostPresentationObservationOutcome],
    ) {
        self.retire_where(|output| {
            outcomes.iter().any(|outcome| {
                matches!(outcome, HostPresentationObservationOutcome::Retired {
                stream, settled_through, ..
            } if output.stream() == *stream && output <= *settled_through)
            })
        });
    }

    #[cfg(test)]
    pub(in crate::engine) fn retire_keys(&mut self, retired: &BTreeSet<HostFrameKey>) {
        self.retire_where(|output| retired.contains(&output));
    }

    fn retire_where(&mut self, mut retired: impl FnMut(HostFrameKey) -> bool) {
        let Self::Waiting { outputs } = self else {
            return;
        };
        let had_outputs = !outputs.is_empty();
        outputs.retain(|output| !retired(*output));
        if had_outputs && outputs.is_empty() {
            *self = Self::Failed;
        }
    }

    pub(in crate::engine) fn retain_emissions(&self, emissions: &mut BTreeSet<HostFrameKey>) {
        match self {
            Self::Waiting { outputs } => emissions.extend(outputs.iter().copied()),
            Self::Presented { output } => {
                emissions.insert(*output);
            }
            Self::Failed => {}
        }
    }
}
