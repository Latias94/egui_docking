# A1 owner-reference independent review

Latest status: FINAL IMPLEMENTATION SOURCE REVIEW COMPLETE, including the precise cold-root repair and four full-workspace diagnostic fixes. Root reports the four corrected cases green; production-core compilation and final qualification remain root-owned and incomplete. The final follow-up records the current source hashes and explicitly retains the negative cold-value cost evidence. No performance or completed A1 qualification is claimed.

Historical initial status: PLAN REVIEW COMPLETE; implementation review was then pending the worker's READY handoff. That initial read-only review used checkpoint `91f42ce` and `owner-ref-implementation.md`. The first source check preceded the creation of `graph/authority.rs`, so requirements below are not allegations that an unfinished implementation had already violated them. This lane ran no Cargo, tests, benchmark, timing, Git write or child agent; its only later repository edits were the three explicitly assigned fixture migrations recorded below.

## Plan verdict

The concrete plan is viable and appropriately scoped. Retain `graph + operation + canonical` as the trusted committed-graph owner, keep the same executor, use one generation-aware normative placement directory and opaque exact root references, and keep real geometry facts separate. No additional setter layer or generic proof framework is required. Cold affected-root pre/post comparisons are acceptable; common Select/Item Move completion must remain proportional to the actual change/frontier, excluding the candidate clone already present in this checkpoint.

The plan explicitly lists the important publication paths: Builder, standalone canonicalization, local Select/Item Move, every general prefix, general canonical completion, approved closes, owned follow-up and typed recovery. It also keeps layout/override migration with the separate layout owner. Those are correct decisions. The implementation is not yet qualified by this review; the following concrete obligations must be visible in the final code and behavioral tests.

## Timing of placement differs from timing of issuance

**Required clarification:** issuance can close at the command transition; parent placement must be current whenever the existing executor asks for a parent, including inside that command. “Refresh between commands” is insufficient for the ordinary Item route.

The current writer calls `parent_link` during edge insertion and wrapping (`operation.rs:3178,3296`). After insertion, local empty-source repair finds the actual source path (`canonical.rs:256`). In a same-root outer/inner wrapper case that path can include a new wrapper. If the directory still describes the pre-insert tree until the command ends, local repair can miss the new parent or repair an obsolete one.

Audit these concrete changes, without adding field-by-field setters:

- Same-axis insertion: the inserted node gets the correct parent/index, and every later sibling index shifts if an index is stored. The existing target's index may shift too.
- Wrapper insertion: the wrapper occupies the target's previous parent slot (or becomes the root); target and payload point to the wrapper. Root nodes have no parent. Install this actual placement before another lookup uses it.
- Child removal: update shifted sibling indices. A temporarily singleton split remains the actual parent until canonical repair removes it.
- Collapse: surviving replacement gets the old container's parent/root placement. Retire the collapsed node's directory entry.
- Same-axis flatten: reparent every promoted child to the surviving parent, update its actual index, and remove the flattened container entry. Merely deleting the old container is insufficient.
- Cross-root subtree transfer: retag the actual moved subtree; preserve the real parent links within it and replace its attachment link. This is O(moved subtree), an accepted cold cost.
- Empty merged/detached residue: a live node outside all current roots is **unrooted**, not still a member of its historical root or automatically a member of the target. Retired nodes have no directory entry; exact generations matter.

If the directory stores only parent NodeId and derives index from the real parent's children, there are fewer maintained index facts. Either representation is acceptable; do not claim a stored index is normative while leaving shifts implicit.

## Cold pre/post comparison and command staleness

The stated cold route is sound when it snapshots the actual affected state immediately before the **current** checked command, executes through the same writer, and completes current membership/issuance before checking the next command. It may walk affected roots in this cold route. It must not accidentally be called by the ordinary Select/Item Move route, including a supposedly harmless pre-completion helper that captures every root's values.

The comparison must include the exact-reference facts: root node and central; actual root presentation carrier; node identities/kinds; tab order, selection and MRU; split axis/children/order and f32 weight bits. Rect, peer roster/z order and policy are separate facts and do not renew graph issuance just because the overall command outcome changed.

Refreshing only current reachable nodes leaves a specific hole: entries for nodes which were in an affected pre-root but are now detached/unrooted can survive. Clear/reconcile the old affected ownership as well as assigning current members. Handle roots which were retired and therefore cannot be traversed after execution. Do not clear unrelated roots.

The second command must see a real earlier change even if a later command could restore the initial values. No transaction-origin acceptance rule, numeric version exemption, old-fingerprint fallback, or batch-tail-only renewal is valid. Conversely, checked no-ops and disjoint prefixes preserve later source/target usability. Same-root source and target independently check their own exact refs after the live root/node/membership checks, preserving source-first diagnostics.

Final general normalization is another owner transition. Its real pre/post state or concrete changed-root result must drive additional issuance and placement completion; comparing only transaction-initial base versus final normalized candidate is not an adequate replacement. A genuine one-command normalized no-op retains authority. A batch containing real A→B→A staged changes can have a false durable report while retaining new authority under the explicitly stricter ABA contract; equality must not resurrect the initial issuance.

## Every publishing entry point

The plan names these routes; the completed implementation should make their completion calls auditable:

