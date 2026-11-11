# Native editor release qualification

The actual eight-pane `egui_dockspace_native` **basic example** is a maintained
release gate alongside the separate native smoke applications. Its composition
includes real editable panes, scrolling, contained windows and native children.
Passing a simplified smoke application does not qualify this editor composition.

[`qualify_editor.py`](qualify_editor.py) invokes the two existing finite drivers
sequentially and fails if either fails. It does not build anything, submit
programmatic docking commands, infer Rust behavior or replace their assertions.

| Driver | Required behavior |
| --- | --- |
| [`editor_docking_x11.py`](editor_docking_x11.py) | Five fresh editor processes: physically drag an inactive tab into Outline, Console, Inspector, Preview and Profiler; verify actual merge membership, selection, presentation and quiescence. |
| [`editor_x11.py`](editor_x11.py) | Real text edits and OS close through native first-live, Dock Back, close/Allow/reopen, locking and Default reset; preserved application text and final native quiescence. A serde build also covers JSON restore, missing content/reload and replacement of two live children. |

The runner always enables the existing `EGUI_DOCKSPACE_EDITOR_ROOT_REBIND=1`
extension: cross-surface reset and fresh Root edits, plus saved-document and
preset rebinds with serde. The driver independently observes that the original
Root receives no `DestroyNotify`, and checks retired child X11 resources.
Lifecycle commands inside that workflow remain programmatic where its driver
documents them. The five docking cases supply actual pointer input.

The lifecycle driver also checks descriptive OS titles for Inspector and the
simultaneous Inspector/Assets children while retaining its PID, exact window ID,
and destruction assertions. Unicode captions are compared in Python rather than
through the window-search tool's locale-sensitive regular expressions.

The separate `basic --command-smoke` journey retains its nine docking commands
and applies four styles while a native child is live: geometry, visual-only,
unchanged, and restored defaults. It checks each terminal result, fresh child
content bounds, the unchanged physical window ID, and retained application data
before completing the second native round trip.

## Run against a built editor

Build serially from the repository root, using the repository target directory:

```text
cargo build --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --example basic --all-features --locked -j1
```

Run on a dedicated X11 display with `xdotool`, `xprop` and an EWMH window manager
with a published work area. The Python drivers need Python 3.10 or later; Root
continuity also loads `libX11`. Do not share the display with another input test.
The runner fixes X11 scale to 1 and enables the Root continuity extension.

With an existing Xvfb/Openbox session, including the native qualification Docker
environment, reuse that display and pass the built executable:

```text
python3 integration/egui-native-smoke/qualify_editor.py /native-target/debug/examples/basic
```

On Linux without an existing display, start an isolated Xvfb and let the runner
own Openbox. Software GL is configured by the caller in CI or the container.

```text
xvfb-run -a --server-args="-screen 0 1280x1024x24 +extension RANDR +extension XTEST" python3 integration/egui-native-smoke/qualify_editor.py --start-openbox integration/egui-fork-workspace/target/debug/examples/basic
```

For a release, use the all-features build so the persistence journeys execute.
To additionally qualify the non-serde profile, rebuild without `--all-features`
and rerun the same command; the existing lifecycle driver checks its declared
profile and exact edit count. Retain the complete console log, source commit or
dirty inventory, dependency pins, build features and display environment with
the release evidence. A success line is printed only after both drivers pass.
These are X11 reference-backend results; other platforms require their own
physical-input qualification.

## CI integration

The default [CI workflow](../../.github/workflows/ci.yml) runs native workspace
tests, including smoke unit tests, two real-renderer publication/backpressure
checks, and macOS/Windows compile checks. The separate
[Native qualification workflow](../../.github/workflows/native-qualification.yml)
is manually dispatched from GitHub Actions. Run it before a release and when
changing native input, output ownership, lifecycle or dependency pins. It builds
the applications once in the native workspace's target directory and runs:

- Actual renderer/output, text backpressure, shared Root, capture and lifecycle
  boundary tests under Xvfb/Openbox, including their ignored real-window cases.
- Public pane commands and style changes in the eight-pane editor.
- Both editor input drivers with persistence and Root continuity enabled.
- Programmatic native redock and all three native close policies.
- Physical X11/XTest drag-out and redock.

The workflow uploads run logs with the source revision and toolchain as an
artifact. Dependency pins and enabled features are available from that source
and workflow. It does not re-run the dependency forks' internal test suites;
those belong to the fork repositories.

[`with_openbox.py`](with_openbox.py) owns window-manager startup, readiness and
cleanup for a single command on a caller-supplied isolated display. It does not
build applications or infer their behavior. [`close_x11.py`](close_x11.py)
waits for each close-ready child and sends Alt-F4 through the window manager;
the smoke application checks the close policy and terminal state. Both remain
directly runnable for diagnosis:

```text
xvfb-run -a python3 integration/egui-native-smoke/with_openbox.py -- python3 integration/egui-native-smoke/close_x11.py integration/egui-fork-workspace/target/debug/egui-native-smoke
```

CI or the caller owns Xvfb, software GL configuration, build scheduling and the
selected executable. Do not share the display with another input test.
