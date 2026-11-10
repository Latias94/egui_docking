# Dockspace architecture and design audit

Date: 2026-11-10 (Asia/Shanghai)  
Baseline: `2289c889b30343bb705d9b6786f09dc09d633619`  
Branch: `refactor/egui-pane-ergonomics`  
Method: three Codex reviewers, two review rounds, primary-agent cross-checks and narrow executable probes.

## Assessment

The canonical workspace, stable product identities and renderer-independent session are a sound foundation. This audit found no justification for replacing the N-ary forest, introducing another docking tree, or attributing the remaining performance costs to Rust ownership in general. There are specific ownership and derivation mistakes to fix first.

The largest measured avoidable cost is outside layout: an ordinary document-bound commit clones both complete external-item maps, including retained history. The most consequential correctness problem is also an ownership mistake: the native adapter retains its single focus dispatch owner until semantic focus succeeds, although the core already owns activation intent, observation and successor arbitration.

Three broader seams deserve redesign after the defects and duplicate work are removed: per-surface measurement retention, configuration application before encoding, and host-specific native preparation. These are deliberate current protocols with extension costs, not evidence that all of their checks are redundant.

| Priority | Count | Meaning in this report |
| --- | ---: | --- |
| P1 | 1 | Confirmed native focus liveness defect |
| P2 | 12 | One further behavior defect; measured or statically established duplicate work, interface coupling, and design or CI debt |
| P3 | 2 | Benchmark provenance and configuration-maintenance debt |
| Contract decision | 1 | Hidden-pane minimum availability; an explicit product tradeoff |

P2 design debt is distinguished from a broken feature below. Static algorithmic evidence does not establish a wall-clock regression or an ImGui performance ratio.

## Review lanes and coverage

| Lane | First round | Second round |
| --- | --- | --- |
| `arch_core` | Domain model, graph mutations, reducer, authority, replacement | Public host interface, focus access, configuration phases, measurement contract |
| `arch_render` | Measurement, compiler, paint program, exact output, egui input | Test and CI selection, content-widget scaling, benchmark provenance |
| `arch_native` | Create/first-live/retirement, callback ownership, shutdown | Focus successor verification, independent persistence attribution |
| Primary agent | Public consumers, persistence, documentation and dependencies | Cross-checks, counterexamples, probes, measurement and synthesis |

All reviewer lanes were read-only and ran no Cargo commands. The primary agent ran Cargo serially with `CARGO_BUILD_JOBS=1` and reused `target`. No product implementation changed. Temporary test-module declarations and the temporary harness example were removed with exact content/hash checks. Reproduction sources and results remain only in the [audit artifacts](2026-11-10-architecture-design-audit/).

The excluded review directory was not inspected. The reference checkout with unrelated edits was not inspected. The native reviewer used only a targeted read of the separate pinned egui focus fork to establish what `Requested` means; its revision matched `0d9c48d765954dd3eff99c0d3cfa686e9d8591cf`.

## Findings

### A01 — P1: native focus dispatch ownership can block successors and shutdown

**Confirmed behavior defect; coordinator-model reproduction passed.**

