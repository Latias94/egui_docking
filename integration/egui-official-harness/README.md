# Official egui compatibility harness

This package verifies that `egui_dockspace` builds and its public single-surface
facade can be consumed with the unmodified crates.io releases of `egui` and
`eframe` 0.36.1.

The manifest is an independent workspace root, and the repository root currently
uses the same official registry coordinates without a local source patch. Do not
add patches, source replacements, or compatibility feature gates here: a compile
failure caused by a fork-only API is the result this harness is intended to
expose. Keeping this package independent also prevents a future development-only
root patch from silently changing the compatibility gate.

Run the gate from the repository root:

```text
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --locked -j1
```

Inspect the resolved dependency sources when diagnosing a failure:

```text
cargo tree --manifest-path integration/egui-official-harness/Cargo.toml --locked
```

Both `egui v0.36.1` and `eframe v0.36.1` in that tree must come from the
registry. The only intentional path dependencies are `egui_dockspace` and its
local `dockspace` dependency.
