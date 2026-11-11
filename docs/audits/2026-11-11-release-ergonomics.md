# Release ergonomics and native-host qualification

Baseline: `7555bea` on `refactor/egui-pane-ergonomics`.
Plan: [release ergonomics and validation](../plans/2026-11-11-release-ergonomics-and-validation.md).
The implementation stays local; no remote publication or upstream framework
change is included. The [check manifest](2026-11-11-release-ergonomics/checks.json) binds commands,
retained logs and source hashes. The [physical command record](2026-11-11-release-ergonomics/physical-commands.json)
retains the isolated display profile and exact actual-regression invocations.

## Interface and customization changes

Host-frame `*_current` convenience actions return the existing
`DockspaceSubmittedAction` identity. Applications can match each submitted
intent to its exact mixed-batch outcome through `submitted_action_outcome`,
including policy rejection and stale presentation. Submission acceptance does
not claim that the product action applied. Egui command documentation and the
README now make the same distinction between `Ok(result)` and its action status.
This intentionally changes the current-action return signatures.

Native applications can submit `NativeCommand::SetPolicy` through the existing
bounded application command slot. Complete policy replacement and the lock-only
convenience both use the original configuration owner and atomic frame commit.
Queued or dropped-frame policy stays unpublished; unconsumed terminal results
retain slot occupancy. Applications retain their own policy value when editing
rules incrementally. No second mutation authority or configuration queue was
introduced.

The presentation popup now uses the existing local-frame resolver and floating
visual overrides. Frame geometry, shadow, border width and corner radius still
come from the supplied egui UI. Rich titles retain explicit span colors in
tabs, overflow rows and drag ghosts. Corresponding docking text overrides win,
then local egui text overrides, then explicit spans and state defaults. The
original painter still applies disabled tint. `PaneView::title`, `minimum_size`,
`ui` and `PaneResponse` retain application state and content customization;
custom application menus use the existing command and close-decision paths.
No per-pane color registry or generic rendering callback was added.

## Module locality

Native mailbox preparation/output handling and its original unit tests now live
in private child modules of the original owner. Preparation methods retain their
lock scopes and bodies. The product harness separates presentation-menu and
output-authority proofs while retaining their existing test registration and
assertions. The mailbox parent shrinks from 6,299 to 4,443 lines; the product
parent from 6,525 to 4,898. This is a responsibility split, with no forwarding
layer or new state owner.

## Component verification

The host is macOS `aarch64-apple-darwin`, Rust 1.95. Independent workspaces use
`CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0` and their existing targets. Feature
configurations within a workspace run sequentially. Exact final command arrays,
exit codes and timing are retained in the manifests; earlier unchanged core and
host gates have their original logs.

| Gate | Result | Existing skips |
| --- | ---: | ---: |
| Main workspace, all features and targets | 2,356 passed | 3 |
| Minimal core, no default features | 2,057 passed | 3 |
| External host, default / serde | 377 / 432 passed | 0 |
| Product harness, default / serde | 114 / 119 passed | 0 |
| Official-egui consumer | 27 passed | 0 |
| Full native workspace, including smoke targets | 695 passed | 1 |
| Egui library after rich-title correction | 169 passed | 0 |
| Workspace / native doctests | 11 / 2 passed | 0 |

Workspace and native correctness/suspicious Clippy, strict documentation,
formatting, the core dependency boundary and diff whitespace checks pass.
Clippy retains unrelated existing style/pedantic warnings; this record does not
claim an all-warning-free repository.

Popup and four rich-title rendering cases have actual red/green evidence. The
current-action test first failed to compile against the former unit return;
its first runnable expectation was corrected to the actual
`PresentationUnavailable` after a preceding selection invalidated geometry.
Both complete host configurations then pass. Native policy tests cover exact
slot admission, no-op replacement, per-pane restrictions, lock preservation and
dropped-frame retry; no pre-fix red is claimed for that new command.

## Native qualification corrections

The capture regression originally reused a scene after a release had committed
a same-group reorder. It now remeasures using actual window geometry and each
surface's placement before executing and reporting paint. Its rebased case also
changes geometry by one logical point: an identical declaration is intentionally
a no-op and cannot prove a new placement generation. The original inequality,
receipt, capture, rollback and quiescence assertions remain. All three placement
profiles and both receipt-delay variants pass. This test uses real X11 capture
with recording presentation transport; it is not a GPU presentation claim.