After an exact `Requested` callback, [focus_control.rs:283](../../crates/egui_dockspace_native/src/focus_control.rs#L283) moves the sole owner to `AwaitingGlobalFocus`. Only observation of that binding releases it through `take_observed`. A focus request can be dispatched successfully without the OS granting focus, for example under activation restrictions or user interference. Every subsequent focus effect actually emitted by the core then fails `retain` at [effect_dispatch.rs:382](../../crates/egui_dockspace_native/src/coordinator/effect_dispatch.rs#L382).

The core permits successor activation and separately tracks already emitted hazards. The probe first asserted that selection of B really emitted `RequestFocus(B)`, then showed that the adapter kept A as owner and did not dispatch B. This is not a claim that every Select necessarily emits an effect.

Fatal quarantine compounds the problem. [coordinator.rs:2985](../../crates/egui_dockspace_native/src/coordinator.rs#L2985) cancels only queued focus; [mailbox.rs:2773](../../crates/egui_dockspace_native/src/mailbox.rs#L2773) admits global-focus observations through ordinary admission, which quarantine closes. A live root with an already requested focus can therefore keep [coordinator quiescence](../../crates/egui_dockspace_native/src/coordinator.rs#L665), and consequently `is_shutdown_drained`, false. Four model shutdown turns preserved the owner without producing an error.

**Repair:** consume the adapter dispatch owner after the exact successful dispatch callback; leave semantic focus success, late-effect hazards and successor choice to core. For shutdown, explicitly terminate dispatched ownership as indeterminate where appropriate and drain its original exact callback reservation. Preserve token/window/binding matching and the distinction between not dispatched and outcome unknown. [NativeEffectRequest::accepted](../../crates/dockspace/src/runtime/native_effect.rs#L334) already distinguishes dispatch acceptance from completion by ordinary focus facts.

**Verification:** retain the model trace as a regression with corrected expectations; test denied focus followed by a new explicit target, delayed predecessor observations, and quarantine with the root still alive. Real window-manager refusal was not exercised in this audit. The test journal helper bypasses production admission and must not be used to claim that quarantine accepts or rejects actual global-focus callbacks.

### A02 — P2: an equal presentation configuration cancels a pending native rehome

**Confirmed behavior defect; control and failure paths reproduced.**

[engine.rs:3905](../../crates/dockspace/src/engine.rs#L3905) correctly reports `PresentationConfigReplaced { changed: false }` for an equal value. However, [engine/reducer.rs:1121](../../crates/dockspace/src/engine/reducer.rs#L1121) cancels pending release obligations and fails a pending presentation rehome based on the input variant, independently of the outcome.

The same native-to-contained operation remained pending and later reached `Applied` in the control. Adding replacement with the existing configuration produced `changed: false`, cleared pending presentation and settled the operation as `TargetNotPresented`. Content stayed on its source; no data loss was observed.

**Repair:** derive invalidation from the actual accepted change, preferably in the configuration reducer's changed branch or an explicit invalidation result. Do not remove invalidation for real geometry changes. This is a small reducer change plus a regression, not a new state machine.

**Verification:** equal replacement followed by exact presentation must still apply; real configuration change must invalidate old obligations; dropping a replacement frame must preserve committed obligations.

### A03 — P2: document-bound idle commits copy all external-item history

**Confirmed performance defect; allocation and local timing evidence available.**

Every ordinary serde-enabled document-bound commit calls `binding.prepare_publication` at [runtime/host_frame.rs:1031](../../crates/dockspace/src/runtime/host_frame.rs#L1031). [document.rs:396](../../crates/dockspace/src/document.rs#L396) only needs reconciled placement preferences but clones the entire binding. [ExternalItemKeyMap](../../crates/dockspace/src/external_item_key.rs#L24) owns two `BTreeMap`s containing `String`s. Both maps and their string payloads are deep-copied. The item scope is an Arc and is not the cause. Old placement preferences are also cloned and then overwritten by the separately prepared preferences.

The public probe held the layout at **one live pane** and varied registered keys. Unused bootstrap keys model the same retained map cost as closed historical panes; the probe did not open and close thousands of panes. No save, input or encoding occurred inside the measured frame.

| Registered/historical keys | Binding | Median ns/frame | Allocation events/frame | Requested bytes/frame |
| ---: | --- | ---: | ---: | ---: |
| 0 | Unbound control | 11,025 | 68 | 51,952 |
| 1 | Persistent | 11,293 | 74 | 53,032 |
| 128 | Persistent | 19,228 | 368 | 74,424 |
| 1,024 | Persistent | 68,487 | 2,460 | 232,056 |
| 16,384 | Persistent | 919,411 | 38,294 | 2,920,984 |

Timing used nine batches of 64 release-mode frames after warmup, with heap profiling inactive; a separate dhat sample counted allocations. These are local macOS arm64 observations, not latency guarantees. Requested bytes are cumulative allocations, not live or peak memory. There is no ImGui comparison in these results.

**Repair:** prepare only the fallible placement publication, then install that prepared field after successful engine commit. Keep generation, identity maps and scope in their existing owner. The restore path already owns its incoming binding and can reconcile it without cloning the whole binding again. A new persistent-map abstraction or sidecar revision system is unnecessary for this fix.

**Verification:** idle allocations must stop scaling with key history. Preserve prepare-before-publish, dropped/failed-frame rollback, order-independent recovery-host cleanup, staging/cleanup binding exceptions, monotonic save generation, and prepared/suspended restore rejection after a key or save-sidecar change. The document hash remains a valid cold save/load integrity check.

### A04 — P2: no-op selection replaces snapshot identity and breaks derived-state reuse

**Confirmed duplicate-work defect; allocation identity and structural counts reproduced.**

An already-selected item still passes through a transaction snapshot with a fresh Arc at [operation.rs:130](../../crates/dockspace/src/operation.rs#L130). [engine.rs:5841](../../crates/dockspace/src/engine.rs#L5841) publishes it even when the command reports no workspace change. The version remains unchanged. The provenance check at [presentation_authority.rs:121](../../crates/dockspace/src/engine/presentation_authority.rs#L121) correctly rejects the old allocation's index.

Rebuilding detects equal semantic values and returns early at [presentation_authority.rs:436](../../crates/dockspace/src/engine/presentation_authority.rs#L436), leaving the index source unmatched. With two roots, reselection built four root fingerprints. A subsequent Existing Open, which retained its workspace allocation, built zero fingerprints in the control and four after reselection. The mismatch therefore survives the initial command.

**Counterexample retained:** three truly input-free frames built zero fingerprints; they do not request a rebuild. This defect must not be described as an unconditional traversal on every idle frame.

**Repair:** preserve the original snapshot for a proven single-command semantic/authority no-op, or make the publication result explicitly describe unchanged ownership. Do not generalize from `report.changed() == false`: net-zero rehome transactions can still advance root issuance to prevent ABA.

**Verification:** index reuse after reselection and a later Existing Open, preserved pane-focus requests, unchanged workspace version, and the existing ABA/owner-reference tests.

### A05 — P2: semantic snapshots and the workspace index derive the same root data twice

**Confirmed duplicate derivation; initialization counter evidence plus cross-file tracing.**

[capture_surface_semantic_snapshots](../../crates/dockspace/src/engine/presentation_authority.rs#L782) calls `root_fingerprint` for each main/contained root. That function [builds an entire root index and keeps only its fingerprint](../../crates/dockspace/src/workspace.rs#L792). The same requirements rebuild subsequently calls [WorkspaceIndex::build](../../crates/dockspace/src/engine/presentation_authority.rs#L557), which repeats the root walk. Both immutable results are retained in Arc-backed representations, but their construction is not shared.

Each build allocates traversal state, member information and fingerprint node payloads at [workspace.rs:821](../../crates/dockspace/src/workspace.rs#L821). Initialization of the two-root fixture performed four root builds. A complete cross-root dock also showed multiple builds, but its six builds include command preparation; this report does not attribute all six to the duplicate pair.

**Repair:** derive a workspace index once, then use its existing root fingerprint payloads to construct surface snapshots. Use the same entry point at initialization and ordinary requirement rebuild. Share derivation only within the same exact workspace source and version; prospective native workspaces retain their separate provenance. Further sharing of graph node payloads is optional and should follow measurement.

**Verification:** one derivation per required root for this shared source, retained old-output scope checks, selection rebinding and unchanged-sibling behavior. A root generation alone cannot replace the existing scoped semantic snapshots.

### A06 — P2: adapter drafts deep-copy immutable receiver and focus tables

**Confirmed ownership/design debt; wall-clock and allocation slopes not measured.**

[PointerScene](../../crates/egui_dockspace/src/product_pointer.rs#L78) derives Clone over owned receiver and focus Vecs. Both ordinary update owners clone `ProductPointer`, including all pending and presented scenes, at [product_owned_run.rs:283](../../crates/egui_dockspace/src/product_owned_run.rs#L283) and [product_dockspace.rs:782](../../crates/egui_dockspace/src/product_dockspace.rs#L782). [sync_presented](../../crates/egui_dockspace/src/product_pointer.rs#L273) can clone the same table again while preserving the pending artifact until commit.

The tables are immutable after emission. Receiver count includes chrome plus every accepted ordinary content declaration at [product_render/tabs.rs:128](../../crates/egui_dockspace/src/product_render/tabs.rs#L128), including hover-only entries and duplicate declarations. It scales with content widgets and retained outputs, not just panes. Focus entries principally scale with selected pane focus targets.

**Repair:** share immutable table slices or complete PointerScene values. Continue isolating mutable pointer position, touch contacts, run-consumption markers and pending/presented ownership in each draft. No second scene registry is needed.

**Verification:** failed-draft rollback, reverse output settlement, duplicate declarations and exact artifact matching; measure draft-copy allocations as content registrations and retained outputs increase. Existing public owned-output tests measure a recording sink, not these adapter tables.

### A07 — P2: input clearance repeats a quadratic rectangle subtraction per event

**Confirmed algorithmic debt in the official host-local pointer path; no new timing claim.**

[product_pointer.rs:543](../../crates/egui_dockspace/src/product_pointer.rs#L543) creates the unknown-widget rectangle list per event, then matches each owned declaration with a linear search and `Vec::remove`. K valid, distinct widget IDs sharing one rectangle can cause K(K-1)/2 element moves just from removing the front, before accounting for the other joins. [The event mapping](../../crates/egui_dockspace/src/product_pointer.rs#L593) and stationary continuation repeat this work.

**Repair:** join the frozen framework snapshot with this exact scene's declarations once per input packet, then test remaining blockers by position for each event. Preserve the multiset rule: a declared widget cannot remove a different undeclared blocker at the same rectangle. Deduplicating rectangle values into a plain set is incorrect.

**Verification:** K=1/16/128 declared overlapping widgets with zero or one undeclared overlapping blocker; count join work and verify clearance. The existing [same-rectangle blocker test](../../integration/egui-product-harness/tests/product.rs#L5547) protects correctness but not the growth dimension. Current product cost examples return empty PaneResponse and settle immediately, so they do not isolate this dimension or A06.

### A08 — P2: bounded native input admission repeatedly scans the complete retained state under lock

**Confirmed algorithmic/design debt in the configured finite-storage profile; callback latency not measured.**

[on_window_event](../../crates/egui_dockspace_native/src/mailbox.rs#L6223) holds the native host lock while checking `can_capture_pointer_callback`. [capacity.rs:98](../../crates/egui_dockspace_native/src/mailbox/capacity.rs#L98) recomputes owned and reserved storage. This walks pointer payloads at line 69, pending pointer translations at line 122, and roster/owner state. After capture, [capture_window_event](../../crates/egui_dockspace_native/src/mailbox.rs#L3057) performs ordinary admission and pointer reservation checks again.

As a burst fills Q raw slots, repeated full-queue accounting produces quadratic cumulative scanning. It occurs before cooperative reducer budgeting and inside the callback lock. The pointer payload has a fixed known size, so that part of the recomputation is avoidable. This claim is scoped to finite-storage checks, not every default/unconfigured callback.

**Repair:** first avoid repeated accounting in the same locked admission. Then charge fixed backing once and maintain variable reservations through existing owner receive/transfer/release operations. Keep the complete traversal for explicit diagnostics and accounting cross-check tests; avoid a parallel queue or allocation-address registry.

**Verification:** occupancy-dependent admission work and lock time, plus equivalence against full accounting through partial commits, retry, terminal release, Destroy and ownership transfer. Preserve age/record bounds and prepaid terminal capacity.

### A09 — P2: querying pane focus reserves rendering capacity

**Confirmed public-interface coupling; both effects reproduced.**

The only public pane-focus request and observation preparation methods belong to SurfacePaintPlan at [runtime/paint.rs:1878](../../crates/dockspace/src/runtime/paint.rs#L1878). Acquiring that plan calls [surface_work](../../crates/dockspace/src/runtime/paint.rs#L1315), which performs output admission and inserts the surface into `encoding_surfaces`, even if no program is executed. The core already has a published focus-intent query; the public facade does not expose it independently.

The probe used capacity two, two retained surface streams and one in-flight B output. Reading A's focus request without encoding A consumed the last free output reservation and deferred B. With two B outputs in flight, A's new focus request could not be read at all. Capacity one was not a valid two-stream fixture because the configuration also bounds retained streams; the passing probe uses the actual two-stream contract.

**Repair:** expose an independently queryable published focus request and let that request prepare its observation. Preserve exact intent/domain/epoch/target/native-guard validation and do not expose an uncommitted new intent. Do not weaken genuine output admission before pane callbacks.

**Verification:** focus can progress while unrelated renderer outputs are retained; reading focus does not reserve an output slot; stale focus observations remain rejected.

### A10 — P2: adapters duplicate measurement-retention policy and apply a global gesture freeze

**Confirmed policy coupling/design debt; visible layout consequences not newly exercised.**

The official owned run, callback entry and native surface driver each combine global gesture state, preview state and retained-output state to decide whether to measure: [product_owned_run.rs:410](../../crates/egui_dockspace/src/product_owned_run.rs#L410), [product_dockspace.rs:846](../../crates/egui_dockspace/src/product_dockspace.rs#L846), [surface_driver.rs:892](../../crates/egui_dockspace_native/src/surface_driver.rs#L892). A gesture on A therefore stops title-intrinsic/minimum refresh on an otherwise Ready B when its own topology and coordinates have not changed.

**Repair:** first centralize the policy and report per-surface reasons; then consider having SurfaceWork carry measurement refresh permission. Core should own which geometry must stay exact for an active preview or native transfer. Framework-specific final-pass resource retention remains adapter-owned.

**Verification:** A remains in a gesture while B changes title/minimum metadata; B's resolver behavior and A's original authority must be tested together. Retain exact-preview, native first-live and output-capacity counterexamples. Simply deleting the current freeze is not a safe fix.

### A11 — P2: core native bringup includes a particular host preparation script

**Architectural debt; existing staged readiness checks protect real invariants.**

[NativeBringupPhase](../../crates/dockspace/src/frame/native_bringup.rs#L22) encodes hidden observation, pre-show presentation, show acknowledgement, visible observation and post-show presentation. [native_ready_proof_is_current](../../crates/dockspace/src/frame/native_bringup.rs#L293) validates that complete history. The adapter correspondingly maintains create/show/staging callback ownership across coordinator, effect handling and mailbox resources.

The core already supports [DirectPrepared](../../crates/dockspace/src/frame/native_bringup.rs#L372), so another host can avoid the staged script today. This is not an inability to integrate another engine. The design cost is maintaining host preparation steps in the docking core as well as the host, and two readiness protocols indefinitely.

**Repair options:** consolidate staged driving in one adapter module first; later evaluate one exact, revocable Prepared/readiness-generation seam. Move renderer/window preparation procedure into the host while keeping source reservation, binding lifetime, ready revocation, exact semantic first-live, atomic topology publication and compensation/retirement in core.

**Verification:** run the same core transfer scenarios through direct and hidden-then-prepared hosts, including readiness revocation, late outputs, cancellation and compensation. This is a separate high-risk redesign after simpler fixes, not a reason to delete the current proof chain piecemeal.

### A12 — P2: configuration applies after encoding and requires a second update

**Confirmed, documented protocol limitation; same-update probe passed.**

[runtime/host_frame.rs:797](../../crates/dockspace/src/runtime/host_frame.rs#L797) documents the phase. [engine/host_frame.rs:2534](../../crates/dockspace/src/engine/host_frame.rs#L2534) queues configuration without changing the candidate used for measurement. The final reducer processes contributions at [line 702](../../crates/dockspace/src/engine/reducer.rs#L702) before configuration at [line 778](../../crates/dockspace/src/engine/reducer.rs#L778).

Replacing tab height 28 with 44, then measuring and encoding in that frame, produced a 28-high bar; only after commit did the session expose 44. Hosts need another update to measure/encode the new configuration. This does not prove mixed 44-high core geometry with 28-high adapter controls in one output: both current encoding inputs are old. The proposed mixed-version defect was rejected during cross-checking.

**Repair direction:** interpret the accepted input prefix under its old facts/policy, close semantic input, apply queued configuration to the same private candidate, then measure/encode. Configuration arriving after encoding begins belongs to a later update or must explicitly restart this pass. A restarted encoding pass must retain the accepted input candidate and must not replay producer input or claim to roll back application callback side effects. This requires revisiting checkpoints and invalidation; moving only a getter is insufficient.

**Verification:** same-update new configuration and output geometry, old-prefix causal interpretation, repeat-pass stability, rollback, native readiness, and rejection of old-configuration output authority. Keep this phase redesign separate from the small A02 correctness repair.

### A13 — P2: existing native persistence regressions are absent from complete CI selection

**Confirmed CI-selection gap, not evidence that the covered functions are broken.**

The root workspace excludes native. The [native runner](../../scripts/run_egui_fork_harness.py#L16) and [no-debug-assertions run](../../.github/workflows/ci.yml#L362) do not enable serde. Native lib `--all-features` CI runs filter for [four real-window tests](../../.github/workflows/ci.yml#L433) or specific text-backpressure tests. They do not execute the serde-gated [layout persistence module](../../crates/egui_dockspace_native/src/surface_driver/application/layout_tests.rs#L357).

That module already contains root-rebinding atomicity, conflict recovery, command-slot release, incoming identity and invalid-document cases, plus service deadline/owner interactions. Editor smoke coverage does not replace those failure-path regressions.

**Repair:** enable serde for the existing full native lib gate or add its narrow persistence module to the existing serial runner. A new OS matrix is unnecessary. This audit ran the omitted narrow subset locally; see the validation receipt below.

### A14 — P3: product comparison does not reject changed measurement inputs

**Confirmed provenance gap; no evidence that prior results were actually contaminated.**

[compare_products.py:189](../../scripts/compare_products.py#L189) records source hashes, and line 229 records executable hashes. Later ABBA runs reopen the shared target executable; final success does not recheck either source or binary. Concurrent editing/building can therefore mix conditions under one metadata record. The [docking comparison](../../scripts/compare_docking.py#L256) already performs final input verification.

**Repair:** copy that small final verification into the product runner, optionally checking each executable before launch. Do not add isolated build registries or a generalized compiler-analysis script. Test rejection with a temporary executable substitution; no real benchmark build is needed.

### A15 — P3: style consistency relies on a second hand-maintained field list

**Maintenance debt; no currently missing mapped field found.**

[style.rs:344](../../crates/egui_dockspace/src/style.rs#L344) maps style fields into validated core geometry, while [native_support.rs:42](../../crates/egui_dockspace/src/native_support.rs#L42) independently lists fields for mismatch admission. A future mapped field can silently escape the second list.

**Repair:** keep `DockStyle::validate()` as a separate prerequisite, including adapter-only fields such as ghost offset. Replace only the hand-maintained mismatch admission decision with `style.presentation_config_with(accepted)? == *accepted`, preserving core-only settings; retain a field list only for optional diagnostic names. A later geometry/visual-style separation is possible but unnecessary for this small repair.

**Verification:** custom core-only settings, every represented mismatch, and visual-only changes. No new parallel configuration owner is needed.

## Product contract decision: hidden-pane minimum availability

All tab items, including hidden ones, require minimum-size measurements so any visible tab can become a dragged leaf without inventing its preview minimum. [scene_manifest.rs:189](../../crates/dockspace/src/scene_manifest.rs#L189) explains this; [runtime measurement](../../crates/dockspace/src/runtime/measurement.rs#L233) requests every key. A missing hidden minimum makes the entire contribution unavailable even though current durable layout only uses the selected item's minimum.

The public probe confirmed: with A selected and B hidden, valid bounds/tab/strip measurements plus unavailable minimum for B yielded no Ready surface; answering B exactly yielded Ready. This is a real constraint, but relaxing it changes the product contract, so it is not classified as a general validation bug.

The low-cost choice is to require pane minimum metadata independently of widget/resource loading and correct the public request comment, which currently calls the item selected. The alternative is to distinguish measurements required for current layout from those required only for potential operations: render A while B remains inactive, and withhold B's size-dependent preview or Ready output until its exact minimum exists. Ordinary accepted selection need not wait for renderer measurement; changing that semantic contract would require a separate decision. That alternative must preserve preview accuracy and selection-reuse assumptions within the existing manifest/compiler.

## Necessary complexity and rejected suspicions

- **One canonical forest remains the authority.** RootStructure, scene programs and semantic snapshots are derived immutable views. No independently mutable egui docking graph was found.
- **Stable product IDs and generational nodes are correctly separated.** Runtime NodeId and raw reducer protocols remain private to normal consumers. Internal unit tests can see them without making them public extension points.
- **Historical three-full-graph-copy conclusions are obsolete.** Host-frame candidates share the workspace snapshot; transaction forks still copy allocator/map metadata while sharing immutable node payloads. The remaining copies identified here are specifically document maps and adapter output tables; structural snapshot derivation is another distinct cost.
- **Validation checks protect different facts.** Item conservation, command reference freshness, compiler-owned structure, exact output/pass matching and old-visible-output scope are not interchangeable. Remove redundant work at its owner, not every check that mentions a version.
- **Sink acceptance, output publication, actual Presented and core acceptance of that result are different events.** Async/retained hosts need these distinctions. A sink that records program steps proves public contract behavior, not GPU, pane callbacks or OS focus.
- **Native retirement phases earn their keep.** Destroyed observation, committed tombstone, route retirement and final quiescence prevent late callbacks from entering a successor lifetime.
- **Document hashes are cold integrity checks.** They do not claim authentication and are not an every-frame fingerprint problem. Save generation advances only after successful JSON encoding.
- **Update field transfer is not a separate P2.** HostFrame and owned Update repeat fields, but exhaustive destructuring catches additions and lease drop order has a purpose. No omitted field or lost authority was found.
- **The official egui shape audit has a documented evidence constraint.** A final shape count cannot prove that callbacks did not alter earlier paint; the fork already supplies a protected checkpoint. No unsupported recommendation to remove this proof is made.
- **Scripts are generally narrow.** The dependency checker and native runner do not implement their own Rust resolver or general proof framework. A14 needs only an existing-style provenance recheck.
- **Conformance prose has a small drift.** The [conformance README](../../crates/dockspace_host_conformance/README.md#L14) still describes mandatory presented preview before ordinary release. Executable OGC-03 correctly tests release on first eligible hit without a painted preview. Update the prose; do not regress the implementation or tests.

## Refactoring sequence

| Phase | Work | Approximate implementation scope | Exit evidence |
| --- | --- | --- | --- |
| 1 | A01 focus owner, A02 equal configuration, A13 CI selection | Small changes within native focus/coordinator, core reducer and existing CI; roughly 4–7 production/config files across separate commits | Denied-focus successor and quarantine drain; equal-config transfer; omitted tests selected |
| 2 | A03 document publication, A04 no-op snapshot, A05 shared derivation | Document/runtime publication, mutation publication, workspace/requirements ownership; roughly 5–8 files plus focused regressions | History-independent idle allocation; no-op provenance retained; one shared root derivation |
| 3 | A06/A07 immutable input data, A08 admission accounting | Pointer owner and input join; native capacity/owner mutations; roughly 5–9 files, split by owner | Growth tests for registrations/outputs/queue occupancy; existing blocker and terminal-accounting cases |
| 4 | A09 independent focus interface, A10 per-surface measurement policy, A12 pre-encoding configuration | Runtime interface plus official/native consumers; roughly 7–12 files per coherent change | Public backpressure/focus trace; sibling refresh; one-update config with causal rollback |
| 5 | Evaluate A11 readiness seam; decide hidden minimum contract | A dedicated design change across core bringup and reference native host | Direct and staged host transfer equivalence, readiness revocation, first-live and compensation preserved |

File estimates describe change locality, not schedules or a complete implementation plan. A14/A15 and the conformance prose can be small independent maintenance commits.

Do not bundle all phases into a rewrite. Start with the measured copies and incorrect state ownership, then reassess before changing the graph or moving readiness. A new application action currently crosses product-action, engine-input, compilation and outcome mapping; consolidating those same-shaped commands is a potential later cleanup, not an independently proven defect or a prerequisite for these repairs.

For another renderer, the required work remains: synchronous metadata/measurement resolution; a complete ordered program sink with owned resources; actual framework delivery/occlusion observations; and exact asynchronous output disposition. Native window management is an additional opt-in contract. DirectPrepared already reduces that host's preparation burden. A09/A10/A12 would remove unnecessary knowledge of rendering admission and internal phase choreography from this integration.

## Validation and limitations

- [Core probes](2026-11-10-architecture-design-audit/core_probes.rs): six characterization tests, all passed; [raw output](2026-11-10-architecture-design-audit/core_probes.txt). They demonstrate equal-config cancellation, no-op provenance, duplicate derivation, focus/output coupling, configuration encoding order and hidden-minimum behavior. Assertions deliberately characterize the current defects; they must be inverted or revised when creating permanent repair regressions.
- [Native focus probe](2026-11-10-architecture-design-audit/native_focus_probe.rs): one model test passed; [receipt](2026-11-10-architecture-design-audit/native_focus_probe.txt). It replaces the OS response with an exact injected callback after adapter dispatch, not with a fake successful-focus fact.
- [Persistence cost source](2026-11-10-architecture-design-audit/persistence_cost.rs) and [CSV](2026-11-10-architecture-design-audit/persistence_cost.csv): public release consumer, one live pane, five history sizes, timing separated from heap accounting.
- [Previously omitted native persistence subset](2026-11-10-architecture-design-audit/native_persistence_tests.txt): ten existing tests passed when executed separately with serde; exact selection and result are recorded in the receipt.
- Static cross-checks covered the current CI selection, public exports, dependency declaration, complete traced paths and the specific current tests cited above. No broad test rewrite, OS focus test, GPU benchmark, callback-lock benchmark or fresh ImGui run was performed.
- Initial exploratory probes were corrected where assumptions failed: input-free frames do not rebuild; a central-item Float was not an approved topology mutation; capacity one cannot retain two surface streams. Final recorded core results use the passing corrected probes. These failures were probe assumptions, not additional product findings.
- Prior green CI and earlier editor qualification are background context only. They are not substituted for fresh architecture evidence and do not prove the absence of other defects.

The [machine-readable receipt](2026-11-10-architecture-design-audit/receipt.json) records commands, baseline, artifact hashes and the final local state. Product Rust source remains at the baseline after temporary probe mounts are removed. This audit neither opens an upstream egui PR nor merges main.
