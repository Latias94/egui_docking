# Sampling the remaining small fresh-layout work

A sampling-only driver repeatedly performs the original fresh-layout Adapter
on a fixed validated 16-pane Workspace, separately with one and four logical
views. It copies `opposite`, `tree`, `bounds`, `collect_constraints` and
`project_all_roots` verbatim from the formal probe. Every iteration materializes
fresh leaf answers, reads them through the same identity-checked callback, and
constructs all complete rich projection outputs. Width alternates between
16,380 and 16,444. The output is black-boxed and disposed within each iteration.

This is a locating measurement, not a complete-operation benchmark. There is
no request capture, checked mutation, export or ImGui condition. Reusing the
unchanged Workspace isolates layout; rich-output disposal is included here
but occurs outside the formal operation interval. Its sample proportions are
not formal p50 time shares, allocation counts, peak residency, or latency
predictions for a candidate.

The initial build correctly failed because the normal product facade hides
the private graph, identity and raw layout modules. The diagnostic then copied
the exact `a2365aa` kernel into its temporary package and exposed only those
three module visibilities. Every other source is byte-identical to that
immutable commit. Existing dependency versions/checksums match the root lock.
No repository API, production source or formal probe changed. The initial
failure and correction remain in the evidence.

The corrected driver builds with the production thin-LTO, one-codegen-unit
configuration and one Cargo job. Both runs finish four million fresh layouts
successfully. macOS `sample` attaches for four seconds at a one-millisecond
interval; the reports contain 2,945 one-view and 2,887 four-view main-thread
samples. The runner clears the bypass environment variable, but this non-test
kernel contains no test work ledger and is not an ordinary counter-enabled
qualification gate.

Both reports show substantial execution in `project_measurements`, constraint
composition, the fresh leaf Adapter, and allocation/reallocation/free call
trees. A `RawVec::grow_one` call tree below `project_measurements` contains
266 one-view and 217 four-view samples. That observation supports testing
the current projection continuation stack's growth and storage. It does not
establish the exact percentage exclusively caused by that stack: sampling,
inlining, allocator internals, output disposal and this repeated workload
limit attribution. The reports also retain all other stacks.

The next bounded experiments remain independent: materialized result-owner
consolidation, an ordinary SmallVec continuation stack, and removal of
continuation transitions that only push a state immediately popped again.
The original solver, fresh inputs, complete owned values and late geometry
error order must survive. No constant leaf callback, output cache, fixed depth
limit or untimed materialization is suggested by these samples.

The [evidence archive](2026-11-07-layout-hotspot/evidence.tar.xz) retains 22
inputs plus its manifest: exact driver and diagnostic visibility changes,
dependency lock, complete build/failure receipts, source hashes, both complete
sample reports and run logs. Every archive member round-trips against its
original bytes. The [check receipt](2026-11-07-layout-hotspot/check.json)
binds the immutable source comparison and explicitly excludes a
complete-operation performance claim.

The [independent evidence review](2026-11-07-layout-hotspot/independent-evidence-review.md)
checks the complete archive, exact source/helper bindings, binary identity,
both completed runs and reported sample counts. The static source-binding
file predates execution; actual run receipts and stdout establish completion.
It finds no blocker and confirms the stated diagnostic limits.
