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

## Run against a built editor

Build serially from the repository root, using the repository target directory:

```text
cargo build --manifest-path integration/egui-fork-workspace/Cargo.toml --package egui_dockspace_native --example basic --all-features --locked --target-dir target -j1
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
xvfb-run -a --server-args="-screen 0 1280x1024x24 +extension RANDR +extension XTEST" python3 integration/egui-native-smoke/qualify_editor.py --start-openbox target/debug/examples/basic
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

The `fork-native` job installs the required X11 tools and keeps independent
programmatic and physical smoke applications. It also builds the actual `basic`
example and executes both editor drivers. Its owned-update/output-pressure
tests run under Xvfb so their real-renderer bodies execute instead of taking
the headless platform skip. The actual editor steps are:

```yaml
      - name: Build actual eight-pane native editor
        env:
          CARGO_BUILD_JOBS: 1
        run: >-
          cargo build --manifest-path integration/egui-fork-workspace/Cargo.toml
          --package egui_dockspace_native --example basic --all-features
          --locked --target-dir target

      - name: Qualify actual editor docking and lifecycle
        timeout-minutes: 8
        env:
          LIBGL_ALWAYS_SOFTWARE: "1"
        run: >-
          xvfb-run -a
          --server-args="-screen 0 1280x1024x24 +extension RANDR +extension XTEST"
          python3 integration/egui-native-smoke/qualify_editor.py
          --start-openbox target/debug/examples/basic
```

This recipe reuses the pinned native workspace and the existing Python drivers.
The runner only manages optional window-manager startup; CI or the caller owns
Xvfb, build scheduling and the selected executable. Individual drivers remain
directly runnable for diagnosis with the same executable and environment.
