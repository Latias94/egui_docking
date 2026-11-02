# Dockspace completion audit

**Subsequent review (2026-11-02):** this document records the earlier acceptance
checkpoint. A new [ImGui comparison](audits/2026-11-02-dockspace-imgui-review.md)
demonstrates nine public-path counterexamples outside that matrix. The first
remediation tranche has passed its documented qualification; the complete
[follow-up plan](plans/2026-11-02-dockspace-semantic-state-refactor.md) remains active.
The completion statement below describes the prior Goal checkpoint and is not
current correctness approval of those newly exercised behaviors.

This audit applies the requirements and exit criteria of the
[canonical plan](plans/2026-08-21-1357-refactor-dockspace-direct-manipulation-experience-plan.md).
It does not replace or narrow that plan. The current implementation is
`4f57bfb`, with CI delivery follow-ups `785e51b`, `1611316`, `996fd4c` and
`542294b`.
The earlier `b6185b5` baseline and test-only Burst qualification retain their
historical source identity below.
Open-GPUI, an actual game engine and unenabled physical platform claims retain
the plan's existing scope boundaries.

The overall Goal is **complete within the canonical plan's declared profiles**.
The exact native fork revisions are published and independently fetched from
GitHub; clean-checkout/CI-equivalent reproduction passes, including the final
corrected CI module/list and five real X11 window journeys. The plan's
default Shift suppression gap is now implemented and qualified through the
renderer-neutral host, official adapter, native model and finite X11 reference.
The default native entrypoint and the stronger first-packet Burst check pass their
audit follow-up runs; their earlier all-features or weaker results are not used
as substitutes for those commands.

