# Dockspace behavior-spine validation

The canonical plan was revised on 2026-10-27 to prioritize `dockspace` as a
custom-engine library and egui as its interactive validation adapter. The
external public host consumer now lives in `integration/dockspace-host-harness/`.
Its recording-renderer results prove API and output-contract portability,
not real GPU or OS presentation. Open-GPUI integration remains deferred.

This document records current qualification and the historical U1 validation
baseline. Dated qualifications supersede the historical baseline below. It keeps
semantic journeys without turning the shared conformance crate into a GUI DSL.
Executors resolve their own current semantic bounds and platform facts. The
shared Rust catalog carries the finite L03 source, basin, normalized inset
samples, cadences, and normalized outcomes alongside the existing L08 semantic
steps. It does not carry screen coordinates, guide selectors, frame counts, or
another product authority.

## 2026-11-03 native interaction and visual continuity repair

Manual use after `63112e0` exposed an intermediate blank-frame path and
unreliable contained-tab redocking. The earlier lifecycle and final-layout
qualifications did not establish continuous visible content for those actions.
The [interaction review](audits/2026-11-03-egui-interaction-review.md) records the
source evidence, counterexamples and remaining platform boundaries.

The native driver now commits measurement before painting the current scene
within one application UI call. A measurement-only core frame does not draw a
replacement background or invoke pane callbacks. Ordinary ready scenes refresh
host bounds, titles and pane minima; active/pending manipulation and retained
scene-bound local actions keep their existing ordering constraints.

Contained content drags can resolve ordinary magnetic targets inside the
original floating rectangle. Window-title moves still require explicit targets,
and source-tree/occlusion exclusion remains core-owned. Exact declared pane
hover widgets can clear native docking hover. Every active click/drag identity
at that point must also belong to the exact presented receiver set, so a declared
label cannot hide an undeclared interactive blocker. The native basic example
declares its ordinary headings, log/profiler content and actual scrolling controls.

Fresh qualification of the repair:

| Check | Result |
| --- | --- |
| Workspace, all targets/features | 2,005 passed |
| Product consumer, all targets/features | 119 passed |
| Native workspace, all targets/features | 593 passed; one platform-specific test skipped |
| Official-egui consumer | 15 passed |
| Doctests | 10 passed |
| Core/native Clippy correctness and suspicious | Passed |
| Strict native rustdoc; three workspace format checks | Passed |
| Real macOS Glow, default and single-pass egui | Each passed ten selections, ten OS-window position changes and four host-layout changes |
| Real macOS child-window lifecycle | Default/bounded tear-off and redock, plus all six output-pressure profiles passed |
| Real X11 contained-tab tear-off and ordinary-pane redock | Passed; exact final `[First, Second]`, selected Second, contained presentation retired; 65 UI passes without a blank content pass or native error |

The continuity example fails before the native measurement repair at its first
selection. Both complete full-surface product redocking journeys fail before
removing the contained-exit gate. The declared-hover test and the independent
same-layer unknown-interactive counterexample each fail before their respective
repairs. Positive and negative hover cases pass with the final implementation.

The maintained real-renderer probes are:

```text
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml -p egui_dockspace_native --example presentation_continuity --locked -j1
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml -p egui_dockspace_native --example presentation_continuity --locked -j1 -- --single-pass
```

In an isolated X11 display with a window manager and `xdotool`, the real-input
journey runs with:

```text
cargo run --manifest-path integration/egui-fork-workspace/Cargo.toml -p egui_dockspace_native --example contained_redock_x11 --locked -j1
```

Its AccessKit coordinates only locate actual mouse input. The application checks
committed semantic ownership, current presentation and quiescence; it never issues
programmatic float/dock commands or creates synthetic native input/presentation
authority. The supplied xdotool process has finite deadlines and the driver thread
is joined. These probes check application content emission or explicit retention;
they do not claim pixel equality or GPU scanout timing. macOS physical cross-OS-window
promotion remains outside this checkpoint.

## 2026-11-03 semantic-state final qualification

The final tranche follows `ddd77a5` and completes the functional work in the
[semantic-state plan](plans/2026-11-02-dockspace-semantic-state-refactor.md).
The full P0–P5 scope is complete. The
[completion audit](audits/2026-11-03-semantic-state-completion.md) maps the plan's
phases and invariants to maintained tests and final-source receipts. Historical
receipts below do not substitute for this checkpoint. Implementation is committed
as `72a75d9`, `ddd77a5`, `3a294c6` and `68bb666`; no push or PR is part of delivery.

Touching-boundary resize freezes the exact complete layout projection. It merges
incident gap movement, propagates outer-edge changes through orthogonal splits,
and keeps non-touching siblings at their projected pixel sizes. Every affected
node's minimum and maximum constrain one shared displacement per axis. Pointer,
single-splitter and junction actions reconstruct one deduplicated `ResizeSplits`
transaction. Central leaves consume residual space; zero effective motion keeps
original weight bits. The former incident-only handle solver is deleted. A final
N-ary regression preserves a tiny positive far-side central child when f32 sibling
rounding exhausts its mathematical residual; its own positive weight remains
representable and the existing normalized-sum tolerance is unchanged.

Six public immediate/deferred regressions fail against the immutable `ddd77a5`
snapshot, then pass against this implementation. They exercise exact nested and
N-ary pane sizes, descendant limits, both central choices, four-arm junctions,
semantic adjustments, failed/abandoned candidates and exact sequence retry.
The first test attempts exposed two fixture assumptions (an occluded version-control
separator and a Ready paint plan after a complete release batch). Corrected fixtures
retain all geometric, weight and rollback assertions; final red/green records use
the same test bytes. Earlier fixture failures are not functional counterexamples.

Independent numerical review found strict zero overflow/unallocated checks rejecting
ULP residuals, plus min/max checks ignoring rectangle coordinate subtraction error.
Four maintained min/max endpoint tests fail before the fix and pass afterward on
both axes; large-origin actual overflow, unallocated space and compressed gaps still
reject. All 14 geometry tests pass after the final central-residual follow-up.
Batch command validation also changes from 64 complete root builds to one in its
64-split structural regression, while rejecting
a foreign member and a different frozen fingerprint in the same batch. The cache
exists only within that validation call; it is not another graph or authority owner.

Two additional independent public counterexamples are repaired:

- Closing A's popup while preserving B's valid output allows a new B smooth-scroll
  Begin/Update/End sequence without remeasurement. Existing scroll owners still end
  when semantic popup routing actually changes. The owner captures current routing
  from its validated journal presentation rather than B's historical visual plan.
- A retired logical surface can reattach and present its same-host headless successor
  before old outputs settle. Old Presented/Dropped results and exact stream reclamation
  cannot authorize, retire or disturb that successor. Native endpoints and other
  retirement causes retain their exact reuse guards. Failed/abandoned frames publish
  neither retirement nor presentation. A final foreign-host review additionally
  requires no active owner or a distinct same-host active successor before
  predecessor reclamation. Both proof preparation and consumption recheck this
  condition; two red negative tests now pass.

Local focused evidence is in `dockspace-touching-final-red-gates.json`,
`dockspace-touching-numeric-green.json`, `dockspace-semantic-final-red-gates.json`,
`dockspace-semantic-final-focused-green.json` and
`dockspace-semantic-final-green-batch-proof.json`, all under `/tmp`.
The final source inventory is `/tmp/dockspace-semantic-final-v2-source.json` (512
production, test, manifest, lockfile, README and tool paths). Qualification uses
one Cargo owner, one build job and serial tests. Linux has no nextest, so its
explicit fallback is `cargo test -- --test-threads=1`. MacOS uses nextest.

| Final-source gate | Actual result |
| --- | --- |
| Workspace, all targets/features | 2,005 passed |
| Public default / serde | 213 / 241 passed |
| Product serde / official egui | 117 / 15 passed |
| Native Linux all targets/features | 619 passed; 5 explicit ignored cases |
| Native Linux assertion-disabled library | 540 passed; same 5 explicit ignored cases |
| MacOS literal default entrypoint | 540 passed; 1 explicit opt-in skip |
| MacOS all-feature assertion-disabled library | 540 passed; 1 explicit opt-in skip |
| Tooling | Core/native correctness/suspicious Clippy, strict docs, ten doctests, core dependency isolation and five formatting workspaces pass |
| Matched extracted archives | Cargo verification passes; all 280 core and 36 adapter Rust files and both crate READMEs byte-match source; extracted public 213/241, official 15 and engine example pass |
| MacOS windows | Default/bounded redock and six output-pressure modes pass |
| X11 windows | Default/bounded redock, actual recover/retain/cancel close, six pressure modes, thirteen physical targets, four stationary modes and real Shift suppression/reacquisition pass |
| Explicit native opt-in cases | Original output, full-output capacity, actual press capture, close/create/age renderer boundary and release-mode wall-time gates pass separately |

Native normal/noassert suites include the same-coordinator 10,000-lifetime soak.
Broad-run ignores are not relabelled passed by the model suite; the separate
isolated runs above supply their actual platform/measurement qualification.

The 1 kHz physical load keeps its 900 Hz floor, 100 ms release bound and 4 MiB
storage allowance. Original 100 ms Docker CPU quota periods stall the injector
and fail cadence despite receiving all callbacks; those failed attempts remain
recorded. Instrumentation records 14 throttle periods and 346 ms throttled time.
A 10 ms period reduces gaps but remains below the rate floor. With the same
image, binary, total 1 CPU quota and 4 GiB memory, a 1 ms quota period passes
twice: injection 988.4/988.2 Hz, all 1,000 callbacks at 981.5/989.8 Hz,
release-to-model upper bounds 22.26/22.07 ms, and 3,534,000 reserved bytes.
No input burst, catch-up, code change or lower threshold is used. The successful
load profile explicitly requires its 1 ms quota period; longer-period injector
attempts remain unqualified. Other window gates retain their original container.

Final receipts under `/tmp` use the `dockspace-semantic-final-v2-` prefix:
`linux-gates.json`, `package-gates.json`, `x11-gates.json`,
`x11-isolated-gates.json`, `load-1ms-period.json`, `load-1ms-confirm.json`,
`macos-gates.json` and `format.json`. The independent-review receipt is
`dockspace-semantic-final-review.json`. The earlier v1 source and successful
receipts remain separate from v2's three changed Rust paths and requalification.
These finite records do not certify GPU scanout, unenabled platforms or general
cross-platform ImGui parity. Independent CI changes and `docs/reviews/` remain
outside this work.

## 2026-11-02 semantic-state refactor, first tranche

At this historical checkpoint, the
[follow-up plan](plans/2026-11-02-dockspace-semantic-state-refactor.md)
remained open. This tranche implements all nine executed counterexamples from the
[ImGui review](audits/2026-11-02-dockspace-imgui-review.md), plus independently
reviewed presentation/native interleavings. The following records describe this
tranche only. Production depth handling, dynamic logical surfaces, pending-mutation
consolidation and profiled transaction copying continue in the structural tranche
below; nested touching-boundary resizing remains open.

Changes:

- Preserve the last actually Presented output while settling later Dropped
  emissions, including separately committed and abandoned/retried prefixes.
  A fresh terminal preview still needs its own exact presentation before mutation.
- Delete the workspace popup rendezvous, duplicate proof map and separate gate
  revision issuer. Only the owner measures/paints the popup; current non-owner
  click or drag delivery dismisses it without activating content. Blocked, foreign
  and stale claims cannot dismiss it, and wheel delivery cannot scroll underneath.
- Delete presentation-identity rebasing on defaults/document replacement. Declared
  root, surface and floating IDs and their policies remain stable; epoch changes
  reject old actions, receivers and bindings, while allocation frontiers merge.
- Separate host and managed window-key namespaces in the existing registry.
  Host `u64::MAX` cannot exhaust child allocation; managed keys cannot be imported
  through root/owned-child registration. Exact binding/physical-route checks remain.
- Share manipulation-derived contained movement across content and commit preview,
  preserve oversized dimensions, and preserve durable split weights at zero delta.
- Scope manual scroll pauses to the affected strip. Rebind unchanged stationary
  work through an exact candidate successor with a new token and original deadline;
  changed geometry, old wakes and unpresented outputs remain ineligible.
- Keep staged preparation through unchanged durable observations. Actual readiness,
  coordinate, provider or lifecycle interruption revokes old output. Create and Show
  ACK settlement precedes readiness gating. Source recovery revokes all preparation
  phases; withdrawn batch actions are inert only with exact identity/retirement proof.
- The real W16/O256 pressure case exposed a distinct adapter bug: ordinary output
  saturation rejected an already-emitted lifecycle Show while reserving its terminal.
  Exact visibility callbacks now use their existing prepaid owner slot through ordinary
  draining; Frozen still rejects and quarantine still cannot dispatch a new Show. The
  maintained regression first failed on the original adapter path, then passed.
  The larger last-Presented record also requires 1 MiB for the fixture's 258 report
  slots rather than 768 KiB; the total allowance remains 4 MiB. Combined reserves
  3,794,808 bytes on macOS / 3,795,976 on X11 at peak and forwards all 258 real terminals before exact cleanup.

Qualification is serialized with one build job, no incremental cache, a shared
project target, and one nextest test thread. Debug symbols are disabled to fit the
host disk; the normal runs retain debug assertions. Native noassert uses both
`CARGO_PROFILE_DEV_DEBUG_ASSERTIONS=false` and
`CARGO_PROFILE_TEST_DEBUG_ASSERTIONS=false` and is recorded separately.

| Gate | Current result |
| --- | --- |
| Workspace, all targets/features | 1,970 passed |
| Public host, serde | 211 passed |
| Public host, default | 183 passed |
| Product host, serde | 117 passed |
| Native model, all features | 540 passed; one existing opt-in measurement ignored; includes 10,000 same-coordinator lifetimes |
| Native model, noassert | 540 passed; same existing measurement ignored |
| Clippy correctness/suspicious | Passed; ordinary style/pedantic warnings are not promoted |
| Strict workspace rustdoc | Passed with `RUSTDOCFLAGS=-D warnings` |
| Workspace doctests | 10 passed |
| macOS real-window checks | Default/bounded redock and all six output-pressure cases passed; Combined reaches W16/O256/control2 and retires all exact children |
| X11 real-window checks | Default/bounded redock, all six optimized pressure cases and finite physical guide drag-out/redock/Destroyed/quiescence passed |

The frozen Rust-file hashes and raw commands are recorded locally in
`/tmp/dockspace-semantic-first-tranche-frozen.json`,
`/tmp/dockspace-semantic-first-tranche-gates.json` and
`/tmp/dockspace-semantic-first-tranche-adapter-gates-v2.json`. The final adapter and
platform refresh uses `dockspace-semantic-show-pressure-qualification.json` and
`dockspace-semantic-first-tranche-platform-final.json`; the strengthened fixed-owner
regression has its own focused normal/noassert receipt. Failed compilation
attempts, full-disk link/strip failures and superseded fixture failures remain in
separate `dockspace-semantic-*` logs; they are not counted as passing tests. Only
rebuildable project caches/artifacts were reclaimed. Repository source, independent
CI edits and `docs/reviews/` were preserved. All dependency gates use tracked Git
pins from the existing Cargo cache with `--locked --offline`; no dependency path
substitution, push or PR is part of this tranche.

## 2026-11-02 semantic-state structural tranche

Implementation follows `72a75d9`; the structural tranche passes the qualification below.
Nested touching-boundary resizing was still open at this checkpoint; the final
qualification above closes it.

- One `Option<PendingMutation>` replaces the drag-release, contained-release and
  presentation-rehome Options. One concrete presentation tracker owns exact emitted
  keys, successful presentation and failure, retirement and retention. Each variant
  retains its semantic command and terminal result. Read-only review finds no confirmed
  presentation/cancellation regression against the first tranche.
- `attach_empty_surface_current` and `retire_empty_surface_current` use the existing
  workspace transaction. Empty central roots accept docking in the same frame;
  nonempty main/contained forests and native/recovery/reservation conflicts reject.
  Unrelated presented receivers remain usable. Abandoned and poisoned frames leave
  published topology, version and presentation unchanged. Reattachment keeps semantic
  IDs and the workspace epoch but requires new node/measurement/presentation proof.
  Changed surface requirements use the existing aggregate requirement revision;
  live per-surface state still prunes without adding a history registry.
- Independent review identified a reserved drag ID gap before native-create enrollment.
  Four real press/move/release regressions independently collide with the reserved
  surface or root during active drag and pending release. All four fail before the fix
  and pass afterward; each then delivers the original create only after its exact
  preview presentation. Existing reservation objects supply the conflict check.
- Product declaration compilation, clone, debug and destruction use heap traversal.
  Production scene minima use the shared iterative measurement once, then complete
  bounds-dependent maxima and project. Public 10,000-level declaration, error cleanup
  and complete measured/painted/presented contained-scene cases pass on the ordinary
  thread stack. The old recursive compiler/minimum path is deleted.
- Sampling the first complete deep scene found a repeated full-root fingerprint in
  pane tab-bar policy validation. Expected-pane validation already proves membership,
  owner and selection; the policy now uses that selected item directly. The same debug
  complete-scene test improves from 238 seconds to 56 seconds. This is a local sample,
  not a general linear-runtime or latency claim.
- Default native placement compares all current authoritative work areas by requested
  rectangle intersection, then nearest rectangle distance, then lowest token. Existing
  cascade/clamp follows selection. Thirteen public traces and four low-level cases
  cover gaps, overlaps, deterministic ties, scale, missing facts and stale/foreign input.
- Whole-engine candidates share the immutable canonical workspace through `Arc`.
  Checked mutations retain the existing owned staging copy and publish a new snapshot;
  other ledgers retain their representation. Deterministic counters distinguish shared
  workspace volume from explicit deep copies, and all core rollback/producer tests pass.

The opt-in public `frame_cost` example prints five samples per size with no timing
threshold. A Linux/arm64 optimized local run reports the following medians in microseconds;
concurrent host activity, compiler/runtime and fixture details limit comparisons.
These samples include the whole facade frame, not allocation-only or renderer time.

| Panes | Idle before | Idle after | One selected pane before | One selected pane after |
| --- | ---: | ---: | ---: | ---: |
| 16 | 19.25 | 19.21 | 70.68 | 70.82 |
| 128 | 81.17 | 74.29 | 505.21 | 498.87 |
| 1024 | 611.21 | 506.99 | 4820.95 | 4709.79 |

Pre-reservation-fix macOS workspace 1,984 and public serde 230 pass. They are not the
final source receipt. macOS product qualification then fails during compilation from
full disk; no test result is claimed for that attempt. Only rebuildable project Cargo
output is reclaimed, preserving source, dependency caches and independent changes.
Final frozen-source qualification continues serially with one Linux build job and
one libtest thread in the existing container target. That container has no nextest;
standard Cargo test is the explicit fallback. Exact locally cached registry archives
and sparse-index entries are reused without changing manifests or lockfiles.

The corrected frozen-source Linux runs pass workspace 1,988, public serde 230,
public default 202, product serde 117 and native normal/noassert 540 each. The five ignored native
cases retain their actual scope: three isolated real-X11/renderer cases, one
full-output-capacity renderer case and one opt-in wall-time measurement. They
are not silently counted as passed by the model suites. Strict workspace docs,
Clippy correctness/suspicious and ten doctests pass. Four workspace/harness
format checks and modified Markdown local-link checks pass.

The host disk was subsequently stabilized by reclaiming three inactive July/August
Cargo target directories inside this project's public/product/old-fork harnesses.
They contained no tracked files and no active build. No repository source, user
edits, reference checkout or dependency cache was removed. Docker was restarted
after its VM exited and the daemon returned EOF; all pre-restart receipts remain
separate from the following real-window refresh.

The final optimized X11 window refresh passes default/bounded redock, all six output
pressure modes and physical guide drag-out/redock/Destroyed/quiescence. The native
workspace uses the tracked remote egui/Winit pins with no path substitutions. MacOS
first-tranche window receipts remain separate; this structural slice's post-reservation-fix
window refresh is X11. The final plan qualification still refreshes enabled profiles
when the remaining touching-boundary resize implementation is frozen.

Source and command records:
`/tmp/dockspace-semantic-structural-final-snapshot.json`,
`/tmp/dockspace-semantic-structural-linux-final-gates.json`,
`/tmp/dockspace-semantic-drag-reservation-red-v3.log`,
`/tmp/dockspace-semantic-drag-reservation-green.log` and
`/tmp/dockspace-semantic-frame-cost-summary.json` and
`/tmp/dockspace-semantic-structural-x11-gates.json`.

## Remote fork publication and clean-checkout delivery

After explicit user authorization, both qualified branches are fast-forwarded
without force: Winit `dockspace-pointer-facts-0.30.13` from `f378b50f` to
`2664589f0bd6fd03b2a5490f23bcd692853e63dd`, then egui
`dockspace-native-seam-0.37` from `0716beee` to
`56e3db49b66096b58a23b8eb762bdc0a73ae043c`. New empty repositories fetch both
exact SHAs from GitHub and independently observe the same advertised branch
heads. The prior `not our ref` result is resolved. Intermittent SSL failures
are retained separately from missing-object failures. Egui's LFS lock query
also times out; the final push disables only that query after confirming no
changed file uses LFS. It does not bypass missing LFS content.

Commit `785e51b` changes only the three CI checkout refs. Commit `1611316`
generates Winit's ignored source-test lockfile once before locked commands;
`996fd4c` installs `libwayland-dev` before the Linux framework tests. The full
X11 run exposes two stale CI names under `monitor::tests`; both tests actually
pass under `monitor::notify_mask_tests`. Commit `542294b` fixes the two required
names and their module filter, with a separate literal CI follow-up. The
pre-existing full eframe-framework and cancel-close workflow changes remain
unstaged and are not claimed as committed CI coverage. The original independent
change is preserved.
Actionlint passes for the updated workflow.

Clean-checkout qualification passes from detached `996fd4c`, followed by the
corrected CI module/list and real-window gates at `542294b`. The root source
is cloned with `--no-local --no-hardlinks`; each platform starts with an empty
Cargo Git cache. Only crates.io registry data and existing target artifacts are
reused. Exact fork commits are fetched directly from GitHub without local object
seeding, alternates or dependency path overrides. A slow full-history transfer
is explicitly interrupted and replaced with a direct remote exact-SHA shallow
fetch; this transport choice is recorded rather than presented as a completed
ordinary full-history download. No root branch is pushed and no PR is created.
The environment and remote receipts are
`/tmp/dockspace-remote-delivery-environment.json` and
`/tmp/dockspace-authorized-fork-publish.json`. The current native command
checkpoint is `996fd4c`; the corrected module/list and window follow-up uses
`542294b`. Both have the same 500 frozen production/test/package paths as
`4f57bfb`, compared byte-for-byte in the main repository and clean checkout.

| Clean-checkout gate | Actual result and scope |
| --- | --- |
| Dependency source resolution | macOS and Linux each resolve all12 Git-family packages to the published pair in their initially empty Cargo Git caches. No dependency path override, alternate or local object seed is used. |
| macOS native default | The literal `python3.13 scripts/run_egui_fork_harness.py` entrypoint passes539 with one opt-in measurement skipped, including the same-coordinator10,000-lifetime soak. Native library check also passes. |
| macOS native default-feature noassert | The exact `--lib` command passes528 with one opt-in measurement skipped, including its10,000-lifetime soak. This is distinct from the earlier all-feature539 result. |
| Winit macOS facts | All8 pointer-route tests pass, including the7 required CI names. |
| Fork egui | Complete library passes113, including all8 required output-settlement tests and the completed-pass/elastic/nested-scroll cases. |
| Fork eframe/Glow | Host-seam54 and Glow deferred-clear5 pass with the committed feature sets. |
| Fork WGPU | The eframe WGPU host check passes. After installing the missing Wayland development dependency, the exact retained-color-attachment test passes1. |
| Winit X11 facts | The complete X11 filter passes80 with2 explicit isolated-platform tests ignored. All26 required X11 and7 macOS CI names are independently listed and confirmed executed successfully. The corrected four X11 module commands pass, including the actual `monitor::notify_mask_tests` module. |
| Linux native default | All-targets passes539 in total:528 library,10 interaction-authority and1 storage-profile tests. Five opt-in platform/measurement tests remain ignored. The library includes its10,000-lifetime soak. |
| Linux native default-feature noassert | Library passes528 with5 explicit ignores, including its10,000-lifetime soak. |
| Native artifacts and integrity | Native smoke binaries build, all68 smoke unit tests pass, native-workspace formatting passes, and the final committed workflow passes actionlint. All7 final workflow/list/module gates pass. |
| Remote-derived real X11 windows | All5 journeys pass: programmatic redock, recover-close and retain-close through real WM Alt-F4, physical guide dragging, and real Shift down/up followed by fresh motion, release, redock, exact DestroyNotify and quiescence. |

The initial source-fork component attempt fails because its ignored workspace
lockfile is absent; `1611316` generates it before subsequent locked commands.
The first retained-overlay attempt fails at `wayland-client.pc` discovery,
before any test runs. `996fd4c` installs the package and the identical test then
passes. Package-index and installation downloads use bounded network retries;
their slow transfer is recorded independently from Rust test results. The full
X11 log also exposes two stale required CI names; the diagnostic identifies the
exact passed replacements and `542294b` corrects both names and the module
filter. No test scope is silently removed to hide a failure.

Exact commands, results and resolved earlier attempts are retained in
`/tmp/dockspace-remote-native-qualification.json`,
`/tmp/dockspace-remote-native-v2-qualification.json`,
`/tmp/dockspace-remote-native-v3-qualification.json`,
`/tmp/dockspace-remote-environment-dependencies.json` and
`/tmp/dockspace-remote-ci-monitor-name-diagnostic.json`.
The macOS/Linux Git-package manifests are
`/tmp/dockspace-remote-mac-resolution-final.json` and
`/tmp/dockspace-remote-linux-resolution.json`; the eight actually executed
output-settlement names are checked in
`/tmp/dockspace-remote-ci-test-membership.json`.
Final command/name and real-window results are
`/tmp/dockspace-remote-ci-final-qualification.json`,
`/tmp/dockspace-remote-winit-ci-membership.json` and
`/tmp/dockspace-remote-window-qualification.json`.
The final source/provenance audit is
`/tmp/dockspace-remote-delivery-final-audit.json`.

This is local CI-equivalent reproduction, not a GitHub Actions run. macOS and
Linux are arm64 with Rust1.95; Linux uses the isolated one-CPU/4GiB Docker
environment, Xvfb/Openbox and software GL. It does not expand Windows/macOS
physical-input, Wayland, NativeLocal/Touch OS, deeper source ancestry or arbitrary
mixed-DPI claims. The10,000-lifetime runs are coordinator models, not10,000 OS
windows. The separately owned full-framework and cancel-close CI hunks are not
counted as committed workflow coverage. Removing only this session's four CI
commits in memory reproduces their original file hash exactly; the proof is
`/tmp/dockspace-remote-preserved-user-edits.json`. No root branch is pushed and
no PR is created.

All remaining U9 delivery checks are complete. The
[completion audit](COMPLETION_AUDIT.md) closes R1–R25, the acceptance examples,
implementation units and Definition of Done within the plan's declared profiles.
Historical qualifications below retain their original source identities and
resolved failures; the current remote-derived result supersedes their former
publication blocker.

## 2026-11-01 ordered magnetic suppression qualification

The completion audit's real Shift gap is now implemented through one semantic
targeting intent. `PointerEventMetadata::with_magnetic_targeting` carries an
edge's optional directive; omission preserves the current gesture and a new
press initializes the default. Modifier-only local controls use
`SurfacePointerEvent::MagneticTargetingChanged` in the existing host order,
without a pointer sequence, coordinate, receiver query or fabricated motion.
Native `ModifiersChanged` callbacks record one semantic input before raw
acknowledgement and use the existing shared budget. Reduction selects the active
owner; translation neither selects an owner nor fans out over devices.

Disabling removes only magnetic acquisition, preview and affordance. Explicit
guide/tab-gap targets, complete contained position and the source continuation
remain intact. Enabling permits a fresh real Move or Release to acquire; it
does not resurrect a preview or modify a pending release's frozen exact output.
Native controls require the same provider and an exact current binding which
is the source or the known keyboard-focus window. Retired, foreign and inactive
controls are inert. The existing transport preflight runs before allocating
the semantic Box. No second gesture owner, clock, registry or docking graph is
introduced.

Focused public3, native10, core-review2 and egui-product3 traces pass. The core
review cases additionally preserve complete contained pane/chrome geometry and
allow a focused Root to control a Child while an unfocused third binding is
inert. Independent review closes both findings and confirms that stationary
continuations cannot recreate magnetic preview. Capacity review finds no size
increase in the inspected arm64 PointerEventMetadata/EngineInput layouts;
actual native capacities remain dynamically charged and are rerun below.

The real X11/eframe/Glow journey passes on frozen source. XTest Shift_L down
clears the Root magnetic Dock emission while the original Child press/capture,
desktop point, native motion count and button count remain unchanged. Keyup
keeps it absent through two fresh inspection steps. Only a distinct safe real
Move reacquires a fresh Dock emission, followed by the same real Release,
redock, exact DestroyNotify, semantic retirement and quiescence. Painted
inspection emissions do not claim GPU presentation or direct core-commit
receipts; public/coordinator tests retain the exact output-barrier proof.

The first physical attempt already passed both modifier phases but its fixture
then incorrectly required full guides before a fresh real edge. A cleared
affordance need not emit those guides. The corrected fixture checks unchanged
geometry and absent preview before motion, uses previously observed guides only
to choose the safe point, then checks every newly emitted guide and preview
after that motion. It does not weaken fresh acquisition or inject another edge.
Both attempts remain recorded in `/tmp/dockspace-magnetic-physical-first.json`
and `/tmp/dockspace-magnetic-physical-v2.json`.

All23 library/tool gates pass on the frozen production source: workspace1958,
public163/serde191, product112/serde117, official15, native default539,
macOS native all-targets587 and assertion-disabled library539, and Linux
library539 both normally and assertion-disabled. Each native configuration
includes its actual same-coordinator10,000-lifetime soak. One opt-in measurement
remains skipped in nextest; Linux retains five explicit ignored platform/
measurement tests. Correctness/suspicious Clippy on core/macOS/Linux, strict
rustdoc, ten doctests, framework-dependency isolation, formatting and diff also
pass. Frozen source and commands/results are
`/tmp/dockspace-magnetic-qualified-source.json` and
`/tmp/dockspace-magnetic-final-qualification.json`.

Finite window refresh passes macOS default/bounded Redock, both platforms' six
output-pressure modes, Linux smoke unit tests, real X11 hover positive/InputOnly
negative, stationary overflow/exit-stop, guide/nested-edge/clipped insertion
and1kHz center input. The latter matches all1000 callbacks and measures a
35.400334ms release-to-model upper bound, with sampled owned/reserved bytes
2,358,993/3,513,528 under4MiB. These are finite observations rather than latency
guarantees. Shift also passes the complete real journey with native debug
assertions disabled. Results are
`/tmp/dockspace-magnetic-window-qualification.json` and
`/tmp/dockspace-magnetic-window-followup-v3.json`, supplemented by the normal
Shift result above.

Separately invoked real capture passes normally and assertion-disabled. The
close/create/age journey passes original press lineage, stale T1 rejection after
C2, distinct T2 identity, no premature selection, natural250ms freeze, delayed
q1/q2 close receipts, cancelled-create source preservation, exact A/B Destroyed,
output zero and quiescence. Its two cfg(test) oracles required migration: capture
queries now compare exact device/time independently of new targeting intent;
the WM-aware fork now supplies exact source-window capture/delivery instead of
Unknown. Both identity assertions are strict and retain original press/device/
binding checks. Independent review reports no P1/P2. A GUI-panic attempt was
followed by a T1 timeout; after isolated-display real button/modifier cleanup,
the final retry passes. Residual buttondown is a plausible cause, not an
independently observed fact. Original failures are preserved in the window
qualification, followup and followup-v2 records; successful capture also has
`/tmp/dockspace-magnetic-window-followup.json`. Only these two ignored Linux
test files change after the production freeze; the source receipt records them.

The package refresh passes all six matched-package gates. Both archives are
rebuilt; all275 core and36 adapter Rust files and both READMEs match source byte
for byte. The extracted public consumers pass default163/serde191, the engine
example runs and the official consumer passes15. Framework dependencies remain
registry-sourced. Core's ordinary package verification passes. Adapter's first
ordinary verification resolves an older cached registry core0.1.0 and fails on
the new API; a command-line patch attempt with the workspace's unchanged locked
graph also fails before mutation. Final verification deliberately checks the
extracted adapter against this same extracted unpublished core via an explicit
patch, then runs the independent consumers. Tarball production uses no-verify;
the subsequent matched check and tests supply verification. Neither repository
lockfiles nor registry cache contents are rewritten to disguise the mismatch.
This qualifies the paired unpublished artifacts, not availability of their
eventual registry release. Evidence, hashes and exact commands are
`/tmp/dockspace-magnetic-package-v2-qualification.json`; the original failed
verification and unused-patch diagnostic remain in
`/tmp/dockspace-magnetic-package-qualification.json` and
`/tmp/dockspace-magnetic-package-matched-core.log`.

The U4 Shift follow-up is qualified for these host and finite reference profiles.
Earlier qualification sections keep their historical source scope and are not
reused as fresh results for this production change. At this checkpoint, exact
remote fork availability, CI-ref alignment and clean-checkout reproduction were
unfinished, with no push or PR performed. The subsequent explicitly authorized
publication and remote-derived qualification are recorded above.

## 2026-11-01 completion audit and strict Burst/default-feature follow-up

The [completion audit](COMPLETION_AUDIT.md) maps R1–R25, AE1–AE14, implementation
units, resource budgets, named commands and delivery artifacts to inspected
current authorities and actual test receipts. All six core/conformance/consumer
Git trees match qualified `677f291` before the following product-test-only change;
all274/36 packaged Rust files and both READMEs still byte-match current source.
The source/tree/archive provenance is
`/tmp/dockspace-completion-audit-provenance.json`.

Independent review found that the existing complete product Burst executor
submitted all five edges in one packet but its preview helper could silently
send another PointerMoved if the first output lacked preview. Earlier passing
records therefore did not independently prove the stronger no-compensating-input
claim. The Burst branch now panics instead of requesting that fallback. Its
initial output must contain the exact magnetic preview; subsequent outcome
settlement sends only empty input. The original packet's completed egui run may
contain repeated passes, with raw ingress still consumed once. This is not a
claim that the first egui pass must be terminal.

The three affected L03 tests pass with default features and again with serde:
the supported-cadence matrix, the complete60-case matrix and the actual ordinary
button/editor/scroll/canvas matrices. Other tests are explicitly filtered out,
not relabelled as a fresh full product run. Exact source hash, commands and logs
are in `/tmp/dockspace-completion-strict-burst.json`. Independent call-chain
review confirms that the helper cannot add a run or Move, and fixture time is
absent so it cannot fabricate a timed continuation.

The plan's literal default-native script is also run independently of the prior
all-features qualification. `python3.13 scripts/run_egui_fork_harness.py` passes529
tests with default features, excluding the smoke package, including its actual
10,000-lifetime same-coordinator soak. One opt-in wall-time test remains skipped
in this entrypoint and retains its separate dated qualification. Evidence is
`/tmp/dockspace-completion-native-default.json` and its log. No production logic
changes in this follow-up. Continuing the default-profile audit then identifies
an actual remaining implementation gap: Shift is carried for scroll projection
but does not suppress magnetic dragging. The plan's explicit default suppression
must be implemented and tested; it cannot be inferred from target lock or scroll
tests. Remote fork availability, CI-ref alignment and clean-checkout reproduction
also remain open. The audit does not claim whole-Goal completion.

## 2026-11-01 real X11 stationary overflow steps

The finite reference-host journey uses `EGUI_DOCKSPACE_PHYSICAL_STATIONARY=overflow`
with the actual eframe/Glow host and XTest input. A fixed three-tab layout uses
400-point tabs; real tear-off and exact first-live leave Anchor selected and
Main inactive in the original Root. The current strip has 164 points of remaining
scroll range. One real held Child motion enters the right edge band after
subtracting overlay controls. The point is checked after physical rounding and
cannot overlap Close, scroll/menu controls or actual guides. The test reads the
configured line extent and interval rather than hard-coding timer behavior.

Three fixed diagnostic slots bind current FullOutput AccessKit stats to exact
Root/window NativeOutputTokens: baseline, first step and second step. Repeat
clears that pass's candidate; Terminal is marked after forwarding the original
affine finalization. Only the matching real `Presented` terminal, forwarded to
the native host, seals a slot. Tokens differ and completion ordinals increase.
The first fresh sample leaves offset0; subsequent independent samples produce
offset40 and offset80, with matching full-tab translation and unchanged viewport,
maximum and model selection. Each next sample starts after the preceding exact
Presented callback and at least one configured interval after the previous
callback's end. This reports renderer terminal forwarding, not a fabricated
direct core-commit receipt. Actual continuation uses the existing audited core
presentation barrier; its exact commit/rollback contracts retain library coverage.

The `overflow-cancel` control moves out after the first exact Presented step.
It observes the original native motion and then the matching held source UI
position before starting two quiet intervals. A later distinct exact Presented
output stays at offset40 and uses the same query. The probe remains active through
the terminal's viewport commands and the next fresh source-input barrier.
Any new query token, matching-source Release or capture reacquisition makes the
failure sticky. This closes two independent-review P2 gaps in the initial test,
which could stop observing before late commands and retain historical Acquired.
Both modes retain the real original press/capture and continue the same held
gesture through current Center redock, exact DestroyNotify, semantic viewport
retirement and quiescence. Slow UI suffixes and a genuine repeated output remain
part of each case.

The first real run failed because the new fixture expected explicit AccessKit
`selected=false`; the production renderer clears that field for inactive tabs.
The reader now follows that producer and separately checks exact public-model
Main/Anchor/Child selection. All 19 final frozen gates pass: macOS29/Linux60
physical-smoke unit cases, both-platform correctness/suspicious Clippy, positive
and stop-control journeys twice with native assertions explicitly enabled and
once with them disabled, plus hover positive/real-InputOnly negative, default
guide, manual clipped insertion and 1 kHz center regressions. Formatting and
diff pass. The two enabled-assertion positive runs measure conservative sample
intervals of 75.054/69.952 ms and 76.638/69.023 ms; the disabled run measures
79.303/69.542 ms against the configured50 ms. Quiet stop observations arrive
125.750–152.797 ms after native exit, with the actual two-interval timer starting
only after the later source UI exit barrier. These are observed finite timings,
not latency bounds. Both independent-review P2 findings are re-reviewed and closed.
Source and commands/results are in
`/tmp/dockspace-stationary-overflow-final-source.json` and
`/tmp/dockspace-stationary-overflow-final-qualification.json`, with the combined
scope/review receipt in `/tmp/dockspace-stationary-overflow-qualified.json`.
The three smoke Rust files are committed as `b35b8a1`; core and native
production logic remain at `0e0316b`. Final documentation also corrects the
public native document-restore comment to describe its already implemented
explicit Child declarations, and removes the obsolete single-surface-only
README claim. Strict native rustdoc, formatting and diff pass again in
`/tmp/dockspace-stationary-overflow-doc-close.json`; this is a documentation-only
change to the production crate, without another library/soak rerun.
Arbitrary strips, deeper source ancestry and unclaimed platform/Touch/native-local
profiles remain outside this finite qualification.

## 2026-11-01 final native dependency delivery check

The exact tracked fork pair is locally committed and selected by all 12 native
Git-family packages without path overrides. Remote branches still advertise
Winit `f378b50f` and egui `0716beee`, and clean temporary bare repositories cannot
fetch the new exact `2664589f` / `56e3db49` revisions: both return `not our ref`.
This is an actual remote dependency failure, not merely a stale tracking ref.
Evidence is `/tmp/dockspace-native-delivery-remote-heads.json` and
`/tmp/dockspace-native-delivery-clean-fetch.json`.

No fork is pushed and no PR is created. The separately owned working CI change
is preserved byte-for-byte. A reviewable three-ref-only proposal is prepared at
`/tmp/dockspace-native-ci-pin-update.patch`; it does not alter the working file.
Publishing the already committed fork branches, aligning those CI refs and
reproducing the native gates from a clean checkout remain the final delivery
boundary. The renderer-neutral/official-adapter archive qualification above is
independent. The overall Goal remains active until that boundary closes.

## 2026-11-01 capture API archive consumer refresh

The final core capture API at `677f291` is included in newly packaged and
Cargo-verified `dockspace` and `egui_dockspace` archives. All 274/36 Rust files
and both crate READMEs byte-match their current sources. Independent consumers
of the extracted archives pass public default160, public serde188, official
egui15 and the self-drawn engine example. Metadata keeps egui/eframe and their
framework families registry-sourced; only core and the official adapter resolve
to the extracted packages. Optional-native Git dependencies are outside this
archive gate. The retained `clipboard-win 5.4.1` yanked-lock warning is unchanged.

The first extraction driver used system Python3.9, which lacks the requested
tarfile data filter, after Cargo packaging had passed. Repeating with installed
Python3.13 completes all five gates. Exact archive hashes, extraction directory,
commands and results are in `/tmp/dockspace-capture-package-qualification.json`.
The dockspace archive SHA-256 is
`e6e9732c429eb08cd6881bd15e89bfaceb6e35dfc2eaa27bd9d29609f3294127`;
the adapter archive SHA-256 is
`bd0d1bb89e1e47a13f38e01b3dd29d34f573a2187bcfc8b93587f620485426d2`.
Registry publication and remote native-fork reproduction remain separate work.

## 2026-11-01 complete X11 stationary hover-selection journey

The fork-backed reference smoke now has explicit positive and negative stationary
modes. Both use the actual eframe/Glow host, real XTest input, prospective child
content and exact first-live presentation before the Child-source drag. They
preserve the required PassThrough source-input path. Capture Pending/Acquired
and pointer observations are forwarded unchanged by the diagnostic handler.
The positive case proves ROOT hover independently from original Child delivery
and capture. Its first fresh sample leaves Main inactive; a distinct later
sample selects Main while the same Primary remains held without intervening
motion/button events. The held gesture then uses the current Center preview,
redocks [Anchor, Main, Child], observes real Child DestroyNotify, waits for its
semantic viewport to retire and reaches quiescence. This is a full reference
eframe/GPU journey, unlike the earlier recording-receiver/simulated-transport case.

The initial test measured first callback to observed selected model. Independent
review showed the wrapper's 3 ms slow suffix could hide an early second sample.
The final test brackets each synchronous inner callback with begin/end Instants
and requires `second_begin - first_end >= configured_delay`, along with distinct
tokens and the later committed non-retained model. This conservative lower bound
cannot borrow time from UI or reducer delay. One strengthened run measured
171.94 ms between capture intervals and 191.32 ms to the selected-model observation;
these are different measurements and neither is a hard latency bound.

A precise temporary core mutation shortens acquisition restart by 100 ms. An
initial host-written mutation was not recompiled in the container and incorrectly
left the negative control green; that attempt is explicitly rejected as evidence.
The repeated control writes inside the container, asserts the mutated bytes and
requires `Compiling dockspace` in its build log. It reaches a real second sample
after about 85 ms and fails the fresh-interval assertion, as intended. The original
source hash is restored before rebuilding the final binary. Evidence is
`/tmp/dockspace-wm-short-dwell-actual.json`; the ineffective attempt and fingerprint
diagnosis remain in `/tmp/dockspace-wm-stationary-short-dwell-red.json` and
`/tmp/dockspace-wm-stationary-fingerprint.log`.

The negative case now creates an actual foreign root InputOnly blocker over Main
using a separate X connection. It validates real viewability and pointer-child
interception. Creation happens after the immutable exact Main motion is received,
before its original callback returns to Winit, so subsequent queries see the real
blocker without changing that input packet. An earlier UI-entry placement raced
the first query and failed the strict blocked-hover assertion on repeat; it is
removed. A genuine Observed result must retain the exact desktop point, Unknown
or Foreign hover and independently known Child capture/delivery. No query,
Unavailable or Unsupported cannot pass. Main stays inactive through matching
post-result model observation and real Repeat/slow-suffix pressure. The blocker
lives on the diagnostic probe until assertions finish, then is destroyed before
the same held drag resumes and redocks/cleans up. Known capture cannot repair a
blocked hover lane.

The frozen final qualification passes all 36 gates. Positive and negative each
pass twice normally and once without native debug assertions. The normal positive
runs measure conservative capture intervals of 179.327 ms and 179.393 ms; the
assertion-disabled run measures 183.084 ms. The macOS native workspace passes575,
and native libraries pass529 without assertions on both macOS and Linux, including
the unchanged same-coordinator 10,000-lifetime soak. The preceding same-pin Linux
normal library passes529 in
`/tmp/dockspace-wm-integration-native-first-gates.json`; its library source is
unchanged by the later fixture timing correction. Eframe libraries pass
macOS104/Linux99 on this Git pair. Final correctness/suspicious Clippy on both
platforms, strict native rustdoc, formatting and diff pass. Mac default/bounded
redock, all six pressure cases on each platform, physical guide/non-guide center,
nested edge, clipped overflow insertion and 1 kHz center load also pass.

Independent review closes the sample timing P2 and finds no further confirmed
P1/P2 in the finite test proof or blocker ordering. Frozen source is
`/tmp/dockspace-wm-stationary-source.json`; final commands/results are in
`/tmp/dockspace-wm-stationary-final-qualification.json`, with independent review
in `/tmp/dockspace-wm-stationary-independent-reviews.json`. Stationary overflow,
native-local/Touch OS input, deeper source ancestry and broader platform/product
criteria retain their own scope; this journey does not close the complete Goal.
Final metadata in `/tmp/dockspace-wm-stationary-final-metadata.json` resolves all
12 Git-family packages, including Winit's `dpi`, to the exact aligned fork pair.

## 2026-11-01 observed-parent X11 capture component

Local Winit `2664589f0bd6fd03b2a5490f23bcd692853e63dd` extends the finite source
profile to direct-root windows and clients below one root-observed parent.
Root SubstructureNotify is selected before owned window creation. The existing
window row retains only its immediate parent and whether that finite history is
available; refresh closes the existing mutation fence. Deeper, failed or cross-root
source history cannot issue a qualified original press. Native parent unmap,
reparent and destruction revoke matching source owners, while unrelated and
synthetic root notifications cannot retire capture. No second ancestry registry,
capture owner, issuer or window-manager policy table is added.

Hover discovery is bounded to 32 child-query steps. Intermediate replies only
name the next query object; the terminal reply itself must name a live owned
child and supplies its own root coordinates and button state. Unknown leaves,
subtree exit and depth exhaustion remain Unknown; failed queries are Unavailable.
This follows Xorg's [XIQueryPointer implementation](https://raw.githubusercontent.com/mirror/xserver/master/Xi/xiquerypointer.c),
which derives child from the current sprite's ancestor chain. Known routing
still identifies an owned subtree, not its deepest embedded foreign child.
Capture/delivery continue to require the exact active explicit duty, successful
fresh G2, original press and final native interval/FIFO checks.

The public ignored Openbox test confirms actual non-override-redirect
reparenting, one real press/master pointer, source-directed delivery while over
the peer, and ordinary Unknown versus fresh known source routes. A foreign
InputOnly sibling intercepts the sprite inside the same WM frame without changing
root child; hover cannot penetrate to the peer. Separate native root lifetimes
and a synthetic parent Unmap leave the original capture intact. Creating and
immediately dropping another owned window queues Map/Configure ahead of its real
Destroy; its exact Destroyed callback arrives once.

The private checkpoint test uses two actual presses in one loop. It unmaps only
the WM frame after the terminal position reply and after G2, proves the source
remains mapped but Unviewable, and checks revocation before any restoration.
Foreign frame restoration temporarily bypasses MapRequest redirection and
restores the original flag without calling a Winit mutator. A successful ordinary
sample after remap excludes hidden-source/queue excuses; the old press cannot
acquire or observe again. A second WM-controlled press exercises the other
checkpoint. Exact native cleanup remains on the original row.

Real execution exposed four implementation/fixture boundaries. Plain frame remap
was redirected and left it Unmapped. Existing XI NotifyUngrab alone did not revoke
the hidden source identity. Calling the old reset helper for an unknown root
window closed the global fence and masked missing parent matching; exact affected
filtering fixes that. Finally, dead-weak garbage collection during queued Map
discarded an owned terminal before Destroy, so the row now lives until its exact
native DestroyNotify. Precise parent-reset and unrelated-reset negative controls
fail as intended on the final tests. Synthetic Unmap and owned-terminal cases
also have observed red-to-green runs. The first parent-reset negative incorrectly
passed through the broad global reset; it is retained as a failed control, not
acceptance evidence.

Final frozen qualification passes 16 gates: real managed/ancestor cases normally
and without Winit debug assertions; macOS119/Linux119 pure cases; Linux91 library
cases with two native tests separately ignored; all four prior real X11 gates;
all-target compilation; correctness/suspicious Clippy; strict rustdoc; formatting
and diff. The original crossing fixture now locates a queued XI crossing using an
always-false XCheckIfEvent predicate, without consuming or reordering earlier root
Create/Map notifications. Both exact FIFO rejection and interval-history checks
remain. Independent source/parent/lifecycle review found no remaining confirmed
P1/P2 within this finite profile. Evidence is
`/tmp/dockspace-wm-pointer-qualified.json`,
`/tmp/dockspace-wm-pointer-source.json` and
`/tmp/dockspace-wm-pointer-frozen-qualification.json`.

Egui `56e3db49b66096b58a23b8eb762bdc0a73ae043c` pins that Winit revision and
passes macOS104/Linux99 eframe library tests. Native manifests and lockfiles now
select the same pair using locally seeded Git objects. The complete positive
eframe/GPU stationary journey and integration refresh pass the qualification
above; these component results do not close U7/U8/U9/U10/U11 or the Goal. Both forks remain
unpublished, and remote-only checkout remains unqualified.

## 2026-11-01 native capture acquisition and ordered release integration

This slice connects the finite platform operation to the existing core gesture,
eframe property queue, native window resource and ordered callback journal.
Core adds only the read-only `NativePointerCaptureRequest`: its opaque equality
includes the exact owner/session, source binding and frozen source-input
treatment, independently of hover targets or finite timed work. Pending accepted
input temporarily hides it and cannot prove native cleanup. Local, Touch,
unknown-device and invalid source streams cannot acquire this requirement.

Local egui/eframe `97ebef0412e07811dc070afeb44fa3f34dad52e0` pins Winit
`3199a442b10a8d6d5b85529599c4cdeb24df1320`. All eight native workspace patches,
smoke inspection and both relevant lockfiles select the same pair. Qualification
uses those exact Git objects seeded locally, without path overrides. Neither
commit is published; remote clean-checkout reproduction and independently owned
CI refs remain open. Eframe library tests pass macOS104/Linux99; Glow/WGPU
all-target checks and correctness/suspicious Clippy pass on both platforms.
Strict eframe rustdoc and formatting pass. The first minimal-feature docs run
found existing links to optional inspection/persistence items; the complete
documentation feature set passes without suppressing warnings.

Acquire and Release commands freeze the original real press, device, window,
action and opaque command identity. Eframe invokes each command once and leaves
Pending polling to the host. Window mismatch or missing active loop is explicit
NotExecuted, independent of backend Unavailable. Native reserves one shared
capture terminal slot through full commit and preserves the original callback
ordinal/time across duplicate, foreign, dropped-frame and retry paths. A fixed
hold in the existing resource row prevents retired cleanup backing from being
pruned after acquisition. A bounded two-slot model verifies exact release,
foreign-release isolation and backing reuse. Unexecuted cleanup with a possible
native duty retains that obligation until exact release or Destroyed; it cannot
manufacture quiescence.

The isolated Linux test uses two actual XTest presses, real Winit capture and
fresh source capture/delivery samples, recording core receivers and simulated
eframe receipt transport. It exercises Pending/full-commit retry, duplicate and
foreign receipts, one accepted acquisition event, real Primary release and final
resource/gesture quiescence. It does not run an eframe host or GPU renderer.
The first fixture incorrectly expected a continuation while native input was
pending after frame drop; its assertion now follows the public `None` contract
and also verifies the native duty remains retained.

Independent review found two real ordering defects. An acquisition unresolved
past the old deadline now defers ordinary observations at prepare/admit/reduce.
A query already queued before acquisition converts its first fresh facts to
CaptureAcquired even if the old ordinary deadline is due. Timing restarts from
the actual sample without selecting or scrolling. A newly settled acquisition
can rearm one previously blocked challenge; its failed sample is bounded again.
The Pending-deadline case failed on the original implementation. Temporarily
removing late-query conversion also fails the exact no-selection assertion;
the original source hash is restored before final qualification. Both real
journeys pass on the repaired source. Independent core, eframe and native reviews
report no remaining confirmed P1/P2 within this slice.

The core API is committed locally as `677f291`. Frozen broader qualification
passes workspace1,953, public160/serde188, product114, official15 and ten doctests.
The macOS native workspace passes575; its assertion-disabled library passes529.
Linux native libraries pass529 normally and529 without native debug assertions,
with five platform tests ignored in those broad runs. All four native runs
include the unchanged same-coordinator 10,000-lifetime soak. The real capture
journey passes separately normally and without native assertions, and the real
close/create/age journey passes independently. Those finite real tests do not
turn the other ignored platform cases into broad-run passes.

Root/native correctness/suspicious Clippy, strict rustdoc, core dependency
isolation, final formatting and exact Git-source metadata pass. The first Linux
all-target Clippy exposed an existing ambiguous `AtomEnum::NONE.into()` in the
physical smoke's property parser under serde. Explicit `u32::from` fixes that
comparison without changing parsing; the Linux all-target gate passes on retry.
Core and native library source hashes remain unchanged across this fixture fix.
Mac default/bounded redock and all six macOS/Linux pressure scenarios pass.
Rebuilt Linux physical guide, non-guide center, nested edge, stationary-negative
and 1 kHz load journeys pass. Broader physical target variants retain their
earlier source scope rather than being relabelled fresh.

Source hashes and commands are in
`/tmp/dockspace-capture-qualified-source.json`,
`/tmp/dockspace-capture-eframe-gates.json`,
`/tmp/dockspace-capture-core-gates.json` and
`/tmp/dockspace-capture-native-qualification.json`, with follow-up window/lint
gates in `/tmp/dockspace-capture-followup-qualification.json`, exact dependency
selection in `/tmp/dockspace-capture-final-metadata.json` and scoped independent
review in `/tmp/dockspace-capture-independent-reviews.json`. Ordinary WM-framed
source/hover, a complete positive eframe/GPU stationary journey, native-local and
Touch OS input, and the remaining U7/U8/U9/U10/U11 boundaries remain open. The
next WM test is an unqualified draft and is excluded from this source snapshot.

## 2026-11-01 finite fresh X11 capture sampling

Local Winit commit `3199a442b10a8d6d5b85529599c4cdeb24df1320` qualifies a finite fresh capture observation on the
existing exact device/press duty. Only an already Active explicit capture of a
viewable direct-root source may refresh the same-client XI2 Async/Async grab.
The observation takes Q1's one root position/hover reply, executes a real G2
regrab, then rejects pending native events and revalidates the original claim.
Success can publish source capture/delivery at that sample; ordinary None and
implicit-only queries retain Unknown. A refused G2 supplies no new proof while
preserving the original cleanup obligation. An indeterminate result, source
retirement or release keeps cleanup until that exact native duty finishes.

Sampling is a phase on the existing per-device row. Native expansion marks its
interval invalid before dispatch, so returning to an empty FIFO cannot restore
Q1. No additional registry, issuer or clock is added. Ordinary grab reservation
continues to reject before ungrab. Sample completion after Primary replacement,
failed cleanup and a newer release claim cannot authorize or release a
successor. Acquisition and sampling use the same production XI2 event mask,
including Motion/Button/Enter/Leave required to reject changed query facts.

The first native checks pass explicit known samples over source and peer,
strict Unknown ordinary/implicit/foreign-frozen samples, and all five real
presses' original cleanup checks. A separate ignored native library case uses
the same private observation implementation with two fixed interleave points:
after Q1 and after a confirmed successful G2. Production supplies a no-op
closure, with no dynamic hook registration. Source hide at either point rejects
the entire sample and keeps release Pending through dispatch completion;
ordinary source/peer confinement cannot ungrab it. A completed native-entry
guard also rejects Q1 despite an empty final queue, and a fresh query can recover.
The foreign InputOnly map changes the root query's child to that exact foreign
window between Q1 and G2. XPeekEvent confirms a selected XI2 Enter/Leave at the
head without consuming its cookie. The old Q1 is rejected while the crossing
remains queued, and normal dispatch plus a new query recovers. These private
checkpoints execute the same observation implementation as the public method;
they do not expose a production hook.

Final frozen-source gates pass macOS119 and Linux119 pure cases (the same
57 state tests in two wrappers plus five ordinary-dispatch cases), Linux91
library cases, all-target compilation, correctness/suspicious Clippy, strict
rustdoc and exact-path nightly formatting. Four real X11 gates pass separately:
three-press deterministic interleavings in 0.27 seconds, the five-press explicit
capture/freeze case in 0.47 seconds, owner mutation in 0.06 seconds and ordinary
position/hover in 0.34 seconds. Existing wrapper dead-code warnings remain.
The first final Clippy pass required a semantics-preserving Option `?` rewrite;
the whole affected gate set then reran on unchanged final source hashes.

Three precise negative controls fail as intended. Bypassing interval validity
accepts stale Q1; wrongly restoring a retired sample to Active violates automatic
cleanup phase/claim assertions; omitting the actual native-entry invalidation
causes the real interleave case to return double Known instead of Unavailable.
The temporary production mutation was hash-checked and removed before final
qualification. An earlier cleanup test explicitly called release and could mask
missing automatic retirement; its final version instead requires Retiring and
one automatic claim. Evidence is
`/tmp/dockspace-fresh-sample-qualified-gates.json`,
`/tmp/dockspace-fresh-sample-qualified-source.json`,
`/tmp/dockspace-fresh-sample-red-controls.json` and
`/tmp/dockspace-fresh-sample-red-native-entry.json`. Independent production review
reported no confirmed P1/P2; final assertion strengthening and real execution
were performed by the main verification owner.

These facts cover server-logical state at the sample, not physical button state,
uninterrupted old-grab history, future freedom from freeze, an atomic foreign
window hierarchy walk or full stationary behavior. An unknown root child,
including a window-manager frame, stays Unknown. The selected native workspace
still pins Winit `39a8165d` and egui `9729e354a`; it has no automatic acquisition
command and is not using this newly committed local sample implementation. Core
and adapter production source remain unchanged. Eframe/native acquisition,
exact release/retirement, the core acquisition event, non-direct-root platform
coverage and positive stationary journeys remain subsequent plan work. This
component checkpoint does not close U7/U8/U9/U10/U11 or the complete Goal.

## 2026-11-01 elapsed-time service coexistence and XI2 regrab cross-check

The finite elapsed-service fixture separates the release source, tokenless
close target and waiting native bootstrap into three logical surfaces. It
establishes the source through actual core Create acknowledgement, PreShow,
Show acknowledgement, PostShow and exact first-live acceptance before arming
the reorder. The source route must bind before a hidden snapshot can publish
its staging request; the initial fixture bound it too late and correctly
failed with OutputRouteAttachmentFailed. No production rule was weakened.

A cfg(test)-only callback cost advances the fixture's existing nanosecond
clock by 3 ms after real pointer translation. Clock reads remain pure; cloned
translation candidates share the same fixture clock. Five live turns each
receive sixteen idle records at one-millisecond intervals, and all five
exhaust elapsed time with record allowance still available. Reusing a spent
budget cannot claim another successor, append input or publish another version.
The same close/create owners progress in the third turn while refill continues,
with one stable child binding and preserved close-source content. After finite
drainage, pending external creation returns Wait rather than empty-frame polling.

The accepted release retains its original manipulation and affine output.
Returning that exact output settles its artifact once and yields one Dock
terminal after 35 ms on the controlled clock, preserving the pending
manipulation identity. The source tabs become `[Second, First]`; unrelated close
and waiting-child content retain their surfaces. Total controlled time is
95 ms, within the explicit 100 ms assertion, and measured/reserved input data
stay inside the selected 4 MiB profile. A fixed fixture observer on the existing
report-settlement boundary continues counting the exact manipulation and
artifact through all later service/close reports, drain and two final Waits.
Both counts stay exactly one; the test does not stop observing after its first
terminal. It counts artifact settlement, including retirement, rather than
claiming that the old preview remains current after docking. Deliberate source
mutation/stale-output rejection belongs to the existing separate regressions.

Release feedback is observed through the existing public core continuation
sharing the service turn's one budget before service consumes that report.
This is not service-only terminal observation, a real 1 kHz run, measured CPU
cost, OS/GPU latency or a hard real-time bound. The injected callback may run
past the cooperative 2 ms deadline; subsequent work must yield. This closes
the finite elapsed-time coexistence model, not arbitrary perpetual overload
or every runnable-owner/backend combination. Production layout and runtime
code exclude the probe and recording-renderer wrappers through cfg(test).

The adjacent test-only Winit commit `a4b051c4590fbef1f43ec0a7d3bdf4557f481e60`
extends the same five-press real X11 case. Same-client XI2 Async/Async regrab
using the production pointer mask succeeds before foreign freeze and returns
Frozen while a distinct client's paired keyboard freezes the actual release.
All Unknown observations and exactly-once release checks remain. The real case
passes in 0.46 seconds, with correctness/suspicious Clippy and nightly format
checks. The original claim cleans up first; an exact own-client/device guard
covers an indeterminate test-only regrab after that final press. No successor
grab exists in this cleanup path.

Regrab success supplies a server-execution-time freeze check. It replaces the
server grab and does not prove old-lease continuity, physical button state or
atomic consistency with a separate earlier position/hover query. Current
production pins and capture/delivery Unknown remain unchanged; positive
stationary input is still open. Local regrab evidence is
`/tmp/dockspace-x11-regrab-qualification.json`. Final affected gates pass 527
macOS library cases, 527 Linux library cases and the same 527 Linux cases with
native debug assertions disabled. The just-qualified 10,000-lifetime case is
excluded; production lifecycle code is unchanged. The isolated real O256 case
also passes with the final test-only layout, sampling
2,143,997/3,267,728 owned/reserved bytes. Correctness/suspicious Clippy,
formatting and patch integrity pass; existing advisory warnings are retained.
Final source hashes and commands are in
`/tmp/dockspace-elapsed-observer-qualification.json`. Independent review's
feedback-observation P2 is fixed and re-reviewed. Earlier focused/source
records remain diagnostic evidence, rather than final-source qualification.

The next finite sampling experiment must bracket a root position/hover query
with an actual same-client XI2 regrab, reject any queued native event through
the final reply, and revalidate the exact original active capture claim. It
must not drain events and reuse the old query. The production Enter/Leave and
Motion/Button mask is part of this prerequisite: a foreign hierarchy change
or thawed queued input must invalidate the sample before publication. This is
a proposed boundary, not implemented support or a complete protocol proof.

## 2026-11-01 combined lifecycle owners at O256 and foreign freeze counterexample

The existing W4/A1/E2/M4/L2 finite lifecycle model now admits 256 original
eframe output identities while retaining close predecessors/successors, a
cancelled but admitted create attempt, the exact dispatched single Focus owner,
and an accepted release. All output reservations are retained-framebuffer
owners without new affine paint. The real renderer supplies their identities
and original terminals; pointer, close, create, Focus and semantic presentation
facts remain model inputs. This is not a physical journey or 256 concurrent GPU
frames, nor an all-classes maximum profile.

Full occupancy rejects the next ordinary input and freezes B exactly at the
accepted release. The dropped release and overload candidates preserve that
boundary and their session state. Returning each original output terminal
removes only its exact reservation, decreases ordinary occupancy by one, and
does not append a semantic journal edge, alter B or settle another owner.
Subsequent create-failure, release-presentation, Focus and close settlement
retain the existing drop/retry and final-quiescence checks. Those subsequent
commits occur after output drainage; continued O256 occupancy during every
later commit is outside this case. The full state contains an already reserved
Focus callback; it does not promise admission of a new queued Focus at O256.

The all-feature Linux case passes in an isolated renderer subprocess. Its
parent kills and reaps an unfinished child on deadline, assertion unwind or
wait-query failure. A single inline shutdown output belongs to the fixture,
outside the tested bridge, so consuming eframe's later Close input cannot
prematurely freeze model admission. The four existing combined controls pass
separately; the unchanged 10,000-lifetime case is not rerun for this test-only
extension. Sampled owned/reserved data is 2,143,965/3,267,680 bytes under the
unchanged 4,194,304-byte limit, excluding fixture/process/renderer storage.
Initial attempts retain failed shutdown and queued-Focus assumptions in their
diagnostic logs; no production code changed to make this case pass.

The service layer also has a joint controlled refill case: 529 initial idle
motion records, then 16 more at one-millisecond intervals before each of six
turns. The first two turns exhaust the 256-record allowance. Reusing either
exhausted budget makes no further claim, record or empty-frame publication.
Refill continues while a ready create starts in turn 3 and close policy drains
in turn 4, with one unchanged child binding and bounded storage. The controlled
clock reaches 96 ms; both owners progress before refill stops. After drainage,
waiting for the external create fact returns Wait without publishing empty
frames. This tokenless close model checks policy progress and preserved source,
not successful OS Allow or destruction. Its first failing fixture incorrectly
expected policy approval to remove source content before Destroyed; correcting
that expectation needs no production change. Time remains frozen inside each
dispatch, so this is record-budget coverage and does not certify repeated
elapsed-time expiry or active-release latency under that load.

Winit test-only commit `beb11f6ccafe52337a736c470036644de02ff164` extends the
same real explicit-capture test beyond its original four presses. A distinct
X11 client successfully grabs the exact paired keyboard with a synchronous
paired pointer while the original explicit pointer effect is Acquired. An
XTest Primary release and injector-side round trip complete, but the root
query still reports logical Primary-down and Winit has received no release.
Checked AsyncDevice and AsyncPair on Winit's original connection leave that
foreign freeze intact. Both ordinary and expected-press observations keep
capture/delivery Unknown. Foreign keyboard ungrab delivers exactly one real
source/device release and retires the original press. The complete five-press
case passes in 0.39 seconds; foreign grab and injected-button cleanup use RAII.

The integration's production Winit pin remains `39a8165d`; the subsequent fork
commits change only this test and do not require a runtime repin. The core,
egui fork and native production sources retain their preceding qualification.
Positive stationary input, the elapsed-time service/deadline combination and the
remaining U7/U8/U9/U10/U11 criteria stay open. Current local logs are
`/tmp/dockspace-u7-owner-combined-real.log`,
`/tmp/dockspace-u7-owner-combined-model.log` and
`/tmp/dockspace-x11-foreign-freeze.log`. Final affected gates and source hashes
are recorded in `/tmp/dockspace-owner-followup-qualification.json`; the service
and isolated O256 logs use the `/tmp/dockspace-owner-followup-` prefix.

## 2026-11-01 serialized X11 capture effects and exact cleanup

The Winit effect seam is locally committed as
`39a8165dd434c33fc2f7198d922a4d3a4d76bbb6`; egui
`9729e354a1985cd9d8bbc7ca6306c68656635e06` aligns its internal dependency
and crates.io patch. The main integration patches, inspection dependency and
lockfile select the same pair. Both macOS and Linux now qualify those tracked
locked Git sources without path overrides, using locally seeded objects. Linux
uses only a shared target-directory override. Both commits remain unpublished.

Explicit acquisition and cleanup live on the existing device row and original
Primary identity. Ordinary grab mutations reserve that same connection owner
across checked ungrab, replacement grab, reply handling and mode publication.
Existing window modes retain indeterminate native results. Native calls run
outside the capture mutex. Late success, Primary-up with Secondary held,
all-up, device removal and unwind keep exact native cleanup duties. Confirmed
cleanup precedes ordinary takeover; stale claims cannot ungrab a successor.

New red/green regressions closed three release boundaries: queued presses
cannot qualify immediately after cleanup; reentrant sync/drain cannot open
admission during unlocked native release; ungrab cannot restore the original
automatic capture or held Primary identity. Dispatch begin and completion close
the existing FIFO fence. The released device stays Unknown until all-up and a
genuine new press; unrelated devices retain their implicit identity while losing
explicit attempts invalidated by the shared fence.

Fresh fork gates pass 105 pure tests (the same 50 state cases in two production
module wrappers, plus five ordinary-dispatch cases), Linux library84,
all-target checks, correctness/suspicious Clippy and strict rustdoc. The real
X11 explicit-capture case passes acquisition Pending-to-Acquired, ordinary-grab
rejection, persistent ordinary confinement, source motion over a peer,
Primary-up with Secondary held, automatic cleanup, stale releases and exact
successor release delivery. After explicit ungrab with Primary still held,
actual peer motion retains Unknown capture. Existing real owner-revocation and
fresh position/hover-query cases also pass. Glow100 and WGPU94 eframe library
tests pass locked/offline against the new Git source.

The affected native matrix passes normal572 and noassert571; normal includes a
fresh same-coordinator 10,000-lifetime case in 165.309 seconds, and noassert
skips that just-run case. Correctness/suspicious Clippy, strict native rustdoc,
workspace formatting, exact metadata source checks and core dependency
isolation pass. Linux library525 and the separately invoked retained-query,
close/create/age OS fixture pass. All 32 window gates pass: the Git-source
release build, library and lineage gates, default/bounded macOS redock, six
output-pressure cases per platform, and fifteen physical X11 entries.

The finite 1 kHz journey matches all 1,000 callbacks at 1,003.1 Hz after
999.8 Hz injection; release-to-model upper bound is 40.161959 ms. Complete
Root/Child/standalone service maxima are 0.556292/0.557625/1.365292 ms.
These measurements retain their configured profile and cooperative timing
scope. Core/public consumer source is unchanged from the preceding acquisition
timing checkpoint, so its workspace1,953 and public157/185 results are retained
as prior evidence rather than reported as newly rerun gates.

Evidence is `/tmp/dockspace-explicit-effect-fork-source.json`,
`/tmp/dockspace-explicit-effect-fork-qualification.json`,
`/tmp/dockspace-explicit-effect-egui-qualification.json`,
`/tmp/dockspace-explicit-effect-native-qualification.json`,
`/tmp/dockspace-explicit-effect-window-qualification.json` and
`/tmp/dockspace-explicit-effect-integrity-qualification.json`. The qualified
source inventory is `/tmp/dockspace-explicit-effect-qualified-source.json`.
The three release red/green controls retain their separate diagnostic logs.
Test-wrapper dead-code and
existing advisory Clippy warnings retain their local scope. `Acquired` reports
the settled operation effect; fresh observation capture/delivery remain Unknown
and automatic acquisition is not yet wired through eframe. Positive OS
stationary execution, full U7/U8/U9/U10/U11 and remote-only fork reproduction
remain open.

## 2026-11-01 native capture acquisition timing boundary

The core, native, real-window and extracted-package gates pass. Native desktop
continuations expose
the core-derived `NativeDragSourceInput` through a read-only runtime projection
and can return `capture_acquired_event()`. The new `CaptureAcquired(token)`
travels through the existing ordered native pointer journal. Core records the
first accepted acquisition time on ActiveDrag and rebases its current hover or
edge-scroll interval, without receiver queries, motion, selection, scrolling
or payload delivery. Native-local and touch requests expose no acquisition
event. Existing hosts with independently continuous capture need not use it.

An older target challenge may restart the current target within its original
desktop gesture; another gesture or a token issued before local-to-desktop
handoff cannot authorize acquisition. Accepted duplicates do not postpone the
interval again, including untimed duplicates. Missing first-acquisition time,
delivery or capture clears timed work without inferring owner loss. Known loss
still cancels; obsolete tokens cannot hide a current capture observation from
the interaction owner. Unknown-to-known recovery records the boundary without
restoring a cleared target. Frame drop keeps the original candidate retryable.

The nine new public cases and existing native stationary controls pass 22/22.
They include overdue dwell 240→390 ms, exact 389/390 ms execution, duplicate and
older-target handling, dropped-frame replay, foreign session/source rejection,
local-token handoff rejection, unknown and known-loss cases, obsolete-token ×
capture-loss intersections and exact edge-scroll output settlement. The ledger
case rejects acquisition-created/replaced streams and preserves atomic retry.
The initial edge-scroll fixture incorrectly assumed a narrow requested layout
would override native client geometry and that an edge always hit a tab body.
The corrected fixture reports a 300px physical/200-point logical client,
commits measurements before paint and registers the actual strip descriptor.
Hover-only clearance still requires that descriptor's exact current output;
click/drag delivery has no strip fallback. It retains the exact 40px step,
successor deadline and presentation barrier assertions.

Fresh final results pass workspace 1,953, public default/serde 157/185,
doctests 10, native 572 and native noassert 571. Normal native includes a fresh
same-coordinator 10,000-lifetime case in 165.273 seconds; noassert skips that
just-run case. Both workspaces pass correctness/suspicious Clippy, strict
rustdoc and formatting. The real refresh passes Linux library 525 plus the
separately invoked retained-query/close/create/age OS fixture, default/bounded
macOS redock, six output-pressure cases per platform and all fifteen X11
physical entries. The 1 kHz case matches all 1,000 native callbacks at 994.4 Hz
after 999.8 Hz injection; release-to-model upper bound is 40.968459 ms. Selected
Root/Child/standalone service maxima are 0.401/0.517750/1.420875 ms. These finite
measurements do not establish a hard timing bound.

Both packages verify; their 274/36 Rust files and READMEs match current source.
Extracted public consumers pass 157/185, official-egui 15, the self-drawn engine
example and dependency isolation. Package SHA256 values are
`96fb962eecaa057136a38760e46b85822ee48df36847dc5ccd6ea1b5b62bf752`
for dockspace and
`d76641c1736607a94069619293e54f23bd44eb6887e6905cd3913ee820a38883`
for egui_dockspace. Existing advisory lint/dead-code and yanked clipboard lockfile
warnings retain their previously documented scope.

Independent source review found and closed the old-capture precondition,
Unknown-delivery cancellation, local-token handoff and obsolete acquisition
capture-loss defects before final qualification. These are source reviews,
separate from main-thread Cargo execution. At this checkpoint the Winit
resource model was a separately unfinished working-tree draft, outside these
tracked Git-source native gates. It dispatched no grab/ungrab and reported no
new capture proof.
Ordinary application grab mutation and exact cleanup ordering must be settled
before that model can drive native effects. Positive stationary OS execution
and the overall Goal remain open.

Evidence is `/tmp/dockspace-capture-boundary-qualified-source.json`,
`/tmp/dockspace-capture-boundary-qualified.json`,
`/tmp/dockspace-capture-boundary-window-qualification.json`,
`/tmp/dockspace-capture-boundary-package-qualification.json` and
`/tmp/dockspace-capture-boundary-review.json`. macOS native gates use the tracked
unpublished Git pins without overrides. Linux uses the same committed Winit
`8334294b` exported into the isolated snapshot, excluding the unfinished model;
its exact archive and local override are recorded in
`/tmp/dockspace-capture-boundary-linux-baseline.json`. This is local-source
qualification, not remote-only reproducibility. The separately tested 47-case
draft model is recorded in
`/tmp/dockspace-capture-boundary-winit-model-qualification.json`; it also passes
81 Linux library tests and correctness/suspicious Clippy. Independent source
review confirms its exact resource, successor and sync/drain boundaries at
SHA256 `1e58ef0039f5f81b1f63373b49390bdbee3c96cddb40d2eac4754638d3f4a883`.
These model/library results are not OS capture or ordinary-grab coordination
evidence. The draft remains uncommitted until its actual dispatch owner is
integrated and qualified.

## 2026-11-01 source-specific physical drag capability requirements

The source, native, real-window and extracted-package gates below pass.
Internal `NativeDragSourceInput::{Preserve, PassThrough}` replaces the private
input-mode enum, and both `physical_cross_surface_drag(source_input)` and
`physical_native_drag(source_input)` now require an explicit profile. The
redundant `native_outside_all_tear_off` query and its coordinator wrapper are
removed. These are core implementation queries; the public host continues to
declare `runtime::NativeHostCapabilities` and supply independent facts.
Preserve does not require `PointerHitTestControl`, since it dispatches
no such effect. PassThrough still requires that control. Neither mode bypasses
inventory, desktop hover/position, button state, hit observation, exact source
delivery/capture, current presentation or placement requirements.

Core still selects the actual mode from source ownership and complete payload;
capability composition does not override that decision. The existing route
freezes that mode. Pending release retains `(binding, source_input)` before
routing ends, and the tear-off placement proof retains its original source mode.
Snapshot and close observation reconciliation compare old and new capabilities
under that same profile. No second route owner, lifecycle registry or authority
issuer is added.

The focused regressions cover External Root, partial Child, complete main with
contained siblings and complete contained content under Unknown/Unsupported
control; complete sole owned Child remains rejected. Active capability loss
and native pending release retain Preserve or cancel PassThrough with its typed
reason and exact restore obligation. Independent capture/delivery negatives
remain. Public handoff and stationary cases run with and without control and
retain stream/session, exact output, stale artifact and continuation checks.
The first focused run exposed a test expecting a workspace Delivered event at
native create request time. Its corrected assertion checks the exact emitted
NativePresentationRequested saga/effect/binding and unchanged source topology;
first-live transfer remains a separate gate.

Final qualification passes the 38 focused regressions, workspace1,952, public
default/serde148/176 and doctests10. Native normal572 includes a fresh same-owner
10,000-lifetime case in 168.654 seconds; native noassert571 skips that just-run
long case. Linux library525 passes with three explicit opt-in OS tests ignored
and the soak filtered; the real retained-query/close/create/age OS fixture runs
separately and passes. Both workspaces pass correctness/suspicious Clippy,
strict rustdoc and formatting. Other advisory Clippy and existing dead-code
warnings remain outside those enforced lint groups.

The real refresh passes default/bounded macOS redock, six output-pressure cases
on each of macOS and X11, and all fifteen X11 physical entries. Root never
enables whole-window pass-through; complete Child still requires exact Applied
enable and restore. X11 Combined retains O256/control2 plus W16, forwards all
258 terminals and finishes within resident/reserved2,375,289/3,490,072 bytes.
The 1 kHz journey matches all 1,000 native callbacks at 1,007.0 Hz after 999.9 Hz
injection, with a release-to-model upper bound of 25.610958 ms. Selected
Root/Child/standalone service maxima are 0.367416/0.346541/2.011792 ms; one
standalone sample exceeds the 2 ms cooperative target. These are finite
measurements, not a hard timing guarantee or positive stationary OS execution.

Both packages verify. All 274/36 archived Rust files and their READMEs match
the source; extracted public consumers pass148/176, the self-drawn engine
example and official-egui15. Core dependency isolation passes. Package hashes
are `a6f149e2c86f58fd2af86123b2b173bf5d4f850041fef9b8145729272144d29a`
for dockspace and
`75083b3c0183155bb42bdf7d18cdc865ba0210144e95d7a4f2da271df2296e69`
for egui_dockspace. The temporary extractor initially used a tarfile keyword
unsupported by the local Python; checked regular-file extraction resumed the
already successful package gate without rerunning physical checks. The
existing yanked `clipboard-win 5.4.1` lockfile warning remains.

Independent source review confirmed no P1/P2 in the frozen profile, pending
release, proof-currentness, no-route rejection or snapshot-loss paths. The
renderer-neutral audit found no confirmed core-owner conflict, duplicate
terminal authority or required public host API blocker. It corrected an
intermediate documentation mistake: the mode, composed queries and placement
proofs stay internal, while public hosts use `runtime::NativeHostCapabilities`.
This is an internal behavior correction, not a new public mode-selection API.
These reviews are source-only, separate from the main-thread execution above.

Evidence is `/tmp/dockspace-source-input-qualified-source.json`,
`/tmp/dockspace-source-input-qualified.json`,
`/tmp/dockspace-source-input-window-qualification.json`,
`/tmp/dockspace-source-input-package-qualification.json` and
`/tmp/dockspace-source-input-review.json`. The implementation extends `8d43aee`
without changing the aligned unpublished egui/Winit revisions. The next
protocol audit is `/tmp/dockspace-source-input-next-protocol-review.json`:
deadline acquisition cannot retroactively authorize earlier dwell; a fresh
lease needs its own accepted timing boundary and successor-safe release.
Positive stationary OS capture, delivery, ancestry and freeze proof, remote
fork availability and the complete Goal remain open.

## 2026-11-01 expected-press queries and source-window input preservation

The source, native and real-window gates below pass. Winit
`8334294b2a8b21cf68269d1af931a17e68d21559` optionally validates the original
Primary press before and after a query. Eframe
`687558fd469dc4849783c5ae3b9fd7244a0f92c5` freezes that identity in its existing
prepared/queued command. Results, callback keys and retirement remain Copy and
retain only their existing command identity. The adapter's existing Primary
row and Preparation/Attempt preserve the same optional identity. Admission
rechecks the frozen value; continuation conversion requires two present equal
identities. Ordinary queries without identity still settle their original
receipt, and cannot produce continuation input. Identity does not grant fresh
capture, delivery, ancestry or freedom from protocol freeze.

The real retained-result fixture exposed a separate core defect: dragging a
partial External Root payload enabled whole-window pass-through, so its next
real motion lost hover and revoked the newer continuation. The sole existing
route owner now distinguishes preserving native input from pass-through.
External Root, partial Child payloads, complete main roots with contained
siblings and complete contained roots preserve input. Only a runtime-owned
Child's complete sole main root without contained siblings enables
pass-through. A preserving route requires the exact current routeable binding,
receiving-input facts and no unsettled pass-through saga; cross-window/tear-off
use additionally requires independent source delivery and capture. Release
retains only its already validated pre-edge capture. Complete Child drags keep
the exact enable/restore acknowledgement lifecycle.

That correction also exposed missing quiescence ownership for a Create which
was cancelled before native admission. Its exact route now transfers under
the existing host lock to the same prepaid retirement store used for detached
workspace incarnations. Physical admission wins the same lock and prevents
this path from discarding an admitted/captured create. A dropped cancellation
candidate retains the original binding and retirement owner. No Destroyed
observation or create receipt is synthesized for a window that never existed.

A subsequent layout-reset regression exercises cancellation after the original
binding became noncurrent. Its first runnable failure is a source presentation
stream retained by core authority; `TerminalBindingGuardMissing` was the review
hypothesis, not the observed first failure. The original negative result now
enters the existing pending-result owner before exact quiescence enters the
same ordered ingress. Core accepts a cross-epoch definitive failure only for
the exact original unobserved Create/Replacement still awaiting appearance,
with no destructive cleanup successor. The immutable request epoch and
delivered provider remain ledger-validated. Foreign provider, wrong request
epoch, indeterminate result, observed window and existing destructive lineage
all fail without changing state. Normal and cross-reset shutdown regressions
and the direct core fence regression pass. The final full gates retain the
same source in `/tmp/dockspace-press-route-qualified-source.json`.

The actual X11 journey now retains real T1/C1 for original P1, commits a new
same-press motion C2, rejects late T1, and permits T2 through the Some-identity
gate while its Unknown capture/delivery cannot select a tab. Escape/all-up
returns to presented Root quiescence. The original delayed close/create/age
suffix still reaches exact A/B Destroyed, unchanged source and <=4 MiB storage.
The fixture never invents press identities, renderer terminals or native
callbacks. Its earlier C2 timeout and native PassThrough snapshot remain in
`/tmp/dockspace-expected-press-lineage-diagnostic.log`.

The optimized physical matrix passes all fifteen entries, including all
center/edge/tab/nested/overflow targets, stationary-negative and 1 kHz. Every
entry now rejects any Root enable across tear-off, redock and retirement;
complete Child still requires Applied enable and post-release Applied restore.
The final refresh matches all 1,000 native callbacks at 1,011.7 Hz after
999.8 Hz injection; release-to-model upper bound is 22.563041 ms. Selected
Root/Child/standalone service maxima are 0.273333/0.476875/1.242208 ms. These finite observations do
not establish a hard latency bound or positive stationary OS execution.

The final source passes core1,948, public default/serde148/176, doctests10,
native normal572 and noassert571, Linux library525, both workspaces'
correctness/suspicious Clippy and strict rustdoc. Native normal includes a fresh
10,000-lifetime case in 173.650 seconds; noassert skips that just-run case.
Linux explicitly retains three opt-in ignored OS tests and filters the soak;
the actual lineage OS test runs separately and passes. Earlier full
runs retain 45 and then two failures from superseded Root enable/ACK fixtures.
Their replacements assert input preservation while retaining first-live,
destruction, incarnation, release-output and bounded-capacity contracts. The
public handoff case changes insertion side after the first presented preview,
so the terminal mutation still waits for the new exact output. The final
checkpoint includes the never-admitted retirement and cross-epoch cleanup
corrections. A later full run's one old fixture failure is retained: a
late-appearance control had supplied a definitive dispatch failure, which now
correctly ends its appearance wait. It now supplies Indeterminate instead and
keeps exact late-appearance compensation assertions. The initial final refresh
also records an out-of-disk compiler failure, followed by removal of obsolete
project codegen caches and a successful rerun.

Final real-window gates include default and bounded macOS redock, all six
output-pressure cases on each of macOS and X11, the complete fifteen-entry X11
physical matrix and the retained-query/close/create/age journey. The bounded
macOS redock peaks at resident/reserved 2,334,809/3,488,912 bytes under 4 MiB.
The X11 Combined case retains O256/control2 plus W16 roster, forwards all 258
terminals and finishes at resident/reserved 2,375,289/3,490,072 bytes. No extra
platform, mixed-DPI or positive stationary claim follows.

Winit's unchanged query slice retains its 61 library tests, real expected-press
and ordinary-observation checks, assertion-disabled real query, Clippy and
format gates. Eframe passes 100 selected-feature library tests against exact
Winit Git source, Clippy, strict docs and formatting. Its first strict-doc
attempt with inspection/storage features disabled retained existing optional
link failures; enabling those documented features passes. Current outer
locked/offline metadata resolves one exact instance of every fork package
without path overrides, using locally seeded Git objects. Both commits remain
unpublished; remote-only reproduction and CI are not qualified.

Evidence is `/tmp/dockspace-expected-press-final-gate-source.json`,
`/tmp/dockspace-expected-press-core-complete-qualification.json`,
`/tmp/dockspace-expected-press-fork-query-qualification.json`,
`/tmp/dockspace-expected-press-native-completed.json`,
`/tmp/dockspace-preserve-route-lineage-real.json` and
`/tmp/dockspace-preserve-route-physical-qualification.json`. Those earlier physical
results precede the retirement/epoch corrections and retain that source scope.
The actual lineage fixture gains an explicit ReceivesInput assertion, removes
per-motion diagnostic logging and passes again in
`/tmp/dockspace-expected-press-final-lineage-real.json`. Final refresh source and
gates are `/tmp/dockspace-press-route-qualified-source.json`,
`/tmp/dockspace-press-route-qualified.json`,
`/tmp/dockspace-press-route-window-build.json`,
`/tmp/dockspace-press-route-real-qualification.json` and
`/tmp/dockspace-press-route-review.json`. Package consumers are recorded in
`/tmp/dockspace-press-route-package-qualification.json`. The complete Goal
remains open.

Both refreshed packages pass Cargo's all-feature locked/offline packaging and
verification. Extracted-package consumers pass public148/176, the self-drawn
engine example, official-egui15 and core dependency isolation. Official
consumer metadata retains registry egui/eframe/epaint/emath/ecolor; optional
native Git forks do not enter those packages. All 274/36 archived Rust paths
and both crate READMEs match the frozen source. The archive verification keeps
the existing yanked Windows transitive warning without changing that lock or
claiming Windows qualification. Archive hashes for this slice are:

| Package | SHA-256 |
| --- | --- |
| `dockspace-0.1.0.crate` | `fa8c5faf3078a5b71283a8e13176111cdf8eb0094eb5c4399e02b39e653197e9` |
| `egui_dockspace-0.1.0.crate` | `3f83da711e6d1fb5520fe6590372b34221ac22670af33c64f7e341c48b8161fb` |

## 2026-11-01 original Primary-press identity source

Winit `cd1a739a231248a5bb567db65ca64dcf890a9ab5` adds a Clone-only opaque
`PointerPressToken` to MouseInput, leaving PointerEventFacts Copy. One existing
capture row owns pending validation or the active press sequence. Its connection
domain remains alive with old tokens, so a recreated connection cannot match
an old sequence/XID/device. A necessary GetWindowAttributes request supplies
the complete 64-bit XCB sequence; only the still-current candidate on a live
viewable source can complete after the unlocked reply. Modifiers/hover
reentrancy is followed by exact identity validation before event delivery.

Primary release revokes active use before callbacks, even when Secondary
retains the implicit capture route. A later motion/enter Primary-up mask also
revokes it; missing masks, mutation, unmap, destroyed/reused routes and device
removal cannot restore it. Repeated press, non-Primary edges and failed
validation cannot supplement or replace identity. A release may echo only the
historical identity it ended. Other backends and model packets explicitly
report absence. No query API, positive capture/delivery fact or second
registry/clock is added.

Winit passes 61 library tests normally and without its debug assertions, the
real direct-root grab/unmap/remap fixture in both modes, all-target Clippy and
strict rustdoc/formatting. The real fixture also holds Secondary, releases
Primary with the original historical token and Window capture, then requires
a different token on the next Primary press. Native macOS all-target gates
pass 569 in both adapter assertion modes; Linux library passes 523. Their
unchanged 10,000-lifetime case is skipped because both configurations passed
earlier in this session; opt-in OS tests retain explicit ignored status.
Native Clippy and formatting pass. Existing modeled MouseInput constructors
only add `primary_press: None`, without changing production translation.

Egui `6e601d79b88baee69148e9a15496d4c2e12d081e` adopts the Winit schema and
passes its own 101 eframe library tests using the exact Git dependency and no
path override. Outer patches, inspection and lockfile agree on the new pair;
locked/offline Git metadata and all-target/all-feature compilation pass.
Registry versions and the official egui workspace are unchanged. These remain
unpublished local Git revisions, with no remote/CI qualification.

The optimized X11 center, stationary-negative and active 1 kHz journeys pass
after rebuilding this source. All 1,000 injected motions match native
callbacks, release-to-model upper bound is 24.402834 ms, and exact Destroyed,
retirement and quiescence follow. The selected active Root/Child/standalone
service maxima are 0.677167/0.895709/1.709166 ms; this finite sample is not a
hard latency guarantee. Stationary remains negative because identity does not
prove capture, delivery, ancestry or protocol thaw.

Source and results are in `/tmp/dockspace-original-primary-winit-source.json`,
`/tmp/dockspace-original-primary-adapter-source.json`,
`/tmp/dockspace-original-primary-egui-source.json`,
`/tmp/dockspace-original-primary-focused-qualification.json`,
`/tmp/dockspace-original-primary-qualification.json`,
`/tmp/dockspace-original-primary-real-qualification.json` and
`/tmp/dockspace-original-primary-git-qualification.json`. The first E0603
private import failure is retained; the qualified production change fixes only
that import relative to the independently reviewed implementation. Final Winit
diff SHA-256 is `f00dff7e1d37b714aa374de3abf8d24cc397f17b814a4584bac10665ea573208`.
Independent review finds no confirmed P1/P2 in issuance, revocation and schema
migration. Expected-press query/adapter propagation is the next prerequisite;
positive stationary input and the complete Goal remain open.

## 2026-11-01 archive refresh after cleanup and Git alignment

Both publishable archives are refreshed after the `237934b` Root-close/Focus
cleanup and current dependency documentation. Cargo packages and verifies core
and official adapter together, all features and locked/offline. The archives
contain the same 280/42 files and 274/36 Rust paths; every Rust file and both
crate READMEs match current source bytes. Their SHA-256 values are:

| Archive | SHA-256 |
| --- | --- |
| `dockspace-0.1.0.crate` | `06003a9646630f571e24b69151026213256d7430fa2db3de4d06dbc8eb6c38ec` |
| `egui_dockspace-0.1.0.crate` | `7c40279ea07442afd81c6aab33e2f24c49bf27a570704c5a00cffb1b7d9eeccd` |

Independent consumers depend only on the extracted packages. Public default
148, public serde 176, official-egui 15, the self-drawn engine example and core
dependency isolation pass. Consumer metadata confirms the entire egui 0.37
family remains registry-sourced; only extracted core patches the adapter's
core dependency. The temporary fixture reuses its previous paths for build
cache economy. Its missing locks and source files were reconstructed from the
exact current harnesses before testing; the earlier locked failure remains in
the log and is not counted as validation.

Evidence is `/tmp/dockspace-aligned-package-source.json`,
`/tmp/dockspace-aligned-package-qualification.json`,
`/tmp/dockspace-aligned-package-official-metadata.json` and matching
`/tmp/dockspace-aligned-package*.log` files. The yanked Windows transitive
`clipboard-win 5.4.1` warning is retained without a lockfile change or Windows
qualification claim. No crate publication, native remote reproducibility or
overall completion is implied.

## 2026-11-01 aligned native Git dependency sources

Local egui commit `aa54db470bed739062e00d6b1514015bbdd9c686` pins Winit
`3084fc09a7dca269d41d7aaa955369e365db6a71` in its workspace dependency,
registry patch and lockfile. All eight native egui-family patches, smoke's
direct inspection dependency and the shared integration lockfile select the
same pair. Only Git revisions/source IDs change; registry package versions,
checksums and dependency edges are preserved. The root/official/product
manifests and lockfiles retain official egui 0.37.0 and are unaffected.

The inner egui workspace passes locked/offline `eframe`/`egui-winit` library
checks with glow/X11 features and no path overrides. Full outer metadata places
every egui/inspection/default-font package in the exact egui Git checkout and
Winit/dpi in its exact Git checkout, with one instance each. The tracked outer
workspace then passes 569 all-feature/all-target tests across seven binaries,
without path overrides. Two tests are skipped: the opt-in OS fixture and this
slice's unchanged 10,000-lifetime case, already freshly run in both assertion
modes in the preceding single-Focus qualification. No additional soak or
assertion-disabled Git-source run is claimed.

Cargo's Git objects were seeded from the local fork repositories before these
offline checks. The commits are not pushed. This proves coherent exact Git
source selection on this machine, not remote fetch availability or clean remote
checkout reproduction. The separately owned CI checkout configuration remains
untouched and is not declared aligned or passing. The existing Python launcher
already uses tracked pins; no override switch or source-analysis script is added.

Evidence is `/tmp/dockspace-egui-inner-git-check.log`,
`/tmp/dockspace-native-repin-metadata.json`,
`/tmp/dockspace-native-repin-git-tests.log` and
`/tmp/dockspace-native-repin-git-qualification.json`. Independent review checks
both clean fork checkouts, exact metadata paths, five source-selection files
and unchanged official dependency bytes, finding no confirmed P1/P2. Earlier
path-override qualifications retain their historical source revisions. Remote
publication/reproduction, positive stationary input and full U9 remain open.

A subsequent detached local clone of `8b95744` has no `repo-ref` checkout or
path overrides and passes locked/offline metadata resolution. Its Git status
is clean before and after, with the same exact egui/Winit/inspection Git
sources. `/tmp/dockspace-native-clean-git-checkout.json` and
`/tmp/dockspace-native-clean-git-metadata.json` record this finite checkout
check. It uses the same locally seeded Cargo objects; compilation is not
repeated in the clone, and remote availability/CI remain unqualified.

## 2026-11-01 X11 local mutation capture revocation

Local Winit fork `3084fc09` moves the existing implicit capture state into one
XConnection mutex shared by event production and supported window mutation
paths. Grab/ungrab, drag initiation, immediate/deferred unmap and window Drop
invalidate retained capture evidence before request dispatch. A mutation guard
uses the existing native-event depth, holds no Display/capture lock while
sending, and closes admission again before releasing its depth on normal exit
or unwind. A server sync plus normal FIFO drain must complete before another
first press can acquire evidence. Invalidated devices also require observed
all-up. Native modifier/enter/wheel callbacks reread the sole owner after
reentrant application work.

The first implementation held a user Display lock across XCB dispatch. Source
review of libxcb socket handover and libX11 return_socket confirmed a possible
cross-thread wait cycle. The qualified implementation removes that lock;
XSync and XEventsQueued also run outside the capture mutex. Bounded production
guard tests cover nested mutation, sync completion before/after Drop, stale
empty-queue results, callback depth, unwind and failed fences.

An opt-in real XTest test uses a direct-root override-redirect source. It queues
release/press during the original press callback, performs a local grab
mutation, and requires the queued press to remain Unknown. Held unmap/remap
cannot restore capture; observed all-up and a new press restore it. The final
fixture fails precisely on the old `6c10c87d` source and passes on the fix
normally, without Winit debug assertions, and in two normal repeats. Winit
library tests pass 51/51 in both modes; all-target correctness/suspicious Clippy
and nightly formatting pass. Confined may succeed or be rejected after its
preceding ungrab: the test does not claim both branches or confinement success.
An earlier WM-managed fixture encountered a rejected grab and inconsistent
baseline activation; those logs remain, and its results do not qualify ordinary
WM-managed window continuity.

The joined native single-Focus source passes another Linux library run (523,
three ignored and the already qualified soak skipped), the actual close/create
age-boundary test, optimized late-staging/late-first-live/combined pressure,
pane-center and nested-edge physical journeys, stationary-negative and active
1 kHz input. Each checks its existing exact output/Destroyed/retirement boundary.
The 1,000 injected points match all native callbacks; release-to-model upper
bound is 34.742083 ms. Root/Child service maxima are 0.361917/0.380459 ms and
standalone service has one 2.144375 ms sample above the cooperative 2 ms
threshold. These measurements retain overruns and establish no hard latency
bound. macOS optimized late-staging, late-first-live and combined pressure also
pass against the frozen pre-X11 Winit source, qualifying the shared Focus
cleanup independently from Linux changes.

Evidence is `/tmp/dockspace-x11-capture-owner-source.json`,
`/tmp/dockspace-x11-capture-owner-qualification.json`, the final
`/tmp/dockspace-x11-capture-owner-baseline-direct-root.log` control and
`/tmp/dockspace-single-focus-capture-real-qualification.json`. Independent
review confirms the final six-path hashes and finds no confirmed P1/P2 within
this boundary. This slice does not prove ancestor unmap/reparent continuity,
arbitrary WMs, actual cross-thread Xlib/XCB contention, original Primary-press
identity or positive stationary capture/delivery. Queries still report those
missing facts Unknown. Native remote pins remain unreproducible; neither U7/U8
nor the full Goal is closed.

## 2026-11-01 removal of unreachable Root close and duplicate Focus queue

Production registration has forbidden RuntimeOwned Root windows since the
owned-Child contract: existing windows are External, owned registrations are
Child, and reservation rejects RuntimeOwned Root. Recovery and replacement
preserve those facts. The old `RequestRootClose` cleanup effect therefore had
no supported producer. This breaking cleanup removes both public effect
variants, the native CloseViewport branch and its injected legacy queue tests.
External Root held-close/Allow/Veto/Clear and Child cleanup retain their exact
protocols. Private cleanup now returns the existing PlatformEffect directly,
preserving exact binding, ownership, role and compensating-create checks.

Focus was the only reachable ordinary queue command. Its existing
`NativeFocusControl` already retained request, route, token and Queued versus
Dispatched state. Dispatch now consumes that single owner directly, removing
the second command Vec and mirrored route/state. Callback reservation still
precedes enqueue; failure retains the exact request's AdapterRejected result.
Retirement/reset preserve Queued ProviderStopped versus dispatched
AcknowledgementLost and keep original callback receipts through settlement.

`NativeStorageLimit::new` now takes `(input, byte_limit, resource_slots,
close_slots)`. The command-slot argument and backing reservation are removed;
one inline Focus owner is counted once. Its request/result budget and protected
callback receipt remain prepaid through the existing effect/resource owners.
Historical C0/C2 records below describe their dated source, not this API.

Frozen core qualification passes workspace 1,942, public 148/176, ten doctests,
strict rustdoc, correctness/suspicious Clippy and formatting. Native all-targets
qualification passes 570 normally and 570 without native debug assertions,
including both fresh same-coordinator 10,000-lifetime soaks. Linux library gates
pass 523 in each mode, explicitly skipping its unchanged 10,000-lifetime case
already run on macOS and leaving three opt-in tests ignored. Native Clippy,
strict rustdoc and formatting pass. The finite Focus mixed model still checks
production rejection, drop/retry, exact release delivery and late-token
isolation; its current sampled owned/reserved peak is
2,593,056/4,194,048 bytes under 4 MiB, with queued Focus peak one.

These native model gates freeze Winit at `6c10c87d`, with egui `8231fb5a`;
ongoing X11 capture changes have separate evidence. Source and commands are in
`/tmp/dockspace-unique-focus-core-source.json`,
`/tmp/dockspace-unique-focus-core-qualification.json`,
`/tmp/dockspace-unique-focus-core-followup.json`,
`/tmp/dockspace-unique-focus-native-source.json` and
`/tmp/dockspace-unique-focus-native-qualification.json`. The first native compile
retains its diagnostic for one omitted real-close call-site migration; the
qualified source fixes that call and retains canonical Allow plus Child
retirement. Final native aggregate SHA-256 is
`1323f5c704f8a6945917fba09c1581d1b9862ec390a92e8662cfe4f94f5eacc8`.
Independent core/native review found no confirmed P1/P2. This closes a bounded
deletion/ownership slice, not overall U9 or the complete plan.

## 2026-11-01 ordinary Focus ownership across the mixed boundary

`ordinary_focus_peak_one_survives_release_pressure_and_late_predecessor_callbacks`
extends the finite close/create/accepted-release model with one ordinary Focus
command. An owned child's real semantic rehome emits ReleaseChild and
RequestFocus; production `accept_native_effects` and
`dispatch_viewport_commands` populate and drain the actual ordinary queue.
The fixture models the resulting terminal callback, rather than running an OS
window. C2/E2 is configured, while the measured ordinary queue peak is C1.

The dispatched predecessor's protected callback remains outstanding across
ordinary ingress freeze, cancelled-create Failed cleanup and dropped/retried
frame candidates. Failed cleanup emits another genuine RequestFocus, which
production rejects through its existing single Focus owner without replacing
the predecessor token or losing the rejection input. Retiring that predecessor
allows the accepted release's exact output to deliver once and dispatch its
successor. Late predecessor and wrong-route callbacks cannot spend the
successor's owner. Canonical command retirement, close Clear, source layout and
full quiescence remain checked.

The final three-file source passes the new exact Linux test, all four affected
finite mixed models on Linux normally and without native debug assertions,
all four with macOS nextest, Linux all-target/all-feature correctness/suspicious
Clippy, package formatting and diff checks. The new model accepts 934 ordinary
records and samples 2,593,248 owned / 4,194,240 reserved bytes under 4 MiB.
The unchanged 10,000-lifetime soak and full core/native suites are not repeated
for this test-only extension and two `cfg(test)` read-only accessors.

Evidence is `/tmp/dockspace-focus-owner-source.json`,
`/tmp/dockspace-focus-owner-qualification.json` and the corresponding
`/tmp/dockspace-focus-owner-*.log` files. The final combined fixture SHA-256 is
`cf854ca94604c9471ba2e69730ad8872b3207de8a070351fc628bd03f77a2af9`.
Earlier fixture failures remain recorded: debug-stack overflow, a terminal
roster used for normal retirement, a stale hard-coded Root identity, missing
repaint after retirement, and an unhandled genuine recovery Focus request.
Bounded helpers retain the default test stack, and every emitted native request
is now consumed through production handling. Qualification freezes Winit at
`6c10c87d` independently from ongoing capture-owner development; egui remains
`8231fb5a`. This model does not certify real OS Focus callback interleaving,
two-command occupancy, positive stationary input or the complete plan.

## 2026-11-01 real close/create callbacks across an age boundary

The opt-in Linux test
`app::native_boundary_x11::real_native_close_create_age_boundary` runs the
production `NativeDockspaceApp`, Glow renderer, host and affine pass finalizer
under isolated Xvfb/Openbox. It uses W4/A1, E2, M4/C2/L2 and a 4 MiB combined
allowance. C2 is configured capacity, not a two-command occupancy claim.

A reaches real first-live. B receives an actual deferred create admission;
the fixture defers its first output, cancels the semantic saga and preserves
the original OS-attempt owner. XTest Alt-F4 produces q1's real WM close,
HoldClose and canonical Veto callback. The fixture retains that callback, then
requires a distinct q2 parked behind it. It also retains one actual Root
renderer terminal. With service and new UI paused, real Root motion ages past
250 ms. Initial freeze correctly requires `ContinueBoundary`; q2 is within the
previously accepted prefix, without claiming to read exact B.

Real standalone host service commits overload cancellation while all three
owners remain outstanding. Returning the original callback and terminal and
admitting B's actual first output then produces a production Created receipt,
exact A/B Destroyed observations, two quiescent retirements and unchanged
source ownership. Tabs remain `[3,5,6]`. Selected owner states satisfy the
combined storage queries and 4 MiB allowance. No token, callback, snapshot,
release or renderer terminal is manufactured.

Final zero-output observation is deliberately controlled by the fixture: it
routes the final real terminal's wake to GUI service, lets that production
service commit, then closes ordinary UI admission only after quiescence and
zero backlog are observed together on the GUI thread. It does not directly
clear an owner. This checks finite cleanup, not natural idle rendering.
It also does not qualify physical drag/release or the model's once-only
accepted-release delivery.

The final source passes the first corrected real run and three consecutive
repeats, one assertion-disabled all-feature real run, Linux all-target/all-feature
Clippy correctness/suspicious, targeted formatting and diff checks. The test
is ignored by ordinary suites because it requires an isolated X11 display,
XTest, `xdotool` and a WM with Alt-F4; these explicit ignored runs are the
positive qualification. Existing core and native suites keep their preceding
qualification; no new whole-workspace count is claimed here.

Earlier failures remain in `/tmp/dockspace-real-boundary-physical*.log`: the
first fixture expected Replay before the mandatory boundary; a repeated
`xdotool --sync` motion to its current point stalled; and driver-thread
polling either blocked the last settlement or raced a continuous Root output.
The final fixture uses two distinct real motions and GUI-service drain.
Evidence is `/tmp/dockspace-real-boundary-source.json`,
`/tmp/dockspace-real-boundary-qualification.json`,
`/tmp/dockspace-real-boundary-drain-repeat.json` and the matching logs. Forks
remain local egui `8231fb5a` and Winit `6c10c87d`. Broader command combinations,
positive stationary facts, remote reproducibility and the overall plan remain
open.

## 2026-11-01 core and official adapter archive consumers

The local U9 package gate at `7809e57` verifies both publishable crates in one
Cargo 1.95 invocation:

```text
CARGO_BUILD_JOBS=1 CARGO_INCREMENTAL=0 cargo package -p dockspace -p egui_dockspace --all-features --locked --allow-dirty --offline
```

Cargo packages and verifies both normalized manifests through its temporary
local registry. `--allow-dirty` permits the separately owned CI/review changes
and the ongoing native-only fixture; none changes either packaged crate.
The core archive contains 280 files, including 274 Rust files, and the adapter
contains 42 files, including 36 Rust files. Every packaged Rust file and both
READMEs match their source bytes. Archive SHA-256 values are:

| Archive | SHA-256 |
| --- | --- |
| `dockspace-0.1.0.crate` | `c63bb4bfff67208334e6c8eb54aa68df30ed65e287eb4f6e4abc1ffbab0c8022` |
| `egui_dockspace-0.1.0.crate` | `2c191bbd2cf48c366506d3bac02fb483acfa47d261c7cb73082355ead63db265` |

Independent copies of the public host and official egui harnesses depend only
on the extracted packages. The official harness patches the adapter's declared
registry core dependency to the extracted core; dependency metadata confirms
that the entire egui 0.37 family still comes from crates.io. Default public
consumer 148/148, serde public consumer 176/176, official consumer 15/15, the
`engine_host` example and the core dependency-boundary guard pass. Both Cargo
package verifications pass. No additional full-workspace or native run is
claimed for this documentation-only checkpoint.

Evidence is `/tmp/dockspace-u9-package-source.json`,
`/tmp/dockspace-u9-package-qualification.json`,
`/tmp/dockspace-u9-core-adapter-packages.log` and the corresponding
`/tmp/dockspace-u9-package-*.log` consumer logs. The package command retains a
warning that lockfile dependency `clipboard-win 5.4.1` is yanked; it has not
been silently updated or qualified on Windows. These local archive checks do
not publish a crate, certify a clean remote checkout of optional native forks,
or complete U9/the overall plan.

## 2026-11-01 cancelled create cleanup with an accepted release

The next mixed-owner model exposed a real lifecycle gap. Once a create saga was
cancelled, its original dispatch still had an exact callback/cleanup owner. A
late Failed result reached the effect ledger, but the cleanup reducer indexed
only destructive cleanup effects. The original create ID was absent from that
index, leaving `may_reappear=true` forever and no terminal binding guard when
the recorder later reclaimed its quiescence prefix.

The existing cleanup owner now recognizes its exact compensated create on
DispatchFailed/Unsupported, only before any window observation or destructive
cleanup lineage. It clears the appearance possibility and reuses normal
drive/Finish, staging release and provider-bound guard publication. Unknown
outcome, a different create, an observed window and an existing destructive
predecessor keep their obligations. There is no second index or lifecycle
registry, and a missing terminal guard remains an error.

Two core state regressions and both finite owner cases pass. The W4/A1, E1,
M4/C0/L2 mixed case retains two close successors and the cancelled create's OS
attempt, accepts a local Primary release before fixed freeze B and then fills
ordinary ingress. Dropped release/cancellation/Failed/presentation candidates
retry exactly. Cancellation finishes without waiting for those owners. An
unrelated surface's Presented output cannot deliver the release; only its exact
output produces one Delivered(Dock) with the same manipulation ID and reorder
`[3,5,6]` to `[5,6,3]`. Cleanup and quiescence finish under the same 4 MiB bound.

The initial fixture attempted a new gesture during a still-active create saga
and was correctly rejected by PresentationTransitionPending. The corrected
scenario cancels that semantic transition while retaining its genuine pending
OS-attempt owner; it does not weaken the gesture gate. The subsequent missing
guard failure and backtrace are preserved in
`/tmp/dockspace-mixed-service-focused-guard-backtrace.log`.

All 24 affected final checks pass: workspace 1,942; public 148/176;
product/official 114/15; native 571 in both assertion modes; Linux library 525
in both assertion modes; no-default compilation, Clippy, strict documentation,
10 doctests and formatting. The unchanged 10,000-lifetime soak is not repeated.
Optimized macOS and X11 builds and their late-staging/late-first-live real-window
journeys pass. Those existing journeys preserve their original compatibility
scope; they are not the new mixed close/create/release OS acceptance.

Evidence is `/tmp/dockspace-cancelled-create-qualification.json`,
`/tmp/dockspace-cancelled-create-real-qualification.json` and
`/tmp/dockspace-cancelled-create-source.json`; focused results are in
`/tmp/dockspace-cancelled-create-focused.json`. Independent review found no
confirmed P1/P2 in the constrained fallback and its tests. Explicit local forks
remain egui `8231fb5ab0f3` and Winit `6c10c87da2a0`, not the tracked remote pins.
This model uses test close
cancellation and create admission/Failed, plus real core affine painted outputs.
It does not certify canonical fork-held close and actual OS-create callbacks
crossing one freeze boundary, nor does it cover nonzero ordinary command slots.
Those native acceptance boundaries, positive stationary input and U9 remain open.

## 2026-11-01 complete host service during the active 1 kHz journey

The physical X11 runner now selects service samples by their entry time between
the first matched native motion and the original native Primary release. Each
accepted output retains one admission start through `before_output_input`, UI
and its last successful Terminal finalizer. Repeat keeps that start; a later
unwind after only Repeat is incomplete, not a successful terminal sample.
Eframe exposes the frozen disposition through a read-only accessor so a wrapping
host forwards the same affine finalization. Admission/input Defer, absent final
passes and standalone `on_service` work are reported separately. Pending
diagnostics are bounded to 32 entries, with backing reported outside the runtime
input budget. An admitted tail can finish after release.

All 57 Linux physical-runner tests and Clippy correctness/suspicious pass. The
optimized real X11 journey matches all 1,000 callbacks at 1,004.7 Hz after
999.7 Hz injection with no catch-up. Release-to-model is bounded by 20.874 ms;
the sampled root UI maximum alone is 0.106 ms. Full Root, Child and standalone
service each have 25 actual samples, with maxima 0.329, 0.176 and 0.985 ms. There
are no missing final passes, deferred samples, duplicate admissions or pending
diagnostic entries in that run. No Repeat or Defer happened in this real trace;
controlled-time regressions cover those branches and the interrupted-Repeat
negative case. Input owned/reserved peaks are 2,338,009/3,492,512 bytes under
4,194,304 bytes. Diagnostic backing is 3,976 bytes, separately reported.

These are elapsed observations for the stated finite active profile, including
scheduling and wrapper overhead. They exclude rendering/terminal delay after
the finalizer and do not prove a hard OS-loop yield or 2 ms wall-time guarantee.
The initialization/atomic overruns in earlier pressure results remain valid.
The adapter now also requires active Primary for a stationary device lookup;
Secondary alone, including after Primary release, grants no continuation query.
The focused native Primary/stationary regression set passes 10/10. Positive OS
stationary capture is still unavailable.

Existing native regression cases pass 570/570 with assertions and 570/570 without
native assertions, with Clippy and formatting. These runs exclude the unchanged
10,000-lifetime soak and the new mixed-owner test that is still failing; the
separately qualified wall-time profile is not repeated. The unchanged core and
public-host library retain their preceding source qualification.

Evidence is `/tmp/dockspace-active-service-source.json`,
`/tmp/dockspace-active-service-native-qualification.json`,
`/tmp/dockspace-mixed-service-linux-final.json` and the matching qualified logs.
Eframe's accessor is local fork commit
`8231fb5ab0f37b72684f1c0f7807716be16c877e`, with unchanged Winit
`6c10c87da2a0`. These explicit local overrides do not qualify remote tracked
pins. The mixed close/create/release model was still under investigation at
this service checkpoint; its subsequent fix and qualification are recorded above.

## 2026-11-01 application Default and named presets after Root continuation

The example following `c011e98` now restores its immutable Default using the
initial layout's incoming host identity. Each bounded named preset stores its
JSON alongside the Root identity from the same saved layout. Both preserve the
ordinary same-surface APIs and explicitly request Root continuation when the
current host differs; they do not parse the document or predict rebased IDs.

All eight affected checks pass: native Clippy correctness/suspicious and
formatting, Linux basic/persistence builds, both original six/eight-edit X11
workflows and both extended eight/twelve-edit workflows. The extension executes
the actual application Default and named-preset methods after the Root changes,
waits for fresh presentation and then edits the real focused TextEdit. The
persistence path traverses surfaces 1, 1000, 1001, 1002 and 1003 in the observed
run; these are observations, not an ID allocation contract. All eight pane IDs,
tab order, selected items and text survive. An independent X11 connection sees
the same original Root throughout without DestroyNotify; all two/six Child
lifetimes retire before final quiescence. Static independent review found no
confirmed P1/P2 in the five changed example/script paths. The first Clippy
attempt exposed an inferred closure lifetime; explicit input types fixed it
before these final checks.

Evidence is `/tmp/dockspace-example-root-presets-source.json`,
`/tmp/dockspace-example-root-presets-qualification.json` and the corresponding
`/tmp/dockspace-example-root-presets-*.log`. Only example and executor sources
change, so this qualification reuses `c011e98`'s unchanged core/native library
evidence below rather than claiming a new full-workspace run. Local fork
overrides remain explicit. Positive stationary OS facts, combined capacity and
service cases and U9 delivery are still open.

## 2026-11-01 external Root continuation across logical surfaces

The slice following `ba10cb6` adds explicit native layout/document replacement
which continues one exact external Root on another incoming logical surface.
The source surface may disappear. Rebase resolves the incoming Root and saved
Child recovery/placement references before preparation. Existing registry
reconciliation mints the successor and keeps exact predecessor terminal guards;
it creates no Root window, Destroy effect or synthetic Destroyed observation.

Core proposed/committed continuation pairs feed the native bridge's existing
resource preparation. It validates the actual physical window and awaiting
receipt before commit, installs its exact successor route, and publishes the
application's root-surface field under the same bridge lock. Predecessor
resources match complete successor identities, not surface/token heuristics.
Their late output, property callbacks and producer quiescence stay isolated.

Focused core and public document paths pass. The first independent review found
an unconditional rebase-map field with a serde-only import; default compilation
is repaired and passes. Adapter review also found an occupied target could turn
ordinary command rejection into runtime shutdown; prequeue conflict checks and
a typed RootContinuationRejected terminal now preserve normal operation. New native model
fixtures were corrected to supply configured capabilities, measure/present source
authority and use the ordered managed reset boundary. Those failures are retained
in `/tmp/dockspace-root-continuation-focused-*.log`. Prequeue occupancy checks now
apply only to a still-active logical target: an old route for a removed surface
cannot block its fresh rebased identity. The conflict regression covers this
case, a later CloseRequested rejection and a subsequent successful command.

Final X11 basic and persistence extensions pass seven/ten real edits while an
independent connection watches the original Root for DestroyNotify. Persistence
performs layout A-to-B followed by JSON B-to-rebased-A, preserving one physical
window and all eight pane records, tabs, selection and text through quiescence.
The first persistence attempt clicked a content center and timed out; that point
does not prove absence of a restored floating-panel blocker. The corrected script
uses actual TextEdit focus before real typing, as the existing reopen/reset focus
journeys do. The original focused failures remain separate from the final record.

All 61 final checks pass against 477 frozen source, manifest, script and lockfile
paths:

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,940/1,940; no-default-feature check, Clippy correctness/suspicious, strict docs, 10 doctests and formatting pass. |
| Independent consumers | Public 148/148 normally and 176/176 with persistence; product 114/114 and official egui 15/15. |
| Native models | 571/571 including the same-coordinator 10,000-lifetime soak; assertion-disabled 570/570 excludes that duplicate soak. |
| Linux library | 524/524 in each assertion mode, plus the separate real old-output renderer regression. |
| Native compatibility | macOS and X11 default/bounded lifecycle and six optimized output-pressure modes per platform pass. |
| Editor X11 | Original basic/persistence six/eight-edit workflows pass; opt-in Root continuation extensions pass seven/ten edits with the same physical Root, exact old-Child destruction and final quiescence. |
| Physical X11 | All fifteen guide, center/edge/tab/nested/overflow, stationary-negative and 1 kHz journeys pass. Positive stationary OS input and scanout remain unqualified. |

Bounded root-boundary reserved peaks are 3,491,216 bytes on macOS and
3,492,376 bytes on X11 under 4,194,304 bytes. Pressure root-service maxima for
combined/ordered/duplicate/reordered/late-staging/late-first-live are macOS
9.575/4.191/4.117/4.333/4.238/4.479 ms and X11
3.667/1.951/1.988/1.894/1.667/1.334 ms. Initialization and atomic work can exceed
the 2 ms cooperative threshold; these observations are not a hard service cap.
The 1 kHz run matches all 1,000 native callbacks at 981.9 Hz after 999.9 Hz
injection, with a 28.339 ms release-to-model upper bound and 0.978 ms maximum
sampled root UI update. Its sampled owned/reserved data peaks are
2,338,721/3,492,512 bytes under 4 MiB. Allocator metadata, graph/renderer state
and separately reported fixture allocations are outside that input-data measure.

Evidence is `/tmp/dockspace-root-continuation-source.json`,
`/tmp/dockspace-root-continuation-qualification.json` and corresponding
`/tmp/dockspace-root-continuation-final-*.log`. This still uses explicit clean
local egui `92c921f7235c` and winit `6c10c87da2a0` overrides. Tracked remote pins,
positive stationary facts, remaining combined capacity/service cases and final
U9 delivery remain open; the complete Goal is not finished.

## 2026-10-31 physical overflow insertion and retained focus

The slice following `dead258` adds one finite `overflow-tab-before` X11 target.
Its fixed three-tab fixture keeps two 400-point tabs after Child tears off, so
the target still overflows. Real selection/backward-scroll clicks leave selected
Main partially clipped. Passive diagnostics expose full/visible member bounds,
viewport and maximum scroll offset from the existing public paint records.
One checked interior point excludes close/scroll controls, actual guides and
edge-scroll bands. Two increasing output steps must agree on the exact insertion
preview; the final model must be `[Anchor, Child, Main]` with Child selected,
followed by exact OS destruction, viewport retirement and quiescence. This does
not claim positive stationary OS auto-scroll or physical scanout.

The real journey exposed three existing defects. Disabled overlay controls used
hover-only egui sense, allowing click/drag to claim the underlying tab against
core's control winner. They now reserve both lanes while disabled controls keep
no executable receiver or accessibility action. A clipped close button also
suppressed its entire visible tab; compilation and validation now preserve an
operable clipped body while omitting unavailable close chrome. Finally, a
still-visible retired child could carry stored OS focus into a cleanup snapshot.
Snapshot focus eligibility now requires a current Pending/Admitted binding and
explicit current visibility.

The first final workspace run passed 1,934/1,935 tests and exposed a left-clipped
tail inconsistency during manual scrolling: compilation emitted a tab without
close chrome, but validation permits that omission only at a clipped right edge.
Left-clipped tails now require complete close chrome and an operable body;
16/20/48-point cases exercise this boundary. Disabled overlay controls also keep
both pointer lanes without joining keyboard focus, and their disabled AccessKit
nodes clear inherited actions. A completed-pass regression checks lane blocking,
focus withdrawal, accessibility, scrolling and unchanged selection together.

The core's consumption boundary additionally handles an already-queued focus
fact whose exact binding retired earlier in the same prefix. Only recognized
Retiring/cleanup/terminal identities become effectively Unknown; unknown
incarnations and foreign providers remain rejected. Raw generation/envelope
history is compared before this authority withdrawal, so duplicate retirement
does not invent a same-generation conflict and stale input cannot revoke newer
focus. True conflicts, gaps and tombstone requirements remain unchanged.

All 59 final checks pass against 476 frozen source, manifest, script and lockfile
paths. The final real overflow run passes exact preview, `[Anchor, Child, Main]`
order, selection, DestroyNotify and quiescence after the complete repairs.

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,936/1,936; default-feature check, Clippy correctness/suspicious, strict docs, 10 doctests and formatting pass. |
| Independent consumers | Public default 147/147 and persistence 173/173; product 114/114 and official egui 15/15. |
| Native models | 564/564 including the same-coordinator 10,000-lifetime soak; assertion-disabled 563/563 excludes that duplicate soak. The separate opt-in real renderer test is not counted as a model pass. |
| Linux library | 517/517 in both assertion modes, excluding the repeated soak; the separate real old-output renderer regression passes. |
| Native lifecycle | macOS and X11 default/bounded create, first-live, redock, retirement and quiescence pass. Bounded root-boundary reserved peaks are 3,488,528 / 3,489,688 bytes under 4,194,304 bytes. |
| Output pressure | Both platforms pass combined, ordered, duplicate, reordered, late-staging and late-first-live release modes. |
| X11 editor | Basic six-edit/two-Child and persistence eight-edit/six-Child workflows pass, including actual replacement-window edits and cleanup. |
| X11 physical matrix | Fifteen journeys pass: guide control; three pane centers; four edges; both tab halves; nested center/edge; clipped overflow insertion; stationary-negative; 1 kHz load. |

Pressure root-service maxima in the six listed mode order are macOS
11.607/4.480/4.718/4.512/4.636/4.620 ms and X11
2.655/3.615/3.374/2.155/1.804/1.180 ms. These include initialization and draining;
they are measurements, not a hard 2 ms service guarantee. The 1 kHz XTest journey
injects all 1,000 moves at 995.8 Hz and matches all 1,000 native callbacks at
976.9 Hz, with a 42.750 ms release-to-model upper bound and 1.059 ms maximum
sampled root UI update. Its root-boundary owned/reserved peaks are
2,336,033 / 3,489,824 bytes under 4 MiB. Positive stationary OS selection or
scrolling and physical scanout are not claimed.

Evidence: `/tmp/dockspace-overflow-insertion-source.json`,
`/tmp/dockspace-overflow-insertion-qualification.json` and the corresponding
`/tmp/dockspace-overflow-insertion-final-*.log` files. The first workspace failure
is preserved in `first-workspace.log` with its original frozen source, rather
than overwritten as a successful attempt. The native scope uses explicit local
egui `92c921f7235c` / winit `6c10c87da2a0` overrides; tracked remote pins still do
not qualify a clean checkout. The complete Goal and remaining host rebinding,
native stationary, capacity/platform and U9 delivery boundaries remain open.

## 2026-10-31 atomic native multi-surface document restoration

The slice following `447b34b` adds the opt-in
`submit_native_document_restore` terminal boundary. Document graph, durable
sidecars and explicit Child creation intents publish together, with the intent
list compiled after surface/recovery-host rebasing. One real external Root must
survive as an exact same-window successor. Every extra surface requires an
explicit Child declaration, that Root as recovery host and positive saved outer
placement. A second retained external Root cannot silently change role.
Ordinary document restore and preference reads remain inert.

The existing establishment map now holds mutually exclusive unbound and bound
branches. An unbound intent owns no binding, effect or recovery obligation;
its complete frozen roster uses the existing recovery store. Budgeted native
service advances at most one intent per call after fresh Root facts, known
capabilities, exact inherited cleanup/terminal-guard quiescence and producer
capacity. Cancellation or definite rejection stops automatic work while keeping
the surface suspended. Explicit retry consumes the same entry only after
revalidation. Failed/dropped frames publish no intent consumption or Create.
Effect-report pressure retains requests in the original ledger instead of
stopping accepted lifecycle continuation.

Public regressions exposed a retired-window observation gap: cancellation before
the first appearance retained a real cleanup owner, but the facade rejected the
late Live facts needed to drive it. The existing producer row now projects only
whether core still owns exact cleanup; such facts may advance that cleanup,
without reinserting a registry record or granting input/focus/admission. A bare
terminal guard still rejects Live. A focused core regression verifies increasing
window generations across cleanup commits and their reset after exact destruction.
Independent bounded reviews confirm this isolation and the retained-Root conflict
repair. The original failed public models remain in separate `/tmp` logs.

Nine public restore journeys and eight adapter layout tests pass on the final
source. The real serde X11 workflow also passes with eight actual focused edits and
six distinct Child lifetimes: two original journeys, two simultaneous preset
predecessors and two replacement windows. The external X11 owner confirms both
predecessors alive before releasing restore, checks both are destroyed before
editing the fresh Inspector, and verifies both successors are destroyed after
Dock Back. All eight stable pane identities, tab order, selection and application
text survive; final quiescence is required.

All 43 final checks pass against 476 frozen source, manifest, script and lockfile
paths:

| Final gate | Result and scope |
| --- | --- |
| Core | 1,928/1,928 workspace tests; no-default-features all-target compile; 13 focused initial-owner tests |
| Public engine host | default 147/147; serde 173/173, including nine native document restore journeys |
| egui product / official consumers | 114/114 and 15/15 |
| Native serde | 561/561 including one same-coordinator 10,000-lifetime soak; opt-in wall-time case skipped |
| Native assertion-disabled serde | 560/560; repeated long soak excluded and opt-in wall-time case skipped |
| Linux library | 516/516 normally and assertion-disabled; repeated long soak excluded and two explicit ignore cases retained |
| Real renderer terminal | Explicit Linux old-output test passes and releases only its predecessor resource |
| Real-window workflows | macOS and X11 default/bounded lifecycle; X11 six basic edits and eight serde edits across six Child lifetimes, fresh output and final quiescence |
| Pressure compatibility | Six release modes on each platform, including combined W16/O256/control2/protected1 saturation |
| Quality | Core/native correctness/suspicious Clippy, strict rustdoc, ten doctests and workspace/public/native formatting |

The pressure groups peak at 2,374,009 resident and 3,488,528 reserved bytes on
macOS, and 2,374,905 resident and 3,489,688 reserved bytes on Linux, under their
selected 4,194,304-byte allowance. Summed `over_2ms` counters are eight and six;
largest measured boundaries are 9.878709 ms and 4.177 ms. The cooperative budget
does not promise a hard OS scheduling deadline. Timing and native storage retain
their existing measurement scope.

`/tmp/dockspace-native-document-restore-source.json` and
`/tmp/dockspace-native-document-restore-qualification.json` retain its exact scope;
focused pre-freeze runs and original failures have separate logs. The ninth
public journey covers definite policy rejection, no automatic reopen after
unlock and explicit retry. Late cleanup facts do not restore old binding,
admission, local-coordinate or focus authority; an additional core test covers
nonzero observation-generation continuity across commits. Native dependencies
still use the same clean local egui/winit overrides as `447b34b`; no new remote
checkout reproducibility or macOS/Windows physical multi-surface claim is made.
Only rebuildable project incremental and stale project test executables were
collected during disk pressure; sources and dependency libraries were preserved.
Complete host removal/rebinding, native overflow insertion and the remaining
U7/U8/U9/U10/U11 Goal remain open.

## 2026-10-31 native hosting for existing logical surfaces

The slice following `349a513` adds
`DockspaceHostFrame::create_native_surface_current`. A complete existing
logical surface keeps its main root, split weights, selected tabs and ordered
contained forest while acquiring a native child window. An admitted application
root owns the fallback recovery destination. The existing replacement lifecycle
now carries a typed Initial or Destroyed origin; creation uses `CreateWindow`
and shares strategy, readiness, exact first-live, failure and cleanup ownership.
First-live admission changes neither topology nor workspace version.

Healthy Initial attempts cannot trigger automatic contained recovery. Failed
or cancelled attempts with no observed coordinates preserve the complete
logical roster. Explicit retry reuses its recovery obligation and reserved
converted-main identity, revalidates current policy/host/roster, and allocates a
fresh binding. Predecessor cleanup and terminal guards retain their exact
incarnation. Cancellation is typed and never fabricates Destroyed or a dispatch
failure. The existing shutdown cancellation query also covers Initial owners.

Independent review exposed a policy gap: global native enablement and fallback
recovery authorization alone did not honor a surface's Native exclusion or a
root/item's `NativeSurface` restriction. Two public regressions first observed
the incorrect Create request. The existing pure policy evaluator now checks
every normalized root and all its items, including unselected tabs, for both
initial creation and explicit retry. Ten public-only tests pass, including 32
policy rejection combinations across DirectPrepared and Staged. The other
journeys cover readiness revocation, exact first-live, failure/retry,
cancellation, late Create/Presented/Destroyed, producer quiescence and frame
rollback under finite storage.

All 42 final checks pass against 474 frozen source, manifest, script and lockfile
paths. The final qualification includes:

| Final gate | Result and scope |
| --- | --- |
| Focused initial/retry lifecycle | 13/13 exact owner, cancellation and retry tests |
| Workspace | 1,927/1,927 with all features and targets |
| Public engine host | default 147/147; serde 164/164, including ten existing-surface native creation journeys |
| egui product / official consumers | 114/114 and 15/15 |
| Native serde | 558/558 including one same-coordinator 10,000-lifetime soak; opt-in wall-time test skipped |
| Native assertion-disabled serde | 557/557; repeated long soak excluded and opt-in wall-time test skipped |
| Linux library | 513/513 normally and assertion-disabled; repeated long soak excluded and two explicit ignore cases retained |
| Exact real renderer terminal | Linux old-output token test runs explicitly and releases only its predecessor resource |
| Real-window compatibility | macOS and X11 default/bounded lifecycle; X11 six basic and seven serde focused edits, two destroyed children and final quiescence |
| Pressure compatibility | Six release modes on each platform, including combined W16/O256/control2/protected1 saturation |
| Quality | Core/native correctness/suspicious Clippy, strict rustdoc, ten doctests and workspace/public/native formatting |

The pressure modes peak at 2,374,009 resident and 3,488,528 reserved bytes on
macOS, and 2,384,105 resident and 3,489,688 reserved bytes on Linux, within the
selected 4,194,304-byte allowance. Their summed `over_2ms` counters are ten and
fourteen; the largest measured boundaries are 9.730166 ms and 2.581916 ms. These
are cooperative service measurements, not a hard 2 ms scheduling guarantee.

A prior assertion-disabled run exposed a test that assumed a system-clock 2 ms
budget must drain in one service call. It now injects the existing fixed clock
for its semantic-drain assertion; independent budget/yield tests remain intact.
The first workspace attempt
stopped during compilation because the project target exhausted available disk;
it ran no tests. Only rebuildable project incremental caches were collected,
and subsequent checks disable incremental compilation.

Evidence is `/tmp/dockspace-native-surface-create-source.json` and
`/tmp/dockspace-native-surface-create-qualification.json`, with the original
policy red/green and fixed-clock focused logs retained separately. Independent
bounded review confirms the policy repair and reports no new confirmed P1/P2.
The platform runs refresh existing native compatibility; the new Initial API is
qualified by public recording and coordinator journeys, not by an additional OS
creation journey. Native runs use clean local overrides at egui
`92c921f7235c87ae28ea09ae522ef55fd8333b41` and winit
`6c10c87da2a06871371de7d30fedc656cf7c3681`. Tracked remote pins are still older,
so this does not prove reproducibility from a clean remote checkout.

Document preferences remain inert reads. The native facade's automatic
multi-surface restore, complete host removal/rebinding and the wider
U7/U8/U9/U10/U11 plan remain open.

## 2026-10-31 explicit document surface hosting

The multi-surface restore follow-up starts from `dbfae70` by replacing an
ambiguous placement-only document contract. Document and surface-preference
schemas are V2; the workspace/key component versions stay unchanged. Explicit
native Root/Child hosting and each child's recovery-host surface share the
existing per-surface preference map with optional physical placement. There is
no content-tree copy or second lifecycle registry. Binding hashes cover the
declarations; provider, window token, work-area token, recovery anchor and runtime
obligation identities stay out of the wire document. V1 is explicitly unsupported
before payload decoding or application identity lookup.

Save captures exact registered roles and current child recovery ownership,
including verified replacement ownership. Unbound surfaces retain previously
saved declarations, while removed surfaces are pruned. Metadata-only rows can
express a native host without a rectangle. Placement-dependent hints without
outer bounds, unknown/self/declared-child recovery hosts and unknown surfaces
reject. Native role is never inferred from placement or main-root shape.

The new `surface_presentation_preferences()` projection is available on session,
prepared restore and candidate frame. It reads committed/pending document data
without saving observations or advancing generation. Terminal document restore
remaps both the surface and its child recovery-host reference together with graph
identity rebase; drop and failure publish neither. The graph view continues to
own each surface's entire main root and contained forest. Reading preferences
does not create native windows or grant binding/coordinate/presentation authority.

Independent review identified a first-save omission for a child retained in
`AwaitingRecoveryHost` without a current registry record or any previous saved
metadata. The public regression first failed because only the root row was
captured. Save now uses the exact retained pending binding/role together with
its bound recovery obligation, without deriving ownership from geometry. The
repaired public test passes. A companion first-save test also observed a
missing root declaration after both root and child were destroyed. Capture now
uses the same-domain root recovery anchor retained for that application-owned
logical surface, while the child still requires its exact pending obligation.
All five public-only preference tests pass. Their fixture includes a child main
root and two ordered contained roots, stable keys and historical closed items,
incoming/candidate/committed projections, drop/retry rebase, native geometry,
metadata without geometry and the two first-save retirement boundaries above.

All 18 final compatibility checks pass against 473 frozen source, manifest,
script and lockfile paths:

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,915/1,915 with all features and targets |
| Public engine host | default 137/137; serde 154/154 |
| egui product / official consumers | 114/114 and 15/15 |
| Native serde | 557/557 normally and with native debug assertions disabled; long soak intentionally excluded from these repeats, opt-in wall-time remains skipped |
| Linux library | 513/513 with serde; long soak excluded and wall-time/renderer-specific tests retain their explicit ignore status |
| Real V2 workflow | X11 named preset capture/restore, drawn missing-pane placeholder, reload, actual recovered focus, seven real edits, two destroyed children and final quiescence |
| Quality | Core/native correctness/suspicious Clippy, strict rustdoc, ten doctests and workspace/public/native formatting |

Current evidence is `/tmp/dockspace-surface-preferences-source.json` and
`/tmp/dockspace-surface-preferences-qualification.json`. The original failed
first-save models and successful focused repairs have separate logs. Freeze
verification rejected earlier in-flight qualification when the new regression
changed; those partial results are retained separately and are not final-source
qualification. Native runs still use clean local fork overrides at the revisions
recorded for `dbfae70`. No fork, input, pressure or lifecycle implementation
changed; the earlier complete physical, pressure and 10,000-lifetime evidence
retains its `dbfae70` source scope rather than being claimed anew here.

Final bounded reviews find no confirmed remaining P1/P2. The root-anchor fallback
relies on the existing replacement clear/reissue, logical-vacancy cleanup and
Root/Child exclusion contracts; the anchor itself is not an epoch-bearing
presentation proof. It captures document intent only. Managed creation of an
already restored logical surface remains the next implementation step. The
native facade still accepts a single incoming host surface; native multi-surface
restoration and the complete U7/U8/U9/U10/U11 Goal remain open.

## 2026-10-31 native defaults and document restore

The slice following `6effe1c` adds `request_layout_reset` and optional native
`serde` capture/restore to the existing single application command/result slot.
Preparation checks the configured host before queue admission. An obsolete
queued replacement returns typed `Stale` without overwriting newer published
state or entering shutdown. Save rejects an occupied command or result slot,
preserving a queued restore's prepared document sidecars. Layout and persistence
errors retain their original typed source.

The replacement report preserves exact native prefix results and their causal
order, then emits the unique layout/document terminal. Superseded product
results remain omitted. The bridge holds its existing mutex while it prepays
successor resource backing, commits core and installs the same physical root
window's new exact route. Failure before publication releases speculative rows.
The existing resource row retains the predecessor until old output and property
receipts settle; neither callback lookup nor a late renderer terminal retargets
them to the successor. Fresh native facts and exact Presented establish new
input/focus authority. No second window lifecycle registry is introduced.

The current facade accepts a single incoming configured host surface, including
contained windows. Existing children retired by reset follow the established
effect/Destroyed/cleanup path. Additional incoming logical surfaces require
explicit future native window recreation. The example owns an immutable default
and eight bounded named JSON slots; recognized unavailable pane content keeps
its identity and draws a reloadable placeholder. Application data stays outside
the layout document.

Independent review identified two boundaries missing from the initial green
journeys. Retained root predecessors now enter the existing retirement store
without a Destroyed requirement, retain a core terminal guard and report exact
producer quiescence after all old references finish. An eight-reset model uses
only two producer/resource slots; delayed property/output receipts block early
release. A public-host model also drops and retries the original quiescence
prefix before the guard compacts. The original missing-candidate and subsequent
missing-core-guard failures were both observed before repair. Locked document
restore now rejects before queue admission with `OperationConflict`; Busy and
Stopped retain precedence. Its original incorrect `Ok(())` admission was also
observed red, and the repaired test continues through unlock, restore and save.

All 43 final checks pass against 472 frozen source, manifest, script and
lockfile paths. The focused 24 native coordinator/application tests also pass.
Final results are:

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,907/1,907 with all features and targets |
| Public engine host / egui consumers | default 137/137, serde 149/149; product 114/114, official 15/15 |
| Native default | 554/554, including one same-coordinator 10,000-lifetime soak; opt-in wall-time case skipped |
| Native serde / assertion-disabled serde | 557/557 each; these repeats exclude the already covered long soak and retain the opt-in wall-time skip |
| Linux library | 513/513 normally and assertion-disabled; long soak excluded, wall-time and renderer-specific cases ignored here |
| Real renderer old output | The renderer-specific Linux test runs explicitly and passes with two producer/resource slots; old output blocks quiescence, then releases only its predecessor |
| Real-window workflows | macOS and X11 default/bounded lifecycle; X11 six basic and seven serde real focused edits, fresh Default presentation, missing content draw/reload, two destroyed children and final quiescence |
| Pressure | Six optimized output-pressure modes on each platform, including W16/O256/control2/protected1 cross-saturation |
| Quality | Core/native correctness/suspicious Clippy, strict rustdoc, ten doctests and workspace/native/public formatting |

The six pressure modes keep their complete timing samples. macOS peaks at
2,374,009 resident and 3,488,528 reserved bytes; Linux peaks at 2,374,905 and
3,489,688 bytes under the selected 4,194,304-byte allowance. The summed
`over_2ms` counters are eight on macOS and thirteen on Linux. Their largest
recorded service boundaries are 11.1255 ms in Bootstrap and 3.610917 ms in
Draining, respectively. These checks prove
bounded storage, exact terminal ordering and cleanup; they do not establish a
hard 2-ms service guarantee. Renderer time and terminal-delay fixture backing
retain their separately labelled measurement scope.

Current artifacts are `/tmp/dockspace-native-layout-source.json` and
`/tmp/dockspace-native-layout-qualification.json`, including all commands,
timestamps, raw logs, timing samples and source hashes. The tracked native
lockfile adds only the six packages required by serde. Both fork worktrees stay
clean at egui `92c921f7235c87ae28ea09ae522ef55fd8333b41` and Winit
`6c10c87da2a06871371de7d30fedc656cf7c3681`; qualification still uses explicit
local overrides. The old tracked remote pins remain insufficient to reproduce
this native source.

Independent review found no remaining confirmed P1/P2 in this slice. Provider
handoff interleaved with replacement, a mixed old-Destroyed/successor-live
snapshot and combined speculative-prepay/commit-failure watermark assertions
remain outside this review's executed combinations. The previous full physical
insertion and 1 kHz matrices retain their dated source scope. Complete native
multi-surface document recreation, host removal/rebinding, clipped/overflow tab
insertion, positive OS stationary capture/delivery, combined service admission,
platform certification and U9 packaging remain open. This qualifies the
single-host defaults/document slice; U10, U7/U8, U9 and the full Goal remain active.

## 2026-10-31 terminal replacement after bounded native replay

The slice following `a136e86` adds public
`DockspaceHostFrame::submit_layout_reset` and serde
`submit_document_restore`. Hosts append either replacement to the ordinary
final frame returned after their bounded native prefix. Synchronous conveniences
use that same submission path. The replacement-specific branch which discarded
a pending candidate and rebuilt replay under a different item scope is deleted.

Earlier native and application input uses the published document's identity
scope. Only the final document-replacement append admits the reconciled target
scope, requiring the original scope to match and every old key to remain
recognized. Scope expansion and append form one private operation; failed
append poisons the candidate. Sequence allocation occurs before scope changes.
Workspace, key history, placement preferences, generation and input watermarks
still publish through the existing single commit. Layout construction, decoding
and atomic replacement are not covered by a hard 2-ms wall-time guarantee.

The public reset models now cover direct final-frame submission, stale/foreign/
locked preparations, measurement, old output/focus retirement and identity
rebasing against earlier same-frame mutations. A real presented receiver answers
the first edge of an atomic native group before a record budget yields. Both
`poll + submit` and the synchronous reset wrapper process only the remaining
edge, preserve its candidate selection and publish the same root successor.
Dropping that frame retries from the committed watermark rather than leaking
private input or allocator progress.

Four serde consumer tests cover new recognized keys, previously known closed
items, unavailable content identity, exact generation/key history, sidecar
invalidation, locked restore, replacement input fences and native binding
successors. Their controlled record budgets also verify partial commit,
unpublished suffix resumption and drop/failed-commit replay counts. A separate
internal regression forces duplicate-sequence failure after scope expansion;
later input and finish reject, all published frontiers remain unchanged, and a
fresh prepared restore retries successfully. No public test seam is added.

All 22 final checks pass against 464 frozen tracked source/manifest/script paths.
The focused 12 consumer tests and internal poison regression also pass. Exact
final compatibility results are:

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,907/1,907 with all features and targets |
| Public host / product / official consumers | default 134/134, serde 145/145; product 114/114 and official 15/15 |
| Native normal | 543/543, including the same-coordinator 10,000-lifetime soak; opt-in wall-time case skipped |
| Native assertion-disabled | 542/542; the long soak is intentionally excluded from this repeat and covered by the normal run above, while the opt-in wall-time case stays skipped |
| Linux library | 498/498 normally and assertion-disabled; long soak excluded here |
| Real-window compatibility | macOS default/bounded lifecycle; X11 five real edits, actual reopened focus, two child lifetimes, close/Allow/reopen/lock/unlock and quiescence |
| Quality | core/native correctness/suspicious Clippy, strict docs, ten doctests, workspace/native/public formatting |

Current artifacts are
`/tmp/dockspace-terminal-replacement-source.json` and
`/tmp/dockspace-terminal-replacement-qualification.json`. The first new focus
assertion expected a central fallback after its own prefix explicitly focused a
different still-selected pane; the corrected assertion follows the existing
newer-intent contract, with production focus logic unchanged.
Independent production review found no confirmed P1/P2 and requested the
receiver-resumption and expanded-scope append-failure models described above;
both now pass. This slice does not rerun the preceding six-pressure, physical
docking or 1 kHz matrices, whose dated evidence retains its earlier source scope.
Fork sources remain unchanged and local overrides remain necessary; clean remote
pin reproduction and package delivery are still separate U9 work.

This completes a renderer-neutral replacement integration boundary only. Native
application reset/preset/document commands, root-window rebinding and showcase
restore/missing-pane journeys remain subsequent work. The earlier fixed-host
`HostSurfaceRequired` constraint remains in force.

## 2026-10-31 native content-close and arrangement commands

The slice following `117578c` extends the existing single application-action
slot with selection, reopen, item/root close requests, initial/deferred decisions
and arrangement locking. Results now use `NativeActionStatus`; docking results
retain their existing `DockspaceActionStatus` inside `Dock`. An unconsumed
result still owns the slot. Configuration is appended after local and focus
input in the same final root frame. Native surface-close policy remains a
separate existing OS-request owner.

The core now returns exact `DockspaceSubmittedAction` identities for close
requests, both decision kinds and policy replacement. Policy reports include
each input's `changed` flag and intermediate workspace version. The native
facade uses `submitted_action_outcome`, including when other close requests
share that frame. `pending_content_close_plans` projects current item/root plans
from the existing core inventory, including chrome requests and deferred
continuations; it adds no history or registry and excludes native surface
requests, stale plans and terminal plans.

Seven core regressions cover exact multiple-request/decision/configuration
results, dropped-attempt and foreign-session isolation, deferred continuation,
current chrome discovery and invalidation. Nine native coordinator regressions
cover the same facade boundary with bounded result retention, group voting,
reopen with application data intact, local selection before lock, selection
while locked, stale requests and foreign tokens. All sixteen pass. The full
workspace passes 1,906/1,906; public-host default and serde pass 132/132 and
139/139, product and official consumers pass 114/114 and 15/15. Core Clippy,
strict rustdoc and all ten doctests also pass.

An independent review found that closing the fixed host's only root could prune
its logical surface and leave later application actions without their host
roster. The corresponding guard was observed red before repair and green
afterward. The facade now returns `HostSurfaceRequired` for that close and for
an exact initial/deferred Allow from a chrome-created plan. Busy/stopped ordering,
Veto/Deferred and stale/foreign/duplicate-token handling remain unchanged.
Closing all individual panes in a central root keeps its empty dock area;
the regression continues through lock/unlock and reopening. A complete group
may still close when another presentation remains, and reopening allocates a
fresh root. Core's `RootClosed` contract is unchanged. Replacement/rebinding
after removing the complete fixed host remains explicit future work.

The native `basic` inventory and close-decision UI now have finite real-window
qualification. X11 adds programmatic close/Allow/reopen, actual TextEdit focus
before announcing the fifth external edit, then lock/rejected move/select/unlock
and final quiescence. The five edits use real X11 keyboard input on the same
Inspector ItemId, and both child windows are confirmed destroyed. The fifth
edit types using the already restored widget focus. Its original extra click
hit the surviving contained Preview which covered the TextEdit center and
caused a timeout; that failed run and a geometry diagnostic remain separate
attempts. The corrected workflow passed before and after the fixed-host repair.
These results exercise public commands, not physical clicks on every inventory
or decision control.

All 40 final checks pass against 462 frozen tracked source/manifest/script paths.
Earlier native checks before the fixed-host repair are retained in
`/tmp/dockspace-layout-actions-pre-host-guard-qualification.json` with their
separate source snapshot. Current evidence is
`/tmp/dockspace-layout-actions-qualification.json` and
`/tmp/dockspace-layout-actions-source.json`; the host guard's focused red and
green logs use the matching `-host-surface-red.log` and `-host-surface-green.log`
names. These are local evidence artifacts rather than checkout assets.

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,906/1,906, all targets/features |
| Public host / product / official consumers | 132/132 default, 139/139 serde; 114/114 product and 15/15 official |
| Native normal / assertion-disabled | 543/543 each, including same-coordinator 10,000-lifetime soak; opt-in wall-time case skipped |
| Linux native library | 498/498 per configuration, long soak excluded here and included above |
| Native real windows | macOS and X11 default/bounded flows and all six optimized pressure scenarios per platform |
| X11 editor | five real focused edits, close/Allow/reopen, automatic focus recovery, rejected locked move, selection while locked, unlock, Destroyed and quiescence |
| Quality | core/native/Linux correctness/suspicious Clippy, strict core/native docs, ten doctests, workspace/native/public formatting |

Across the six pressure scenarios, standalone service maxima are 469.208
microseconds on macOS and 948.5 microseconds on X11. Full output callback maxima
are 9.4905 ms and 2.465792 ms; over-2-ms observations remain in the logs. These
measurements retain the cooperative budget boundary and make no hard-real-time
claim. This slice does not refresh the previous twelve-target physical docking
or 1 kHz qualification; their dated results keep their original source scope.
Forks remain unchanged and unpublished, using the same explicit local overrides.
Final independent static review confirms no P1/P2 in the fixed-host guard.
The focused models do not enumerate rootless-only-contained, simultaneous
shutdown/busy and every active-plan token replay combination; this is bounded
coverage rather than exhaustive state-space certification.
Reset/defaults, named presets, document restore and the other open
native/platform/package gates remain outside this completion claim.

## 2026-10-31 native content input and bounded editor showcase

This slice is qualified by 61 passing final checks against 436 tracked Rust and
script source paths following `233657c`. Its original snapshot also included
eight generated `target/` files; those are excluded from the committed-source
count. All 436 source paths and both forks' recorded sources match their
committed blobs. The main commit is `117578c`. The locally committed egui/eframe fork is
`92c921f7235c87ae28ea09ae522ef55fd8333b41`; Winit remains
`6c10c87da2a06871371de7d30fedc656cf7c3681`. Earlier stationary
results retain their historical source scope; they do not qualify this change.

The 2026-10-31 dependency recheck confirms upstream's
[egui/eframe release list](https://github.com/emilk/egui/releases) still marks
0.37.0 as its latest stable release. The
[Winit release list](https://github.com/rust-windowing/winit/releases) includes
0.31.0-beta.4 as a prerelease and 0.30.13 as the stable line used here. No new
stable-version migration is required for this slice. The crates.io API could
not be reached directly or through the configured local proxy; the check uses
the upstream release pages rather than claiming a successful registry query.

The native `basic` example now provides eight stable application panes with
editable text, scroll and drawn content. Widget declarations refresh from the
actual responses on every paint; editable panes use stable content focus IDs.
The separate X11 driver supplies real click/keyboard input and a real window
manager close. It checks four exact edits through native first-live, Dock Back,
a second child and OS-close recovery, stable item identity, destruction and
quiescence. Its initial click waits 400 ms after pointer placement, beyond the
250 ms ingress-age limit. Diagnostics locate content without granting authority.

Real content exposed an input-loss defect: the native driver consumed egui
input in an empty retained pass, removing missing controls from focus and hit
participation. A generic eframe pre-input hook now lets the host settle ordered
prefixes with the exact output budget before extracting native input. A deferred
pass keeps the original backend buffers and its original exact NotPresented
terminal. Child retries retain the existing exact-route semantic wake.

Ordinary successful surface terminals request a coalesced native host-service
event, which runs no application logic/UI, extracts no input and creates no
output scope. Service uses the existing clock and bounded input profile;
ordered capture identities, age, resource counts and full-commit ownership are
unchanged. Budget exhaustion retries in a later native event. First-live,
required geometry, stationary and durable lifecycle work keep explicit repaint
wakes. The same reducer handles pre-input and standalone service; overload
cancellation is work even when the raw prefix is empty. No speculative
post-UI framebuffer-retention flag or input replay remains.

The close workflow also exposed a global-focus callback whose binding had lost
focus eligibility since capture. The adapter revalidates the exact committed
core binding before recording Dock focus, downgrades obsolete proof to Unknown
and acknowledges the original callback once. It cannot retarget a successor or
settle an outstanding focus request with that stale proof. Visible staging
children remain reportable before first-live admission.

A later full refresh caught a separate core close-emission invariant:
delivering Accept moved the binding to AwaitingDestroyed and advanced coordinate
authority after ordinary snapshot reconciliation, while leaving its old Ready
scene paintable. A subsequent paint combined a current NativeUnavailable token
with the old NativeReady scene and correctly failed the exact-coordinate gate.
The shared close-emission boundary now demotes invalidated scene authority in
the same commit that delivers Accept. It preserves layout and confirmed visual
fallback, while withholding new paint and interaction until legitimate readiness
returns. A focused managed-close test was observed red before this repair and
green afterward; the authority comparison itself remains unchanged.
An adjacent fixture was also observed red when the old measurement was only a
pending retained candidate. Coordinate invalidation now tests for a confirmed
fallback before choosing Stale; otherwise it enters Bootstrap. Accept delivery
therefore remains valid without inventing a previously presented framebuffer.

Completed-pass content regressions cover TextEdit, Button and canvas receiver
mapping, undeclared controls and frontmost overlay blocking. The physical X11
fixture now uses registered editable TextEdit content for actual docking
preview/commit targets; these physical results are qualified separately from
receiver mapping. The pressure fixture records standalone host-service duration
in addition to admission-through-finalization duration, and checks final owners
reach zero before the application closes.

Failed diagnostic evidence is retained: the original missing content input,
root staging-capability misclassification, non-idempotent close deferral,
parked-terminal final-boundary timeout, stale global-focus rejection and the
close-emission coordinate mismatch are not passing gates. The pre-close-repair
qualification is retained separately in
`/tmp/dockspace-native-editor-pre-close-revocation-qualification.json`; its
available logs are archived under the matching `-logs` directory. Final evidence
is `/tmp/dockspace-native-editor-final-qualification.json` and
`/tmp/dockspace-native-editor-final-snapshot.json`. These local artifacts are
not portable checkout assets.

| Final gate | Result and scope |
| --- | --- |
| Workspace | 1,899/1,899 with all features and all targets |
| Public engine consumer | default 132/132; serde 139/139 |
| Product / official egui consumers | 114/114 and 15/15 |
| Native normal / assertion-disabled | 534/534 each, including the same-coordinator 10,000-lifetime soak; the opt-in wall-time case remains skipped |
| eframe Glow / WGPU / assertion-disabled | 101/101, 95/95 and 101/101; feature-disabled compile also passes |
| Linux native library | 489/489 normally and with adapter assertions disabled; long soak excluded here and covered by the full native runs above |
| Real macOS / X11 lifecycle | default and bounded redock flows; six optimized output-pressure scenarios per platform, including W16/O256 and final owner-zero checks |
| Physical X11 content | all twelve center/edge/tab-half/nested targets over registered TextEdit content; separate 1 kHz journey and truthful stationary-negative journey |
| Real X11 editor | four exact focused edits, 400 ms initial idle, two child lifetimes, Dock Back, OS-close recovery, Destroyed and quiescence |
| Quality | ten doctests; core/native/fork and Linux correctness/suspicious Clippy; strict core/native/fork docs; workspace/native/public/fork formatting |

The final 1 kHz entry observes 1,000/1,000 matching motions, 997.5 Hz injection
and 998.3 Hz callbacks, with release-to-model observation upper bound 2.176875 ms.
These are finite XTest observations, not hardware polling or latency guarantees.
Combined standalone service maxima are 541.416 microseconds on macOS and 754.292
microseconds on X11; full output-callback maxima remain 11.103958 ms and
3.055958 ms, including their over-2-ms samples. No hard wall-time claim follows.

Final independent static reviews confirm no remaining P1/P2 in the reviewed
input/service, focus normalization and close-emission changes. Dynamic coverage
is limited to the listed models and real-window journeys. Tracked manifests
still pin older fork revisions; the new local revisions remain unpublished.
Explicit local overrides and their lockfile source adjustment were used for
qualification. The task's temporary
egui lockfile adjustment was restored after testing. No clean-checkout native
reproducibility, complete U8/U10/U11/U9 or broader platform claim follows.

## 2026-10-31 finite native stationary scheduling

The reference adapter now connects the existing core continuation to eframe's
prepared pointer-observation command. It adds no docking graph, continuation
issuer, clock owner or lifecycle registry. Scheduling is limited to an existing
Winit Mouse identity with an active button and a committed Desktop continuation.
NativeLocal, Touch and unknown devices do not enter this sampler.

A future deadline requests a root timer without input facts. Due preparation
pre-pays one record from the shared callback budget before application painting
can exhaust it. The unqueued command has no callback or input authority. It can
survive Repeat passes of that exact outer output; finalization revalidates the
core request, device, window and input boundary before reserving the existing
receipt and dispatching. Abandonment, a lost output owner and quarantine drop
unqueued preparation. Only the final non-Repeat output pass can dispatch.
If no preparation could fit the budget, a future deadline still uses a timer;
an already due deadline retains one wake on the exact current output terminal.
There is at most one in-flight observation per session, and an obsolete query
occupies its original receipt until full commit. Output acceptance for pending
stationary geometry uses the existing presentation wake and receiver owner.

The callback freezes actual desktop, hover, delivery and capture routes once
into the existing window command receipt. The raw journal holds only its small
correlation key. Replay retains the original ordinal and shared-clock capture
time, emits the existing `Continuation(token)` event and uses current exact
receiver authority. It never creates a synthetic motion or release. Unknown
facts stay Unknown; Unsupported, Unavailable and retirement produce a terminal
without a sample. Each unchanged core token can trigger at most one query.
Only a new core token, including one from a valid same-position real motion,
can authorize another attempt.

Nineteen focused tests pass: seven real-core/Winit coordinator models, four
mailbox lifecycle tests, five frozen-sample tests and three read-only device
lookup tests. They cover deadline/budget admission, one in-flight receipt,
full-commit release, original sequence/time through frame drop and retry,
unavailable/unknown results, late release, successor isolation, detach and
post-overload terminal admission. The two added preparation models explicitly
advance the shared clock beyond 2 ms before final admission and reject raw
input arriving after preparation without creating a callback obligation. An
independent review identified a conditional progress risk in the original
finalizer-only scheduler behind ordinary application work after the docking UI;
preparation now pre-pays the original budget. The earlier implementation and
its completed 524-test normal run are superseded, while its assertion-disabled
run was intentionally interrupted. A model presents a new concrete artifact
before the sample arrives and proves that old bound receiver/hover authority
cannot authorize selection, including on retry. A paint descriptor for the
same semantic scene can still be rebound to a new presentation; it is not
itself presentation authority. These are controlled backend/core contracts,
not successful OS stationary execution.

The real X11 journey exposed a core distinction: a stationary query which
cannot observe delivery is not an input edge proving delivery loss. Previously
that Unknown value cancelled the held gesture, so even a subsequent exact real
motion could not restore its target. The native coordinator regression failed
at the active-gesture assertion before the repair. The continuation reducer now
revokes its pending timed action without inferring loss or replacing known
capture with Unknown. Known delivery and capture loss still use the existing
gates, independently; ordinary Moved/Release and obsolete-token handling retain
their prior rules. Two public Desktop/NativeLocal regressions cover Unknown
delivery with known/unknown capture, duplicate rejection, same-gesture rearming
and exact-output release, plus None/Foreign delivery/capture cancellation
controls. The full native-stationary public target passes 13/13 normally and
with core debug assertions disabled. Static review finds no remaining confirmed
P1/P2; wrong-source/stale-binding rejection retains its existing gates. The new
positive release case re-confirms facts through real motion before release;
it does not separately certify direct release immediately after an Unknown
sample.

The new opt-in X11 entry sets
`EGUI_DOCKSPACE_PHYSICAL_STATIONARY=negative` and leaves physical target/load
unset. Its fixed `[Anchor, Main, Child]` layout leaves Main inactive after Child
tear-off. It holds a real Child-window press over ROOT Main, requires a real
`Observed` query callback, checks independent Unknown hover/capture/delivery
and unchanged public-model selection after that exact result, then resumes real
motion in the same held gesture and checks exact center redock, destruction and
quiescence. Pressure starts before Main motion can mint its continuation; the
entry requires query progress with a 3 ms application suffix and one real
repeated output. No query, Unavailable, Unsupported or timeout is accepted as
this negative qualification.
Final X11 qualification passes that complete journey. Its sample records 18
slow suffixes and two actual pass finalizations for one outer output. The
callback-following UI observation alone does not prove exact receipt commit;
the coordinator models prove full ordered commit, and the physical journey
separately proves subsequent same-gesture redock and retirement. The initial
physical fixture incorrectly assumed a source-window hover would mint a Desktop
continuation and timed out without a query; it is not a passing negative gate.
Later failed fixtures retain their missing semantic boundary, first-remaining
tab selection, stale bind-mount binary and pre-repair cancellation evidence.

Current X11 sampling cannot prove a grab owner or delivery route. This
implementation does not enable successful OS stationary selection or scrolling
by filling that gap from the expected window or cached implicit capture.
NativeLocal/Touch automatic sampling, broader platform readiness, the native
showcase and U9 remain open.

Final core and macOS test refresh against the frozen source:

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 1,897 passed |
| Public consumer, default / serde | 132 / 139 passed |
| Product / official-egui consumer | 114 / 15 passed |
| Public native stationary, core assertions disabled | 13 passed |
| Native all-targets, normal / adapter assertions disabled | 526 / 526 passed |
| Same-coordinator 10,000-lifetime soak | Independently passed in both native runs |
| Workspace doctests | 10 passed |
| Linux adapter library, normal / adapter assertions disabled | 484 / 484 passed; soak filtered on Linux |
| Linux physical-runner unit target | 50 passed |
| macOS and X11 real-window redock | Default and bounded on each platform |
| macOS and X11 release output pressure | All six cases on each platform |
| X11 physical docking | All twelve ordinary targets, separate 1 kHz and stationary-negative entries |
| Core/native Clippy, strict rustdoc, formatting and patch integrity | Passed |

One explicit opt-in native wall-time test remains skipped in ordinary suites;
its separate scope is unchanged. The 1 kHz entry matches 1,000/1,000 callbacks,
with a release-to-model upper bound of 34.865 ms. Its actual root UI envelope
and the separate Combined synchronous-host envelope retain their distinct
measurement boundaries. Combined root maxima are 8.387417 ms on macOS and
2.231459 ms on Linux, including respectively three and one over-2-ms samples;
no hard service bound follows. Bounded redock holds 2,335,691/3,488,144 bytes
owned/reserved on macOS and 2,336,180/3,489,304 on Linux under the unchanged
4 MiB allowance. These are observed input-data capacities, not process RSS.

The 55 successful final records are at
`/tmp/dockspace-auto-stationary-final-qualification.json`, tied to all 447 Rust
source hashes in `/tmp/dockspace-auto-stationary-final-snapshot.json`; logs use
the same prefix. Three independent review scopes cover scheduling/receipts,
physical negative predicates and core missing-owner semantics, with no remaining
confirmed P1/P2. The native builds still use unpublished local egui/Winit
overrides, unchanged from `70f7b517`/`6c10c87d`; tracked remote pins do not
reproduce them. This entry supersedes the earlier pre-Unknown-delivery native
results of the same test count. The full Goal remains active.

## 2026-10-31 queued native callbacks and fractional create requests

Real release output pressure exposed a child callback which was still valid at
the native layer but absent from the current core presentation roster. Ordered
O256/control2/protected-staging1 runs failed with
`SurfaceOutsideRoster { surface: SurfaceId(2) }`.
An error-only probe confirmed both initial and current frame rosters were `[1]`,
with no pending native input, output prefix, reduced callback or control output.
The old driver painted an empty background and then attempted to measure the
ineligible child. Continuous successful reruns with probes had initially hidden
the timing; a clean build reproduced the error again on run seven, and the
error-only probe reproduced it on run five. Those failures remain recorded.

The driver now uses this frame's actual roster to retain the previous native
output. It completes and commits only actual roster contributions, retains the
existing child's semantic wake, and attaches the next root cycle to the current
exact output terminal. It neither creates a contribution for the absent child
nor changes core eligibility. The qualified path does not carry discarded
local actions; their separate finalizer checks are unchanged. In a temporary
instrumented green run, two of 24 real Ordered journeys reached this same absent
roster state, for three callbacks total. Every journey then completed first-live,
redock, destruction, retirement and final zero output owners. All probes were
removed before final qualification.

The new nested physical fixture exposed a second production boundary. Its
source pane is 179.75 physical pixels wide. Core correctly requested a native
child, but the adapter's integer-only conversion rejected that desired rectangle
before reserving or creating a window. The source remained recoverable and
returned to quiescence, while the runner timed out awaiting first-live. The
focused real-event coordinator regression first failed at zero retained
deferred specs instead of one.

The adapter's platform-request conversion now rounds positions and expands
positive extents with `ceil`; zero dimensions and values outside the native
integer range still reject. Both Create and Replacement use this boundary.
The original desired placement remains in the core proposal and retained spec.
No requested rectangle supplies content bounds, coordinates or readiness:
subsequent native snapshots and exact first-live output remain authoritative.
This follows the public `NativeWindowPlacement` contract, which permits a host
to adjust a request and requires reporting actual resulting facts. Negative
positions, subpixel positive extents, integer overflow and empty dimensions are
covered. The real-event regression also checks unchanged durable version/source
ownership, pending create, absent durable child and absent child coordinates.

Focused request tests pass 4/4. Independent review of the roster fix and request
conversion found no remaining confirmed P1/P2. Final frozen-source macOS
normal and assertion-disabled native gates each pass 507 tests, including their own
same-coordinator 10,000-lifetime soak. One explicitly opt-in wall-time test remains
skipped in ordinary runs and retains its separate release qualification. The
core is unchanged from `7003b13`; its 1,897 workspace, public 130/serde137,
product114, official15 and ten-doctest results retain that earlier scope.

Final cross-platform refresh is recorded in
`/tmp/dockspace-service-roster-geometry-final-qualification.json` against
`/tmp/dockspace-service-roster-geometry-final-snapshot.json`. The exact ingress
red and green controls are `/tmp/dockspace-fractional-create-red-exact.json`
and `/tmp/dockspace-fractional-create-green.json`; the earlier invalid test
setup is explicitly superseded. The root-roster red/green logs retain their
`dockspace-native-service-error-probe` and `dockspace-native-roster-retain-green`
prefixes. The production corrections are committed as `f4575cc` (current frame
roster) and `5d42e1e` (platform request quantization).

The final macOS and Linux request regression passes normally and without
adapter debug assertions. Native correctness/suspicious Clippy, strict rustdoc,
both workspaces' formatting and patch integrity pass. MacOS and X11 each pass
default/bounded programmatic redock and all six release output-pressure cases.
X11 passes all 12 physical targets and refreshes its separate optimized
1 kHz center-load entry. Source hashes were rechecked against both the working
tree and native qualification snapshot. Native builds still require the
unpublished local fork overrides in `PLATFORM_SUPPORT.md`; this does not
establish clean-checkout reproduction from the tracked remote revisions.

## 2026-10-31 synchronous native host service and nested physical targets

The real output-pressure proxy measures from entry to its `on_output_begin`
through the last inner `on_output_pass_finalized` return. The inner callback
includes the actual driver, shared dispatch budget, core commit, effect
scheduling, pass settlement, native finalizer and lock release. The interval
also contains fixture application bookkeeping, repeated-pass work and
measurement overhead, so it is a conservative synchronous host envelope.
Renderer/GPU work and terminal-delivery delay occur after this boundary and
are excluded. It does not measure actual return to the OS loop or certify
hard 2 ms service, hardware polling or sustained active-input throughput.

A fixed 258-entry pending sample buffer uses exact real output identities.
Repeated passes retain the original admission start and close once at the real
terminal. Root, child, Draining and Defer totals report count, passes, repeat,
minimum, maximum, sum and observed over-2-ms samples without retaining unbounded
history. A terminal without finalization increments `not_finalized` instead of
inventing a zero sample. The sample buffer's 26,832 bytes are reported separately
from the native 4 MiB allowance. A forced real discard in Combined requires at
least one actual token with multiple finalized passes. Review corrected the
ordinary Defer boundary to include its synchronous terminal flush before timing
stops; early-return Defer paths still close exactly once.

The final optimized Combined samples are:

| Platform / segment | Samples | Finalized passes | Maximum | Samples at least 2 ms |
| --- | --- | --- | --- | --- |
| macOS root | 936 | 937 (one repeated output) | 8.920083 ms, Bootstrap | 4 |
| macOS child | 282 | 282 | 0.399458 ms, RetireSatellites | 0 |
| macOS Draining subset | 56 | 56 | 2.260917 ms | 1 |
| macOS Defer | 5,905 | 0 | 0.077375 ms | 0 |
| Linux root | 860 | 861 (one repeated output) | 2.097750 ms, Draining | 2 |
| Linux child | 609 | 609 | 0.497584 ms, RetireSatellites | 0 |
| Linux Draining subset | 97 | 97 | 2.097750 ms | 1 |
| Linux Defer | 32 | 0 | 0.052542 ms | 0 |

Draining is a subset of root/child observations, not extra independent samples.
Both runs report `not_finalized=0`. Counts and outliers depend on actual
event-loop scheduling and the finite workload; they are not percentiles or
portable upper bounds. Combined keeps all 16 real window identities and
O256/control2/protected-staging1 simultaneously, then returns every child and
all 258 original terminals to quiescence. Owned/reserved native data samples
are 2,350,249/3,455,424 bytes on macOS and 2,350,409/3,455,800 bytes on Linux,
under 4 MiB and excluding the separately reported fixture allocations.

The fixed X11 runner adds `nested-pane-center` and `nested-pane-edge` (west) to
`EGUI_DOCKSPACE_PHYSICAL_TARGET`. Its initial layout is
`Horizontal[central Child, Vertical[Sibling, Main]]` with ancestor weights
`[0.25,0.75]` and `[0.375,0.625]`. After physical tear-off, the protected central
source remains empty. Actual painted Main content and full-tab diagnostics
identify the target leaf; points exclude every actual guide hit and the outer
root band after physical rounding and again before release. Center requires
Main's content preview. West requires the target leaf's left payload, including
its tab strip and excluding the splitter, rather than a half-root rectangle.
The final model preserves both ancestors, their weights and Sibling order;
west insertion adds a third, different-axis split with exact `[Child,Main]`
payload and equal weights. Both journeys require exact native destruction,
semantic retirement and quiescence. No internal target or authority is
constructed by the diagnostic.

The runner has 50 Linux tests, including eight new geometry/model/target cases;
macOS compiles and runs the applicable 25 geometry cases. Both nested physical
journeys pass after the fractional-request correction. The finite nested
targets deliberately reject the 1 kHz combination; the existing center-load
profile retains its separate scope. Arbitrary nested configurations, clipped
or overflowed tab insertion, mixed DPI and high-rate edge docking remain open.
Measurement results and final cross-platform qualification use the exact
snapshot and qualification record above. Overall U7/U8, U9 and the complete
Goal remain open.

The rebuilt 1 kHz center-load run injects all 1,000 checked distinct points at
999.6 Hz without catch-up, matches all 1,000 callbacks at 991.0 Hz, and reports
45.310875 ms release-to-model upper bound with maximum sampled root UI update
0.788958 ms. The single root-UI duration is narrower than the synchronous
admission-to-finalizer measurement and does not replace it. The environment
remains Rust 1.95, Linux/arm64, Xvfb/Openbox, one X screen at scale 1,
2 CPU/4 GiB and software GL with `LP_NUM_THREADS=1`.

## 2026-10-31 terminal guards for never-created native bindings

The finite close/create capacity model exposed a production reclamation fault:
after a conclusively failed create, the coordinator could retire its exact
binding and report producer quiescence, but the next committed-prefix
reclamation required a Destroyed-only guard which that binding never had.
`configured_close_and_create_owners_survive_freeze_and_exact_terminal_retry`
first failed with `DestroyedBindingGuardMissing`; an extension of the existing
pure-core create-dispatch-failure test independently reproduced the same error.
Neither test supplies a fabricated physical Destroyed observation.

Core now retains a terminal binding guard for a conclusively never-created
incarnation. It uses the same existing cleanup indexes and exact-provider
reclamation path as observed destruction. The internal names, retention query
and errors now describe terminal guards: `terminal_binding_guards()` and
`TerminalBindingGuardMissing/ProviderMismatch` replace their destroyed-only
names. Guard absence and foreign-provider rejection remain errors. Failed
recovery-replacement discard and unobserved cleanup completion use the same
helper; observed destruction retains its own exact observation-provider proof.

Independent review identified a second boundary: an accepted native prefix can
publish a Pending binding while effect-report capacity delays its Create
emission. Its producer has already received the binding, so cross-frame
cancellation still requires quiescence. A new capacity regression reproduced
the missing guard. The existing effect record now stores the exact provider
of successful binding publication, independently of effect delivery. The
existing atomic publication boundary writes it only for current creation or
replacement bindings. The terminal helper prefers the original immutable
delivery provider and otherwise uses that publication fact, never a guessed
current provider. Same-candidate cancellation before any binding publication
retains no external guard. No second registry, owner, issuer or lifecycle graph
is introduced.

The same published-but-undelivered fixture also resets the layout. Independent
review found that restore's never-created fast exit released its staging
resource without calling the terminal helper; the reset variation first
reproduced guard count zero instead of one. That exit now shares the helper.
The final review confirms this reset correction, publication/provider lineage
and the separate exact-Destroyed path without remaining confirmed P1/P2.

The new W4/A1, E1, M4/C0/L2 model remains within the selected 4 MiB allowance.
Two existing close requests retain cancellation acknowledgements while a real
core Create owns its effect and admitted route. A pre-freeze close successor
and 1,745 accepted ordinary cursor records establish boundary B; a second
successor and Failed enter their original protected receipts after B. Duplicate
terminals do not advance capture. Actual cancellation and Failed candidates
are dropped and retried; neither pending terminal blocks the single overload
cancellation, and Failed cannot quiesce before full commit. Per-owner Clear
promotes only its original successor with unchanged ordinal/captured time;
both resulting requests then finish independently. Final source ownership,
route/close/retirement cleanup, prefix reclamation and quiescence are checked.
This is a finite tokenless coordinator model, not a real OS close/create test.

Final frozen-source qualification passes 1,897 workspace tests, public consumers
130 normally / 137 with serde, 114 product and 15 official-egui tests, plus ten
doctests. Native all-targets gates pass 502/502 normally and 502/502 without
adapter debug assertions; each includes the same-coordinator 10,000-lifetime
soak. The explicitly opt-in wall-time test is the one ordinary-run skip and
retains its separate release record. Core/native correctness/suspicious Clippy,
strict rustdoc, formatting and patch integrity pass.

The finite close/create case passes on macOS and Linux normally and without
adapter debug assertions. MacOS samples 2,136,448 owned / 4,194,144 reserved
bytes; Linux samples 2,136,416 / 4,194,136 bytes, under 4,194,304 bytes on both.
These are native owned input-data capacities and promises, excluding allocator
metadata, core workspace/reducer/renderer state and fixture allocations.
MacOS default/bounded real-window redock and all six pressure journeys pass;
X11 reruns all ten finite physical journeys and six pressure cases successfully.
The unchanged unpublished local egui/Winit overrides remain required.
Current source and exact results are `/tmp/dockspace-close-create-snapshot.json`,
`/tmp/dockspace-close-create-qualification.json` and matching logs. The initial
failure, publication/cancellation and reset red controls remain separate. This
closes the identified finite close/create retry boundary, not every capacity
combination or real OS close/create proof. Full host-service timing, native
scoped/stationary and broader platform claims, the showcase and U9 remain open.
Overall U7/U8 and the complete Goal are not complete.

## 2026-10-31 X11 physical tab-half insertion

The finite X11/XTest runner adds `tab-half-before` and `tab-half-after` to
`EGUI_DOCKSPACE_PHYSICAL_TARGET`. Each uses the preceding real drag-out,
first-live, held-source drag-back, exact native DestroyNotify, semantic viewport
retirement and quiescence journey. The target is the single fully visible Main
tab without overflow; physical points must lie strictly in its named half and
outside Close Main and every actual guide hit after rounding and again before
release. No fallback target is substituted. The high-rate extension still
rejects these targets before starting the gesture and remains center-only.

The existing AccessKit Tab bounds intentionally describe the drag receiver,
excluding the close button. An initial after-half attempt incorrectly treated
that rectangle as the whole tab; it selected the actual left half and observed
the before marker, then failed waiting for the after marker. The failed log is
retained. The fix preserves that accessibility contract and adds one passive
Label per painted tab under the existing opt-in native diagnostic flag. It
reports `TabPaintRecord::visible_bounds()` directly; it has no action, receiver,
input authority or output-presentation receipt. Ordinary product use leaves it
disabled. The new unit test verifies opt-in behavior, exact full bounds and
unchanged drag semantics.

The runner now reads those full bounds and the original Close Main receiver.
It selects a point in the left half or the available right-half interval before
the close control, verifies the actual narrow insertion marker and rechecks
frozen pane/workspace, tab/close, native client and scale geometry before
physical release. The measured target points are (47,14) and (71,14) in this
scale-1 fixture. Before requires marker `[28,29] x [0,28]` and ordered
`[Child,Main]` tabs; After requires `[101.84375,102.84375] x [0,28]` and ordered
`[Main,Child]`. A pane-center preview cannot satisfy the insertion marker check,
even where its final tab order would match After.

Fresh qualification passes 42 Linux runner tests and all ten real journeys:
the two new tab halves, four edges, three centers and guide control. The
adapter's 70 all-feature unit tests, all 114 product and 15 official-egui tests
also pass. Independent read-only review finds no confirmed P1/P2 in the
geometry diagnostic, half selection, marker, frozen-coordinate or terminal
checks. Current source hashes and exact commands/results are
`/tmp/dockspace-physical-tab-half-snapshot.json`,
`/tmp/dockspace-physical-tab-half-qualification.json` and matching logs.

Only the finite runner and opt-in adapter diagnostic change. Core and fork
behavior retain their prior qualification; this is not evidence for clipped or
overflowed tab insertion, nested target competition, mixed DPI, native
stationary capture, hardware Touch or the complete U7/U8/platform matrix.
The environment remains Linux/arm64, Rust 1.95, Xvfb/Openbox, one X screen,
scale 1, a 2-CPU/4-GiB container and software GL with one renderer thread.
Native execution still requires the unpublished local fork overrides recorded
in platform support; remote-only dependency reproducibility remains open.

## 2026-10-31 opt-in release reducer wall-time measurement

The existing internal active-load fixture adds the explicitly ignored
`wall_time_active_prefix_and_receiver_deadline_preserve_exact_release` test.
Ordinary CI retains deterministic injected-clock assertions and does not assert
machine-dependent `elapsed <= 2 ms`. This opt-in test instead shares one
system `NativeMonotonicClock` among ingress age, reduction budget and measurement.
It records every `reduce_callback_prefix` plus `poll_host_frame` segment from
before immutable-prefix claim until poll returns a frame or None. All segments
within one outer turn retain the same original budget; a partial core replay
does not receive a new deadline to improve the measurement.

The fixture presses a real core receiver, admits 257 active moves, injects one
3 ms receiver delay across two distinct retained moves, releases, paints and
settles the exact output. It verifies raw ordinal continuity, one-time recording
and completion, the expired same-budget zero-progress poll, exact unstarted
receiver continuation and one Pending/Delivered pair sharing the manipulation
identity. Source ownership remains unchanged until that output settles.
Measurement scope is explicit: commit, painting, output settlement, logging
and actual return to the OS event loop are outside each measured segment.
The fixture creates no OS window or GPU submission and adds no public profiler,
clock owner or lifecycle registry.

The reviewed Linux/arm64 release run passes. Its nine segments include an active
256-record raw claim that completes 238 records at 2.005833 ms, followed by
18-record and one-record exact continuation. The deliberately delayed receiver
segment takes 4.702917 ms, completes only its current record, and leaves the next
distinct point untouched. The immediate same-budget poll records zero claims,
zero completions and zero receiver queries; the following fresh turn completes
the successor. The release and presentation segments take 28.833 and 38.167
microseconds in this finite sample. Two segments exceed 2 ms and remain in the
reported results. These are observations, not a hard upper bound or a latency
percentile. Cooperative expiry prevents starting subsequent core records;
preparation, the currently running record and OS preemption may overrun.

The reviewed macOS release run also passes with nine segments. Its active claim
completes 153 records at 2.00825 ms, then resumes exactly 103 and one records.
The delayed receiver takes 4.618084 ms including a measured 4.528833 ms sleep,
followed by the same zero-progress expired poll and exact successor. Release
and presentation segments take 66.459 and 39.542 microseconds. It likewise
reports two over-deadline segments. The test prints `debug_assertions=false`
and its measured scope in both platform logs; the initial release compilation
time is recorded separately from these segment samples.

Independent review found one fixture P2: injecting the delay only when both
moves had already been translated could reject legal partial translation after
OS preemption. The corrected test delays the first actual receiver query and
accepts its pending successor in either raw or core storage, retaining exact
point-order and expired zero-progress assertions. Both platform release runs,
nine adjacent controlled regressions normally and without native debug
assertions, correctness/suspicious Clippy, formatting and patch integrity pass
on the reviewed source. Earlier samples remain marked superseded in the local
evidence record. Production core, adapter and fork sources are unchanged.

Run the Linux qualification with its documented explicit local fork overrides:

```text
cargo test --release \
  --manifest-path integration/egui-fork-workspace/Cargo.toml \
  -p egui_dockspace_native --lib --locked --offline \
  --config /tmp/dockspace-linux-local-callback-egui.toml \
  wall_time_active_prefix_and_receiver_deadline_preserve_exact_release \
  -- --ignored --nocapture --test-threads=1
```

The current source and exact invocation/results are
`/tmp/dockspace-service-time-snapshot.json`,
`/tmp/dockspace-service-time-qualification.json` and matching logs. The full
host claim-to-event-loop-yield service envelope, sustained OS throughput,
broader layout sizes and the remaining U7/U8 criteria remain separate work.

## 2026-10-31 simultaneous W16 roster and O256 real outputs

The real output-pressure executable adds the finite `combined` scenario. It
selects I64/256 KiB, W16/A8/128 KiB, R258/E16/768 KiB and M48/C32/L16 within
the unchanged 4,194,304-byte native input-data allowance. This is an explicit
profile combining maximum window count with full ordinary output occupancy;
it does not saturate every independently configured input, display, effect,
property or close class at once.

Fourteen satellite roots move through the public application command queue,
real deferred-window creation, complete semantic content and exact first-live
acceptance before the fifteenth child enters the original staging-pressure
journey. Fixture identities come from actual pane output tokens; callbacks do
not themselves prove first-live. Unlike the five legacy W2 cases, the combined
case never pauses complete root-roster capture during filling. In the exact
root `on_output_begin` that returns Defer at O256/control2/protected-staging1,
its fresh borrowed roster must contain precisely all 16 expected native
window/viewport pairs once, in the same context. Runtime owned/reserved data
is checked while that complete occupancy still exists.

The proxy then forwards all 258 original output terminals in completion order.
Every child redocks through a checked public action, receives its exact native
`Destroyed` callback and loses semantic surface authority before quiescence.
The runner requires a fresh one-window root roster, the exact ordered
`[1..16]` Main tabs, no contained roots and zero ordinary/control output owners
after the final root terminal. Native Destroyed may omit a viewport identity
after backend map retirement; the fixture always requires the exact original
window id and also checks a supplied viewport id. The initial macOS attempt
reached full combined pressure but was terminated after the fixture incorrectly
required a nonempty Destroyed viewport; its failed evidence is retained.

The sampled normal macOS journey passes in 6.667 seconds and Linux/X11 in
13.816 seconds. Both the new combined case and all five preceding pressure
cases pass on each platform. macOS samples 2,352,033 owned / 3,455,424 reserved
bytes, Linux samples 2,352,530 / 3,457,752 bytes, beneath 4 MiB. The checked
initial reservation is 3,455,624 bytes on macOS. These figures exclude
allocator metadata, workspace/renderer state and fixture storage. The bounded
terminal proxy reports 18,576 bytes separately, with another 1,424 bytes for
the fixed satellite/expected-identity fixture. This is delayed real terminal
delivery, not 256 simultaneous GPU frames or physical input qualification.

The combined journey additionally passes on both platforms with native adapter
debug assertions disabled, sampling 2,350,249/3,455,424 bytes on macOS and
2,350,409/3,457,752 bytes on Linux. Independent static review finds no confirmed
P1/P2 in the combined roster, terminal-order, identity, lock or cleanup paths.

Only the smoke runner and its direct cross-platform Winit import change;
the latter matches native Destroyed without adding a public seam. Production
core/adapter/fork sources retain their preceding qualification. Current
correctness/suspicious Clippy passes. Source hashes, commands and results are
`/tmp/dockspace-w16-o256-snapshot.json`,
`/tmp/dockspace-w16-o256-qualification.json` and matching logs. Native execution
still uses the explicit unpublished local egui/Winit overrides described in
[platform support](PLATFORM_SUPPORT.md#dependency-reproducibility-and-open-scope).
Remaining close/create capacity combinations, actual service-time measurement,
native scoped/stationary input, broader physical coverage and U9 remain open.

## 2026-10-31 configured maximum roster and terminal fanout

The existing 4 MiB combined model profile now runs with all 16 configured
binding lifetimes simultaneously. Its complete 16-window roster coexists with
ordinary ingress and 32 reserved property callback receipts: one real fork-issued
focus identity and one visibility identity per window. Ordinary capacity accepts
1,360 raw cursor records before freezing. After that freeze, all 32 exact command
retirements and 16 original Destroyed callbacks still obtain their reserved
ordered ingress. No extra ordinary record, OS window or eframe output token is
fabricated for those terminals.

The candidate holding the transferred 16-window roster is dropped and retried
with unchanged published version/recorded input and the same complete envelope
and shared payload; the matching snapshot publishes exactly once. The preceding
empty freeze boundary cannot satisfy that retry check. Every accepted terminal
drains through the existing budgeted coordinator path, followed by exactly one
overload cancellation.
Application layout removal retires core authority, the existing protected
terminal roster publishes all 16 destroyed tombstones, and a dropped whole-batch
route-retirement preparation preserves every route for its retry. Quiescence
then removes all 16 recognized bindings and retained host references; the
following empty prelude also retires the committed quiescence prefix and core
guards. Repeated property terminals are inert. Independent review caught the
initial empty-frame drop and missing final reclamation boundary; the final
qualification reruns both corrected paths.

The maximum-roster case and three adjacent admission/accepted-release controls
pass 4/4 normally and 4/4 without native debug assertions on macOS. Both runs
sample 2,325,314 owned / 4,194,296 reserved bytes under the unchanged
4,194,304-byte allowance. The new maximum-roster case also passes 1/1 normally
and 1/1 without native debug assertions on Linux/arm64, sampling 2,325,282 owned /
4,194,288 reserved bytes with the same 1,360-record boundary.
This is owned input-data capacity, excluding allocator metadata,
workspace/renderer state and fixture allocations. The constant injected clock
isolates capacity from the independent age and wall-time budgets. Clippy,
formatting and patch integrity pass. Only one existing test file changes;
production source and earlier real-window qualifications are retained rather
than rerun. Source and exact commands/results are at
`/tmp/dockspace-maximum-roster-snapshot.json`,
`/tmp/dockspace-maximum-roster-qualification.json` and matching logs.

This closes the W16 profile's concurrent roster/property/Destroyed model boundary.
It does not combine the separate W2/R258 O256 real-output profile with maximum
roster pressure, fill close/create owner classes, prove active-release semantics
for this cursor-only fixture, or establish OS callback timing and the 2 ms
reducer-service boundary. The adjacent accepted-release case retains its own
one-window evidence. Full U7/U8/U9 remains open.

## 2026-10-31 X11 non-guide pane-edge docking

The physical runner now qualifies four additional finite X11/XTest journeys:
`EGUI_DOCKSPACE_PHYSICAL_TARGET=pane-edge-north`, `pane-edge-south`,
`pane-edge-east` and `pane-edge-west`. The respective normalized content points
are (0.30,0.16), (0.30,0.84), (0.84,0.25) and (0.16,0.25). Their physically
rounded positions must remain inside the named inner-pane basin, outside the
outer-root band and every actual renderer guide hit. The same checks run again
before release. No guide fallback or replacement point is allowed.

The target wait now dispatches exact preview validation by the requested basin.
For this one-pane initial root, an edge preview occupies the corresponding half
of the workspace after its splitter gap; a center preview still covers pane
content. The model observation independently identifies the actual layout:
center requires ordered [Main,Child] tabs, while an edge requires two singleton
tabs leaves with the expected axis/order and equal weights in the original root.
The driver requires the observed layout to equal its selected target. Merely
returning Child to the root cannot pass an incorrect merge or split.

A fresh North red run reproduced the original runner bug in 4.011 seconds:
`wait_for_pane_center_scene` always called `verify_center_preview`, rejected the
valid half-root rectangle until its deadline and then performed its explicit
XTest release cleanup. That release caused a legitimate split and retirement.
The release was not evidence of a synthetic Winit event or a core docking fault.
The same real journey passes after the target-aware wait correction; no
production core, adapter or fork code changes in this slice. Temporary native
event/early-retirement diagnostic logging has been removed.

Fresh Linux qualification passes all 40 runner tests, all eight real journeys
(four edge, three center, one guide control), correctness/suspicious Clippy and
formatting. Each successful journey still checks held source input, a painted
Dock program before physical release, final model, exact X11 DestroyNotify,
semantic viewport retirement and quiescence. A viewport step is only a
pass-progress barrier, not a native release callback receipt or GPU presentation
proof. The Linux container lacks nextest, so its attempted nextest invocation
is recorded as unavailable and the actual tests use serial `cargo test`.
An initial fixture compile error is retained separately from the passing run.

Only the runner's three Rust files differ from the preceding production source
qualification. Core/public/official/native production gates were not rerun for
this runner-only change. The eight fresh journeys use the same Linux/arm64,
Rust 1.95, Xvfb/Openbox, 2-CPU/4-GiB environment, one X screen, scale 1 and
software GL with one renderer thread. Source hashes and exact commands/results
are `/tmp/dockspace-physical-edge-snapshot.json`,
`/tmp/dockspace-physical-edge-qualification.json` and matching logs; the red
control is `/tmp/dockspace-physical-edge-red.json`.

Qualification still requires the unpublished local egui `70f7b517` and Winit
`6c10c87d` overrides. A direct tracked-pin check fails because the older remote
egui/Winit revisions lack the current native host seams; it is not a reproducible
native checkout. The 1-kHz extension remains center-only and rejects edge targets
before starting the gesture. This slice closes the finite four-direction
non-guide edge smoke gap, not tab-half insertion, nested target competition,
mixed DPI, native stationary capture, maximum-roster/O256 pressure, reducer
service-time qualification, the native showcase or U9 packaging/pin closure.
The complete plan and Goal remain open.

## 2026-10-31 L03 single-batch Burst qualification

The formerly known-invalid egui product Burst gate now passes on the current
source. The complete test sends pointer move, press, threshold move, target
move and release in one raw egui packet for every L03 basin and normalized
non-guide sample. The product executor publishes the magnetic Dock preview
and then settles the exact output and one terminal workspace mutation; no
compensating input frame, fixed-frame wait, guide targeting or ignored test is
used.

The focused fresh gate is recorded in
`/tmp/dockspace-l03-burst-qualification.json` and
`/tmp/dockspace-l03-burst.log`:

```text
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml \
  --all-targets --all-features --locked --offline --test-threads=1 \
  --no-fail-fast \
  -E 'test(shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics)'
```

The focused result is 1/1 passed, 0 ignored. The complete product harness
also passes 114/114 with no ignored tests. This closes the U3c Burst gate and
updates the L03 product executor from the previous 45-case cadence subset to
the complete 60-case matrix: five basins, three normalized non-guide samples
and four cadences. The remaining physical edge/nested/mixed-DPI native matrix
is a separate optional-host qualification.

## 2026-10-31 continuous X11 input and pending-release wake

This slice is locally qualified. The real X11/XTest extension uses
`EGUI_DOCKSPACE_PHYSICAL_LOAD=1khz` with an explicit pane-center-a/b/c target.
All actual content/workspace/guide exclusions and the real painted Dock preview
stabilize before the measured load. It then sends 1,000 distinct motions through
the original checked XTest requests and physical pointer queries, and immediately
releases without a subsequent inspection or idle drain. Starts are at least
1 ms apart; late wakes shift later starts without catch-up. Actual average rates
and maximum start gap are reported; this is not a fixed absolute clock grid.

The smoke-only probe retains one source identity and five fixed desktop points,
not an event history or another input owner. It requires the exact viewport,
window, device, desktop-point order and callback count. A new primary press,
source destruction or uncorrelated device removal before model observation
invalidates the measurement; normal source retirement after commit is allowed.
Native roster changes during injection fail. Early model success, incomplete
injection, missing callback facts and unavailable/invalid storage samples also
fail rather than borrowing a later gesture or an earlier successful sample.
Ordinary runs do not execute the optional callback timer or diagnostic mutex.

The measured interval is conservative:
native release callback <= accepted release <= typed Delivered/model atomic
commit <= first root model observation. It does not directly timestamp accepted
input or typed feedback, and callback counts do not prove every motion was
accepted by the adapter. Root UI duration is reported separately, and owned /
reserved maxima are root-frame-boundary samples of input data, not RSS or
instantaneous process peaks. Same-tabs model order, exact DestroyNotify,
semantic retirement and full quiescence remain required.

The load exposed two production gaps. One UI invocation consumed only one
short callback prefix despite output/lifecycle barriers splitting active motion;
raw backlog reached 250 records and the original 250 ms age limit cancelled the
drag. The adapter now commits multiple original prefixes using one unchanged
256-record/2-ms token budget, replaces only an inert RetainPrevious staged pass,
and exits on no progress, exhaustion, an empty queue or semantic paint. It does
not merge barriers or refresh the deadline. Separately, idle release obligations
were omitted from quiescence and could lose the next semantic/terminal wake.
Core now derives pending queries from its existing drag/contained/rehome state,
requests missing target paint, and avoids requesting duplicate paint once an
exact output is in flight. Native retains a precise after-current wake for
those outputs and for missing semantic work. No new registry or output owner is
introduced.

The final three high-rate runs use an optimized build, Docker
Linux/arm64, Rust 1.95, Xvfb/Openbox, a 2-CPU/4-GiB container and software GL with
`LP_NUM_THREADS=1`:

| Target | Injected / matched native callbacks | Injection / callback Hz | Release-to-model upper bound |
| --- | --- | --- | --- |
| pane-center-a | 1,000 / 1,000 | 990.2 / 989.3 | 69.195875 ms |
| pane-center-b | 1,000 / 1,000 | 999.8 / 999.7 | 36.474208 ms |
| pane-center-c | 1,000 / 1,000 | 999.8 / 1,000.2 | 37.324750 ms |

The first three successful runs report 992-1,005 callback Hz and 30-51 ms
release bounds; the final runs above retain every measurement instead of
selecting the fastest sample. Across all six, sampled input data reaches at
most 2,304,279 owned / 3,455,936 reserved bytes under the unchanged 4 MiB profile;
largest root UI update is 1.097583 ms. Failed attempts
remain explicit: an absolute-slot injector failed immediately, unoptimized
builds exceeded the original age limit, immediate-release runs exposed overload
or idle settlement stalls, and the fixed production source with unrestricted
software-renderer threads completed docking at only 669 Hz and correctly
failed its rate gate. None counts as a high-rate qualification.

All 37 main gates and six supplemental gates pass. The main workspace passes
1,894 tests; the external public consumer passes 130 default / 137 serde tests,
including the refreshed cross-surface pending-target assertions. Product and
official-egui consumers pass 114 and 15 tests. Native normal and assertion-disabled
workspaces each pass 496 tests, including both same-coordinator 10,000-lifetime
soaks. The Linux runner passes 37 tests. Ten doctests, strict core/native rustdoc,
correctness/suspicious Clippy on host and Linux, formatting, core dependency
isolation and verified core packaging pass. macOS default/bounded real-window
redock, all four non-load X11 physical journeys, and all five pressure cases on
each platform pass. Independent static review confirms the prefix/owner/wake
boundaries; a failed later candidate preserves already committed earlier prefixes
and the same budget. It does not make an entire UI invocation atomic.

An added no-repaint regression initially treated the first preview promotion as
equivalent; that promotion changes interaction semantics. The corrected case
actually presents the same program again before requiring no repaint. The two
new release tests also cover rollback of paint and settlement, Dropped output,
and later output not replaying a cancelled release. Contained and root-rehome
tests cover the shared pending query. The native regression isolates pending
release as the only remaining reason not to report quiescence.

Final evidence is `/tmp/dockspace-physical-load-complete-snapshot.json`,
`/tmp/dockspace-physical-load-final-qualification.json` and
`/tmp/dockspace-physical-load-supplement-qualification.json`; exploratory and
failed logs retain the same prefix. All 440 Rust sources match the final source
manifest. The only post-freeze test change was the public consumer assertions,
which were rerun in both configurations. Native qualification still uses explicit unpublished local
fork overrides. This finite profile does not certify hardware 1 kHz, the 2 ms
reducer service boundary, maximum-roster/O256 cross-saturation, non-guide edge
and nested native targets, mixed DPI, or automatic stationary capture. Full
U7/U8/U9 remains open.

## 2026-10-30 combined native capacity and accepted release

One test now composes actual obligations in the same bounded 4 MiB profile:
an active drag, prepared focus/visibility callback receipts, a complete one-window
roster and raw backlog. Successor command receipts are explicitly rejected while
the original two remain pending. The source accepts its release before ordinary
capacity rejects a later motion. Repeating that rejected source spends no ordinal
or core recorder/version progress. The two reserved command retirements remain
admissible after the ordinary freeze.

The drain shares one budget from prelude through callback/core polling in each
pass. It drops the release candidate once and retries that recorded immutable
edge. Before its exact output settles, every committed report must preserve
source ownership; a Delivered(Dock) report requires the previously produced
artifact's matching settlement. The complete trace contains exactly one
Pending(AwaitingPresentation) and one Delivered(Dock), with the same manipulation
identity, one output settlement and one overload cancellation. Both commands
and the recovery terminal roster retire, duplicate command terminals are inert,
and backlog, pending input/owners and gesture state reach quiescence.

The new combination and the adjacent controlled-1kHz and original capacity-freeze
regressions pass **3/3 normally and 3/3 without native debug assertions**.
The combination retains 743 ingress records, two command receipts and one roster
window; it completes 750 budgeted records and samples 2,780,113 owned input-data
bytes. The test continuously checks owned <= reserved <= 4 MiB. It does not
require the 4 MiB profile to fit all 4,096 raw slots. The neighboring controlled
model retains all 10,000 moves and observes 31 ms release latency on its injected
clock, with a 16-record/15-ms backlog; this is not real-time or OS input evidence.

Independent review found the first test only aggregated final output/feedback
counts; the final version adds the per-report settlement/ownership assertions
above. The five final gates pass: normal/noassert focused tests, Clippy,
formatting and patch integrity. Only this test file changes; production sources
are byte-identical to `/tmp/dockspace-physical-basin-snapshot.json`, so the prior
full core/native/real-window qualification is retained rather than rerun.
Evidence is in `/tmp/dockspace-combined-pressure-snapshot.json`,
`/tmp/dockspace-combined-pressure-qualification.json` and matching logs.

The fixture uses real affine core outputs and fork-issued command identities,
without inventing eframe output tokens. Platform input effects use their actual
dispatch-failure path; no OS command or GPU presentation is claimed. This
finite pressure combination is not the O256 output configuration, maximum
roster/fanout, all-limits simultaneous saturation, RSS or physical 1 kHz latency.
Those complete U7 acceptance boundaries remain open.

## 2026-10-30 X11 non-guide pane-center docking

This slice is locally qualified by 36 final gates. The physical X11/XTest
runner retains its
Center-guide control and adds three fixed pane-content points, at normalized
(0.35,0.35), (0.65,0.35) and (0.65,0.65). It freezes actual content/workspace
bounds before press, uses the current client origin/scale for physical rounding,
and checks the round-tripped point against every actual guide hit, including
repeated directions from separate clusters. Default presentation configuration
is only a fixed-smoke selection precondition; it is not a target-solver oracle.
No candidate falls back to a guide or another point.

All five renderer guide directions publish passive labels over their actual
receiver hit bounds. The opt-in native diagnostic also emits a same-surface
Dock marker after its preview shapes enter the program, using the real preview
rectangle. It is always `painted`: core layout projection currently applies to
Contained previews, not Dock. Independent review rejected the initial helper
test's impossible Dock+projected combination. The corrected marker means
program emission only, not GPU presentation, current receiver authority or
release delivery. The original Native-preview diagnostic keeps its conditions.

The runner waits for two increasing complete semantic scene samples with the
same roster epoch, scale, content/workspace, all guide hits and real Dock preview.
Same-epoch missing control outputs retain the preceding complete sample, as in
the existing selector wait; Missing never returns success or fills the current
scene. An old side preview after source motion resets the baseline and retries
within the deadline. Illegal nodes/values or changed target geometry fail.
The final point is rechecked against current guides and physical bounds, source
held-input observations and the actual X11 button state before real release.
Success additionally requires actual model views to place both Main and Child
in the original root/tabs as [Main,Child], followed by exact X11 DestroyNotify,
retired semantic inventory and quiescence. A step or painted marker never
substitutes for that terminal journey.

All four final real journeys pass: the original guide control and each of the
three non-guide points. The final post-retirement lookup also uses a bounded
stable semantic wait; a one-shot lookup had landed in a legitimate control
output after the model/order and OS retirement checks passed. That failed
attempt is retained at
`/tmp/dockspace-physical-basin-linux-physical-pane-center-a-before-retirement-wait.log`.
Initial scene-wait failures and the independent transition-preview finding
remain recorded as well. No failed attempt is counted as a pass.

Fresh qualification passes workspace1,891/public130/serde137, product114,
official-egui15, doctest10 and native494/494, including both same-coordinator
10,000-lifetime soaks. macOS default/bounded redock and five pressure cases per
platform pass. The bounded sample remains 2,302,971 owned / 3,455,424 reserved
bytes within 4 MiB/4,096 raw slots. Linux runner32 tests, correctness/suspicious
Clippy, strict docs, formatting and core dependency isolation pass. Both
independent reviews report no remaining confirmed P1/P2. The final runner-only
post-retirement refreeze is followed by fresh Linux compilation, runner tests,
all four real journeys and formatting; product/renderer source did not change.
All 439 Rust paths match `/tmp/dockspace-physical-basin-snapshot.json`; full
commands/results and detailed logs are under `/tmp/dockspace-physical-basin-*`.
Fork source remains unchanged from the preceding Touch qualification.

These three native smoke points are additional finite samples, not the shared
L03 complete60-case matrix. They do not cover non-guide edges, tab half
insertion, multiple nested targets, mixed DPI or high-rate physical input,
and do not complete U7/U8.

## 2026-10-30 native Touch event-time coordinate pairs

This slice is locally qualified by 34 final gates. The Winit Touch schema now
carries the existing PointerEventFacts. Wayland down/motion reads physical position and
content geometry under the same window lock and keeps one private Copy sample
in the existing per-seat touch map. Motion replaces the complete pair. Up and
cancel have no new position, so the production event constructor preserves the
last sample without consulting current size or scale. Cancellation skips a
missing window and continues processing other contacts. Other backends supply
default facts; no capture, desktop, hover or modifier capability is inferred.
The coordinate-less terminal contract follows the
[Wayland protocol](https://wayland.freedesktop.org/docs/html/apa.html#protocol-spec-wl_touch).

The native adapter reads original Touch facts and checks that the finite contact
position exactly matches its local sample. Original geometry selects NativeLocal;
failed geometry/capability checks remain LocalUnknown without desktop fallback.
Missing original geometry preserves the former DesktopUnknown lane. Touch never
emits a dual-location handoff, and no registry or token issuer is added. Callback
clones retain the same immutable shared payload, identity and original time.

Eight focused adapter tests pass, including the seven new fact/pair tests and
an existing touch/pan compatibility case. The Linux Winit library passes all 40
unit tests, including four production sample/constructor regressions for old
geometry terminals, independent contacts, whole-pair motion replacement and
rounding. These are deterministic contract tests, not Wayland compositor or
physical touch evidence. The first producer filter matched zero tests; that
attempt is not counted, and the full library run confirms all four tests execute.
The old-pair adapter regression currently proves translator ContactEnded/tail/ID
handling; existing core public native Touch tests qualify core terminal semantics.

Affected qualification passes native491/491 normally and without debug
assertions, including both same-coordinator 10,000-lifetime soaks; eframe
Glow94/WGPU88/Linux89, no-seam compilation, strict docs, Clippy, formatting,
macOS default/bounded redock, X11 real observation and physical guide-targeted
redock, and five output-pressure cases on each platform. The new bounded
root-boundary sample is 2,302,971 owned / 3,455,424 reserved bytes within the
same 4 MiB/4,096-raw-slot contract. Its increase reflects actual inline Touch
facts; capacity formulas continue using full entry sizes without double charging.
This remains a sampled input-data bound, not combined saturation or RSS proof.

All 439 main Rust paths match the native snapshot. Core/public/official-egui
sources are unchanged from their prior qualification; those semantic results
were not rerun here. Source, commands/results and detailed logs are at
`/tmp/dockspace-native-touch-facts-*`. Initial local-override lock preparation
failures are preserved and are not passing gates. The excluded generated
lockfile delta is unchanged from the previous fork prerequisite.
Independent frozen producer/adapter review finds no remaining confirmed P1/P2.
The qualified Winit commit is `6c10c87da2a06871371de7d30fedc656cf7c3681`; egui remains at
`70f7b51712284561d3408950273f128ff556a81b`, both unpublished.
Fresh native capture and automatic stationary scheduling remain unavailable;
this slice does not certify OS Touch, other backend implementations, full
U7/U8 or dependency packaging.

## 2026-10-30 native pointer observation prerequisite

The new fork seam is locally qualified by 34 final gates. It does not enable
automatic native stationary gestures. Winit can query an exact enabled X11
master pointer after native event expansion has ended, with pre/post pending-event checks. One root
XIQueryPointer reply supplies both the current server-logical desktop point and
its limited root-child hover. A direct known Winit child is identified; a WM or
foreign frame remains Unknown. No local geometry or position is reconstructed.

Independent review rejects retained implicit capture as queried authority.
An ancestor unmap/remap can end a grab without the client receiving its own
UnmapNotify or a guaranteed Ungrab crossing. A pre-command queued press can also
overwrite local invalidation after an ungrab command. XIQueryPointer identifies
neither a current grab owner nor a protocol freeze. Capture and delivery thus
remain Unknown. A multi-reply foreign hierarchy traversal is also rejected:
unchanged root child/point checks cannot prove that all intermediate queried
edges coexisted. The final implementation uses a single reply and adds no
capture registry, topology model or server-wide grab.

Eframe reuses its existing property command queue and retirement owner. A
prepared opaque token, viewport, expected physical window and exact DeviceId
correlate the result. A replacement is never queried for its predecessor; an
unavailable or unsupported response is terminal, and retirement preserves
reentrant successor work. The result callback only reports facts. It creates
no CursorMoved, docking input, polling timer or automatic continuation.
The native adapter treats these currently unowned observation retirements as
Unreserved; they cannot claim focus, visibility or pass-through receipts.

The eframe gates pass 94 Glow tests, 88 WGPU tests and 89 Linux tests with both
renderers, including seven new request/result, unknown-fact, successor and
retirement tests. Compiling both renderers without the seam also passes. A real
X11/XTest opt-in journey passes with override-redirect A/B windows: while the
primary button is held, input stays delivered to A and queries follow two
target positions in B; repeat stationary queries generate no
motion, native-event expansion and undispatched prefixes return Unavailable,
and an invalid device or destroyed expected window cannot produce a sample.
The affected native gates pass 484 tests normally and 484 without native debug
assertions, both with the same-coordinator 10,000-lifetime soak. macOS
default/bounded programmatic redock, the X11 physical guide-targeted docking
journey and five output-pressure cases on each platform pass. The bounded sample
remains 2,146,235 owned / 3,298,688 reserved bytes within 4 MiB/4,096 raw slots.
These gates are tied to `/tmp/dockspace-pointer-observation-snapshot.json`; they
are not complete native-readiness or automatic stationary certification. The
native query test does not separately isolate an event arriving between the pre/post query fences, a WM wrapper,
multiple master pointers or device removal. Its positive samples follow real
motion delivery, so the no-cache conclusion additionally relies on production
source inspection. Eframe's queue tests exercise the native-query boundary with
recorded results; the X11 test exercises the real Winit query, not an automatic
eframe-to-docking stationary journey.

The registry audit still reports egui 0.37.0 as the latest stable release
(registry update 2026-10-15). Winit remains stable at 0.30.13; 0.31.0-beta.4 is
a prerelease (registry update 2026-10-24), outside the stable baseline contract.
egui_tiles is now 0.18.0 (2026-10-19), but remains reference-only and is not a
product dependency. This slice therefore does not change these baselines.
The qualified local commits are Winit
`98642f5e8460677a5b96bcefb611ac8af437da9b` and egui
`70f7b51712284561d3408950273f128ff556a81b`. Both remain unpublished. All 438
main Rust paths match the native snapshot, with only the two adapter retirement
matches changed from the previous qualification; core/public/official-egui
source is unchanged. The preceding workspace1,889/public130/serde137 results
are preserved by exact source identity, not rerun for this fork prerequisite.
Independent producer and eframe reviews report no remaining confirmed P1/P2
within this restricted observation contract.

Native and eframe correctness/suspicious Clippy, strict docs, formatting and
patch integrity pass. Evidence retains the initial Linux offline cache miss,
the eframe docs feature-combination correction, missing Linux Clippy component
and nightly-rustfmt failure. The final refreeze moves only the existing SAFETY
comment and formats one test string; the real producer test, lint, docs and
format gates pass again. Source manifests, commands, results and registry audit
are under `/tmp/dockspace-pointer-observation-*`. The eframe local-override
lockfile delta was excluded from commits. Tracked remote native fork pins still
do not reproduce these seams; dependency packaging and U7/U8 remain open.

## 2026-10-30 public native stationary requests

The existing pointer gesture now projects a Native-specific stationary request
without a second token issuer or scheduling registry. It identifies the original
source binding, pointer/event/token, target point/deadline and, for NativeLocal,
the still-current local-coordinate capability. Surface and Native projections
filter their own provider domains. Pending recorded native input suppresses
native requests; the adapter must independently drain its accepted mailbox
before future automatic scheduling.

Timed/touch entry points explicitly admit NativeJoined NativeLocal/Desktop
lanes while leaving standalone DesktopGlobal and `scope.is_local()` unchanged.
Native execution requires exact original Primary press, current source binding,
capture/delivery and local generation. Unknown capture revokes a pending
challenge; a stale token cannot reinterpret an old coordinate lane or ask new
receivers. Known native capture loss still cancels the gesture. Desktop Touch
uses the validated source-delivery point and retains Touch-specific hit geometry.

Public recording-renderer qualification covers hover dwell, Touch
long press/tap/yield, exact-output edge scrolling, expanded Desktop Touch
splitters, read-only/domain isolation, coordinate/capture/receiver loss and
obsolete tokens after handoff. The first two target fixtures needed explicit
registration of their scroll receiver and correct use of the splitter drag
lane. Pending release has already consumed the active drag decoration, and an
obsolete native receiver is conservatively compiled as Unknown; only a current
but contradictory winner is a protocol error which rolls back the whole frame.

Independent review found a real capture-revocation gap: non-Primary native
button edges updated ledger capture but did not clear the retained challenge.
Unknown capture followed by exact capture could therefore restore the old
deadline. The public regression fails before the fix and passes after routing
matching native button edges through the shared capture-change reducer. The
same-pointer scroll path now clears Pressed Touch deadlines and observes capture
loss while continuing the original scroll reduction. The initial full run was
intentionally interrupted after this finding; it is not a passing gate. Red,
green and interrupted evidence are retained under
`/tmp/dockspace-native-stationary-secondary-*`,
`/tmp/dockspace-native-stationary-capture-final-green.log` and
`/tmp/dockspace-native-stationary-before-capture-fix-*`.

Cross-window stationary hover also requires the exact source's confirmed
Applied route before scheduling or executing on a different surface. Its public
trace uses different source manipulation and target hover points, preserves A's
binding through B's dwell selection and restores routing on the real terminal.
Wrong pointer/reused-stream tokens remain structural journal rejections.
All 29 fresh gates pass: workspace1,889, public130/serde137, native484/484
normally and without native debug assertions, ten doctests,
correctness/suspicious Clippy, strict docs, formatting and patch integrity.
Both native configurations run 10,000 lifetimes on the same coordinator. The
core dependency boundary passes. Fourteen new public journeys, parameterized
across both native lanes and authority failures, qualify the explicit host path.
Independent runtime and execution reviews find no remaining confirmed P1/P2.

macOS default/bounded programmatic redock, X11/XTest physical tear-off,
first-live, guide-targeted redock, exact DestroyNotify/inventory retirement and
quiescence, and five output-pressure cases on each platform pass. The bounded
root-boundary sample remains 2,146,235 owned / 3,298,688 reserved bytes within
the unchanged 4 MiB/4,096-raw-slot contract. It is input-data capacity evidence,
not RSS or complete cross-saturation qualification.

All 438 Rust paths match the native qualification snapshot. The last added
scroll fixture needed one cargo-fmt-only line wrapping correction after its
passing semantic gates. Current bytes are verified equal to rustfmt applied
to that frozen tested file; production source is unchanged. The original format
failure is retained at
`/tmp/dockspace-native-stationary-format-public-before-fmt.log`, followed by
passing format and remaining gates. Source/provenance and full commands/results
are at `/tmp/dockspace-native-stationary-snapshot.json` and
`/tmp/dockspace-native-stationary-qualification.json`; detailed logs use
`/tmp/dockspace-native-stationary-*`.

This slice does not schedule native adapter wakes or claim fresh OS stationary
observations, native Touch event geometry, native-local OS handoff or complete
U7/U8 qualification. Unpublished fork overrides and dependency packaging remain
separate work, as do real-time input load and non-guide physical docking.

## 2026-10-30 native callback handoff emission

The follow-up adapter slice selects a dual-location packet only for an original
CursorMoved with event geometry, an Exact local sample and finite desktop
coordinates. The immutable callback payload retains that sample while the
original WindowEvent owns desktop coordinates. Translation uses those frozen
facts and routes without consulting the gesture or adding a lane registry.
Button/wheel selection and X11 synthetic focus handling keep their existing
boundaries. Dual cursor compaction compares exact coordinate authority.

A complete raw callback/mailbox/translation/core regression fails against the
previous adapter at the first handoff preview, while its ordinary Desktop
control passes. That red evidence is retained at
`/tmp/dockspace-handoff-emission-red.log`. The journey also checks frame retry,
original source credit/age, late local preview rejection and exact Desktop
output delivery. The same journey now passes, alongside its ordinary Desktop
control and six new inline capture/translation/compaction cases. All 25 focused
tests pass; green evidence is at `/tmp/dockspace-handoff-emission-green.log`.

All 21 affected native gates pass: 484 tests normally and 484 without native
debug assertions, including both same-coordinator 10,000-lifetime soaks,
correctness/suspicious Clippy, strict docs, formatting and patch integrity,
macOS default/bounded programmatic redock, X11/XTest physical tear-off,
first-live, guide-targeted redock, exact destruction/retirement/quiescence, and
five output-pressure scenarios per platform. The bounded root-boundary sample
remains 2,146,235 owned / 3,298,688 reserved bytes within 4 MiB and 4,096 raw slots.
It is input-data capacity evidence, not RSS or combined cross-saturation.

Only three native adapter source paths change. Core/public/official-egui Rust
sources are hash-identical to the preceding handoff qualification, so its
workspace1,889/public116/serde123/doctest10 and static gates are reused rather
than rerun. All 435 Rust paths match the updated native snapshot. Independent
review finds no confirmed P1/P2. Source/provenance and commands/results are at
`/tmp/dockspace-handoff-emission-snapshot.json` and
`/tmp/dockspace-handoff-emission-qualification.json`.

This qualifies callback fact capture/emission and full adapter-to-core traces.
It does not prove a real native-local OS handoff, Wayland presentation,
native touch/stationary scheduling, non-guide physical docking or complete U7/U8.
The unchanged unpublished local forks still need explicit local overrides.

## 2026-10-30 explicit native-local to desktop handoff

`NativeLocalToDesktopHandoff` carries the original local sample and independent
desktop facts for one event through the existing joined recorder. Recording
does not inspect the active gesture. Only an ordered Moved edge on an active
NativeLocal drag can upgrade, after proving the same provider, stream, session,
source binding/generation, original Primary press and observed/gesture/event
capture. The source's current observed desktop content origin must map both
physical samples and their frozen logical conversion exactly. A hover on B is
queried using B's desktop projection, independently of A's manipulation point.

The private projection retains the raw journal edge and supplies one effective
view to candidate generation, receiver validation and reduction. All changes
remain in the existing candidate transaction. Upgrade rebinds the existing
gesture and its exact source continuation, clears preview/acquisition,
affordance, timers and lane-sensitive position state, and preserves source
identity. Invalid upgrades process the original local event; ordinary Desktop
input cannot promote a Local gesture. Physical cross-window drop still requires
the existing exact pass-through effect to be observed Applied.

Eleven public traces exercise same-window and confirmed cross-window delivery,
input/presentation frame retry, Dropped output, inactive-tab source continuation,
invalid mapping/capture/pointer/origin, coordinate rebase, foreign-batch rejection,
late local output, wrong-winner suffix rollback, independent A/B points and
local terminal fallback. The first public run exposed three fixture errors:
two expected a consumed preview to remain current after layout publication, and
one requested a newly selected scene before the input commit. The corrected
fixtures use exact settlement plus Delivered(Dock), and paint the new scene
after its committed measurements. Initial failures remain recorded at
`/tmp/dockspace-handoff-public-focused.log` and
`/tmp/dockspace-handoff-inactive-red.log`.

All 29 frozen gates pass: workspace1,889, public116/serde123, native476/476
normally and without native debug assertions, ten doctests, correctness/suspicious
Clippy, strict docs, formatting and patch integrity. Both native configurations
run 10,000 lifetimes on the same coordinator. The core dependency boundary also
passes. Independent identity/coordinate and transaction/output reviews find no
remaining confirmed P1/P2. All 435 Rust paths match the native snapshot.

macOS default/bounded programmatic redock, X11/XTest physical tear-off,
first-live, guide-targeted redock, exact DestroyNotify/inventory retirement and
quiescence, and five output-pressure cases per platform pass. The bounded
root-boundary sample stays at 2,146,235 owned / 3,298,688 reserved bytes under
the unchanged 4 MiB/4,096-raw-slot contract. These are measured input-data
capacities, not RSS or complete cross-saturation evidence. Source hashes and
commands/results are at `/tmp/dockspace-handoff-snapshot.json` and
`/tmp/dockspace-handoff-qualification.json`; detailed logs use
`/tmp/dockspace-handoff-*`.

This qualifies the public host handoff contract and its reference regressions.
The callback-emission entry above separately qualifies the adapter path. Neither
enables native stationary scheduling or proves a native-local OS transition.
The unchanged unpublished local forks still require explicit overrides, and
remote pins do not reproduce them. U7/U8, non-guide physical acceptance,
dependency packaging and the overall plan remain open.

## 2026-10-30 native callback local coordinates

Winit now freezes native scale and client size together with physical mouse
coordinates on macOS and Wayland. X11, Windows and other unqualified paths keep
the new optional event geometry absent. eframe independently samples callback
geometry using one native-scale read and the same `f32` presentation-scale
calculation as its window snapshots. Client size remains available without a
desktop origin, and a client resize revokes retained output even without one.

The adapter freezes event/callback/committed geometry into an opaque sample
under the existing mailbox lock. One immutable shared callback payload owns
routes and the local location; raw WindowEvent remains inline. Complete payload
data and worst-case translator growth enter one shared preflight before journal
identity admission. Retry shares the allocation, and single-event processing
drops its temporary clone before returning source capacity. Lifecycle callbacks
do not allocate pointer payloads. The existing viewport row refreshes its
capability after committed reports, including first-live and partial input
commits without a queued platform snapshot. There is no capability registry.

Local capture requires explicit original event geometry. Independent review
found that X11's synthetic focus CursorMoved has neither event geometry nor a
desktop point; treating missing desktop position as NativeLocal cancelled an
armed Desktop gesture. Event and complete raw-to-output regressions fail before
the positive-geometry guard and pass afterward. Unknown local coordinates never
authorize idle compaction; exact samples compare their coordinate authority
without comparing positions or allocation addresses.

Current focused evidence passes 34 adapter tests, including three complete
raw capture/mailbox/translator/core journeys: size-only reorder with frame
retry and exact output settlement, an old-scale terminal followed by pointer
reuse, and a Desktop gesture surviving the synthetic X11 focus move. Each uses
real semantic receiver descriptors and retains original age/device metadata.
An accepted preview may retire when its delivery changes layout in the same
commit; exact settlement and one Delivered(Dock) prove the terminal, rather than
requiring that old preview to remain the new layout's current presentation.

Winit unit tests pass 15 cases on macOS and 36 on Linux (serial cargo test in
the container, which has no nextest). Eframe passes 87 Glow, 81 WGPU and 87
assertion-disabled combined-renderer tests, checks both renderers without the
seam, correctness/suspicious Clippy and strict docs. The initial strict-doc
command omitted optional features referenced by existing unconditional links;
its failure and corrected inspection/persistence command are both preserved.

All 29 frozen main/adapter gates pass: 1,874 workspace tests, 105 default / 112
serde public-consumer tests, 476 native tests both normally and without native
debug assertions, ten doctests, correctness/suspicious Clippy, strict rustdoc,
formatting and patch integrity. Each native configuration runs 10,000 lifetimes
on the same coordinator. An initial native run passed 473/476: three existing
fact-equality fixtures omitted the newly explicit client-size field. Updating
only those expectations makes the full native gates pass; the original failed
attempt is retained. All 433 Rust paths match the native qualification snapshot.

macOS default/bounded programmatic redock, X11/XTest physical tear-off,
first-live, guide-targeted redock and destruction, and five output-pressure
scenarios per platform pass. The bounded root-boundary owned-input sample peaks
at 2,146,235 bytes, with 3,298,688 reserved under the unchanged 4,194,304-byte
limit and 4,096 raw slots. These figures count input data under the existing
capacity contract, not whole-process RSS or all possible cross-saturation.

The clean local forks are Winit `e8dd500fd5a78884de0092db81f60e8649da18aa` and
egui/eframe `154ac8e77e3af81aa7ccb9c8c0f8a7769f339c3f`. Both use explicit local
overrides for qualification; tracked remote dependency pins do not reproduce
these unpublished commits. Source hashes and complete commands/results are at
`/tmp/dockspace-local-callback-final-snapshot.json` and
`/tmp/dockspace-local-callback-final-qualification.json`; fork evidence is under
`/tmp/dockspace-local-callback-fork-*` and
`/tmp/dockspace-winit-local-geometry-*`. X11 focus-gap red/green evidence is
retained under `/tmp/dockspace-local-callback-x11-gap-red.log` and
`/tmp/dockspace-local-callback-adapter-green.log`.

This does not qualify real native-local OS input, Wayland presentation, positive
in-gesture local-to-desktop handoff, native touch/stationary scheduling, combined
capacity cross-saturation or non-guide physical docking. X11 continues to use
the existing desktop lane; its cached DPI/size state does not yet justify an
event-local geometry claim.

## 2026-10-30 joined native local pointer

The joined backend now freezes a NativeJoined host domain. It accepts desktop
or native-local event locations through the same recorder and sequence. Ordinary
pointer enrollment cannot mint this scope. Headless HostLocal/SurfaceLocal guards
remain unchanged, and `is_local()` does not enable native timers. The existing
gesture owner freezes a lane at a validated press and retains it through pending
release; the next gesture on the same pointer may choose again.

`NativeLocalCoordinates` is an opaque projection of the committed exact backend,
binding, existing coordinate generation and local size/scales. It does not retain
desktop placement or observation sequence. Capture compares the event-generation
size/native scale against callback geometry and the committed tuple, then freezes
the physical point and one presentation-scale conversion. The reducer rechecks
current host, binding, generation and point at the original input ordinal. No
new coordinate issuer, input producer, gesture map or lifecycle registry is added.
Continuation tokens keep their stream/session/generation reference while the
existing gesture retains the lane, avoiding growth of every pointer edge.

Native-local manipulation preserves real Native delivery/capture and binding
retirement. A local hover can query only that exact endpoint and needs current
output authority; a source continuation cannot authorize stale hover. Finite
client-area exit is retained without granting desktop OutsideAll authority.
Missing or incompatible terminal coordinates cancel the old owner while the
real edge commits, preserving ordinary pointer reuse and pending-output rules.

All 29 frozen-source gates pass: 1,874 workspace tests, 105 default / 112 serde
public-consumer tests, 458 native tests both normally and without native debug
assertions, ten doctests, core/native correctness and suspicious Clippy, strict
rustdoc, formatting and patch integrity. Each native configuration completes
10,000 lifetimes on the same coordinator. All 432 Rust paths match between the
main tree and native snapshot. Nine new public journeys cover size-only reorder
with deferred exact output and frame
retry, Dropped output, geometry mismatch, foreign binding/session and atomic
admission, committed-only capability grant, stale samples behind a new snapshot,
outside/Unknown terminals, inactive-tab continuation, and choosing Desktop for
the next gesture on the same pointer. Capability identity also remains stable
across identical newer observations.

Independent review found a missed native focus gate: NativeJoined with a complete
empty pointer roster was treated as Unknown. A real recorder/frame regression
first fails to reveal the historical pane; accepting the joined native domain
repairs it. The KnownDown and Unknown cases remain suppressed. All three focused
focus cases pass; red/green logs are under
`/tmp/dockspace-native-local-focus-{red,green}.log`.

macOS default/bounded programmatic redock and five output-pressure scenarios
pass. The bounded root-boundary sample remains at 3,000,339 resident / 4,152,792
reserved bytes under the unchanged 4,194,304-byte limit. X11/XTest physical
tear-off, first-live, guide-targeted redock and destruction pass, as do its five
output-pressure scenarios. This does not certify non-guide physical acceptance.
The source manifest and complete commands/results are retained at
`/tmp/dockspace-native-local-final-snapshot.json` and
`/tmp/dockspace-native-local-final-qualification.json`. The qualification uses
local egui fork `86feb7f8cc49217bff54a8924b2cb525f4af3285` and remote-locked Winit
`f378b50f29cafafec31cd8675c17646b4ef57d70`; preparation in the separate local Winit
reference checkout was not used. The tracked remote egui pin still does not
reproduce the local egui fork.

This slice targets the public engine host. The egui adapter still uses Desktop;
Winit event-generation geometry, eframe callback geometry and immutable adapter
sample capture remain to be connected and qualified. Positive local-to-desktop
handoff, native stationary scheduling and the broader U7/U8/platform gates remain
open. Existing Desktop real-window regressions do not prove a native-local OS path.

## 2026-10-30 native local geometry

The existing coordinate envelope now separates required physical client size
and native/presentation scales from optional desktop content/outer bounds.
Known content bounds establish their size; independently supplied conflicting
size rejects before recorder publication. Size-only snapshots can authorize
local scene compilation and exact presentation, including DirectPrepared
first-live. Desktop conversions and placement preserve the original Unknown
reason and fail without a real content origin. Recovery reports the same
typed authority gap instead of inventing an origin.

The existing coordinate observation generation, registry row and scene capture
continue to own all these facts. Size, position, outer bounds and scales remain
in the existing generation comparison; changing any authoritative geometry
revokes old scene authority. No second coordinate registry, sequence issuer,
pointer producer or gesture owner is added. Standalone local-input guards and
the managed native desktop-only input API are unchanged in this prerequisite.

All 1,851 workspace tests and 96 default / 103 serde independent-consumer tests
pass on the frozen 13-path Rust change. New cases cover size-only local paint and
exact output, stable selection with frame retry, contradictory sizes and producer
watermarks, direct first-live, origin withdrawal/return and old-output retirement,
and rejection of desktop pointer input without a real desktop origin.

Independent review found that an authoritative local update could overwrite the
historical desktop projection used by destruction recovery. A failing unit and
whole-forest lifecycle regression demonstrated the loss. Recovery now retains
the last complete observed desktop tuple rather than mixing an old origin with
new scales or sizes. Local minimum measurements compare with the same binding's
last actual local snapshot, not its historical placement projection. An existing
row with unavailable coordinates or another binding cannot borrow that cache;
only the original removed-row retry retains its historical fallback. Rooted and
rootless forests survive two size-only generations, fresh presentation and
destruction with all root/item identities and ordering intact. Live desktop
routing remains unavailable throughout. The focused geometry/recovery suite
passes 83 cases; the full gates include these cases and the remaining regressions.

All 29 frozen-source gates pass: 1,851 workspace tests, 96 default / 103 serde
public-consumer tests, 458 native tests both normally and with native debug
assertions disabled, ten doctests, core/native correctness and suspicious Clippy,
strict rustdoc, formatting, and patch integrity. Each native configuration runs
10,000 lifetimes on the same coordinator. All 430 Rust paths match between the
main tree and native qualification snapshot.

macOS default and bounded programmatic redock reach quiescence. The bounded
root-boundary sample peaks at 3,000,339 resident / 4,152,792 reserved bytes under
the unchanged 4,194,304-byte limit. Both macOS and X11 pass ordered, duplicate,
reordered, late-staging and late-first-live output-pressure scenarios. X11/XTest
also passes physical tear-off, first-live, guide-targeted redock and destruction;
this smoke does not certify non-guide physical acceptance.

The cache unit and forest red runs, the intermediate cache-only failure and final
green recovery run are retained under `/tmp/dockspace-local-geometry-recovery-*`.
Final source hashes and commands/results are in
`/tmp/dockspace-local-geometry-final-snapshot.json` and
`/tmp/dockspace-local-geometry-final-qualification.json`. Native qualification
uses the explicit local egui override at `86feb7f8cc49217bff54a8924b2cb525f4af3285`;
the tracked remote pin does not reproduce this local fork. Joined native local
input, handoff, stationary scheduling, combined-cap cross-saturation and the
broader U7/U8/U11 physical gates remain separate work.

## 2026-10-30 native preparation strategies

Native enrollment explicitly freezes DirectPrepared or Staged for the provider
lifetime; CreateWindow and RequestReplacement carry that strategy. Creation has
its own typed acknowledgement, independent from Show. Created, Prepared and
renderer-preparation failure are complete-snapshot facts in the existing binding
record and use the existing window observation sequence. There is no separate
readiness registry or sequence issuer.

Both strategies produce one private readiness proof and enter the existing
prospective-program and exact first-live pipeline. DirectPrepared accepts the
first Prepared/create acknowledgement in one post-emission snapshot when content
coordinates and visibility are valid. It emits no staging output or ShowWindow.
Staged keeps its exact renderer outputs and show fences. Its create acknowledgement
is now reported through creation facts, rather than a visibility property.
The duplicated native-create/bringup phase mirror and generic visibility-proof
getters are removed; query phases project the one private owner state.

Readiness provenance is retained with the prospective scene and first-live
barrier. Compatible observations keep it stable; withdrawal revokes the output
floor and resumes preparation without repeating creation. Renderer failure uses
the same cleanup owner. Waiting-first-live replacements participate in provider
retirement, and staged replacements retain their Show effect while readiness
validation still references it.

The independent consumer passes 91 default and 98 serde tests, including twelve
public preparation journeys:
same-snapshot readiness, complete content, output/frame drop and retry, late
cancellation, visibility/coordinate/positive-size requirements, readiness
withdrawal, foreign create authority, unrelated-root version revalidation,
renderer-failure cleanup, direct replacement, early Staged creation acknowledgement
and replacement readiness recovery. The latter two regressions failed before
their production fixes and pass afterward. Creation now settles before
visibility/coordinate early returns. Replacement recovery retains valid
measurements while issuing a new semantic ticket above its new admission floor;
old outputs cannot admit it, and ordinary repaint needs no extra measurement.
Focused coordinator regressions for preparation failure, provider handoff and
terminal-effect retention pass. All 1,842 workspace tests pass on the frozen
79-path changed-Rust snapshot; all 427 tracked Rust files under crates/integration
also byte-match the native validation checkout. Native default and
assertion-disabled configurations each pass 458 tests, including the same-instance
10,000-lifetime model. All 10 doctests, core/native correctness/suspicious Clippy,
strict rustdoc, formatting and whitespace gates pass.

macOS programmatic redock passes with unlimited and bounded storage. The bounded
run samples 3,000,339 resident and 4,152,024 reserved input-data bytes below its
4,194,304-byte selection. All five real-window output-pressure scenarios pass
on macOS and X11. X11/XTest passes drag-out, exact semantic child first-live,
guide-targeted drag-back, exact DestroyNotify and retired viewport/quiescence.
The fresh Linux run uses Rust 1.95 Linux/arm64, Xvfb/Openbox, software GL and a
1 CPU / 4 GiB container. A compiler disk-exhaustion failure and Docker's resulting
stopped-engine EOF were environment failures before test execution; after
recovery, all 29 frozen-source gates pass. Source/command/result evidence is in
`/tmp/dockspace-direct-final-snapshot.json` and
`/tmp/dockspace-direct-final-qualification.json`. The early-ack failure is recorded
in `/tmp/dockspace-direct-staged-ack-red.json`, its first green run in
`/tmp/dockspace-direct-final-public-focused.log`, and replacement recovery's
red/green logs use the `dockspace-direct-replacement-renewal` prefix.

Independent core and adapter reviews find no remaining determined P1/P2 after
the replacement-output fix. The real Created callback through an Unknown
visibility production snapshot and frame retry has not been run as one complete
OS trace; separate public-host, adapter rollback and static reviews cover its
parts. The current physical smoke deliberately targets the Center guide. Earlier
descriptions of it as non-guide evidence were incorrect: actual non-guide
physical acceptance remains open. Native qualification still uses local egui
`86feb7f8cc49217bff54a8924b2cb525f4af3285` through an explicit override; tracked
remote pins do not reproduce it. U7 scoped/stationary input, cross-saturation,
broader physical evidence, native showcase and U9 packaging remain open.

## 2026-10-30 R15 first-live ownership replacement

The previous native protocol published topology at non-interactive PostShow,
before its first semantic child output. The replacement keeps the original
workspace, root/item ownership and version throughout staging. A checked,
temporary workspace command feeds the existing scene compiler for the child's
complete pane/tab program. The retained scene slot records its saga, binding,
resource and source-version provenance; it grants no receiver authority before
admission and does not become another topology owner.

An accepted current exact first-live output commits transfer, workspace revision,
binding admission, resource release and observation acceptance in the same
rollbackable host-frame candidate. Dropped output, abandoned output, frame
drop/retry, incompatible policy, cancellation, stale concurrent output and late
predecessor facts cannot publish the prospective workspace. Pending Item, Tabs
and Subtree sources use the same complete frozen source guard. Tick-final
vacancy also captures a newly admitted target, so an immediate same-frame dock
back retires the exact child and suppresses its delayed focus request.
If a destroyed predecessor has already transferred cleanup to its exact terminal
owner, its vacancy can settle alongside a replacement at the same logical
surface without mutating the successor or duplicating cleanup.

An active workspace popup is dismissed before transfer and requires a fresh
Inactive child output. The popup's existing exact-roster interaction barrier
remains closed until every surviving surface presents. A surface still awaiting
that successor no longer erases unchanged exact proofs already accepted for its
siblings. Inactive surface roster changes preserve independent output identity.
Popup measurement directories are normalized in both Active and Inactive states.
The presentation retention manifest includes exact proofs held by a staging
popup gate, so adapters retain receiver resources until those proofs are revoked
or replaced even while interaction remains unavailable.

Every pending create can use ordinary cancellation, including first-live content
readiness. The old transferred-create recovery API and resource handoff into
surface recovery are removed. Admitted surface recovery still owns its logical
roster and exact replacement lifecycle; create resources belong only to their
original create/first-live owner or its binding cleanup.

An ordered presentation observation establishes the post-observation application
version before following surface actions. The regression rejects the former
source-version command, accepts the current child's prepared tab action, and
checks that dropping and retrying the whole candidate preserves input and
producer watermarks. Independent commands after that barrier still share their
accepted application base.

Deferred native painting uses the bridge's staging decision plus exact current
route and binding authority. It no longer requires a durable child surface as a
prerequisite for painting that surface's first-live content. A content-ready
pending-child fixture verifies Semantic and Retain dispositions while source
ownership remains unchanged, including foreign, retired and forgotten binding
rejection. Programmatic macOS redock passes with unlimited and bounded storage,
and all five macOS output-pressure scenarios pass. The late-output scenarios
assert shutdown quiescence in the final root terminal and zero output owners
after event-loop exit; a later OS destruction fact for the external root is new
teardown work rather than an unsettled child or output obligation.

Frozen qualification passes 1,837 workspace tests, 86 public-consumer tests,
10 doctests, correctness/suspicious core/native Clippy, strict core/native
rustdoc and formatting. Native normal and assertion-disabled configurations
each pass 453 tests, including the same-instance 10,000-lifetime model. The two
obsolete durable-surface classification tests are replaced by the real pending
child paint-disposition regression. An independent review of the final ordered
version, deferred-paint and shutdown-boundary changes reports no P1/P2 findings.

Programmatic macOS redock passes with unlimited and bounded storage. The bounded
run samples 2,996,883 owned input bytes and 4,145,752 reserved bytes under 4 MiB.
The X11/XTest physical smoke passes drag-out, exact child first-live, guide-targeted
drag-back, exact DestroyNotify and retired viewport/quiescence checks in the
Rust 1.95 Linux/arm64 Xvfb/Openbox environment. All five real-window output
pressure scenarios pass on both macOS and X11: ordered, duplicate, reordered,
late-staging and late-first-live. These retain their existing model, platform
and storage-accounting boundaries; they do not certify additional physical
platforms or transient allocator peaks.

Source hashes, exact commands and outcomes are retained in
`/tmp/dockspace-r15-qualified-snapshot.json`,
`/tmp/dockspace-r15-qualified-qualification.json` and the consolidated
`/tmp/dockspace-r15-qualification.json`. Native validation uses the explicit
unpublished local egui fork override at
`86feb7f8cc49217bff54a8924b2cb525f4af3285`; the tracked remote egui pin does not
reproduce that source. Earlier full-suite results below describe the superseded
protocol. The preparation-strategy qualification above supersedes the earlier
strategy gap; native scoped/stationary input,
broader physical matrix and complete U7/U8 remain open; R15, U8 and the
Definition of Done have not been weakened.

## 2026-10-30 native original capture metadata and touch-route suppression

Native single-event and immutable cursor-prefix translation now read each
record's original `ReservedHostRecord.captured_at` from the existing journal.
The timestamp is not resampled during translation, core replay or frame retry.
Touch events carry Touch metadata; cursor, mouse-button and capture events carry
their mouse class. Scroll/Pan events retain Unknown device class because this
adapter does not observe their physical input class. The original timestamp
still accompanies those edges.

Core freezes the edge metadata into its exact receiver candidate and exposes it
through `NativeReceiverQuery::metadata()`. Hosts do not supply or replace it when
answering a query. The existing 256-move dropped-frame fixture now checks every
point, original capture time, device class and order on both attempts, with an
injected clock advanced separately before translation and before each replay.
A single press checks its original time on both click and drag queries. The
Touch fixture checks capture time/class before retention, original translator
state on retry, once-only recording and dropped/committed core boundaries.

Touch snapshots now use the same filtered pointer delivery lookup as PanGesture.
A retained window lifecycle binding cannot recreate pointer authority after a
Destroyed callback suppresses that route. Regressions cover the earlier exact
snapshot, the later suppressed snapshot, Ended/Cancelled, retired tails and a
fresh contact identity without resurrecting the old binding.

Frozen qualification passes 1,824 workspace tests, 86 public-consumer tests,
10 doctests, correctness/suspicious core/native Clippy, strict core/native
rustdoc and formatting. Native normal and assertion-disabled configurations
each pass 452 tests, including their same-instance 10,000-lifetime model. The
bounded macOS programmatic redock reaches first-live, redock, retirement and
quiescence, sampling 3,005,859 owned input / 4,155,496 reserved bytes under
4 MiB. Independent review reports no remaining P1/P2 in this metadata/suppression
slice. Source hashes and commands are retained in
`/tmp/dockspace-native-pointer-metadata-snapshot.json` and
`/tmp/dockspace-native-pointer-metadata-qualification.json`. These gates cover
the existing transfer protocol; the open R15 correction above is separate.

This is metadata and route-isolation groundwork. It does not add native local
coordinates, touch long press, enlarged touch hit targets, stationary scheduling
or validated local-to-desktop handoff. Native physical-touch claims remain
unqualified, and the native gates continue to use the explicit unpublished egui
fork override.

## 2026-10-30 controlled kilohertz active-ingress qualification

One bounded native coordinator/session now receives 10,000 distinct cursor
moves at 1 ms intervals in injected monotonic time, with one dispatch every
16 ms. The fixture enters through owned raw `WindowEventRecord` capture,
bridge admission and the production callback-prefix translator. Callback
reduction and core polling share one `NativeInputBudget` for each dispatch;
the test checks both a lower and an upper bound on completed-record accounting.
It never restarts that budget to hide a yield. Every active move reaches the
core recorder, active state and the docking preview survive every accepted
prefix, and no early manipulation feedback or native create is allowed.

The bridge-retained ingress backlog is checked after every capture against its
exact expected count and age, including nonempty `Some(age)` values. Its peak
is 16 records / 15 ms. Capacity samples cover capture, dispatch, release and
settlement boundaries under the combined 4 MiB profile. They count owned input
data capacity and its reservations, not process RSS or allocations inside a
dispatch. Renderer/receiver storage and allocator overhead retain their existing
documented exclusions.

The release frame publishes exactly one Pending(AwaitingPresentation), keeps
the source item in its original root and emits exactly one complete painted
output for that surface. The model reports that exact output as Presented.
The next dispatch must settle its artifact and publish exactly one Delivered
(Dock) with the same manipulation identity. Delivery changes the topology and
invalidates the old preview, so the artifact is settled without remaining
current receiver authority; `presents(artifact)` is not required after that
mutation. The resulting accepted-release-to-terminal delay is 31 ms in the
controlled schedule, below its 100 ms model budget.

Normal and native-debug-assertion-disabled nextest gates each pass the model
and six existing callback-prefix regressions. Correctness/suspicious native
Clippy and formatting pass. Both model runs sample 3,021,220 owned input bytes.
In a disposable source snapshot, forcing active prefixes into idle-motion
coalescing fails the intended recorder-count assertion (7 instead of 22), then
the production file is restored byte-for-byte. Commands, source hashes and
results are retained in `/tmp/dockspace-active-load-qualification.json` and
`/tmp/dockspace-active-load-negative-qualification.json`.

This is callback-to-core model evidence. The dispatch clock does not advance
while a reducer runs, so it does not establish a 2 ms service wall-clock bound,
OS delivery at 1,000 Hz, GPU presentation or real-time 100 ms release latency.
Native pass-through effects enter their original adapter queue; the fixture
neither dispatches them to an OS nor fabricates Applied callbacks. Physical
input, native scoped/stationary continuity and combined-cap cross-saturation
remain separate qualification work. This slice does not complete U7/U8 or the
full plan, and still uses the explicit unpublished local egui fork override.

## 2026-10-30 recovery after transfer and before first-live admission

Historical qualification of the earlier protocol, superseded by the ownership
replacement above. Its transferred-create recovery APIs are removed; these
results remain as provenance rather than a current API or acceptance claim.

The host frame now exposes exact pending recovery surfaces and an explicit
`recover_transferred_native_create` operation. It resolves the saga and binding
from the current candidate, then checks phase, registry admission, retained
resource, first-live barrier, bound recovery obligation, domain, anchor and
normalized complete source facts. Hosts supply neither a recovery target nor
replacement geometry. The existing complete-roster recovery compiler publishes
the obligated logical recovery; tick-final vacancy transfers retained resources
to the original BindingCleanup and waits for real window destruction. No
Presented, Destroyed or native-close fact is fabricated.

The immediate result distinguishes no candidate, Applied and Blocked. Applied
still requires frame commit; Blocked preserves every lifecycle owner. Both
attempts have an exact submitted-action result in the public report. Frame
drop, failed publication and effect-report pressure retain the original
transaction and delivery ledgers. A session query derives pending create work
from the existing coordinator so a visible but unadmitted child cannot be
mistaken for quiescent shutdown.

Fatal native dispatch tries candidates within its original shared budget,
charging every attempt and continuing past Blocked entries until one recovery
applies. It stores no persistent rotation cursor or new candidate registry.
Blocked-only reports do not count as progress or revive generic repaint demand.
When host authority is unavailable, the actual matching viewport Ui can supply
current dock/popup bounds and pane/tab metrics through the existing measurement
port. Only a committed Ready measurement schedules the next recovery boundary.
Foreign hosts receive a targeted wake through their existing route; a missing
route retains the blocked obligation. Existing no-frame invalidation and
overload terminal boundaries still take precedence.

Concurrent creates exposed two old source-reservation errors. Before transfer,
an unchanged contained source may keep its original reserved floating while an
independent create progresses; its owner, floating record and complete frozen
NodeSource must remain exact. After transfer, independent publication protects
the current complete child roster rather than the pre-transfer source owner.
Every saga still needs its own binding/resource/obligation proof. Removing one
recovered surface changes the global popup roster and invalidates other old
measurement tickets. A second host therefore returns HostAuthorityUnavailable
until a fresh measurement boundary; no extra same-tick authority bypass was
retained. Two-host regressions cover both candidate fairness and sequential
cleanup with the required fresh measurements.

The real `late-first-live` smoke waits for actual ownership transfer, withholds
one original real first-live terminal and injects a duplicate previous real
terminal. It returns the withheld terminal only after fatal quarantine, then
stops external repaint demand. Success requires the exact primary output-order
error, no cleanup error, restored source items, no child first-live promotion,
zero output owners and drained shutdown. This is a delayed real-window terminal
journey with a small ordinary output peak, not the separate O256 qualification.
The old `b69a363` production source times out in Draining after all output owners
reach zero; that negative control is recorded in
`/tmp/dockspace-first-live-negative-qualification.json`.

Focused verification passes 11 core tests, including exact phase/binding
isolation, rollback, occupied effect reports, late terminal rejection and two
independent native roots. Native focused verification passes four shutdown
tests, including per-attempt budget accounting and Blocked/measurement wakes.
The selected source hashes are recorded in
`/tmp/dockspace-first-live-recovery-snapshot.json`.

Final frozen verification passes 1,824 workspace tests, 86 public-consumer
tests, 10 doctests, correctness/suspicious core/native Clippy, strict rustdoc
and formatting. Native normal and assertion-disabled configurations each pass
448 tests, including the same-instance 10,000-lifetime model. The full workspace
gate also preserves the older same-tick newly visible child-vacancy/focus
regression; partial tab-created children retain their existing vacancy protocol.
Default/bounded macOS redock and all five macOS/Linux output scenarios pass;
the unchanged Linux physical XTest journey passes separately. The late-first-live
scenario peaks at ordinary3/control1, forwarding its one real withheld terminal:
macOS samples 2,843,172 resident / 3,807,664 reserved bytes and Linux samples
2,843,140 / 3,808,560 bytes. The O256 journeys retain their independent
ordinary256/control2 assertions; they are not inferred from the small first-live
case. Results are recorded in
`/tmp/dockspace-first-live-recovery-qualification.json` and
`/tmp/dockspace-first-live-recovery-linux-qualification.json`. These native gates
use the unchanged unpublished local egui fork override; the tracked remote pin
still does not reproduce the local seam. A public-harness link initially failed
because the disk was full; removing only this workspace's regenerable incremental
compiler cache allowed the same frozen sources to pass the resumed gate.

This slice does not establish
1,000 Hz/release timing, native scoped/stationary continuity, configured-cap
cross-saturation, the broader physical platform matrix or complete U7/U8.

## 2026-10-29 fatal cleanup before native ownership transfer

Historical checkpoint. The first-live replacement above extends this same
cancellation operation to every pending phase and supersedes its old skip rule.

`DockspaceHostFrame::cancel_next_native_create` selects at most one eligible
create from the current frame candidate and uses the existing
`CancelNativeCreate -> abort_native_create -> BindingCleanup` transaction.
It preserves source ownership and skips creates already awaiting first-live
presentation. Frame drop and failed commit publish neither cancellation nor
source-sequence advancement. Pure lifecycle cancellation may commit while
effect-report capacity is occupied; its compensating effect stays in the
original ledger until delivery capacity returns. Mixing ordinary application
input into that frame retains the existing capacity restriction.

Fatal shutdown invokes this operation only after its accepted input prefix is
complete and no overload terminal has closed that frame's input phase. It
cancels at most one create per complete boundary, charges one unit to the same
256-record/2-ms allowance and wakes root for continuation. Exhausted budgets
defer the operation. Exact late window facts, input restoration, compensating
close, Destroyed, tombstone and binding quiescence retain their existing owners.
No additional lifecycle registry or cancellation authority is introduced.

Targeted core regressions cover current-candidate selection, rollback/retry,
occupied effect reports, ordinary-input rejection and exact first-live/rehome
boundaries. Native lifecycle models cover pre-show without a staging terminal
and a captured create whose acknowledgement/hidden facts arrive after fatal,
including budget exhaustion, repeated shutdown and once-only retirement. They
do not fabricate opaque fork output tokens or certify physical OS delivery.

The real-window `late-staging` output-pressure scenario withholds the original
protected child terminal until fatal quarantine has abandoned its affine paint.
All 258 original terminals then return in order. Success requires the exact
output-order primary error, no cleanup error, unchanged source ownership,
no child first-live promotion, zero output owners and drained shutdown. After
the late terminal returns, both UI and background test repaint demand stop;
the remaining cleanup uses production wakes. The background thread retains
only its timeout watchdog. Linux/arm64 Xvfb/Openbox/software GL passes this
scenario. The earlier production source fails its negative control with all
258 terminals returned and no ordinary output left, but shutdown still stuck
in Draining. That negative control retained external repaint demand, so its
failure was not caused by an absent test wake.

Source identities and initial focused results are retained in
`/tmp/dockspace-late-staging-snapshot.json`,
`/tmp/dockspace-late-staging-core-targeted.log`,
`/tmp/dockspace-late-staging-native-targeted.log`,
`/tmp/dockspace-late-staging-linux-run.log` and
`/tmp/dockspace-late-staging-negative-qualification.json`.
The final frozen slice passes 1,815 workspace tests, 86 public-consumer tests,
10 doctests, core/native correctness/suspicious Clippy, strict core/native
rustdoc and formatting. Native normal and assertion-disabled configurations
each pass 446 tests, including their same-instance 10,000-lifetime model runs.
Default/bounded macOS redock and all four pressure scenarios pass. Linux passes
the unchanged physical XTest journey and all four pressure scenarios. macOS
late-staging samples 2,843,172 resident / 3,807,664 reserved bytes; Linux samples
2,843,140 / 3,808,560 bytes, with the 18,576-byte test proxy charged separately.
Results are recorded in `/tmp/dockspace-late-staging-qualification.json` and
`/tmp/dockspace-late-staging-linux-qualification.json`. No fork implementation
changes were needed; these runs still use the explicit unpublished local fork
override rather than the older tracked remote pin.

This checkpoint closes pre-transfer cleanup only. The subsequent first-live
recovery qualification above handles transferred ownership separately; the
cancellation API intentionally cannot abort it. No full U7/U8 claim follows.

## 2026-10-29 bounded roster recapture and X11 physical recovery

Ordinary complete-roster capture now distinguishes a borrowed payload backing
from exhausted window capacity. When a committed baseline and an accepted
snapshot retain both prepaid backings, the host requests one fresh recapture
through its existing protected roster receipt. Repeated deferred passes do not
mint another generation. Already accepted facts drain in order; root control
passes retain the previous framebuffer and suppress semantic/application
actions until the fresh snapshot passes both full commit and exact settlement.
An ordinary output abandoned at the first failed capture is also recognized as
control. Existing promised child staging keeps its protected path. No third
payload, additional lifecycle authority or relaxed input/output cap is added.
Actual roster window-capacity exhaustion still freezes ordinary ingress.

All 11 roster protocol tests pass, including ordinary-backing reuse without
overload, coalesced recapture, exact settlement barriers, fixed reservation
capacity and genuine window-capacity overload. The unchanged Linux physical
driver now completes its actual XTest drag-out, child first-live, drag-back,
exact child DestroyNotify, semantic inventory removal and final quiescence
journey under Xvfb/Openbox/software GL. The production drag-out preview precedes
release (steps 29/30); the drag-back Center-guide preview precedes release
(steps 114/116). These are observed steps, not fixed frame-count assertions.
The build uses the unpublished local egui fork at
`86feb7f8cc49217bff54a8924b2cb525f4af3285` through explicit source overrides.
The 420-source hash manifest and physical log are retained in
`/tmp/dockspace-roster-backpressure-snapshot.json` and
`/tmp/dockspace-roster-backpressure-physical-x11.log`.

The same frozen native slice passes 444 tests normally and 444 with native
debug assertions disabled, including the same-instance 10,000-lifetime model
in each run. Correctness/suspicious Clippy, strict rustdoc, formatting and
default/bounded macOS programmatic redock pass. All three real output-pressure
journeys pass on macOS and Linux, retaining the existing O256/control2 staging
and exact terminal-order assertions. macOS pressure samples 2,843,172 resident /
3,807,664 reserved bytes; Linux samples 2,843,140 resident / at most 3,809,080
reserved bytes. Bounded macOS redock samples 3,005,859 / 4,155,496 bytes. Each
remains below 4,194,304 bytes; pressure-proxy backing is separately 18,576 bytes.
Results are recorded in `/tmp/dockspace-roster-backpressure-qualification.json`
and `/tmp/dockspace-roster-backpressure-linux-qualification.json`. The core and
fork implementation are unchanged in this qualified slice; their earlier
independent gates are not reported as newly rerun.

This supersedes the earlier missing-preview blocker recorded below. It proves
this physical journey, including a Center-guide redock; it does not qualify the
full non-guide-point matrix, mixed-DPI platforms, 1,000 Hz/release timing,
configured-cap roster/output cross-saturation or native scoped/stationary input.
The later qualification above closes pre-transfer staging cleanup; transferred
first-live recovery remains separate work.
U7/U8 remain incomplete.

## 2026-10-29 U7 real output-owner pressure

The new `egui-native-output-pressure` binary uses actual eframe output tokens
and terminal results. Its bounded proxy delays at most 258 terminal callbacks;
that test-owned backing is reported separately from runtime data. It does not
fabricate opaque identities or model 256 queued GPU frames. The selected 4 MiB
profile has I64/256 KiB, W2/A1/16 KiB, R258/E2/768 KiB and M4/C2/L2. The larger
presentation bound prevents R64 from masking the independent ordinary O256 cap.

The proxy first lets actual creation establish an awaiting staging request,
defers its child candidate and fills ordinary output owners. During filling it
pauses new complete inventory capture, including terminal-roster recapture;
already accepted inventory and all ordinary window/effect callbacks retain
their original owners. Each output's exact window snapshot still passes through.
This controlled inventory pause isolates O256 from the separate two-roster
backing limit. The next root candidate resumes fresh complete roster capture
after terminal delivery; successful first-live/redock/retirement is required.
It does not qualify simultaneous full output and roster saturation.

The test requires exactly 256 ordinary owners, one root control and one protected
child staging owner. It correlates real control tokens with NotPresented for
root and Presented for the protected child, then requires another root candidate
to Defer. All 258 results return in their original completion order. The ordered
journey must recover ordinary admission, redock, retire the child, settle one
last root boundary and leave zero output owners after window exit. Separate
processes replay a real duplicate or deliver the second real result first;
they must preserve the precise output-order fatal error, have no extra cleanup
errors, and finish the existing cleanup protocol. Their final root terminal
checks drained shutdown and zero output owners before event-loop teardown.

These journeys exposed three production issues. Deferred admission consumed an
OS repaint while egui still considered its wake queued; the fork now acknowledges
that consumed wake before calling host admission, without consuming input or
advancing a pass. Output prefix preparation repeatedly merged all owner ingress
and could exhaust the next dispatch before useful work; it now freezes bounded
original ordinal/token membership once, with a linear complete-prefix path and
checked per-token reads. Two simultaneous prefix scratch buffers are prepaid in
the existing owner allowance. Quarantine now retains the first protocol error
while draining validated terminal ingress, consumes already captured staging
terminals, rejects new create/show/focus/input-enable work, and runs compensating
close/release/cleanup through the original retirement protocol. It publishes no
new staging requests. Read-only occupancy and error queries add no authority.
The consumed-wake state retains the previous real pass's repaint delay until
the next actual begin-pass, preserving egui input timing history. A final root
close pass settles accepted work while retaining its existing framebuffer;
it cannot issue an owner whose terminal would require a post-exit root cycle.
The smoke observes the current staged retention and original owner count before
requesting exit, then checks every final root terminal and window exit again.
Finite application construction also requires a root resource and close slot
before provider enable; pure storage profiles may still describe zero slots.

Fresh frozen gates pass 443 native tests normally and 443 with native debug
assertions disabled, including all 10,000 model lifetimes in each run. The fork
passes 196 egui/eframe tests with Glow, 190 with WGPU, and 196 with native debug
assertions disabled across both renderers. Both renderers also compile without
the native seam. Correctness/suspicious Clippy, strict rustdoc, formatting and
default/bounded macOS programmatic redock pass. The public profile regression
also checks pre-enrollment root resource/close-slot refusal. The native/fork
checks use the same frozen 420 Rust sources and both modified fork sources.
Results are recorded in `/tmp/dockspace-output-pressure-qualification.json`.
Removing only the consumed-wake acknowledgement call in an isolated fork
snapshot makes the deferred-child regression fail (exit 100); exact restoration
passes (exit 0). The live fork source remains hash-identical throughout.
The control is recorded in `/tmp/dockspace-repaint-admission-control.json`.
The fork changes are locally committed at
`86feb7f8cc49217bff54a8924b2cb525f4af3285` and remain unpublished; tracked native
manifests still pin the earlier public baseline, so these gates use explicit
local source overrides rather than claiming that remote pin reproduces them.

Ordered, duplicate and reordered macOS output-pressure journeys all pass with
actual O256/control2/protected-staging1 occupancy. Each forwards the original
258 real results, sampling 2,843,172 resident bytes and 3,807,664 reservation
bytes under the selected 4,194,304-byte limit. The separately bounded proxy
backing is 18,576 bytes. The ordinary bounded redock samples 3,005,859 resident
and 4,155,496 reservation bytes; its unlimited comparison samples 2,750,841
resident bytes. Independent prefix/cutoff/scratch review found no remaining
concrete blocker in this slice.

The same committed source also builds both smoke binaries on Linux/arm64 and
passes ordered, duplicate and reordered real-token pressure under Xvfb/Openbox
with software GL. All three reach O256/control2/protected-staging1 and forward
258 original results, sampling 2,843,140 resident bytes and 3,808,176 reservation
bytes below the same 4 MiB limit. Terminal and event-loop-exit owner checks pass.
This is deterministic delayed delivery over real native windows, not physical
pointer evidence. Logs and source identities are recorded in
`/tmp/dockspace-output-pressure-linux-qualification.json` and
`/tmp/dockspace-output-pressure-linux-{build,ordered,duplicate,reordered}.log`.

A staging paint whose terminal has not arrived at quarantine
still needs its own cancellation/late-terminal qualification; these captured
terminal journeys do not close that case. Physical X11, active 1,000 Hz/release
timing, native scoped/stationary behavior and configured-cap cross-saturation
remain open. Full U7/U8 readiness is not established.

## 2026-10-29 X11 physical harness callback migration

The Linux-only diagnostic handler now returns the exact inner
`NativeWindowEventDisposition`, preserving held-close decisions, and forwards
property-command retirement and its wake. Its physical pointer barrier treats
`Retired` as an unapplied command rather than proof that pass-through changed.
The current fork's complete handler trait is forwarded without substituting
callback authority. The macOS cfg does not compile these files.

On the existing Rust 1.95 Linux/arm64 container, the physical binary compiles
and all 29 unit tests pass. Under Xvfb/Openbox with software GL, the bounded
programmatic redock completes first-live and retirement, sampling 3,005,827
resident bytes and 4,123,712 reservation bytes below the selected 4 MiB limit.
The build uses the same unpublished local egui fork and tracked Winit revision.
Logs and frozen source identities are retained in
`/tmp/dockspace-linux-handler-{tests,build,bounded-redock,physical}.log` and
`/tmp/dockspace-linux-handler-snapshot.json`.

At this migration checkpoint the physical XTest journey was **not qualified**:
real press, threshold crossing,
routed outside motion and the pass-through Applied ACK are observed, but the
production native preview is absent before release. The driver fails at
`drag-out-x11-events`; this remains a release blocker under investigation.
Neither the unit tests nor programmatic redock replace that physical gate.
A fresh minimal two-site diagnostic against the current source directly records
the failure: with two retained records, oldest age 2.67 ms and all byte/raw/output
capacity checks still passing, ordinary complete-roster recapture finds both
payload backings borrowed. That path freezes ordinary ingress and commits
`InputOverload`, cancelling the active Dragging owner. This is distinct from
expired age or exhausted output capacity. The diagnostic changes only the first
freeze and Cancelled-event logging in a disposable snapshot;
`/tmp/dockspace-native-freeze-diagnostic-run.log` records the direct cause.
The subsequent bounded roster-recapture qualification above replaces this
transient cancellation and closes this specific physical missing-preview blocker.

## 2026-10-29 U7 explicit bounded application and lifecycle model

`NativeStorageLimit` is an explicit public application choice. It validates the
same complete combined profile as the coordinator constructor, including
independent resource, command and close bounds. `NativeDockspaceApp` accepts it
through `new_with_storage_limit`; existing constructors retain their unlimited
storage choice. The application reports selected reservations separately from
current resident capacity. These quantities cover the documented native input
and lifecycle data, excluding the core workspace, renderer/receiver data,
application actions, diagnostics and allocator metadata.

The programmatic smoke selects `bounded` or `unlimited` through
`EGUI_DOCKSPACE_NATIVE_SMOKE_STORAGE`. The bounded choice uses the same 4 MiB,
I512/768 KiB, W16/A8/128 KiB, R64/E16/256 KiB and M48/C32/L16 configuration as
the combined admission regression. Every root update requires
`resident <= reserved <= 4 MiB`; terminal logging labels the selected profile
and reports both sampled peaks. Sampling is at host boundaries, while the
existing admission promises cover accepted construction overlap.

The byte-saturation regression now commits the freeze prelude, drains the exact
accepted source prefix, retries a dropped prefix candidate, and drops/retries
the independent overload cancellation. Raw coalescing and retained core pointer
reduction share one dispatch budget. Cancellation commits once, releases the
existing promises and permits a fresh capture without increasing the quota.
An additional callback after freeze cannot mint another source identity.

A separate 10,000-lifetime model constructs one bounded coordinator and session
before the loop. Each cycle registers a root, captures its exact Destroyed fact,
retires core authority through public layout reset, commits the original
destroyed tombstone, retires the route and reports quiescence once. It resolves
the next rebased surface through a stable application item. Periodic candidate
and retirement-preparation drops exercise exact retry. A single retained
predecessor supplies late facts; no history of reports or callbacks accumulates.
Every cycle requires zero remaining route/retirement/reference obligations,
unchanged prepaid producer backing, plateaued idle resident/reserved capacity
and the same 4 MiB allowance. The model reuses the numeric host token with fresh
winit IDs; it does not qualify physical window creation or same-winit-ID aliasing.

Fresh gates pass 442 tests in the complete normal native workspace plus one
additional public-only storage-profile integration test, and 443 tests in the
complete workspace with native debug assertions disabled. Both complete runs
execute all 10,000 model lifetimes. The public test rejects missing subprofiles
and the complete initial requirement minus one before consuming a session,
constructs an app at the exact requirement and verifies the default has no
selected reservation. Correctness/suspicious Clippy, strict rustdoc and formatting
also pass. Qualification is recorded in
`/tmp/dockspace-bounded-host-qualification.json`, with 418 frozen Rust source
files matching the tested snapshot.

Both macOS programmatic redock runs complete first-live and retirement. The
bounded run's root-boundary resident peak is 3,005,859 bytes and its reservation
peak is 4,122,680 bytes, below the selected 4,194,304-byte limit. The default run
is explicitly labelled unlimited and samples a 2,750,705-byte resident peak.
Independent model/recovery and constructor reviews find no remaining concrete
blocker within this slice. The unpublished egui fork remains unchanged at
`bac446e142f93a5dc92695ab32e15a12e6dc7453`; tracked remote pins still name the
older public baseline. A separately prepared Linux-only handler-forwarding fix
is excluded from this source qualification.

This explicit opt-in does not certify full U7/U8: full ordinary-output pressure, active 1,000 Hz input
and release timing, native scoped/stationary continuity, configured-cap terminal
fanout and the physical platform matrix remain separate gates.

## 2026-10-29 U7 initial combined-storage admission

Raw journal and both complete roster payloads now use fallible vector
preallocation, actual-capacity checks and checked initial reservation queries.
The production bridge constructor uses those paths together with the already
prepaid output owner. Its fresh storage query includes bridge inline data,
4,096 raw entries, both complete roster backings and ordinary/control output
backing before any capture or route owner. Selected combined construction
allocates roster backing directly for W/A; generic construction retains its
existing default bounds. Empty retained storage remains charged. This does not
make Arc allocation or all framework/allocator OOM behavior recoverable.

The private `NativeCombinedStorageLimit` checks the complete initial promise
before allocating native owner backing or enabling the core provider:

```text
initial = fresh_bridge + pointer_retry + snapshot + owners
        + coordinator_uncovered_inline + transport + producer + reports
```

Every sum is checked. The last three terms use the selected complete byte
allowances rather than current resident samples. Already promised O/S/R headers
are deducted from the coordinator inline term while their padding and unrelated
fields remain. The permanent translator header stays there; the separate bridge
pointer promise pays the retry header and overlapping dynamic data. Fresh bridge
inline retains its old empty lifecycle/resource headers alongside the complete
owner promise, covering initial replacement overlap. After installation, those
owners' actual backing, staging and external promise remainder share the same
host allowance. The complete O is not added a second time.

All host preparation now precedes core enable: combined allowance, lifecycle,
resource, pending-result and snapshot/work-area stores. Core enrollment still
validates I/P/R subprofile requirements and exact existing-state replacement
before its own publication. A separate selected-reservation query exposes the
current host baseline plus future work promises, while the existing resident
query keeps its narrower meaning.

Fresh native gates pass 439 tests normally and 439 without native debug
assertions, plus correctness/suspicious Clippy. The 11 additional tests cover
initial raw/roster allocation, full two-roster construction arithmetic, empty
spare backing, actual capacity, fresh bridge accounting, and complete combined
admission. The finite fixture selects 4 MiB total, 512 core transport slots with
768 KiB, W16/A8 with 128 KiB producer allowance, 64 presentation/16 effect slots
with 256 KiB report allowance, and M48/C32/L16 host entries with their computed
owner allowance. Initial required bytes minus one rejects; the exact requirement
enrolls successfully and resident bytes remain within selected reservations.
A zero monotonic clock isolates byte admission from the age gate: pointer source
growth exhausts the combined allowance before the raw record bound, and refusal
mints no further capture identity or core watermark.

Removing only the pending pointer-growth promise in a disposable snapshot makes
that finite regression fail (exit 100). Exact restoration passes all 15 selected
initial/owner cases (exit 0), with snapshot/source bytes equal again. Results are
recorded in `/tmp/dockspace-initial-storage-control.json` and the complete gate
report in `/tmp/dockspace-initial-storage-qualification.json`. All 416 frozen
source files match the tested snapshot. Two independent initial-constructor
reviews found no remaining concrete blocker in their checked scope.

The macOS programmatic redock still uses the default host path and passes with
a sampled root-boundary peak of 2,750,841 owned input-data bytes. It does not
qualify the selected combined configuration. The app entry remains unlimited;
finite real-window execution, saturation recovery, full-output pressure,
1,000 Hz/release timing and the 10,000-cycle soak still precede product enablement.
The combined constructor and byte rejection are qualified primitives, not full
U7/U8 readiness. Scoped input, native showcase and physical platform gates remain
open; core/local adapter sources and the unpublished fork are unchanged.

## 2026-10-29 U7 output-prefix and fatal-cleanup construction

Output-prefix preparation now freezes the continuous Output segment by capture
journal ordinal and count, without collecting results. Reading copies one
terminal from its existing output owner and advances the range before caller
cleanup can remove that owner. It does not reapply Replay or the submitted-head
skip after the first submission makes a boundary pending. A new callback cannot
enter the frozen range; a missing expected member remains `OutputOrderViolation`.
Normal unfinalized-pass cleanup uses that same range. Selection scans output
owners' original terminal ingress rather than rebuilding a full ordered journal
for every item; final submission still performs its existing exact ordering check.

Fatal quarantine enters its existing mode, then claims one exact output at a
time using the original abandonment transition. Affine paint and cleanup
notification leave the host lock together. Unstarted create requests are selected
and removed individually, preserving already admitted or captured physical
create owners. The plans Vec, aggregate token Vec and extend/sort/dedup are
deleted from production. Pass-action teardown likewise visits the minimum key
of its two existing stores instead of collecting a BTreeSet and error Vec;
errors go directly to the caller's diagnostic sink, while durable local action
ownership retains its existing semantics.

Independent review exposed two shutdown boundaries which the first version
missed. A captured Created window already has `route.window`, so the narrower
"awaiting create attempt" query cannot decide whether its request is cancellable.
Fatal cancellation now checks the exact route's existing create/window receipt
ownership. A real core child-effect model covers both admitted and captured
states through repeated quarantine, preserving the same request, route and
published/recorded watermarks. It uses the existing private test create receipt;
no opaque fork attempt or output token is fabricated. The second boundary is
an already captured, unattached, unsubmitted NotPresented terminal hidden behind
an earlier non-output callback. Quarantine now abandons that exact owner through
the existing ingress-removal path, preventing shutdown from waiting forever for
an affine attachment. Attached or submitted terminal owners remain for ordinary
settlement. Actual Created/negative-terminal ordering still requires its stated
real-output qualification; the model is not promoted to physical evidence.

Fresh native qualification passes 428 tests normally and 428 without native
debug assertions, correctness/suspicious Clippy and the macOS programmatic
tear-off/first-live/redock/retirement journey. That real-window round trip samples
2,750,689 owned input-data bytes at root boundaries. Results are recorded in
`/tmp/dockspace-output-construction-qualification.json`. Its disposable snapshot
contains the exact 413-file source state for this slice; the separately prepared
initial-constructor primitives are excluded from these gates and this commit.

A disposable-snapshot control restores only the narrower awaiting-attempt
cancellation predicate. The captured-create regression fails (exit 100), then
exact source restoration passes all four selected create/replacement/shutdown
cases (exit 0). Results are recorded in
`/tmp/dockspace-output-construction-control.json`; the restored coordinator
matches the frozen source again.

The independent final review closes both shutdown findings and reports no new
correctness blocker. Full-output saturation, fatal real-output ordering, batch
timing, complete initial combined admission and configured-cap/soak qualification
remain open. In particular, removing collection does not establish the 2 ms
batch limit, and the complete 4 MiB product profile remains disabled. Diagnostic
error history and renderer/application action ownership are separate from the
defined native input-data allowance. The core/local adapter source and local
unpublished fork remain unchanged from their preceding qualifications.

## 2026-10-29 U7 inline observations, route wakes and prepaid output backing

The existing exact window-resource entry now owns its presentation ACK and
NoFrame invalidation inline. The two coordinator maps are deleted. Route
reservation, bind, attach and replacement prepare the resource slot before
changing their route; a refused M capacity leaves that route unchanged. The
resource's backing-retention bit is derived from existing route and announced
output references under the original host mutex. It carries no independent
lifecycle authority and does not itself make an idle route pending work.
Replacement and unbind explicitly retire the predecessor; old ACKs, invalidation
records and callbacks keep their exact entry until their original settlement.
Clearing observations alone does not release the current route's resource slot.

Create/show callbacks check their original prepaid presentation slot before
taking or accepting the affine request. ACK lookup for roster compilation is
borrowed under the same lock as destroyed observations, avoiding recursive
bridge locking and an extra ACK copy. Exact-binding uniqueness is preserved
across viewport aliases. NoFrame keeps the first exact record, and abandonment
plus obligation recording occurs under one lock before the terminal callback can
release its last output reference. Preparation reserves M ready records and M
repaint slots before changing any record. Stale removal, accepted selection and
repaint selection then update those buffers in place; commit compare-clears the
complete original record. The selected owner allowance includes that construction
peak rather than collecting multiple ready/stale/accepted/fallback copies.

Semantic repaint state now belongs to the original viewport route as
Idle/Blocked/Ready. The two host sets and production take/collect are deleted.
Replacement preserves that viewport's wake; exact removal ends it and an old
retirement cannot clear its successor. Only a complete clear causal boundary
releases Blocked. Each Ready is claimed under the host lock and repainted after
unlock; an already scheduled staging repaint consumes a duplicate semantic
obligation without scheduling twice. A host without a repaint context leaves
Ready retained. An unknown route does not mint a wake owner; the fork's Defer
path still schedules the root recovery boundary.

Ordinary output backing is now a sorted Vec prepaid for its existing O=256
limit. The root control owner remains inline and child-control backing remains
independent. The fallible production constructor prepares both stores before
core enrollment; actual capacities remain charged after removal. Insert refuses
a duplicate or full store with the original incoming resource instead of
overwriting an accepted owner. Shared byte admission now charges the complete
backing once in its host baseline, so a new output consumes a prepaid slot and
does not add a second per-entry byte charge. Pointer, resource, lifecycle and
snapshot allowance checks use the same prepaid baseline.

Fresh qualification passes 427 native tests normally and 427 with native debug
assertions disabled, plus correctness/suspicious Clippy. The macOS programmatic
tear-off, first-live, redock and retirement journey exits 0 and samples 2,750,825
owned input-data bytes at root boundaries. The tested snapshot matches all 413
frozen Rust source files. Results are recorded in
`/tmp/dockspace-observation-storage-qualification.json`. The core/local adapter
sources remain unchanged from the preceding 1,813 workspace and 86 public-host
qualification; this slice reruns the changed native workspace.

The 17 added regressions cover exact inline observations and alias conflicts,
pin/retire/prune, route wake replacement and once-only claim, empty output
backing accounting, and finite NoFrame preparation/drop/retry without growth.
One disposable-snapshot control removes only the host-reference retention
condition. The native finite NoFrame regression then fails (exit 100) because a
live route loses its original slot after observation settlement. Exact source
restoration passes all 19 selected observation/wake/owner cases (exit 0), and
snapshot/source bytes match again. Results are recorded in
`/tmp/dockspace-observation-storage-control.json`.

Independent wake, output and pin/ACK/NoFrame reviews found no remaining concrete
blocker in this slice. A preliminary compile failure was an old test's implicit
parent import; two old assertions were updated to distinguish a live route's
idle retained backing from pending callback work. The output prepayment tests
and macOS round trip do not qualify full-O saturation, duplicate-token or
iteration-order pressure. No public test token minting seam was added for those
unexecuted cases. Output-prefix/unfinalized/quarantine construction, complete
initial combined admission, configured-cap load/release latency and lifecycle
soak remain ahead of product-profile enablement. The complete 4 MiB profile is
still disabled; scoped native input, native showcase and physical platform gates
remain open. The local unpublished fork and tracked dependency pins are unchanged.

## 2026-10-29 U7 prepaid retirement, close and viewport-route owners

The selected private owner allowance now prepays one W retirement backing shared
by materialized windows and failed creates, one W route backing and one W exact
retirement-marker backing, plus an independent L close-entry store. Retirement
entries exclude another entry for the same physical lifetime across both kinds;
exact provider/viewport mismatches cannot acquire an existing release owner.
Refused release and failed-create transfers return the original acknowledgement
and receipt. Route/create admission reserves the eventual terminal backing before
accepting that lifetime. Callback transfer, full commit, tombstone publication
and successful quiescence append retain their existing distinct boundaries.

The route owner is now one flat Vec. Window publication, pointer suppression and
focus reportability live on that original entry; reverse maps and the separate
focus set are removed. Captured windows retain their former fallback semantics
until reducer publication. Prepared retirement markers remain exact and can
outlive replacement of their viewport route. Whole-batch validation and backing
reservation precede marker installation; rejection installs no partial marker.
Close entries retain exact binding keys, one current capture, one successor and
inline cancellation ingress. Finite saturation refuses a new capture before Hold
or journal identity, while exact duplicates and existing-owner terminals remain
admissible. Empty reusable backing remains charged.

Retirement preparation owns one route Vec and one token sink, sized by the
existing complete output-owner bound rather than by W. It reserves both before
installing markers; commit moves both buffers and no longer collects a temporary
token vector for each binding. Drop removes the original markers for retry.
Exact receiver retirement drops its affine paints directly from their output
owners without a token copy. Quiescence copies one candidate at a time, skips
blocked entries, preserves append-rejected owners and reports errors directly
into the caller's existing sink. Surface-driver token cleanup processes every
notification before returning its first error.

Close policy no longer collects request or item vectors. It freezes a finite
request-admission count, copies one decision token at a time and compresses
completed request IDs into one prepaid L scratch buffer. Every Result return
restores that scratch, and only a successful frame commit settles those IDs.
Partial budgets and deferred decisions retain their original owners.

Fresh qualification passes 410 native tests normally and 410 with native debug
assertions disabled, plus correctness/suspicious Clippy. The macOS programmatic
tear-off, first-live, redock and retirement journey exits 0 and samples 2,391,049
owned input-data bytes at root boundaries. The tested disposable snapshot matches
all 410 frozen Rust source files. Results are recorded in
`/tmp/dockspace-lifecycle-storage-qualification.json`. The core and egui local
sources are unchanged from the preceding 1,813 workspace/86 public-host gates;
this qualification reruns the changed native workspace.

The 23 added regressions exercise shared W retirement saturation and capability
return, full-commit and rejected-snapshot retry, mixed quiescence cursors, L close
saturation/duplicates/successors, route replacement, unpublished capture, batch
marker rollback, exact constructor byte difference of one and actual backing
preservation. Two preliminary failures were fixture defects: the largest
non-overflowing integer quotient was mistaken for overflow, and independent
journals supplied duplicate ordinals to one retirement owner. The corrected
fixtures use an overflowing successor count and one shared journal, respectively.
No implementation change was inferred from those failed assumptions.

Independent retirement, route and close-driver reviews found no remaining
concrete blocker in this slice. Semantic wake storage, presentation/no-frame
ACK ownership and remaining output/construction backing still precede combined
profile enablement. The complete 4 MiB profile remains disabled; configured-cap
load/release latency, lifecycle soak, native scoped input, native editor showcase
and physical platform certification remain open. The fork is unchanged from
local unpublished `bac446e142f93a5dc92695ab32e15a12e6dc7453`; tracked pins
still name the earlier public baseline.

## 2026-10-29 U7 prepaid resource, input, effect and deferred owners

The selected private `NativeOwnerStorageLimit` now combines independent M exact
window-resource entries and C ordinary commands with producer W and effect-report
E. Configuration validates the complete allowance and preallocates all selected
backing before core enrollment. Resource keys include both viewport and exact
binding, so predecessor callback owners continue to occupy their own entries.
Finite clear, prune and command drain retain their original backing and charge.
Create and show stores share E live requests while retaining their distinct
prepaid backing. Stable input observations use W; manual quiescence now shares
the automatic path's retained-reference guard, including stable observations
which no longer report pending work. The public coordinator session fixes its
provider for its lifetime; a future public provider-handoff API would need its
own observation-slot admission before backend dispatch.

Input command pending storage prepays E+1: one accepted head can still await its
snapshot after releasing its effect-report lease, while E successor requests
retain those leases. Retirement rotates the same deque in order and moves
affine results directly into the existing prepaid result owner. Observation
capacity refusal retains the exact callback and original request for retry.
The selected configuration checks overflow and actual allocated capacities;
generic/default dimensions remain unlimited.

The local eframe seam now prepares a non-Clone property command and its exact
token without enqueueing work. Focus, visibility and pass-through dispatch
reserve their callback and retain the original effect owner before consuming
the preparation. Rejected preparation only spends identity; no command or
terminal callback is queued. Staging reserves its complete unique owner set,
request backing and wake backing under the existing host mutex, then holds
that mutex through core publication and installs without further allocation.
The core staging query returns a borrowed exact-size iterator. The necessary
deferred declaration copy is explicitly reserved and counted before the app
releases the runtime lock to call egui. Production command drain and input
retirement no longer collect temporary command/result vectors.

Fresh qualification passes 1,813 all-feature workspace tests, 86 external
public-host tests, ten doctests, strict rustdoc and correctness/suspicious Clippy.
The local unpublished-fork native workspace passes 387 tests normally and 387
with native debug assertions disabled, plus correctness/suspicious Clippy.
The macOS programmatic tear-off, first-live, redock and retirement journey exits
0 and samples 2,389,273 owned input-data bytes at root boundaries. These results
are recorded in `/tmp/dockspace-owner-storage-qualification.json`; the tested
snapshot matches all 407 frozen source files. This is a workload sample rather
than configured-cap saturation or a physical-input/mixed-DPI qualification.

A disposable-snapshot control removes only the accepted input head's extra
pending slot. The real one-lease/accepted-head/successor regression fails
(exit 100). Restoring the exact source passes all 18 selected input, resource
and coordinator-owner cases (exit 0), with snapshot/source bytes matching again.
Results are recorded in `/tmp/dockspace-owner-storage-control.json`.

The local eframe fork commit
`bac446e142f93a5dc92695ab32e15a12e6dc7453` passes 82 Glow tests, 76 WGPU
tests and 82 combined-renderer tests with eframe debug assertions disabled.
Both renderers also compile without `native-host-seam`. The new regression
requires empty backend queues during preparation and after rejected preparation,
then verifies exact token/value order only after enqueue. Results are recorded
in `/tmp/dockspace-owner-storage-fork-qualification.json`. This fork commit is
local and unpublished; tracked dependency pins remain unchanged.

Independent reviews found no remaining concrete blocker in this slice. They
also confirmed the next boundaries: retirement's materialized and failed-create
paths must share one lifetime allowance; close owners need exact-entry
admission; route/index and semantic-wake storage, presentation/no-frame ACKs
and retirement construction still need prepaid capacity. The complete 4 MiB
product profile remains disabled. Configured-cap load/release latency, lifecycle
soak, native scoped input, native showcase and physical certification remain
open. Tracked native fork pins still name the earlier public baseline and do
not reproduce the local unpublished seam.

## 2026-10-29 U7 producer and complete-roster construction admission

`NativeInputStorageLimit::with_producer_storage_limit` selects a separate
`NativeProducerStorageLimit` for current/retired binding lifetimes, complete
work-area rosters, producer replacement and snapshot construction. Native
enrollment checks the initial registry, required bytes and overflow before
publishing a provider. The public reserved-byte query exposes the fixed peak
promise; the resident query includes producer inline state, padding and actual
vector capacities. Generic/default producer storage remains unlimited.

One flat binding entry retains admission and window/close generations through
route loss and retirement. Only successful quiescence append releases its slot;
rejected append retains that exact lifetime. Registration also checks existing
queued demand before append. With one selected lifetime, a refused second
registration does not block the first accepted prefix. A candidate-local
allocation budget covers semantic/pointer tear-off and automatic recovery.
Full storage returns ordinary rejection or host recovery without minting a new
binding. Final prepublication preparation independently checks the complete
old/new union and constructs native binding/admission reports, including reset
and document restore. Dropping either preparation publishes neither candidate.

Snapshot ingestion validates each actual iterator entry without trusting size
hints, reserves bounded scratch and all normalized vectors, and installs its
generation candidates only after successful recorder append. Platform snapshot
constructors sort in place and validate without temporary tree allocations.
Caller-owned observation iterators, work-area buffers and delivered report
containers remain host data; final recorded payloads belong to transport, while
producer storage covers their construction overlap.

The native adapter uses the selected window/display bounds for a distinct fixed
snapshot promise under its existing private host allowance. Conversion reserves
the whole live-plus-destroyed merge, ACK/focus vectors, old/new display-token and
route storage, and future committed routes. Work-area commit fills and moves the
already prepared backing after core publish. Capacity failure leaves the exact
source roster unacknowledged and does not record Unknown. Borrowed work-area
overflow normalizes to Unknown before validation and preserves complete window
facts; both borrowed comparison and frozen-source equality retain that capacity
diagnostic so changed authority can be captured again.

The real native-release regression initially exposed an invariant in the
presentation-delayed path: a full lifetime store rejected creation, but pending
release settlement required unconditional delivery. It now consumes the original
pending release with one rejection using its original cause/session. Other proof
types retain their existing delivery invariant. The regression drops two
candidates, verifies that the accepted preview report is recorded only once,
then requires one terminal rejection and no feedback on the next frame.

Fresh all-feature workspace qualification passes 1,813 tests. The external public
host passes 86 tests, including fixed-quota registration, drop/retry, retirement
and quiescence progress without private reconfiguration. Ten doctests, strict
rustdoc and correctness/suspicious Clippy pass. Results are recorded in
`/tmp/dockspace-producer-storage-qualification.json`.

The matching local unpublished-fork native workspace passes 360 tests normally
and 360 with native debug assertions disabled, plus correctness/suspicious
Clippy. The macOS programmatic tear-off/first-live/redock/retirement journey
exits 0 and samples 2,385,753 owned input-data bytes at root boundaries. This is
a workload cross-check, not configured-cap saturation. Adapter regressions cover
one-byte-short configuration, exact original-envelope retry through the production
capacity barrier, whole live/destroyed merging, prepared ACK/focus backing across
drop, shared host headroom, logical window rejection before capture identity, and
work-area normalization/deduplication. Work-area tests also compare old/new
construction peaks and backing identity through commit and Unknown revocation.
Tracked fork pins remain unchanged and do not reproduce the local seam yet.

A disposable-snapshot negative control removes only the presented-release
capacity terminal. The real native-release regression fails with
`InputPrefixReductionFailed` (exit 100). Restoring the exact implementation
passes all ten producer regressions, both normally and without core debug
assertions; results are recorded in
`/tmp/dockspace-producer-storage-control.json`. Final accounting review also
corrects the inline `Option` for prefix retirement: transport charges it even
while empty, producer does not charge it again, and owned receipt backing is
added only while held. The exact byte-boundary and ownership-transfer tests
check that distinction without claiming physical destruction or settlement.

This qualifies the selected producer/construction primitive, not the complete
4 MiB product profile. Native host lifecycle/command owners, successful-dispatch
ACK storage and their constructor overlap still need a common prepaid allowance.
Configured-cap load/release latency, lifecycle soak, native scoped input and
physical platform certification remain open.

## 2026-10-29 U7 prepaid terminal reporting and effect backpressure

`NativeInputStorageLimit::with_report_storage_limit` now selects a separate
`NativeReportStorageLimit` for unsettled presentation outputs, retained stream
metadata and externally delivered effect failure reports. Enrollment validates
the report allowance and existing-store replacement overlap, preallocates the
finite backing, and only then publishes its native provider. An insufficient
allowance leaves provider identity, published state and watermarks unchanged.
The public reserved-byte query exposes that fixed promise so native hosts can
combine it with transport and their existing lifecycle/resource allowances.
The generic/default path remains unlimited.

Host-frame commit prepares the complete core transition before publication,
including document restore, and admits its affine outputs/effects there.
Presentation results occupy the original shared pending-output slot. Explicit
reporting and cross-thread Drop fill that slot without allocating another map
or queue. Generation/recorded metadata and submitted observations use flat
storage with explicit vector capacities. Successfully retiring output prefixes
can fund new emissions in the same frame; failed preparation returns no armed
output and a dropped candidate preserves every original obligation.

Effect requests prepay their future abandoned-result backing before external
delivery. A storage lease moves into explicit negative results and survives
submission rejection and cleanup correlation. Request Drop transfers its slot
into the existing session queue without a self-owning Arc cycle. Queue recording
consumes only a successful prefix in place; the first rejected result and the
remaining ordered suffix retain their backing and reservation. The adapter's
selected finite pending-result deque preallocates its complete slot set and
retries in the same backing. Accepted dispatch releases the failure-report
promise; successful-observation facts keep their original host command/lifetime
owners, whose storage is part of the remaining combined admission work.

Independent review exposed two progress failures in the first finite-report
design. First, a failed/abandoned ShowWindow behind full ingress could hold the
last report slot while accepted staging-close input required CompensatingClose.
Core now limits actual external delivery to available report slots and retains
the undelivered suffix as Requested on its original effect ledger. The accepted
native prefix can commit before the old result enters freed ingress, after which
the same compensation identity is delivered once. New application-only
operations requiring unavailable effect delivery still reject atomically.
Second, a delayed native close Accept could outlive cancellation of its plan.
After input reduction, queued close resolutions are reconciled against their
exact current plan; obsolete undelivered Accepts are invalidated, while already
delivered requests retain their causal role. Only the matching Cancel is then
delivered.

Fresh core qualification passes 1,801 all-feature workspace tests, 85 external
public-host tests, ten doctests, strict rustdoc and correctness/suspicious
Clippy. The five enrollment/atomic-application/staging-progress/deferred-close
regressions also pass with dockspace debug assertions disabled. Public-host
coverage includes one-slot reject/retry, same-frame retirement/re-emission and
cross-thread Drop with no report-backing growth. Core tests additionally cover
capacity difference of one byte, unpublished preparations, partial abandoned
drain, result leases and exact cleanup transfer. Both real staging progress
variants preserve producer watermarks and original source resources through
dropped candidates; root cancellation delivers only one exact Cancel.
Results are recorded in `/tmp/dockspace-report-storage-core-qualification.json`.

The matching local unpublished-fork native workspace passes 344 tests normally
and 344 with native debug assertions disabled, plus correctness/suspicious
Clippy. Its macOS programmatic tear-off, first-live, redock and retirement
journey exits 0 with a sampled root-boundary high-water mark of 2,385,865 owned
input-data bytes. Results are recorded in
`/tmp/dockspace-report-storage-native-qualification.json`. This sample is not
configured-cap saturation or a physical-input/mixed-DPI certification. Tracked
fork pins still name the earlier public baseline. Final independent close/
backpressure review found no further concrete blocker after the fixes.

Two disposable-snapshot controls separately disable effect-delivery backpressure
and obsolete close-resolution invalidation. Each matching behavioral regression
fails (exit 100). Restoring both exact source files passes all three staging/
deferred-close regressions (exit 0); snapshot/source bytes match again. Results
are recorded in `/tmp/dockspace-report-storage-control.json`. An initial control
command was rejected during locked metadata resolution because the native
override did not match the root workspace lockfile; source was restored before
retrying with the root's ordinary configuration. That preflight failure was not
counted as a behavioral negative result.

This qualifies the selected core report primitive and its progress semantics,
not the complete product 4 MiB profile. Producer and lifetime-owner bounds,
roster/work-area conversion and replacement peaks still require combined
admission before product enablement, configured-cap load/soak and native scoped
input qualification. Physical platform certification remains separate.

## 2026-10-29 U7 pointer translation capacity and terminal storage

The native bridge now promises pointer translation storage before accepting a
raw callback. For each accepted source it charges the maximum addition of one
device entry and one button entry, including both current and retry copies.
Existing translator descendants and the overlapping candidate are also reserved.
The selected private host allowance combines these promises with bridge-owned
data and the caller's reservation for other external work. No per-allocation
registry or second pointer/lifecycle authority is introduced.

Core retention happens before source transfer. Under the existing bridge lock,
successful transfer installs the candidate, destroys the old translator backing,
updates the current reservation and consumes the exact raw prefix. Failed core
recording leaves the original translator and raw identity available for retry.
The raw source's original count and age still belong to the full commit boundary;
its translator-growth promise moves to the installed state at retention.

Device state, removal gates and phaseful-scroll tail gates now occupy one device
map. Mouse active/cancelled state and press origin occupy one button map. Binding
retirement changes button states and touch entries in place; provider removal
and overload cancellation reuse existing entries or remove unneeded ones.
These terminals do not allocate new translator data. Retained tombstones remain
charged at the complete device-entry size, including when they are idle.
Identity and scroll-sequence counters survive cancellation. A reused removed
device or overload scroll-tail entry receives a fresh semantic identity at
explicit start; idle cursor and discrete-scroll input keep their normal entry.

Independent review found that checking pointer growth and output space
separately could spend the last output entry's headroom and strand a retained
scroll tail in Recovering. Admission now checks their sum with checked
arithmetic. The minimum-capacity regression rejects the source before minting
ingress identity; another regression retains the scroll tail through drain and
cancellation and verifies ordinary output recovery. With a further callback's
allowance, late scroll phases remain ignored and a fresh Started is accepted.

Fresh serialized qualification passes 344 native tests normally and 344 with
native debug assertions disabled, plus correctness/suspicious Clippy. Tests
exercise prepaid source saturation, retry-candidate abandonment, complete source
transfer, charged tombstones, many buttons/contacts, and a real coordinator/core
journey which drains every accepted contact before one overload cancellation.
Dropped input and cancellation frames retain their existing ownership and retry
progress. The macOS programmatic tear-off, first-live, redock and retirement
journey exits 0, sampling 2,386,698 owned input-data bytes at root boundaries.
Its measurement is a workload cross-check, not a bound on future construction.
Results are recorded in `/tmp/dockspace-pointer-storage-qualification.json`.

A disposable-snapshot negative control removes only the ordinary output entry
from the pointer-growth admission check. The minimum-headroom regression fails
(exit 100); restoring the exact source passes it (exit 0). The original and
restored snapshot bytes match the working source. Results are recorded in
`/tmp/dockspace-pointer-storage-control.json`. Independent review rechecked the
joint admission and both scroll recovery cases and found no further must-fix in
this slice.

The complete product byte profile remains disabled. Future presentation/effect
report promises and roster/producer construction overlap still need admission
before the total 4 MiB, configured-cap load/soak, scoped native input and physical
platform claims can be qualified. This slice uses the same local unpublished
egui fork as the preceding output work; tracked native pins remain unchanged.

## 2026-10-29 U7 output admission and prepaid control storage

The local eframe seam now asks `on_output_begin` for
`NativeOutputAdmission::Proceed | Defer`. Both Glow and WGPU perform that
admission before extracting egui-winit input or running application/UI updates.
A deferred candidate never enters the active-output stack, produces no pass
finalization or terminal callback, and consumes no output-completion ordinal.
Its candidate nonce may be spent. The existing after-current root wake schedules
progress, while previous framebuffer authority and buffered input stay owned by
their original framework paths. No rejected-token registry is introduced.

The native adapter limits ordinary output ownership to 256 entries and checks a
selected private host allowance before inserting another owner. That allowance
combines current bridge data with an explicit reservation for external work;
rejected configuration preserves the previous allowance. Output exhaustion
freezes the existing ordinary-admission boundary, and recovery cannot reopen
ordinary input until output capacity fits again. The full product byte profile
remains disabled while the other growth boundaries are still open.

One root control owner is inline, independently of a preallocated backing for
256 child control/staging owners. The complete backing remains charged while
empty. A child physical window cannot occupy another prepaid slot before its
existing slot retires. Diagnostic and terminal-create results use the same
exact output identity, dispatch-budget lease and continuous completion checks,
then release the prepaid owner without appending ordinary output ingress.
Already promised child staging can retain its affine paint and terminal in the
same prepaid backing until full commit. A busy protected slot defers before UI
and input consumption rather than accepting an ownerless terminal obligation.

Root diagnostic identity persists for the whole pass, including after overload
cancellation changes the admission mode. Such a pass drains accepted core work,
retains the existing framebuffer or uses NoFrame, and cannot publish a new
semantic paint or execute queued ordinary application actions. Binding references,
lookup, abandonment, budget return and retirement cover ordinary and prepaid
owners through one private output-resource collection.

The required local fork is now committed at
`af92097942f551914644f9d36f73dfba948e4997`; it remains unpublished, and tracked
native dependency pins still name the earlier public baseline. Fresh serialized
qualification passes 339 native tests normally and 339 without native debug
assertions, plus correctness/suspicious Clippy. The complete fork eframe library
passes 81 Glow tests, 75 WGPU tests and 81 combined-renderer tests with eframe
debug assertions disabled. Glow and WGPU also compile with the native-host seam
disabled. The macOS programmatic tear-off, first-live, redock and retirement
journey exits 0 with a sampled root-boundary high-water mark of 2,384,816 owned
input-data bytes. The new preallocated control backing remains included in that
sample. Results are recorded in
`/tmp/dockspace-native-output-capacity-qualification.json` and
`/tmp/dockspace-native-output-capacity-fork-qualification.json`.

The capacity regressions cover selected-byte rejection, external reservation
charging, fixed backing after transfer and recovery only after storage becomes
available. A disposable-snapshot control omits only promised external-work bytes
from admission; the reservation regression fails (exit 100), and restoration
passes (exit 0). Control results are recorded in
`/tmp/dockspace-native-output-capacity-control.json`. Fork tests cover nested child/root deferral, contiguous retry
completion, retained framebuffer preservation and disabled-host behavior. Their
model results and backend source placement do not establish a physical
keyboard/text-input retry journey or configured-cap real-window saturation.
That saturation journey remains required when the complete product profile is
enabled. Remaining device candidates, future report/drop promises, lifetime
promises and roster-construction overlap still precede the total 4 MiB claim.

## 2026-10-29 U7 close-decision owner and scheduling

`DockspaceHostFrame::pending_close_decisions(request)` visits the current
candidate's unresolved initial decisions without allocating a complete plan.
Hosts can copy a bounded prefix and retain only the exact request identity
between commits. Deferred continuations remain explicit, and non-decision
phases, foreign/compacted requests and pending earlier native input yield no
initial tokens. A dropped candidate preserves the published decisions.

The native close owner now retains one `CloseRequestId` in its existing exact
binding record. It no longer clones boxed item lists on report settlement or
copies all retained plans into the driver. The driver bounds request and item
preparation by the current shared record allowance, checks the deadline between
decisions and accounts for every submitted decision. Partial commits keep the
same owner and rearm the existing root wake. Only a successful frame clears
request IDs proven to have no remaining initial decisions; failed or dropped
frames cannot release that progress.

The public-host regression closes 300 panes in 64-decision prefixes, verifies
original-token retry after dropping a candidate, and checks deferred/foreign/
vetoed requests plus the complete native-prefix barrier. Native qualification
includes more than one batch of pane decisions and a supplied partial record
allowance. Close-owner owned-data capacity now depends on exact owner count,
independently of each plan's pane count. Core application reports retain their
existing complete-plan contract; this slice removes adapter duplicate storage.

Fresh serialized qualification passes 1,786 all-feature workspace tests,
84 external public-host tests, ten doctests, strict rustdoc, and
correctness/suspicious Clippy. The local unpublished-fork native workspace passes
337 tests normally and 337 with native debug assertions disabled, plus its
correctness/suspicious Clippy. The macOS programmatic tear-off, first-live,
redock and retirement journey exits 0 with a sampled root-boundary high-water
mark of 2,024,968 owned input-data bytes. Results are recorded in
`/tmp/dockspace-close-decision-qualification.json`. A disposable-snapshot control
removes only per-decision record charging; the 259-pane/three-record regression
fails because its allowance is not exhausted (exit 100), and restoration passes
(exit 0). Control results are recorded in
`/tmp/dockspace-close-decision-control.json`.

The total shared input allowance, remaining output/report promises and
constructor peaks are still open. This work does not enable the product cap or
qualify native load, soak or physical input.

## 2026-10-29 U7 fixed-size binding input termination

`report_native_input_binding_retired(binding)` records one provider-bound
backend fact for an exact window incarnation. Core retains held-button origins,
the preceding native capture dependency across Unknown reports, and each smooth
scroll sequence's initial native delivery through semantic termination. The fact
ends only matching live pointer streams and closes matching scroll provider
tails. Idle delivery and a successor with the same surface/token are not
ownership. Other bindings retain their streams and complete button knowledge;
already accepted releases keep their exact preview/output obligations.

The fact does not advance the pointer watermark, end the host-frame input phase,
publish window destruction or declare callback quiescence. Staged pointer state
is synchronized before subsequent same-batch input. Enrollment checkpoints
cannot be replayed to restore retired dependencies, while an existing explicit
recovery opportunity keeps its normal fresh-roster contract.

The native adapter records the fact through the original physical-window
receipt and retains once-only progress there across route-to-retirement moves.
Only successful recording installs local tail suppression. Later close/effect
failure cannot enqueue the same fact again. The adapter no longer allocates
per-device cancellation/reset vectors or a temporary retired-touch key vector,
and no longer splits one Destroyed callback across synthetic pointer prefixes.
Retired scroll lanes reject late phases until an explicit start; their local
tombstone does not retain an already closed core provider tail or dead binding.

Regressions cover one record slot ending five pressed streams after a dropped
frame, a surviving sixth stream on another binding, Unknown capture, held
origins after later delivery elsewhere, idle delivery, token/incarnation ABA,
checkpoint invalidation, suppressed scroll sequences, same-batch presented
receiver reuse, and pending dock/contained release preservation. Native tests
exercise the physical receipt, full-transport retry and later close-correlation
failure instead of authorizing retirement from an unowned synthetic event.

Fresh serialized qualification passes 1,786 all-feature workspace tests,
81 external public-host tests, ten doctests, strict rustdoc, and
correctness/suspicious Clippy. The local unpublished-fork native workspace passes
332 tests normally and 332 with native debug assertions disabled, plus its
correctness/suspicious Clippy. The macOS programmatic tear-off, first-live,
redock and retirement journey exits 0 with a sampled root-boundary high-water
mark of 2,024,888 owned input-data bytes. Results are recorded in
`/tmp/dockspace-binding-input-qualification.json`. A disposable-snapshot control
removes only staged-ledger synchronization for the binding fact; the real
presented-receiver same-batch regression fails with `ButtonAlreadyPressed`
(exit 100), while restoration passes (exit 0). Control results are recorded in
`/tmp/dockspace-binding-input-control.json`.

The shared 4 MiB allowance, remaining variable terminal storage, constructor
peaks, scoped native input and load/soak qualification remain open. The product
default is still unlimited; this removes Destroyed-binding record fanout rather
than qualifying the complete admission profile.

## 2026-10-29 U7 native core-capacity backpressure

The native coordinator can exercise the facade's selected transport limit
through one internal enrollment path, while its default product configuration
remains unlimited until complete host/terminal admission is ready. The public
core `prepare_native_pointer_capacity()` performs explicit committed-prefix
maintenance before caller iteration and reports a finite pointer-preparation
allowance or unlimited recording. A suspended candidate retains its data.
`has_pending_native_input()` distinguishes an accepted prefix which can make
progress through commit from a permanently undersized empty recorder.

Cursor claims use that allowance and the remaining scheduling budget to select
their exact raw prefix. Translation streams directly into prepared journals
instead of allocating another `Vec<NativePointerInput>`. Only successful
recording transfers the raw prefix and installs the translator candidate. A full
recorder retains the raw head and source identity, marks the existing host-frame
boundary pending, and yields so accepted input can commit. Capacity errors are
converted to that yield only while accepted core records remain; an empty core
which cannot admit the next indivisible terminal group returns the original
capacity error rather than spinning through empty frames.

Affine effect-result submission now reclaims a committed prefix after its
original provider/binding/epoch validation and before append. Rejection still
returns the original result. This prevents a retry from remaining falsely full
until a later pointer operation performs maintenance and overtakes it.

Core regressions cover temporary zero capacity, frame drop, reclaim after exact
commit, suspended-candidate preservation, an empty recorder too small for one
edge, and direct effect retry without an extra empty frame. The bounded native
journey uses a real pressed translator stream, clips a cursor prefix to two
remaining slots, retains the raw suffix at full capacity, and resumes the same
pointer after commit. A five-device terminal group against four slots explicitly
fails without consuming the raw Destroyed callback or changing translator state.

Fresh serialized qualification passes 1,780 all-feature workspace tests,
80 external public-host tests, ten doctests, strict rustdoc, and
correctness/suspicious Clippy. The local unpublished-fork native workspace passes
330 tests normally and 330 with native debug assertions disabled, plus its
correctness/suspicious Clippy. The macOS programmatic tear-off, first-live,
redock and retirement journey exits 0 with a sampled root-boundary high-water
mark of 2,024,952 owned input-data bytes. The qualification record is
`/tmp/dockspace-native-core-backpressure-qualification.json`. A disposable
snapshot control removes only the cursor-prefix capacity clipping. The bounded
journey then fails because it cannot transfer the two admissible records
(exit 100); restoring clipping passes (exit 0). Control results are recorded in
`/tmp/dockspace-native-core-backpressure-control.json`.

This establishes retry and error behavior for the bounded core allowance. It does
not yet reserve terminal fanout before creating native obligations or enable the
combined 4 MiB product admission profile. Existing obligation storage, raw/core
peaks, native scoped input and load/soak qualification remain required.

## 2026-10-29 U7 native owned-data integration prerequisite

`NativeDockspaceApp::input_storage_capacity_bytes()` now combines the current
owned data across native transport owners at a locked host boundary. It counts
the preallocated raw backing, both shared roster backings, exact route/index,
window-resource, output, close and retirement entries, queued-snapshot
acknowledgement/focus and prepared-work-area storage, retained work-area state,
effect/input controls, pointer translation devices and tombstones, core recorded
transport, and core pending report sidecars. The core sidecar query is separately
available as `native_input_pending_report_capacity_bytes()`.

Each module counts inline state and existing key/value data plus the actual
owned descendant capacities. Parent owners deduct already-inline child state.
Close decision plans include their boxed item data. Independent entries remain
charged even when they refer to the same window lifetime. Shared roster data is
charged through its two backing owners; external fixture allocations are compared
through existing Arc handles, including a queued snapshot's last reference.
Shared abandoned effect/presentation queues are counted through the session,
without following each affine handle back into the same allocation. No address
registry, allocator-node estimate or source parser is introduced.

Effect command storage now has the finite `Close | Focus` representation used by
its actual producers; the typed queue cannot silently accept an unmeasured
dynamic command payload. Work-area commit allocates its known complete route
capacity before extending it, preserving exact validation and Unknown routing
revocation. Tests cover raw transfer/commit retaining backing, report settlement,
close-plan decisions, full callback/retirement barriers, shared and distinct
roster allocations, queued sidecar spare capacity, retained command capacity and
input tombstones. The real-window smoke samples owned input data after each root
UI callback, records its high-water mark and checks that profile against 4 MiB.

Fresh serialized qualification passes all 1,778 all-feature workspace tests,
80 external public-host tests, ten doctests, strict rustdoc, and
correctness/suspicious Clippy. The local unpublished-fork native workspace passes
328 tests normally and 328 with native debug assertions disabled, plus its
correctness/suspicious Clippy. The macOS programmatic tear-off, first-live,
redock and retirement journey exits 0 and samples a maximum of 2,024,872 owned
input-data bytes at root boundaries. A disposable-snapshot control disables only
same-allocation roster deduplication; its regression fails on duplicate charging
(exit 100), and restoring the implementation passes (exit 0). Results are recorded
in `/tmp/dockspace-native-owned-storage-qualification.json` and
`/tmp/dockspace-native-owned-storage-control.json`.

The measurement excludes allocator/BTree-node/Arc metadata, opaque callbacks/clock closures,
core workspace/authority, renderer/receiver data, application action/transition queues,
diagnostic errors and caller buffers. It measures current data at stable
boundaries, not transient constructor peaks or promised future terminal storage.
The final shared allowance still must reserve that future storage before
admission, combine the host and recorder allowance, and qualify overload/load,
native scoped input and soak. A passing sampled smoke does not complete those
requirements or establish physical-input or mixed-DPI support.

## 2026-10-29 U7 bounded prepared-pointer transfer

Public native pointer batch/sequence recording now prepares a bounded vector of
`PointerEdgeJournal` handles directly, replacing whole-iterator
`Vec<NativePointerInput>` collection and the old private append wrappers. Each
edge is compiled against the producer's captured pointer watermark. Journals
move into the recorder after successful preparation; their owned contents are
not copied. Atomic batches retain one commit boundary, while sequences keep
their independent boundaries.

The optional transport allowance reserves N prepared-journal handles. Its
read-only preparation limit checks both the staging peak beside all currently
retained payloads and the final append after permitted prefix retirement. A
core-issued watermark must match the recorder's provider, ordinal and record
identity before it can release prospective slots or data. A suspended replay
receives no retirement credit. Fixed backing credit is never released. The
staging check cannot subtract committed payloads which are still physically
retained during caller iteration.

Caller-controlled `size_hint` does not choose allocations. A full allowance
rejects at the first nonadmissible item without reading the remaining suffix.
Iteration and the iterator's destructor finish before maintenance or append;
`into_iter`, `next` and exhausted-iterator `Drop` panics preserve committed
payloads, buffer backing, producer watermarks and non-rewinding append identity.
Invalid later facts also fail during preparation. Successful preparation is
followed by existing prefix reclamation, an actual-capacity preflight and one
append transaction. Single-edge recording checks capacity before allocating its
journal contents.

The new public-path regressions cover early rejection of an unbounded iterator,
huge size hints, all three caller panic boundaries, captured-sequence ordering,
unchanged backing, staging-peak rejection and later successful retirement credit.
Recorder tests independently reject forged retirement identity and prove that
committed data remains charged at the staging peak. The original pointer-prefix,
atomic-257, replay and byte/slot tests remain ordinary gates.

Fresh serialized qualification passes all 1,777 all-feature workspace tests,
80 external public-host tests, ten doctests, strict rustdoc, and
correctness/suspicious Clippy. The local unpublished-fork native workspace passes
316 tests normally and 316 without native debug assertions, plus its
correctness/suspicious Clippy and the macOS programmatic tear-off, first-live,
redock and retirement journey. A disposable-snapshot negative control changes
only staging admission to subtract committed data prematurely; the peak
regression fails because an over-budget preparation is admitted. Restoring the
implementation passes the same regression. The qualification record is
`/tmp/dockspace-prepared-pointer-qualification.json`; the control results are
`/tmp/dockspace-prepared-pointer-peak-control.json`.

This closes public pointer iterator collection inside the selected transport
allowance. Producer maps, callback buffers, window/output/terminal resource
owners and pending report queues still require combined native admission before
claiming the complete 4 MiB gate. Native scoped input, load/latency, soak and
physical-platform qualification remain open.

## 2026-10-29 U7 optional recorded-transport admission

`NativeInputStorageLimit` lets a managed host select a retained record limit
and a byte allowance through
`enable_managed_native_host_with_input_storage_limit`. Generic recording and
the existing default enrollment remain unlimited. The configured allowance
reserves actual recorder backing, one full N-record replay batch, its overlapping
commit guard, and prefix-retirement receipt storage. This includes whole atomic
groups larger than the 256-record scheduling boundary. Payload and descendant
capacities are charged once through the recorder; rollback and retirement return
only their corresponding data charge, without releasing backing credit.

Append rejection precedes payload/liveness Arc construction and record identity
allocation. Snapshot and pointer-roster construction use prospective finite
capacity checks and explicit vector capacities; final append checks the actual
capacity. Recorder configuration and the initial checkpoint complete inside the
existing engine enrollment candidate, before either provider is published.
Insufficient storage returns `InputCapacity` without panic or partial enrollment.
Prefix settlement transfers the original receipt binding buffer into its result
instead of creating a second transport copy.

Polling reclaims committed input before recording abandoned effects. A full
effect or presentation prelude keeps its failed item and remaining suffix under
the existing owner while accepted input drains. Capacity failure does not
discard already collected presentation settlement entries, including a later
stream whose report was recorded before an earlier stream became reportable.
Pending report submission survives suspended atomic replay, candidate drop, and
commit, and continues to block application input and overload cancellation.
An empty recorder which cannot admit its next report returns a capacity error
instead of returning an empty pending frame indefinitely. The native coordinator
retains affine effect results rejected for input capacity for exact retry.

Boundary regressions exercise insufficient fixed/checkpoint storage, exact and
one-byte-short snapshot budgets, full slots, whole-batch rejection, 257-record
atomic replay, prefix retirement, affine effect retry, abandoned effects before
and during replay, an effect abandoned after poll, and exact partial-stream
presentation settlement. A separate external consumer uses only the public
limit/enrollment/input/frame APIs and verifies failed enrollment, full admission,
drop/retry, actual capacity, and subsequent successful input.

Fresh serialized qualification passes all 1,772 all-feature workspace tests,
80 external public-host tests, ten doctests, strict rustdoc, and
correctness/suspicious Clippy. The local unpublished-fork native workspace
passes 316 tests normally and 316 with native debug assertions disabled, plus
its correctness/suspicious Clippy. The final source completes macOS
programmatic tear-off, first-live, redock and retirement with exit 0.
The command results are recorded in
`/tmp/dockspace-native-bounded-recorder-qualification.json`; the workspace test
log is `/tmp/dockspace-native-bounded-recorder-core.log`.

This primitive bounds recorded and replay transport. Temporary public iterator
collection, producer maps and host-side callback/resource/report storage remain
outside its measurement boundary. Prepared journal transfer must close the
iterator peak before the final shared 4 MiB allowance is enabled. This evidence
does not complete U7, load/latency/soak, native scoped input, or physical-platform
qualification, and does not change the unpublished-fork pin limitation.

## 2026-10-29 U7 single-record native device removal

An uncorrelated raw DeviceRemoval now records one provider-bound fact in joined
backend order, replacing the adapter's per-pointer/per-scroll synthetic terminal
Vec. Recording advances the backend ordinal once without advancing the pointer
watermark. Core uses the shared managed-input cancellation kernel with
PointerStreamCancelled and DeviceRemoved scroll reasons; this observed fact
does not close the host-frame input phase or reuse the overload outcome.

The reset clears live streams and revokes button/capture facts to Unknown while
preserving the provider, clock, identity counters, native lifecycle and consumed
release/output obligations. Frame staged and candidate pointer ledgers are
synchronized before the next record, and presented interaction authority is
refreshed. A fresh complete roster can recover authority at the committed reset
boundary, including after later edges. No empty physical roster is invented.

The adapter first records the fact, then forgets its device state and retains
late-tail tombstones; recording failure cannot clear the translator or consume
the raw callback. A dropped candidate replays the same record. The shared native
gesture/scroll and exact pending-drag/contained-release fixtures cover both
overload cancellation and device removal without duplicating their setup.
Transport tests cover one-record cost, unchanged pointer sequence, drop/retry,
subsequent same-frame input and nonmanaged rejection. The public consumer test
also submits an application action after the fact in the same frame. Adapter
normal/quarantine journeys use the production capture gate and retain a single
record across frame drop.

The same-batch real-receiver regression starts from an actual pressed pointer,
records removal then presses that same pointer at the exact presented receiver,
and requires a strictly newer stream incarnation. A disposable-snapshot
negative control removes only DeviceRemoval's staged-ledger synchronization;
the regression fails with ButtonAlreadyPressed at the post-removal press. This
isolates the reset synchronization from the remaining cancellation and
presentation-authority refresh behavior. Restoring the implementation passes
that exact regression.

Fresh serialized qualification passes all 1,753 all-feature workspace tests,
79 external public-host consumer tests, ten workspace doctests, strict rustdoc,
and core correctness/suspicious Clippy. The local unpublished-fork native
workspace passes 316 tests normally and 316 with native debug assertions
disabled, plus its correctness/suspicious Clippy. The final source completes the
macOS real-window programmatic tear-off, first-live, redock and retirement
journey with exit 0. Independent core/adapter review reports no confirmed P1/P2
in this slice. This evidence does not add physical input or remote-pin claims.

This removes DeviceRemoval transport fanout, not the O(N) core cleanup work.
Destroy-binding and caller-owned atomic groups remain separate capacity work;
the complete 4 MiB shared allowance, load/latency and soak qualification are
still open.

## 2026-10-29 U7 preallocated raw and roster storage

The native raw journal now allocates its 4,096 callback slots at initialization
and cannot grow at push. A rejected push receives no capture ordinal. Pop,
transfer and commit reuse the same backing. Fatal quarantine shares the source
count/age boundary instead of bypassing capacity. A DeviceRemoval may still end
an existing provider tail while quarantine is Running, but cannot cross its
fixed boundary; a new close cannot create an ordinary owner after freeze.
Existing exact close successors retain their single parked owner slot.

Ordinary and terminal root rosters share two preallocated payload backings,
each with 256 window slots and 64 work-area slots. Capture mutates only a
uniquely owned Arc and never creates a third backing. Complete borrowed facts
are compared before capture, including geometry, backend and work areas, so an
unchanged root does not generate another snapshot. Backing exhaustion freezes
ordinary admission without minting a roster ordinal. The current output becomes
diagnostic and retains an after-current wake; it cannot wait for a new semantic
paint. Accepted snapshots still settle without a newer capture, releasing a
backing for the protected obligation.

New lifetime admission checks the union of route, resource and retirement roots
against the reserved window capacity. Duplicate roots share one lifetime
commitment; moving a window into retirement does not return it prematurely.
Checks precede route reserve/bind/attach/replace and create reservation. Both
coordinator reserve paths now use that bridge boundary. Oversized work-area
facts revoke only work-area authority to Unknown, retain the full window roster
and preserve reusable backing. Invalid and Unknown facts also retain storage.

The new tests cover raw full/reject/reuse identity, quarantine full and exact
age boundaries, close successor admission while frozen, duplicate lifetime
roots and retirement capacity, both terminal receipt barriers, retained Arc
references, rejected retry and ordinary-to-terminal backing reuse. The pool
tests are private owner/receipt models; they do not fabricate fork output tokens
or claim a complete real-token exhaustion interleaving matrix.

Fresh serialized local-override qualification passes 316 native workspace tests
normally and 316 with native debug assertions disabled, plus
correctness/suspicious Clippy and formatting/diff checks. The final source
completes the macOS real-window programmatic tear-off, first-live, redock and
retirement smoke with exit 0. A disposable-snapshot negative control removes
only the raw push capacity check: its full/reject/reuse regression fails at the
unexpectedly accepted overflow; restoring the implementation passes that test.
Independent final review reports no remaining confirmed P1/P2 in this slice.
The quarantine DeviceRemoval journey also commits mouse and touch terminals
through the production capture gate, checking exactly two new core records and
an empty translator after commit.

Core/replay and prelude/fanout growth still need their shared byte allowance.
These bounded raw and roster stores do not yet qualify the total 4 MiB budget,
native input-load/latency, lifecycle soak or physical-platform input.

## 2026-10-29 U7 reclaim before native input append

Native facade recording now retires committed input before appending its next
prefix, rather than retaining the previous payloads until another frame poll.
The recorder's backing capacity remains charged and reusable. A suspended
atomic replay skips this maintenance and keeps its original private candidate,
presentation attempt, ingress watermark and replay buffer across later appends.
Retirement preserves the existing presentation-compaction/error ordering.

Public batch/sequence entry points first perform a read-only host check and
fully collect the caller's iterator, then reclaim and append. The first full
gate caught retirement occurring before an iterator panic; the corrected order
passes the existing exact-state panic regression without weakening its checks.
An invalid appended suffix still rolls back to the savepoint after committed
maintenance, preserving all previously uncommitted input.

Fresh serialized qualification passes all 1,748 all-feature workspace tests and
correctness/suspicious Clippy. New coverage checks committed 256 plus next 256
payload reclamation with unchanged recorder capacity; the retained atomic 257
test also verifies its unchanged candidate identity and storage. This removes
one transfer overlap but does not implement the complete 4 MiB admission cap.

## 2026-10-29 U7 fixed payload footprint reduction

The backend ingress payload now boxes its low-frequency semantic `EngineInput`.
The recorder API and total ordering remain unchanged; reduction moves the boxed
input into the existing semantic path. Platform and pointer payloads no longer
reserve inline space for the largest semantic input. On the freshly compiled
arm64 test artifact, payload data decreases from 584 to 248 bytes, saving 336
data bytes per nonsemantic record before allocator overhead. These sizes are
artifact-specific, not a cross-platform ABI or a total-memory claim.

Semantic records pay for one additional Box allocation. The native input query
charges that boxed data once through its recorder, while immutable batches and
their clones continue sharing the same outer payload and box. Regressions bound
the fixed envelope relative to the actual snapshot types and verify shared
semantic box identity and capacity accounting. This reduces the ingress storage
cost; ordinary byte admission and protected variable-payload backing are still
unfinished.

Fresh serialized checks pass 1,747 all-feature workspace tests, 306 native
workspace tests normally and 306 without native debug assertions, plus core
correctness/suspicious Clippy. The final boxed source also completes the macOS
real-window programmatic redock smoke with exit 0. Independent review found no
confirmed blocker in the boxing, reduction or byte accounting changes. Existing
strict docs and external-consumer qualification in the transport prerequisite
remain separate evidence; this layout-only slice adds no public API.

## 2026-10-29 U7 native snapshot capacity preparation

The native adapter's raw journal now reports inline envelope data plus its
`VecDeque` backing capacity. Pop, raw-to-core transfer, source commit and clear
do not falsely subtract retained backing. Parked ingress remains charged to its
existing external owner rather than being counted as another raw queue slot.
This diagnostic helper is not yet an admission limit.

Roster conversion now borrows frozen work-area facts, moves the core work-area
Vec into the session, and retains only settlement data in the prepared sidecar.
Unknown work areas revoke routing while preserving display token history and
its monotonic frontier, without cloning that map. Restoring Exact facts reuses
the original display token. Destroyed observations are consumed into the
compiled roster without a second intermediate Vec. Capture and validation use
unstable sorting while preserving duplicate-identity rejection.

Serialized local-override qualification passes 306 native workspace tests
normally and 306 with native debug assertions disabled, plus
correctness/suspicious Clippy and formatting/diff checks. The macOS real-window
programmatic redock smoke completes tear-off, first-live, redock and retirement
with exit 0. Independent review found no confirmed P1/P2 in this slice. These
checks use the local unpublished fork, do not change tracked remote pins and
do not certify physical input or the unfinished 4 MiB admission budget.

## 2026-10-29 U7 recorded-input capacity measurement prerequisite

The public session and open host frame now expose
`native_input_owned_capacity_bytes()`. The query counts the backend recorder's
fixed envelope, record-buffer capacity, shared payload/liveness data, payload
vector capacities, replay cursor or commit-guard buffers, and a pending
prefix-retirement receipt. Replay batches add their own record buffers without
charging their shared payloads again. Full commit alone does not retire recorded
payloads, and prefix retirement leaves the recorder's spare capacity charged.

This is recorded input transport capacity, not total runtime or process memory.
Host buffers, pending effect/presentation reports, reducer authority and
candidate state, renderer state, allocator overhead, and Arc control blocks are
excluded. The finite native producer contract is explicit: an unsupported
semantic shape produces an invariant error rather than a partial byte total.
In particular, the three fixed native close actions are accepted, while a
low-level dynamic rehome program cannot silently bypass this boundary.

Fresh serialized qualification passes 1,745 all-feature workspace tests, 79
external public-host consumer tests, ten workspace doctests, strict rustdoc and
correctness/suspicious Clippy. Regressions cover spare capacity, shared payload
identity, suspended atomic replay, open-frame commit guards, payload retirement,
distinct native pointer contents, and rejection of unaccounted semantic data.

The query does not itself enforce admission. Ordinary 4 MiB accounting, transfer
peak reservations and preallocated protected variable payloads remain open.
This prerequisite does not complete U7 or change the existing fork pin limits.

## 2026-10-29 U7 fixed-boundary source admission and cancellation checkpoint

The optional native adapter now freezes ordinary admission at the first capture
boundary B when source count reaches 4,096 or oldest retained source age reaches
250ms. A rejected ordinary callback receives no capture ordinal. Source count
and age include ready owner envelopes, parked close successors and records
already handed to core until their complete original boundary commits. This
checkpoint does not implement the separate 4 MiB owned-capacity limit.

One private Running/Draining/Recovering state shares a derived dispatch query
across callback replay, output prefixes and frame polling. Freeze first drains
any already recorded core prelude or atomic suffix. Only currently replayable
accepted records through B may then enter core. Post-B protected terminals stay
in their original slots without moving B. An unresolved external callback or
parked close successor cannot delay the independent live-input cancellation.
Full report settlement still precedes eligibility for that cancellation.

The cancellation frame records the existing core overload action, retaining its
exact submission and B. Only a successful commit with the matching outcome
finishes B and resets the translator once. Drop and failure retain Draining and
the previous translator. Reset preserves identity counters and produces no
synthetic releases or claim of an empty physical roster. Exact pending releases
retain their existing output obligations. Recovering resumes ordinary admission
only after count and age recover; fatal/shutdown mode still independently gates
ordinary work.

Terminal viewport rosters now use one inline envelope slot rather than the
ordinary queue. Repeated requirements coalesce by generation while occupied.
The slot cannot be reused until both its complete core boundary and exact
snapshot settlement arrive, in either order. Rejection rearms the same
outstanding requirement after both gates. Freeze and later lifecycle callbacks
request this protected snapshot path, so old parked-source age cannot prevent
the observations needed to finish existing effects. Variable-size roster
payload storage still allocates and remains part of the unfinished byte budget.

Control outputs keep their shared dispatch budget and close their exact token
without appending another ordinary output record. All Draining prelude/prefix
frames keep product UI pending and retain previous presentation, while the
actual core report controls boundary release. The finalizer defers queued
application actions when an unprocessed callback remains. Pending effect,
quiescence and close-policy preludes cannot continually preempt cancellation.

Independent review found and closed two recovery defects: Draining prelude
frames could otherwise paint into an abandoned diagnostic output, and fatal
mode could prevent Recovering bookkeeping from ever finishing. Source tests now
cover fatal before and after cancellation, inert prelude and final prefix,
ordinary rejection after fatal, and eventual quiescence. Other new regressions
cover 4,096/4,097 capture admission, the 249/250ms boundary, post-B exact command
and Destroyed receipt retention, an atomic 257-record core suffix, cancellation
drop/retry, parked-close promotion with original identity/age, and terminal
roster settlement in both orders. Close journeys remain tokenless owner models;
they do not certify canonical fork Hold/Veto callback delivery.

The final boundary review also found that a new effect failure derived from a
committed pre-B frame could bypass the pending-result owner and enter core
before cancellation. Failed, unsupported, cleanup and shutdown-derived results
now share a Draining gate and remain in the existing pending owner. A real core
close-effect regression verifies unchanged producer count through cancellation
drop/retry, then one later result recording after the exact cancel commits.
The observed but unadmitted child compensating-close fixture covers the same
gate for a failed original create effect generated by retirement dispatch.
Callback results belonging to the accepted pre-B callback remain part of that
original ordered prefix.

Fresh serialized qualification passes 303 native workspace tests normally and
303 with native debug assertions disabled, plus correctness/suspicious Clippy,
formatting and diff checks. The frozen final source completes real-window macOS
programmatic tear-off, first-live, redock and retirement with exit 0. A
disposable-snapshot negative control removes the Draining UI gate and fails the
inert-prelude regression; another removes derived-result deferral and fails
both the close-effect and compensating-create result regressions at premature
producer-count advancement. Each restored implementation passes its exact
regression. Independent final review reports the identified boundary defects
closed, with no remaining confirmed P1/P2 in this slice. These checks do not
provide a complete real-token output/callback interleaving matrix or physical
input certification.

Byte ownership measurement and preallocation, the 1,000Hz/100ms release profile,
10,000-cycle lifecycle soak, native scoped device/time/stationary capture and
physical-platform qualification remain open. This slice does not complete U7,
U8 or the full plan, and does not change the unpublished fork or remote pins.

## 2026-10-29 U7 create and physical-window terminal storage prerequisite

An admitted create receipt now holds its sole Created-or-Failed callback
envelope inline. A physical-window receipt similarly holds its sole Destroyed
envelope. Live routes and existing retirement entries participate in the same
mailbox head, output barrier, retained count, original age and binding-retention
queries. Only the first exact terminal invokes the capture closure. Transfer
requires the existing semantic acknowledgement, and complete commit additionally
requires handoff of that original envelope. Neither a duplicate nor an unrelated
full boundary may reopen or prematurely release the receipt.

Destroyed capture fills the already attached window receipt before any move to
retirement. Its pointer suppression is stored inline on the route, replacing
the separate set. The reducer validates the exact Received viewport, binding,
window and event ordinal before moving the same receipt into retirement; the
capture ordinal and age travel with it. Failure similarly moves the original
create receipt to unmaterialized retirement before payload handoff. Missing
effect-result recording still blocks failure quiescence after source accounting
has completed.

Review identified a remaining Created capture allocation in the reverse window
index. `capture_output_window` now fills only the existing route's window and
inline receipt; the Created reducer publishes the index through `attach_output`.
Until then, exact-window route lookup falls back to the existing route records.
Window conflict checks, pointer queries and output admission use that same
lookup. Repeated capture cannot rebuild the index or receipt. A Destroyed event
arriving in that interval remains Received, with its suppression and envelope
unchanged when the reducer later publishes the index. Retirement map insertion
also occurs in the reducer rather than terminal capture.

Four bridge regressions cover raw/Destroyed/raw capture order, unchanged age and
identity across the route-to-retirement move, A1 retirement with active A2,
create-failure result-recorded variants, and Destroyed before Created index
publication. A separate route model verifies absent index entries, exact-window
fallback, peer exclusion and duplicate capture before publication. The new
retirement model rejects a commit between semantic transfer and envelope handoff.
All envelopes are captured by real journals; no arbitrary capture identity or
opaque renderer token is fabricated. The Created tests remain route/receipt
models rather than a complete opaque-token callback/effect matrix.

Serialized qualification on the frozen source passes all 283 native workspace
tests normally and all 283 with native debug assertions disabled.
Correctness/suspicious Clippy, formatting and diff checks pass. The real-window
macOS programmatic redock reaches tear-off, first-live, redock and retirement
with exit 0. A disposable-snapshot negative control removes only Destroyed's
envelope-handoff commit guard: the retirement regression fails at the premature
commit assertion, and passes again after restoration. Independent final reviews
found no remaining confirmed P1/P2 in the terminal-storage changes. No allocator
instrumentation or new physical-input certification was performed.

The source changes close the inline terminal-storage prerequisite for the
existing output/create/command/close/staging/window owners. This is not a global
allocation profile or completed overload protection. Fixed-boundary ordinary
freeze, once-only committed cancellation and translator reset, byte/count/age
admission, protected variable-size terminal-roster capacity,
sustained-load/release-latency and soak gates remain open. The fork is
still local at `1f7c541fde99fd69192622a1a36cb642de58ed29`; tracked remote pins are
unchanged.

## 2026-10-29 U7 single native host-state prerequisite

The native bridge now owns one mutex-protected `NativeHostState` containing
routes, ingress, resource reservations and retirement. Coordinator and host
frames no longer retain a separately shared viewport-map lock. Live and retired
receipts keep their previous sole owner; this change does not duplicate them or
add a lifecycle registry. Private route closures return copied or owned results
before core, eframe or another bridge operation runs.

Staging route/binding/duplicate checks and resource publishability now use one
state snapshot. No-frame readiness reads output reservation and exact route
together, including a valid route after the output reservation disappeared.
Debug formatting, frame commit, prepared route retirement and callback capture
avoid nested bridge locking. The finalizer remains outside the state mutex.

Review found that duplicate finalizer installation could drop the rejected
external trait object while holding the host lock. Installation now keeps its
atomic attachment check but moves the result out before disposal. A regression
uses a destructor which checks lock availability and re-enters the real bridge
query; it does not require a fake output token or a blocking timeout.

The frozen local-fork workspace passes 277 all-target tests normally and 277
with native debug assertions disabled. Correctness/suspicious Clippy, formatting
and diff checks pass. A real-window macOS programmatic redock completes with
exit 0. Restoring the rejected-finalizer lock scope in a disposable snapshot
makes the destructor regression fail at its lock-availability assertion, without
blocking; restoring production code passes. Independent review found no remaining confirmed lock or
ownership P1/P2 in this migration. Create/Destroyed inline terminal storage,
fixed-boundary overload admission, capacity/load/soak and physical-platform
qualification remain open. The unpublished fork and tracked remote pins are
unchanged.

## 2026-10-29 U7 output, staging and close terminal storage prerequisite

Exact output results now retain their callback envelope inside the output
resource reserved at output begin. Staging paint and close cancellation use
their existing inline receipt slots. None of these accepted terminal paths
pushes into the ordinary raw queue. The mailbox merges raw records and ready
resource slots by their original capture ordinal; it does not register a
second terminal queue or cache another head. A submitted output keeps its
envelope until its complete core boundary commits. Callback transfer keeps
the journal's count and original age until that same boundary.

A close successor parked behind Clear now becomes ready in its existing close
owner. Promotion neither allocates ordinary queue capacity nor changes its
capture identity or age. The old sorted reinsertion and raw retain interfaces
are deleted. Staging Received/Transferred slots survive clear and retirement;
only Awaiting work may be revoked there. A newer staging basis waits for full
commit and is reissued from core's existing per-frame lifecycle requests.

Independent review found a fatal-drain boundary: a captured staging callback
could try to bind paint after quarantine abandoned or retired its output.
The reducer now checks output eligibility after predecessor replay and budget
checks, drops the candidate and transfers that exact staging ingress without
producing paint. Discard is idempotent when the output terminal has already
removed its owner. This preserves the complete-commit requirement without
reopening a retired output.

The new regressions exercise capture order across reversed resource-map keys,
first/exact-basis staging capture, duplicate and wrong-ordinal rejection,
Received/Transferred retention through retirement, reissue after commit, and
close promotion with unchanged identity and capture time. Staging requests and
close capabilities come from public core lifecycle operations; the private
receipt models deliberately do not fabricate opaque eframe output/close tokens.
The actual quarantine staging-reducer branch and a complete
Output1/Callback/Output2 partial/full-commit matrix still require a genuine
native runner fixture. Static review and programmatic native compatibility do
not replace that missing focused coverage.

Serialized local-fork qualification passes all 276 native workspace cases
normally and all 276 with native debug assertions disabled. The five new cases
pass in that same matrix. Correctness/suspicious Clippy, formatting and diff
checks pass. A real-window macOS programmatic redock again reaches tear-off,
first-live, redock and retirement with exit 0. In a disposable snapshot, changing
staging clear to remove Received as well as Awaiting makes the retirement
regression fail at retained binding ownership; restoring the implementation
passes the same test. No production-tree mutation is used for that control.

This prerequisite does not complete protected overload admission. Create and
Destroyed envelopes still need their own pre-reserved storage joined with the
same ordered head. Fixed-boundary freeze, ordinary record/byte/age caps, load,
release latency and soak remain open. The local fork revision is unchanged at
`1f7c541fde99fd69192622a1a36cb642de58ed29`; no dependency publication or platform
physical certification is implied.

## 2026-10-29 U8 resource and command terminal ownership checkpoint

Native output, deferred-window, create-attempt, physical-window and held-close
reservations now retain their existing resource owner through raw handoff,
partial replay, route suppression and complete core commit. An admitted create
keeps one non-Clone terminal receipt and moves it into the existing retirement
entry on failure. A physical window reserves its Destroyed receipt on attach;
displacement and release move the same receipt into retirement. Uncommitted
receipts prevent tombstones and quiescence. Active and retired ownership is
checked before rebinding a window or binding.

Held close captures its original journal reservation in the sole close owner
before returning Hold. Cancellation requires both semantic Clear and a complete
core boundary commit before its fixed successor can publish. Dropped frames and
partial replay retain the source identity and age. The duplicate window-resource
close slot and lifecycle close counter are removed. Existing tests use exact
adapter model identities; the programmatic redock run does not exercise a real
canonical WM Hold/Veto request.

The three property-command receipts now contain an inline terminal envelope.
Only the first exact token/window/value result captures an ordinal; duplicates
cannot recapture. The unified mailbox head compares raw and owner-held records
in capture order. Cursor claims and output-prefix barriers stop at either kind
of head. Transfer preserves count and oldest age until complete commit, while
retired A1 command receipts can coexist with an active A2 resource. This slice
does not yet move every other lifecycle terminal payload out of the raw queue.

Independent review found that fatal-quarantine roster rearming scanned only the
raw queue and could miss an owner-held pointer or visibility result. It now
scans both. A second issue left a command Awaiting forever when eframe forgot
its queued operation before execution. Local fork commit
`1f7c541fde99fd69192622a1a36cb642de58ed29` reports an exact Retired result after
releasing host locks, without performing or claiming an OS operation. The
adapter consumes it through the original command receipt and acknowledges the
associated input effect as rejected. Focus and presentation remain ungranted.
Reentrant callbacks and replacement-window queue ownership are covered in the
fork.

Fresh serialized qualification on the frozen source:

| Gate | Result | Scope |
| --- | --- | --- |
| Local-fork native workspace, all targets | 271 passed | Resource/close ownership, exact command ingress, dispatch retirement and existing native compatibility |
| Same workspace, native debug assertions disabled | 271 passed | Includes both fatal-roster rearming cases |
| Fork host-seam tests | 63 passed | Includes ordered retirement, replacement guard and callback reentrancy |
| Fork WGPU build | Passed | Native-host seam enabled |
| Correctness/suspicious Clippy and formatting | Passed | Native workspace; fork formatting also passes |
| Real-window macOS programmatic redock | Exit 0 | Tear-off, first-live, redock and retirement reach completion |

A disposable-snapshot negative control removes the owner-held callback scan
from fatal quarantine. The rearming regression fails at the expected missing
capture assertion; restoring production source passes. The main working tree
is unchanged by that mutation. Independent final reviews found no additional
confirmed P1/P2 in the command-terminal slice. There is no dedicated native
output-token/callback interleaving fixture in these new tests; the shared
output-prefix path is reviewed and exercised by native compatibility and the
programmatic smoke, without claiming a complete output interleaving matrix.

The fork remains local and unpublished. Tracked native manifests and CI still
pin `0716beee5b7284389f2e80ffb6b24b5af6ae2910`; verification uses explicit local
overrides in a refreshed disposable snapshot. This checkpoint is a local
resource-ownership prerequisite, not a remotely reproducible native baseline.
Protected terminal slots for the remaining resource kinds, fixed-boundary
ordinary admission freeze, record/byte/age caps, sustained-load/release latency,
soak and physical-platform qualification remain open U7/U8/U11 work.

## 2026-10-29 Disjoint touch-cut buffer reuse

Touch fragment subtraction now checks for a positive intersection before
rebuilding its buffer. Disjoint cuts, empty cuts and an empty fragment set return
without replacing the Vec; overlapping cuts retain the existing exact partition
logic. A test-only counter measures buffer rebuild paths, not allocator calls or
RSS. The regression submits 1,024 disjoint cuts and two empty cuts with zero
rebuilds, then verifies the actual overlapping split and full removal.

Focused qualification passes 12 workspace cases: all six touch-geometry units,
four real egui touch-target journeys and the existing 1,024-pane selection and
contained-cancellation profiles. Seven public-host tests pass, including all six
touch-target contracts and the balanced 16/128/1,024-pane owned-output fixture.
Correctness/suspicious Clippy and formatting remain passing gates.
A disposable-snapshot negative control removes only the intersection guard:
the counter regression then observes 1,026 rebuilds instead of zero and fails.
Restoring production source passes the same case.

On this machine's debug build, the public structural fixture took 4.096s before
the guard and 3.319s after it; the 1,024-pane resize-selection profile changed
from 4.872s to 4.222s, and the contained-cancellation profile from 8.332s to 7.345s.
These are single-run observations, not a latency guarantee or a controlled
allocator profile. The deterministic counter is the regression authority.
Resize-by-protected-region traversal and existing record lookup costs remain;
this change does not claim linear total hit-manifest compilation or introduce a
spatial tree.

## 2026-10-28 U3d output-bound touch resize targets

Explicit local Touch now uses device-specific resize footprints compiled into
the same exact-output hit manifest as the base controls. Splitter handles expand
around their structural divider, junctions around their structural intersection,
and contained directions use disjoint eight-way partitions around the original
mouse bands. `touch_resize_hit_extent` defaults to 44 logical points, accepts
finite values of at least 44, and never reduces a larger configured base target.
Mouse, Pen, Unknown and the original `host_pointer_hit` query keep base geometry.

The requested extent precedes clipping. Splitters/junctions clip to root and
surface bounds; contained targets may extend outside the painted frame and then
clip to the surface. Visible title/tab/close/menu chrome, complete disabled
strips and real front-window occlusions are removed from touch fragments. Hidden
background controls cannot cut holes in a front window. Pane content does not
automatically exclude an expanded target, but framework widgets retain their
ordinary delivery-clearance proof. Junctions retain precedence over ordinary
handles. Only the final highest stack can be ambiguous; lower-stack conflicts
cannot hide a later unique higher receiver. Highest-stack ambiguity rejects.

`host_pointer_hit_for_device` queries this manifest using explicit device facts.
Positive receiver coverage, semantic winner and requested-lane absence validation
use the same profile. Gesture preparation rechecks the same exact Drag winner
and preserves source, policy, coordinate, durable-rectangle, roster and occlusion
checks. The obsolete scene-only splitter resolver, its error family and two
single-caller prepare wrappers are removed. Armed touch starts from its frozen
real press point; there is no snap to the painted edge or first-motion jump.
Desktop input remains on the existing base geometry until native device and
stationary capture receive their separate qualification.

Independent review found and closed two P2s:

- A known-empty Drag receipt could conceal a real expanded Touch receiver.
  Validated receipts now privately retain their trusted candidate's requested
  delivery lanes. Only actually requested Click/Drag `NoReceiver` facts receive
  the corresponding device-aware absence check. Unknown, Blocked, unrequested
  lanes and the existing phaseful Scroll rules retain their meanings. The public
  failure/retry regression proves rollback and once-only arming at the original
  provider sequence. Runtime pointer submission returns the original reduction
  cause, preserving its error chain and `HostProtocol` classification while the
  failed frame remains poisoned.
- A surface-clipped empty base resize band lost its semantic identity before the
  compiler could create its still-visible Touch fragment. Manifest construction
  now temporarily retains resize identities and removes them only when both
  profiles are empty. The public West-edge regression starts at `(2,300)` for a
  frame at `(-20,160,320,240)`, with a zero-width base hit. A +30 contact movement
  previews `(10,160,290,240)` and commits only after exact Presented settlement.
  Mouse/Pen/Unknown remain unable to hit that empty base.

Receiver descriptors describe the base rectangle, which may be empty; they do
not expose a misleading union box for clipped touch fragments. Drawing bounds,
durable bounds and drop occlusions remain unchanged. Large finite extents use
the original unclipped center rather than subtracting expanded endpoints; unit
coverage includes `1e20` and `f64::MAX`, preserving disjoint small-window and
partially off-surface partitions.

Public journeys cover real splitter/junction and contained edge/corner gestures
from outside the original hit, explicit device distinctions, unchanged
pre-delivery state, Presented/Dropped settlement, terminal deduplication,
protected controls, front occlusion, surface clipping and stale-output rejection.
Real owned-run egui journeys cover outside-base splitter and contained resizing,
ordinary pointer misses, external widgets, disabled UI and an actual contained
close tap. Split weights retain the shared f32 projection tolerance of `1e-4`;
contained rectangles are checked at the exact contact delta.

Fresh serialized qualification on the frozen source:

| Gate | Result | Scope |
| --- | --- | --- |
| Workspace nextest, all features and targets | 1,735 passed | Includes five geometry regressions, four configuration cases and four real egui journeys |
| Public-host consumer, all features and targets | 79 passed | Six new touch-target cases plus all previous host contracts |
| Complete product harness, all features and targets | 114 passed | Ordinary, contained, blocker, overflow, focus, restore and Burst matrix |
| No-default workspace touch/continuation filter, core debug assertions disabled | 38 passed | Device/time, geometry, configuration, contact and existing continuation behavior |
| No-default public touch-target consumer, core debug assertions disabled | 6 passed | All new public device/geometry/proof journeys |
| Local-fork native compatibility, all targets | 260 passed | Existing native lifecycle and adapter compatibility |
| Correctness/suspicious Clippy | Passed | Workspace, all features and targets |
| Strict docs, all features and no defaults | Passed | Warnings denied |
| Doctests | 10 passed | Workspace, all features |

A disposable-snapshot negative control removes requested-lane absence checking
and early empty-base identity retention. Both corresponding public regressions
fail: the false absence is accepted, and the clipped West query returns PaneBody
instead of ContainedResize. Restoring production source passes both. The main
working tree is unchanged by these mutations. Independent final static review
confirms both P2s closed and no other confirmed P1/P2 in this slice.
Workspace/public formatting and diff checks pass. Native compatibility uses the
retained authorized native tree and local egui fork
`7c7b3233541ec724bee561454ba8e9fca384f32e` through a refreshed disposable snapshot;
tracked native pins and the separate native/CI/smoke changes remain outside this
commit. No fork publication is implied.

This is local input and headless renderer-contract qualification. Native
device/time/stationary capture, physical touch hardware, optional native
overload/lifecycle/load/soak/platform gates and the native showcase remain open.
This slice does not complete the overall docking/multiview plan.

## 2026-10-28 U10 local stationary tab-strip edge scrolling

An active local drag now shares one finite continuation between hover selection
and tab-strip edge scrolling. Edge scrolling takes precedence while a fresh
current-output hover receipt places the pointer in an overflowing content edge.
The default band is 24 logical points, interval 50ms, and step one existing
40-point scroll line. A zero band or `None` interval disables it; a zero interval
is rejected. Each band is capped at half the remaining content width, with the
midpoint choosing Forward. A narrow content viewport may have no neutral center.

The exact Scroll winner must be that strip. Click/Drag winners also exclude
close buttons, enabled and disabled strip controls, foreign blockers and
ambiguous receivers. Overlay controls are removed using their exact measured
leading/trailing placement, rather than inferred from their screen position.
After one step, a new token waits for the exact newly compiled output to be
accepted as Presented. A delayed wake performs one step without replaying missed
intervals; maximum offset stops scheduling. Selection, workspace version and pane
focus remain unchanged. Native stationary routing is not inferred from this
local capability.

Manual scroll input and strip/menu commands revoke timed drag work. A phaseful
manual owner retains its strip until End/Cancel; fresh motion can reacquire an
edge afterward. The shared offset commit reuses current valid measurements to
compile new geometry while retaining the active source gesture and last logical
position. It clears stale preview/affordance state and refreshes every affected
surface, including a different surface whose preview was removed. New target
geometry still requires the ordinary exact presentation barrier.

`pointer_continuation_awaits_presentation(surface)` supplies scheduling
information for deferred renderers. The official adapter includes it in terminal
receipt wake scheduling, so a sleeping host accepts the result before asking for
the next deadline. It does not request continuous repainting while the renderer
has not reported a result. A real egui regression first exhausts repaint demand
to `Duration::MAX`, reports the exact output and verifies that the renderer result
wakes the host; the test failed before this fix.

Fresh public-host coverage verifies stationary repeated steps with independent
source/hover coordinates, early deadlines, dropped-frame retry with the same
token, independent surface settlement, stale receiver rejection, blocked/left
edges, terminal-before-continuation order and final quiescence. Manual-owner
tests cover Begin/Update/End, explicit wheel input, revoked-token rejection and
cross-surface preview removal with exact output identity. A leading/trailing
matrix allocates three overlay controls whose group crosses the raw viewport
midpoint while leaving usable content, proving that placement retains scrolling.
The test accepts unclaimed strip space, which is a valid current-output edge
location even when a partially clipped tab has no operable body.

Real owned-run egui journeys cover repeated automatic steps, delayed renderer
waking, explicit scroll takeover, hidden-tab menu selection and selected-tab
reveal. The close-control case first proves that the visible close lies in the
forward edge band with remaining overflow. In a disposable snapshot, removing
Click/Drag control clearance makes that test fail with offset 40 instead of 0;
restoring production passes. Production source is unchanged by the mutation.

Fresh serialized qualification on this source:

| Gate | Result | Scope |
| --- | --- | --- |
| Workspace nextest, all features and targets | 1,722 passed | Core, conformance and official egui, including four real edge-scroll journeys and timing/style configuration |
| Public-host consumer, all features and targets | 73 passed | Five new stationary, manual-owner and overlay-placement tests |
| Complete product harness, all features and targets | 114 passed | Existing ordinary, contained, overflow, focus, restore and Burst matrix |
| No-default workspace edge/continuation filter, core debug assertions disabled | 17 passed | Timing configuration, stream/token behavior and real product continuation paths |
| No-default public edge-scroll consumer, core debug assertions disabled | 5 passed | All new public stationary, manual-owner and placement journeys |
| Local-fork native compatibility, all targets | 260 passed | Existing native lifecycle and adapter compatibility |
| Correctness/suspicious Clippy | Passed | Workspace, all features and targets |
| Strict docs, all features and no defaults | Passed | Warnings denied |
| Doctests | 10 passed | Workspace, all features |

Independent final review found and closed two P2s: cross-surface preview refresh
after explicit scroll, and same-side overlay groups crossing the midpoint. It
reports no remaining confirmed P1/P2 in the timed owner, exact-output boundary,
source continuation, shared recompile, control clearance or wake path.
Formatting and diff checks pass after import/module ordering normalization.

Native compatibility uses the retained authorized native working tree and local
egui fork `7c7b3233541ec724bee561454ba8e9fca384f32e` through a refreshed disposable
source snapshot. Tracked native pins, native/CI/smoke changes and existing review
notes are outside this commit. No push, physical touch, WM-close, mixed-DPI,
native stationary input, overload/soak or whole-plan completion is claimed.
Expanded touch targets and the optional native editor showcase remain open.

## 2026-10-28 U3d explicit local-touch arbitration

Local touch now shares the existing Pressed owner and finite continuation token.
Explicit Touch on manipulation chrome waits without Select, Raise, pane focus or
geometry mutation. The configurable defaults are 350ms long press and 8 logical
points of slop. A fresh current-output continuation arms the contact; motion
beyond slop before arming yields to content, early contact end activates once,
and stationary armed end emits a renderer-neutral context-menu request. Untimed
Touch can tap or yield but cannot earn timed arming. Missing coordinates cannot
activate a frozen source. Ordinary close and strip/menu controls keep tap
semantics and apply slop on both motion and release, without a docking deadline.

Armed motion uses the existing tab, splitter and contained owners. It prepares
manipulation without eagerly selecting the source or raising its contained root,
which preserves the same-surface target proof for a first Move+ContactEnded
batch. Required source Raise is still authorized without executing it. Successful
item delivery stages Select and a same-root contained-source Raise against the
post-move candidate; contained resize stages its Raise after the rectangle
command. The original command and activation publish together once, under the
same policy and recovery guards, or the entire candidate is rejected. Item
content focus uses the existing post-delivery focus contract. Cross-root docking
does not add an unrelated destination Raise policy to the qualified primary
operation. No hover stale
proof is broadened, and pending/dropped outputs cannot activate the source.

The official owned-run adapter keeps an explicit contact roster with monotonically
allocated provider pointer IDs. Generic pointer events remain Unknown. Potential
synthesized generic duplicates are suppressed only from docking ingress; they
still reach ordinary egui content. Entering suppression retires an already
accepted generic stream once with cancellation at its actual raw boundary, so a
held generic terminal cannot vanish behind Touch. Adapter identities/positions
publish after the accepted raw boundary, before the separate due-continuation
boundary. A later failure cannot rewind that accepted roster. End/Cancel retire
only an enrolled contact; reusing an OS contact ID allocates a fresh pointer.

The semantic menu request carries opaque equality identity, final report version,
pointer, surface, root, optional item and logical point. The official adapter
checks version and surface after measurement, consumes it for the current
viewport/paint pass, and opens the existing root presentation menu using current
prepared commands. It does not create another docking policy or select the tab.
The product test actually clicks Float from that menu and verifies the contained
result. A separate real ScrollArea journey verifies an 80-point content drag
changes its scroll offset while core layout and gesture state remain unchanged.
Its pane explicitly registers drag scrolling and uses a nonshrinking viewport;
this is headless egui widget evidence, not physical touch-device certification.

Independent review and new public/product regressions exposed and repaired:

- Release-only movement inside a large close region bypassed slop and requested
  a close. The regression fails before the terminal slop check and passes after it,
  while a fresh short tap still requests one close.
- Eager Select/Raise on first armed motion invalidated the same-batch release
  target. The inactive-source product Burst failed before manipulation-only entry
  and now reaches exact preview delivery. Public-host tests also cover rear title
  sources without changing durable selection/order before settlement.
- Suppression could lose pointer one's generic terminal. The mixed-source test
  covers the generic release both before and after Touch Start, exact cancellation,
  subsequent touch activation and a fresh generic press.
- Armed complete-root touch needed the same source suppression as the drag it
  enters. Without it, first contained motion claimed its own tab gap and failed
  receiver geometry validation. The shared complete-root classifier now resolves
  the same winner on both sides of that transition.
- Deferring activation also required successful same-group reorder to select/focus
  the item and rear resize to raise. Public tests exercise Presented/Dropped
  results, unchanged pre-delivery state and once-only terminal activation; the
  reorder matrix includes main and rear-contained sources.

The new transform regression preserves existing terminal contracts: a splitter
uses its presented source handle and frozen constraints to commit at release;
contained move/resize and docking wait for the exact output barrier. Temporary
drag decoration is not mistaken for a durable pane subtree. A disposable-snapshot
negative control removes the source-root restriction from deferred Raise. The
cross-root policy regression then fails at exact-output settlement with
`ContainedTransformDisabled`; restoring current production source passes the
same case. The production worktree is unchanged by that mutation.

Fresh serialized qualification passes on the final source:

| Gate | Result | Scope |
| --- | --- | --- |
| Workspace nextest, all features and targets | 1,714 passed | Core, conformance and official egui, including seven new product touch/mixed-source cases |
| Public-host consumer, all features and targets | 68 passed | Eight new local-touch cases; source activation covers main/rear, Presented/Dropped and unrelated destination policy |
| Complete product harness, all features and targets | 114 passed | Existing ordinary, contained, blocker, overflow, focus and Burst matrix |
| No-default workspace touch/continuation filter, core debug assertions disabled | 25 passed | Timing configuration, token/stream and real product input behavior |
| No-default public touch consumer, core debug assertions disabled | 8 passed | All new public touch and delivery-activation journeys |
| Local-fork native compatibility, all targets | 260 passed | Existing native lifecycle and adapter compatibility |
| Correctness/suspicious Clippy | Passed | Workspace, all features and targets |
| Strict docs, all features and no defaults | Passed | Core, conformance and official adapter with warnings denied |
| Doctests | 10 passed | Workspace, all features |

The focused public regression files are
`integration/dockspace-host-harness/tests/touch_interaction.rs` and
`touch_delivery_activation.rs`; product journeys are in
`crates/egui_dockspace/src/product_touch_tests.rs` and
`product_pointer/tests.rs`. Independent final review confirms no remaining
confirmed P1/P2 in the touch owner, deferred activation, source suppression and
publication boundaries. Formatting and diff checks pass.

Native compatibility uses the authorized native working tree and local egui fork
`7c7b3233541ec724bee561454ba8e9fca384f32e`, via a refreshed disposable source
snapshot and local patch configuration. Tracked native dependency pins remain
unchanged and do not yet reproduce those newer seams. The native working tree,
CI and smoke changes stay outside this touch commit. No fork publication or
physical WM-close, mixed-DPI or touch-device certification is claimed.

Expanded at-least-44-point touch targets clipped away from title/tab controls,
native device/time/stationary capture and active-drag edge scrolling remain
open. This slice does not close all of U3d, U10 or the overall plan.

## 2026-10-28 U3d finite stationary continuation

The core now exposes one opaque, finite `DockspacePointerContinuation` from the
existing active drag owner. It contains only the provider pointer, deadline,
logical hover surface, stationary point and a core-minted token. Reading it does
not mutate the workspace. A host schedules the deadline and returns the token
through the normal ordered pointer batch with explicit event time, current
capture and fresh receiver facts; the core rejects a different stream, stale
owner, early time, stale presentation or changed hover target. The continuation
edge is distinct from `Moved`, so a timer never fabricates motion.

The initial timed behavior is hover tab selection. The core keeps the dwell start
when the same semantic scene is repainted with a newer emission, while requiring
the fresh receiver authority to remain current. It re-resolves framework
clearance and the semantic click winner at the stationary point, selects only a
different tab, does not request pane focus, recaptures the drag source facts and
keeps the original drag active. Workspace/policy changes, a changed output stream,
coordinate authority or owner, and loss of capture invalidate the action. The existing
presentation configuration defaults to 150ms and can disable or customize it.

The official egui adapter schedules repaint at the core deadline and submits an
explicit continuation edge after the original raw batch time reaches that
deadline. It never submits a synthetic move. A public-host regression covers
cross-surface source/hover routing, 159ms versus 160ms behavior, dropped-frame
retry, duplicate/foreign pointer rejection, fresh output repaint, no added pane
focus, source-fingerprint recapture and final release, with selected and inactive
source tabs. Blocked/Unknown hover invalidates the deadline; later fresh clearance
cannot revive its token. Foreign capture ends the gesture. The official egui
regression covers repeated same-scene repaints, Unknown-time non-execution,
deadline selection and final docking. Two ledger-only tests prove continuations
cannot create/reuse another stream or advance a rejected suffix's clock/sequence,
and that stationary edges preserve buttons until an actual terminal edge.

Independent review found a P1 in the wake getter: requiring the old source
presentation to remain current hid valid work after an inactive-source press
Select. It now reuses the existing exact source continuation proof, retaining
provider/target currentness separately. Review confirms the fix without a new
confirmed P1/P2. Removing that branch in a scratch mutation fails the inactive
source regression; restoring it passes again.

A further terminal-order regression found that a due Moved selecting before a
same-batch release invalidated the release's target scene. Motion now only
updates the finite challenge; only Continuation executes timed selection. The
official adapter completes the accepted raw batch first, then submits a surviving
due continuation using that same current point/receiver basis in the next ordered
pointer boundary. It collects both reports. A released gesture has no continuation
to execute, and moving pointers do not need to go idle. Public-host and real egui
cases cover the previously failing due Move+Release batch, alongside idle and
moving-pointer deadline selection. No stale-hover proof was relaxed.

Fresh gates pass 1,705 workspace all-feature/all-target tests, 60 public-host
cases, 114 complete product cases, and 260 local-fork native compatibility cases.
Four targeted configuration/ledger/egui cases also pass without default features
or core debug assertions. Correctness/suspicious Clippy, strict core/adapter docs
with and without default features, and ten doctests pass. Independent review
confirms the terminal-order fix and second-boundary receipt/feedback/retry
handling without a new confirmed finding. The native count is unchanged and includes the new pointer
edge translation arm. A direct native gate without the local override fails
against the tracked earlier fork pin, which predates the required new seams;
the patched fork workspace passes. Formatting and diff checks pass. This slice does not claim
350ms touch long press, explicit native stationary input capture, active-drag
edge scrolling, or full U3d/U10 completion; those remain open.

## 2026-10-28 U3d official-egui batch observation time

The owned `run_ui` boundary captures explicit `RawInput.time` before egui runs,
and the existing pointer packet keeps that value across reruns. A representable
value reaches every ordered HostPointerInput through PointerEventMetadata;
missing or invalid time remains Unknown. Generic pointer events retain Unknown
device class because egui may synthesize them from touch. This adapter reports
one batch observation time, not individual OS timestamps. No predicted frame
time, timer-produced motion or new input owner is introduced.

One regression changes egui InputState.time inside a discarded pass and verifies
that both reruns keep the original raw Some/None and packet identity. A second
runs ordinary product input at 10.0, Unknown and 10.1 seconds, then submits a
10.05-second probe through real core input and receives HostProtocol. The probe
bypasses egui only because egui itself rejects backwards frame time before the
application callback; it verifies that the product path actually advanced the
core clock. Equal event times and device/time rollback remain covered by the
existing ledger/public-host metadata cases.

Fresh workspace all-feature/all-target gates pass 1,701 cases; the complete
all-feature product harness passes 114, and both clock cases also pass without
default features. Correctness/suspicious Clippy and strict adapter docs with and
without default features pass. The adapter doctest and 260 local-fork native
compatibility cases also pass. Independent review found no confirmed defect in
raw capture, Unknown/device handling, rerun/rejection/drop or lane-change clock
origin. Some invalid-time/lane-change combinations were checked statically;
the two new tests do not claim exhaustive coverage of those combinations.

This completes explicit batch-time capture for the official adapter only.
Native device/time capture, explicit touch-source routing, finite gesture-bound
continuation, actual hover selection, touch long press and stationary edge
scroll remain open. U3d and the overall refactor are not complete.

## 2026-10-28 U8 held-close ingress ownership

HostRecords now owns the single NativeCloseControl instance. Callback ingress
captures the original request before returning HoldClose. Duplicate canonical
requests retain that owner without appending another raw record. Requested is
marked recorded only after the core recorder accepts it; synchronous rejection
and dropped core frames preserve the same ingress owner and retry boundary.
The coordinator, close driver, effects, shutdown, binding references, quiescence
and read-only close progress all use this owner through short mailbox borrows.
The parallel close callback slot in window resources and the duplicate lifecycle
progress counters are deleted.

Cancellation reserves its once-only receipt in that same close owner before
Veto dispatch. Only the exact first callback enters the existing journal.
Received and Transferred are not Committed: a complete successful native frame
commits the receipt, and successful semantic Clear settlement is also required
before release. Late unreserved callbacks cannot recreate a close. Canonical
captures cannot settle Clear without their reserved receipt. The tokenless unit
model shortcut is test-only; production does not use a post-Requested capture
wrapper.

A newer close during cancellation uses one fixed successor slot on the existing
owner. It retains the original immutable event and non-Clone journal reservation,
including its capture ordinal and timestamp. It cannot bind the old core close,
occupy the raw FIFO head or delay the predecessor's cancellation callback. Only
full Clear settlement promotes it and returns the original reservation to the
same journal, ordered against remaining records without refreshing its age.
Deferred reservations remain included in retained-count/oldest-age accounting;
this groundwork does not claim U7 capacity or overload qualification.

Two new adapter-model regressions cover pre-reducer ownership, duplicate capture,
Requested frame drop/retry, wrong-window successor rejection, one successor,
unrelated raw progress, partial/dropped Clear intervals, duplicate report
settlement and exactly one new core request after promotion. A journal regression
checks original ordinal, original age and ordering across reservation handoff.
These models deliberately use no fabricated opaque fork close token. Independent
production-path review found no confirmed blocker in canonical routing, recorder
failure, transfer/commit separation, successor promotion, lock order or quiescence.

Fresh local-fork gates pass 260 all-target cases normally and 260 with native
debug assertions disabled. Correctness/suspicious Clippy, strict native rustdoc,
smoke build, formatting and diff checks pass. Two scratch negative controls remove
pre-Hold capture and renew a deferred reservation respectively; both fail the
corresponding regression, and restoring production code passes both cases.
Three rebuilt macOS Glow redock runs complete in 1.463, 0.617 and 0.617 seconds.

This qualifies the local ownership implementation and its model boundaries.
It does not certify real WM-close input, mixed DPI, physical docking input or the
canonical adapter Hold/Veto journey. macOS Accessibility trust is unavailable in
this environment, so the real close-button journey was not executed. Native
sources remain dirty against the unpublished local egui fork; tracked dependency
pins have not changed. Protected overload admission, load/latency/soak and the
remaining U7/U8 exit criteria remain open. The next core priority is U3d timed
continuation and U10 stationary active-drag overflow through the same journal.

## 2026-10-28 U8 physical-window Destroyed reservations

An attached physical window now owns one non-Clone terminal receipt in its
current route. A displaced/unbound window or accepted release moves that same
receipt into the existing retirement entry. The receipt retains viewport,
WindowId, binding and actual create-attempt identity; no synthetic lifetime
authority, parallel old-window registry or second journal is introduced.
Create-success capture attaches the observed real window before the first output
is reduced, so pre-admission retirement can still retain its physical obligation.

Destroyed ingress first claims the exact current or retired receipt, suppresses
only its matching route and freezes the old binding into the existing journal.
Duplicates cannot append another record. Receipt Received, Transferred and
Committed states remain distinct. Snapshot tombstones, route retirement and
quiescence require the complete callback boundary. A later physical window on
the same viewport has an independent binding/WindowId; old route retirement
compares that binding before removal and preserves the successor's sidecars.

Independent review found three gaps in the initial version. Admission checked
only the live route map, permitting a binding or WindowId already retained by
retirement to be reused; bind, attach, replacement and create-success now
preflight against the existing retirement owner. `retain_window` returns its
original receipt on conflict rather than overwriting another owner. Same-window
rebind after Destroyed capture could orphan the old pointer-reset tail or copy
old tombstone progress to a successor. Rebind now requires the route's receipt
to remain Awaiting; once capture moves it into retirement, the operation rejects
before mutation. Queued Destroyed records are no longer rewritten. An Awaiting
same-physical-window rebind retains the original receipt.

Public adapter-model regressions cover displaced W1 and successor W2, duplicate
capture/transfer, dropped retirement preparation, partial/full commit, old late
callbacks after quiescence, binding/WindowId reuse rejection and Awaiting versus
Received rebind. The scroll regression still records provider reset before an
independent later close-correlation failure using a real matching Destroyed
owner; an unrelated window is rejected before journal admission instead of
being treated as that owner. The pre-admission compensating-close fixture now
reports its observed physical window explicitly before applying core facts.

Native all-target gates pass 257 cases normally and 257 without native debug
assertions. Correctness/suspicious Clippy, strict native rustdoc and the smoke
build pass. Follow-up independent production review confirms all three findings
closed without a new confirmed defect. Removing retirement admission protection
in a scratch mutation makes the binding/WindowId reuse regression fail; restoring
it passes the same case again. Three rebuilt macOS Glow redock runs
complete in 1.637, 0.564 and 0.512 seconds. These are model and programmatic
window-lifecycle results, not physical-input, mixed-DPI or WM-close certification.

The close-owner body now has fixed current/successor capture and cancellation
receipt primitives with four tokenless model tests. They are compiled in this
gate but not yet integrated at callback ingress, and do not prove canonical fork
Hold/Veto behavior. Held-close ingress integration and protected overload
admission, capacity/load/latency/soak remain open. Native source stays dirty
against the unpublished local fork; tracked pins are unchanged.

## 2026-10-28 U3d ordered device and event-time facts

`PointerEventMetadata` retains an explicit device class and optional monotonic
timestamp on the existing pointer edge. The host-local, surface-local and native
facades forward the same metadata without deriving receiver, capture or motion
facts. Unknown samples remain unknown; `untimed(device)` preserves an observed
class when time is unavailable. No gesture timer or new input owner is added.

The existing ledger stores time in its active provider record and known device
class in each active stream record. Time is nondecreasing across all pointer IDs
within that provider incarnation, including equal-time batches. A known device
class cannot change in a live stream. Unknown samples neither reset the clock
nor reclassify a known stream, and historical observations are not copied into
the sample. End/cancel permits a fresh stream for a reused pointer ID. Overload
retains provider time; creating a new provider permits a new clock origin.

All validation and frontiers advance in the speculative journal plan. A bad
suffix leaves time, stream allocation, buttons, capture and sequence unchanged.
Four ledger cases cover cross-pointer ordering, unknown samples, device reuse,
overload/new-provider clocks and untimed device observations. Two downstream
public-host cases prove dropped-frame and bad-suffix retry using only public
input; a private native case verifies metadata in the immutable recorder payload
before raw-to-core replay, frame drop, exact retry and contact reuse.

The complete workspace passes 1,699 all-feature/all-target cases. The independent
public host passes 59 and the product harness passes 114. Five metadata cases
also pass without serde or core debug assertions. Correctness/suspicious Clippy
and strict core/adapter rustdoc pass. Independent read-only review found no
confirmed defect in provider ordering, rollback, unknown facts or device-stream
identity.

This completes the input-fact groundwork only. Existing adapters still omit
metadata until their capture paths are adapted. The finite gesture-bound
continuation, actual hover selection, touch long press and stationary edge
scroll remain U3d/U10 work. Input metadata alone grants no timed interaction
capability and does not complete the refactor.

## 2026-10-28 U8 create-attempt terminal ownership

One admitted deferred-create attempt now owns a non-Clone receipt in its
existing viewport route. Success and failure share its once-only terminal
choice; success uses the exact attempt/window rather than a particular output
token, so later outputs from the same window cannot consume the create effect
again. Failure callback records retain the real fork attempt. Duplicate or
conflicting callbacks cannot grow the journal or reopen a transferred receipt.
No additional registry or callback queue is introduced.

Failure ACK atomically takes the original receipt from the route and moves it
into the existing unmaterialized retirement entry under route-to-records lock
order. The single `NativeRetirementState` instance now resides in HostRecords,
and coordinator/effect paths access it through short bridge methods. Raw
handoff is not publication: partial commits and dropped core frames preserve
the obligation. Only a complete successful frame boundary commits a transferred
create receipt. A failure whose affine result has not entered the recorder
survives unrelated full boundaries and cannot become a quiescence candidate.
The existing first-live/presentation acknowledgements retain their own meaning.

Four new receipt/bridge cases cover terminal conflict, duplicate handoff,
partial/full commit, result retry, quiescence and successor isolation. The
existing real core create-failure fixture additionally drops its result frame
and proves the same obligation survives. Initial compilation found three
fixture failures: duplicate wake expectation, replacement before receipt
handoff, and a cross-surface replacement fixture. They now reflect once-only
admission and legal current-binding handoff without relaxing production binding
checks. Helper cases use a permitted None attempt; they do not pretend to mint
real fork authority.

Refreshed local-fork sources pass 248 all-target native tests normally and 248
without native debug assertions. Correctness/suspicious Clippy and strict native
rustdoc pass. Independent static review found no confirmed P1/P2 in receipt
transfer, commit-hook reachability, lock order or quiescence. Removing the
recorded-result prerequisite in scratch makes the two failure-receipt tests
fail. Three rebuilt macOS Glow redock processes complete in 1.643, 0.565 and
0.515 seconds through the existing lifecycle checks.

This is only the create-owner prerequisite. Physical Destroyed and first
held-close ingress reservations, overload protected admission, capacity/load/
latency/soak and complete U8 readiness remain open. Programmatic macOS redock
does not certify physical input, WM-close, mixed-DPI or create-failure OS
behavior. The native adapter remains dirty against unpublished local fork
`7c7b32335`; tracked pins are unchanged. The main workspace and official product
qualification is recorded in the overflow checkpoint below.

## 2026-10-28 follow-up overflow and timed-input audit

The new public product transition exercises explicit forward scroll while the
selected tab stays unchanged, a clipped old close action, menu selection/reveal,
and backward scroll through both pointer and AccessKit input. It exposed two
production defects. Forward/backward alignment searched only operable tab
records, omitting the clipped members it needed to reveal. The solver now uses
the exact bar's complete member roster. Overlay strip controls also left the
underlying tab's drag lane executable, so a valid pointer press could produce
two different executable owners and roll back the batch. Strip controls now
occlude both click and drag lanes while their semantic action remains click;
the independent receiver-proof validation and conflicting-owner invariant are
unchanged. Both new transition cases pass with the fixes. They do not establish
stationary active-drag edge scrolling or complete R25 arbitration.

The complete workspace remains green at 1,694 all-feature/all-target cases;
the public host passes 57 and the product harness now passes 114. The initial
unfixed test failed to scroll for both AccessKit and pointer activation. After
the roster correction, the immediate pointer-backward step exposed the exact
click-control versus tab-drag invariant; preserving that step now proves the
lane-occlusion correction. These failures were reproduced before the fixes,
without extending the causal settlement budget or inserting ordinary wait
frames to hide either issue.
Both transition cases also pass with serde and core debug assertions disabled.
Correctness/suspicious Clippy, strict core/adapter rustdoc, formatting and
whitespace checks pass.

A separate read-only audit confirms that the planned 150 ms hover selection and
350 ms touch-long-press profile are still missing. Current pointer records have
no gesture time or device class, and native touch translation enters ordinary
distance-threshold primary input. Contact identity, loss and cancellation tests
are lifecycle evidence; geometric target hysteresis is not hover dwell evidence.
Receiver-current capability flags describe authority rather than these features.
The canonical plan now explicitly retains the existing timing requirements as
U3d, sharing a finite gesture-bound continuation with U10 edge scrolling. The
U3a/U3b/U3c replacement and Burst qualification remains valid; complete timed
interaction qualification stays open.

## 2026-10-28 U10 layout-only reset without document lineage

`DockspaceSession::prepare_layout_reset` produces an opaque non-cloneable value
bound to one published session/version. Standalone and managed-native reset
frames borrow it; dropping or failing the candidate preserves it for retry,
while successful publication makes it stale. Arrangement lock and document
item-key restrictions are checked at entry. Native hosts must join their
ordered prefix rather than bypass it through standalone replacement. Both
reset and document restore share the replacement phase, native pipeline and
final semantic/configuration gate. Their native conflict classification remains
available through both `kind()` and `native_kind()`.

Retired surface, root and contained identities are rebased directly from the
validated layout without serialization. Incoming fresh identities are reserved
before allocating replacements, preventing collision with retired names.
Application ItemIds, tab selection, node content and contained rectangles remain
intact. The shared helper also replaces the former snapshot-based document
rebase and remaps imported viewport placements. A document-bound reset may reuse
closed registered items without replacing its document, key history, viewport
placement sidecars or save generation; unknown numeric items are rejected.

The public host proves repeated cloned defaults after actual close/retirement,
mixed retired/fresh identity allocation, stale/foreign/locked preparation,
dropped-frame retry, rejection of old actions/output/focus, native-prefix join
and closed-key reuse. The private native fixture adds a workspace-mutating
selection prefix before reset: preparation checks the published version,
rebasing uses the prefix candidate, drop preserves both source and backend
watermarks, retry is exact, and successful commit consumes the prefix once.
The simpler public Moved-prefix case proves ordered joining, not a workspace
mutation. Removing incoming-identity reservation in scratch fails the collision
test with an invalid workspace rather than silently passing.

The egui facade exposes `Dockspace::reset_layout` with no serde requirement,
and the editor Default button uses its application-owned immutable layout.
Named durable presets retain their document restore path. The product test
closes the whole root, resets to fresh presentation identities, clears previous
render observations, and proves current pane rendering, unsaved text retention,
default selection and lock/unlock behavior. Measurement stages contributions;
commit installs Ready scenes for a later paint frame. Reset does not add a
second immediate-paint pipeline.

Current main-source gates pass 1,694 workspace all-feature/all-target tests,
57 public-host cases and 112 product cases. The no-serde reset suites pass five
public-host cases and one product case. The workspace-mutating native-prefix
case also passes with serde and core debug assertions disabled. Follow-up
independent read-only review found no remaining confirmed reset defect after
the shared native error classification was restored.

The official compatibility harness passes 15 cases. Correctness/suspicious
Clippy, strict core/adapter rustdoc, documentation examples, formatting and
whitespace checks pass. Refreshed local-fork sources pass 244 native all-target
cases normally and 244 without native debug assertions, plus native Clippy and
strict rustdoc. Three rebuilt macOS Glow redock processes complete in 1.596,
0.617 and 0.621 seconds through create, first-live, redock, retirement and
quiescence. These are compatibility and programmatic real-window results,
not physical-input, mixed-DPI, WM-close or OS-driven layout-reset qualification.
The local fork remains unpublished at `7c7b32335`; tracked dependency pins are
unchanged and do not yet reproduce the dirty native adapter.

U10 still needs complete overflow-owner acceptance and the optional native
showcase. U7/U8 physical-window/create/held-close reservations, finite capacity,
overload integration and load/latency/soak remain open. This slice does not
complete the whole refactor or qualify a physical-input platform.

## 2026-10-28 U10 live restore focus and missing-content recovery

Shared ReplaceWorkspace/RestoreWorkspace now creates one semantic focus successor
inside the rollbackable candidate using the replacement input's original causal
stamp. A newer pending intent wins over older observed surface history. A stable
item remains preferred only when it is selected in the restored layout; restoring
focus never changes the saved tab selection. Otherwise the existing target
surface, or first current surface in stable order, selects main central, main
visual-order selection, front contained selection, then workspace. An empty
workspace emits no request. Replacement clears old authority and preserves
monotonic request identities; drop leaves the published old intent unchanged,
retry produces the exact new successor, and old-epoch observations are inert.
Capture distinguishes NoHistory, Dock and OutsideDock. Explicit Foreign/None
focus does not invoke the primary-pane fallback, while a newer semantic pending
intent still takes precedence.

The same intent carries Local, Waiting or Bound native-target state. A native
destination starts Waiting and cannot be published or acknowledged as a local
pane request. Only a current eligible binding resumes it with the same ID and
causal stamp. Other surfaces' native registration does not turn a logical target
native. Native-root model tests prove changed epoch/incarnation, no request while
unready, rejection of the old observation and exact fresh-binding resumption;
they do not claim OS focus execution. Waiting is retained across the existing
provider-replacement/focus-stream-fault paths without changing its type. Those
paths can re-enroll a successor; no production terminal provider-stop API is
currently exposed. Logical surface/item retirement still reconciles the intent.

Independent review found three gaps in the first version: a fresh target focus
observation could replace the retained intent, explicit outside focus was folded
into no history, and FocusDelta could expose internal Waiting state. Matching
replacement observations now advance only their baseline, capture uses explicit
three-way state, and both sides of FocusDelta filter Waiting. The native fixture
includes repeated KnownForeign while hidden followed by the exact Ready target;
the original request ID/causal survive. Outside focus with old pane history and
Waiting-to-Bound/Local delta transitions have direct regressions. Restoring all
three earlier behaviors in scratch fails four selected tests. Follow-up read-only
review confirmed the three findings closed without a new confirmed defect.

Four new public document/recording-host cases cover no-history primary fallback, relocated stable targets,
pending versus observed history, each fallback, saved background selection,
dropped restore and exact retry, old observation rejection and NotFocused/Focused
settlement. The fixture resolves remapped current surface identities rather than
assuming IDs from the saved declaration survive retirement. Stubbing successor
creation initially failed all three existing cases; the no-history case was
added during review. Final current-source recompilation includes all four.

A separate public egui product case restores a recognized key whose pane content
is temporarily unavailable. It renders the built-in placeholder, reports the
missing stable ItemId, leaves its location intact and ends an unavailable focus
request without claiming Focused. Restoring the application registry renders
the same item in place; the completed unavailable request does not replay, and
ordinary selection can focus the actual TextEdit. Key validation remains strict:
rejecting an identity rejects the complete document. Content availability does
not silently delete layout ownership or bypass the application close workflow.

Two additional product cases prove locked overflow selection by pointer,
AccessKit and keyboard, inert old close controls, and locked rear contained
tab/content activation with real pane focus and raise. Title/resize attempts,
including old AccessKit resize actions, preserve rectangles, item ownership and
tab order. These exercise the real adapter rather than only policy evaluation.

The final main workspace passes 1,693 all-feature/all-target tests; the independent
public host passes 51 and the product harness passes 111. Five replacement focus,
outside-capture, native-readiness and delta tests also pass without core debug assertions. Correctness/suspicious
Clippy, strict core/adapter rustdoc, formatting and whitespace pass. Refreshed
local-fork native sources pass 244 all-target cases normally and 244 without
native debug assertions. Three rebuilt macOS Glow redock runs complete in
1.601, 0.509 and 0.562 seconds through the existing lifecycle/quiescence checks.
These are recording/model and programmatic real-window evidence, not physical
input, WM-close, mixed-DPI or an OS-driven document-restore focus journey.

U10 still needs a general serde-free reset/default-layout entry, final overflow
owner acceptance and the optional native showcase. U7/U8 physical resource
ownership, finite capacity, overload integration and load/latency/soak remain
open. The local fork is unchanged at unpublished `7c7b32335`; tracked pins remain
unchanged. The whole refactor remains active.

## 2026-10-28 U10 arrangement lock, reopen focus and bounded editor layouts

`DockPolicy::set_arrangement_locked` adds an independent revisioned restriction
without replacing operation filters or item/source/surface rules. Ordinary
membership, docking, reorder, presentation and geometry commands reject while
locked. Select, tab scrolling/navigation, content, focus and permitted contained
raise remain available. Raise has a distinct request with the original
contained-rule checks; it does not authorize a rectangle edit. Irreversible
native recovery retains its existing separate authority, while copied ordinary
commands cannot acquire that exception. Effective close capability includes the
lock, keeping close planning, scene chrome and final close revalidation aligned.

Live document replacement checks the lock both at the public frame entry and in
the authoritative workspace reducer. Initially locked layout construction is
valid. An explicit unlock must commit before applying a layout in a later
frame; dropping the configuration frame preserves the published lock and rules.
Policy revision already invalidates stale actions and close plans, so no new
lock revision or gesture owner is introduced.

Inspection found that the shared tab press path authorized drag eligibility
before selecting. A lock therefore also prevented real pointer tab navigation.
Single-tab policy denial now continues through existing select/raise/focus and
returns before creating a drag session. Group/title drags and non-policy errors
remain rejected; a required raise still obeys its original rules. The public
test covers same-batch press/move/release and a committed press with no active
gesture before later motion/release. Restoring the old gate fails the selected
item assertion. Independent read-only review found no new event, release-routing
or policy bypass defect.

Reopening uses the existing OpenItem command and application-owned catalog.
Newly opened content becomes selected and requests the same pane-focus intent;
Existing preserves topology, selection and pending or settled focus. The public
consumer closes/reopens a stable item, emits and presents the complete surface
program, and settles NotFocused then Focused against the actual request. Missing
anchors reject explicitly. Removing OpenItem from the focus branch fails that
consumer's exact focus-target assertion.

The persistence example is now an eight-pane editor shell with text, scrolling
and a small application-drawn scene. It retains pane data, derives closed
inventory from its own catalog, keeps an immutable default and at most eight
user presets, and offers explicit lock/unlock, reopen, reset and preset controls.
Names and storage stay application-owned. Review caught an initial fallback
which still used the original root/surface after identity retirement. The shell
now resolves the current public view. Its regression closes the complete root,
observes that no logical surface remains, resets Default, verifies both root
and surface were remapped, then closes/reopens and renders Document through the
actual AccessKit output. It never treats absence of the old root as proof of a
rootless current surface.

The main workspace passes 1,688 all-feature/all-target tests. The independent
public host passes 47; the official-egui product harness passes 108 and the
official compatibility harness passes 15. Twelve arrangement-lock tests also
pass without core debug assertions, including authoritative replacement
rollback, ordinary transaction rollback, recovery isolation and legacy policy
serde defaults. Correctness/suspicious Clippy, strict core/adapter rustdoc,
formatting and whitespace checks pass. After the scratch mutations, current
main-source recompilation was observed for the restored green witnesses.

Refreshed local-fork native sources pass 244 all-target tests both normally and
without native debug assertions. Three rebuilt macOS Glow redock processes
complete in 1.571, 0.550 and 0.537 seconds through the existing lifecycle and
quiescence assertions. These are compatibility and programmatic real-window
results, not physical-input, WM-close, mixed-DPI or overload qualification.

U10 is not complete. Restore-time focus/missing-pane recovery, full overflow
owner acceptance and the optional native showcase remain open. No generic
serde-free layout reset API is established by the document-based example.
U7/U8 capacity, physical-window/create/held-close ingress ownership and overload
integration remain unchanged; the local fork is unpublished and pins unchanged.

## 2026-10-28 U8 exact command callbacks and retirement-safe property dispatch

The local eframe fork now provides opaque tokens for host-owned focus and
visibility commands, alongside pointer pass-through. Each typed command owns
its native operation and echoes its token. Ordinary Focus/Visible commands and
builder deltas cannot consume a typed token or produce a typed callback.
The property queue belongs to the native-host attachment and preserves enqueue
order per viewport, including cross-property order. Missing attachments schedule
no operation; viewport/window retirement removes their pending typed commands.

Inspection found two concrete identity defects in the previous adapter: an
ordinary Focus result could claim a newer focus request on the same window,
and a same-value visibility result captured before Show ownership could consume
the later Show request. The adapter now matches token, viewport, window and
binding before consuming either request. Requested focus still requires a
subsequent real focus observation. Show dispatch still requires later window
and first-live presentation evidence. Removing these token checks in a local
scratch mutation fails both new delayed-result regressions.

The window resource also owns fixed callback receipts for pointer pass-through,
focus, visibility and held-close cancellation. A callback moves its existing
receipt through received and transferred states; only a complete committed
causal boundary settles it. Partial commit and raw-to-core handoff cannot permit
a successor to overwrite the receipt. Committed identity remains a deduplication
tombstone without keeping the runtime pending. This is command reservation
groundwork, not yet protected overload admission or complete resource accounting.

Independent review found an additional retirement defect: ordinary companion
property commands used to wake hidden logic could remain in Context or renderer
deferred commands after the typed queue was cleared, then affect a successor
window. The fork now requests repaint without emitting property companions.
Glow and WGPU logic-only dispatch include pending typed targets explicitly,
without forcing hidden UI execution. The regression replays both pre-retirement
FullOutput/LogicOutput commands and residual Context commands against a
successor, preserving application-owned ordinary commands. Restoring companions
fails with the three stale ordinary property operations. FullOutput fixtures
first settle the upstream initial theme/font output and explicitly clear unused
texture deltas; these are command models, not renderer submission evidence.

The final combined Glow/WGPU fork library passes 75 tests normally and 75 with
eframe debug assertions disabled. The refreshed native workspace passes all
244 all-target tests normally and 244 with native debug assertions disabled.
Correctness/suspicious Clippy, strict rustdoc, formatting and diff whitespace
pass. Fork rustdoc enables inspection and persistence to resolve upstream's
feature-specific links; the minimal-feature documentation attempt failed on
those existing links and is not counted as a pass. Independent follow-up review
found both identity defects and the companion retirement defect closed.

Three clean macOS Glow redock processes complete in 2.039, 0.612 and 0.619 seconds
through tear-off, first-live, redock, retirement and runtime quiescence. This
qualifies programmatic native window/renderer lifecycle, not physical pointer
input, WM-close, mixed DPI or overload triggering. The local fork commit is
`7c7b3233541ec724bee561454ba8e9fca384f32e`; it is not pushed. Tracked native and
CI pins remain at the published baseline, so local overrides are still required.

U7/U8 remain incomplete. Exact create-attempt, held-close ingress and physical
window-terminal reservations must survive logical route retirement before
finite protected admission can be integrated. The planned 4,096-record/4 MiB/
250 ms limits, fixed overload boundary, committed translator recovery and
load/latency/soak gates remain open, as do later plan units.

## 2026-10-28 U8 output resource ownership and U7 translator recovery groundwork

The native output token now has one resource owner for its reservation,
dispatch budget, affine surface/staging paint, renderer terminal and submitted
state. The coordinator's separate `pending_outputs` map and the bridge's
separate dispatch-budget map are removed. The callback journal retains only
the output token, so a renderer result is no longer copied into a parallel
envelope with an independent submitted flag. The coordinator still reports
the owned paint to core before marking that exact token submitted; callback
arrival alone does not promote receiver or presentation authority.

The budget starts before root-roster capture. Its affine loan releases the
mailbox lock before reduction, preserves credits across pass/finalizer loans,
and returns to the same output owner. A physical terminal closes the loan slot;
late return cannot reopen it or recreate a deleted owner. A terminal arriving
before paint attachment can still receive its exact paint until submission.
Duplicate, abandoned, mismatched and already submitted attachments fail while
returning the affine output to the caller. Partial commit retains the owner;
complete causal-boundary commit reclaims submitted tokens in journal order.
Quarantine and exact binding retirement keep their prior terminal/drop rules.

Two new owner tests exercise budget reuse/late return and a real core-painted
native surface with late attachment and rejected rebind ownership. The terminal
flag is set directly in these unit models; they do not claim to mint a fork
renderer token. Scratch variants permitting late budget restoration and
submitted paint reattachment fail both assertions. Independent output-owner
review found no confirmed new attachment, commit-cursor, receiver or lock-order
regression. Real-window validation below separately exercises actual tokens.

`NativePointerTranslator::finish_overload_cancellation` is also implemented as
a post-commit hook for the upcoming latch integration. It discards old mouse
buttons/origins/capture, touch enrollment and scroll state without emitting
another input journal or claiming physical release. Pointer and scroll identity
counters remain monotonic. Old phaseful scroll lanes retain a tombstone until
an explicit new Start; their Moved/Ended/Cancelled tails cannot become new
discrete input. Idle Moved-only wheel lanes and ordinary hover resume, while
existing device-removal restart gates remain intact. This hook is not called
by the driver yet: only the future committed overload terminal may invoke it.

The translator stub first failed four of five focused translation regressions;
the implementation passes all five. Independent translator review found no
confirmed defect; helper invocation after an already retired scroll tail and
repeated overload are statically checked paths rather than additional exercised
test cases in this slice. The refreshed local-fork native workspace
passes 238 all-target tests normally and 238 with native debug assertions
disabled. Correctness/suspicious Clippy, strict rustdoc, formatting and diff
whitespace pass. Three clean macOS Glow redock processes finish in 2.078,
0.563, and 0.618 seconds through first-live, retirement and runtime quiescence.
This is programmatic output/window lifecycle evidence, not physical-input,
mixed-DPI, WM-close or overload-trigger qualification.

Command and exact window-terminal reservations remain separate prerequisite
work. Record/owned-byte/oldest-age limits, the fixed accepted overload boundary,
protected once-only terminal admission, invoking translator recovery after
the exact cancellation commit, and load/latency/soak qualification remain open.
The local egui fork is still unpublished and tracked native/CI pins unchanged.

## 2026-10-28 U8 deferred-window resource ownership prerequisite (partial)

The mailbox's separate create-reservation, hidden-render-binding and staging
maps have been replaced by one `NativeWindowResources` record per viewport.
That record has one exact binding and owns the create rectangle, hidden-render
lease and pending staging request together. Foreign reserve, clear and retire
operations leave the current owner intact. Clearing one obligation preserves
the other two; the entry disappears only when its last obligation is gone.
Fatal quarantine still clears staging while retaining create/hidden cleanup.
This record is adapter resource ownership, not another core lifecycle machine.

Staging publication validates the complete binding set before changing any
slot. The host frame validates route/owner compatibility before core commit,
and publication rechecks the batch under the mailbox lock. Empty control
frames retain pending staging; same-binding updates replace their observation
basis, and an exact request already in the raw journal stays in flight. A
claimed staging request transfers to that existing ordered journal without
clearing its create or hidden-render owner.

The former generic map-only staging test has been replaced by exact-owner
coverage and a coordinator regression using a real core hidden-create staging
request. It proves atomic foreign-batch rejection, pending staging across
empty frames, retention after create/hidden completion, and stale cleanup
isolation. A scratch variant that clears staging on an empty publication fails
the explicit retention assertion. Independent review found no confirmed new
owner, staging, lock-order or commit/publication race in the production paths.

The refreshed local-fork native workspace passes 231 all-target tests normally
and 231 with native debug assertions disabled. Correctness/suspicious Clippy,
strict rustdoc, formatting and diff whitespace pass. Three clean macOS Glow
redock processes finish in 1.427, 0.568, and 0.563 seconds through first-live,
retirement and runtime quiescence. This remains programmatic window/renderer
lifecycle evidence, without physical input, WM-close or mixed-DPI claims.

This is the deferred-window portion of the prerequisite, not completed U8 or
U7. Exact output/command/window-terminal reservations, protected once-only
terminal slots, full record/byte/age admission, the overload latch, and committed
translator recovery remain to be integrated. The working native adapter still
needs the unpublished local egui fork; tracked dependency and CI pins have not
been advanced and do not reproduce these source changes remotely.

## 2026-10-28 U7 shared-ingress storage and source-age groundwork

Backend records now share an immutable payload allocation between the recorder,
frozen batches, batch clones and publication retries. Pointer journals also
share their edge vector and complete authority checkpoint when the reducer
clones a payload. Attaching a checkpoint to a shared journal uses copy-on-write
after validation; the original branch remains unchanged. Consuming edges moves
the vector for a unique owner and copies only the edges for a shared owner.
Payload lifetime remains independent of publication authority: rollback revokes
the old record, reused ordinals get new identities, and prefix retirement still
requires the exact committed record identity.

The native mailbox now records one private capture identity and timestamp for
each accepted raw record, using the same injected clock source as its dispatch
budget. Raw-to-core transfer drops the raw payload while retaining a source
reservation count and the original oldest capture time through partial commit,
yield and dropped-frame retry. Only completion of the corresponding committed
boundary releases that reservation. Idle coalescing removes obsolete cursor
records in place without re-numbering or re-timing the surviving observation.
These are source reservations, not a claim of complete resident-byte accounting.

Six core regressions check allocation sharing, copy-on-write isolation,
rollback/ordinal reuse, and last-owner release after committed-prefix retirement.
Scratch variants which restore payload or journal deep clones fail the relevant
owner assertions. Four additional native tests cover transferred count/age,
explicit handoff through partial commit, and dropped cursor claims/coalescing.
Removing transfer reservations in a scratch variant reproduces two failures.
Independent read-only review found no confirmed regression in either unit.

The core storage/pointer neighborhood passes all 122 selected tests; the main
workspace passes 1,674 all-feature/all-target cases. All six new storage tests
also pass with core debug assertions disabled; the independent public host
harness passes all 40 cases. The refreshed local-fork native
workspace passes 230 all-target tests both normally and with native debug
assertions disabled. Both workspaces pass correctness/suspicious Clippy and
strict public rustdoc; Cargo formatting and diff whitespace pass. Three clean
macOS Glow redock processes finish in 1.481, 0.624, and 0.619 seconds through
first-live, retirement and runtime quiescence. They qualify programmatic
renderer/window lifecycle compatibility, not physical dragging, mixed DPI,
WM-close behavior, or overload admission.

A shared target reused a deliberately mutated scratch artifact once without
compilation; that run is excluded. Current-source recompilation was observed
for the passing core storage tests and refreshed native integration. Scratch
refresh now updates source timestamps instead of preserving older copied
timestamps before switching the shared target between source locations.

This is groundwork for U7 admission, not completed 4,096-record/4 MiB/250 ms
enforcement. Owner slots, the fixed overload boundary, cancellation scheduling,
and byte accounting remain open. The plan now performs U8's resource-ownership
consolidation before that integration so protected terminals use existing exact
owners instead of adding temporary reservation maps or a second unbounded queue.
Ordinary pointer callbacks rejected after freeze are not accepted edges; they
continue through the platform. The planned adapter integration must retire
the surviving translator state only after cancellation commits, without
synthesizing releases or an empty roster.
U8's final readiness and platform verification still require completed U7.

## 2026-10-28 U7 overload kernel checkpoint (adapter admission pending)

`DockspaceHostFrame::cancel_native_input_for_overload` provides a terminal
managed-host input operation outside ordinary backend/pointer record capacity.
It requires the exact joined provider and presentation host, with all accepted
native predecessors reduced. The ordinary recorder rejects this control input.
Measurement and paint remain available afterward, while later semantic,
configuration and pointer inputs are rejected. Its submitted-action handle
correlates the exact committed `NativeInputCancelledForOverload` outcome.

The kernel ends live gestures, routing and all active scrolling, revokes button
and capture facts to Unknown, and invalidates prepared pointer candidates.
Provider identity, pointer watermark and monotonic stream/scroll identities
remain intact. Pending drag/contained releases retain their own previews,
outputs and final presentation settlement; native create obligations continue.
Cancelling an unconsumed contained gesture restores its current durable source
projection without rolling back workspace changes or advancing its version.
Frame drop and failed publication leave cancellation unpublished and retryable.

Independent review found one recovery defect: clearing authority also closed
every checkpoint boundary, so ordinary focus restoration would remain gated by
Unknown after later button releases. A restricted overload-resync boundary now
follows accepted pointer edges until a complete roster commits. The new
`DockspaceSession::report_native_pointer_roster` validates the current exact
provider, native captures and committed pointer watermark, then records a
normal checkpoint without manufacturing a pointer edge. Unknown preserves the
opportunity for later complete facts at the same watermark. A held button stays
KnownDown; only actual complete observations and release can prove all released.
The initial enrollment rules and ordinary post-enrollment checkpoint rejection
remain in force outside overload recovery.

Both the cleanup stub and the recovery stub had observed behavioral failures
before their implementations. Five recovery regressions include late
press/release, Unknown followed by same-watermark Exact, rollback/retry, and
stale/foreign rejection. The complete overload filter passes 20 tests,
including a public consumer which exhausts all 256 ordinary record credits
before issuing the terminal operation. Main-workspace all-feature/all-target
tests pass 1,668 cases; the public no-default overload target passes once;
the independent public host harness passes all 40 cases; 19 overload library
tests also pass with debug assertions disabled.
Correctness/suspicious Clippy, strict rustdoc, formatting and diff whitespace
pass. Independent follow-up review found the recovery defect closed.

After refreshing the local-fork native workspace to core commit `e3cbf32`, its
226 all-target tests pass both normally and with native debug assertions
disabled. Three clean macOS Glow redock runs finish in 1.692, 0.567, and 0.558
seconds through first-live, retirement and runtime quiescence. These establish
lifecycle compatibility with the new core; the native overload latch is not yet
wired and is not exercised by that programmatic redock scenario.

This checkpoint implements the renderer-neutral cancellation/recovery kernel.
It does not implement the adapter's 4,096-record, 4 MiB, 250 ms admission limits,
its reserved overload latch, or the corresponding end-to-end overload/latency
qualification. Those remain U7 work rather than guarantees inferred from a
successful kernel cancellation.

## 2026-10-28 U7 native paint continuity checkpoint (not remotely pinned)

Real macOS Glow windows exposed two adapter defects which deterministic core
replay alone did not detect. First, a completed preview deferred measurement
but was also prevented from confirming its paint. Redock then waited forever
for the exact target presentation. A ready, complete paint without actions now
confirms its renderer output even when feeding new geometry back into
measurement is deferred. Incomplete transient paint still fails closed.

Second, repeated runs could stall after post-show transfer and before the
child's first-live presentation. A timeout-only diagnostic captured a Ready
child scene, an empty callback journal, no pending output or effect, and no
remaining blocked/ready child wake. Semantic unblocking used a coalescible
one-shot repaint, which could disappear into an already queued placeholder or
retained pass. It now arms the same retained repaint obligation as staging.
The regression drains egui's initial viewport stabilization first: without
that step, the default extra repaint could make the assertion pass for the
wrong reason. The corrected semantic test failed before the fix; both staging
and semantic queued-child tests pass afterward.

After removing all temporary diagnostic code and rebuilding, three consecutive
`egui-native-smoke --scenario=redock` processes completed in 1.817, 0.612, and
0.569 seconds. Each traversed tear-off, first-live, redock, child retirement,
and the existing runtime-quiescence predicate. These are real-window and
renderer-lifecycle results on this macOS machine. They do not certify physical
pointer dragging, mixed-DPI behavior, WM-close Allow/Veto, Windows, or X11.
Earlier timeouts are retained as failing evidence, not reclassified as passes
or hidden by increasing the smoke's global 30-second deadline.

Follow-up review found a second way to lose the child's paint obligation:
raw callbacks could already be claimed while an uncommitted native suffix
remained, or an admitted semantic child pass could yield a partial frame and
retain its old framebuffer. The prelude now also requires the claimed boundary
to commit. Both `None` and retained partial child paths re-register their paint
obligation before the complete commit can release it. A new mailbox regression
failed on the former empty-journal-only gate; another exercises two partial
commits, dropping the final frame, and exact retry before any child wake occurs.
That test registers the child directly, so entry through the surface driver's
two registration branches is also covered by static review and real windows,
not claimed as a fully headless surface-driver execution. After this additional
fix, all 226 native workspace tests pass and three clean real-window repeats
complete in 1.532, 0.569, and 0.562 seconds.

The adapter also bounds output-prefix preparation to at most 256 terminals and
checks the shared deadline between them. A submitted-prefix cursor avoids
rescanning prior outputs across continuation attempts. Final core input commit
retires those reservations; partial commit and frame drop retain them. An
unsubmitted output suffix blocks application actions and close policy even
when the core recorder has drained. Frozen viewport rosters share one immutable
`Arc` allocation across queued, in-flight, and committed references rather than
cloning their owned vectors at each boundary. This is not yet complete resident
memory accounting.

The local-fork native workspace passes 226 all-target tests. This checkpoint
uses the native working-tree source with the previously qualified core
`fa8870c` snapshot; the concurrent overload kernel is qualified separately.
Independent read-only review found no confirmed blocker in the output cursor,
shared-roster ownership, or completed-preview receipt change. More-than-256
output stress with drop/abandon/retry, native ingress record/byte/age limits,
the reserved overload latch, sustained-load latency, and lifecycle soak remain
open. The required egui fork is still the unpublished local `b4ec9ff8` commit;
tracked workspace and CI pins remain `0716beee`. This does not complete U7 or
the overall refactor.

## 2026-10-28 U7 budgeted native adapter checkpoint (historical, not remotely pinned)

The working adapter now uses the budgeted native poll in ordinary surface
updates, terminal action submission, staging, close and shutdown. Eframe output
begin starts the budget before callback claims; the same token lends and returns
that budget across Repeat passes and the finalizer. Only its physical terminal
removes the slot. No mailbox or budget mutex remains locked during reduction.
Ignored/coalesced host-only callbacks consume the same record allowance; records
accepted by the joined core producer are charged by replay, without double
counting. `native_input_recorded_count` is a diagnostic counter, not a commit
acknowledgement, input capability or external replay cursor.

`None` retains unpublished atomic work and schedules another native output.
`Some.pending` commits only its exact input prefix and keeps the callback
boundary, submitted-output reservations and queued snapshot/close settlement.
Snapshot/close results seen in an earlier prefix remain on their existing owner
until the final report. Terminal action submission checks pending before taking
application Queued or local Pending actions. Old pass-local presentation/focus
facts are regenerated by a later semantic paint. Retained finalizers continue to
support submitting already Pending local actions without a new paint.

Close policy records each request once. Its existing pending owner retains any
exact decision plan across yields; the public plan outcome now includes its
exact native close request instead of requiring surface-based correlation.
Budget-deferred runnable close work preserves both root repaint and physical
terminal wake. Waiting for a real close callback is not itself runnable policy
work and does not request an idle repaint loop.

Two initial regressions reproduced premature mailbox release and consumption of
a snapshot whose outcome was absent from a partial report. Independent review
then found three additional problems: exhausted-before-claim was a fatal
protocol error, budget-deferred shutdown close could lose its wake, and host-only
callbacks did not consume record credits. All three had failing targeted tests
before their fixes. Current tests cover both early/late snapshot and Clear
outcomes, atomic yield with an empty raw mailbox, exact policy-plan continuation,
translation deadline suffix retention, pre-claim exhaustion and inert callback
accounting. These are deterministic adapter models, not OS/GPU certification.

Fresh gates use one Cargo build job, one nextest thread and the existing target:

| Gate | Result |
| --- | --- |
| Main workspace, all features and targets | 1,648 passed |
| Local-fork native workspace, all targets | 222 passed |
| Local-fork native workspace, debug assertions disabled | 222 passed |
| Public core budget consumer, default features off | 1 passed |
| Core budget library tests, debug assertions disabled | 19 passed |
| Both workspaces, correctness/suspicious Clippy denied | Passed |
| Both workspaces, strict public rustdoc | Passed |
| Cargo formatting and diff whitespace | Passed |

The core additions are renderer-neutral. Native tests use a current source
snapshot and explicit local fork overrides; all ten egui-family packages resolve
once to local 0.37.0, with Winit still at `f378b50f`. The fork is unpublished and
tracked workspace/CI pins still name `0716beee`. Native source migration remains
uncommitted until that dependency handoff can be made reproducible. No real
window, GPU or physical input qualification is claimed. Active journal
record/byte/age caps, the overload latch, bounded output-prefix admission,
scoped native input, sustained-load/release-latency and lifecycle soak remain
unfinished. This checkpoint does not complete U7, U8 or the overall plan.

## 2026-10-28 U7 local adapter held-close checkpoint (historical, not remotely pinned)

The working native adapter now consumes the local fork's exact held-close API.
The callback freezes its core binding and canonical opaque request together;
only successfully retained mapped callbacks return HoldClose. Core Allow/Veto
effects resolve that request on the actual UI context. Cancellation retains the
complete token, finds its original pending binding, and does not look up a newer
window route. Neither policy branch falls back to ordinary Close/CancelClose.
Requested facts enter the recorder before duplicate-suppression state is added,
so a rejected append cannot make retry silently swallow the original callback.

`NativeCloseProgress` exposes two fixed-size saturating counters: requests
retained at ingress and pending cancellations actually cleared by a committed
core transition. The new `cancel-close` smoke waits for one new committed
cancellation, both live/presented windows, the unchanged child layout, and
quiescence. It cannot succeed merely because the initial runtime is idle.
The X11 CI matrix now includes this real WM-close scenario. Eframe CI runs the
complete combined-renderer library suite rather than module-name filters that
would omit sibling close-test modules. These CI changes have not run here.

Local adapter gates use a current-working-tree snapshot outside the repository
and an explicit Cargo config override to the unpublished fork, leaving tracked
dependency pins and lockfiles untouched. Cargo metadata proves all ten selected
egui-family packages use one local 0.37.0 source; Winit remains at `f378b50f`.
Native workspace all-target tests pass: 211 normally and 211 with debug
assertions disabled. Correctness/suspicious Clippy groups are denied and pass;
Strict adapter rustdoc, formatting and diff whitespace pass. Existing non-denied
style/dead-code warnings remain. Exact gate counts and source fingerprints are recorded separately.

Independent read-only review found no confirmed defect in the token/original
binding and retirement paths. It did not treat a filtered ordinary Close as a
proven root deadlock: runtime-owned roots use deferred viewport omission, while
the external eframe root is retained by the core's external-owner policy.
The new rejected-Requested-fact regression first failed when suppression state
was deliberately inserted before the fallible append in a temporary source
snapshot; the current append-first implementation passes and preserves the same
raw callback across repeated rejection without creating pending close state.
Real-token success through the entire adapter, held-close/independent-cleanup
interleaving, and Clear-append failure still need targeted runtime/window proof.
Synthetic fixtures explicitly test tokenless rejection or correlation models;
they no longer pretend to authorize typed platform decisions.

This is not remotely reproducible native CI qualification: the workspace, smoke
and checkout pins still name `0716beee`, and the required local fork commit is
not pushed. No real-window, physical-input or GPU run is claimed. The adapter
still uses synchronous host-frame entry points; sharing the budget through
claim/poll/Repeat/finalization/close/shutdown and preserving partial settlement
remain unfinished U7 work. This checkpoint does not complete U7 or U8.

## 2026-10-28 U7 local native-close hold prerequisite (historical fork checkpoint)

The local egui/eframe fork commit
`b4ec9ff8bc719321442cf421fc7f5a496b80154d` adds neutral exact-request close hold.
It is not pushed. At this checkpoint the main native workspace and smoke still
used the published `0716beee` revision; neither handlers nor dependency pins had
migrated. These results qualify the local fork prerequisite only, not the
production native adapter's budget or close workflow. The core runtime remains
at its separately qualified `f719928` checkpoint.

The pre-translation native callback carries a canonical request including its
attachment, viewport, window and original event identity. Returning its exact
HoldClose prevents backend translation without issuing CancelClose. The existing
close ledger retains an optional Allow/Veto decision across updates. A weak
context handle does not create another owner or queue. Final Veto reports the
original cancellation once; root Allow requests shutdown, and child Allow leaves
physical child retirement with its host. Duplicate OS requests and decisions are
inert, foreign/retired requests fail closed, and exact cleanup preserves successor
requests. Full UI, logic-only and first input-hook resolution use the same path.

Independent static review found two P2 defects. A Veto resolved in a child update
could let the renderer's retained root Close reappear after the held gate ended.
The fix consumes the backend-owned event before cloning any viewport input, in
all four Glow/WGPU capture paths. A real deferred-child EpiIntegration callback
regression first reproduced the later root shutdown, then passed after the fix.
Glow recreation also retained old close authority until successful successor
creation. Its exact mapping/request cleanup now precedes old Window Arc drop and
the replacement admission callback. The builder-recreate regression first failed
because the old request still accepted Allow in a Defer callback; it now rejects
both decisions, clears old events/maps and preserves a later successor. No fake
Destroyed callback is used to make retirement pass.

The original review and separate follow-up are preserved. Both findings are
closed by the targeted follow-up; static review is not counted as a test run.
The Glow recreation case deliberately uses renderer-owned mapping state without
an OS Window, while calling actual builder-patch and admission code. It is not
physical-window, real platform Defer, GPU or OS-input certification.

Fresh local-fork gates (single build job and one nextest thread):

| Gate | Result |
| --- | --- |
| Combined Glow/WGPU eframe library | 69 passed |
| Glow-only eframe library | 69 passed |
| WGPU-only eframe library | 63 passed |
| Combined library, debug assertions disabled | 69 passed |
| Glow without native-host-seam | Compiles |
| Correctness and suspicious Clippy groups, all eframe targets | Passed with both groups denied |
| Strict eframe rustdoc with inspection/persistence enabled | Passed |
| eframe formatting and diff whitespace | Passed |

The first strict-doc attempt with both optional features off exposed existing
links to their absent exports; enabling those features qualifies the complete
documentation without changing unrelated upstream docs. Existing non-denied
style and feature-off unused-variable warnings remain.

Next U7 integration must retain the canonical token with the callback-time exact
native binding, translate core decisions through this seam, and share one budget
across claim, poll, Repeat/finalization, close and shutdown. Partial reports must
keep queued snapshot/close state and submitted output reservations. The eventual
remote fork pin must be available and all egui-family sources aligned before CI
or packaging can claim the new path. Admission caps, overload, scoped native
input and release-latency qualification remain separate unfinished work.

## 2026-10-28 U7 budgeted runtime continuation checkpoint (in progress)

The renderer-neutral runtime exposes `NativeMonotonicClock`, `NativeInputBudget`
and `DockspaceSession::poll_native_host_frame`. One caller-owned budget starts
before prefix claim and remains the same through selection, receiver resolution
and reduction. It yields after 256 completed records or an inclusive two-ms
deadline. This is a cooperative boundary, not a hard real-time promise: one
already started record and all of its exact receiver receipts finish first.

Ordinary input seals the actual completed prefix. An unfinished indivisible
group instead returns `None` and retains the same unpublished core candidate and
submitted presentation observation inside the existing runtime native owner.
The next poll continues that candidate rather than rebuilding its processed
records or recapturing sidecar observations. Recorder selection clones at most
the ordinary prefix or the first complete oversized atomic group, never its
later independent suffix. Later appends cannot join a frozen attempt.

Returned frames and committed reports expose `native_input_pending()`. Until the
earlier accepted suffix drains, application semantic/configuration input is
rejected before prepared-action conversion, source-sequence assignment or
pane-focus generation allocation. Hosts must check the pending flag before
taking their own affine queued actions. Rendering and explicit surface settlement
remain possible.
Binding-roster admission now records the exact unsettled ordinal; an unrelated
earlier prefix commit cannot clear it. Presentation sidecars settle only the
stream/generation reports actually present in the committed transition.

Deterministic tests cover 257 independent records as 256+1; a 257-record atomic
group as an unpublished 256-record attempt plus one resumed record; later append
exclusion; already expired budgets; dropped/revoked candidates and reused
ordinals; application gates; two presentation streams split across commits; and
the exact unsettled roster boundary. A real presented tab receiver callback moves
the injected clock to three ms: the current press completes and seals, while the
release remains unstarted until the next poll. A separate integration test uses
only public API types for ordinary and atomic continuation.

The synchronous `begin_native_host_frame` convenience still deliberately drains
without a time budget and reuses the same reducer. The egui native coordinator
has not switched to budgeted polling yet. Its migration must share one dispatch
budget across Repeat/finalization/close, retain submitted-output reservations and
queued snapshot/close state across partial reports, and avoid taking queued
application/local actions while yielding. In particular the current eframe seam
can close a window in the same update before a delayed core decision: an exact
neutral close hold is required; `CancelClose` would incorrectly cancel the real
request. Read-only fork inspection confirmed this against the pinned source.

No adapter-wide throughput, admission-capacity, overload, release-latency or
physical-platform qualification is claimed by this runtime API checkpoint.

Independent static review found one P2: a native document restore after a budget
pause reused the old frame's item-identity scope and rejected a newly restored
item. A public regression first failed with `ItemIdentityOutsideScope(ItemId(2))`.
The restore entry now discards only that unpublished attempt and rebuilds from
the committed watermark under the restore's expanded identity scope. Ordinary
polls still preserve their exact candidate. The same regression passes, and a
separate follow-up review closes the finding; the original review is preserved.

Fresh checkpoint gates:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,647 passed |
| Fork-backed native workspace, all targets | 203 passed (synchronous compatibility, not budgeted adapter qualification) |
| Budget tests without default features, including the public consumer binary | 18 passed |
| Budget and restore tests, all features, debug assertions disabled | 19 passed |
| Real receiver callback advancing controlled time to three ms | Passed |
| Root/native correctness and suspicious Clippy groups | Passed with both groups denied |
| Core/adapter doctests | 10 passed |
| Strict core/adapter rustdoc, formatting and diff whitespace | Passed |

All Cargo verification uses one build job and one nextest test thread. Existing
style/dead-code warnings remain outside the denied Clippy groups. These are core
protocol and adapter compatibility results, not new OS or GPU evidence.

## 2026-10-28 U7 core record-boundary replay checkpoint (in progress)

Core ingress replay now reduces at most one record per explicit advance. A pointer
record remains incomplete until its exact receiver receipts validate and reduce;
submitting those receipts does not implicitly start its successor. The existing
cursor owns both the fetched and completed positions, with no alternate reducer.
`RecordBoundary` reports the completed count and whether the boundary permits
publication. Callers explicitly resume or seal the actual processed prefix.

Sealing truncates the immutable batch before constructing the existing commit
guard. Only processed records advance the candidate watermark or enter that guard.
Selected but unprocessed tail records remain recorder-owned and rollbackable both
before and after prefix publication. A discarded candidate replays from the live
engine watermark; revoking and reusing a processed ordinal cannot revive the old
append identity. Pending pointer receipts, completed/empty cursors and indivisible
group interiors reject invalid continuation or seal operations.

`record_native_pointer_batch` now freezes explicit indivisible commit groups.
The new `record_native_pointer_sequence` shares its all-or-none admission and
iterator-unwind protection, while preserving a commit boundary after each edge.
The native cursor-claim path uses the sequence API. Device removal and both
Destroyed stages retain their original atomic-batch API. No callback payload can
escape the private append transaction before its group metadata is frozen.
Bounded recorder selection stops before a group which would cross its limit and
returns a typed error when even the first pending group cannot fit. It cannot
silently split the terminal roster or accept an interior committed watermark.

Eight direct core regressions cover precise 256-record sealing, identity-sensitive
retry, dropped frames, tail ownership before and after publication, pending pointer
receipts, empty/complete cursor misuse and atomic group boundaries. The 257-record
atomic case retains the same private candidate after record 256 and publishes the
whole group only after record 257; this proves the core continuation mechanism,
not runtime scheduling. Three native admission regressions compare batch/sequence
boundaries and verify invalid suffix and iterator-panic rollback in both modes.

The runtime currently drives these explicit boundaries until its complete suffix
finishes. The injected 256-record/2-ms scheduling budget, runtime-owned suspended
atomic attempts, pending-suffix mailbox/action gates and capacity/overload work
remain unfinished. This checkpoint does not qualify U7 or platform input latency.

An independent static review checked record/receipt advancement, exact guard
membership, rollback/reused identities, private metadata freezing, group-aware
selection and native terminal call sites. It found no confirmed defect. This is
separate from the executable gates below and excludes future runtime scheduling.

Fresh checkpoint gates:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,627 passed before the last test-only strengthening |
| Final core prefix regressions, including the additional eighth case | 8 passed |
| Ingress/admission focused run before the final prefix-only addition | 37 passed |
| Fork-backed native workspace, all targets | 203 passed |
| Final core prefix, group-selection and admission tests with debug assertions disabled | 16 passed |
| Root/native correctness and suspicious Clippy groups | Passed with both groups denied |
| Core doctests | 9 passed |
| Strict core/adapter rustdoc, both workspace format checks and diff whitespace | Passed |

The final test-only strengthening adds post-publication tail rollback and checks
revocation after the producer reuses the processed ordinal. Production behavior
did not change after the full workspace run. Existing style/dead-code warnings
remain outside the denied Clippy groups.

## 2026-10-28 U7 first bounded-cursor prefix checkpoint (in progress)

The native coordinator now claims up to 256 consecutive cursor records on one
exact device/window/viewport/binding/route lane. Active gestures keep every
motion edge. Idle coalescing remains before semantic sequencing and is disabled
when either core gesture or adapter button/contact state is active. Focus,
output, capture/route changes, contact, scroll phase and lifecycle records stop
the cursor prefix; they retain their existing causal barrier semantics.

`CursorPrefixClaim` borrows the existing mailbox mutex. Membership cannot change
while a candidate translator builds a batch and the core recorder accepts it.
A translation or recording failure drops the claim without changing the raw
queue or published translator. Once accepted, the raw payload is removed and
only the existing recorder owns replay. A dropped host frame retains that exact
semantic prefix; the mailbox commit barrier prevents later callbacks joining it.
There is no new throughput queue or gesture terminal owner.

The old `reduce_callback_head` entry is deleted, with driver, shutdown and test
callers migrated to `reduce_callback_prefix`. Its optional nonzero progress value
reports the actual raw prefix count; barrier callbacks report one. The former
universal one-callback/one-frame assertion now specifically verifies distinct
focus barriers. The existing active-core-gesture test checks count=2, preserving
its no-coalescing contract while both records share one commit boundary.

The recorder's new `batch_prefix_after` clones only the selected complete records.
It uses the actual last selected ordinal, exact predecessor append identity,
retired watermark and existing liveness/commit guard. `batch_after` delegates to
the same implementation for the existing complete-suffix contract. Four core
regressions cover 257 records as 256+1, non-consuming identical retry, empty and
retired boundaries, failed publication and rollback/reused-ordinal rejection.

Evidence was collected before behavior changes. Both native prefix tests failed
against the old implementation with the queue at ordinal 3 instead of 258 or 6.
All four core prefix tests failed on the old complete suffix (257 instead of 256,
or two instead of one). The final native tests also cover unretained-claim drop,
foreign-authority failure without queue/translator publication and exact replay
after a dropped frame. A real active-drag regression compares all 256 event-time
receiver-query points and their order on both abandoned and committed attempts;
queue counts alone are not treated as proof of semantic content.

An independent static review found no concrete defect. It identified that the
initial count/queue tests alone did not prove semantic-edge content; the active
drag point-by-point replay regression closes that evidence gap. Static review
is not counted as a test run or physical input qualification.

This is an incremental U7 checkpoint. Runtime replay still consumes its complete
recorder suffix, and the non-cursor callback families still use their existing
single-record barriers. The 4,096-record/4-MiB/250-ms admission limits, reserved
overload terminal, cooperative 2-ms reduction budget, bounded output prefixes,
1,000-Hz release-latency profile, local-touch coordinates and validated scope
continuity remain required U7 work. No whole-U7 or physical-platform completion
claim follows from this checkpoint.

Fresh checkpoint gates:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,616 passed |
| Core backend-ingress regressions | 26 passed, including the four new prefix cases |
| Fork-backed native workspace, all targets | 203 passed |
| Native library with debug assertions disabled | 178 passed |
| Active drag: 256 receiver points, abandoned attempt and identical retry | Passed |
| Root/native correctness and suspicious Clippy groups | Passed with both groups denied |
| Strict core/adapter rustdoc, workspace formatting and diff whitespace | Passed |

Existing dead-code/style warnings remain outside these denied lint groups. The
native results are deterministic adapter evidence, not new OS input certification.

## 2026-10-28 U12 core host readiness qualification

The renderer-neutral local/contained host contract is qualified independently of
egui and native platform hosting. This completes U12's core host readiness
checkpoint. U7/U8/U10/U11/U9 and the overall docking/multiview plan remain
separate unfinished work. The application still owns UI/rendering, focus
execution, input facts and output disposition.

`integration/dockspace-host-harness/examples/engine_host.rs` is a runnable complete
walkthrough. It consumes the borrowed surface program into an owned recording,
ends the frame borrow, processes the retained records, and settles two logical
surfaces as Presented and Dropped. It does not grant receiver authority before
successful settlement. It executes an exact application focus request after
presentation, submits its observation, completes a root close through per-item
approval, and deduplicates a real cancellation by publication identity.

`host_coordinate_domains.rs` completes AE14 through eight combinations of
Immediate/Deferred consumption, window DPI 1.25/2 and framebuffer densities
2/0.75 or 0.75/2. A captured source route may lie outside A while the independently
mapped hover route lies inside B. The hover point comes from actual pane content
and misses all guide receivers; the host proves clearance and core chooses the
magnetic target. Each complete input batch waits for its exact preview output,
then publishes one changed result and one Delivered(Dock), with the same topology
and quiet subsequent settlement. Public versions remain opaque; exact internal
revision counts are covered by the core regression. A second independent static
review found no additional issue in this fixture. This is synthetic host-mapping
evidence, with explicit scale domains, rather than real platform DPI certification.

The independent Cargo workspace uses only public `dockspace` and application
record types. Its 39 default / 40 serde tests cover owned/deferred output, two
logical surfaces with equal local coordinates, complete input batches, stale and
foreign authority, rollback/retry, full contained content, commands, close,
feedback and structural output cost. The separate `dockspace_host_conformance`
crate is also a public API client: it supplies the complete 60-case L03 matrix
through immediate program acceptance. Its different crate boundary grants no
access to core-private APIs. The egui executor separately covers all four cadences,
including Burst, and 240 actual-content widget cases. These evidence sets are
complementary; the recording harness does not claim physical rendering.

Packaging was tested against the emitted artifact rather than another workspace
path. `cargo package -p dockspace --all-features --locked --allow-dirty` packaged
and verified the core. The archive was extracted to a temporary directory, and a
copied independent consumer depended exclusively on that extracted crate. Its
39 default / 40 serde tests and runnable host example passed with `--locked`.
The archive SHA-256 is
`bf821da1642e07f9a2ead8e23a526f890fd7abb17e90da110bf4a4f7a344af66`.
All 164 packaged Rust source files and the README were byte-compared with the
current crate and matched. It contains the U6 core source plus the U12 README
update; the packaging dirty flag records that documentation change, and no
registry publication occurred.

`check_core_dependencies.py` inspects Cargo's normal and build dependency graph
with all features and targets. It rejects the named UI/platform framework
families without parsing Rust sources. An independent review caught the initial
normal-only check; the corrected guard was verified on an isolated copied core
with an actual egui build-dependency and build script. It returned exit code 1
and identified egui/emath/epaint/accesskit. The real consumer graph passed.
CI now runs this guard, the host example, and every default/serde product target,
including the new feedback binary.

The public structural fixture uses balanced pane trees. It counts public work
and retained recording storage, including spare Vec capacity:

| Panes | Measurement requests | Program steps | Owned records | Record capacity | Retained recording bytes |
| --- | --- | --- | --- | --- | --- |
| 16 | 49 | 126 | 242 | 256 | 63,512 |
| 128 | 385 | 1,006 | 1,906 | 2,048 | 507,928 |
| 1,024 | 3,073 | 8,134 | 15,306 | 16,384 | 4,063,256 |

A `needs_drop` assertion prevents ordinary nested owned allocations from silently
escaping this recording-buffer count. The fixture complements the fresh U4 core
compilation counters at the same pane counts and the resolver/selection work
regressions. It does not measure total core allocations, GPU resources, wall-clock
latency or native ingress budgets.

| Gate | Result |
| --- | --- |
| Verified core archive and extracted downstream default / serde | Passed / 39 / 40 tests |
| Extracted downstream runnable engine host | Passed |
| Real dependency graph / isolated egui build-dependency negative case | Passed / rejected with exit 1 |
| Public output structural fixture, 16 / 128 / 1,024 panes | Passed |
| AE14 immediate/deferred, independent DPI/density, eight combinations | Passed |
| Adapter package contents | Listed successfully; not a registry dependency verification claim |
| Host formatting and diff whitespace | Passed |
| Strict docs / doctests after the U12 README update | Passed / 10 passed |
| Core/public/product/official/native regression | The U6 matrix in the next section uses identical core Rust source |

Historical U1 known-invalid Burst and incomplete contained rows below are retained
as audit provenance. U3/U4/U5 qualifications supersede those local blockers.
Native physical qualification is still open and is never inferred from these
recording, package, model or headless egui gates.

## 2026-10-28 U6 commands, application close, feedback and focus

Contained-window close now requests a complete root close plan. Pointer, Enter,
Space and AccessKit use the same close path; the menu's separate `Close Group`
uses the same application approval workflow. `Dock Back` remains a distinct
content-preserving command. Protected groups disable close while leaving an
otherwise valid Dock Back available. The obsolete contained-close-as-Dock-Back
frozen action, input variant and terminal helper are deleted.

The public engine consumer and egui facade cover Allow, Veto, deferred Allow,
replayed initial/continuation tokens and group atomicity. Both packaged eframe
examples actually consume permitted close decisions. Their tests invoke the
real `App::ui` callback and accessible close control; the persistence example
retains application-owned pane data and restores it from its prior document.
These tests are included as example test targets rather than merely compiling
example files.

Committed interaction events now project typed pending and terminal feedback
with one opaque manipulation identity and a distinct stable publication ID.
Motion produces no status announcements. Pending drag and contained releases
retain their original identity on presentation loss, policy change, document
restore, replacement or provider retirement. Retirement returns the actual
committed cancellation rather than manufacturing a later event. Splitter
release covers both successful delivery and consumed input-authority rejection,
followed by another successful gesture and quiet settlement.

An egui shown response forwards its committed reports and retirements in order.
Direct commands, close decisions, policy changes and restore expose their own
feedback in `DockspaceMutation::manipulation_feedback()`. The application consumes
that result at the call boundary; a later shown response does not replay it.
This fixes the independently reviewed Pending -> programmatic Dock Back/close
feedback loss without introducing a deferred forwarding queue or new identities.

Focus requests now distinguish a pane from its remaining surface workspace.
Focused item close selects the next visual tab, or the previous tab at the end,
and updates durable MRU. This intentionally differs from historical GPUI
follow-up-close selection and is recorded in the OGC-02 catalog. Closing an
unfocused target preserves a newer intent. A group close can restore a surviving
observed destination; otherwise it requests the remaining workspace. Closing a
whole standalone surface does not request focus on its retired workspace.
Pending, vetoed and rejected close decisions do not change focus.

Explicit selection publishes a pane intent even for an already selected item.
Dock/Float resolve the destination after mutation; root-level commands consider
only a currently selected pane, so a hidden historical target cannot receive an
unfulfillable request. Exact NotFocused/Focused observations exercise the public
focus lifecycle. egui registers a persistent workspace focus target and settles
it through the existing output boundary. Escape still follows egui's native
focus-clearing behavior; cancellation feedback does not request a substitute.

Independent review identified hidden-history focus, direct-command feedback loss,
a vacuous old close-as-Dock-Back assertion, missing cross-surface focus assertions
and a large close-test block. Fixes and focused regressions address these; the six
new product close tests live in a child module with their original assertions.
The review also identified pre-existing programmatic Select focus behavior, which
is completed here as part of the common command loop rather than classified as a
new regression. This is model, public-consumer and headless egui evidence. It does
not certify physical native input, GPU presentation or overall plan completion.

Fresh verification on the final U6 source state:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets (including both example tests) | 1,612 passed |
| Public host U6 contract targets, default / serde | 37 / 38 passed |
| egui product, default / all features | 104 / 108 passed |
| Official egui, all features and all targets | 15 passed |
| Fork-backed native workspace, all targets | 198 passed |
| Strict rustdoc / doctests | Passed / 10 passed |
| Correctness and suspicious Clippy groups | Passed with both groups denied |
| Workspace/harness formatting and diff whitespace | Passed |

The public U6 counts name the four contract binaries (`public_contract`,
`contained_projection`, `close_commands`, `manipulation_feedback`); U12 output-cost
qualification is separate. Existing style/pedantic Clippy warnings are not a
clean-lint claim. The original review receipt preserves the pre-fix snapshot;
follow-up evidence records the implemented fixes and fresh gates instead of
rewriting that review history.

## 2026-10-28 U5 hover-port and composed-content qualification

The common host port uses the existing output-bound candidate, clearance,
blocked, no-receiver and unknown answers. U5 does not add a second target solver
or make egui widget ancestry a core responsibility. Official egui applications
declare ordinary content identities; popup, modal, foreign-layer, unknown-widget,
unsupported-coordinate and stale-pass cases remain conservative boundaries.

New qualification evidence:

- The product harness runs real `Button`, `TextEdit`, `ScrollArea` and canvas
  responses through all five magnetic basins, three non-guide samples and four
  existing cadences: 240 combinations. Every input sample lies inside the actual
  child response, not pane whitespace. The complete Burst gesture stays in one
  input batch. Before release there is no durable mutation; the exact final
  topology occupies one changed frame and reaches stable quiescence. This
  frame-level counter does not claim to count transactions within an egui run.
- Separate ordinary-input journeys verify button click, text editing, wheel
  scrolling and canvas drag ownership. Docking hover neither clicks the child
  widget nor converts it into a docking handle. Unknown content and overlapping
  undeclared widgets remain blockers across 24 widget/sample combinations;
  duplicating a declared ID cannot erase another widget with identical bounds.
- The official harness creates real `Popup`, `Modal`, foreground `Area` and
  same-layer explicit blockers and checks their actual bounds. It rejects both
  initial hover and terminal release through them, including a preview acquired
  before the blocker appeared. A completed unrelated egui pass cannot supply
  widget proof to an older accepted Dockspace output.
- The public-only host consumer submits current routes with old candidate,
  clearance, no-receiver and blocked facts after a new emission or scene. None
  can borrow the current output. A valid route with unknown release facts clears
  a previously presented preview without docking or floating fallback; later
  pointer selection and semantic commands still work in the same session.

Independent review found no actionable defect, but identified that the product
matrix's changed-frame counter cannot alone prove transaction-level exactly-once
delivery. The existing core
`move_then_release_with_new_preview_waits_for_exact_presentation` regression now
also checks exactly one durable revision and one `Delivered` event on settlement,
then two later settlement frames with no new revision or delivery event. This
complements the public host's duplicate-release and dropped-frame retry journey;
the product matrix retains its narrower observable frame-level claim.

The front-contained public journey exposed a real kernel defect: a background
magnetic acquisition could remain latched when the pointer entered a different
front occlusion layer near the new basin's hysteresis margin. The old target's
larger geometric envelope was incorrectly sufficient to retain it. Acquisition
and observation now carry the core `SceneLayerKey`; a different front layer or
an outside-surface observation always accepts the newly resolved result instead
of retaining the old target. Source-only scene-lineage rewriting preserves that
layer. Same-layer hysteresis is unchanged. The regression first reproduced the
background root as active, then passed with the front candidate explicitly
`Rejected`, no preview, no commit and no contained fallback after the fix.

Fixture qualification did not relax the production boundary: a full-size text
editor uses a fitting minimum row count rather than overflowing a small pane,
and non-guide samples are checked against actual guide rectangles. A target
preview after source removal is not compared to an obsolete pre-removal layout.

These are deterministic kernel, public-consumer and headless composed-UI tests,
not physical input, GPU presentation or native multiview certification. U5's
common port and official boundary do not certify the fork-backed final-stack
extension; that remains U7/U8. User-visible typed manipulation feedback, Close
Group versus Dock Back, close consumption in examples and focus successors
remain U6 work. U12 readiness and the overall plan are not declared complete.

Fresh verification on the U5 source state:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,597 passed |
| Public host, default / serde | 30 / 30 passed |
| egui product, default / all features | 96 / 100 passed |
| Official egui harness, all features and all targets | 15 passed |
| Fork-backed native workspace, all targets | 198 passed |
| Strict rustdoc / doctests | Passed / 10 passed |
| Correctness and suspicious Clippy groups | Passed with both groups denied |
| Workspace/harness formatting and diff whitespace | Passed |

Clippy still reports existing style/pedantic warnings. The native count remains
deterministic regression evidence rather than a new physical-platform claim.

## 2026-10-28 U4 complete contained content projection

Contained title/group moves and eight-direction resizing now compile their
transient outer rectangle through the existing surface compiler. Contained
chrome, pane subtrees, splitters, hit manifests and semantic bounds therefore
share one layout input. Only terminal delivery mutates durable placement.
The pending-release path retains the complete projected tree until its exact
output is presented or dropped; cancellation restores the durable layout from
the retained complete measurements without requiring another host callback.
Cached measurements are reused only under their original requirement ticket.

Source-only successors retain target-layout provenance from the exact presented
scene used for solving, not from an unpresented candidate. This preserves exact
preview tokens, guide visibility and target hysteresis across a source layout
update without granting stale hover authority. Changed external measurements do
not inherit that lineage. Prepared contributions revalidate the current
projection owner and rectangle at installation, and a cancelled projected source
cannot supply new title, resize, tab or splitter source geometry before durable
geometry is presented again.

The public `with_clear_hover_lane` fact and native `ClearHoverLane` answer make
framework clearance explicit and output-bound. Core chooses the semantic winner
after each preceding input edge, including the Armed-to-Dragging transition.
Complete sources no longer occlude targets behind themselves; partial sources,
other contained roots, popups, unknown widgets and blocked releases retain their
blocking semantics. Existing `with_hover` and `with_no_hover` remain exact
assertions rather than fallbacks. The adapter paints the real projected source
once, keeps its grabbing cursor, and omits duplicate ghost/preview shells.

New and strengthened proof boundaries:

- `integration/dockspace-host-harness/tests/contained_projection.rs` covers a
  two-pane split root, continuous move, every resize direction, stable visual
  identities and projected receivers, pending Presented/Dropped settlement,
  active-output revocation, changed external bounds/minima, ordered delayed
  output settlement, and a pre-frozen same-batch behind-source docking journey
  with a blocked-release control. A complete tabs source is also presented after
  each magnetic-boundary jitter sample: its preview, active guide and final
  topology stay latched inside the release margin and change outside it.
- `integration/egui-product-harness/tests/product.rs` checks real pane callback
  rectangles, content markers, application widgets, opacity, stable widget IDs,
  one selected-item callback per pass, and matching AccessKit/chrome geometry.
  Existing complete-source gap/ghost assertions now require visible full
  content; partial-source gap coverage remains intact.
- `engine/contained_projection/authority_tests.rs` rejects the A-presented /
  B-unpresented / C-source-only target-proof splice, a prepared contribution
  installed after its owner was cancelled, and re-acquisition from a cancelled
  but still-displayed transient source. A partly offscreen source remains
  operable through its visible title, resize, tab and splitter receivers: source
  checks use the existing unclipped occlusion geometry, not the clipped frame.
- `engine/contained_projection/tests.rs` counts compilation at 16, 128 and 1,024
  panes. A batch of 64 move events compiles zero times during input and once at
  input close; repeated paint-plan queries and unchanged frames reuse that
  output. Cancellation compiles once to restore the durable layout.
- `engine/contained_projection/measurement_tests.rs` verifies that same-frame
  fresh measurement compiles only once, while measuring surface B still updates
  the active source on A. Blocked/unknown hover preserves valid source motion
  without authorizing a target or a release.
- `engine/contained_projection/selection_cost_tests.rs` counts actual pane
  visits and bar lookups during installation. The old nested lookup reproduced
  136 / 8,256 / 524,800 pane visits at 16 / 128 / 1,024 panes. The replacement
  builds one `PaneSceneId` index and stays within the linear visit budget, with
  one lookup per bar and unchanged missing-selection semantics.

Independent review and follow-up closed the clipped-source geometry defect,
fresh-measurement duplicate compilation, quadratic selection lookup, and the
cross-projection hysteresis coverage gap. The duplicate compilation and
quadratic lookup have observed failing-before/passing-after count regressions.
A narrow follow-up also reproduced and fixed source feedback loss after a
blocked/unknown-hover measurement. Its four source-acquisition paths include the
internal splitter, and the full durable workspace remains unchanged on rejection.
The native clearance concern was rejected with semantic-lane and blocker
evidence: framework clearance is not a semantic passthrough or target authority.
The original local review receipt remains a historical pre-follow-up report;
these fixes and the final test matrix are caller-owned closure evidence, not a
claim that reviewers executed Cargo or that the complete plan is finished.

Fresh functional verification on the U4 source state:

| Gate | Result |
| --- | --- |
| Root workspace, all features and all targets | 1,596 passed |
| Public host, default / serde | 27 / 27 passed |
| egui product, default / all features | 93 / 97 passed |
| Official egui harness, all features and all targets | 12 passed |
| Fork-backed native workspace, all targets | 198 passed |
| Strict rustdoc / doctests | Passed / 10 passed |
| Correctness and suspicious Clippy groups | Passed with both groups denied |
| Workspace/harness formatting and diff whitespace | Passed |

Cargo commands use one build job, the shared target directory and one nextest
test thread. Other pre-existing pedantic/style Clippy warnings are not promoted
to errors by this gate. These are deterministic library, recording-renderer and headless
adapter checks, not physical platform certification. The broader ordinary-widget
hover qualification remains U5, full commands/focus remain U6, and independent
host retained-state/latency qualification remains U12. U7/U8/U11 physical native
qualification and the remaining plan units are not completed by this gate.

## 2026-10-28 U3 journal-only gesture replacement

U3a, U3b and U3c now pass the replacement gate. `SurfaceGesturePhase`, all local
gesture-preparation APIs and `EngineInput::Local*Gesture` variants are deleted.
The local tab/splitter/contained phase reducers, contained primary-press action,
local position identity and gesture-specific `LocalReady` authority variants are
also deleted. `GestureOwner` is now a private alias for the admitted
`PointerStreamId`, not an alternate owner enum. Capture authority and drag
threshold origin are mandatory; `Authority::Unknown` remains an explicit fact,
not an absent owner. Read-only gesture views return their definite pointer and
stream identities without a legacy optional branch.

Non-pointer candidate actions remain valid without pointer/native enrollment.
Their focused modules are now `semantic_candidate.rs` and
`contained_adjustment.rs`. Accepted host-local tab activation now publishes the
same core pane-focus intent when no native viewport owns focus. Full command and
focus experience certification remains U6, rather than being inferred from this
single integration correction.

Presentation feedback tests now use public host batches and complete output
execution/presentation observations. Contained tests preserve causal press/Raise,
measurement, actual presentation and motion boundaries. The complete contained
redock Burst stays one input batch; its terminal artifact may retire when exact
preview presentation commits the topology, so it is not reused as receiver
authority. Native tests keep their actual provider domain and submit empty
checkpoints in semantic frames. The coordinator's cursor-coalescing test now
begins a real managed native press while its translator remains idle.

Two tests tied only to the deleted local-owner state are removed. Two duplicate
local splitter commit tests are covered by existing journal handle/junction
journeys; their cancellation peer is migrated to a real journal stream cancel.
The resulting root count decreases by four, without deleting presentation,
contained or native lifecycle journeys. The obsolete local-only identity
reservation is deleted; its atomic floating-counter exhaustion check now covers
the canonical composite drag reservation instead.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,583 passed | Journal-only kernel, semantic candidate actions and egui adapter |
| Public engine host, default and serde | 20 passed each | Framework-neutral downstream contract |
| Product harness, default | 93 passed, none ignored | Controlled chrome, cancellation and output-bound receiver facts |
| Product harness, all features | 97 passed, none ignored | Includes the unchanged complete L03 Burst matrix |
| Official harness, all features and targets | 12 passed | Public API plus caller-owned demo compilation |
| Native workspace, all targets | 198 passed | Includes the migrated managed-domain cursor-coalescing test |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Removed APIs have no required documentation seam |
| Correctness/suspicious Clippy and formatting | Passed | Nextest uses one test thread; no concurrent Cargo commands |

Independent read-only review found no new P1/P2 in source continuation,
presentation revalidation, capture authority, threshold origin or exact-owner
cancellation after deletion. These deterministic gates do not establish full
live contained content (U4), arbitrary egui widget/blocker interoperability (U5),
the complete application loop (U6), structural cost/readiness (U12), or native
physical platform claims. The overall refactor remains active.

## 2026-10-28 U3b ordered egui transport checkpoint

The official adapter now captures the raw event packet at `run_ui` entry,
qualifies each event position against an exact accepted output, and submits the
ordered batch before measurement or paint. Run identity prevents replay during
egui discard reruns. A failed packet cannot silently retry with different facts;
the next run cancels the previous owner before accepting fresh events. Explicit
touch cancellation, focus loss and Escape terminate the same journal owner.
Changing egui Context or viewport retires the previous affine producer before
the adapter clears its routing cache.

Click and drag delivery are independent semantic lanes. In particular, contained
frame click fallbacks cannot replace title/resize drag receivers. Rear contained
content activation now uses the journal's exact click fact and the core's checked
Raise command rather than a second renderer activation frontend. Pending release
retains its frozen drop affordance as well as its preview until output settlement.

The renderer no longer reconstructs docking gesture phases from `Response`.
Keyboard, AccessKit, application commands and the existing scroll subsystem keep
their semantic paths. All remaining internal preview-only ACK inputs, factories
and reducers are deleted; their fixtures use complete output execution and exact
presentation observations instead.

Portable egui qualification is deliberately conservative. Receiver declarations
belong to one accepted output and one exact completed egui pass. The previous
pass's anonymous interactive rectangles cannot be borrowed by an older output
accepted late. Content registrations read only `this_pass.widgets`; qualification
reads only `prev_pass.widgets`, without `read_response`'s cross-pass fallback.
Both emission-time and input-time coordinates must use identity layer transforms;
an invalid coordinate domain cancels the stream and never supplies a Known raw
manipulation route. Unknown interactive widgets block this lane.
`PaneView::docking_content_widgets` lets the application
explicitly declare ordinary widgets belonging to its content slot; it is not an
automatic ancestry resolver and must not include popup/modal/foreign controls.
Each exact widget ID removes at most one anonymous rectangle, so duplicate
declarations cannot erase an unknown equal-rectangle blocker.

New product regressions cover raw close-click replay after output-hook discard,
explicit touch cancel and fresh-press recovery, late presentation versus newer
unpresented receiver facts, registered/unregistered contained content, and a
duplicate content declaration overlapping an unknown same-rectangle widget.
Context rebinding without a queued output report, transformed output followed by
an identity transform, and coordinate changes during an active resize all cancel
or reject stale authority and preserve durable placement.
Discarded-preview tests require cancellation without later implicit delivery,
then prove a fresh qualified gesture works. The resize test checks the actually
painted transient move output; it no longer requires a redundant release-frame
ghost when that preview was already accepted before release.

The original complete L03 magnetic matrix is no longer ignored. Its Burst
executor still delivers press, threshold move, target move and release in the
same raw packet; no compensating input frames were added. Ordinary rendering
and exact output settlement may happen afterward.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,587 passed | Core, conformance and official adapter |
| Public engine host, default and serde | 20 passed each | Public renderer-neutral contract, no native enrollment |
| Product harness, default | 93 passed, none ignored | Includes the unchanged complete Burst matrix |
| Product harness, all features | 97 passed, none ignored | Adds persistence to the same product gates |
| Official harness, all features and targets | 12 passed | Includes caller-owned demo compilation |
| Native workspace, all targets | 198 passed | Recording/unit tests on the pinned fork; not physical certification |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Public API and compile-fail documentation |
| Correctness/suspicious Clippy and formatting | Passed | One Cargo worker and Nextest test thread |

Two independent static reviews found and closed Click/Drag lane conflation,
touch cancellation, Context/viewport retirement and transformed-coordinate
continuation boundaries. The core contained activation review found no blocker
within its scoped change. Temporarily accepting a newer pass's receiver facts
and allowing one widget ID to consume multiple anonymous rectangles made the
late-presentation and equal-rectangle blocker regressions fail respectively.
Both mutations were restored before the final passing gates.

At this earlier transport checkpoint U3 was not complete: the core
`LocalResponse` owner and local gesture frontends still required deletion and
test migration. The journal-only replacement section records their subsequent
retirement. Full contained content projection (U4), broader egui blocker
interoperability (U5), accepted semantic focus bridging (U6), public host
readiness/cost certification (U12), and physical native certification are not
established by these headless results. The following U3a/U2 sections retain their
historical gate counts and pre-transport boundaries.

## 2026-10-27 U3a host input domain and terminal convergence

The public host now owns one affine pointer producer across logical surfaces.
`enable_host_pointer` requires no native enrollment or presented surface;
`submit_host_pointer_batch` assigns one global sequence to all ordered edges.
Every edge independently qualifies manipulation/source and hover coordinates
with exact presented capabilities. Capture belongs to the host domain. Unknown
logical routes do not grant desktop outside-all authority. Existing ledger,
receiver, rollback, producer-drain and retirement mechanisms remain the owners
of transport integrity.

The downstream public consumer submits a complete press/move/release batch from
A to B, where both surfaces have identical bounds. Manipulation stays in A while
hover selects a different point in B. Actual owned recording outputs carry the
pending preview, and Presented or Dropped settlement respectively commits or
cancels the frozen release. No follow-up pointer events are synthesized. A
selection-changing press uses a separate measurement/paint cycle after the
complete input batch; pending release survives this real rendering lifecycle.

The ten new public journeys cover actual target-output settlement,
selection-changing activation, source/hover coordinate isolation, unknown
hover, dropped-frame retry and repeated release, stale route/current receiver
mixing, foreign-session capabilities, invalid manipulation after a valid press,
invalid hover during capture, and failed-suffix rollback. Additional core tests cover local
owner cancellation without panic, rear-contained Raise/resize/release,
HostLocal producer lifecycle and ledger boundaries, and locked scroll tails
whose event-time route or receiver becomes unknown.

Tab and contained release now share their actual terminal implementations
between journal and local-response frontends. Exact-owner cancellation also
uses one implementation; local session/source/target checks remain at the
frontends. A terminal splitter release with unavailable coordinates cancels
its owner instead of leaving a gesture waiting for an already-consumed release.
Legacy internal preview ACK reducers and the response-based input frontends
remain explicitly scheduled for U3b retirement.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,585 passed | Core, conformance and official adapter; includes source-continuation, locked-scroll and exact hover-emission regressions |
| Public engine host, default and serde | 20 passed each | Ten new public pointer journeys; normal dependencies contain no egui, Winit or WGPU |
| Product harness, all features | 90 passed, 1 ignored | Original single-call Burst exclusion remains open |
| Official harness, all features | 12 passed | Includes caller-owned WGPU demo compilation |
| Native workspace, all targets | 198 passed | Existing egui 0.37 fork pin; physical scenarios not executed |
| Strict rustdoc and documentation tests | Passed; 10 doc tests | Workspace API documentation and compile-fail boundaries |
| Correctness/suspicious Clippy and formatting | Passed | Shared test support is compiled once; builds use one Cargo worker and Nextest thread |

Independent review and the public tests closed four input boundaries: target
hover must use its own coordinate route; logical cross-surface movement does
not require native-window drag capability; Select/Raise may retain only the
exact active source's existing continuation; and that continuation expires
when another host takes the surface's presentation stream. Hover and new
delivery remain strict current-output claims. Another pointer cannot borrow
the source continuation. Stream checks use current active ownership rather
than historical host lookup.

Two mutation checks were executed. Restoring source coordinates in the hover
comparison fails the complete cross-surface release test. Removing source
continuation fails the unselected-tab release test. Production code was restored
before the final gates. These recording and unit tests do not establish OS
input delivery or physical GPU presentation.

A follow-up proof review added exact route-authority validation before dispatching
any known HostLocal hover disposition. `NoReceiver` and `Blocked` now receive the
same exact emission binding as a `Dock` result, even though neither goes through
Dock-region geometry validation. The regression produces two actual presentations
with identical semantic output and different emissions: generic semantic receipt
validation accepts their unchanged interaction geometry, while the HostLocal
route check rejects the mixed emission. Matching route/receipt pairs remain valid.
Removing this check makes the regression fail. The workspace, both public-host
feature gates, correctness/suspicious Clippy, strict rustdoc and formatting were
rerun after restoration. Preview position remains derived from the validated hover
position; delivery and threshold continue using the independent manipulation route.

Public rollback coverage now includes a valid prefix that selects an unselected
source and begins dragging, followed by a release claiming no receiver where B's
current drop receiver wins. The batch fails after the private candidate's version
has changed. Dropping the poisoned frame preserves published selection, version,
surface proofs and interaction state. Retrying the original event order with the
correct release fact succeeds, then exact preview presentation delivers once.
This verifies producer-watermark rollback through the public retry contract,
without exposing private stream IDs or counters. Separate active-source cases
reject stale and foreign manipulation even with a valid current hover, and reject
stale and foreign hover even with valid capture and manipulation. Temporarily
reading the raw manipulation route or advancing the producer watermark from the
frame guard's Drop makes the corresponding public regression fail. Both changes
were restored before the default and serde gates passed all 20 tests each.

U3 remains open. The egui adapter still reconstructs pointer phases from its
frame-level `Response`, and its original single-call Burst matrix is still
ignored. U3b must consume ordered raw edges through the same host owner with
event-bound receiver facts, remove duplicate pointer actions/local owners and
internal ACK paths, then qualify the unchanged Burst matrix in U3c. The plan
records this split; neither the full U3 nor the full U13/AE12-AE14 matrix is
declared complete by this slice.

## 2026-10-27 U2 owned-output settlement

The official adapter now completes the same exact output contract used by engine
and native hosts. `run_ui` returns owned output plus an affine host receipt;
returning from egui does not acknowledge presentation. The actual WGPU demo owns
acquisition, submission and `queue.present`, and reports the result after present.
Receipt drop retires its exact outputs. Discarded predecessors remain owned until
the terminal receipt is disposed, so an intermediate pass cannot revoke its own
replacement while egui is still rendering it.

| Gate | Result | Scope |
| --- | --- | --- |
| Root workspace, all features and targets | 1,571 passed | Complete core, conformance and official-adapter regression suite |
| Product harness, all features | 90 passed, 1 ignored | Existing single-call Burst exclusion remains a U3 blocker |
| Official harness, all features | 12 passed | Includes WGPU demo compilation and delayed pane-focus observation |
| Native workspace, all targets | 198 passed | Existing exact egui 0.37 fork pin; smoke binaries compile, physical scenarios are not run |
| Public engine host, default and serde | 10 passed each | Downstream public API only; no egui dependency |
| Strict workspace rustdoc and doc tests | Passed; 10 doc tests | Includes compile-fail protection for both removed public preview ACK methods |
| Correctness/suspicious Clippy and formatting | Passed | Cargo builds and Nextest execution remain serialized |

Complete surface spans include ordinary geometry, panes, previews, clip data,
post-tessellation transforms and exact pixel scale. Pane callback checkpoints
reject overwriting shapes already emitted by the current program. Other-layer
snapshots retain layer identity, transform and content. Nonempty unenumerated
layers are explicitly unavailable. The marker scrubber removes only private
markers and preserves user compounds and callbacks. This is a conservative
synchronous single-layer admission contract, not inspection of application paint
callback internals or physical GPU presentation.

Independent review found and closed four boundary gaps: pane callbacks deleting
already emitted chrome, identical shapes migrating between layers, delayed focus
observations being lost before core accepts their artifact, and child receipts
waking the root viewport. Waiting focus observations remain paired with their
artifact until a committed `presents` or `retires` result; context/viewport lane
changes retire that adapter cache. A failed observation frame leaves the waiting
observations intact. Affine focus observations are consumed only after the output
has been accepted; failure of that separate submission requires a fresh pane
observation rather than duplicating its capability.

Two core corrections are also covered directly. Successful exact preview
promotion preserves its still-current target and source gap, while Dropped
successors still revoke it. Identical complete measurement and coordinate facts
retain the immutable Ready candidate; changed facts still replace it, and every
concrete paint keeps a distinct emission key. Replacement fixtures now change
geometry explicitly instead of relying on identical remeasurement to force a
new scene stamp.

Three mutation checks were executed: removing the positive-promotion guard,
removing the pane append check, and dropping waiting focus observations each
made its corresponding behavioral regression fail. Restoring the production
code passes the final gates above. The redundant renderer-pass bridge and both
public preview-only ACK methods are deleted. U3's manipulation-owner and ordered
logical-surface input work remains separate; no native multiview or real-window
interaction completion is inferred from these headless gates.

## 2026-10-27 U14 and dependency migration baseline

The implementation preserves the audit below as historical failure evidence.
The following results were freshly executed after the U14 fixes, public host
contract work, and official egui 0.37.0 migration. Cargo builds were serialized
with `CARGO_BUILD_JOBS=1`, and Nextest used `--test-threads=1`.

| Gate | Result | Scope |
| --- | --- | --- |
| Workspace, all features and targets | 1,566 passed | Official egui 0.37.0; includes three new failed-output/focus regressions |
| Independent public host, default features | 10 passed | Owned geometry/content/identity records, exact present/drop and commit authority, out-of-order results, two logical surfaces and layout commands |
| Independent public host, `serde` | 10 passed | Includes dual-stream receiver isolation, retired and stale output rejection, automatic output drop, popup barrier retention, and failed core commit/retry |
| Product harness, all features | 90 passed, 1 ignored | Official egui/kittest 0.37.0; existing Burst exclusion retained |
| Official consumer, all features | 9 passed | Includes compilation of the caller-owned WGPU demo |
| Native workspace, egui 0.37 fork | 180 passed | Remote exact pin `0716beee5b7284389f2e80ffb6b24b5af6ae2910`, three test binaries |
| Complete native workspace, including smoke unit targets | 198 passed | Five test binaries; this compiles the smoke executables but does not run a real-window scenario |
| Native library, debug assertions disabled | 173 passed | Same remote 0.37 pin |
| Fork egui and egui-wgpu libraries | 112 passed | Includes five new real elastic-scroll consumption/snapshot regressions and retained-overlay test |
| Fork eframe library, Glow + WGPU + native seam | 53 passed | Host finalizer, output identity, create, retention and renderer seams |

The native fork branch `dockspace-native-seam-0.37` is committed and pushed.
Manifests, lockfile, CI checkout, README, and seam admission agree on the exact
revision. Winit remains at `f378b50f29cafafec31cd8675c17646b4ef57d70`.
The 0.37 scroll query preserves source-dependent elastic consumption as
`Unknown` rather than publishing known-empty. Offscreen screenshots preserve
their callback path without falsely reporting surface presentation.

The public core acknowledgement now cross-checks the actual committed scene's
accepted exact emission instead of echoing a host `Presented` result. Retired
surfaces and invalidated scenes settle without promotion. Valid proofs waiting
behind a multi-surface popup barrier remain retainable until the sibling surface
presents; their receiver bindings stay unavailable until the barrier completes.

Workspace correctness/suspicious Clippy gates, strict core/adapter rustdoc,
eight documentation compile-fail tests, and formatting across the root and
independent workspaces pass. Real-window and physical X11 scenarios were not
rerun on this macOS host; platform claims require their separate CI evidence.

U14 removes required mutations from six `debug_assert!` expressions and promotes
native receivers immediately after successful core commit. The old shared
acknowledgement queue is deleted; idle hosts no longer depend on follow-up input
to consume a committed presentation. A regression test failed before this fix
and passes afterward:
`receiver::tests::committed_host_frame_promotes_receivers_without_follow_up_input`.
Before upgrading the native fork, the repaired 0.36 baseline passed all 180
native workspace tests and all 173 native library tests with debug assertions
disabled. CI now carries the assertion-disabled gate.

The final review also required an exact-emission regression inside
`receiver::tests::exact_hit_without_hover_lane_is_unclaimed_while_uncertain_hits_fail_closed`.
The test now emits the same semantic scene twice in one session with identical
binding and coordinates, presents the newer output, and verifies that its native
query accepts only the newer artifact. Temporarily removing the emission-key
comparison made that test fail at the old-artifact rejection; restoring the
comparison passes. This distinguishes exact output identity from semantic scene
equality.

One non-blocking coverage recommendation remains: exercise managed-host renderer
termination through `prepare_output_prefix`, abandon the subsequent host frame,
then retry and verify exact promotion and one boundary release. Current core
failed-commit/retry, idle ACK, and atomic receiver tests cover its component
contracts, but are not that complete coordinator path. No production defect was
confirmed in this path by the review.

The public recording consumer does not register OS windows or a pointer
provider. Its same-coordinate surface tests use prepared tab actions; they
do not prove ordered multi-surface pointer routing or the complete 60-case
gesture matrix. The actual public UI event pipeline remains U3 work. Complete
program acceptance is explicitly documented as separate from GPU submission
and final presentation; core accepts the exact reported prefix in a later
successful host-frame commit.

### Remaining U3 replacement boundary

The official adapter now preserves complete execution receipts and uses the same
exact `PaintedSurfaceOutput` present/drop path as public engine and native hosts.
Both public preview-only ACK methods and the redundant renderer-pass bridge are
deleted. Caller-owned egui runs return `DockspaceOutput`; the actual renderer
owner takes the complete output and reports its terminal result exactly once.
No-report, receipt drop, output mismatch, discarded predecessor, foreign lane,
unwind, delayed report and pane-focus eligibility are covered by real emissions.

The official integrity check brackets the complete surface span and compares all
ordered shapes, clip rectangles, post-tessellation transforms and pixel scale
after output hooks. Its explicit admission limit is synchronous, append-only pane
painting on the supplied layer. Cross-layer writes are unavailable rather than
silently omitted. Application callbacks must honor source-gap opacity and clip;
the adapter does not inspect their renderer internals. Once output ownership is
split, the host must preserve the complete output it reports. Shape membership
never proves GPU submission or physical presentation.

U3 still replaces the independent local-response and journal terminal reducers,
supports ordered logical-surface routing, and removes the known-invalid Burst
exclusion. U2 output integration does not claim those later gates.

## 2026-10-27 audit baseline (before repairs)

The audit checked `3216855` plus the existing 26 uncommitted U2b files. These
results describe that worktree, not a clean release or the planned egui upgrade.
All Cargo builds used `CARGO_BUILD_JOBS=1`; Nextest used `--test-threads=1`.

| Gate | Fresh result | Limit |
| --- | --- | --- |
| Workspace, all features and targets | 1,563 passed | Does not include the excluded native workspace |
| Product harness, all features | 90 passed, 1 ignored | Full Burst matrix remains excluded |
| Ignored L03 complete matrix, explicitly selected | Failed | First Center/Burst sample `(280, 280)` has no magnetic preview after eight causal observations |
| Official consumer harness | 9 passed | No real-window run was performed in this audit |
| Native workspace, default test profile | 179 passed | Debug assertions enabled |
| Native library, debug assertions disabled | 164 passed, 8 failed | 172 library tests; does not include the other native test binaries |
| Workspace and native formatting checks | Failed | Existing U2b formatting differences remain |
| `git diff --check` | Passed | Patch whitespace only |

The native assertion-disabled run used:

```text
CARGO_BUILD_JOBS=1 cargo nextest run --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --lib --locked --test-threads=1 --no-fail-fast --config 'profile.test.package.egui_dockspace_native.debug-assertions=false'
```

Required mutations in `NativeCoordinator::prepare_output_prefix`,
`HostRecords::reserve_output`, and `HostRecords::settle_viewport_roster` occur
inside `debug_assert!` arguments and disappear when assertions are disabled.
The failures cover six ingress/create/input lifecycle cases and two terminal
roster settlement/rearm cases. U14 owns this correction and its configuration gate.

The current U2b receiver path already waits for a successful core presentation
commit before promoting an artifact. That is stronger than the older immediate
physical-callback promotion. Remaining work includes idle ACK draining,
consistent commit exits, and removal of local preview-only ACK authority;
passing default tests do not establish those deletion gates.

No physical OS-input, multi-monitor, GPU submission, or current-release package
validation was rerun in this audit. The selected egui 0.37.0 migration and the
new public engine-host harness remain implementation work under U15 and U13.

## Historical U1 journey directory (superseded by dated qualifications above)

| Journey | Semantic source / basin | Preview | Outcome / ownership | Focus / quiescence | Cadences | Executors and current status |
| --- | --- | --- | --- | --- | --- | --- |
| L01 | Tab body; below-threshold select or threshold-crossing drag | None before selection | Select without drag; source remains owner | Selected tab; local quiescence | Burst, Stepwise | `core-host` and `egui-product-headless` characterize selection; exact human threshold remains product evidence |
| L02 | Tab body; semantic tab gap before/after a sibling | Visible insertion candidate | Reorder once; local target owns the tab | Moved tab; local quiescence | Burst, Stepwise, ReleaseWithFinalMove, DiscardBeforeRelease | Both existing shared product/core paths cover the local command spine; product drag evidence is normative, pass-count details are not |
| L03 | Tab body; pane center plus north/south/east/west basins | Magnetic candidate, independent of guide targeting | Center merge or edge split; target owns payload | Moved tab; local quiescence | Burst, Stepwise, Hesitant, ReleaseWithFinalMove | `core-host` and `egui-product-headless` pass all 60 finite cases. A crate-private renderer receipt proves each product point remains outside every registered guide receiver; the black-box Burst executor also publishes the preview from the same raw packet. |
| L04 | Contained title/edge; surface free space and eligible pane basin | Full contained-window projection | Move/resize/re-dock commits once; contained or target owner | Moved item; local quiescence | Stepwise, Hesitant, ReleaseWithFinalMove | Durable placement and ownership are covered; full-window projection remains an intentional known-invalid baseline |
| L05 | Tab body; current basin can become stale or disappear | Candidate is cleared while source stays recoverable | Cancel/reject and restore source without partial mutation | Preserve source focus; local quiescence | Stepwise, TargetDisappears | `core-host` stale-target and target-destruction fixtures provide the current normative evidence |
| L06 | Tab body; center/edge basin with release barrier | Exact candidate must be presented before release commits | Commit presented candidate or restore source | Moved item or preserved source; local quiescence | Burst, Hesitant, ReleaseWithFinalMove, DiscardBeforeRelease | Discarded-preview rejection is normative; release-wait and ordinary fixed-hover tests remain characterization or known-invalid until causal readiness replaces fixed frames |
| L07 | Group handle, contained title, menu, and tab close; no overlapping owner | Group/command preview only where applicable | One semantic owner executes each chrome action | Close policy chooses successor; local quiescence | Burst, Stepwise | Close-region and group-drag tests characterize the current ownership boundary |
| L08 | Splitter, tab, close, keyboard, AccessKit, and touch sources; semantic command target | Splitter proposal where applicable | Shared prepared command commits layout action | No focus guessed from dwell; local quiescence | Stepwise, TouchLongPress | Existing shared local scenario runs through `core-host` and `egui-product-headless`; touch long-press has a typed capability gap |
| N01 | Contained/native title; outside-all release | Native pending while source remains recoverable | Create → first-live → redock → retirement | No inferred pane focus; native lifecycle quiescence | Stepwise | Core-host managed lifecycle passes; official/product hosts remain capability-negative without native facts |
| P01 | Tab body in one window; center/edge basin in another window | Completed-pass hover candidate | Physical redock under one qualified handoff | Moved item; native lifecycle quiescence | Burst, Hesitant, ReleaseWithFinalMove | X11 physical smoke is characterization evidence; other hosts fail closed until their physical gates pass |

The current shared local journeys are the finite L03
`MagneticDockingBasins` matrix and the existing L08
`LocalTabCloseSplitterAndContained` scenario. L03 expands one source, five
basins, three normalized non-guide samples per basin, and four cadences without
becoming a general GUI language. Both executors emit normalized
`ItemDocked`/`Quiescent` observations; product readiness uses a causal predicate
with a diagnostic upper bound.

- `crates/dockspace_host_conformance/src/tests.rs::shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics`
- `crates/egui_dockspace/src/product_dockspace_tests.rs::shared_l03_supported_points_use_renderer_registered_magnetic_receivers`
- `integration/egui-product-harness/tests/product.rs::shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics` (fresh 60-case Burst-inclusive gate; 1/1 focused and 114/114 complete harness)
- `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`
- `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics`

N01 uses the existing lifecycle scenario and remains a separate capability
boundary. P01 is never inferred from a local or official-egui run.

## Historical U2a ordered SurfaceProgram evidence

The ordered traversal slice is verified independently from the later atomic
presented-artifact and host-terminal-settlement work. Core, product-headless,
and fork-backed native tests now consume `SurfacePaintPlan::visit_surface_program`
for root operations, popups, previews, guides, source-gap disposition, and
feedback. Surface-wide flat plan iterators are crate-private or test-only, and
the deleted synthetic walker cannot recreate a missing operation from parallel
records. This evidence proves traversal membership and renderer order; it does
not claim `RendererPassBridge` promotion, final host settlement, or unwind
abandonment.

Current gates:

- `cargo nextest run -p dockspace --lib --locked -j1 --no-fail-fast`:
  1430/1430 passed on 2026-08-31.
- `cargo nextest run --manifest-path crates/dockspace_host_conformance/Cargo.toml --locked -j1`:
  37/37 passed on 2026-08-31.
- `cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --locked -j1`:
  86/86 passed with one explicitly skipped test on 2026-08-31.
- `cargo nextest run --manifest-path crates/egui_dockspace_native/Cargo.toml --locked -j1`:
  178/178 passed on 2026-08-31.
- `cargo check -p egui_dockspace --all-targets --locked -j1`: passed.
- `cargo check --manifest-path integration/egui-fork-workspace/Cargo.toml --workspace --all-targets --locked -j1`:
  passed.

The remaining atomic artifact, durable discarded-pass action queue, official
host finalizer, and native unwind/retirement paths stay deferred to the next
settlement slice.

## Historical U1 oracle matrix

Each row is classified as `normative`, `characterization-only`, or
`known-invalid`. Only normative rows may gate U1 completion. A source is listed
only when the named test or function exists in the repository; an intentional
gap is written as such instead of being given a synthetic test name.

| Row | Journey / concern | Classification | Existing evidence or explicit gap | Decision |
| --- | --- | --- | --- | --- |
| U1-01 | L01 action: click selects a tab | normative | `integration/egui-product-harness/tests/product.rs::default_features_click_selects_a_tab` | Preserve selection without drag |
| U1-02 | L01 retained frame: drag decoration after threshold | characterization-only | `integration/egui-product-harness/tests/product.rs::active_tab_drag_leaves_an_accessible_gap_and_paints_one_pointer_ghost` | Keep as visual baseline; fixed deltas do not gate behavior |
| U1-03 | L02 action: reorder commits once | normative | `integration/egui-product-harness/tests/product.rs::default_features_tab_drag_reorders_across_multipass_discard` | Preserve item order and ownership |
| U1-04 | L03 presentation: outer guide targeting | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Visual guide coverage only; exact guide targeting is not docking acceptance evidence |
| U1-05 | L03 presentation: center guide hit | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_center_guide_merges_tabs` | Do not promote guide aiming to the product contract |
| U1-06 | L04 action: durable contained move/resize/redock | normative | `integration/egui-product-harness/tests/product.rs::contained_move_resize_select_focus_and_dock_back_preserve_exact_ownership` | Durable placement changes only at terminal commit |
| U1-07 | L04 presentation: full-window contained drag preview shell | characterization-only | `integration/egui-product-harness/tests/product.rs::contained_title_drag_paints_a_full_window_preview_in_the_same_run` | The window-sized shell is present; live pane-content projection remains a later U4 gate |
| U1-08 | L05 action: stale target has no partial mutation | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_rejects_a_stale_target_without_partial_commit` | Keep fail-closed stale rejection |
| U1-09 | L05 presentation: target loss preserves source drag | normative | `crates/dockspace_host_conformance/src/tests.rs::private_multi_surface_fixture_target_destruction_preserves_the_source_drag` | Clear only the lost target route |
| U1-10 | L06 retained frame: discarded preview cannot release | normative | `integration/egui-product-harness/tests/product.rs::default_features_discarded_preview_cannot_release_a_drag` | Release needs current presented evidence |
| U1-11 | L06 presentation: release waits for exact preview | characterization-only | `integration/egui-product-harness/tests/product.rs::default_features_release_waits_for_the_new_preview_to_be_painted` | Retain the regression baseline, but its exact coordinates and fixed preparation/settlement frames are not causal preview-readiness proof |
| U1-12 | L06 retained frame: ordinary readiness via fixed hover loop | known-invalid | `integration/egui-product-harness/tests/product.rs::default_features_paint_outer_guides_and_dock_each_edge` | Replace fixed ordinary waits with causal predicates |
| U1-13 | L07 action: close receiver stays disjoint from tab receiver | normative | `integration/egui-product-harness/tests/product.rs::tab_close_semantics_follow_the_exact_close_region_occlusion` | One close owner, no action stealing |
| U1-14 | L08 action: shared local observations are executor-parallel | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_local_interaction_scenario_maps_to_core_host_semantics`; `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` | Keep the shared scenario narrow and normalized |
| U1-15 | L08 action: touch long-press | known-invalid | No existing executor test; `ScenarioCapability::TouchLongPress` remains unsupported | Preserve typed capability-negative evidence |
| U1-16 | N01 first-live ordering | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Record current causal ordering; do not infer an ownership verdict here |
| U1-17 | N01 retirement | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Preserve as lifecycle evidence until runtime replacement requalifies it |
| U1-18 | N01 quiescence | characterization-only | `crates/dockspace_host_conformance/src/tests.rs::native_lifecycle_scenario_reports_terminality_presentation_first_live_and_quiescence` | Measure bounded cleanup, not callback count |
| U1-19 | P01 physical drag barriers | characterization-only | `integration/egui-native-smoke/src/bin/physical_x11/driver/scenario.rs::perform_drag_out`; `::move_to_center_guide_while_pressed`; `::native_drag_preview_barrier` | X11 is evidence to requalify, not a portable authority |
| U1-20 | P01 missing native facts | known-invalid | `integration/egui-product-harness/tests/product.rs::shared_local_interaction_scenario_maps_to_egui_product_headless_semantics` (admission assertion only) | No executor performs the physical gesture, reports the exact missing native facts, and proves source ownership unchanged; keep the gap explicit |
| U1-21 | L03 ordinary magnetic basin matrix | normative | `crates/dockspace_host_conformance/src/tests.rs::shared_l03_magnetic_docking_scenario_maps_to_core_host_semantics`; `crates/egui_dockspace/src/product_dockspace_tests.rs::shared_l03_supported_points_use_renderer_registered_magnetic_receivers`; `integration/egui-product-harness/tests/product.rs::shared_l03_supported_magnetic_docking_maps_to_egui_product_semantics` | Core-host proves 5 basins × 3 normalized non-guide samples × 4 cadences; the crate-private product receipt proves the 45 supported product cases remain outside every renderer-registered guide receiver; the black-box product executor proves preview, true same-call ReleaseWithFinalMove, normalized docking, and stable quiescence |
| U1-22 | L03 single-call product Burst | normative | `integration/egui-product-harness/tests/product.rs::shared_l03_complete_magnetic_docking_matrix_maps_to_egui_product_semantics`; fresh evidence `/tmp/dockspace-l03-burst-qualification.json` | Every basin/sample/cadence publishes the magnetic preview from the same raw packet and commits one terminal docking mutation; no ignored gate or compensating input frame |

The N01 rows deliberately do **not** classify “native ownership before
first-live” as known-invalid. The existing fixture demonstrates a sequence,
but this U1 baseline does not promote that observation to an ownership verdict.
That comparison belongs to the later lifecycle replacement unit.

## Historical U1 capability boundaries

| Executor | L01–L08 | N01 | P01 |
| --- | --- | --- | --- |
| `core-host` | L03 full 60-case matrix and L08 shared command scenario pass | Programmatic lifecycle scenario passes | Unsupported without `NativePhysicalAuthority` |
| `egui-product-headless` | L03 complete 60-case matrix, including same-packet Burst, and L08 shared command scenario pass | Capability-negative | Capability-negative |
| `egui-official-harness` | Public local facade; the caller-owned demo supports exact terminal-output settlement, while unmodified eframe examples remain capability-negative | Capability-negative | Capability-negative |
| `native-coordinator-trace` / `egui-native-smoke` | Adapter-specific evidence | Programmatic lifecycle evidence | Physical X11 evidence only |

Capability-negative is a successful result when the exact missing fact and
unchanged source ownership are reported. No executor may substitute a rectangle
scan, final hover, fixed guide center, fixed ordinary frame count, receipt, or
internal scene/graph identity for a missing fact.

## Historical U1 verification notes

- Shared scenarios contain finite semantic sources, basins, normalized inset
  samples, cadences, selectors, and expected observations only. Coordinates,
  graph/scene identifiers, receipts, and frame counts stay in private executor
  code or core geometry tests.
- Ordinary product readiness must use a causal predicate with a bounded
  diagnostic timeout. Fixed pass counts remain valid only when discard/terminal
  pass count is itself the contract.
- The remaining contained-content projection, overshoot/release-hysteresis,
  product Burst, and touch capability gaps are retained as explicit evidence. The window-sized
  preview shell is now covered separately and is not treated as full content
  projection until U4.
- The lightweight native example is a discoverability checkpoint. The complete
  game-editor showcase and layout/preset shell remain assigned to U10.
