Numeric solver contract and validated-total candidate review

No source blocker in candidate 1. Returning the existing checked weight total from `validate_inputs`, passing that scalar through the two checked callers, and reusing it only in the noncentral desired-size branch is the smallest concrete deletion supported by this source. It removes one repeated reduction on feasible noncentral solves of any arity, including binary. It does not introduce a second solver or omit validation. This approval is source-only: no qualification or speedup is inferred.

I read neither design_minimal's new report nor another opinion. I ran no build, test, benchmark, sampler, timing command, archived runner or network request and changed only this report.

The exact reviewed package is `B/validated-weight-total-source/`, where `B=/tmp/dockspace-imgui-twofold-20261107-ea4161e`. Its `runtime.patch` SHA256 is `c4ea86a96b794d8ede5445b8723932154ced2010bc91bec206a5934a011bb422`; scope SHA256 is `69016fa40240c8bc1a0c1fb4d713ed1488fcab15a34aabab7ced8f389d6e9e18`. Both complete base files equal immutable `a46fddae12be5a81ac19144f55ab34066a745c5c`, whose runtime is `0e441c8`. Candidate hashes and current files match the package:

| Source | Base SHA256 | Candidate SHA256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` | `c6c2242176fca48d7b604c7ab5e023c57093508081d6c7c1d4535bc27ca1dc52` |
| `crates/dockspace/src/layout/topology_tests.rs` | `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2` | `859bce05e75e7733f34db07744b5d064dbc89256b7e300350cebcc9a79be459a` |

The frozen numeric reference remains byte-identical, SHA256 `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6`. All line references below use the immutable base unless identified as candidate lines.

The validation and error order must remain exact.

Both `solve_axis` and private `solve_axis_into` first call the same validator (`layout.rs:313–353`). Its order is:

1. Reject nonfinite extent.
2. Reject negative extent; negative zero is accepted.
3. Reject weight/constraint length mismatch.
4. Reject an out-of-bounds central index.
5. Visit weights in input order, rejecting each nonfinite value before testing that same value for negativity.
6. Perform the original checked full weight reduction, with quantity `"weight"`.
7. Reject a nonempty zero/nonpositive total only when there is no central child.

The executor next handles the empty axis, checks `"minimum"` and then `"maximum"`, and only afterward returns an infeasible-minimum or infeasible-maximum outcome (`355–397`). In particular, checking minimum infeasibility does not permit skipping the maximum aggregate. Desired sizing, clamping, the checked `"assigned size"` reduction, correction, and outcome construction follow in their existing order. Constraint scalar validation belongs to `AxisConstraint::new` (`41–55`); the solver must not invent a new validation phase for privately fabricated invalid AxisConstraint fields.

Examples that distinguish real contracts from tempting shortcuts:

- Length mismatch precedes invalid central index and bad weights. With valid lengths, invalid central precedes bad weights.
- With no central, `[-1, NaN]` rejects negative index 0; `[NaN, -1]` rejects nonfinite index 0. A whole-slice “has NaN” scan would change the first error.
- `[MAX, MAX]` rejects the full `"weight"` aggregate even with a central child whose own weight will later be ignored by desired sizing.
- Extent 1, weights `[1,1]`, and valid constraints `[(1,MAX),(1,MAX)]` reject the `"maximum"` aggregate. Returning minima immediately because their sum exceeds extent would change that error.
- `AxisConstraint::new` rejects nonfinite bounds, a negative minimum and maximum below minimum at construction. Relaxing these inputs for an allegedly unconstrained binary path changes the accepted domain.

Candidate 1 preserves all these checks and positions.

The scalar-reuse proof is narrow and complete.

The original noncentral branch at `1339–1342` repeats the same `checked_sum(weights.iter().copied(), "weight")` already completed at `1300`. Between these evaluations there is no callback, weight mutation, alternative input, or mutable alias to the weight slice. The intervening work checks immutable constraints and initializes/writes the separate sizes slice. Safe Rust's ordinary `&[f64]` and `&mut [f64]` boundary does not permit those writes to mutate the weights.

Consequently, whenever the second reduction is reached, it has the same ordered f64 inputs and the same reduction operation as the successful first reduction. It cannot newly produce an error. The candidate forwards the exact returned f64 value, rather than reconstructing it from the child count, subtracting another weight, normalizing it, or changing the reduction expression.

This covers negative zero, subnormals, very unequal weights and finite aggregate extremes. There is no arithmetic identity such as replacing two independently rounded desired sizes with a complement. The original expression remains `extent * (weight / weight_total)` for every child, with the same clamp and subsequent sums. The successful output bits therefore retain the same argument values and operation sequence. The validator still supplies the same offending input value and index on an error; no NaN payload or sign is synthesized by this change.

The full checked sum also stays before minimum/maximum aggregates. The empty-axis return still passes through the original validator and returns the supplied extent as unallocated. No nonempty-total rule is newly applied to an empty axis.

For central inputs, the forwarded total is deliberately unused by desired sizing. Candidate `desired_sizes_into` retains the original filtered sibling reduction, the `sibling_total > 1` decision, scale calculation, child-order products, checked `"desired size"` reduction, and central remainder (`1324–1348`). “Central weight ignored” concerns allocation, not admission: its scalar and contribution to the full validation aggregate must still be checked. Computing sibling total as `full_total - central_weight` is invalid. For example, with weights `[MAX,1]` and central 0, the full rounded total is MAX; subtracting MAX loses the sibling weight 1, whereas the original filtered sum is 1.

There are only two production callers of `solve_validated_axis_into`, and both now pass the result from their own immediately preceding `validate_inputs` call. No direct unchecked caller or detached scalar source was introduced. The generic correction, central preference, stable residue order and exact-assigned-total reuse are unchanged.

A binary formula needs stricter proof than mathematical equivalence.

The smallest safe boundary for a later no-correction guard is after the original input checks, both aggregate checks, desired sizing, original clamp and checked assigned reduction. Exact `assigned == extent` proves that neither correction branch runs. At that point the existing tail must produce positive-zero overflow and unallocated: extent is finite/nonnegative, aggregate totals are finite, and the allocation tolerance is nonnegative. This sufficiency is not binary-specific; the current executor already exploits equality to avoid the second assigned sum.

A future fast path at an earlier point must establish the same facts using the same rounded operations and rejoin the one generic executor on guard failure. It must not duplicate full validation, central sizing, water filling or output construction. Merely adding a binary “inactive constraints” guard can add comparisons and duplicate work on failure while the compiler already knows the small slice lengths. The present source provides no performance evidence for that tradeoff, so this review does not request an additional guard candidate before scalar reuse is measured.

Several proposed shortcuts would violate the bitwise contract:

- Extent 1, weights `[1,2]`, constraints `[(0,1),(0,1)]`, no central: the original outputs independently rounded `1/3` and `2/3`, whose checked sum rounds to 1. Replacing the second value by `1 - first` changes its bits by one ULP. No correction was required to justify this example.
- Let `t` be the smallest positive subnormal, extent `2^1000`, weights `[t,2]`, and both constraints `(0,2^1000)`. Their maximum aggregate `2^1001` is finite. Original `extent * (t / 2)` is zero because the division underflows first; `(extent * t) / 2` is `2^-75`. The large second size makes the assigned sum round to extent in both cases, so final correction does not erase this difference.
- “Weights are equal” does not authorize `extent/2` before full weight validation: equal MAX weights fail `"weight"` aggregation. Known normalized durable weights do not justify changing public/raw/override admission in this numeric slice.
- “Assigned is close to extent” is not the existing no-correction condition. Noncentral redistribution uses a tolerance derived from the residual amount, while final reporting uses extent/minimum/maximum. Central grow/shrink can apply a bounded delta even before redistribution tests its small residual. A threshold-based early return can therefore change bits or central behavior.
- Zero comparisons do not authorize canonicalizing signed-zero sizes or replacing the original `clamp`, `min` or `max` operations. Direct bounds/capacity reconstruction must preserve their exact behavior.

ImGui's pixel rounding, binary live-node update, or a mathematical “sum equals extent” theorem is not an oracle for any of these cases.

The added test is useful, with explicit limits.

`binary_axis_scalar_boundaries_match_frozen_solver` adds one semantic oracle test, not a mirror of the implementation. Its 11 extents × 11 weight pairs × 6 constraint pairs × 4 central choices cover 2,904 combinations. Successful sizes, overflow and unallocated are compared by `to_bits`; failing cases compare the frozen and actual Debug result, which distinguishes the relevant error variant/index/quantity even when NaN defeats ordinary equality. That failure comparison does not separately prove NaN payload-bit equality; preservation of the original error-producing code supplies that source argument.

The table includes signed zero, minimum subnormal, minimum normal, MAX, invalid extent/weights, central 0/1/invalid 2, zero weights, and aggregate failures. Existing `caller_owned_axis_matches_frozen_solver` (`topology_tests.rs:54–87` in the base) independently checks success bits and errors across arities 0–15 and ordinary clamps/central cases. The full layout oracle remains separate. Existing `tests/layout_solver.rs` also records the structural mismatch and constructor contracts; it should not be counted as a new executed check by this source review.

For any subsequent arithmetic/guard treatment, the most valuable small additions are the two explicit rounding counterexamples above, plus the finite-minimum/infinite-maximum aggregate-priority example. They can be table rows in the existing independent numeric oracle rather than a new framework. Candidate 1 does not require tests asserting a private scalar parameter or a count of sum calls.

The performance risk is real and bounded.

This removes a repeated checked reduction only when the old noncentral desired-size branch ran. Central, empty, and infeasible-constraint paths did not execute that second reduction. No owner, output buffer, or allocation is removed. The extra f64 argument and its longer live range can change register pressure, call lowering, inlining and generated code; central and early-out paths can incur that code-generation cost without deletion of executed work. Nothing here predicts a percentage gain or proves the former reduction was a hotspot.

The predeclared full-matrix/negative-tail rule in the package is an appropriate way for Root to decide retention on the actual `3a885b...` control. Existing independent bitwise/layout/error/raw/deep/staged/MRU/old-output qualification remains necessary, with the frozen reference unchanged. Source equivalence permits that experiment; only its executed results can decide benefit.
