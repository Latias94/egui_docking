# Independent architecture and simplification review

Date: 2026-11-06. Reviewed the current uncommitted implementation diff against `34ab23a` / runtime base `5484071`. Excluded my added `engine/tests/host_authority.rs` regression from the review findings, as requested. Also excluded user-owned CI/review/reference changes. All source/line references below identify the version inspected; implementation is concurrent and may move them.

No source was edited. No Cargo, test, benchmark, Git write, network request, or nested agent was run. The test result reported by the parent is not an independently rerun result here.

Follow-up status: both P2 findings below were corrected by the implementation owner and independently rechecked read-only. The final follow-up section closes them and supersedes the earlier request-changes verdict. The original findings remain as the review record.

## Verdict

The substantive retention is real. The eligible selection path bypasses requirement derivation and the complete surface compiler/finalizer, and installation directly rebinds output wrappers over the existing hit/touch, semantic and paint-link storage. It does not reconstruct full facts and then compare them to discover sharing. The selected slice has useful Depth inside the existing owner and does not need a new public frame Interface, arena rewrite or generic dependency framework.

I found no new correctness/authority blocker in the inspected source-binding and retained-installation paths. I do request two bounded changes before treating this implementation as architecturally complete: the new full-workspace selection capture per surface and the new per-root full-pane title lookup introduce avoidable work on the conservative path. Both reuse information already present during construction and can be fixed without expanding the design.

## Findings to resolve in this change

### P2: Per-surface plans now scan and store selection for the entire workspace

Location: `crates/dockspace/src/scene.rs:367-374`, called by `scene_compiler/surface.rs:82`; standalone validation also calls it at `scene/validation.rs:192`.

`bind_workspace_selection` loops over every workspace node and inserts every tabs node into a new `SecondaryMap` for one `PresentationPlan`. The compiler invokes it independently for each surface. Before this diff, a plan stored selected state only in its own pane/tab records; it did not allocate an all-workspace selected-state copy per surface.

For S surfaces and N workspace nodes, compiling the surface roster now performs O(S × N) new node visits and stores redundant selection entries across all S plans. In the concrete case of S one-pane surfaces, this adds quadratic total capture/storage for S selection values that each surface uses only once. It also extends the same irrelevant selection entries into fallback/emitted plan lifetimes. A one-surface click benchmark does not expose this amplification.

Fix: capture the selected binding for a pane while constructing that pane/program operation, where `NodeId` and `selected` are already supplied, or share a single owner-built selection snapshot between plans from the same workspace publication. Keep the retained rebind's old/new selection isolation. No extra traversal is needed to discover these values.

Storage detail: merely omitting unrelated inserts while retaining one dense `SecondaryMap` per plan may still allocate through a high global slot index. A scoped sparse map using existing storage facilities, a genuinely shared common snapshot, or plan-local rows avoids that problem; do not introduce a new arena to solve it. The decisive requirement is that independent one-pane surfaces do not each retain an N-entry/capacity selection table.

Validation: a small multi-surface construction counter/retained-size assertion should establish that selecting/capturing one surface's bindings does not visit/copy unrelated nodes. Use the existing full-surface behavior checks for output correctness. There is no need for a new benchmark family before deleting the evident extra scan.

### P2: The stable root-title recipe is recovered by a full pane scan for every root

Location: `crates/dockspace/src/scene/program.rs:297-305`.

The linked tape correctly stores `title_tabs` and resolves its selected item through the current plan binding. However, the linker obtains that recipe by taking the draft's selected item and searching `plan.facts.pane_records` from the beginning for every root. With R roots and P panes on a surface, this introduces O(R × P) work; many one-pane contained roots give a quadratic lookup sum. The previous linker copied the title item directly and did not make this new join.

Fix: record the title pane/node recipe when `push_root_operation` receives its pane operation and selected value (`scene/program.rs:474-485`). At that point the Implementation already owns the stable `PaneSceneId`, and it can preserve the current rule of choosing the first pane whose selected value is `Some`. Carry that recipe into the linked tape directly. Test-only inspection can still expose the currently resolved title item. This removes the new lookup while preserving the already-correct title semantics.

Validation: keep the changed-first-pane title regression and the existing contained-root title ordering/empty-pane tests. A construction count for multiple roots should show one title choice per root, not repeated visits over the whole surface's panes.

These are new representation costs in the diff, not requests to eliminate every existing whole-root operation. They were reported in commentary before this final report.

## What was actually removed or bypassed

