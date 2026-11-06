# Independent rebase review: Tabs-owned MRU on single NodeResult

## Findings

**No blocking source finding.** The frozen patch preserves the original reviewed MRU implementation, retains the current single-NodeResult changes, and adds the requested registered history-only staged-selection regression without changing prior tests. It is suitable for root-owned integration and serial qualification. Source review does **not** establish successful compilation, passing tests, measured performance, or the ImGui twofold objective.

No source correction, extra framework, or additional regression is requested by this review. Compilation, registered contract tests and complete paired measurements remain root's qualification gates, rather than findings asserted by this static review.

## Exact reviewed identity

Review completed at `2026-11-06T21:55:48.404559+00:00`. All following identities were independently recomputed from the files, rather than accepted from `scope-checks.json`:

- Rebased patch: `/tmp/tabs-owned-mru-current/tabs-owned-mru-current.patch`, SHA-256 `9046e9d14bb92bdb7aab2e8935b08aa3f4f5849f135e1b0edef595f50f216c01`, 55,215 bytes, exactly 24 existing Rust paths, +318/-371.
- Manifest: `manifest.json`, SHA-256 `a71c68dc7c76c97d7e768dfa2cb1f5af89bcb130f2a2a367ad6a9e035993cd4f`; base manifest SHA-256 `7ce4e03a00877b13f6b4d570bf832aff9a7a930f3e5188eea0be7ce82d15b221`.
- Base 24-path aggregate: `e44e1de5699e9c436f79d04fdda74e7916d24b286949ec776fbd538662898d28`.
- Candidate 24-path aggregate: `7dde1e11eb8b9468b51d15570ba4848ab768a1706b7c1840559c127a3ad2cebd`.
- Aggregate encoding: ascending UTF-8 path strings, each followed by NUL, its lowercase hexadecimal SHA-256, and newline. The actual files in both `base/` and `tree/` have exactly the manifest's 24 paths; every per-file hash, byte length and addition/deletion count matches. Each captured base file also matched its live repository source during review.
- Reconstructing all 24 unified diffs from those complete input/output pairs reproduces the supplied patch **byte for byte**. READY's patch, manifest and README identities and all six artifact hash/size receipts match.
- Original candidate patch: `/tmp/tabs-owned-mru/tabs-owned-mru.patch`, SHA-256 `1ea14af6f5cdcefea0bb170390dbad8270f3fd845c1cbe975a27d3830b13e9d8`. The original candidate manifest and all 23 original changed base/output hashes match their captured sources.
- The prior independent review `/tmp/tabs-owned-mru/review.md` was read in full; its current SHA-256 is `36cb4529e8581289b906000ca92cab4e234484d565bb486fc30a134ce280a7d0`. Its MRU semantic conclusions are carried forward on verified byte identity, with the three NodeResult differences and the appended test independently examined here.

## Rebase integrity and both conflicts

Twenty of the original 23 candidate output files are byte-identical to their original MRU outputs. Only the following three differ. For each file, the complete sequence of removed/inserted byte blocks from **old base -> current base** is exactly the sequence from **old MRU output -> rebased MRU output**. No additional MRU change is hidden inside these files.

| Candidate source | Preserved NodeResult delta blocks | Current-base SHA-256 | Rebased-output SHA-256 |
| --- | ---: | --- | --- |
| `crates/dockspace/src/docking_cost.rs` | 4 | `3043546e6fa33ffe73511a1035d0ed4366f8a076e340241448f79073db33a00d` | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |
| `crates/dockspace/src/scene_compiler/future_layout.rs` | 4 | `bb0faa4b98574a70e5fc38a28c391e8970a40aa24942bdc07b5980e68ec9eba9` | `495190e02a57fce8dc83596d61bd583ab4e687150f63b5bbbe31b394923b32c3` |
| `crates/dockspace/src/scene_compiler/surface.rs` | 9 | `6f3d945673d9032cfc9eb629bf98e57da3c525318a77ddd08c7657dd11aa5b2c` | `abf030c112a4d6290d573d5f77669d94247a6dbd92d572d9109209fa87795feb` |

An independent parse of the original patch finds 85 hunks. Exactly 83 old-context bodies occur once in the current captured base. The only two absent contexts are the documented original `docking_cost.rs` hunk `@@ -355,9 +357,9 @@` and `future_layout.rs` hunk `@@ -136,13 +136,14 @@`.

