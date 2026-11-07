Independent actual-parent review: binary noncentral desired-size initialization

No source blocker found in the supplied candidate. It is an independent treatment on `a46fddae12be5a81ac19144f55ab34066a745c5c`, with runtime `0e441c8` and formal control executable `3a885b09604d6552acb307d07e87d0ea40247b99866fcfcb8c37bb7d3bb8819b`. It does not retain the rejected validated-total scalar argument or reuse mechanism. The bounded explicit initialization is suitable for Root's ordinary qualification and complete paired measurements; this review establishes neither executed correctness nor a performance benefit.

I reviewed only actual source pairs, scope and the unchanged reference. I did not read design_minimal's new report, execute a build/test/runner/profile/timing command, access the network, or modify repository/candidate files. Only this report was written.

Exact binding

The package is `B/binary-desired-actual-source/`, where `B=/tmp/dockspace-imgui-twofold-20261107-ea4161e`. Its final `scope.json` SHA256 is `07c71b09e5830f9e547392fa38ecb68c0cc4697214e96c922ac08306a15a69fa`; its formatted `runtime.patch` SHA256 is `ead58ce0f0824e24032e669c8633ab17da14851319d79295fe1b36bb85cbddd2`. Both actual-base files equal the immutable `a46fdda` Git objects byte for byte; all four source hashes match scope:

| Source | Actual-base SHA256 | Candidate SHA256 |
| --- | --- | --- |
| `crates/dockspace/src/layout.rs` | `01f21abad6c4d71856dbf17ae9eec90a97d7fbe7717bb2e6da87895f2fed5dcd` | `fd5402e5e2f2a5c636450d63161ec92a6dbad52ee70bd104d8a413b1c106a46c` |
| `crates/dockspace/src/layout/topology_tests.rs` | `67671850120413fbaec896b7eea930b7eb52086ccf4864b8486ab21ed8ea1ad2` | `859bce05e75e7733f34db07744b5d064dbc89256b7e300350cebcc9a79be459a` |

The complete runtime comparison has one changed hunk, replacing the original desired-size call and clamp loop at base `layout.rs:399–402`. The test comparison adds one 70-line oracle test. I also checked the updated actual source after Root's formatting: its only difference from the earlier reviewed candidate `512f8724...` is multiline tuple-pattern formatting and a trailing comma; no expression or branch changed. The live reference equals immutable `a46fdda`, SHA256 `4b6ff84c9bed4b65a202e1c771b5f176b161bbb27b99dba25e4875c7b08c07c6`.

The actual guard and operation sequence preserve the contract.

Candidate `layout.rs:399–418` matches the tuple `(central_index, weights, constraints, &mut *sizes)`. Its special arm requires exactly `None`, two weights, two constraints and two mutable output slots. It performs the original `checked_sum(weights.iter().copied(), "weight")`, then both original `extent * (weight / weight_total)` expressions, then the first and second original clamp operations in input order.

Both desired values are computed before either clamp, as in the old helper. Their temporary residence changes from the private sizes buffer to two local f64 values. There is no callback, observable intermediate output, or mutable alias through the immutable weights/constraints. Each final clamped output receives the same expression result and the same bounds. Division/multiplication order, checked reduction expression, child ordering and signed-zero operations are unchanged. No complement formula, normalization shortcut, tolerance guard, or floating-point reassociation is introduced.

The extra source occurrence of the checked sum is in mutually exclusive branches. On each feasible noncentral solve, the validator still performs its original reduction, followed later by exactly one original desired-stage reduction. This treatment does not claim to remove either executed sum. The original generic desired helper and clamp loop remain unchanged in the fallback; binary central and every other arity take that fallback.

Validation, aggregate errors and correction remain at their original stages.

The public and private checked wrappers are unchanged. In particular, `validate_inputs` still returns `Result<(), LayoutError>`, and `solve_validated_axis_into` keeps its original parameters. The original extent/length/central/indexed-weight/full-weight-total/zero-total checks all precede the executor. The empty-axis path, checked minimum reduction, checked maximum reduction, and both infeasible-bound returns all precede the new match.

Therefore:

- Nonfinite extent still wins before length, central or weight errors; negative extent follows it. Length mismatch precedes central bounds; central bounds precede indexed scalar weight errors.
- Each weight's nonfinite check precedes its negativity check, in child order. Negative zero is still accepted, and subnormal inputs are not canonicalized.
- Full weight aggregate overflow still rejects `[MAX, MAX]`, including central inputs, before constraint aggregation. A central weight remains ignored only for desired allocation, not admission.
- Even when finite minima exceed extent, maximum aggregate overflow still wins before the infeasible-minimum outcome; the match cannot bypass either aggregate.
- Constructor rejection of invalid `AxisConstraint` min/max remains unchanged. The patch adds no new checks or error order for privately fabricated invalid constraints.

After either match arm, the exact shared checked assigned-size reduction, grow/shrink functions, stable residue consumption, exact-assigned-total reuse, final tolerance and overflow/unallocated construction remain byte-identical to base. Thus an initially rounded sum below/above extent still receives the original correction. Central sibling-weight/desired-size aggregation, central-first bounded adjustment and central error behavior are untouched.

No layout traversal, gap timing, parent-edge result ownership, binary/nary scratch selection, leaf read, old-output lifetime, command authority or staged candidate behavior changes.

The existing test addition is a useful independent witness.

The candidate's `binary_axis_scalar_boundaries_match_frozen_solver` is byte-identical to the previous 2,904-combination witness: 11 extents × 11 weight pairs × six constraint pairs × four central choices. Its successful size/outcome comparisons use `to_bits`; invalid cases compare Debug results, preserving variant/index/quantity comparison even for NaN. This does not itself compare error NaN payload bits; the unchanged validation expression supplies that source argument.

The witness covers zero signs, smallest subnormal, minimum normal, MAX, invalid extent/weights, both central positions and invalid central 2, and aggregate failures. The unchanged general-arity bitwise numeric oracle and full-layout oracle remain independent. Reusing this test on the restored parent retains no candidate-1 runtime mechanism. No implementation-mirroring test of the match arm or local variable layout is needed.

Cost and experiment boundary

The change exposes two fixed desired/clamp values while retaining one generic solver, one correction path and the original required arithmetic. It can help code generation, but adds an arity/central match and a small duplicated initialization block. The compiler may already specialize the old loops, and branch placement, code size, register pressure or fallback dispatch can regress central/nary and whole-operation results.

The supplied rule is explicitly against actual `a46fdda`: at least 28/42 complete median improvements, at least 30/42 ImGui threshold cells with all medium/large cells preserved, at least four of the six preselected single-view small layout medians improving, and publication of all tails/negatives. Apply it to this independent source, without importing candidate 1's measured outcome or choosing retention solely because threshold inventory increases. Source review permits this bounded experiment; only the recorded ordinary/work/full-vector checks and paired results can justify retention.
