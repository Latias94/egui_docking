# Final runtime-tree and owner-reference integration review

Date: 2026-11-07. Base: qualified checkpoint `91f42ce`. Result: no blocking correctness or architecture defect found in the four-file layout patch or its current owner-reference call boundary. This is source review only; root owns the running serial compile and subsequent execution gates. No repository file, build, test, benchmark or timing was changed or run by this review.

## Reviewed source identity and scope

The four layout files still match the stationary implementation handoff byte for byte. Aggregate SHA-256, using sorted relative path bytes, NUL, then file bytes:

`6a32a530ea9fd01ba933a4275e289c5a7c404d72114c490fd04e84693d237331`

Reviewed changes: `layout.rs`, `layout/topology.rs`, `layout/topology_tests.rs`, and `layout/override_tests.rs`. Read-only boundary checks covered `command.rs::NodeSource`, the live/indexed `workspace.rs::verify_reference` methods, graph's closed storage/Builder boundary, and `transaction.rs::TransactionCapture`. The cold owner issuance/executor audit belongs to the other review lane.

`git diff 91f42ce` is empty for both `layout/reference.rs` and `layout/projection.rs`. Their current SHA-256 values are respectively `93afdf6e8ad79d81f2efdafc4814a4bb6076fd9f535129f82ce33f9ee12ddf4e` and `c31bfdad6ec84a8714f1d93fa93f97fdb96cb96226349263018da09c62dd742e`.

## Sealed compiler assumptions are supported at the caller boundary

`RootLayoutTopology::compile` now accepts `&Workspace` (`layout/topology.rs:57`); the malformed compiler accepts `&WorkspaceData` only under `cfg(test)` (`:166`). Production `measure_root` calls the former (`layout.rs:747`), while the test-only `project_draft` calls the latter (`:628`). No runtime Boolean switches the visited/cycle checks off for an arbitrary draft.

The trusted premise is a rooted tree with existing complete-generation child IDs, unique structural ownership, and matching child/weight cardinality. `Workspace` owns private storage (`graph.rs:389`); its ordinary storage getter returns only `&WorkspaceData` (`:418`). Builder edits stay in raw storage, and `build` goes through complete canonicalization/validation before `issue_storage` (`graph.rs:710`, `canonical.rs:229`). The empty default Workspace has no root and reaches the normal `MissingRoot` error.

The new transaction staging API does not hand an unfinished candidate to arbitrary layout callers. Its closure receives `TransactionCapture`, whose Workspace field is private (`transaction.rs:125`). Its public methods expose node/root/MRU reads and typed command captures, with no Workspace getter, Deref, AsRef or conversion. This matters even for the cfg(test) public command/layout paths: a typed captured NodeSource is sufficient to construct an override, but it cannot recover the staged Workspace and call the sealed compiler before finalization. Internal graph mutation and publication correctness remain the graph owner's responsibility.

## Direct child slots preserve traversal, arithmetic and error order

For each split Enter, the compiler reserves exactly its child count in one contiguous range, records immutable weight bits once, pushes its Exit, then pushes child Enter frames in reverse enumerated order (`topology.rs:109–132`). Each child Enter fills the parent's exact reserved slot with its dense row before processing the child (`:87–95`). Consequently:

- children are entered left to right, as before;
- every child measurement completes before the parent Exit;
- Exit copies measurements from the reserved slots in original child order (`:141–155`);
- leaf answers and split composition errors occur at the same DFS entry/exit positions;
- every reachable node contributes one row and one measurement, with no generation truncation.

The zero measurement and zero child-row placeholders remain private construction state. Under the sealed-tree premise, no successful Exit reads an unfinished slot. An earlier error discards the entire local topology and values. No partially filled projection escapes.

Durable weights are copied on Enter instead of Exit. They are immutable throughout the borrow, and reading their validated bits has no fallible operation, so this change does not reorder a numerical error. The solver, composition helpers, f32-to-f64 conversion and per-child arithmetic are unchanged in the patch.

The old checked traversal is retained in `compile_draft`, including active-versus-completed rows, child/weight mismatch detection, missing-child joins and final Option completion checks. Its original malformed DAG/cycle/error priority is exercised through the registered raw diagnostic tests, rather than by manufacturing an unchecked runtime Workspace.

## Split ordinals and identity-ordered directories remain consistent

Production split storage now receives preorder ranges and ordinals. `finish_directory` subsequently sorts the public iteration directory by full `NodeId::Ord` (`topology.rs:303`). It does not rewrite the ordinal stored in each `LayoutNodeKind::Split`.

This is consistent with the consumer. `prepare_split_projection` writes the outcome using that node's stored ordinal (`layout.rs:1047`), and `SplitMap::at_row` reads the same ordinal (`projection.rs:236`). `split_rows` selects identity-ordered iteration and supplies the split count; it is never used as an ordinal-to-node inverse. Every split receives one distinct ordinal in `0..split_count`, and directory sorting preserves that count. The separate child/gap ranges are likewise read from the same owning node at write and access time.

