**Findings first**

No source correctness, diagnostic-order, or oracle-independence blocker was found in the current exact `measurement-child-rows.patch`, SHA-256 `3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e`. The candidate is suitable as the next independent slice after its qualified slab base, followed by Root's serial compilation/behavior checks and separate measurement. No source correction or additional test framework is requested.

The production change removes child-gather storage while retaining the same numeric passes and fresh leaf/output work. The reference keeps its own tree traversal and fallible gather. It already shared composition helpers before this patch; that existing shared arithmetic boundary is accurately acknowledged, not presented as a wholly independent arithmetic oracle. The old solver-aggregate change is absent.

This is an independent read-only source audit. Only `/tmp/measurement-child-rows/review-final.md` was written. I read the prior review as background, then checked the current complete base/tree, patch, receipts, unchanged contracts, actual call surface, numerical bodies, raw compiler, reference adapter, and relevant retained tests. No Cargo, Rust compilation, test, benchmark, timing, archived runner, formatting, repository/candidate/fork/cache mutation, or sub-agent execution was performed.

**Exact artifact and composition binding**

The current patch is three paths, 6,941 bytes, +26/-40. I checked all exact path sets, per-file hashes/lengths, sorted source-set hashes, six contract snapshots, manifest/README/READY bindings, and independently reconstructed all three patch results in memory. Each result equals its complete frozen candidate file.

| Source under `crates/dockspace/src/` | Exact base SHA-256 | Exact candidate SHA-256 |
| --- | --- | --- |
| `layout.rs` | `e4597343ab8b0f8d69f94bebf7e23e190905dace9e77b942b2187c2ef555d26c` | `026384059d20282efcf773ad0daebc2722b0b2feb493933310ea9418e2fd6372` |
| `layout/reference.rs` | `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a` | `ea4afaf3fc82be4e3f27f8c00aa558d5aed0ff37ff15a184d3db3dc49f7e7b27` |
| `layout/topology.rs` | `d2d06077b9a5bfb03f764b0ed9c862de4e9aca3a65946059e480b8412f356f56` | `89e94dc277cbfe3d4734ddd16f25fffee4c735b5144f75fd3e4b119006ece9c9` |

The saved layout base is byte-identical to `/tmp/child-extents-slab-current/tree/crates/dockspace/src/layout.rs`, so the storage composition is exact. The verdict is against that frozen slab output and the saved topology/reference inputs; it does not assume the live layout file has already applied that slab. Integrate the patch after the matching slab or explicitly merge its hunks. Contract snapshots are review context, not files to overwrite, and later RootStructure/counter/native changes must be retained.

| Artifact | SHA-256 |
| --- | --- |
| Current patch | `3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e` |
| Three-path base set | `3eb2580e07bb0c3d670be826083b403f61c4dacaaefa769f0ff89784e1d70984` |
| Three-path candidate set | `9a0c41e3751877468dc92bcf90a9407bb89d1d16e87157f9e2de887da6ad68e1` |
| `base-manifest.json` | `6342d75e456e6c945c854eb8f959b3c68c0db48c2635cd207d2fd43ddef942ee` |
| `manifest.json` | `776d4c9182843d6f8df3ae66c580eb1866f8d2101785e2dfb5cded0ce655e99c` |
| `scope-checks.json` | `618381620c52c48f420e4bcef6d5038c95eedadef7af033e3d4801c16fe273c8` |
| `README.md` | `8308b3e9d28f58183029de85d4490fc800398dec3f4889ce78b14f1d7a1ad32d` |
| `READY.json` | `a9f444ebc8b4315bd84df207f8c0f583d088b2b60794399b9006c1497220bb1d` |
| Preserved previous review | `c89a89af148af4a80f1cbd07af84d90f0cd930e96ee8d62b115f653e28d4433c` |

**Actual iterator and borrow boundary**

All composition helpers remain private. Their current complete call surface is three `compose_constraints` calls, one `compose_minimum` call, and three `compose_maximum` calls across the three files. Each supplied iterator is a standard slice Iter/map sequence. No custom provider, custom iterator type, new owner, cache, flag, unsafe initialization, or extra numeric executor is introduced.

Production `RootStructure::measure_with_reader` (`tree/.../layout/topology.rs:136–177`) retains its fresh measurement Vec and original postorder. Every Tabs row still calls the supplied fresh reader at `:154`; every split now maps its immutable child-row list to existing `Constraints` at `:160–168`. These Constraints and SubtreeMeasurement values are Copy. The iterator closure only copies a stored field from the local Vec; it does not query a leaf, construct/validate a new constraint, or invoke a user callback.

