# Public host contract harness

This independent workspace depends only on `dockspace` through its public API.
Its `serde` feature forwards to `dockspace/serde`; it has no UI or platform dependencies.

`RecordingRenderer` accepts the complete ordered surface program, copies geometry,
opaque identity, receiver descriptors, and pane-content requests into owned records,
and returns the core execution receipt. Recording is sink acceptance, not final presentation.
The tests inspect records after frame borrows end and report exact affine outputs as
`Presented` or `Dropped`. Only a committed core acknowledgement grants receiver authority.
They also cover independently settling pending surfaces with identical receiver coordinates,
accepted artifacts waiting behind a popup's cross-surface presentation barrier, late outputs
after whole-surface redocking or pane-selection invalidation, abandoned outputs unblocking
newer presentation prefixes, and semantic layout commands without registering a pointer
producer or enabling native hosting.

The U3 host input tests submit complete ordered batches across two logical
surfaces with identical bounds. Source manipulation and target hover carry
independent exact presentation routes. Release waits for actual recording-output
settlement, with no synthetic pointer events. Negative journeys exercise unknown
routes, stale or foreign authority during an active gesture, failed frames, and
producer retirement. A wrong-winner release after a selection-changing valid
prefix must roll back the published version, selection, interaction and surface
proofs; retrying the same ordered edges must deliver exactly once. The retry
contract also detects producer watermarks advancing during rollback.

The U4 consumer checks full contained content during move and every resize
direction, exact pending-output settlement, cancellation, changed measurements,
and a target behind the complete source in a pre-frozen input batch. A complete
tabs-root journey presents each source-only projection while jittering across a
magnetic boundary: the target stays latched inside its release margin and changes
outside it. Framework-clearance facts never replace the core target solver.

This is public-contract and renderer-neutral input evidence. It does not prove
physical OS event delivery, actual GPU presentation, or completion of the full
60-case product matrix. The egui ordered-input migration remains part of U3.

The U6 public consumer completes item/root close plans with Allow, Veto and
exact deferred continuations, independently of pointer registration. Feedback
tests consume pending and terminal statuses by stable manipulation identity,
check publication deduplication, and preserve pending cancellation on producer
retirement. These recording tests distinguish command/feedback semantics from
adapter focus execution and physical presentation.

Run the independent gates from the repository root:

```text
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --locked --test-threads=1
cargo tree --manifest-path integration/dockspace-host-harness/Cargo.toml --all-features --edges normal --locked
```
