# Retained surface facts and current bindings

Date: 2026-11-06. Baseline: `34ab23adf8062960298abbc5fbcba370104f3b86` (runtime `5484071`). Status: complete; see the [qualified result and migration report](../performance/2026-11-06-retained-facts.md).

The user authorizes breaking changes, direct caller migration and removal of redundant code. This plan executes the [architecture diagnosis](../performance/2026-11-06-architecture-design-review.md). It does not assume that Rust, correctness checks or framework neutrality impose the present latency.

## Outcome and qualification

An ordinary selection whose actual geometry dependencies remain unchanged retains the same immutable surface geometry, program, pointer/touch geometry and semantic receiver inventory. It publishes current selection/command/output bindings without reconstructing those facts. Both egui and the complete headless recorder consume this representation through their existing real measurement/paint/settlement seams.

The working acceptance target chosen for this goal is at least 10% lower 1,024-pane complete two-frame click p50 in each of two independent serial comparison groups, with the complete renderer-free selection update improving in the same direction. Keep the original 16/128/1,024-pane workloads, all measurement questions, every pane/tab/content, both click frames, full draw data and output settlement. This is a goal, not a predicted result or a measurement already obtained. Investigate repeatable regressions in other cases and disclose allocation/current-plus-fallback/in-flight memory costs.

Correctness must cover actual effective minima and selected-item policy, tab reveal/scroll, MRU, source press/arm/release, drag activation, dropped/restarted frames, stale and unpresented output, native first-live/freeze/recovery and retirement. Geometry sharing never supplies input authority.

## Design alternatives and choice

Three fresh, read-only design lanes considered different Interfaces:

| Design | Depth and Locality | Cost | Decision |
| --- | --- | --- | --- |
| Three-entry frame owner plus a topology-corresponding immutable fact tree | Hides all preparation and permits leaf/path reconstruction | Replaces program traversal, flat public inspection and substantial host orchestration | Useful later if changed geometry needs finer updates; not required to retain a fully unchanged geometry block |
| General retained snapshots with leaf/root/composition facts | Explicitly serves multi-surface, native and historical-output use | Many new chunk ownership and composition interfaces before the ordinary case is proven | Adopt its separation of facts, binding and emission; defer universal chunking |
| Common synchronous selection with whole-fact retention | The owner knows the edit; actual input comparison establishes whether geometry survives | Requires genuine separation of selected state and command authority from geometry | Selected first implementation |

The Module remains the existing session/engine/frame owner. Workspace, layout and compiler are concrete in-process dependencies. egui and the owning headless recorder are two actual Adapters. Do not add provider routing, a generic dependency graph or a universal outer-run trait.

The first slice intentionally retains the existing general transaction arena and public detached measurement use cases. They are not rewritten merely because breaking changes are allowed. Changed public geometry/target Interfaces are migrated directly, with no deprecated aliases or compatibility reconstruction. Later measurement/storage work needs its own attributed benefit. Removing all root proofs or replacing all node storage is not a prerequisite for the concrete avoided-work result below.

## Representation

### Immutable geometry and recipes

Introduce a privately owned immutable surface-facts block containing pane/tab/bar/splitter/contained/drop geometry, canonical program order, guides and magnetic geometry. A full builder produces this block once. An eligible selection shares its exact owner; it never flattens a new set of tables to discover equality.

Remove `PaneRecord.selected` and `TabRecord.selected`. These records describe geometry. Their bound paint views read current selection directly from a plan-owned `SparseSecondaryMap<NodeId, Option<ItemId>>`, scoped to that surface's panes. Capture it during pane construction; neither scan the whole workspace for each surface nor size a dense map by global slot indices. Public inspection that needs selection crosses the current plan/binding Interface. Update all repository callers and tests instead of preserving stale fields.