In rebased `docking_cost.rs:360`, the keyed oracle retains `projection.layout.nodes[&node].rect`; the neighboring change only adds the Tabs pattern's `..`. Production exporter iteration still takes each NodeResult's rectangle in preorder. In `future_layout.rs:133`, the projection lookup retains `.nodes.get(&target.tabs()).map(|result| result.rect)` and its exact MissingNodeProjection root/node error. Its separate candidate-node lookup retains MissingNode and TargetIsNotTabs errors. The only MRU migration there is the Tabs match pattern. `surface.rs:840` continues iterating `projection.nodes`, clipping `projected.rect`; its MRU hunk only adds `..` and does not revert any NodeResult migration.

These source and delta comparisons support the documented two-conflict resolution; the author-written rebase receipt alone was not used as proof.

## MRU ownership, validation, persistence and rollback

The original candidate's 20 byte-identical outputs include every semantic MRU owner/writer, strict validator, persistence path, and ordinary work ledger. Targeted inspection agrees with the prior review:

- `graph.rs:170` derives Node equality over items, selected and the owned `mru` Vec. WorkspaceData equality compares complete Node values. `NodeStore::get_mut` remains `Arc::make_mut`; immutable clones retain the complete historical Tabs value. Generational lookup still governs the public `tab_mru` accessor. There is no separate persistent TabMruStore, and changed outputs contain none of the removed store types/counters/diagnostics.
- Selection promotes history through the same mutable Tabs borrow. The original reviewed insert/remove/merge, visual reorder, rehome and replacement behavior is unchanged by this rebase. Complete Builder replacement still returns the old complete value; raw `set_tab_mru` still permits malformed draft input and leaves rejection to validation.
- Removing MissingTabMru, UnexpectedTabMru and MissingRuntimeTabMru removes states made impossible by required ownership. It does not permit empty history on populated tabs or weaken permutation checks. Length, duplicate/foreign/missing entries and selected/head mismatch remain rejected, in the original diagnostic order; selection and global duplicate-item checks retain their order. Canonical validation still invokes the exact-permutation predicate.
- `SnapshotNode::Tabs` still requires its serialized `mru` Vec, retains tagged/unknown-field schema rules, captures the actual complete history and restores it directly into the Node. No serde default or silent MRU repair was added. Preflight still includes history item identities.
- The actual transaction stage captures from its current candidate on every push. Its final comparison at `operation.rs:822` compares normalized candidate values, including history; issuance remains separate. The captured `transaction.rs` is unchanged. Live `graph/authority.rs` matches the original review's source exactly at SHA-256 `c4542993266ad497b50cdd30e00bbf6fc656819925d8d52942290b54d7d121e2`.
- Existing owner-reference coverage, strict ABA, staged failure and late-normalization rollback cases remain byte-for-byte before the appended test. The registered MRU behavior suite `tests/tab_mru.rs` is unchanged from the original review at SHA-256 `e325e593e62f6938d5947c2697b7728bfd6377c1868c01b515b0eba3b2d35835`. This is registration/source evidence, not execution evidence.

The source hash basis for these targeted semantic checks is:

| Rebased source | SHA-256 |
| --- | --- |
| `crates/dockspace/src/graph.rs` | `dde95ddfa9e733448a4565404d9b6a612a3f614735bc39ba2cc4233066b80c2a` |
| `crates/dockspace/src/graph/shared_storage.rs` | `18c6f75150643779acb9c71c30486634b19f7ccec5677e4cc64ce9043fcc532e` |
| `crates/dockspace/src/drop_resolver.rs` | `f8b093f247417b4abe2a79dc13febb0549bf214066c4bc9dc0739a72e2579a7f` |
| `crates/dockspace/src/validation.rs` | `bcbc2b79d3da167b101b40ae1683986d44f0b58bdcf3c7e865a64df081c02d79` |
| `crates/dockspace/src/persistence.rs` | `bb30a2a4d3383ed633c7a42e2c5d7ddc9246573c12b08a48a569bc5059437b34` |
| `crates/dockspace/src/operation.rs` | `b18c0f29b0561af1bbed80600d72d94825d5fb5ac1708abf1af7fe864d15c0b5` |
| `crates/dockspace/src/operation/tests/owner_reference.rs` | `8a39eb6df3d844f952af7b550f75f51c51ab6d5c853b47f8748b69adb55158d4` |

## Added history-only staged witness

`operation.rs:3911` already registers `operation::tests::owner_reference`. The new test is appended at `owner_reference.rs:925`; every previous byte of the module is preserved. The complete new output hash is recorded above. The supplied original witness is exactly the old candidate's supplied witness at SHA-256 `3c2650549d3729f330a5b43cee013b26ac5f9e4b91bdef2afd7a6a3c09643fd9`; formatting changes whitespace only. The formatted witness is SHA-256 `e706198e5a8f0e996c6f6129b41a73f71f70d6a94d8a64a99d1f7265387d4bd6`. The candidate module equals the captured original module plus one newline and that complete formatted witness.