Cloning these Map iterators clones their slice cursor and captured shared borrow. It does not clone the measurements Vec, allocate child storage, or evaluate the mapping function. Consuming the cloned iterator does reread the child row for each numeric pass. That is an implementation cost difference, not repeated measurement or a missing numeric operation.

The struct fields are evaluated in the original order: successful `compose_constraints(...)?` first, then the ordered short-circuit `contains_central` any (`topology.rs:169–171`). If constraints fail, the boolean scan is not evaluated. The full RHS finishes before `values[row]` is assigned, so the parent's write cannot invalidate or interleave with the child iterator's reads. Final `check_central` remains after the complete postorder at `:176`; its complete source is unchanged. No borrowed iterator or scratch reference is returned as output ownership.

**Minimum/maximum arithmetic and diagnostic order**

The minimum arithmetic tuple/body is byte-identical to base: 561 bytes, hash `4e8215a88b21f8fe237917998d657a31c11be92b04f380c0e94c13e4bfcd2749`. The maximum body is also exact: 581 bytes, hash `5e586e971532c121cea91f36250b4fa93edda3bd9508979419e3615a9a7e463c`. Only iterator preparation/signatures change. For every numeric pass, the mapped child constraints have the same values in the same order as the former gathered slice.

| Calculation | Horizontal branch | Vertical branch |
| --- | --- | --- |
| Minimum width | Child-order sum, then splitter extent; label `subtree minimum width` | Original `fold(0.0, f64::max)` |
| Minimum height | Original `fold(0.0, f64::max)` | Child-order sum, then splitter extent; label `subtree minimum height` |
| Maximum width | Child-order sum, then splitter extent; label `subtree maximum width` | Original `fold(f64::INFINITY, f64::min)` |
| Maximum height | Original `fold(f64::INFINITY, f64::min)` | Child-order sum, then splitter extent; label `subtree maximum height` |

The tuple still evaluates width before height; minimum still completes before maximum. LogicalSize construction stays after each tuple. `compose_constraints` (`layout.rs:791–814`) checks zero cardinality from the iterator length before computing total splitter extent, then minimum, then maximum unless minimum-only, then `Constraints::new`. Minimum-only still returns the minimum for both bounds during the initial pass. All checked-sum quantity strings and finite checks remain original.

`total_splitter_extent` (`layout.rs:869–886`) retains the original saturating splitter count, repeated thickness sum, and `NonFiniteSplitterExtent` fields/order. `checked_sum` (`:1325–1334`) retains `sum::<f64>()` and the same final finite check. The new map does no floating arithmetic while loading Constraints, and no width/height/splitter reduction is fused or reused.

I independently read the installed Rust 1.95.0 standard-library sources, without invoking a compiler: float Sum folds from `-0.0` with `a + b` (`core/src/iter/traits/accum.rs:156–165`); Map's fold forwards the accumulator to the underlying iterator (`adapters/map.rs:84–88`, `:124–128`); Chain's fold consumes its first iterator, then its trailing iterator (`adapters/chain.rs:116–126`). Thus the extra standard map layer preserves ordered addition, initial signed zero, and trailing splitter addition for the inspected pinned toolchain. No reassociated manual summation or cached aggregate is added.

Production row reads now occur lazily after cardinality/splitter validation, whereas the old gather eagerly copied valid rows first. In this graph-owned production directory, those reads are immutable, side-effect-free indexing over valid completed rows. This changes neither a reachable domain error nor callback order. It is not a reason to remove the fallible gathers on malformed/raw input, which correctly remain.

**Minimum-only delayed maxima**

`layout.rs:602–623` still waits until bounds are known, derives leaf maxima from bounds and the measured root minimum, then traverses the same postorder. For each split it reads child constraints from the already updated child measurement rows and calls the same maximum composition. Only after successful composition does it take `&mut measurements[row]` and validate/write the parent constraint.

The iterator is consumed before that mutable parent borrow; it cannot observe a partially updated parent or retain a stale borrowed temporary. Fresh leaf maximum, splitter extent, width/height order, `Constraints::new`, and dependency on completed child maxima remain. The separate production maximum gather Vec is removed, without moving this pass into initial measurement or changing override/minimum priority.

**Raw diagnostic compiler is still fully checked**

`layout/topology.rs:19–134` differs only in one composition argument. Reversing `child_values.iter().map(|child| child.constraints)` to its former slice argument reproduces that complete previous raw prefix exactly, hash `45e98ea987a73192418fd3b33792257faa2f289e6aa44350e975f1aabea4237a`.

The raw path still:

