# dockspace host conformance

This unpublished crate is the renderer-neutral `host-driver` executor from the
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
This is host-driver evidence only; it does not claim egui or native-window
coverage.

Run with:

```console
cargo nextest run -p dockspace_host_conformance --all-targets
```
