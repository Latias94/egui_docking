# Independent review of the prepared candidate/report slice

Date: 2026-11-07. Verdict: no blocking correctness defect found in this patch. This review covers only the prepared report ownership slice. The reviewer authored the separate leaf-reader candidate; this document does not claim an independent review of that work.

No repository edit, patch application, Cargo/build/test execution or timing was performed. Statements below are source findings, not executed qualification.

## Exact subject

Reviewed baseline: `ecf78bacbdffdfb51f4a57c3f95073a71898a826`.

- `candidate-report.patch` SHA-256: `1b0a17402068d3b2d782bbe5e39538cb3366e30f6b0a45e95236293e815425b9`.
- `README.md` SHA-256: `1b3d8dca508598000f1e8f4327ba99beb943720a82fb0510223435dd7151b660`.
- Twelve-file base aggregate SHA-256: `f7a8ddad7f228c8585f13191da0d9dc86457fcc8775f42cfb7d3ddde35bc4a48`.
- Twelve-file proposed aggregate SHA-256: `9c6e2c890e254be46f6d38a7950b841700eee54713acf5b9755cc5eb8a3c89c4`.

Aggregates use sorted relative path bytes, NUL, then full file bytes. Every saved base file was compared byte for byte to `git show ecf78ba:<path>`; all twelve match. Every proposed file matches its supplied proposed hash, and its complete unified diff is present in the patch. The four unchanged contract hashes for graph, authority, command and canonicalization match both ecf78ba and the current workspace. The README's earlier “91f42ce plus stationary A1 working tree” provenance is compatible with these verified ecf78ba file identities.

## The common Item Move changed result is exact for this writer

The relevant change is `proposed/.../operation.rs:654–702`: successful local finishing no longer recomputes Workspace equality or restores initial issuance; it freezes `applied.outcome.changes_workspace()`.

I traced the unchanged actual writer, rather than assuming a true command flag is always exact:

- Same-tabs Center returns false before detach. Same-tabs TabGap delegates to `reorder_item`, whose old/new position comparison is exact; a real reorder changes ordered items, while selection/MRU remain untouched (`operation.rs:3435–3504` in the verified base).
- Same-node edge returns false only for a noncentral singleton tabs source. It returns before detach, allocation and root issuance. The canonical input is already normalized, so the later local finisher has no structural work to undo in this case.
- Different-tabs Center/TabGap transfers the unique existing item into a different live tabs record. That destination did not already contain the item, remains nonempty, and cannot normalize back to its original membership (`operation.rs:2992`, `:3155`).
- An actual Item edge transfer allocates a new nonempty tabs node in `insert_payload_at_edge` (`operation.rs:3262`). Item detach removes only the item/MRU and possibly root/presentation membership; it does not retire the old source arena node before the allocation (`:2992`, `:3507`, `:3571`). The newly allocated complete generational key is therefore absent from the original live-node set.
- The local finisher can remove the original empty source and redundant split containers, but it neither removes nor merges away the new nonempty payload leaf (`canonical.rs:268–329`, `:354–385`). Even a root collapse preserves that nonempty leaf identity. The final Workspace therefore contains a new live key and differs under the actual generation-aware WorkspaceData equality (`graph.rs:373`).

The same-root node-payload equality shortcut and exact inner-edge node-payload no-op do not invalidate this argument: this preparation branch requires `MovePayload::Item`, and the non-Item shortcuts are separate writer branches. A protected central singleton self-edge is also handled correctly: it is not the false early-return case; its old central leaf remains empty and the new nonempty payload survives.

Consequently I found no current Item detach/reinsert path that reports true but restores every original NodeId/tab/MRU/weight value. Restoring geometry and exact split ratios is insufficient. The added `an_item_edge_move_restoring_the_same_shape_still_changes_node_identity` regression targets that distinction: original root and split survive with the same two-child shape/weights, while the old tabs key retires and the new one remains.

The argument is deliberately writer-specific. Reusing the old Item tabs node during a future edge insertion, or adding canonical tab merging, would invalidate it and require revisiting this changed result. The patch does not generalize the optimization to node/subtree moves or transient staged programs.

The common Select branch is also exact under the sealed selected-first MRU invariant: when selection is unchanged, `promote_tab_mru` finds the item already first and performs no logical edit; otherwise selection itself differs and the actual writer renews issuance.

## Private pair and publication barriers

`PreparedTransaction` moves into the operation owner with private candidate/report fields and a private constructor (`proposed/.../operation.rs:33–43`). Before consumption, external modules receive only `candidate() -> &Workspace`; there is no mutable candidate/report accessor, Clone or independent pair constructor. A crate-private plain TransactionReport constructor does not let another module forge a PreparedTransaction.

