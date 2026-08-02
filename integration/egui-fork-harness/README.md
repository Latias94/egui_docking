# Fork-backed egui integration harness

This excluded workspace verifies the graph-agnostic native seams carried by the
release-pinned egui fork. Its manifest patches the egui family to the immutable
public commit recorded in `Cargo.lock`; the publishable root workspace remains
registry-only.

Run the complete local gate from the repository root:

```text
python3 scripts/run_egui_fork_harness.py
```

The runner builds both native renderers and executes the harness with nextest
under `--locked`. The harness is intentionally fork-only and is not evidence
that the crates.io adapter supports native multi-viewport docking. Develop and
test fork changes inside `repo-ref/egui-release`; after committing and pushing
them, advance the three manifest pins and lockfiles together.