- resolves the root and every node, with the same exact missing errors;
- distinguishes active rows (`None`) from completed rows, rejecting active cycles before reuse;
- checks split child/weight cardinality at the original Enter position;
- preserves DFS order and completed-subtree reuse;
- clears and gathers every child measurement through fallible row/value lookups before composition;
- updates the topology child ranges and complete payload after successful composition;
- performs the same ordered central any, final root-central check, and directory completion.

A missing/incomplete raw child therefore still wins before its parent's composition/splitter diagnostic where it did before. No unchecked production row mapping has been substituted into this diagnostic owner.

**Reference remains an independent traversal/output oracle**

There are exactly two reference argument adapters: one in delayed maximum completion (`reference.rs:115–120`) and one in the measurement Exit step (`:230–239`). Reversing those two map expressions to the former slice argument reproduces the entire 28,186-byte base reference file, SHA-256 `2b2eee151d88bdd83048f0dfe7a4931a769aff6616da59e060200819008b209a`.

The reference still traverses actual tree children, tracks active/completed nodes, performs its own fallible `child_measurements` gather (`reference.rs:271–283`), retains separate measurement/output collections and capture, and uses its unchanged projection/axis-allocation implementation. It has not adopted the production child-row reading closure or bypassed missing/cycle/cardinality checks. Comparing it to production can still expose wrong row joins, skipped children, order changes, stale measurement reads, and output corruption.

Its shared `compose_constraints` and `compose_maximum` calls already existed before the patch. Therefore this candidate does not erase a previously independent arithmetic implementation, but a common helper arithmetic defect could affect both sides. Exact arithmetic-body comparison and the explicit fold-order audit above provide the evidence at that shared boundary; the report must not rely solely on oracle agreement to prove helper arithmetic. The README and prior review state this limitation correctly.

**Unchanged solver, leaves, outputs, and coverage**

Independent comparisons verified the complete public/solver/entry prefix, the fresh NodeResult/projection/override/leaf-measurement region, the 18,207-byte complete splitter/DFS/axis-validation/solver/inline-test suffix, raw prefix after its one adapter, central check, and fresh initialization. The source does not restore the rejected solver aggregate: weight validation, desired-size computation, grow/shrink passes, both assigned-size sums, and solver error paths remain byte-identical. The original projection continuation and traversal are retained.

The unchanged slab projection contract is hash `d770e03663941e840e5e37222d9663092c3e699f27b51dee535d301abc492c60`; it still owns all child extents, node results, outcomes, and deep-cloned scalar results. A fresh measurement Vec and fresh projection outputs are built for each request. No callback/measurement/scratch reference is substituted for owned output, and a failed current computation cannot write an older output. Benchmark/timer/export work is untouched by this patch.

No test is added, deleted, or relaxed. Relevant retained source coverage includes dense complete-value direct/minimum oracle comparison, fresh reader/metrics cases, `constraint_reader_preserves_depth_first_errors_and_override_precedence` (`contracts/.../topology_tests.rs:415`), minimum-before-override priority (`:814`), completed-subtree reuse and active-cycle/missing-leaf order (`:876`), malformed/raw missing-node cases, sparse generations, complete old-output clone ownership, late geometry failure, and the registered 10k-depth/drop case. Override tests also remain exact. These are suitable existing Root-owned qualification targets; no passing result is claimed here.

**Source cost and limits**

Production full measurement removes one reusable child-gather Vec and its explicit E per-edge SubtreeMeasurement materializations for a split root. The separate minimum-only delayed-max path removes its own gather Vec/materializations. A leaf-only path has no nonempty gather allocation to remove. Raw/reference gathers are deliberately retained.

This is not a claim that one allocation occurred per split or that all copying disappears: each numeric pass now loads and copies child Constraints directly by row. All numeric passes and central propagation remain; fresh measurement/output vectors also remain. Indexed rereads, bounds checks, monomorphized code shape, and less contiguous child access may offset saved materialization/allocation work. No allocator-byte, RSS, latency, speedup, or goal-attainment result follows from this source review.

The installed standard sources used for the fold argument are bound by hashes `ded19e8fda12eb066c97d237afcdfe3c44d058494e3e51f4e291807bcbba08a0` (`traits/accum.rs`), `2a9cb86b5110b292f271ec1cd8904a9bad27f8d553af0141837f41e753fe83d3` (`adapters/map.rs`), and `64e46d2857d1df7bf3e4028e9183f83c237e51d88e3d31c740d43e976b8d0231` (`adapters/chain.rs`). They were read only.

The candidate can proceed as this bounded measurement-storage slice after its exact slab composition. Root remains the sole owner of compilation, retained behavior gates, and any measured keep/drop decision.
