# Core lifecycle and persistence review

Date: 2026-11-10 (Asia/Shanghai). Baseline: `822930f`.
Implementation commits: `724abc4`, `589c5d3`, `2a5ad8a`, `444d618`.
Status: implementation, independent review and final qualification complete at `444d618`.

Three independent Codex lanes inspect close coordination and policy, persistence
and identity, and input/presentation. The primary lane checks layout and numeric
configuration, confirms findings through the independent public host, and owns
serial Cargo verification. Review and reference exclusions remain untouched.

## R01: saved fractional geometry fails its own document integrity check

The persistence dependency used serde_json's default floating-point parser while
the document binding hash serialized the decoded numeric values again. A finite,
ordinary contained width, `449.49106478873813`, changes by one f64 ULP during
default JSON decoding. Consequently the untouched bytes from a successful save
fail to reload with `BindingHashMismatch`.

Two public regressions create a persistent rootless surface with contained
content, save the document, and use either `from_document_json` or
`prepare_document_restore_json`. Both fail in run
`8172f648-7870-4133-8fad-f5887a0686b7`
([before](2026-11-10-core-lifecycle-persistence-review/red-document-float.log)).

The dockspace crate itself now enables serde_json's `float_roundtrip` feature.
The save format, hash algorithm, schema and strict geometry validation remain
unchanged. The dependency contract also applies when dockspace is consumed
independently of this workspace. Both regressions require successful restoration,
exact rectangle equality, preserved external item keys and the expected epoch.

## R02: close-capability drift blocks native destruction publication

An approved native CloseContent request must retain its exact external close
obligation after Accept is issued. If a target pane's effective close capability
then changes between Immediate and DeferredAllowed, the frozen content check
returned an internal invariant error. Processing the exact Destroyed observation
therefore poisoned the entire input prefix instead of preserving content and
settling the native lifetime.

The two public direction tests fail with `InputPrefixReductionFailed` in run
`438927e0-883b-4f5a-a2b8-d22c052d3388`
([before](2026-11-10-core-lifecycle-persistence-review/red-native-close-content.log)).
The repair classifies an actual capability change as
`CommandError::CloseCapabilityChanged`, an expected rejection. Existing native
destruction recovery can then retain the content while retiring the old binding.
Corrupt frozen item order and other true invariants still propagate errors.
Public tests require unchanged content, retired bindings, a publishable later
frame and an inert replay of the terminal decision.

## R03: empty central main prevents closing nonempty contained content

A legal native surface may have an empty central main root and nonempty
contained roots. CloseContent rejected every individually empty root even though
its contract and final transaction cover the complete surface item roster.
The contained item could never reach application approval.

The public regression fails with `ContentUnavailable` in the same native-close
run. Capture now requires a nonempty total decision roster and retains every
root source, including the empty main. The exact Destroyed observation removes
the complete surface, main and contained roots only after approval. An unrelated
surviving surface and its item remain intact. Entirely empty CloseContent
requests remain unavailable.

## R04: accepted presentation fractions cannot be represented by the model

The public configuration builder checked its f64 docking fraction against the
open unit interval. Consumers then converted it to f32. Values such as
`0.99999999` and `1e-50` passed construction but became 1 or 0, which the command
and scene compiler correctly reject. A validated configuration could therefore
prevent normal surface compilation.

The public rejection regression fails in run
`7d74b656-c9c5-4b5b-9ce0-29348db24b2d`. The builder now validates the converted
value using the existing model `DockFraction` constructor, retaining the prior
non-finite error and reporting the original rejected f64 value. The model's
smallest positive subnormal remains valid configuration. Concrete geometry and
weight multiplication still own their separate representability checks.

An initial positive control used the smallest normal f32 fraction and required
successful right-edge preview geometry at ordinary coordinates. That assertion
exceeded the configuration contract: such a positive fraction can be too small
to express a nonempty rectangle relative to the pane's far edge. The final
controls separately test positive subnormal configuration acceptance and actual
measurement with representable ordinary and near-one fractions.

## Focused verification

The eight public regressions and controls for R01 through R04 pass in run
`6fcee99c-7f7b-4f71-8bf6-d6163c52970c`
([command](2026-11-10-core-lifecycle-persistence-review/focused-repairs.json),
[result](2026-11-10-core-lifecycle-persistence-review/focused-repairs.log)).
Fourteen core configuration and close-error classification checks pass in run
`092a5a49-49b0-44ee-86ff-4505bfbf011c`
([command](2026-11-10-core-lifecycle-persistence-review/focused-core-controls.json),
[result](2026-11-10-core-lifecycle-persistence-review/focused-core-controls.log)).
The corruption control explicitly requires a non-expected invariant error;
the capability-drift control requires the exact typed expected rejection.

