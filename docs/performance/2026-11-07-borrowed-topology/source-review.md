# Independent final continuation challenge

I do **not** recommend declaring the search exhausted yet. One additional, narrowly bounded deletion is justified by the current source and surviving instructions: borrow the completed topology through synchronous projection instead of acquiring a temporary third `Arc<RootStructure>` owner. It needs only the private layout call chain, leaves both durable output owners intact, and does not require changing the frozen caller. I recommend screening that one treatment before the final stopping decision. This is not a prediction that it will win.

The seven rejected treatments should remain rejected. I found no reason to repeat their campaigns, combine their deltas, weaken their gates, or retain a favorable initial result after a failed confirmation. I also found no present justification for undertaking compiled traversal events, a new container, or a scratch API migration as the next step.

## Evidence and current state

This review read the seven reports and decisions, the corrected current-source diagnostic, the experiment ledger, and the relevant live implementation. It did not read the other final stopping assessment. No builds, tests, runners, sampling, timing, network access, or repository modifications were performed. Static `llvm-objdump` inspection only read the existing diagnostic executable.

The retained runtime is `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`, qualified control head `c7508eec3ac44138305797691244ae9c180d7680`, formal executable SHA-256 `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. The numerical result remains 30/42 conditions at most 2x ImGui: 16 panes 1.8111–3.1244x and 2/14 passing; 128 panes 1.2285–1.4969x and 14/14; 1024 panes 0.8292–1.1445x and 14/14. Twelve small conditions still miss.

Both fixtures are balanced binary trees. Rust's fixture constructs two children at `docking_cost.rs:77–80` and asserts binary topology during export at line 298; the pinned wrapper uses `DockBuilderSplitNode`. Sixteen is the total pane count, not Rust's split arity. A 16-way-versus-binary explanation is inapplicable.

The ledger records all 324 control inputs restored after the seventh rejection. I additionally compared the nine source files listed below directly with `git show c7508ee:<path>`; every byte matched. This is a selected-source check, not a new independent rehash of all 324 inputs.

| Treatment | Decisive evidence retained | Qualification boundary |
| --- | --- | --- |
| Source nonempty fact | Move gates passed twice; repeatable material resize negatives failed protection | Twelve ordinary gates and additive Rust confirmation |
| Equal binary half-share | Initial 34/42 complete and 13/14 small-layout improvements became 20/42 and 6/14 on confirmation | Twelve gates; both campaigns retained |
| Build pending reserve | Edge 6/12 below 8; small edge 2/4 below 3 | Twelve gates; failed initial screen |
| Placement roster borrow | Edge 5/12 below 8; small edge 2/4 below 3 | Three early gates, work and full vectors; not twelve gates |
| Tabs insertion COW | Affected 2/18 below 12; small affected 1/6 below 4 | Three early gates, work and full vectors |
| Debug prefix | Complete 14/42 below 28; small layout 3/14 below 8 | Three early gates, work and full vectors; release defense restored |
| Live Tabs facts | Affected 4/18 below 12; small affected 1/6 below 4; complete 10/30/2 | Three early gates, work and full vectors |

These are negative results for their particular implementations. The seventh specifically covers Live Tabs recapture, not every edge/indexed authority path. The sixth explicitly refined an internal release-panic defense; it was not a claim to preserve every internal panic. Neither treatment survives in the retained source.

The current diagnostic binds the retained core and only raises the diagnostic iteration cap. Its executable is `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`. Corrected inclusive timed-layout classifications are 169 for four-view resize, 142 for single-view center, and 177 for single-view edge; edge candidate preparation has 218. These are location evidence, not exclusive clock shares or allocation percentages. Fixture construction and later result destruction cannot be merged into timed cost. The correction excluding one inlined fixture subtree is retained.

## The remaining small, concrete opportunity

At `layout.rs:504–508`, public synchronous projection clones the workspace's completed topology Arc. That local owner is passed by value into `project_measurements` at lines 516 and 602–610. The helper then clones the same allocation into `NodeMap` and `SplitMap` at lines 661–662, uses the original local owner for traversal, and drops it before returning. Two durable map owners are necessary for the existing independently owned map/clone interfaces; the synchronous third owner is not.

This work survives in the current diagnostic, independently of the earlier failed Live Tabs treatment:

| Existing instruction | Source role |
| --- | --- |
| `project_all_roots`, unslid `0x100850a28`: `ldadd ... [x9]` | Acquire the local topology Arc; it is saved at `sp+0x88` |
| `0x100850ed0` / `0x100850ee0` | Pass that saved pointer as the topology argument and call `project_measurements` |
| `project_measurements`, `0x100b37df4` and `0x100b37e10`: `ldadd` on the same allocation | Acquire the two durable map owners |
| Success return through `0x100b391e0` to `0x100b39110–128`, including `ldaddl ... [x19]` | Release the original local owner |

The helper saves its incoming topology pointer at entry, and restores that pointer before the common release. These are ordinary Arc reference-count operations, not three heap allocations. Their presence establishes neither their latency share nor an achievable speedup.

The smallest Interface change is private: `project_measurements` accepts `&Arc<RootStructure>`. The direct public entry borrows `workspace.root_structure(root)` rather than cloning it; the two output maps still each clone that Arc. The delayed `RootMeasurement` continues to own its topology while measurements wait for bounds, borrowing its own Arc only during the consuming projection call. The cfg(test) raw adapter keeps its locally owned compiled Arc and borrows it through the same helper.

This can remain in `layout.rs`, covering its three actual call sites and the helper signature/body. It requires no public lifetime, new proof, provider, cache, state bit, secondary executor, changed output representation, or new dependency. In particular:

- The workspace borrow already covers synchronous leaf measurement and projection. It keeps the original allocation alive until both output owners have been acquired.
- The delayed measurement owner must not be weakened into a borrowed workspace-only view or lose its independent values.
- Public results remain fully owned; node and split maps and their clones may outlive the workspace and local measurement.
- Leaf callbacks, override validation, postorder composition, central routing, numeric validation, raw DAG revisits, prefix checks and late geometry errors stay in their current order.
- Error paths drop any output owners already acquired. Removing only the extra local owner introduces no publication or rollback boundary.

This removes one Arc retain/release pair per direct projected root. It does not remove any fresh result field, buffer, leaf read, arithmetic, or normalized export word. It affects the actual all-root formal call chain without migrating `docking_cost.rs`. ABI/register-allocation changes may outweigh the deletion, as earlier experiments demonstrate. No gain is asserted.

The cheapest location check has already been completed by the static instructions above. The next useful validation is the existing bounded source review and early complete-operation screen, with work/full-vector equality and predeclared complete, small-layout and tail protection. Use the retained binary as control; do not compare against a rejected candidate or invent an Arc microbenchmark. Existing delayed-minimum, raw-DAG, late-error, output-clone and lifetime coverage is pertinent; a mirror test asserting a particular number of Arc calls adds no semantic oracle. Retention would still require the remaining ordinary gates and the declared confirmation policy.

## Why the larger alternatives do not compel another implementation now

**Compiled projection events:** this is a real unmeasured way to eliminate the per-root continuation stack, not something the failed SmallVec or fused-control experiments disproved. But the stack holds a cursor and the original compressed splitter thickness across child traversal (`layout.rs:959–969,1208–1230`). Recovering a cursor from a rectangle width would introduce a subtraction and change signed-zero/rounding behavior. Reading an already computed rectangle maximum can avoid that subtraction, but still needs parent-edge occurrence and thickness information and the original descendant-before-gap order. A persistent event/parent-edge plan adds root-build work and retained metadata exactly where timed edge completion already does work. Raw shared-subtree occurrences cannot be collapsed to one node row. The present profile locates a stack allocation/free, not an exclusive opportunity large enough to establish that this broader trade is worthwhile. It remains a possible later architectural experiment, not a required next screen.

**Cross-root scratch:** on this binary matrix measurement gather, axis constraints, weights and sizes use local two-element arrays. Default Vec headers do not allocate payloads. Eligible scratch allocates only when correction actually needs it. A reusable operation-local scratch owner could mainly amortize continuation stacks across views, but the frozen formal Adapter calls stateless `project_root` for each root (`docking_cost.rs:215–253`). Honest reuse needs a caller/API migration; hidden global, thread-local or workspace caches would be the wrong seam. Do not count nonexistent four-per-root numeric allocations or move collection outside the timer to justify that change.

**Transient split rows and root finish:** `build_rows` uses `split_rows.len()` as the provisional split ordinal and pushes rows at `root_structure.rs:178–183`; `finish_directory:210–220` clears and fills that same Vec in identity order. A scalar could avoid provisional stores, but would not eliminate the final owner, its required capacity, or the one identity sort. The raw adapter assigns ordinals in a different completion order, which the final map already accommodates. There is only one sort of `ordered_rows`, followed by a filter; it is not two root sorts. Removing the final split directory instead would trade it for filtering during every identity-ordered split iteration and require revisiting exact-size/double-ended iterator behavior. No current attribution isolates the provisional stores as worthwhile. This is a small possible cleanup, not evidence requiring another performance campaign or a whole-directory redesign.

**References and independent candidate storage:** source/target authority, forged target facts, exact-generation membership, MRU and staged rollback still require real checks and owned candidate state. Live target rederivation contains repeat work, but its directly tested Tabs deletion failed. Moving the same approach to edge validation is untested, not refuted; the present small edge locations do not provide a distinct cost basis demanding that near-variant next. Replacing the generational candidate directory or its branch ownership would be a substantially larger mutation model, and the mature persistent-directory treatment already has negative complete results. Neither Rust's language nor ownership itself explains a measured floor.

**Fresh inputs and outputs:** the single node-values buffer already serves measurement and projection. The retained split/child buffers materialize the declared complete rich results. `docking_cost.rs:197–250` still gathers actual leaf input; lines 329–350 produce a fresh owned normalized vector; lines 441–468 time capture through that export. Constant leaf answers, borrowed exports, skipped fields or deferred calculation would change the workload. Their necessity under this contract is not a numerical lower bound.

## Publication and wording

The draft `docs/performance/2026-11-07-post-main-result.md` correctly preserves 30/42, the twelve misses, both products' binary topology, failed additive confirmations, the three-versus-twelve-gate distinction and the current diagnostic's limited scope. Its statement that no new runtime delta is retained is correct. I found no contrary result that would justify keeping one of the seven candidates.

Before calling the search practically exhausted, add the private borrowed-topology finding and resolve its bounded screen. This does not require changing the archived seven decisions. Describe the provisional split-row scalar cleanup as avoiding stores, rather than implying that it necessarily entails a complete structural redesign.

If this remaining ownership deletion fails its own complete-operation rules, the current larger, unlocated tradeoffs do not by themselves compel continued variants. A practical stop could then rest on tested treatments, preserved negative evidence, current-source localization and the absence of another sufficiently attractive bounded change. It would still not establish optimality, impossibility, a Rust floor, or all-cell numerical success. Candidate count alone supplies no stopping criterion.

Finally, this review does not close publication. The existing six green jobs qualify `c7508ee`; the final pushed head must still receive its own six-job qualification.

## Source and evidence bindings

All source paths below are relative to the repository and were byte-equal to `c7508ee` during this review.

| Path | SHA-256 |
| --- | --- |
| `crates/dockspace/src/layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` |
| `crates/dockspace/src/layout/topology.rs` | `1c0bd28862711316cbe9f963e46133a7f6440e774fc6686e5a1c056f626c94cb` |
| `crates/dockspace/src/layout/projection.rs` | `fb6fb6454034656114bfdcb80049f53fbbc325a780af00f720efd7769fa1494e` |
| `crates/dockspace/src/graph/root_structure.rs` | `a89bf210b4836d10380007e1d5594ad6c76281a338b12d10f0408ba7ee98eca7` |
| `crates/dockspace/src/graph/authority.rs` | `89d0d337cdc2365caf62f5dd2d48d6f9132b21c2d4b882e9c24096cc6602a600` |
| `crates/dockspace/src/graph/shared_storage.rs` | `18c6f75150643779acb9c71c30486634b19f7ccec5677e4cc64ce9043fcc532e` |
| `crates/dockspace/src/workspace.rs` | `6cbcca776bfca60b1e767dc909121f06ae751110ea1f9fd265ea398da644aa36` |
| `crates/dockspace/src/operation.rs` | `7586ec06b29b77a794133e0c554d4fcca7a6faa16b8c550b86034ad394175862` |
| `crates/dockspace/src/docking_cost.rs` | `01d8438fc79095aacae935170aff3ed7eb8b5e497aa368c9966889b789adf2d3` |

Evidence root: `/tmp/dockspace-imgui-twofold-20261107-ea4161e/post-main-continuation`.

| Evidence | SHA-256 |
| --- | --- |
| `experiment-log.json` | `cf440ec57d46bb6f1bcd0a0770a068a3d34bf606010d427d984f9ec29bde5eed` |
| `current-main-profile/receipt.json` | `ec94fad210c92a69b6d0d86060bcf40b592542f7a0857f81353a159fc13a0f01` |
| `current-main-profile/attribution.json` | `0e2282ea56034a1b3a18ac4ba654a5dc54eeb8176cc937a2a8c3eb6c0044eed6` |
| `current-main-profile/diagnostic_docking_cost` | `35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590` |
