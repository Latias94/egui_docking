# Checked move: source attribution before measurement

Scope: the current `docking_cost.rs` driver and production code rooted at `a9ace58`, as inspected on 2026-11-06. This report contains source facts and falsifiable counter predictions only. No benchmark output, elapsed time, allocator result, or profile was read or produced. All line numbers refer to `crates/dockspace/src/` in the current checkout.

The selected Interface is one current-index capture followed by `WorkspaceTransaction::apply`. Its transaction Module does not receive the capture index. This Seam is why sharing source/target capture does not eliminate live proof reconstruction inside publication. All-root layout/export are subsequent benchmark phases; they are not part of the transaction itself.

## 1. Request capture

| Source anchor | Actual work in these fixtures | Scope and reason |
| --- | --- | --- |
| `docking_cost.rs:139` `capture_request` | Build one current `WorkspaceIndex`, walk first/last leaf paths, capture Item2 and center/edge target, allocate the command vector. | Timed caller work. Neither proof nor policy is prepared outside the timer. |
| `workspace.rs:82` `WorkspaceIndex::build` | Scan all surface/main/contained owner rosters; allocate owner/root BTreeMaps; call `build_root_index` for **every** root. | Whole workspace, including roots untouched by the request. This is the chosen existing index Interface, not a requirement that every one-item request inspect unrelated roots. |
| `workspace.rs:824` `build_root_index` | Visit each node in one root; allocate traversal/visited storage and a fingerprint row vector; copy tab item/MRU slices into Arcs and split child/weight-bit slices into Arcs; accumulate item count and preserve root/central/presentation identity. | Complete root value proof, including selected/MRU, not just topology. Fresh Arc containers are allocated around copied values; this is not a zero-copy view of workspace storage. |
| `workspace.rs:270`, `:314`, `:390`, `:411` indexed capture methods | Check index version, node existence and indexed membership; source checks its local tab item; target reads central/selected policy scope; clone the already captured fingerprint Arc. | No second full root build for source and target at this caller. Same-root source/target share one fingerprint. |

`docking_cost.rs:116` resolves operands along actual child paths, proportional to tree height, rather than calling `locate_item_by_id`'s global scan. The index is dropped before request capture returns. Proof Arcs retained by the command survive; fingerprints for unused roots and index membership sets can be destroyed in the request phase.

## 2. Transaction preparation and live proof reconstruction

`transaction.rs:51` calls `operation::prepare_transaction` (`operation.rs:602`), which invokes `prepare_transaction_inner` (`:644`). For each of these single Move transactions:

| Source anchor | Work | What is required versus chosen Implementation |
| --- | --- | --- |
| `operation.rs:651`; `transaction.rs:11`; `graph.rs:356` | One complete `Workspace::clone`: SlotMap nodes and their vectors, MRU map/vectors, roots, surfaces and contained metadata. | Isolation/rollback is required by `apply`'s “unchanged on any failure” contract. A complete deep clone is this implementation's mechanism; atomicity does not intrinsically require cloning unrelated roots. |
| `operation.rs:654`; `graph.rs:447` | Build a BTreeMap item multiset by scanning all nodes/items in the original workspace. | Transaction item conservation is required. This is the first full roster derivation for this boundary. |
| `operation.rs:658`, `:1367`, `:1349` | `Move.opened_item()` is `None`; applied item delta is `None`. | No new-item insertion or item-ledger mutation occurs for these requests. Do not attribute a scan to either helper. |
| `operation.rs:717`, `:1043`, `:908` | Arrangement lock check, then `DropCommandEligibility::new` validates source; `check_target` validates target and semantics, derives effective policy facts, evaluates policy. | Authorization is real and timed. The one-item payload facts allocate small vectors; policy checks the payload/rule maps, not every workspace item. |
| `operation.rs:1522` | `move_payload` validates source and target **again**, before detaching anything. | Safety checks must hold at application. In this one-command path, authorization and execution see the same candidate values and there is no mutation between the two validation rounds. Their independent recapture is repeated work, not a required number of validations. |

