# Independent source review of closed cold effects

Status: source review accepted; no confirmed correctness or scope blocker. This is not compilation, test or performance qualification. Repository source and all original proposal artifacts were read only. Review date: 2026-11-07.

Reviewed inputs were common's `cold-effects-candidate/README.md`, all seven exact base/candidate file pairs, the complete `candidate.patch`, relevant live query/validation code and existing behavioral tests. The original seven source pairs and all three original patch hashes match their receipts. The original complete cold patch is `e2585debdd47e94b2582e628b07575ea9259d9636b115cbb89cf38e14f1211c6`.

Line references below use the composed `proposed/crates/dockspace/src/` tree unless explicitly called live or dependency-only. Composition does not introduce a different cold implementation.

## Canonical effects are actual writes

`canonical.rs:503` accumulates the boolean from each completed postorder rewrite. The input gate still rejects shared children, root-as-child relationships, shared root nodes and cycles before this traversal. Consequently a removed child identity or flattened container cannot reappear elsewhere in the same root and cancel the structural change. The traversal allocates no new NodeIds.

`rewrite_split` at line 554 has an exact unchanged branch: at least two retained children, no changed child identity and no same-axis child returns `(Some(node_id), false)` without rewriting the Node or normalizing its weights. It preserves the existing f32 bits. Its true branches either remove a node, change the child roster, or replace a same-axis child container with its distinct descendants. An emptied unprotected Tabs also returns true and is subsequently swept. OR is therefore exact for the root value at canonical entry/exit; it is not a prediction that a command will eventually change values.

The original input gate, ascending root order, weight arithmetic, normalization expression, item multiset comparison and strict output gate remain. `CanonicalEffects` is returned only after these gates succeed (`canonical.rs:418`). Recording retired generational IDs inside the existing immutable reachability sweep permits exact placement removal without collecting before-values. A swept empty orphan can retire without changing any rooted value or unrelated issuer.

`complete_runtime_rewrite` at line 387 first forgets retired placement rows, then walks only actual rewritten roots and mints their authority. An unchanged root receives neither a placement walk nor a new issuer. Canonical rewriting cannot edit a presentation carrier, so reusing the already completed concrete carrier is sound. Builder/import still consume the complete canonical/strict path and issue a new owner; they do not publish the effect receipt as an unchecked Workspace.

## Birth, move and carrier completion remain prefix-local

| Writer | Actual completion and reviewed invariant |
| --- | --- |
| Open | `operation.rs:1717` completes the insertion root after the existing checked insertion. Edge insertion already assigns the new subtree and direct child placements. |
| Tabs/Subtree/Item Move | `operation.rs:1799` completes source and distinct target after detach/insert, deduplicating the same root. Detach unroots the old subtree and refreshes shifted parent indices; edge attachment assigns the moved subtree and new/direct parent rows. Center merge leaves the emptied detached Tabs unrooted until the existing final sweep. |
| RootContent::Move | `operation.rs:2996` completes the actual donor before the new-root writer returns. The eventual birth assigns the detached subtree to its new root. Whole-donor moves remain rejected by the existing RootContent contract. |
| Attach/Create/Install | The four writers complete birth only after installing the actual Main/Contained carrier (`operation.rs:2020`, 2089, 2147, 2185). `graph/authority.rs:268` rejects duplicate live owner entries, assigns actual membership and issues a fresh Arc. A removed RootId reused later receives fresh authority. |
| Rehome/Promote | `operation.rs:2252` and 2407 complete the carrier actually selected and installed by the successful writer. `complete_root_rehome` compares only that concrete carrier; same-main no-op retains issuance and membership needs no traversal. |
| Remove/retire | `operation.rs:3616` remains the common owner retirement after the real root/presentation removal and subtree unrooting. Root/surface close and empty-surface retirement reuse it. |
| Rect/roster/raise | These keep the exact root graph/carrier issuer; existing separate metadata preconditions remain. This agrees with the old fingerprint's carrier/content boundary. |

Completion occurs synchronously inside the command writer, before the next actual-stage capture/check. Final canonicalization is still once per general batch. The proposal neither accepts references from a predicted branch nor delays issuer/membership repair to the batch tail. An error discards the isolated candidate at the same existing publication barriers.

Existing `assert_actual_placement` derives its expectation from actual roots and child rosters, rather than trusting the directory under test. Its cross-root subtree, transient merged residue, shifted parent indices and newborn/donor cases remain relevant behavior oracles; strict storage validation alone would not establish directory correctness.

## No-op, selection and ABA

All Move early returns still follow source-first, independent-target and policy validation. Same-Tabs Item no-op/reorder, self-center Tabs and exact inner-edge roster/weight equality return before detach. They preserve original authority. The new same-root frozen-follow-up test covers the literal inner-edge path, rather than claiming it exercises a detach/normalize/equal path.

