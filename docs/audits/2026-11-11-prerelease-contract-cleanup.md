# Prerelease contract cleanup

Baseline: `7ccc1a5`. The project is unreleased. This change deliberately replaces
the persistence format and removes superseded internal interfaces without
migration readers or compatibility aliases.

Implementation commits: core `90820f0`, egui `97b88d1`, native `939dd8c`.

## Persistence

The complete document has one schema, `[1, payload]`. Workspace, external item
keys and viewport preferences are ordinary strict objects. Three private
component envelopes, their version constants, payload mirrors, tuple
serializers, visitors and nested version errors were removed. Unsupported outer
versions still reject before current-payload decoding or application identity
recognition.

Nullable fields must be present. Explicit `null` represents intentional absence;
omission is malformed input. Serde otherwise treats a missing `Option` field as
`None`, even without `serde(default)`. The existing required-option decoder is
shared across the nine nullable fields. The historical `arrangement_locked`
default was also removed.

An internally tagged Serde unit variant ignores unknown payload fields even
when its enum has `deny_unknown_fields`. The Root hosting preference therefore
decodes through a strict empty object. Its public unit-variant interface remains
simple; no second enum or migration representation was introduced.

Document generation, presentation identity frontiers and runtime epochs remain
monotonic. They protect lineage and stale authority; they are not schema
versions.

## Egui

Pointer authority has three concrete modes: `HostJournal`, `LocalResponses` and
`ExternalJournal`. Unused boolean combinations, cancellation history and the
renderer-local duplicate cancellation path were removed. Host pointer retirement
still cancels the exact active gesture before painting. Source gaps and previews
follow the resulting core paint program.

The always-empty `presentation_actions` channel was removed through the native
consumer. Exact execution receipts, receiver declarations, output proof and
pane-focus observations retain their separate responsibilities. Four narrow
group-drag aliases were replaced with the existing region-based interface.

A confirmed UI defect was repaired: policy-disabled tabs had inherited their
empty interactive drag bounds as hover and accessibility bounds. They now use
their visible bounds while remaining hover-only and disabled. They expose no
Click/Focus actions or docking receiver. The public regression checks both
callback-only and terminal-owned runs, actual tooltip painting, mouse input and
AccessKit requests. The existing owned-to-unowned drag regression additionally
checks exactly one provider-retirement feedback and no repeated delivery on the
old release.

## Native lifecycle

Manual coordinator reserve/attach/replace/unbind wrappers and the
`NativeViewportReplacement` mode enum were removed. Actual replacement uses the
existing retired-predecessor reservation followed by deferred creation and
`attach_output`. Same-window Root continuation remains owned by the real core
Prepared commit. Its repeated validation after publication was removed because
the complete roster and physical receipt are already validated under the same
mailbox lock before commit.

Unused effect/inventory/quiescence reporting wrappers and diagnostics were
removed. Tests now cross the active create, Prepared continuation and automatic
quiescence protocols. They retain late-callback rejection, exact predecessor
checks, one-shot physical receipts, Destroyed-before-release, input sidecar
retention, bounded admission and failure atomicity. Platform-specific helpers
used by Linux tests remain available.

The root README's obsolete one-surface-per-window limitation and stale multiview
anchor were corrected to match the existing shared external Root contract.
Runtime-owned Children retain their whole-client placement and the physical
qualification remains limited to the recorded Linux X11 profile.

## Verification

Status: implementation and final qualification complete. Rust source and tests
add 1,063 lines and remove 1,844 lines, a net reduction of 781 lines.

Three independent lanes reviewed persistence authority, egui behavior and native
lifecycle, with a separate cross-review of the final changes. No confirmed P1/P2
remained in the inspected change set.

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,354 passed; three existing skips |
| Core without optional features | 2,056 passed; three existing skips |
| Independent public host with persistence | 430 passed |
| Affected default-feature host suites | 55 passed across three binaries |
| Official egui product consumer | 114 default / 119 persistence passed |
| Official egui consumer, including demo tests | 27 passed |
| Native workspace, including fork product consumer | 652 passed; one existing skip |
| Workspace and native correctness/suspicious Clippy | Both passed |
| Workspace and native documentation with warnings denied | Both passed |
| Documentation examples | Eleven passed |
| Five-workspace formatting and renderer-neutral dependency boundary | All passed |
| Linux native unit suite, excluding window-event-loop fixtures | 629 passed; thirteen ignored; five explicitly filtered |
| Isolated actual-eframe X11 processes | Fourteen passed |
| Bounded 4 MiB create/first-live/redock/retirement | Passed; resident/reserved peaks 2,157,864/3,274,816 bytes |
| Shared-window physical journey, normal and late-discard | Both passed |

The [nineteen serial receipts](2026-11-11-prerelease-contract-cleanup/serial-gates.json)
and [twenty X11 receipts](2026-11-11-prerelease-contract-cleanup/real-x11-gates.json)
retain exact commands, UTC timestamps and their logs. Cargo used one build job,
disabled incremental compilation and reused the existing target directories.
Stored logs normalize trailing whitespace.
The [source manifest](2026-11-11-prerelease-contract-cleanup/source-sha256.json)
contains 59 changed Rust files and Cargo manifests. One final import-only cleanup
moved a test-only type behind `cfg(test)`; native tests were rerun afterward.
All final gates match the recorded hashes, which were checked again after X11.

An initial extra Linux unit run accidentally launched several event-loop fixtures
inside one test process. Winit rejected the later loops with `RecreationAttempt`.
The [failed launch and log](2026-11-11-prerelease-contract-cleanup/intermediate-x11-event-loop-recreation.json)
are separate from the successful receipts. The corrected pure unit run explicitly
filters all five event-loop tests; those five each pass in their own process
within the fourteen isolated renderer tests. No product change or assertion
removal was used to resolve the launch error.

The physical qualification scope is Linux X11 with the existing Xvfb/Openbox and
Glow fixture. It does not establish physical mixed-monitor DPI behavior or
extend Wayland, Windows or macOS qualification. This cleanup does not supply a
new ImGui performance comparison or rerun the complete eight-pane editor drivers.