| Responsibility | Current implementation | Review conclusion |
| --- | --- | --- |
| Selected-path requirement construction | `SceneRequirementManifest::after_selection` shares each question inventory and the tab-bar requirement map, recomputes the changed bar's actual effective policy/close facts, and retickets the selected surface. `advance_selection_revision` publishes that result without calling complete draft derivation. | Genuine avoided derivation. The conservative builder remains for a different dependency case; it is not a redundant second representation. |
| Workspace structural index | `WorkspaceIndex::after_selection` shares membership and layout-topology owners, updates the selected root's immutable fingerprint value and publishes the new index version. | Genuine removal of root membership/index/layout reconstruction. Remaining fingerprint copying/searching is real and described below. |
| Complete surface build | `engine/surface_contribution.rs:346-385` checks owner-issued selection preparation, current source version/coordinates, allowed interaction phase, previous fact identity, complete answer equality, actual effective minima and unchanged reveal/scroll. It returns a rebound plan before `compile_surface_plan`. | No full record construction/finalization on the successful retained branch. |
| Installed hit/touch, semantic and program facts | `scene/lifecycle.rs:1264-1279` uses `shares_facts` to choose `rebind_output`, clones the existing program-link owner, and bypasses region replay, touch compilation, semantic sort and paint linking. | Genuine avoided installed-view derivation; not rebuild-then-compare. |
| Selected display values | `PaneRecord.selected` and `TabRecord.selected` are deleted. Runtime paint wrappers receive their current selected values from the plan binding. Linked roots use `title_tabs` rather than a frozen item. | Stable facts no longer capture those changing values. The title capture is semantically fixed, subject to the lookup-cost finding above. |
| Per-target command proofs | `DropTargetRecord` holds one typed `DropTargetLocation`; its ID is derived. Embedded `DropDestination`, ID-versus-destination validation, and target-time frozen-source captures are removed. | Meaningful deletion: impossible duplicate identity states no longer need repeated consistency validation. |
| Selected adoption during contribution install | The reducer deletes the intermediate `selected_by_pane` BTreeMap and reads each bar's selected value directly from the binding before adopting resolved offsets. | Actual association work disappears, and equal-scroll selection still updates `resolved_selection`. |
| Large immutable equality | `PresentationPlan`, hit manifest and layout-input comparison use exact shared-owner short circuits where appropriate. They retain public value comparison for distinct objects. | Bounded improvement; this is not a claim that every scene/header comparison disappeared. |

## Authority and dependency checks

The following previously identified hazards have concrete replacements in the inspected implementation:

- `presented_root_structure_is_current` now reads the output's `plan.command_basis(root)` directly. It no longer depends on finding an arbitrary proof-bearing drop target. It continues owner/topology and presented split-weight checks.
- `drop_resolver::rebind_topology_target` takes the typed location plus the bound root basis, resolves that same live target, checks surface/root identity and compares frozen/current presentation structure. The source of frozen facts changed; it was not silently replaced by unconditional live target capture.
- Drop eligibility still uses current `DropCommandEligibility` and policy after structural rebinding. Equality of bar policy does not authorize a newly selected target's drop rule.
- `SurfaceMeasurements::same_values` compares every key/value and availability independently of the ticket. External ticket/base/policy/coordinate validation occurs before the retained guard. Matching known-authoritative old values establishes the same complete relation; stale detached tickets are not authorized by equality.
- The geometry guard compares old/new **effective outer minima**, including configured minimum and visibility, even though the full per-item answer inventory is identical. It computes the selected reveal/clamp result from retained strip geometry and current strip state. Changed inputs fall back to the sole complete compiler.
- Armed and nonprojecting Pressed are explicitly permitted in the retained branch, so real changed-tab presses are not excluded by an Idle-only optimization predicate. Actual drag/resize/contained projections remain excluded.
- Old plan binding maps and root bases remain immutable through copy-on-write. Geometry sharing does not mutate an old output's selected value or grant it a new emission identity. Candidate, fallback and observed authority continue to be distinct lifecycle states.
- Selected revision/preparation is copied with the owning presentation state for rollback/checkpointing and cleared on incompatible revision/rebuild or successful Ready publication. Only one preparation is retained; it is not an unbounded history/cache.

I did not find an additional source-binding correctness error in those paths. This is a static review, not independent verification of all native lifecycle outcomes.

## Remaining costs and later optimization, not blockers

The retained branch is not a constant-time selection implementation, and it should not be described that way:

- `WorkspaceIndex::after_selection` clones the root map, then `Arc::make_mut` clones the changed root's fingerprint-node array and linearly finds the changed tabs record (`workspace.rs:201-209`). Converting per-node item/MRU/children/weight vectors to `Arc` reduces deep nested allocation, but the root-wide node-array copy/search remains. The existing `root_fingerprint_builds/node_visits == 0` test does not count this path's cloning/searching; it proves no call to the old builder, not zero structural volume.
- `SceneRequirementManifest::after_selection` clones the shallow surface map (`scene_manifest.rs:949`). `scene.reconcile_manifest` still walks its surface roster. These are O(number of surfaces), while the expensive per-item inventories remain shared.
- `rebind_selection` copies the selected binding on write (`scene.rs:424`), currently the whole secondary map. This is especially broad because of finding 1; after scoping/sharing is corrected there may still be a legitimate per-surface binding copy.
- Complete measurement callbacks and keyed map assembly remain, followed by complete answer comparison. That is deliberately outside this slice; no claim is made that the detached measurement protocol was deleted.
- Contributions still iterate every current bar to adopt offsets (`presentation_authority.rs:1311-1321`). That is existing presentation work, now cheaper after removing the pane association map. A selected-only adoption change should follow attribution if it matters.
- Numerical layout, geometry validation, touch derivation and semantic linking remain necessary when actual inputs change. Keeping their sole conservative implementation is appropriate Locality, not redundant code to delete for appearance.
- `SurfaceProgram::finish` already consumes root drafts and deferred vectors. The new assignments that empty those fields after linking (`program.rs:437-444`) are harmless defensive cleanup; most clear already-empty owners. A distinct draft/linked type could remove the remaining empty builder headers, but that is not warranted by this review and is not needed for the acceptance goal.

