# Separate semantic publication from presentation authority

One session owns committed semantic state. A host frame stages ordered input,
measurements and output against one private candidate, then publishes atomically.
Dropping or failing the frame leaves the committed state intact. Ordinary accepted
input does not wait for a newly drawn preview or roll back when an output drops.

Sink acceptance, frame commit, actual presentation and admission of that terminal
presentation result are separate boundaries. Each committed output carries an
exact affine handle. The host reports that artifact as Presented or Dropped, and
a subsequent successful frame admits its result before granting receiver
authority. This prevents discarded, stale or superseded output from authorizing
input while allowing asynchronous rendering and bounded backpressure.

Native transfers which retire their source may retain a released drag as one
pending mutation until its exact preview is presented. Pointer Idle does not
mean that this transaction has finished. The presentation ledger owns output
terminals; the mutation references matching emissions, including those emitted
before release. Source or target authority loss must terminate the pending
mutation as well as invalidate active feedback. Late output results still settle
their ledger entries but cannot revive a cancelled mutation or authorize a newer
gesture.

A draw traversal or GPU submission alone cannot prove presentation. Coalescing
hosts must settle every discarded handle and preserve per-stream terminal order.
See [retained UI publication](../ADAPTER_TESTING.md#retained-ui-publication).

Recovery follows the same separation. A coordinate change revokes transient
input and presentation authority, then fresh measurement and presentation restore
interaction. Discarding a candidate preserves its preceding committed workspace;
dropping a committed output still requires that exact artifact's terminal report.
Neither operation requires rebuilding the logical layout.

`DockspaceView::to_layout()` copies the logical state represented by that view
without carrying runtime authority. A session view captures committed state;
a frame view captures its candidate. Applications can retain a committed copy
through host failure and create a fresh session with their existing pane data,
policy and presentation configuration. Document sidecars and native hosting
preferences use their existing persistence flow. Native reconstruction requires
settling the predecessor's outstanding window/output owners and enrolling fresh
bindings; copying layout is not an in-place reset of the failed host.
