# Independent review: local binary split inputs

No source blocker was found in the one-file candidate. The change preserves the original solver, floating-point operations, error order and complete projection writes while selecting local input arrays for actual binary shapes. This is a source review, not successful Rust typechecking or an executed behavior/performance result. Root owns verification. Only this review file was written; the native fix package and the binary candidate source, patch and manifests remain stationary.

The review is bound to these exact artifacts:

| Artifact | SHA-256 |
| --- | --- |
| `base/crates/dockspace/src/layout.rs` | `5b0333ecec21e55215d9ec641acfe2dca6f6270e560e3184ba6f4810f66de6ab` |
| `tree/crates/dockspace/src/layout.rs` | `0581670f57affb6cbe36ad946743ce7e620f1a6a80da1bcbf16eb99ba7a58911` |
| `binary-split-inputs.patch` | `8b59150dea86587f64bac80f6fe222a14959f0367ce0a2dfcc7ecc82dcbb183a` |

The base still equals the captured `/tmp/projection-frame-current/tree/` prerequisite. Independent byte comparisons confirmed that the complete prefix and suffix outside `prepare_split_projection` are unchanged, the child-to-axis mapper body is unchanged, and the complete central/geometry/solver/outcome/state tail differs only in the selected constraints slice operand. The function retains exactly one `solve_axis_into` call. Manifest, READY and all eight copied contract hashes match. A read-only `git apply --check --whitespace=error` against this candidate's `base/` returned zero with no output; nothing was applied. These checks do not establish applicability to an independently advanced repository file.

The branch boundaries at candidate layout.rs:983-1036 are sound:

| Selected input | Constraint storage | Weight storage and preserved boundary |
| --- | --- | --- |
| Two children, two graph weights, no override | Exactly two local constraints, in child order | Exactly two local f64 conversions, in graph order |
| Two children, graph weights of another length | Exactly two local constraints | Full graph-weight Vec refill; the mismatch is not truncated or padded |
| Any other child count, no override | Full constraint Vec refill | Full graph-weight Vec refill, including a two-weight slice paired with a nonbinary child count |
| Present override at any arity | Selected solely by actual child count | The entire original override f64 slice; graph weights are not selected |
| Binary/nonbinary and override transitions | A selected fallback always clears before filling | Old scratch contents and capacities never determine the selected slice |

Zero- and one-child slices retain the original fallback behavior. The raw draft compiler also still rejects child/weight cardinality differences before projection (`contracts/crates/dockspace/src/layout/topology.rs:61-70`). Even if a malformed shape reaches this private function, the new match preserves the actual lengths for the original validator. No equal-cardinality invariant is used to index a weight slice.

Floating-point construction is preserved. The pure constraint mapper reads the same copied measurement and the same axis-specific min/max fields. Binary array operands evaluate the first child before the second, matching the previous iterator order. Graph weight operands use the exact previous `f64::from(weight.get())` expressions in order. Overrides remain the previously validated full f64 values; no narrowing, rescaling, normalization, new arithmetic or reordered sum is introduced. The min/max slab loop still writes every child before weight selection. Both assigned-size reductions, water filling, central handling, saturation and tolerance remain in the unchanged solver.

Error priority is preserved. Public override/source validation and leaf-measurement entry paths are byte-identical, including their intentionally different precedence. Inside preparation, the central-child scan and projected splitter geometry still precede the same solver call (layout.rs:1037-1053). `validate_inputs` still checks nonfinite extent, negative extent, cardinality, central index, each weight's finiteness/sign, then the ordered weight total (layout.rs:1166-1207). The subsequent ordered minimum/maximum sums and later geometry/continuation errors are unchanged. An override still wins before any graph-weight materialization. The array branches introduce no additional fallible operation or returned error.

The borrow and initialization boundaries at layout.rs:996-1053 have no apparent source issue. Each conditionally initialized array is borrowed only by the branch that first assigns its complete value. The fallback and override paths never read an uninitialized array. `axis_constraints` can borrow `scratch.constraints` while the later code accesses only the disjoint `weights` and `eligible` fields. Both local arrays outlive the unique solver call. `SplitProjectionState` stores only its original scalar progress fields, so neither input slice escapes or accumulates along the explicit traversal. The slab borrow ends before outcome publication, as before. Rust compilation remains necessary to confirm the exact compiler's acceptance; no compiler or formatter was run for this review.

The cheapest meaningful root-owned qualification reuses registered existing tests, without adding a storage-form test framework:

- `layout::topology_tests::dense_layout_matches_frozen_traversal` exercises full and minimum projections of generated 2-4 child trees against the unchanged reference, including mixed scratch transitions, central nodes and constrained bounds.
- `layout::tests::measured_minimums_preserve_projection_constraints_and_resize_overrides` explicitly combines a three-child outer split and binary inner split, both central modes, and overridden/unoverridden input.
- `layout::override_tests::batch_validation_preserves_root_duplicate_and_weight_error_priority`, `layout::topology_tests::constraint_reader_preserves_depth_first_errors_and_override_precedence` and `layout::topology_tests::minimum_measurement_errors_precede_override_validation_for_cold_and_warm_topology` retain the relevant precedence witnesses.
- `layout::topology_tests::caller_owned_axis_matches_frozen_solver` checks unchanged solver scalar bits but does not itself reach the new materialization branches. Deep traversal, late geometry, raw malformed topology and owned previous-output witnesses remain available for the broader gate.

No test was added or changed. The local arrays avoid acquisition of the two fallback buffers only where a root never needs those buffers. Scratch was already reused per root, so this is not two allocations saved per split. Mixed arity, fully overridden weights, branch/code-size costs and actual stack lowering limit any source-only performance inference. No allocator sample, latency or speedup is claimed. Cargo, compilation, tests, binaries, timing and sub-agents were not used.