The actual plan binding stores a selected-value map and root fingerprint bases; it does **not** retain an `Arc<Workspace>` for every output. The earlier design-review warning about complete old workspace lifetimes therefore does not apply literally to this implementation. Memory reporting should instead count the selected binding tables, shared roots/measurement maps, and current/fallback/in-flight ownership actually present.

## Small Interface assessment

The new `pane_selected`/`tab_selected` helpers provide selected display state through a bound plan while geometry records stay stable. Runtime paint wrappers freeze those selected values for their call and continue binding receiver descriptors to the current output. I found no internal caller newly confusing a geometry record with command/output authority.

The helpers accept a record reference and perform a node-key lookup; they do not establish foreign-record membership. Their callers in this diff use records from their own plan. Do not add expensive per-paint membership revalidation as a speculative fix; either document that local-record expectation or address it with the scoped/row binding change if public semantics require more. This is not a demonstrated production bug in the inspected callers.

The three concrete owner/private roles are coherent: the presentation owner issues the preserving edit case; plan binding holds current display/source values; lifecycle binds independently reusable facts to output authority. The two real Adapters continue crossing the established Seam. There is no new provider protocol or shallow public wrapper tree to unwind.

## Recommended completion

Resolve the two newly introduced conservative-path traversals using information the builder already owns. Preserve the fact-identity, old/new selection isolation, fresh-ticket and real press/Armed/release tests while doing so. Then review the final qualification evidence on its actual complete workloads. No further broad architecture work is required by this review, and no performance target is claimed achieved from source inspection alone.

## Follow-up: both P2 findings closed

Date: 2026-11-06, after the implementation owner reclaimed all source write authority and started serial Cargo verification. This follow-up only read the changed source/tests and appended this report; it ran no Cargo or tests and edited no source.

**P2 1 — closed.** `PresentationPlan.selection` now uses `slotmap::SparseSecondaryMap`. Production `compile_surface_scene` no longer calls `bind_workspace_selection`. Instead, `push_surface_program_pane_operation` receives the already-owned `PaneSceneId` and selected value and inserts exactly that pane's binding (`scene/program.rs:704-714`). It performs no workspace traversal. The standalone/synthetic binder (`scene.rs:367-376`) now iterates only `self.facts.pane_records`, performs direct workspace node reads, and sizes its sparse map to that local roster.

This removes both causes identified by the review: there is no per-surface whole-workspace capture, and a high global slot index does not size a dense per-surface array. Copy-on-write selection rebinding now copies that surface's sparse selected-value map, preserving isolation from old output bindings. It does not reintroduce a full workspace snapshot.

The new `a_surface_binding_only_stores_its_own_pane_selections` regression (`scene/tests.rs:24-65`) creates 128 independent surfaces and compiles the final surface, whose node has a high global slot. It asserts one stored selection and capacity below 128, checks the selected item, then exercises the synthetic binder and asserts the same bounded storage and equal result. This is a meaningful regression for the exact storage amplification; the static production call trace establishes removal of the full node scan. Test execution remains the root agent's serial verification responsibility.

**P2 2 — closed.** `SurfaceProgramRootDraft` and `SurfaceProgramBlock::RootBegin` now carry `title_tabs` directly. `push_surface_program_pane_operation` passes `selected.map(|_| id.tabs)`; `push_root_operation` sets the recipe only when the draft has no title pane. This preserves the former choice of the first pane with a nonempty selection. `finish` carries that recipe into the block, and `link` copies it directly into `LinkedSurfaceProgramRoot` (`scene/program.rs:276-305`). The full `pane_records.iter().find_map` lookup is gone.

Runtime traversal still resolves `title_item` through `root.title_tabs.and_then(|tabs| self.selected_in_tabs(tabs))` (`scene/program.rs:841-846`), so a retained selection updates title semantics without changing the tape. The migrated main/contained ordering test inspects the stable recipe and resolves it through the plan binding (`scene_compiler/tests.rs:521-553`); its expected items still cover main and multiple contained roots.

No additional blocker was found in these two fixes. They use construction facts already owned by the existing Module, remove the newly introduced cross-owner searches, and require no new public Interface, dependency, arena, or protocol. The retained-facts architecture is approved from this review's source/ownership/simplification perspective. Full correctness and the complete-workload performance target remain subject to the root agent's exact-final serial verification and qualification.