## R05: restored policy revives an obsolete native content approval

Native effect-settlement phases correctly survive authority changes so the
external destruction obligation can finish. However, content deletion checked
only the final effective capability. An Immediate to DeferredAllowed to Immediate
sequence, or Immediate to Disabled to Immediate, therefore restored an old
deletion approval after that approval had lost its relevant policy facts.

Both public tests fail because exact Destroyed changes the workspace and removes
the content in run `c267e6d6-0fb3-49d1-861d-9857ca0dc6e1`
([before](2026-11-10-core-lifecycle-persistence-review/red-native-close-policy-return.log)).
The repair makes deletion permission monotonically revocable within the existing
close owner. Its private `prepared_revoked` flag prevents reacquiring the payload
after the first relevant scope mismatch. Native effect, acknowledgement and
terminal cleanup obligations remain present. Reconciliation checks only effective
CloseSurface permission and the frozen target pane capabilities; it performs no
whole-workspace preparation or global-revision rejection. A public control
changes an unrelated pane's close policy and still completes the original
approved deletion.

## R06: a migrated tab's retained receiver opens a menu on its former surface

Moving a main root from B into a contained presentation on A can leave B alive
with its own unchanged contained stacking roster. Its old actual output may
remain presented. The public receiver cannot bind again, but a previously bound
event-time receiver still proves which old pixels received the input.

The touch waiting path skipped the existing current-target check used by normal
source activation. A touch press, long-press continuation and contact end then
published a context menu with surface B and an item/root now owned by A.
The public regression fails with that exact wrong-surface menu in run
`f4a624c1-3bd7-419e-abcd-3193bc937668`
([before](2026-11-10-core-lifecycle-persistence-review/red-main-root-receiver-inputs.log)).
The adjacent waiting/armed-before-migration control already passes: the existing
mutation reconciliation cancels those active gestures. The repair reuses current
receiver validation at touch admission and in the shared continuation/release
presentation check, preserving useful old-output authority for unrelated targets.

## R07: starting scroll on a migrated strip poisons independent input

The same public main-root migration leaves an old exact scroll receiver in the
visible output while live tab-strip state correctly removes its B key. Initial
scroll ownership used only the old manifest and geometry. Applying the delta
then failed with `tab-strip scroll state is unavailable`, an internal invariant,
and prevented the whole frame from publishing.

The scroll regression reaches that error in the same run. It requires stale
scroll to be suppressed locally and an independent valid selection on A to
publish in the same frame. Initial ownership now reuses the existing continuation
lifecycle checks. Those checks also verify the current receiver and require a
resolved offset from live strip state before application. Failure follows typed
ReceiverUnknown suppression rather than poisoning the frame. No additional
fingerprint, state table or public interface is introduced.

## Final focused verification

All 51 public regressions and adjacent close, layout-lock, touch, popup-scroll,
edge-scroll and retained-hover controls pass in run
`a7f91070-a11a-4f3c-8895-39a323f9bcde`
([command](2026-11-10-core-lifecycle-persistence-review/focused-final.json),
[result](2026-11-10-core-lifecycle-persistence-review/focused-final.log)).
Fourteen public cases are new, across four independent host binaries. Both
policy-return tests and the two retained-receiver failure tests now pass;
the already-working waiting/armed control remains intact.

## Probe corrections and remaining qualification

The first migrated-input fixture moved a contained root. That changes B's
stacking roster and explicitly revokes B's interaction authority before the
proposed input. Its setup assertion failed, so those failures did not establish
R06/R07 and no production change was made from them. The
[initial probe log](2026-11-10-core-lifecycle-persistence-review/red-migrated-receiver-inputs.log)
is retained. The final main-root fixture keeps B's contained roster unchanged,
strictly confirms retained output authority, and reaches both actual failures.

Independent cross-review confirms all seven repairs and reports no blocking
P1/P2. The persistence follow-up finds no additional key-history, frontier,
paused-sidecar or atomic-publication counterexample. The close follow-up checks
exact effect/provider/binding matching, wrong acknowledgements, cancellation
frontiers, policy-dependent child recovery and post-publication terminal
compaction. The input follow-up checks retained sibling outputs, selection and
reorder scope, equivalent redraw, smooth-scroll tails and popup cycles. All
three lanes finish without another confirmed P2 in those inspected sequences.

