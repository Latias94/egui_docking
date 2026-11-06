# Independent leaf-reader and export-reader source review

Verdict: **no blocking correctness or workload-integrity finding in either frozen patch**. Leaf-reader is already applied; all 11 live files match its saved candidate bytes. Export-reader remains unapplied; all five live files match its saved base bytes. I independently read both diffs, the saved complete implementations and relevant current call paths, and recomputed hashes. I did not edit repository files or run Cargo, tests, benchmark binaries or timing.

Full per-path hash verification is in `leaf-export-review-hashes.json`. The review applies to these exact patches; a later transplant must preserve the same behavior and update its source receipt.

## Leaf-reader findings

**No blocker. Fresh N inputs remain real and inside the layout timer.** `docking_cost::project_all_roots` creates a new Vec for each root, recursively visits the actual Workspace tree and appends one explicit `(full NodeId, Constraints)` row for every current Tabs node. It does not replace the N answers with a constant-return callback, fixture count or prewarmed topology. The owning Peekable consumes the next row only when its full generational identity matches the actual requested leaf. The graph is immutably borrowed throughout production/consumption, so the producer and measured DFS cannot see different trees. Extra unused answers are not synthesized; each real leaf contributes exactly one row.

The unchanged `measure` brackets place all Vec growth, actual leaf discovery, measurement, projection and input-iterator drop after `mutated` and before `projected`. Request capture, mutation, fresh rich output allocation and complete export retain their original timer positions. The separate layout_cost probe keeps its existing keyed inputs and timing; it is not the primary fresh-input comparison and is not used to move materialization outside the operation.

**The callback preserves the two distinct error-priority contracts.** The complete-constraint entry still checks the requested root and validates every override before the first callback. It then invokes `RootLayoutTopology::compile` directly and the unchanged project_measurements with minimums_only=false, equivalent to the previous measure_root(...Constraints).project_with_weights route. On Enter, Tabs invokes read_leaf and maps None to MissingLeafConstraints for that exact node. Children are pushed in reverse so reads are left-to-right DFS. A subtree aggregate still runs on Exit before a later sibling leaf is queried. No global callback collection/preflight changes that order. Minimum-only entry points retain keyed measurement first and override validation during projection, including warm-index behavior.

The raw diagnostic compiler is untouched, including completed-DAG reuse and cycle/central/missing-node errors. It still reads LeafMeasurements by key rather than consuming a sequential callback multiple times for diagnostic DAG edges. The sealed runtime compiler's only measurement change is how it obtains each explicit leaf value; numerical formulas, central propagation, full projection slabs and sorted identity directory remain.

**Coverage is registered and discriminating.** The sparse/recycled fixture uses nonuniform values and a hand-written leaf order that differs from NodeId order, verifies full input consumption and compares all rich output fields with the frozen oracle. The new four-leaf test checks omissions at multiple DFS positions, zero callback calls for an invalid override, and an earlier aggregate overflow winning over a later missing leaf. Existing minima/override priority, raw diagnostics, owned output and 10k coverage remain. Two mechanically migrated legacy integration files are unregistered under autotests=false; no coverage conclusion relies on them.

Root's supplied leaf verification receipts record successful targeted/workspace/public-integration/Clippy/docs/doctest/dependency checks. That is separate execution evidence supplied by root, not a run performed by this reviewer. The current source hashes match the reviewed leaf candidate.

## Export-reader findings

**No blocker. The output stays a fresh complete semantic export.** `NodeMap::iter_preorder` zips the immutable first-Enter topology rows with their existing owned values. Both sealed and raw checked compilation assign those rows in left-to-right DFS first-visit order; valid runtime Workspaces are trees. This method adds no cache, preprocessing, storage, allocation or Workspace borrow. Sorted iter/keys/values, get/Index, equality and debug behavior are unchanged. Slice Zip/Map supplies the documented exact-size, double-ended and fused behavior; all reachable rows and full generational identities remain present.

The exporter still walks the current Workspace root and current child roster recursively, computes root ordinal and binary path, reads current axis, ordered items and selected item, and writes every original word to a new owned Vec. Each projected row is matched against that current full NodeId, and root exhaustion is checked. Runtime NodeIds remain the full geometry join key; normalized output keeps its original ordinal/path schema rather than adding or removing a serialized NodeId field.

