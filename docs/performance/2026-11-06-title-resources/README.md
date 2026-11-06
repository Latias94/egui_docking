# Same-pass title resource evidence

See the [results and architecture notes](../2026-11-06-title-resources.md).

- `frozen-product/`: final ABBA and BAAB groups, raw complete-product CPU rows and recomputable summaries.
- `candidate-product/`: final production build/source environment and separate egui/ImGui reference.
- `title-reuse-only/`: the first candidate's complete qualification, exact source patch and neutral/negative observations.
- `map-pilot-*`: exploratory idle-only intermediate condition, separate from final qualification.
- `baseline-provenance.json`, `state.json`, `source.diff.gz`: exact frozen baseline and final source/executable identities.
- `allocation-*.csv` and `allocation-summary.csv`: four separate active-DHAT runs with full work counters, request counts and boundary/prefix live bytes.
- `allocation-provenance.json`, `allocation-instrumentation.json`, `allocation-baseline/` and `allocation-candidate/`: identical allocation-only workload source and lockfiles, build manifests and exact library inventories. Baseline extraction comes from Git; large source extraction and executables are not committed.
- `*-attribution.json`, `*.sample.txt.gz`, `*.json`: separately sampled CPU evidence and execution intervals.
- `checks.json`, `gate-commands.json`, `*.log.gz`: exact-source final checks; earlier drafts/zero-test harness attempt are explicitly retained separately.
- `code-review*.md`: independent read-only implementation reviews, prompts and exit receipts.
- `*.py`: historical local orchestration and the standalone evidence verifier. These do not extend the public benchmark interface.
- `SHA256SUMS`: every other archived file's digest, including the verifier.

Run `python3 docs/performance/2026-11-06-title-resources/verify_evidence.py` to recompute coverage, CPU percentiles, allocation summaries, source scope and formal-timing separation. The verifier needs only Python and archived files. It does not execute historical command paths, rerun tests or rebuild programs.

CPU latency is qualified only by the ordinary production executables. Active-DHAT elapsed times are not CPU performance results. Requested bytes are cumulative allocation requests, live values are measured boundaries, and prefix peaks are cumulative per scenario; none is RSS. The full-product ImGui ratio is not a renderer-free docking-core ratio. Sampling categories overlap and include preparation/disposal beyond formal frame intervals.

Runtime binaries remain under `/tmp/dockspace-refresh-20261106-3803549/`. Raw build metadata carries the precommit Git label; exact inventories identify the actual candidate. Protected user paths are recorded as hashes only and were not changed or committed by this work.

`evidence-review.md` independently verifies arithmetic, raw rows, Git/source and binary provenance, allocation intervals, gates and timing separation. Its reviewed archive had 187 manifest entries; `packaging-after-review.json` records that manifest digest, lossless compression of source patches/build manifests, and CSV line-ending normalization after review. Source code and measurements did not change.