For a single general Tabs/Subtree Move which performs work but normalizes back to equal durable Workspace values, `TransactionDraft::finish` still makes the final exact comparison and restores original root issuance (`operation.rs:799`). This is preserved in the composed prepared-report version: one comparison supplies both the sealed report and the lone Moved outcome. That restoration is not generalized to a multi-command program. A→B→A carrier or selection transitions keep their fresh per-prefix identity even if the final report is false.

The ordinary single Item path keeps the earlier prepared-report proof: real different-pane membership changes or a newly allocated surviving nonempty Item Tabs make the actual outcome exact; literal no-ops return before detach. This cold patch does not replace that reasoning with an unconditional `changed = true`, reintroduce the two full Workspace comparisons or widen the general restoration rule.

Selection still mints the existing concrete selection lineage only for an actual selected-item change. Canonical no-op retains that lineage. `select_tabs_item` is a small private shared implementation, not a second executor: checked Select validates the source first; approved close computes a surviving actual visual successor before invoking it. The helper preserves the same selected/MRU mutation and does not issue an independent close authority.

## Approved close and failure semantics

Item close retains its frozen complete item multiset, policy requirement and exact source validation. It removes the actual item, picks the same next visual/previous visual successor, updates MRU, cleans an emptied noncentral root and completes that actual root (`operation.rs:396`). It no longer captures a new source over the intermediate partially completed close state. Root deletion already retires its authority; the later completion on a missing root remains harmless.

Root close and whole-surface close preserve frozen item/roster/order checks, current policy checks, exact per-root sources, unique ownership checks, complete canonical/strict validation and final item reconciliation. They continue removing every approved root through the same concrete retirement path. Unrelated roots are not renewed merely because canonicalization ran. Failures still cannot publish a partial candidate, and a retired/recreated root cannot reacquire an old Arc.

The two exact weight-sum frontier witnesses are unchanged. `validate_split_weights`, input diagnostic reconstruction, output diagnostic reconstruction, traversal ordering and normalization formulas are unchanged by the cold slice. New completion methods add no scalar validation ahead of these branches. The only removed prevalidation work is effect fingerprint materialization over a closed writer's current legal graph; source/target semantic and policy checks remain. There is no new numeric error priority inversion found in source.

## Costs and test claims

Removing the old NodeFingerprint-based effect classifier is real: `ColdRootTransition`, `actual_root_value`, both begin methods and `finish_cold_roots` disappear, and no replacement cold path calls `root_fingerprint`. New root birth walks its actual members once; canonical completion may walk an entire actually rewritten root. The general full normalizer, item ledgers, clone metadata, exact final equality and initial Builder carrier discovery remain real work. The two small effect Vecs also have real allocation/retirement costs.

The zero expectations in the background and native fixtures are scoped correctly: scene sealing, WorkspaceIndex/geometry capture and the source-size fingerprint occur before their work reset. Those retained geometry/index values are still built and owned. The recovery test retains the original 16/128/1024 workload and reset boundary. Zero here means no additional effect snapshots during the measured operation; it does not mean zero fingerprints over a complete frame/lifecycle. The live `workspace.rs:823` and independent drop/scene value matching remain unchanged. No fingerprint work was moved out of a timer by this proposal.

No measured speedup, asymptotic total bound or passing-suite claim follows from this review. Existing ROI limits in the README are accurate: general canonical traversal and real roster editing remain, including a possible quadratic roster-edit term in N-root recovery.

Root should qualify the assembled source with the existing owner/stage/placement matrix, approved close and native rollback cases, single/general Item equivalence, both weight-sum witnesses and the same complete recovery scale. The proposed two new tests add prefix-local rehome ABA and literal no-op/frozen-follow-up checks. A focused assertion of old-reference reuse on a genuinely rewritten-but-equal single general Move would strengthen that specific restoration branch; its current correctness is source-traced, not established by the newly added literal-no-op test. This is a nonblocking coverage distinction, not a request for another executor or a broad new fixture framework.

## Composition review

`cold-after-report-copy.patch` is incremental to the exact prepared-report and copy-after-prepared-report proposals, not to arbitrary current HEAD. Their original source receipts and patch hashes were checked before composition. Comparing both proposals to their common A1 source found no overlapping edited line intervals. Adjacent changes share diff context, so a blind contextual application is still less clear than the composed copies.

The important adjacent hunk in `canonical.rs:644` preserves both copy's whole-record `NodeStore::replace` (avoiding an unnecessary payload detachment) and cold's `(Some(node_id), true)` result. `operation.rs` keeps the prepared candidate/report seal and one final general comparison, the copy slice's borrowed removed Arc, and all concrete cold completions. Resolver/test overlaps retain the read-only prepared accessor, physical storage counter fields, generation fixture extraction and the new scoped zero-effect expectations. No new runtime change beyond those original slices was introduced.

The composed runtime delta is still +162/−165 lines across graph authority, canonical and operation. The original cold proposal, prepared-report proposal and copy proposal are unchanged. Complete exact source and patch hashes are in `receipt.json`; the seven input/output copies are retained for root review.