| Entry | Review obligation |
| --- | --- |
| `canonical::build_workspace`, Builder `build` | Normalize/validate raw Data first, then create fresh root births and complete placement. A clone into raw Builder followed by build cannot silently inherit arbitrary old authority. Valid unchanged runtime clones do inherit authority. |
| `canonicalize_workspace` | A valid no-op canonicalization keeps existing root refs; any actual rewrite closes membership/issuance before replacing the committed value. |
| Single Select | Selected/MRU actual delta only, no placement rebuild or full-root before-image. Equal reselection keeps issuance. |
| Single Item Move + `finish_item_move` | Complete direct insert and final repair frontier; maintain intra-command placement; renew actual source/target once for the completed transition, deduplicating same root. Preserve the qualified input/output numeric gates and rollback. |
| General command loop | Actual per-prefix pre/post classification and placement refresh before the next source/target validation, including Resize's validate-all-before-write rule. |
| General full canonical finish | Complete collapsed/flattened/root-node/orphan changes without losing earlier staged staleness or renewing untouched roots. |
| `prepare_content_close` | Separate entry from `apply_command`; item removal, successor selection, source-root cleanup and full canonical finish all close owner state. |
| `prepare_surface_content_close` | Validate the complete frozen roster before deletion; retire each actual root and placement; unrelated surfaces remain unaffected. |
| `apply_owned_draft_command` / snapshot wrapper | Follows the same owner maintenance. A failed draft is discarded; neither its new refs nor the empty placeholder can be published. |
| `prepare_surface_recovery_transaction` | Typed exact recovery bypasses policy only. It still checks current refs and updates each prefix's owner state. Moving one root changes rosters but must not stale a different untouched contained-root source. |

A particular close detail deserves source review: item close currently recaptures the successor and invokes checked Select after the item was removed (`operation.rs:359`). If owner renewal is delayed across that private transition, do not let this recapture issue a supposedly current exact ref over incomplete state. Complete the relevant owner state before recapture, or select the already validated successor through the existing private mutation primitive while keeping the transition closed. This requires no new executor or public setter Interface.

Workspace durable equality must exclude issuance bookkeeping, while tests independently check normative placement. A Workspace equality pass cannot hide a misplaced directory row by designating metadata nondurable.

## Independent placement behavior oracle is required

The existing `validate_workspace_data` traverses durable graph/presentation storage. It does not currently know the new placement directory. Therefore the current local/general matrix's `candidate.validate()` success alone will **not** demonstrate the new metadata invariant.

Use a small test-local oracle, not a new production validator framework: traverse the actual roots and child vectors, independently collect `(NodeId -> actual RootId, parent/index)` using complete generations, and compare that with the owner directory or its real `capture_reference/parent_link/path_to_node` behavior. Include all live arena nodes so staged unrooted residue is checked, and check retired IDs so stale entries are not ignored. A full scene WorkspaceIndex's independently discovered members is also a useful real membership oracle, but membership alone does not verify parent/index placement. Do not build the expected set from the new directory itself.

Cover the discriminating shapes with the existing fixtures/matrix where possible:

1. Same-parent edge insert before the first/middle child, checking shifted original siblings and their real parent indices.
2. Empty-source ancestor collapse and same-axis flatten, checking promoted children, updated root node and removed old split IDs.
3. A real subtree crossing roots, plus an unaffected third root whose members and ref stay usable.
4. A merged tabs payload/removed-root residue in the private staged state, before full sweep: old live identity is unrooted, not an accepted member of either arbitrary root.
5. Same-root wrapper insertion followed by empty-source repair, checking the path used during completion includes the new wrapper.

A parameterized oracle call in the existing local-vs-general matrix gives broader value than many nearly identical fixtures. The work-ledger snapshot must end before this deliberate full test walk. Root owns executing all tests.

## WorkspaceIndex and selection rebinding

Ordinary index capture must bind to the **stored exact root issuance** from that index. Matching `WorkspaceVersion` and a currently existing node is insufficient. In particular, forwarding an old index request straight to `workspace.capture_*` silently grants fresh authority and defeats the same-version stale-index contract, even if target policy fields are now derived safely.

Require tests that retain an index, perform a real checked same-version selection/topology edit, and call its capture methods with the changed Workspace. The affected root must reject as stale; an unaffected root may remain usable if the index Interface is per-root bound. Also test a separately built equal Workspace with a foreign issuer, and recycled NodeId generation. These tests must retain the engine/index version deliberately; incrementing it first makes them trivial.

`after_selection` is an intentional, narrower rebind. It must associate the retained geometry with the **actual checked Select transition**, bind the old indexed root to that transition's previous exact ref, and bind the result to its current ref. Merely checking epoch/current node and reading a fresh ref from an arbitrary supplied Workspace is not enough. Make this provenance explicit in the concrete transition/caller rather than building a general proof object system. Rebind only the actual selected root; do not refresh unrelated indexed roots by reading whatever issuer happens to be current. Existing complete geometry stays shared; selected/MRU do not require cloning a root-sized geometry vector.

Test a foreign/mismatched old index or an unrelated intervening same-version edit as a rebind rejection, alongside a real checked Select that retains geometry, updates selected/title/policy binding and rejects the old literal command. A read-only scene index may retain old geometry facts for admitted presentation uses; that does not grant old index command capture permission.

## Scope discipline and next review

No unnecessary framework is visible in the written plan. Keep that property: one concrete command executor, one owner, one exact command authority protocol, one normative placement directory. Geometry NodeFingerprint must never regain execution authority after exact-ref mismatch. Extra caches, generic mutation providers, field setter taxonomies or a second rebinding Module are not required.

`layout.rs`, `layout/*` and the newly assigned `layout/override_tests.rs` mechanical migration stay with design_snapshot. Common may document the exact read Interface and required call-site change, but must not edit those files. Root remains the only Cargo/test/timing owner.

At READY, independent source review will trace actual hot completion call graphs for absence of full-root refresh, every entry above, stored-index binding, all transient placement updates and the independent test oracle. A passing graph-only suite or the previous checkpoint does not substitute for that review. This report accepts the plan's architecture, identifies the concrete completion requirements, and makes no claim that the ongoing implementation has compiled or passed them.

## Follow-up: independently replayed prefixes lose staged-batch expressiveness

Read-only follow-up requested by root. Evidence below was checked against `git show 91f42ce:<path>` to avoid treating concurrent implementation edits as the original contract. **The cross-branch rejection is real and deliberate under opaque issuance, but keeping only a preconstructed command Vec would lose a useful staged-batch capability. Preserve that capability through the actual candidate, rather than accepting refs from a predicted sibling branch.**