The unchanged nine-word header is ordinal, path, kind, x, y, width, height, item_count, selected, followed by every item word. Integral/finite checks, binary-fixture assertions, checked path arithmetic, capacity policy and returned Vec ownership remain. The removed operations are keyed rect search and one duplicate immutable Workspace node lookup; current semantic reads are retained once. The added ID/exhaustion checks are inside the existing export interval, not hidden outside timing. The measure timestamps and outside normalized-state/member validation are unchanged. The test-local old keyed exporter is not called from the measured path.

**Production resize ordering is valid and does not reject a legitimate complete projection.** The frontier walker already traverses the entire actual root. Its root plus reverse-pushed child stack produces exactly the preorder of RootLayoutTopology::compile; warm projection uses the same immutable topology after structural matching. Complete projection creates a rect for every row, including clipped/zero-size nodes, so there is no legitimate sparse-partial projection that the new exhaustion check excludes. N-ary trees, central leaves and recycled identities do not change this correspondence. Selection or weight changes alone do not reorder NodeIds; existing source, measurement, bounds and weight checks still govern admission.

A changed descendant order means old projected positions do not belong to the current ordered tree. Rejecting that mismatch is correct geometry admission. The new equal-extent swap test captures a fresh outer source from the rebuilt Workspace, so it is not trivially rejected as an old source Arc; it exercises the retained-projection/current-order correspondence. Existing per-child validation checks extents, not complete positional order, so equal-size swaps can escape those older checks. The new match closes that gap without weakening any visited, incident, constraint, split/gap or interval check. The original Option-returning failure mode and numerical calculations remain.

**Owned old outputs and full words are retained.** NodeMap owns its values and shares an immutable topology Arc, and the new iterator borrows only that map. The existing old-output test now rereads saved preorder after another output and after dropping index/Workspace. The recycled/n-ary oracle fixture checks each ID/rect, exact size hints, decrementing length, mixed front/back consumption and repeated exhaustion; existing sorted/generation assertions remain. A new registered exporter test compares every word for all primary operations at 1/4 views with both the preserved keyed exporter and the independent normalized expected state. It uses 16-pane fixtures; the usual full matrix/vector gate remains necessary at 128/1024.

Export-reader has only source preparation evidence so far. No speedup, 52–75us export-budget attribution, or full correctness qualification follows from deleting lookups. Root must compile/run its new registered contracts, the real resize/host suite and unchanged full exported-vector matrix before measurement. Existing scene compilation's emission order is untouched.

## Exact identities

Aggregation independently recomputed as sorted relative path bytes, NUL, complete file bytes; every saved per-file base and candidate hash matched its manifest.

| Patch | Base aggregate SHA-256 | Candidate aggregate SHA-256 |
| --- | --- | --- |
| leaf-reader, 11 paths | `4416429dc57aaa8a7186cb41afa021a22ea7539912997a1050f621c0be8609c8` | `5832e67919b285e801a8e97297695e66f55a9e5d27692f825b45a71fd14a4e31` |
| export-reader, 5 paths | `cb32d6c749b9b2570b83dfe07ff5d658dcf4b65a20bb9c2301a72eb9ffc813ba` | `c63708060deb32118e8582185d9fd86ac282cb8cc0f2a9e00f44b8b48a9dfa83` |

Leaf patch SHA-256: `9e5536d3d29d927e1f3ed0db088243e6f20e75c1daef58635185485671e31efc`.

Export patch SHA-256: `5f0351097009ad379ff8847de034196f333c9b2da1c766ef5635f3fb41cc94a3`.

The three shared leaf candidate paths exactly equal export's saved base: docking_cost.rs `c5186bac7b0a2cfe32f3c69b593efdb40a882fea48365d38c2089267abdc8c57`; splitter tests `e4e5e111ef77fd794e223e3bd28cf8b29730f9989970d564d016787f870e031d`; topology tests `0a045129ebe37bc5cfd365651ab6ad6865f199a34d8b68e5694de4f60ad654a2`. Export therefore explicitly builds on leaf, not on the older map caller.

Frozen `layout/reference.rs` remains `93afdf6e8ad79d81f2efdafc4814a4bb6076fd9f535129f82ce33f9ee12ddf4e`; neither patch includes that file. Leaf's core layout base is `354f015c74a2e942d006d7c1e510a26df3f43ebf49e42bf0763687ab38fdd44f`; export's production projection base is `c31bfdad6ec84a8714f1d93fa93f97fdb96cb96226349263018da09c62dd742e`, and its production frontier base is `82b640b9bc539a826b8b0bd758bcd2d55e2a6be9f14908bebc6fa0502e1c8e3b`. No solver or frozen-oracle modification is part of either diff.
