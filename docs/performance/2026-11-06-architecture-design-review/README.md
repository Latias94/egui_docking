# Architecture review evidence

This directory supports the [architecture diagnosis](../2026-11-06-architecture-design-review.md) of implementation `54840717a2454c957b13a667a939ddf8229e52e9`.

## Independent research lanes

- [State ownership and invalidation](state-architecture.md): current idle/selection call chains, transaction copies, source proofs, dependency locality, and cross-examination of the scene proposal.
- [Scene and measurement](scene-architecture.md): measurement protocols, installed views, receiver associations, and a counter-review that narrows the original generation proposal. Its final counter-review takes precedence over the initial broad destination recommendation.
- [ImGui and adapter boundaries](imgui-architecture.md): pinned upstream source, native bridge responsibilities, fair extraction experiments, and cross-review challenges.

All lanes were read-only researchers. They wrote only their own temporary Markdown reports, ran no Cargo/tests/benchmarks, and created no nested agents. Root collected sampling separately and reconciled their claims. The reports contain source references and explicitly distinguish facts from unrun experiments.

## Current-product sampling

- `source-environment.json`: reviewed commit, verified 565 recorded Rust/adapter/build/workload hashes, frozen executable digest, original build metadata, and existing qualification archive location.
- `current-{idle,tab_click}.sample.txt.gz`: full raw macOS `sample` reports, compressed without changing their uncompressed contents. Physical-footprint fields are incidental sampler output, not a qualified RSS result.
- `current-{idle,tab_click}.json`: original process/sampler commands, timestamps, return codes and executable hashes.
- `current-{idle,tab_click}-attribution.json`: inclusive call-tree category roots/counts, denominator, raw-capture hash and limitations.
- `current-{idle,tab_click}.csv.gz`: captured product output while sampling, retained for workload context. Its timings are instrumented observations, not formal before/after or ImGui comparison results.
- `summarize_samples.py`: a small, read-only reproduction of the category counts and raw-capture hashes. It does not inspect Rust semantics or qualify new performance results.

Run:

```text
python3 docs/performance/2026-11-06-architecture-design-review/summarize_samples.py
```

Each category unions its nonoverlapping matched call-tree roots. Categories overlap with other categories and cannot be added. The output-proof category includes application rendering under audit closures, so it does not measure exclusive audit cost. Measurement/resources includes real callbacks/text. Captures include initialization, warmup, untimed click preparation, validation and disposal; inlining may attribute work to callers.

The sampling executable was the previously qualified `candidate-product-bin`, verified against the original binary digest and recorded inputs before use. No source was compiled for this review. The raw process output and CPU samples ran together; no formal performance comparison ran alongside sampling. The existing [linked-targets qualification](../2026-11-06-linked-targets.md) supplies the complete-product ratios and renderer-free update summaries cited by the diagnosis. New sampling does not requalify those medians or establish a current pure-core ImGui ratio.

The original executable and temporary receipts remain in `/tmp/dockspace-architecture-review-20261106-5484071/`; executables are not committed. Source/library/compiler reproducibility is limited to the recorded provenance and prior qualification, not a fresh rebuild of every dependency. This documentation-only round ran no Rust tests and changes no runtime behavior.
