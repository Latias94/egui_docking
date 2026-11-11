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

A draw traversal or GPU submission alone cannot prove presentation. Coalescing
hosts must settle every discarded handle and preserve per-stream terminal order.
See [retained UI publication](../ADAPTER_TESTING.md#retained-ui-publication).
