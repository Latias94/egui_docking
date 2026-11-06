# Independent review: solver aggregate reuse

Date: 2026-11-07. Verdict: **no blocking numerical or error-equivalence defect found** in the prepared patch or its exact application on the leaf-reader baseline. This reviewer did not author the solver patch. This is a source and existing-receipt review, not a newly executed validation or performance measurement.

Only source and temporary artifacts were read. No repository edit, Cargo/compiler/test execution, timing, benchmark or Git mutation was performed. The patch was replayed entirely in memory for provenance verification.

## Exact subject

| Artifact | Independently calculated SHA-256 |
| --- | --- |
| `solver-aggregate.patch` | `c1e9e5788c922d2042378b8dc22545cf373ca39be476e4c439d27441802b6937` |
| original `base-layout.rs` | `354f015c74a2e942d006d7c1e510a26df3f43ebf49e42bf0763687ab38fdd44f` |
| original candidate `layout.rs` | `f46c9e5c3d6659263995604fc4e28a1839ed3ae3f1bdbac4ba39f1b4d174d16b` |
| actual leaf-reader `applied-base-layout.rs` | `8970418724001c0b3955c86cf95cb24d9fe040331dfbc27a5dc56c83a9e153f1` |
| actual applied/live `crates/dockspace/src/layout.rs` | `4c06a8e7b3584926c681fa931e42da4f99cc9448fa3b2548178b161f76e66a71` |
| `applied-source.json` | `3040c7ea1e30e80c16c200cbbbc9da9789af14efc84bc6a21f35fa58cdf4eb14` |
| original `receipt.md` | `70883af2f40d0343fd6256568144c78d6e99c638b96c7d0fd24fc6a122a1dc27` |
| unchanged frozen `layout/reference.rs` | `93afdf6e8ad79d81f2efdafc4814a4bb6076fd9f535129f82ce33f9ee12ddf4e` |

Every hunk matches exactly on the original base and reproduces the saved original candidate. On the actual leaf-reader base, each hunk also has one exact context match: the final four hunks shift by 18 lines, with no content adjustment. Replaying these hunks reproduces the current live file byte for byte and matches `applied-source.json`, whose base commit is `5ea7fbb2b10fe1bbac851da1d179d1b78df7a4c0`. The patch changes one file, **+54/−11**, including one added test. The difference between the two candidate hashes is the pre-existing leaf-reader work, not an unexplained solver edit.

## Reused weight total preserves the original arithmetic

The only producers of the new private weight_total argument are the owned solve_axis and caller-owned solve_axis_into wrappers. Both obtain it from the same validate_inputs call that previously computed and discarded that scalar. Its existing checked_sum consumes `weights.iter().copied()` in the original order, with the same finite and zero-total checks.

The deleted later noncentral checked_sum consumed the identical immutable slice in the identical order. There is no writer or callback between validation and desired-size allocation. Reusing its successful finite result preserves the denominator and every `extent * (weight / weight_total)` operation; it does not reassociate a sum, infer normalization or substitute a different multiplication/division formula.

For a central child, the implementation deliberately retains the original sibling-only checked sum, threshold at 1.0, scale calculation, ordered desired-size sum and residual assignment. The central child's own weight remains excluded from allocation while still participating in the original total-weight validation. In particular, a total-weight overflow is still an error even if the central branch could otherwise ignore the central scalar. Zero weights with a central child remain allowed. The new full weight_total is not substituted for the sibling total.

The weight slice, constraint construction and fresh leaf/weight input materialization remain unchanged. This is local reuse of one already-required scalar during the same solve, without persistent state or another preparation layer.

## Assigned-size sum is omitted only when its input did not change

The initial clamped sizes and their checked sum are unchanged. If assigned is below extent, the same grow function runs with the same remainder and is followed by the original post-mutation checked sum. If assigned exceeds extent, the same holds for shrink and its excess. Eligible-set iteration, bounded deltas, residue consumption and their order are unchanged.

If neither branch runs, the first checked sum and validated extent are finite, so the comparisons establish numerical equality, including the existing signed-zero behavior. Sizes have not been written after that first sum. The old second sum would therefore read the same ordered size values and return the same scalar bits. Retaining the first scalar preserves overflow/unallocated results. It does not replace a residual sum after mutation.

The allocation tolerance, all residual comparisons/subtractions, f32 weight conversion and projection coordinates are unchanged. Subnormal finite inputs, signed zero and rounding-sensitive weights follow the same operations; this slice introduces no new approximate comparison or tolerance.

## Error and mutation ordering

The original validation order remains: finite extent, negative extent, cardinality, central-index bounds, each scalar weight in order, finite total-weight aggregate, then the noncentral nonempty zero-total check. Owned solve still validates at the same point before allocating its output sizes. Caller-owned solve still validates before writing its supplied slice.

Empty weights still cross validation before their early return. For nonempty input, the minimum aggregate and then maximum aggregate are both checked before either infeasible-minimum or infeasible-maximum return. No overflow check moved behind an early success. The deleted repeated weight sum could not introduce another error after the same immutable slice had already passed validation. The omitted no-mutation assigned sum likewise cannot fail after the first identical finite sum succeeded.

Central sibling-weight and desired-size failures retain their original position. Post-grow/post-shrink assigned-size overflow remains checked at the original point. No fallible step moved across a mutating redistribution or a caller-visible result.

The new solver_aggregate_errors_precede_infeasible_minima test is registered in layout.rs's existing test module. With extent 0 and two `[min=1, max=f64::MAX]` constraints, it verifies both central and noncentral cases: overflowing weights produce the weight-aggregate error; finite weights produce the maximum-aggregate error before the otherwise infeasible-minimum return. It checks the owned solver, independent frozen solver and caller-owned solver, including unchanged sentinel sizes on error. This is a useful boundary regression, while the existing property/oracle tests provide the successful numerical coverage.

## Frozen oracle and available root validation

The independent frozen solver still recomputes both deleted sums. Its bytes match the recorded frozen hash above; the comparison was not changed to share the new production data flow. The existing caller_owned_axis_matches_frozen_solver property compares complete results and each successful size/overflow/unallocated value's bits. The existing dense_layout_matches_frozen_traversal checks complete rich projection values through the actual caller-owned solve path. Existing old-output/deep-tree tests remain in the ordinary suite.

I read root's completed `solver-aggregate-verified/checks-fmt,targeted,workspace,clippy.json` and its logs. The manifest records normal counters enabled, 320 input hashes, unchanged sources, and the same applied layout/frozen-oracle hashes reviewed here. It records successful serial fmt, targeted, full workspace and Clippy correctness/suspicious checks. The logs report **77 targeted passed** and **2225 workspace passed, 3 skipped**; targeted output explicitly includes the new aggregate-error test and both numerical oracle properties. These are root's executions, not tests run by this reviewer.

That check manifest's SHA-256 is `717fa0621b0993ed3607ebe831c8c6fd0964d02bf46697e0990f96571fd9992b`. No performance conclusion follows from this review or those checks. The complete fixed-workload comparison remains root's responsibility, with existing timer scopes, fresh inputs, complete outputs and owned lifetimes intact.