### Concrete original evidence

| Original source at `91f42ce` | What it actually does | Opaque-ref consequence |
| --- | --- | --- |
| `tests/recovery_policy_authority.rs:466–565`, `child_vacate_then_repopulate_in_one_tick_preserves_obligation_for_later_destroy` | Lines 524–531 clone the engine Workspace, execute `vacate` on the clone, and capture a post-vacancy source. It then submits `vacate` and `repopulate` to one real host frame and asserts **both** process successfully and the recovery obligation survives. | The real vacate issues another Arc than the simulated vacate; the predicted second source is stale. Preserve the same one-frame successful vacancy/repopulation behavior by capturing from the actual frame after its first append. |
| `tests/recovery_policy_authority.rs:598–676`, `root_vacate_then_repopulate_in_one_tick_preserves_the_exact_anchor` | Same prediction pattern at 629–636 for the host root; success must preserve its exact recovery anchor and binding. | Same cross-branch refusal. Moving to two separately published frames or changing the second outcome to rejection would not preserve the tested behavior. |
| `tests/tick_final_surface_vacancy.rs:792–818`, `native_vacate_command`, used by `same_tick_native_first_live_admission_then_vacancy_releases_the_child` at 1111 | The helper executes the pending native-create command on `future`, captures the not-yet-live root, and returns a later Rehome command. The test submits it after actual first-live admission and requires the child release/retirement behavior. | A newly created root also has a distinct birth in the real branch. Capture the source from the actual frame after the admission/observation reduction that created it; retain the same final vacancy and release assertions. |
| `src/engine/tests/native_lifecycle.rs:791–818,3339–3364` | These `future` clones are used to compute real recovery-policy facts for an obligation, rather than to return a post-prefix NodeSource for a later command. | Do **not** label every predicted Workspace invalid. Value-based policy/placement calculations are separate from replayable command authority; these inspected helpers do not exhibit the specific stale-ref problem. |

The existing ordinary WorkspaceTransaction tests also prove that old same-root references must reject: `tests/transaction_atomicity.rs:260–286`, `operation/tests/checked_move.rs:73–140`. Those tests remain rejection tests. Other successful general tests use no-op/disjoint prefixes or commands with no future root reference, for example `operation.rs:3897–4049` and `tests/rootless_structural.rs:251–302`; they do not prove that dynamic same-root staging remains expressible.

I did not find an existing successful WorkspaceTransaction net-zero select-away/select-back test in the inspected original sources. Do not present such a test as already present. The old public Interface nevertheless expresses it directly: apply Select B on a predicted clone of a two-item `[A, B]` pane, capture A on that clone, then apply `[Select B, Select A]` to the original. The old exact value proofs agree after the first prefix; the final selected/MRU values equal the original and the transaction report is false. Opaque independent issuance necessarily rejects the predicted second source. This is a new required behavior test for the replacement staging Interface, alongside an explicit test that the old sibling-branch ref is rejected.

### The engine already has a real incremental candidate; commit does not replay it

`CoreHostFrame::append_input` calls `append` (`engine/host_frame.rs:2253–2265`). Its semantic branch immediately invokes `self.candidate.reduce_sequenced_input` and stores the reduced outcome (`:2490–2530`); it does not merely queue the command. `frame.view().workspace()` reads this same candidate (`:526–531`, `:1403`). A second command can therefore capture its source after the first append, on the actual candidate lineage.

`DockEngine::prepare_host_frame_owned` destructures that already-reduced `candidate` and `reduced_inputs` from the frame (`engine/reducer.rs:203–260`). Preparation applies the later fences/settlement, and prepared commit moves the candidate into the engine (`engine/host_frame.rs:3117–3228`). There is **no final replay of the semantic WorkspaceCommand Vec** on another fresh candidate. Root refs minted in the actual frame survive that ownership transfer.

Migrate the two recovery-policy tests using the existing TestInputStream/frame helpers: begin one frame, append `vacate`, read/capture `repopulate_source` from `frame.view().workspace()`, append repopulate in that same frame, then perform the existing complete presentation and commit. Keep the version/arrival/vacancy protocol and all successful recovery assertions. The current `support::submit_inputs` helper takes a preconstructed input list (`tests/support/mod.rs:1010–1032`); an inline use of the existing stream/frame is enough, with no new test protocol framework.

For native first-live vacancy, capture from the actual post-observation/admission frame candidate, not from a published later frame, so first-live admission and vacancy can still settle in the same intended host frame. Announce these integration-test paths before expanding the worker's current src-only ownership.

The existing pointer activation pair does not require a new batch mechanism: `engine/pointer_transaction.rs:3803–3871` constructs Raise then Select, and Raise changes only peer roster order, outside exact root issuance. Both initial graph refs remain current if A1 implements that exclusion correctly. Likewise the complete recovery program's root rehomes are disjoint. These specific existing Vec programs are not counterexamples to opaque refs.

The journal's post-move delivery activation is another valid owned path: `stage_journal_workspace_draft` captures each activation against its actual owned post-move Workspace and applies it there (`engine.rs:3474–3515`). `PreparedDropMutation` retains the actual candidate Arc and consumes it at release; it does not replay its post-move graph merely to mint new refs. Preserve those paths. They solve their own composed operation but are not a public, late-normalized Workspace batch Interface.

### Minimum new Interface for true Workspace staging

The public `WorkspaceTransaction` currently owns only `Vec<WorkspaceCommand>`; `apply` clones the initial Workspace and runs that Vec (`transaction.rs:25–64`, `operation.rs:659–695`). There is no public place to capture a command from the actual candidate after a successful changing prefix. Add one concrete staged entry, keeping ordinary preconstructed Vec application for literal captured commands and preserving its stale-reference rejections.

A small Interface can combine capture and execution rather than exposing a mutable draft:

```rust
WorkspaceTransaction::apply_with(&mut live, &policy, |batch| {
    batch.push(|current| {
        Ok(WorkspaceCommand::Select {
            source: current.capture_item_source(root, tabs, b)?,
        })
    })?;
    batch.push(|current| {
        Ok(WorkspaceCommand::Select {
            source: current.capture_item_source(root, tabs, a)?,
        })
    })?;
    Ok(())
})?;
```

This is an illustrative new method, not a request to adopt the exact name. `batch.push` invokes the constructor exactly once with a narrow capture/read view of the **actual** candidate, then executes the produced command exactly once through the existing checked executor. Capturing does not authorize execution: current source/target/policy validation still runs. The view exposes familiar capture/root/node reads and no mutation, issuer constructor or conversion into a committed Workspace.

Do **not** expose an unnormalized `&Workspace` publicly as the staged view. The general draft can contain empty/singleton/same-axis/unrooted intermediate state, and the parallel sealed-runtime layout/compiler work relies on public Workspace being canonical. A small concrete `TransactionCapture<'_>` view can call the same owner capture Implementation without implementing Deref to Workspace or granting canonical layout access. This is a read facet, not a new executor, general proof provider or setter layer.

The owner Implementation owns one candidate, the existing item ledger, outcomes and first failure. Factor the current general loop body into its existing stage operation: validate declared item, call `apply_command`, complete current issuance/placement, apply item delta and record the outcome. Both the literal Vec route and the staged constructor route use it. Preserve failure index and terminal failure: catching/ignoring a failed `push` in user code cannot make a partially modified candidate publishable. Subsequent capture/execution must refuse after that first failure, and completion returns it.

After the closure succeeds, run the **existing single** prune/canonicalization/strict validation/item reconciliation finish, compute exact final net change, and return the actual completed candidate. The crate-private prepare counterpart can hand that candidate to engine publication barriers; public apply consumes it immediately. A prepared result stores the candidate/report, not a Vec intended to be replayed during commit.

Do not build this by repeatedly calling `apply_owned_draft_command`: that helper currently runs whole general settlement after **each** follow-up (`operation.rs:716–737`). It would canonicalize/prune prefixes and change late-normalization semantics. Do not execute a preview closure on one clone merely to save its constructed commands and then feed them through another Vec; that recreates the exact cross-branch failure.

The new staged route can always use the cold general finish. Existing single Select/Item Move literal commands retain their qualified fast preparation. This keeps the implementation bounded: one additional construction Interface over one executor and finalizer, with no batch-origin exemption, deterministic token interning, issuer cache, transition replay trust, or compatibility value-proof path.

### Necessary acceptance before claiming staged batches preserved

1. Actual-candidate Select B then capture/Select A succeeds atomically, both real outcomes occur, selected/MRU return exactly to base, and final `TransactionReport.changed()` is false. Under strict ABA the old base root ref may still be stale; distinguish this from report equality.
2. A source captured after the same prefix on a separate predicted clone rejects. A source captured from the actual stage succeeds; an old same-stage pre-prefix source still rejects at the correct index.
3. A later command failure discards the whole stage even if its Result is ignored by the construction closure; no partial candidate/report publishes.
4. A graph-changing prefix plus the existing zero-width-contained late-output witness is accepted through command staging and fails at final canonical/strict completion with the same typed error and complete rollback. Also retain a legal prefix whose transient structure is normalized only once at finish.
5. Preserve the two real recovery-policy success tests and native first-live/vacancy success through actual frame capture, including obligations/anchors/bindings/release effects. Do not replace them with rejection tests or split their intended host-frame atomicity.

Conclusion: the current engine incremental candidate is sufficient to migrate the identified engine callers without broad new engine API. It does not replace the missing Workspace staged-batch construction capability. A thin actual-candidate batch entry, sharing the current general body and finalizer, is the smallest complete bridge. A1 can legitimately break cross-branch command replay; it cannot claim equivalent staged expression if no actual-candidate replacement exists.


## Authorized actual-frame test migration — READY, stationary

Root assigned precisely the two actual files `crates/dockspace/tests/recovery_policy_authority.rs` and `crates/dockspace/src/engine/tests/native_lifecycle.rs`. The former is registered by `behavior_tests.rs` using its tests-directory path; there is no `src/engine/tests/recovery_policy_authority.rs` file. No other source file was edited by this lane.

Migrated the two successful recovery-policy vacancy/repopulation tests to the existing real TestInputStream/CoreHostFrame flow. Each appends the first vacate command, captures the second source from that same frame's current `view().workspace()`, appends the second command in the same semantic frame, completes its existing retained/unavailable presentation obligations and finishes once. The original initial expected version and both-success, binding/anchor/obligation, later-destruction and item-preservation assertions remain. The obsolete WorkspaceTransaction import was removed. No predicted scratch Workspace, cross-branch source replay or second published frame remains in these two cases.

Migrated `tick_final_vacancy_suppresses_focus_for_a_newly_visible_native_binding` in the owned native-lifecycle file. Removed its `after_native_commit` scratch clone and predicted post-create source. The actual first-live presentation observation is still sealed before semantic input; the source is now captured from that sealed frame's actual newly admitted root. Kept the original expected-after-native-commit version and added a direct equality assertion against the actual frame version. The same frame appends rehome and finishes, preserving release, missing native surface/viewport, suppressed RequestFocus and cleared pending-activation assertions.

The other two native-lifecycle `future` clones compute recovery policy obligation facts rather than carrying a post-prefix NodeSource into replay. They remain intentional value calculations and were not changed merely for containing a clone.

Only direct rustfmt with `skip_children=true` and read-only `git diff --check` were run; both completed successfully. No Cargo/build/tests/timing, Git mutation or global formatting. No pass is claimed. Source is now stationary for root's `cargo check --tests` and focused/full serial verification. Common's handoff files were not touched, and this lane will not edit either owned test again during that verification.

