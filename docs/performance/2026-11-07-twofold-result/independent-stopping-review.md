# Independent final stopping review

**Recommendation: stop this bounded optimization-search round at retained `0e441c8ddeb6f85b9b0e8237303b5def32e178ae`. The all-cell 2x numerical target remains unmet: 30/42 group/cells pass and twelve small group/cells miss.** The alternative acceptance route—reasonable optimization exhaustion with independent review—is supported for this round. This is not a claim of optimality, impossibility, a Rust language penalty or a latency lower bound.

The last specifically attributed opportunity was completed rather than abandoned: standard-Vec initial reservation passed ordinary qualification, met its predeclared comparison rules and was retained. I find no additional, comparably concrete and bounded structure change that the current evidence requires trying before stopping. This performance decision does not replace existing native/product qualification.

## Final source and measured gap

Final frozen binary: `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. Its actual control is `49baba1b2b7ad8e152e8b4da180a2e130d5fce9f`, binary `6d255458eac25cd9dcf629b88cfca6959c78db7ed2bb7ccf08fe2cba6332dcfd`.

I checked all 324 frozen source hashes against the immutable final commit and the 323 ordinary-qualification inputs against that receipt. The sole delta from 49baba1 is the `project_subtree` initialization: `Vec::with_capacity(topology.nodes.len().min(8))`, followed by the original first Enter push. Probe, timer, traversal, numerical checks, fresh leaf inputs and complete owned output/export are unchanged.

Independent recomputation from the final ImGui campaign gives:

| Total panes | Group/cells meeting 2x | Complete p50 / pinned ImGui |
| --- | --- | --- |
| 16 | 2/14 | 1.8111–3.1244x |
| 128 | 14/14 | 1.2285–1.4969x |
| 1024 | 14/14 | 0.8292–1.1445x |

These are two groups of the 21 declared cells. Both small single-view resize conditions pass. The twelve misses comprise both groups of single-view center/edge and all four-view cases; four-view resize is 2.1328x in both groups. They remain visible failures of the numerical goal. Different campaigns have different observed control times; ratios and absolute times must not be mixed across campaigns.

## Evidence checked for the final reserve

I independently recomputed all five phase p50/p95/max summaries from both campaigns' **16128 raw observations**, checked 48 unique sample indices per process/cell and 96 observations per condition/group/cell, and compared all **336 complete exported vectors** byte-for-byte. Raw state digests agree across conditions for each workload/width. Executable hashes and environment/result bindings match. Both campaigns use the original serial ABBA/BAAB protocol and eight warmups.

The twelve counter-enabled ordinary gates return zero, including core, workspace, public integrations, both host variants and the existing independent numeric/full-layout, deep, raw, error and lifetime witnesses. The 21-row/20-column work CSV is byte-identical to the gather control. It measures logical work, not allocation count or capacity bytes. Archive verification confirms 425 manifest-bound inputs; available original inputs also match the archived bytes.

Against actual control, complete p50 has **28 improvements / twelve regressions / two ties**, reductions -3.93% to +9.08%. P95 has 23/18/1, reductions -79.10% to +61.15%; max has 25 improvements/seventeen regressions, reductions -444.03% to +86.36%. Every adverse cell and tail remains in the evidence. The result is bounded improvement, not uniform acceleration.

The retention rule was recorded at `05:30:16 UTC`, before the first comparison at `05:43:51 UTC`: at least 28 median improvements, all medium/large cells still within 2x, inventory at least 30, and no layout-p50 regression in either group of either preselected diagnostic workload. It is met. Small four-view resize layout changes `1167→1042` and `1166→1041 ns`; complete medians improve 8.57%/8.76%. Single-view edge layout improves in both groups, while its complete operation ties in group 1 and improves 1.28% in group 2. No further capacity was selected after observing these scores.

Bindings under `B=/tmp/dockspace-imgui-twofold-20261107-ea4161e`:

| Input | SHA-256 |
| --- | --- |
| `projection-stack-reserve-frozen/source-receipt.json` | `03a4404a1fa9ed1f38a8aad04e3cef4a64e4f984b0d9ff7bb55d758f6ea262f6` |
| `projection-stack-reserve-frozen/runtime.patch` | `c7c97ca62a98971d86b0365d8e76eff93bf99385e3f556bddec0f0f44e67e2ac` |
| `projection-stack-reserve-frozen/against-baseline/summary.csv` | `346f04c65ff3571731c4d4411a33b219199ad4c544e00c84c1f13dbe9ffaf73a` |
| `projection-stack-reserve-frozen/against-imgui/summary.csv` | `a57ec57e38e0503d5208585304852420fa21bf37885d9bc357632ac026a08ed6` |

## Overall result against the original

Use **`final-retained-confirmation/against-original`**, not the earlier 49baba1 confirmation. It pairs this final binary with original `ea4161e221ff8687f344b98f4223dec1efa38873`, binary `72dae29c00afdf725ead3a24a1b9826e48f02314803ed4a38094bb5d4d66ab6b`.

I recomputed its 8064 raw observations, summaries and 168 paired complete vectors. Complete p50 improves **42/42**, by **78.8192–96.0642%**; p95 improves **42/42**, by **78.1469–96.4670%**. Maximum improves 41 cells and regresses one: group 2, 16 panes/four views/edge, **29041→39042 ns**, a **34.4375% regression**. That tail is retained without assigning an unmeasured cause. The greatest max reduction is 97.8439%.

This is a direct overall comparison on the same declared matrix, not summed slice gains or a held-out workload. Its summary SHA-256 is `c38bca8f3aa5f8c97eebe5c854bb3152190c5bdb9eae813c3517ca51a1f4ae6a`; `original-confirmation.json` is `ad64ec19310f080a667f696b34a042b6927492d38224f2b374eb4cac34ba9996`. Substantial original-to-final progress does not erase the twelve comparator misses.

## What has been tried and rejected

Earlier complete statistics were checked against each experiment's own frozen summary and decision. Their controls differ; percentages cannot be chained or pooled into one universal treatment.

| Distinct implementation | Actual control | p50 improve / regress / tie | Candidate deviation from control | p95 / max regressions |
| --- | --- | --- | --- | --- |
| SmallVec16 continuation | `eb94cb2` | 0 / 42 / 0 | +2.52% to +33.01% | 39 / 23 |
| Fused continuation transitions | `eb94cb2` | 5 / 36 / 1 | -3.38% to +12.44% | 31 / 20 |
| Numeric-executor inline hint | `eb94cb2` | 18 / 19 / 5 | -8.57% to +3.91% | 17 / 20 |
| Initial committed-axis validation split | `07f566a` | 1 / 41 / 0 | -1.54% to +24.47% | 38 / 28 |
| Axis split plus hints on packed results | `37e388f` | 0 / 42 / 0 | +2.34% to +25.54% | 38 / 30 |
| Persistent NodeStore/allocator separation | `37e388f` | 0 / 42 / 0 | +67.75% to +151.02% | 41 / 35 |

Positive deviation means slower. Favorable medians and tails were preserved alongside rejection. These are different tests of temporary storage, control flow, compiler boundaries, validation and canonical storage, with ordinary qualification and complete comparisons. The persistent implementation truly removed the N-handle fork traversal, yet complete moves and resize regressed. That rejects this tradeoff, not every possible persistent directory.

This record also includes retained local canonicalization, owner-issued references, payload/MRU/metadata sharing, unified node values, packed child results and binary gathering, as well as the separately documented rejected slab and child-row treatments. The search did not stop after merely replacing a container or after one failed hint.

## Why the current diagnosis supports this disposition

The useful `current-long-profile` belongs to the **49baba1 control**, not the final reserve binary. I checked its separate diagnostic binary `b6c2b06483361b118d22c916ef0f06e1f9f8546d4d3d80eea11bf914a587e900`, 324-entry source maps and complete probe pair. Only the iteration ceiling changes from 10000 to 1000000; per-iteration work and core are identical, and source restoration precedes sampling. The earlier short profile remains preserved as inadequate.

Independent call-tree parsing excludes fixture ancestry and nested double counting:

| Diagnostic workload | Worker samples | Named timed layout | Named timed prepare | Continuation-grow inclusive samples |
| --- | --- | --- | --- | --- |
| 16 panes / 4 views / resize | 2905 | 193 | 0 | 25 |
| 16 panes / 1 view / edge | 2941 | 175 | 218 | 18 |

The grow path is measured `project_all_roots→project_measurements +4260`. Saved assembly identifies the return address `0x100b388ac`, a 48-byte/eight-aligned grow, then writes the Enter tag, row and four rectangle scalars. Thus it locates a concrete continuation expansion; counts are neither calls nor clock/allocator fractions. Fixture, postvalidation, disposal and ambiguous inlined probe samples remain excluded or unclassified.

Reserve directly changes the initial allocation responsible for those early expansions, while preserving standard Vec and all event order. It still allocates per root. On this ABI the seven-node root requests seven frames instead of its former grown capacity of four: 144 additional requested temporary payload bytes, not measured allocator bytes. Larger roots retain growth and allocation classes/code generation can regress. Its measured gain does not prove this expansion's exclusive causal contribution or eliminate every possible future grow.

## Boundary of reasonable exhaustion

Remaining source work includes checked source/target/policy facts, an independent candidate, actual local mutation/repair and issuance, fresh leaf reads, bottom-up constraints, top-down geometry, rich materialized outputs and fresh owned export. Their behavior is required; their present representation is not theoretically mandatory.

`NodeStore::clone` still copies its SlotMap of Arc handles. We tested a mature persistent alternative and retained its broad failure. New issuance on actual edits still creates identity; it cannot simply be omitted without changing stale/branch contracts. Full solver checks remain, with two failed boundary/code-generation experiments. These facts identify possible design space, not another evidenced low-risk deletion.

Cross-root continuation/scratch reuse would widen the interface and lifetime handling while addressing expansion already targeted by this smaller local change; it supplies no independent one-view benefit. The preserved stacks do not separately attribute another avoidable scratch owner with a bounded solution. Repeating capacity choices, inline hints or container substitutions now would be speculative search rather than a necessary continuation. No fresh inputs, owned fields, error checks or publication barriers should be removed to obtain the score.

Accordingly, **stop this round and disclose the remaining twelve small-cell misses**. Reopen only for a new observation tying specific avoidable work to a bounded implementation and falsifiable complete-operation comparison. This conclusion does not establish optimality, unreachability or a language floor, and does not use summed phase medians as a lower bound. It asks for no additional candidate or measurement.

Only this report was written. No Cargo, compiler, tests, runner, timing, profiler, network, repository/fork/cache/candidate mutation or delegation was performed. No other final stopping opinion was read.
