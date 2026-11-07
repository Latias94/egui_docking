# Committed axis input source review

## Result and scope

No source-review blocker was found in the exact candidate identified below. The omitted checks are redundant for the current closed committed writers; raw draft projection and each split with an actual selected override retain the complete validator. Extent checks remain at the original solver boundary. The numerical executor and traversal order are unchanged.

This is an independent, read-only source review of the candidate, not execution or qualification. No Cargo, compiler, tests, timing, runner, or archived script was run. Only this review file was written for this candidate. Repository, fork, cache, candidate sources, and other candidate artifacts were not modified.

## Exact source binding and parent composition

Patch SHA-256: `20d377d1f3d86f936b1aa3d4a085bfe181cab52c78c93b5906c3c17726d69159` (12,944 bytes; two paths; runtime +43/-23 and tests +201/-0).

| Source relative to `crates/dockspace/src/` | Base SHA-256 | Candidate SHA-256 |
| --- | --- | --- |
| `layout.rs` | `9ba215565d486e56d7c5435e123691948fece5a25ff5479bb245f793c4ab989f` | `2189cdcdd4d4f76ee1625c8e2109715ee270ac5c5c9b427d69020db6bd030585` |
| `layout/topology_tests.rs` | `5863123fd6b1733f1ad1e0bfeb0d9f67b6c18e9b20703b9705079e3ae90cf5db` | `9c951debe0eebabbf48d8ef87ef8d82b3e433e7e9242132a94311558077850ff` |

- `manifest.json`: `f2daaff1c6dc9c4517b5961e63cf313f777bc532710f6ea0a005afd291473914`.
- `base-manifest.json`: `833367a406999f189d2f93eb2e71d48d3dae8c6c367a33f65c06729a59b64199`.
- Base path-set digest: `41135ab44aca6d8876cde6f80d38e8e50fc9506ead0b53aa758d7e2aab0d8449`.
- Candidate path-set digest: `d350409347a7fd9a7a4065b48a8ba9f988986f40eb612f056dbb8a840825c685`.

I recomputed both complete file pairs, byte lengths, path-set digests, artifact hashes, and READY-to-manifest binding. All match. An independent in-memory application of all 13 patch hunks reconstructs the exact two tree files, with the recorded line counts.

The capture head `16e4f9440ad225088f4b1eab2d2677c741705e31` is not by itself the complete parent: it lacks the three child-row changes. Applying the original child-row patch (`3f826644008677c59c44917c4f38dda5542ccd3ccdca286d379c572a3a34322e`) in memory to that immutable head reconstructs the recorded parent `layout.rs`, `layout/reference.rs` (`ea4afaf3fc82be4e3f27f8c00aa558d5aed0ff37ff15a184d3db3dc49f7e7b27`), and `layout/topology.rs` (`89e94dc277cbfe3d4734ddd16f25fffee4c735b5144f75fd3e4b119006ece9c9`). The other 12 recorded source-context hashes match the immutable head. All 15 also matched the repository when read during this review. `layout/projection.rs` remains the retained three-vector source (`d86c008dd78c5f1ed805de76b71d102a29dd3dab4e6ee37ca8f3cb74839edfb0`). This binds a retained-control-plus-child-rows parent, without the rejected slab or a later frame, binary-buffer, or unified-result overlay.

## Why the committed boundary is closed

The false route requires a ready `RootStructure`, not an arbitrary `Node` value. `Workspace` exposes immutable node access; editable public input remains in `WorkspaceBuilder`. `root_structure` returns no body during an Editing state. `SharedSplit` owns the exact immutable arena payload, and the child range is built from that payload's child roster. Copy-on-write storage preserves old bodies. A delayed `RootMeasurement` additionally borrows its workspace, so its retained rows cannot be combined with a concurrently mutated workspace through this API.

The relevant weight producers retain the necessary guarantees:

| Producer | Guarantee in the reviewed parent |
| --- | --- |
| Public scalar and split construction (`graph.rs:53`, `:254`) | `SplitWeight` has no public scalar field or deserializer bypass. `new` rejects non-finite and nonpositive values; normalization also checks represented results. Arbitrary public `Node::Split` construction still enters a Builder. |
| Builder, canonicalization, and snapshot import (`graph.rs:725`; `canonical.rs:223`, `:436`, `:689`; `persistence.rs:458`, `:622`, `:773`) | Build validates the input collection before rewriting and strictly validates the output before issuing a Workspace. Import checks scalar data, collection invariants, and the final Builder result. |
| Edge insertion and wrapper birth (`operation.rs:3366`, `:3400`) | Both newly computed shares pass `SplitWeight::new`; child and weight rosters change together. Underflow or an invalid remainder rejects the operation. |
| Detach and flatten (`operation.rs:3148`; `canonical.rs:576`) | Remaining shares are normalized, or the singleton receives a checked unit share. Flattened products are normalized with checked represented results. |
| Checked Item completion (`canonical.rs:231`) | The inserted split and every final live rewritten split retain scalar/cardinality checks. Unchanged branches inherit committed values. |
| Resize (`command.rs:204`; `operation.rs:1940`, `:1992`) | Public updates carry checked `SplitWeight` values; execution validates current source, duplicate split, cardinality, and normalization before copying. The root is marked Editing before writes and rebuilt afterward. General publication still runs canonical validation. |

