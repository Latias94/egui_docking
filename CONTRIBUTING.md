# Contributing

Use Rust 1.95 with rustfmt and Clippy, Python 3.10+ for repository scripts, and
[cargo-nextest](https://nexte.st/) for tests. Start with the [examples](docs/EXAMPLES.md)
to reproduce behavior in the appropriate host.

## Repository layout

| Path | Responsibility |
| --- | --- |
| `crates/dockspace` | Renderer-neutral public model, policy and session runtime |
| `crates/egui_dockspace` | Official egui adapter and packaged application examples |
| `crates/egui_dockspace_native` | Unpublished adapter for the pinned native forks |
| `crates/dockspace_host_conformance` | Shared semantic test scenarios |
| `integration/dockspace-host-harness` | Independent public core consumer |
| `integration/egui-official-harness` | Official egui compatibility and complete-output editor |
| `integration/egui-product-harness` | Official egui behavior tests |
| `integration/egui-fork-workspace` | Separate native adapter/test workspace |
| `integration/egui-native-smoke` | Native applications and physical-input drivers |
| `integration/gpui-harness` | Local Open GPUI retained adapter and interaction tests |
| `docs` | Integration guides, platform limits and architecture decisions |
| `scripts` | Core dependency boundary and opt-in performance comparisons |

Keep the renderer-neutral core independent of egui, an async executor and an OS
event loop. Pane data belongs to the application. Add behavior behind existing
owners before introducing another state owner or protocol.

## Local checks

Reuse the existing target directories. Run build/test commands sequentially and
choose a small job count when the machine is busy. Set `CARGO_BUILD_JOBS=2`
in the environment to bound compilation; nextest's `-j2` limits test concurrency.

```sh
cargo fmt --all --check
python3 scripts/check_core_dependencies.py
cargo nextest run --workspace --all-features --all-targets --locked -j2
cargo clippy --workspace --all-features --all-targets --locked -j2 -- -A warnings -D clippy::correctness -D clippy::suspicious
cargo test --doc --workspace --all-features --locked -j2
```

For a targeted change, run its affected tests first. Core changes also need the
minimal feature configuration and independent public consumer:

```sh
cargo nextest run -p dockspace --no-default-features --all-targets --locked -j2
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --locked -j2
cargo nextest run --manifest-path integration/dockspace-host-harness/Cargo.toml --features serde --locked -j2
```

Adapter changes need the corresponding official consumers. Native changes use
the separate fork workspace; do not add fork patches to the official one.

```sh
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --all-targets --locked -j2
cargo nextest run --manifest-path integration/egui-product-harness/Cargo.toml --features serde --all-targets --locked -j2
cargo nextest run --manifest-path integration/egui-official-harness/Cargo.toml --features demo --all-targets --locked -j2
cargo fmt --manifest-path crates/egui_dockspace_native/Cargo.toml -p egui_dockspace_native --check
cargo nextest run --manifest-path integration/egui-fork-workspace/Cargo.toml --workspace --all-features --all-targets --locked -j2
```

Headless tests do not qualify actual OS input or GPU presentation. Native
physical checks require the isolated environments documented in
[platform support](docs/PLATFORM_SUPPORT.md) and the
[native qualification guide](integration/egui-native-smoke/README.md).
`.github/workflows/ci.yml` owns the default gates. The separate
`.github/workflows/native-qualification.yml` runs the complete Linux X11
real-window and physical-input checks on demand. Run it before a release and
when changing native input, presentation, lifecycle or dependency pins.

## Reports and changes

For a bug report, include the source revision, example/host, OS/backend, enabled
features, reproduction steps and observed result. Distinguish a committed text
value or final layout from a visual artifact or platform log line. Chinese IME
reports should say whether composition, candidate placement or committed text
is affected, and name the input method.

Keep changes scoped and describe the user-visible behavior and relevant checks.
Use English code, API documentation and Conventional Commit messages. Update
the example or guide when an interface changes. Keep raw benchmark results and
qualification logs in CI artifacts or release attachments with their source and
environment. Record durable architectural trade-offs in short ADRs; temporary
plans and session logs do not belong in the maintained documentation.

Release and repository maintenance are documented in
[Maintaining](docs/MAINTAINING.md).
