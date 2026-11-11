# Prerelease maintenance qualification

Baseline: `fa73fe1` on `refactor/egui-pane-ergonomics`.
Plan: [prerelease maintenance refactor](../plans/2026-11-11-1510-refactor-prerelease-maintenance-plan.md).
Qualification host: macOS, `aarch64-apple-darwin`, Rust 1.95.0, 2026-11-11.
Delivery is local; there is no push, PR, merge or upstream framework change.
The [check manifest](2026-11-11-prerelease-maintenance/checks.json) records final
log summaries, log digests and implementation source hashes. Compressed logs
sit beside it.

## Implementation

The obsolete semantic receiver event, compiled manifest and reducer were removed
together. Current keyboard and accessibility actions continue through prepared
actions. Navigation, clipped-menu, destroyed-child ordering and popup-hit tests
retain their behavior checks through the current action and hit protocols.
Tests specific to forged legacy deliveries or duplicate legacy navigation were
removed with that protocol.

Transaction conveniences, borrowed commit wrappers, legacy fixture projections
and diagnostic queries now compile only for their actual test or persistence
consumers. Core has no module-level `dead_code` or `unused_imports` allowances.
Retained shared protocol cases and identity-macro helpers carry item-level
reasons. Default and all-feature production checks finish without warnings.

Encoding state moves as one value across suspension and resumption. Egui pending
focus observations have one owner; failed candidates preserve retryable entries,
and successful publication controls retirement. Native Root requests, committed
cache and token-frozen placement snapshots share one owner while retaining their
distinct authority and capacity accounting.

One typed native callback-head query replaces repeated per-kind probing. Its
lock is released before reduction; exact acknowledgement still follows core
acceptance, and raw event prefixes and output barriers retain their order.
Core commit and native route installation remain in the existing critical
section. No queue, public API, state machine or mutation authority was added.

Focus implementation and tests, command authorization/topology/close/reference
operations, click handling, egui run input/textures and product support now have
private modules with one existing coordinator or executor. The 46 focus tests
and existing product journeys remain registered. Valid deferred actions still
survive unpublished passes and unrelated revisions; this cleanup does not add a
Presented prerequisite. Existing published focus-request side effects retain
their original timing.

## Final verification

Independent workspaces used `CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0`; configurations
sharing a workspace ran sequentially. Commands used `--locked --offline` and
reused their existing target directories. All nextest runs used
`--test-threads=2 --no-fail-fast --status-level fail --final-status-level fail`.

| Scope | Cargo arguments after `nextest run` | Passed | Existing skips |
| --- | --- | ---: | ---: |
| Workspace | `--workspace --all-features --all-targets` | 2,351 | 3 |
| Minimal core | `-p dockspace --no-default-features` | 2,057 | 3 |
| External host, default | `--manifest-path integration/dockspace-host-harness/Cargo.toml` | 376 | 0 |
| External host, persistence | previous manifest plus `--features serde` | 431 | 0 |
| Egui product, default | `--manifest-path integration/egui-product-harness/Cargo.toml` | 114 | 0 |
| Egui product, persistence | previous manifest plus `--features serde` | 119 | 0 |
| Official consumer | `--manifest-path integration/egui-official-harness/Cargo.toml --all-features` | 27 | 0 |
| Native workspace | `--manifest-path integration/egui-fork-workspace/Cargo.toml --workspace --exclude egui-native-smoke --all-targets --all-features` | 652 | 1 |

Additional gates:

- `cargo check -p dockspace --lib` with `--no-default-features` and
  `--all-features`: both pass without warnings.
- Workspace and native all-target/all-feature Clippy pass with
  `-A warnings -D clippy::correctness -D clippy::suspicious`.
- Strict documentation passes with `RUSTDOCFLAGS='-D warnings'`: core and egui
  together, and the native crate in its integration workspace, all with
  `--all-features --no-deps`.
- `cargo test --doc --workspace --all-features`: eleven doctests pass.
- Formatting passes for the main workspace and all four affected consumer
  workspaces. Native formatting names the two owned packages explicitly.
- `python3 scripts/check_core_dependencies.py` and staged/unstaged
  `git diff --check` pass. Removed receiver/event/manifest symbols have no live
  source references.

An intermediate build exhausted disk space. Removing only this project's
rebuildable integration incremental caches freed approximately 30 GiB; affected
gates were rerun. Native Clippy also found one empty line after a doc comment;
it was removed before the successful lint run. Intermediate failures are not
counted as successful verification.

## Review and limits

Implementation lanes and independent cross-reviews checked the ownership and
module boundaries. A final four-persona Codex review covered correctness,
testing, maintainability and reliability; it produced no confirmed P1/P2.
Review run: `20261111-161359-66cb2575`. A separate simplification pass found no
worthwhile duplication or added runtime work; repeated lint attributes and
redundant new private comments were removed during cleanup.

The final review explicitly reports sampling limits for the large mechanical
diff. It is not a proof that every moved function was independently inspected.
The primary implementation lane owns the executed verification above; reviewers
did not run Cargo. After its diff freeze, one doc-comment blank line was removed
and one unused test-build getter was narrowed to its serde feature. The latter
was checked again in the minimal-core suite.

The external host validates renderer-independent API and output contracts with
a recording renderer. These runs add no real-window, GPU, mixed-DPI, Linux,
Windows, Wayland or ImGui timing qualification. Existing physical-platform and
performance records retain their original scope. Neither excluded reference
tree nor the excluded review directory was read or changed.
