use egui::{Context, FullOutput, Id, RawInput, Ui};

fn presentation_provider_state_id() -> Id {
    Id::new("egui_dockspace_test_presentation_provider")
}

/// One explicit completed outer egui render boundary observed by the test provider.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TestPresentationBoundary(u64);

impl TestPresentationBoundary {
    fn checked_next(self) -> Option<Self> {
        self.0.checked_add(1).map(Self)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct CompletedTestPresentationBoundary {
    pub(crate) boundary: TestPresentationBoundary,
    pub(crate) final_pass: u32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TestPresentationProviderState {
    active: TestPresentationBoundary,
    completed: Option<CompletedTestPresentationBoundary>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TestPresentationProviderSnapshot {
    pub(crate) active: TestPresentationBoundary,
    pub(crate) completed: Option<CompletedTestPresentationBoundary>,
}

/// Runs one complete egui host frame and reports its final boundary afterwards.
///
/// All callbacks and discard passes within this call observe the same boundary.
/// Only a later call can retire the emissions produced here.
pub(crate) fn run_ui(
    context: &Context,
    input: RawInput,
    mut run_ui: impl FnMut(&mut Ui),
) -> FullOutput {
    context.data_mut(|data| {
        let state_id = presentation_provider_state_id();
        if data
            .get_temp::<TestPresentationProviderState>(state_id)
            .is_none()
        {
            data.insert_temp(state_id, TestPresentationProviderState::default());
        }
    });

    let mut final_pass = None;
    let output = context.run_ui(input, |ui| {
        final_pass = Some(
            u32::try_from(ui.ctx().current_pass_index())
                .expect("test egui pass index must fit in u32"),
        );
        run_ui(ui);
    });
    let completed_final_pass = output
        .platform_output
        .num_completed_passes
        .checked_sub(1)
        .and_then(|pass| u32::try_from(pass).ok())
        .expect("a completed egui run must report a representable final pass");
    assert_eq!(
        final_pass,
        Some(completed_final_pass),
        "the last callback pass must match the completed FullOutput boundary",
    );

    context.data_mut(|data| {
        let state_id = presentation_provider_state_id();
        let mut state = data
            .get_temp::<TestPresentationProviderState>(state_id)
            .expect("the deterministic test provider is installed");
        state.completed = Some(CompletedTestPresentationBoundary {
            boundary: state.active,
            final_pass: completed_final_pass,
        });
        state.active = state
            .active
            .checked_next()
            .expect("test presentation boundary must not overflow");
        data.insert_temp(state_id, state);
    });
    output
}

pub(crate) fn current_presentation_provider(
    context: &Context,
) -> Option<TestPresentationProviderSnapshot> {
    context.data(|data| {
        data.get_temp::<TestPresentationProviderState>(presentation_provider_state_id())
            .map(|state| TestPresentationProviderSnapshot {
                active: state.active,
                completed: state.completed,
            })
    })
}

/// Removes the deterministic provider without changing dockspace state.
///
/// This lets lifecycle tests prove that an accepted `Unknown` capture keeps
/// its unsettled outputs available for a later authoritative provider.
pub(crate) fn remove_presentation_provider(context: &Context) {
    context.data_mut(|data| {
        data.remove::<TestPresentationProviderState>(presentation_provider_state_id());
    });
}
