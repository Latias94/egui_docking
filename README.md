# egui_dockspace

`egui_dockspace` is a deterministic docking and multi-viewport system for egui.
The repository is split into two crates:

- `dockspace`: a renderer-neutral docking graph, layout solver, transaction engine,
  interaction protocol, and viewport lifecycle coordinator.
- `egui_dockspace`: an egui renderer and native viewport adapter built on top of
  the headless core.

The rewrite intentionally has no compatibility layer for the former
`egui_docking` or `egui_tiles` public APIs and persistence formats. The design
contract and implementation sequence are recorded in
[`docs/plans/2026-07-18-001-headless-dockspace-refactor-plan.md`](docs/plans/2026-07-18-001-headless-dockspace-refactor-plan.md).

## Status

The new architecture is under active implementation. The renderer-neutral
behavior contract is executable, and production code will only expose workflows
after their invariants and capability gates are covered by tests.

## Development

The workspace uses Rust 1.92 and edition 2024.

```text
cargo nextest run --workspace --all-features --all-targets
cargo clippy --workspace --all-features --all-targets -- -D warnings
cargo fmt --all --check
```

Reference repositories under `repo-ref/` are research inputs and independent Git
repositories. They are never Cargo path dependencies of the publishable workspace.

## License

`dockspace` is licensed under Apache-2.0. `egui_dockspace` is licensed under MIT OR
Apache-2.0. See `THIRD_PARTY.md` for prior-art and fixture provenance.