Exact live validation sequence, for both center and edge targets:

```
authorization:
  source verify_reference                     -> 1 complete source-root capture
  target verify_reference                     -> 1 complete target-root capture
  target rederive frozen facts by capture_*   -> 1 complete target-root capture
application:
  source verify_reference                     -> 1 complete source-root capture
  target verify_reference                     -> 1 complete target-root capture
  target rederive frozen facts by capture_*   -> 1 complete target-root capture
```

The source chain is `validate_move_payload_with` (`operation.rs:2582`) -> `validate_item_source_with` (`:2512`) -> `ReferenceAuthority::Live` (`:2434`) -> `Workspace::verify_reference` (`workspace.rs:777`). Target validation is `validate_tab_target` (`operation.rs:2669`) or `validate_edge_target` (`:2636`). Both first verify the fingerprint, then call a standalone workspace capture through `ReferenceAuthority` (`:2451` / `:2465`) to rederive target facts. Those captures are `workspace.rs:642` / `:667`, not the supplied caller's indexed captures.

Every live `capture_reference` (`workspace.rs:797`) first does a separate root-membership search through `root_contains_node` / `subtree_contains` (`:917` / `:924`), then `root_fingerprint` (`:815`) rediscovers the presentation owner and calls `build_root_index` again. Source membership can stop at the first leaf; target membership reaches the last leaf in this fixture. Root proof construction always visits the complete root. `presentation_for_root` (`:993`) linearly searches surface/contained owner rosters and can stop when the owner is found.

The fresh live fingerprint is compared with the captured value; `command.rs:123` / `:138` show the complete structural/selection/MRU/presentation data. The checks reject genuinely stale proofs and cannot simply be removed while preserving this current Interface. The contract does not demand repeated construction of the same complete snapshot during one unchanged candidate phase, nor unrelated-root indexing at request capture. A future reuse boundary would have to bind facts to that exact candidate and stop reusing them after a command mutates it; caller version labels alone are insufficient for a multi-command transaction.

Policy specifics: `operation.rs:932` constructs retained and undocked payload facts; `:1184` looks up the root presentation owner, and the Item branch creates a one-element vector. `policy.rs:1470` checks source/target rules; `:2083` checks payload uniqueness. These are payload-local/rule-map operations here, not full graph scans. `DropCommandEligibility::new_indexed` exists (`operation.rs:917`) but is **not** used by this `WorkspaceTransaction::apply` path.

## 3. Actual mutation and extra root scans

| Source anchor | Center / cross-root center | Perpendicular inner-edge case |
| --- | --- | --- |
| `operation.rs:2776`, `:3261` | Remove Item2 from its two-tab source and MRU; source remains Item1-selected. | Same. |
| `operation.rs:3321`; `workspace.rs:967` | `cleanup_item_source_root` collects **all remaining items of the source root into a new Vec**, then asks only whether the Vec is empty. It traverses the complete source root even though its first pane remains nonempty. | Same. This scan is not reflected in `item_multiset_scans`. |
| `operation.rs:2910`, `:2929`, `:2977` | Append one item at target; select/promote Item2; scan/modify only the small target item/MRU vectors. | Create one singleton tabs node plus one wrapper split. |
| `operation.rs:1320`; `workspace.rs:941` | No edge parent search. | Authorization's weight check calls `parent_link`, a root traversal with a visited set. |
| `operation.rs:3029`, `:3132` | No edge parent search. | `insert_payload_at_edge` calls `parent_link`, then the opposite-axis branch calls `wrap_target_branch`, which calls `parent_link` again. The wrapper is unattached until that second search completes. Thus there are three parent searches including authorization. |
| `operation.rs:3569` | Scan/retain every surface to prune vacant ones; no surface is removed. | Same. |

The empty-root outcome and parent association must be correct. A full temporary item list is not required merely to answer emptiness, and locating the same target parent three times is not a required semantic count. These are source-level repetition opportunities, not measured priorities.

