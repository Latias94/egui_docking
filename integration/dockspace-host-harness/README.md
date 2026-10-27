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

This is the U13 public-contract proof, not an actual UI event pipeline or completion
of the full 60-case matrix. The generic UI event pipeline belongs to U3.

Run the independent gates from the repository root:

```text
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --locked --test-threads=1
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --locked --test-threads=1
cargo tree --manifest-path integration/dockspace-host-harness/Cargo.toml --all-features --edges normal --locked
```
