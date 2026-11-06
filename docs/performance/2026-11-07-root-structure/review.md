# Independent source review: graph-owned shared RootStructure

Status: **final independent source review of the author's READY candidate**. No confirmed correctness or scope blocker found. Source review supports root-owned integration and serial qualification of patch `8dc1059d026310e0ef26a9ff5bcb1bca92cd8d00cff62a0bcf833a0c1404ad0b`. This is not a compilation, behavior-test or performance qualification. One nonblocking receipt-format correction is recorded below.

Scope: `/tmp/root-structure-shared-arc/{base,tree}`, the exact final patch, `README.md`, `READY.json`, and manifests. Read-only inspection; no repository changes, candidate edits, Cargo, compilation, tests, benchmarks, timing, or Git writes. This report is the reviewer's only written artifact. The source conclusions below describe the final candidate tree, including the explicit prefix-completion changes and sorted actual-changed-root lookup. Root reports the cold + initial-issuance baseline is now committed as `80403cb`; the file-level preconditions in this candidate remain the review's exact source binding. Previous candidates' timings are not results for this patch.

## Conclusion

No confirmed correctness blocker found in the inspected implementation. The Module moves the structural body under the existing graph owner, rather than adding a revision-keyed cache or another authority protocol. The Interface separates exact command issuance from immutable structure. `RootStructureState::Editing` is private ownership state while the concrete graph writers mutate a discardable candidate; layout refuses that state instead of independently reconstructing or matching the graph.

There is a real performance boundary: a split edit rebuilds the affected root's complete dense structure and placements, including its identity directory. Nonempty center Item moves can retain both source and target bodies. General finalization still visits all owners once; the latest candidate uses binary search over the normalizer's already sorted actual changed roots. These are source facts, not measured attribution or a claim of regression in the recovery workload.

## Ownership, identity, and lifetime

- `graph/root_structure.rs::SharedSplit` owns one `Arc<Node>` cloned directly from `NodeStore::shared`. Axis, child identities, and `SplitWeight` values are borrowed from that same immutable arena allocation. There is no copied layout weight-bit array, second split-value object, or floating-point conversion retained in the structure.
- `RootStructure::build` constructs preorder node rows, direct child-row joins, postorder measurement order, identity-sorted lookup rows, split rows, and splitter ranges. The child-row joins are structural indexes; they do not duplicate the canonical child-identity or weight payloads.
- `NodeStore::get_mut` remains `Arc::make_mut`. An old projection's structure retains its split payloads, so a later successful writer cannot mutate the old output's axis/children/weights in place. Projection arrays remain separately owned and clone independently. No reference cycle is introduced: a split payload contains only node identities, not a reference back to its Workspace or RootStructure.
- `RootReference` remains the command authority. `complete_root_change` renews issuance without invalidating geometry for Tabs-only edits. Selection changes only issuance/provenance; rehome changes only presentation issuance. `retain_original_issuance` now restores references only. It deliberately does not restore an old structural body over a current normalized body that happens to compare equal by value.
- `RootStructure::build` trusts a checked graph-owner prefix or fully validated Builder result. It is not a second validator. Cycles/shared ownership cannot be published through production Builder/import or the concrete checked writers; malformed-draft layout diagnostics retain their explicit checked compiler under `cfg(test)`.

## Writer and publication audit

| Entry or edit | Structural ownership/completion | Review result |
| --- | --- | --- |
| Builder, decode/import | `build_workspace` performs complete canonicalization and strict validation before `Workspace::issue_storage`; each actual carrier calls `complete_root_birth` | New bodies are issued from validated actual graph data. Import still reaches `builder.build()` in persistence. |
| Single Select / Reorder / center Open | Tabs/MRU values change, while split payloads and leaf identities remain unchanged | Keeping the existing body is correct; exact command issuance still changes when required. |
| Nonempty center Item Move | Only Tabs/MRU payloads change; `complete_root_structure` is a no-op on Ready owners | Both bodies can share across the move; no topology match or runtime compile is needed. |
| Detach a node from a split | `remove_split_child` marks the actual parent root Editing before its split payload is mutated | Removed subtrees are unrooted and direct child placements refreshed before prefix capture. |
| Edge insertion / wrapping | `insert_payload_at_edge` marks the target root Editing before parent or root-node replacement | Both same-axis insertion and new wrappers are covered. |
| Single Item finalizer | `finish_item_move` invalidates before source-path rewrite/outer flattening; completes source and distinct target **after** local repair, failed-frontier diagnostics, retired placement removal, and direct roster refresh | It does not seal pre-repair rows into the completed candidate. Empty/retired roots have no surviving owner to complete. |
| General Move prefix / exact SurfaceRecovery program | `TransactionDraft::push_command` completes only the command's source and distinct target after execution/item delta, before the next capture | No per-command scan over unrelated owners. This completion does not perform canonicalization early. |
| RootContent::Move feeding a fresh root | `detach_root_content` completes its donor, then creation installs the actual carrier and calls `complete_root_birth` | Donor, fresh root, and subsequent prefix captures see the completed concrete state. |
| Real split resize | Only changed records enter `begin_root_edit`; roots with actual changes complete once after all validated weights have been installed | The body shares the new exact arena payload. Multiple resizes in one root do not publish an intermediate body. |
| Same-value split resize | The loop skips unchanged entries before `NodeStore::get_mut` | No unnecessary split payload COW, body replacement, or issuance renewal. |
| Fresh main/contained roots and empty surface | All four creation paths install their carrier then call `complete_root_birth` | Initial body and normative placement are built together. |
| Rehome / contained promotion / rect / raise | Bodies retain the same node identities and split values; dedicated presentation/metadata owners remain in use | Geometry structure sharing does not grant stale command authority. |
| Approved item close | Existing policy/source/multiset checks remain; final full canonicalization removes emptied noncentral leaves and completes their changed roots | A nonempty Tabs-only close correctly preserves the body. |
| Approved root/surface close / empty-surface retirement | `remove_root_and_presentation` removes the root owner; actual subtree removal follows; surviving roots pass terminal canonicalization where applicable | No retired root retains a current body or issuance in the candidate. Old outputs may continue to own historical payloads. |
| Owned engine follow-up | `apply_owned_draft_inner` executes on the discardable owner, then canonicalizes before assigning the candidate back | No unfinished body crosses its successful return barrier. Failure abandons the candidate. |
| Standalone canonicalize / general batch finalization | `take_root_structures` temporarily removes read bodies before raw writes; only roots absent from actual `changed_roots` recover their prior body; remaining Editing owners rebuild after successful full checks | A preexisting Editing root was never captured, so it cannot accidentally recover an obsolete body. Unchanged rewrites preserve the original split allocation because `rewrite_split` returns before replacement. |