`payload_node(Item)` returns `None` (`operation.rs:3611`). Therefore subtree-payload containment scans and complete-tabs payload collection branches are **not** executed for these requests. `move_removes_complete_root` is a Create*Root-content helper, not an extra scan on this ordinary Move path. The same-tabs/no-op helper (`:3191`) returns without mutation for distinct source/target leaves and does not short-circuit the requested moves.

## 4. Whole-workspace canonicalization and validation

`canonicalize_draft` (`canonical.rs:205`) validates/rebuilds the whole candidate. It does **not** call the public `canonicalize_workspace` wrapper at `:184`, so it does not add a second full workspace clone.

| Source anchor | Actual traversal/allocation |
| --- | --- |
| `canonical.rs:408` `validate_nodes` | Scan all nodes; validate selected/MRU, duplicate item ownership, split weights/edges and parent uniqueness. Builds parent HashMap and item-owner BTreeMap. `graph.rs:15` `is_exact_tab_mru` builds two BTreeSets for every tabs leaf. Then scans the MRU-key map for unexpected entries. |
| `canonical.rs:549` `validate_acyclic` | Allocate a full node in-degree map; scan edges; populate/process a queue for the complete graph. |
| `canonical.rs:496` `validate_roots` | Scan all root records and parent relationships. Extra central-subtree reachability is skipped because these fixtures have no central node. |
| `canonical.rs:211` | Derive a complete candidate item multiset before rewriting. |
| `canonical.rs:218`, `:266` | Allocate a root roster and rewrite every root. `workspace.nodes.get(...).cloned()` is executed on both Enter and Exit for each node (`:275`). Even unchanged nodes clone their tab/split vectors twice. The postorder result map covers each root's nodes. |
| `canonical.rs:315` `rewrite_split` | Allocate `rewritten` and `flattened` temporary vectors for every split. The primary alternating-axis fixture and perpendicular edge insertion avoid same-axis flattening and weight renormalization; the traversal/allocation still occurs before the unchanged return at `:375`. |
| `canonical.rs:610`, `:232` | Derive reachability from every root; retain all arena nodes and MRU rows against the resulting HashSet. No dead nodes are removed in these particular successful moves. |
| `canonical.rs:236` | Derive another complete item multiset and compare it with the pre-rewrite multiset. |
| `canonical.rs:244`; `graph.rs:502`; `validation.rs:365` | Run the strict complete validator after canonicalization. This validates every root's graph, global orphan inventory, items, MRU, central metadata and presentation owners. |
| `validation.rs:534` | Strict graph traversal clones every visited Node once, including its owned vectors. `:603` separately builds item/seen HashSets for each tabs MRU check; `:721` derives/validates presentation ownership maps. |
| `operation.rs:678` | Derive a fourth full transaction-boundary item multiset and compare with the ledger captured before application. |

For a valid disjoint forest, rewriting therefore executes two full sets of Node clones and strict output validation one more set, on the **post-move** graph. The explicit workspace-clone ledger records none of those per-Node clone calls. They are distinct from the one pre-move workspace deep clone.

Canonical topology and item conservation must hold. Generic canonicalization accepts broader drafts and verifies its own conservation, while transaction preparation verifies command deltas across application. Those are distinct responsibilities. The resulting repeated roster derivations, duplicate node/MRU checks and full unchanged-root visits describe the current Implementation; neither rollback nor stale-source rejection by itself mandates this exact collection of passes. Removing a pass would require the owner to prove its precondition/postcondition elsewhere, including multi-command failures, rather than assuming these success fixtures establish general safety.

## 5. Changed detection and publication: do not exaggerate equality work

`operation.rs:676` computes `candidate != original` for the single Move outcome; `transaction.rs:97` computes it again for the report. `graph.rs:365` defines equality in short-circuit order: roots, surfaces, contained metadata, MRU, arena length, then node values.

There are two **calls** to workspace equality. They are not two guaranteed full graph scans. In these fixtures, center moves change the first source MRU row; edge moves additionally change MRU map length. Both comparisons stop at MRU before reaching the final arena-node traversal. Root/presentation rosters may still be compared first. The code should not be described as traversing all Nodes twice for changed detection in this workload.