`into_parts` and `into_snapshot_parts` consume exactly those frozen parts (`:53–59`). Publish still assigns the same owned candidate and drops the replaced Workspace at the existing publication point (`:62`). No mutable interior path was added to Workspace. The existing staged TransactionCapture remains a narrow read/capture facet, with no conversion to the not-yet-finalized Workspace.

The old `into_parts(original)` used its original argument only to compute a changed flag. It did not check a source version, current base or publication predicate. Removing that argument therefore does not remove such a guard. The report now states change relative to the actual preparation input, and all reviewed call sites previously passed that same input.

The patch changes only the conversion argument/accessor at the caller sites. Source inspection confirms the following remain after or around conversion:

- ordinary engine pending-presentation and workspace/publication barriers, followed by staged publication (`engine.rs:5687`);
- journal delivery activation against the owned post-move candidate, its final equality comparison, pending-presentation/recovery barriers and one atomic publication (`engine.rs:3457–3546`);
- contained-drop preflight and journal staging, and real pointer press/activation publication barriers (`engine/pointer_transaction.rs:3277`, `:3871`);
- timed hover-select and contained semantic Raise/Select barriers (`engine/pointer_timed.rs:731`, `engine/semantic_candidate.rs:313`);
- recovery source-retirement/action-barrier checks, later candidate selection and its existing final comparison (`engine/surface_runtime.rs:2510`, `:2733–2775`);
- native root-create preflight checks before/after preparation and its publication stage (`engine/native_tear_off.rs:515`);
- resolver future projection before consuming the same completed candidate (`drop_resolver.rs:938`).

Once a caller consumes the pair and performs an owned follow-up, the original frozen report does not describe arbitrary later edits. The existing journal and recovery comparisons covering those edits are unchanged. No caller was changed to trust the preparation report across such edits without its existing final check.

## General net-zero and issuer semantics

`TransactionDraft::finish` still prunes, canonicalizes and checks terminal/item reconciliation failures. It now performs exactly one explicit final Workspace comparison (`proposed/.../operation.rs:810–834`) and reuses that result for the frozen aggregate report and a lone Moved outcome.

Workspace equality compares durable values and complete live NodeIds, not the separate authority issuance. Therefore a two-command staged Select away/back can have false aggregate changed, two true command outcomes and a strict new owner reference. The finish code does not OR per-command flags and does not restore initial issuance for a multi-command net-zero program. The existing actual-stage test still asserts exact MRU restoration and rejection of the old source reference.

The original special case for a single general Moved outcome that normalizes equal still restores original root issuance. Its scope and condition are preserved. Removing that restoration from the local Item path is safe for the false early-return cases above, which never issued a new ref, and for actual edge transfers, which cannot normalize equal. The patch introduces no new ABA relaxation.

For non-Move/multi-command general routes, the equality moves from conversion time into successful preparation. It runs before the unchanged item reconciliation, so a later failure still returns the same error and discards the candidate. This may perform equality earlier for candidates later rejected by a preview, but is not a correctness or performance claim. No additional fallible validation is reordered.

## Caller migration and coverage

A read-only overlay of the twelve proposed files over the current source was searched across the workspace. No stale `transaction::PreparedTransaction` import, old PreparedTransaction conversion argument or direct old candidate-field access remains. Similar names in host-frame code refer to a different prepared engine product and are unaffected. No constructor was introduced outside the operation owner. All original conversion calls in engine, resolver, transaction tests and local command tests are represented by the patch.

The existing local Item matrix still compares complete candidate and report against `prepare_general_candidate`, whose changed result comes from equality. This remains an independent check of the new local outcome classification, despite both routes using the new pair constructor. Selection property cases, true/no-op reports, numeric failure witnesses, discard/rollback, retired-root placement and actual-stage net-zero checks are not weakened. The new identity regression is additive.

One nonblocking coverage opportunity is worth retaining: the local/general matrix compares durable Workspace equality, which intentionally ignores issuance. Since this patch removes local `retain_original_issuance`, a small assertion that old source/target refs remain accepted after each genuine local Item no-op would directly lock that contract. The current source takes all those branches before issuance, so this is not a found defect or a reason to block compilation. The existing Select no-op issuer assertion and the general net-zero strict ABA test cover related, but different, cases.

Root should execute the unchanged local/general matrix and the new identity regression, the existing actual-stage net-zero/rollback cases, and normal engine/native/recovery barriers after applying. No passing result is inferred from this source review. No broad framework, extra proof metadata or source rewrite is needed to address the reviewed slice.