One already identified follow-up remains **outside this assignment**: `crates/dockspace/tests/tick_final_surface_vacancy.rs`, helper `native_vacate_command` and `same_tick_native_first_live_admission_then_vacancy_releases_the_child`, still capture a post-create source from an independently executed `future` clone. It needs the same actual post-admission frame capture migration, retaining its successful same-tick ReleaseChild/retirement assertions. Root must assign that exact path before it is edited; this lane did not expand into it.

Handoff source SHA-256:

- `crates/dockspace/tests/recovery_policy_authority.rs`: `9137f49afce84fc067257031c60effac7edc059de94ea3f09fdcb6007bcea350`
- `crates/dockspace/src/engine/tests/native_lifecycle.rs`: `67f3c59bb69d7ab855683a13916d287bb334c6d5372b7a80908c404e6fc28a6c`


## Third authorized fixture migration — three files READY, stationary

After root stopped the first check and assigned `crates/dockspace/tests/tick_final_surface_vacancy.rs`, removed its prediction-only `native_vacate_command` helper and obsolete transaction import. `same_tick_native_first_live_admission_then_vacancy_releases_the_child` now captures its rehome source from the actual post-first-live-seal `frame.view().workspace()` and executes it in that same frame. Existing successful command, absent surface/viewport, exactly one ReleaseChild and exact BindingRetirementStatus assertions remain intact. No core, helper module, or other test was edited.

The prior two assigned files remain unchanged since their previous handoff. Direct rustfmt (third file only, skip_children=true) and diff whitespace checks passed. No Cargo/tests/timing were run. All three files are now stationary; root/common can continue their serial compile integration. Read-only owner-reference review continues independently.

Three-file SHA-256 receipt:

- `crates/dockspace/tests/recovery_policy_authority.rs`: `9137f49afce84fc067257031c60effac7edc059de94ea3f09fdcb6007bcea350`
- `crates/dockspace/src/engine/tests/native_lifecycle.rs`: `67f3c59bb69d7ab855683a13916d287bb334c6d5372b7a80908c404e6fc28a6c`
- `crates/dockspace/tests/tick_final_surface_vacancy.rs`: `42b474bd68826f558ba57f006b24b5a335b11f6e0c337c99c0def42d7fbeda01`

## Independent final Implementation review — second-check source

Read-only review on 2026-11-07, after common's ten-test acceptance handoff and root's two test-call corrections. **No confirmed runtime P1 or scope blocker was found in the inspected A1 Implementation. The previously identified staging expressiveness, stale-index provenance and placement-oracle gaps are closed in source.** This is a source-review verdict, not a compilation, test or performance qualification. Root owns those checks. No repository file was changed during this review; the three migrated fixture files remain stationary and retain the hashes above.

The actual Module is still the concrete `graph + operation + canonical` owner. Its Interface adds exact root issuance and an actual-candidate transaction construction entry; the existing executor and canonicalizer perform the work. There is no second executor, generic proof provider, transition interning, numeric-version exemption, mutable Workspace escape, or value-proof fallback after an exact-reference failure. This preserves Depth and Locality without adding a setter hierarchy.

### Exact authority, target facts and stored indexes

`graph/authority.rs:19,49,98,230` implements exact `Arc` identity. Unchanged Workspace clones inherit the same root refs; Builder completion issues independent refs. A changed Select issues a new ref with one weak predecessor and the selected tabs identity, rather than retaining a strong history chain. An actual published A→B→A may reject the original ref even when durable Workspace equality is true; that stricter breaking rule is deliberately tested.

`workspace.rs:801` checks current root existence, the complete live NodeId generation, normative root membership, and only then reads the current root ref. `verify_reference` compares that ref with the supplied one. Move checks source first, then target independently (`operation.rs:1164,2814,2833,2866`), including the same-root case. Wrong-root nodes cannot gain authority from a valid root token. Edge scope checks and current target-shape checks remain present. Typed SurfaceRecovery still bypasses policy only: it reaches the same exact source/target and relation checks, after its existing exact-program/roster gate (`operation.rs:559,1549,1703`).

**TabTarget and EdgeTarget still contain crate-visible `surface`, `central`, and `rule` snapshot fields. They have not been deleted or made unforgeable.** Validation captures the actual current target, compares the entire target value, and rejects forged facts before current-policy eligibility. The existing forged-field test remains a real rejection oracle (`operation/tests/checked_move.rs:184`). This is sufficient for A1; removing those few fields is not a condition of this review.

`WorkspaceIndex::capture_reference` checks the index's stored exact root ref against the supplied Workspace (`workspace.rs:298`); it does not forward an old index request directly into an unrestricted fresh capture. Indexed outer-edge capture has its own exact check. This closes same-version and foreign-Builder capture without requiring the engine version to advance. `after_selection` additionally checks the other indexed roots, the root roster, and either an unchanged ref or the concrete Select predecessor/tabs relation (`workspace.rs:165`). Intervening root edits and foreign issuers cannot silently refresh index command authority. The engine's existing allocation-and-version binding remains in `engine/presentation_authority.rs:121`.

### Hot completion and local canonical frontier

The single Select and policy Item Move preparation branches remain (`operation.rs:601,614`). Hot Select/Reorder/Resize/Item execution is explicitly excluded from cold root snapshots (`operation.rs:1520`). Select renews only on a real selected change; the sealed MRU invariant makes selecting the already-selected item a true no-op. Reorder and Resize renew only roots with real value changes, after their checked writes. A one-command normalized-equal Move restores original issuance only after complete durable Workspace equality; it does not lend initial refs to later batch commands.

The placement updates cover the actual structural transitions:

