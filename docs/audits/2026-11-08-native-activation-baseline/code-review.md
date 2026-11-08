# Independent review

Scope: ten Rust files changed relative to `c0daedf`, including the activation
coordinator, reserved-create gate and eight test/fixture files. Two independent
reviewers examined the final diff without running Cargo or the GUI and without
accessing `docs/reviews/`.

## Standards

No findings or static build concerns. `activation_observation_baseline()` names
the distinction between ordering evidence and Known focus authority. Reusing
the existing generation and tombstone state avoids another state machine,
public API or queue. Coordinator and lifecycle tests cover complementary
boundaries.

The fixture loops follow the existing managed lifecycle test style and do not
justify another one-use helper. They require unique exact release/focus effects
and reject duplicate, incorrect-binding or additional operations. The single
focus checks preserve the exclusion of pass-through restoration.

## Spec

No substantive findings. A valid Unknown envelope supplies only a generation;
uninitialized, missing, conflicting, skipped or replaced streams remain blocked.
Immediate success still requires real target focus. Otherwise the existing
capability-supported RequestFocus retains the original causal stamp, binding
and effect correlation.

The reserved winner remains subject to activation eligibility, item ownership,
staging and first-live admission. Dispatch consumes its old reservation; a later
main-window observation follows the existing cancellation/effect settlement
path and cannot start that creation request again. Observe-only and lifecycle
compensation requirements remain unchanged.

The four fixture updates preserve their original cleanup, presentation and drag
assertions. `accepted().is_none()` correctly acknowledges command dispatch while
leaving success to an external focus fact. Continuing fixtures submit their
simulated observations through the formal host interface; they do not fabricate
pane success.

The additional native-model setup facts match their scenario premises: redock
destinations are already active roots, and local drags begin in their focused
input window. These observations use the formal interface and precede pressure
counting. The ordinary-focus soak still reports Foreign focus later, exercises
exact predecessor/successor tokens and checks a queue peak of one. No effect
slots, byte budgets, edge counts, cleanup or quiescence assertions were relaxed.
Unknown activation remains directly covered by the new regressions.

Final static findings: Standards 0; Spec 0. Execution results belong to the main
verification run and the separately recorded actual-window journey.
