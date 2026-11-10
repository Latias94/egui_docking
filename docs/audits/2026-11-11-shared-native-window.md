# Shared native window qualification

Status: **complete for the recorded finite Linux X11 profile**. Baseline:
`78c1eaa`.
This record covers the complete G01 journey in the
[approved design](../plans/2026-11-10-shared-native-window-design.md).
All sixteen serial and eighteen real-X11 gates pass. The acceptance map below
distinguishes public-host, native component and actual physical coverage.

## Ownership and public contract

One real external Root window owns physical geometry, observation sequences,
focus, capture, close and resource lifetime. Its independent logical surfaces
retain exact placement generations, measurements, presentation streams and
receiver artifacts. Runtime-owned Child windows retain their whole-client
placement and first-live ownership transfer.

The public host submits one complete nonempty placement roster inside a
rollbackable frame. Layout size is independent of the physical clip. One Joined
edge carries a physical window-client point; core projects captured source and
actual hover independently. A framework window-hit answer must identify a real
presented surface artifact before the surface receiver question is issued.

One native output token owns the complete final-pass contribution collection.
All proof validation precedes confirmation and publication. The renderer
terminal settles the owned outputs together. A native close freezes all hosted
content into one Allow/Defer/Veto decision. Destroyed is observed once for the
window while exact per-placement obligations drain independently.

## Regressions found during integration

The retained logs in [the evidence directory](2026-11-11-shared-native-window/)
include intermediate failures. They are diagnostic evidence, not successful
gate receipts.

- Shared output capacity must be checked for the complete window before taking
  buffered text or IME input. Unmeasured surfaces reserve distinct output slots;
  their stream identity is checked after measurement establishes the endpoint.
  Charging an unknown endpoint as a new stream incorrectly deferred existing
  one-slot consumers and prospective Child presentation.
- A terminal physical observation may carry its original callback correlation
  after placement changes. Window lifetime checks preserve that correlation;
  exact effect and placement ledgers still determine what may settle.
- Destruction requires retirement guards for every exact placement. Quiescing
  one sibling must not reclaim the token or erase another sibling's guard.
- Destroyed revokes live coordinates. Recovery may use retained layout facts
  without restoring revoked input or presentation authority.
- Repeated identical placement submission remains a no-op while a close owns
  its scope. Actual membership and geometry changes stay refused.
- An initial explicit placement equal to the implicit whole-client layout must
  still be stored. Its exact binding and current output remain valid, while
  later client resize preserves the declared layout and clip.
- Preparing a complete-root native move must reject removal of its own sole
  recovery surface. The availability query and submission now share that
  preflight; a retained sibling root supplies the positive group-move control.
- The reference adapter must run the complete roster for which native input was
  admitted. A roster announced after input admission becomes eligible on a
  later run. The ordinary Root entry follows that same frozen roster; Child
  callbacks remain singleton. Real X11 probes check exact text delivery across
  both roster expansion and entry-point switching.
- Registering a transformed egui area must compute visibility before entering
  `memory_mut`. Calling `Ui::is_visible` inside that write closure reenters the
  context lock. The real-process failure and backtrace are retained separately.
- A current surface with a replacement window must query only that window's
  route. Falling back to its predecessor's retained route exposes an attachment
  before the successor is admitted. Historical lookup remains available only
  when the session has no current window for that logical surface, preserving
  occupied-target rejection during workspace replacement.
- The physical smoke consumes AccessKit updates as deltas, retaining unchanged
  ancestors and pruning detached subtrees before calculating Tab bounds. It
  obtains client-to-root coordinates from X11 `TranslateCoordinates` rather
  than assuming `xdotool getwindowgeometry` reports the client origin. The
  diagnosed decorated-window offset was `(1, 20)` pixels; no offset is encoded
  in the driver. The final ordinary and late-discard journeys pass.
- A pane callback can precede semantic presentation and the window manager's
  final caption update. The physical driver requires the public surface
  presentation observation, then waits within its existing timeout for exactly
  one visible Child with the expected title. An initial `Dockspace panel 4`
  caption is not evidence that the real Child is absent.
- Exact Unicode `xdotool search --name` did not match the Child even after its
  final `_NET_WM_NAME(UTF8_STRING)` and `getwindowname` were correct; the legacy
  `WM_NAME(STRING)` encoding differed. The driver now enumerates visible named
  windows, reads each UTF-8 caption and requires exactly one full-string match.
  It does not accept the fallback caption or weaken the expected identity.
