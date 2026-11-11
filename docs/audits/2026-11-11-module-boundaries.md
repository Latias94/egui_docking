# Dockspace module boundaries and background completion

Baseline: `7d75e56`, on `refactor/egui-pane-ergonomics`.

Implementation commits: `73913c8` (surface compilation), `85c0211` (engine and
viewport coordination), `2375150` (native mailbox), `7250619` (pane providers and
background completion).

## Scope and design

The approved change separates the remaining mixed production responsibilities
in surface compilation, engine reduction, viewport coordination and the native
mailbox. It also makes boxed pane providers usable by generic native owners and
documents how application-owned background work completes without borrowing UI
or docking transactions across tasks.

The following invariants remain with their original owners:

- `DockEngine` owns graph mutation, rollback candidates and atomic publication.
- `ViewportCoordinator` owns the provider, registry and effect ledger together.
- `NativeHostState` retains one ordered journal and its existing lock. Native
  placement preparation and core publication still share that lock.
- Output reservations, retirement obligations, exact effect acknowledgements and
  retry paths retain their original ownership and drop behavior.
- Pane rendering remains synchronous within one egui pass. The core gains no
  executor, async trait, task scheduler or second mutation authority.

## Module map

| Owner | Extracted implementation |
| --- | --- |
| Surface compiler | `surface/tabs.rs`, `guides.rs`, `contained.rs`, `measurement.rs`, `error.rs`; explicit imports and scoped visibility |
| Engine | `workspace_publication.rs` with `commands` and `configuration`; `presentation_authority/continuation`; `pointer_session/source`; engine errors |
| Existing engine modules | Placement validation in `contained_adjustment`; resize adjustments in `pointer_splitter`; presentation retention in `retention`; recovery authorization in `surface_vacancy` |
| Viewport coordinator | `provider`, `workspace_restore`, `snapshot`, `input`, `error` |
| Native mailbox | `ordered_input`, `output`, `lifecycle_receipts`, `placement`, `route_retirement`, `framework_callbacks` |

| Parent source file | Before | After |
| --- | ---: | ---: |
| `scene_compiler/surface.rs` | 3,642 | 935 |
| `engine.rs` | 6,421 | 2,177 |
| `frame.rs` | 4,700 | 2,240 |
| `egui_dockspace_native/src/mailbox.rs` | 4,443 | 1,368 |

These are total file line counts, including inline tests and declarations.
Implementations move into cohesive private modules rather than disappearing;
the reduced parent sizes are a navigation improvement, not a performance result.

Engine drag-source index tests move to a private test file. Existing behavior
tests remain the verification surface; the extraction adds no tests that merely
assert file structure or private forwarding.

## Pane providers and background work

`Box<T: PaneView + ?Sized>` delegates all five callbacks, including overridden
focus observations. The trait retains support for borrowed and non-`Send` local
providers. The native owner additionally accepts `Box<dyn PaneView + Send>`
through its existing generic interface and retains its required `'static`
lifetime.

The `background_save` example uses an application-owned worker and a bounded
one-result channel. A successful initial Deferred decision yields the exact
continuation retained by the UI. The worker receives an owned text snapshot,
destination and egui wake handle. Completion permits or vetoes that original
close; stale completion cannot close a replacement layout. A failed close commit
retains completion for retry. Editing pauses while saving, so the example cannot
close over newer unsaved content. An application that keeps editing must compare
its own document revision before allowing closure.

Native documentation calls out lock-held registry callbacks, non-reentrancy,
explicit repaint after pane-data updates and the distinction between command
admission and committed completion. A native application must retain pending
completion while its existing command slot is Busy and consume the terminal
action status before submitting another command.

## Verification

All commands use the existing targets, run serially and limit Cargo to two build
jobs. Nextest commands use `--locked --no-fail-fast`; the full workspace commands
also use `--all-features --all-targets`.

| Gate | Result |
| --- | --- |
| Workspace, all features and targets | 2,360 passed, 3 existing skips |
| Minimal core, all targets, no optional features | 2,057 passed, 3 existing skips |
| Independent public host, default / serde | 377 / 432 passed |
| Official egui product, default / serde, all targets | 114 / 119 passed |
| Official egui public consumers | 24 passed |
| Official caller-owned demo workflows | 3 passed |
| Native workspace, all features and targets | 696 passed, 1 existing skip |
| Final pane-provider and background-save tests | 8 passed |
| Workspace / native correctness and suspicious Clippy | Passed |
| Workspace / native strict rustdoc, `RUSTDOCFLAGS=-D warnings` | Passed |
| Workspace / native doctests | 11 / 2 passed |
| Core normal/build dependency boundary | Passed |
| Workspace / native formatting and changed-file whitespace | Passed |

The final focused run includes the additional real-worker test and the final
startup behavior: background completion always resolves on a later UI turn,
including spawn failure. The complete workspace run above preceded that final
example-only adjustment; its affected provider/example tests were rerun afterward.
The final focused command is:

```text
cargo nextest run -p egui_dockspace --all-features --all-targets --locked -j2 --no-fail-fast -E 'test(pane::tests) | binary(background_save)'
```

The new regression coverage exercises a borrowed non-`Send` boxed provider,
custom focus observations, the generic native owner with a boxed `Send` provider,
pending save retention, I/O failure, disconnected completion, stale close tokens
after layout replacement and an actual worker writing its frozen snapshot before
the UI permits closure. The worker test removes its newly created temporary copy.

Source comparison preserved five large surface-compilation ranges, twenty
engine/coordinator ranges and twenty-one native mailbox ranges after allowing
scoped visibility, whitespace and rustfmt trailing commas. Commit order and lock
scope remain inside those unchanged implementations. This comparison supplements
the behavior tests; it is not an independent architectural or physical review.

This is a macOS maintenance and integration qualification. Existing native-smoke
Linux-only helper dead-code warnings are unchanged. There are no new ignored
tests, physical-platform qualification or timing claims.
