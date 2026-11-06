# Architecture and root-cause review

Reviewed `docs/performance/2026-11-06-docking-kernel-results.md`, the current transaction/index/layout implementation, existing contract tests, the archived work counters, and the existing CPU sample. No source or result document was edited. No Cargo, compiler, test, benchmark, profiler, Git write, network access or child agent was used. Python was used only to check existing CSV values; no new execution sample was produced.

**Verdict: no blocking root-cause or architecture finding.** The document supports prioritizing checked publication in this matched workload. It distinguishes measured phases from mechanism counts and proposed savings, correctly acknowledges existing dense/scratch implementation, and does not treat CPU process samples as phase attribution. The proposed owner boundaries are compatible with stale-proof, rollback and batch contracts under the conditions below. They are design directions, not validated implementations or measured speedups.

## Independent evidence checks

The archived `work-counts.csv` has exactly 21 unique expected matrix rows. All seven emitted counters in every row match the predictions made before measurement:

* Commands: `fingerprint_builds = V+6`, `fingerprint_node_visits = (2T-V)+6*(2T/V-1)`, item scans 4, item visits `8T`, transaction candidate clones 1, cloned nodes `2T-V`, cloned tab items `2T`.
* External-bounds resize: all seven counters are zero.

The source still has the predicted layering: `transaction.rs:56` -> `operation.rs:644`; authorization at `:660` and application at `:664` use the same candidate before mutation. Source validation occurs in both rounds; target validation both verifies the root fingerprint and re-captures target facts in each round (`operation.rs:2636` / `:2669`). `WorkspaceIndex` is used by caller capture, not consumed by `apply`.

I also recomputed summed same-execution phase shares from the existing formal raw CSV. At 1,024 panes/four views, mutation shares are 72.5521% center, 73.0967% edge and 72.7195% cross-view; layout shares are 19.1307–19.5601%, request 6.4001–6.5169%, export 1.2906–1.3709%. One-view center/edge mutation shares are 78.1620% / 79.2116%. Resize layout shares are 93.6420% four-view / 89.6344% one-view. These agree with the document's rounded statements. This is arithmetic on existing evidence, not a rerun or independent timing experiment.

## Dense layout and scratch: correctly acknowledged, with one optional precision edit

The current source already implements all of the following:

* Dense structural storage: `layout/topology.rs:9` stores nodes, child row indices, weight bits and postorder in Vecs. `compile` at `:42` builds them, and `measure` at `:204` reuses those rows for current measurements.
* Reused child scratch inside compilation/measurement: `child_values.clear()` at `layout/topology.rs:118` / `:224`.
* Reused projection scratch within one root projection: `ProjectionScratch` at `layout.rs:926`, allocated once in `project_subtree` at `:940`, then cleared/extended for each split at `:988` / `:1015`. This is not a persistent per-frame result arena.
* An existing indexed topology reuse path: `measure_root_minimums_with_index` at `layout.rs:770` checks a current index's topology and remeasures it. `WorkspaceIndex::build_reusing_layout` at `workspace.rs:137` preserves independently matching rows across revisions. `scene_compiler/surface.rs:1951` already uses that path.

The probe's `project_root` path (`layout.rs:441` -> `:476` -> `measure_root` at `:751`) compiles fresh topology each time. Its common adapter creates current keyed leaf-constraint inputs, and the projection still returns keyed rectangle/constraint/split maps (`layout.rs:579`) plus per-split owned vectors (`:1002`, `:1007`, `:1051`). Thus results-document lines 192–199 are materially correct. No recommendation should be rewritten as “implement dense layout” or “add scratch from scratch.”

**Optional P3 wording:** at results lines 194 and 249–253, call this “the full-projection entry used by this probe,” rather than “public full projection path,” and state that the existing indexed topology route is the starting point. `layout` is an implementation module outside tests; the measured path is not a new public product API. Suggested addition:

> Indexed scene measurement already retains matching topology. Extend that existing owner and the current within-projection scratch reuse through the full-projection/result seam; this experiment does not measure that indexed scene path.

This is a precision improvement, not a reason to reject the reported layout timings or the current priority.

## One checked owner: contract-compatible, not permission to trust caller versions

Results priority 1 is sound. `authorize_workspace_command` and `apply_command` are adjacent on the same immutable-before-application candidate. A private checked result can combine authorization, full-root stale validation, target-policy facts and executable operands without deriving identical roots repeatedly in that interval. The document correctly says to consume that result for mutation and invalidate reuse afterwards.