- Pane layout and final-output callbacks run separately from the physical
  driver's thread. Publishing a pane rectangle with an empty Tab point between
  those callbacks exposed an incomplete observation despite an earlier valid
  presentation. The smoke must publish its geometry only after joining the pane
  rectangle and current accessibility tree in the completed output hook.
- The output plugin runs after the current Context viewport has returned to
  Root, including for a Child's output. Its actual native output token still
  identifies the Child. The diagnostic run observed Context `FFFF` with token
  viewport `7A02`, while the Child pane callback used `7A02`. Accessibility trees,
  completed-pass authorities and pending geometry must use the token viewport.
- Final independent review rejected a placement-readiness counter: discarded
  passes in one run could each observe the predecessor's presentation and
  advance it before the new mapping committed. Readiness must join B's actual
  completed-output origin/scale with current A/B presentation on a subsequent
  native output token. Repeated passes alone are not new-placement evidence.

## Independent review disposition

Three implementation lanes exchanged read-only reviews after their repairs.
The final core review covered registry publication, producer retention, physical
callback correlation, first-live, close/recovery, exact retirement and output
admission. The native review covered Joined delivery, window-hit artifacts,
capture/hover separation, modifier callbacks, route replacement and resource
receipts. The rendering review covered transformed paint/hit/accessibility,
product menus, contribution ownership, startup continuation and complete-roster
retirement prepayment.

Confirmed defects were repaired and receive their final execution results from
the gate receipts. No review may substitute its static assessment for a real
platform test. The final review did not retain an unresolved reachable P1/P2 in
its inspected G01 scope; subsequent real-process failures still require repair.

Two speculative passthrough findings were withdrawn after checking their
generation boundary. Public shared External Roots use `Preserve`; whole-window
passthrough is generated only for a singleton runtime-owned Child moving its
complete unique root. The review did not fabricate a shared Child or relabel a
Child effect as a shared Root request. A positive shared-Root control checks
that B's drag preserves input and emits no passthrough effect. Existing Child
tests cover Applied, physical roster publication, exact ACK settlement, restore
and stale-window rejection. The narrow window-fact cleanup retains the original
exact command binding, token, value and ACK source.

## Acceptance coverage

The four public shared-window suites contain 27 passing tests. Their
`RecordingRenderer` results establish the renderer-neutral host contract.
Native output-token, completed-pass and physical-window behavior require the
separate adapter and X11 gates; the public recording host does not certify them.

| Design step | Public host evidence | Additional execution boundary |
| --- | --- | --- |
| One W, independent A/B placements | [shared_window_layout.rs](../../integration/dockspace-host-harness/tests/shared_window_layout.rs): candidate drop/retry, complete roster and one physical observation | Actual framework composition belongs to the shared-window X11 consumer. |
| Independent measurement and presentation | Layout composition and [shared_window_input.rs](../../integration/dockspace-host-harness/tests/shared_window_input.rs): both affine outputs, batch Presented and committed receiver admission | Native collection tests verify one eframe token, final-pass proof validation and one publication. |
| A to B in one stream | `one_window_stream_moves_between_scaled_placements_once_after_candidate_retry` and `captured_source_outside_both_clips_can_return_to_another_placement` | Physical pointer delivery and real widget selection belong to the X11 journey. |
| Child creation and first live | `first_live_child_returns_to_a_scaled_shared_placement_through_one_handoff_event`, plus the existing [native_surface_creation.rs](../../integration/dockspace-host-harness/tests/native_surface_creation.rs) Dropped/late-output and retry controls | The shared public fixture uses programmatic group tear-off; physical outside-all creation belongs to the X11 journey. |
| Child return and retirement | The same handoff test proves a partial payload return with a retained Child; [shared_window_retirement.rs](../../integration/dockspace-host-harness/tests/shared_window_retirement.rs) and close tests prove exact-lane draining and token reuse | Returning the whole Child and observing real window teardown requires the X11 consumer and existing editor lifecycle checks. |
| Placement/DPI and Root continuation | Layout tests for moving B, updated physical facts and complete continuation; input tests reject old-epoch terminals and accept fresh presentation | Real shared-window execution changes B's origin/scale and resets the Root. OS DPI remains one in that profile; DPI revocation is protocol-tested. |
| Atomic Allow/Defer/Veto and recovery | [shared_window_close.rs](../../integration/dockspace-host-harness/tests/shared_window_close.rs): all three decisions, exact recovery anchors, late callbacks and both quiescence orders | The shared physical consumer closes through the window manager. Its run does not independently certify physical Defer/Veto dialogs. |

