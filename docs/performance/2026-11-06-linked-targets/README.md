# Linked target-receiver evidence

See the [results and ownership design](../2026-11-06-linked-targets.md).

- `frozen-product/`: two serial ABBA/BAAB comparisons of the frozen baseline and final candidate; raw complete-product rows and recomputable percentile summaries.
- `candidate-product/`: production build/source environment and separate complete egui/ImGui reference.
- `headless/`: all renderer-free cases/scales, per-process CPU summaries and separately recorded allocation summaries. Raw per-sample headless timing is not available; do not pool process medians.
- `baseline-provenance.json`: exact product baseline and the original headless executable environment, with all relevant source inputs checked against the new baseline commit.
- `state.json`, `source.diff.gz`: exact final source scope/hashes and patch.
- `checks.json`, `gate-commands.json`, `*.log.gz`: exact-source validation commands, receipts and logs.
- `code-review.md`, prompt and exit receipt: independent read-only implementation review.
- `*.py`: historical local orchestration and the standalone data verifier; no Rust-name parsing or new benchmark interface.
- `SHA256SUMS`: every other archived file's digest, including the verifier.

Run `python3 docs/performance/2026-11-06-linked-targets/verify_evidence.py`. This reads archived data, recomputes product percentiles/work equality, checks headless coverage/output counts and verifies source scope, check snapshots and recorded timing separation. It does not rebuild anything or execute historical paths.

Active DHAT and CPU timing are separate. Requested bytes are cumulative allocation requests; boundary live values and cumulative prefix peaks are not RSS. The link adds retained scene storage and some installation allocations. This is a CPU/storage tradeoff, not a memory improvement. Full-product ImGui ratios are not renderer-free docking-core ratios.

Executables remain under `/tmp/dockspace-linked-receivers-20261106-2935571/`; they are not committed. The precommit source label in build metadata is supplemented by exact source hashes. Protected user files are recorded as hashes only and were not changed by this work.