| Concrete transition | Inspected completion |
| --- | --- |
| Detach a split child | `operation.rs:3062` unroots the actual removed subtree and refreshes the surviving direct roster, including every shifted sibling index. |
| Same-axis edge insertion | `operation.rs:3244,3289` assigns the payload to the destination and refreshes the actual parent's direct children after insertion. |
| Insert a wrapper | `operation.rs:3352` saves the original parent before rewriting, places the wrapper in that slot, and refreshes its two actual children immediately. Subsequent source-path repair therefore follows the new wrapper. |
| Cross-root subtree transfer | Detach clears the old attachment; edge insertion retags the actual payload subtree. Cold completion refreshes the complete actual roots before the next staged command. |
| Merge tabs into another pane | The detached empty live source remains unrooted until the final canonical sweep; it cannot be captured under either arbitrary root. |
| Local collapse / flatten | `canonical.rs:317` forgets removed result nodes, places the actual surviving root node with no parent, then refreshes every surviving output split's real children. Flattened children were entered in the rewrite results and are covered by retirement cleanup. |
| Root/surface retirement | `operation.rs:3564` detaches presentation, removes the root, unroots its actual tree and removes its issuer; arena removal also forgets placement (`graph.rs:478`). |

The local Item normalizer retains the qualified floating-point safeguards (`canonical.rs:244,303`): the inserted split's pre-rewrite scalar/cardinality/sum check, every final live rewritten split's strict output check, and the full original diagnostic only on a failed frontier. The two exact `weight_sum_witness` tests remain present. This A1 diff does not loosen epsilon, introduce a global success-path fallback, or bypass those failure diagnostics. Source and optional outer-target repairs retain stable root ordering. I found no new canonical publication gap in these paths.

### Cold commands and every publication route

The current cold Implementation snapshots **all existing roots**, plus a requested new root identity for creation, at each cold command (`operation.rs:1532`). It is broader than a touched-root-only design and must not be described as O(frontier). It is within the agreed cold-general scope. `finish_cold_roots` compares actual raw root values and refreshes placement/presentation before returning to the next command (`graph/authority.rs:300`). Clearing previous membership is conditioned on its still belonging to the previous root, so an earlier destination refresh is not undone when the source root is processed later.

Cold equality includes root node/central, presentation carrier, complete node identities/kinds, tab order/selected/MRU, split axis/children and exact weight bits. Contained rectangles, peer roster order and policy are intentionally separate facts; a roster-only Raise does not invalidate graph refs. The raw before/after values are temporary classification data, not retained alternative execution authority.

All inspected publishing routes complete ownership in the concrete owner:

- Builder/import calls canonical completion before `Workspace::issue_storage` (`canonical.rs:229`); Builder remains raw and unissued.
- Standalone canonicalization and general final canonicalization capture actual before values and reconcile after rewriting (`canonical.rs:188,219`). Every general command prefix has already completed its own authority; final normalization may renew again for a real topology repair.
- Surface close removes each root through the same retirement helper, then canonicalizes the surviving graph (`operation.rs:183`).
- Item/root close has an outer cold transition covering the actual removal as well as inner canonical repairs (`operation.rs:320`). Its temporary successor selection is owner-internal and the completed result is the only published value.
- Owned follow-up calls the same checked `apply_command`, then the same full finalizer, and keeps an empty valid placeholder if the discardable candidate fails (`operation.rs:797`).
- Prepared transactions contain the actual candidate; `into_parts`/`publish` move that candidate after report construction (`transaction.rs:206`). They do not replay a command Vec at publication. Existing engine publication barriers remain after candidate preparation.

No production mutable access to `Workspace.storage` was added outside the existing graph owner. Read-only `storage()` and test-only Builder diagnostics do not mint an unchecked Workspace. The normative directory is part of the cloned/discardable candidate; a rejected candidate cannot change the original Workspace's refs or placements.

### Actual staging and behavior-oracle closure

`WorkspaceTransaction::apply_with` / `WorkspaceTransactionStage::push` exposes one narrow actual-candidate construction Seam (`transaction.rs:60,90`). `TransactionCapture` exposes node/root/MRU/capture reads without Deref, Workspace conversion, layout access or mutation. Both literal Vec execution and incremental capture use `TransactionDraft::push_command` and `finish` (`operation.rs:668,697`). Each constructor runs once against the actual completed prefix. A failed capture/execution poisons the draft, refuses subsequent constructors and prevents final publication even if user code ignores the push Result. Prune/canonicalization/item reconciliation run once at the end. Thus the rejected predicted-sibling protocol has a working replacement without a batch-origin exception.

The ten new owner-reference tests are registered and inspect real behavior, not merely token constructors. They cover independent builds, committed ABA, net-zero actual-stage selected/MRU with a false final changed report, predicted-branch rejection, ignored-stage failure, one-final-normalization and late invalid-output rollback, same-version/foreign indexes, Select rebind lineage and unrelated-root interference, complete-generation reuse, cross-root subtree transfer, unrooted staged residue, and selection-versus-resize geometry permissions.

The independent placement oracle (`operation/tests/owner_reference.rs:32`) builds expected root/parent/index/path facts from real root records and child vectors before consulting the directory. It checks every live arena node against every root, actual presentation ownership, and unrooted residue. It is invoked for every successful existing local Item matrix result (`operation/tests/local_item_move.rs:148`), after the structural-work snapshot. This closes the specific gap where durable `Workspace::validate()` alone would overlook metadata corruption. Dead-node behavior is separately guarded by complete-generation arena existence; the oracle does not directly enumerate private SparseSecondaryMap storage.

The three assigned engine fixtures preserve their original successful same-frame behavior through actual `frame.view().workspace()` capture. Remaining native test scratch clones only compute recovery-policy value facts (`engine.rs:5356`); they do not replay post-prefix NodeSource authority.

### Retained costs, qualification and small documentation note

This slice removes full value-proof construction from command capture/checking and removes the command-only WorkspaceIndex helpers. It intentionally retains the whole Workspace candidate clone, added candidate-directory clone, existing final durable equality scans, complete scene geometry fingerprints, and broad cold root walks. `WorkspaceIndex::after_selection` still clones/updates the real selected root fingerprint; it is not an O(1) geometry-representation change. No lower bound, speedup or 2× completion claim follows from this source review. The docking-cost request still captures its real source/target within the existing request phase (`docking_cost.rs:141`), rather than moving acquisition into fixture setup.

