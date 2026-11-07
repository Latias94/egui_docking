# Equal-binary desired sizing: source review

No source blocker found. The new guard is a valid exact specialization inside the existing desired-sizing helper. Qualification and complete-operation timing remain pending; source-level arithmetic removal does not establish a performance win.

Reviewed the live source against `post-main-continuation/equal-binary-source/base/` and its original predeclaration. Observed source bindings:

| File | Base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` | `69d76d26c66b61f7fa9a19068a01ed248522d155d701e6e43a90c4d4d52e1ef6` |
| `crates/dockspace/src/layout/topology_tests.rs` | `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2` | `41ffce25e6766bb49bff1ffc31be8cbe28f2cb97a370307a306498bf18222b57` |

Runtime delta: eight added lines after the central early return in `desired_sizes_into`. No existing arithmetic, fallback, public interface, output, or timer was otherwise changed.

The subsequently saved `candidate/` pair matches both reviewed live hashes exactly. `runtime-and-test.patch` SHA-256 is `aad30c7ad464608ebd73027b8715b53617726479924d77716ea5991bdf2a4896`.

## Exactness and error order

The only caller is the existing numeric executor (`layout.rs:399`). Both public and projection entry points still run `validate_inputs` first (`319,351`). That preserves extent, cardinality, central-index, ordered weight, aggregate-weight and zero-total errors. The executor still computes minimum then maximum aggregates and handles infeasible bounds before desired sizing (`371–397`).

With no central child, two equal weights that passed those checks must both be finite and strictly positive. Doubling such a binary floating-point value is exact when finite, including subnormal values; overflow is already rejected by the original weight-total check. Therefore each original ratio `w / sum(w,w)` is exactly representable as `0.5`. The new expression is the same multiplication, `extent * 0.5`; it introduces neither a reciprocal approximation nor a residual-derived second allocation. Signed-zero and subnormal extents retain the original multiplication result, including underflow rounding.

The removed desired-stage checked sum cannot introduce a new first error after the identical earlier validation succeeded: weights are immutable and no intervening callback changes them. Zero/signed-zero pairs, equal negative weights, infinity, NaN, and overflowing equal weights cannot reach the guard. Equality is exact, not a tolerance.

`sizes.fill` uses the executor's existing matching output-length invariant; public allocation and projection preparation still supply one output slot per weight. It only changes initialization. Clamping, assigned sums, grow/shrink, bounded redistribution, residue order, exact-total reuse, tolerance and outcomes remain the sole original implementation. Central and unequal/nonbinary inputs retain their existing paths. There is no second solver, stored trust state, or borrowed lifetime change.

## The 31 fixed oracle cases

Static counting agrees with `calls == 31`. Successful results compare every size and both outcomes by `to_bits` against the unchanged frozen solver. Explicit NaN-bearing errors compare payload bits rather than relying on NaN equality.

Coverage includes five equal-weight scales through the smallest/largest subnormal, minimum normal and largest finite safe doubling; signed-zero/tiny/odd-subnormal/maximum extents; grow and shrink after clamps; infeasible minima/maxima; an adjacent unequal-weight fallback; both central positions; and a nary fallback. Competing errors cover extent before length/index/weight, length before central/weight, central before weight, ordered negative/NaN checks, zero total before maximum overflow, equal-weight sum overflow, and minimum before maximum aggregate failure.

The fixed cases support the mathematical argument; they are not exhaustive floating-point testing. Existing random bitwise solver and full-layout/raw/error/ownership oracles remain necessary qualification. No additional mirrored test or framework is requested.

Only static diff, caller, proof and test-source inspection was performed. No other agent's witness plan was read. No source/candidate was changed, and no build, test, runner, timing or network operation was performed.