The production `complete_root_structures` call is confined to `complete_runtime_rewrite`. It is not called from every command prefix. Prefix Move, Open, Resize, donor completion and root birth use a bounded root scope.

Important final-source locations (all relative to `tree/crates/dockspace/src/`):

| Fact | Source location |
| --- | --- |
| Exact arena split allocation and body/placement construction | `graph/root_structure.rs:16`, `graph/root_structure.rs:92`, `graph/root_structure.rs:136` |
| Private withdrawal, one-root completion, terminal all-owner completion | `graph/authority.rs:102`, `graph/authority.rs:109`, `graph/authority.rs:136`, `graph/authority.rs:154` |
| Item completion occurs after local repair and placement cleanup | `canonical.rs:328` |
| General Move prefix completes source and distinct target only | `operation.rs:789` |
| Unchanged Resize skips mutable access | `operation.rs:1985` |
| Actual changed-root binary search and terminal completion | `canonical.rs:403` |
| Fresh root structure and reference-only issuance restoration | `graph/authority.rs:355`, `graph/authority.rs:404` |

## Layout semantics and removed paths

`WorkspaceRootIndex` no longer owns a `OnceLock` body. `build_reusing_layout`, `layout_topology_slot`, `RootLayoutTopology::matches`, runtime topology compilation, and the topology's duplicated weight-bit storage are removed. Scene measurement borrows the actual Workspace-owned body directly. Full fingerprints still contain weight bits for existing semantic/presentation comparisons; the patch does **not** remove those protocol facts, and must not be described as doing so.

Numeric measurements remain fresh for each reader/minimum/metrics set. Measurement uses the retained postorder; split composition and child summation retain their prior order. Projection reads the shared exact f32 weights and converts them to f64 at the same solve boundary. The solver, numeric validation suffix, splitter math, and frozen reference oracle have not been replaced. Preorder construction versus the raw compiler's postorder allocation of child ranges changes storage layout only; each per-node child order, splitter order, and split outcome index still joins through that node's own row ranges.

The malformed-draft compiler retains the existing active/completed row-state rule, DFS stack order, cardinality gate before descending, completed-subtree reuse, and central reachability check. Partial compilation is returned only on success. Production layout returns `UnfinishedRoot` before calling the leaf reader if a private edit has no completed body.

## Costs and bounded concerns

1. `complete_root_structure(root)` rebuilds every reachable node in that actual root, reinserts its placements, and sorts the identity directory. Source-level bound is O(n + n log n) for an edited root, with a stack of pending frames and new row arrays. It is not an O(changed-frontier) update. This replaces a former layout-time build; whether moving and sharing this work improves any measured cell must be established by root's controlled measurements.
2. General Move prefixes can build a temporarily noncanonical root, then terminal canonicalization can rebuild it again if normalization changes that root. That is a source-visible repeated traversal, not a second executor. The capture facet still cannot be used for layout/scene compilation, and terminal canonicalization remains responsible for final normalization/errors.
3. `take_root_structures` and terminal `complete_root_structures` each visit all surviving owners once per full normalization. They do not reproduce the earlier R-per-command cold recovery issue. Initial Builder issuance calls one birth per actual root and does not use the terminal all-owner completion loop per root.
4. The latest `complete_runtime_rewrite` checks `effects.changed_roots.binary_search(&root)` for each captured previous body. This is valid because `canonicalize_validated_draft` visits the `BTreeMap` root roster in ascending order and pushes each actually changed root at most once. With R captured owners and C actually rewritten roots, the membership work is O(R log(C + 1)); there is no added O(R*C) linear-membership term in this version. The earlier WIP used `contains`, but that superseded implementation is not a finding against the latest candidate. The exact recovery-to-center pattern can have C = 0 and many retired owners. Do not describe this as a measured quadratic recovery regression.
5. The new root build is not counted by the existing root-fingerprint or full-canonicalization ledger fields. An assertion that those fields remain zero does not establish that a structural edge move performed zero root traversal or zero allocation. No counter/schema change is required merely for this review, but future attribution must name the work actually counted.