The fixture begins with three items `[1,2,3]`, selected 1 and history `[1,2,3]`. Each selection in `[3,1]` is captured inside `stage.push` from the actual completed prefix. Selected returns to 1 while history becomes `[1,3,2]`. The test checks both Selected outcomes are changed, the aggregate report is changed, visual item order is unchanged, both exact new and original histories, Workspace inequality, rejection of the old source reference as StaleNode, and actual placement through the pre-existing helper. The original Workspace clone remains alive throughout. This exercises the history-only equality/publication boundary that the prior review identified as an optional useful regression; the rebase now includes it without weakening existing ABA or rollback tests.

## Formal measurement and work-accounting boundaries

Independent byte comparisons confirm the current base and rebase retain the fixture/request/fresh-projection prefix, complete owned exporter function, phase measure function, formal `docking_cost` function and CSV, and expected full-word oracle. No solver, layout topology or RootStructure source is part of this patch. All five contract captures match their base-manifest identities and live sources:

- `layout.rs`: `325f56f16cacbde46cce531461c8830417a6aa8ff4af06f6c87145a7528963df`.
- `layout/projection.rs`: `d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0`.
- `layout/topology.rs`: `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56`.
- `graph/root_structure.rs`: `de30fe16e4587efe9cc181f5d1ca46b66250fa082c5b4363efa5f1c9a1215a62`.
- `transaction.rs`: `0970cc5343f6eec5bc300603e78303782b3e817edb86064b3694fac4544106a2`.

Additional reproducible byte-region checks use the stated exact source boundaries, so their hashes need not use the receipt author's different boundary conventions:

| Exact region in `docking_cost.rs` | Bytes | SHA-256 |
| --- | ---: | --- |
| File start up to `fn append_node` | 8,363 | `1567481d1ca73d8618ecea1a4699c3f45804fd2e566633a40d6488d02e39b58e` |
| `fn export_state(` up to the newline preceding its following `#[test]` | 704 | `19a6031b1ccaeddd3195b31d6d55b460212d91df79cbed0875eb10ba8c5fc2da` |
| `fn measure(` up to `fn split_rect(` | 1,123 | `a056aff86457f1b38c7850085d0fa74b736b0ebc06195d05a3cc6f58d7dd1011` |
| Formal docking-cost test's `#[test]` up to ordinary work test's `#[test]` | 3,767 | `270a71b651925b0a28ba89b3a6a57f01571efed4d2d98c7da9d4070d045b3126` |
| `fn split_rect(` up to `struct Counts {` | 3,249 | `f23912b81b295028e311c6f000d0423c8037f0934879f0a32b3114e2edc7cb68` |
| `fn docking_work()` to EOF, compared with the original MRU output | 3,366 | `5c599d5c976b3b3e30f1f81f733a4779cdbba8dc0bc61b52e17938c2615b2a31` |

Fresh constraint collection still recursively visits every Tabs leaf and constructs each root projection within the measured operation. Request capture, mutation, layout and full owned-word export remain in their original order and timing boundaries; retained projections/words still outlive the dropped fixture in the formal test. Complete words are still compared with the expected-state oracle. The existing selected/item/geometry export protocol has not been narrowed by this rebase.

Ordinary work CSV changes from 23 to 20 columns, removing exactly `mru_store_forks`, `mru_handle_clones` and `mru_payload_clones`, preserving every remaining column's order. The 20 format fields still align with two named values and 18 explicit arguments. `mru_item_copies` and all three RootStructure counters remain. Actual Node::clone records both visual item and MRU element copies; the logical `tab_mru_items` inventory still accumulates actual owned history lengths. Reset/snapshot boundaries and disabled-ledger gating are unchanged. There is no claim that constructor allocations, arbitrary transient Vec copies, bytes, capacity or RSS are counted.

## Limits and handoff

The prior representation costs remain relevant: colocating history can enlarge the Node enum allocation, makes visual reorder copy unchanged history when COW detaches, can make history-only draft edits copy visual items, and can copy the old complete history for a shared owned replacement return. Forking the Workspace still clones arena handles and other directories. Removal of the separate MRU map is a concrete ownership change, not measured evidence of allocation reduction, lower latency, or twofold goal attainment.

This was one independent, bounded, read-only source review. I did not run Cargo, a compiler, tests, benchmarks, timers, Git writes, network access or subagents; did not modify the repository, old candidate, frozen candidate sources, caches or forks; and did not recreate the earlier whole-repository review. The only artifact written is this requested report. Root remains the sole owner of serial typechecking, registered contract/serde/ABA/rollback/copy-counter tests, product/native regressions and full-state paired measurements for 16/128/1024 panes x 1/4 views.

Review complete; candidate stationary; STOP.