The public tests also refuse stale placement artifacts, empty rosters, duplicate
physical facts, pending-close mutations and unavailable recovery. Typed
Blocked/Unknown/WindowBackground controls do not substitute for arbitrary real
framework overlay or foreign-window tests. Existing provider, first-live,
discarded-pass, output-terminal and capacity suites retain those component
obligations. In [native_admission.rs](../../crates/dockspace/src/engine/native_admission.rs),
`window_first_live_requires_each_exact_member_above_its_own_floor` rejects a
duplicate contribution, an old floor, another window and another placement;
`first_live_roster_rejects_partial_or_foreign_replacement_without_mutation`
rejects partial, duplicate, foreign and reordered replacement rosters.
The three real text probes separately check fixed shared capacity,
same-run roster expansion and switching to the ordinary Root entry; none
substitutes for a physical docking journey.

## Completion evidence

All sixteen final serial gates pass. Exact commands, environment overrides,
timestamps and exit status are retained in
[serial-gates.json](2026-11-11-shared-native-window/serial-gates.json).

| Gate | Final result |
| --- | --- |
| Core and conformance, all features and targets | 2,164 passed; three existing skips |
| Public persistence host, all targets | 417 passed |
| egui reference adapter | 186 passed |
| Official-egui consumer | 24 passed |
| Product consumer | 114 default and 119 persistence passed |
| Native workspace, all features and targets | 633 passed; one existing skip |
| Workspace and native Clippy | Correctness and suspicious checks passed; Linux-specific shared smoke checked separately |
| Workspace and native documentation | Passed with `RUSTDOCFLAGS=-D warnings` |
| Workspace doctests | 11 passed |
| Workspace, public-host and native formatting | Passed |
| Renderer-neutral dependency boundary | Passed |

Cargo runs are serialized with one build job, no incremental compilation and
the existing target directory. The final format-only repair affects the public
revocation fixture; its pre-format failure remains diagnostic evidence.
All eighteen real X11 gates pass, with exact commands and timestamps in
[real-x11-gates.json](2026-11-11-shared-native-window/real-x11-gates.json):
ten isolated actual-eframe tests, the bounded 4 MiB redock profile, new ordinary
and late-discard shared-window journeys, and the complete editor drivers, plus
their build/environment gates. The three new text probes check shared output
capacity, same-run roster expansion and switching to the ordinary Root entry.
The editor retains twelve real focused edits, six destroyed Child lifetimes,
the original Root through rebinding and final quiescence.

Both final shared-window logs retain B's completed origin `(450, 64)` and scale
`1.4`, followed by the distinct Root token which confirms current presentation.
The final independent review verified this proof closes the discarded-pass
readiness finding. The Linux-only consumer also passes targeted
[correctness/suspicious Clippy](2026-11-11-shared-native-window/linux-shared-smoke-clippy.json).

The physical environment is Linux X11, Xvfb/Openbox, eframe/Glow with software
rendering, one 1920x1080 screen and native DPI one. Placement A uses scale 1;
B uses 1.25, then moves and changes to 1.4. The reference adapter's physical
qualification stays distinct from public renderer-neutral host coverage. OS DPI
transitions, physical Defer/Veto dialogs, arbitrary shared-window overlay
negatives, other platforms and new ImGui timing results are not established by
this run.

The final [source manifest](2026-11-11-shared-native-window/source-manifest.json)
records every changed source input relative to the baseline. Diagnostic failure
logs remain in the evidence directory and are never counted as passed gates.
Terminal trailing whitespace and extra blank lines at EOF are normalized in
the stored logs; commands, diagnostics, test results and timestamps are retained.

Core and public-host implementation is committed locally as `270d81b`. Native
adapter and actual-window consumer implementation is committed as `3136d5e`.
The qualification documents and evidence are committed separately. No push,
pull request or upstream framework change is part of this work.