`transaction.rs:102` publishes only after every earlier fallible stage succeeds. Assigning the candidate destroys the replaced original workspace in this timed call. On an error, the isolated candidate is dropped and the caller's workspace remains unchanged. Rollback is the contract; the chosen whole-workspace allocation/destruction mechanism and the two separate changed comparisons are implementation costs.

## 6. Predictions for the independent counter build

These are predictions derived before measurement, not receipts. Let total panes be `T`, views/roots be `V`, per-root panes `P=T/V`, original whole-workspace nodes `N=2T-V`, and original per-root nodes `n=2P-1`. There are `2T` items throughout. Source/target proofs are checked before mutation, so edge's two new nodes do not increase the fingerprint or clone-volume predictions.

For **each** successful `center_move`, `edge_split`, and supported `cross_view_move` row:

* Fingerprint builds: `V + 6` — the caller indexes V roots; authorization/application each build one source and two target root snapshots.
* Fingerprint node visits: `N + 2*n_source + 4*n_target = N + 6n` for these equally sized initial roots.
* `item_multiset_scans = 4`; `item_multiset_item_visits = 4*(2T) = 8T`.
* `transaction_candidates = 1`; `cloned_nodes = N`; `cloned_tab_items = 2T`.
* Intra-root center and cross-root center at V=4 have identical values for those emitted counters. That does not imply equal runtime: root-owner search positions, membership prefixes and later graph state differ.
* Resize changes external root bounds and performs no transaction/index capture: all listed transaction/fingerprint/multiset counters should be zero for its measured section.

| T | V | Predicted fingerprint builds | Predicted fingerprint node visits | Predicted item scans / item visits | Predicted candidate clones / nodes / tab items |
| --- | --- | --- | --- | --- | --- |
| 16 | 1 | 7 | 217 | 4 / 128 | 1 / 31 / 32 |
| 16 | 4 | 10 | 70 | 4 / 128 | 1 / 28 / 32 |
| 128 | 1 | 7 | 1785 | 4 / 1024 | 1 / 255 / 256 |
| 128 | 4 | 10 | 630 | 4 / 1024 | 1 / 252 / 256 |
| 1024 | 1 | 7 | 14329 | 4 / 8192 | 1 / 2047 / 2048 |
| 1024 | 4 | 10 | 5110 | 4 / 8192 | 1 / 2044 / 2048 |

`docking_work` resets after fixture setup and snapshots before independent result validation (`docking_cost.rs:769`). Thus setup clones and post-check item scans should not enter these values. All-root `project_root` uses empty weight overrides; the fingerprint-building branch in `layout.rs:603` is never entered. Normalization/export makes no transaction/proof calls. If observed counters disagree, inspect the exact call path/reset boundary before using this table for explanation.

The normal counter build intentionally includes the ledger's clone-volume traversal (`drop_resolver.rs:1108`). Formal timing compiles out `structural_work::update` work via `option_env!` (`:1238`). Counter-run clock readings are not timing evidence.

## 7. Falsifiable follow-up questions

1. At fixed T, V=4 should reduce the fingerprint-node-visit inventory relative to V=1 while preserving the global clone and multiset inventories. The counter table can falsify this; it predicts no elapsed-time ratio.
2. Edge should show the same emitted transaction counters as center while doing extra parent searches, creating two Nodes, and normalizing/validating a graph larger by two Nodes. Identical ledger counts cannot establish identical work or explain edge timing alone.
3. A CPU/allocator profile may reveal fingerprint materialization, per-node normalization/validation copies, multiset maps, or workspace clone/destruction as material costs. Source occurrence and asymptotic scope do not establish which dominates.
4. Layout/export timing must remain separately visible. They also traverse all current roots and allocate outputs; their costs cannot be attributed to transaction proof or rollback merely because they share the same total timer.

No source changes, Cargo, tests, timing, Git writes or nested agents were performed for this trace.