The retained semantic checks still include node/root membership, complete captured root values, selected/MRU, owner/presentation, target scope and policy. Reducing a complete-root proof to a leaf proof would change the present stale-reference contract. Reusing the benchmark caller's index or an epoch/revision alone would also be unsound after an internal candidate edit. There is no contradiction between removing repeated derivation and retaining those checks once at the candidate owner.

A concrete batch counterexample is `[Select(Item2), Move(old_root_proof)]`: selection changes the complete root fingerprint without changing topology or an externally published version. The old Move must still fail against the updated candidate, and the Select must roll back with the transaction. Conversely, a genuinely unchanged first command must not invalidate a later proof merely because it is later in the batch. `operation/tests/local_selection.rs:188` exercises mixed/multiple commands against general-path behavior; `workspace.rs:777` compares live values rather than caller labels.

## Incremental validation and touched roots: valid directions with an explicit trusted boundary

Priorities 2–3 do not intrinsically violate atomicity or invariants. Full candidate cloning, four item-multiset materializations and whole-forest normalization are the current mechanism; they are not a lower bound imposed by rollback. The document already requires a valid owned baseline, complete checks at the draft/import boundary, and joint commit of cross-root identities, MRU and presentations.

The necessary boundary must remain explicit in a future implementation:

| Condition | Existing behavior that must survive |
| --- | --- |
| Validity is owner-held, not inferred from a `Workspace` variable name. | `operation.rs:39` validates a snapshot; `draft_mut` at `:53` revokes selection eligibility. Unknown/mutated drafts retain general cleanup/rejection. `operation/tests/local_selection.rs:138` covers an orphan and missing MRU in a different pane. A touched-root validator cannot silently treat those drafts as trusted. |
| Batch checks use the actual staged candidate before each command. | `operation.rs:657` executes sequential authorization/application, then canonicalizes once at batch end (`:672`). Normalizing after every command or checking every operand only against the initial graph can alter accepted batches and stale failures. |
| Global item uniqueness remains accounted for. | A new item already owned by an untouched root must still be rejected (`operation.rs:1367` and the typed item ledger). Merely validating touched subtrees cannot prove this without retained global ownership facts. |
| Every fallible step precedes joint publication. | `transaction.rs:227` has a late invalid contained rectangle after an earlier successful movement; it must roll back topology and item effects together and remain retryable. `transaction.rs:175` also permits discarding a prepared candidate and retrying it. |
| Arena and presentation identity are part of staging. | Touched-root topology is not the complete write set: shared SlotMap allocation/removal, node generations, root/surface rosters, MRU, contained backlinks, and cleanup must be staged/committed together. Cross-root moves cannot publish one side early. |

These are implementation acceptance conditions, not a request for another framework or a broader current refactor. The report's stated caution is adequate. An optional extra sentence under priority 2 would help avoid a later “delete `canonicalize_draft` and rely on tests” interpretation:

> The optimized path must require an owner-validated baseline; raw drafts retain the general path, and later batch commands are checked against the staged state produced by earlier commands.

No existing contract test was executed for this review; its source was inspected to ground these conditions.

## CPU capture and causal limits

Results lines 176–182 describe the CPU capture correctly. Its receipt explicitly says it includes setup, pre/post validators and disposal. The existing stack contains an actual ancestry `prepare_transaction_inner` -> `canonicalize_validated_draft` -> `validate_workspace` -> `Validator::walk` -> `Node::clone` -> allocation, so this is more than a flat symbol appearing somewhere in the process. Other samples show repeated `build_root_index` below transaction paths.

That ancestry corroborates execution of the claimed mechanisms inside transaction processing. It does not make process-wide percentages a mutation-phase share, isolate a removable millisecond budget, count allocations, or establish that one clone site dominates. The result document makes none of those unsupported claims.

The view-count contrast also is not a controlled deletion of fingerprints: at fixed panes it changes root sizes/count, owner search positions, projection shape and some allocations. The wording “consistent with” and explicit no-ablation qualification at results lines 162–166 are appropriate. The observed phase ranking is sufficient to guide investigation, while the exact speedup of priorities 1–4 remains unknown.

The stated linear lower bound applies to the declared complete fresh flat export, which writes every node/item. It does not establish a time floor, require whole-root proof repetition, imply richer projection maps are unavoidable, or claim the residual Rust/ImGui ratio is irreducible. The current final paragraph preserves that distinction.

## Disposition

Publishable as written from this review's architecture/root-cause scope. No blocker or required source change. The two optional clarifications above improve the future refactor brief: acknowledge the existing indexed layout route and explicitly bind incremental validation to a trusted baseline plus staged per-command checks. Keep timing, counters, source facts and CPU ancestry separate as the current document does.