The primary lane also rules out a near-tolerance raw-weight fixture as a new
public finding: public split construction normalizes those shares, and no
ordinary operation producing the exceptional input has been established. No
strict canonicalization check is removed based on that fixture.

These bounded reviews do not prove all interleavings correct. Existing ImGui
timing and physical-platform scopes are unchanged.

## Full serial qualification

All sixteen serial gates pass against unchanged source at `444d618`, using
`CARGO_BUILD_JOBS=1`, `CARGO_INCREMENTAL=0` and the existing target directory.
Commands, timestamps and exit codes are recorded in
[serial-gates.json](2026-11-10-core-lifecycle-persistence-review/serial-gates.json).

| Gate | Result | Evidence |
| --- | --- | --- |
| Core and shared conformance, all features/targets | 2,134 pass; 3 existing skips | [core-tests.log](2026-11-10-core-lifecycle-persistence-review/core-tests.log) |
| Independent persistence host, all targets | 366 pass across 67 binaries | [host-serde.log](2026-11-10-core-lifecycle-persistence-review/host-serde.log) |
| egui adapter, all features/targets | 181 pass | [adapter-tests.log](2026-11-10-core-lifecycle-persistence-review/adapter-tests.log) |
| Official unmodified-egui consumer | 24 pass | [official-consumer.log](2026-11-10-core-lifecycle-persistence-review/official-consumer.log) |
| Product default / persistence | 114 / 119 pass | [default](2026-11-10-core-lifecycle-persistence-review/product-default.log), [persistence](2026-11-10-core-lifecycle-persistence-review/product-serde.log) |
| Pinned native fork, all features/targets | 593 pass; 1 existing skip | [native-tests.log](2026-11-10-core-lifecycle-persistence-review/native-tests.log) |
| Workspace/native correctness and suspicious Clippy | Pass | [serial-gates.json](2026-11-10-core-lifecycle-persistence-review/serial-gates.json) |
| Strict workspace/native Rustdoc | Pass with `RUSTDOCFLAGS=-D warnings` | [serial-gates.json](2026-11-10-core-lifecycle-persistence-review/serial-gates.json) |
| Workspace doctests | 11 pass | [workspace-doctests.log](2026-11-10-core-lifecycle-persistence-review/workspace-doctests.log) |
| Workspace/host/native format and core dependency constraint | Pass | [serial-gates.json](2026-11-10-core-lifecycle-persistence-review/serial-gates.json) |

The recording host's Presented facts establish public protocol behavior. Actual
Linux X11 qualification below separately exercises real windows and rendering.

## Actual Linux X11 qualification

All thirteen window-manager, build and execution gates pass against `444d618`
in [real-x11-gates.json](2026-11-10-core-lifecycle-persistence-review/real-x11-gates.json).
Seven exact eframe cases execute in separate real-window processes. They cover
successor preparation, terminal output hooks, style publication, output capacity,
native ingress, retained editing and native resize.

The 4,194,304-byte bounded create/first-live/redock/retirement profile passes.
Its recorded root-boundary peaks are 2,220,400 resident bytes and 3,340,616
reserved bytes
([native log](2026-11-10-core-lifecycle-persistence-review/native-redock-bounded.log)).
These characterize owned storage in this finite profile, not total process
memory or an ImGui timing comparison.

Both complete editor drivers pass at `2026-11-10T10:21:26.080674+00:00`
([editor log](2026-11-10-core-lifecycle-persistence-review/actual-editor-lifecycle.log)).
They exercise actual content docking, twelve real focused edits, six destroyed
child lifetimes, first-live ownership, Dock Back, OS-close recovery,
programmatic close/Allow/reopen, arrangement locking, Default reset, named JSON
restore, missing-pane rendering/reload, live Child replacement and Root rebinding.
The original Root window survives, tabs and selection remain consistent, and
final quiescence holds.

Qualification reuses the existing one-CPU, 4 GiB container, Xvfb/Openbox at
1920 by 1080, software Glow and `/native-target`. Cargo uses one build job with
incremental compilation and development/test debug information disabled;
LLVMpipe uses one thread. This refreshes the recorded finite X11 profiles
without extending other physical-platform certification. Source remains
unchanged from `444d618` after qualification.
