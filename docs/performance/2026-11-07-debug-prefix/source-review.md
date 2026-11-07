# Independent post-main stopping challenge

**I would not declare this round exhausted yet.** One newly examined, small production-only experiment is justified: move the first-visit prefix bookkeeping in projection entirely to debug builds, explicitly relying on the closed RootStructure construction invariant. It deletes per-node work without changing the public workload or introducing metadata, an alternate solver, or a scratch API. This is not an approval of a performance result.

A second, lower-priority opportunity is fused target verification and borrowed fact rederivation. It deserves cheap inspection of the existing diagnostic before another implementation, not a new broad profile or unconditional build campaign. Compiled projection events and cross-root scratch remain substantially less attractive at present.

## What the five experiments actually establish

I read the five experiment reports, the current diagnostic report and `post-main-continuation/experiment-log.json`. I did not consult the other stopping review. At inspection all **324** live formal input hashes matched the current control receipt: source checkpoint `c7508ee`, runtime `0e441c8`, frozen executable `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. The retained result is still 30/42, with twelve small conditions missing.

| Actual treatment | Useful negative evidence |
| --- | --- |
| Source-nonempty | Deletes a real content query; small-move gates pass twice, but repeated material resize negatives violate its guard. |
| Exact equal-binary desired sizing | Initial 34 complete / 13 small-layout improvements do not reproduce: confirmation 20 / 6. |
| Graph-build pending reservation | Located growth, but only 6/12 edge and 2/4 small-edge improvements. |
| Borrowed placement roster | Removes a real temporary copy, but only 5/12 edge and 2/4 small-edge improvements. |
| Insertion-aware Tabs COW | Avoids clone-then-grow, but only 2/18 center/cross and 1/6 small improvements. |

The first three completed twelve ordinary gates. The latter two stopped after their declared three-gate performance screens, with work/full-vector checks; calling all five fully qualified would be inaccurate. These results support rejecting those exact implementations and avoiding trivial capacity/container retries. They do not make a sixth independent hypothesis invalid by count, or turn genuine source deletions into mandatory latency gains.

The current diagnostic is source-bound to this control, unlike the old 49baba1 stack observation. Its corrected timed resize ancestry is 169 inclusive observations, not the original 170. The sample establishes locations, not time shares. `project_measurements +1632` is split-outcome allocation; `+1984` is the continuation stack; `+6564` disposes that stack.

## Concrete next candidate: debug-only prefix defense

`layout.rs:991–1002` maintains `visited_prefix` and performs a release assertion on every Enter. The final completeness assertion is already debug-only (`1020–1021`). The current output Vec is fully allocated before geometry, so the old increment no longer determines storage construction or a public result length.

This can be reconsidered on a precise boundary, rather than labeled “overvalidation”:

- `Workspace` storage/authority is private (`graph.rs:418–423`). Builders normalize and validate before issuing a Workspace (`729–730`).
- Every production RootStructure is created by `build`/`build_after_item_move`, whose common `build_rows` appends a row on DFS Enter and schedules children in reverse so they execute in their original order (`root_structure.rs:145–198`). The root is row zero.
- `finish_directory` sorts a separate identity index; it does not reorder those preorder nodes or their child ranges.
- Ready structure is published as an immutable Arc; structural edits change it to Editing and completion builds a new body (`authority.rs:103–186`). Existing restoration reinstalls a previously owned body. I found no production writer that mutates published rows.
- The cfg(test) raw compiler uses the same DFS first-encounter numbering (`layout/topology.rs:35–79`). Completed shared-subtree revisits use earlier rows; cycles and missing nodes fail before projection. Re-entering a completed subtree cannot discover a previously unnumbered descendant.
- Projection schedules each next child only after its preceding sibling's complete subtree and following gap. Inductively, first Enter is at the next prefix row; raw repeated Enter can only address an earlier row.

Thus a skipped in-range prefix is not expressible through the supported public projection/Builder inputs. The release assertion is a private programming-error defense, not an input-dependent LayoutError or an authority check. Numeric operations, array bounds, error ordering, complete fresh results and old-output ownership need not change.

Earlier unified-values review correctly required preserving this release rejection under that candidate's stated differential contract. A new explicit experiment may refine the internal defensive boundary, but must acknowledge it: malformed manually constructed private RootStructure gaps would lose that particular release panic defense. Do not silently claim byte-for-byte behavior for unsupported internals. Keep the full prefix assertion/increment and final completeness check in debug, including the raw test adapter; do not replace completeness with the prefilled Vec length.

The expected deletion is one release comparison/branch plus conditional increment per Enter, with no new persistent flag or validation framework. I performed the cheapest location step: static disassembly of the existing source-matched diagnostic (`35eeac8262ad7a4445a6bfb14f4f03de1d1672181338d42ef922eabd09ca2590`). Its `project_measurements` has prefix load/compare/panic branch/conditional increment/store at unslid `0x100b38200–0x100b3821c` (`ldr`, `cmp`, `b.hi`, `cinc`, `str`); ordinary row bounds checks remain distinct. The work survives optimization. This is an instruction-location finding, not a time share.

No additional sampler or allocator attribution is needed for this hypothesis. It is small enough to justify a normal isolated performance screen, followed by required full qualification/confirmation only if promising. Existing full-layout/raw-DAG/deep/late-error/ownership oracles remain the semantic checks; a test of the chosen `cfg` spelling would add no value.

## A second source duplication worth locating, not automatically implementing

`operation.rs:2928–2946` verifies the supplied target reference, then calls full `capture_tab_target`. The public capture again checks membership and clones the current RootReference (`workspace.rs:626–640,763–789`). Edge validation similarly verifies then recaptures (`operation.rs:2895–2925`). The current center sample contains this actual chain under timed `check_move`, separately from request capture.

Source/target authority and forged surface/central/rule facts must still be checked. What appears redundant is the second structural lookup and temporary owned issuance used only to compare facts. A small concrete borrowed fact helper could serve capture and validation, leaving capture to clone its durable reference and validation to compare scalar facts after its required reference checks. It must preserve root/node/outside/stale/type/fact/index error priority; the Indexed path must check both the index's exact issuer and the supplied target's issuer. Do not conflate them because root IDs match, trust public-crate snapshot fields, or create a reusable checked token.

The cheapest next step is inspect the existing optimized `validate_tab_target`/capture path for repeated checks and atomic clone/drop. If that work is already eliminated, stop this hypothesis. If it remains, a narrowly scoped fused helper is meaningfully different from the rejected arithmetic propagation or placement borrowing. Source alone supplies no nanosecond prediction.

## Why the larger remaining directions are not mandatory now

**Compiled projection events:** they can remove the per-root stack, but a direct plan adds approximately two structural events per edge, pays for compilation on changed roots, and still needs exact compressed thickness at post-descendant gap construction. Storing thickness expands fresh outcome state; recomputation repeats numeric work. Raw DAG occurrence/last-write behavior needs an explicit adapter decision. This is a legitimate architecture experiment, but a much larger ownership/traversal tradeoff than the new guard candidate. The known stack allocation is insufficient evidence that this added metadata will improve complete edge operations.

**Cross-root scratch:** on the formal binary matrix constraints, weights, sizes and measurement gather already use local arrays. Reuse chiefly amortizes the continuation stack, not four imaginary heap buffers. The frozen Adapter invokes stateless `project_root` separately for every root (`docking_cost.rs:242`). Honest explicit scratch reuse requires caller/API migration; hidden thread-local or Workspace caches are inappropriate. Treat that as a separately declared product/API experiment, not an unchanged-formal-workload win.

**Fresh input/results/export:** `docking_cost.rs:197–256` actually gathers fresh leaf answers and projects every root. `329–350` exports a fresh owned Vec and joins actual graph identities/content. The three rich projection buffers own materialized node constraints/rectangles and all child min/max/actual/gaps. Deleting these owners or replacing the reader with constant answers would change the declared work. Their placeholder initialization is not automatically minimal, but safely removing it needs new construction ordering; neither the rejected slab nor a source owner count establishes its value.

**“Duplicate root sorting”:** `finish_directory:210–220` sorts one identity directory once, then filters it for split iteration. The build's provisional `split_rows.push` is overwritten, so a scalar ordinal could avoid some temporary stores. That does not delete the required identity sort or a final owner, and currently lacks separate location evidence. Presentation sorting in initial issuance and full WorkspaceIndex builds are cold/other paths; the formal request captures live opaque refs. Do not transfer fixture sorting samples into timed command attribution.

The reasonable endpoint is evidence-based: test a supported small deletion that is still concrete, reject it if its own complete results fail, and defer larger unlocated tradeoffs. Do not combine rejected deltas hoping for unmeasured synergy. After the guard hypothesis is resolved, if target recapture has no remaining duplicate work worth a bounded screen and no new concrete location changes that assessment, stopping this round is defensible despite the twelve misses. That would be a practical decision for this workload and contract, not optimality, a Rust/ownership floor, all-cell success, or a prohibition on future optimization.

Evidence bindings: scope SHA-256 `9b02a466aa65dd3b8b8a4d3a08280033cf3adad08e8266ab14662664f778de39`; experiment log `2cfd6c2206de9e65d68fea4aa3abf862ad0e67491e182574f863e7442d67371d`; current `layout.rs` `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd`. Only static reading/hashing and inspection of an existing binary's instructions were performed. No repository/candidate change, Cargo, test, runner, profile, timing, network or additional agent was used.
