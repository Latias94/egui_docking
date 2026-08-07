# Fork-backed egui integration harness

This package verifies the graph-agnostic native seams carried by the
release-pinned egui fork. The sibling `egui-fork-workspace` owns the single
fork patch and lockfile shared by this harness, the native runtime, and the
real-window E2E package. The publishable root workspace remains registry-only.

Run the complete local gate from the repository root:

```text
python3 scripts/run_egui_fork_harness.py
```

The runner builds both native renderers and executes the harness with nextest
under `--locked`. The harness is intentionally fork-only and is not evidence
that the crates.io adapter supports native multi-viewport docking. Develop and
test fork changes inside `repo-ref/egui-release`; after committing and pushing
them, advance the single workspace pin and lockfile.