Only a small optional prose cleanup remains: `command.rs:38` and `drop_resolver.rs:985` still use older “captured reference/proof” wording around NodeFingerprint. A final description should distinguish the retained geometry value from current exact command authority. This is not a runtime blocker and does not justify changing frozen code.

Root should qualify the ten owner tests, checked/local/general and both numeric-frontier regressions, then existing close/recovery/presentation/native suites and unchanged complete-operation timing/state exports. I ran no Cargo, test, benchmark or measurement command. The source evidence supports proceeding with root's serial qualification, not skipping it.

### Review receipt

The second-check receipt predates root's two owner-test call corrections. At this review, the main receipt hashes for workspace.rs, operation.rs and local_item_move.rs still match; owner_reference.rs is now `8312009e24b512c4aea736b9e6c9cd5c3bdf9009e4fe8786296f836828a70df4`. Additional reviewed owner files:

- `graph.rs`: `de1db6457502140b905691e5e3a31f8164d53132389a33200c76480a68947f23`
- `graph/authority.rs`: `3468feb5f040eae4669f55454bedcca4b4c2ce84ad1db8db4203192a137641b7`
- `operation.rs`: `3fea59deca6915c7c7430254c2b90de16b697576bf662c9b5be0d68fb82fd608`
- `canonical.rs`: `949fc41b6dc693ae809803f6d89fb906e040eecb0526ddf4fe41b8167488e584`
- `workspace.rs`: `6e8ee75de58f56379a57d008bbe1573bfc76bf836f551946d5701e4402d5533c`
- `transaction.rs`: `08d3f1324d95b38350be32d88490db67c22aff5e8120896f2ffd94aeb5e8a065`

All three fixture hashes listed above remain unchanged. This receipt records inspected source bytes and does not replace root's final frozen-build receipt.

## Follow-up: precise cold roots after root's targeted diagnostics

This follow-up supersedes the earlier second-check all-root cold-loop assessment and source receipt for `operation.rs`. Root reports that the targeted run ended with 81 passes, a `local_selection` mixed-batch error-equality test failure, and an interrupted 1024-root recovery test after 118.593 seconds (its earlier debug run was approximately 0.492 seconds). The new ten owner-reference tests, placement matrix, both f32 regressions, actual 10k-depth and layout cases were reported green. These are root-reported checks; I did not run or independently time them, and the interrupted test is not a completed performance result.

**Confirmed regression, fix present in source, validation pending:** the superseded `apply_command` took and completed before/after facts for every root on every cold command. Each root capture also called the actual presentation roster lookup. For a recovery program with R roots and O(R) commands, those nested loops can perform cubic roster work. Allowing whole-graph work in the cold finalizer did not make that per-command multiplication acceptable. The independent pre-targeted source review identified the all-root breadth but did not flag this amplification; the targeted diagnostic exposed a real regression that must close before qualification.

The updated exhaustive match at `operation.rs:1520` selects only the actual command roots, while preserving the same executor and finish mechanism. I checked it against every WorkspaceCommand variant and its concrete writer:

| Command family | Roots classified before/after | Why this covers the actual writer |
| --- | --- | --- |
| Move Tabs/Subtree | Payload root and target root, deduplicated by `begin_cold_roots` | Detach can retire the source; insertion changes the target. Same-root commands compare once and retain issuance on an actual no-op. Cross-root retagging still completes before the next prefix. |
| Open | Target root | A new item and any wrapper affect only this existing root. |
| AttachEmptySurface | Requested new root | Before is absent; after refresh establishes its real members, presentation and issuer. A collision fails without publication. |
| CreateSurfaceRoot / CreateContainedRoot / InstallMainRoot | Requested new root plus the `RootContent::Move` donor root, if any | Item/node detach can change the donor, and the actual destination must gain membership and issuance. OpenItem has no donor. Roots of unchanged peer presentations are not mutated. |
| RehomeRoot / PromoteContained | Source root | Node membership stays in this root, while its presentation carrier can change. Peer roster edits do not change another root's graph/carrier facts. Actual carrier equality retains a true no-op's ref. |
| RemoveEmptyRoot / RetireEmptySurface | No cold snapshot | Their concrete removal helper already unroots each removed tree and retires its issuer. No surviving root has its graph or carrier rewritten. Final normalization sweeps unrooted arena residue. |
| UpdateContainedRect / UpdateContainedPresentation / RaiseContained | No cold snapshot | Their checked writes affect rectangle and/or peer order only. Root graph and presentation carrier remain unchanged; rect/roster preconditions remain separate and still checked. They must not invalidate refs merely because a presentation outcome changed. |
| Select / Reorder / ResizeSplits / Move Item | No cold snapshot | The already-reviewed concrete hot writers complete their actual root changes directly. |

I found no missing donor/new-root, retirement, same-root dedup, no-op, rect or Raise case in this match. Source/target reference validation remains independent, and every included cold root finishes before a later staged command. The `finish_cold_roots` conditional clearing still protects subtree membership already reassigned to another root. No version shortcut, cache or batch-tail-only authority rule was introduced.

The final general canonicalizer, standalone canonicalization and approved close still use broad cold completion once at their existing completion points. Their remaining root/presentation walks are real residual costs; they must not be described as removed. The new match removes the all-root **per-command** multiplier. I make no prediction that a specific time or total complexity target has now passed; root must rerun the interrupted scale case and ordinary qualification.

The mixed-batch test correction is also appropriate in source. Its two independently executed candidates necessarily issue different current Arcs on the same prefix. Comparing complete StaleNode errors across those candidates incorrectly requires equal newly minted issuers. The updated assertion preserves error kind, command index, role, node, the original expected ref, matching current RootId and each branch's actual expected-versus-current inequality. It does not turn a stale command into a success or weaken publication rollback. The corrected test still requires root execution.