These concrete boundaries justify equal constraint/weight lengths, finite positive committed weights, and a finite positive total. The central index is obtained by `position` over the same children, so it is in range. Converting checked f32 shares to f64 does not introduce non-finite values. No production writer found in this scope fabricates unchecked scalar tuple values. Internal malformed fixtures remain a different input route; the candidate does not grant them committed status.

This relies on the present writer contract. `RootStructure::build` itself is not a second scalar validator, and its cardinality assertion is debug-only. A future private writer that publishes unchecked weights would violate this boundary and must be reviewed there. This candidate adds no persistent trust flag, cache, proof object, public API, or alternate executor.

## Routing, arithmetic, and first errors

The complete committed entry calls `project_measurements::<false>` at candidate `layout.rs:511`. Both delayed project entry variants reach the same false route through `project_with_weights` at `:583`. The sole raw draft entry, behind `cfg(test)`, uses `<true>` at `:651`; the constant propagates through the two private traversal layers without another selector. At `:1024`, the full validator is selected by `CHECK_AXIS_INPUTS || override_weights.is_some()`. The actual result of the existing per-node map lookup is reused. An override on another split neither skips checks for the selected override nor changes this split's route.

Override source/root/freshness/duplicate/type/cardinality/normalization checks are byte-identical and retain their original order. Full projection validates overrides before reading leaves; delayed measurement still precedes its override validation. Importantly, the override collection check alone is not a reason to skip `validate_inputs`: internal `[-1, 2]` sums to one, while `[0, 1]` retains its existing accepted solver semantics. Both selected-override cases still enter the full scalar validator.

`LogicalRect::from_min_max` validates finite ordered corners, but subtracting opposite extreme corners can produce an infinite width or height. The candidate correctly keeps `projected_splitter_geometry`, then the same subtraction, then non-finite/negative extent checks at the former axis-solver call site. It introduces no early root-wide numeric check. In the full route, the remainder of `validate_inputs` is byte-identical after replacing the original extent checks with `validate_extent(extent)?`. The extracted extent helper contains exactly those former checks in the same order.

I independently compared all 19 numerical/boundary function bodies named in `scope-checks.json`; they are byte-identical. This includes the public arbitrary-f64 `solve_axis`, validated executor, override validator, measurement composition, splitter geometry, continuation, size calculation, redistribution, aggregates, and tolerance. Projection frames and scratch representation are unchanged. Raw missing-node/cardinality/cycle/completed-DAG behavior and actual Enter timing are preserved. A raw numeric weight failure still occurs at its original Enter after measurement, rather than during compilation. Descendant work and pending parent gaps therefore retain their previous error precedence. Owned output storage, old-output isolation, and export are untouched.

## Test assessment

Both additions are meaningful behavioral witnesses using existing APIs and the independent reference traversal; they do not mirror the new constant or assert which helper was called. Removing the single new `LogicalPoint` import leaves the entire old test file as an exact byte prefix. No old assertion or reference implementation changed.

`committed_axis_inputs_keep_infinite_extent_and_delayed_error_priority` covers both axes with finite corners whose derived extent is infinite. It checks the exact `NonFiniteExtent` result after both leaf reads, the reference outcome, and the distinct delayed bounds-completion Geometry error. It also preserves invalid-override-before-reader and missing-minimum-before-delayed-override order. Its internal negative and zero override cases check the required scalar rejection/acceptance boundary and compare complete results to the reference, so the zero case is not supported only by `is_ok`.

`raw_axis_inputs_keep_earlier_geometry_before_later_bad_weights` keeps measurements finite while arranging an infeasible outer split. With the ordinary child first, the later child's x endpoint overflows before that child's split is entered, so Geometry must win over its NaN weight. Swapping the children makes the NaN split enter first, so `NonFiniteWeight` must win. Both actual and reference results are checked separately; the NaN comparison uses `is_nan`. This would expose a raw route accidentally using the committed shortcut or an eager raw weight preflight that changed first-error order.

The additions are two bounded witnesses, not a new framework or a replacement for the existing generated solver/layout oracle, override, writer/import, recycled-identity, and owned-output checks. Existing registered examples include `caller_owned_axis_matches_frozen_solver`, `dense_layout_matches_frozen_traversal`, `batch_validation_preserves_root_duplicate_and_weight_error_priority`, and `owned_node_rows_keep_previous_outputs_after_late_geometry_error`. No execution result is claimed here.

## Optional simplification and cost limits

Root's proposed simplification is recommended as a small follow-up variant, not a blocker in these bytes. A repository-wide source search finds only one caller of private `solve_axis_into`: this preparation site. At the same location, choosing `validate_inputs(...)` for raw/selected-override inputs and `validate_extent(...)` otherwise, then making one shared `solve_validated_axis_into(...)` call, preserves validation order and the executor arguments. The now-unused wrapper can then be deleted. The public `solve_axis` already validates before directly calling the executor. Preserving a byte-identical wrapper solely because it appears in the original 19-function receipt would not justify duplicated argument lists. Such a variant must receive its own exact patch binding; this review does not silently substitute it for the stationary candidate.

The present change removes a per-weight scalar scan and a validation-only sum on committed splits without a selected override. It retains f32-to-f64 weight gathering, constraints gathering, minimum/maximum and numerical weight sums, redistribution, geometry, traversal, allocations, and complete owned outputs. It adds no source-level stack or initialization work beyond the existing per-split selection, and the override map lookup count is unchanged. It does not change the asymptotic layout work. No speedup, allocator saving, binary-size result, or progress toward the macro target follows from this static review. Root remains the execution and adoption owner.