The shared-window driver snapshots requested placement changes by exact native
output token. Its asynchronous physical driver can no longer change a declaration
between repeated passes of one output. An expanded scheduling window reproduces
the old `MultipassOutputChanged` and exercises the corrected path. Production
same-run declaration validation remains intact. A direct X11 pointer query also
avoids `xdotool --sync` waiting for a nonexistent motion when already at the
requested coordinate.

The final keyboard Alt+F4 shortcut occasionally returned without a native close
request. A failing observation had zero retained requests and a quiescent,
error-free runtime after five seconds; sending real `WM_DELETE_WINDOW` to the
exact Root then produced one retained request and natural exit zero. The driver
now sends that ordinary client message directly through x11rb. It still exercises
Winit close delivery and native request retention/Allow. It never destroys the
client with `XDestroyWindow`. Two instrumented scheduling-window runs and both
final clean ordinary/late-discard journeys return zero naturally. All temporary
backtraces, scheduling delays and diagnostic environment branches were removed.

Intermediate timeouts, a diagnostic-context blind spot for Child pointer logs,
and early runner argument mistakes remain separate evidence. One runner label
said bounded without setting the bounded environment; another passed `--discard`
instead of `--late-discard`. Neither is counted as the corresponding qualification.
The final explicit bounded profile and correct discard flag are recorded below.

## Final physical results and limits

All eighteen isolated native regression processes pass on the final library
source, including real original-press capture, retained focus, actual pass
identity, old-output retirement, shared transformed composition and seven text /
framebuffer-capacity paths. Each actual eframe event loop runs in its own process.
The capture test covers six recording-transport journeys internally.

The final eight-pane editor qualification completes twelve actual focused edits,
six Child lifetimes with destruction, first-live, Dock Back, OS-close recovery,
close/Allow/reopen, lock rejection and continued selection, default/preset/JSON
replacement, an actually drawn missing placeholder, reload and Root rebinding.
The original Root has no DestroyNotify and the runtime finishes quiescent.

The final command smoke completes nine docking commands and four style updates,
preserves a live Child identity and application pane data, and finishes presented
and quiescent. Both presentation-continuity profiles complete ten selections,
ten moves and four layout changes without blank or stale frames. Ordinary and
late-discard shared-window runs complete A-to-B, A-to-native-Child-to-B, the changed
B origin/scale, whole-Root replacement and ordinary WM close. The bounded redock
profile reports peak owned resident data 2,157,864 bytes and reserved data
3,274,816 bytes against its 4,194,304-byte limit. These are the protocol's charged
data counters, not total process RSS. The Linux non-window component suites also
pass. Runtime policy replacement is component-qualified; real editor lock and
style commands are exercised, but no additional physical per-pane-policy UI
journey is claimed.

The physical profile is Linux X11, Xvfb/Openbox, eframe/Glow, software rendering,
one 1,920-by-1,080 screen and native scale one. Shared Root placements use scales
one and 1.25, then 1.4 for the second surface. No new Windows, macOS, Wayland,
physical mixed-monitor DPI, multi-screen or ImGui timing claim is made.

## Independent review

Four independent static reviewers covered correctness, testing, reliability and
maintainability in run `20261111-175034-928a7eae`, with no confirmed P1/P2.
The [completed review receipt](2026-11-11-release-ergonomics/main-review.json)
records all plan items met and an empty validator batch.
The report retains sampling limits for the large mechanical diff. The physical
shared-window correction was excluded from that frozen source package and then
received its own independent review against final blob
`81f20dc068ea0d6f17d2d3d8972393ce13213b64`, also with no P2 or higher finding.
That review traced the actual pinned Winit/native close path. Reviewers did not
run Cargo or claim independent platform execution; the parent owns the results
above. A simplification pass found no worthwhile additional change.

Local implementation commits are `e14186e` (core identities), `c664733`
(native policy), `e6986c9` (egui custom styling), `ace755e` (module locality),
`1c02a4b` (capture fixture) and `b51e062` (physical driver). Final source hashes
in the manifest match those implementations. Direct protocol close qualifies
ordinary client-close handling, not Openbox's Alt+F4 key binding.