Current verdict: no additional confirmed correctness omission in the precise affected-root fix. The broad-loop regression is **resolved in source, pending serial validation**, and the earlier broad-loop receipt is superseded. All three fixture files remain stationary; this follow-up changed only this report.

## Final repair review: retained cold cost and four workspace failures

Read-only review of common's stationary `workspace-four-fixes-source.json` handoff. **No new blocker found.** Root reports all four corrected cases executed green and production-core compilation underway. The preceding workspace run had 2219 passes and four failures; the report does not turn that earlier run into a full pass. Checkpoint `91f42ce` having six green CI jobs also does not qualify this uncommitted A1 diff.

The four repairs are test-only changes. They neither remove runtime checks nor add a runtime bypass:

1. `engine/tests/host_authority.rs:1648` now expects 0 fingerprint builds and 0 node visits for the existing real ordinary tab press. This particular fixture presses the already-selected item, so it prepares no Workspace transaction. Its two source captures use actual root issuance. The existing arm, pointer watermark, clone and real release assertions remain unchanged; release still asserts zero fingerprint builds. This is a valid common-path zero count, not evidence that all background, recovery or changed-selection flows have zero whole-value work.
2. `drop_resolver.rs:2854` still resolves a real background target, explicitly asserts an `InstallMainRoot` command, checks the single geometric winner/transaction/candidate clone, and checks the actual final candidate node count. Its revised counts account for cold effect classification and general canonical completion. The old zero assertion on this path was false.
3. `engine/tests/native_lifecycle.rs:206` retains the actual presented background target, winner/index-reuse and transaction assertions. Its revised 9-build/9-visit baseline represents two original singleton-node roots plus one new root, not an index rebuild. The donor retains another item, so no old arena node retires here. Other lifecycle behavior remains unchanged by this repair.
4. `operation.rs:4148` now appends through a current captured target and real checked Open. The test no longer mutates canonical tabs/MRU storage without renewing issuance. `changed_source_item_rejects_frozen_close_without_mutation` still requires `TransactionError::Precondition::StaleSurfaceRoster` and exact rollback (`operation.rs:4267`); unrelated-root Open still permits the pending close and preserves that unrelated item (`:4249`). No assertion was changed to accept a different error or rejection of the formerly successful unrelated-root case.

For the scale background fixture let R be the original root count (equal to `spec.surfaces` here), S the donor's original reachable node count, and V the original total node count. The affected-root transition captures the donor before, donor after, and the new-root birth: three builds. General completion captures all R+1 roots before and after canonicalization: another 2(R+1). Thus **builds = 2R+5**, not zero. The source singleton leaf becomes empty, and its binary parent collapses at final normalization. New-root birth adds one node, leaving V+1 nodes before and V−1 afterward. Donor captures visit 2S nodes, giving **visits = 2S+2V+1**.

| Fixture | R | S | V | Builds | Visits |
| --- | ---: | ---: | ---: | ---: | ---: |
| 16 panes, balanced | 1 | 31 | 31 | 7 | 125 |
| 128 panes, balanced | 8 | 31 | 248 | 21 | 559 |
| 128 panes, hot-root skew | 8 | 226 | 247 | 21 | 947 |
| 1024 panes, balanced | 32 | 63 | 2016 | 69 | 4159 |
| 1024 panes, hot-root skew | 32 | 1922 | 2015 | 69 | 7875 |

These source-derived formulas agree with the unchanged fixture construction and revised concrete baseline table. Their constants are structural work counts, not phase timings or allocation measurements. A1 removes command authority materialization on common Select/Item Move but **does not remove full value snapshots from cold root creation/general normalization**. Their carrier discovery still walks actual presentation rosters. The present count tests do not prove linear wall time or rule out that remaining quadratic roster factor. Replacing final canonical effect classification is a separate slice and is not part of this repair.

The additional cold coverage is meaningful: `a_new_root_and_its_donor_issue_actual_refs_before_the_next_prefix` (`operation/tests/owner_reference.rs:564`) creates a contained root from a moved item on the actual stage, immediately captures/executes against that new root and the changed donor, checks the donor ref differs from its old issuance, keeps both genuine no-op Select outcomes, and verifies an unrelated root ref plus the independent placement oracle. The unchanged 16/128/1024-root recovery fixture (`surface_recovery.rs:1669`) still executes every real command and bounds fingerprint builds/visits by `6*(command_count+1)`, while retaining the existing whole-item reconciliation bounds. It detects the removed per-command/all-roots multiplication; it does not hide it by shrinking the fixture. The exhaustive cold-root match reviewed above remains unchanged in scope.

Final reviewed SHA-256 receipt, independently read from the stationary files and matched to common's four-fix receipt:

- `drop_resolver.rs`: `57d8c465dfca14388ab26b99d1c13be586522559e05bf5d42d277419b78c0f44`
- `operation.rs`: `1af34d0400c32d0649a563ceb5c0d147aec36eca543fc4dd1ce33671281e92a8`
- `engine/tests/host_authority.rs`: `e20fd62ad9ec31b93c7b5f79ba3013b6267800c78f39d8105f19fa90c424f574`
- `engine/tests/native_lifecycle.rs`: `a2dc00f3a9033e84abcad3c30d1c1f681e8479f7839c1834b912684301041146`
- `operation/tests/owner_reference.rs`: `7d0dd09f1404aa51b7c691297c286b9d5b85d1762f498899b7b125fc58a5174f`
- `surface_recovery.rs`: `0267e4585ec42fea87ff4b9ec866f06f1c7e8d950fad87c0a7bbb7deb727defd`

The previously reviewed authority.rs, canonical.rs, workspace.rs and transaction.rs hashes remain unchanged. The native-lifecycle file's earlier migration hash is superseded only by common's explicitly reassigned baseline repair; this reviewer made no further repository edit. No Cargo, timing, test, Git write or nested agent was used in this follow-up. No additional API work is required by the findings above; await root's frozen production/full-suite and performance qualification.