## Existing and candidate test evidence to qualify

This is source inspection, not a test result. The candidate extends the actual-graph placement oracle to check every reachable row's exact identity, kind, child-row joins and direct `Arc::ptr_eq` with the current arena split payload. The existing local/general Item Move matrix already calls that oracle. Existing staged, transient residue, fresh-root donor, rehome/ABA, generation reuse and import/Builder tests remain relevant.

Candidate assertions also cover Select body retention, nonempty center Item Move retaining both bodies, target edge body replacement, changed resize replacement, and same-value resize preserving both the body Arc and the actual split Arc. The explicit unfinished-body test checks rejection before leaf callbacks and an owned completed projection after all Workspace owners are dropped.

The existing frozen traversal/solver properties, identity-sorted and preorder iteration tests, changed same-version Builder cases, current-metrics/minima tests, override precedence, malformed DFS-error cases and registered 10k-depth test should be retained in root's qualification. The exact cold normalized-equal single Move must continue to restore only the original issuance; a resulting body may be new if the actual split allocation changed. The internal raw split-resize fixture now explicitly begins and completes its structural edit, preserving the intended stale-reference assertion. The new `UnfinishedRoot` error is classified as an internal runtime error by the exhaustive runtime error mapper. Existing rollback, approved close and recovery suites must run against the integrated source; this review does not substitute for them.

Independent static byte comparisons confirm that the override validator, constraint composition functions, splitter geometry and DFS continuation, the numeric-validation/solver suffix, the `solve_axis`/caller-owned entry bodies, frozen reference file and the registered 10,000-depth test remain unchanged. These checks compare saved source bytes; they did not execute Rust code.

## Nonblocking receipt correction

`candidate-hashes.json::path_set_hash_format` describes ascending path order, but the declared base/candidate set hashes reproduce using the manifest's existing array order. That array places descendants such as `graph/authority.rs` before `graph.rs`, and `layout/projection.rs` before `layout.rs`; it is not ascending lexical path order. All individual file hashes, final patch bytes, patch reconstruction, README hash and READY manifest hash are correct.

The recorded set hashes are `94fbc705a65eab3df64b024ef63d7760cd1a8d40299777c5d3310fd67d142b6f` (base) and `4b6096fc6b411e16479afacec2144f3348613b9c80bfdfc63d896f3b04b1b827` (candidate). Applying the documented lexical path sort instead yields `fb690680df9c0bf2a683c8551db2db37785b339afa3e8d4969c580278094b605` and `24016af1a01e1fa14349b246a6a3eca685330fd85b5fe94818c5a7bc1d76b797` respectively. A future receipt can either describe manifest-array order accurately or use the declared lexical order consistently. This is a reproducibility/documentation nit; it does not invalidate the per-file or patch source binding and does not require changing runtime code.

## Final frozen source binding

The author's explicit READY is dated `2026-11-06T20:52:02.374195+00:00` and declares candidate writes stopped. Parent separately confirmed this READY. I then independently checked:

- Final patch SHA-256: `8dc1059d026310e0ef26a9ff5bcb1bca92cd8d00cff62a0bcf833a0c1404ad0b`; 79,151 bytes; 16 changed paths, one new file.
- `candidate-hashes.json` SHA-256: `dd9f3fbf24b5ddaa48341fa3261125f08a22b78f8dea510dd88795ea8c0cbb11`, matching READY.
- `README.md` SHA-256: `883aef5e18ab57ba51525a8d2f415e416d0260b3dc55daf57347e68d6e008644`, matching READY and manifest.
- All 16 exact base/candidate file hashes and byte lengths. The final patch hunks reconstruct each candidate file exactly from its declared base in memory, with exact context and line counts. No patch was applied or temporary copy written by this reviewer.
- All 263 full-base receipt inputs. All 246 unchanged files present in the candidate source tree match that base. The source-only tree does not copy `crates/dockspace/Cargo.toml` or `rust-toolchain.toml`; both remain present and hash-checked in the full base, and neither is a patch change. There are no unlisted added or modified source files.
- The unchanged-file scope hashes, independent oracle, override validator, numeric blocks and registered deep test. Later slab/frame/fusion/NodeResult candidates are excluded and require explicit overlapping-hunk composition if integrated afterward.

Final verdict: source review passes for the exact patch above, with the receipt-format nit and the explicit root-rebuild/repeated-prefix cost boundaries documented. Root should perform the planned integration, complete behavior qualification and paired measurements against the actual `80403cb` baseline before making performance or acceptance claims.
