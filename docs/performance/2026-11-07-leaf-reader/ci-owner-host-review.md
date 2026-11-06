# CI host resize-output regression: read-only diagnosis

Verdict: real correctness regression in A1's resize projection capture ordering. Not runner capacity/cancellation, a durable weight mutation, or an expected stricter Arc rejection. Repository remains untouched by this review; no build or test was run.

The retained log for job 128104107432 checked out ecf78bacbdffdfb51f4a57c3f95073a71898a826 on Ubuntu 26.04/Rust 1.95.0. Its default public-host nextest run stopped after 200 passes and this failure; 91 further tests were not run. `integration/dockspace-host-harness/tests/nested_splitter.rs:832` calls `report_presentation(pending, false)`, and line 277 observes `presents(artifact) == true`. Prior assertions already confirmed original durable weights, unchanged workspace revision and the completed no-op resize delivery.

## Exact cause

1. `scene_compiler/surface.rs:84` calls `ready.set_resize_projection(resize_overrides)` immediately after constructing the empty plan.
2. A1 changed that setter (`scene.rs:477`) to filter using `self.command_basis(root)`, since opaque command refs no longer contain presentation geometry. No command basis has been bound at this point. The main basis is bound around surface.rs:134; each contained basis is bound around :178.
3. Every override is filtered out. `compile_root` still receives the actual resize_overrides argument directly, so layout/splitter geometry is correctly resized. The emitted plan nevertheless carries an empty resize_projection inventory.
4. After the pointer returns to the origin and releases without a durable edit, the older resized output is reported late. `presentation_authority.rs:1928` consults `presented_transient_geometry_is_current`; the empty projection passes `.all(...)` vacuously (`presented_input.rs:50`). That output is retained instead of retired.

This also causes the measurement reuse gate's projection-length comparison (`engine/surface_contribution.rs:324`) to miss legitimate active-resize reuse. A1 exact ref equality is not the cause of this observed acceptance.

## Minimal fix

Move the one `set_resize_projection(resize_overrides)` call to after the complete main/contained root compilation loop, immediately before `ready.derive_splitter_operability()`. No new workspace parameter, fingerprint construction, issuance exception, duplicate owner capture or setter rewrite is needed.

The setter is infallible and the compiler does not read resize_projection while constructing root geometry; it continues to pass overrides as the original direct layout input. All production getter consumers are outside this compile phase. Every contained basis is installed before its clipping/early-continue check, so offscreen root ownership filtering remains complete. Main and contained bases belong to this surface and retain their real immutable geometry. Moving only this bookkeeping call therefore preserves filtering and typed error order; a failed compilation still publishes no plan.

A complete optional two-file patch is `ci-resize-projection.patch`, pinned by `ci-resize-projection-base.json`. It moves this call and extends the already registered core test `scene_compiler::surface::shared_splitter_tests::wide_split_retains_one_resize_snapshot_per_projection` to assert the actual plan's exact projection source/weights (one projection for both 4- and 64-child fixtures) and the unprojected plan's empty inventory. Those new assertions fail against the current ordering while the old geometry-only assertions can pass.

Root's meaningful verification is the existing unchanged host case `nested_splitter::four_arm_junction_return_to_origin_restores_original_weight_bits_without_a_commit` (including Immediate and Deferred branches), the complete nested_splitter integration target, and the normal/default + serde host matrix. The harness is a separate manifest and its tests are not qualified merely by a workspace/core pass. Do not change the `false` expectation, clear the late artifact, force cancellation, or weaken receiver checks.

## Independent review of root's actual minimal move

Root reproduced the unchanged test locally at the same line, retained ci-owner-host-local-red.log, then moved only the registration call in the live compiler. I read the actual resulting surface.rs: the early call is absent; `ready.set_resize_projection(resize_overrides)` now follows both root loops, before derive_splitter_operability/compile_splitter_junctions/finish_surface_program. **Source review accepts this minimal fix; no further production change is needed for the diagnosed ordering bug.** Root owns its test run; I did not execute one.

The getter inventory has no construction-time reader in scene_compiler. `compile_root` and layout always use the explicit original override slice; the infallible registration only adds the corresponding semantic admission inventory. Main basis binds before its root compile. Every contained basis binds before measurement and the offscreen continue. All typed compiler errors keep their original relative order; moving an infallible assignment between them cannot turn a failing draft into an output. Surface filtering is retained, and no fingerprint or owner recapture was introduced.

I also checked the other A1 mechanical consumers for the same early-basis dependency:

- Runtime drag decoration resolves only `scene.ready()?.candidate().plan()`, then reads the completed plan's command_basis for source surface/root/subtree facts. It is not invoked while an empty plan is being assembled.
- DropResolver obtains frozen target geometry from an already published target plan, captures current exact target authority through the complete WorkspaceIndex, then compares current/frozen real geometry values. Neither capture occurs during construction of its basis map.
- Presented receiver admission reads complete observed output plans; its transient projection predicate now receives the inventory that was previously lost.
- Scene validation explicitly rebinds all surface command bases from its complete index before final semantic validation/publication; it does not itself call set_resize_projection early.
- Splitter geometry and engine source/resize continuation use sealed Workspace/index exact refs. Builder issuance runs after full raw completion; live graph writers complete issuance between command prefixes. These paths do not read a partially installed plan basis.

No second same-class ordering bug was found in this bounded scan. This is a source-level result, not an all-A1 correctness proof. The existing host return-to-origin Deferred branch is the discriminating acceptance: the stale resized output must retire, while the returned output still presents and no durable resize commit occurs. The existing unit wide_split test is a cheap place to assert projection inventory in addition to geometry, if root elects to add those few assertions.