Remove proof-bearing `DropTargetRecord.destination` and the duplicated ID-versus-destination semantic check. A typed target location stores surface/root/node/edge/tab-gap/fraction geometry intent and derives its stable ID. A target does not contain `ItemSource`, a full root fingerprint or an output ticket. Guide sharing continues to reference the one canonical target geometry.

### Current semantic and output binding

A plan binds the shared facts to its current selection and per-root command basis, actual retained measurement inputs and current measurement provenance. The engine retains the current immutable workspace/index. Target resolution uses the existing `rebind_topology_target` responsibility, now taking the typed location plus the plan's source basis. It checks exact structural identity and evaluates actual current drop policy. Equal bar policy is never a substitute for drop policy.

Candidate, fallback, observed input authority and concrete emissions remain separate. A newly bound plan receives fresh output identity. Rebind hit geometry and semantic receiver storage to that identity using small wrappers; do not rerun region emission, touch compilation, semantic sort or paint linking for unchanged facts. The old output keeps its own immutable source and ledger state.

The full conservative builder and retained path produce this same representation. Keep one complete compiler. A private retained-facts preparation result can authorize sharing because its owner established the dependency case; arbitrary external plans cannot mint it.

### Owner-issued selection effect and structural inventory

Route every successfully changed single Select through one revision/presentation path. The editing owner already establishes canonical preservation; use that fact to retain structural membership, layout topology and complete measurement-key inventory. Refresh current root semantics and the changed bar's effective policy rather than deriving every surface inventory again.

The initial implementation can retain the general workspace transaction copy while removing unrelated presentation derivation. Shared immutable fingerprint arrays and structural membership may reduce its associated copying without redesigning the arena. Do not claim that a whole workspace clone disappeared unless counters prove it did.

Represent at most one eligible selection rebound in the existing presentation owner, alongside the exact prior surface facts and changed bar. Another incompatible edit or general refresh clears it. It is an owned preparation result, not a cross-frame fingerprint/history map. Encoding rollback/checkpointing must include it; its actual source/version/coordinate inputs are rechecked before use.

## Dependency test for retaining facts

1. One changed ordinary main-root Select, stable topology, item order/ownership, configuration and policy snapshot. Initially exclude contained composition, popup, pending relevant native/rehome/recovery and geometry-producing drag/resize/transform projections. Permit a nonprojecting tab press/Armed state and its release; preserve its post-selection source capture and threshold activation behavior.
2. Recompute the changed bar's full effective policy from the new selected-item rule. A changed policy takes the complete path and constructs the correct current question inventory.
3. Execute the complete current Q. Matching includes every item minimum, intrinsic and strip value, bounds and relevant optional values. Any changed answer or unavailable/invalid input takes the appropriate conservative/unavailable path. Ready idle already bypasses compiler exact-set validation; do not claim to remove that nonexistent idle work.
4. Compare old and new selected actual effective outer minima using the same calculation as the complete compiler, including configured pane minimum and bar height. Different minima require rebuilding even when all per-item answers are unchanged.
5. Resolve the changed strip's current selection/reveal/clamp using retained width/chrome inputs. Any changed receiver geometry, scroll, control membership or protected chrome requires the complete path in the first slice. Equal bar bounds alone does not establish equal touch geometry.
6. If all inputs match, preserve the exact surface-facts, hit/touch, semantic-membership and program-link owners. Bind the new selection/source/measurement/output facts. Complete paint and settlement still run.

Unknown cases are handled by the single complete builder. This is a bounded dependency case, not a universal incremental engine. A complete replay to establish that the new geometry matches is not acceptance.

## Implementation and deletion sequence