Node lookup continues to binary-search the sorted directory using the entire NodeId (`topology.rs:318`). Storage remains proportional to reachable nodes/edges, independent of arena holes and high occupied slots. There is no slot-only or root-sized secondary map hidden in this compiler.

## Exact owner references remain independently checked

`NodeSource::reference()` returns `&RootReference` (`command.rs:243`), and layout passes it directly to the actual live or indexed authority API (`layout.rs:663`). Every override runs this call independently; the previous fingerprint pointer cache, root fingerprint rebuild and skip flag are gone.

The live method captures actual root/node membership before comparing the supplied exact reference (`workspace.rs:781–820`). The indexed method first checks version, indexed root, live root, current node existence/membership and the index's own reference (`workspace.rs:299–322`). It then independently verifies the supplied source reference against Workspace (`:325–335`). Thus a current source does not authorize a stale index, and a current index does not authorize a stale source. A shared reference does not bypass per-node existence or membership.

The override loop preserves this ordering (`layout.rs:655–708`): source-root mismatch; actual authority; duplicate node; actual split kind; cardinality; ordered f64 conversion and normalized sum. A stale second source for an already-mentioned node still fails authority before the duplicate check. A missing or foreign node carrying a valid same-root reference still fails membership.

The public complete-constraint entry still checks requested-root existence and all overrides before measuring (`layout.rs:483`). The minima path still completes measurement before validating overrides during `RootMeasurement::project/project_with_index` (`:528`). Existing cold/warm error-order tests retain that intentional difference.

Geometry reuse remains a separate concern: `topology.matches` compares actual root node, central leaf, complete node IDs, node kinds, axes, ordered children and exact weight bits (`topology.rs:326`). Live leaf contents, minima and metrics are remeasured. A structurally reusable topology does not grant command authority. The retained test at `topology_tests.rs:441` explicitly combines shared topology with rejection of an old semantic target reference.

## Oracle independence and complete owned output

The frozen reference still measures through its own BTreeMap/BTreeSet traversal, projects through its old per-split owned vectors, and executes its own numerical solver (`reference.rs:164`, `:312`, `:581`). It does not call the new sealed compiler, checked compiler, dense solver or dense projection writer. Its result remains a separate owned BTreeMap value type. `capture` copies every actual node rectangle, node constraint, split axis, gap rectangle, child minimum/maximum/actual extent, central index and both residual fields (`reference.rs:39–69`).

The exact independence boundary should remain explicit: both reference and production still use the existing override-validation adapter, leaf-value types and composition helpers. This was already the boundary at `91f42ce`, and none of those numeric helpers was changed by the reviewed patch. The differential oracle therefore checks traversal/storage/solving; the explicit authority and error-priority assertions check command acceptance. A passing reference comparison alone cannot prove owner authorization.

Successful dense outputs still own all rectangle, constraint, child-extents, gap and outcome vectors, and share only immutable topology via Arc (`layout.rs:606`, `projection.rs:170`). Neither Workspace, leaf input, index nor scratch is borrowed by `LayoutProjection`. Each new solve allocates its own mutable output vectors. The registered old-output test retains the first complete captured result, creates a second projection with different bounds/metrics, drops index and Workspace, then rereads the first (`topology_tests.rs:126`).

## Actual registered coverage and remaining verification

`layout.rs` declares `topology_tests` and `override_tests` under `cfg(test)` (`:1342–1349`), and `lib.rs:128` includes layout in the library test build. These are ordinary registered unit tests. `Cargo.toml` still has `autotests = false`; no claim here relies on legacy `tests/layout_solver.rs`.

The active `projects_ten_thousand_nested_splits_without_call_stack_growth` (`topology_tests.rs:348`) builds 10,000 alternating-axis splits, projects the complete tree, drops leaf constraints and Workspace, then verifies 20,001 rectangles/constraints, 10,000 split outcomes, two entries in every child min/max/extent array and one gap per split before explicitly dropping the projection. Alternating axes prevent canonical flattening from removing the intended depth. The structural and projection stacks and the output ownership remain iterative/flat.

The new sparse/recycled case (`topology_tests.rs:232`) uses retained old-generation leaves plus recycled slots, a separate high-slot root, n-ary/opposite-axis splits, nonuniform finite constraints and a central leaf. It compares complete cold constraints and cold/warm minima outputs against the frozen reference, verifies full identity-ordered queries, rejects retired generations and excludes the other root. Existing properties cover general fanout, bounds, gaps, central allocation and complete captured results; the axis property also compares successful scalar output bits.

One useful remaining seam regression is a warm topology plus a same-version stale index and a newly captured current split source: geometry measurement should remain usable, while `project_with_index` with that source must reject the stale index owner. Current source checks both references and the core owner-reference tests exercise the same stale-index gate; the layout-specific test would lock the intentional distinction between structural hints and command authority in one call chain. This is a coverage opportunity, with no current acceptance bypass found.

No further source blocker or required layout algorithm change was found. Root's running combined compile, focused registered tests including the active 10k case, and later ordinary qualification remain the execution evidence. This review makes no new pass or performance claim and does not attribute a gain to the unmeasured linear compiler slice.
