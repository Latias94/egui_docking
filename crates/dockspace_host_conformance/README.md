# dockspace host conformance

This unpublished crate is the renderer-neutral `core-host` executor from the
Open GPUI docking conformance catalog. It uses only `dockspace`'s public runtime
and model APIs. It does not own a `DockEngine`, replay core traces, or ask the
core hit resolver to choose receiver facts.

Current executable coverage:

- `OGC-01`: repeated same-axis docking produces a canonical N-ary split, and a
  stale edge target is rejected without mutation.
- `OGC-02`: merge and follow-up close preserve ownership, selection, and the
  target tabs' local MRU order.
- `OGC-03`: first-hit, cached, and stale receiver facts remain inert; only a
  release over the receiver bound to the current, exactly presented preview
  commits the move.

The OGC-03 driver captures opaque receiver descriptors while painting, consumes
affine output capabilities only after an exact final-presentation result, and
then reports framework delivery or hover facts through the public runtime
facade. It does not import a core hit resolver or construct provider receipts.

The crate also owns the private-workspace semantic description for
`LocalTabCloseSplitterAndContained`. The `core-host` test in this crate and the
`egui-product-headless` kittest harness both map the same tab selection, close,
splitter adjustment, and contained-resize steps without sharing runtime IDs,
coordinates, or output authority. The OGC cases above remain `core-host`
evidence only and do not claim native-window coverage.

The fixed U1 `JOURNEY_CATALOG` names L01-L08, N01, and P01 plus their cadence
and admission requirements. It is deliberately not a GUI driver language.
Semantic source/basin/preview/outcome/ownership/focus/quiescence checkpoints,
oracle classifications, executor assignments, and known gaps live in
`docs/VALIDATION.md`. The existing shared scenario is linked to L08; the
managed lifecycle scenario is linked to N01.

The test crate also keeps a private two-surface transaction fixture. Each
mutation boundary freezes the complete logical roster and records one exact
contribution for every surface; output capabilities then settle in their normal
affine presentation boundaries. The fixture proves complete-roster
commit/drop-abort, stale-target rejection, superseded-output isolation, and
unknown-owner failure without minting presentation or native-window evidence.

The managed lifecycle fixture separately proves action terminality, ownership
publication, first-live, redock, retirement, and quiescence. The private
two-surface managed-native fixture also destroys the exact target window during
a source-owned drag and proves that the target preview is cleared without
canceling the source gesture. These fixtures exercise public runtime authority;
they do not claim real operating-system window evidence.

Run with:

```console
cargo nextest run -p dockspace_host_conformance --all-targets --locked -j1
```