- [x] Freeze qualified baseline product/headless binaries and source hashes; snapshot user-owned dirty files.
- [x] Obtain three independent designs and choose a concrete representation.
- [x] Separate typed drop-target geometry from command proofs. Migrate resolver, validation, presented-input callers and tests; delete old destination/semantic-duplication helpers.
- [x] Split immutable surface facts from current plan binding. Remove geometry-selected fields and migrate bound inspection/painting. Keep direct indexed selection reads.
- [x] Add the owner-issued Select effect and structural inventory rebind. Share immutable question inventories and structural index members; remove the selected path's full requirement derivation and unrelated strip-roster reconciliation.
- [x] Implement actual dependency checking and retained scene installation. Reuse hit/touch/semantic/program-link owners directly; keep current output/coordinate/lifecycle checks.
- [x] Remove comparison/replay work made obsolete by exact shared ownership. Keep full value equality available where it still represents a real public diagnostic contract; do not globally assert floating-point equality is reflexive.
- [x] Add focused interface-level counterexamples and avoided-work assertions. Migrate broken callers directly and remove tests that only assert deleted representation details, retaining behavioral coverage.
- [x] Run applicable serial checks, independent implementation/evidence review, two qualified complete-workload comparison groups and separate allocation measurements. Resolve actual failures; do not weaken workloads to meet the target.
- [x] Record final design/migration/performance evidence and prepare the intentional changes for commit/push on `refactor/dockspace`.

Primary files: `drop_target.rs`, `drop_resolver.rs`, `scene.rs`, `scene/records/tab.rs`, `scene/program.rs`, `scene/lifecycle.rs`, `scene/validation.rs`, `presentation_hit.rs`, `semantic_manifest.rs`, `workspace.rs`, `scene_manifest.rs`, `scene_compiler/{requirements,surface}.rs`, `engine/{presentation_authority,surface_contribution}.rs`, runtime paint views, and their actual host/test callers.

Delete rather than wrap: embedded target commands/proofs; duplicate target identity validation; selected flags in immutable geometry; selected-path structural requirement regeneration; full installed-view derivation when unchanged-facts ownership already establishes it; caller plumbing for those removed Interfaces. Retain real geometry validation for new geometry, distinct semantic/pointer reachability, complete measurements and the actual final-presentation contract.

## Verification and investment decisions

Use the existing structural-work ledger or small test-only counters at real construction sites. An eligible selection must show zero complete surface compiler, hit emission/touch construction, semantic roster construction and paint relinking calls. Compare complete normalized facts and outputs against the conservative builder, plus selected/MRU/policy/authority results. Required callbacks/program/content counts remain unchanged. Do not require zero transaction copies unless that work has actually been replaced.

Specific counterexamples: unequal selected minima; selected-dependent bar visibility; equal bar but different drop policy; clipped selection/reveal; unrelated changed measurement in the same frame; future-drop source metric lookup; first touch after retained output; press/Armed/release and subsequent activation; foreign/passive/absent receivers; final-hook paint mutation; failed or dropped candidate; repeat-pass restart; source frozen by native first-live; host/coordinate rebind and retirement; combined occlusion and coverage failures.

Use `cargo fmt` and `cargo nextest` where available, one Cargo build job and one test thread, reusing existing targets. Run focused checks first, then workspace, headless default/serde, native and doctest/dependency checks appropriate to final changes. Builds, tests, profiling, allocation instrumentation and formal timing remain serial and separated. Validate the exact final source before claiming qualification.

The goal does not finish when a small identity/receiver change wins a few percent. It finishes when the substantive retained-facts path is implemented, redundant replaced code is removed, correctness is reviewed and the complete workload meets the declared target. If the local path misses, examine actual eligibility and remaining measured work before choosing the next change. Do not expand into a generic proof/compiler framework or claim the gap is a theoretical lower bound.

## Local state and continuity

Execution artifacts and the three original design reports: `/tmp/dockspace-fearless-20261106-34ab23a/`. Its baseline receipt records existing binary provenance; runtime binaries are not committed. The user-owned `.github/workflows/ci.yml`, `docs/reviews/` and `repo-ref/egui` are outside implementation ownership. Never restore, delete, stage or overwrite those changes.