The current production follow-up is scoped separately in
[VALIDATION.md](VALIDATION.md#2026-11-01-ordered-magnetic-suppression-qualification).
It adds ordered semantic targeting intent to core and both adapters, preserves
contained geometry and exact pending-release decisions, and has passing public,
product and native focused traces plus a real X11 Shift down/up/reacquisition
journey. Historical tree/package identity below applies to the prior baseline;
it is not a byte-match claim for this follow-up. All23 fresh library/tool gates,
finite window follow-ups and six matched-package gates pass. The two ignored
Linux platform-test oracles are refreshed separately without another production
change. Packages are checked as a matched unpublished pair; this does not claim
that the older cached registry core already exposes the new API.

## Evidence identity and scope

At the audit baseline, the complete Git trees for `crates/dockspace`, `crates/dockspace_host_conformance`,
`crates/egui_dockspace` and the three public/product/official consumers match
qualified core commit `677f291`. The subsequent audit changes only the product
test's Burst fallback. Native production logic remains `2dfe390`; later source
changes are smoke diagnostics and a public document-restore documentation fix.
All 274 core and 36 official-adapter packaged Rust files, plus both READMEs,
were compared byte-for-byte against that baseline. Archive hashes match
their qualification record. These checks are recorded in
`/tmp/dockspace-completion-audit-provenance.json`.

| Evidence | Actual result and boundary |
| --- | --- |
| Current ordered magnetic suppression | Workspace1958, public163/serde191, product112/serde117 and official15 pass. Native default539, macOS all-targets587, macOS noassert539 and Linux normal/noassert539 pass, each including its10,000-lifetime soak. Tooling, finite window pressure/stationary/input regressions, real Shift normal/noassert and matched extracted-package consumers pass. Frozen source, commands and resolved attempts are recorded by the `dockspace-magnetic-*` receipts in VALIDATION.md. The275/36 packaged Rust files and both READMEs byte-match current source. |
| Remote-derived delivery | The same500 frozen paths match both main and clean checkout. Independent initially empty Cargo Git caches retrieve the published pair directly from GitHub, and all12 Git-family packages agree. macOS default539/default-feature noassert528 and Linux default539/default-feature noassert528 pass with their10,000-lifetime soaks. Fork egui113, host-seam54, Glow5, WGPU check/retained1, Winit macOS8/X11 80, smoke68, final seven CI/list/module gates and five real X11 journeys pass. Explicit skips/ignores retain their actual counts in the final delivery record. |
| Core capture gates | Workspace 1,953 and public default 160/serde 188 pass at the matching core/consumer trees. `dockspace-capture-core-gates.json` and individual logs identify commands and test names. |
| Product/official follow-up | Product 114, official 15, ten doctests, dependency isolation, correctness/suspicious Clippy and strict docs pass in `dockspace-capture-followup-qualification.json`. The stronger Burst first-packet condition passes its separate follow-up below. |
| Extracted package consumers | Both archives are Cargo-verified. Extracted public default 160/serde 188, official 15 and the engine example pass; framework dependencies remain registry-sourced. `dockspace-capture-package-qualification.json`. |
| Aligned native/WM reference | All 36 final gates pass on egui `56e3db49b` / Winit `2664589f`, including macOS workspace 575, native assertion-disabled libraries 529 on macOS/Linux, same-coordinator 10,000-lifetime soak, programmatic windows, pressure and finite physical journeys. `dockspace-wm-stationary-final-qualification.json`. |
| Stationary overflow | All 19 final gates pass on the three frozen smoke files committed as `b35b8a1`. Two actual steps and real-exit stop each pass twice with native assertions enabled and once disabled. `dockspace-stationary-overflow-qualified.json`. |
| Documentation close | Strict native rustdoc, formatting and diff pass after the production documentation-only change. `dockspace-stationary-overflow-doc-close.json`. |
| Default native entrypoint | The exact `python3.13 scripts/run_egui_fork_harness.py` entrypoint passes 529 tests, including the same-coordinator 10,000-lifetime soak, with one opt-in measurement skipped. `dockspace-completion-native-default.json`. |
| Strict Burst first packet | Three affected L03 tests pass with default features and again with serde: supported cadences, the complete 60-case matrix and ordinary-content widget matrices. The Burst preview helper must succeed from its initial complete raw packet; requesting another frame now panics. `dockspace-completion-strict-burst.json`. |

Evidence filenames above refer to local files under `/tmp`, not portable
checkout assets. The test sources and commands below are the reproducible
oracles. Expected mutation-negative failures and resolved earlier attempts
remain labelled separately in [VALIDATION.md](VALIDATION.md); they are not
counted as passing ordinary runs or silently discarded.

## Requirements R1–R25

“Qualified” means the stated public contract or enabled finite profile, not
universal OS, GPU-scanout or arbitrary-load certification. A recording renderer
proves the public host contract; actual platform claims use their own window
and physical evidence.

| Requirement | Current authority and concrete oracle | Audit status |
| --- | --- | --- |
| R1: tab arming and ordered boundaries | `engine/pointer_transaction.rs` owns press/move reduction. `pointer_host_frame::surface_local_tab_exact_threshold_move_begins_drag` and `public_contract::host_pointer_same_batch_release_waits_for_the_actual_target_preview` pass. Product L03 covers all basins/cadences; the stricter first-packet assertion passes default and serde. | Qualified. |
| R2: common manipulation semantics | `deliver_drag_release` feeds the shared prepared transaction. `product_actions::docking_a_complete_root_preserves_group_order_and_selection`, product reorder and the complete L03 layout matrix pass. | Qualified. |
| R3: core magnetic priority and latch | `drop_resolver` and `runtime/direct_manipulation` own acquisition. `rejected_front_candidate_never_falls_through_its_occlusion`, deterministic structural ties and `magnetic_edge_latch_survives_small_center_overshoot_and_release` pass. Ordered suppression has full public/product/native and real X11 normal/noassert evidence in the current follow-up. | Qualified within the stated host and finite reference profiles. |
| R4: exact-output release barrier | `finish_drag_release_after_evaluation` and pending-release settlement preserve the latch. Public same-batch Presented/Dropped tests and `move_then_release_with_new_preview_waits_for_exact_presentation` check one durable mutation. | Qualified. |
| R5: complete contained projection | `engine/contained_projection` and the common scene compiler move pane/hit/semantic geometry together. Public `host_local_all_eight_resize_directions_recompile_both_panes_and_receivers` and product live content/ID/bounds tests pass. | Qualified. |
| R6: fact-qualified native promotion | `native_tear_off` validates the chosen lane. Unknown-work-area and capability-loss tests preserve source with typed outcomes; real X11 journeys cover first-live/redock/retirement. | Qualified within declared capabilities. |
| R7: common commands and distinct chrome | `runtime/session` preparation and `DockspaceHostFrame` submission own semantic actions. Public group-voting/deferred close and product protected-close-versus-Dock-Back tests pass. | Qualified. |
| R8: typed feedback and focus intent | `runtime/manipulation_feedback` keeps stable manipulation identity and distinct publication. Public pending-terminal/no-replay and product Escape/context-retirement-without-focus-theft tests pass. | Qualified. |
| R9: one gesture owner and terminal path | Private `ActiveGesture`/`GestureOwner` and `cancel_gesture_owner` own lifetime and cancellation. Exactly-once cancellation, pending policy cancellation and late-output inertness pass. Old `LocalResponse` owner, phase API and preview-only ACK reducer are deleted. | Qualified. |
| R10: one hierarchical program | `scene_compiler/surface` and `execute_surface_program` preserve root/chrome/pane/contained/guide order. Tests remove a program operation while retaining flat data and confirm hit/semantic membership cannot resurrect it. | Qualified. |
| R11: emission versus presentation | Core presentation reduction and native `commit_presentations` promote exact artifacts only after commit. Public failed-core-commit, renderer-terminal-before-core-commit and output-hook-unwind tests pass. | Qualified; no scanout claim. |
| R12: exact host hover/blocker port | `SurfacePointerReceiverFacts` and native receiver resolution bind artifact, stream, pass and identity. Actual button/editor/scroll/canvas points and popup/modal/overlay/front-contained/stale-hover tests pass. | Qualified at each documented adapter boundary. |
| R13: bounded causal input | `runtime/native/session`, `NativeInputBudget` and adapter admission preserve prefix identity, original age and protected terminals. Budgeted-host, elapsed-service, overload retry and accepted-release-under-pressure tests pass; resource scope is below. | Qualified configured profiles, including the default entrypoint. |
| R14: explicit local/global scope and handoff | `HostPointerRoute` and `NativePointerLocation` keep source/delivery separate from hover. Public size-only local release and exact local-to-desktop handoff retry tests pass. | Qualified host contract; unclaimed OS profiles remain unqualified. |
| R15: incarnation-safe first-live | DirectPrepared/Staged share `engine/native_projection` publication. Public first Created/Prepared, unrelated-root-edit, revoked readiness, cancellation and late-output tests preserve durable source until exact first-live. | Qualified local contract and finite native profile. |
| R16: application and engine-host facades | `DockspaceSession`/`DockspaceHostFrame` expose documented measurement, input, program, disposition and effects. Extracted `engine_host` runs without pointer registration or egui; application facades hide this plumbing. | Qualified. |
| R17: shared prepared commands | Pointer, keyboard and AccessKit use prepared surface actions. Product SourceGap identity, disabled delayed actions, occluded controls and contained cardinal-resize keyboard/AccessKit tests pass; dwell does not imply pane focus. | Qualified. |
| R18: independent semantic journeys | Public and product executors run actual commands. Native tests separately record model, real-window and physical evidence, including exact destruction and quiescence. Catalog metadata alone is not counted. | Qualified finite claims. |
| R19: framework-neutral core | `scripts/check_core_dependencies.py` checks normal/build dependencies across all features/targets; no egui/eframe/AccessKit/Winit/WGPU/GPUI enters core. Extracted consumers compile default and serde. | Qualified. |
| R20: logical surfaces and coordinate domains | Public independent-surface output tests and `window_dpi_and_framebuffer_density_are_independent_for_cross_surface_batches` run eight immediate/deferred/DPI/density combinations without native enrollment. | Qualified; not physical mixed-DPI certification. |
| R21: owned deferred output | Public owned program survives frame borrow; reversed results and retired/redocked surfaces cannot revive old receivers. All reporting still enters exact committed presentation. | Qualified. |
| R22: host-owned measurements and time | Fixed/cached measurement APIs, explicit metadata and injected clocks need no widget tree or device. Stationary retry and same-frame contained measurement tests preserve one compile and one continuation. | Qualified. |
| R23: current egui and minimal seams | Official matched `=0.37.0`, aligned published fork pins and seam-removal conditions are documented. Official callback capability-negative, native normal/noassert and remote-derived CI-equivalent checks pass. egui_tiles is reference-only; Open-GPUI is deferred. | Qualified, including clean-checkout delivery. |
| R24: portability and structural cost | `owned_output_cost` tests 16/128/1024 panes after releasing frame borrow. Contained batch and selection cost tests count one compile and bounded pane visits at all three sizes. Retained recording bytes are scoped separately from native ingress. | Qualified. |
| R25: complete representative layout shell | Public closed-catalog reopen, lock/reset/restore focus, recognized-missing recovery and native Default/preset Root-continuation workflows pass. Manual overflow, exact stationary 0/40/80 and exit-stop 40 retain original capture and finish cleanup. | Qualified finite local/X11 shell. |

The corresponding source/test roots are [core](../crates/dockspace/src/),
[core integration tests](../crates/dockspace/tests/),
[public consumer](../integration/dockspace-host-harness/tests/),
[product consumer](../integration/egui-product-harness/tests/),
[official consumer](../integration/egui-official-harness/tests/) and
[native coordinator](../crates/egui_dockspace_native/src/coordinator/tests/).

## Acceptance examples and implementation units

| Plan examples | Matching requirement evidence |
| --- | --- |
| AE1 / AE1b | R1–R4/R12: ordinary content matrices and missing output/hover capability; strict Burst first-packet follow-up passes default and serde. |
| AE2 / AE3 | R3/R5: hysteresis, complete contained subtree and all eight directions. |
| AE4 / AE5 | R4/R11/R21: same-batch release, exact terminal, discard/unwind, supersession and rollback. |
| AE6 / AE7 | R6/R13/R15: create/replacement/late predecessor, bounded prefix/overload and real load. |
| AE8 / AE9 / AE10 | R7–R9/R17/R25: distinct close/Dock Back, keyboard/AccessKit/touch arming, layout lock and stable focus/feedback. |
| AE11 | R6/R14/R18: explicit unavailable platform facts with usable local behavior. |
| AE12 / AE13 / AE14 | R16/R19–R24: public framework-neutral consumer, owned deferred records and independent logical surfaces/DPI/density. |

U1–U6 and U12–U13 map to the public/core/product rows above, including U3's
deletion and full Burst requirements. U14/U15 add default/noassert behavior,
official0.37 migration and admitted native seams. U7/U8 add bounded ingress,
scoped/handoff contracts, exact native lifecycle and the enabled X11 profile.
U10 adds the tested layout shell and overflow ownership. U11's unenabled
Mac/Windows/Touch/deeper-ancestry claims are capability limits, not silently
passing platform tests. U9's exact delivery actions below are complete.

## Resource budgets and measurements

| Budget | Inspected implementation and actual evidence | Limit |
| --- | --- | --- |
| 4096 records / 4MiB / 250ms age | Raw admission and shared source reservations preserve captured age until full commit. Configured W16 terminal fanout, mixed close/create/release, combined owner-storage and overload tests pass; real center-load samples owned 2,358,993/reserved 3,513,528 bytes under 4 MiB. | Explicit finite profiles; excludes allocator, renderer and fixture data as documented. |
| 256 records / 2ms cooperative yield | `NativeInputBudget` checks inclusive limits at atomic boundaries. Core 257th-record continuation, native shared-budget/Repeat/close handling and elapsed-service tests pass. Actual overruns remain recorded. | A cooperative threshold, not a hard real-time or OS-yield bound. |
| Release under 100 ms at 1000 Hz | Frozen final X11 center load matches all 1000 callbacks at 999.9 Hz and measures release-to-model upper bound 37.376 ms. | Finite measured profile, excluding explicitly pending OS/renderer work. |
| At most64 terminal summaries / 1024 retained identity history entries | Published unreferenced terminal payload compacts to scalar frontiers/merged retirement ranges; no history ring is required. Published-effect compaction, one semantic watermark, exact quiescence and stale-candidate tests pass. | Live causal/cleanup guards remain charged until their actual proof releases them; cumulative IDs are not resident history counts. |
| 10,000 lifecycle cycles | The same coordinator is reused, drops/retries and late predecessors are exercised, obligations return to zero and idle owned/reserved capacity remains flat. Native normal/noassert runs include the soak. | Model lifetimes, not 10,000 physical OS windows. |

## Named gates, artifacts and remaining delivery

The plan's core, public consumer, shared journeys, product/official consumer,
dependency isolation, documentation, packaging and integrity commands have
matching actual receipts in the evidence sets above. The default native script
passes its exact entrypoint rather than being substituted with
`--all-features`. Its one skipped opt-in wall-time test is covered separately by
the dated release measurement; the default run does not relabel it as passed.
The existing example, persistence, conformance, platform and
fork-admission documents are present and refer to the same public contracts.
No new GUI DSL, source parser, lifecycle registry or docking graph is introduced.

The stricter Burst assertion proves the original packet's completed egui run,
which may contain multipass rendering. It does not require the first egui pass
to be final. Raw input is frozen once at the run boundary and cannot replay on
Repeat; outcome settlement after release supplies only empty input. The fixture
has no explicit raw time, so it also cannot inject a timed continuation as a
hidden replacement for the missing Move. Independent source review confirms
this exact distinction.

The default-profile audit matches the configured 6-point drag threshold,
0.52 center fraction, 0.15 outer band clamped 32–72, 8-point hysteresis, 150 ms hover,
350 ms touch/8-point slop, 44-point touch targets, 16-point splitter step and
24-point/50 ms edge scrolling. `baseline_geometry_is_valid_and_renderer_neutral`
and behavior tests exercise those values. The original audit found that scroll
modifier handling could not satisfy Shift suppression. The follow-up adds
optional targeting intent on real edges and an ordered modifier-only control,
invalidates magnetic latch authority without losing contained position, preserves
explicit targets and frozen releases, and requires fresh acquisition after
reenabling. Full fresh gates and the matched archive refresh now pass; the U4
default-profile suppression gap is closed.

The current ImGui reference is `d88f238d1d363ef387382b2e29ad12cf96f826ac`, matching
the plan. The dated 2026-11-01 refresh records 0.37.0 as the latest stable
[official egui release](https://github.com/emilk/egui/releases) at that checkpoint.
Current manifests and qualification use that matched baseline. Earlier research
checkpoints preserve their own source scope.

Following explicit user authorization, Winit
`2664589f0bd6fd03b2a5490f23bcd692853e63dd` and egui
`56e3db49b66096b58a23b8eb762bdc0a73ae043c` are published through ordinary
fast-forward pushes, and independent empty repositories fetch both exact SHAs
from GitHub. Commits `785e51b`, `1611316`, `996fd4c` and `542294b` align the
three CI refs, generate Winit's ignored source-test lockfile, install the Wayland
development dependency and select the actual `monitor::notify_mask_tests` module.
The separately owned workflow changes remain unstaged. Native clean-checkout/
CI-equivalent reproduction passes with initially empty Cargo Git caches, direct
remote retrieval and no local object seeding or dependency path overrides. The
corrected CI's 26 X11 and 7 macOS required names are listed and actually passed;
all four final X11 module commands pass. Programmatic redock, two real WM-close
policies, physical guide dragging and real Shift/reacquisition each pass in the
remote-derived X11 environment. Exact commands, source identities, earlier
resolved failures and ignored-test limits are in the
[final delivery record](VALIDATION.md#remote-fork-publication-and-clean-checkout-delivery).

The original CI file hash remains
`6bee9bedd379f71fa42b8cff9c027ce0491257de31b6baa54b895924327a1ff9`.
`docs/reviews/` remains outside this work's writes and commits. The two fork
pushes have explicit user authorization; the root branch is not pushed and no
PR is created. Removing only this session's four CI changes in memory reproduces
that original hash exactly. All R1–R25, acceptance examples, implementation units,
named gates, package boundaries and Definition of Done now have matching
evidence within the stated capability limits. No further core architecture
replacement is required by this audit.
